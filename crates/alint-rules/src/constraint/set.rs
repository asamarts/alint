//! `set` (design `v0.23/selectors-and-constraints.md` section 6): the set of
//! `field:`'s values in each group, against another set: a reference group
//! of the same relation (`{ group: literal }`), or every value of another
//! relation's field (`{ relation: R, field: F }`, a semi-join: section 8).
//!
//! - **Groups:** every value of `group_by:` (or the relation, as one group),
//!   every resource the relation reads when `group_by:` is `path` or a path
//!   token, and every literal of `groups:`. A group with no rows lacks every
//!   value of the other set: at its first resource, or without a path for a
//!   group only `groups:` names.
//! - **Findings:** a missing value sits at the group's first resource,
//!   related to the row of the other set that holds it; an extra value sits
//!   at the row that holds it; a shared value sits at the row that holds it,
//!   related to a row of the other set that holds it.
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

use super::{Groups, Place, Run, literal_type, spot_in, type_of};
use crate::constraint::values::{Key, Kind, Scalar};
use crate::constraint::{CounterpartIndex, SetAssertion, SetMode, SetTarget};
use crate::plural_groups::{Keys, Probe, Side};

/// One group of a `set`.
struct SetGroup<'r> {
    /// Its `group_by:` value; none when the relation is one group.
    value: Option<Scalar>,
    rows: Vec<usize>,
    /// Its first resource in canonical order, where a value it lacks sits
    /// when no structural counterpart matches: none for a group only
    /// `groups:` names.
    first: Option<&'r RelPath>,
    /// Every resource belonging to this group in canonical order, used by
    /// target-file placement (#595).
    resources: BTreeSet<&'r RelPath>,
    /// Only a literal of `groups:` names it, so its value is shown as a
    /// literal is.
    literal: bool,
}

/// The set each group is compared with.
struct Other<'r> {
    /// The rows that hold its values: the input's, for a reference group, or
    /// the other relation's.
    rows: &'r RelationRows,
    /// The field of `rows` that holds them.
    at: usize,
    /// Each value and the rows that hold it, in the order first held.
    held: Groups,
    /// The reference group's place among the groups; none for another
    /// relation's field.
    group: Option<usize>,
    /// How a message names it: `` `locale` "en" ``, or `` `name` of relation
    /// `names` ``.
    shown: String,
    /// The label of a related location at a row of it.
    label: String,
}

/// What one comparison of a group reads.
struct Compared<'c, 'r> {
    set: &'c SetAssertion,
    field_at: usize,
    group_at: Option<usize>,
    /// Each row's value of the compared field.
    values: &'c [Scalar],
    other: &'c Other<'r>,
}

/// Attach `path` to group `value`, seeding a zero-row group if none exists yet.
fn add_group_resource<'r>(
    out: &mut Vec<SetGroup<'r>>,
    index: &mut HashMap<Scalar, usize>,
    value: Scalar,
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
    /// target names (the input's, for a `{ group: }` target), and `kept`
    /// says, for each resource the input lists, whether `where:` may keep a
    /// row of it, when groups sit at resources without rows.
    pub(in crate::constraint) fn set(
        &mut self,
        set: &SetAssertion,
        other_rows: &'r RelationRows,
        kept: Option<&[bool]>,
    ) {
        let Some(field_at) = self.position(&set.field) else {
            return;
        };
        let group_at = match &set.group_by {
            Some(name) => match self.position(name) {
                Some(at) => Some(at),
                None => return,
            },
            None => None,
        };
        let Some(values) = self.column(field_at, "assert.set.field") else {
            return;
        };
        let all: Vec<usize> = (0..values.len()).collect();
        self.one_type(&all, field_at, &values, "assert.set.field");
        if !self.failures.is_empty() {
            return;
        }
        let Some(groups) = self.set_groups(set, group_at, kept) else {
            return;
        };
        let Some(other) = self.other_set(set, field_at, &values, &groups, other_rows) else {
            return;
        };
        let compared = Compared {
            set,
            field_at,
            group_at,
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
                    self.suppress_findings(other.held.order.len(), false);
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

    /// The groups: each value of the field at `group_at` with its rows; each
    /// resource the relation reads that `where:` may keep a row of (`kept`),
    /// when the field is a path token; and each literal of `groups:`.
    /// Without `group_by:`, the relation is one group, whose resources are
    /// every one it reads and may keep a row of.
    fn set_groups(
        &mut self,
        set: &SetAssertion,
        group_at: Option<usize>,
        kept: Option<&[bool]>,
    ) -> Option<Vec<SetGroup<'r>>> {
        let rows: &'r RelationRows = self.rows;
        let resources = rows
            .resources
            .iter()
            .enumerate()
            .filter(|(index, _)| kept.is_none_or(|kept| kept[*index]))
            .map(|(_, resource)| resource);
        let Some(group_at) = group_at else {
            let group_resources: BTreeSet<&'r RelPath> = resources.map(Resource::path).collect();
            let first = group_resources.iter().next().copied();
            return Some(vec![SetGroup {
                value: None,
                rows: (0..rows.rows.len()).collect(),
                first,
                resources: group_resources,
                literal: false,
            }]);
        };
        let (groups, group_values) = self.groups_by(group_at, "assert.set.group_by")?;
        // Each literal names a group: comparing it with the groups' values
        // (of one type, so the first row's) must not cross types.
        let mut literals: Vec<(&Value, String)> = Vec::new();
        if let SetTarget::Group(reference) = &set.target {
            literals.push((reference, format!("assert.set.{}.group", set.mode.key())));
        }
        for (index, literal) in set.groups.iter().enumerate() {
            literals.push((literal, format!("assert.set.groups[{index}]")));
        }
        for (literal, option) in &literals {
            let literal = Scalar::of_value(literal).expect("the builder takes scalar literals");
            if group_values
                .first()
                .is_some_and(|value| value.kind() != literal.kind())
            {
                let cell = self.cell(0, group_at);
                let message = format!(
                    "`{option}`: the literal is {} and `{}` holds {} at {}; the comparison \
                     would cross types, so the assertion has no verdict",
                    literal_type(&literal),
                    self.name(group_at),
                    type_of(cell),
                    self.located(cell)
                );
                self.fail(Some(cell.at.resource.path().as_path()), message);
                return None;
            }
        }
        let mut out: Vec<SetGroup<'r>> = Vec::with_capacity(groups.order.len());
        let mut index: HashMap<Scalar, usize> = HashMap::with_capacity(groups.order.len());
        for (value, members) in groups.iter() {
            let group_resources: BTreeSet<&'r RelPath> = members
                .iter()
                .map(|&row| rows.rows[row].at.resource.path())
                .collect();
            let first = group_resources.iter().next().copied();
            index.insert(value[0].clone(), out.len());
            out.push(SetGroup {
                value: Some(value[0].clone()),
                first,
                resources: group_resources,
                rows: members.clone(),
                literal: false,
            });
        }
        // Grouped by a path token, every resource forms its group, made as
        // its rows' is.
        if let Some(by_resource) = &set.resource_groups {
            for resource in resources {
                let value = Scalar::String(by_resource.group_of(resource).into());
                add_group_resource(&mut out, &mut index, value, resource.path());
            }
        }
        // Grouped by `entry.root`, single-root stub files removed by
        // `strip-root` (`fr:`, `fr: {}`, `fr: []`) still form zero-row groups
        // anchored at the stub file (#598).
        if set.stub_root_groups {
            for stub in &rows.stub_rows {
                if let Ok(value) = Scalar::of(&stub.cells[group_at])
                    && !value.is_null()
                {
                    add_group_resource(&mut out, &mut index, value, stub.at.resource.path());
                }
            }
        }
        for literal in &set.groups {
            let value = Scalar::of_value(literal).expect("the builder takes scalar literals");
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
        Some(out)
    }

    /// The set each group is compared with, or `None` after a failure.
    fn other_set(
        &mut self,
        set: &SetAssertion,
        field_at: usize,
        values: &[Scalar],
        groups: &[SetGroup<'r>],
        other_rows: &'r RelationRows,
    ) -> Option<Other<'r>> {
        match &set.target {
            SetTarget::Group(reference) => {
                self.reference_group(set, reference, field_at, values, groups)
            }
            SetTarget::Relation { field, written, .. } => {
                self.relation_set(set, field, written, field_at, values, other_rows)
            }
        }
    }

    /// The reference group `reference` names, which must have rows.
    fn reference_group(
        &mut self,
        set: &SetAssertion,
        reference: &Value,
        field_at: usize,
        values: &[Scalar],
        groups: &[SetGroup<'r>],
    ) -> Option<Other<'r>> {
        let mode = set.mode.key();
        let group_by = set
            .group_by
            .as_deref()
            .expect("a group target has `group_by:`");
        let group_at = self.rows.field(group_by)?;
        let reference = Scalar::of_value(reference).expect("the builder takes scalar literals");
        let shown = if self.redact_all || self.rule.hide_literals || self.sensitive(group_at) {
            "<redacted>".to_string()
        } else {
            reference.show(false)
        };
        let Some(index) = groups
            .iter()
            .position(|group| group.value.as_ref() == Some(&reference) && !group.rows.is_empty())
        else {
            self.fail(
                None,
                format!(
                    "`assert.set.{mode}.group`: the reference group (`{group_by}` {shown}) has no \
                     rows; a set is compared with it, so it must have some"
                ),
            );
            return None;
        };
        Some(Other {
            rows: self.rows,
            at: field_at,
            held: held(&groups[index].rows, values),
            group: Some(index),
            shown: format!("`{group_by}` {shown}"),
            label: "the reference group's row with it".to_string(),
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
        // The first row of each type, in row order.
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
        // The other set's values are of one type, and the compared field's
        // (of one type already) are of that type too.
        let clash = match (kinds.len(), kinds.iter().next()) {
            (2.., _) => {
                let mut rows: Vec<usize> = kinds.values().copied().collect();
                rows.sort_unstable();
                Some(((rows[0], false), (rows[1], false)))
            }
            (1, Some((&kind, &row))) => values
                .iter()
                .position(|value| value.kind().is_some_and(|ours| ours != kind))
                .map(|ours| ((ours, true), (row, false))),
            _ => None,
        };
        if let Some((first, second)) = clash {
            // A row of the input (`true`) or of the other relation.
            let describe = |run: &Self, (row, input): (usize, bool)| {
                let (cell, name): (&Cell, String) = if input {
                    (run.cell(row, field_at), format!("`{}`", run.name(field_at)))
                } else {
                    (
                        &other_rows.rows[row].cells[at],
                        format!("`{field}` of relation `{written}`"),
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
            return None;
        }
        // A sensitive field's values show in the compared field: the loader
        // made it sensitive too, with every field that spells the same values.
        Some(Other {
            rows: other_rows,
            at,
            held: theirs,
            group: None,
            shown: format!("`{field}` of relation `{written}`"),
            label: format!("the row of relation `{written}` with it"),
        })
    }

    /// How a message names group `group`.
    fn subject(&self, compared: &Compared<'_, '_>, group: &SetGroup<'_>) -> String {
        if let (Some(value), Some(group_at)) = (&group.value, compared.group_at) {
            return format!(
                "`{}` {}",
                self.name(group_at),
                value.show(self.group_hidden(group, group_at))
            );
        }
        let named = self.rule.plan.relation(self.rule.inputs[0]);
        if named.is_inline() {
            "the inline input".to_string()
        } else {
            format!("relation `{}`", named.name)
        }
    }

    /// Whether group `group`'s value stays out of messages and keys: the
    /// field is sensitive, or the value is a literal the environment wrote.
    /// The second is defence in depth: a literal the environment wrote in
    /// `assert:` redacts every message of the rule already
    /// ([`ConstraintRule::hide_literals`](crate::constraint::ConstraintRule)).
    fn group_hidden(&self, group: &SetGroup<'_>, group_at: usize) -> bool {
        self.redact_all || self.sensitive(group_at) || (group.literal && self.rule.hide_literals)
    }

    /// Whether a compared value stays out of generated messages and baseline
    /// keys (ADR-0050 D9).
    fn value_hidden(&self, compared: &Compared<'_, '_>) -> bool {
        self.redact_all
            || self.sensitive(compared.field_at)
            || compared.other.rows.fields[compared.other.at].sensitive
    }

    /// Whether `{{value}}` in a custom `message:` stays redacted (ADR-0050 D9):
    /// also redacts whenever `assert:` holds environment-derived literals.
    fn placeholder_value_hidden(&self, compared: &Compared<'_, '_>) -> bool {
        self.value_hidden(compared) || self.rule.hide_literals
    }

    /// Precompute a per-group `O(1)` structural counterpart index when
    /// comparing against a same-relation `{ group: ... }` target (#595).
    /// Cross-relation `{ relation: R, field: F }` targets fall back to
    /// `group.first` (ADR-0050 D9).
    fn counterpart_index(
        compared: &Compared<'_, 'r>,
        group: &SetGroup<'r>,
    ) -> Option<CounterpartIndex<'r>> {
        compared.other.group?;
        compared
            .set
            .resource_groups
            .as_ref()
            .map(|by_resource| by_resource.counterpart_index(&group.resources))
    }

    /// A finding's key: `set`, the group, the value and `what`.
    fn set_key(
        &self,
        compared: &Compared<'_, '_>,
        group: &SetGroup<'_>,
        value: &Scalar,
        what: &str,
    ) -> Key {
        let mut key = Key::new("set");
        match (&group.value, compared.group_at) {
            (Some(group_value), Some(group_at)) => {
                key.value(group_value, self.group_hidden(group, group_at));
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
    /// group's first resource, related to `theirs`.
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
        let ref_path = other.rows.rows[theirs].at.resource.path();
        let target_path = counterparts
            .and_then(|index| index.lookup(ref_path))
            .or(group.first);
        let at = target_path.map_or(Place::Nowhere, |path| Place::Resource(path.as_path()));
        let hide_other = self.hide_keys || hide_value;
        let holder = spot_in(other.rows, theirs, other.at, hide_other);
        let related = self.related_at(vec![(holder, other.label.as_str())], 0);
        let key = self.set_key(compared, group, value, "missing");
        // A reference group's row is a row of the input, which `message:`
        // renders; another relation's is not.
        let reported = other.group.is_some().then_some(theirs);
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
    /// row `theirs`: at `row`, related to `theirs`.
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
        let hide_other = self.hide_keys || hide_value;
        let holder = spot_in(other.rows, theirs, other.at, hide_other);
        let related = self.related_at(vec![(holder, other.label.as_str())], 0);
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
        let other = compared.other;
        let mode = compared.set.mode;
        if mode == SetMode::EqualTo {
            let counterparts = Self::counterpart_index(compared, group);
            for (idx, (value, holders)) in other.held.iter().enumerate() {
                if !ours.rows.contains_key(value) {
                    if self.at_finding_cap() {
                        if ours.rows.is_empty() {
                            self.suppress_findings(other.held.order.len() - idx, false);
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
            match (mode, other.held.rows.get(value)) {
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
