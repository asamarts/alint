//! `explain`'s resolved plan of a rule that reads relations, the
//! `constraint` kind (design `v0.23/selectors-and-constraints.md` sections 5,
//! 9, 10 and 11): its assertion, every relation it reads in the order a run
//! evaluates them, each with its source, reader, fields and `where:`, the
//! config source that declared each named object, and the selectors that
//! trigger the rule under `--changed`. A rule that reads no relation has no
//! plan, so a version 1 rule's `explain` is unchanged.
//!
//! The plan is configuration, never data: no value a relation reads from a
//! file is shown. A manifest-derived `scope_filter:` predicate shows the
//! paths it resolves to, as `explain` shows a rule's (ADR-0010). What the
//! environment wrote stays hidden, as `explain` hides a rule's
//! environment-written fields:
//!
//! - a named selector that reads it shows `<redacted>` for its `paths:` and
//!   `scope_filter:`;
//! - a relation whose own definition reads it shows `<redacted>` for every
//!   value that definition holds (an inline selector, its reader, a
//!   `bound:` path, a join's `unique:`), and its names;
//! - a config source an environment-chosen `extends:` entry reached shows as
//!   `list` shows it ([`Redact`]);
//! - an assertion the environment wrote (`assert:`) shows `<redacted>`, and
//!   a rule whose `input:` is hidden (`sensitive: true`) shows no plan.
//!
//! A sensitive field (`sensitive: true`, a `where:` or `transform:` entry
//! the environment wrote, or a field that shows such a field's value) shows
//! its `where:` literals and patterns, its transforms, and the literals the
//! assertion compares with it (`to: { row: }`, `{ group: }`, `groups:`) as
//! `<redacted>`, here and in the rule's `options:`.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::Write;
use std::path::Path;

use alint_core::RuleEntry;
use alint_core::relation::plan::{
    EntriesFormat, FieldSource, JoinSide, Member, NamedRelation, PathToken, Plan, Predicate, Read,
    Reader, RelationDef, RelationId, Rows, SelectorDef, SelectorRef, Transform,
};
use alint_core::relation::{TriggerOrigin, Value};
use alint_output::sanitize_terminal as term;
use anyhow::Result;
use serde::Serialize;

use super::{Redact, explain_field_is_sensitive, explain_repository_path};

const REDACTED: &str = "<redacted>";

/// A value, or `<redacted>` in its place.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub(super) enum Shown<T> {
    Value(T),
    Redacted(&'static str),
}

impl<T> Shown<T> {
    fn hidden_if(hidden: bool, value: impl FnOnce() -> T) -> Self {
        if hidden {
            Self::Redacted(REDACTED)
        } else {
            Self::Value(value())
        }
    }
}

/// A rule's plan, or `<redacted>` for a rule whose `input:` is hidden.
pub(super) type ExplainPlan = Shown<Box<PlanView>>;

#[derive(Debug, Serialize)]
pub(super) struct PlanView {
    /// The `id` of the rule's input in `relations`.
    input: usize,
    /// The `id` of a `set`'s other relation in `relations`.
    #[serde(skip_serializing_if = "Option::is_none")]
    set_target: Option<usize>,
    /// The `id` of a `reference`'s target relation in `relations`.
    #[serde(skip_serializing_if = "Option::is_none")]
    reference_target: Option<usize>,
    /// The assertion, in readable form.
    assertion: Shown<AssertionView>,
    /// Every relation the rule reads, in the order a run evaluates them.
    relations: Vec<RelationView>,
    /// Under `--changed`, the rule runs when a changed path is one of these
    /// selectors' (design section 9).
    trigger: Vec<SelectorView>,
    /// The cost class (design section 8).
    cost: String,
}

#[derive(Debug, Serialize)]
struct AssertionView {
    /// `equal`, `unique`, `set`, `reference`, `count`, `compare`, or `satisfies`.
    family: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct RelationView {
    /// Its place in `relations`, by which a join's sides, an `in:` or
    /// `not_in:` list, the trigger and the plan's `input` and `set_target`
    /// name it.
    id: usize,
    /// As it is declared: a name, or where an inline `input:` is written
    /// (`rules[0].input`).
    name: String,
    /// Each way the config writes a reference that reaches it, qualified
    /// with the `extends:` alias that reference uses where it has one
    /// (`deps.ranges`): the rule's own first, then each relation's reads.
    written: Vec<String>,
    /// What the human output calls it, wherever it refers to it: its first
    /// spelling, with the config source that declares it when another
    /// relation listed would show the same, and its place when that is the
    /// same too.
    #[serde(skip)]
    label: String,
    inline: bool,
    /// `input` and `set_target`, as the rule reads it; empty for one only
    /// another relation reads.
    roles: Vec<&'static str>,
    /// The config source that declares it.
    declared_in: String,
    #[serde(flatten)]
    rows: RowsView,
    fields: Vec<FieldView>,
    #[serde(rename = "where")]
    filters: Vec<FilterView>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum RowsView {
    Part(Box<PartView>),
    Union { union: Vec<PartView> },
    Join { join: JoinView },
}

#[derive(Debug, Serialize)]
struct PartView {
    from: SelectorView,
    #[serde(skip_serializing_if = "Option::is_none")]
    entries: Option<Shown<EntriesView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    records: Option<Shown<RecordsView>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    read: Option<Shown<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    optional: Option<Shown<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<Vec<FieldView>>,
}

#[derive(Debug, Serialize)]
struct EntriesView {
    format: &'static str,
    at: Vec<String>,
    leaves: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    strict: bool,
}

#[derive(Debug, Serialize)]
struct RecordsView {
    format: &'static str,
    at: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    strict: bool,
}

#[derive(Debug, Serialize)]
struct JoinView {
    /// The `id` of its left side in `relations`.
    left: usize,
    /// The `id` of its right side in `relations`.
    right: usize,
    on: Vec<OnPair>,
    unique: Shown<&'static str>,
}

#[derive(Debug, Serialize)]
struct OnPair {
    left: String,
    right: String,
}

#[derive(Debug, Clone, Serialize)]
struct SelectorView {
    /// A named selector's name, as the reference writes it in a relation's
    /// `from:` and as it is declared in the trigger; `None` for an inline one.
    selector: Option<String>,
    /// For an inline selector in the trigger: the `id` in `relations` of
    /// the relation it is part of.
    #[serde(skip_serializing_if = "Option::is_none")]
    relation: Option<usize>,
    /// For an inline selector in the trigger that is part of a `union:`:
    /// the index of its part.
    #[serde(skip_serializing_if = "Option::is_none")]
    part: Option<usize>,
    declared_in: String,
    #[serde(flatten)]
    scope: ScopeView,
}

/// What a selector picks: its `paths:` and its `scope_filter:`.
#[derive(Debug, Clone, Serialize)]
struct ScopeView {
    paths: Shown<PathsView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope_filter: Option<Shown<ScopeFilterView>>,
}

#[derive(Debug, Clone, Serialize)]
struct PathsView {
    include: Vec<String>,
    exclude: Vec<String>,
}

#[derive(Debug, Serialize)]
struct FieldView {
    name: String,
    /// The source as the config writes it (`entry.key`, `bound: $.name`).
    source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    optional: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    on_missing: Option<&'static str>,
    transforms: Shown<Vec<String>>,
    sensitive: bool,
}

#[derive(Debug, Serialize)]
struct FilterView {
    field: String,
    /// `in`, `not_in`, `matches` or `not_matches`.
    predicate: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    members: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pattern: Option<Shown<String>>,
}

/// A `scope_filter:` as `explain` shows it: its gates, and the paths each
/// manifest-derived predicate resolves to (ADR-0010's legibility
/// mitigation), each relative to the root.
#[derive(Debug, Clone, Serialize)]
pub(super) struct ScopeFilterView {
    has_ancestor: Option<Vec<String>>,
    changed_since: Option<String>,
    manifest_scopes: Vec<ManifestScopeView>,
}

#[derive(Debug, Clone, Serialize)]
struct ManifestScopeView {
    predicate: &'static str,
    source: String,
    paths: Vec<String>,
}

impl ScopeFilterView {
    /// `spec`, with its manifests read below `root`; `name` names the rule or
    /// selector in an error.
    pub(super) fn of(name: &str, spec: &alint_core::ScopeFilterSpec, root: &Path) -> Self {
        let manifest_scopes = alint_core::ScopeFilter::from_spec(name, spec.clone())
            .map(|filter| filter.resolved_manifest_sets(root))
            .unwrap_or_default()
            .into_iter()
            .map(|resolved| ManifestScopeView {
                predicate: if resolved.include {
                    "include_manifest_paths"
                } else {
                    "exclude_manifest_paths"
                },
                source: explain_repository_path(root, &resolved.source),
                paths: resolved
                    .paths
                    .iter()
                    .map(|path| explain_repository_path(root, path))
                    .collect(),
            })
            .collect();
        Self {
            has_ancestor: spec.has_ancestor.clone(),
            changed_since: spec.changed_since.clone(),
            manifest_scopes,
        }
    }

    /// The human lines below a `scope_filter:` label, each `indent` deep.
    pub(super) fn write_human(&self, out: &mut impl Write, indent: &str) -> Result<()> {
        let dim = alint_output::style::DIM;
        if let Some(names) = &self.has_ancestor {
            writeln!(
                out,
                "{indent}{dim}has_ancestor: {dim:#} {}",
                term(&names.join(", "))
            )?;
        }
        if let Some(reference) = &self.changed_since {
            writeln!(
                out,
                "{indent}{dim}changed_since:{dim:#} {}",
                term(reference)
            )?;
        }
        for scope in &self.manifest_scopes {
            // Forward-slash every path so `explain` reads identically on
            // Windows and Unix. Both the manifest's own path and the paths it
            // lists (read from repo content, e.g. package.json) are untrusted.
            let paths = if scope.paths.is_empty() {
                "(no paths resolved)".to_string()
            } else {
                scope.paths.join(", ")
            };
            writeln!(
                out,
                "{indent}{dim}{}:{dim:#} {} -> {}",
                scope.predicate,
                term(&scope.source),
                term(&paths)
            )?;
        }
        Ok(())
    }
}

/// The plan of `entry`, when it reads relations: with each config source
/// shown as `redact` leaves it, and manifests read below `root`.
pub(super) fn plan_of(entry: &RuleEntry, root: &Path, redact: Redact<'_>) -> Option<ExplainPlan> {
    let (plan, inputs) = entry.rule.relation_inputs()?;
    if explain_field_is_sensitive(entry, "input") {
        return Some(Shown::Redacted(REDACTED));
    }
    let references = plan
        .rule(entry.rule.id())
        .map_or(&[][..], |rule| &rule.references);
    let input = inputs[0];
    let order = plan.evaluation_order(inputs);
    let builder = Builder {
        plan,
        root,
        redact,
        scopes: std::cell::RefCell::new(HashMap::new()),
        positions: order
            .iter()
            .enumerate()
            .map(|(position, &id)| (id, position))
            .collect(),
    };
    let written = builder.written(&order, references);
    let labels = labels(
        &order
            .iter()
            .map(|id| {
                let named = plan.relation(*id);
                (written[id][0].as_str(), builder.declared_in(named.source))
            })
            .collect::<Vec<_>>(),
    );
    let relations = order
        .iter()
        .zip(&labels)
        .map(|(&id, label)| {
            let mut roles = Vec::new();
            for reference in references.iter().filter(|r| r.relation.id == id) {
                let role = if reference.field == "input" {
                    "input"
                } else if reference.field == "assert.reference.to.relation" {
                    "reference_target"
                } else {
                    "set_target"
                };
                if !roles.contains(&role) {
                    roles.push(role);
                }
            }
            builder.relation(id, written[&id].clone(), label.clone(), roles)
        })
        .collect();
    let trigger = entry
        .rule
        .trigger_scope()
        .map(|scope| {
            scope
                .origins()
                .map(|origin| builder.trigger(origin))
                .collect()
        })
        .unwrap_or_default();
    let set_target = references
        .iter()
        .find(|reference| {
            reference.field != "input" && reference.field != "assert.reference.to.relation"
        })
        .map(|reference| builder.position(reference.relation.id));
    let reference_target = references
        .iter()
        .find(|reference| reference.field == "assert.reference.to.relation")
        .map(|reference| builder.position(reference.relation.id));
    let assert = assert_option(entry);
    let assertion = Shown::hidden_if(explain_field_is_sensitive(entry, "assert"), || {
        assertion(
            &assert,
            &|field| field_is_sensitive(plan, input, field),
            set_target
                .or(reference_target)
                .map(|target| labels[target].as_str()),
        )
    });
    let cost = cost(&assert);
    Some(Shown::Value(Box::new(PlanView {
        input: builder.position(input),
        set_target,
        reference_target,
        assertion,
        relations,
        trigger,
        cost,
    })))
}

/// What the human output calls each relation listed, from its first
/// spelling and the config source that declares it: the spelling alone
/// when no other relation listed has it, then with the source, then with
/// its place in the list too, so no two relations read alike.
fn labels(relations: &[(&str, String)]) -> Vec<String> {
    let mut labels: Vec<String> = relations
        .iter()
        .map(|(spelling, _)| (*spelling).to_string())
        .collect();
    let shared =
        |labels: &[String], label: &str| labels.iter().filter(|other| *other == label).count() > 1;
    let spellings = labels.clone();
    for (label, (spelling, source)) in labels.iter_mut().zip(relations) {
        if shared(&spellings, spelling) {
            *label = format!("{spelling} ({source})");
        }
    }
    let sourced = labels.clone();
    for (position, label) in labels.iter_mut().enumerate() {
        if shared(&sourced, label) {
            let _ = write!(label, " #{}", position + 1);
        }
    }
    labels
}

/// The rule's `assert:` option as JSON.
fn assert_option(entry: &RuleEntry) -> serde_json::Value {
    entry
        .extra()
        .and_then(|extra| extra.get("assert"))
        .and_then(|value| serde_json::to_value(value).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn field_is_sensitive(plan: &Plan, id: RelationId, field: &str) -> bool {
    plan.relation(id)
        .relation
        .field(field)
        .is_some_and(|field| field.sensitive)
}

/// Option `key` of `entry` (its value `value`) with what a sensitive field
/// holds hidden: the literals `assert:` compares with a sensitive field of
/// the input, and an inline `input:`'s `where:` literals, patterns and
/// transforms of its sensitive fields. Every other option, and every option
/// of a rule that reads no relation, is returned as it is.
#[allow(clippy::too_many_lines)]
pub(super) fn redact_option(
    entry: &RuleEntry,
    key: &str,
    mut value: serde_json::Value,
) -> serde_json::Value {
    let Some((plan, inputs)) = entry.rule.relation_inputs() else {
        return value;
    };
    let input = inputs[0];
    let sensitive = |field: &str| field_is_sensitive(plan, input, field);
    let hide = |slot: &mut serde_json::Value| *slot = serde_json::Value::from(REDACTED);
    match key {
        "assert" => {
            for pointer in ["/equal/to/row", "/compare/to/row"] {
                if let Some(row) = value
                    .pointer_mut(pointer)
                    .and_then(serde_json::Value::as_object_mut)
                {
                    for (field, literal) in row.iter_mut() {
                        if sensitive(field) {
                            hide(literal);
                        }
                    }
                }
            }
            let compare_field = value
                .pointer("/compare/field")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            if compare_field.as_deref().is_some_and(sensitive)
                && let Some(val) = value.pointer_mut("/compare/to/value")
            {
                hide(val);
            }
            let satisfies_version = value
                .pointer("/satisfies/version")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            if satisfies_version.as_deref().is_some_and(sensitive)
                && let Some(val) = value.pointer_mut("/satisfies/range_literal")
            {
                hide(val);
            }
            let satisfies_range = value
                .pointer("/satisfies/range")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            if satisfies_range.as_deref().is_some_and(sensitive)
                && let Some(val) = value.pointer_mut("/satisfies/version_literal")
            {
                hide(val);
            }
            let set_field = value
                .pointer("/set/field")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string);
            let set_group_by = field_name_or_list(value.pointer("/set/group_by"));
            if let Some(set) = value.get_mut("set") {
                for mode in ["equal_to", "subset_of", "disjoint_from"] {
                    if let Some(group) = set.pointer_mut(&format!("/{mode}/group")) {
                        redact_group_literal(group, &set_group_by, &sensitive, &hide);
                    }
                    if set_field.as_deref().is_some_and(sensitive)
                        && let Some(values) = set
                            .pointer_mut(&format!("/{mode}/values"))
                            .and_then(serde_json::Value::as_array_mut)
                    {
                        for item in values {
                            hide(item);
                        }
                    }
                }
                for literal in set
                    .get_mut("groups")
                    .and_then(serde_json::Value::as_array_mut)
                    .into_iter()
                    .flatten()
                {
                    redact_group_literal(literal, &set_group_by, &sensitive, &hide);
                }
            }
            let count_group_by = field_name_or_list(value.pointer("/count/group_by"));
            if let Some(groups) = value
                .pointer_mut("/count/groups")
                .and_then(serde_json::Value::as_array_mut)
            {
                for literal in groups {
                    redact_group_literal(literal, &count_group_by, &sensitive, &hide);
                }
            }
        }
        "input" if plan.relation(input).is_inline() => {
            let hidden = |field: &str| sensitive(field);
            if let Some(transform) = value
                .get_mut("transform")
                .and_then(serde_json::Value::as_object_mut)
            {
                for (field, transforms) in transform.iter_mut() {
                    if hidden(field) {
                        hide(transforms);
                    }
                }
            }
            if let Some(filters) = value
                .get_mut("where")
                .and_then(serde_json::Value::as_object_mut)
            {
                for (field, predicates) in filters.iter_mut() {
                    if !hidden(field) {
                        continue;
                    }
                    let Some(predicates) = predicates.as_object_mut() else {
                        hide(predicates);
                        continue;
                    };
                    for (name, predicate) in predicates.iter_mut() {
                        match (name.as_str(), predicate.as_array_mut()) {
                            ("in" | "not_in", Some(members)) => {
                                for member in members.iter_mut().filter(|m| !m.is_object()) {
                                    hide(member);
                                }
                            }
                            _ => hide(predicate),
                        }
                    }
                }
            }
        }
        _ => {}
    }
    value
}

/// A literal as a message quotes it: a string in double quotes, a boolean
/// bare.
fn literal_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) if text == REDACTED => text.clone(),
        other => other.to_string(),
    }
}

fn names(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str().map(str::to_string))
        .collect()
}

fn text_of<'v>(value: &'v serde_json::Value, key: &str) -> &'v str {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}

fn field_name_or_list(value: Option<&serde_json::Value>) -> Vec<String> {
    match value {
        Some(serde_json::Value::String(name)) => vec![name.clone()],
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

fn redact_group_literal(
    literal: &mut serde_json::Value,
    group_by: &[String],
    sensitive: &dyn Fn(&str) -> bool,
    hide: &dyn Fn(&mut serde_json::Value),
) {
    match literal {
        serde_json::Value::Array(items) => {
            for (slot, field) in items.iter_mut().zip(group_by) {
                if sensitive(field) {
                    hide(slot);
                }
            }
        }
        other => {
            if group_by.first().is_some_and(|field| sensitive(field)) {
                hide(other);
            }
        }
    }
}

fn show_group_literal(
    literal: &serde_json::Value,
    group_by: &[String],
    sensitive: &dyn Fn(&str) -> bool,
) -> String {
    match literal {
        serde_json::Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    if group_by.get(idx).is_some_and(|field| sensitive(field)) {
                        REDACTED.to_string()
                    } else {
                        literal_text(item)
                    }
                })
                .collect();
            format!("[{}]", parts.join(", "))
        }
        other => {
            if group_by.first().is_some_and(|field| sensitive(field)) {
                REDACTED.to_string()
            } else {
                literal_text(other)
            }
        }
    }
}

fn format_field_list(fields: &[String]) -> String {
    match fields {
        [one] => one.clone(),
        many => format!("[{}]", many.join(", ")),
    }
}

/// `assert:` in readable form, with the literals compared with a field
/// `sensitive` reports hidden, and a `set` or `reference` target relation
/// called `target` (as the relation list calls it) when the rule reads one.
#[allow(clippy::too_many_lines)]
fn assertion(
    assert: &serde_json::Value,
    sensitive: &dyn Fn(&str) -> bool,
    target: Option<&str>,
) -> AssertionView {
    let literal = |field: &str, value: &serde_json::Value| {
        if sensitive(field) {
            REDACTED.to_string()
        } else {
            literal_text(value)
        }
    };
    if let Some(unique) = assert.get("unique") {
        let mut text = format!("unique: key [{}]", names(unique.get("key")).join(", "));
        let determines = names(unique.get("determines"));
        if !determines.is_empty() {
            let _ = write!(text, ", determines [{}]", determines.join(", "));
        }
        return AssertionView {
            family: "unique".into(),
            text,
        };
    }
    if let Some(equal) = assert.get("equal") {
        let field = text_of(equal, "field");
        let to = equal.get("to");
        let group_by = field_name_or_list(equal.get("group_by"));
        let text = if let Some(row) = to
            .and_then(|to| to.get("row"))
            .and_then(serde_json::Value::as_object)
        {
            let row: Vec<String> = row
                .iter()
                .map(|(name, value)| format!("{name} = {}", literal(name, value)))
                .collect();
            format!(
                "equal: field {field}, to the source row where {}",
                row.join(" and ")
            )
        } else if let Some(other) = to
            .and_then(|to| to.get("field"))
            .and_then(serde_json::Value::as_str)
        {
            format!("equal: field {field}, to field {other} of the same row")
        } else if !group_by.is_empty() {
            format!(
                "equal: field {field}, within each group of {}",
                format_field_list(&group_by)
            )
        } else {
            format!("equal: field {field}, across every row")
        };
        return AssertionView {
            family: "equal".into(),
            text,
        };
    }
    if let Some(reference) = assert.get("reference") {
        let src = if let Some(field) = reference.get("field").and_then(serde_json::Value::as_str) {
            format!("field {field}")
        } else {
            format!("fields [{}]", names(reference.get("fields")).join(", "))
        };
        let to = reference.get("to").unwrap_or(&serde_json::Value::Null);
        let dst = if let Some(field) = to.get("field").and_then(serde_json::Value::as_str) {
            format!("field {field}")
        } else {
            format!("fields [{}]", names(to.get("fields")).join(", "))
        };
        let rel = target.unwrap_or_else(|| text_of(to, "relation"));
        let mut text = format!("reference: {src} in {dst} of relation {rel}");
        if reference
            .get("skip_missing")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            text.push_str(", skip_missing");
        }
        return AssertionView {
            family: "reference".into(),
            text,
        };
    }
    if let Some(count) = assert.get("count") {
        let group_by = field_name_or_list(count.get("group_by"));
        let mut text = if count.get("distinct").and_then(serde_json::Value::as_bool) == Some(true) {
            format!("count: distinct {}", text_of(count, "field"))
        } else {
            "count: rows".to_string()
        };
        if !group_by.is_empty() {
            let _ = write!(text, " per group of {}", format_field_list(&group_by));
        }
        for bound in ["min", "max", "exact"] {
            if let Some(n) = count.get(bound).and_then(serde_json::Value::as_u64) {
                let _ = write!(text, ", {bound} {n}");
            }
        }
        let groups: Vec<String> = count
            .get("groups")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .map(|group| show_group_literal(group, &group_by, sensitive))
            .collect();
        if !groups.is_empty() {
            let _ = write!(text, ", groups [{}]", groups.join(", "));
        }
        return AssertionView {
            family: "count".into(),
            text,
        };
    }
    if let Some(compare) = assert.get("compare") {
        let field = text_of(compare, "field");
        let op = text_of(compare, "op");
        let to = compare.get("to").unwrap_or(&serde_json::Value::Null);
        let mut text = if let Some(other) = to.get("field").and_then(serde_json::Value::as_str) {
            format!("compare: field {field} {op} field {other} of the same row")
        } else if let Some(val) = to.get("value") {
            format!("compare: field {field} {op} {}", literal(field, val))
        } else if let Some(row) = to.get("row").and_then(serde_json::Value::as_object) {
            let row: Vec<String> = row
                .iter()
                .map(|(name, value)| format!("{name} = {}", literal(name, value)))
                .collect();
            format!(
                "compare: field {field} {op} the source row where {}",
                row.join(" and ")
            )
        } else {
            format!("compare: field {field} {op}")
        };
        if let Some(order) = compare.get("order").and_then(serde_json::Value::as_str) {
            let _ = write!(text, ", order {order}");
        }
        return AssertionView {
            family: "compare".into(),
            text,
        };
    }
    if let Some(satisfies) = assert.get("satisfies") {
        let ver_field = satisfies.get("version").and_then(serde_json::Value::as_str);
        let rng_field = satisfies.get("range").and_then(serde_json::Value::as_str);
        let lhs = if let Some(field) = ver_field {
            format!("field {field}")
        } else if let Some(val) = satisfies.get("version_literal") {
            if rng_field.is_some_and(sensitive) {
                REDACTED.to_string()
            } else {
                literal_text(val)
            }
        } else {
            String::new()
        };
        let rhs = if let Some(field) = rng_field {
            format!("field {field}")
        } else if let Some(val) = satisfies.get("range_literal") {
            if ver_field.is_some_and(sensitive) {
                REDACTED.to_string()
            } else {
                literal_text(val)
            }
        } else {
            String::new()
        };
        let mut text = format!("satisfies: {lhs} in {rhs}");
        if let Some(dialect) = satisfies.get("dialect").and_then(serde_json::Value::as_str) {
            let _ = write!(text, ", dialect {dialect}");
        }
        if satisfies
            .get("include_prerelease")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            text.push_str(", include_prerelease");
        }
        return AssertionView {
            family: "satisfies".into(),
            text,
        };
    }
    let set = assert.get("set").unwrap_or(&serde_json::Value::Null);
    let field = text_of(set, "field");
    let group_by = field_name_or_list(set.get("group_by"));
    let mut text = format!("set: values of {field}");
    if !group_by.is_empty() {
        let _ = write!(text, " per group of {}", format_field_list(&group_by));
    }
    let relation = target;
    for mode in ["equal_to", "subset_of", "disjoint_from"] {
        let Some(target) = set.get(mode) else {
            continue;
        };
        if let Some(group) = target.get("group") {
            let shown = show_group_literal(group, &group_by, sensitive);
            let _ = write!(text, ", {mode} group {shown}");
        } else if let Some(values) = target.get("values").and_then(serde_json::Value::as_array) {
            let shown: Vec<String> = values.iter().map(|v| literal(field, v)).collect();
            let _ = write!(text, ", {mode} values [{}]", shown.join(", "));
        } else {
            let _ = write!(
                text,
                ", {mode} field {} of relation {}",
                text_of(target, "field"),
                relation.unwrap_or_else(|| text_of(target, "relation"))
            );
            let to_group_by = field_name_or_list(target.get("group_by"));
            if !to_group_by.is_empty() {
                let _ = write!(text, " per group of {}", format_field_list(&to_group_by));
            }
            if target
                .get("all_target_groups")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
            {
                text.push_str(", all_target_groups");
            }
        }
    }
    let groups: Vec<String> = set
        .get("groups")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .map(|group| show_group_literal(group, &group_by, sensitive))
        .collect();
    if !groups.is_empty() {
        let _ = write!(text, ", groups [{}]", groups.join(", "));
    }
    if set.get("plurals").and_then(serde_json::Value::as_str) == Some("cldr") {
        text.push_str(", plurals cldr");
    }
    AssertionView {
        family: "set".into(),
        text,
    }
}

/// The cost class (design section 8): every family reads each row once,
/// and a join reads its unique side into a table; a `set` with `equal_to:`
/// also lists the other set's values for each group, each one it lacks a
/// finding.
fn cost(assert: &serde_json::Value) -> String {
    let mut cost = "linear in the rows of each relation it reads".to_string();
    if assert.pointer("/set/equal_to").is_some() {
        cost.push_str(", plus the other set's values for each group (equal_to:)");
    }
    cost
}

/// What builds the views of one rule's plan.
struct Builder<'a> {
    plan: &'a Plan,
    root: &'a Path,
    redact: Redact<'a>,
    /// Each selector's scope, built once: a named selector a rule's
    /// relations share is shown with each, and its manifests read once.
    scopes: std::cell::RefCell<HashMap<TriggerOrigin, ScopeView>>,
    /// Each relation's place in the plan's `relations`, its `id` there.
    positions: HashMap<RelationId, usize>,
}

impl Builder<'_> {
    fn declared_in(&self, source: alint_core::relation::plan::SourceIndex) -> String {
        (self.redact)(&self.plan.source(source).display).into_owned()
    }

    /// Relation `id`'s place in the plan's `relations`.
    fn position(&self, id: RelationId) -> usize {
        self.positions[&id]
    }

    /// Each way the config writes a reference that reaches each relation
    /// of `order`, each once: the rule's own references first, then each
    /// reference a relation makes to those it reads, the relations that
    /// read others first; a relation no reference names (none) by its name.
    fn written(
        &self,
        order: &[RelationId],
        references: &[alint_core::relation::plan::RuleReference],
    ) -> HashMap<RelationId, Vec<String>> {
        let mut written: HashMap<RelationId, Vec<String>> = HashMap::with_capacity(order.len());
        let mut add = |id: RelationId, spelling: String| {
            let spellings = written.entry(id).or_default();
            if !spellings.contains(&spelling) {
                spellings.push(spelling);
            }
        };
        for reference in references {
            add(
                reference.relation.id,
                self.spelled(reference.relation.id, reference.relation.alias.as_deref()),
            );
        }
        for &id in order.iter().rev() {
            for read in self.plan.relation(id).relation.reads() {
                add(read.id, self.spelled(read.id, read.alias.as_deref()));
            }
        }
        for &id in order {
            written
                .entry(id)
                .or_insert_with(|| vec![self.plan.relation(id).name.clone()]);
        }
        written
    }

    /// Relation `id`'s name, qualified with `alias`.
    fn spelled(&self, id: RelationId, alias: Option<&str>) -> String {
        let name = &self.plan.relation(id).name;
        match alias {
            Some(alias) => format!("{alias}.{name}"),
            None => name.clone(),
        }
    }

    fn relation(
        &self,
        id: RelationId,
        written: Vec<String>,
        label: String,
        roles: Vec<&'static str>,
    ) -> RelationView {
        let named = self.plan.relation(id);
        let relation = &named.relation;
        let hidden = named.definition_reads_environment;
        let rows = match &relation.rows {
            Rows::Part(part) => RowsView::Part(Box::new(self.part(id, 0, part, named))),
            Rows::Union(parts) => RowsView::Union {
                union: parts
                    .iter()
                    .enumerate()
                    .map(|(index, part)| self.part(id, index, part, named))
                    .collect(),
            },
            Rows::Join(join) => RowsView::Join {
                join: JoinView {
                    left: self.position(join.left.id),
                    right: self.position(join.right.id),
                    on: join
                        .on
                        .iter()
                        .map(|(left, right)| OnPair {
                            left: left.clone(),
                            right: right.clone(),
                        })
                        .collect(),
                    unique: Shown::hidden_if(hidden, || match join.unique {
                        JoinSide::Left => "left",
                        JoinSide::Right => "right",
                    }),
                },
            },
        };
        RelationView {
            id: self.position(id),
            name: named.name.clone(),
            written,
            label,
            inline: named.is_inline(),
            roles,
            declared_in: self.declared_in(named.source),
            rows,
            fields: fields(relation, hidden),
            filters: self.filters(relation),
        }
    }

    fn part(
        &self,
        id: RelationId,
        index: usize,
        part: &alint_core::relation::plan::Part,
        named: &NamedRelation,
    ) -> PartView {
        let hidden = named.definition_reads_environment;
        let from = self.selector(
            &part.from,
            TriggerOrigin::Inline {
                relation: id,
                part: index,
            },
            named,
        );
        let part_fields = part.fields.as_ref().map(|defs| {
            defs.iter()
                .map(|(name, field)| field_view(name, field, &[], hidden))
                .collect()
        });
        match &part.reader {
            Reader::Entries(entries) => PartView {
                from,
                entries: Some(Shown::hidden_if(hidden, || EntriesView {
                    format: format_name(entries.format),
                    at: entries.at.clone(),
                    leaves: entries.leaves,
                    strict: entries.strict,
                })),
                records: None,
                read: None,
                optional: None,
                fields: part_fields,
            },
            Reader::Records(records) => PartView {
                from,
                entries: None,
                records: Some(Shown::hidden_if(hidden, || RecordsView {
                    format: format_name(records.format),
                    at: records.at.clone(),
                    strict: records.strict,
                })),
                read: None,
                optional: None,
                fields: part_fields,
            },
            Reader::Read { read, optional } => PartView {
                from,
                entries: None,
                records: None,
                read: Some(Shown::hidden_if(hidden, || read_json(read))),
                optional: Some(Shown::hidden_if(hidden, || *optional)),
                fields: part_fields,
            },
        }
    }

    /// A relation's `from:` selector, in relation `named`; `inline` is where
    /// an inline one sits.
    fn selector(
        &self,
        from: &SelectorRef,
        inline: TriggerOrigin,
        named: &NamedRelation,
    ) -> SelectorView {
        match from {
            SelectorRef::Named(reached) => {
                let selector = self.plan.selector(reached.id);
                let name = &selector.name;
                SelectorView {
                    selector: Some(match &reached.alias {
                        Some(alias) => format!("{alias}.{name}"),
                        None => name.clone(),
                    }),
                    relation: None,
                    part: None,
                    declared_in: self.declared_in(selector.source),
                    scope: self.scope(TriggerOrigin::Named(reached.id)),
                }
            }
            SelectorRef::Inline(_) => SelectorView {
                selector: None,
                relation: None,
                part: None,
                declared_in: self.declared_in(named.source),
                scope: self.scope(inline),
            },
        }
    }

    /// The scope of the selector at `origin`, built once.
    fn scope(&self, origin: TriggerOrigin) -> ScopeView {
        if let Some(scope) = self.scopes.borrow().get(&origin) {
            return scope.clone();
        }
        let (definition, name, hidden) = match origin {
            TriggerOrigin::Named(id) => {
                let selector = self.plan.selector(id);
                (
                    Some(&selector.selector),
                    selector.name.clone(),
                    selector.reads_environment,
                )
            }
            TriggerOrigin::Inline { relation, part } => {
                let named = self.plan.relation(relation);
                let definition = match &named.relation.rows {
                    Rows::Part(held) => Some(&held.from),
                    Rows::Union(parts) => parts.get(part).map(|held| &held.from),
                    Rows::Join(_) => None,
                }
                .and_then(|from| match from {
                    SelectorRef::Inline(selector) => Some(selector.as_ref()),
                    SelectorRef::Named(_) => None,
                });
                (
                    definition,
                    format!("the selector of relation `{}`", named.name),
                    named.definition_reads_environment,
                )
            }
        };
        let scope = scope_view(definition, &name, hidden, self.root);
        self.scopes.borrow_mut().insert(origin, scope.clone());
        scope
    }

    fn trigger(&self, origin: TriggerOrigin) -> SelectorView {
        match origin {
            TriggerOrigin::Named(id) => {
                let selector = self.plan.selector(id);
                SelectorView {
                    selector: Some(selector.name.clone()),
                    relation: None,
                    part: None,
                    declared_in: self.declared_in(selector.source),
                    scope: self.scope(origin),
                }
            }
            TriggerOrigin::Inline { relation, part } => {
                let named = self.plan.relation(relation);
                SelectorView {
                    selector: None,
                    relation: Some(self.position(relation)),
                    part: matches!(named.relation.rows, Rows::Union(_)).then_some(part),
                    declared_in: self.declared_in(named.source),
                    scope: self.scope(origin),
                }
            }
        }
    }

    fn filters(&self, relation: &RelationDef) -> Vec<FilterView> {
        let mut filters = Vec::new();
        for (field, predicates) in &relation.filters {
            let hidden = relation.field(field).is_some_and(|field| field.sensitive);
            for predicate in predicates {
                let (name, members, pattern) = match predicate {
                    Predicate::In(members) => ("in", Some(members), None),
                    Predicate::NotIn(members) => ("not_in", Some(members), None),
                    Predicate::Matches(pattern) => ("matches", None, Some(pattern)),
                    Predicate::NotMatches(pattern) => ("not_matches", None, Some(pattern)),
                };
                filters.push(FilterView {
                    field: field.clone(),
                    predicate: name,
                    members: members.map(|members| {
                        members
                            .iter()
                            .map(|member| self.member(member, hidden))
                            .collect()
                    }),
                    pattern: pattern.map(|pattern| Shown::hidden_if(hidden, || pattern.clone())),
                });
            }
        }
        filters
    }

    fn member(&self, member: &Member, hidden: bool) -> serde_json::Value {
        match member {
            Member::Literal(_) if hidden => serde_json::Value::from(REDACTED),
            Member::Literal(Value::Bool(flag)) => serde_json::Value::Bool(*flag),
            Member::Literal(Value::String(text)) => serde_json::Value::String(text.to_string()),
            // The loader takes a string or a boolean only.
            Member::Literal(_) => serde_json::Value::from(REDACTED),
            Member::Relation { relation, field } => serde_json::json!({
                "relation": self.position(relation.id),
                "field": field,
            }),
        }
    }
}

/// A selector's scope: `definition` (none when the plan has no such
/// selector), named `name` in an error, with every value hidden when
/// `hidden`.
fn scope_view(
    definition: Option<&SelectorDef>,
    name: &str,
    hidden: bool,
    root: &Path,
) -> ScopeView {
    match definition {
        Some(definition) if !hidden => {
            let (include, exclude) = definition.paths.include_exclude();
            ScopeView {
                paths: Shown::Value(PathsView { include, exclude }),
                scope_filter: definition
                    .scope_filter
                    .as_ref()
                    .map(|spec| Shown::Value(ScopeFilterView::of(name, spec, root))),
            }
        }
        _ => ScopeView {
            paths: Shown::Redacted(REDACTED),
            scope_filter: definition
                .and_then(|definition| definition.scope_filter.as_ref())
                .map(|_| Shown::Redacted(REDACTED)),
        },
    }
}

/// A `read:` as the config writes it.
fn read_json(read: &Read) -> serde_json::Value {
    match read {
        Read::Json {
            query,
            strict: true,
        } => serde_json::json!({ "json": query, "strict": true }),
        Read::Json {
            query,
            strict: false,
        } => serde_json::json!({ "json": query }),
        Read::Yaml(query) => serde_json::json!({ "yaml": query }),
        Read::Toml(query) => serde_json::json!({ "toml": query }),
        Read::Regex(pattern) => serde_json::json!({ "regex": pattern }),
        Read::Lines { comment: None } => serde_json::json!({ "lines": {} }),
        Read::Lines {
            comment: Some(comment),
        } => serde_json::json!({ "lines": { "comment": comment } }),
        Read::WholeFile => serde_json::json!({ "whole_file": {} }),
    }
}

fn format_name(format: EntriesFormat) -> &'static str {
    match format {
        EntriesFormat::Json => "json",
        EntriesFormat::Yaml => "yaml",
        EntriesFormat::Toml => "toml",
    }
}

fn fields(relation: &RelationDef, hidden: bool) -> Vec<FieldView> {
    let per_part = relation.uses_part_fields();
    relation
        .fields
        .iter()
        .map(|(name, field)| {
            if per_part {
                FieldView {
                    name: name.clone(),
                    source: "per part".to_string(),
                    optional: None,
                    on_missing: None,
                    transforms: Shown::hidden_if(
                        field.sensitive && !relation.transforms_of(name).is_empty(),
                        || {
                            relation
                                .transforms_of(name)
                                .iter()
                                .map(transform_text)
                                .collect()
                        },
                    ),
                    sensitive: field.sensitive,
                }
            } else {
                field_view(name, field, relation.transforms_of(name), hidden)
            }
        })
        .collect()
}

fn field_view(
    name: &str,
    field: &alint_core::relation::plan::FieldDef,
    transforms: &[Transform],
    hidden: bool,
) -> FieldView {
    let (optional, on_missing) = match &field.source {
        FieldSource::Bound {
            optional: true,
            on_missing,
            ..
        } if !hidden => (
            Some(true),
            Some(match on_missing {
                alint_core::relation::plan::BoundMissing::Skip => "skip",
                alint_core::relation::plan::BoundMissing::Missing => "missing",
            }),
        ),
        FieldSource::Record { optional: true, .. } if !hidden => (Some(true), None),
        _ => (None, None),
    };
    FieldView {
        name: name.to_string(),
        source: field_source(&field.source, hidden),
        optional,
        on_missing,
        // Whether a field has transforms is written literally; which
        // ones a sensitive field has may be the environment's.
        transforms: Shown::hidden_if(field.sensitive && !transforms.is_empty(), || {
            transforms.iter().map(transform_text).collect()
        }),
        sensitive: field.sensitive,
    }
}

/// A field's source as the config writes it, with `bound:` and `record:` paths
/// hidden when `hidden`: the only parts of a source that can read the
/// environment.
fn field_source(source: &FieldSource, hidden: bool) -> String {
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
        FieldSource::EntryKey => "entry.key".into(),
        FieldSource::EntryValue => "entry.value".into(),
        FieldSource::EntryParent => "entry.parent".into(),
        FieldSource::EntryRoot => "entry.root".into(),
        FieldSource::EntryPath => "entry.path".into(),
        FieldSource::Value => "value".into(),
        FieldSource::Bound { query, .. } => {
            if hidden {
                format!("bound: {REDACTED}")
            } else {
                format!("bound: {query}")
            }
        }
        FieldSource::Record { query, .. } => {
            if hidden {
                format!("record: {REDACTED}")
            } else {
                format!("record: {query}")
            }
        }
        FieldSource::Side(JoinSide::Left, field) => format!("left.{field}"),
        FieldSource::Side(JoinSide::Right, field) => format!("right.{field}"),
    }
}

fn transform_text(transform: &Transform) -> String {
    match transform {
        Transform::Trim => "trim".into(),
        Transform::Lower => "lower".into(),
        Transform::SemverMajor => "semver-major".into(),
        Transform::SemverMinor => "semver-minor".into(),
        Transform::StripRoot => "strip-root".into(),
        Transform::Split { on, part, required } => {
            let on = serde_json::Value::from(on.as_str());
            if *required {
                format!("split(on: {on}, part: {part}, required: true)")
            } else {
                format!("split(on: {on}, part: {part})")
            }
        }
        Transform::StripPrefix(prefix) => {
            format!("strip_prefix({})", serde_json::Value::from(prefix.as_str()))
        }
        Transform::StripSuffix(suffixes) => match suffixes.as_slice() {
            [one] => format!("strip_suffix({})", serde_json::Value::from(one.as_str())),
            many => format!(
                "strip_suffix([{}])",
                many.iter()
                    .map(|s| serde_json::Value::from(s.as_str()).to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        },
        Transform::Prefix(prefix) => {
            format!("prefix({})", serde_json::Value::from(prefix.as_str()))
        }
        Transform::Suffix(suffix) => {
            format!("suffix({})", serde_json::Value::from(suffix.as_str()))
        }
    }
}

/// Human output: the `plan:` block that ends `explain`. `reads_ignore_files`
/// is whether the walk reads `.gitignore` and `.ignore` files (the config's
/// `respect_gitignore:`, unless `--no-gitignore`), so that a change to one
/// runs the rule.
pub(super) fn write_human(
    out: &mut impl Write,
    plan: &ExplainPlan,
    reads_ignore_files: bool,
) -> Result<()> {
    let dim = alint_output::style::DIM;
    let plan = match plan {
        Shown::Redacted(marker) => {
            writeln!(out, "{dim}plan:      {dim:#} {marker}")?;
            return Ok(());
        }
        Shown::Value(plan) => plan,
    };
    writeln!(out, "{dim}plan:{dim:#}")?;
    let assertion = match &plan.assertion {
        Shown::Value(assertion) => assertion.text.as_str(),
        Shown::Redacted(marker) => marker,
    };
    writeln!(out, "  {dim}assert:{dim:#} {}", term(assertion))?;
    writeln!(out, "  {dim}relations, in evaluation order:{dim:#}")?;
    let labels: Vec<&str> = plan
        .relations
        .iter()
        .map(|relation| relation.label.as_str())
        .collect();
    for relation in &plan.relations {
        write_relation(out, relation, &labels)?;
    }
    // What a change must touch to run the rule (`TriggerScope::hits`): a
    // path a selector picks; an ignore file (`.gitignore`, `.ignore`) the walk
    // reads, or a directory Git lists whole (a submodule, a nested
    // repository), where its `paths:` can match; a `has_ancestor` marker or a
    // manifest its `scope_filter:` consults.
    let ignore_file = if reads_ignore_files {
        "an ignore file, "
    } else {
        "a "
    };
    writeln!(
        out,
        "  {dim}trigger:{dim:#} under --changed, the rule runs on a change to a path these \
         selectors pick, to {ignore_file}submodule or nested repository where they can \
         match, or to a marker or manifest their scope_filter: consults"
    )?;
    for selector in &plan.trigger {
        write!(out, "    ")?;
        write_selector(out, "    ", selector, &labels)?;
    }
    writeln!(out, "  {dim}cost:{dim:#} {}", plan.cost)?;
    Ok(())
}

/// Relation `relation`, with each relation it refers to called by its label
/// in `labels` (by `id`).
fn write_relation(out: &mut impl Write, relation: &RelationView, labels: &[&str]) -> Result<()> {
    let dim = alint_output::style::DIM;
    write_relation_heading(out, relation)?;
    match &relation.rows {
        RowsView::Part(part) => write_part(out, "      ", part, labels)?,
        RowsView::Union { union } => {
            // Each part by its place in `union:`, from 0, as a message that
            // names a part does (`relations.pins.union[2]`).
            for (index, part) in union.iter().enumerate() {
                writeln!(out, "      {dim}union[{index}]:{dim:#}")?;
                write_part(out, "        ", part, labels)?;
            }
        }
        RowsView::Join { join } => {
            let on: Vec<String> = join
                .on
                .iter()
                .map(|pair| format!("{} = {}", pair.left, pair.right))
                .collect();
            let unique = match &join.unique {
                Shown::Value(side) => side,
                Shown::Redacted(marker) => marker,
            };
            writeln!(
                out,
                "      {dim}join:{dim:#} left {}, right {}, on {}, unique {unique}",
                term(labels[join.left]),
                term(labels[join.right]),
                term(&on.join(" and "))
            )?;
        }
    }
    write_fields(out, &relation.fields)?;
    if !relation.filters.is_empty() {
        writeln!(out, "      {dim}where:{dim:#}")?;
        for filter in &relation.filters {
            let test = match (&filter.members, &filter.pattern) {
                (Some(members), _) => {
                    let members: Vec<String> = members
                        .iter()
                        .map(|member| member_text(member, labels))
                        .collect();
                    format!("[{}]", members.join(", "))
                }
                (None, Some(Shown::Value(pattern))) => {
                    serde_json::Value::from(pattern.as_str()).to_string()
                }
                _ => REDACTED.to_string(),
            };
            writeln!(
                out,
                "        {} {} {}",
                term(&filter.field),
                filter.predicate,
                term(&test)
            )?;
        }
    }
    Ok(())
}

/// A relation's heading: its label, the file that declares it, its roles,
/// and each spelling when the config reaches it by more than one.
fn write_relation_heading(out: &mut impl Write, relation: &RelationView) -> Result<()> {
    let dim = alint_output::style::DIM;
    let mut marks: Vec<&str> = Vec::new();
    for role in &relation.roles {
        marks.push(match (*role, relation.inline) {
            ("input", true) => "inline input",
            ("input", false) => "input",
            ("reference_target", _) => "reference target",
            _ => "set target",
        });
    }
    let marks = if marks.is_empty() {
        String::new()
    } else {
        format!("  ({})", marks.join(", "))
    };
    writeln!(
        out,
        "    relation {}  {dim}{}{dim:#}{marks}",
        term(&relation.label),
        term(&relation.declared_in)
    )?;
    // A relation the config reaches by more than one spelling lists each.
    if relation.written.len() > 1 {
        writeln!(
            out,
            "      {dim}written:{dim:#} {}",
            term(&relation.written.join(", "))
        )?;
    }
    Ok(())
}

/// A relation's fields, with their sources, transforms and sensitivity.
fn write_fields(out: &mut impl Write, fields: &[FieldView]) -> Result<()> {
    let dim = alint_output::style::DIM;
    writeln!(out, "      {dim}fields:{dim:#}")?;
    for field in fields {
        let mut notes = Vec::new();
        if field.optional == Some(true) {
            match field.on_missing {
                Some("missing") => notes.push("optional, on_missing: missing".into()),
                _ => notes.push("optional".into()),
            }
        }
        match &field.transforms {
            Shown::Value(transforms) if transforms.is_empty() => {}
            Shown::Value(transforms) => notes.push(format!("transform: {}", transforms.join(", "))),
            Shown::Redacted(marker) => notes.push(format!("transform: {marker}")),
        }
        if field.sensitive {
            notes.push("sensitive".into());
        }
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!("  ({})", notes.join("; "))
        };
        writeln!(
            out,
            "        {}: {}{}",
            term(&field.name),
            term(&field.source),
            term(&notes)
        )?;
    }
    Ok(())
}

/// An `in:` or `not_in:` member: a literal, or a reference to a relation
/// called by its label in `labels`.
fn member_text(member: &serde_json::Value, labels: &[&str]) -> String {
    match member {
        serde_json::Value::Object(reference) => format!(
            "{{ relation: {}, field: {} }}",
            reference
                .get("relation")
                .and_then(serde_json::Value::as_u64)
                .and_then(|id| labels.get(usize::try_from(id).ok()?))
                .copied()
                .unwrap_or_default(),
            reference
                .get("field")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
        ),
        other => literal_text(other),
    }
}

fn write_part(out: &mut impl Write, indent: &str, part: &PartView, labels: &[&str]) -> Result<()> {
    let dim = alint_output::style::DIM;
    write!(out, "{indent}{dim}from:{dim:#} ")?;
    write_selector(out, indent, &part.from, labels)?;
    if let Some(entries) = &part.entries {
        let entries = match entries {
            Shown::Value(entries) => {
                let mut text = format!("{} at {}", entries.format, entries.at.join(", "));
                if entries.leaves {
                    text.push_str(", leaves");
                }
                if entries.strict {
                    text.push_str(", strict");
                }
                text
            }
            Shown::Redacted(marker) => (*marker).to_string(),
        };
        writeln!(out, "{indent}{dim}entries:{dim:#} {}", term(&entries))?;
    }
    if let Some(records) = &part.records {
        let records = match records {
            Shown::Value(records) => {
                let mut text = format!("{} at {}", records.format, records.at.join(", "));
                if records.strict {
                    text.push_str(", strict");
                }
                text
            }
            Shown::Redacted(marker) => (*marker).to_string(),
        };
        writeln!(out, "{indent}{dim}records:{dim:#} {}", term(&records))?;
    }
    if let Some(read) = &part.read {
        let mut text = match read {
            Shown::Value(read) => read_text(read),
            Shown::Redacted(marker) => (*marker).to_string(),
        };
        match &part.optional {
            Some(Shown::Value(true)) => text.push_str(", optional"),
            Some(Shown::Redacted(marker)) => {
                let _ = write!(text, ", optional: {marker}");
            }
            _ => {}
        }
        writeln!(out, "{indent}{dim}read:{dim:#} {}", term(&text))?;
    }
    if let Some(part_fields) = &part.fields {
        writeln!(out, "{indent}{dim}fields:{dim:#}")?;
        for field in part_fields {
            let mut notes: Vec<&str> = Vec::new();
            if field.optional == Some(true) {
                match field.on_missing {
                    Some("missing") => notes.push("optional, on_missing: missing"),
                    _ => notes.push("optional"),
                }
            }
            if field.sensitive {
                notes.push("sensitive");
            }
            let notes = if notes.is_empty() {
                String::new()
            } else {
                format!("  ({})", notes.join("; "))
            };
            writeln!(
                out,
                "{indent}  {}: {}{}",
                term(&field.name),
                term(&field.source),
                term(&notes)
            )?;
        }
    }
    Ok(())
}

/// A `read:` in readable form: `json $.name`, `json $.name, strict`, `regex "^v(.+)$"`, `lines`.
fn read_text(read: &serde_json::Value) -> String {
    let Some(map) = read.as_object() else {
        return String::new();
    };
    let Some((form, argument)) = map.iter().find(|(k, _)| k.as_str() != "strict") else {
        return String::new();
    };
    let mut text = match argument {
        serde_json::Value::String(text) if form == "regex" => {
            format!("regex {}", serde_json::Value::from(text.as_str()))
        }
        serde_json::Value::String(text) => format!("{form} {text}"),
        serde_json::Value::Object(options) => match options.get("comment") {
            Some(comment) => format!("{form}, comment {comment}"),
            None => form.clone(),
        },
        _ => form.clone(),
    };
    if map.get("strict").and_then(serde_json::Value::as_bool) == Some(true) {
        text.push_str(", strict");
    }
    text
}

/// A selector: its name (or `inline selector`) and source on this line,
/// then its `paths:` and `scope_filter:` below, `indent` deeper; the
/// relation an inline one in the trigger is part of is called by its label
/// in `labels`.
fn write_selector(
    out: &mut impl Write,
    indent: &str,
    selector: &SelectorView,
    labels: &[&str],
) -> Result<()> {
    let dim = alint_output::style::DIM;
    match (&selector.selector, selector.relation) {
        (Some(name), _) => write!(out, "selector {}", term(name))?,
        (None, Some(relation)) => write!(
            out,
            "the inline selector of relation {}{}",
            term(labels[relation]),
            selector
                .part
                .map(|part| format!(", union[{part}]"))
                .unwrap_or_default()
        )?,
        (None, None) => write!(out, "inline selector")?,
    }
    writeln!(out, "  {dim}{}{dim:#}", term(&selector.declared_in))?;
    let deeper = format!("{indent}  ");
    let paths = match &selector.scope.paths {
        Shown::Value(paths) => {
            let mut text = if paths.include.is_empty() {
                "**".to_string()
            } else {
                paths.include.join(", ")
            };
            if !paths.exclude.is_empty() {
                let _ = write!(text, "  (excluding {})", paths.exclude.join(", "));
            }
            text
        }
        Shown::Redacted(marker) => (*marker).to_string(),
    };
    writeln!(out, "{deeper}{dim}paths:{dim:#} {}", term(&paths))?;
    match &selector.scope.scope_filter {
        Some(Shown::Value(filter)) => {
            writeln!(out, "{deeper}{dim}scope_filter:{dim:#}")?;
            filter.write_human(out, &format!("{deeper}  "))?;
        }
        Some(Shown::Redacted(marker)) => {
            writeln!(out, "{deeper}{dim}scope_filter:{dim:#} {marker}")?;
        }
        None => {}
    }
    Ok(())
}
