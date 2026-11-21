//! What a relation does with rows once it has them: a `join:` of two
//! relations, then `transform:`, then `where:` (design sections 6 to 8).
//!
//! Every comparison follows section 6: a value compares by its type and its
//! text (a number by its lexeme, so `1.0` is not `1`); null matches nothing
//! and is in no set, and so is a missing value; and a membership or join
//! comparison between two types is an analysis failure naming both sides,
//! never a silent non-match (in `where:`, unless another predicate drops the
//! row, which no answer would keep: `Evaluator::refine`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use super::failure::{Failures, at, key_path, located, type_name};
use super::{Cell, Evaluator, FieldInfo, FieldRow, RelationNote, RelationRows, Site};
use crate::relation::Resource;
use crate::relation::limits::LimitKind;
use crate::relation::plan::{
    FieldSource, JoinDef, JoinSide, Member, NamedRelation, PathToken, Predicate, RelationDef,
    RelationId, Rows, Transform,
};
use crate::relation::pointer::{Pointer, Segment};
use crate::relation::transform::StringTransform;
use crate::relation::value::Value;
use crate::rule::AnalysisFailureKind;

/// The relations `relation` reads ([`RelationDef::reads`]).
pub(super) fn reads(relation: &RelationDef) -> Vec<RelationId> {
    relation.reads().into_iter().map(|read| read.id).collect()
}

/// The kind of a value, as comparisons keep types apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Kind {
    Bool,
    Number,
    String,
    Other,
}

fn kind(value: &Value) -> Kind {
    match value {
        Value::Bool(_) => Kind::Bool,
        Value::Number(_) => Kind::Number,
        Value::String(_) | Value::Path(_) => Kind::String,
        _ => Kind::Other,
    }
}

/// A value that can match: neither null nor missing.
fn comparable(cell: &Cell) -> Option<&Value> {
    cell.value
        .as_ref()
        .filter(|value| !matches!(value, Value::Null))
}

/// What holds a value a comparison meets: a row's cell, or a literal of a
/// `where:` list.
#[derive(Debug, Clone)]
enum Witness {
    Row(Cell),
    Literal(Value),
}

impl Witness {
    fn value(&self) -> &Value {
        match self {
            Self::Row(cell) => comparable(cell).expect("a witness holds a comparable value"),
            Self::Literal(value) => value,
        }
    }

    /// `a string at `a.b` in x.json`, or `the literal "x"` (`a literal
    /// string` when the field is sensitive). `hide_keys` hides a row's key
    /// path ([`key_path`](super::failure::key_path)).
    fn describe(&self, sensitive: bool, hide_keys: bool) -> String {
        match self {
            Self::Row(cell) => {
                let resource = cell.at.resource.path().as_str();
                match &cell.at.pointer {
                    Some(pointer) if !pointer.segments().is_empty() => format!(
                        "{} at {} in {resource}",
                        type_name(self.value()),
                        key_path(pointer, hide_keys)
                    ),
                    _ => format!("{} in {resource}", type_name(self.value())),
                }
            }
            Self::Literal(value) if sensitive => format!("a literal {}", bare_type(value)),
            Self::Literal(value) => format!("the literal {}", render_value(value)),
        }
    }
}

/// The first value of each type, in the order seen: what a message names
/// when two types meet.
#[derive(Default)]
struct Kinds(BTreeMap<Kind, Witness>);

impl Kinds {
    fn add(&mut self, cell: &Cell) {
        if let Some(value) = comparable(cell) {
            self.0
                .entry(kind(value))
                .or_insert_with(|| Witness::Row(cell.clone()));
        }
    }

    fn add_literal(&mut self, value: &Value) {
        if !matches!(value, Value::Null) {
            self.0
                .entry(kind(value))
                .or_insert_with(|| Witness::Literal(value.clone()));
        }
    }

    /// A value of `self` and one of `other` whose types differ: some
    /// comparison between the two would cross types.
    fn clash<'k>(&'k self, other: &'k Self) -> Option<(&'k Witness, &'k Witness)> {
        self.0.iter().find_map(|(mine, witness)| {
            other
                .0
                .iter()
                .find(|(theirs, _)| theirs != &mine)
                .map(|(_, theirs)| (witness, theirs))
        })
    }
}

/// `string`, `number`: a type without its article, after `a literal`.
fn bare_type(value: &Value) -> &'static str {
    match kind(value) {
        Kind::Bool => "boolean",
        Kind::Number => "number",
        Kind::String => "string",
        Kind::Other => "value",
    }
}

/// The most characters of a value a message quotes: a longer one is cut
/// there and its length named, so a key of a megabyte never makes a message
/// of one (or of six, escaped).
const QUOTED_CHARS: usize = 64;

/// A value as a message quotes it: a string escaped and in quotes, a number
/// by its lexeme, each cut at 64 characters with its length named.
#[must_use]
pub fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => quoted(text, true),
        Value::Number(number) => quoted(number.lexeme(), false),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "null".to_string(),
        other => quoted(&format!("{other:?}"), false),
    }
}

/// `text`, escaped and in quotes when `string`, or its first
/// [`QUOTED_CHARS`] characters and its length: `"abc"... (1048576 bytes)`.
pub(super) fn quoted(text: &str, string: bool) -> String {
    let (shown, cut) = match text.char_indices().nth(QUOTED_CHARS) {
        Some((end, _)) => (&text[..end], true),
        None => (text, false),
    };
    let shown = if string {
        format!("{shown:?}")
    } else {
        shown.to_string()
    };
    if cut {
        format!("{shown}... ({} bytes)", text.len())
    } else {
        shown
    }
}

impl Evaluator<'_> {
    /// Apply `relation`'s transforms, then its filters, to `rows`.
    ///
    /// Every predicate of `where:` tests every row, so the order they are
    /// written in changes nothing. A row that some predicate is false for is
    /// dropped, whatever the others say of it: a type error on it (a pattern
    /// on a number, a list of another type) is no failure, since no answer
    /// would keep the row. A row that no predicate is false for is kept, or,
    /// when a predicate cannot test it, fails the relation with that type
    /// error, the first in the order of the relation's fields and then of
    /// `in`, `not_in`, `matches`, `not_matches`.
    pub(super) fn refine(
        &self,
        site: &Site,
        named: &NamedRelation,
        mut rows: RelationRows,
        failures: &mut Failures,
    ) -> Option<RelationRows> {
        if !failures.is_empty() {
            return None;
        }
        let relation = &named.relation;
        // Under `strip-root`, a top-level key whose value is null, `{}` or
        // `[]` (a stub locale file, `fr:`) holds no keys: its row goes before
        // any transform reads it, so its file is one with no rows.
        let stubs = take_stub_roots(&mut rows, relation);
        for (field, transforms) in &relation.transforms {
            let position = rows.field(field)?;
            for transform in transforms {
                match string_transform(*transform) {
                    Some(transform) => {
                        self.transform_field(site, &mut rows, position, field, transform, failures);
                    }
                    None => self.strip_root(site, &mut rows, &stubs, position, field, failures),
                }
            }
        }
        if !failures.is_empty() {
            return None;
        }
        let tests = self.tests(site, &rows, &relation.filters, failures)?;
        if !failures.is_empty() {
            return None;
        }
        // A value a join's side, or a relation a list reads, skipped is a
        // row the join lacks, or one the list lacks, so that a row it would
        // have dropped is kept: their notes say so.
        for read in reads(relation) {
            if let Ok(read) = self.computed(read)
                && let Err(limit) = self.inherit_notes(&mut rows, &read)
            {
                failures.push_limit(site.failure(
                    AnalysisFailureKind::Evaluation,
                    None,
                    super::limit_reached(&limit),
                ));
                return None;
            }
        }
        if tests.is_empty() {
            return Some(rows);
        }
        let mut kept = Vec::with_capacity(rows.rows.len());
        // The resources a type error was reported for: one failure each.
        let mut reported = HashSet::new();
        for row in std::mem::take(&mut rows.rows) {
            let mut error = None;
            let mut dropped = false;
            for test in &tests {
                match test.verdict(&row.cells[test.position]) {
                    Verdict::Pass => {}
                    Verdict::Fail => {
                        dropped = true;
                        break;
                    }
                    Verdict::TypeError => {
                        error.get_or_insert(test);
                    }
                }
            }
            match error {
                _ if dropped => {}
                None => kept.push(row),
                Some(test) => {
                    let cell = &row.cells[test.position];
                    if !reported.contains(cell.at.resource.path()) {
                        reported.insert(cell.at.resource.path().clone());
                        failures.push(test.type_error(site, cell));
                    }
                }
            }
        }
        if !failures.is_empty() {
            return None;
        }
        rows.rows = kept;
        Some(rows)
    }

    /// Each `where:` predicate, ready to test a row, in the order
    /// [`refine`](Self::refine) reports type errors in: a pattern that does
    /// not compile, or a list whose relation failed or holds too many
    /// values, fails the relation whatever its rows.
    fn tests<'r>(
        &self,
        site: &Site,
        rows: &RelationRows,
        filters: &'r [(String, Vec<Predicate>)],
        failures: &mut Failures,
    ) -> Option<Vec<Test<'r>>> {
        let mut tests = Vec::new();
        for (field, predicates) in filters {
            let position = rows.field(field)?;
            let sensitive = rows.fields[position].sensitive;
            for predicate in predicates {
                let (name, wanted, rank) = match predicate {
                    Predicate::In(_) => ("in", true, 0),
                    Predicate::NotIn(_) => ("not_in", false, 1),
                    Predicate::Matches(_) => ("matches", true, 2),
                    Predicate::NotMatches(_) => ("not_matches", false, 3),
                };
                let check = match predicate {
                    Predicate::Matches(pattern) | Predicate::NotMatches(pattern) => {
                        match regex::Regex::new(pattern) {
                            Ok(regex) => Check::Pattern(regex),
                            Err(error) => {
                                failures.push(site.failure(
                                    AnalysisFailureKind::Evaluation,
                                    None,
                                    format!("`where.{field}.{name}` does not compile: {error}"),
                                ));
                                continue;
                            }
                        }
                    }
                    Predicate::In(members) | Predicate::NotIn(members) => {
                        match self.member_set(site, field, name, members, failures) {
                            Some(set) => Check::Member(set),
                            None => continue,
                        }
                    }
                };
                tests.push(Test {
                    field,
                    position,
                    sensitive,
                    hide_keys: site.keys_sensitive,
                    name,
                    wanted,
                    rank,
                    check,
                });
            }
        }
        tests.sort_by_key(|test| (test.position, test.rank));
        Some(tests)
    }

    /// The set an `in` or `not_in` list stands for: its literals, and every
    /// value of each referenced relation's field, which the distinct-value
    /// limit bounds.
    fn member_set(
        &self,
        site: &Site,
        field: &str,
        name: &str,
        members: &[Member],
        failures: &mut Failures,
    ) -> Option<MemberSet> {
        let mut set = MemberSet::default();
        for member in members {
            match member {
                Member::Literal(value) => {
                    set.kinds.add_literal(value);
                    if !matches!(value, Value::Null) {
                        set.values.insert(value.clone());
                    }
                }
                Member::Relation {
                    relation,
                    field: of,
                } => {
                    let read = match self.computed(relation.id) {
                        Ok(rows) => rows,
                        Err(theirs) => {
                            failures.inherit(&theirs);
                            return None;
                        }
                    };
                    let Some(position) = read.field(of) else {
                        unreachable!("the plan check names a field of the relation")
                    };
                    for row in &read.rows {
                        let cell = &row.cells[position];
                        let Some(value) = comparable(cell) else {
                            continue;
                        };
                        if set.values.insert(value.clone())
                            && let Err(limit) = self
                                .limits
                                .check(LimitKind::DistinctValues, set.values.len())
                        {
                            let relation = &self.plan.relation(relation.id).name;
                            failures.push(site.failure(
                                AnalysisFailureKind::Evaluation,
                                None,
                                format!(
                                    "`where.{field}.{name}` reads `{relation}.{of}`: {limit}, so \
                                     the relation is not evaluated rather than cut short; narrow \
                                     `{relation}`"
                                ),
                            ));
                            return None;
                        }
                        set.kinds.add(cell);
                    }
                }
            }
        }
        Some(set)
    }

    /// A `join:` relation's rows: for each row of the side that is not
    /// unique, in its order, the row of the unique side whose `on:` fields
    /// equal its own, when there is one.
    pub(super) fn join(
        &self,
        site: &Site,
        named: &NamedRelation,
        join: &JoinDef,
        failures: &mut Failures,
    ) -> Option<RelationRows> {
        // Each side is a relation of its own, evaluated before the join, whose
        // rows took their cells from the run as they were built: no side is
        // built past the run's cap, and `JoinInputs` caps what one join takes.
        let left = self.side(join.left.id, failures)?;
        let right = self.side(join.right.id, failures)?;
        let sides = [
            ("left", &join.left.id, &left),
            ("right", &join.right.id, &right),
        ];
        for (word, id, rows) in sides {
            if let Err(limit) = self.limits.check(LimitKind::JoinInputs, rows.rows.len()) {
                failures.push(site.failure(
                    AnalysisFailureKind::Evaluation,
                    None,
                    format!(
                        "the join's {word} side (`{}`): {limit}, so the relation is not \
                         evaluated rather than cut short; narrow that side",
                        self.plan.relation(*id).name
                    ),
                ));
                return None;
            }
        }
        let (unique, probe) = match join.unique {
            JoinSide::Left => (&left, &right),
            JoinSide::Right => (&right, &left),
        };
        // Each `on:` pair's positions, on the unique side and the other.
        let mut keys = Vec::with_capacity(join.on.len());
        for (left_field, right_field) in &join.on {
            let (left_at, right_at) = (left.field(left_field)?, right.field(right_field)?);
            let (unique_at, probe_at) = match join.unique {
                JoinSide::Left => (left_at, right_at),
                JoinSide::Right => (right_at, left_at),
            };
            keys.push(JoinKey {
                left_field,
                right_field,
                unique_at,
                probe_at,
                sensitive: left.fields[left_at].sensitive || right.fields[right_at].sensitive,
            });
        }
        if let Some(failure) = key_type_clash(site, join.unique, &keys, unique, probe) {
            failures.push(failure);
            return None;
        }
        let unique_name = match join.unique {
            JoinSide::Left => &self.plan.relation(join.left.id).name,
            JoinSide::Right => &self.plan.relation(join.right.id).name,
        };
        let table = unique_table(site, join.unique, unique_name, &keys, unique, failures);
        if !failures.is_empty() {
            return None;
        }
        let fields = join_fields(named, &left, &right)?;
        let mut out = RelationRows {
            fields: fields.iter().map(|(info, _, _)| info.clone()).collect(),
            // Shared with the side, never copied: a chain of joins holds one
            // list.
            resources: Arc::clone(&probe.resources),
            ..RelationRows::default()
        };
        for row in &probe.rows {
            let Some(found) = key_of(&keys, row, false).and_then(|key| table.get(&key)) else {
                continue;
            };
            let matched = &unique.rows[*found];
            if let Err(limit) = self
                .limits
                .check(LimitKind::JoinOutputs, out.rows.len() + 1)
            {
                failures.push(site.failure(
                    AnalysisFailureKind::Evaluation,
                    None,
                    format!(
                        "{limit}, so the relation is not evaluated rather than cut short; \
                         narrow its sides"
                    ),
                ));
                return None;
            }
            if let Err(limit) = self.take_cells(fields.len().max(1)) {
                failures.push_limit(site.failure(
                    AnalysisFailureKind::Evaluation,
                    None,
                    super::limit_reached(&limit),
                ));
                return None;
            }
            let (left_row, right_row) = match join.unique {
                JoinSide::Left => (matched, row),
                JoinSide::Right => (row, matched),
            };
            // A cell's copy shares its value, provenance and key path.
            out.rows.push(FieldRow {
                cells: fields
                    .iter()
                    .map(|(_, side, position)| match side {
                        JoinSide::Left => left_row.cells[*position].clone(),
                        JoinSide::Right => right_row.cells[*position].clone(),
                    })
                    .collect(),
                at: Arc::clone(&row.at),
            });
        }
        Some(out)
    }

    /// Add `from`'s notes to `rows`'s, their bytes taken from the run: each
    /// that `rows` does not already hold, so that a relation read twice (a
    /// join of a relation with itself, or with a join that read it) notes
    /// each skip once.
    fn inherit_notes(
        &self,
        rows: &mut RelationRows,
        from: &RelationRows,
    ) -> Result<(), crate::relation::LimitExceeded> {
        let held: HashSet<_> = rows.notes.iter().map(RelationNote::key).collect();
        for note in &from.notes {
            if !held.contains(&note.key()) {
                self.take_bytes(super::read::note_bytes(&note.skipped, &note.message))?;
                rows.notes.push(note.clone());
            }
        }
        Ok(())
    }

    /// One side of a join, or its failures as the join's.
    fn side(&self, id: RelationId, failures: &mut Failures) -> Option<Arc<RelationRows>> {
        match self.computed(id) {
            Ok(rows) => Some(rows),
            Err(theirs) => {
                failures.inherit(&theirs);
                None
            }
        }
    }

    /// Apply one string transform to the field at `position` of every row. A
    /// value it does not change stays shared, and rows that share a value
    /// share what it becomes; each new text takes its bytes from the run.
    fn transform_field(
        &self,
        site: &Site,
        rows: &mut RelationRows,
        position: usize,
        field: &str,
        string_transform: StringTransform,
        failures: &mut Failures,
    ) {
        // Each text transformed, by the text it was, while the field is.
        let mut done: HashMap<Arc<str>, Arc<str>> = HashMap::new();
        for row in &mut rows.rows {
            let cell = &mut row.cells[position];
            match &cell.value {
                None => {}
                Some(Value::String(text)) => {
                    let became = if let Some(became) = done.get(text) {
                        Arc::clone(became)
                    } else {
                        let applied = string_transform.apply(text);
                        let became: Arc<str> = if applied == **text {
                            Arc::clone(text)
                        } else {
                            if let Err(limit) = self.take_bytes(applied.len()) {
                                failures.push_limit(site.failure(
                                    AnalysisFailureKind::Evaluation,
                                    None,
                                    super::limit_reached(&limit),
                                ));
                                return;
                            }
                            applied.into()
                        };
                        done.insert(Arc::clone(text), Arc::clone(&became));
                        became
                    };
                    cell.value = Some(Value::String(became));
                }
                Some(other) => {
                    failures.push(site.failure(
                        AnalysisFailureKind::Evaluation,
                        Some(resource_of(cell)),
                        format!(
                            "`transform.{field}` applies `{}` to {}{}, but the string transforms \
                             apply to strings; transform a string field",
                            string_transform.name(),
                            type_name(other),
                            at(cell, site.keys_sensitive)
                        ),
                    ));
                }
            }
        }
    }
}

/// How a field gets its value from a resource's path alone: the path token
/// it holds, and the transforms applied to it, innermost relation first.
type FromPath = (PathToken, Vec<StringTransform>);

/// One relation of the chain [`Evaluator::resources_kept`] walks: its
/// fields whose values a resource's path gives, and its `where:` predicates
/// on them.
struct PathLevel<'p> {
    at: RelationId,
    fields: HashMap<&'p str, FromPath>,
    filters: Vec<(String, Vec<Predicate>)>,
}

/// The fields of `relation` whose values a resource's path gives: a path
/// token, or a join's copy of a field of the side whose resources it lists
/// that the path gives there (`inner`), each after the relation's string
/// transforms.
fn path_fields<'p>(
    relation: &'p RelationDef,
    inner: &HashMap<&'p str, FromPath>,
) -> HashMap<&'p str, FromPath> {
    let mut fields = HashMap::new();
    'fields: for (name, field) in &relation.fields {
        let from = match (&field.source, &relation.rows) {
            (FieldSource::Path(token), _) => Some((*token, Vec::new())),
            (FieldSource::Side(side, copied), Rows::Join(join)) if *side != join.unique => {
                inner.get(copied.as_str()).cloned()
            }
            _ => None,
        };
        let Some((token, mut transforms)) = from else {
            continue;
        };
        for transform in relation.transforms_of(name) {
            // The plan check refuses `strip-root` on a path token.
            let Some(transform) = string_transform(*transform) else {
                continue 'fields;
            };
            transforms.push(transform);
        }
        fields.insert(name.as_str(), (token, transforms));
    }
    fields
}

impl Evaluator<'_> {
    /// Relation `id`, then down the side of each join whose resources it
    /// lists, innermost first, each with its fields the path gives and its
    /// `where:` predicates on them.
    fn path_levels(&self, id: RelationId) -> Option<Vec<PathLevel<'_>>> {
        let mut chain = vec![id];
        while let Rows::Join(join) = &self.plan.relation(chain[chain.len() - 1]).relation.rows {
            if chain.len() > self.plan.relations.len() {
                return None;
            }
            chain.push(match join.unique {
                JoinSide::Left => join.right.id,
                JoinSide::Right => join.left.id,
            });
        }
        let mut levels: Vec<PathLevel<'_>> = Vec::with_capacity(chain.len());
        for &at in chain.iter().rev() {
            let relation = &self.plan.relation(at).relation;
            let inner = levels.last().map(|level| &level.fields);
            let fields = path_fields(relation, inner.unwrap_or(&HashMap::new()));
            let filters = relation
                .filters
                .iter()
                .filter(|(field, _)| fields.contains_key(field.as_str()))
                .cloned()
                .collect();
            levels.push(PathLevel {
                at,
                fields,
                filters,
            });
        }
        Some(levels)
    }

    /// [`super::resources_kept`] of relation `id`, which the memo holds with
    /// every relation it reads.
    pub(super) fn resources_kept(&self, id: RelationId, resources: &[Resource]) -> Vec<bool> {
        let kept_all = || vec![true; resources.len()];
        let Some(levels) = self.path_levels(id) else {
            return kept_all();
        };
        // Each level's tests, built as its `where:` builds them.
        let mut tested = Vec::new();
        for level in &levels {
            let Ok(rows) = self.computed(level.at) else {
                return kept_all();
            };
            let named = self.plan.relation(level.at);
            let site = Site {
                prefix: named.name.clone(),
                sensitive: named.reads_environment,
                keys_sensitive: self.keys_sensitive(level.at),
            };
            let mut failures = Failures::default();
            let Some(tests) = self.tests(&site, &rows, &level.filters, &mut failures) else {
                return kept_all();
            };
            if !failures.is_empty() {
                return kept_all();
            }
            for test in tests {
                tested.push((&level.fields[test.field], test));
            }
        }
        if tested.is_empty() {
            return kept_all();
        }
        resources
            .iter()
            .map(|resource| {
                let tokens = super::read::resource_tokens(resource);
                tested.iter().all(|((token, transforms), test)| {
                    let mut text = super::read::token_text(&tokens, *token).to_string();
                    for transform in transforms {
                        text = transform.apply(&text);
                    }
                    test.verdict_of(Some(&Value::String(text.into()))) != Verdict::Fail
                })
            })
            .collect()
    }
}

/// One `on:` pair of a join: its fields, and their positions on the unique
/// side and on the other.
struct JoinKey<'k> {
    left_field: &'k str,
    right_field: &'k str,
    unique_at: usize,
    probe_at: usize,
    /// Either field is sensitive, so a message shows no key.
    sensitive: bool,
}

/// A row's join key, or `None` when a field is null or missing, which joins
/// nothing.
fn key_of(keys: &[JoinKey<'_>], row: &FieldRow, unique_side: bool) -> Option<Vec<Value>> {
    keys.iter()
        .map(|key| {
            let at = if unique_side {
                key.unique_at
            } else {
                key.probe_at
            };
            comparable(&row.cells[at]).cloned()
        })
        .collect()
}

/// A key compared with a key of another type is a failure, whichever rows
/// meet: the types each side's keys hold must agree.
fn key_type_clash(
    site: &Site,
    unique_side: JoinSide,
    keys: &[JoinKey<'_>],
    unique: &RelationRows,
    probe: &RelationRows,
) -> Option<super::RelationFailure> {
    for key in keys {
        let mut unique_kinds = Kinds::default();
        for row in &unique.rows {
            unique_kinds.add(&row.cells[key.unique_at]);
        }
        let mut probe_kinds = Kinds::default();
        for row in &probe.rows {
            probe_kinds.add(&row.cells[key.probe_at]);
        }
        let Some((mine, theirs)) = probe_kinds.clash(&unique_kinds) else {
            continue;
        };
        let (left_side, right_side) = match unique_side {
            JoinSide::Left => (theirs, mine),
            JoinSide::Right => (mine, theirs),
        };
        let Witness::Row(left_cell) = left_side else {
            unreachable!("a join side holds rows")
        };
        return Some(site.failure(
            AnalysisFailureKind::Evaluation,
            Some(resource_of(left_cell)),
            format!(
                "the join compares `{}`, {}, with `{}`, {}; join keys compare within one type, \
                 so transform one side, or join on fields of one type",
                key.left_field,
                left_side.describe(false, site.keys_sensitive),
                key.right_field,
                right_side.describe(false, site.keys_sensitive),
            ),
        ));
    }
    None
}

/// The unique side's rows by key, checked while built: a key seen twice is
/// a failure naming both rows.
fn unique_table(
    site: &Site,
    unique_side: JoinSide,
    unique_name: &str,
    keys: &[JoinKey<'_>],
    unique: &RelationRows,
    failures: &mut Failures,
) -> HashMap<Vec<Value>, usize> {
    let word = match unique_side {
        JoinSide::Left => "left",
        JoinSide::Right => "right",
    };
    let mut table: HashMap<Vec<Value>, usize> = HashMap::with_capacity(unique.rows.len());
    for (index, row) in unique.rows.iter().enumerate() {
        let Some(key) = key_of(keys, row, true) else {
            continue;
        };
        let Some(&first) = table.get(&key) else {
            table.insert(key, index);
            continue;
        };
        let shown = if keys.iter().any(|key| key.sensitive) {
            "a key".to_string()
        } else {
            format!("the key {}", render_key(&key))
        };
        let at = keys
            .iter()
            .map(|key| key.unique_at)
            .find(|&col| unique.rows[first].cells[col].at != row.cells[col].at)
            .unwrap_or(keys[0].unique_at);
        let earlier = &unique.rows[first].cells[at];
        let later = &row.cells[at];
        failures.push(site.failure(
            AnalysisFailureKind::Evaluation,
            Some(resource_of(later)),
            format!(
                "the join's unique side ({word}, `{unique_name}`) repeats {shown}: {} and {}; \
                 `unique: {word}` needs each key once on that side, so make the key unique \
                 there, or name the other side unique",
                located(earlier, site.keys_sensitive),
                located(later, site.keys_sensitive),
            ),
        ));
    }
    table
}

/// A join's fields: each `left.F` or `right.F`, with the side and the
/// position it copies. A copy of a sensitive field is sensitive.
fn join_fields(
    named: &NamedRelation,
    left: &RelationRows,
    right: &RelationRows,
) -> Option<Vec<(FieldInfo, JoinSide, usize)>> {
    named
        .relation
        .fields
        .iter()
        .map(|(name, field)| {
            let FieldSource::Side(side, of) = &field.source else {
                return None;
            };
            let rows = match side {
                JoinSide::Left => left,
                JoinSide::Right => right,
            };
            let position = rows.field(of)?;
            Some((
                FieldInfo {
                    name: name.clone(),
                    sensitive: field.sensitive || rows.fields[position].sensitive,
                },
                *side,
                position,
            ))
        })
        .collect()
}

/// An `in`/`not_in` list's values, and the first value of each type for a
/// message.
#[derive(Default)]
struct MemberSet {
    values: HashSet<Value>,
    kinds: Kinds,
}

impl MemberSet {
    /// The first member whose type is not `kind`: a value of `kind` compared
    /// with the list would meet it.
    fn other_than(&self, kind: Kind) -> Option<&Witness> {
        self.kinds
            .0
            .iter()
            .find(|(held, _)| **held != kind)
            .map(|(_, witness)| witness)
    }
}

/// One `where:` predicate on one field, ready to test rows.
struct Test<'r> {
    field: &'r str,
    position: usize,
    /// The field is sensitive: a message shows no literal of its list.
    sensitive: bool,
    /// Key paths are hidden ([`Site`]).
    hide_keys: bool,
    /// `in`, `not_in`, `matches` or `not_matches`.
    name: &'static str,
    /// Whether a row passes when the value is in the list or matches.
    wanted: bool,
    /// Its place among one field's predicates, for the order of type errors.
    rank: u8,
    check: Check,
}

enum Check {
    Pattern(regex::Regex),
    Member(MemberSet),
}

/// What one predicate says of one row.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Pass,
    Fail,
    /// The predicate cannot test the row's value: a pattern on a value that
    /// is not a string, or a list holding another type.
    TypeError,
}

impl Verdict {
    fn of(passes: bool) -> Self {
        if passes { Self::Pass } else { Self::Fail }
    }
}

impl Test<'_> {
    fn verdict(&self, cell: &Cell) -> Verdict {
        self.verdict_of(cell.value.as_ref())
    }

    /// What the predicate says of a value (`None`: a missing one).
    fn verdict_of(&self, value: Option<&Value>) -> Verdict {
        match &self.check {
            Check::Pattern(regex) => match value {
                // A missing value matches no pattern.
                None => Verdict::of(!self.wanted),
                Some(Value::String(text)) => Verdict::of(regex.is_match(text) == self.wanted),
                Some(_) => Verdict::TypeError,
            },
            // Null and a missing value are in no list.
            Check::Member(set) => match value.filter(|value| !matches!(value, Value::Null)) {
                None => Verdict::of(!self.wanted),
                Some(value) if set.other_than(kind(value)).is_some() => Verdict::TypeError,
                Some(value) => Verdict::of(set.values.contains(value) == self.wanted),
            },
        }
    }

    /// The analysis failure for a row this predicate cannot test.
    fn type_error(&self, site: &Site, cell: &Cell) -> super::RelationFailure {
        let (field, name) = (self.field, self.name);
        let detail = match &self.check {
            Check::Pattern(_) => format!(
                "`where.{field}.{name}` tests {}{}, but a pattern tests strings; filter a string \
                 field, or transform this one",
                type_name(
                    cell.value
                        .as_ref()
                        .expect("a missing value matches no pattern")
                ),
                at(cell, self.hide_keys)
            ),
            Check::Member(set) => {
                let probe = Witness::Row(cell.clone());
                let member = set
                    .other_than(kind(probe.value()))
                    .expect("a type error has a member of another type");
                format!(
                    "`where.{field}.{name}` compares {} with {}; a value is in a list only beside \
                     values of its own type, so compare values of one type (transform the field, \
                     or change the list)",
                    probe.describe(self.sensitive, self.hide_keys),
                    member.describe(self.sensitive, self.hide_keys),
                )
            }
        };
        site.failure(
            AnalysisFailureKind::Evaluation,
            Some(resource_of(cell)),
            detail,
        )
    }
}

fn resource_of(cell: &Cell) -> Arc<std::path::Path> {
    Arc::from(cell.at.resource.path().as_path())
}

/// A join key as a message shows it: `"react"`, or `("a", 1)` for several
/// fields.
fn render_key(key: &[Value]) -> String {
    match key {
        [single] => render_value(single),
        _ => format!(
            "({})",
            key.iter().map(render_value).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// A key path `strip-root` stripped: the one it was, the one it became, and
/// that one's text.
type Stripped = (Arc<Pointer>, Arc<Pointer>, Arc<str>);

/// The most top-level keys a `strip-root` failure names; it counts the rest.
const NAMED_ROOTS: usize = 3;

/// `` `en`, `fr`, `de` and 2 more ``: the first keys of `count`.
fn roots_named(named: &[&Segment], count: usize) -> String {
    let shown = named
        .iter()
        .map(|segment| {
            super::failure::key_path(&Pointer::from_segments(vec![(*segment).clone()]), false)
        })
        .collect::<Vec<_>>()
        .join(", ");
    if count > named.len() {
        format!("{shown} and {} more", count - named.len())
    } else {
        shown
    }
}

/// Each resource's top-level keys, by its path: a set, and the first few to
/// name.
type Roots<'r> = BTreeMap<&'r str, (HashSet<&'r Segment>, Vec<&'r Segment>)>;

/// Add the top-level key of `path`, `cell`'s key path, to its resource's.
fn add_root<'r>(roots: &mut Roots<'r>, cell: &'r Cell, path: &'r Pointer) {
    let (set, named) = roots.entry(cell.at.resource.path().as_str()).or_default();
    if let Some(first) = path.segments().first()
        && set.insert(first)
        && named.len() < NAMED_ROOTS
    {
        named.push(first);
    }
}

/// The string transform `transform` is, or `None` for `strip-root`, which
/// reads a key path's segments rather than a string.
const fn string_transform(transform: Transform) -> Option<StringTransform> {
    match transform {
        Transform::Trim => Some(StringTransform::Trim),
        Transform::Lower => Some(StringTransform::Lower),
        Transform::SemverMajor => Some(StringTransform::SemverMajor),
        Transform::SemverMinor => Some(StringTransform::SemverMinor),
        Transform::StripRoot => None,
    }
}

/// Take out of `rows`, keeping the others' order, each row whose field under
/// `strip-root` is a stub root: a top-level key whose value is null, `{}` or
/// `[]` (a locale file that is `fr:` or `fr: {}`), below which there is
/// nothing to strip to. Such a file has no keys, as an empty one has none,
/// but its key still counts as its one top-level key
/// ([`Evaluator::strip_root`]). A top-level key holding anything else stays,
/// and fails `strip-root`.
fn take_stub_roots(rows: &mut RelationRows, relation: &RelationDef) -> Vec<FieldRow> {
    let stripped: Vec<usize> = relation
        .transforms
        .iter()
        .filter(|(_, transforms)| transforms.contains(&Transform::StripRoot))
        .filter_map(|(field, _)| rows.field(field))
        .collect();
    let is_stub = |row: &FieldRow| {
        stripped.iter().any(|&position| {
            let cell = &row.cells[position];
            cell.at.empty
                && cell
                    .path
                    .as_ref()
                    .is_some_and(|path| path.segments().len() == 1)
        })
    };
    if !rows.rows.iter().any(is_stub) {
        return Vec::new();
    }
    let (stubs, kept) = std::mem::take(&mut rows.rows)
        .into_iter()
        .partition(is_stub);
    rows.rows = kept;
    stubs
}

impl Evaluator<'_> {
    /// `strip-root`: drop the first segment of an `entry.path` field, as
    /// `key_parity`'s `strip_root` does. Each resource must hold one
    /// top-level key among its rows and its `stubs` (the stub roots
    /// [`take_stub_roots`] took out, which are no rows), and every row's key
    /// path at least two segments. Rows that share a key path share what it
    /// becomes, and each new one takes its bytes from the run.
    fn strip_root(
        &self,
        site: &Site,
        rows: &mut RelationRows,
        stubs: &[FieldRow],
        position: usize,
        field: &str,
        failures: &mut Failures,
    ) {
        let sensitive = rows.fields[position].sensitive || site.keys_sensitive;
        // Each resource's top-level keys: a set, and the first few to name.
        let mut roots: Roots<'_> = BTreeMap::new();
        for row in &rows.rows {
            let cell = &row.cells[position];
            let Some(path) = &cell.path else {
                failures.push(site.failure(
                    AnalysisFailureKind::Evaluation,
                    Some(resource_of(cell)),
                    format!(
                        "`strip-root` on `{field}` applies to an `entry.path` value, and this \
                         row's is not one; bind `{field}` to `entry.path` in every part"
                    ),
                ));
                continue;
            };
            add_root(&mut roots, cell, path);
        }
        for row in stubs {
            let cell = &row.cells[position];
            if let Some(path) = &cell.path {
                add_root(&mut roots, cell, path);
            }
        }
        for (resource, (set, named)) in &roots {
            if set.len() > 1 {
                let shown = if sensitive {
                    set.len().to_string()
                } else {
                    roots_named(named, set.len())
                };
                failures.push(site.failure(
                    AnalysisFailureKind::Evaluation,
                    Some(Arc::from(std::path::Path::new(resource))),
                    format!(
                        "`strip-root` on `{field}` needs one top-level key in each resource, but \
                         this one has {shown}; read one top-level object per file, or drop \
                         `strip-root`"
                    ),
                ));
            }
        }
        // Each key path stripped, by the one it was, which the entry keeps
        // alive (so its address names it) while the field is stripped.
        let mut done: HashMap<*const Pointer, Stripped> = HashMap::new();
        for row in &mut rows.rows {
            let cell = &mut row.cells[position];
            let Some(path) = &cell.path else {
                continue;
            };
            if path.segments().len() < 2 {
                let shown = if sensitive {
                    "a key path".to_string()
                } else {
                    super::failure::key_path(path, false)
                };
                let resource = resource_of(cell);
                failures.push(site.failure(
                    AnalysisFailureKind::Evaluation,
                    Some(resource),
                    format!(
                        "`strip-root` on `{field}` needs every key path to have at least two \
                         segments, but {shown} has one; read the keys below the top-level key, \
                         or drop `strip-root`"
                    ),
                ));
                continue;
            }
            let (stripped, text) = if let Some((_, stripped, text)) = done.get(&Arc::as_ptr(path)) {
                (Arc::clone(stripped), Arc::clone(text))
            } else {
                let stripped = Pointer::from_segments(path.segments()[1..].to_vec());
                let text: Arc<str> = stripped.render().into();
                if let Err(limit) =
                    self.take_bytes(super::shared_pointer_bytes(&stripped) + text.len())
                {
                    failures.push_limit(site.failure(
                        AnalysisFailureKind::Evaluation,
                        None,
                        super::limit_reached(&limit),
                    ));
                    return;
                }
                let stripped = Arc::new(stripped);
                let entry = (Arc::clone(path), Arc::clone(&stripped), Arc::clone(&text));
                done.insert(Arc::as_ptr(path), entry);
                (stripped, text)
            };
            cell.value = Some(Value::String(text));
            cell.path = Some(stripped);
        }
    }
}
