use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::facts::FactSpec;
use crate::located_fix::LocatedOutcome;
use crate::registry::RuleRegistry;
use crate::report::{FixItem, FixReport, FixRuleResult, FixStatus};
use crate::rule::{
    AnalysisFailureKind, Applicability, Context, FindingDisposition, FixEdit, Fixer, Rule,
    RuleResult, Violation,
};
use crate::trace::{
    TraceCounts, TraceExtractionObservation, TraceOptions, TracePreview, TracePreviewCollector,
    TracePreviewRow, TraceReason, TraceRecorder, TraceSensitivity, TraceSink, TraceStage,
    TraceStageClass, TraceStatus,
};
use crate::walker::FileIndex;
use crate::when::{WhenEnv, WhenExpr};
mod fix;
mod gates;
mod run;

/// Run a parallel `job` inside an explicitly-built rayon pool, degrading gracefully
/// if the OS refuses worker threads. `par_iter`'s LAZY global-pool init PANICS ("The
/// global thread pool has not been initialized") when thread creation fails under
/// `RLIMIT_NPROC` / pids.max pressure (common in hardened CI / containers -- the
/// self-hosted runner has hit pids limits); a linter must degrade, not crash with
/// the "alint crashed" banner. Building an explicit pool avoids that lazy init.
///
/// The pool is built ONCE and cached, via a fallback chain: the default (`num_cpus`)
/// pool, else a single spawned thread, else a `use_current_thread` pool that spawns
/// ZERO new threads (runs on the caller). Caching is REQUIRED, not just an
/// optimization: a second `use_current_thread` build on the same thread fails ("the
/// current thread is already part of another thread pool") and its drop does not
/// release the thread in time, so a per-call build would panic at the SECOND
/// dispatch site under pressure -- exactly the crash this guards. One cached pool is
/// `install`d by both dispatch sites and every run. `install` only changes WHICH
/// pool executes; `par_iter().collect()` preserves order regardless, so results are
/// identical. (The HCL parse thread's spawn failure was hardened the same way; this
/// is the rayon analogue.)
///
/// Caveat: the cached pool is process-global. If the third tier (the current-thread
/// pool) is what builds, that pool is affine to whichever thread first initialized
/// it, so a later run from a different thread would `install` onto that thread. This
/// is unreachable for the CLI (a single main thread) and the per-file LSP path does
/// not use this helper; it can only arise once both threaded tiers have already
/// failed, and is the accepted trade for killing the real per-call crash.
fn with_worker_pool<R: Send>(job: impl FnOnce() -> R + Send) -> R {
    static POOL: std::sync::OnceLock<Option<rayon::ThreadPool>> = std::sync::OnceLock::new();
    match POOL
        .get_or_init(|| {
            rayon::ThreadPoolBuilder::new()
                .build()
                .or_else(|_| rayon::ThreadPoolBuilder::new().num_threads(1).build())
                .or_else(|_| {
                    rayon::ThreadPoolBuilder::new()
                        .num_threads(1)
                        .use_current_thread()
                        .build()
                })
                .ok()
        })
        .as_ref()
    {
        Some(pool) => pool.install(job),
        // The zero-new-thread (current-thread) pool essentially always builds, so
        // `None` -- ALL three tiers failed -- is near-unreachable. If it ever does,
        // `job()` runs `par_iter` with no pool installed, which re-enters rayon's
        // LAZY global-pool init: the exact pre-guard behavior, NOT sequential
        // execution (it parallelizes if the global pool can init, else panics as
        // before). That is no worse than before this guard and requires rayon to be
        // wholly unable to build even a current-thread pool (the process is already
        // doomed at that point).
        None => job(),
    }
}

/// Cheap helper: emit a `tracing::info!` event with elapsed
/// nanoseconds since `start` plus arbitrary key/value pairs.
/// Used by the engine's phase + per-rule timing breakdown so a
/// scaling profile (`RUST_LOG=alint_core::engine=info` at
/// 10k/100k/1M) can show which phase (or rule) is growing
/// super-linearly. Off by default — only fires when info is
/// enabled for this target, so production runs pay nothing.
macro_rules! phase {
    ($start:expr, $phase:expr $(, $k:ident = $v:expr)* $(,)?) => {
        // u128 → u64 saturating cast: `elapsed_us` overflows u64 only
        // after ~584,000 years of wall time. The lossy cast is
        // intentional (we never need the high bits) — picking
        // `try_into().unwrap_or(u64::MAX)` instead of an `as` cast
        // also pegs the rare overflow at u64::MAX rather than
        // silently wrapping, which keeps log readers honest.
        let elapsed_us = u64::try_from($start.elapsed().as_micros()).unwrap_or(u64::MAX);
        tracing::info!(
            phase = $phase,
            elapsed_us = elapsed_us,
            $($k = $v,)*
            "engine.phase",
        );
    };
}
pub(super) use phase;

/// Pre-filtered `FileIndex`es for git-tracked rules. v0.9.11
/// structural fix lets the engine narrow the index handed to
/// each opted-in rule, so the rule's `evaluate()` no longer
/// needs to do its own `is_git_tracked(...)` check per file
/// (the `git_tracked_only`-silently-dropped recurrence-risk
/// shape that audit-tested in v0.9.10 is closed).
///
/// Each variant is `Option<FileIndex>` so the engine only pays
/// the build cost for modes that at least one rule opts into.
#[derive(Debug)]
struct GitTrackedIndexes {
    /// Index containing only files where `git_tracked.contains(path)`.
    /// Handed to rules with [`GitTrackedMode::FileOnly`].
    file_only: Option<FileIndex>,
    /// Index containing dirs where `dir_has_tracked_files(path,
    /// &git_tracked)` plus tracked files. Handed to rules with
    /// [`GitTrackedMode::DirAware`].
    dir_aware: Option<FileIndex>,
}

#[derive(Debug)]
struct AnalysisTimeCache {
    normalized_root: PathBuf,
    resolution: crate::AnalysisTimeResolution,
}

fn normalize_analysis_root(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| {
        if root.is_absolute() {
            root.to_path_buf()
        } else {
            std::env::current_dir().map_or_else(|_| root.to_path_buf(), |cwd| cwd.join(root))
        }
    })
}

fn cached_analysis_time(cache: &AnalysisTimeCache, root: &Path) -> crate::AnalysisTimeResolution {
    if normalize_analysis_root(root) == cache.normalized_root {
        cache.resolution
    } else {
        crate::AnalysisTimeResolution::Unavailable(
            crate::AnalysisTimeUnavailable::EngineRootChanged,
        )
    }
}

/// Return of [`Engine::collect_live_per_file_entries`]: the per-file
/// entries that should evaluate this run (paired with their position in
/// `self.entries`), plus any gate results (`when` errors or empty-scope
/// assertions) to emit verbatim.
type LivePerFileEntries<'a> = (Vec<(usize, &'a RuleEntry)>, Vec<(usize, RuleResult)>);

/// One fix-pass report, its composed in-memory file buffers, and every path
/// whose public diff/proposed edit must be suppressed as sensitive.
type FixRunOutput = (FixReport, BTreeMap<PathBuf, Vec<u8>>, HashSet<PathBuf>);

/// Shared inputs for gates that run before a rule evaluates. The aligned slices
/// are indexed by `Engine::entries`; keeping them together makes that invariant
/// explicit at every dispatch path.
#[derive(Default)]
struct ManifestGate {
    used: bool,
    failures: Vec<Violation>,
}

struct RunGateInputs<'a, 'env> {
    when_env: &'a WhenEnv<'env>,
    manifest_gates: &'a [ManifestGate],
    git_required: &'a [bool],
    git_available: bool,
    changed_active: bool,
}

#[derive(Debug, Default)]
struct TraceExtractionAggregate {
    seen: AtomicBool,
    inputs: AtomicU64,
    values: AtomicU64,
    parse_failures: AtomicU64,
    cardinality_checks: AtomicU64,
    cardinality_mismatches: AtomicU64,
}
const TRACE_PREVIEW_SHARDS: usize = 64;

#[derive(Debug)]
struct TracePreviewAggregate {
    limit: usize,
    shards: Box<[Mutex<Option<TracePreviewCollector>>]>,
}

impl TracePreviewAggregate {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            shards: (0..TRACE_PREVIEW_SHARDS)
                .map(|_| Mutex::new(None))
                .collect(),
        }
    }

    fn push(&self, row: TracePreviewRow) -> Result<()> {
        let index = rayon::current_thread_index().map_or(TRACE_PREVIEW_SHARDS - 1, |index| {
            index % (TRACE_PREVIEW_SHARDS - 1)
        });
        let mut shard = self.shards[index].lock();
        if shard.is_none() {
            *shard = Some(TracePreviewCollector::new(self.limit)?);
        }
        shard
            .as_mut()
            .expect("preview collector initialized")
            .push(row)
    }

    fn finish(&self) -> Result<TracePreview> {
        let mut merged = TracePreviewCollector::new(self.limit)?;
        for shard in &self.shards {
            if let Some(worker) = shard.lock().take() {
                merged.absorb(worker.finish())?;
            }
        }
        Ok(merged.finish())
    }
}

/// Recorder shared only by one selected-rule trace execution.
///
/// Rule evaluation may happen on a rayon worker, so stage insertion is
/// synchronized. High-cardinality loops never lock this recorder: they emit one
/// aggregate stage after completing their production work.
#[derive(Debug)]
struct TraceSession {
    meta: crate::TraceMeta,
    recorder: Mutex<Option<TraceRecorder>>,
    error: Mutex<Option<Error>>,
    extraction: TraceExtractionAggregate,
    preview_limit: Option<usize>,
    extraction_preview: Option<TracePreviewAggregate>,
    sensitive_evidence: bool,
    sensitive_paths: bool,
}

fn checked_atomic_add(counter: &AtomicU64, value: u64) -> bool {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let Some(next) = current.checked_add(value) else {
            return false;
        };
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return true,
            Err(actual) => current = actual,
        }
    }
}

impl TraceSession {
    fn with_options(
        meta: crate::TraceMeta,
        options: TraceOptions,
        sensitive_evidence: bool,
        sensitive_paths: bool,
    ) -> Self {
        let preview_limit = options.preview_limit();
        let extraction_preview = preview_limit
            .filter(|_| meta.allowed_stages.contains(&TraceStageClass::Extraction))
            .map(TracePreviewAggregate::new);
        Self {
            recorder: Mutex::new(Some(TraceRecorder::new(meta.clone()))),
            meta,
            error: Mutex::new(None),
            extraction: TraceExtractionAggregate::default(),
            preview_limit,
            extraction_preview,
            sensitive_evidence,
            sensitive_paths,
        }
    }

    fn allows(&self, class: TraceStageClass) -> bool {
        self.meta.allowed_stages.contains(&class)
    }

    fn record(&self, stage: TraceStage) {
        if self.error.lock().is_some() {
            return;
        }
        let result = self
            .recorder
            .lock()
            .as_mut()
            .expect("trace recorder is live during evaluation")
            .record(stage);
        if let Err(error) = result {
            *self.error.lock() = Some(error);
        }
    }

    fn record_kind_stage(&self, stage: TraceStage) {
        if matches!(
            stage.class,
            TraceStageClass::Gate | TraceStageClass::Outcome
        ) {
            *self.error.lock() = Some(Error::internal(format!(
                "rule producer emitted engine-owned trace stage `{:?}`",
                stage.class
            )));
            return;
        }
        self.record(stage);
    }

    fn add_extraction(&self, observation: TraceExtractionObservation) {
        self.extraction.seen.store(true, Ordering::Relaxed);
        for (counter, value, label) in [
            (
                &self.extraction.inputs,
                observation.inputs,
                "extraction.inputs",
            ),
            (
                &self.extraction.values,
                observation.values,
                "extraction.values",
            ),
            (
                &self.extraction.parse_failures,
                observation.parse_failures,
                "extraction.parse_failures",
            ),
            (
                &self.extraction.cardinality_checks,
                observation.cardinality_checks,
                "cardinality.checks",
            ),
            (
                &self.extraction.cardinality_mismatches,
                observation.cardinality_mismatches,
                "cardinality.mismatches",
            ),
        ] {
            if value == 0 {
                continue;
            }
            if !checked_atomic_add(counter, value) {
                *self.error.lock() = Some(Error::internal(format!(
                    "trace counter `{label}` overflowed"
                )));
                return;
            }
        }
    }

    fn flush_extraction(&self) {
        if !self.extraction.seen.swap(false, Ordering::Relaxed) {
            return;
        }
        let inputs = self.extraction.inputs.load(Ordering::Relaxed);
        let values = self.extraction.values.load(Ordering::Relaxed);
        let parse_failures = self.extraction.parse_failures.load(Ordering::Relaxed);
        let cardinality_checks = self.extraction.cardinality_checks.load(Ordering::Relaxed);
        let cardinality_mismatches = self
            .extraction
            .cardinality_mismatches
            .load(Ordering::Relaxed);
        let (extraction_status, extraction_reason) = if parse_failures > 0 {
            (TraceStatus::AnalysisFailure, TraceReason::ParseFailed)
        } else {
            (TraceStatus::Satisfied, TraceReason::ExtractionSucceeded)
        };
        let mut extraction_stage = TraceStage::new(
            TraceStageClass::Extraction,
            "structured_query",
            extraction_status,
            extraction_reason,
        )
        .with_counts(TraceCounts {
            inputs,
            values,
            analysis_failures: parse_failures,
            ..TraceCounts::default()
        });
        if let Some(preview) = &self.extraction_preview {
            match preview.finish() {
                Ok(preview) => extraction_stage = extraction_stage.with_preview(preview),
                Err(error) => {
                    *self.error.lock() = Some(error);
                    return;
                }
            }
        }
        self.record(extraction_stage);
        let (cardinality_status, cardinality_reason) =
            if parse_failures > 0 && cardinality_checks == 0 {
                (TraceStatus::NotEvaluated, TraceReason::ParseFailed)
            } else if parse_failures > 0 {
                (TraceStatus::AnalysisFailure, TraceReason::ParseFailed)
            } else if cardinality_mismatches > 0 {
                (TraceStatus::Violated, TraceReason::CardinalityMismatch)
            } else {
                (TraceStatus::Satisfied, TraceReason::CardinalitySatisfied)
            };
        self.record(
            TraceStage::new(
                TraceStageClass::Cardinality,
                "query_matches",
                cardinality_status,
                cardinality_reason,
            )
            .with_counts(TraceCounts {
                inputs: cardinality_checks,
                values,
                violations: cardinality_mismatches,
                analysis_failures: parse_failures,
                ..TraceCounts::default()
            }),
        );
    }

    fn finish(
        &self,
        rule_id: String,
        kind: String,
        result: Option<RuleResult>,
    ) -> Result<crate::RuleTrace> {
        if let Some(error) = self.error.lock().take() {
            return Err(error);
        }
        self.recorder
            .lock()
            .take()
            .ok_or_else(|| Error::internal("trace recorder was already finalized"))?
            .finish(rule_id, kind, result)
    }
}

impl TraceSink for TraceSession {
    fn record_kind(&self, stage: TraceStage) {
        self.record_kind_stage(stage);
    }

    fn record_extraction(&self, observation: TraceExtractionObservation) {
        self.add_extraction(observation);
    }

    fn preview_limit(&self) -> Option<usize> {
        self.preview_limit
    }

    fn evidence_sensitivity(&self, producer_sensitive: bool) -> TraceSensitivity {
        if self.sensitive_evidence || producer_sensitive {
            TraceSensitivity::Sensitive
        } else {
            TraceSensitivity::Public
        }
    }

    fn paths_are_sensitive(&self) -> bool {
        self.sensitive_paths
    }

    fn record_preview(&self, row: Result<TracePreviewRow>) {
        if self.error.lock().is_some() {
            return;
        }
        let row = row.map(|mut row| {
            if self.sensitive_paths {
                row.redact_path();
            }
            row
        });
        let result = match (&self.extraction_preview, row) {
            (_, Err(error)) => Err(error),
            (Some(preview), Ok(row)) => preview.push(row),
            (None, Ok(_)) => Err(Error::internal(
                "rule producer emitted an extraction preview row when preview was disabled",
            )),
        };
        if let Err(error) = result {
            *self.error.lock() = Some(error);
        }
    }
}

/// Visitor used while walking top-level and recursively nested runtime rules.
pub(crate) type RuleTreeVisitor<'a> =
    dyn FnMut(&dyn Rule, Option<&crate::RuleSpec>) -> Result<()> + 'a;

/// A rule bundled with an optional `when` expression. Rules with a `when`
/// that evaluates to false at runtime are skipped (no `RuleResult` is
/// produced) — same observable effect as `level: off`, but gated on facts.
#[derive(Debug, Clone)]
pub struct RuleEntry {
    pub rule: Arc<dyn Rule>,
    pub when: Option<WhenExpr>,
    /// The rule's originating [`RuleSpec`](crate::config::RuleSpec), retained so
    /// config-scoped tooling (`alint explain`, `alint list`) can render the
    /// rule's configured detail — kind, `paths:`, `message:`, `when:` source, and
    /// kind-specific options — from one source of truth rather than a handful of
    /// ad-hoc per-field copies. The engine also reads `expect_matches` while
    /// evaluating the rule's configuration-integrity assertion.
    ///
    /// `None` for entries built without a spec (`Engine::new`, nested/iterator
    /// child rules); such entries render as if every display field were empty.
    /// INVARIANT: `list --category` maps a `None` spec (an empty
    /// [`kind`](RuleEntry::kind)) to "no categories" and silently drops the rule,
    /// so any config-loading path that wants `--category` to work MUST attach the
    /// spec via [`with_spec`](RuleEntry::with_spec).
    pub spec: Option<Arc<crate::config::RuleSpec>>,
    /// The rule's resolved top-level `allow_out_of_root:` permission, threaded
    /// into the fixer's [`crate::rule::FixContext`] so a config-declared fix path can escape
    /// the root only when the user's own config opted this rule in. Defaults to
    /// `false` (confined) — the safe default for `Engine::new` / nested rules.
    pub allow_out_of_root: bool,
}

impl RuleEntry {
    pub fn new(rule: Box<dyn Rule>) -> Self {
        Self {
            rule: Arc::from(rule),
            when: None,
            spec: None,
            allow_out_of_root: false,
        }
    }

    /// Record the rule's resolved `allow_out_of_root:` permission (see
    /// [`RuleEntry::allow_out_of_root`]).
    #[must_use]
    pub fn with_allow_out_of_root(mut self, allow: bool) -> Self {
        self.allow_out_of_root = allow;
        self
    }

    #[must_use]
    pub fn with_when(mut self, expr: WhenExpr) -> Self {
        self.when = Some(expr);
        self
    }

    /// Attach the rule's originating [`RuleSpec`](crate::config::RuleSpec) (see
    /// [`RuleEntry::spec`]) — the single source config-scoped tooling renders
    /// the rule's kind, paths, message, `when:` source, and options from.
    #[must_use]
    pub fn with_spec(mut self, spec: Arc<crate::config::RuleSpec>) -> Self {
        self.spec = Some(spec);
        self
    }

    /// Whether this entry opts into the empty-scope assertion.
    #[must_use]
    pub fn expect_matches(&self) -> bool {
        self.spec.as_ref().is_some_and(|spec| spec.expect_matches)
    }

    /// The rule's kind (e.g. `file_exists`), or `""` when built without a spec.
    /// An empty kind maps to "no categories" for `list --category` (the rule is
    /// silently dropped), matching the pre-projection behaviour.
    #[must_use]
    pub fn kind(&self) -> &str {
        self.spec.as_ref().map_or("", |s| s.kind.as_str())
    }

    /// The rule's `paths:` scope, if the spec configured one.
    #[must_use]
    pub fn paths(&self) -> Option<&crate::config::PathsSpec> {
        self.spec.as_ref().and_then(|s| s.paths.as_ref())
    }

    /// The rule's `scope_filter:` config, if set — for `alint explain` to render
    /// the manifest / diff / ancestor gates the `paths:` glob alone doesn't show.
    #[must_use]
    pub fn scope_filter(&self) -> Option<&crate::ScopeFilterSpec> {
        self.spec.as_ref().and_then(|s| s.scope_filter.as_ref())
    }

    /// The rule's custom `message:`, if set (see [`RuleEntry::spec`]).
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.spec.as_ref().and_then(|s| s.message.as_deref())
    }

    /// The rule's authored `when:` source text, retained so `alint explain` can
    /// show the written expression rather than the parsed AST. `None` when the
    /// rule is unconditional.
    #[must_use]
    pub fn when_src(&self) -> Option<&str> {
        self.spec.as_ref().and_then(|s| s.when.as_deref())
    }

    /// The rule's kind-specific options (the flattened non-common `RuleSpec`
    /// fields, e.g. `pattern`, `max_lines`), or `None` when built without a spec.
    #[must_use]
    pub fn extra(&self) -> Option<&yaml_serde::Mapping> {
        self.spec.as_ref().map(|s| &s.extra)
    }

    /// Whether this configured rule tree consumes deterministic analysis time.
    ///
    /// Nested iterator rules are instantiated with stable placeholder path
    /// tokens, matching the engine's preparation traversal.
    /// Pattern hints ([`crate::pattern_hints`]) for this rule and every
    /// nested rule it instantiates.
    #[must_use]
    pub fn pattern_hints(&self, registry: &RuleRegistry) -> Vec<crate::pattern_hints::ConfigHint> {
        crate::pattern_hints::rule_tree_hints(registry, self.rule.as_ref(), self.spec.as_deref())
    }

    pub fn wants_analysis_time(&self, registry: &RuleRegistry) -> Result<bool> {
        let mut wants = false;
        visit_rule_tree_with_registry(
            registry,
            self.rule.as_ref(),
            self.spec.as_deref(),
            &mut |rule, _spec| {
                wants |= rule.wants_analysis_time();
                Ok(())
            },
        )?;
        Ok(wants)
    }

    /// Whether this configured rule tree has an active Git capability.
    ///
    /// Metadata, not a kind allow-list, is authoritative. A nested rule only
    /// contributes when its compiled `when:` can activate for this index; this
    /// is the same per-iteration environment used by runtime evaluation.
    fn requires_git(
        &self,
        registry: &RuleRegistry,
        index: &FileIndex,
        when_env: &WhenEnv<'_>,
    ) -> Result<bool> {
        let mut requires = false;
        visit_active_rule_tree_with_registry(
            registry,
            self.rule.as_ref(),
            self.spec.as_deref(),
            index,
            when_env,
            &mut |rule, spec| {
                if let Some(spec) = spec
                    && let Some(meta) = registry.meta(&spec.kind)
                {
                    requires |= meta.effective_capabilities().any(|capability| {
                        matches!(capability.capability, crate::Capability::Git { .. })
                            && capability_condition_is_active(&capability.condition, spec)
                    });
                } else {
                    // `Engine::new` can receive rules without source specs. Keep
                    // the runtime traits as the conservative fallback for the
                    // two Git requirements they expose directly.
                    requires |= rule.git_tracked_mode() != crate::rule::GitTrackedMode::Off
                        || rule.wants_git_blame();
                }
                Ok(())
            },
        )?;
        Ok(requires)
    }
}

fn option_value_is_set(value: &yaml_serde::Value) -> bool {
    match value {
        yaml_serde::Value::Null => false,
        yaml_serde::Value::Bool(value) => *value,
        yaml_serde::Value::String(value) => !value.is_empty(),
        yaml_serde::Value::Sequence(value) => !value.is_empty(),
        yaml_serde::Value::Mapping(value) => !value.is_empty(),
        yaml_serde::Value::Number(_) => true,
        yaml_serde::Value::Tagged(value) => option_value_is_set(&value.value),
    }
}

fn capability_condition_is_active(
    condition: &crate::CapabilityCondition,
    spec: &crate::RuleSpec,
) -> bool {
    match condition {
        crate::CapabilityCondition::Always => true,
        crate::CapabilityCondition::OptionSet { option } => spec
            .extra
            .get(option.as_ref())
            .is_some_and(option_value_is_set),
        crate::CapabilityCondition::OptionMatches { option, predicate } => {
            let Some(value) = spec
                .extra
                .get(option.as_ref())
                .and_then(|value| value.as_str())
            else {
                return false;
            };
            match predicate {
                crate::OptionPredicate::ContainsLegacyEnvReference => value.contains("${"),
            }
        }
        crate::CapabilityCondition::FixOpConfigured { op } => spec
            .fix
            .as_ref()
            .is_some_and(|fix| fix.op_name() == op.as_ref()),
        crate::CapabilityCondition::CommonFieldSet { field } => match field.as_ref() {
            "scope_filter.changed_since" => spec
                .scope_filter
                .as_ref()
                .and_then(|filter| filter.changed_since.as_ref())
                .is_some(),
            // Git currently has no other common-field condition. Keep unknown
            // future fields inactive until their effective-value semantics are
            // defined here rather than treating mere YAML presence as truth.
            _ => false,
        },
    }
}

fn unavailable_git_failure() -> Violation {
    Violation::new("Git is unavailable or the lint root is not a usable Git repository")
        .as_analysis_failure(AnalysisFailureKind::ResourceRead)
}

pub(crate) fn visit_rule_tree_with_registry(
    registry: &RuleRegistry,
    rule: &dyn Rule,
    spec: Option<&crate::RuleSpec>,
    visit: &mut RuleTreeVisitor<'_>,
) -> Result<()> {
    visit(rule, spec)?;
    if rule.nested_rule_specs().is_empty() {
        return Ok(());
    }

    let tokens = crate::template::PathTokens::from_path(Path::new("_"));
    for (idx, compiled) in rule.nested_rule_specs().iter().enumerate() {
        let nested_spec = compiled
            .spec
            .instantiate(rule.id(), idx, rule.level(), &tokens);
        let nested_rule = registry.build(&nested_spec).map_err(|error| {
            Error::rule_config(
                rule.id(),
                format!(
                    "nested rule #{idx} (`{}`) could not be prepared: {error}",
                    compiled.spec.kind
                ),
            )
        })?;
        visit_rule_tree_with_registry(registry, nested_rule.as_ref(), Some(&nested_spec), visit)?;
    }
    Ok(())
}

fn when_expr_uses_iter(expr: &WhenExpr) -> bool {
    use crate::when::Namespace;

    match expr {
        WhenExpr::Ident { ns, .. } | WhenExpr::Call { ns, .. } => *ns == Namespace::Iter,
        WhenExpr::Not(inner) => when_expr_uses_iter(inner),
        WhenExpr::And(left, right)
        | WhenExpr::Or(left, right)
        | WhenExpr::Cmp { left, right, .. } => {
            when_expr_uses_iter(left) || when_expr_uses_iter(right)
        }
        WhenExpr::Matches { left, .. } => when_expr_uses_iter(left),
        WhenExpr::List(items) => items.iter().any(when_expr_uses_iter),
        WhenExpr::Literal(_) => false,
    }
}

fn configured_iteration_gate(spec: Option<&crate::RuleSpec>) -> Result<Option<WhenExpr>> {
    spec.and_then(|spec| spec.extra.get("when_iter"))
        .and_then(yaml_serde::Value::as_str)
        .map(crate::when::parse)
        .transpose()
        .map_err(|error| {
            Error::Other(format!(
                "prepared nested iteration gate is invalid: {error}"
            ))
        })
}

fn configured_select_scope(spec: Option<&crate::RuleSpec>) -> Result<Option<crate::Scope>> {
    let Some(value) = spec.and_then(|spec| spec.extra.get("select")) else {
        return Ok(None);
    };
    let patterns = match value {
        yaml_serde::Value::String(pattern) => vec![pattern.clone()],
        yaml_serde::Value::Sequence(patterns) => patterns
            .iter()
            .filter_map(yaml_serde::Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => return Ok(None),
    };
    crate::Scope::from_patterns(&patterns).map(Some)
}

fn nested_iteration_accepts_entry(spec: Option<&crate::RuleSpec>, is_dir: bool) -> bool {
    match spec.map(|spec| spec.kind.as_str()) {
        Some("for_each_dir") => is_dir,
        Some("for_each_file") => !is_dir,
        // `every_matching_has` and extension kinds can iterate either shape.
        // Unknown nested owners stay conservative rather than losing a needed
        // dependency.
        _ => true,
    }
}

/// Whether a nested spec reaches at least one runtime iteration.
///
/// The scan is dependency preparation only: it never builds or evaluates the
/// child. It mirrors the parent's configured selector and `when_iter:` gate,
/// then evaluates the child's already-compiled `when:` with the exact
/// per-iteration facts. This prevents a dependency probe/read for a child whose
/// gate is false for every iteration.
fn nested_rule_can_activate(
    parent_spec: Option<&crate::RuleSpec>,
    parent_gate: Option<&WhenExpr>,
    select: Option<&crate::Scope>,
    nested: &crate::CompiledNestedSpec,
    index: &FileIndex,
    when_env: &WhenEnv<'_>,
) -> bool {
    let child_uses_iter = nested.when.as_ref().is_some_and(when_expr_uses_iter);
    let parent_uses_iter = parent_gate.is_some_and(when_expr_uses_iter);

    if !child_uses_iter
        && nested
            .when
            .as_ref()
            .is_some_and(|expr| !matches!(expr.evaluate(when_env), Ok(true)))
    {
        return false;
    }
    if !parent_uses_iter
        && parent_gate.is_some_and(|expr| !matches!(expr.evaluate(when_env), Ok(true)))
    {
        return false;
    }

    for entry in index.dirs().chain(index.files()) {
        if !nested_iteration_accepts_entry(parent_spec, entry.is_dir)
            || select.is_some_and(|scope| !scope.matches(&entry.path, index))
        {
            continue;
        }
        let iter = crate::when::IterEnv {
            path: &entry.path,
            is_dir: entry.is_dir,
            index,
        };
        let env = WhenEnv {
            facts: when_env.facts,
            vars: when_env.vars,
            iter: Some(iter),
            env: when_env.env,
        };
        if parent_gate.is_some_and(|expr| !matches!(expr.evaluate(&env), Ok(true)))
            || nested
                .when
                .as_ref()
                .is_some_and(|expr| !matches!(expr.evaluate(&env), Ok(true)))
        {
            continue;
        }
        return true;
    }
    false
}

fn visit_active_rule_tree_with_registry(
    registry: &RuleRegistry,
    rule: &dyn Rule,
    spec: Option<&crate::RuleSpec>,
    index: &FileIndex,
    when_env: &WhenEnv<'_>,
    visit: &mut RuleTreeVisitor<'_>,
) -> Result<()> {
    visit(rule, spec)?;
    if rule.nested_rule_specs().is_empty() {
        return Ok(());
    }

    let parent_gate = configured_iteration_gate(spec)?;
    let select = configured_select_scope(spec)?;
    let tokens = crate::template::PathTokens::from_path(Path::new("_"));
    for (idx, compiled) in rule.nested_rule_specs().iter().enumerate() {
        if !nested_rule_can_activate(
            spec,
            parent_gate.as_ref(),
            select.as_ref(),
            compiled,
            index,
            when_env,
        ) {
            continue;
        }
        let nested_spec = compiled
            .spec
            .instantiate(rule.id(), idx, rule.level(), &tokens);
        let nested_rule = registry.build(&nested_spec).map_err(|error| {
            Error::rule_config(
                rule.id(),
                format!(
                    "nested rule #{idx} (`{}`) could not be prepared: {error}",
                    compiled.spec.kind
                ),
            )
        })?;
        visit_active_rule_tree_with_registry(
            registry,
            nested_rule.as_ref(),
            Some(&nested_spec),
            index,
            when_env,
            visit,
        )?;
    }
    Ok(())
}

/// Executes a set of rules against a pre-built [`FileIndex`].
///
/// The engine owns a [`RuleRegistry`] so cross-file rules (e.g.
/// `for_each_dir`) can build nested rules on demand during evaluation.
/// Optional `facts` and `vars` (set via the builder chain) are evaluated
/// at run time and threaded into each rule's [`Context`] and into the
/// `when` expression evaluator that gates rules.
#[derive(Debug)]
pub struct Engine {
    entries: Vec<RuleEntry>,
    registry: RuleRegistry,
    facts: Vec<FactSpec>,
    vars: HashMap<String, String>,
    fix_size_limit: Option<u64>,
    analysis_time: Option<crate::AnalysisTimeResolution>,
    process_environment: Option<BTreeMap<String, String>>,
    analysis_time_required: OnceLock<bool>,
    analysis_time_cache: Mutex<Option<AnalysisTimeCache>>,
    /// In `--changed` mode, the set of paths (relative to root)
    /// that the user wants linted. `None` means "full check"; the
    /// engine bypasses every changed-set short-circuit. See
    /// [`Engine::with_changed_paths`] for the contract.
    changed_paths: Option<HashSet<PathBuf>>,
    /// When set (`fix --baseline`), the loaded baseline: the fix pass SKIPS
    /// (surfaces, never applies) any violation the baseline suppresses, so `fix`
    /// resolves only NEW findings -- matching `check --baseline`. `None` means
    /// "fix everything". See [`Engine::with_fix_baseline`].
    fix_baseline: Option<crate::baseline::Baseline>,
}

#[allow(clippy::too_many_lines)]
fn evaluate_per_file_entries<const TRACE: bool>(
    root: &Path,
    walk_index: &FileIndex,
    live: &[(usize, &RuleEntry)],
    any_full_index: bool,
    filtered_ctx: Option<&Context<'_>>,
    per_file_ctx: &Context<'_>,
) -> (Vec<(usize, Violation)>, TraceCounts) {
    let considered = TRACE.then(|| AtomicU64::new(0));
    let matched = TRACE.then(|| AtomicU64::new(0));
    let by_file: Vec<(usize, Violation)> = with_worker_pool(|| {
        walk_index
            .entries
            .par_iter()
            .filter(|e| !e.is_dir)
            .flat_map_iter(|file_entry| {
                if let Some(considered) = &considered {
                    considered.fetch_add(1, Ordering::Relaxed);
                }
                // Whether this file is in the `--changed` set (always true
                // for a full run, or when the walk is already the filtered
                // index). Only full-index per-file rules see the others.
                let in_changed = !any_full_index
                    || filtered_ctx.is_none_or(|f| f.index.contains_file(&file_entry.path));
                // 1. Decide which per-file rules apply to this
                // file. Per-file rules expose their scope via
                // `PerFileRule::path_scope`; we filter on it
                // before any I/O so files no rule cares about
                // never get read. Carrying `entry_idx` through
                // here avoids an O(L) `position` lookup per
                // applicable rule per file inside the inner
                // dispatch loop below.
                let applicable: Vec<(usize, &RuleEntry)> = live
                    .iter()
                    .filter(|(_, entry)| {
                        if !in_changed && !entry.rule.requires_full_index() {
                            return false;
                        }
                        // 1a. Path-scope glob — cheap, dropping
                        // files no rule cares about before any
                        // further work.
                        // v0.9.10: `Scope::matches` consults both
                        // path-glob AND `scope_filter` in one
                        // call (Scope owns its optional filter
                        // since the v0.9.10 structural fix). The
                        // separate v0.9.6 `entry.rule.scope_filter()`
                        // check this used to do is now folded in.
                        entry
                            .rule
                            .as_per_file()
                            .expect("live entries are per-file rules by construction")
                            .path_scope()
                            .matches(&file_entry.path, per_file_ctx.index)
                    })
                    .map(|(idx, entry)| (*idx, *entry))
                    .collect();
                if let Some(matched) = &matched
                    && !applicable.is_empty()
                {
                    matched.fetch_add(1, Ordering::Relaxed);
                }
                if applicable.is_empty() {
                    return Vec::new();
                }
                // 2. Read once for every applicable rule. When a relation
                // consumer enabled the run-scoped cache, this read is shared
                // with it; otherwise the ordinary path reads directly and pays
                // no cache lookup. Every failure after repository discovery is
                // a typed `resource_read` analysis failure for each owning
                // rule; an unreadable or over-cap in-scope file can never look
                // like a policy pass.
                if let Some(cache) = per_file_ctx.index.read_cache() {
                    let read = cache.read_index_path(root, &file_entry.path, file_entry.size);
                    return match read.as_ref() {
                        crate::relation::ResourceRead::Present(bytes) => {
                            dispatch_per_file::<TRACE>(
                                per_file_ctx,
                                &applicable,
                                &file_entry.path,
                                bytes,
                            )
                        }
                        crate::relation::ResourceRead::Missing => violations_for_each(
                            &applicable,
                            &crate::rule::missing_file_violation(&file_entry.path),
                        ),
                        crate::relation::ResourceRead::TooLarge(size) => violations_for_each(
                            &applicable,
                            &crate::rule::too_large_file_violation(&file_entry.path, *size),
                        ),
                        crate::relation::ResourceRead::Unreadable(message) => violations_for_each(
                            &applicable,
                            &Violation::new(format!("could not read file: {message}"))
                                .with_path(file_entry.path.clone())
                                .as_analysis_failure(AnalysisFailureKind::ResourceRead),
                        ),
                    };
                }
                let abs = root.join(&file_entry.path);
                let bytes = match crate::walker::read_for_analysis(&abs, file_entry.size) {
                    crate::walker::AnalysisRead::Bytes(b) => b,
                    crate::walker::AnalysisRead::Missing => {
                        return violations_for_each(
                            &applicable,
                            &crate::rule::missing_file_violation(&file_entry.path),
                        );
                    }
                    crate::walker::AnalysisRead::TooLarge(size) => {
                        return violations_for_each(
                            &applicable,
                            &crate::rule::too_large_file_violation(&file_entry.path, size),
                        );
                    }
                    crate::walker::AnalysisRead::Unreadable(e) => {
                        return violations_for_each(
                            &applicable,
                            &crate::rule::unreadable_file_violation(&file_entry.path, &e),
                        );
                    }
                };
                // 3. Dispatch. Every applicable rule sees the
                // same byte slice; the file is read exactly once
                // even though N rules may produce violations
                // against it.
                dispatch_per_file::<TRACE>(per_file_ctx, &applicable, &file_entry.path, &bytes)
            })
            .collect()
    });
    (
        by_file,
        if TRACE {
            TraceCounts {
                considered: considered.as_ref().unwrap().load(Ordering::Relaxed),
                matched: matched.as_ref().unwrap().load(Ordering::Relaxed),
                ..TraceCounts::default()
            }
        } else {
            TraceCounts::default()
        },
    )
}

/// Dispatch one file's bytes to every applicable per-file rule. Shared by the
/// direct read path and the relation-cache path so both observe identical
/// evaluation and failure semantics.
fn dispatch_per_file<const TRACE: bool>(
    ctx: &Context<'_>,
    applicable: &[(usize, &RuleEntry)],
    path: &Path,
    bytes: &[u8],
) -> Vec<(usize, Violation)> {
    let mut out = Vec::new();
    for (entry_idx, entry) in applicable {
        let pf = entry
            .rule
            .as_per_file()
            .expect("live entries are per-file rules by construction");
        let result = if TRACE {
            pf.evaluate_file_traced(ctx, path, bytes)
        } else {
            pf.evaluate_file(ctx, path, bytes)
        };
        match result {
            Ok(violations) => {
                for violation in violations {
                    out.push((*entry_idx, violation));
                }
            }
            Err(error) => {
                out.push((
                    *entry_idx,
                    Violation::new(format!("rule evaluation failed: {error}"))
                        .with_path(path.to_path_buf())
                        .as_analysis_failure(AnalysisFailureKind::Evaluation),
                ));
            }
        }
    }
    out
}

/// One read failure for every rule that would have evaluated the file.
fn violations_for_each(
    applicable: &[(usize, &RuleEntry)],
    violation: &Violation,
) -> Vec<(usize, Violation)> {
    applicable
        .iter()
        .map(|(entry_idx, _)| (*entry_idx, violation.clone()))
        .collect()
}

/// One file a fix pass would change, captured by [`Engine::stage_fixes`] for
/// `alint fix --diff`: the repo-relative `path` and the `old` / `new` byte
/// contents (the composed result of every fixer that touched it), so a caller
/// can render a diff without the change being written. [`kind`](Self::kind)
/// distinguishes an in-place content edit from a whole-file create / delete /
/// rename so the renderer can use the `/dev/null` and rename conventions.
#[derive(Debug, Clone)]
pub struct StagedFix {
    pub path: PathBuf,
    pub old: Vec<u8>,
    pub new: Vec<u8>,
    pub kind: StagedKind,
}

/// How a [`StagedFix`] changes its file, so a diff renderer can pick the right
/// header convention (git uses `/dev/null` for a create/delete and explicit
/// `rename from`/`rename to` lines for a rename).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedKind {
    /// An existing file's contents change in place (the content-fixer path).
    Modify,
    /// A new file (`old` is empty; the diff's `---` side is `/dev/null`).
    Create,
    /// A removed file (`new` is empty; the diff's `+++` side is `/dev/null`).
    Delete,
    /// A rename from `from` to [`StagedFix::path`]; `old`/`new` are the file's
    /// bytes (equal when the rename doesn't also rewrite content).
    Rename { from: PathBuf },
    /// A permission-bit change (`chmod`); `old`/`new` bytes are the (unchanged)
    /// file content, and the diff renders a git-style `old mode` / `new mode` pair.
    Chmod { old_mode: u32, new_mode: u32 },
}

/// Append a fix item to the fixpoint's aggregated results, grouping by
/// `rule_id` so there is exactly one [`FixRuleResult`] per rule across all
/// passes. This is the append half of the loop's keep-last merge: the caller
/// has already retained-out any earlier item this pass supersedes, so appending
/// here cannot duplicate a superseded violation. See docs/design/v0.17/fixpoint.md 7.
/// Append one rule's fix items and analysis failures, coalescing the several
/// fixer regimes into one deterministic result.
fn push_fix_result(
    results: &mut Vec<FixRuleResult>,
    rule_id: Arc<str>,
    level: crate::Level,
    items: Vec<FixItem>,
    analysis_failures: Vec<crate::AnalysisFailure>,
) {
    if let Some(rr) = results.iter_mut().find(|r| r.rule_id == rule_id) {
        rr.items.extend(items);
        rr.analysis_failures.extend(analysis_failures);
    } else {
        results.push(FixRuleResult {
            rule_id,
            level,
            items,
            analysis_failures,
        });
    }
}

fn push_fix_item(
    results: &mut Vec<FixRuleResult>,
    rule_id: &Arc<str>,
    level: crate::Level,
    item: FixItem,
) {
    if let Some(rr) = results.iter_mut().find(|r| r.rule_id == *rule_id) {
        rr.items.push(item);
    } else {
        results.push(FixRuleResult {
            rule_id: Arc::clone(rule_id),
            level,
            items: vec![item],
            analysis_failures: Vec::new(),
        });
    }
}

/// Map a located edit's [`LocatedOutcome`] to its reported [`FixStatus`], with a
/// summary derived from the edit's path. A Phase-1 op that emits located edits
/// can carry a richer summary; Phase 0 never reaches this with real data (no
/// fixer opts into the located path), so the generic wording is only exercised
/// by the engine's fixture test.
fn located_status(
    edit: &FixEdit,
    tier: Applicability,
    outcome: LocatedOutcome,
    dry_run: bool,
) -> FixStatus {
    let path = located_edit_path(edit);
    match outcome {
        LocatedOutcome::Applied => FixStatus::Applied(if dry_run {
            format!("would edit {path}")
        } else {
            format!("edited {path}")
        }),
        LocatedOutcome::Suggested => FixStatus::Suggested {
            // Tier-specific hint, matching the whole-file suggested path: an
            // Unsafe edit IS applied by `--unsafe-fixes`; a Suggestion-tier edit
            // never auto-applies. Avoids the formatter's "(suggested: suggested
            // ...)" doubling.
            summary: if tier == Applicability::Unsafe {
                format!("rewrite {path} (requires --unsafe-fixes)")
            } else {
                format!("rewrite {path} (suggestion only; not auto-applied)")
            },
            // A located edit always has an editor-expressible form (a byte-range
            // ReplaceRange), so the suggestion always carries it.
            edit: Some(edit.clone()),
        },
        LocatedOutcome::SkippedConflict => FixStatus::declined(format!(
            "edit to {path} skipped: conflicts with another edit"
        )),
        LocatedOutcome::Dropped => {
            FixStatus::declined(format!("edit to {path} not applicable at this tier"))
        }
    }
}

/// The display path a located edit touches, for its status summary.
fn located_edit_path(edit: &FixEdit) -> String {
    match edit {
        FixEdit::ReplaceRange { path, .. }
        | FixEdit::SetContent { path, .. }
        | FixEdit::CreateFile { path, .. }
        | FixEdit::DeleteFile { path }
        | FixEdit::SetMode { path, .. } => path.display().to_string(),
        FixEdit::RenameFile { from, to } => format!("{} -> {}", from.display(), to.display()),
    }
}

/// Pick the [`Context`] a rule should evaluate against:
/// `full_ctx` if it [`requires_full_index`](Rule::requires_full_index),
/// otherwise the changed-only filtered context (falling back to
/// `full_ctx` when no `--changed` set is configured).
fn pick_ctx<'a>(
    rule: &dyn Rule,
    dependency_changed: bool,
    full_ctx: &'a Context<'a>,
    filtered_ctx: Option<&'a Context<'a>>,
    git_file_only_ctx: Option<&'a Context<'a>>,
    git_dir_aware_ctx: Option<&'a Context<'a>>,
) -> &'a Context<'a> {
    // v0.9.11: git-tracked filtering wins over both `--changed`
    // filtering and the full-index path. The 4 existence rules
    // that opt in already declare `requires_full_index = true`
    // (their verdict needs the whole tree, not just the changed
    // subset), so this substitution is safe — we're swapping
    // their full-index Context for a pre-narrowed one.
    match rule.git_tracked_mode() {
        crate::rule::GitTrackedMode::FileOnly => {
            return git_file_only_ctx.unwrap_or(full_ctx);
        }
        crate::rule::GitTrackedMode::DirAware => {
            return git_dir_aware_ctx.unwrap_or(full_ctx);
        }
        crate::rule::GitTrackedMode::Off => {}
    }
    if rule.requires_full_index() || dependency_changed {
        full_ctx
    } else {
        filtered_ctx.unwrap_or(full_ctx)
    }
}

fn effective_path_scope(rule: &dyn Rule) -> Option<&crate::Scope> {
    rule.as_per_file()
        .map(super::rule::PerFileRule::path_scope)
        .or_else(|| rule.path_scope())
}

/// Return whether an active rule in `entry`'s tree uses a manifest-derived
/// scope and each distinct resolution failure that prevents it from obtaining
/// a verdict. Failures stay owned by
fn manifest_scope_failures(
    entry: &RuleEntry,
    registry: &RuleRegistry,
    index: &FileIndex,
    when_env: &WhenEnv<'_>,
) -> Result<ManifestGate> {
    let mut gate = ManifestGate::default();
    let mut seen: HashMap<String, usize> = HashMap::new();
    visit_active_rule_tree_with_registry(
        registry,
        entry.rule.as_ref(),
        entry.spec.as_deref(),
        index,
        when_env,
        &mut |rule, spec| {
            let Some(filter) = effective_path_scope(rule).and_then(crate::Scope::scope_filter)
            else {
                return Ok(());
            };
            gate.used |= !filter.manifest_predicates().is_empty();
            for predicate in filter.manifest_predicates() {
                let cache_key = predicate.cache_key();
                let existing = seen.get(cache_key).copied();
                let Some(failure) = index.manifest_failure(cache_key) else {
                    continue;
                };
                let finding_index = if let Some(index) = existing {
                    index
                } else {
                    let index = gate.failures.len();
                    seen.insert(cache_key.to_string(), index);
                    gate.failures.push(
                        Violation::new(failure.message.clone())
                            .with_path(predicate.source().to_path_buf())
                            .as_analysis_failure(failure.kind),
                    );
                    index
                };
                if predicate.redacts_by_default()
                    || spec.is_some_and(|nested_spec| nested_spec.sensitive)
                {
                    gate.failures[finding_index].redact_message(rule.id());
                }
            }
            Ok(())
        },
    )?;
    Ok(gate)
}

fn nonfixable_rule_result(entry: &RuleEntry, violations: Vec<Violation>) -> RuleResult {
    RuleResult::new(
        Arc::from(entry.rule.id()),
        entry.rule.level(),
        entry.rule.policy_url().map(Arc::from),
        violations,
        false,
    )
}

fn trace_gate(
    trace: Option<&TraceSession>,
    name: &'static str,
    status: TraceStatus,
    reason: TraceReason,
) {
    if let Some(trace) = trace {
        trace.record(TraceStage::new(TraceStageClass::Gate, name, status, reason));
    }
}

fn trace_analysis_time(trace: Option<&TraceSession>, ctx: &Context<'_>) {
    let Some(trace) = trace else { return };
    let Some(resolution) = ctx.analysis_time else {
        return;
    };
    let (status, reason) = if resolution.time().is_some() {
        (TraceStatus::Satisfied, TraceReason::AnalysisTimeAvailable)
    } else {
        (
            TraceStatus::AnalysisFailure,
            TraceReason::AnalysisTimeUnavailable,
        )
    };
    trace_gate(Some(trace), "analysis_time", status, reason);
}

fn trace_selection(trace: Option<&TraceSession>, entry: &RuleEntry, ctx: &Context<'_>) {
    let Some(trace) = trace
        .filter(|trace| trace.allows(TraceStageClass::Selection) && !trace.meta.nested_evaluation)
    else {
        return;
    };
    let considered = u64::try_from(ctx.index.entries.len()).expect("index length fits u64");
    let matched = if let Some(per_file) = entry.rule.as_per_file() {
        u64::try_from(
            ctx.index
                .files()
                .filter(|file| per_file.path_scope().matches(&file.path, ctx.index))
                .count(),
        )
        .expect("index length fits u64")
    } else if let Some(scope) = entry.rule.path_scope() {
        u64::try_from(
            ctx.index
                .entries
                .iter()
                .filter(|candidate| scope.matches(&candidate.path, ctx.index))
                .count(),
        )
        .expect("index length fits u64")
    } else {
        u64::try_from(ctx.index.entries.len()).expect("index length fits u64")
    };
    let reason = if matched == 0 {
        TraceReason::SelectorNoMatch
    } else {
        TraceReason::SelectorMatched
    };
    trace.record(
        TraceStage::new(
            TraceStageClass::Selection,
            "dispatch_input",
            TraceStatus::Satisfied,
            reason,
        )
        .with_counts(TraceCounts {
            considered,
            matched,
            ..TraceCounts::default()
        }),
    );
}

fn trace_assertion(trace: Option<&TraceSession>, result: &RuleResult) {
    let Some(trace) = trace else { return };
    trace.flush_extraction();
    let (status, reason) = if !result.analysis_failures.is_empty() {
        (TraceStatus::AnalysisFailure, TraceReason::AssertionFailed)
    } else if !result.violations.is_empty() {
        (TraceStatus::Violated, TraceReason::AssertionViolated)
    } else {
        (TraceStatus::Satisfied, TraceReason::AssertionSatisfied)
    };
    trace.record(
        TraceStage::new(TraceStageClass::Assertion, "policy", status, reason).with_counts(
            TraceCounts {
                violations: u64::try_from(result.violations.len()).expect("length fits u64"),
                notes: u64::try_from(result.notes.len()).expect("length fits u64"),
                analysis_failures: u64::try_from(result.analysis_failures.len())
                    .expect("length fits u64"),
                ..TraceCounts::default()
            },
        ),
    );
}

enum EntryGateOutcome {
    Evaluate,
    Skip,
    Result(RuleResult),
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn evaluate_entry_gates(
    entry_idx: usize,
    entry: &RuleEntry,
    ctx: &Context<'_>,
    full_index: &FileIndex,
    gate_inputs: &RunGateInputs<'_, '_>,
    skipped_for_changed: bool,
    trace: Option<&TraceSession>,
) -> EntryGateOutcome {
    if skipped_for_changed {
        trace_gate(
            trace,
            "changed_scope",
            TraceStatus::Skipped,
            TraceReason::ChangedScopeEmpty,
        );
        if !entry.expect_matches() {
            return EntryGateOutcome::Skip;
        }
    }
    if let Some(expr) = &entry.when {
        match expr.evaluate(gate_inputs.when_env) {
            Ok(true) => {
                trace_gate(trace, "when", TraceStatus::Satisfied, TraceReason::WhenTrue);
            }
            Ok(false) => {
                trace_gate(trace, "when", TraceStatus::Skipped, TraceReason::WhenFalse);
                return EntryGateOutcome::Skip;
            }
            Err(error) => {
                trace_gate(
                    trace,
                    "when",
                    TraceStatus::AnalysisFailure,
                    TraceReason::WhenEvaluationFailed,
                );
                let failure = Violation::new(format!("when evaluation failed: {error}"))
                    .as_analysis_failure(AnalysisFailureKind::Condition);
                return EntryGateOutcome::Result(nonfixable_rule_result(entry, vec![failure]));
            }
        }
    }
    let manifest_gate = &gate_inputs.manifest_gates[entry_idx];
    if !manifest_gate.failures.is_empty() {
        trace_gate(
            trace,
            "manifest",
            TraceStatus::AnalysisFailure,
            TraceReason::ManifestUnavailable,
        );
        return EntryGateOutcome::Result(nonfixable_rule_result(
            entry,
            manifest_gate.failures.clone(),
        ));
    }
    if trace.is_some() && manifest_gate.used {
        trace_gate(
            trace,
            "manifest",
            TraceStatus::Satisfied,
            TraceReason::ManifestAvailable,
        );
    }
    if gate_inputs.git_required[entry_idx] {
        if !gate_inputs.git_available {
            trace_gate(
                trace,
                "git",
                TraceStatus::AnalysisFailure,
                TraceReason::GitUnavailable,
            );
            return EntryGateOutcome::Result(nonfixable_rule_result(
                entry,
                vec![unavailable_git_failure()],
            ));
        }
        trace_gate(
            trace,
            "git",
            TraceStatus::Satisfied,
            TraceReason::GitAvailable,
        );
    }
    let scope_violation = match entry.spec.as_deref() {
        Some(spec) => {
            match crate::expect_matches_violation(entry.rule.as_ref(), spec, full_index, ctx) {
                Ok(violation) => violation,
                Err(error) => {
                    trace_gate(
                        trace,
                        "expect_matches",
                        TraceStatus::AnalysisFailure,
                        TraceReason::ExpectMatchesFailed,
                    );
                    let failure =
                        Violation::new(format!("scope assertion evaluation failed: {error}"))
                            .as_analysis_failure(AnalysisFailureKind::Evaluation);
                    return EntryGateOutcome::Result(nonfixable_rule_result(entry, vec![failure]));
                }
            }
        }
        None => None,
    };
    if let Some(violation) = scope_violation {
        trace_gate(
            trace,
            "expect_matches",
            TraceStatus::Violated,
            TraceReason::ExpectMatchesViolated,
        );
        return EntryGateOutcome::Result(nonfixable_rule_result(entry, vec![violation]));
    }
    if entry.expect_matches() {
        trace_gate(
            trace,
            "expect_matches",
            TraceStatus::Satisfied,
            TraceReason::ExpectMatchesSatisfied,
        );
    }
    if skipped_for_changed {
        return EntryGateOutcome::Skip;
    }
    if gate_inputs.changed_active {
        trace_gate(
            trace,
            "changed_scope",
            TraceStatus::Satisfied,
            TraceReason::ChangedScopeMatched,
        );
    }
    trace_analysis_time(trace, ctx);
    EntryGateOutcome::Evaluate
}

fn run_entry(
    entry_idx: usize,
    entry: &RuleEntry,
    ctx: &Context<'_>,
    full_index: &FileIndex,
    gate_inputs: &RunGateInputs<'_, '_>,
    skipped_for_changed: bool,
    trace: Option<&TraceSession>,
) -> Option<RuleResult> {
    match evaluate_entry_gates(
        entry_idx,
        entry,
        ctx,
        full_index,
        gate_inputs,
        skipped_for_changed,
        trace,
    ) {
        EntryGateOutcome::Evaluate => {}
        EntryGateOutcome::Skip => return None,
        EntryGateOutcome::Result(result) => return Some(result),
    }
    trace_selection(trace, entry, ctx);
    let violations = match entry.rule.evaluate(ctx) {
        Ok(v) => v,
        Err(e) => vec![
            Violation::new(format!("rule evaluation failed: {e}"))
                .as_analysis_failure(AnalysisFailureKind::Evaluation),
        ],
    };
    let (violations, is_fixable) = mark_fixability(violations, entry.rule.fixer());
    let result = RuleResult::new(
        Arc::from(entry.rule.id()),
        entry.rule.level(),
        entry.rule.policy_url().map(Arc::from),
        violations,
        is_fixable,
    );
    trace_assertion(trace, &result);
    Some(result)
}

/// Stamp each violation's per-violation fixability ([`Violation::is_fixable`])
/// from the rule's fixer, and return the rule-level flag ("the rule declares a
/// fixer") alongside. Centralizes the two-level derivation for the `RuleResult`
/// assembly sites.
///
/// `is_fixable` means "a bare `alint fix` would resolve THIS violation", so it
/// is BOTH per-violation convertibility ([`Fixer::can_fix`] -- an unconvertible
/// `café.rs` under `snake` is not fixable even though its rule has a fixer) AND
/// tier-gated at the default (`Safe`) threshold: an `Unsafe` fixer such as
/// `file_remove` only *suggests* under a bare `fix`, so its violations are not
/// tagged/counted "auto-fixable" by `check` (that would promise a resolution a
/// bare `fix` never delivers -- `--unsafe-fixes` is required, which `check` does
/// not assume). The rule-level [`RuleResult::is_fixable`] ("the rule declares a
/// fixer") is independent and still backs the machine formats.
/// Whether `v` is a finding its rule's fixer can never resolve: the rule marked
/// it [`Violation::not_fixable`], or the fixer's per-violation
/// [`Fixer::can_fix`] declines it. `check` never tags such a finding fixable
/// ([`mark_fixability`]) and `fix` reports it `unfixable` without invoking the
/// fixer, so the two always agree.
fn is_unfixable_finding(fixer: &dyn Fixer, v: &Violation) -> bool {
    v.not_fixable || !fixer.can_fix(v)
}

fn mark_fixability(
    mut violations: Vec<Violation>,
    fixer: Option<&dyn Fixer>,
) -> (Vec<Violation>, bool) {
    match fixer {
        Some(f) => {
            // The default bare-`fix` threshold. An Unsafe/Suggestion/Never tier
            // does not apply here, so those violations are not "auto-fixable".
            let applies_by_default = f.applicability().applies_at(Applicability::Safe);
            for v in &mut violations {
                // Only policy violations are remediation targets. Notes and
                // analysis failures are partitioned after this pass.
                v.is_fixable = v.disposition == FindingDisposition::Violation
                    && applies_by_default
                    && !is_unfixable_finding(f, v);
            }
            (violations, true)
        }
        None => (violations, false),
    }
}

#[cfg(test)]
mod tests;
