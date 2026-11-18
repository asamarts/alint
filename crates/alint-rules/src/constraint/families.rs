//! The `constraint` families over a relation's rows (design section 6):
//! `unique`, `equal` in its three forms, and `set` against a group of the
//! same relation. Each finding sits at a row's field, or at a resource, with
//! related locations at the rows it is compared with; an analysis failure
//! (a null group, a comparison across types, a missing source row) leaves
//! the rule without a verdict, so it reports the failures and no finding.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use alint_core::relation::Value;
use alint_core::relation::eval::{Cell, FieldRow, RelationRows, located, type_name};
use alint_core::{Context, RelatedLocation, Violation};

use super::places::Places;
use super::values::{Key, Kind, Scalar};
use super::{ConstraintRule, SetMode};

/// The most other values a generated message lists.
const LISTED_VALUES: usize = 5;

/// One evaluation of a rule's assertion over its relation's rows.
pub(super) struct Run<'r> {
    rule: &'r ConstraintRule,
    rows: &'r RelationRows,
    places: Places<'r>,
    /// Key paths stay out of messages and baselines: a field that holds keys
    /// is sensitive, or the relation's configuration reads the environment.
    hide_keys: bool,
    findings: Vec<Violation>,
    failures: Vec<Violation>,
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
        let named = rule.plan.relation(rule.input[0]);
        let hide_keys = named.reads_environment
            || alint_core::relation::eval::keys_sensitive(&rule.plan, rule.input[0]);
        Self {
            rule,
            rows,
            places: Places::new(ctx),
            hide_keys,
            findings: Vec::new(),
            failures: Vec::new(),
        }
    }

    /// The findings, or, when the assertion has no verdict, its failures.
    pub(super) fn finish(self) -> Vec<Violation> {
        verdict(self.findings, self.failures)
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

    fn name(&self, position: usize) -> &str {
        &self.rows.fields[position].name
    }

    fn cell(&self, row: usize, position: usize) -> &'r Cell {
        &self.rows.rows[row].cells[position]
    }

    fn located(&self, cell: &Cell) -> String {
        located(cell, self.hide_keys)
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

    /// A finding at `cell` (or at resource `at` when there is no cell),
    /// identified by `key` alone, with `related` locations; its message is
    /// the rule's `message:` rendered for `reported`, or `generated`.
    fn report(
        &mut self,
        at: Place<'_>,
        generated: String,
        key: Key,
        related: Vec<RelatedLocation>,
        reported: usize,
    ) {
        let message = match &self.rule.message {
            Some(template) => self.render(template, &self.rows.rows[reported]),
            None => generated,
        };
        let mut finding = match at {
            Place::Cell(cell) => {
                let finding = Violation::new(message).with_path(cell.at.resource.path().as_path());
                match self.places.range(cell) {
                    Some(range) => finding.with_range(range),
                    None => finding,
                }
            }
            Place::Resource(path) => Violation::new(message).with_path(path),
        };
        finding = finding
            .with_baseline_key(key.into_string())
            .with_path_free_identity()
            .with_related_locations(related)
            .with_not_fixable();
        self.findings.push(finding);
    }

    /// The rule's `message:` with each `{{row.FIELD}}` replaced by the
    /// reported row's value of the field (`<redacted>` for a sensitive
    /// field's).
    fn render(&self, template: &str, row: &FieldRow) -> String {
        alint_core::template::render_message(template, |namespace, name| {
            if namespace != "row" {
                return None;
            }
            let position = self.rows.field(name)?;
            if self.sensitive(position) {
                return Some("<redacted>".to_string());
            }
            Some(match &row.cells[position].value {
                None => "<missing>".to_string(),
                Some(Value::Null) => "null".to_string(),
                Some(value) => match Scalar::of_value(value) {
                    Ok(scalar) => scalar.text().to_string(),
                    Err(_) => alint_core::relation::eval::render_value(value),
                },
            })
        })
    }

    fn related(&mut self, cell: &Cell, label: &str) -> RelatedLocation {
        self.places.related(cell, label)
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
    /// key `value`.
    fn repeats(&mut self, key_at: &[usize], value: &[Scalar], members: &[usize]) {
        let first = self.cell(members[0], key_at[0]);
        for &row in &members[1..] {
            let cell = self.cell(row, key_at[0]);
            let mut key = Key::new("unique");
            key.part("row").row(cell, self.hide_keys);
            let message = format!(
                "{} is not unique: the first row with it is {}",
                self.describe(key_at, value),
                self.located(first)
            );
            let related = vec![self.related(first, "the first row with this key")];
            self.report(Place::Cell(cell), message, key, related, row);
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
    /// distinct value, at the first row with it, related to the other rows
    /// with it and to one row of each other value. Each finding's key is
    /// `base` and the value.
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
            let at = self.cell(holders[0], fields[varying]);
            let mut related = Vec::new();
            for &row in &holders[1..] {
                let cell = self.cell(row, fields[varying]);
                related.push(self.related(cell, "another row with this value"));
            }
            for (other, (_, rows)) in distinct.iter().enumerate() {
                if other != index {
                    let cell = self.cell(rows[0], fields[varying]);
                    related.push(self.related(cell, "a row with another value"));
                }
            }
            let mut here = format!("{} here", self.show_tuple(fields, value, typed));
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
            if value.iter().any(Scalar::is_null) {
                message.push_str(
                    " (null matches nothing, so a row without a value agrees with no row)",
                );
            }
            let mut key = base.clone();
            for (&position, part) in fields.iter().zip(value) {
                key.value(part, self.sensitive(position));
            }
            self.report(Place::Cell(at), message, key, related, holders[0]);
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
        if expected.is_null() {
            let message = format!(
                "`assert.equal.to.row`: the source row ({source_desc}, at {}) holds no value of \
                 `{field}`",
                self.located(source_cell)
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
            let cell = self.cell(row, field_at);
            let typed = value.kind().is_some() && value.kind() != expected.kind();
            let shown = |scalar: &Scalar| {
                if typed {
                    scalar.show_typed(sensitive)
                } else {
                    scalar.show(sensitive)
                }
            };
            let here = if cell.value.is_none() {
                "missing".to_string()
            } else {
                shown(&value)
            };
            let message = format!(
                "`{field}` is {here} here, but {} in the source row ({source_desc}, at {})",
                shown(&expected),
                self.located(source_cell)
            );
            let mut key = Key::new("equal");
            key.part("source").row(cell, self.hide_keys);
            let related = vec![self.related(source_cell, "the source row")];
            self.report(Place::Cell(cell), message, key, related, row);
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
            let cell = self.cell(row, field_at);
            let other_cell = self.cell(row, other_at);
            let typed =
                value.kind().is_some() && theirs.kind().is_some() && value.kind() != theirs.kind();
            let shown = |scalar: &Scalar, cell: &Cell, sensitive: bool| {
                if cell.value.is_none() {
                    "missing".to_string()
                } else if typed {
                    scalar.show_typed(sensitive)
                } else {
                    scalar.show(sensitive)
                }
            };
            let message = format!(
                "`{field}` is {} and `{other}` is {} in this row; they must be equal{}",
                shown(&value, cell, self.sensitive(field_at)),
                shown(&theirs, other_cell, self.sensitive(other_at)),
                if value.is_null() || theirs.is_null() {
                    " (null matches nothing)"
                } else {
                    ""
                }
            );
            let mut key = Key::new("equal");
            key.part("row").row(cell, self.hide_keys);
            let related = vec![self.related(other_cell, &format!("`{other}` in this row"))];
            self.report(Place::Cell(cell), message, key, related, row);
        }
    }

    // ------------------------------------------------------------------
    // `set` against a group of the same relation.

    pub(super) fn set(&mut self, field: &str, group_by: &str, group: &Value, mode: SetMode) {
        let (Some(field_at), Some(group_at)) = (self.position(field), self.position(group_by))
        else {
            return;
        };
        let option = format!("assert.set.{}.group", mode.key());
        let reference = Scalar::of_value(group).expect("the builder takes scalar literals");
        let Some((groups, values)) = self.set_values(field_at, group_at, &reference, &option)
        else {
            return;
        };
        let group_sensitive = self.sensitive(group_at);
        let reference_shown = if self.rule.hide_literals || group_sensitive {
            "<redacted>".to_string()
        } else {
            reference.show(false)
        };
        let reference_key = vec![reference.clone()];
        let Some(reference_rows) = groups.rows.get(&reference_key) else {
            self.fail(
                None,
                format!(
                    "`{option}`: the reference group (`{group_by}` {reference_shown}) has no \
                     rows; a set is compared with it, so it must have some"
                ),
            );
            return;
        };
        let held = |rows: &[usize]| {
            let mut held = Groups::new();
            for &row in rows {
                if !values[row].is_null() {
                    held.add(vec![values[row].clone()], row);
                }
            }
            held
        };
        let reference = Reference {
            field,
            group_by,
            field_at,
            group_sensitive,
            shown: reference_shown,
            held: held(reference_rows),
            mode,
        };
        for (group_value, rows) in groups.iter() {
            if group_value != &reference_key {
                self.compare_group(&reference, &group_value[0], rows, &held(rows));
            }
        }
    }

    /// The findings of group `group` (rows `rows`, whose values are `held`)
    /// against `reference`.
    fn compare_group(
        &mut self,
        reference: &Reference<'_>,
        group: &Scalar,
        rows: &[usize],
        held: &Groups,
    ) {
        let Reference {
            field,
            group_by,
            field_at,
            group_sensitive,
            ref shown,
            ..
        } = *reference;
        let sensitive = self.sensitive(field_at);
        let subject = format!("`{group_by}` {}", group.show(group_sensitive));
        let base = |value: &Scalar, what: &str| {
            let mut key = Key::new("set");
            key.value(group, group_sensitive)
                .value(value, sensitive)
                .part(what);
            key
        };
        if reference.mode == SetMode::EqualTo {
            // A missing value sits at the group's first resource.
            let first = rows
                .iter()
                .map(|&row| self.cell(row, field_at).at.resource.path())
                .min()
                .expect("a group has rows")
                .clone();
            for (value, holders) in reference.held.iter() {
                if held.rows.contains_key(value) {
                    continue;
                }
                let holder = self.cell(holders[0], field_at);
                let message = format!(
                    "{subject} lacks `{field}` {}, which `{group_by}` {shown} holds",
                    value[0].show(sensitive)
                );
                let related = vec![self.related(holder, "the reference group's row with it")];
                self.report(
                    Place::Resource(first.as_path()),
                    message,
                    base(&value[0], "missing"),
                    related,
                    holders[0],
                );
            }
        }
        for (value, holders) in held.iter() {
            let (what, message, related) = match (reference.mode, reference.held.rows.get(value)) {
                (SetMode::EqualTo | SetMode::SubsetOf, None) => {
                    let related = holders[1..]
                        .iter()
                        .map(|&row| {
                            let cell = self.cell(row, field_at);
                            self.places
                                .related(cell, "another row of the group with it")
                        })
                        .collect();
                    let message = format!(
                        "{subject} holds `{field}` {}, which `{group_by}` {shown} lacks",
                        value[0].show(sensitive)
                    );
                    ("extra", message, related)
                }
                (SetMode::DisjointFrom, Some(theirs)) => {
                    let cell = self.cell(theirs[0], field_at);
                    let message = format!(
                        "{subject} and `{group_by}` {shown} both hold `{field}` {}",
                        value[0].show(sensitive)
                    );
                    let related = vec![
                        self.places
                            .related(cell, "the reference group's row with it"),
                    ];
                    ("shared", message, related)
                }
                _ => continue,
            };
            let at = self.cell(holders[0], field_at);
            self.report(
                Place::Cell(at),
                message,
                base(&value[0], what),
                related,
                holders[0],
            );
        }
    }

    /// The rows grouped by the field at `group_at`, and the values of the
    /// field at `field_at`, or `None` after a failure: a null group, groups
    /// or values of two types, or a reference literal of another type than
    /// the groups.
    fn set_values(
        &mut self,
        field_at: usize,
        group_at: usize,
        reference: &Scalar,
        option: &str,
    ) -> Option<(Groups, Vec<Scalar>)> {
        let (groups, group_values) = self.groups_by(group_at, "assert.set.group_by")?;
        // The literal names a group: comparing it with the groups' values
        // must not cross types.
        if let Some(row) = group_values
            .iter()
            .position(|value| value.kind() != reference.kind())
        {
            let cell = self.cell(row, group_at);
            let message = format!(
                "`{option}`: the literal is {} and `{}` holds {} at {}; the comparison would \
                 cross types, so the assertion has no verdict",
                literal_type(reference),
                self.name(group_at),
                type_of(cell),
                self.located(cell)
            );
            self.fail(Some(cell.at.resource.path().as_path()), message);
            return None;
        }
        let values = self.column(field_at, "assert.set.field")?;
        let all: Vec<usize> = (0..values.len()).collect();
        self.one_type(&all, field_at, &values, "assert.set.field");
        self.failures.is_empty().then_some((groups, values))
    }
}

/// What a `set` compares each group with: the reference group's values
/// (`held`), shown as `shown`.
struct Reference<'a> {
    field: &'a str,
    group_by: &'a str,
    field_at: usize,
    group_sensitive: bool,
    shown: String,
    held: Groups,
    mode: SetMode,
}

/// Where a finding sits.
#[derive(Clone, Copy)]
enum Place<'p> {
    Cell(&'p Cell),
    Resource(&'p Path),
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
}
