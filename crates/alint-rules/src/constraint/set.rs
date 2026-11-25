//! `set` (design `v0.23/selectors-and-constraints.md` section 6, ADR-0050 D3):
//! the set of `field:`'s values in each group, against another set: a reference
//! group of the same relation (`{ group: literal | [literals...] }`), an inline
//! list of literals (`{ values: [...] }`), or every value of another relation's
//! field (`{ relation: R, field: F, group_by?: ..., all_target_groups?: bool }`,
//! a semi-join: section 8).
//!
//! - **Groups:** every tuple of `group_by:` values (or the relation, as one
//!   group), every resource the relation reads when every `group_by:` field is
//!   a path token (or `entry.root` stubs under #598), and every literal of
//!   `groups:` (plus absent target groups when `all_target_groups: true`). A
//!   group with no rows lacks every value of the other set: at its first
//!   resource, or without a path for a group only `groups:` or
//!   `all_target_groups:` names.
//! - **Findings:** a missing value sits at the structural counterpart (#595) or
//!   the group's first resource, related to the row of the other set that holds
//!   it (when backed by rows); an extra value sits at the row that holds it; a
//!   shared value sits at the row that holds it, related to a row of the other
//!   set that holds it.
//! - **Cost:** one pass over the rows builds each set as a hash map, and a
//!   group's values are looked up in the other set, so a comparison costs
//!   the group's values, plus, for `equal_to:`, the other set's (each value
//!   it lacks is a finding). `plurals: cldr` decides each plural group on
//!   demand ([`Probe`]), so it keeps those bounds.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use alint_core::MAX_RELATED_LOCATIONS;
use alint_core::relation::eval::{Cell, RelationRows};
use alint_core::relation::{LimitKind, Pointer, RelPath, Resource, Value};

use super::{Groups, Place, Related, Run, literal_type, spot_in, type_of};
use crate::constraint::values::{Key, Kind, Scalar};
use crate::constraint::{CounterpartIndex, SetAssertion, SetMode, SetTarget};
use crate::plural_groups::{Keys, Probe, Side};

/// One group of a `set`.
struct SetGroup<'r> {
    /// Its `group_by:` value tuple; none when the relation is one group.
    value: Option<Vec<Scalar>>,
    rows: Vec<usize>,
    /// Its first resource in canonical order, where a value it lacks sits
    /// when no structural counterpart matches: none for a group only
    /// `groups:` or `all_target_groups:` names.
    first: Option<&'r RelPath>,
    /// Every resource belonging to this group in canonical order, used by
    /// target-file placement (#595).
    resources: BTreeSet<&'r RelPath>,
    /// Only a literal of `groups:` names it, so its value is shown as a
    /// literal is.
    literal: bool,
}

/// Values held by the target side of a `set` assertion.
enum OtherHeld {
    /// One target set shared by every input group.
    Single(Groups),
    /// Per-group target sets keyed by `to.group_by:` tuple values (ADR-0050 D3).
    ByGroup(HashMap<Vec<Scalar>, Groups>),
}

/// The set (or per-group map of sets) each group is compared with.
struct Other<'r> {
    /// The rows that hold its values: the input's for a reference group, the
    /// other relation's for `{ relation: }`, or `None` for `{ values: [...] }`.
    rows: Option<&'r RelationRows>,
    /// The field of `rows` that holds them (0 when `rows` is `None`).
    at: usize,
    held: OtherHeld,
    /// The reference group's place among the groups; none for another
    /// relation's field or inline values.
    group: Option<usize>,
    /// How a message names it: `` `locale` "en" ``, `` `name` of relation
    /// `names` ``, or `the listed values`.
    shown: String,
    /// The label of a related location at a row of it.
    label: String,
    /// Target-side sensitivity per `to.group_by:` column (defence in depth).
    target_group_sensitive: Vec<bool>,
}

impl Other<'_> {
    fn held_for(&self, group: &SetGroup<'_>) -> Option<&Groups> {
        match &self.held {
            OtherHeld::Single(groups) => Some(groups),
            OtherHeld::ByGroup(map) => group.value.as_ref().and_then(|key| map.get(key)),
        }
    }
}

/// What one comparison of a group reads.
struct Compared<'c, 'r> {
    set: &'c SetAssertion,
    field_at: usize,
    group_at: &'c [usize],
    /// Each row's value of the compared field.
    values: &'c [Scalar],
    other: &'c Other<'r>,
}

/// Attach `path` to group `value`, seeding a zero-row group if none exists yet.
fn add_group_resource<'r>(
    out: &mut Vec<SetGroup<'r>>,
    index: &mut HashMap<Vec<Scalar>, usize>,
    value: Vec<Scalar>,
    path: &'r RelPath,
) {
    if let Some(&at) = index.get(&value) {
        out[at].resources.insert(path);
        out[at].first = out[at].resources.iter().next().copied();
    } else {
        index.insert(value.clone(), out.len());
        out.push(SetGroup {
            value: Some(value),
            rows: Vec::new(),
            first: Some(path),
            resources: BTreeSet::from([path]),
            literal: false,
        });
    }
}

/// The values the rows `rows` hold (null is in no set), each with its rows.
fn held(rows: &[usize], values: &[Scalar]) -> Groups {
    let mut held = Groups::new();
    for &row in rows {
        if !values[row].is_null() {
            held.add(vec![values[row].clone()], row);
        }
    }
    held
}

impl<'r> Run<'r> {
    /// `set`; `other_rows` are the rows of the relation a `{ relation: R }`
    /// target names (the input's, for a `{ group: }` or `{ values: }` target),
    /// and `kept` says, for each resource the input lists, whether `where:`
    /// may keep a row of it, when groups sit at resources without rows.
    pub(in crate::constraint) fn set(
        &mut self,
        set: &SetAssertion,
        other_rows: &'r RelationRows,
        kept: Option<&[bool]>,
    ) {
        let Some(field_at) = self.position(&set.field) else {
            return;
        };
        let group_at = if set.group_by.is_empty() {
            Vec::new()
        } else {
            let Some(positions) = self.positions(&set.group_by) else {
                return;
            };
            positions
        };
        let Some(values) = self.column(field_at, "assert.set.field") else {
            return;
        };
        let all: Vec<usize> = (0..values.len()).collect();
        self.one_type(&all, field_at, &values, "assert.set.field");
        if !self.failures.is_empty() {
            return;
        }
        let Some((mut groups, group_cols)) = self.set_groups(set, &group_at, kept) else {
            return;
        };
        let Some(other) = self.other_set(
            set,
            field_at,
            &group_at,
            &values,
            &group_cols,
            &mut groups,
            other_rows,
        ) else {
            return;
        };
        let compared = Compared {
            set,
            field_at,
            group_at: &group_at,
            values: &values,
            other: &other,
        };
        let reference = match other.group {
            Some(index) if set.plurals => {
                let Some(keys) = self.key_paths(&groups[index].rows, field_at) else {
                    return;
                };
                Some(keys)
            }
            _ => None,
        };
        for (index, group) in groups.iter().enumerate() {
            if Some(index) == other.group {
                continue;
            }
            if self.finding_cap_breached() {
                if reference.is_none()
                    && compared.set.mode == SetMode::EqualTo
                    && group.rows.is_empty()
                {
                    let count = other.held_for(group).map_or(0, |held| held.order.len());
                    self.suppress_findings(count, false);
                    continue;
                }
                self.suppress_findings(0, true);
                break;
            }
            match &reference {
                Some(reference) => self.plural_group(&compared, group, reference),
                None => self.exact_group(&compared, group),
            }
        }
    }

    /// The groups: each value tuple of the fields at `group_at` with its rows;
    /// each resource the relation reads that `where:` may keep a row of
    /// (`kept`), when every grouping field is a path token; surviving stub
    /// roots (#598); and each literal tuple of `groups:`.
    /// Without `group_by:`, the relation is one group, whose resources are
    /// every one it reads and may keep a row of.
    fn set_groups(
        &mut self,
        set: &SetAssertion,
        group_at: &[usize],
        kept: Option<&[bool]>,
    ) -> Option<(Vec<SetGroup<'r>>, Vec<Vec<Scalar>>)> {
        let rows: &'r RelationRows = self.rows;
        let resources = rows
            .resources
            .iter()
            .enumerate()
            .filter(|(index, _)| kept.is_none_or(|kept| kept[*index]))
            .map(|(_, resource)| resource);
        if group_at.is_empty() {
            let group_resources: BTreeSet<&'r RelPath> = resources.map(Resource::path).collect();
            let first = group_resources.iter().next().copied();
            return Some((
                vec![SetGroup {
                    value: None,
                    rows: (0..rows.rows.len()).collect(),
                    first,
                    resources: group_resources,
                    literal: false,
                }],
                Vec::new(),
            ));
        }
        let (groups, group_cols) = self.groups_by_fields(group_at, "assert.set.group_by")?;
        // Each literal tuple names a group: comparing each component with its
        // column's values (of one type) must not cross types.
        if let SetTarget::Group(reference) = &set.target {
            let option = format!("assert.set.{}.group", set.mode.key());
            if !self.check_group_tuple_types(reference, group_at, &group_cols, &option) {
                return None;
            }
        }
        for (index, tuple) in set.groups.iter().enumerate() {
            let option = format!("assert.set.groups[{index}]");
            if !self.check_group_tuple_types(tuple, group_at, &group_cols, &option) {
                return None;
            }
        }
        let mut out: Vec<SetGroup<'r>> = Vec::with_capacity(groups.order.len());
        let mut index: HashMap<Vec<Scalar>, usize> = HashMap::with_capacity(groups.order.len());
        for (value, members) in groups.iter() {
            let group_resources: BTreeSet<&'r RelPath> = members
                .iter()
                .map(|&row| rows.rows[row].at.resource.path())
                .collect();
            let first = group_resources.iter().next().copied();
            index.insert(value.clone(), out.len());
            out.push(SetGroup {
                value: Some(value.clone()),
                first,
                resources: group_resources,
                rows: members.clone(),
                literal: false,
            });
        }
        // Grouped by path tokens, every resource forms its group, made as its
        // rows' is.
        if !set.resource_groups.is_empty() {
            for resource in resources {
                let Some(tuple) = set
                    .resource_groups
                    .iter()
                    .map(|rg| rg.group_of(resource).map(|s| Scalar::String(s.into())))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                add_group_resource(&mut out, &mut index, tuple, resource.path());
            }
        }
        // Grouped by `entry.root` (alone or alongside path tokens), single-root
        // stub files removed by `strip-root` (`fr:`, `fr: {}`, `fr: []`) still
        // form zero-row groups anchored at the stub file (#598).
        if set.stub_root_groups {
            for stub in &rows.stub_rows {
                let Some(tuple) = group_at
                    .iter()
                    .map(|&pos| Scalar::of(&stub.cells[pos]).ok().filter(|v| !v.is_null()))
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                add_group_resource(&mut out, &mut index, tuple, stub.at.resource.path());
            }
        }
        for tuple in &set.groups {
            let value: Vec<Scalar> = tuple
                .iter()
                .map(|literal| {
                    Scalar::of_value(literal).expect("the builder takes scalar literals")
                })
                .collect();
            if !index.contains_key(&value) {
                index.insert(value.clone(), out.len());
                out.push(SetGroup {
                    value: Some(value),
                    rows: Vec::new(),
                    first: None,
                    resources: BTreeSet::new(),
                    literal: true,
                });
            }
        }
        Some((out, group_cols))
    }

    /// The target set (or per-group map of target sets), or `None` after a
    /// failure.
    #[allow(clippy::too_many_arguments)]
    fn other_set(
        &mut self,
        set: &SetAssertion,
        field_at: usize,
        group_at: &[usize],
        values: &[Scalar],
        group_cols: &[Vec<Scalar>],
        groups: &mut Vec<SetGroup<'r>>,
        other_rows: &'r RelationRows,
    ) -> Option<Other<'r>> {
        match &set.target {
            SetTarget::Group(reference) => {
                self.reference_group(set, reference, field_at, group_at, values, groups)
            }
            SetTarget::Values(literals) => self.values_set(set, literals, field_at, values),
            SetTarget::Relation {
                field,
                group_by,
                all_target_groups,
                written,
                ..
            } => {
                if group_by.is_empty() {
                    self.relation_set(set, field, written, field_at, values, other_rows)
                } else {
                    self.relation_set_grouped(
                        set,
                        field,
                        group_by,
                        *all_target_groups,
                        written,
                        field_at,
                        group_at,
                        values,
                        group_cols,
                        groups,
                        other_rows,
                    )
                }
            }
        }
    }

    /// The reference group `reference` names, which must have rows.
    fn reference_group(
        &mut self,
        set: &SetAssertion,
        reference: &[Value],
        field_at: usize,
        group_at: &[usize],
        values: &[Scalar],
        groups: &[SetGroup<'r>],
    ) -> Option<Other<'r>> {
        let mode = set.mode.key();
        let ref_scalars: Vec<Scalar> = reference
            .iter()
            .map(|lit| Scalar::of_value(lit).expect("the builder takes scalar literals"))
            .collect();
        let shown = group_at
            .iter()
            .zip(&ref_scalars)
            .map(|(&pos, val)| {
                let value_text =
                    if self.redact_all || self.rule.hide_literals || self.sensitive(pos) {
                        "<redacted>".to_string()
                    } else {
                        val.show(false)
                    };
                format!("`{}` {value_text}", self.name(pos))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let Some(index) = groups
            .iter()
            .position(|group| group.value.as_ref() == Some(&ref_scalars) && !group.rows.is_empty())
        else {
            self.fail(
                None,
                format!(
                    "`assert.set.{mode}.group`: the reference group ({shown}) has no rows; a set \
                     is compared with it, so it must have some"
                ),
            );
            return None;
        };
        Some(Other {
            rows: Some(self.rows),
            at: field_at,
            held: OtherHeld::Single(held(&groups[index].rows, values)),
            group: Some(index),
            shown,
            label: "the reference group's row with it".to_string(),
            target_group_sensitive: Vec::new(),
        })
    }

    /// Inline literal target set (`{ values: [...] }`, ADR-0050 D3).
    fn values_set(
        &mut self,
        set: &SetAssertion,
        literals: &[Value],
        field_at: usize,
        values: &[Scalar],
    ) -> Option<Other<'r>> {
        let option = format!("assert.set.{}.values", set.mode.key());
        let mut theirs = Groups::new();
        let mut kinds: BTreeMap<Kind, usize> = BTreeMap::new();
        for (idx, literal) in literals.iter().enumerate() {
            let scalar = Scalar::of_value(literal).expect("the builder takes scalar literals");
            if let Some(kind) = scalar.kind() {
                kinds.entry(kind).or_insert(idx);
            }
            let before = theirs.order.len();
            theirs.add(vec![scalar], idx);
            if theirs.order.len() > before
                && let Err(limit) = self
                    .limits
                    .check(LimitKind::DistinctValues, theirs.order.len())
            {
                self.fail(
                    None,
                    format!(
                        "`{option}`: {limit}, so the assertion has no verdict rather than one over \
                         part of the set"
                    ),
                );
                return None;
            }
        }
        if kinds.len() > 1 {
            let mut indices: Vec<usize> = kinds.values().copied().collect();
            indices.sort_unstable();
            let first = Scalar::of_value(&literals[indices[0]]).expect("scalar literal");
            let second = Scalar::of_value(&literals[indices[1]]).expect("scalar literal");
            self.fail(
                None,
                format!(
                    "`{option}`: item {} is {} and item {} is {}; the comparison would cross \
                     types, so the assertion has no verdict",
                    indices[0],
                    literal_type(&first),
                    indices[1],
                    literal_type(&second),
                ),
            );
            return None;
        }
        if let Some((&kind, &lit_idx)) = kinds.iter().next()
            && let Some(ours_row) = values
                .iter()
                .position(|value| value.kind().is_some_and(|ours| ours != kind))
        {
            let cell = self.cell(ours_row, field_at);
            let lit_scalar = Scalar::of_value(&literals[lit_idx]).expect("scalar literal");
            let message = format!(
                "`{option}`: `{}` holds {} at {} and the listed value is {}; the comparison \
                 would cross types, so the assertion has no verdict",
                self.name(field_at),
                type_of(cell),
                self.located(cell),
                literal_type(&lit_scalar),
            );
            self.fail(Some(cell.at.resource.path().as_path()), message);
            return None;
        }
        Some(Other {
            rows: None,
            at: 0,
            held: OtherHeld::Single(theirs),
            group: None,
            shown: "the listed values".to_string(),
            label: String::new(),
            target_group_sensitive: Vec::new(),
        })
    }

    /// Every value of field `field` of the other relation, `written` as the
    /// rule names it, or `None` after a failure: a value no comparison
    /// takes, values of two types, or of another type than the compared
    /// field's (`field_at`, holding `values`), or more distinct values than
    /// the limit allows.
    fn relation_set(
        &mut self,
        set: &SetAssertion,
        field: &str,
        written: &str,
        field_at: usize,
        values: &[Scalar],
        other_rows: &'r RelationRows,
    ) -> Option<Other<'r>> {
        let option = format!("assert.set.{}.field", set.mode.key());
        let Some(at) = other_rows.field(field) else {
            self.fail(
                None,
                format!(
                    "`{option}`: the rows of `{written}` have no field `{field}`, which the plan \
                     declares; this is a bug in alint, so please report it"
                ),
            );
            return None;
        };
        let limits = self.limits;
        let mut theirs = Groups::new();
        let mut kinds: BTreeMap<Kind, usize> = BTreeMap::new();
        for (row, other_row) in other_rows.rows.iter().enumerate() {
            let cell = &other_row.cells[at];
            let value = match Scalar::of(cell) {
                Ok(value) => value,
                Err(held) => {
                    let message = format!(
                        "`{option}`: `{field}` of relation `{written}` holds {held} at {}; a \
                         constraint compares strings, numbers, booleans and null",
                        self.located(cell)
                    );
                    self.fail(Some(cell.at.resource.path().as_path()), message);
                    return None;
                }
            };
            let Some(kind) = value.kind() else {
                continue;
            };
            kinds.entry(kind).or_insert(row);
            let before = theirs.order.len();
            theirs.add(vec![value], row);
            if theirs.order.len() > before
                && let Err(limit) = limits.check(LimitKind::DistinctValues, theirs.order.len())
            {
                self.fail(
                    None,
                    format!(
                        "`{option}`: reads `{written}.{field}`: {limit}, so the assertion has no \
                         verdict rather than one over part of the set; narrow `{written}`"
                    ),
                );
                return None;
            }
        }
        if !self.check_relation_column_types(
            &option, field, written, field_at, values, other_rows, at, &kinds,
        ) {
            return None;
        }
        Some(Other {
            rows: Some(other_rows),
            at,
            held: OtherHeld::Single(theirs),
            group: None,
            shown: format!("`{field}` of relation `{written}`"),
            label: format!("the row of relation `{written}` with it"),
            target_group_sensitive: Vec::new(),
        })
    }

    /// Per-group cross-relation target sets (`to.group_by:`, ADR-0050 D3).
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn relation_set_grouped(
        &mut self,
        set: &SetAssertion,
        field: &str,
        target_group_by: &[String],
        all_target_groups: bool,
        written: &str,
        field_at: usize,
        group_at: &[usize],
        values: &[Scalar],
        group_cols: &[Vec<Scalar>],
        groups: &mut Vec<SetGroup<'r>>,
        other_rows: &'r RelationRows,
    ) -> Option<Other<'r>> {
        let field_opt = format!("assert.set.{}.field", set.mode.key());
        let group_opt = format!("assert.set.{}.group_by", set.mode.key());
        let Some(at) = other_rows.field(field) else {
            self.fail(
                None,
                format!(
                    "`{field_opt}`: the rows of `{written}` have no field `{field}`, which the \
                     plan declares; this is a bug in alint, so please report it"
                ),
            );
            return None;
        };
        let mut dst_group_at = Vec::with_capacity(target_group_by.len());
        for name in target_group_by {
            let Some(pos) = other_rows.field(name) else {
                self.fail(
                    None,
                    format!(
                        "`{group_opt}`: the rows of `{written}` have no field `{name}`, which \
                         the plan declares; this is a bug in alint, so please report it"
                    ),
                );
                return None;
            };
            dst_group_at.push(pos);
        }
        let limits = self.limits;
        let mut by_group: HashMap<Vec<Scalar>, Groups> = HashMap::new();
        let mut group_order: Vec<Vec<Scalar>> = Vec::new();
        let mut field_kinds: BTreeMap<Kind, usize> = BTreeMap::new();
        let mut group_kinds: Vec<BTreeMap<Kind, usize>> = vec![BTreeMap::new(); dst_group_at.len()];
        let mut total_distinct = 0usize;
        for (row, other_row) in other_rows.rows.iter().enumerate() {
            let mut gkey = Vec::with_capacity(dst_group_at.len());
            for (j, &gpos) in dst_group_at.iter().enumerate() {
                let gcell = &other_row.cells[gpos];
                let gval = match Scalar::of(gcell) {
                    Ok(v) => v,
                    Err(held) => {
                        let message = format!(
                            "`{group_opt}`: `{}` of relation `{written}` holds {held} at {}; a \
                             constraint compares strings, numbers, booleans and null",
                            target_group_by[j],
                            self.located(gcell)
                        );
                        self.fail(Some(gcell.at.resource.path().as_path()), message);
                        return None;
                    }
                };
                if gval.is_null() {
                    let message = format!(
                        "`{group_opt}`: `{}` of relation `{written}` has no value at {}; every \
                         row's group must have one",
                        target_group_by[j],
                        self.located(gcell)
                    );
                    self.fail(Some(gcell.at.resource.path().as_path()), message);
                    return None;
                }
                if let Some(kind) = gval.kind() {
                    group_kinds[j].entry(kind).or_insert(row);
                }
                gkey.push(gval);
            }
            let fcell = &other_row.cells[at];
            let fval = match Scalar::of(fcell) {
                Ok(v) => v,
                Err(held) => {
                    let message = format!(
                        "`{field_opt}`: `{field}` of relation `{written}` holds {held} at {}; a \
                         constraint compares strings, numbers, booleans and null",
                        self.located(fcell)
                    );
                    self.fail(Some(fcell.at.resource.path().as_path()), message);
                    return None;
                }
            };
            if !by_group.contains_key(&gkey) {
                group_order.push(gkey.clone());
                by_group.insert(gkey.clone(), Groups::new());
            }
            if let Some(kind) = fval.kind() {
                field_kinds.entry(kind).or_insert(row);
                let target_group = by_group.get_mut(&gkey).expect("inserted above");
                let before = target_group.order.len();
                target_group.add(vec![fval], row);
                if target_group.order.len() > before {
                    total_distinct += 1;
                    if let Err(limit) = limits.check(LimitKind::DistinctValues, total_distinct) {
                        self.fail(
                            None,
                            format!(
                                "`{field_opt}`: reads `{written}.{field}`: {limit}, so the \
                                 assertion has no verdict rather than one over part of the set; \
                                 narrow `{written}`"
                            ),
                        );
                        return None;
                    }
                }
            }
        }
        for (j, kinds) in group_kinds.iter().enumerate() {
            if !self.check_relation_column_types(
                &group_opt,
                &target_group_by[j],
                written,
                group_at[j],
                &group_cols[j],
                other_rows,
                dst_group_at[j],
                kinds,
            ) {
                return None;
            }
        }
        if !self.check_relation_column_types(
            &field_opt,
            field,
            written,
            field_at,
            values,
            other_rows,
            at,
            &field_kinds,
        ) {
            return None;
        }
        if all_target_groups {
            let existing: BTreeSet<Vec<Scalar>> = groups
                .iter()
                .filter_map(|group| group.value.clone())
                .collect();
            for gkey in group_order {
                if !existing.contains(&gkey) {
                    groups.push(SetGroup {
                        value: Some(gkey),
                        rows: Vec::new(),
                        first: None,
                        resources: BTreeSet::new(),
                        literal: false,
                    });
                }
            }
        }
        let target_group_sensitive = dst_group_at
            .iter()
            .map(|&pos| other_rows.fields[pos].sensitive)
            .collect();
        Some(Other {
            rows: Some(other_rows),
            at,
            held: OtherHeld::ByGroup(by_group),
            group: None,
            shown: format!("`{field}` of relation `{written}`"),
            label: format!("the row of relation `{written}` with it"),
            target_group_sensitive,
        })
    }

    /// Check that a target relation column holds one scalar type and matches
    /// the corresponding input column's type.
    #[allow(clippy::too_many_arguments)]
    fn check_relation_column_types(
        &mut self,
        option: &str,
        target_field: &str,
        written: &str,
        input_pos: usize,
        input_values: &[Scalar],
        other_rows: &RelationRows,
        other_pos: usize,
        kinds: &BTreeMap<Kind, usize>,
    ) -> bool {
        let clash = match (kinds.len(), kinds.iter().next()) {
            (2.., _) => {
                let mut rows: Vec<usize> = kinds.values().copied().collect();
                rows.sort_unstable();
                Some(((rows[0], false), (rows[1], false)))
            }
            (1, Some((&kind, &row))) => input_values
                .iter()
                .position(|value| value.kind().is_some_and(|ours| ours != kind))
                .map(|ours| ((ours, true), (row, false))),
            _ => None,
        };
        if let Some((first, second)) = clash {
            let describe = |run: &Self, (row, input): (usize, bool)| {
                let (cell, name): (&Cell, String) = if input {
                    (
                        run.cell(row, input_pos),
                        format!("`{}`", run.name(input_pos)),
                    )
                } else {
                    (
                        &other_rows.rows[row].cells[other_pos],
                        format!("`{target_field}` of relation `{written}`"),
                    )
                };
                (
                    format!("{name} holds {} at {}", type_of(cell), run.located(cell)),
                    cell.at.resource.path().as_path(),
                )
            };
            let ((one, path), (two, _)) = (describe(self, first), describe(self, second));
            let message = format!(
                "`{option}`: {one} and {two}; the comparison would cross types, so the \
                 assertion has no verdict"
            );
            self.fail(Some(path), message);
            return false;
        }
        true
    }

    /// How a message names group `group`.
    fn subject(&self, compared: &Compared<'_, '_>, group: &SetGroup<'_>) -> String {
        if let Some(values) = &group.value
            && !compared.group_at.is_empty()
        {
            return compared
                .group_at
                .iter()
                .enumerate()
                .zip(values)
                .map(|((idx, &group_at), value)| {
                    format!(
                        "`{}` {}",
                        self.name(group_at),
                        value.show(self.group_hidden(compared, group, idx, group_at))
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
        }
        let named = self.rule.plan.relation(self.rule.inputs[0]);
        if named.is_inline() {
            "the inline input".to_string()
        } else {
            format!("relation `{}`", named.name)
        }
    }

    /// Whether group `group`'s `idx`-th value stays out of messages and keys:
    /// the field is sensitive, or the value is a literal the environment wrote.
    fn group_hidden(
        &self,
        compared: &Compared<'_, '_>,
        group: &SetGroup<'_>,
        idx: usize,
        group_at: usize,
    ) -> bool {
        self.redact_all
            || self.sensitive(group_at)
            || compared
                .other
                .target_group_sensitive
                .get(idx)
                .copied()
                .unwrap_or(false)
            || (group.literal && self.rule.hide_literals)
    }

    /// Whether a compared value stays out of generated messages and baseline
    /// keys (ADR-0050 D9 / D3).
    fn value_hidden(&self, compared: &Compared<'_, '_>) -> bool {
        self.redact_all
            || self.sensitive(compared.field_at)
            || compared
                .other
                .rows
                .is_some_and(|rows| rows.fields[compared.other.at].sensitive)
            || (compared.other.rows.is_none() && self.rule.hide_literals)
    }

    /// Whether `{{value}}` in a custom `message:` stays redacted (ADR-0050 D9):
    /// also redacts whenever `assert:` holds environment-derived literals.
    fn placeholder_value_hidden(&self, compared: &Compared<'_, '_>) -> bool {
        self.value_hidden(compared) || self.rule.hide_literals
    }

    /// Precompute a per-group `O(1)` structural counterpart index when
    /// comparing against a same-relation `{ group: ... }` target (#595) on a
    /// single path-token `group_by:`.
    fn counterpart_index(
        compared: &Compared<'_, 'r>,
        group: &SetGroup<'r>,
    ) -> Option<CounterpartIndex<'r>> {
        compared.other.group?;
        match compared.set.resource_groups.as_slice() {
            [single] => Some(single.counterpart_index(&group.resources)),
            _ => None,
        }
    }

    /// A finding's key: `set`, the group tuple (or `all`), the value and `what`.
    fn set_key(
        &self,
        compared: &Compared<'_, '_>,
        group: &SetGroup<'_>,
        value: &Scalar,
        what: &str,
    ) -> Key {
        let mut key = Key::new("set");
        match &group.value {
            Some(group_values) if !compared.group_at.is_empty() => {
                for ((idx, &group_at), group_value) in
                    compared.group_at.iter().enumerate().zip(group_values)
                {
                    key.value(
                        group_value,
                        self.group_hidden(compared, group, idx, group_at),
                    );
                }
            }
            _ => {
                key.part("all");
            }
        }
        key.value(value, self.value_hidden(compared)).part(what);
        key
    }

    /// Value `value` of the other set, held by its row `theirs`, which group
    /// `group` lacks: at the structural counterpart in `counterparts` when
    /// unique (same-relation `{ group: }` targets only, #595), else the
    /// group's first resource, related to `theirs` when backed by rows.
    fn missing(
        &mut self,
        compared: &Compared<'_, 'r>,
        group: &SetGroup<'r>,
        counterparts: Option<&CounterpartIndex<'r>>,
        value: &Scalar,
        theirs: usize,
    ) {
        let other = compared.other;
        let hide_value = self.value_hidden(compared);
        let message = format!(
            "{} lacks `{}` {}, which {} holds",
            self.subject(compared, group),
            compared.set.field,
            value.show(hide_value),
            other.shown
        );
        let (target_path, related, reported) = if let Some(other_rows) = other.rows {
            let ref_path = other_rows.rows[theirs].at.resource.path();
            let target_path = counterparts
                .and_then(|index| index.lookup(ref_path))
                .or(group.first);
            let hide_other = self.hide_keys || hide_value;
            let holder = spot_in(other_rows, theirs, other.at, hide_other);
            let related = self.related_at(vec![(holder, other.label.as_str())], 0);
            let reported = other.group.is_some().then_some(theirs);
            (target_path, related, reported)
        } else {
            (group.first, Related::empty(), None)
        };
        let at = target_path.map_or(Place::Nowhere, |path| Place::Resource(path.as_path()));
        let key = self.set_key(compared, group, value, "missing");
        self.report(
            at,
            message,
            key,
            related,
            reported,
            Some((value, self.placeholder_value_hidden(compared))),
        );
    }

    /// Value `value`, held by group `group`'s rows `holders`, which the
    /// other set lacks: at its first row, related to the others.
    fn extra(
        &mut self,
        compared: &Compared<'_, 'r>,
        group: &SetGroup<'_>,
        value: &Scalar,
        row: usize,
        others: &[usize],
    ) {
        let field_at = compared.field_at;
        let places: Vec<_> = others
            .iter()
            .take(MAX_RELATED_LOCATIONS)
            .map(|&row| (self.spot(row, field_at), "another row of the group with it"))
            .collect();
        let omitted = others.len() - places.len();
        let hide_value = self.value_hidden(compared);
        let message = format!(
            "{} holds `{}` {}, which {} lacks",
            self.subject(compared, group),
            compared.set.field,
            value.show(hide_value),
            compared.other.shown
        );
        let related = self.related_at(places, omitted);
        let key = self.set_key(compared, group, value, "extra");
        let at = self.spot(row, field_at);
        self.report(
            at,
            message,
            key,
            related,
            Some(row),
            Some((value, self.placeholder_value_hidden(compared))),
        );
    }

    /// Value `value`, held by group `group`'s row `row` and the other set's
    /// row `theirs`: at `row`, related to `theirs` when backed by rows.
    fn shared(
        &mut self,
        compared: &Compared<'_, 'r>,
        group: &SetGroup<'_>,
        value: &Scalar,
        row: usize,
        theirs: usize,
    ) {
        let other = compared.other;
        let hide_value = self.value_hidden(compared);
        let message = format!(
            "{} and {} both hold `{}` {}",
            self.subject(compared, group),
            other.shown,
            compared.set.field,
            value.show(hide_value)
        );
        let related = if let Some(other_rows) = other.rows {
            let hide_other = self.hide_keys || hide_value;
            let holder = spot_in(other_rows, theirs, other.at, hide_other);
            self.related_at(vec![(holder, other.label.as_str())], 0)
        } else {
            Related::empty()
        };
        let key = self.set_key(compared, group, value, "shared");
        let at = self.spot(row, compared.field_at);
        self.report(
            at,
            message,
            key,
            related,
            Some(row),
            Some((value, self.placeholder_value_hidden(compared))),
        );
    }

    /// Group `group` against the other set, values compared as they are.
    fn exact_group(&mut self, compared: &Compared<'_, 'r>, group: &SetGroup<'r>) {
        let ours = held(&group.rows, compared.values);
        let other_held = compared.other.held_for(group);
        let mode = compared.set.mode;
        if mode == SetMode::EqualTo
            && let Some(other_held) = other_held
        {
            let counterparts = Self::counterpart_index(compared, group);
            for (idx, (value, holders)) in other_held.iter().enumerate() {
                if !ours.rows.contains_key(value) {
                    if self.at_finding_cap() {
                        if ours.rows.is_empty() {
                            self.suppress_findings(other_held.order.len() - idx, false);
                        } else {
                            self.suppress_findings(1, true);
                        }
                        return;
                    }
                    self.missing(
                        compared,
                        group,
                        counterparts.as_ref(),
                        &value[0],
                        holders[0],
                    );
                }
            }
        }
        for (value, holders) in ours.iter() {
            let theirs = other_held.and_then(|held| held.rows.get(value));
            match (mode, theirs) {
                (SetMode::EqualTo | SetMode::SubsetOf, None) => {
                    if self.at_finding_cap() {
                        self.suppress_findings(1, true);
                        return;
                    }
                    self.extra(compared, group, &value[0], holders[0], &holders[1..]);
                }
                (SetMode::DisjointFrom, Some(theirs)) => {
                    if self.at_finding_cap() {
                        self.suppress_findings(1, true);
                        return;
                    }
                    self.shared(compared, group, &value[0], holders[0], theirs[0]);
                }
                _ => {}
            }
        }
    }

    /// The key path of each row of `rows` that holds a value (an
    /// `entry.path` field's, which the builder checked), with the first row
    /// that holds it.
    fn key_paths(&mut self, rows: &[usize], field_at: usize) -> Option<Keys<'r, usize>> {
        let mut keys = Keys::new();
        for &row in rows {
            let cell: &'r Cell = self.cell(row, field_at);
            if matches!(cell.value, None | Some(Value::Null)) {
                continue;
            }
            let Some(path) = cell.path.as_deref() else {
                let message = format!(
                    "`assert.set.plurals`: `{}` holds a value without a key path at {}; this is \
                     a bug in alint's plan check, so please report it",
                    self.name(field_at),
                    self.located(cell)
                );
                self.fail(Some(cell.at.resource.path().as_path()), message);
                return None;
            };
            keys.entry(Cow::Borrowed(path.segments())).or_insert(row);
        }
        Some(keys)
    }

    /// Group `group` against the reference group's key paths `reference`,
    /// CLDR plural groups merged over the two. A merged value is its
    /// group's key path, at its `other` member's row, or its first member's.
    fn plural_group(
        &mut self,
        compared: &Compared<'_, 'r>,
        group: &SetGroup<'r>,
        reference: &Keys<'r, usize>,
    ) {
        let Some(peer) = self.key_paths(&group.rows, compared.field_at) else {
            return;
        };
        let ours = held(&group.rows, compared.values);
        let mut probe = Probe::new(reference, &peer);
        let merged = probe.merged(Side::Peer);
        let text = |key: &[alint_core::relation::Segment]| {
            Scalar::String(Pointer::render_segments(key).into())
        };
        let mode = compared.set.mode;
        if mode == SetMode::EqualTo {
            let counterparts = Self::counterpart_index(compared, group);
            let mut missing: Vec<(usize, Scalar)> = probe
                .merged(Side::Reference)
                .into_iter()
                .filter(|(key, _)| !merged.contains_key(key))
                .map(|(key, theirs)| (theirs, text(&key)))
                .collect();
            missing.sort_unstable_by_key(|(theirs, _)| *theirs);
            let total_missing = missing.len();
            for (idx, (theirs, value)) in missing.into_iter().enumerate() {
                if self.at_finding_cap() {
                    self.suppress_findings(total_missing - idx, !merged.is_empty());
                    return;
                }
                self.missing(compared, group, counterparts.as_ref(), &value, theirs);
            }
        }
        let mut found: Vec<(usize, Scalar, Option<usize>)> = Vec::new();
        for (key, &row) in &merged {
            match (mode, probe.find(Side::Reference, key)) {
                (SetMode::EqualTo | SetMode::SubsetOf, None) => found.push((row, text(key), None)),
                (SetMode::DisjointFrom, Some(theirs)) => {
                    found.push((row, text(key), Some(theirs)));
                }
                _ => {}
            }
        }
        found.sort_unstable_by_key(|(row, ..)| *row);
        let total_found = found.len();
        for (idx, (row, value, theirs)) in found.into_iter().enumerate() {
            if self.at_finding_cap() {
                self.suppress_findings(total_found - idx, false);
                return;
            }
            if let Some(theirs) = theirs {
                self.shared(compared, group, &value, row, theirs);
            } else {
                // The group's other rows with the value `row` holds.
                let others: Vec<usize> = ours.rows[&vec![compared.values[row].clone()]]
                    .iter()
                    .copied()
                    .filter(|&other| other != row)
                    .collect();
                self.extra(compared, group, &value, row, &others);
            }
        }
    }
}
