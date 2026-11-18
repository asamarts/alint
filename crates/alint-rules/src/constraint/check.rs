//! The builder's check of a `constraint` rule against its input relation
//! (design section 8, "plan check"): every field the assertion and the
//! message name exists, every literal can match the field it is compared
//! with, and what a later step adds is refused as not yet supported.

use alint_core::relation::Value;
use alint_core::relation::plan::{FieldSource, JoinSide, Plan, RelationId, Rows};
use alint_core::{Error, Result};

use super::options::{self, Plurals};
use super::{Assertion, SetMode, relation_label};

/// Checks one rule's options against relation `input` of `plan`.
pub(super) struct Checker<'p> {
    plan: &'p Plan,
    input: RelationId,
    rule: &'p str,
}

/// What a field's values hold, as far as the plan knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holds {
    /// Strings (or null): a path or path token, a key, a key path, a `read:`
    /// value, or a value a transform made.
    Strings,
    /// Any value: an `entry.value` or a `bound:` value.
    Any,
}

impl<'p> Checker<'p> {
    pub(super) fn new(plan: &'p Plan, input: RelationId, rule: &'p str) -> Self {
        Self { plan, input, rule }
    }

    fn fail(&self, path: &str, message: impl std::fmt::Display) -> Error {
        Error::rule_config(self.rule, format!("`{path}`: {message}"))
    }

    fn not_yet(&self, path: &str, what: &str) -> Error {
        self.fail(
            path,
            format!(
                "{what} is not yet supported by the `constraint` kind in this build; it comes \
                 in a later step of version 2"
            ),
        )
    }

    /// The fields of the input relation, in the order written.
    fn field_names(&self) -> Vec<&'p str> {
        self.plan
            .relation(self.input)
            .relation
            .fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Refuse `name` at `path` unless the input relation has that field.
    fn field(&self, name: &str, path: &str) -> Result<()> {
        let names = self.field_names();
        if names.contains(&name) {
            return Ok(());
        }
        let listed = names
            .iter()
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>()
            .join(", ");
        Err(self.fail(
            path,
            format!(
                "`{name}` is not a field of {}; its fields are {listed}",
                relation_label(self.plan, self.input)
            ),
        ))
    }

    /// Each of `names` is a field, none twice.
    fn fields(&self, names: &[String], path: &str, what: &str) -> Result<()> {
        if names.is_empty() {
            return Err(self.fail(path, format!("{what} needs at least one field name")));
        }
        for (index, name) in names.iter().enumerate() {
            let at = format!("{path}[{index}]");
            self.field(name, &at)?;
            if names[..index].contains(name) {
                return Err(self.fail(&at, format!("`{name}` is listed twice")));
            }
        }
        Ok(())
    }

    /// The assertion, checked.
    pub(super) fn assertion(&self, assert: &options::Assert) -> Result<Assertion> {
        match (&assert.equal, &assert.unique, &assert.set) {
            (Some(equal), None, None) => self.equal(equal),
            (None, Some(unique), None) => self.unique(unique),
            (None, None, Some(set)) => self.set(set),
            (equal, unique, set) => {
                let written: Vec<&str> = [
                    ("`equal`", equal.is_some()),
                    ("`unique`", unique.is_some()),
                    ("`set`", set.is_some()),
                ]
                .into_iter()
                .filter_map(|(key, present)| present.then_some(key))
                .collect();
                let found = if written.is_empty() {
                    "none".to_string()
                } else {
                    written.join(" and ")
                };
                Err(self.fail(
                    "assert",
                    format!("holds exactly one of `equal`, `unique` and `set`; found {found}"),
                ))
            }
        }
    }

    fn unique(&self, unique: &options::Unique) -> Result<Assertion> {
        self.fields(&unique.key, "assert.unique.key", "`key:`")?;
        let determines = match &unique.determines {
            Some(determines) => {
                self.fields(determines, "assert.unique.determines", "`determines:`")?;
                if let Some((index, name)) = determines
                    .iter()
                    .enumerate()
                    .find(|(_, name)| unique.key.contains(name))
                {
                    return Err(self.fail(
                        &format!("assert.unique.determines[{index}]"),
                        format!("`{name}` is part of the key, which determines it trivially"),
                    ));
                }
                determines.clone()
            }
            None => Vec::new(),
        };
        Ok(Assertion::Unique {
            key: unique.key.clone(),
            determines,
        })
    }

    fn equal(&self, equal: &options::Equal) -> Result<Assertion> {
        self.field(&equal.field, "assert.equal.field")?;
        let Some(to) = &equal.to else {
            if let Some(group_by) = &equal.group_by {
                self.field(group_by, "assert.equal.group_by")?;
                if group_by == &equal.field {
                    return Err(self.fail(
                        "assert.equal.group_by",
                        format!(
                            "groups by `{group_by}`, the field it compares, so every group agrees"
                        ),
                    ));
                }
            }
            return Ok(Assertion::EqualGroups {
                field: equal.field.clone(),
                group_by: equal.group_by.clone(),
            });
        };
        if equal.group_by.is_some() {
            return Err(self.fail(
                "assert.equal.group_by",
                "an `equal` with `to:` compares every row with one row or field, and takes no \
                 `group_by:`",
            ));
        }
        match (&to.row, &to.field) {
            (Some(row), None) => {
                if row.is_empty() {
                    return Err(self.fail(
                        "assert.equal.to.row",
                        "names the source row by one or more fields and their literals; it \
                         is empty",
                    ));
                }
                let mut literals = Vec::with_capacity(row.len());
                for (name, literal) in row {
                    let path = format!("assert.equal.to.row.{name}");
                    self.field(name, &path)?;
                    literals.push((name.clone(), self.literal(literal, name, &path)?));
                }
                Ok(Assertion::EqualSource {
                    field: equal.field.clone(),
                    row: literals,
                })
            }
            (None, Some(other)) => {
                self.field(other, "assert.equal.to.field")?;
                if other == &equal.field {
                    return Err(self.fail(
                        "assert.equal.to.field",
                        format!("compares `{other}` with itself"),
                    ));
                }
                Ok(Assertion::EqualField {
                    field: equal.field.clone(),
                    other: other.clone(),
                })
            }
            _ => Err(self.fail(
                "assert.equal.to",
                "holds exactly one of `row:` (a named source row) and `field:` (another field \
                 of the same row)",
            )),
        }
    }

    fn set(&self, set: &options::Set) -> Result<Assertion> {
        self.field(&set.field, "assert.set.field")?;
        if set.plurals == Some(Plurals::Cldr) {
            return Err(self.not_yet("assert.set.plurals", "`plurals: cldr`"));
        }
        if set.groups.as_ref().is_some_and(|groups| !groups.is_empty()) {
            return Err(self.not_yet("assert.set.groups", "`groups:`"));
        }
        let targets = [
            (SetMode::EqualTo, &set.equal_to),
            (SetMode::SubsetOf, &set.subset_of),
            (SetMode::DisjointFrom, &set.disjoint_from),
        ];
        let written: Vec<_> = targets
            .iter()
            .filter_map(|(mode, target)| target.as_ref().map(|target| (*mode, target)))
            .collect();
        let [(mode, target)] = written.as_slice() else {
            return Err(self.fail(
                "assert.set",
                "holds exactly one of `equal_to:`, `subset_of:` and `disjoint_from:`",
            ));
        };
        let path = format!("assert.set.{}", mode.key());
        if target.relation.is_some() || target.field.is_some() {
            return Err(self.not_yet(&path, "a `set` against another relation's field"));
        }
        let Some(group) = &target.group else {
            return Err(self.fail(
                &path,
                "names the reference group: `{ group: literal }`, the rows whose `group_by:` \
                 field holds the literal",
            ));
        };
        let Some(group_by) = &set.group_by else {
            return Err(self.fail(
                &path,
                "a reference group (`{ group: ... }`) is a value of the `group_by:` field, and \
                 this `set` has no `group_by:`",
            ));
        };
        self.field(group_by, "assert.set.group_by")?;
        if group_by == &set.field {
            return Err(self.fail(
                "assert.set.group_by",
                format!("groups by `{group_by}`, the field whose values it compares"),
            ));
        }
        if self.is_path_token(self.input, group_by) {
            return Err(self.not_yet(
                "assert.set.group_by",
                "a `set` grouped by `path` or a path token (where every resource forms a \
                 group, rows or not)",
            ));
        }
        let group = self.literal(group, group_by, &format!("{path}.group"))?;
        Ok(Assertion::Set {
            field: set.field.clone(),
            group_by: group_by.clone(),
            group,
            mode: *mode,
        })
    }

    /// Literal `literal`, compared with field `field`, at `path`: a string,
    /// or a boolean for a field that can hold one.
    fn literal(&self, literal: &options::Literal, field: &str, path: &str) -> Result<Value> {
        // A core-schema tag (`!!str`) was applied when the YAML was read; a
        // value under any other tag reaches the options as a mapping, which
        // is no literal.
        let written = &literal.0;
        if written.is_null() {
            return Err(self.fail(
                path,
                "null matches nothing, so no row ever holds it; write a string or a boolean",
            ));
        }
        if written.is_number() {
            return Err(self.fail(
                path,
                "a number cannot be written as a literal in this build: a value compares by \
                 the text it is written with (`1.10` is not `1.1`), which the config's YAML \
                 does not keep; write it as a string (`'1.10'`) to match a string value",
            ));
        }
        let Some(value) = Value::from_literal(written) else {
            return Err(self.fail(path, "expected a literal: a string or a boolean"));
        };
        if let Value::Bool(flag) = value
            && self.holds(self.input, field) == Holds::Strings
        {
            return Err(self.fail(
                path,
                format!(
                    "`{field}` holds strings, and a boolean never equals a string; quote it \
                     (`'{flag}'`) to match that text"
                ),
            ));
        }
        Ok(value)
    }

    /// Each `{{row.FIELD}}` of `message` names a field.
    pub(super) fn message(&self, message: &str) -> Result<()> {
        let mut rest = message;
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else {
                break;
            };
            if let Some(("row", name)) = after[..end]
                .trim()
                .split_once('.')
                .map(|(namespace, name)| (namespace.trim(), name.trim()))
            {
                self.field(name, "message")?;
            }
            rest = &after[end + 2..];
        }
        Ok(())
    }

    /// What field `name` of relation `id` holds once its transforms ran.
    fn holds(&self, id: RelationId, name: &str) -> Holds {
        let mut at = (id, name.to_string());
        for _ in 0..=self.plan.relations.len() {
            let relation = &self.plan.relation(at.0).relation;
            if !relation.transforms_of(&at.1).is_empty() {
                return Holds::Strings;
            }
            let Some(field) = relation.field(&at.1) else {
                return Holds::Any;
            };
            match (&field.source, &relation.rows) {
                (
                    FieldSource::Path(_)
                    | FieldSource::EntryKey
                    | FieldSource::EntryParent
                    | FieldSource::EntryRoot
                    | FieldSource::EntryPath
                    | FieldSource::Value,
                    _,
                ) => return Holds::Strings,
                (FieldSource::Side(side, copied), Rows::Join(join)) => {
                    let next = match side {
                        JoinSide::Left => join.left.id,
                        JoinSide::Right => join.right.id,
                    };
                    at = (next, copied.clone());
                }
                _ => return Holds::Any,
            }
        }
        Holds::Any
    }

    /// Whether field `name` of relation `id` is `path` or a path token, or a
    /// join's copy of one.
    fn is_path_token(&self, id: RelationId, name: &str) -> bool {
        let mut at = (id, name.to_string());
        for _ in 0..=self.plan.relations.len() {
            let relation = &self.plan.relation(at.0).relation;
            match (
                relation.field(&at.1).map(|field| &field.source),
                &relation.rows,
            ) {
                (Some(FieldSource::Path(_)), _) => return true,
                (Some(FieldSource::Side(side, copied)), Rows::Join(join)) => {
                    let next = match side {
                        JoinSide::Left => join.left.id,
                        JoinSide::Right => join.right.id,
                    };
                    at = (next, copied.clone());
                }
                _ => return false,
            }
        }
        false
    }
}
