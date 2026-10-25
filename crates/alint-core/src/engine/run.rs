//! Check, trace, and single-file engine execution.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use parking_lot::Mutex;
use rayon::prelude::*;

use crate::error::{Error, Result};
use crate::facts::{FactSpec, FactValues, evaluate_facts_with_environment};
use crate::registry::RuleRegistry;
use crate::report::Report;
use crate::rule::{AnalysisFailureKind, Context, PerFileRule, Rule, RuleResult, Violation};
use crate::trace::{
    TraceOptions, TraceReason, TraceSink, TraceStage, TraceStageClass, TraceStatus,
};
use crate::walker::FileIndex;
use crate::when::WhenEnv;

use super::{
    Engine, EntryGateOutcome, LivePerFileEntries, RuleEntry, RunGateInputs, TraceSession,
    evaluate_entry_gates, evaluate_per_file_entries, mark_fixability, phase, pick_ctx, run_entry,
    trace_assertion, with_worker_pool,
};

impl Engine {
    /// Backward-compatible: wrap each rule in a [`RuleEntry`] with no `when`.
    pub fn new(rules: Vec<Box<dyn Rule>>, registry: RuleRegistry) -> Self {
        let entries = rules.into_iter().map(RuleEntry::new).collect();
        Self {
            entries,
            registry,
            facts: Vec::new(),
            vars: HashMap::new(),
            fix_size_limit: Some(1 << 20),
            analysis_time: None,
            process_environment: None,
            analysis_time_required: OnceLock::new(),
            analysis_time_cache: Mutex::new(None),
            changed_paths: None,
            fix_baseline: None,
        }
    }

    /// Construct from rule entries (each carrying an optional `when`).
    pub fn from_entries(entries: Vec<RuleEntry>, registry: RuleRegistry) -> Self {
        Self {
            entries,
            registry,
            facts: Vec::new(),
            vars: HashMap::new(),
            fix_size_limit: Some(1 << 20),
            analysis_time: None,
            process_environment: None,
            analysis_time_required: OnceLock::new(),
            analysis_time_cache: Mutex::new(None),
            changed_paths: None,
            fix_baseline: None,
        }
    }

    #[must_use]
    pub fn with_fix_size_limit(mut self, limit: Option<u64>) -> Self {
        self.fix_size_limit = limit;
        self
    }

    /// Make the fix pass baseline-aware (`fix --baseline`): a violation the
    /// `baseline` suppresses is SKIPPED (reported as `baselined`, never applied),
    /// so `fix` resolves only new findings. Classification reuses
    /// [`crate::baseline::apply`] per rule (the fingerprint includes the rule id,
    /// so per-rule application equals report-level), per fixpoint pass, on the
    /// current file content -- identical to how `check --baseline` suppresses.
    #[must_use]
    pub fn with_fix_baseline(mut self, baseline: crate::baseline::Baseline) -> Self {
        self.fix_baseline = Some(baseline);
        self
    }

    /// The `fix_size_limit` (bytes) a bare `fix` pass enforces: a file larger
    /// than this is skipped rather than fixed. `None` disables the limit.
    /// Exposed so the SARIF proposed-edit computation
    /// ([`crate::proposed_fix::attach_proposed_edits`]) can apply the SAME guard
    /// and never advertise a fix for a file `fix` would skip.
    #[must_use]
    pub fn fix_size_limit(&self) -> Option<u64> {
        self.fix_size_limit
    }

    #[must_use]
    pub fn with_facts(mut self, facts: Vec<FactSpec>) -> Self {
        self.facts = facts;
        self
    }

    #[must_use]
    pub fn with_vars(mut self, vars: HashMap<String, String>) -> Self {
        self.vars = vars;
        self
    }
    /// Replace host inheritance for every rule/fixer/custom-fact child process
    /// with this complete deterministic environment.
    #[must_use]
    pub fn with_process_environment(mut self, environment: BTreeMap<String, String>) -> Self {
        self.process_environment = Some(environment);
        self
    }

    /// Override the reference instant used by every time-dependent rule.
    /// Without this, the run resolves the current HEAD committer time.
    #[must_use]
    pub fn with_analysis_time(mut self, analysis_time: crate::AnalysisTime) -> Self {
        self.analysis_time = Some(crate::AnalysisTimeResolution::available(
            analysis_time,
            crate::AnalysisTimeSource::Explicit,
        ));
        self
    }

    /// Reuse an already-resolved reference instant for a diagnostic execution.
    #[must_use]
    #[doc(hidden)]
    pub fn with_analysis_time_resolution(
        mut self,
        resolution: crate::AnalysisTimeResolution,
    ) -> Self {
        self.analysis_time = Some(resolution);
        self
    }

    /// Restrict evaluation to the given set of paths (relative to
    /// the alint root). Per-file rules see a [`FileIndex`]
    /// filtered to only these paths; rules that override
    /// [`Rule::requires_full_index`] (cross-file + existence
    /// rules) still see the full index but are skipped when
    /// their [`Rule::path_scope`] doesn't intersect the set.
    ///
    /// An empty set normally short-circuits to a no-op report — there's nothing
    /// to lint. Rules with `expect_matches: true` still evaluate their full-tree
    /// scope assertion so an empty diff cannot hide a vacuous configuration.
    /// Pass `None` (or omit) to disable `--changed` semantics entirely.
    #[must_use]
    pub fn with_changed_paths(mut self, set: HashSet<PathBuf>) -> Self {
        self.changed_paths = Some(set);
        self
    }

    pub fn rule_count(&self) -> usize {
        self.entries.len()
    }

    /// Effective top-level rule ids and canonical kinds available to diagnostic drivers.
    #[doc(hidden)]
    pub fn trace_rule_inventory(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().filter_map(|entry| {
            let spec = entry.spec.as_deref()?;
            Some((
                entry.rule.id(),
                self.registry.canonical_kind(spec.kind.as_str()),
            ))
        })
    }

    /// The fixer for the loaded rule with this id, if the rule declares
    /// one. Lets a caller (the LSP server) build an "Apply fix" edit for
    /// a specific violation without re-deriving the rule set.
    pub fn fixer_for(&self, rule_id: &str) -> Option<&dyn crate::rule::Fixer> {
        self.entries
            .iter()
            .find(|e| e.rule.id() == rule_id)
            .and_then(|e| e.rule.fixer())
    }

    /// Whether the loaded rule with this id is a per-file rule (the kind
    /// [`Engine::run_for_file`] re-evaluates). Lets the LSP server tell
    /// per-file findings (refreshed on every edit) apart from cross-file
    /// ones (refreshed only on save), so it can preserve the latter
    /// while re-running the former. Unknown ids return `false`.
    pub fn is_per_file(&self, rule_id: &str) -> bool {
        self.entries
            .iter()
            .find(|e| e.rule.id() == rule_id)
            .is_some_and(|e| e.rule.as_per_file().is_some())
    }

    /// Whether a public edit for `path` could serialize bytes read by an active
    /// sensitive rule. The LSP calls this after a successful full run before
    /// offering a code action; `true` is the conservative answer when fact or
    /// manifest resolution cannot be reproduced.
    pub fn path_contains_sensitive_evidence(
        &self,
        root: &Path,
        index: &FileIndex,
        path: &Path,
    ) -> bool {
        let Ok(fact_values) = evaluate_facts_with_environment(
            &self.facts,
            root,
            index,
            self.process_environment.as_ref(),
        ) else {
            return true;
        };
        let when_env = WhenEnv {
            facts: &fact_values,
            vars: &self.vars,
            iter: None,
            env: None,
        };
        if self
            .resolve_manifest_paths(root, index, &when_env, None)
            .is_err()
        {
            return true;
        }
        let ctx = Context {
            root,
            index,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: None,
            git_blame: None,
            analysis_time: None,
            process_environment: self.process_environment.as_ref(),
            trace: None,
        };
        let target = Self::normalized_write_target(root, path);
        self.entries.iter().any(|entry| {
            if entry
                .when
                .as_ref()
                .is_some_and(|when| !matches!(when.evaluate(&when_env), Ok(true)))
            {
                return false;
            }
            let explicit = entry
                .spec
                .as_deref()
                .is_some_and(crate::RuleSpec::has_sensitive_evidence);
            let scope_path = path.strip_prefix(root).unwrap_or(path);
            let scope = entry
                .rule
                .path_scope()
                .or_else(|| entry.rule.as_per_file().map(PerFileRule::path_scope));
            let manifest_sensitive =
                scope.is_some_and(|scope| scope.sensitive_manifest_sources().next().is_some());
            if (explicit
                || entry.rule.path_contents_sensitive_by_default()
                || (entry.rule.as_per_file().is_some() && manifest_sensitive))
                && scope.is_some_and(|scope| scope.matches(scope_path, index))
            {
                return true;
            }
            let mut found = false;
            entry
                .rule
                .visit_sensitive_paths(explicit, &ctx, &mut |candidate| {
                    found |= Self::normalized_write_target(root, candidate) == target;
                });
            found
        })
    }

    /// Evaluate exactly one effective configured rule through production
    /// dispatch and return its typed execution trace.
    ///
    /// The selected engine shares the rule instance and registry builders with
    /// this engine, copies the immutable run inputs, and never executes an
    /// unrelated rule. The returned result is unbaselined, matching
    #[doc(hidden)]
    /// [`Engine::run`] before report-level baseline filtering.
    pub fn trace_rule(
        &self,
        rule_id: &str,
        root: &Path,
        index: &FileIndex,
    ) -> Result<crate::RuleTrace> {
        self.trace_rule_with_options(rule_id, root, index, TraceOptions::counts_only())
    }

    /// Selected-rule trace with optional bounded evidence preview.
    #[doc(hidden)]
    pub fn trace_rule_with_options(
        &self,
        rule_id: &str,
        root: &Path,
        index: &FileIndex,
        options: TraceOptions,
    ) -> Result<crate::RuleTrace> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.rule.id() == rule_id)
            .ok_or_else(|| Error::Other(format!("unknown rule id `{rule_id}`")))?;
        let configured_kind = entry
            .spec
            .as_deref()
            .map(|spec| spec.kind.as_str())
            .ok_or_else(|| {
                Error::internal(format!(
                    "cannot trace rule `{rule_id}` because its source RuleSpec is unavailable"
                ))
            })?;
        let canonical_kind = self.registry.canonical_kind(configured_kind).to_string();
        let meta = self.registry.meta(configured_kind).ok_or_else(|| {
            Error::internal(format!(
                "cannot trace rule `{rule_id}` because kind `{configured_kind}` has no metadata"
            ))
        })?;
        meta.trace.validate_for_kind(&canonical_kind)?;
        let selected = Self {
            entries: vec![entry.clone()],
            registry: self.registry.clone(),
            facts: self.facts.clone(),
            vars: self.vars.clone(),
            fix_size_limit: self.fix_size_limit,
            analysis_time: self.analysis_time,
            process_environment: self.process_environment.clone(),
            analysis_time_required: OnceLock::new(),
            analysis_time_cache: Mutex::new(None),
            changed_paths: self.changed_paths.clone(),
            fix_baseline: None,
        };
        let sensitive_evidence = entry
            .spec
            .as_deref()
            .is_some_and(crate::RuleSpec::has_sensitive_evidence);
        let sensitive_paths = self.has_sensitive_path_selection(entry);
        let trace = TraceSession::with_options(
            meta.trace.clone(),
            options,
            sensitive_evidence,
            sensitive_paths,
        );
        let mut report = selected.run_internal(root, index, Some(&trace))?;
        if report.results.len() > 1 {
            return Err(Error::internal(format!(
                "selected-rule trace for `{rule_id}` produced {} rule results",
                report.results.len()
            )));
        }
        trace.finish(rule_id.to_string(), canonical_kind, report.results.pop())
    }

    // ~125 lines but each block has its own purpose (changed-set
    // short-circuit, fact eval, git probe, filtered-index build,
    // cross-file partition, per-file partition, assembly). Splitting
    // would mean threading the same ~6-arg context tuple through
    // four helpers that share lifetimes — net worse for the reader.
    // The function reads top-to-bottom as one phased pipeline.
    #[allow(clippy::too_many_lines)]
    pub fn run(&self, root: &Path, index: &FileIndex) -> Result<Report> {
        self.run_internal(root, index, None)
    }

    /// Enable the run-scoped relation read and decode caches when a configured
    /// rule consumes the relation layer. A disabled rule costs nothing; a rule
    /// that is merely skipped by `when:` still enables the caches, which only
    /// adds a per-file lookup for the files that rule's per-file peers read.
    pub(super) fn enable_relation_caches(&self, index: &FileIndex) {
        if self
            .entries
            .iter()
            .any(|entry| entry.rule.uses_relations() && entry.rule.level() != crate::Level::Off)
        {
            index.enable_relation_caches();
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn run_internal(
        &self,
        root: &Path,
        index: &FileIndex,
        trace: Option<&TraceSession>,
    ) -> Result<Report> {
        if let Some(trace) = trace {
            let level = self
                .entries
                .first()
                .ok_or_else(|| Error::internal("selected-rule engine has no entry"))?
                .rule
                .level();
            let (status, reason) = if level == crate::Level::Off {
                (TraceStatus::Skipped, TraceReason::LevelOff)
            } else {
                (TraceStatus::Satisfied, TraceReason::LevelEnabled)
            };
            trace.record(TraceStage::new(
                TraceStageClass::Gate,
                "level",
                status,
                reason,
            ));
            if level == crate::Level::Off {
                return Ok(Report {
                    results: Vec::new(),
                });
            }
        }
        let t_total = Instant::now();
        self.ensure_manifest_scope_resolvable()?;
        // Enable the relation caches only when a rule consumes the layer. The
        // ordinary per-file path checks `index.read_cache()` and reads directly
        // when it is `None`, so this leaves a relation-free run's hot path
        // unchanged.
        self.enable_relation_caches(index);
        // Empty changed-set fast path: nothing to lint, return
        // an empty report rather than walk the entries list at
        // all. Saves the fact-evaluation pass too.
        if self.changed_paths.as_ref().is_some_and(HashSet::is_empty)
            && !self.any_expect_matches()?
        {
            if let Some(trace) = trace {
                trace.record(TraceStage::new(
                    TraceStageClass::Gate,
                    "changed_scope",
                    TraceStatus::Skipped,
                    TraceReason::ChangedScopeEmpty,
                ));
            }
            return Ok(Report {
                results: Vec::new(),
            });
        }

        let t_facts = Instant::now();
        let fact_values = evaluate_facts_with_environment(
            &self.facts,
            root,
            index,
            self.process_environment.as_ref(),
        )?;
        phase!(t_facts, "evaluate_facts", facts = self.facts.len() as u64);
        let when_env = WhenEnv {
            facts: &fact_values,
            vars: &self.vars,
            iter: None,
            env: None,
        };

        let t_git = Instant::now();
        let git_required = self.git_requirements(index, &when_env)?;
        let git_tracked = Self::collect_git_tracked_if_needed(root, &git_required);
        let git_available = git_tracked.is_some();
        let git_blame = self.build_blame_cache_if_needed(root, git_tracked.as_ref());
        let analysis_time = self.resolve_analysis_time_if_needed(root);
        phase!(t_git, "git_setup");

        let t_filter = Instant::now();
        let filtered_index = self.build_filtered_index(index, None);
        phase!(
            t_filter,
            "build_filtered_index",
            files = index.entries.len() as u64,
        );

        let t_git_idx = Instant::now();
        let git_tracked_indexes = self.build_git_tracked_indexes(index, git_tracked.as_ref());
        phase!(
            t_git_idx,
            "build_git_tracked_indexes",
            built = u64::from(git_tracked_indexes.is_some()),
        );

        let full_ctx = Context {
            root,
            index,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
            git_blame: git_blame.as_ref(),
            analysis_time: analysis_time.as_ref(),
            process_environment: self.process_environment.as_ref(),
            trace: trace.map(|trace| trace as &dyn TraceSink),
        };
        let filtered_ctx = filtered_index.as_ref().map(|fi| Context {
            root,
            index: fi,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
            git_blame: git_blame.as_ref(),
            analysis_time: analysis_time.as_ref(),
            process_environment: self.process_environment.as_ref(),
            trace: trace.map(|trace| trace as &dyn TraceSink),
        });
        let git_file_only_ctx = git_tracked_indexes
            .as_ref()
            .and_then(|gti| gti.file_only.as_ref())
            .map(|fi| Context {
                root,
                index: fi,
                registry: Some(&self.registry),
                facts: Some(&fact_values),
                vars: Some(&self.vars),
                git_tracked: git_tracked.as_ref(),
                git_blame: git_blame.as_ref(),
                analysis_time: analysis_time.as_ref(),
                process_environment: self.process_environment.as_ref(),
                trace: trace.map(|trace| trace as &dyn TraceSink),
            });
        let git_dir_aware_ctx = git_tracked_indexes
            .as_ref()
            .and_then(|gti| gti.dir_aware.as_ref())
            .map(|fi| Context {
                root,
                index: fi,
                registry: Some(&self.registry),
                facts: Some(&fact_values),
                vars: Some(&self.vars),
                git_tracked: git_tracked.as_ref(),
                git_blame: git_blame.as_ref(),
                analysis_time: analysis_time.as_ref(),
                process_environment: self.process_environment.as_ref(),
                trace: trace.map(|trace| trace as &dyn TraceSink),
            });

        // Resolve active `scope_filter.changed_since:` diffs + manifest path
        // sets ONCE, before ANY dispatch. BOTH the cross-file/rule-major
        // partition below and the per-file partition read these caches via
        // `Scope::matches` — a non-per-file rule (e.g. `filename_case`)
        // dispatches in the rule-major loop, so resolving after it would
        // silently empty its manifest/diff scope. Repository unavailability is
        // gated before diff operations, and false `when:` rules do not touch
        // either dependency.
        //
        // Resolve against the FULL index (so the manifest is reachable even
        // when unchanged), then copy the maps onto every alternate index a
        // context may dispatch against (the `--changed` filtered index and the
        // git-tracked file-only / dir-aware indexes) — each is a fresh
        // `FileIndex` with empty caches, and the declared set is independent of
        // which files it holds.
        if git_available {
            self.resolve_changed_paths(root, index, &when_env)?;
        }
        self.resolve_manifest_paths(root, index, &when_env, None)?;
        let alt_indexes = [
            filtered_index.as_ref(),
            git_tracked_indexes
                .as_ref()
                .and_then(|g| g.file_only.as_ref()),
            git_tracked_indexes
                .as_ref()
                .and_then(|g| g.dir_aware.as_ref()),
        ];
        for fi in alt_indexes.into_iter().flatten() {
            if let (Some(paths), Some(failures)) =
                (index.manifest_paths_map(), index.manifest_failures_map())
            {
                fi.set_manifest_paths(paths.clone(), failures.clone());
            }
            if let Some(map) = index.changed_paths_map() {
                fi.set_changed_paths(map.clone());
            }
        }
        let manifest_gates = self.manifest_gates_by_entry(index, &when_env)?;
        let gate_inputs = RunGateInputs {
            when_env: &when_env,
            manifest_gates: &manifest_gates,
            git_required: &git_required,
            git_available,
            changed_active: self.changed_paths.is_some(),
        };
        // A `has_ancestor` marker lookup on the `--changed` index must see unchanged
        // files too. Only the changed-set index shares the full tree's file set
        // (the git-tracked indexes are deliberately subsets and must not), and only
        // when a rule will actually look one up on it (`needs_tree_files`):
        // building the set is a pass over the full index that other `--changed`
        // runs should not pay.
        if let Some(fi) = filtered_index.as_ref()
            && self.needs_tree_files(index, None, false)
        {
            fi.share_tree_files_from(index);
        }

        // Per-rule wall-time accumulator for the cross-file
        // partition. One AtomicU64 per entry, indexed by
        // entry position in `self.entries`. Workers add their
        // rule's elapsed nanoseconds atomically; we dump the
        // breakdown after the partition completes. Per-rule
        // timing in a parallel partition is necessarily
        // wall-time (a single rule can't span threads), so
        // the totals here = sum of per-thread elapsed across
        // workers, which still localises which rule dominates.
        let cross_rule_ns: Vec<AtomicU64> =
            (0..self.entries.len()).map(|_| AtomicU64::new(0)).collect();

        // Cross-file partition: rules that don't opt into the
        // file-major dispatch path (cross-file rules + per-file
        // rules that haven't migrated yet). Same parallelism
        // shape as v0.9.2 — rule-major par_iter.
        let t_cross = Instant::now();
        let cross_results: Vec<(usize, RuleResult)> = with_worker_pool(|| {
            self.entries
                .par_iter()
                .enumerate()
                .filter_map(|(idx, entry)| {
                    if entry.rule.as_per_file().is_some() {
                        return None;
                    }
                    let skipped_for_changed =
                        self.skip_for_changed(entry.rule.as_ref(), full_ctx.index, None);
                    let ctx = pick_ctx(
                        entry.rule.as_ref(),
                        self.dependency_changed(entry.rule.as_ref()),
                        &full_ctx,
                        filtered_ctx.as_ref(),
                        git_file_only_ctx.as_ref(),
                        git_dir_aware_ctx.as_ref(),
                    );
                    let t_rule = Instant::now();
                    let result = run_entry(
                        idx,
                        entry,
                        ctx,
                        full_ctx.index,
                        &gate_inputs,
                        skipped_for_changed,
                        trace,
                    );
                    // u128 → u64 saturation: same rationale as `phase!` —
                    // elapsed_ns exceeds u64 only after ~584 years per rule;
                    // clamp rather than panic on the hot path.
                    let elapsed_ns = u64::try_from(t_rule.elapsed().as_nanos()).unwrap_or(u64::MAX);
                    cross_rule_ns[idx].fetch_add(elapsed_ns, Ordering::Relaxed);
                    result.map(|rr| (idx, rr))
                })
                .collect()
        });
        phase!(
            t_cross,
            "cross_file_partition",
            rules = self
                .entries
                .iter()
                .filter(|e| e.rule.as_per_file().is_none())
                .count() as u64,
        );
        // Per-rule cross-file dump: skip zero-elapsed slots
        // (rules that ran on the per-file path or were
        // skipped by `--changed`). Sorted descending by
        // elapsed so the worst offenders are at the top of
        // the log.
        if tracing::level_enabled!(tracing::Level::INFO) {
            let mut rows: Vec<(&str, u64)> = self
                .entries
                .iter()
                .enumerate()
                .filter_map(|(idx, entry)| {
                    let ns = cross_rule_ns[idx].load(Ordering::Relaxed);
                    if ns == 0 {
                        return None;
                    }
                    Some((entry.rule.id(), ns))
                })
                .collect();
            rows.sort_by_key(|(_, ns)| std::cmp::Reverse(*ns));
            for (rule_id, ns) in rows {
                tracing::info!(
                    phase = "cross_file_rule",
                    rule = rule_id,
                    elapsed_us = ns / 1000,
                    "engine.phase",
                );
            }
        }

        // (`scope_filter.changed_since:` diffs + manifest path sets were resolved
        // and propagated to every index before the cross-file partition above, so
        // both partitions see a populated `Scope::matches` cache.)

        // Per-file partition: file-major loop reads each file
        // once and dispatches to every per-file rule whose scope
        // matches. Coalesces N reads of one file across N rules
        // sharing it.
        let t_per_file = Instant::now();
        let per_file_results =
            self.run_per_file(root, &full_ctx, filtered_ctx.as_ref(), &gate_inputs, trace);
        phase!(
            t_per_file,
            "per_file_partition",
            rules = self
                .entries
                .iter()
                .filter(|e| e.rule.as_per_file().is_some())
                .count() as u64,
        );

        // Final assembly preserves `self.entries` order so the
        // output Vec is deterministic + tests that index by
        // position keep working. Each entry slot fills from
        // either the cross-file or per-file partition. A PASSING
        // rule still fills its slot (an empty-violations
        // `RuleResult`, so it counts toward "N rule(s) passed");
        // only a rule that did not run -- skipped by `--changed`
        // scope or a false `when:` -- leaves its slot empty.
        let t_assembly = Instant::now();
        let mut cross_by_idx: HashMap<usize, RuleResult> = cross_results.into_iter().collect();
        let mut per_file_by_idx: HashMap<usize, RuleResult> =
            per_file_results.into_iter().collect();
        let mut results = Vec::with_capacity(self.entries.len());
        for idx in 0..self.entries.len() {
            let result = cross_by_idx
                .remove(&idx)
                .or_else(|| per_file_by_idx.remove(&idx));
            if let Some(mut result) = result {
                self.redact_result_for_entry(idx, &mut result);
                results.push(result);
            }
        }
        phase!(t_assembly, "assembly", results = results.len() as u64);
        phase!(t_total, "engine_run_total");
        Ok(Report { results })
    }

    /// Per-file dispatch loop. Walks `index.files()` in parallel
    /// and, for each file, calls every applicable per-file rule's
    /// `evaluate_file` against a single `std::fs::read`. Returns
    /// `(entry-index, RuleResult)` tuples for every live per-file
    /// rule -- a passing rule (zero violations) included, as an
    /// empty-violations `RuleResult`, matching the cross-file path
    /// (which always emits a result) so it counts as passed -- plus
    /// the gate results `collect_live_per_file_entries` short-circuited
    /// (a `when` error or a failed `expect_matches`).
    #[allow(clippy::too_many_lines)]
    pub(super) fn run_per_file<'a>(
        &'a self,
        root: &'a Path,
        full_ctx: &'a Context<'a>,
        filtered_ctx: Option<&'a Context<'a>>,
        gate_inputs: &RunGateInputs<'_, '_>,
        trace: Option<&TraceSession>,
    ) -> Vec<(usize, RuleResult)> {
        let (live, gate_results) = self.collect_live_per_file_entries(full_ctx, gate_inputs, trace);
        if live.is_empty() {
            return gate_results;
        }

        // Under `--changed`, the per-file pass reads only the changed files, but
        // every scope match and every evaluation uses the FULL context: whether a
        // file is in scope (a `has_ancestor` marker) and what it references are
        // facts about the repository, not about the diff. A per-file rule that
        // declares `requires_full_index` (its verdict on a file depends on other
        // files, such as `markdown_paths_resolve`'s targets) is evaluated on every
        // in-scope file, so deleting an unchanged target is still caught.
        let any_full_index = filtered_ctx.is_some()
            && live
                .iter()
                .any(|(_, entry)| entry.rule.requires_full_index());
        let walk_index = match filtered_ctx {
            Some(filtered) if !any_full_index => filtered.index,
            _ => full_ctx.index,
        };
        let per_file_ctx = full_ctx;

        // Each file-major iteration produces a Vec of
        // `(entry-index, Violation)` tuples. The flatten
        // gathers them all; aggregation below buckets them by
        // entry-index back into per-rule `RuleResult`s.
        //
        // We iterate `index.entries` (a Vec) via `par_iter()`
        // and filter out directories *inside* the parallel
        // pipeline rather than calling `index.files().par_bridge()`.
        // `par_bridge` wraps a sequential iterator using a
        // Mutex-guarded channel; at 1M entries that lock turns
        // into a contention bottleneck across 24 worker
        // threads. The native `par_iter` on the underlying Vec
        // uses Rayon's work-stealing slabs instead — same
        // observable iteration, no shared lock on the hot
        // path.
        let (by_file, selection_counts) = if trace.is_some() {
            evaluate_per_file_entries::<true>(
                root,
                walk_index,
                &live,
                any_full_index,
                filtered_ctx,
                per_file_ctx,
            )
        } else {
            evaluate_per_file_entries::<false>(
                root,
                walk_index,
                &live,
                any_full_index,
                filtered_ctx,
                per_file_ctx,
            )
        };
        if let Some(trace) = trace.filter(|trace| trace.allows(TraceStageClass::Selection)) {
            let reason = if selection_counts.matched == 0 {
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
                .with_counts(selection_counts),
            );
        }

        // Bucket violations by entry-index, then rebuild
        // `RuleResult` per live entry preserving each rule's
        // metadata (level / policy_url / is_fixable).
        let mut bucket: HashMap<usize, Vec<Violation>> = HashMap::new();
        for (idx, v) in by_file {
            bucket.entry(idx).or_default().push(v);
        }
        let mut results = gate_results;
        for (idx, entry) in live {
            // A live per-file rule that produced no violations is a
            // passing rule — emit an empty-violations `RuleResult` so
            // it appears in the pass count, matching the cross-file
            // path (which always emits a result). Previously these
            // were dropped, so a silently-passing per-file rule was
            // missing from "All N rule(s) passed" (the count read as 0).
            let violations = bucket.remove(&idx).unwrap_or_default();
            let (violations, is_fixable) = mark_fixability(violations, entry.rule.fixer());
            let result = RuleResult::new(
                Arc::from(entry.rule.id()),
                entry.rule.level(),
                entry.rule.policy_url().map(Arc::from),
                violations,
                is_fixable,
            );
            trace_assertion(trace, &result);
            results.push((idx, result));
        }
        results
    }

    /// Pre-filter the per-file entries that should evaluate this run:
    /// opt-in via `as_per_file`, not skipped by `--changed`, and `when`
    /// resolved. `when` evaluates against constant facts + vars (no
    /// `iter` namespace at the engine level), so its verdict is
    /// independent of the file being scanned — resolve it once per rule
    /// here rather than per file. A `when` error or a failed
    /// `expect_matches` assertion short-circuits to a per-rule result, matching
    /// the rule-major path's `run_entry` for parity.
    ///
    /// Returns `(live entries, gate results)`. Shared by
    /// [`Engine::run`]'s file-major loop and [`Engine::run_for_file`].
    pub(super) fn collect_live_per_file_entries<'a>(
        &'a self,
        ctx: &Context<'_>,
        gate_inputs: &RunGateInputs<'_, '_>,
        trace: Option<&TraceSession>,
    ) -> LivePerFileEntries<'a> {
        let index = ctx.index;
        let mut live: Vec<(usize, &RuleEntry)> = Vec::new();
        let mut gate_results: Vec<(usize, RuleResult)> = Vec::new();
        for (idx, entry) in self.entries.iter().enumerate() {
            if entry.rule.as_per_file().is_none() {
                continue;
            }
            // A full-index per-file rule is never `--changed`-skipped by its scope:
            // a change outside its scope (a deleted target) can still change its
            // verdict, exactly as for `markdown_links_resolve`.
            let skipped_for_changed = !entry.rule.requires_full_index()
                && self.skip_for_changed(entry.rule.as_ref(), index, None);
            match evaluate_entry_gates(
                idx,
                entry,
                ctx,
                index,
                gate_inputs,
                skipped_for_changed,
                trace,
            ) {
                EntryGateOutcome::Evaluate => live.push((idx, entry)),
                EntryGateOutcome::Skip => {}
                EntryGateOutcome::Result(result) => gate_results.push((idx, result)),
            }
        }
        (live, gate_results)
    }

    pub(super) fn cached_or_evaluate_facts<'a>(
        &self,
        root: &Path,
        index: &'a FileIndex,
    ) -> Result<&'a FactValues> {
        if let Some(values) = index.cached_facts() {
            return Ok(values);
        }
        let computed = evaluate_facts_with_environment(
            &self.facts,
            root,
            index,
            self.process_environment.as_ref(),
        )?;
        index.set_facts(computed);
        Ok(index.cached_facts().expect("facts just set on the index"))
    }

    /// Re-evaluate only the per-file rules that apply to a single file,
    /// using caller-supplied `bytes` (the LSP server's in-memory edited
    /// copy is authoritative for unsaved edits — see
    /// `docs/design/v0.11/single_file_reevaluation.md`). The cost is
    /// proportional to *one* file's evaluation, not the whole tree's, so
    /// an editor can call this on every (debounced) keystroke.
    ///
    /// Cross-file rules (those without an `as_per_file` view) are
    /// intentionally NOT run — the caller re-runs those on save, and
    /// only the ones whose scope intersects the changed file. `when:` is
    /// resolved once against the engine's constant facts/vars, exactly
    /// as [`Engine::run`] does.
    ///
    /// Returns [`Error::FileNotInIndex`] when `file_path` isn't in the
    /// cached `index` — distinct from "ran but found nothing." The
    /// caller reads it as "this file is excluded from linting"
    /// (`.gitignore` / `ignore:` / outside the walked tree).
    pub fn run_for_file(
        &self,
        root: &Path,
        index: &FileIndex,
        file_path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<RuleResult>> {
        if !index.contains_file(file_path) {
            return Err(Error::file_not_in_index(file_path));
        }
        self.enable_relation_caches(index);

        // Facts are constant for an index's lifetime, so cache them: the LSP
        // calls this path on every keystroke and re-scanning would dominate.
        let fact_values = self.cached_or_evaluate_facts(root, index)?;
        let when_env = WhenEnv {
            facts: fact_values,
            vars: &self.vars,
            iter: None,
            env: None,
        };
        let git_required = self.git_requirements(index, &when_env)?;
        let git_tracked = Self::collect_git_tracked_if_needed(root, &git_required);
        let git_available = git_tracked.is_some();
        let git_blame = self.build_blame_cache_if_needed(root, git_tracked.as_ref());
        let analysis_time = self.resolve_analysis_time_if_needed(root);
        // Per-file rules may carry `scope_filter.changed_since:` or manifest
        // scopes; resolve active dependencies before any `Scope::matches`.
        if git_available {
            self.resolve_changed_paths(root, index, &when_env)?;
        }
        self.resolve_manifest_paths(root, index, &when_env, None)?;

        let ctx = Context {
            root,
            index,
            registry: Some(&self.registry),
            facts: Some(fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
            git_blame: git_blame.as_ref(),
            analysis_time: analysis_time.as_ref(),
            process_environment: self.process_environment.as_ref(),
            trace: None,
        };

        let manifest_gates = self.manifest_gates_by_entry(index, &when_env)?;
        let gate_inputs = RunGateInputs {
            when_env: &when_env,
            manifest_gates: &manifest_gates,
            git_required: &git_required,
            git_available,
            changed_active: self.changed_paths.is_some(),
        };
        let (live, gate_results) = self.collect_live_per_file_entries(&ctx, &gate_inputs, None);

        // Dispatch each in-scope rule against the supplied bytes. The
        // raw violations (notes included) are bucketed by entry index;
        // `RuleResult::new` partitions notes out below. A rule that is
        // applicable but emits nothing leaves no bucket entry → no
        // result, matching `run`'s "passing rules omitted" semantics.
        let mut bucket: HashMap<usize, Vec<Violation>> = HashMap::new();
        for (idx, entry) in &live {
            let pf = entry
                .rule
                .as_per_file()
                .expect("live entries are per-file rules by construction");
            if !pf.path_scope().matches(file_path, index) {
                continue;
            }
            match pf.evaluate_file(&ctx, file_path, bytes) {
                Ok(vs) => {
                    if !vs.is_empty() {
                        bucket.entry(*idx).or_default().extend(vs);
                    }
                }
                Err(e) => bucket.entry(*idx).or_default().push(
                    Violation::new(format!("rule evaluation failed: {e}"))
                        .with_path(file_path)
                        .as_analysis_failure(AnalysisFailureKind::Evaluation),
                ),
            }
        }

        let mut by_idx: HashMap<usize, RuleResult> = gate_results.into_iter().collect();
        for (idx, entry) in &live {
            if let Some(violations) = bucket.remove(idx) {
                let (violations, is_fixable) = mark_fixability(violations, entry.rule.fixer());
                by_idx.insert(
                    *idx,
                    RuleResult::new(
                        Arc::from(entry.rule.id()),
                        entry.rule.level(),
                        entry.rule.policy_url().map(Arc::from),
                        violations,
                        is_fixable,
                    ),
                );
            }
        }
        // Preserve `self.entries` order, mirroring `run`'s assembly.
        let mut results = Vec::with_capacity(by_idx.len());
        for idx in 0..self.entries.len() {
            if let Some(mut result) = by_idx.remove(&idx) {
                self.redact_result_for_entry(idx, &mut result);
                results.push(result);
            }
        }
        Ok(results)
    }
}
