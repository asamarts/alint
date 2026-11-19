//! `constraint`: an assertion over the rows of a relation (design
//! `v0.23/selectors-and-constraints.md` section 6, ADR-0034). A version 2
//! kind, experimental as version 2 is: a config of `version: 1` cannot use
//! it, and it cannot be nested under a `for_each_*` kind's `require:`.
//!
//! It takes one relation as `input:` (a name, or an inline relation, which
//! the loader plans as a named one) and one assertion:
//!
//! - **`equal`:** rows agree on `field:` within each group (`group_by:`,
//!   optional), with one named source row (`to: { row: { F: literal } }`), or
//!   with another field of the same row (`to: { field: G }`);
//! - **`unique`:** no two rows share `key:`; with `determines:`, rows that
//!   share `key:` agree on the determined fields;
//! - **`set`:** the set of `field:`'s values in each group (`group_by:`, or
//!   the relation as one group) is equal to, a subset of or disjoint from
//!   another set: a reference group of the same relation (`{ group: literal
//!   }`), or every value of another relation's field (`{ relation: R, field:
//!   F }`). The groups are every value of `group_by:`, every resource the
//!   relation reads when it groups by `path` or a path token, and every
//!   literal of `groups:`; `plurals: cldr` collapses CLDR plural groups of
//!   an `entry.path` field against a reference group, as `key_parity` does.
//!
//! The engine evaluates the rule's relations (its input, and the relation
//! its `set` compares against) before rules run, in plan order; the rule
//! reads their outcomes from the run's memo. A relation that fails gives the
//! rule its analysis failures, never a pass. Every finding is
//! identified by its rule and its key alone, never its path
//! ([`alint_core::Violation::path_free_identity`]): it sits at a key's first
//! row or a group's first resource, which moves when rows are added or
//! renamed.
//!
//! The kind needs a planned relation, which only the loader makes, so its
//! tests are in `alint-dsl` (`src/tests/constraint.rs` for the builder and
//! `src/tests/constraint_families.rs` for the families), in
//! `crates/alint/tests/constraint_cli.rs` and in the e2e scenarios.

mod check;
mod families;
mod options;
mod places;
mod values;

use std::collections::HashSet;
use std::sync::Arc;

use alint_core::relation::eval::{self, RelationNote, Skipped};
use alint_core::relation::plan::{PathToken, Plan, RelationId};
use alint_core::relation::{Resource, StringTransform, Value};
use alint_core::{AnalysisFailureKind, Context, Error, Level, Result, Rule, RuleSpec, Violation};

pub use options::options_schema;

/// The lowest config `version:` that may use the kind.
pub const MIN_CONFIG_VERSION: u32 = 2;

/// What the assertion compares, its fields checked against the input
/// relation.
#[derive(Debug, Clone)]
enum Assertion {
    /// `unique`, with `determines:` when it is not empty.
    Unique {
        key: Vec<String>,
        determines: Vec<String>,
    },
    /// `equal` within groups (`group_by:` or the relation).
    EqualGroups {
        field: String,
        group_by: Option<String>,
    },
    /// `equal` against the one row whose fields hold these literals.
    EqualSource {
        field: String,
        row: Vec<(String, Value)>,
    },
    /// `equal` within each row.
    EqualField { field: String, other: String },
    /// `set`.
    Set(Box<SetAssertion>),
}

/// A `set` assertion: each group's set of `field`'s values against `target`.
#[derive(Debug, Clone)]
struct SetAssertion {
    field: String,
    /// The group of each row; without it, the relation is one group.
    group_by: Option<String>,
    /// When `group_by:` is `path` or a path token: every resource the
    /// relation reads forms its group, rows or not.
    resource_groups: Option<ResourceGroups>,
    target: SetTarget,
    mode: SetMode,
    /// `groups:`: the groups that must exist, rows or not.
    groups: Vec<Value>,
    /// `plurals: cldr`: CLDR plural groups of the key paths merge.
    plurals: bool,
}

/// The set each group is compared with.
#[derive(Debug, Clone)]
enum SetTarget {
    /// The reference group: the rows whose `group_by:` value is this.
    Group(Value),
    /// Every value of `field` across the rows of `relation`, which the
    /// config names as `written` (`names`, or `alias.names`).
    Relation {
        relation: RelationId,
        field: String,
        written: String,
    },
}

/// How a `set` grouped by a path token gives a resource its group: the
/// token's text, then the transforms the group field's values took, in
/// order, as the resource's rows have it.
#[derive(Debug, Clone)]
struct ResourceGroups {
    token: PathToken,
    transforms: Vec<StringTransform>,
}

impl ResourceGroups {
    fn group_of(&self, resource: &Resource) -> String {
        let mut text = eval::path_token(resource, self.token);
        for transform in &self.transforms {
            text = transform.apply(&text);
        }
        text
    }
}

/// How each group's set relates to the reference group's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SetMode {
    EqualTo,
    SubsetOf,
    DisjointFrom,
}

impl SetMode {
    fn key(self) -> &'static str {
        match self {
            Self::EqualTo => "equal_to",
            Self::SubsetOf => "subset_of",
            Self::DisjointFrom => "disjoint_from",
        }
    }
}

#[derive(Debug)]
pub struct ConstraintRule {
    id: String,
    level: Level,
    policy_url: Option<String>,
    message: Option<String>,
    plan: Arc<Plan>,
    /// The relations the rule reads, as [`Rule::relation_inputs`] lists
    /// them: its input, then the relation its `set` compares against, when
    /// that is another.
    inputs: Vec<RelationId>,
    assertion: Assertion,
    /// The assertion's literals came from the environment, so no message
    /// quotes them.
    hide_literals: bool,
}

pub fn build(spec: &RuleSpec) -> Result<Box<dyn Rule>> {
    Ok(Box::new(build_rule(spec)?))
}

fn build_rule(spec: &RuleSpec) -> Result<ConstraintRule> {
    let cfg = |message: String| Error::rule_config(&spec.id, message);
    let options: options::Options = spec
        .deserialize_options()
        .map_err(|error| cfg(format!("invalid `constraint` options: {error}")))?;
    let (plan, input) = input_relation(spec)?;
    let references = plan.rule(&spec.id).map_or(&[][..], |rule| &rule.references);
    let checker = check::Checker::new(&plan, input, &spec.id, references);
    let assertion = checker.assertion(&options.assert)?;
    if let Some(message) = &spec.message {
        checker.message(message)?;
    }
    let mut inputs = vec![input];
    if let Assertion::Set(set) = &assertion
        && let SetTarget::Relation { relation, .. } = &set.target
        && *relation != input
    {
        inputs.push(*relation);
    }
    Ok(ConstraintRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        plan,
        inputs,
        assertion,
        hide_literals: spec.field_is_environment_derived("assert"),
    })
}

/// The plan the rule's `input:` resolved in, and the relation it names.
fn input_relation(spec: &RuleSpec) -> Result<(Arc<Plan>, RelationId)> {
    let resolved = spec.relation_plan.as_ref().and_then(|plan| {
        let reference = plan
            .rule(&spec.id)?
            .references
            .iter()
            .find(|reference| reference.field == "input")?;
        Some((Arc::clone(plan), reference.relation.id))
    });
    resolved.ok_or_else(|| {
        Error::rule_config(
            &spec.id,
            format!(
                "a `constraint` rule reads a relation, which only a config of `version: \
                 {MIN_CONFIG_VERSION}` declares (the kind is experimental); a version 1 \
                 config, a rule nested under `require:` and a rule built outside a \
                 configuration have none"
            ),
        )
    })
}

/// How a message names relation `id`: ``relation `deps` (.alint.yml)``, or
/// ``the inline relation at `rules[0].input` (.alint.yml)``.
fn relation_label(plan: &Plan, id: RelationId) -> String {
    let named = plan.relation(id);
    let display = &plan.source(named.source).display;
    if named.is_inline() {
        format!("the inline relation at `{}` ({display})", named.name)
    } else {
        format!("relation `{}` ({display})", named.name)
    }
}

impl Rule for ConstraintRule {
    alint_core::rule_common_impl!();

    fn uses_relations(&self) -> bool {
        true
    }

    fn requires_full_index(&self) -> bool {
        true
    }

    fn supports_expect_matches(&self) -> bool {
        false
    }

    fn relation_inputs(&self) -> Option<(&Arc<Plan>, &[RelationId])> {
        Some((&self.plan, &self.inputs))
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        // A relation the rule reads that fails leaves it without a verdict:
        // its failures, every relation's, and nothing else.
        let mut outcomes = Vec::with_capacity(self.inputs.len());
        let mut failed = Vec::new();
        for &id in &self.inputs {
            match eval::evaluate(&self.plan, id, ctx) {
                Ok(rows) => outcomes.push(rows),
                Err(failures) => failed.extend(
                    failures
                        .iter()
                        .map(|failure| failure.to_violation(&self.id)),
                ),
            }
        }
        if !failed.is_empty() {
            return Ok(failed);
        }
        let rows = &outcomes[0];
        // What the relation a `set` compares against skipped is a value its
        // set lacks, as a `where:` list's relation's is a row it lacks. The
        // input may hold the same notes, taken from it through a list or a
        // join: each is reported once.
        let mut seen = HashSet::new();
        let mut notes = Vec::new();
        for outcome in &outcomes {
            notes.extend(
                outcome
                    .notes
                    .iter()
                    .filter(|note| seen.insert(note.key()))
                    .cloned(),
            );
        }
        let (mut reported, skipped) = Self::notes(&notes);
        let mut run = families::Run::new(self, rows, ctx);
        match &self.assertion {
            Assertion::Unique { key, determines } => run.unique(key, determines),
            Assertion::EqualGroups { field, group_by } => {
                run.equal_groups(field, group_by.as_deref());
            }
            Assertion::EqualSource { field, row } => run.equal_source(field, row),
            Assertion::EqualField { field, other } => run.equal_field(field, other),
            Assertion::Set(set) => {
                let other = match &set.target {
                    SetTarget::Relation { relation, .. } => {
                        let at = self.inputs.iter().position(|id| id == relation);
                        &outcomes[at.expect("the builder lists the relation as an input")]
                    }
                    SetTarget::Group(_) => rows,
                };
                // The resources a group may sit at: those `where:` may keep
                // a row of, as their paths alone decide.
                let kept = (set.group_by.is_none() || set.resource_groups.is_some())
                    .then(|| eval::resources_kept(&self.plan, self.inputs[0], rows, ctx));
                run.set(set, other, kept.as_deref());
            }
        }
        reported.extend(run.finish(skipped));
        Ok(reported)
    }
}

impl ConstraintRule {
    /// What the relation's `read:` parts skipped, as `cross_file` reports it
    /// (design section 5): the notes, one per computed value (`${VAR}`); and
    /// the findings, one per resource and part whose values are not strings,
    /// naming their kinds, which an assertion with no verdict withholds with
    /// its own.
    fn notes(skipped: &[RelationNote]) -> (Vec<Violation>, Vec<Violation>) {
        let (mut notes, mut findings) = (Vec::new(), Vec::new());
        for note in skipped {
            let path = note.resource.path().as_path();
            let message = format!("{}: {}", crate::slash(path), note.message);
            match &note.skipped {
                Skipped::NonLiteral => {
                    notes.push(Violation::new(message).with_path(path).as_note());
                }
                Skipped::NonString { part, kinds } => {
                    let mut key = values::Key::new("non-string");
                    key.part(note.resource.path().as_str())
                        .part(&part.to_string());
                    for (kind, count) in kinds {
                        key.part(kind.as_str()).part(&count.to_string());
                    }
                    findings.push(
                        Violation::new(message)
                            .with_path(path)
                            .with_baseline_key(key.into_string())
                            .with_path_free_identity()
                            .with_not_fixable(),
                    );
                }
            }
        }
        (notes, findings)
    }

    /// An analysis failure of the assertion: no verdict, never a pass.
    fn failure(&self, at: Option<&std::path::Path>, mut message: String) -> Violation {
        message.insert_str(0, &format!("{}: ", self.input_label()));
        let mut finding = Violation::new(message);
        if let Some(path) = at {
            finding = finding.with_path(path);
        }
        finding.as_analysis_failure(AnalysisFailureKind::Evaluation)
    }

    fn input_label(&self) -> String {
        relation_label(&self.plan, self.inputs[0])
    }
}

pub static META: alint_core::KindMeta = alint_core::KindMeta {
    dispatch: alint_core::DispatchClass::FullIndex { path_scope: false },
    paths: alint_core::PathsSupport::Forbidden,
    scope_filter: alint_core::ScopeFilterSupport::Rejected {
        reason: std::borrow::Cow::Borrowed(
            "a `constraint` rule takes no scope_filter: the selectors of its relation pick \
             what it reads",
        ),
    },
    git_tracked_only: alint_core::GitTrackedSupport::None,
    respect_gitignore: false,
    root_only: false,
    selectors: std::borrow::Cow::Borrowed(&[]),
    since: alint_core::SinceSupport::None,
    capabilities: std::borrow::Cow::Borrowed(&[]),
    content: alint_core::ContentAccess::OtherFiles,
    formats: std::borrow::Cow::Borrowed(&[
        alint_core::Format::Json,
        alint_core::Format::Yaml,
        alint_core::Format::Toml,
    ]),
    dependencies: std::borrow::Cow::Borrowed(&[]),
    fix_ops: std::borrow::Cow::Borrowed(&[]),
    expect_matches: false,
    trace: alint_core::TraceMeta::ASSERTION,
    nested_rules: false,
};
