//! The builder's check of a `constraint` rule against its input relation
//! (design section 8, "plan check"; ADR-0050 D1 to D4): every field the
//! assertion and the message name exists, every literal can match the field it
//! is compared with, and each assertion's options fit the fields it reads.

use std::collections::HashMap;

use alint_core::relation::plan::{
    FieldSource, JoinSide, Plan, RelationId, Rows, RuleReference, Transform,
};
use alint_core::relation::{StringTransform, Value};
use alint_core::{Error, Result};

use super::options::{self, Plurals};
use super::{
    Assertion, CompareAssertion, CompareLiteral, CompareOp, CompareOrder, CompareTarget,
    CountAssertion, ReferenceAssertion, ResourceGroups, SetAssertion, SetMode, SetTarget,
    relation_label,
};

/// Checks one rule's options against relation `input` of `plan`.
pub(super) struct Checker<'p> {
    plan: &'p Plan,
    input: RelationId,
    rule: &'p str,
    /// The relations the rule's fields name, as the loader resolved them
    /// (`input`, a `set`'s `{ relation: R }`, or a `reference`'s `to.relation`).
    references: &'p [RuleReference],
}

/// What a field's values hold, as far as the plan knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holds {
    /// Strings (or null): a path or path token, a key, a key path, a `read:`
    /// value, or a value a transform made.
    Strings,
    /// Any value: an `entry.value`, a `bound:` value, or a `record:` value.
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

    /// Each of `names` is a field of `id`, none twice.
    fn fields_of(&self, id: RelationId, names: &[String], path: &str, what: &str) -> Result<()> {
        if names.is_empty() {
            return Err(self.fail(path, format!("{what} needs at least one field name")));
        }
        for (index, name) in names.iter().enumerate() {
            let at = format!("{path}[{index}]");
            self.field_of(id, name, &at)?;
            if names[..index].contains(name) {
                return Err(self.fail(&at, format!("`{name}` is listed twice")));
            }
        }
        Ok(())
    }

    fn fields(&self, names: &[String], path: &str, what: &str) -> Result<()> {
        self.fields_of(self.input, names, path, what)
    }

    /// Validate a single-or-list `group_by:` option against relation `id`.
    fn group_by_of(
        &self,
        id: RelationId,
        raw: Option<&options::FieldNameOrList>,
        path: &str,
        excluded_field: Option<&str>,
        excluded_reason: &str,
    ) -> Result<Vec<String>> {
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };
        let names = match raw {
            options::FieldNameOrList::One(name) => {
                self.field_of(id, name, path)?;
                vec![name.clone()]
            }
            options::FieldNameOrList::Many(names) => {
                self.fields_of(id, names, path, "`group_by:`")?;
                names.clone()
            }
        };
        if let Some(excluded) = excluded_field
            && names.iter().any(|name| name == excluded)
        {
            return Err(self.fail(path, format!("groups by `{excluded}`, {excluded_reason}")));
        }
        Ok(names)
    }

    /// Look up a resolved relation reference for `field_name`.
    fn resolved_relation(&self, field_name: &str, path: &str) -> Result<(RelationId, String)> {
        let Some(reference) = self
            .references
            .iter()
            .find(|reference| reference.field == field_name)
        else {
            return Err(self.fail(
                path,
                "the loader did not resolve the relation; this is a bug in alint, so please \
                 report it",
            ));
        };
        let relation = reference.relation.id;
        let name = &self.plan.relation(relation).name;
        let written = match &reference.relation.alias {
            Some(alias) => format!("{alias}.{name}"),
            None => name.clone(),
        };
        Ok((relation, written))
    }

    /// The assertion, checked.
    pub(super) fn assertion(&self, assert: &options::Assert) -> Result<Assertion> {
        let present: Vec<&str> = [
            ("`equal`", assert.equal.is_some()),
            ("`unique`", assert.unique.is_some()),
            ("`set`", assert.set.is_some()),
            ("`reference`", assert.reference.is_some()),
            ("`count`", assert.count.is_some()),
            ("`compare`", assert.compare.is_some()),
        ]
        .into_iter()
        .filter_map(|(key, on)| on.then_some(key))
        .collect();
        if present.len() != 1 {
            let found = if present.is_empty() {
                "none".to_string()
            } else {
                present.join(" and ")
            };
            return Err(self.fail(
                "assert",
                format!(
                    "holds exactly one of `equal`, `unique`, `set`, `reference`, `count` and \
                     `compare`; found {found}"
                ),
            ));
        }
        if let Some(equal) = &assert.equal {
            self.equal(equal)
        } else if let Some(unique) = &assert.unique {
            self.unique(unique)
        } else if let Some(set) = &assert.set {
            self.set(set)
        } else if let Some(reference) = &assert.reference {
            self.reference(reference)
        } else if let Some(count) = &assert.count {
            self.count(count)
        } else if let Some(compare) = &assert.compare {
            self.compare(compare)
        } else {
            unreachable!("checked present.len() == 1")
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
            let group_by = self.group_by_of(
                self.input,
                equal.group_by.as_ref(),
                "assert.equal.group_by",
                Some(&equal.field),
                "the field it compares, so every group agrees",
            )?;
            return Ok(Assertion::EqualGroups {
                field: equal.field.clone(),
                group_by,
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
            (Some(row), None) => Ok(Assertion::EqualSource {
                field: equal.field.clone(),
                row: self.source_row_literals(row, "assert.equal.to.row")?,
            }),
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

    fn source_row_literals(
        &self,
        row: &std::collections::BTreeMap<String, options::Literal>,
        path: &str,
    ) -> Result<Vec<(String, Value)>> {
        if row.is_empty() {
            return Err(self.fail(
                path,
                "names the source row by one or more fields and their literals; it is empty",
            ));
        }
        let mut literals = Vec::with_capacity(row.len());
        for (name, literal) in row {
            let at = format!("{path}.{name}");
            self.field(name, &at)?;
            literals.push((name.clone(), self.literal(literal, name, &at)?));
        }
        Ok(literals)
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
        let group_by = self.group_by_of(
            self.input,
            set.group_by.as_ref(),
            "assert.set.group_by",
            Some(&set.field),
            "the field whose values it compares",
        )?;
        let target = self.set_target(set, &group_by, *mode, target, &path)?;
        let plurals = set.plurals == Some(Plurals::Cldr);
        if plurals {
            if !matches!(target, SetTarget::Group(_)) {
                return Err(self.fail(
                    "assert.set.plurals",
                    "`plurals: cldr` decides each plural group over the key paths of two groups \
                     of one relation, so it takes a `{ group: }` target, not `{ relation: }` or \
                     `{ values: }`",
                ));
            }
            self.key_paths(&set.field)?;
        }
        let groups = self.set_groups(set, &group_by, *mode, &target)?;
        let resource_groups = self.multi_resource_groups(&group_by, "assert.set.group_by")?;
        let stub_root_groups = self.is_stub_root_group_by(&group_by);
        Ok(Assertion::Set(Box::new(SetAssertion {
            field: set.field.clone(),
            group_by,
            resource_groups,
            stub_root_groups,
            target,
            mode: *mode,
            groups,
            plurals,
        })))
    }

    #[allow(clippy::too_many_lines)]
    fn set_target(
        &self,
        set: &options::Set,
        group_by: &[String],
        mode: SetMode,
        target: &options::SetTarget,
        path: &str,
    ) -> Result<SetTarget> {
        let forms = "`{ group: literal }`, `{ values: [...] }`, or `{ relation: R, field: F }`";
        let has_rel_keys = target.relation.is_some()
            || target.field.is_some()
            || target.group_by.is_some()
            || target.all_target_groups.is_some();
        match (&target.group, &target.values, has_rel_keys) {
            (Some(group), None, false) => {
                if group_by.is_empty() {
                    return Err(self.fail(
                        path,
                        "a reference group (`{ group: ... }`) is a value of `group_by:`, and \
                         this `set` has no `group_by:`",
                    ));
                }
                Ok(SetTarget::Group(self.group_literal(
                    group,
                    group_by,
                    &format!("{path}.group"),
                )?))
            }
            (None, Some(values), false) => {
                if values.is_empty() {
                    return Err(self.fail(
                        &format!("{path}.values"),
                        "`values:` needs at least one literal",
                    ));
                }
                let mut parsed = Vec::with_capacity(values.len());
                for (index, literal) in values.iter().enumerate() {
                    let at = format!("{path}.values[{index}]");
                    parsed.push(self.literal(literal, &set.field, &at)?);
                }
                Ok(SetTarget::Values(parsed))
            }
            (None, None, true) => {
                let Some(_) = &target.relation else {
                    return Err(self.fail(
                        path,
                        "names another relation's field as `{ relation: R, field: F }`, and \
                         `relation:` is missing",
                    ));
                };
                let Some(field) = &target.field else {
                    return Err(self.fail(
                        path,
                        "names another relation's field as `{ relation: R, field: F }`, and \
                         `field:` is missing",
                    ));
                };
                let field_name = format!("assert.set.{}.relation", mode.key());
                let (relation, written) =
                    self.resolved_relation(&field_name, &format!("{path}.relation"))?;
                self.field_of(relation, field, &format!("{path}.field"))?;
                let target_group_by = self.group_by_of(
                    relation,
                    target.group_by.as_ref(),
                    &format!("{path}.group_by"),
                    Some(field),
                    "the field whose values it compares",
                )?;
                if !target_group_by.is_empty() {
                    if group_by.is_empty() {
                        return Err(self.fail(
                            &format!("{path}.group_by"),
                            "a per-group cross-relation target (`group_by:`) requires `group_by:` \
                             on the `set` assertion",
                        ));
                    }
                    if target_group_by.len() != group_by.len() {
                        return Err(self.fail(
                            &format!("{path}.group_by"),
                            format!(
                                "target `group_by:` has {} field(s), but `assert.set.group_by` \
                                 has {}; they must match in arity",
                                target_group_by.len(),
                                group_by.len()
                            ),
                        ));
                    }
                }
                if target.all_target_groups.is_some() {
                    if mode != SetMode::EqualTo {
                        return Err(self.fail(
                            &format!("{path}.all_target_groups"),
                            "`all_target_groups:` is valid only under `equal_to:`",
                        ));
                    }
                    if target_group_by.is_empty() {
                        return Err(self.fail(
                            &format!("{path}.all_target_groups"),
                            "`all_target_groups:` requires `group_by:` on the target relation",
                        ));
                    }
                }
                Ok(SetTarget::Relation {
                    relation,
                    field: field.clone(),
                    group_by: target_group_by,
                    all_target_groups: target.all_target_groups.unwrap_or(false),
                    written,
                })
            }
            (None, None, false) => Err(self.fail(path, format!("names the other set: {forms}"))),
            _ => Err(self.fail(
                path,
                format!("names one other set ({forms}), not a combination"),
            )),
        }
    }

    fn set_groups(
        &self,
        set: &options::Set,
        group_by: &[String],
        mode: SetMode,
        target: &SetTarget,
    ) -> Result<Vec<Vec<Value>>> {
        let Some(literals) = set.groups.as_ref().filter(|groups| !groups.is_empty()) else {
            return Ok(Vec::new());
        };
        if group_by.is_empty() {
            return Err(self.fail(
                "assert.set.groups",
                "names groups, which are values of `group_by:`, and this `set` has no `group_by:`",
            ));
        }
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
        let mut groups: Vec<Vec<Value>> = Vec::with_capacity(literals.len());
        let mut seen: HashMap<Vec<Value>, usize> = HashMap::with_capacity(literals.len());
        for (index, literal) in literals.iter().enumerate() {
            let at = format!("assert.set.groups[{index}]");
            let tuple = self.group_literal(literal, group_by, &at)?;
            if let Some(earlier) = seen.insert(tuple.clone(), index) {
                return Err(self.fail(
                    &at,
                    format!("names the group `assert.set.groups[{earlier}]` names"),
                ));
            }
            if matches!(target, SetTarget::Group(reference) if *reference == tuple) {
                return Err(self.fail(
                    &at,
                    "names the reference group, which must have rows anyway (a reference group \
                     without rows is an analysis failure); list only the groups compared with it",
                ));
            }
            groups.push(tuple);
        }
        Ok(groups)
    }

    fn reference(&self, reference: &options::ReferenceAssert) -> Result<Assertion> {
        let fields = match (&reference.field, &reference.fields) {
            (Some(field), None) => {
                self.field(field, "assert.reference.field")?;
                vec![field.clone()]
            }
            (None, Some(fields)) => {
                self.fields(fields, "assert.reference.fields", "`fields:`")?;
                fields.clone()
            }
            _ => {
                return Err(self.fail(
                    "assert.reference",
                    "holds exactly one of `field:` and `fields:`",
                ));
            }
        };
        let (target, written) = self.resolved_relation(
            "assert.reference.to.relation",
            "assert.reference.to.relation",
        )?;
        let target_fields = match (&reference.to.field, &reference.to.fields) {
            (Some(field), None) => {
                self.field_of(target, field, "assert.reference.to.field")?;
                vec![field.clone()]
            }
            (None, Some(fields)) => {
                self.fields_of(target, fields, "assert.reference.to.fields", "`fields:`")?;
                fields.clone()
            }
            _ => {
                return Err(self.fail(
                    "assert.reference.to",
                    "holds exactly one of `field:` and `fields:`",
                ));
            }
        };
        if fields.len() != target_fields.len() {
            return Err(self.fail(
                "assert.reference.to",
                format!(
                    "source has {} key field(s) and `to` has {}; they must have the same arity",
                    fields.len(),
                    target_fields.len()
                ),
            ));
        }
        Ok(Assertion::Reference(Box::new(ReferenceAssertion {
            fields,
            target,
            target_fields,
            written,
            skip_missing: reference.skip_missing.unwrap_or(false),
        })))
    }

    fn count(&self, count: &options::Count) -> Result<Assertion> {
        if count.min.is_none() && count.max.is_none() && count.exact.is_none() {
            return Err(self.fail(
                "assert.count",
                "needs at least one of `min:`, `max:` and `exact:`",
            ));
        }
        if count.exact.is_some() && (count.min.is_some() || count.max.is_some()) {
            return Err(self.fail(
                "assert.count.exact",
                "`exact:` is mutually exclusive with `min:` and `max:`",
            ));
        }
        if let (Some(min), Some(max)) = (count.min, count.max)
            && min > max
        {
            return Err(self.fail(
                "assert.count",
                format!("`min:` ({min}) is greater than `max:` ({max})"),
            ));
        }
        let distinct = count.distinct.unwrap_or(false);
        match (distinct, &count.field) {
            (true, Some(field)) => self.field(field, "assert.count.field")?,
            (true, None) => {
                return Err(self.fail(
                    "assert.count.distinct",
                    "`distinct: true` requires `field:` naming the field whose distinct values \
                     are counted",
                ));
            }
            (false, Some(_)) => {
                return Err(self.fail(
                    "assert.count.field",
                    "`field:` applies only when `distinct: true` is set",
                ));
            }
            (false, None) => {}
        }
        let group_by = self.group_by_of(
            self.input,
            count.group_by.as_ref(),
            "assert.count.group_by",
            count.field.as_deref(),
            "the field whose distinct values it counts",
        )?;
        let mut groups = Vec::new();
        if let Some(literals) = count.groups.as_ref().filter(|g| !g.is_empty()) {
            if group_by.is_empty() {
                return Err(self.fail(
                    "assert.count.groups",
                    "names groups, which are values of `group_by:`, and this `count` has no \
                     `group_by:`",
                ));
            }
            let requires_positive =
                count.min.is_some_and(|n| n > 0) || count.exact.is_some_and(|n| n > 0);
            if !requires_positive {
                return Err(self.fail(
                    "assert.count.groups",
                    "`groups:` names groups that must exist, which only `min > 0` or `exact > 0` \
                     checks",
                ));
            }
            let mut seen: HashMap<Vec<Value>, usize> = HashMap::with_capacity(literals.len());
            for (index, literal) in literals.iter().enumerate() {
                let at = format!("assert.count.groups[{index}]");
                let tuple = self.group_literal(literal, &group_by, &at)?;
                if let Some(earlier) = seen.insert(tuple.clone(), index) {
                    return Err(self.fail(
                        &at,
                        format!("names the group `assert.count.groups[{earlier}]` names"),
                    ));
                }
                groups.push(tuple);
            }
        }
        let resource_groups = self.multi_resource_groups(&group_by, "assert.count.group_by")?;
        let stub_root_groups = self.is_stub_root_group_by(&group_by);
        Ok(Assertion::Count(Box::new(CountAssertion {
            group_by,
            resource_groups,
            stub_root_groups,
            field: count.field.clone(),
            min: count.min,
            max: count.max,
            exact: count.exact,
            groups,
        })))
    }

    fn compare(&self, compare: &options::Compare) -> Result<Assertion> {
        self.field(&compare.field, "assert.compare.field")?;
        let order = match compare.order.unwrap_or_default() {
            options::CompareOrder::Exact => CompareOrder::Exact,
            options::CompareOrder::Lexical => CompareOrder::Lexical,
            options::CompareOrder::Numeric => CompareOrder::Numeric,
        };
        let op = match compare.op {
            options::CompareOp::Eq => CompareOp::Eq,
            options::CompareOp::Ne => CompareOp::Ne,
            options::CompareOp::Lt => CompareOp::Lt,
            options::CompareOp::Le => CompareOp::Le,
            options::CompareOp::Gt => CompareOp::Gt,
            options::CompareOp::Ge => CompareOp::Ge,
        };
        if order == CompareOrder::Exact && !matches!(op, CompareOp::Eq | CompareOp::Ne) {
            return Err(self.fail(
                "assert.compare.op",
                format!(
                    "`op: {}` requires an ordered `order:` (`lexical` or `numeric`); `order: \
                     exact` supports only `eq` and `ne`",
                    op.key()
                ),
            ));
        }
        let target = match (&compare.to.field, &compare.to.value, &compare.to.row) {
            (Some(other), None, None) => {
                self.field(other, "assert.compare.to.field")?;
                if other == &compare.field {
                    return Err(self.fail(
                        "assert.compare.to.field",
                        format!("compares `{other}` with itself"),
                    ));
                }
                CompareTarget::Field(other.clone())
            }
            (None, Some(value), None) => CompareTarget::Value(self.compare_literal(
                value,
                &compare.field,
                order,
                "assert.compare.to.value",
            )?),
            (None, None, Some(row)) => {
                CompareTarget::Row(self.source_row_literals(row, "assert.compare.to.row")?)
            }
            _ => {
                return Err(self.fail(
                    "assert.compare.to",
                    "holds exactly one of `field:`, `value:`, and `row:`",
                ));
            }
        };
        Ok(Assertion::Compare(Box::new(CompareAssertion {
            field: compare.field.clone(),
            op,
            order,
            target,
        })))
    }

    fn compare_literal(
        &self,
        written: &alint_core::yaml::Value,
        field: &str,
        order: CompareOrder,
        path: &str,
    ) -> Result<CompareLiteral> {
        match order {
            CompareOrder::Exact => {
                let lit = options::Literal(written.clone());
                Ok(CompareLiteral::Scalar(self.literal(&lit, field, path)?))
            }
            CompareOrder::Lexical => match written.as_str() {
                Some(s) => Ok(CompareLiteral::Scalar(Value::String(s.into()))),
                None => Err(self.fail(
                    path,
                    "`order: lexical` requires a string literal in `to.value`",
                )),
            },
            CompareOrder::Numeric => {
                let Some(num) = written.as_f64() else {
                    return Err(self.fail(
                        path,
                        "`order: numeric` requires a finite number literal in `to.value`",
                    ));
                };
                if !num.is_finite() {
                    return Err(self.fail(
                        path,
                        "`order: numeric` rejects non-finite numbers (`NaN`/`inf`) in `to.value`",
                    ));
                }
                let raw = written
                    .as_i64()
                    .map(|i| i.to_string())
                    .or_else(|| written.as_u64().map(|u| u.to_string()))
                    .unwrap_or_else(|| num.to_string());
                Ok(CompareLiteral::Number(raw))
            }
        }
    }

    /// Validate a group key literal (scalar for 1-field `group_by`, k-array
    /// for k-field `group_by`).
    fn group_literal(
        &self,
        literal: &options::GroupLiteral,
        group_by: &[String],
        path: &str,
    ) -> Result<Vec<Value>> {
        match (group_by.len(), literal) {
            (1, options::GroupLiteral::Single(lit)) => {
                Ok(vec![self.literal(lit, &group_by[0], path)?])
            }
            (1, options::GroupLiteral::Tuple(_)) => Err(self.fail(
                path,
                "a single-field `group_by:` expects a scalar group literal, not a list",
            )),
            (k, options::GroupLiteral::Single(_)) => Err(self.fail(
                path,
                format!(
                    "`group_by:` has {k} fields, so each group literal must be a list of length \
                     {k}"
                ),
            )),
            (k, options::GroupLiteral::Tuple(items)) => {
                if items.len() != k {
                    return Err(self.fail(
                        path,
                        format!(
                            "group literal has {} element(s), but `group_by:` has {k} field(s)",
                            items.len()
                        ),
                    ));
                }
                let mut out = Vec::with_capacity(k);
                for (index, (lit, field)) in items.iter().zip(group_by).enumerate() {
                    let at = format!("{path}[{index}]");
                    out.push(self.literal(lit, field, &at)?);
                }
                Ok(out)
            }
        }
    }

    /// When every field in `group_by` derives from a path token (on the
    /// non-unique side of any join), returns one `ResourceGroups` per field;
    /// otherwise returns an empty vec.
    fn multi_resource_groups(
        &self,
        group_by: &[String],
        path: &str,
    ) -> Result<Vec<ResourceGroups>> {
        if group_by.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(group_by.len());
        for name in group_by {
            match self.resource_groups(name, path)? {
                Some(rg) => out.push(rg),
                None => return Ok(Vec::new()),
            }
        }
        Ok(out)
    }

    fn resource_groups(&self, name: &str, path: &str) -> Result<Option<ResourceGroups>> {
        let mut at = (self.input, name.to_string());
        let mut levels: Vec<&[Transform]> = Vec::new();
        for _ in 0..=self.plan.relations.len() {
            let relation = &self.plan.relation(at.0).relation;
            levels.push(relation.transforms_of(&at.1));
            if relation.uses_part_fields() {
                let sources = relation.field_sources(&at.1);
                if let Some(FieldSource::Path(token)) = sources.first().map(|f| &f.source)
                    && sources
                        .iter()
                        .all(|f| f.source == FieldSource::Path(*token))
                {
                    let transforms = self.collect_string_transforms(&levels, path)?;
                    return Ok(Some(ResourceGroups {
                        token: *token,
                        transforms,
                    }));
                }
                return Ok(None);
            }
            match (
                relation.field(&at.1).map(|field| &field.source),
                &relation.rows,
            ) {
                (Some(FieldSource::Path(token)), _) => {
                    let transforms = self.collect_string_transforms(&levels, path)?;
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
                            path,
                            format!(
                                "`{name}` is a path token that {} copies from its `unique:` \
                                 side; grouping by a path token gives every resource the relation \
                                 reads a group, and a join reads the resources of its other side, \
                                 so group by a field of that side",
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

    fn collect_string_transforms(
        &self,
        levels: &[&[Transform]],
        path: &str,
    ) -> Result<Vec<StringTransform>> {
        let mut transforms = Vec::new();
        for transform in levels.iter().rev().flat_map(|level| level.iter()) {
            transforms.push(match transform {
                Transform::Trim => StringTransform::Trim,
                Transform::Lower => StringTransform::Lower,
                Transform::SemverMajor => StringTransform::SemverMajor,
                Transform::SemverMinor => StringTransform::SemverMinor,
                Transform::Split { on, part, required } => StringTransform::Split {
                    on: on.clone(),
                    part: *part,
                    required: *required,
                },
                Transform::StripPrefix(p) => StringTransform::StripPrefix(p.clone()),
                Transform::StripSuffix(s) => StringTransform::StripSuffix(s.clone()),
                Transform::Prefix(p) => StringTransform::Prefix(p.clone()),
                Transform::Suffix(s) => StringTransform::Suffix(s.clone()),
                Transform::StripRoot => {
                    return Err(self.fail(
                        path,
                        "`strip-root` applies to a path token; this is a bug in alint's plan \
                         check, so please report it",
                    ));
                }
            });
        }
        Ok(transforms)
    }

    fn is_stub_root_group_by(&self, group_by: &[String]) -> bool {
        !group_by.is_empty()
            && group_by
                .iter()
                .any(|name| self.is_entry_root(self.input, name))
            && group_by.iter().all(|name| {
                self.is_entry_root(self.input, name) || self.is_path_token(self.input, name)
            })
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
                        transform_name(transform),
                        at.1,
                        relation_label(self.plan, at.0)
                    ),
                ));
            }
            if relation.uses_part_fields() {
                let sources = relation.field_sources(&at.1);
                if !sources.is_empty() && sources.iter().all(|f| f.source == FieldSource::EntryPath)
                {
                    return Ok(());
                }
                break;
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

    /// Each `{{row.FIELD}}` of `message` names a field, and `{{value}}` is
    /// accepted only on `set` and `reference` assertions (#594, ADR-0050 D9);
    /// arbitrary non-alint `{{...}}` tokens stay literal.
    pub(super) fn message(&self, message: &str, assertion: &Assertion) -> Result<()> {
        let mut rest = message;
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else {
                break;
            };
            let inner = after[..end].trim();
            if inner == "value" {
                if !matches!(assertion, Assertion::Set(_) | Assertion::Reference(_)) {
                    return Err(self.fail(
                        "message",
                        "`{{value}}` is only available on `set` and `reference` assertions; use \
                         `{{row.FIELD}}` to reference a field of the reported row",
                    ));
                }
            } else if let Some(("row", name)) = inner
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
            if relation.uses_part_fields() {
                let sources = relation.field_sources(&at.1);
                return if !sources.is_empty()
                    && sources.iter().all(|f| source_is_strings(&f.source))
                {
                    Holds::Strings
                } else {
                    Holds::Any
                };
            }
            let Some(field) = relation.field(&at.1) else {
                return Holds::Any;
            };
            match (&field.source, &relation.rows) {
                (source, _) if source_is_strings(source) => return Holds::Strings,
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

    /// Whether field `name` of relation `id` reads `entry.root` directly or
    /// copies an `entry.root` field through a `join:` (#598).
    fn is_entry_root(&self, id: RelationId, name: &str) -> bool {
        let mut at = (id, name.to_string());
        for _ in 0..=self.plan.relations.len() {
            let relation = &self.plan.relation(at.0).relation;
            if relation.uses_part_fields() {
                let sources = relation.field_sources(&at.1);
                return !sources.is_empty()
                    && sources.iter().all(|f| f.source == FieldSource::EntryRoot);
            }
            match (
                relation.field(&at.1).map(|field| &field.source),
                &relation.rows,
            ) {
                (Some(FieldSource::EntryRoot), _) => return true,
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

    /// Whether field `name` of relation `id` is `path` or a path token, or a
    /// join's copy of one.
    fn is_path_token(&self, id: RelationId, name: &str) -> bool {
        let mut at = (id, name.to_string());
        for _ in 0..=self.plan.relations.len() {
            let relation = &self.plan.relation(at.0).relation;
            if relation.uses_part_fields() {
                let sources = relation.field_sources(&at.1);
                return !sources.is_empty()
                    && sources
                        .iter()
                        .all(|f| matches!(f.source, FieldSource::Path(_)));
            }
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

fn source_is_strings(source: &FieldSource) -> bool {
    matches!(
        source,
        FieldSource::Path(_)
            | FieldSource::EntryKey
            | FieldSource::EntryParent
            | FieldSource::EntryRoot
            | FieldSource::EntryPath
            | FieldSource::Value
    )
}

/// A transform as a configuration writes it.
fn transform_name(transform: &Transform) -> &'static str {
    match transform {
        Transform::Trim => "trim",
        Transform::Lower => "lower",
        Transform::SemverMajor => "semver-major",
        Transform::SemverMinor => "semver-minor",
        Transform::StripRoot => "strip-root",
        Transform::Split { .. } => "split",
        Transform::StripPrefix(_) => "strip_prefix",
        Transform::StripSuffix(_) => "strip_suffix",
        Transform::Prefix(_) => "prefix",
        Transform::Suffix(_) => "suffix",
    }
}
