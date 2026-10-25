//! Dependency preparation, changed-path routing, and manifest gates.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::rule::{Rule, Violation};
use crate::walker::FileIndex;
use crate::when::WhenEnv;

use super::{
    AnalysisTimeCache, Engine, GitTrackedIndexes, ManifestGate, RuleTreeVisitor,
    cached_analysis_time, effective_path_scope, manifest_scope_failures, normalize_analysis_root,
    visit_active_rule_tree_with_registry, visit_rule_tree_with_registry,
};

impl Engine {
    /// Resolve active Git dependencies from each configured rule tree's
    /// capability metadata. The vector stays aligned with `self.entries`.
    pub(super) fn git_requirements(
        &self,
        index: &FileIndex,
        when_env: &WhenEnv<'_>,
    ) -> Result<Vec<bool>> {
        self.entries
            .iter()
            .map(|entry| {
                if entry
                    .when
                    .as_ref()
                    .is_some_and(|expr| !matches!(expr.evaluate(when_env), Ok(true)))
                {
                    Ok(false)
                } else {
                    entry.requires_git(&self.registry, index, when_env)
                }
            })
            .collect()
    }

    /// Probe repository usability once when any configured rule tree has an
    /// active Git dependency. `Some(empty)` is a usable unborn/empty repository;
    /// `None` is either "not needed" or "unavailable", distinguished by the
    /// per-entry requirements at the routing sites.
    pub(super) fn collect_git_tracked_if_needed(
        root: &Path,
        required: &[bool],
    ) -> Option<HashSet<PathBuf>> {
        required
            .iter()
            .any(|required| *required)
            .then(|| crate::git::collect_tracked_paths(root))
            .flatten()
    }

    /// Build the lazy per-file blame cache when a loaded rule asks for it,
    /// reusing the tracked set from the single repository-usability probe.
    pub(super) fn build_blame_cache_if_needed(
        &self,
        root: &Path,
        tracked: Option<&HashSet<PathBuf>>,
    ) -> Option<crate::git::BlameCache> {
        // Nested `require:` rules count too: a `git_blame_age` under
        // `for_each_dir` reads the same cache.
        let mut any_wants = self.entries.iter().any(|e| e.rule.wants_git_blame());
        if !any_wants {
            let _ = self.visit_all_rules(&mut |rule, _spec| {
                any_wants |= rule.wants_git_blame();
                Ok(())
            });
        }
        if !any_wants {
            return None;
        }
        Some(crate::git::BlameCache::new_with_tracked_paths(
            root.to_path_buf(),
            tracked?,
        ))
    }

    pub(super) fn resolve_analysis_time_if_needed(
        &self,
        root: &Path,
    ) -> Option<crate::AnalysisTimeResolution> {
        let any_wants = *self.analysis_time_required.get_or_init(|| {
            self.entries
                .iter()
                .any(|entry| entry.wants_analysis_time(&self.registry).unwrap_or(false))
        });
        if !any_wants {
            return None;
        }
        if let Some(resolution) = self.analysis_time {
            return Some(resolution);
        }
        let mut cache = self.analysis_time_cache.lock();
        if let Some(cached) = cache.as_ref() {
            return Some(cached_analysis_time(cached, root));
        }
        let entry = AnalysisTimeCache {
            normalized_root: normalize_analysis_root(root),
            resolution: crate::resolve_analysis_time(root, None),
        };
        let resolution = cached_analysis_time(&entry, root);
        *cache = Some(entry);
        Some(resolution)
    }

    /// Build a [`FileIndex`] containing only the entries the user
    /// said they care about (the `--changed` set). Returns `None`
    /// when no changed-set is configured — callers fall back to
    /// the full index.
    /// Build the `--changed`-filtered index used to confine per-file rules to the
    /// working-tree diff. `created` (2b) is the set of files THIS `fix` run created
    /// on an earlier pass: they are not in the git diff, but a fix in scope made
    /// them, so they join the changed set and a per-file content rule can complete
    /// a create-then-fix cascade on them (the design's "changed set plus files a
    /// fix in scope created", auto-fix.md 5.7). `None` (the `check` path) confines
    /// to the diff alone.
    pub(super) fn build_filtered_index(
        &self,
        full: &FileIndex,
        created: Option<&HashSet<PathBuf>>,
    ) -> Option<FileIndex> {
        let set = self.changed_paths.as_ref()?;
        let entries = full
            .entries
            .iter()
            .filter(|e| set.contains(&*e.path) || created.is_some_and(|c| c.contains(&*e.path)))
            .cloned()
            .collect();
        let filtered = FileIndex::from_entries(entries);
        full.share_relation_caches(&filtered);
        Some(filtered)
    }

    /// Build the per-mode pre-filtered indexes for git-tracked
    /// rules. v0.9.11 structural fix for the
    /// `git_tracked_only`-silently-dropped recurrence-risk
    /// shape (see `docs/design/v0.9/git-tracked-filtered-index.md`).
    ///
    /// Returns `None` when no rule opts in (no
    /// `GitTrackedMode::FileOnly` or `DirAware` declared). When the tracked set
    /// is unavailable, defensive empty indexes are built; metadata-driven
    /// dependency routing prevents every active owning rule from dispatching
    /// against them.
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
    pub(super) fn build_git_tracked_indexes(
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

        // Repository probing failed. Build defensive empty indexes so no
        // consumer can accidentally see the full tree; active dependent rules
        // are short-circuited to `resource_read` analysis failures before
        // dispatch.
        let Some(tracked) = tracked else {
            let file_only = any_file_only.then(|| FileIndex::from_entries(Vec::new()));
            let dir_aware = any_dir_aware.then(|| FileIndex::from_entries(Vec::new()));
            if let Some(index) = file_only.as_ref() {
                full.share_relation_caches(index);
            }
            if let Some(index) = dir_aware.as_ref() {
                full.share_relation_caches(index);
            }
            return Some(GitTrackedIndexes {
                file_only,
                dir_aware,
            });
        };

        let file_only = if any_file_only {
            let entries = full
                .entries
                .iter()
                .filter(|e| !e.is_dir && tracked.contains(&*e.path))
                .cloned()
                .collect();
            let index = FileIndex::from_entries(entries);
            full.share_relation_caches(&index);
            Some(index)
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
            let index = FileIndex::from_entries(entries);
            full.share_relation_caches(&index);
            Some(index)
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
    /// Whether some rule will look up a `scope_filter.has_ancestor` marker on the
    /// `--changed`-filtered index, so that index needs the full tree's file set.
    ///
    /// A rule counts only if it has such a predicate (from the scope it exposes, or
    /// from its spec, so a kind that hides its scope is still seen) AND it will be
    /// evaluated on the filtered index: it is not `--changed`-skipped, not
    /// `requires_full_index`, has no changed dependency, and is not git-tracked
    /// (each of those gets another index). `per_file_on_filtered` is `false` for
    /// `check`, whose per-file pass always matches scope against the full index,
    /// and `true` for `fix`, which evaluates per-file rules on the filtered index.
    /// Nested rules are evaluated with their parent's full context, so only
    /// top-level rules matter.
    pub(super) fn needs_tree_files(
        &self,
        index: &FileIndex,
        created: Option<&HashSet<PathBuf>>,
        per_file_on_filtered: bool,
    ) -> bool {
        self.entries.iter().any(|entry| {
            let rule = entry.rule.as_ref();
            let exposed = rule
                .as_per_file()
                .map(crate::rule::PerFileRule::path_scope)
                .or_else(|| rule.path_scope())
                .and_then(crate::Scope::scope_filter)
                .is_some_and(|filter| !filter.has_ancestor_names().is_empty());
            let declared = entry.spec.as_deref().is_some_and(|spec| {
                spec.scope_filter
                    .as_ref()
                    .and_then(|f| f.has_ancestor.as_ref())
                    .is_some_and(|names| !names.is_empty())
            });
            if !exposed && !declared {
                return false;
            }
            if rule.as_per_file().is_some() && !per_file_on_filtered {
                return false;
            }
            rule.git_tracked_mode() == crate::rule::GitTrackedMode::Off
                && !rule.requires_full_index()
                && !self.dependency_changed(rule)
                && !self.skip_for_changed(rule, index, created)
        })
    }

    /// Whether a file the rule depends on, outside its own scope, is in the
    /// `--changed` set ([`Rule::changed_dependencies`]). Always `false` without
    /// `--changed`.
    pub(super) fn dependency_changed(&self, rule: &dyn Rule) -> bool {
        let Some(set) = &self.changed_paths else {
            return false;
        };
        rule.changed_dependencies()
            .into_iter()
            .any(|dep| crate::pathsafe::normalize_confined(dep).is_some_and(|p| set.contains(&p)))
    }

    pub(super) fn skip_for_changed(
        &self,
        rule: &dyn Rule,
        index: &FileIndex,
        created: Option<&HashSet<PathBuf>>,
    ) -> bool {
        let Some(set) = &self.changed_paths else {
            return false;
        };
        if self.dependency_changed(rule) {
            return false;
        }
        let Some(scope) = rule.path_scope() else {
            return false;
        };
        // If manifest resolution failed, matching through the unavailable set
        // could falsely classify an include-scoped rule as inactive. Ignore
        // only that predicate while deciding invocation activity: globs and all
        // other filters can still prove that the rule is outside `--changed`.
        let manifest_failed = scope.scope_filter().is_some_and(|filter| {
            filter
                .manifest_predicates()
                .iter()
                .any(|predicate| index.manifest_failure(predicate.cache_key()).is_some())
        });
        // A file THIS `fix` run created (2b) joins the changed set, so a rule scoped
        // ENTIRELY to created files is not skipped -- its cascade can complete.
        // `check` passes `None` (nothing is created during a check), so `check` and
        // `fix` skip the same rules on the original tree; `fix` only additionally
        // keeps a rule alive for files it creates.
        !set.iter().chain(created.into_iter().flatten()).any(|p| {
            if manifest_failed {
                scope.matches_without_manifest(p, index)
            } else {
                scope.matches(p, index)
            }
        })
    }

    /// Pre-resolution invocation check for manifest-backed scopes.
    ///
    /// Manifest membership itself cannot decide whether its own source should
    /// be read. Match the configured globs and every already-resolved
    /// non-manifest predicate; if none can see a changed/created path, the rule
    /// is invocation-inactive and its manifest remains untouched.
    pub(super) fn skip_for_changed_without_manifest(
        &self,
        rule: &dyn Rule,
        index: &FileIndex,
        created: Option<&HashSet<PathBuf>>,
    ) -> bool {
        let Some(set) = &self.changed_paths else {
            return false;
        };
        if self.dependency_changed(rule) {
            return false;
        }
        let Some(scope) = effective_path_scope(rule) else {
            return false;
        };
        !set.iter()
            .chain(created.into_iter().flatten())
            .any(|path| scope.matches_without_manifest(path, index))
    }

    /// Whether a fix for `violation` under `rule` would write OUTSIDE the
    /// `--changed` blast radius (the diff plus files this run created). Only
    /// meaningful when `--changed` is active (`changed_paths` set); returns
    /// `false` otherwise, so a full `fix` is unaffected. A path-bearing violation
    /// is judged by its own path; a PATHLESS one (a create whose target comes from
    /// config, e.g. `file_exists`) is judged by the rule's scope matching some
    /// in-scope path -- the same test [`skip_for_changed`] uses. A rule with no
    /// scope for a pathless violation cannot be confined (an additive create with a
    /// config-chosen target), so it is treated as in-scope. Out-of-scope writes are
    /// demoted to Suggestions in the fix loop rather than applied (would widen the
    /// blast radius) or dropped silently (2b, auto-fix.md 5.7).
    ///
    /// SYMLINK write-through (by design, whole-phase audit F2): this judges the
    /// LOGICAL `violation.path` (what the diff names), not the resolved physical
    /// target [`write_atomic`] writes through to. If an in-scope path is a symlink
    /// to an out-of-diff file, the write follows the link (`write_atomic` resolves
    /// symlinks so the link node is preserved) and physically lands outside the
    /// changed set. That matches how `--changed` membership is defined (git reports
    /// logical paths) and how a fix WITHOUT `--changed` already writes through
    /// symlinks; the demote gate is a blast-radius policy on named paths, not a
    /// physical-containment sandbox.
    pub(super) fn writes_outside_changed(
        &self,
        rule: &dyn Rule,
        violation: &Violation,
        index: &FileIndex,
        created: &HashSet<PathBuf>,
    ) -> bool {
        let Some(set) = &self.changed_paths else {
            return false;
        };
        match violation.path.as_deref() {
            Some(p) => !(set.contains(p) || created.contains(p)),
            None => match rule.path_scope() {
                Some(scope) => !set
                    .iter()
                    .chain(created.iter())
                    .any(|p| scope.matches(p, index)),
                None => false,
            },
        }
    }

    /// Visit every configured rule, including recursively nested `require:`
    /// rules synthesized with stable placeholder path tokens. Nested selectors
    /// build their children lazily during evaluation, but scope-filter caches
    /// must be populated before any rule evaluates; exposing the nested specs
    /// through [`Rule::nested_rule_specs`] lets this preparation pass discover
    /// the same effective runtime scopes ahead of dispatch.
    pub(super) fn visit_all_rules(&self, visit: &mut RuleTreeVisitor<'_>) -> Result<()> {
        for entry in &self.entries {
            visit_rule_tree_with_registry(
                &self.registry,
                entry.rule.as_ref(),
                entry.spec.as_deref(),
                visit,
            )?;
        }
        Ok(())
    }

    /// Visit configured rule trees whose top-level and nested gates can
    /// activate for this index.
    ///
    /// A false or invalid condition cannot consume a runtime dependency: the
    /// dispatch path either skips it or reports the condition failure itself.
    pub(super) fn visit_active_rules(
        &self,
        index: &FileIndex,
        when_env: &WhenEnv<'_>,
        visit: &mut RuleTreeVisitor<'_>,
    ) -> Result<()> {
        for entry in &self.entries {
            if entry
                .when
                .as_ref()
                .is_some_and(|expr| !matches!(expr.evaluate(when_env), Ok(true)))
            {
                continue;
            }
            visit_active_rule_tree_with_registry(
                &self.registry,
                entry.rule.as_ref(),
                entry.spec.as_deref(),
                index,
                when_env,
                visit,
            )?;
        }
        Ok(())
    }

    pub(super) fn any_expect_matches(&self) -> Result<bool> {
        let mut found = false;
        self.visit_all_rules(&mut |_rule, spec| {
            found |= spec.is_some_and(|spec| spec.expect_matches);
            Ok(())
        })?;
        Ok(found)
    }

    /// Resolve every distinct active `scope_filter.changed_since:` ref and
    /// cache each `<ref>...HEAD` diff on the index once per run, before any
    /// `Scope::matches` reads it. Repository usability has already been probed;
    /// false or invalid top-level and nested `when:` gates are excluded so they
    /// cannot trigger a Git operation or a stale-ref error.
    pub(super) fn resolve_changed_paths(
        &self,
        root: &Path,
        index: &FileIndex,
        when_env: &WhenEnv<'_>,
    ) -> Result<()> {
        if index.changed_paths_initialized() {
            return Ok(());
        }
        let mut refs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        self.visit_active_rules(index, when_env, &mut |rule, _spec| {
            // Per-file rules expose their scope via `PerFileRule::path_scope`
            // (the `Rule::path_scope` default is `None`); rule-major rules
            // expose it via `Rule::path_scope`. changed_since is a per-file
            // concept, so prefer the per-file scope, falling back to the
            // rule-level one.
            let scope = rule
                .as_per_file()
                .map(crate::rule::PerFileRule::path_scope)
                .or_else(|| rule.path_scope());
            if let Some(scope) = scope
                && let Some(filter) = scope.scope_filter()
                && let Some(since) = filter.changed_since()
            {
                refs.insert(since.to_string());
            }
            Ok(())
        })?;
        if refs.is_empty() {
            return Ok(());
        }
        let mut map = std::collections::HashMap::new();
        for since in refs {
            match crate::git::collect_changed_paths_checked(root, &since) {
                Ok(Some(set)) => {
                    map.insert(since, set);
                }
                Ok(None) => {
                    map.insert(since, std::collections::HashSet::new());
                }
                Err(crate::git::CommitRangeError::BadRange { stderr }) => {
                    return Err(crate::error::Error::Other(format!(
                        "scope_filter.changed_since: could not resolve `{since}...HEAD`: \
                         {stderr}. Common cause: shallow clone. In a GitHub Actions PR \
                         workflow, use `actions/checkout@v4` with `fetch-depth: 0` so the \
                         base ref is reachable."
                    )));
                }
            }
        }
        index.set_changed_paths(map);
        Ok(())
    }

    /// Resolve every active rule's manifest-derived path set once per run and
    /// cache it on the index. Keys identify `(source, extract, derive_target)`,
    /// so shared predicates read and parse once. Missing, unreadable, malformed,
    /// or non-string manifests cache a typed structural failure beside an empty
    /// defensive set; active owning rules consume that failure before scope
    /// matching. False or invalid top-level and nested `when:` gates, plus
    /// rules excluded by the invocation, do not read their manifests. A
    /// successfully resolved empty include set still uses `expect_nonempty` to
    /// warn about an intentional but potentially surprising no-op. Reads are
    /// confined to the repo root; `source` was confined at build time.
    /// Fail loud if a rule declares a manifest-derived scope (`include`/
    /// `exclude_manifest_paths`) but exposes no scope for the engine to resolve
    /// (`as_per_file` and `path_scope` both `None`). Without this the predicate
    /// silently no-ops: the resolver never discovers it, so its cache stays empty
    /// and `Scope::matches` reads the empty set (include matches nothing, exclude
    /// excludes nothing) — the exact silent-no-op class fuzzing found on rule
    /// kinds that apply a scope but forgot to expose it (see `Rule::path_scope`).
    pub(super) fn ensure_manifest_scope_resolvable(&self) -> Result<()> {
        self.visit_all_rules(&mut |rule, spec| {
            let Some(sf) = spec.and_then(|spec| spec.scope_filter.as_ref()) else {
                return Ok(());
            };
            if sf.include_manifest_paths.is_none() && sf.exclude_manifest_paths.is_none() {
                return Ok(());
            }
            if rule.as_per_file().is_none() && rule.path_scope().is_none() {
                return Err(Error::rule_config(
                    rule.id(),
                    "this rule kind does not support `scope_filter.include_manifest_paths` / \
                     `exclude_manifest_paths`: it exposes no per-file scope for the engine to \
                     resolve, so the manifest set would silently never apply"
                        .to_string(),
                ));
            }
            Ok(())
        })
    }

    pub(super) fn resolve_manifest_paths(
        &self,
        root: &Path,
        index: &FileIndex,
        when_env: &WhenEnv<'_>,
        created: Option<&HashSet<PathBuf>>,
    ) -> Result<()> {
        if index.manifest_paths_initialized() {
            return Ok(());
        }
        // Group predicates by cache key: rules sharing a `(source, extract,
        // derive_target)` config resolve once. BTreeMap keeps the resolution
        // order deterministic (ADR-0003). The key deliberately omits `sense` /
        // `expect_nonempty` (the resolved SET doesn't depend on them), so a group
        // can mix `include` and `exclude` predicates over the same manifest.
        let mut groups: std::collections::BTreeMap<
            String,
            Vec<crate::scope_filter::ManifestPredicate>,
        > = std::collections::BTreeMap::new();
        for entry in &self.entries {
            if entry
                .when
                .as_ref()
                .is_some_and(|expr| !matches!(expr.evaluate(when_env), Ok(true)))
                || (!entry.expect_matches()
                    && !entry.rule.requires_full_index()
                    && self.skip_for_changed_without_manifest(entry.rule.as_ref(), index, created))
            {
                continue;
            }
            visit_active_rule_tree_with_registry(
                &self.registry,
                entry.rule.as_ref(),
                entry.spec.as_deref(),
                index,
                when_env,
                &mut |rule, _spec| {
                    if let Some(scope) = effective_path_scope(rule)
                        && let Some(filter) = scope.scope_filter()
                    {
                        for pred in filter.manifest_predicates() {
                            groups
                                .entry(pred.cache_key().to_string())
                                .or_default()
                                .push(pred.clone());
                        }
                    }
                    Ok(())
                },
            )?;
        }
        if groups.is_empty() {
            index.set_manifest_paths(HashMap::new(), HashMap::new());
            return Ok(());
        }
        let mut paths = HashMap::new();
        let mut failures = HashMap::new();
        for (key, group) in groups {
            // Every predicate in the group shares one `(source, extract,
            // derive_target)`, so any member resolves the same set. A direct
            // confined read reaches a gitignored manifest and matches `explain`.
            // Cache failures beside an empty set; active owning rules route the
            // typed failure before policy evaluation.
            let rep = &group[0];
            let resolution = crate::scope_filter::read_manifest_confined(root, rep.source())
                .and_then(|text| rep.resolve_set(&text));
            let set = match resolution {
                Ok(resolved) => crate::scope_filter::ManifestSet::from_paths(resolved),
                Err(failure) => {
                    failures.insert(key.clone(), failure);
                    crate::scope_filter::ManifestSet::from_paths(Vec::new())
                }
            };
            // A successfully resolved but empty include remains a diagnostic
            // warning unless the user opted out. Read/parse failures are findings,
            // not duplicate log-only warnings.
            if !failures.contains_key(&key)
                && set.is_empty()
                && group
                    .iter()
                    .any(crate::scope_filter::ManifestPredicate::warns_on_empty)
            {
                tracing::warn!(
                    "scope_filter.include_manifest_paths: `{}` resolved to no paths (the manifest \
                     declares none), so the rule matches nothing. Set `expect_nonempty: false` if \
                     that is intended.",
                    rep.source().display()
                );
            }
            paths.insert(key, set);
        }
        index.set_manifest_paths(paths, failures);
        Ok(())
    }

    pub(super) fn manifest_gates_by_entry(
        &self,
        index: &FileIndex,
        when_env: &WhenEnv<'_>,
    ) -> Result<Vec<ManifestGate>> {
        self.entries
            .iter()
            .map(|entry| {
                if entry
                    .when
                    .as_ref()
                    .is_some_and(|expr| !matches!(expr.evaluate(when_env), Ok(true)))
                {
                    Ok(ManifestGate::default())
                } else {
                    manifest_scope_failures(entry, &self.registry, index, when_env)
                }
            })
            .collect()
    }
}
