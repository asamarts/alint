//! The plan check (design section 8): before the walk, once every reference
//! has resolved, the loader refuses a plan that cannot run. It checks:
//!
//! - every selector compiles: its globs, and its `scope_filter:` predicates
//!   (a manifest `source:` is a repository-relative path outside any Git
//!   directory, which keeps a selector's reads inside the root and out of
//!   `.git`, ADR-0043 decisions 3 and 5);
//! - no join reads itself through other joins, and no `where:` membership
//!   reference forms a cycle; the error names the cycle;
//! - every part of a `union:` binds every field;
//! - a join's `on:` names a field of each side, and its `left.F`/`right.F`
//!   fields name fields of the sides;
//! - every field `transform:` and `where:` name exists, and every
//!   `{ relation, field }` reference names a field of that relation;
//! - `strip-root` applies to a field bound to `entry.path`, first and once,
//!   and `semver-major`/`semver-minor` not to a field bound to a path or
//!   directory token (`path`, `dir`, `ext`, `entry.path`, `entry.parent`,
//!   `entry.root`);
//! - the literals of `in`/`not_in` can match: no null (which is in no set),
//!   no number (a value compares by the text it is written with, which the
//!   configuration's YAML does not keep), and no boolean for a field that
//!   holds strings.
//!
//! A regex that does not compile and a `JSONPath` that does not parse are
//! refused at decode, where the key path is exact. The checks of a
//! `constraint` rule's `key:`, `determines:`, `group_by:`, `field:`, `to:` and
//! `message:` are phase 7's.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use alint_core::relation::Value;
use alint_core::relation::plan::{
    FieldDef, FieldSource, JoinSide, Member, PathToken, Plan, Predicate, Reached, Read, Reader,
    RelationDef, RelationId, Rows, SelectorDef, SelectorId, SelectorRef, Transform,
};
use alint_core::{Error, Result};

use super::resolve::{Offered, located, suggest};

/// What a plan-check error is about: the check that failed, which the
/// resolver reruns alone ([`recheck`]) on a copy of the plan with the
/// environment's values masked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Checked {
    /// A named selector compiles.
    Selector(SelectorId),
    /// A relation's inline selectors compile.
    InlineSelectors(RelationId),
    /// No relation reads itself (the error names relations only).
    Order,
    /// A relation's rows, transforms and filters.
    Relation(RelationId),
}

/// Check `plan`, and return its relations in an order where each comes after
/// every relation it reads; or the first error, with the check it failed.
pub(super) fn check(plan: &Plan) -> std::result::Result<Vec<RelationId>, Box<(Checked, Error)>> {
    let fail = |checked: Checked| move |error| Box::new((checked, error));
    for index in 0..plan.selectors.len() {
        let checked = Checked::Selector(SelectorId(index));
        recheck_selectors(plan, checked).map_err(fail(checked))?;
    }
    for index in 0..plan.relations.len() {
        let checked = Checked::InlineSelectors(RelationId(index));
        recheck_selectors(plan, checked).map_err(fail(checked))?;
    }
    let order = order(plan).map_err(fail(Checked::Order))?;
    let fields = Fields::new(plan);
    for index in 0..plan.relations.len() {
        let checked = Checked::Relation(RelationId(index));
        check_relation(&fields, RelationId(index)).map_err(fail(checked))?;
    }
    Ok(order)
}

/// Run `checked` alone on `plan`, as [`check`] runs it.
pub(super) fn recheck(plan: &Plan, checked: Checked) -> Result<()> {
    match checked {
        Checked::Selector(_) | Checked::InlineSelectors(_) => recheck_selectors(plan, checked),
        Checked::Order => order(plan).map(drop),
        Checked::Relation(id) => check_relation(&Fields::new(plan), id),
    }
}

/// `id` and every relation it reads, through join sides and membership
/// references, transitively: the relations whose definitions a relation's
/// check reads (a join's copy of a field is described by the field it
/// copies).
pub(super) fn read_by(plan: &Plan, id: RelationId) -> Vec<RelationId> {
    let mut seen = HashSet::from([id]);
    let mut found = vec![id];
    let mut next = 0;
    while let Some(&at) = found.get(next) {
        next += 1;
        for (read, _) in reads(&plan.relation(at).relation, "") {
            if seen.insert(read) {
                found.push(read);
            }
        }
    }
    found
}

/// Each relation's fields by name, and what each field holds once found, so
/// the checks cost one lookup per name a config writes however many fields a
/// relation declares, and however long a chain of joins a field follows.
struct Fields<'p> {
    plan: &'p Plan,
    /// Each relation's fields, the first of each name.
    declared: Vec<HashMap<&'p str, &'p FieldDef>>,
    /// Each relation's fields with at least one transform.
    transformed: Vec<HashSet<&'p str>>,
    /// What [`Fields::holds`] found, by relation and field.
    holds: RefCell<HashMap<(usize, &'p str), Holds>>,
}

impl<'p> Fields<'p> {
    fn new(plan: &'p Plan) -> Self {
        let mut declared = Vec::with_capacity(plan.relations.len());
        let mut transformed = Vec::with_capacity(plan.relations.len());
        for named in &plan.relations {
            let relation = &named.relation;
            let mut fields = HashMap::with_capacity(relation.fields.len());
            for (name, field) in &relation.fields {
                fields.entry(name.as_str()).or_insert(field);
            }
            declared.push(fields);
            // `transforms_of` reads the first entry of a name.
            let mut seen = HashSet::new();
            transformed.push(
                relation
                    .transforms
                    .iter()
                    .filter(|(name, _)| seen.insert(name.as_str()))
                    .filter(|(_, transforms)| !transforms.is_empty())
                    .map(|(name, _)| name.as_str())
                    .collect(),
            );
        }
        Self {
            plan,
            declared,
            transformed,
            holds: RefCell::default(),
        }
    }

    /// Inspect the underlying origin of `field` in relation `id`, following
    /// `join:` copies while distinguishing a mixed-source per-part `union:` from
    /// an intermediate transformed field.
    fn origin_info(&self, id: RelationId, field: &'p str) -> FieldOrigin<'p> {
        let mut at = (id.0, field);
        for _ in 0..=self.plan.relations.len() {
            let Some(definition) = self.field(RelationId(at.0), at.1) else {
                return FieldOrigin::Missing;
            };
            let relation = &self.plan.relations[at.0].relation;
            if relation.uses_part_fields() {
                let sources = relation.field_sources(at.1);
                let Some(first) = sources.first() else {
                    return FieldOrigin::Missing;
                };
                if sources.iter().all(|f| f.source == first.source) {
                    return FieldOrigin::Single(&first.source);
                }
                return FieldOrigin::MixedUnion(sources.into_iter().map(|f| &f.source).collect());
            }
            let FieldSource::Side(side, name) = &definition.source else {
                return FieldOrigin::Single(&definition.source);
            };
            let Rows::Join(join) = &self.plan.relations[at.0].relation.rows else {
                return FieldOrigin::Missing;
            };
            let next = match side {
                JoinSide::Left => join.left.id,
                JoinSide::Right => join.right.id,
            };
            if self.transformed[next.0].contains(name.as_str()) {
                return FieldOrigin::Transformed;
            }
            at = (next.0, name.as_str());
        }
        FieldOrigin::Missing
    }

    /// Field `name` of relation `id`, as [`RelationDef::field`] finds it.
    fn field(&self, id: RelationId, name: &str) -> Option<&'p FieldDef> {
        self.declared[id.0].get(name).copied()
    }

    /// What field `field` of relation `id` holds once its transforms have
    /// run, following a join's `left.F`/`right.F` to the side's field. Each
    /// field a chain passes is found once, and remembered.
    fn holds(&self, id: RelationId, field: &'p str) -> Holds {
        let mut known = self.holds.borrow_mut();
        let mut walked = Vec::new();
        let mut at = (id.0, field);
        // What a field holds when nothing narrower is found (and when a chain
        // outruns the bound, which an acyclic plan never does).
        let mut found = Holds::Any;
        for _ in 0..=self.plan.relations.len() {
            if let Some(&held) = known.get(&at) {
                found = held;
                break;
            }
            #[cfg(test)]
            HOLDS_STEPS.with(|steps| steps.set(steps.get() + 1));
            walked.push(at);
            if self.transformed[at.0].contains(at.1) {
                found = Holds::Strings;
                break;
            }
            let Some(definition) = self.field(RelationId(at.0), at.1) else {
                break;
            };
            let relation = &self.plan.relations[at.0].relation;
            if relation.uses_part_fields() {
                found = relation
                    .field_sources(at.1)
                    .into_iter()
                    .map(|f| source_holds(&f.source))
                    .reduce(widen_holds)
                    .unwrap_or(Holds::Any);
                break;
            }
            let (side, name) = match &definition.source {
                FieldSource::Side(side, name) => (side, name),
                source => {
                    found = source_holds(source);
                    break;
                }
            };
            let Rows::Join(join) = &self.plan.relations[at.0].relation.rows else {
                break;
            };
            let next = match side {
                JoinSide::Left => join.left.id,
                JoinSide::Right => join.right.id,
            };
            at = (next.0, name.as_str());
        }
        for step in walked {
            known.insert(step, found);
        }
        found
    }
}

#[cfg(test)]
thread_local! {
    /// The fields [`Fields::holds`] has looked at on this thread, each step
    /// of a join chain one: the tests bound them.
    static HOLDS_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Compile the named selector or the inline selectors `checked` names.
fn recheck_selectors(plan: &Plan, checked: Checked) -> Result<()> {
    match checked {
        Checked::Selector(id) => {
            let named = &plan.selectors[id.0];
            let display = &plan.source(named.source).display;
            check_selector(
                &named.selector,
                display,
                &format!("selectors.{}", named.name),
            )
        }
        Checked::InlineSelectors(id) => {
            let named = plan.relation(id);
            let display = &plan.source(named.source).display;
            let path = super::decode::relation_path(&named.name);
            match &named.relation.rows {
                Rows::Part(part) => {
                    if let SelectorRef::Inline(selector) = &part.from {
                        check_selector(selector, display, &format!("{path}.from"))?;
                    }
                }
                Rows::Union(parts) => {
                    for (index, part) in parts.iter().enumerate() {
                        if let SelectorRef::Inline(selector) = &part.from {
                            check_selector(
                                selector,
                                display,
                                &format!("{path}.union[{index}].from"),
                            )?;
                        }
                    }
                }
                Rows::Join(_) => {}
            }
            Ok(())
        }
        Checked::Order | Checked::Relation(_) => Ok(()),
    }
}

/// Compile `selector`, at `path` of the source `display`, as a rule's
/// `paths:` and `scope_filter:` compile.
fn check_selector(selector: &SelectorDef, display: &str, path: &str) -> Result<()> {
    alint_core::Scope::from_paths_spec(&selector.paths)
        .map_err(|error| located(display, &format!("{path}.paths"), error))?;
    if let Some(filter) = &selector.scope_filter {
        alint_core::ScopeFilter::from_spec(path, filter.clone()).map_err(|error| {
            let message = match error {
                Error::RuleConfig { message, .. } => message,
                other => other.to_string(),
            };
            // The `has_ancestor:` hint points a rule at its scope; a selector
            // has `paths:` of its own.
            let message = message.replace(
                "use `paths:` on the rule's main scope",
                "use the selector's own `paths:`",
            );
            located(display, &format!("{path}.scope_filter"), message)
        })?;
    }
    Ok(())
}

/// The relations `relation` reads, each with the key path that names it, in
/// the order written: a join's sides, then its filters' `{ relation, field }`
/// references.
fn reads(relation: &RelationDef, path: &str) -> Vec<(RelationId, String)> {
    let mut reads = Vec::new();
    if let Rows::Join(join) = &relation.rows {
        reads.push((join.left.id, format!("{path}.join.left")));
        reads.push((join.right.id, format!("{path}.join.right")));
    }
    for (field, predicates) in &relation.filters {
        for predicate in predicates {
            let (list, members) = match predicate {
                Predicate::In(members) => ("in", members),
                Predicate::NotIn(members) => ("not_in", members),
                Predicate::Matches(_) | Predicate::NotMatches(_) => continue,
            };
            for (index, member) in members.iter().enumerate() {
                if let Member::Relation { relation, .. } = member {
                    reads.push((
                        relation.id,
                        format!("{path}.where.{field}.{list}[{index}].relation"),
                    ));
                }
            }
        }
    }
    reads
}

/// Every relation after the relations it reads, or the error naming the
/// first cycle found: relations in plan order, the relations each reads in
/// the order written. The walk keeps its own stack, so a long chain of joins
/// cannot exhaust the thread's.
fn order(plan: &Plan) -> Result<Vec<RelationId>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        New,
        Open,
        Done,
    }
    let edges: Vec<Vec<(RelationId, String)>> = plan
        .relations
        .iter()
        .map(|named| reads(&named.relation, &super::decode::relation_path(&named.name)))
        .collect();
    let mut state = vec![State::New; edges.len()];
    let mut order = Vec::with_capacity(edges.len());
    for start in 0..edges.len() {
        if state[start] != State::New {
            continue;
        }
        state[start] = State::Open;
        // Each open relation, with the number of its edges taken so far.
        let mut stack = vec![(start, 0usize)];
        while let Some(&(node, taken)) = stack.last() {
            let Some((next, _)) = edges[node].get(taken) else {
                state[node] = State::Done;
                order.push(RelationId(node));
                stack.pop();
                continue;
            };
            let last = stack.len() - 1;
            stack[last].1 += 1;
            match state[next.0] {
                State::New => {
                    state[next.0] = State::Open;
                    stack.push((next.0, 0));
                }
                State::Open => return Err(cycle_error(plan, &edges, &stack, next.0)),
                State::Done => {}
            }
        }
    }
    Ok(order)
}

/// The error for the cycle that closes at `start`, which `stack` holds open.
fn cycle_error(
    plan: &Plan,
    edges: &[Vec<(RelationId, String)>],
    stack: &[(usize, usize)],
    start: usize,
) -> Error {
    let from = stack
        .iter()
        .position(|(node, _)| *node == start)
        .unwrap_or_default();
    let first = plan.relation(RelationId(start));
    let name = |node: usize| {
        let named = plan.relation(RelationId(node));
        if named.source == first.source {
            format!("`{}`", named.name)
        } else {
            format!("`{}` ({})", named.name, plan.source(named.source).display)
        }
    };
    let mut chain: Vec<String> = stack[from..].iter().map(|(node, _)| name(*node)).collect();
    chain.push(name(start));
    let (node, taken) = stack[from];
    let path = &edges[node][taken - 1].1;
    located(
        &plan.source(first.source).display,
        path,
        format!(
            "relation `{}` reads itself: {} (a join's sides and a filter's `{{ relation, field \
             }}` references cannot form a cycle); remove one of these references",
            first.name,
            chain.join(" -> ")
        ),
    )
}

/// What the values of a field hold, as far as the plan knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Holds {
    Strings,
    StringsOrNull,
    Any,
}

fn source_holds(source: &FieldSource) -> Holds {
    match source {
        FieldSource::Path(_)
        | FieldSource::EntryKey
        | FieldSource::EntryRoot
        | FieldSource::EntryPath
        | FieldSource::Value => Holds::Strings,
        FieldSource::EntryParent => Holds::StringsOrNull,
        FieldSource::EntryValue
        | FieldSource::Bound { .. }
        | FieldSource::Record { .. }
        | FieldSource::Side(..) => Holds::Any,
    }
}

fn widen_holds(a: Holds, b: Holds) -> Holds {
    match (a, b) {
        (Holds::Strings, Holds::Strings) => Holds::Strings,
        (Holds::Strings | Holds::StringsOrNull, Holds::Strings | Holds::StringsOrNull) => {
            Holds::StringsOrNull
        }
        _ => Holds::Any,
    }
}

/// A field source as a config writes it.
fn source_text(source: &FieldSource) -> String {
    match source {
        FieldSource::Path(token) => match token {
            PathToken::Path => "path",
            PathToken::Dir => "dir",
            PathToken::Basename => "basename",
            PathToken::Stem => "stem",
            PathToken::Ext => "ext",
            PathToken::ParentName => "parent_name",
        }
        .to_string(),
        FieldSource::EntryKey => "entry.key".to_string(),
        FieldSource::EntryValue => "entry.value".to_string(),
        FieldSource::EntryParent => "entry.parent".to_string(),
        FieldSource::EntryRoot => "entry.root".to_string(),
        FieldSource::EntryPath => "entry.path".to_string(),
        FieldSource::Value => "value".to_string(),
        FieldSource::Bound { query, .. } => format!("bound: {query}"),
        FieldSource::Record { query, .. } => format!("record: {query}"),
        FieldSource::Side(JoinSide::Left, field) => format!("left.{field}"),
        FieldSource::Side(JoinSide::Right, field) => format!("right.{field}"),
    }
}

const fn transform_text(transform: &Transform) -> &'static str {
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

/// A relation reference as it was written: `name`, or `alias.name`.
fn written(plan: &Plan, reached: &Reached<RelationId>) -> String {
    let name = &plan.relation(reached.id).name;
    match &reached.alias {
        Some(alias) => format!("{alias}.{name}"),
        None => name.clone(),
    }
}

fn field_names(relation: &RelationDef) -> Offered<'_> {
    Offered::bare(
        relation
            .fields
            .iter()
            .map(|(name, _)| name.as_str())
            .collect(),
    )
}

/// "`f` is not a field of {of}; its fields are ...".
fn not_a_field(field: &str, of: &str, relation: &RelationDef) -> String {
    format!(
        "`{field}` is not a field of {of}; {}",
        suggest(field, &field_names(relation), "its fields are", "fields")
    )
}

/// Why a union part reading with `reader` cannot bind a field from `source`.
fn unbound(reader: &Reader, source: &FieldSource) -> Option<String> {
    let entry = matches!(
        source,
        FieldSource::EntryKey
            | FieldSource::EntryValue
            | FieldSource::EntryParent
            | FieldSource::EntryRoot
            | FieldSource::EntryPath
    );
    match reader {
        Reader::Entries(_) if *source == FieldSource::Value => Some(
            "`value` is a `read:` row's value; an `entries:` row has `entry.value`".to_string(),
        ),
        Reader::Entries(_) if matches!(source, FieldSource::Record { .. }) => {
            Some("`record:` sources need `records:`; an `entries:` row has `entry.*`".to_string())
        }
        Reader::Records(_) if entry => {
            Some("`entry.*` sources need `entries:`; a `records:` row has `record:`".to_string())
        }
        Reader::Records(_) if *source == FieldSource::Value => {
            Some("`value` is a `read:` row's value; a `records:` row has `record:`".to_string())
        }
        Reader::Read { .. } if entry => {
            Some("`entry.*` sources need `entries:`; a `read:` row has `value`".to_string())
        }
        Reader::Read { .. } if matches!(source, FieldSource::Record { .. }) => {
            Some("`record:` sources need `records:`; a `read:` row has `value`".to_string())
        }
        Reader::Read { read, .. } if matches!(source, FieldSource::Bound { .. }) => {
            unstructured_form(read).map(super::decode::unbound_by)
        }
        _ => None,
    }
}

/// What a part binds, by its reader: the parts of one kind bind the same
/// fields ([`unbound`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReaderKind {
    Entries,
    Records,
    /// A `read:` of a JSON, YAML or TOML document.
    Document,
    /// A `read:` of the file as text.
    Text,
}

impl ReaderKind {
    const ALL: [Self; 4] = [Self::Entries, Self::Records, Self::Document, Self::Text];

    const fn of(reader: &Reader) -> Self {
        match reader {
            Reader::Entries(_) => Self::Entries,
            Reader::Records(_) => Self::Records,
            Reader::Read { read, .. } if unstructured_form(read).is_none() => Self::Document,
            Reader::Read { .. } => Self::Text,
        }
    }
}

/// The key of a `read:` form that reads the file as text (`regex`, `lines`,
/// `whole_file`), and so binds no `bound:` field; `None` for a document.
const fn unstructured_form(read: &Read) -> Option<&'static str> {
    match read {
        Read::Json(_) | Read::Yaml(_) | Read::Toml(_) => None,
        Read::Regex(_) => Some("regex"),
        Read::Lines { .. } => Some("lines"),
        Read::WholeFile => Some("whole_file"),
    }
}

/// Where a relation's errors point: its source's display and its key path.
struct At<'p> {
    display: &'p str,
    path: String,
}

impl At<'_> {
    fn fail(&self, under: &str, message: impl std::fmt::Display) -> Error {
        located(self.display, &format!("{}{under}", self.path), message)
    }
}

fn check_relation(fields: &Fields<'_>, id: RelationId) -> Result<()> {
    let plan = fields.plan;
    let named = plan.relation(id);
    let at = At {
        display: &plan.source(named.source).display,
        path: super::decode::relation_path(&named.name),
    };
    check_rows(fields, &named.relation, &at)?;
    check_transforms(fields, id, &at)?;
    check_filters(fields, id, &at)
}

/// A union's parts bind every field; a join's `on:` and `left.F`/`right.F`
/// name fields of its sides.
fn check_rows(fields: &Fields<'_>, relation: &RelationDef, at: &At<'_>) -> Result<()> {
    let plan = fields.plan;
    match &relation.rows {
        Rows::Union(parts) if !relation.uses_part_fields() => {
            // Whether a part binds a field depends only on its reader's kind,
            // so the first part of each kind is the first a field can fail on.
            let mut firsts: Vec<usize> = ReaderKind::ALL
                .into_iter()
                .filter_map(|kind| {
                    parts
                        .iter()
                        .position(|part| ReaderKind::of(&part.reader) == kind)
                })
                .collect();
            firsts.sort_unstable();
            for (name, field) in &relation.fields {
                for &index in &firsts {
                    if let Some(why) = unbound(&parts[index].reader, &field.source) {
                        return Err(at.fail(
                            &format!(".fields.{name}"),
                            format!(
                                "every part of a union binds every field, and `union[{index}]` \
                                 cannot bind `{}`: {why}",
                                source_text(&field.source)
                            ),
                        ));
                    }
                }
            }
        }
        Rows::Join(join) => {
            let left = &plan.relation(join.left.id).relation;
            let right = &plan.relation(join.right.id).relation;
            for (left_field, right_field) in &join.on {
                let under = format!(".join.on.{left_field}");
                if fields.field(join.left.id, left_field).is_none() {
                    let of = format!("`{}`, the join's left side", written(plan, &join.left));
                    return Err(at.fail(&under, not_a_field(left_field, &of, left)));
                }
                if fields.field(join.right.id, right_field).is_none() {
                    let of = format!("`{}`, the join's right side", written(plan, &join.right));
                    return Err(at.fail(&under, not_a_field(right_field, &of, right)));
                }
            }
            for (name, field) in &relation.fields {
                let FieldSource::Side(side, side_field) = &field.source else {
                    continue;
                };
                let (reached, word) = match side {
                    JoinSide::Left => (&join.left, "left"),
                    JoinSide::Right => (&join.right, "right"),
                };
                let of_side = &plan.relation(reached.id).relation;
                if fields.field(reached.id, side_field).is_none() {
                    return Err(at.fail(
                        &format!(".fields.{name}"),
                        format!(
                            "`{word}.{side_field}` names no field of `{}`, the join's {word} \
                             side; {}",
                            written(plan, reached),
                            suggest(
                                side_field,
                                &field_names(of_side),
                                "its fields are",
                                "fields"
                            )
                        ),
                    ));
                }
            }
        }
        Rows::Part(_) | Rows::Union(_) => {}
    }
    Ok(())
}

/// Underlying non-join origin of a relation field for `check_transforms`.
enum FieldOrigin<'p> {
    Single(&'p FieldSource),
    MixedUnion(Vec<&'p FieldSource>),
    Transformed,
    Missing,
}

/// Each transformed field exists, and its transforms fit its source.
fn check_transforms(fields: &Fields<'_>, id: RelationId, at: &At<'_>) -> Result<()> {
    let named = fields.plan.relation(id);
    let relation = &named.relation;
    for (field_name, transforms) in &relation.transforms {
        let Some(field) = fields.field(id, field_name) else {
            return Err(at.fail(
                &format!(".transform.{field_name}"),
                not_a_field(field_name, &format!("relation `{}`", named.name), relation),
            ));
        };
        // A join's output field holds what the side's field it copies held,
        // including when that side is a mixed-reader per-part `union:`.
        let origin_info = fields.origin_info(id, field_name);
        let origin = match &origin_info {
            FieldOrigin::Single(source) => Some(*source),
            FieldOrigin::MixedUnion(_) | FieldOrigin::Transformed | FieldOrigin::Missing => None,
        };
        let any_path_origin = match &origin_info {
            FieldOrigin::Single(source) => {
                matches!(source, FieldSource::Path(_) | FieldSource::EntryPath)
            }
            FieldOrigin::MixedUnion(sources) => sources
                .iter()
                .any(|source| matches!(source, FieldSource::Path(_) | FieldSource::EntryPath)),
            FieldOrigin::Transformed | FieldOrigin::Missing => false,
        };
        let bound_to = match (&field.source, &origin_info) {
            (_, FieldOrigin::MixedUnion(_)) if relation.uses_part_fields() => {
                "different sources across `union:` parts".to_string()
            }
            (FieldSource::Side(..), FieldOrigin::MixedUnion(_)) => format!(
                "`{}`, a copy of a union field with different sources across `union:` parts",
                source_text(&field.source)
            ),
            (FieldSource::Side(..), FieldOrigin::Single(origin)) => format!(
                "`{}`, a copy of `{}`",
                source_text(&field.source),
                source_text(origin)
            ),
            (FieldSource::Side(..), FieldOrigin::Transformed | FieldOrigin::Missing) => format!(
                "`{}`, a copy of a transformed field",
                source_text(&field.source)
            ),
            (source, _) => format!("`{}`", source_text(source)),
        };
        let scope = if relation.uses_part_fields() {
            " in every part"
        } else {
            ""
        };
        for (index, transform) in transforms.iter().enumerate() {
            let problem = match transform {
                Transform::StripRoot if origin != Some(&FieldSource::EntryPath) => Some(format!(
                    "`strip-root` drops the first segment of a key path, so it applies only to a \
                     field bound to `entry.path`{scope}, or to a join's copy of one no \
                     transform has changed; `{field_name}` is bound to {bound_to}"
                )),
                Transform::StripRoot if index > 0 => Some(format!(
                    "`strip-root` reads the segments of the key path `entry.path` binds, so it \
                     comes first among a field's transforms, and once; `{}` before it leaves a \
                     string",
                    transform_text(&transforms[index - 1])
                )),
                Transform::SemverMajor | Transform::SemverMinor if any_path_origin => {
                    Some(format!(
                        "`{}` reads a version, and `{field_name}` is bound to {bound_to}, which \
                     names where a value is, not a version; apply it to the field that holds \
                     the version",
                        transform_text(transform),
                    ))
                }
                _ => None,
            };
            if let Some(problem) = problem {
                return Err(at.fail(&format!(".transform.{field_name}[{index}]"), problem));
            }
        }
    }
    Ok(())
}

/// Each filtered field exists, each membership reference names a field of
/// its relation, and each literal can match.
fn check_filters(fields: &Fields<'_>, id: RelationId, at: &At<'_>) -> Result<()> {
    let plan = fields.plan;
    let named = plan.relation(id);
    let relation = &named.relation;
    for (name, predicates) in &relation.filters {
        if fields.field(id, name).is_none() {
            return Err(at.fail(
                &format!(".where.{name}"),
                not_a_field(name, &format!("relation `{}`", named.name), relation),
            ));
        }
        for predicate in predicates {
            let (list, members) = match predicate {
                Predicate::In(members) => ("in", members),
                Predicate::NotIn(members) => ("not_in", members),
                Predicate::Matches(_) | Predicate::NotMatches(_) => continue,
            };
            for (index, member) in members.iter().enumerate() {
                let under = format!(".where.{name}.{list}[{index}]");
                match member {
                    Member::Relation {
                        relation: reached,
                        field,
                    } => {
                        let other = &plan.relation(reached.id).relation;
                        if fields.field(reached.id, field).is_none() {
                            let of = format!("`{}`", written(plan, reached));
                            return Err(
                                at.fail(&format!("{under}.field"), not_a_field(field, &of, other))
                            );
                        }
                    }
                    Member::Literal(value) => {
                        if let Some(problem) = literal_problem(value, fields, id, name) {
                            return Err(at.fail(&under, problem));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Why literal `value` in an `in`/`not_in` list of field `field` can never
/// match as written.
fn literal_problem<'p>(
    value: &Value,
    fields: &Fields<'p>,
    id: RelationId,
    field: &'p str,
) -> Option<String> {
    match value {
        Value::Null => Some(
            "null is in no set, so it never matches `in` and never fails `not_in`; remove it"
                .to_string(),
        ),
        Value::Number(_) => Some(
            "a number cannot be listed in this build: a value compares by the text it is \
             written with (`1.10` is not `1.1`), which the config's YAML does not keep; write \
             it as a string (`'1.10'`) to match a string value"
                .to_string(),
        ),
        Value::Bool(flag) if fields.holds(id, field) != Holds::Any => Some(format!(
            "`{field}` holds strings, and a boolean never equals a string; quote it (`'{flag}'`) \
             to match that text"
        )),
        _ => None,
    }
}

#[cfg(test)]
#[path = "plan_check_tests.rs"]
mod tests;
