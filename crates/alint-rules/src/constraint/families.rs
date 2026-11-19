//! The `constraint` families over a relation's rows (design section 6):
//! `unique`, `equal` in its three forms, and `set` (in `set.rs`). Each
//! finding sits at a row's field, or at a resource, with
//! related locations at the rows it is compared with; an analysis failure
//! (a null group, a comparison across types, a missing source row) leaves
//! the rule without a verdict, so it reports the failures and no finding.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;
use std::sync::Arc;

use alint_core::relation::Value;
use alint_core::relation::eval::{Cell, FieldRow, Provenance, RelationRows, located, type_name};
use alint_core::relation::plan::{
    FieldSource, JoinSide, Part, Plan, Read, Reader, RelationId, Rows,
};
use alint_core::{Context, MAX_RELATED_LOCATIONS, RelatedLocation, Violation};

use super::ConstraintRule;
use super::places::{Asked, Placed, Places};
use super::values::{Key, Kind, Scalar};

#[path = "set.rs"]
mod set;

/// The most other values a generated message lists.
const LISTED_VALUES: usize = 5;

/// One evaluation of a rule's assertion over its relation's rows.
pub(super) struct Run<'r> {
    rule: &'r ConstraintRule,
    rows: &'r RelationRows,
    places: Places<'r>,
    /// Key paths stay out of messages and baselines: a field that holds keys
    /// is sensitive, or the relation's configuration reads the environment
    /// (whose findings the engine redacts whole as well, but whose keys and
    /// places this keeps clean before it does).
    hide_keys: bool,
    findings: Vec<Pending>,
    failures: Vec<Violation>,
}

/// A finding whose places wait for [`Places::resolve`]: its own, and each
/// related location's.
struct Pending {
    finding: Violation,
    at: Option<Asked>,
    related: Related,
}

impl Pending {
    /// The finding, at its places. Each related row is listed or counted:
    /// rows at one place (a resource's rows without a span) collapse into
    /// one location, and the others count as left out, so the locations
    /// listed and the count add up to the rows the message relates to.
    fn placed(self, placed: &Placed) -> Violation {
        let mut finding = self.finding;
        if let Some(asked) = self.at
            && let Some(range) = placed.range(asked)
        {
            finding = if asked.point {
                finding.with_location(range.start_line(), range.start_column())
            } else {
                finding.with_range(range)
            };
        }
        let rows = self.related.locations.len();
        let locations = self.related.locations.into_iter().map(|(location, asked)| {
            match asked.and_then(|asked| placed.range(asked)) {
                Some(range) => location.with_range(range),
                None => location,
            }
        });
        finding = finding.with_related_locations(locations);
        let kept = finding.related_locations.len() + finding.related_locations_omitted;
        finding.related_locations_omitted += self.related.omitted + (rows - kept);
        finding
    }
}

/// Rows that share a value, in the order the first of them appears.
struct Groups {
    order: Vec<Vec<Scalar>>,
    rows: HashMap<Vec<Scalar>, Vec<usize>>,
}

impl Groups {
    fn new() -> Self {
        Self {
            order: Vec::new(),
            rows: HashMap::new(),
        }
    }

    fn add(&mut self, value: Vec<Scalar>, row: usize) {
        if let Some(rows) = self.rows.get_mut(&value) {
            rows.push(row);
        } else {
            self.order.push(value.clone());
            self.rows.insert(value, vec![row]);
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&Vec<Scalar>, &Vec<usize>)> {
        self.order.iter().map(|value| (value, &self.rows[value]))
    }
}

impl<'r> Run<'r> {
    pub(super) fn new(
        rule: &'r ConstraintRule,
        rows: &'r RelationRows,
        ctx: &'r Context<'r>,
    ) -> Self {
        // A key path of any relation the rule reads may reach a message.
        let hide_keys = rule.inputs.iter().any(|&id| {
            rule.plan.relation(id).reads_environment
                || alint_core::relation::eval::keys_sensitive(&rule.plan, id)
        });
        Self {
            rule,
            rows,
            places: Places::new(ctx),
            hide_keys,
            findings: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// `earlier` (findings about the rows, such as values a `read:` skipped)
    /// and the assertion's findings, at their places, or, when the assertion
    /// has no verdict, its failures alone. Placing them reads the resources
    /// they sit in, one at a time ([`Places::resolve`]): when the run's bytes
    /// cannot hold one, the rule has no verdict either.
    pub(super) fn finish(self, mut earlier: Vec<Violation>) -> Vec<Violation> {
        let Self {
            rule,
            places,
            findings,
            mut failures,
            ..
        } = self;
        if failures.is_empty() && !findings.is_empty() {
            match places.resolve() {
                Ok(placed) => earlier.extend(findings.into_iter().map(|f| f.placed(&placed))),
                Err((path, limit)) => failures.push(rule.failure(
                    Some(path.as_path()),
                    format!(
                        "{limit}, so the rule reports no finding rather than findings without \
                         their places; placing its findings reads each file they sit in, one at \
                         a time, beside what the run's relations hold, so read fewer or smaller \
                         files"
                    ),
                )),
            }
        }
        verdict(earlier, failures)
    }

    // ------------------------------------------------------------------
    // Fields, values and failures.

    /// The position of field `name` in the rows, which the builder checked
    /// against the plan.
    fn position(&mut self, name: &str) -> Option<usize> {
        let position = self.rows.field(name);
        if position.is_none() {
            self.fail(
                None,
                format!(
                    "the rows have no field `{name}`, which the plan declares; this is a bug \
                     in alint, so please report it"
                ),
            );
        }
        position
    }

    fn positions(&mut self, names: &[String]) -> Option<Vec<usize>> {
        names.iter().map(|name| self.position(name)).collect()
    }

    fn sensitive(&self, position: usize) -> bool {
        self.rows.fields[position].sensitive
    }

    /// Whether a place at a cell of the field at `position` shows only its
    /// start ([`point`]).
    fn point(&self, position: usize) -> bool {
        point(self.rows, position, self.hide_keys)
    }

    fn name(&self, position: usize) -> &str {
        &self.rows.fields[position].name
    }

    fn cell(&self, row: usize, position: usize) -> &'r Cell {
        &self.rows.rows[row].cells[position]
    }

    fn located(&self, cell: &Cell) -> String {
        located(cell, self.hide_keys)
    }

    /// Where row `row` itself came from: its entry, or its `read:` value.
    fn own(&self, row: usize) -> &'r Arc<Provenance> {
        &self.rows.rows[row].at
    }

    /// Where a finding about row `row`'s field at `position` sits
    /// ([`spot_in`]).
    fn spot(&self, row: usize, position: usize) -> Place<'r> {
        spot_in(self.rows, row, position, self.hide_keys)
    }

    /// Where `place` is, as a message names it.
    fn located_at(&self, place: Place<'_>) -> String {
        match place {
            Place::Cell(cell, _) => self.located(cell),
            Place::Row(at) => self.located(&Cell {
                value: None,
                at: Arc::clone(at),
                span: None,
                path: None,
            }),
            Place::Resource(path) => crate::slash(path),
            Place::Nowhere => "no resource".to_string(),
        }
    }

    /// Which `union:` part read nothing where row `row`'s field at
    /// `position` is missing, after a space: ``(`relations.pins.union[2]`,
    /// `read.json: $.volta.node`, matched nothing)``, so the findings of two
    /// parts that read nothing in one file differ. The part's query is left
    /// out when its relation's configuration reads the environment or the
    /// field is sensitive. Nothing for a value a relation of one part read:
    /// its resource names the row.
    fn read_nothing(&self, row: usize, position: usize) -> String {
        let plan = &self.rule.plan;
        let id = reading_relation(plan, self.rule.inputs[0], self.name(position));
        let named = plan.relation(id);
        let Rows::Union(parts) = &named.relation.rows else {
            return String::new();
        };
        let index = self.cell(row, position).at.part;
        let Some(part) = usize::try_from(index).ok().and_then(|at| parts.get(at)) else {
            return String::new();
        };
        let at = if named.is_inline() {
            format!("{}.union[{index}]", named.name)
        } else {
            format!("relations.{}.union[{index}]", named.name)
        };
        if named.reads_environment || self.sensitive(position) {
            format!(" (`{at}` matched nothing)")
        } else {
            format!(" (`{at}`, `{}`, matched nothing)", part_text(part))
        }
    }

    fn fail(&mut self, at: Option<&Path>, message: String) {
        let failure = self.rule.failure(at, message);
        self.failures.push(failure);
    }

    /// Row `row`'s value of the field at `position`, or `None` after an
    /// analysis failure for a value no comparison takes (a list, a record).
    fn scalar(&mut self, row: usize, position: usize, option: &str) -> Option<Scalar> {
        let cell = self.cell(row, position);
        match Scalar::of(cell) {
            Ok(scalar) => Some(scalar),
            Err(held) => {
                let message = format!(
                    "`{option}`: `{}` holds {held} at {}; a constraint compares strings, \
                     numbers, booleans and null",
                    self.name(position),
                    self.located(cell)
                );
                self.fail(Some(cell.at.resource.path().as_path()), message);
                None
            }
        }
    }

    /// Every row's value of the field at `position`.
    fn column(&mut self, position: usize, option: &str) -> Option<Vec<Scalar>> {
        let mut values = Vec::with_capacity(self.rows.rows.len());
        let mut complete = true;
        for row in 0..self.rows.rows.len() {
            match self.scalar(row, position, option) {
                Some(value) => values.push(value),
                None => complete = false,
            }
        }
        complete.then_some(values)
    }

    /// An analysis failure when the values of `rows` at `position` that can
    /// match are of more than one type: comparing them would cross types.
    fn one_type(&mut self, rows: &[usize], position: usize, values: &[Scalar], option: &str) {
        let mut first: BTreeMap<Kind, usize> = BTreeMap::new();
        for &row in rows {
            if let Some(kind) = values[row].kind() {
                first.entry(kind).or_insert(row);
            }
        }
        let mut witnesses: Vec<usize> = first.into_values().collect();
        witnesses.sort_unstable();
        if let [a, b, ..] = witnesses[..] {
            let (one, two) = (self.cell(a, position), self.cell(b, position));
            let message = format!(
                "`{option}`: `{}` holds {} at {} and {} at {}; the comparison would cross \
                 types, so the assertion has no verdict",
                self.name(position),
                type_of(one),
                self.located(one),
                type_of(two),
                self.located(two)
            );
            self.fail(Some(one.at.resource.path().as_path()), message);
        }
    }

    /// Each row's group: the value of the field at `position`, which must be
    /// present, not null, and of one type in every row.
    fn groups_by(&mut self, position: usize, option: &str) -> Option<(Groups, Vec<Scalar>)> {
        let values = self.column(position, option)?;
        for (row, value) in values.iter().enumerate() {
            if value.is_null() {
                let cell = self.cell(row, position);
                let message = format!(
                    "`{option}`: `{}` has no value at {}; every row's group must have one",
                    self.name(position),
                    self.located(cell)
                );
                self.fail(Some(cell.at.resource.path().as_path()), message);
                return None;
            }
        }
        let all: Vec<usize> = (0..values.len()).collect();
        let before = self.failures.len();
        self.one_type(&all, position, &values, option);
        if self.failures.len() > before {
            return None;
        }
        let mut groups = Groups::new();
        for (row, value) in values.iter().enumerate() {
            groups.add(vec![value.clone()], row);
        }
        Some((groups, values))
    }

    // ------------------------------------------------------------------
    // Findings.

    /// A finding at `at`, identified by `key` alone, with `related`
    /// locations, which leave out `omitted` more; its message is the rule's
    /// `message:` rendered for row `reported` (none, for a value only another
    /// relation holds), or `generated`.
    fn report(
        &mut self,
        at: Place<'_>,
        generated: String,
        key: Key,
        related: Related,
        reported: Option<usize>,
    ) {
        let message = match &self.rule.message {
            Some(template) => self.render(template, reported.map(|row| &self.rows.rows[row])),
            None => generated,
        };
        let (finding, asked) = match at {
            Place::Cell(cell, point) => (
                Violation::new(message).with_path(cell.at.resource.path().as_path()),
                self.places.ask(cell, point),
            ),
            Place::Row(row) => (
                Violation::new(message).with_path(row.resource.path().as_path()),
                None,
            ),
            Place::Resource(path) => (Violation::new(message).with_path(path), None),
            // A group named only in `groups:` has no resource.
            Place::Nowhere => (Violation::new(message), None),
        };
        let finding = finding
            .with_baseline_key(key.into_string())
            .with_path_free_identity()
            .with_not_fixable();
        self.findings.push(Pending {
            finding,
            at: asked,
            related,
        });
    }

    /// The rule's `message:` with each `{{row.FIELD}}` replaced by the
    /// reported row's value of the field (`<redacted>` for a sensitive
    /// field's, and `<no row>` without a reported row).
    fn render(&self, template: &str, row: Option<&FieldRow>) -> String {
        alint_core::template::render_message(template, |namespace, name| {
            if namespace != "row" {
                return None;
            }
            let position = self.rows.field(name)?;
            if self.sensitive(position) {
                return Some("<redacted>".to_string());
            }
            let Some(row) = row else {
                return Some("<no row>".to_string());
            };
            Some(match &row.cells[position].value {
                None => "<missing>".to_string(),
                Some(Value::Null) => "null".to_string(),
                Some(value) => match Scalar::of_value(value) {
                    Ok(scalar) => scalar.text_shown(),
                    Err(_) => alint_core::relation::eval::render_value(value),
                },
            })
        })
    }

    /// A related location at `place`, labelled `label`, and the place it
    /// asked for.
    fn related(&mut self, place: Place<'_>, label: &str) -> (RelatedLocation, Option<Asked>) {
        match place {
            Place::Cell(cell, point) => (
                RelatedLocation::new(cell.at.resource.path().as_path(), label),
                self.places.ask(cell, point),
            ),
            Place::Row(row) => (
                RelatedLocation::new(row.resource.path().as_path(), label),
                None,
            ),
            Place::Resource(path) => (RelatedLocation::new(path, label), None),
            Place::Nowhere => unreachable!("a related location is at a row or a resource"),
        }
    }

    /// The related locations at `places`, which leave out `omitted` more.
    ///
    /// A finding builds no more related locations than it lists
    /// ([`MAX_RELATED_LOCATIONS`]): a family picks the rows it relates to
    /// first ([`related_rows`]), and counts the rest. Building one for every
    /// row and cutting the list after made a finding cost every row, and the
    /// findings quadratic in the values; a debug build stops there.
    fn related_at(&mut self, places: Vec<(Place<'_>, &str)>, omitted: usize) -> Related {
        debug_assert!(
            places.len() <= MAX_RELATED_LOCATIONS,
            "a finding builds {} related locations, past the {MAX_RELATED_LOCATIONS} it lists",
            places.len()
        );
        Related {
            locations: places
                .into_iter()
                .map(|(place, label)| self.related(place, label))
                .collect(),
            omitted,
        }
    }

    /// `` `name` "value" `` for the fields at `positions` and their values.
    fn describe(&self, positions: &[usize], values: &[Scalar]) -> String {
        positions
            .iter()
            .zip(values)
            .map(|(&position, value)| {
                format!(
                    "`{}` {}",
                    self.name(position),
                    value.show(self.sensitive(position))
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// A tuple of values as a message shows it, with their types when
    /// `typed`.
    fn show_tuple(&self, positions: &[usize], values: &[Scalar], typed: bool) -> String {
        let shown: Vec<String> = positions
            .iter()
            .zip(values)
            .map(|(&position, value)| {
                let sensitive = self.sensitive(position);
                if typed {
                    value.show_typed(sensitive)
                } else {
                    value.show(sensitive)
                }
            })
            .collect();
        match shown.as_slice() {
            [one] => one.clone(),
            _ => format!("({})", shown.join(", ")),
        }
    }

    fn field_list(&self, positions: &[usize]) -> String {
        match positions {
            [one] => format!("`{}`", self.name(*one)),
            _ => format!(
                "({})",
                positions
                    .iter()
                    .map(|&position| format!("`{}`", self.name(position)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    // ------------------------------------------------------------------
    // `unique` and `equal` within groups.

    pub(super) fn unique(&mut self, key: &[String], determines: &[String]) {
        let (Some(key_at), Some(determined_at)) = (self.positions(key), self.positions(determines))
        else {
            return;
        };
        let mut columns = Vec::with_capacity(key_at.len());
        for &position in &key_at {
            let Some(values) = self.column(position, "assert.unique.key") else {
                return;
            };
            let all: Vec<usize> = (0..values.len()).collect();
            self.one_type(&all, position, &values, "assert.unique.key");
            columns.push(values);
        }
        if !self.failures.is_empty() {
            return;
        }
        // A null key is never a repeated key.
        let mut groups = Groups::new();
        for row in 0..self.rows.rows.len() {
            let value: Vec<Scalar> = columns.iter().map(|column| column[row].clone()).collect();
            if !value.iter().any(Scalar::is_null) {
                groups.add(value, row);
            }
        }
        for (value, members) in groups.iter() {
            if members.len() < 2 {
                continue;
            }
            if determined_at.is_empty() {
                self.repeats(&key_at, value, members);
            } else {
                let subject = format!("rows with {}", self.describe(&key_at, value));
                let mut base = Key::new("unique");
                for (&position, part) in key_at.iter().zip(value) {
                    base.value(part, self.sensitive(position));
                }
                self.disagreements(
                    members,
                    &determined_at,
                    &subject,
                    &base,
                    "assert.unique.determines",
                );
            }
        }
    }

    /// One finding per row of `members` after the first, which repeats its
    /// key `value`. They share one baseline key, the key's values: the rows
    /// that repeat a key are told apart by their count, so renaming the
    /// first row's file changes none of them.
    fn repeats(&mut self, key_at: &[usize], value: &[Scalar], members: &[usize]) {
        let first = self.spot(members[0], key_at[0]);
        let first_shown = self.located_at(first);
        let mut key = Key::new("unique");
        for (&position, part) in key_at.iter().zip(value) {
            key.value(part, self.sensitive(position));
        }
        for &row in &members[1..] {
            let message = format!(
                "{} is not unique: the first row with it is {first_shown}",
                self.describe(key_at, value),
            );
            let related = self.related_at(vec![(first, "the first row with this key")], 0);
            let at = self.spot(row, key_at[0]);
            self.report(at, message, key.clone(), related, Some(row));
        }
    }

    pub(super) fn equal_groups(&mut self, field: &str, group_by: Option<&str>) {
        let Some(field_at) = self.position(field) else {
            return;
        };
        let groups = match group_by {
            Some(group_by) => {
                let Some(group_at) = self.position(group_by) else {
                    return;
                };
                let Some((groups, _)) = self.groups_by(group_at, "assert.equal.group_by") else {
                    return;
                };
                Some((group_at, groups))
            }
            None => None,
        };
        if self.column(field_at, "assert.equal.field").is_none() {
            return;
        }
        let Some((group_at, groups)) = groups else {
            let members: Vec<usize> = (0..self.rows.rows.len()).collect();
            let mut base = Key::new("equal");
            base.part("all");
            self.disagreements(&members, &[field_at], "rows", &base, "assert.equal.field");
            return;
        };
        for (value, members) in groups.iter() {
            let subject = format!("rows with {}", self.describe(&[group_at], value));
            let mut base = Key::new("equal");
            base.value(&value[0], self.sensitive(group_at));
            self.disagreements(members, &[field_at], &subject, &base, "assert.equal.field");
        }
    }

    /// Rows `members`, which must agree on the fields at `fields`: when they
    /// do not (two values, or a null, which matches nothing), one finding per
    /// distinct value, at the first row with it, related to one row of each
    /// other value and to the other rows with it ([`related_rows`]). Each
    /// finding's key is `base` and the value.
    fn disagreements(
        &mut self,
        members: &[usize],
        fields: &[usize],
        subject: &str,
        base: &Key,
        option: &str,
    ) {
        if members.len() < 2 {
            return;
        }
        let mut values = Groups::new();
        for &row in members {
            let mut value = Vec::with_capacity(fields.len());
            for &position in fields {
                let Some(scalar) = self.scalar(row, position, option) else {
                    return;
                };
                value.push(scalar);
            }
            values.add(value, row);
        }
        let held_null = values.order.iter().flatten().any(Scalar::is_null);
        if values.order.len() == 1 && !held_null {
            return;
        }
        // The first field the rows disagree on, where each finding sits.
        let varying = (0..fields.len())
            .find(|&index| {
                let first = &values.order[0][index];
                first.is_null() || values.order.iter().any(|value| &value[index] != first)
            })
            .unwrap_or(0);
        // Values of different types differ; the message shows the types.
        let typed = (0..fields.len()).any(|index| {
            let mut kinds: Vec<Kind> = values
                .order
                .iter()
                .filter_map(|value| value[index].kind())
                .collect();
            kinds.sort_unstable();
            kinds.dedup();
            kinds.len() > 1
        });
        let distinct: Vec<(Vec<Scalar>, Vec<usize>)> = values
            .iter()
            .map(|(value, rows)| (value.clone(), rows.clone()))
            .collect();
        for (index, (value, holders)) in distinct.iter().enumerate() {
            let at = self.spot(holders[0], fields[varying]);
            let (rows, omitted) = related_rows(&distinct, index, MAX_RELATED_LOCATIONS);
            let places = rows
                .into_iter()
                .map(|(row, same)| {
                    let label = if same {
                        "another row with this value"
                    } else {
                        "a row with another value"
                    };
                    (self.spot(row, fields[varying]), label)
                })
                .collect();
            let related = self.related_at(places, omitted);
            let mut here = format!("{} here", self.show_tuple(fields, value, typed));
            for (&position, part) in fields.iter().zip(value) {
                if part.is_missing() {
                    here.push_str(&self.read_nothing(holders[0], position));
                }
            }
            if holders.len() > 1 {
                let others = holders.len() - 1;
                write!(here, " and in {others} other {}", rows_word(others))
                    .expect("writing to a String cannot fail");
            }
            let mut others: Vec<String> = distinct
                .iter()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .take(LISTED_VALUES)
                .map(|(_, (other, rows))| {
                    format!(
                        "{} in {} {}",
                        self.show_tuple(fields, other, typed),
                        rows.len(),
                        rows_word(rows.len())
                    )
                })
                .collect();
            let unlisted = distinct.len() - 1 - others.len();
            if unlisted > 0 {
                others.push(format!("and {unlisted} more values"));
            }
            let mut message = format!("{subject} disagree on {}: {here}", self.field_list(fields));
            if !others.is_empty() {
                message.push_str("; ");
                message.push_str(&others.join("; "));
            }
            if let Some(nothing) = matches_nothing(value) {
                write!(
                    message,
                    " ({nothing}, so a row without a value agrees with no row)"
                )
                .expect("writing to a String cannot fail");
            }
            let mut key = base.clone();
            for (&position, part) in fields.iter().zip(value) {
                key.value(part, self.sensitive(position));
            }
            self.report(at, message, key, related, Some(holders[0]));
        }
    }

    // ------------------------------------------------------------------
    // `equal` against a source row, and within a row.

    pub(super) fn equal_source(&mut self, field: &str, literals: &[(String, Value)]) {
        let Some(field_at) = self.position(field) else {
            return;
        };
        let mut wanted = Vec::with_capacity(literals.len());
        for (name, literal) in literals {
            let Some(position) = self.position(name) else {
                return;
            };
            let literal = Scalar::of_value(literal).expect("the builder takes scalar literals");
            wanted.push((position, literal));
        }
        let positions: Vec<usize> = wanted.iter().map(|(position, _)| *position).collect();
        let literal_values: Vec<Scalar> =
            wanted.iter().map(|(_, literal)| literal.clone()).collect();
        let source_desc = if self.rule.hide_literals {
            "the literals `to.row` names".to_string()
        } else {
            self.describe(&positions, &literal_values)
        };
        let Some(source) = self.source_row(field_at, &wanted, &source_desc) else {
            return;
        };
        let Some(expected) = self.scalar(source, field_at, "assert.equal.field") else {
            return;
        };
        let source_cell = self.cell(source, field_at);
        let source_at = self.spot(source, field_at);
        let source_shown = self.located_at(source_at);
        if expected.is_null() {
            let message = format!(
                "`assert.equal.to.row`: the source row ({source_desc}, at {source_shown}) holds \
                 no value of `{field}`"
            );
            self.fail(Some(source_cell.at.resource.path().as_path()), message);
            return;
        }
        let sensitive = self.sensitive(field_at);
        for row in 0..self.rows.rows.len() {
            if row == source {
                continue;
            }
            let Some(value) = self.scalar(row, field_at, "assert.equal.field") else {
                return;
            };
            if value == expected {
                continue;
            }
            let typed = value.kind().is_some() && value.kind() != expected.kind();
            let shown = |scalar: &Scalar| {
                if typed {
                    scalar.show_typed(sensitive)
                } else {
                    scalar.show(sensitive)
                }
            };
            let message = format!(
                "`{field}` is {} here{}, but {} in the source row ({source_desc}, at \
                 {source_shown})",
                shown(&value),
                if value.is_missing() {
                    self.read_nothing(row, field_at)
                } else {
                    String::new()
                },
                shown(&expected),
            );
            let mut key = Key::new("equal");
            key.part("source").row(self.own(row), self.hide_keys);
            let related = self.related_at(vec![(source_at, "the source row")], 0);
            let at = self.spot(row, field_at);
            self.report(at, message, key, related, Some(row));
        }
    }

    /// The one row whose fields at `wanted` hold their literals (described
    /// as `source_desc`), or `None` after a failure: no such row, several, or
    /// a literal of another type than its field's values.
    fn source_row(
        &mut self,
        field_at: usize,
        wanted: &[(usize, Scalar)],
        source_desc: &str,
    ) -> Option<usize> {
        let mut sources = Vec::new();
        for row in 0..self.rows.rows.len() {
            let mut matches = true;
            for (position, literal) in wanted {
                let option = format!("assert.equal.to.row.{}", self.name(*position));
                let value = self.scalar(row, *position, &option)?;
                if value.is_null() {
                    matches = false;
                    continue;
                }
                if value.kind() != literal.kind() {
                    let cell = self.cell(row, *position);
                    let message = format!(
                        "`{option}`: the literal is {} and `{}` holds {} at {}; the comparison \
                         would cross types, so the assertion has no verdict",
                        literal_type(literal),
                        self.name(*position),
                        type_of(cell),
                        self.located(cell)
                    );
                    self.fail(Some(cell.at.resource.path().as_path()), message);
                    return None;
                }
                matches &= &value == literal;
            }
            if matches {
                sources.push(row);
            }
        }
        match sources.as_slice() {
            [] => {
                self.fail(
                    None,
                    format!(
                        "`assert.equal.to.row`: no row has {source_desc}; the source row must \
                         exist"
                    ),
                );
                None
            }
            [one] => Some(*one),
            [one, two, ..] => {
                let message = format!(
                    "`assert.equal.to.row`: {} rows have {source_desc}, at {} and {}; exactly \
                     one source row may",
                    sources.len(),
                    self.located(self.cell(*one, field_at)),
                    self.located(self.cell(*two, field_at))
                );
                let at = self.cell(*one, field_at).at.resource.path().as_path();
                self.fail(Some(at), message);
                None
            }
        }
    }

    pub(super) fn equal_field(&mut self, field: &str, other: &str) {
        let (Some(field_at), Some(other_at)) = (self.position(field), self.position(other)) else {
            return;
        };
        for row in 0..self.rows.rows.len() {
            let (Some(value), Some(theirs)) = (
                self.scalar(row, field_at, "assert.equal.field"),
                self.scalar(row, other_at, "assert.equal.to.field"),
            ) else {
                return;
            };
            if value == theirs && !value.is_null() {
                continue;
            }
            let other_cell = self.cell(row, other_at);
            let typed =
                value.kind().is_some() && theirs.kind().is_some() && value.kind() != theirs.kind();
            let shown = |scalar: &Scalar, sensitive: bool| {
                if typed {
                    scalar.show_typed(sensitive)
                } else {
                    scalar.show(sensitive)
                }
            };
            let mut message = format!(
                "`{field}` is {} and `{other}` is {} in this row",
                shown(&value, self.sensitive(field_at)),
                shown(&theirs, self.sensitive(other_at)),
            );
            for (scalar, position) in [(&value, field_at), (&theirs, other_at)] {
                if scalar.is_missing() {
                    message.push_str(&self.read_nothing(row, position));
                }
            }
            message.push_str("; they must be equal");
            if let Some(nothing) = matches_nothing(&[value.clone(), theirs.clone()]) {
                write!(message, " ({nothing})").expect("writing to a String cannot fail");
            }
            let mut key = Key::new("equal");
            key.part("row").row(self.own(row), self.hide_keys);
            let label = format!("`{other}` in this row");
            let other_place = Place::Cell(other_cell, self.point(other_at));
            let related = self.related_at(vec![(other_place, label.as_str())], 0);
            let at = self.spot(row, field_at);
            self.report(at, message, key, related, Some(row));
        }
    }
}

/// Where a finding or a related location sits.
#[derive(Clone, Copy)]
enum Place<'p> {
    /// A cell: its resource, and its span when it has one; only the span's
    /// start when `true`, since the cell holds a sensitive value or key.
    Cell(&'p Cell, bool),
    /// A row with no cell of its own: its resource.
    Row(&'p Arc<Provenance>),
    /// A resource.
    Resource(&'p Path),
    /// No resource: a group named only in `groups:`.
    Nowhere,
}

/// Where a finding about row `row`'s field at `position` of `rows` sits: the
/// field's cell, or, when the cell is not the row's own (a join's copy of a
/// field of its unique side, or a `bound:` field, which many rows share), the
/// row's own entry: its first cell there, one with a span first, or its
/// resource.
///
/// The end of a cell's span gives away the length of what it spans, so only
/// its start is shown ([`point`]) when the cell is a sensitive field's, or
/// when `hide_keys`: then a key could be a sensitive field's value.
fn spot_in(rows: &RelationRows, row: usize, position: usize, hide_keys: bool) -> Place<'_> {
    let own = &rows.rows[row].at;
    let is_own = |cell: &Cell| Arc::ptr_eq(&cell.at, own) || cell.at == *own;
    let cells = &rows.rows[row].cells;
    let at = |position: usize| Place::Cell(&cells[position], point(rows, position, hide_keys));
    if is_own(&cells[position]) {
        return at(position);
    }
    cells
        .iter()
        .position(|cell| is_own(cell) && cell.span.is_some())
        .or_else(|| cells.iter().position(is_own))
        .map_or(Place::Row(own), at)
}

/// Whether a place at the cell of field `position` of `rows` shows only its
/// start: the field is sensitive, or key paths are hidden (a field that
/// holds keys is sensitive, or the relation's configuration reads the
/// environment), since a cell's span may be its key's.
fn point(rows: &RelationRows, position: usize, hide_keys: bool) -> bool {
    hide_keys || rows.fields[position].sensitive
}

/// A finding's related locations, each with the place it asked for, and how
/// many more it leaves out.
struct Related {
    locations: Vec<(RelatedLocation, Option<Asked>)>,
    omitted: usize,
}

/// The rows a finding about `distinct[index]` (a value and the rows with
/// it, among the distinct values of a comparison) relates to, at most `cap`
/// of them, each marked when it holds the same value; and how many more it
/// leaves out. The first row of each other value comes first, so a finding
/// shows what it disagrees with however many rows agree with it; the value's
/// other rows fill what room is left. No more are picked than are listed,
/// however many values and rows there are.
fn related_rows(
    distinct: &[(Vec<Scalar>, Vec<usize>)],
    index: usize,
    cap: usize,
) -> (Vec<(usize, bool)>, usize) {
    let holders = &distinct[index].1;
    let mut rows: Vec<(usize, bool)> = distinct
        .iter()
        .enumerate()
        .filter(|(other, _)| *other != index)
        .map(|(_, (_, rows))| (rows[0], false))
        .take(cap)
        .collect();
    let room = cap - rows.len();
    rows.extend(holders[1..].iter().take(room).map(|&row| (row, true)));
    let total = distinct.len() - 1 + holders.len() - 1;
    let omitted = total - rows.len();
    (rows, omitted)
}

/// `findings`, or `failures` alone when there are any: an assertion with no
/// verdict reports no finding, which would read as part of one. Each family
/// fails before it reports today; this keeps a later one from mixing them.
fn verdict(findings: Vec<Violation>, failures: Vec<Violation>) -> Vec<Violation> {
    if failures.is_empty() {
        findings
    } else {
        failures
    }
}

/// The relation that read field `name` of relation `id`: `id`, or, for a
/// join's copy of a field of a side, the relation that read that field.
fn reading_relation(plan: &Plan, mut id: RelationId, name: &str) -> RelationId {
    let mut name = name.to_string();
    loop {
        let relation = &plan.relation(id).relation;
        match (
            &relation.rows,
            relation.field(&name).map(|field| &field.source),
        ) {
            (Rows::Join(join), Some(FieldSource::Side(side, field))) => {
                id = match side {
                    JoinSide::Left => join.left.id,
                    JoinSide::Right => join.right.id,
                };
                name.clone_from(field);
            }
            _ => return id,
        }
    }
}

/// A `union:` part's query as a message names it: `read.json: $.name`, or
/// `entries.at: $.a, $.b`.
fn part_text(part: &Part) -> String {
    match &part.reader {
        Reader::Read { read, .. } => match read {
            Read::Json(query) => format!("read.json: {query}"),
            Read::Yaml(query) => format!("read.yaml: {query}"),
            Read::Toml(query) => format!("read.toml: {query}"),
            Read::Regex(pattern) => format!("read.regex: {pattern}"),
            Read::Lines { .. } => "read.lines".to_string(),
            Read::WholeFile => "read.whole_file".to_string(),
        },
        Reader::Entries(entries) => format!("entries.at: {}", entries.at.join(", ")),
    }
}

/// Why rows holding `values` agree with none: `null matches nothing`, `a
/// missing value matches nothing`, or both; `None` when they hold neither.
fn matches_nothing(values: &[Scalar]) -> Option<&'static str> {
    let null = values.contains(&Scalar::Null);
    let missing = values.iter().any(Scalar::is_missing);
    match (null, missing) {
        (true, true) => Some("null and a missing value match nothing"),
        (true, false) => Some("null matches nothing"),
        (false, true) => Some("a missing value matches nothing"),
        (false, false) => None,
    }
}

/// What a cell's value is, as a comparison message names it.
fn type_of(cell: &Cell) -> &'static str {
    cell.value.as_ref().map_or("no value", type_name)
}

fn literal_type(literal: &Scalar) -> &'static str {
    match literal.kind() {
        Some(Kind::Bool) => "a boolean",
        Some(Kind::Number) => "a number",
        Some(Kind::String) => "a string",
        None => "null",
    }
}

fn rows_word(count: usize) -> &'static str {
    if count == 1 { "row" } else { "rows" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_withholds_every_finding() {
        let finding = || Violation::new("a finding");
        let failure = || {
            Violation::new("a failure")
                .as_analysis_failure(alint_core::AnalysisFailureKind::Evaluation)
        };
        assert_eq!(verdict(vec![finding()], Vec::new()).len(), 1);
        let reported = verdict(vec![finding(), finding()], vec![failure()]);
        assert_eq!(
            reported
                .iter()
                .map(|violation| violation.message.as_ref())
                .collect::<Vec<_>>(),
            ["a failure"]
        );
    }

    /// Distinct values, the first held by `first` rows and each other by one.
    fn distinct(values: usize, first: usize) -> Vec<(Vec<Scalar>, Vec<usize>)> {
        let mut row = 0;
        (0..values)
            .map(|index| {
                let held = if index == 0 { first } else { 1 };
                let rows = (row..row + held).collect();
                row += held;
                (vec![Scalar::String(index.to_string().into())], rows)
            })
            .collect()
    }

    #[test]
    fn related_rows_are_bounded_and_list_other_values_first() {
        // 10,000 values: 20 rows of other values, the rest counted.
        let many = distinct(10_000, 3);
        let (rows, omitted) = related_rows(&many, 0, 20);
        assert_eq!(rows.len(), 20);
        assert!(rows.iter().all(|(_, same)| !same));
        assert_eq!(omitted, 9_999 + 2 - 20);
        // 28 rows of one value and 2 of another: the other comes first.
        let two = vec![
            (vec![Scalar::Bool(true)], (0..28).collect()),
            (vec![Scalar::Bool(false)], vec![28, 29]),
        ];
        let (rows, omitted) = related_rows(&two, 0, 20);
        assert_eq!(rows[0], (28, false));
        assert_eq!(
            rows[1..],
            (1..20).map(|row| (row, true)).collect::<Vec<_>>()
        );
        assert_eq!(omitted, 8);
        let (rows, omitted) = related_rows(&two, 1, 20);
        assert_eq!((rows, omitted), (vec![(0, false), (29, true)], 0));
    }
}
