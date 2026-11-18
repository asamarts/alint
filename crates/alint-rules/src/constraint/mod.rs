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
//! - **`set`:** the set of `field:`'s values in each group (`group_by:`) is
//!   equal to, a subset of or disjoint from the set of a reference group of
//!   the same relation (`{ group: literal }`).
//!
//! `plurals: cldr`, `groups:`, a `set` against another relation's field and
//! a `set` grouped by `path` or a path token come in a later step; the
//! builder refuses each until then.
//!
//! The engine evaluates the relation before rules run (in plan order); the
//! rule reads its outcome from the run's memo. A relation that fails gives
//! the rule its analysis failures, never a pass. Every finding is
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

use std::sync::Arc;

use alint_core::relation::Value;
use alint_core::relation::eval::{self, RelationRows, Skipped};
use alint_core::relation::plan::{Plan, RelationId};
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
    /// `set` against the group whose `group_by:` value is `group`.
    Set {
        field: String,
        group_by: String,
        group: Value,
        mode: SetMode,
    },
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
    /// The input relation, as [`Rule::relation_inputs`] lists it.
    input: [RelationId; 1],
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
    let assertion = check::Checker::new(&plan, input, &spec.id).assertion(&options.assert)?;
    if let Some(message) = &spec.message {
        check::Checker::new(&plan, input, &spec.id).message(message)?;
    }
    Ok(ConstraintRule {
        id: spec.id.clone(),
        level: spec.level,
        policy_url: spec.policy_url.clone(),
        message: spec.message.clone(),
        plan,
        input: [input],
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
        Some((&self.plan, &self.input))
    }

    fn evaluate(&self, ctx: &Context<'_>) -> Result<Vec<Violation>> {
        let rows = match eval::evaluate(&self.plan, self.input[0], ctx) {
            Ok(rows) => rows,
            Err(failures) => {
                return Ok(failures
                    .iter()
                    .map(|failure| failure.to_violation(&self.id))
                    .collect());
            }
        };
        let mut findings = Self::notes(&rows);
        let mut run = families::Run::new(self, &rows, ctx);
        match &self.assertion {
            Assertion::Unique { key, determines } => run.unique(key, determines),
            Assertion::EqualGroups { field, group_by } => {
                run.equal_groups(field, group_by.as_deref());
            }
            Assertion::EqualSource { field, row } => run.equal_source(field, row),
            Assertion::EqualField { field, other } => run.equal_field(field, other),
            Assertion::Set {
                field,
                group_by,
                group,
                mode,
            } => run.set(field, group_by, group, *mode),
        }
        findings.extend(run.finish());
        Ok(findings)
    }
}

impl ConstraintRule {
    /// What the relation's `read:` parts skipped: a computed value (`${VAR}`)
    /// as a note, and the values that are not strings as one finding per
    /// resource and part that names their kinds, as `cross_file` reports
    /// both (design section 5).
    fn notes(rows: &RelationRows) -> Vec<Violation> {
        let mut findings = Vec::new();
        for note in &rows.notes {
            let path = note.resource.path().as_path();
            let message = format!("{}: {}", crate::slash(path), note.message);
            match &note.skipped {
                Skipped::NonLiteral => {
                    findings.push(Violation::new(message).with_path(path).as_note());
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
        findings
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
        relation_label(&self.plan, self.input[0])
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
