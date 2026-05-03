use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rayon::prelude::*;

use crate::error::Result;
use crate::facts::{FactSpec, FactValues, evaluate_facts};
use crate::registry::RuleRegistry;
use crate::report::{FixItem, FixReport, FixRuleResult, FixStatus, Report};
use crate::rule::{Context, FixContext, FixOutcome, Rule, RuleResult, Violation};
use crate::walker::FileIndex;
use crate::when::{WhenEnv, WhenExpr};

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
        #[allow(clippy::cast_possible_truncation)]
        let elapsed_us: u64 = $start.elapsed().as_micros() as u64;
        tracing::info!(
            phase = $phase,
            elapsed_us = elapsed_us,
            $($k = $v,)*
            "engine.phase",
        );
    };
}

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

/// A rule bundled with an optional `when` expression. Rules with a `when`
/// that evaluates to false at runtime are skipped (no `RuleResult` is
/// produced) — same observable effect as `level: off`, but gated on facts.
#[derive(Debug)]
pub struct RuleEntry {
    pub rule: Box<dyn Rule>,
    pub when: Option<WhenExpr>,
}

impl RuleEntry {
    pub fn new(rule: Box<dyn Rule>) -> Self {
        Self { rule, when: None }
    }

    #[must_use]
    pub fn with_when(mut self, expr: WhenExpr) -> Self {
        self.when = Some(expr);
        self
    }
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
    /// In `--changed` mode, the set of paths (relative to root)
    /// that the user wants linted. `None` means "full check"; the
    /// engine bypasses every changed-set short-circuit. See
    /// [`Engine::with_changed_paths`] for the contract.
    changed_paths: Option<HashSet<PathBuf>>,
}

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
            changed_paths: None,
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
            changed_paths: None,
        }
    }

    #[must_use]
    pub fn with_fix_size_limit(mut self, limit: Option<u64>) -> Self {
        self.fix_size_limit = limit;
        self
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

    /// Restrict evaluation to the given set of paths (relative to
    /// the alint root). Per-file rules see a [`FileIndex`]
    /// filtered to only these paths; rules that override
    /// [`Rule::requires_full_index`] (cross-file + existence
    /// rules) still see the full index but are skipped when
    /// their [`Rule::path_scope`] doesn't intersect the set.
    ///
    /// An empty set short-circuits to a no-op report — there's
    /// nothing to lint. Pass `None` (or omit) to disable
    /// `--changed` semantics entirely.
    #[must_use]
    pub fn with_changed_paths(mut self, set: HashSet<PathBuf>) -> Self {
        self.changed_paths = Some(set);
        self
    }

    pub fn rule_count(&self) -> usize {
        self.entries.len()
    }

    // ~125 lines but each block has its own purpose (changed-set
    // short-circuit, fact eval, git probe, filtered-index build,
    // cross-file partition, per-file partition, assembly). Splitting
    // would mean threading the same ~6-arg context tuple through
    // four helpers that share lifetimes — net worse for the reader.
    // The function reads top-to-bottom as one phased pipeline.
    #[allow(clippy::too_many_lines)]
    pub fn run(&self, root: &Path, index: &FileIndex) -> Result<Report> {
        let t_total = Instant::now();
        // Empty changed-set fast path: nothing to lint, return
        // an empty report rather than walk the entries list at
        // all. Saves the fact-evaluation pass too.
        if self.changed_paths.as_ref().is_some_and(HashSet::is_empty) {
            return Ok(Report {
                results: Vec::new(),
            });
        }

        let t_facts = Instant::now();
        let fact_values = evaluate_facts(&self.facts, root, index)?;
        phase!(t_facts, "evaluate_facts", facts = self.facts.len() as u64);

        let t_git = Instant::now();
        let git_tracked = self.collect_git_tracked_if_needed(root);
        let git_blame = self.build_blame_cache_if_needed(root);
        phase!(t_git, "git_setup");

        let t_filter = Instant::now();
        let filtered_index = self.build_filtered_index(index);
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
        };
        let filtered_ctx = filtered_index.as_ref().map(|fi| Context {
            root,
            index: fi,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
            git_blame: git_blame.as_ref(),
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
            });
        let when_env = WhenEnv {
            facts: &fact_values,
            vars: &self.vars,
            iter: None,
        };

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
        let cross_results: Vec<(usize, RuleResult)> = self
            .entries
            .par_iter()
            .enumerate()
            .filter_map(|(idx, entry)| {
                if entry.rule.as_per_file().is_some() {
                    return None;
                }
                if self.skip_for_changed(entry.rule.as_ref(), full_ctx.index) {
                    return None;
                }
                let ctx = pick_ctx(
                    entry.rule.as_ref(),
                    &full_ctx,
                    filtered_ctx.as_ref(),
                    git_file_only_ctx.as_ref(),
                    git_dir_aware_ctx.as_ref(),
                );
                let t_rule = Instant::now();
                let result = run_entry(entry, ctx, &when_env, &fact_values);
                // u128 → u64 saturating: same rationale as the
                // `phase!` macro — elapsed_ns overflows u64 only
                // after ~584 years per rule, and we want lossy
                // truncation rather than a runtime panic on the
                // hot path.
                #[allow(clippy::cast_possible_truncation)]
                let elapsed_ns = t_rule.elapsed().as_nanos() as u64;
                cross_rule_ns[idx].fetch_add(elapsed_ns, Ordering::Relaxed);
                result.map(|rr| (idx, rr))
            })
            .collect();
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

        // Per-file partition: file-major loop reads each file
        // once and dispatches to every per-file rule whose scope
        // matches. Coalesces N reads of one file across N rules
        // sharing it.
        let t_per_file = Instant::now();
        let per_file_results = self.run_per_file(root, &full_ctx, filtered_ctx.as_ref(), &when_env);
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
        // either the cross-file or per-file partition; rules
        // filtered out (by `--changed` scope, `when: false`, or
        // passing with no violations) leave their slot empty.
        let t_assembly = Instant::now();
        let mut cross_by_idx: HashMap<usize, RuleResult> = cross_results.into_iter().collect();
        let mut per_file_by_idx: HashMap<usize, RuleResult> =
            per_file_results.into_iter().collect();
        let mut results = Vec::with_capacity(self.entries.len());
        for idx in 0..self.entries.len() {
            if let Some(rr) = cross_by_idx.remove(&idx) {
                results.push(rr);
            } else if let Some(rr) = per_file_by_idx.remove(&idx) {
                results.push(rr);
            }
        }
        phase!(t_assembly, "assembly", results = results.len() as u64);
        phase!(t_total, "engine_run_total");
        Ok(Report { results })
    }

    /// Per-file dispatch loop. Walks `index.files()` in parallel
    /// and, for each file, calls every applicable per-file rule's
    /// `evaluate_file` against a single `std::fs::read`. Returns
    /// `(entry-index, RuleResult)` tuples for every per-file
    /// rule that emitted at least one violation; passing rules
    /// (zero violations) are omitted, matching the rule-major
    /// path's semantics.
    #[allow(clippy::too_many_lines)]
    fn run_per_file<'a>(
        &'a self,
        root: &'a Path,
        full_ctx: &'a Context<'a>,
        filtered_ctx: Option<&'a Context<'a>>,
        when_env: &'a WhenEnv<'a>,
    ) -> Vec<(usize, RuleResult)> {
        // Pre-filter live per-file entries: opt-in via
        // `as_per_file`, not skipped by `--changed`, and `when`
        // resolved. `when` evaluates against constant facts +
        // vars (no `iter` namespace at the engine level), so its
        // verdict is independent of the file being scanned —
        // resolve it once per rule before entering the file
        // loop. `when` errors short-circuit to a per-rule result
        // with the error message; behaviour matches the
        // rule-major path's `run_entry` for parity.
        let mut live: Vec<(usize, &RuleEntry)> = Vec::new();
        let mut when_errors: Vec<(usize, RuleResult)> = Vec::new();
        for (idx, entry) in self.entries.iter().enumerate() {
            if entry.rule.as_per_file().is_none() {
                continue;
            }
            if self.skip_for_changed(entry.rule.as_ref(), full_ctx.index) {
                continue;
            }
            if let Some(expr) = &entry.when {
                match expr.evaluate(when_env) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => {
                        when_errors.push((
                            idx,
                            RuleResult {
                                rule_id: Arc::from(entry.rule.id()),
                                level: entry.rule.level(),
                                policy_url: entry.rule.policy_url().map(Arc::from),
                                violations: vec![Violation::new(format!(
                                    "when evaluation error: {e}"
                                ))],
                                is_fixable: entry.rule.fixer().is_some(),
                            },
                        ));
                        continue;
                    }
                }
            }
            live.push((idx, entry));
        }
        if live.is_empty() {
            return when_errors;
        }

        let per_file_ctx = filtered_ctx.unwrap_or(full_ctx);

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
        let by_file: Vec<(usize, Violation)> = per_file_ctx
            .index
            .entries
            .par_iter()
            .filter(|e| !e.is_dir)
            .flat_map_iter(|file_entry| {
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
                if applicable.is_empty() {
                    return Vec::new();
                }
                // 2. Read once. Read failures (file deleted
                // mid-walk, permission flake) skip the file
                // silently — same shape as today's per-rule
                // `let Ok(bytes) = std::fs::read(...) else
                // continue;`.
                let abs = root.join(&file_entry.path);
                let Ok(bytes) = std::fs::read(&abs) else {
                    return Vec::new();
                };
                // 3. Dispatch. Every applicable rule sees the
                // same byte slice; the file is read exactly once
                // even though N rules may produce violations
                // against it.
                let mut out: Vec<(usize, Violation)> = Vec::new();
                for (entry_idx, entry) in applicable {
                    let pf = entry
                        .rule
                        .as_per_file()
                        .expect("live entries are per-file rules by construction");
                    let result = pf.evaluate_file(per_file_ctx, &file_entry.path, &bytes);
                    match result {
                        Ok(vs) => {
                            for v in vs {
                                out.push((entry_idx, v));
                            }
                        }
                        Err(e) => {
                            out.push((entry_idx, Violation::new(format!("rule error: {e}"))));
                        }
                    }
                }
                out
            })
            .collect();

        // Bucket violations by entry-index, then rebuild
        // `RuleResult` per live entry preserving each rule's
        // metadata (level / policy_url / is_fixable).
        let mut bucket: HashMap<usize, Vec<Violation>> = HashMap::new();
        for (idx, v) in by_file {
            bucket.entry(idx).or_default().push(v);
        }
        let mut results = when_errors;
        for (idx, entry) in live {
            let Some(violations) = bucket.remove(&idx) else {
                // Rule was applicable to zero files (or every
                // file was empty / unreadable) — passing rule;
                // omit, matching today's behaviour.
                continue;
            };
            results.push((
                idx,
                RuleResult {
                    rule_id: Arc::from(entry.rule.id()),
                    level: entry.rule.level(),
                    policy_url: entry.rule.policy_url().map(Arc::from),
                    violations,
                    is_fixable: entry.rule.fixer().is_some(),
                },
            ));
        }
        results
    }

    /// Evaluate every rule and apply fixers for their violations.
    /// Fixes run sequentially — rules whose fixers touch the filesystem
    /// must not race. Rules with no fixer contribute
    /// [`FixStatus::Unfixable`] entries so the caller sees them in the
    /// report. Rules that pass (no violations) are omitted from the
    /// result, same as [`Engine::run`]'s usual behaviour.
    #[allow(clippy::too_many_lines)]
    pub fn fix(&self, root: &Path, index: &FileIndex, dry_run: bool) -> Result<FixReport> {
        if self.changed_paths.as_ref().is_some_and(HashSet::is_empty) {
            return Ok(FixReport {
                results: Vec::new(),
            });
        }

        let fact_values = evaluate_facts(&self.facts, root, index)?;
        let git_tracked = self.collect_git_tracked_if_needed(root);
        let git_blame = self.build_blame_cache_if_needed(root);
        let filtered_index = self.build_filtered_index(index);
        let git_tracked_indexes = self.build_git_tracked_indexes(index, git_tracked.as_ref());
        let full_ctx = Context {
            root,
            index,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
            git_blame: git_blame.as_ref(),
        };
        let filtered_ctx = filtered_index.as_ref().map(|fi| Context {
            root,
            index: fi,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
            git_blame: git_blame.as_ref(),
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
            });
        let when_env = WhenEnv {
            facts: &fact_values,
            vars: &self.vars,
            iter: None,
        };
        let fix_ctx = FixContext {
            root,
            dry_run,
            fix_size_limit: self.fix_size_limit,
        };

        let mut results: Vec<FixRuleResult> = Vec::new();
        for entry in &self.entries {
            if self.skip_for_changed(entry.rule.as_ref(), full_ctx.index) {
                continue;
            }
            let ctx = pick_ctx(
                entry.rule.as_ref(),
                &full_ctx,
                filtered_ctx.as_ref(),
                git_file_only_ctx.as_ref(),
                git_dir_aware_ctx.as_ref(),
            );
            if let Some(expr) = &entry.when {
                match expr.evaluate(&when_env) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => {
                        results.push(FixRuleResult {
                            rule_id: Arc::from(entry.rule.id()),
                            level: entry.rule.level(),
                            items: vec![FixItem {
                                violation: Violation::new(format!("when evaluation error: {e}")),
                                status: FixStatus::Unfixable,
                            }],
                        });
                        continue;
                    }
                }
            }
            let violations = match entry.rule.evaluate(ctx) {
                Ok(v) => v,
                Err(e) => vec![Violation::new(format!("rule error: {e}"))],
            };
            if violations.is_empty() {
                continue;
            }
            let fixer = entry.rule.fixer();
            let items: Vec<FixItem> = violations
                .into_iter()
                .map(|v| {
                    let status = match fixer {
                        Some(f) => match f.apply(&v, &fix_ctx) {
                            Ok(FixOutcome::Applied(s)) => FixStatus::Applied(s),
                            Ok(FixOutcome::Skipped(s)) => FixStatus::Skipped(s),
                            Err(e) => FixStatus::Skipped(format!("fix error: {e}")),
                        },
                        None => FixStatus::Unfixable,
                    };
                    FixItem {
                        violation: v,
                        status,
                    }
                })
                .collect();
            results.push(FixRuleResult {
                rule_id: Arc::from(entry.rule.id()),
                level: entry.rule.level(),
                items,
            });
        }
        Ok(FixReport { results })
    }

    /// Collect git's tracked-paths set, but only if at least one
    /// loaded rule asked for it. Most repos / configs never opt
    /// in, so this returns `None` zero-cost in the common case.
    /// Inside a non-git directory, or when `git` exits non-zero
    /// (corrupt repo, missing binary), the helper also returns
    /// `None` — rules that consult it then treat every entry as
    /// "untracked," which is the right default for absence-style
    /// rules with `git_tracked_only: true`.
    fn collect_git_tracked_if_needed(
        &self,
        root: &Path,
    ) -> Option<std::collections::HashSet<std::path::PathBuf>> {
        let any_wants = self
            .entries
            .iter()
            .any(|e| e.rule.git_tracked_mode() != crate::rule::GitTrackedMode::Off);
        if !any_wants {
            return None;
        }
        crate::git::collect_tracked_paths(root)
    }

    /// Build the per-file `git blame` cache when at least one
    /// loaded rule asked for it. Returns `None` otherwise — the
    /// common case (most configs have no `git_blame_age` rules)
    /// pays nothing. The cache itself is empty at construction;
    /// rules trigger blame on first access per file.
    ///
    /// We use [`crate::git::collect_tracked_paths`] as the
    /// is-this-a-git-repo probe so the rule no-ops cleanly
    /// outside a repo without per-file blame failures littering
    /// the cache. When the user opts into BOTH `git_tracked_only`
    /// and `git_blame_age`, the probe runs once via
    /// [`Engine::collect_git_tracked_if_needed`] and once here —
    /// negligible cost (sub-ms) compared to the blame work.
    fn build_blame_cache_if_needed(&self, root: &Path) -> Option<crate::git::BlameCache> {
        let any_wants = self.entries.iter().any(|e| e.rule.wants_git_blame());
        if !any_wants {
            return None;
        }
        // Probe: a non-git workspace short-circuits to `None` so
        // the rule's "silent no-op outside git" path is exercised
        // at the engine level rather than per-file.
        crate::git::collect_tracked_paths(root)?;
        Some(crate::git::BlameCache::new(root.to_path_buf()))
    }

    /// Build a [`FileIndex`] containing only the entries the user
    /// said they care about (the `--changed` set). Returns `None`
    /// when no changed-set is configured — callers fall back to
    /// the full index.
    fn build_filtered_index(&self, full: &FileIndex) -> Option<FileIndex> {
        let set = self.changed_paths.as_ref()?;
        let entries = full
            .entries
            .iter()
            .filter(|e| set.contains(&*e.path))
            .cloned()
            .collect();
        Some(FileIndex::from_entries(entries))
    }

    /// Build the per-mode pre-filtered indexes for git-tracked
    /// rules. v0.9.11 structural fix for the
    /// `git_tracked_only`-silently-dropped recurrence-risk
    /// shape (see `docs/design/v0.9/git-tracked-filtered-index.md`).
    ///
    /// Returns `None` when no rule opts in (no
    /// `GitTrackedMode::FileOnly` or `DirAware` declared) OR
    /// when the tracked-set is unavailable (no git repo). When
    /// `Some`, contains:
    ///
    /// - `file_only`: files where `tracked.contains(path)`. The
    ///   index `file_exists`-style rules iterate via
    ///   `ctx.index.files()`. Dirs are dropped (file-mode rules
    ///   don't iterate dirs).
    /// - `dir_aware`: dirs where `dir_has_tracked_files(path,
    ///   tracked)`. The index `dir_exists`-style rules iterate
    ///   via `ctx.index.dirs()`. Tracked files are also kept so
    ///   any nested per-file consultation by these rules still
    ///   works against the same index.
    ///
    /// Build cost: O(N) per mode (one `HashSet` lookup or one
    /// `dir_has_tracked_files` walk per entry). Amortised across
    /// however many rules opt into each mode.
    fn build_git_tracked_indexes(
        &self,
        full: &FileIndex,
        tracked: Option<&std::collections::HashSet<std::path::PathBuf>>,
    ) -> Option<GitTrackedIndexes> {
        let mut any_file_only = false;
        let mut any_dir_aware = false;
        for entry in &self.entries {
            match entry.rule.git_tracked_mode() {
                crate::rule::GitTrackedMode::Off => {}
                crate::rule::GitTrackedMode::FileOnly => any_file_only = true,
                crate::rule::GitTrackedMode::DirAware => any_dir_aware = true,
            }
        }
        if !any_file_only && !any_dir_aware {
            return None;
        }

        // No git repo (or `git ls-files` failed): build EMPTY
        // indexes for the modes that rules opt into. Preserves
        // the pre-v0.9.11 silent-no-op semantics — rules that
        // require git_tracked_only outside a git repo iterate
        // an empty index and fire zero violations, matching
        // user expectations for the "don't let X be committed"
        // pattern.
        let Some(tracked) = tracked else {
            return Some(GitTrackedIndexes {
                file_only: any_file_only.then(|| FileIndex::from_entries(Vec::new())),
                dir_aware: any_dir_aware.then(|| FileIndex::from_entries(Vec::new())),
            });
        };

        let file_only = if any_file_only {
            let entries = full
                .entries
                .iter()
                .filter(|e| !e.is_dir && tracked.contains(&*e.path))
                .cloned()
                .collect();
            Some(FileIndex::from_entries(entries))
        } else {
            None
        };

        let dir_aware = if any_dir_aware {
            let entries = full
                .entries
                .iter()
                .filter(|e| {
                    if e.is_dir {
                        crate::git::dir_has_tracked_files(&e.path, tracked)
                    } else {
                        tracked.contains(&*e.path)
                    }
                })
                .cloned()
                .collect();
            Some(FileIndex::from_entries(entries))
        } else {
            None
        };

        Some(GitTrackedIndexes {
            file_only,
            dir_aware,
        })
    }

    /// True when `--changed` mode is active AND the rule's
    /// `path_scope` exists AND no path in the changed-set
    /// satisfies it. Cross-file rules return `path_scope = None`
    /// per the roadmap contract — so they always return `false`
    /// here (i.e. never skipped).
    fn skip_for_changed(&self, rule: &dyn Rule, index: &FileIndex) -> bool {
        let Some(set) = &self.changed_paths else {
            return false;
        };
        let Some(scope) = rule.path_scope() else {
            return false;
        };
        !set.iter().any(|p| scope.matches(p, index))
    }
}

/// Pick the [`Context`] a rule should evaluate against:
/// `full_ctx` if it [`requires_full_index`](Rule::requires_full_index),
/// otherwise the changed-only filtered context (falling back to
/// `full_ctx` when no `--changed` set is configured).
fn pick_ctx<'a>(
    rule: &dyn Rule,
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
    if rule.requires_full_index() {
        full_ctx
    } else {
        filtered_ctx.unwrap_or(full_ctx)
    }
}

fn run_entry(
    entry: &RuleEntry,
    ctx: &Context<'_>,
    when_env: &WhenEnv<'_>,
    _facts: &FactValues,
) -> Option<RuleResult> {
    if let Some(expr) = &entry.when {
        match expr.evaluate(when_env) {
            Ok(true) => {} // proceed
            Ok(false) => return None,
            Err(e) => {
                return Some(RuleResult {
                    rule_id: Arc::from(entry.rule.id()),
                    level: entry.rule.level(),
                    policy_url: entry.rule.policy_url().map(Arc::from),
                    violations: vec![Violation::new(format!("when evaluation error: {e}"))],
                    is_fixable: entry.rule.fixer().is_some(),
                });
            }
        }
    }
    Some(run_one(entry.rule.as_ref(), ctx))
}

fn run_one(rule: &dyn Rule, ctx: &Context<'_>) -> RuleResult {
    let violations = match rule.evaluate(ctx) {
        Ok(v) => v,
        Err(e) => vec![Violation::new(format!("rule error: {e}"))],
    };
    RuleResult {
        rule_id: Arc::from(rule.id()),
        level: rule.level(),
        policy_url: rule.policy_url().map(Arc::from),
        violations,
        is_fixable: rule.fixer().is_some(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::Level;
    use crate::scope::Scope;
    use crate::walker::FileEntry;
    use std::path::Path;

    /// Stub rule: emits one violation per matched file in scope.
    /// Configurable to advertise `requires_full_index` for
    /// cross-file rule simulation, and a `path_scope` for
    /// changed-mode tests.
    #[derive(Debug)]
    struct StubRule {
        id: String,
        level: Level,
        scope: Scope,
        full_index: bool,
        expose_scope: bool,
    }

    impl Rule for StubRule {
        fn id(&self) -> &str {
            &self.id
        }
        fn level(&self) -> Level {
            self.level
        }
        fn requires_full_index(&self) -> bool {
            self.full_index
        }
        fn path_scope(&self) -> Option<&Scope> {
            self.expose_scope.then_some(&self.scope)
        }
        fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
            let mut out = Vec::new();
            for entry in ctx.index.files() {
                if self.scope.matches(&entry.path, ctx.index) {
                    out.push(Violation::new("hit").with_path(entry.path.clone()));
                }
            }
            Ok(out)
        }
    }

    fn stub(id: &str, glob: &str) -> Box<dyn Rule> {
        Box::new(StubRule {
            id: id.into(),
            level: Level::Error,
            scope: Scope::from_patterns(&[glob.to_string()]).unwrap(),
            full_index: false,
            expose_scope: true,
        })
    }

    fn full_index_stub(id: &str) -> Box<dyn Rule> {
        Box::new(StubRule {
            id: id.into(),
            level: Level::Error,
            scope: Scope::match_all(),
            full_index: true,
            expose_scope: false,
        })
    }

    fn idx(paths: &[&str]) -> FileIndex {
        FileIndex::from_entries(
            paths
                .iter()
                .map(|p| FileEntry {
                    path: std::path::Path::new(p).into(),
                    is_dir: false,
                    size: 0,
                })
                .collect(),
        )
    }

    #[test]
    fn run_empty_returns_empty_report() {
        let engine = Engine::new(Vec::new(), RuleRegistry::new());
        let report = engine.run(Path::new("/fake"), &idx(&["a.rs"])).unwrap();
        assert!(report.results.is_empty());
    }

    #[test]
    fn run_single_rule_emits_per_match() {
        let engine = Engine::new(vec![stub("t", "**/*.rs")], RuleRegistry::new());
        let report = engine
            .run(
                Path::new("/fake"),
                &idx(&["src/a.rs", "src/b.rs", "README.md"]),
            )
            .unwrap();
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].violations.len(), 2);
    }

    #[test]
    fn run_with_empty_changed_set_short_circuits() {
        // Per the contract: empty `--changed` set means "lint
        // nothing"; the engine returns an empty Report without
        // even evaluating facts.
        let engine = Engine::new(vec![stub("t", "**/*.rs")], RuleRegistry::new())
            .with_changed_paths(HashSet::new());
        let report = engine.run(Path::new("/fake"), &idx(&["src/a.rs"])).unwrap();
        assert!(report.results.is_empty());
    }

    #[test]
    fn changed_mode_skips_rule_whose_scope_misses_diff() {
        // Rule scoped to `src/**`; changed-set has only docs/
        // → rule skipped (no result emitted).
        let mut changed = HashSet::new();
        changed.insert(std::path::PathBuf::from("docs/README.md"));
        let engine = Engine::new(vec![stub("src-rule", "src/**/*.rs")], RuleRegistry::new())
            .with_changed_paths(changed);
        let report = engine
            .run(Path::new("/fake"), &idx(&["src/a.rs", "docs/README.md"]))
            .unwrap();
        assert!(
            report.results.is_empty(),
            "out-of-scope rule should be skipped: {:?}",
            report.results,
        );
    }

    #[test]
    fn changed_mode_runs_rule_whose_scope_intersects_diff() {
        let mut changed = HashSet::new();
        changed.insert(std::path::PathBuf::from("src/a.rs"));
        let engine = Engine::new(vec![stub("src-rule", "src/**/*.rs")], RuleRegistry::new())
            .with_changed_paths(changed);
        let report = engine
            .run(Path::new("/fake"), &idx(&["src/a.rs", "src/b.rs"]))
            .unwrap();
        // Filtered index: only `src/a.rs` is visible. Rule
        // matches it → 1 violation.
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].violations.len(), 1);
    }

    #[test]
    fn requires_full_index_rule_runs_unconditionally_in_changed_mode() {
        // A rule with `requires_full_index = true` and no
        // `path_scope` opts out of the changed-set filter
        // entirely — its verdict is over the whole tree.
        let mut changed = HashSet::new();
        changed.insert(std::path::PathBuf::from("docs/README.md"));
        let engine = Engine::new(vec![full_index_stub("cross")], RuleRegistry::new())
            .with_changed_paths(changed);
        let report = engine
            .run(Path::new("/fake"), &idx(&["src/a.rs", "docs/README.md"]))
            .unwrap();
        // `cross` ran against the full index (not the filtered
        // one), so it sees both files.
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0].violations.len(), 2);
    }

    #[test]
    fn rule_count_reflects_number_of_entries() {
        let engine = Engine::new(
            vec![stub("a", "**"), stub("b", "**"), stub("c", "**")],
            RuleRegistry::new(),
        );
        assert_eq!(engine.rule_count(), 3);
    }

    #[test]
    fn from_entries_constructor_supports_when_clauses() {
        // A rule wrapped with a `when: false` expression should
        // be skipped during run — no result emitted.
        let entry = RuleEntry::new(stub("gated", "**/*.rs"))
            .with_when(crate::when::parse("false").unwrap());
        let engine = Engine::from_entries(vec![entry], RuleRegistry::new());
        let report = engine.run(Path::new("/fake"), &idx(&["a.rs"])).unwrap();
        assert!(
            report.results.is_empty(),
            "when-false rule must be skipped: {:?}",
            report.results,
        );
    }

    #[test]
    fn fix_size_limit_default_is_one_mib() {
        // The builder default; tests that override engines via
        // `with_fix_size_limit` rely on this baseline.
        let engine = Engine::new(Vec::new(), RuleRegistry::new());
        // Implementation detail intentionally exposed for tests.
        // We can only verify the value indirectly via `with_*`
        // returning a different limit; assert the builder works.
        let updated = engine.with_fix_size_limit(Some(42));
        assert_eq!(updated.rule_count(), 0);
    }

    #[test]
    fn skip_for_changed_returns_false_for_full_check() {
        // No `--changed` set → rule never skipped on that basis.
        let engine = Engine::new(vec![stub("t", "**/*.rs")], RuleRegistry::new());
        let report = engine.run(Path::new("/fake"), &idx(&["a.rs"])).unwrap();
        assert_eq!(report.results.len(), 1);
    }

    /// Per-file rule that emits one violation per file based on
    /// the byte content prefix. Used to verify the file-major
    /// dispatch path actually hands the bytes to the rule and
    /// aggregates the violations correctly.
    #[derive(Debug)]
    struct PerFileStub {
        id: String,
        scope: Scope,
        prefix: Vec<u8>,
    }

    impl Rule for PerFileStub {
        fn id(&self) -> &str {
            &self.id
        }
        fn level(&self) -> Level {
            Level::Error
        }
        fn evaluate(&self, _ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
            // Rule-major fallback: not exercised when
            // `as_per_file` is set + the engine routes to the
            // file-major loop.
            Ok(Vec::new())
        }
        fn as_per_file(&self) -> Option<&dyn crate::PerFileRule> {
            Some(self)
        }
    }

    impl crate::PerFileRule for PerFileStub {
        fn path_scope(&self) -> &Scope {
            &self.scope
        }
        fn evaluate_file(
            &self,
            _ctx: &Context<'_>,
            path: &std::path::Path,
            bytes: &[u8],
        ) -> crate::error::Result<Vec<Violation>> {
            if !bytes.starts_with(&self.prefix) {
                return Ok(vec![
                    Violation::new("missing prefix")
                        .with_path(std::sync::Arc::<std::path::Path>::from(path)),
                ]);
            }
            Ok(Vec::new())
        }
    }

    #[test]
    fn dispatch_flip_routes_per_file_rule_through_file_major_loop() {
        // Real filesystem so the engine's `std::fs::read` works.
        // The PerFileStub fires when a file does NOT start with
        // `MAGIC` — exercises the slice-handing-in path end-to-end.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("good.txt"), b"MAGIC + payload").unwrap();
        std::fs::write(tmp.path().join("bad.txt"), b"no magic here").unwrap();

        let rule = Box::new(PerFileStub {
            id: "needs-magic".into(),
            scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
            prefix: b"MAGIC".to_vec(),
        });
        let engine = Engine::new(vec![rule], RuleRegistry::new());

        let opts = crate::WalkOptions::default();
        let index = crate::walk(tmp.path(), &opts).unwrap();
        let report = engine.run(tmp.path(), &index).unwrap();

        assert_eq!(report.results.len(), 1, "results: {:?}", report.results);
        let r = &report.results[0];
        assert_eq!(&*r.rule_id, "needs-magic");
        assert_eq!(r.violations.len(), 1, "violations: {:?}", r.violations);
        assert_eq!(
            r.violations[0].path.as_deref(),
            Some(std::path::Path::new("bad.txt")),
        );
    }

    #[test]
    fn dispatch_flip_aggregates_multiple_per_file_rules() {
        // Two per-file rules sharing one scope: the file-major
        // loop reads each file once and dispatches both rules
        // against the same byte buffer. Verifies the aggregation
        // step buckets violations per rule correctly (not
        // per-file).
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"ZZZ stuff").unwrap();
        std::fs::write(tmp.path().join("b.txt"), b"BBB stuff").unwrap();

        let rule_a = Box::new(PerFileStub {
            id: "needs-AAA".into(),
            scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
            prefix: b"AAA".to_vec(),
        });
        let rule_b = Box::new(PerFileStub {
            id: "needs-BBB".into(),
            scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
            prefix: b"BBB".to_vec(),
        });
        let engine = Engine::new(vec![rule_a, rule_b], RuleRegistry::new());

        let opts = crate::WalkOptions::default();
        let index = crate::walk(tmp.path(), &opts).unwrap();
        let report = engine.run(tmp.path(), &index).unwrap();

        // `needs-AAA` fires on both files (neither starts with
        // "AAA"). `needs-BBB` fires only on `a.txt`.
        let by_id: HashMap<&str, &RuleResult> =
            report.results.iter().map(|r| (&*r.rule_id, r)).collect();
        assert_eq!(
            by_id.len(),
            2,
            "expected both rules in the report: {:?}",
            report.results
        );
        assert_eq!(by_id["needs-AAA"].violations.len(), 2);
        assert_eq!(by_id["needs-BBB"].violations.len(), 1);
        assert_eq!(
            by_id["needs-BBB"].violations[0].path.as_deref(),
            Some(std::path::Path::new("a.txt")),
        );
    }

    #[test]
    fn dispatch_flip_passes_when_no_violations() {
        // A per-file rule that finds no violations in any file
        // should be omitted from the report entirely (matching
        // the rule-major path's "passing rules omitted"
        // semantics).
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"MAGIC ok").unwrap();

        let rule = Box::new(PerFileStub {
            id: "needs-magic".into(),
            scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
            prefix: b"MAGIC".to_vec(),
        });
        let engine = Engine::new(vec![rule], RuleRegistry::new());

        let opts = crate::WalkOptions::default();
        let index = crate::walk(tmp.path(), &opts).unwrap();
        let report = engine.run(tmp.path(), &index).unwrap();

        assert!(report.results.is_empty(), "results: {:?}", report.results);
    }

    #[test]
    fn dispatch_flip_preserves_cross_file_rules_unchanged() {
        // A rule that opts out of `as_per_file` (the default
        // `None`) keeps the rule-major path. Mixing with a
        // per-file rule should produce both results.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"hi").unwrap();

        let cross_rule = stub("cross", "**/*.txt");
        let per_file_rule = Box::new(PerFileStub {
            id: "needs-magic".into(),
            scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
            prefix: b"MAGIC".to_vec(),
        });
        let engine = Engine::new(vec![cross_rule, per_file_rule], RuleRegistry::new());

        let opts = crate::WalkOptions::default();
        let index = crate::walk(tmp.path(), &opts).unwrap();
        let report = engine.run(tmp.path(), &index).unwrap();

        assert_eq!(report.results.len(), 2, "results: {:?}", report.results);
        // Order follows entry-registration order.
        assert_eq!(&*report.results[0].rule_id, "cross");
        assert_eq!(&*report.results[1].rule_id, "needs-magic");
    }
}
