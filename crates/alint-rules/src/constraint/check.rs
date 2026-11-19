//! The builder's check of a `constraint` rule against its input relation
//! (design section 8, "plan check"): every field the assertion and the
//! message name exists, every literal can match the field it is compared
//! with, and each `set` option fits the fields it reads.

use std::collections::HashMap;

use alint_core::relation::plan::{
    FieldSource, JoinSide, Plan, RelationId, Rows, RuleReference, Transform,
};
use alint_core::relation::{StringTransform, Value};
use alint_core::{Error, Result};

use super::options::{self, Plurals};
use super::{Assertion, ResourceGroups, SetAssertion, SetMode, SetTarget, relation_label};

/// Checks one rule's options against relation `input` of `plan`.
pub(super) struct Checker<'p> {
    plan: &'p Plan,
    input: RelationId,
    rule: &'p str,
    /// The relations the rule's fields name, as the loader resolved them
    /// (`input`, and a `set`'s `{ relation: R }`).
    references: &'p [RuleReference],
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
    pub(super) fn new(
        plan: &'p Plan,
        input: RelationId,
        rule: &'p str,
        references: &'p [RuleReference],
    ) -> Self {
        Self {
            plan,
            input,
            rule,
            references,
        }
    }

    fn fail(&self, path: &str, message: impl std::fmt::Display) -> Error {
        Error::rule_config(self.rule, format!("`{path}`: {message}"))
    }

    /// Refuse `name` at `path` unless the input relation has that field.
    fn field(&self, name: &str, path: &str) -> Result<()> {
        self.field_of(self.input, name, path)
    }

    /// Refuse `name` at `path` unless relation `id` has that field.
    fn field_of(&self, id: RelationId, name: &str, path: &str) -> Result<()> {
        let names: Vec<&str> = self
            .plan
            .relation(id)
            .relation
            .fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
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
                relation_label(self.plan, id)
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
        if let Some(group_by) = &set.group_by {
            self.field(group_by, "assert.set.group_by")?;
            if group_by == &set.field {
                return Err(self.fail(
                    "assert.set.group_by",
                    format!("groups by `{group_by}`, the field whose values it compares"),
                ));
            }
        }
        let target = self.set_target(set, *mode, target, &path)?;
        let plurals = set.plurals == Some(Plurals::Cldr);
        if plurals {
            if matches!(target, SetTarget::Relation { .. }) {
                return Err(self.fail(
                    "assert.set.plurals",
                    "`plurals: cldr` decides each plural group over the key paths of two groups \
                     of one relation, so it takes a `{ group: }` target, not `{ relation: R, \
                     field: F }`",
                ));
            }
            self.key_paths(&set.field)?;
        }
        let groups = self.groups(set, *mode, &target)?;
        let resource_groups = match &set.group_by {
            Some(group_by) => self.resource_groups(group_by)?,
            None => None,
        };
        Ok(Assertion::Set(Box::new(SetAssertion {
            field: set.field.clone(),
            group_by: set.group_by.clone(),
            resource_groups,
            target,
            mode: *mode,
            groups,
            plurals,
        })))
    }

    /// The other set of a `set` in mode `mode`: `{ group: literal }`, a
    /// value of `group_by:`, or `{ relation: R, field: F }`, whose relation
    /// the loader resolved.
    fn set_target(
        &self,
        set: &options::Set,
        mode: SetMode,
        target: &options::SetTarget,
        path: &str,
    ) -> Result<SetTarget> {
        let both = "`{ group: literal }` (a group of this relation) or `{ relation: R, field: F }` \
                    (every value of another relation's field)";
        match (&target.group, &target.relation, &target.field) {
            (Some(group), None, None) => {
                let Some(group_by) = &set.group_by else {
                    return Err(self.fail(
                        path,
                        "a reference group (`{ group: ... }`) is a value of the `group_by:` \
                         field, and this `set` has no `group_by:`",
                    ));
                };
                Ok(SetTarget::Group(self.literal(
                    group,
                    group_by,
                    &format!("{path}.group"),
                )?))
            }
            (None, Some(_), Some(field)) => {
                let field_name = format!("assert.set.{}.relation", mode.key());
                let Some(reference) = self
                    .references
                    .iter()
                    .find(|reference| reference.field == field_name)
                else {
                    return Err(self.fail(
                        &format!("{path}.relation"),
                        "the loader did not resolve the relation; this is a bug in alint, so \
                         please report it",
                    ));
                };
                let relation = reference.relation.id;
                let name = &self.plan.relation(relation).name;
                let written = match &reference.relation.alias {
                    Some(alias) => format!("{alias}.{name}"),
                    None => name.clone(),
                };
                self.field_of(relation, field, &format!("{path}.field"))?;
                Ok(SetTarget::Relation {
                    relation,
                    field: field.clone(),
                    written,
                })
            }
            (None, Some(_), None) => Err(self.fail(
                path,
                "names another relation's field as `{ relation: R, field: F }`, and `field:` is \
                 missing",
            )),
            (None, None, Some(_)) => Err(self.fail(
                path,
                "names another relation's field as `{ relation: R, field: F }`, and `relation:` \
                 is missing",
            )),
            (None, None, None) => Err(self.fail(path, format!("names the other set: {both}"))),
            (Some(_), _, _) => {
                Err(self.fail(path, format!("names one other set: {both}, not both")))
            }
        }
    }

    /// `groups:`: literals of the `group_by:` field, none twice and none the
    /// reference group, which must have rows anyway, under `equal_to:` (in
    /// mode `mode`): the only mode in which a group without rows can fail.
    fn groups(&self, set: &options::Set, mode: SetMode, target: &SetTarget) -> Result<Vec<Value>> {
        let Some(literals) = set.groups.as_ref().filter(|groups| !groups.is_empty()) else {
            return Ok(Vec::new());
        };
        let Some(group_by) = &set.group_by else {
            return Err(self.fail(
                "assert.set.groups",
                "names groups, which are values of the `group_by:` field, and this `set` has no \
                 `group_by:`",
            ));
        };
        if mode != SetMode::EqualTo {
            return Err(self.fail(
                "assert.set.groups",
                format!(
                    "names groups that must exist, which only `equal_to:` checks: under `{}:` a \
                     group without rows holds no value, so it passes; compare with `equal_to:`, \
                     or drop `groups:`",
                    mode.key()
                ),
            ));
        }
        let mut groups: Vec<Value> = Vec::with_capacity(literals.len());
        // Each literal's place, by value.
        let mut seen: HashMap<Value, usize> = HashMap::with_capacity(literals.len());
        for (index, literal) in literals.iter().enumerate() {
            let at = format!("assert.set.groups[{index}]");
            let value = self.literal(literal, group_by, &at)?;
            if let Some(earlier) = seen.insert(value.clone(), index) {
                return Err(self.fail(
                    &at,
                    format!("names the group `assert.set.groups[{earlier}]` names"),
                ));
            }
            if matches!(target, SetTarget::Group(reference) if *reference == value) {
                return Err(self.fail(
                    &at,
                    "names the reference group, which must have rows anyway (a reference group \
                     without rows is an analysis failure); list only the groups compared with it",
                ));
            }
            groups.push(value);
        }
        Ok(groups)
    }

    /// When field `name` of the input is `path` or a path token, or a join's
    /// copy of one from the side whose resources the join lists (the side
    /// that is not `unique:`): how a resource gets the group its rows would
    /// have. A join's copy from its `unique:` side is refused: the join does
    /// not list that side's resources.
    fn resource_groups(&self, name: &str) -> Result<Option<ResourceGroups>> {
        let mut at = (self.input, name.to_string());
        // Each relation's transforms of the field, outermost first.
        let mut levels: Vec<&[Transform]> = Vec::new();
        for _ in 0..=self.plan.relations.len() {
            let relation = &self.plan.relation(at.0).relation;
            levels.push(relation.transforms_of(&at.1));
            match (
                relation.field(&at.1).map(|field| &field.source),
                &relation.rows,
            ) {
                (Some(FieldSource::Path(token)), _) => {
                    let mut transforms = Vec::new();
                    for transform in levels.iter().rev().flat_map(|level| level.iter()) {
                        transforms.push(match transform {
                            Transform::Trim => StringTransform::Trim,
                            Transform::Lower => StringTransform::Lower,
                            Transform::SemverMajor => StringTransform::SemverMajor,
                            Transform::SemverMinor => StringTransform::SemverMinor,
                            // The plan check refuses it on anything but an
                            // `entry.path` field.
                            Transform::StripRoot => {
                                return Err(self.fail(
                                    "assert.set.group_by",
                                    "`strip-root` applies to a path token; this is a bug in \
                                     alint's plan check, so please report it",
                                ));
                            }
                        });
                    }
                    return Ok(Some(ResourceGroups {
                        token: *token,
                        transforms,
                    }));
                }
                (Some(FieldSource::Side(side, copied)), Rows::Join(join)) => {
                    let next = match side {
                        JoinSide::Left => join.left.id,
                        JoinSide::Right => join.right.id,
                    };
                    if *side == join.unique && self.is_path_token(next, copied) {
                        return Err(self.fail(
                            "assert.set.group_by",
                            format!(
                                "`{name}` is a path token that {} copies from its `unique:` \
                                 side; a `set` grouped by a path token gives every resource the \
                                 relation reads a group, and a join reads the resources of its \
                                 other side, so group by a field of that side",
                                relation_label(self.plan, at.0)
                            ),
                        ));
                    }
                    at = (next, copied.clone());
                }
                _ => return Ok(None),
            }
        }
        Ok(None)
    }

    /// `plurals: cldr` merges key paths by their segments: `field:` must be
    /// an `entry.path` field, or a join's copy of one, whose text no
    /// transform but `strip-root` (which changes its segments too) changed.
    fn key_paths(&self, name: &str) -> Result<()> {
        let mut at = (self.input, name.to_string());
        for _ in 0..=self.plan.relations.len() {
            let relation = &self.plan.relation(at.0).relation;
            if let Some(transform) = relation
                .transforms_of(&at.1)
                .iter()
                .find(|transform| **transform != Transform::StripRoot)
            {
                return Err(self.fail(
                    "assert.set.plurals",
                    format!(
                        "`plurals: cldr` merges key paths by their segments, and `transform:` \
                         applies `{}` to `{}` in {}, which changes its text but not its \
                         segments; only `strip-root` may apply to the field",
                        transform_name(*transform),
                        at.1,
                        relation_label(self.plan, at.0)
                    ),
                ));
            }
            match (
                relation.field(&at.1).map(|field| &field.source),
                &relation.rows,
            ) {
                (Some(FieldSource::EntryPath), _) => return Ok(()),
                (Some(FieldSource::Side(side, copied)), Rows::Join(join)) => {
                    let next = match side {
                        JoinSide::Left => join.left.id,
                        JoinSide::Right => join.right.id,
                    };
                    at = (next, copied.clone());
                }
                _ => break,
            }
        }
        Err(self.fail(
            "assert.set.plurals",
            format!(
                "`plurals: cldr` merges key paths, so `field:` must be an `entry.path` field (or \
                 a join's copy of one), and `{name}` is not"
            ),
        ))
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

/// A transform as a configuration writes it.
fn transform_name(transform: Transform) -> &'static str {
    match transform {
        Transform::Trim => "trim",
        Transform::Lower => "lower",
        Transform::SemverMajor => "semver-major",
        Transform::SemverMinor => "semver-minor",
        Transform::StripRoot => "strip-root",
    }
}
