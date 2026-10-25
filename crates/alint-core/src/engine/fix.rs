//! Fixpoint execution and staged-fix previews.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::facts::evaluate_facts_with_environment;
use crate::located_fix::{self, LocatedEdit};
use crate::report::{FIX_ERROR_PREFIX, FixItem, FixReport, FixRuleResult, FixStatus};
use crate::rule::{
    AnalysisFailureKind, Applicability, Context, FixContext, FixEdit, FixOutcome, Fixer,
    PerFileRule, ReadForFix, Rule, RuleResult, Violation, read_for_fix, write_atomic,
};
use crate::walker::FileIndex;
use crate::when::WhenEnv;

use super::{
    Engine, FixRunOutput, RuleEntry, StagedFix, StagedKind, is_unfixable_finding, located_status,
    pick_ctx, push_fix_item, push_fix_result, unavailable_git_failure,
};

impl Engine {
    /// Evaluate every rule and apply fixers for their violations.
    /// Fixes run sequentially — rules whose fixers touch the filesystem
    /// must not race. Rules with no fixer contribute
    /// [`FixStatus::Unfixable`] entries so the caller sees them in the
    /// report. Rules that pass (no violations) are omitted from the
    /// result, same as [`Engine::run`]'s usual behaviour.
    ///
    /// `threshold` is the highest [`Applicability`] tier the caller opted into
    /// applying (`Safe` for a bare `alint fix`, `Unsafe` for `--unsafe-fixes`);
    /// it gates the located-edit regime. No Phase-0 op consults it (all are
    /// `Safe` and whole-file), but it is live, not inert: the dormant located
    /// pass below is threshold-driven.
    #[allow(clippy::too_many_lines)]
    pub fn fix(
        &self,
        root: &Path,
        index: &FileIndex,
        walk_opts: &crate::WalkOptions,
        dry_run: bool,
        threshold: Applicability,
    ) -> Result<FixReport> {
        // Non-convergence cap (docs/design/v0.17/fixpoint.md 2): hard-stop after
        // this many re-walk passes and report exit 2. Declared first so the
        // `items_after_statements` lint stays quiet.
        const MAX_PASSES: usize = 10;
        self.ensure_manifest_scope_resolvable()?;
        if self.changed_paths.as_ref().is_some_and(HashSet::is_empty)
            && !self.any_expect_matches()?
        {
            return Ok(FixReport {
                results: Vec::new(),
                non_convergent: false,
            });
        }
        let analysis_time = self.resolve_analysis_time_if_needed(root);
        if dry_run {
            // Single-pass preview: a dry run writes nothing, so there is no
            // changed tree to re-walk. It shows the first pass; a real `fix` may
            // do more (docs/design/v0.17/fixpoint.md 2). No stage sink.
            let (mut report, _, _) = self.fix_run(
                root,
                index,
                &std::collections::HashSet::new(),
                true,
                threshold,
                analysis_time.as_ref(),
                None,
                None,
            )?;
            self.predict_unresolved_creates(root, index, walk_opts, &mut report);
            Self::sanitize_fix_report(&mut report);
            return Ok(report);
        }
        // Byte-level fixpoint (docs/design/v0.17/fixpoint.md): re-walk and re-fix
        // until a pass applies nothing (`applied() == 0`), bounded by MAX_PASSES.
        // There is no apply-once heuristic: a fix's OWN idempotence is what makes
        // a pass stop applying -- whole-file fixers return `Skipped` when there is
        // nothing left to do, `replace` drops a self-reintroducing match, and a
        // located batch that nets no byte change reports `Skipped` (below). So a
        // normalizing / self-guarding fixer converges in a pass or two, a `replace`
        // that re-forms a match across a splice boundary keeps making progress
        // until the file is clean, and a config that genuinely never settles keeps
        // changing the tree until the cap fires -- reported as non-convergence
        // (exit 2), never silently "applied" while a violation stands.
        let mut results: Vec<FixRuleResult> = Vec::new();
        // Violation keys whose fix has already `Applied` in an earlier pass; a
        // later pass must not overwrite that outcome with a "nothing to do" skip
        // (the fix DID land). See the report rule below.
        let mut locked: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut owned_index: Option<FileIndex> = None;
        let mut converged = false;
        // The rules that APPLIED something in the most recent pass. On a cap-out
        // these are the culprits: a pass that applies nothing converges and
        // breaks, so the final pass of a non-convergent run always applied >=1.
        let mut last_applied_rules: Vec<Arc<str>> = Vec::new();
        // Violation keys the CONVERGED pass still saw (its residual). Used to
        // reconcile the aggregate below: a non-Applied item whose violation is
        // gone by convergence (resolved by another rule's fix on a later pass)
        // must be dropped, or `fix` reports a phantom finding and fails an
        // otherwise-clean tree.
        let mut final_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
        // `--changed` created-file tracking (2b): a path present now that was NOT in
        // the pre-fix tree was made by a fix in scope this run. Such files join the
        // effective changed set each pass so a create-then-fix cascade can complete
        // (they are not in the git diff, so per-file rules would otherwise stay
        // confined away from them). Empty and unused when `--changed` is inactive;
        // recomputed from each re-walk against the original paths, so a file a later
        // pass deletes drops back out.
        let track_created = self.changed_paths.is_some();
        let orig_paths: std::collections::HashSet<PathBuf> = if track_created {
            index.entries.iter().map(|e| e.path.to_path_buf()).collect()
        } else {
            std::collections::HashSet::new()
        };
        let mut created: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        // Write targets whose failure was already reported: a persistent failure
        // (a read-only file) re-fires on every pass, but is printed once.
        let write_failures_reported = RefCell::new(HashSet::new());
        for _pass in 0..MAX_PASSES {
            let cur = owned_index.as_ref().unwrap_or(index);
            let (report, _buf, _) = self.fix_run(
                root,
                cur,
                &created,
                false,
                threshold,
                analysis_time.as_ref(),
                Some(&write_failures_reported),
                None,
            )?;
            // F4 tripwire (audit 2026-09-20): the cross-pass merge below keys items
            // by `violation_key`, which for a path-bearing, keyless violation
            // collapses to `(rule_id, path)` (line/column/message ignored). Every
            // SHIPPED fixable rule is safe: a multi-finding fixable rule
            // (`*_path_matches`) sets a per-value `baseline_key` AND its
            // identical-value findings are truly indistinguishable (no line), while
            // the first-offender / whole-file rules emit one finding per file. A
            // FUTURE multi-finding fixable rule that distinguishes its findings only
            // by line/column/message under a COARSE (path) key would let one item
            // silently supersede another in the merge. Fail loudly in debug/tests
            // before it ships: within one rule result, two items whose violations
            // differ must not share a `violation_key`.
            #[cfg(debug_assertions)]
            for rr in &report.results {
                // Only FIXABLE rules are merged/locked by violation_key; a
                // non-fixable rule's items are appended without dedup and never
                // Applied, so a key collision among its findings is benign (they
                // all survive the reconciliation). Guarding on all rules panicked
                // `fix` on valid multi-finding keyless NON-fixable rules
                // (for_each_match, import_gate, ...) -- audit 2026-09-21.
                if self.fixer_for(&rr.rule_id).is_none() {
                    continue;
                }
                let mut seen: HashMap<String, (Option<usize>, Option<usize>, &str)> =
                    HashMap::new();
                for it in &rr.items {
                    let id = (
                        it.violation.line,
                        it.violation.column,
                        it.violation.message.as_ref(),
                    );
                    let key = Self::violation_key(&rr.rule_id, &it.violation);
                    if let Some(prev) = seen.insert(key, id) {
                        assert!(
                            prev == id,
                            "fixable rule {:?} emits distinct violations that collide on \
                             violation_key ({prev:?} vs {id:?}); give them a distinguishing \
                             baseline_key or the fixpoint merge will drop one (audit F4)",
                            rr.rule_id
                        );
                    }
                }
            }
            let applied = report.applied();
            last_applied_rules = report
                .results
                .iter()
                .filter(|rr| {
                    rr.items
                        .iter()
                        .any(|i| matches!(i.status, FixStatus::Applied(_)))
                })
                .map(|rr| Arc::clone(&rr.rule_id))
                .collect();
            let all_keys: std::collections::HashSet<String> = report
                .results
                .iter()
                .flat_map(|rr| {
                    rr.items
                        .iter()
                        .map(move |it| Self::violation_key(&rr.rule_id, &it.violation))
                })
                .collect();
            // Merge this pass into the aggregate (docs/design/v0.17/fixpoint.md 7).
            // An `Applied` for a violation WINS and is locked: a later pass that
            // finds nothing left to do (its idempotence guard yields a `Skipped`)
            // must not mask the fix that actually landed. A violation never yet
            // Applied keeps its LAST status, so a transient state is superseded by
            // the real outcome once a later pass acts on it. Items sharing one
            // violation identity WITHIN a pass (a located `Applied` plus its
            // isolation-conflict `Skipped`) are all kept: the lock is applied only
            // AFTER this pass's items are merged, so the sibling skip is not
            // dropped, and it does not re-appear (the located batch that produced
            // it made progress, so the next pass's re-eval differs).
            let touched: std::collections::HashSet<&String> =
                all_keys.iter().filter(|k| !locked.contains(*k)).collect();
            for rr in &mut results {
                rr.items.retain(|it| {
                    !touched.contains(&Self::violation_key(&rr.rule_id, &it.violation))
                });
            }
            // Failures describe the current evaluation pass, not a historical
            // fix action. Clear the prior pass, then install this pass's records
            // while merging fix items under their existing keep-last contract.
            for rr in &mut results {
                rr.analysis_failures.clear();
            }
            let mut newly_locked: Vec<String> = Vec::new();
            for mut rr in report.results {
                let failures = std::mem::take(&mut rr.analysis_failures);
                if !failures.is_empty() {
                    push_fix_result(
                        &mut results,
                        Arc::clone(&rr.rule_id),
                        rr.level,
                        Vec::new(),
                        failures,
                    );
                }
                for item in rr.items {
                    let key = Self::violation_key(&rr.rule_id, &item.violation);
                    if locked.contains(&key) {
                        continue; // an earlier pass's `Applied` for this violation wins
                    }
                    if matches!(item.status, FixStatus::Applied(_)) {
                        newly_locked.push(key);
                    }
                    push_fix_item(&mut results, &rr.rule_id, rr.level, item);
                }
            }
            locked.extend(newly_locked);
            if applied == 0 {
                converged = true;
                final_keys = all_keys;
                break;
            }
            // Something landed; re-walk so the next pass sees the new tree.
            owned_index = Some(crate::walker::walk(root, walk_opts)?);
            if track_created {
                // Recompute the created set from the fresh tree: any path not in the
                // pre-fix tree was created by a fix this run (2b). Recomputing (vs.
                // accumulating) means a file a later pass DELETES drops back out.
                let fresh = owned_index.as_ref().expect("just assigned");
                created = fresh
                    .entries
                    .iter()
                    .map(|e| e.path.to_path_buf())
                    .filter(|p| !orig_paths.contains(p))
                    .collect();
            }
        }
        // Budget exhausted without an `applied()==0` pass. A slow-but-convergent
        // config may have reached a fixed point on the very last budgeted pass
        // (there was no room for the confirming pass). Confirm with a NON-MUTATING
        // dry-run: if it would apply nothing, the fix DID converge -- do not brand
        // a clean tree non-convergent (which would exit 2). A genuinely
        // non-convergent config (fixes undoing each other) still shows work here
        // and stays non-convergent.
        if !converged {
            let cur = owned_index.as_ref().unwrap_or(index);
            let (confirm, _, _) = self.fix_run(
                root,
                cur,
                &created,
                true,
                threshold,
                analysis_time.as_ref(),
                None,
                None,
            )?;
            let confirm_applied = confirm.applied();
            let confirm_keys: std::collections::HashSet<String> = confirm
                .results
                .iter()
                .flat_map(|rr| {
                    rr.items
                        .iter()
                        .map(move |it| Self::violation_key(&rr.rule_id, &it.violation))
                })
                .collect();
            // The confirmation is the only evaluation of the final on-disk tree.
            // Failures are current state, not an audit trail: replace the tenth
            // pass's records even when the confirmation still finds work.
            for rr in &mut results {
                rr.analysis_failures.clear();
            }
            for mut rr in confirm.results {
                let failures = std::mem::take(&mut rr.analysis_failures);
                if !failures.is_empty() {
                    push_fix_result(
                        &mut results,
                        Arc::clone(&rr.rule_id),
                        rr.level,
                        Vec::new(),
                        failures,
                    );
                }
            }
            if confirm_applied == 0 {
                converged = true;
                final_keys = confirm_keys;
            }
        }
        // Reconcile the residual against the converged final state (see
        // `final_keys` above): keep every `Applied` (the audit trail of what
        // changed) and every non-Applied item whose violation the final pass still
        // saw; drop a non-Applied item whose violation is gone -- it was resolved
        // by another rule's fix on a later pass, so reporting it would fail an
        // otherwise-clean tree with a finding the user cannot even locate (e.g. a
        // size-skip on a file another rule deleted).
        //
        // A fix ERROR is NOT special-cased. A PERSISTENT write error (a read-only
        // target, ENOSPC) keeps re-firing, so its violation stays in `final_keys`
        // and is kept -> exit 1, as intended. An error whose file another rule
        // then removed is MOOT (the file is gone, the tree is in the desired
        // state), so its key leaves `final_keys` and it is dropped -> exit 0,
        // agreeing with `check`. Keeping it here would strand a phantom I/O error
        // on a clean tree.
        //
        // An `Applied` whose violation the converged pass STILL saw is not a
        // resolution: the fix wrote something, but the rule's re-evaluation on
        // the fresh tree reports the same violation (e.g. `file_create` made a
        // gitignored `.env` the walker never indexes, or a `file_create.path`
        // that differs from the rule's `paths:`). The lock above hid the converged
        // pass's own item for that key, so without this demotion `fix` would print
        // "applied" and exit 0 while `check` still fails. Demote it to a declined
        // skip (the violation stands -> nonzero exit at `level: error`), keeping
        // what was done in the reason so the user can see why it did not stick.
        if converged {
            for rr in &mut results {
                rr.items.retain(|it| {
                    matches!(it.status, FixStatus::Applied(_))
                        || final_keys.contains(&Self::violation_key(&rr.rule_id, &it.violation))
                });
                for it in &mut rr.items {
                    if let FixStatus::Applied(summary) = &it.status
                        && final_keys.contains(&Self::violation_key(&rr.rule_id, &it.violation))
                    {
                        it.status = FixStatus::unresolved(format!(
                            "fix ran ({summary}) but the violation still stands on re-check \
                             (is the written path ignored, outside the rule's scope, or \
                             different from the path the rule checks?)"
                        ));
                    }
                }
            }
        }
        // A rule whose every item was superseded (and not re-emitted) drops out,
        // matching `fix_run`'s "rules that pass are omitted".
        results.retain(|rr| !rr.items.is_empty() || !rr.analysis_failures.is_empty());
        // Results can first appear on different passes, but every projection
        // promises configured rule order rather than first-seen order.
        let rule_order: HashMap<&str, usize> = self
            .entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| (entry.rule.id(), idx))
            .collect();
        results.sort_by_key(|rr| rule_order.get(rr.rule_id.as_ref()).copied());
        if !converged {
            let mut stuck: Vec<&str> = last_applied_rules.iter().map(Arc::as_ref).collect();
            stuck.sort_unstable();
            stuck.dedup();
            eprintln!(
                "alint: error: fix did not settle after {MAX_PASSES} passes; these rules were \
                 still applying (a non-convergent config whose fixes undo each other, or a very \
                 slow one): {}. re-run `alint fix` to continue if it is only slow, or `alint \
                 check` to see what remains.",
                stuck.join(", ")
            );
        }
        let mut report = FixReport {
            results,
            non_convergent: !converged,
        };
        Self::sanitize_fix_report(&mut report);
        Ok(report)
    }

    /// Compose the fixes without writing, and hand back what each file would
    /// become: `(repo-relative path, old bytes, new bytes)` for every file a
    /// fixer would change. Powers `alint fix --diff`. Runs the same compose
    /// pass as [`fix`](Self::fix) with the flush suppressed, so the diff
    /// reflects what a real `fix` at this `threshold` would write, plus the
    /// [`FixReport`] (for the exit predicate).
    ///
    /// Fidelity caveat (single-pass, whole-file-op-first). Whole-file ops
    /// (rename/remove) are *recorded* in a stage, not performed, so they don't
    /// mutate the on-disk tree the way a real `fix` does mid-pass. If a config
    /// applies a whole-file op to a file BEFORE a content rule that also matches
    /// it, the real one-pass `fix` renames/removes the file first (and the
    /// content rule then sees the vacated path via the stale index, doing
    /// nothing until a rerun), whereas the stage shows BOTH the whole-file op and
    /// a content edit to the pre-op path. Preview-only (no corruption), and the
    /// dangerous content-first order is faithful (the content edit's
    /// `has_pending_write` makes the whole-file op yield in both the stage and
    /// the real fix). Fully reconciling both orders needs the Phase-1 re-walk.
    ///
    /// # Errors
    /// Propagates hard errors from the fix pass and from reading the current
    /// files and metadata required to construct a faithful preview.
    #[allow(clippy::too_many_lines)]
    pub fn stage_fixes(
        &self,
        root: &Path,
        index: &FileIndex,
        threshold: Applicability,
    ) -> Result<(FixReport, Vec<StagedFix>)> {
        // Whole-file fixers (create / remove / rename) write directly rather
        // than through the compose buffer, so they would mutate the tree during
        // a preview. The stage sink makes them record their `FixEdit` instead;
        // `Some(&sink)` puts every direct-write fixer into that record-not-write
        // mode (see `FixContext::stage_ops`), which is what keeps `--diff` from
        // touching disk.
        self.ensure_manifest_scope_resolvable()?;
        if self.changed_paths.as_ref().is_some_and(HashSet::is_empty)
            && !self.any_expect_matches()?
        {
            return Ok((
                FixReport {
                    results: Vec::new(),
                    non_convergent: false,
                },
                Vec::new(),
            ));
        }
        let stage_ops = RefCell::new(Vec::new());
        let analysis_time = self.resolve_analysis_time_if_needed(root);
        // Single pass: a `--diff` preview shows what one fix pass would compose;
        // it does not re-walk (there is no changed tree to walk), so it cannot
        // preview a multi-pass cascade -- documented in `stage_fixes`' caveat.
        let (mut report, buffer, mut sensitive_targets) = self.fix_run(
            root,
            index,
            /* created */ &std::collections::HashSet::new(),
            /* dry_run */ false,
            threshold,
            analysis_time.as_ref(),
            /* flush */ None,
            Some(&stage_ops),
        )?;

        // Compose-buffer entries are in-place content edits. Keys are resolved
        // absolute write targets; present them repo-relative, paired with the
        // current on-disk (unwritten) bytes. A concurrent removal/read failure
        // is a hard error: substituting empty bytes would fabricate a deletion
        // from an empty file rather than preview the real fix.
        let canon_root = std::fs::canonicalize(root).map_err(|source| Error::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let mut staged = Vec::with_capacity(buffer.len() + stage_ops.borrow().len());
        for (target, new) in buffer {
            let old = std::fs::read(&target).map_err(|source| Error::Io {
                path: target.clone(),
                source,
            })?;
            staged.push(StagedFix {
                path: target
                    .strip_prefix(&canon_root)
                    .unwrap_or(&target)
                    .to_path_buf(),
                old,
                new,
                kind: StagedKind::Modify,
            });
        }

        // Whole-file ops recorded by the stage sink. A fixer only reaches the
        // sink on the path where it would really act (create only when absent,
        // remove/rename only when present), so `old`/`new` follow from the op.
        for edit in stage_ops.into_inner() {
            match edit {
                FixEdit::CreateFile { path, content } => staged.push(StagedFix {
                    path,
                    old: Vec::new(),
                    new: content,
                    kind: StagedKind::Create,
                }),
                FixEdit::DeleteFile { path } => {
                    let target = root.join(&path);
                    let old = std::fs::read(&target).map_err(|source| Error::Io {
                        path: target,
                        source,
                    })?;
                    staged.push(StagedFix {
                        path,
                        old,
                        new: Vec::new(),
                        kind: StagedKind::Delete,
                    });
                }
                FixEdit::RenameFile { from, to } => {
                    let source_path = root.join(&from);
                    let old = std::fs::read(&source_path).map_err(|source| Error::Io {
                        path: source_path,
                        source,
                    })?;
                    staged.push(StagedFix {
                        path: to,
                        new: old.clone(),
                        old,
                        kind: StagedKind::Rename { from },
                    });
                }
                // A chmod: content is unchanged, so `old`/`new` are the current
                // bytes and the diff renders the mode change from the recorded pair.
                FixEdit::SetMode { path, mode } => {
                    let target = root.join(&path);
                    #[cfg(unix)]
                    let old_mode = {
                        use std::os::unix::fs::PermissionsExt as _;
                        std::fs::metadata(&target)
                            .map_err(|source| Error::Io {
                                path: target.clone(),
                                source,
                            })?
                            .permissions()
                            .mode()
                    };
                    #[cfg(not(unix))]
                    let old_mode = 0u32;
                    let content = std::fs::read(&target).map_err(|source| Error::Io {
                        path: target,
                        source,
                    })?;
                    staged.push(StagedFix {
                        path,
                        new: content.clone(),
                        old: content,
                        kind: StagedKind::Chmod {
                            old_mode,
                            new_mode: mode,
                        },
                    });
                }
                // Content-shaped edits belong in the compose buffer, not here;
                // ignore defensively so a future mis-wired fixer can't smuggle a
                // content write past the diff.
                FixEdit::SetContent { .. } | FixEdit::ReplaceRange { .. } => {}
            }
        }

        // Deterministic output: the compose buffer is sorted, the sink is
        // push-ordered, so re-sort the union by the presented path.
        staged.sort_by(|a, b| a.path.cmp(&b.path));
        sensitive_targets.extend(self.sensitive_fix_targets(&report, root));
        staged.retain(|fix| {
            let target = Self::normalized_write_target(root, &fix.path);
            if sensitive_targets.contains(&target) {
                return false;
            }
            if let StagedKind::Rename { from } = &fix.kind {
                let source = Self::normalized_write_target(root, from);
                if sensitive_targets.contains(&source) {
                    return false;
                }
            }
            true
        });
        Self::sanitize_fix_report(&mut report);
        Ok((report, staged))
    }

    /// `--dry-run` has no re-walk to discover that a fix will not stick, so
    /// predict the known cases the real `fix` demotes after its re-check: a
    /// would-be-`Applied` create (`file_create`) whose target the walk would
    /// not index (gitignored, `ignore:`d) or that lies outside the rule's
    /// `paths:`. Those become the same unresolved skip the real run reports.
    /// Other ways a fix can fail its re-check (a cascade through another
    /// rule, content a fixer writes that the rule still rejects) are only
    /// found by running it; `docs/site/cli/fix.md` documents the limit.
    pub(super) fn predict_unresolved_creates(
        &self,
        root: &Path,
        index: &FileIndex,
        walk_opts: &crate::WalkOptions,
        report: &mut FixReport,
    ) {
        for rr in &mut report.results {
            let Some(entry) = self.entries.iter().find(|e| e.rule.id() == &*rr.rule_id) else {
                continue;
            };
            let Some(fixer) = entry.rule.fixer() else {
                continue;
            };
            for it in &mut rr.items {
                let FixStatus::Applied(summary) = &it.status else {
                    continue;
                };
                let Some(FixEdit::CreateFile { path, .. }) =
                    fixer.fix_edit(&it.violation, &[], root)
                else {
                    continue;
                };
                let target: PathBuf = path
                    .components()
                    .filter(|c| !matches!(c, std::path::Component::CurDir))
                    .collect();
                let why = if crate::walker::would_walk_skip(root, walk_opts, &target, false) {
                    format!(
                        "{} is excluded from the walk (.gitignore / `ignore:`), so the rule \
                         would never see it",
                        target.display()
                    )
                } else if entry
                    .rule
                    .path_scope()
                    .is_some_and(|scope| !scope.matches(&target, index))
                {
                    format!("{} is outside the rule's `paths:`", target.display())
                } else {
                    continue;
                };
                it.status = FixStatus::unresolved(format!(
                    "fix would run ({summary}) but would not resolve the violation: {why}"
                ));
            }
        }
    }

    /// Remove remediation-only identity from sensitive findings after every
    /// fixer has finished and before a fix report crosses the public API.
    pub(super) fn sanitize_fix_report(report: &mut FixReport) {
        for result in &mut report.results {
            for item in &mut result.items {
                item.violation.sanitize_sensitive_metadata(&result.rule_id);
            }
        }
    }

    /// Resolved targets touched by sensitive fix items. `stage_fixes` uses this
    /// to omit the entire composed file from `--diff`, including non-sensitive
    /// sibling edits whose whole-file context could contain secret bytes.
    pub(super) fn sensitive_fix_targets(
        &self,
        report: &FixReport,
        root: &Path,
    ) -> HashSet<PathBuf> {
        let mut targets = HashSet::new();
        for result in &report.results {
            let fixer = self.fixer_for(&result.rule_id);
            for item in &result.items {
                if !item.violation.is_sensitive() {
                    continue;
                }
                if let Some(path) = item.violation.path.as_deref() {
                    targets.insert(Self::normalized_write_target(root, path));
                }
                let Some(fixer) = fixer else {
                    continue;
                };
                let bytes = item
                    .violation
                    .path
                    .as_deref()
                    .and_then(|path| std::fs::read(root.join(path)).ok())
                    .unwrap_or_default();
                if let Some(edit) = fixer.fix_edit(&item.violation, &bytes, root) {
                    Self::insert_fix_targets(root, &edit, &mut targets);
                }
            }
        }
        targets
    }

    pub(super) fn insert_fix_targets(root: &Path, edit: &FixEdit, targets: &mut HashSet<PathBuf>) {
        match edit {
            FixEdit::SetContent { path, .. }
            | FixEdit::CreateFile { path, .. }
            | FixEdit::DeleteFile { path }
            | FixEdit::ReplaceRange { path, .. }
            | FixEdit::SetMode { path, .. } => {
                targets.insert(Self::normalized_write_target(root, path));
            }
            FixEdit::RenameFile { from, to } => {
                targets.insert(Self::normalized_write_target(root, from));
                targets.insert(Self::normalized_write_target(root, to));
            }
        }
    }

    pub(super) fn normalized_write_target(root: &Path, path: &Path) -> PathBuf {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
        crate::rule::resolve_write_target(&absolute)
    }

    pub(super) fn has_sensitive_manifest_scope(rule: &dyn Rule) -> bool {
        rule.path_scope()
            .or_else(|| rule.as_per_file().map(PerFileRule::path_scope))
            .is_some_and(|scope| scope.sensitive_manifest_sources().next().is_some())
    }

    pub(super) fn has_sensitive_path_selection(&self, entry: &RuleEntry) -> bool {
        if Self::has_sensitive_manifest_scope(entry.rule.as_ref()) {
            return true;
        }
        let Some(spec) = entry.spec.as_deref() else {
            return false;
        };
        let Some(meta) = self.registry.meta(&spec.kind) else {
            return false;
        };
        spec.field_is_environment_derived("paths")
            || spec.field_is_environment_derived("scope_filter")
            || (meta.nested_rules && spec.field_is_environment_derived("require"))
            || meta
                .selectors
                .iter()
                .any(|selector| spec.field_is_environment_derived(&selector.name))
    }

    pub(super) fn redact_result_for_entry(&self, index: usize, result: &mut RuleResult) {
        let entry = &self.entries[index];
        if Self::has_sensitive_manifest_scope(entry.rule.as_ref()) {
            result.redact_manifest_scope();
        }
        if entry
            .spec
            .as_deref()
            .is_some_and(|spec| spec.field_is_sensitive("policy_url"))
        {
            result.redact_policy_url();
        }
        if entry
            .spec
            .as_deref()
            .is_some_and(crate::RuleSpec::has_sensitive_evidence)
        {
            result.redact_sensitive();
        }
        if self.has_sensitive_path_selection(entry) {
            result.redact_sensitive_paths();
        }
        result.sanitize_sensitive_metadata();
    }

    pub(super) fn record_sensitive_result_paths(
        entry: &RuleEntry,
        result: &RuleResult,
        root: &Path,
        targets: &mut HashSet<PathBuf>,
    ) {
        let explicit = entry.spec.as_deref().is_some_and(|spec| spec.sensitive);
        for finding in result.violations.iter().chain(&result.notes) {
            if (explicit || finding.is_sensitive())
                && let Some(path) = finding.path.as_deref()
            {
                targets.insert(Self::normalized_write_target(root, path));
            }
        }
        for failure in &result.analysis_failures {
            if (explicit || failure.is_sensitive())
                && let Some(path) = failure.path.as_deref()
            {
                targets.insert(Self::normalized_write_target(root, path));
            }
        }
    }
    /// Identity key for grouping a violation's report items ACROSS fixpoint
    /// passes (docs/design/v0.17/fixpoint.md 7). Reuses the baseline fingerprint
    /// (it folds in `rule_id` + path + the rule's stable identity via
    /// `baseline_key`). It is NOT a termination device -- the byte-level fixpoint
    /// converges on "a pass applied nothing", not on this key -- so its coarseness
    /// (a path-bearing keyless violation keys on `(rule_id, path)`, message and
    /// line ignored) is harmless FOR SHIPPED RULES: it only decides which earlier
    /// report item a later pass's item supersedes or which `Applied` outcome to
    /// lock, and every fixable rule either emits one finding per `(rule_id, path)`
    /// or (`*_path_matches`) sets a per-value `baseline_key` whose collisions are
    /// truly identical findings. A future multi-finding fixable rule that
    /// distinguishes findings only by line/message under a coarse key would break
    /// that; the F4 tripwire in `fix` (the fixpoint driver) asserts it in debug.
    /// `file_bytes` is `None` (no re-read); the discriminator is the rule's
    /// `baseline_key` or, absent one, the path (or the message for a pathless
    /// violation).
    pub(super) fn violation_key(rule_id: &str, v: &Violation) -> String {
        crate::baseline::fingerprint(rule_id, v, None)
    }

    // `created` (2b) is the eighth parameter; the signature stays flat rather than
    // bundling into a struct because every caller passes a distinct, local value and
    // the grouping would not clarify anything at the call sites.
    #[allow(clippy::too_many_lines, clippy::too_many_arguments)]
    pub(super) fn fix_run(
        &self,
        root: &Path,
        index: &FileIndex,
        // Files THIS `fix` run created on an earlier pass (2b): they join the
        // `--changed` set so a create-then-fix cascade completes. Empty for a
        // single-pass caller (`--dry-run` / `--diff`) and for a full (non-changed)
        // fix, where it has no effect.
        created: &HashSet<PathBuf>,
        dry_run: bool,
        threshold: Applicability,
        analysis_time: Option<&crate::AnalysisTimeResolution>,
        // `Some` = write the compose buffer; the set holds the targets whose
        // write failure this `fix` run already printed (reported once, not once
        // per fixpoint pass). `None` for a preview (`--dry-run` / `--diff`).
        flush: Option<&RefCell<HashSet<PathBuf>>>,
        stage_ops: Option<&RefCell<Vec<FixEdit>>>,
    ) -> Result<FixRunOutput> {
        self.enable_relation_caches(index);
        let fact_values = evaluate_facts_with_environment(
            &self.facts,
            root,
            index,
            self.process_environment.as_ref(),
        )?;
        let when_env = WhenEnv {
            facts: &fact_values,
            vars: &self.vars,
            iter: None,
            env: None,
        };
        let git_required = self.git_requirements(index, &when_env)?;
        let git_tracked = Self::collect_git_tracked_if_needed(root, &git_required);
        let git_available = git_tracked.is_some();
        let git_blame = self.build_blame_cache_if_needed(root, git_tracked.as_ref());
        let filtered_index = self.build_filtered_index(index, Some(created));
        let git_tracked_indexes = self.build_git_tracked_indexes(index, git_tracked.as_ref());
        let full_ctx = Context {
            root,
            index,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
            git_blame: git_blame.as_ref(),
            analysis_time,
            process_environment: self.process_environment.as_ref(),
            trace: None,
        };
        let filtered_ctx = filtered_index.as_ref().map(|fi| Context {
            root,
            index: fi,
            registry: Some(&self.registry),
            facts: Some(&fact_values),
            vars: Some(&self.vars),
            git_tracked: git_tracked.as_ref(),
            git_blame: git_blame.as_ref(),
            analysis_time,
            process_environment: self.process_environment.as_ref(),
            trace: None,
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
                analysis_time,
                process_environment: self.process_environment.as_ref(),
                trace: None,
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
                analysis_time,
                process_environment: self.process_environment.as_ref(),
                trace: None,
            });
        // Compose mode for a real (non-dry-run) pass: content fixers route
        // their whole-file writes into this buffer instead of hitting disk, so
        // a file touched by several fixers in config order composes in memory
        // and is flushed with a single atomic write per file (below). A
        // `--dry-run` pass has no buffer and writes nothing, exactly as before.
        let compose_buf: Option<RefCell<BTreeMap<PathBuf, Vec<u8>>>> =
            (!dry_run).then(|| RefCell::new(BTreeMap::new()));
        let mut fix_ctx = FixContext {
            root,
            dry_run,
            fix_size_limit: self.fix_size_limit,
            // Set per-entry inside the loop below, so each fixer confines its
            // config-declared paths against the OWNING rule's permission.
            allow_out_of_root: false,
            compose: compose_buf.as_ref(),
            stage_ops,
            process_environment: self.process_environment.as_ref(),
        };

        // Same dependency resolution as `run`, so a fix pass respects per-rule
        // diff and manifest scoping without touching false `when:` rules.
        if git_available {
            self.resolve_changed_paths(root, index, &when_env)?;
        }
        self.resolve_manifest_paths(root, index, &when_env, Some(created))?;
        // Propagate BOTH resolved caches onto every alternate index the fix loop
        // may `pick_ctx` (see `run`): the `--changed` filtered index and the
        // git-tracked file-only / dir-aware indexes, so `fix` respects manifest
        // scope AND `changed_since:` on every dispatch path.
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
        // A `has_ancestor` marker lookup on the `--changed` index must see unchanged
        // files too. Only the changed-set index shares the full tree's file set
        // (the git-tracked indexes are deliberately subsets and must not), and only
        // when a rule will actually look one up on it (`needs_tree_files`):
        // building the set is a pass over the full index that other `--changed`
        // runs should not pay.
        if let Some(fi) = filtered_index.as_ref()
            && self.needs_tree_files(index, Some(created), true)
        {
            fi.share_tree_files_from(index);
        }

        let mut results: Vec<FixRuleResult> = Vec::new();
        // Every sensitive finding path observed during evaluation, including
        // notes that never enter a fix report. A staged diff omits these files
        // so unchanged secret bytes cannot leak through a sibling edit.
        let mut sensitive_paths = HashSet::new();
        // Accumulator for the located-edit regime, filled by fixers that opt in
        // via `collects_located_edits` and applied once per file after the loop.
        // Phase 0 stays empty (no shipped fixer opts in), so the located pass is
        // a genuine no-op here.
        let mut located_batches: BTreeMap<PathBuf, Vec<LocatedEdit>> = BTreeMap::new();
        // Byte-consistency for the located pass (Phase 1): the edits' byte
        // offsets are computed against the file's bytes AT COLLECT TIME. Capture
        // those bytes per file so `apply` splices against the exact bytes the
        // offsets index -- never a re-read that a whole-file fixer may have
        // changed in between (stale-offset corruption). If two rules collect for
        // one file against DIFFERENT bytes (a whole-file fixer buffered it mid-
        // loop), the batch is inconsistent: mark it deferred and skip it this
        // pass (it re-applies on a rerun / the Phase-1 fixpoint), never corrupt.
        let mut located_bytes: BTreeMap<PathBuf, Vec<u8>> = BTreeMap::new();
        let mut located_deferred: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::new();
        // W4 (`fix --baseline`): per-file byte cache for the baseline
        // content-fingerprint, so a file is read at most once per pass for
        // classification. Unused (empty) when no fix-baseline is active.
        let mut fp_cache: HashMap<PathBuf, Option<Vec<u8>>> = HashMap::new();
        for (rule_index, entry) in self.entries.iter().enumerate() {
            // Skip a rule scoped ENTIRELY outside the changed set (plus files this
            // run created), exactly as `check --changed` does, so `fix` and `check`
            // run the same rules and agree. A rule with SOME in-scope target still
            // runs; its OUT-of-scope violations are demoted to Suggestions in the
            // whole-file status loop below (2b), not applied (blast radius) and not
            // dropped (the old silent behavior, which made `fix` exit 0 while
            // `check` reported them and exited 1). `Some(created)` keeps a rule
            // alive for a file a fix in scope created (its cascade must finish).
            let skipped_for_changed =
                self.skip_for_changed(entry.rule.as_ref(), full_ctx.index, Some(created));
            if skipped_for_changed && !entry.expect_matches() {
                continue;
            }
            if let Some(expr) = &entry.when {
                match expr.evaluate(&when_env) {
                    Ok(true) => {}
                    Ok(false) => continue,
                    Err(e) => {
                        push_fix_result(
                            &mut results,
                            Arc::from(entry.rule.id()),
                            entry.rule.level(),
                            Vec::new(),
                            vec![crate::AnalysisFailure::new(
                                AnalysisFailureKind::Condition,
                                format!("when evaluation failed: {e}"),
                            )],
                        );
                        continue;
                    }
                }
            }
            let manifest_gate = &manifest_gates[rule_index];
            if !manifest_gate.failures.is_empty() {
                let evaluated = RuleResult::new(
                    Arc::from(entry.rule.id()),
                    entry.rule.level(),
                    None,
                    manifest_gate.failures.clone(),
                    false,
                );
                if stage_ops.is_some() {
                    Self::record_sensitive_result_paths(
                        entry,
                        &evaluated,
                        root,
                        &mut sensitive_paths,
                    );
                }
                push_fix_result(
                    &mut results,
                    Arc::from(entry.rule.id()),
                    entry.rule.level(),
                    Vec::new(),
                    evaluated.analysis_failures,
                );
                continue;
            }
            if git_required[rule_index] && !git_available {
                let evaluated = RuleResult::new(
                    Arc::from(entry.rule.id()),
                    entry.rule.level(),
                    None,
                    vec![unavailable_git_failure()],
                    false,
                );
                if stage_ops.is_some() {
                    Self::record_sensitive_result_paths(
                        entry,
                        &evaluated,
                        root,
                        &mut sensitive_paths,
                    );
                }
                push_fix_result(
                    &mut results,
                    Arc::from(entry.rule.id()),
                    entry.rule.level(),
                    Vec::new(),
                    evaluated.analysis_failures,
                );
                continue;
            }
            let scope_violation = match entry.spec.as_deref() {
                Some(spec) => match crate::expect_matches_violation(
                    entry.rule.as_ref(),
                    spec,
                    full_ctx.index,
                    &full_ctx,
                ) {
                    Ok(violation) => violation,
                    Err(e) => Some(
                        Violation::new(format!("scope assertion evaluation failed: {e}"))
                            .as_analysis_failure(AnalysisFailureKind::Evaluation),
                    ),
                },
                None => None,
            };
            if scope_violation.is_none() && skipped_for_changed {
                continue;
            }
            // An empty-scope assertion is a configuration-integrity finding,
            // not something the host rule's fixer can repair. Still feed it
            // through baseline classification below so `check --baseline` and
            // `fix --baseline` agree; only the fixer selection is suppressed.
            let scope_assertion_failed = scope_violation.is_some();
            let violations = if let Some(violation) = scope_violation {
                vec![violation]
            } else {
                let ctx = pick_ctx(
                    entry.rule.as_ref(),
                    self.dependency_changed(entry.rule.as_ref()),
                    &full_ctx,
                    filtered_ctx.as_ref(),
                    git_file_only_ctx.as_ref(),
                    git_dir_aware_ctx.as_ref(),
                );
                if stage_ops.is_some() {
                    let explicit = entry.spec.as_deref().is_some_and(|spec| spec.sensitive);
                    entry
                        .rule
                        .visit_sensitive_paths(explicit, ctx, &mut |path| {
                            sensitive_paths.insert(Self::normalized_write_target(root, path));
                        });
                }
                match entry.rule.evaluate(ctx) {
                    Ok(v) => v,
                    Err(e) => vec![
                        Violation::new(format!("rule evaluation failed: {e}"))
                            .as_analysis_failure(AnalysisFailureKind::Evaluation),
                    ],
                }
            };
            // Partition non-remediation findings before baseline and fixer
            // processing. Notes are check-only; failures are reported through
            // their dedicated collection and never reach either path.
            let mut evaluated = RuleResult::new(
                Arc::from(entry.rule.id()),
                entry.rule.level(),
                None,
                violations,
                false,
            );
            if Self::has_sensitive_manifest_scope(entry.rule.as_ref()) {
                evaluated.redact_manifest_scope();
            }
            if stage_ops.is_some() {
                Self::record_sensitive_result_paths(entry, &evaluated, root, &mut sensitive_paths);
            }
            let violations = evaluated.violations;
            if !evaluated.analysis_failures.is_empty() {
                push_fix_result(
                    &mut results,
                    Arc::from(entry.rule.id()),
                    entry.rule.level(),
                    Vec::new(),
                    evaluated.analysis_failures,
                );
            }
            // W4 (`fix --baseline`): SKIP -- surface, never apply -- any violation
            // the baseline grandfathers, so `fix` resolves only NEW findings. Reuse
            // `baseline::apply` on THIS rule's violations: the fingerprint includes
            // the rule id (so per-rule application equals report-level), computed on
            // the CURRENT file content each pass -- identical to how
            // `check --baseline` suppresses. The grandfathered findings are reported
            // as a benign `baselined` skip (they do not fail the exit), and only the
            // live ones reach the fixer.
            let violations = if let Some(bl) = self.fix_baseline.as_ref() {
                let single = crate::Report {
                    results: vec![crate::RuleResult {
                        rule_id: Arc::from(entry.rule.id()),
                        level: entry.rule.level(),
                        policy_url: None,
                        violations,
                        analysis_failures: Vec::new(),
                        notes: Vec::new(),
                        is_fixable: false,
                    }],
                };
                let mut applied = crate::baseline::apply(&single, bl, |rid, v| {
                    let bytes = match v.path.as_ref() {
                        Some(p) => fp_cache
                            .entry(p.to_path_buf())
                            .or_insert_with(|| {
                                let full = root.join(p);
                                let size = std::fs::metadata(&full).map_or(0, |m| m.len());
                                crate::read_capped_or_skip(&full, size)
                            })
                            .as_deref(),
                        None => None,
                    };
                    crate::baseline::fingerprint(rid, v, bytes)
                });
                if !applied.suppressed.is_empty() {
                    let items = applied
                        .suppressed
                        .into_iter()
                        .map(|s| FixItem {
                            violation: s.violation,
                            status: FixStatus::baselined(format!(
                                "{} grandfathered by the baseline (not fixed; run without \
                                 --baseline to fix it, or re-run `alint baseline`)",
                                crate::report::BASELINED_SKIP_PREFIX
                            )),
                        })
                        .collect();
                    push_fix_result(
                        &mut results,
                        Arc::from(entry.rule.id()),
                        entry.rule.level(),
                        items,
                        Vec::new(),
                    );
                }
                // Only the live (new) violations proceed to the fixer.
                applied
                    .live
                    .results
                    .pop()
                    .map_or_else(Vec::new, |r| r.violations)
            } else {
                violations
            };
            if violations.is_empty() {
                continue;
            }
            // `--changed` blast-radius confinement (2b). A full-index rule
            // (existence / cross-file, `requires_full_index() == true`) is handed
            // the FULL index by `pick_ctx` even under `--changed`, because its
            // *check* verdict must consider the whole tree (an unchanged committed
            // `.env` should still fire). But the FIX must not silently widen the
            // blast radius: a fix whose target is OUTSIDE the changed set (and the
            // files this run created) is DEMOTED to a Suggestion in the whole-file
            // status loop below (`writes_outside_changed`), not applied (would
            // delete/rewrite an untouched committed file) and not dropped (the older
            // behavior, which silently hid a required out-of-scope write). An
            // in-scope target -- a changed file, or one a fix in scope created this
            // run -- applies normally. The located regime (per-file only today) is
            // confined by the filtered index, so out-of-scope violations never reach
            // it; the demote lives in the whole-file loop that full-index fixers use.
            let fixer = if scope_assertion_failed {
                None
            } else {
                entry.rule.fixer()
            };
            fix_ctx.allow_out_of_root = entry.allow_out_of_root;
            // Located-edit fixers (Phase 1+) route through the batched
            // located_fix path instead of `apply`: collect their byte-range
            // edits per file (size-guarded via read_for_fix -- invariant 4 on
            // the new path), tag each with the rule index and its ordinal for
            // the deterministic total order, and defer application until after
            // the loop so a file touched by several rules is spliced once. No
            // Phase-0 fixer opts in, so this branch is never entered here.
            if fixer.is_some_and(Fixer::collects_located_edits) {
                let f = fixer.expect("guarded by is_some_and above");
                // `--changed` BLAST-RADIUS INVARIANT (whole-phase audit, F1/LG-1): the
                // located branch confines `--changed` PURELY via the filtered index
                // (`pick_ctx` hands a per-file rule the changed-filtered ctx); it never
                // consults `writes_outside_changed`. Sound ONLY while every located
                // fixer hosts on a PER-FILE rule. A future located fixer on a
                // `requires_full_index()` / git-tracked rule would get the FULL index
                // under `--changed`, and its edits would splice into out-of-diff files
                // with NO demote -- a silent blast-radius escape. `replace` (the sole
                // located fixer) hosts on `file_content_forbidden` (per-file), and the
                // fix-op/kind gate refuses `replace` on any other kind, so this holds
                // today. The escape only exists UNDER `--changed` (without it there is
                // no blast radius to escape), so the tripwire is gated on it; a real
                // per-file located `replace` under `--changed` exercises the passing
                // path (`located_replace_under_changed_is_confined_to_the_diff`). Pin
                // it (docs/design/v0.17/fixpoint.md Phase-2 pre-reqs).
                debug_assert!(
                    self.changed_paths.is_none() || entry.rule.as_per_file().is_some(),
                    "located fixer on non-per-file rule {:?} under `--changed`: located \
                     confinement assumes a per-file host; wire writes_outside_changed \
                     into the located branch before shipping such a fixer",
                    entry.rule.id()
                );
                // Violations the located fixer DECLINED (collected no edit for) --
                // reported as skips so a standing violation is not silently
                // dropped (see below).
                let mut declined: Vec<FixItem> = Vec::new();
                let mut by_file: BTreeMap<PathBuf, Vec<Violation>> = BTreeMap::new();
                for v in violations {
                    // A finding `check` reports as not fixable is never handed to
                    // the fixer: it is reported `unfixable`, exactly as `check`
                    // promised (see `is_unfixable_finding`).
                    if is_unfixable_finding(f, &v) {
                        declined.push(FixItem {
                            violation: v,
                            status: FixStatus::Unfixable,
                        });
                        continue;
                    }
                    let Some(key) = v.path.as_deref().map(Path::to_path_buf) else {
                        continue;
                    };
                    by_file.entry(key).or_default().push(v);
                }
                for (file, file_violations) in by_file {
                    let sensitive = file_violations.iter().any(Violation::is_sensitive);
                    let abs = root.join(&file);
                    let bytes = match read_for_fix(&abs, &file, &fix_ctx) {
                        Ok(ReadForFix::Bytes(b)) => b,
                        // Over the `fix_size_limit`: emit a Skip PER VIOLATION so a
                        // standing violation is not silently dropped. The whole-file
                        // dispatch surfaces the same over-size skip (via the fixer's
                        // own `read_for_fix`); without this the located path would
                        // report "0 unfixable", exit 0, and leave the violation on
                        // disk while `check` (which reads to a far larger cap) still
                        // fails -- a false negative on files between fix_size_limit
                        // and the check read cap. `Skipped` carries the reason.
                        Ok(ReadForFix::Skipped(outcome)) => {
                            let reason = match outcome {
                                FixOutcome::Skipped(s) | FixOutcome::Applied(s) => s,
                            };
                            for v in file_violations {
                                declined.push(FixItem {
                                    violation: v,
                                    status: FixStatus::declined(reason.clone()),
                                });
                            }
                            continue;
                        }
                        // An I/O read error at fix time (a file readable at walk
                        // time -- persistently-unreadable files are dropped by the
                        // walker -- but not now: a TOCTOU race, or a special file).
                        // Report it as a fix error PER VIOLATION so the located path
                        // matches the whole-file dispatch (which surfaces the same
                        // `read_for_fix` error as a `FIX_ERROR_PREFIX` skip) instead
                        // of silently dropping the violation.
                        Err(e) => {
                            for v in file_violations {
                                declined.push(FixItem {
                                    violation: v,
                                    status: FixStatus::errored(format!(
                                        "{FIX_ERROR_PREFIX} could not read {}: {e}",
                                        file.display()
                                    )),
                                });
                            }
                            continue;
                        }
                    };
                    // Capture the collect-time bytes; a second rule reading
                    // different bytes for the same file poisons the batch.
                    match located_bytes.get(&file) {
                        Some(prev) if prev != &bytes => {
                            located_deferred.insert(file.clone());
                        }
                        Some(_) => {}
                        None => {
                            located_bytes.insert(file.clone(), bytes.clone());
                        }
                    }
                    let edits = f.collect_edits(&file_violations, &file, &bytes, root);
                    if edits.is_empty() {
                        // The located fixer collected NO edit for violations it was
                        // handed -- e.g. `replace` dropped a self-re-matching
                        // replacement (the fix would re-introduce the forbidden
                        // pattern). Report each violation as skipped, matching the
                        // whole-file dispatch which always emits one item per
                        // violation. Without this the violation produces no report
                        // item at all, so a STANDING forbidden pattern is reported
                        // as "0 unfixable" and `fix` exits 0 while `check` fails.
                        for v in file_violations {
                            declined.push(FixItem {
                                violation: v,
                                status: FixStatus::declined(format!(
                                    "{}: no applicable fix ({} declined)",
                                    file.display(),
                                    f.describe()
                                )),
                            });
                        }
                        continue;
                    }
                    for (ordinal, collected) in edits.into_iter().enumerate() {
                        located_batches
                            .entry(file.clone())
                            .or_default()
                            .push(LocatedEdit {
                                rule_index,
                                violation_index: ordinal,
                                sensitive,
                                collected,
                            });
                    }
                }
                if !declined.is_empty() {
                    push_fix_result(
                        &mut results,
                        Arc::from(entry.rule.id()),
                        entry.rule.level(),
                        declined,
                        Vec::new(),
                    );
                }
                continue;
            }
            let mut items: Vec<FixItem> = Vec::with_capacity(violations.len());
            for v in violations {
                let status = match fixer {
                    // A finding `check` reports as not fixable (the rule marked it
                    // `not_fixable`, or the fixer's `can_fix` declines it) is never
                    // handed to the fixer: it stands, reported `unfixable` -- the
                    // same verdict `check` gave it -- not a fixer "skip".
                    Some(f) if is_unfixable_finding(f, &v) => FixStatus::Unfixable,
                    // `--changed` confinement (2b): this fix would write OUTSIDE the
                    // changed set (and the files this run created), so demote it to a
                    // Suggestion carrying the proposed edit -- whatever its tier --
                    // rather than applying it (widening the blast radius) or dropping
                    // it (the old silent behavior). `fix_edit` supplies the edit
                    // (`file_remove` ignores the bytes; `file_create` builds a create
                    // edit). Tried FIRST; the guard is `false` for a full (non-
                    // changed) fix, so that path is byte-for-byte unchanged.
                    Some(f)
                        if self.writes_outside_changed(entry.rule.as_ref(), &v, index, created) =>
                    {
                        // A withheld fix (its target is outside the --changed set) is
                        // a Suggestion carrying the proposed edit when the fixer has
                        // one; an editless op (`git_untrack`, no `fix_edit`) still
                        // suggests with `edit: None`, not a decline.
                        FixStatus::Suggested {
                            summary: format!(
                                "{} (outside --changed scope; not auto-applied)",
                                f.describe()
                            ),
                            edit: f.fix_edit(&v, &[], fix_ctx.root),
                        }
                    }
                    // Applied tier at the current threshold: run the fixer.
                    Some(f) if f.applicability().applies_at(threshold) => {
                        match f.apply(&v, &fix_ctx) {
                            Ok(FixOutcome::Applied(s)) => FixStatus::Applied(s),
                            Ok(FixOutcome::Skipped(s)) => FixStatus::declined(s),
                            Err(e) => FixStatus::errored(format!("{FIX_ERROR_PREFIX} {e}")),
                        }
                    }
                    // Available but not applied at this threshold: surface it as a
                    // Suggestion (so `fix --diff --unsafe-fixes` can preview it and
                    // the check-side formats can emit it) rather than silently
                    // applying a destructive fix. The hint is tier-specific: an
                    // `Unsafe` fixer below the threshold IS applied by
                    // `--unsafe-fixes`, but a `Suggestion`-tier fixer never
                    // auto-applies, so telling the user to pass `--unsafe-fixes`
                    // would be wrong. `fix_edit` supplies the edit when the fixer has
                    // one (`file_remove` ignores the bytes); an editless op
                    // (`git_untrack`: `git rm --cached` has no worktree edit) still
                    // suggests with `edit: None` and the tier hint -- NOT a
                    // misleading "not applicable here" decline, since it IS
                    // applicable via `--unsafe-fixes` (F1 audit).
                    Some(f) if f.applicability().suggested_at(threshold) => {
                        let hint = if f.applicability() == Applicability::Unsafe {
                            " (requires --unsafe-fixes)"
                        } else {
                            " (suggestion only; not auto-applied)"
                        };
                        FixStatus::Suggested {
                            summary: format!("{}{hint}", f.describe()),
                            edit: f.fix_edit(&v, &[], fix_ctx.root),
                        }
                    }
                    // A fixer whose tier neither applies nor is suggested here
                    // (`Never`) -- collected for provenance only.
                    Some(f) => {
                        FixStatus::declined(format!("{} is not applied at this tier", f.describe()))
                    }
                    None => FixStatus::Unfixable,
                };
                items.push(FixItem {
                    violation: v,
                    status,
                });
            }
            push_fix_result(
                &mut results,
                Arc::from(entry.rule.id()),
                entry.rule.level(),
                items,
                Vec::new(),
            );
        }

        // Apply the located-edit batches collected above. Per file, the edits
        // are tier-filtered against `threshold`, ordered, overlap-skipped,
        // verified, and spliced by located_fix, then written through the compose
        // buffer so they flush once alongside the whole-file edits. Phase 0:
        // `located_batches` is empty, so this is a no-op -- but `threshold` is
        // genuinely consumed and the path is exercised by the engine's fixture
        // test. Result items are grouped back to their rule and appended.
        //
        // Byte-consistency (Phase 1): the offsets index the bytes captured in
        // `located_bytes` at collect time. Splice against THOSE, not a re-read --
        // a whole-file fixer may have buffered a change to the same file after
        // collection, which would make the offsets stale (corruption). If the
        // file is poisoned (inconsistent reads across rules) or its current bytes
        // differ from the captured ones (a whole-file fixer changed it since),
        // DEFER the batch: report each edit as skipped and leave the file for a
        // rerun / the Phase-1 fixpoint. Never splice into bytes the offsets do
        // not index.
        if !located_batches.is_empty() {
            let mut located_items: BTreeMap<usize, Vec<FixItem>> = BTreeMap::new();
            for (file, batch) in located_batches {
                let abs = root.join(&file);
                let deferred = located_deferred.contains(&file);
                let current = match read_for_fix(&abs, &file, &fix_ctx) {
                    Ok(ReadForFix::Bytes(b)) => Some(b),
                    Ok(ReadForFix::Skipped(_)) | Err(_) => None,
                };
                let original = located_bytes.get(&file);
                // Defer unless the file is consistent AND its current bytes still
                // equal the captured bytes the offsets index. With the Phase-1
                // fixpoint active (`Engine::fix`), the re-walk IS the rerun: this
                // batch is retried against the flushed bytes on the next pass, so
                // no provisional "rerun to apply" item is emitted here -- the retry
                // pass reports the real outcome (applied, or a genuine terminal
                // skip like an isolation conflict). Emitting one would either
                // duplicate that outcome or, if the same concurrent change resolves
                // the source violation (e.g. the file is removed), strand a stale
                // skip that wrongly drives a nonzero exit. A single-pass caller
                // (`--dry-run` / `--diff`) writes nothing, so this branch cannot
                // fire there; the batch simply applies against consistent bytes.
                if deferred || current.as_ref() != original {
                    // In a real multi-pass `fix` the batch is retried on the next
                    // pass against the flushed bytes (silent + correct). A
                    // single-pass STAGE (`--diff`) has no next pass, so the
                    // deferred located edits are absent from the preview -- and the
                    // located offsets index the collect-time bytes, so splicing
                    // them onto the (whole-file-changed) current bytes would
                    // corrupt. Rather than silently omit them (a consumer applying
                    // the previewed patch would keep the violation), warn. Full
                    // multi-pass `--diff` fidelity is a follow-up.
                    if stage_ops.is_some() {
                        eprintln!(
                            "alint: warning: --diff preview omits {} located edit(s) in {} \
                             that a later fix pass would apply (a whole-file fix changed the \
                             file first); run `alint fix` to apply them.",
                            batch.len(),
                            file.display()
                        );
                    }
                    continue;
                }
                let original = original.expect("current == original implies Some").clone();
                let (new_bytes, outcomes) =
                    located_fix::apply_file_edits(&original, batch, threshold);
                let batch_changed = new_bytes != original;
                // `--dry-run` reports the outcomes but writes nothing: skip the
                // stage entirely (commit_write in a dry run has no compose buffer
                // and would write straight to disk, exactly what dry-run forbids).
                if !dry_run
                    && batch_changed
                    && let Err(source) = fix_ctx.commit_write(&abs, &new_bytes)
                {
                    eprintln!("alint: could not stage {}: {source}", file.display());
                }
                for (edit, outcome) in outcomes {
                    let status = located_status(
                        &edit.collected.edit,
                        edit.collected.applicability,
                        outcome,
                        dry_run,
                    );
                    // Per-EDIT discriminators derived from the edit's byte range.
                    // `range_key` distinguishes two edits on ONE file: without it
                    // they collapse to the same `violation_key` (`(rule_id, path)`,
                    // Phase-2 pre-req 1), letting an `Applied` for one node lock-mask
                    // a standing `Skipped` for another across fixpoint passes. Keying
                    // each item by its range keeps them distinct. `is_identity` flags
                    // a no-op rewrite (its replacement equals the bytes it spans).
                    // `ReplaceFixer` emits disjoint, non-identity ranges, so both are
                    // dormant for `replace`; they arm for a multi-node located fixer
                    // (Phase-2 `set_value`).
                    let (range_key, is_identity) = match &edit.collected.edit {
                        FixEdit::ReplaceRange { range, content, .. } => (
                            format!("{}..{}", range.start, range.end),
                            original
                                .get(range.clone())
                                .is_some_and(|orig| orig == content.as_slice()),
                        ),
                        _ => (String::new(), false),
                    };
                    // A PER-EDIT identity rewrite is a no-op even when OTHER edits in
                    // the batch changed the file: report it as a skip, not `Applied`,
                    // so the byte-level fixpoint (converges on "a pass applied
                    // nothing") does not read a no-op as progress and re-walk
                    // forever. The old whole-file `batch_changed` check missed a
                    // MIXED real+identity batch (Phase-2 pre-req 2); the per-edit
                    // check subsumes it -- disjoint edits cannot cancel, so an
                    // unchanged file means every edit was an identity.
                    let status = if is_identity && matches!(status, FixStatus::Applied(_)) {
                        FixStatus::declined(format!(
                            "{}: located edit at {range_key} left its span unchanged",
                            file.display()
                        ))
                    } else {
                        status
                    };
                    let mut violation = Violation::new(format!(
                        "located edit at {range_key} in {}",
                        file.display()
                    ))
                    .with_path(file.clone())
                    .with_baseline_key(range_key.clone());
                    if edit.sensitive {
                        violation.mark_sensitive(&format!(
                            "fix\0{}\0{}\0{range_key}",
                            edit.rule_index,
                            file.display()
                        ));
                    }
                    located_items
                        .entry(edit.rule_index)
                        .or_default()
                        .push(FixItem { violation, status });
                }
            }
            for (rule_index, items) in located_items {
                let entry = &self.entries[rule_index];
                push_fix_result(
                    &mut results,
                    Arc::from(entry.rule.id()),
                    entry.rule.level(),
                    items,
                    Vec::new(),
                );
            }
        }

        // Flush the compose buffer: one atomic write per file any content fixer
        // touched, in deterministic (BTreeMap) order. Keys are the resolved
        // absolute write targets (a symlink and its in-tree target share one
        // key), written directly. `flush` is `None` for a stage (`--diff`) so the
        // buffer is left unwritten for `stage_fixes` to diff; `--dry-run` has no
        // buffer, so both are no-ops there.
        //
        // A file that cannot be written is NOT fatal: it is collected, and every
        // Applied item that resolves to it is downgraded to Skipped, so the rest
        // of the pass still persists and reports. This matches the direct-write
        // path, where a failed `write_atomic` inside a fixer surfaces as
        // `Skipped("fix error: ...")` and the other fixers proceed (a single
        // read-only file must not abort the whole run or lose unrelated fixes).
        // A read-only target genuinely fails here: `write_atomic` refuses it
        // explicitly, since its temp+rename would otherwise succeed on directory
        // permissions alone and silently replace the file.
        if let Some(reported) = flush
            && let Some(buf) = &compose_buf
        {
            let mut failed: Vec<PathBuf> = Vec::new();
            for (target, bytes) in buf.borrow().iter() {
                if let Err(source) = write_atomic(target, bytes) {
                    if reported.borrow_mut().insert(target.clone()) {
                        eprintln!("alint: could not write {}: {source}", target.display());
                    }
                    failed.push(target.clone());
                }
            }
            if !failed.is_empty() {
                for rule in &mut results {
                    for item in &mut rule.items {
                        // Keying note: `failed` holds write-time canonical
                        // targets; this re-derives `resolve_write_target` at
                        // report time. If the file (or its parent) vanished
                        // between the failed write and here, the report-time
                        // canonicalize falls back to the non-canonical path
                        // and won't match, so the item stays `Applied`. That
                        // needs a write failure on a target that then
                        // disappears -- exotic, and the common case (a
                        // read-only file, permission denied) leaves the file
                        // extant so the keys match. A whole-file op can no
                        // longer vanish a composed file itself (it yields via
                        // `has_pending_write`), which removes the in-engine
                        // route to the mismatch.
                        let hits_failed = item.violation.path.as_deref().is_some_and(|p| {
                            failed.contains(&crate::rule::resolve_write_target(&root.join(p)))
                        });
                        if hits_failed && matches!(item.status, FixStatus::Applied(_)) {
                            item.status = FixStatus::errored(format!(
                                "{FIX_ERROR_PREFIX} file could not be written"
                            ));
                        }
                    }
                }
            }
        }
        // Redact only after all fixers have run: remediation needs the original
        // evidence, while every public fix report must be safe. Explicit and
        // environment-derived configuration apply to the whole owning result;
        // producer-detected implicit sensitivity remains carried per finding.
        for result in &mut results {
            let entry = self
                .entries
                .iter()
                .find(|entry| entry.rule.id() == result.rule_id.as_ref());
            let result_sensitive = entry
                .and_then(|entry| entry.spec.as_deref())
                .is_some_and(crate::RuleSpec::has_sensitive_evidence);
            let sensitive_paths =
                entry.is_some_and(|entry| self.has_sensitive_path_selection(entry));
            for failure in &mut result.analysis_failures {
                if result_sensitive {
                    failure.redact_message(&result.rule_id);
                }
                if sensitive_paths {
                    failure.mark_path_sensitive();
                }
            }
            for item in &mut result.items {
                if result_sensitive {
                    item.violation.redact_message(&result.rule_id);
                }
                if sensitive_paths {
                    item.violation.mark_path_sensitive();
                }
                if item.violation.is_sensitive() {
                    let path = item
                        .violation
                        .path
                        .as_deref()
                        .map_or_else(String::new, |path| path.to_string_lossy().into_owned());
                    let locator = format!("fix\0{}\0{path}", result.rule_id);
                    item.status = item.status.clone().redact_detail(&locator);
                }
            }
        }
        Ok((
            // A single `fix_run` never sets non-convergence: that is the
            // fixpoint loop's verdict, assembled in `Engine::fix` from the whole
            // sequence of passes. A raw pass (and `stage_fixes` / `--diff`, which
            // is single-pass) is always `false`.
            FixReport {
                results,
                non_convergent: false,
            },
            compose_buf.map_or_else(BTreeMap::new, RefCell::into_inner),
            sensitive_paths,
        ))
    }
}
