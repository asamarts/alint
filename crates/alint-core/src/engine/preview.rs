//! What `fix --dry-run` and `fix --diff` predict of a create the real `fix`
//! would demote or decline once it sees the result on disk.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::report::{FixReport, FixRuleResult, FixStatus};
use crate::walker::FileIndex;

use super::{Engine, RuleEntry};

impl Engine {
    /// `--dry-run` has no re-walk to discover that a fix will not stick, so
    /// predict the known cases the real `fix` demotes after its re-check: a
    /// would-be-`Applied` create (`file_create`, `dir_create`,
    /// `create_and_register`'s member) that the walk would list under no name
    /// the rule checks, neither the path as written nor, for a rule with a
    /// `paths:` scope, where it lands once symlinks resolve
    /// (gitignored, `ignore:`d, including by an ignore file another create in
    /// the same pass writes, or below a symlinked directory the walk does not
    /// enter), and the rule does not look for on disk itself; a target outside
    /// the rule's `paths:` or landing outside the root; or a created file a
    /// `git_tracked_only` rule would not see as tracked. Those become the same
    /// unresolved skip the real run reports. Other ways a fix can fail its
    /// re-check (a cascade through another rule, content a fixer writes that
    /// the rule still rejects, an edit to an existing ignore file) are only
    /// found by running it; `docs/site/cli/fix.md` documents the limit.
    pub(super) fn predict_unresolved_creates(
        &self,
        root: &Path,
        index: &FileIndex,
        walk_opts: &crate::WalkOptions,
        report: &mut FixReport,
    ) {
        // Each would-be-applied create: where its result and item sit in the
        // report, its rule, the names the walk could list it under (or the path
        // as written when it lands outside the root), whether it is a directory
        // and, for a file, its content.
        let mut creates = Vec::new();
        for (result, rr) in report.results.iter().enumerate() {
            let Some(entry) = self.entries.iter().find(|e| e.rule.id() == &*rr.rule_id) else {
                continue;
            };
            let Some(fixer) = entry.rule.fixer() else {
                continue;
            };
            for (item, it) in rr.items.iter().enumerate() {
                if !matches!(it.status, FixStatus::Applied(_)) {
                    continue;
                }
                let (path, is_dir, content) =
                    match fixer.created_file(&it.violation, root, entry.allow_out_of_root) {
                        Some((path, content)) => (path, false, Some(content)),
                        None => match fixer.created_dir() {
                            Some(dir) => (dir.to_path_buf(), true, None),
                            None => continue,
                        },
                    };
                let names = Self::created_names(root, &path).ok_or_else(|| path.clone());
                creates.push((result, item, entry, path, names, is_dir, content));
            }
        }
        // The walk after the fix reads the ignore files those creates write,
        // where they land. A create skips a file that is already there, so the
        // first create of a path wins, whichever name (through an in-tree
        // symlink) it uses.
        let mut created_ignores: Vec<(&Path, &[u8])> = Vec::new();
        for (_, _, _, _, names, _, content) in &creates {
            let (Ok(names), Some(content)) = (names, content.as_deref()) else {
                continue;
            };
            let landed = names.last().expect("a create lands somewhere");
            if matches!(
                landed.file_name().and_then(std::ffi::OsStr::to_str),
                Some(".gitignore" | ".ignore")
            ) && !created_ignores.iter().any(|(path, _)| path == landed)
            {
                created_ignores.push((landed.as_path(), content));
            }
        }
        // `git ls-files`, once, when a `git_tracked_only` rule creates a file.
        let tracked = std::cell::OnceCell::new();
        for (result, item, entry, path, names, is_dir, _) in &creates {
            // A sensitive item's reason leaves the target out: its rule's
            // configuration or evidence can hold it, as the redacted summary
            // already does.
            let sensitive = report.results[*result].items[*item]
                .violation
                .is_sensitive();
            let shown = |path: &Path| {
                if sensitive {
                    "the target".to_string()
                } else {
                    path.display().to_string()
                }
            };
            let why = match names {
                Err(path) => Some(format!(
                    "{} is outside the root, so the rule would never see it",
                    shown(path)
                )),
                // A rule without a `paths:` scope (`dir_exists`, `cross_file`)
                // looks for the path as written, normalized as it normalizes
                // it, and finds nothing under another name: one written out of
                // the root (absolute, or `..` back in) never matches.
                Ok(_) if entry.rule.path_scope().is_none() => {
                    match crate::pathsafe::normalize_confined(path) {
                        Some(written) => Self::predicted_unresolved(
                            root,
                            index,
                            walk_opts,
                            entry,
                            &written,
                            &shown(&written),
                            *is_dir,
                            &created_ignores,
                            &tracked,
                        ),
                        None => Some(format!(
                            "{} is written as a path outside the root, so the rule would never \
                             match it",
                            shown(path)
                        )),
                    }
                }
                // Resolved when the rule sees the file under any of its names;
                // otherwise the reason for the first, the path as written.
                Ok(names) => {
                    let mut reasons = names.iter().map(|name| {
                        Self::predicted_unresolved(
                            root,
                            index,
                            walk_opts,
                            entry,
                            name,
                            &shown(name),
                            *is_dir,
                            &created_ignores,
                            &tracked,
                        )
                    });
                    let first = reasons.next().flatten();
                    first.filter(|_| reasons.all(|reason| reason.is_some()))
                }
            };
            let it = &mut report.results[*result].items[*item];
            if let (Some(why), FixStatus::Applied(summary)) = (why, &it.status) {
                it.status = FixStatus::unresolved(format!(
                    "fix would run ({summary}) but would not resolve the violation: {why}"
                ));
            }
        }
    }

    /// The root-relative names the walk could list a create of `path` under:
    /// the path as written, when it reads as one inside the root, then where
    /// the file lands once symlinks resolve (last, and once when the two
    /// agree). `None` when it lands outside the root. A fixer joins the path
    /// to the root after normalizing it lexically (`a/../b` is `b`) when it
    /// stays inside, and as written otherwise (only `allow_out_of_root`
    /// permits that): an absolute path replaces the root, and `..` steps up
    /// from the root's real location, which can lead back into it. A path
    /// written as given has each `..` resolved by the OS after the symlinks
    /// before it, so it names a place inside the root through what follows
    /// its last `..` (`<root>/sub/../alias/x` is `alias/x`).
    pub(super) fn created_names(root: &Path, path: &Path) -> Option<Vec<PathBuf>> {
        let canonical = root.canonicalize().ok()?;
        let confined = crate::pathsafe::normalize_confined(path);
        // Joining an absolute path gives that path.
        let joined = root.join(confined.as_deref().unwrap_or(path));
        let written = confined.or_else(|| {
            let components: Vec<_> = joined.components().collect();
            let named = match components
                .iter()
                .rposition(|c| matches!(c, std::path::Component::ParentDir))
            {
                Some(last_up) => {
                    let head: PathBuf = components[..=last_up].iter().collect();
                    let rest: PathBuf = components[last_up + 1..].iter().collect();
                    crate::pathsafe::resolve_existing_prefix(&head)?.join(rest)
                }
                None => joined.clone(),
            };
            named
                .strip_prefix(root)
                .or_else(|_| named.strip_prefix(&canonical))
                .ok()
                .and_then(crate::pathsafe::normalize_confined)
        });
        let landed = crate::pathsafe::resolve_existing_prefix(&joined)?;
        let landed = crate::pathsafe::normalize_confined(landed.strip_prefix(&canonical).ok()?)?;
        Some(match written {
            Some(written) if written != landed => vec![written, landed],
            _ => vec![landed],
        })
    }

    /// Why a create of `target` (root-relative, a directory when `is_dir`,
    /// named `shown` in the reason) would not resolve `entry`'s violation, or
    /// `None` when it would: the
    /// rule sees only tracked files and the target is not one, the walk would
    /// leave it out (counting the ignore files the pass creates) and the rule
    /// does not look for it on disk, or it lies outside the rule's `paths:`.
    #[allow(clippy::too_many_arguments)]
    fn predicted_unresolved(
        root: &Path,
        index: &FileIndex,
        walk_opts: &crate::WalkOptions,
        entry: &RuleEntry,
        target: &Path,
        shown: &str,
        is_dir: bool,
        created_ignores: &[(&Path, &[u8])],
        tracked: &std::cell::OnceCell<Option<HashSet<PathBuf>>>,
    ) -> Option<String> {
        if !is_dir
            && entry.rule.git_tracked_mode() != crate::GitTrackedMode::Off
            && !tracked
                .get_or_init(|| crate::git::collect_tracked_paths(root))
                .as_ref()
                .is_some_and(|t| t.contains(target))
        {
            return Some(format!(
                "{shown} is not tracked in git, and the rule (`git_tracked_only`) sees only \
                 tracked files"
            ));
        }
        if !is_dir && entry.rule.sees_path_past_walk(target) {
            // The rule's own stat refuses a path that resolves out of the root.
            return (!crate::pathsafe::resolved_within_root(&root.join(target), root)).then(|| {
                format!(
                    "{shown} resolves outside the root through a symlink, so the rule would \
                     never see it"
                )
            });
        }
        if crate::walker::would_walk_skip(root, walk_opts, target, is_dir, created_ignores) {
            Some(format!(
                "{shown} is excluded from the walk (an ignore file, `ignore:`, or a symlink \
                 the walk does not enter), so the rule would never see it"
            ))
        } else if entry
            .rule
            .path_scope()
            .is_some_and(|scope| !scope.matches(target, index))
        {
            Some(format!("{shown} is outside the rule's `paths:`"))
        } else {
            None
        }
    }

    /// A preview pass leaves a create that needs its directory to exist
    /// ([`Fixer::needs_existing_directory`]) to this step, which sees every
    /// create of the pass: the real `fix` makes the file once the directory
    /// exists, which another create can arrange (a `dir_create`, or a create
    /// that makes its parents), in the same pass or the next one. Without one,
    /// the create declines as `fix` declines it. Returns the paths, as the
    /// fixers wrote them, of the creates it declined, so `--diff` leaves them
    /// out.
    ///
    /// [`Fixer::needs_existing_directory`]: crate::rule::Fixer::needs_existing_directory
    pub(super) fn decline_creates_into_missing_directories(
        &self,
        root: &Path,
        results: &mut [FixRuleResult],
    ) -> Vec<PathBuf> {
        // Most configurations have no such fixer: skip the walk over every
        // finding.
        if !self.entries.iter().any(|entry| {
            entry
                .rule
                .fixer()
                .is_some_and(crate::rule::Fixer::needs_existing_directory)
        }) {
            return Vec::new();
        }
        // A create's target joined to the root as the fixers join it.
        let joined = |path: &Path| {
            root.join(
                crate::pathsafe::normalize_confined(path)
                    .unwrap_or_else(|| path.components().collect()),
            )
        };
        // Each would-be create, with its fixer, the owning rule's permission
        // and its violation.
        let creates = || {
            results.iter().enumerate().flat_map(|(result, rr)| {
                let entry = self.entries.iter().find(|e| e.rule.id() == &*rr.rule_id);
                let fixer = entry.and_then(|entry| entry.rule.fixer());
                rr.items
                    .iter()
                    .enumerate()
                    .filter(|(_, it)| matches!(it.status, FixStatus::Applied(_)))
                    .filter_map(move |(item, it)| {
                        Some((
                            result,
                            item,
                            fixer?,
                            entry?.allow_out_of_root,
                            &it.violation,
                        ))
                    })
            })
        };
        let mut waiting = Vec::new();
        for (result, item, fixer, allow_out_of_root, violation) in creates() {
            if !fixer.needs_existing_directory() {
                continue;
            }
            let Some((path, _)) = fixer.created_file(violation, root, allow_out_of_root) else {
                continue;
            };
            if let Some(directory) = joined(&path).parent()
                && !directory.is_dir()
            {
                waiting.push((result, item, path, directory.to_path_buf()));
            }
        }
        if waiting.is_empty() {
            return Vec::new();
        }
        // Where the directories the pass's other creates make land.
        let made: Vec<PathBuf> = creates()
            .filter(|(_, _, fixer, _, _)| !fixer.needs_existing_directory())
            .filter_map(|(_, _, fixer, allow_out_of_root, violation)| {
                let directory = match fixer.created_file(violation, root, allow_out_of_root) {
                    Some((path, _)) => joined(&path).parent()?.to_path_buf(),
                    None => joined(fixer.created_dir()?),
                };
                crate::pathsafe::resolve_existing_prefix(&directory)
            })
            .collect();
        let mut declined = Vec::new();
        for (result, item, path, directory) in waiting {
            if Self::directory_location(&directory)
                .is_some_and(|needed| made.iter().any(|dir| dir.starts_with(&needed)))
            {
                continue;
            }
            results[result].items[item].status = FixStatus::declined(format!(
                "{} is in a directory that does not exist (set `create_parents: true` to create it)",
                path.display()
            ));
            declined.push(path);
        }
        declined
    }

    /// Where `directory` has to exist for a write into it, resolved as the OS
    /// resolves it, or `None` when a directory before one of its `..` is
    /// missing: the write needs that one, not just the place it leads back to.
    fn directory_location(directory: &Path) -> Option<PathBuf> {
        let components: Vec<_> = directory.components().collect();
        let Some(last_up) = components
            .iter()
            .rposition(|c| matches!(c, std::path::Component::ParentDir))
        else {
            return crate::pathsafe::resolve_existing_prefix(directory);
        };
        let head: PathBuf = components[..=last_up].iter().collect();
        let rest: PathBuf = components[last_up + 1..].iter().collect();
        crate::pathsafe::resolve_existing_prefix(&head.canonicalize().ok()?.join(rest))
    }
}
