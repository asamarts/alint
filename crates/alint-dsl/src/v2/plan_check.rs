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
//!   and `semver-major`/`semver-minor` not to a field bound to a path that
//!   never holds a version (`path`, `dir`, `ext`, `entry.path`);
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
    FieldDef, FieldSource, JoinSide, Member, PathToken, Plan, Predicate, Reached, Reader,
    RelationDef, RelationId, Rows, SelectorDef, SelectorRef, Transform,
};
use alint_core::{Error, Result};

use super::resolve::{located, suggest};

/// Check `plan`, and return its relations in an order where each comes after
/// every relation it reads.
pub(super) fn check(plan: &Plan) -> Result<Vec<RelationId>> {
    check_selectors(plan)?;
    let order = order(plan)?;
    let fields = Fields::new(plan);
    for index in 0..plan.relations.len() {
        check_relation(&fields, RelationId(index))?;
    }
    Ok(order)
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
            let (side, name) = match &definition.source {
                FieldSource::Path(_)
                | FieldSource::EntryKey
                | FieldSource::EntryRoot
                | FieldSource::EntryPath
                | FieldSource::Value => {
                    found = Holds::Strings;
                    break;
                }
                FieldSource::EntryParent => {
                    found = Holds::StringsOrNull;
                    break;
                }
                FieldSource::EntryValue | FieldSource::Bound(_) => break,
                FieldSource::Side(side, name) => (side, name),
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

fn check_selectors(plan: &Plan) -> Result<()> {
    for named in &plan.selectors {
        let display = &plan.source(named.source).display;
        check_selector(
            &named.selector,
            display,
            &format!("selectors.{}", named.name),
        )?;
    }
    for named in &plan.relations {
        let display = &plan.source(named.source).display;
        let path = format!("relations.{}", named.name);
        match &named.relation.rows {
            Rows::Part(part) => {
                if let SelectorRef::Inline(selector) = &part.from {
                    check_selector(selector, display, &format!("{path}.from"))?;
                }
            }
            Rows::Union(parts) => {
                for (index, part) in parts.iter().enumerate() {
                    if let SelectorRef::Inline(selector) = &part.from {
                        check_selector(selector, display, &format!("{path}.union[{index}].from"))?;
                    }
                }
            }
            Rows::Join(_) => {}
        }
    }
    Ok(())
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
        .map(|named| reads(&named.relation, &format!("relations.{}", named.name)))
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
        FieldSource::Bound(query) => format!("bound: {query}"),
        FieldSource::Side(JoinSide::Left, field) => format!("left.{field}"),
        FieldSource::Side(JoinSide::Right, field) => format!("right.{field}"),
    }
}

const fn transform_text(transform: Transform) -> &'static str {
    match transform {
        Transform::Trim => "trim",
        Transform::Lower => "lower",
        Transform::SemverMajor => "semver-major",
        Transform::SemverMinor => "semver-minor",
        Transform::StripRoot => "strip-root",
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

fn field_names(relation: &RelationDef) -> Vec<String> {
    relation
        .fields
        .iter()
        .map(|(name, _)| name.clone())
        .collect()
}

/// "`f` is not a field of {of}; its fields are ...".
fn not_a_field(field: &str, of: &str, relation: &RelationDef) -> String {
    format!(
        "`{field}` is not a field of {of}; {}",
        suggest(field, &field_names(relation), "its fields are", "fields")
    )
}

/// Why a union part reading with `reader` cannot bind a field from `source`.
fn unbound(reader: &Reader, source: &FieldSource) -> Option<&'static str> {
    let entry = matches!(
        source,
        FieldSource::EntryKey
            | FieldSource::EntryValue
            | FieldSource::EntryParent
            | FieldSource::EntryRoot
            | FieldSource::EntryPath
    );
    match reader {
        Reader::Entries(_) if *source == FieldSource::Value => {
            Some("`value` is a `read:` row's value; an `entries:` row has `entry.value`")
        }
        Reader::Read { .. } if entry => {
            Some("`entry.*` sources need `entries:`; a `read:` row has `value`")
        }
        _ => None,
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
        path: format!("relations.{}", named.name),
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
        Rows::Union(parts) => {
            // A part binds a field unless its reader is of the other kind, so
            // the first part of each kind is the first a field can fail on.
            let mut firsts: Vec<usize> = [
                parts
                    .iter()
                    .position(|part| matches!(part.reader, Reader::Entries(_))),
                parts
                    .iter()
                    .position(|part| matches!(part.reader, Reader::Read { .. })),
            ]
            .into_iter()
            .flatten()
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
        Rows::Part(_) => {}
    }
    Ok(())
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
        for (index, transform) in transforms.iter().enumerate() {
            let problem = match transform {
                Transform::StripRoot if field.source != FieldSource::EntryPath => Some(format!(
                    "`strip-root` drops the first segment of a key path, so it applies only to a \
                     field bound to `entry.path`; `{field_name}` is bound to `{}`",
                    source_text(&field.source)
                )),
                Transform::StripRoot if index > 0 => Some(format!(
                    "`strip-root` reads the segments of the key path `entry.path` binds, so it \
                     comes first among a field's transforms, and once; `{}` before it leaves a \
                     string",
                    transform_text(transforms[index - 1])
                )),
                Transform::SemverMajor | Transform::SemverMinor
                    if matches!(
                        field.source,
                        FieldSource::Path(PathToken::Path | PathToken::Dir | PathToken::Ext)
                            | FieldSource::EntryPath
                    ) =>
                {
                    Some(format!(
                        "`{}` reads a version, and `{field_name}` is bound to `{}`, which never \
                         holds one; apply it to the field that holds the version",
                        transform_text(*transform),
                        source_text(&field.source)
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
