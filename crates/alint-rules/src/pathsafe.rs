//! Path confinement — keep a config-author-controlled path inside the
//! repo root (the untrusted-`extends:` threat the `SPAWNING_RULE_KINDS`
//! gate also defends against). Two layers, because a lexical check alone
//! is not enough:
//!
//! - [`normalize_confined`] (re-exported from `alint-core`, where the
//!   lexical gate and its Kani proof now live) is the *lexical* gate:
//!   pure, no filesystem access. It rejects absolute and `..`-escaping
//!   paths but is **symlink-blind** — a lexically-confined `link/x` still
//!   resolves outside the tree if `link` is an in-repo symlink pointing
//!   out of it.
//! - [`confine_read`] / [`resolved_within_root`] add the *filesystem*
//!   gate: after joining under the root they follow symlinks and re-verify
//!   containment. Every config-derived **read** must go through these, not
//!   the bare lexical pass (the walker's `FileIndex` is separately
//!   symlink-pruned, so index *lookups* are already safe).
//!
//! Design: `docs/design/v0.12/path-confinement.md`.

use std::path::{Path, PathBuf};

// The lexical confinement primitive (`normalize_confined` + its private
// `is_confined` helper) now lives in `alint-core`, along with its Kani
// proof and property tests. Re-exported here so the crate-internal
// `crate::pathsafe::normalize_confined` call sites and the filesystem-aware
// layer below keep resolving unchanged.
pub(crate) use alint_core::normalize_confined;

/// The verdict for a config-derived *read* path, accounting for the
/// rule's `allow_out_of_root` permission (see
/// `docs/design/v0.12/allow_out_of_root.md`).
pub(crate) enum Confined {
    /// In-tree (lexically normalised) - read as today.
    In(PathBuf),
    /// Escapes the root, but the rule is permitted to read it. The
    /// caller reads `root.join(path)` (absolute → itself; `../../x` →
    /// up) and emits an informational note via [`out_of_root_note`].
    AllowedEscape(PathBuf),
    /// Escapes the root and the rule is not permitted - the caller
    /// emits an "escapes the repo root" violation and does not read.
    Denied,
    /// In a Git directory, by name or through an in-repo symlink, whatever
    /// `allow_out_of_root` says: a file there can hold credentials (an
    /// `http.*.extraheader`). The caller reports
    /// [`alint_core::git_directory_read_refusal`] and does not read.
    GitDirectory,
}

/// Confine a config-derived read path, honouring an
/// `allow_out_of_root` permission. `allow_escape` is the per-rule
/// flag the loader resolved from the top-level policy; it is `false`
/// for every rule unless the user's own top-level config opted the
/// rule (or its kind) in.
pub(crate) fn confine(path: &Path, allow_escape: bool) -> Confined {
    match normalize_confined(path) {
        Some(p) => Confined::In(p),
        // Rebuilt from its components, as an in-tree path is: `..` stays, but a
        // trailing `/` goes, which would make a create's write fail on a file.
        None if allow_escape => Confined::AllowedEscape(path.components().collect()),
        None => Confined::Denied,
    }
}

/// Refuse at load a path option `field` names for a rule to read by name (not
/// through the walk), when it names a Git directory in any spelling a file
/// system opens as `.git`: no rule reads there, since a file there can hold
/// credentials. A path that reaches one through a symlink is refused at read
/// ([`confine_read`], the relation read cache).
pub(crate) fn reject_git_directory_read(
    rule_id: &str,
    field: &str,
    path: impl AsRef<Path>,
) -> alint_core::Result<()> {
    match git_directory_read_option(field, path.as_ref()) {
        Some(message) => Err(alint_core::Error::rule_config(rule_id, message)),
        None => Ok(()),
    }
}

/// [`reject_git_directory_read`]'s message, for a parser that wraps its own
/// errors: `` `source.file` .git/config is in a Git directory; rules never
/// read there ``.
pub(crate) fn git_directory_read_option(field: &str, path: &Path) -> Option<String> {
    alint_core::path_names_git_directory(path)
        .then(|| format!("`{field}` {}", alint_core::git_directory_read_refusal(path)))
}

/// The finding for a named read [`confine_read`] refused because `path` is in
/// a Git directory: an analysis failure (the rule has no input to judge), in
/// the terms of the load-time refusal.
pub(crate) fn git_directory_read_failure(path: &Path) -> alint_core::Violation {
    alint_core::Violation::new(alint_core::git_directory_read_refusal(path))
        .with_path(std::sync::Arc::<Path>::from(path))
        .as_analysis_failure(alint_core::AnalysisFailureKind::ResourceRead)
}

/// The informational-note message for a permitted out-of-root read.
pub(crate) fn out_of_root_note(path: &Path) -> String {
    format!(
        "reading out-of-root path {} - permitted by `allow_out_of_root`",
        path.display()
    )
}

/// The filesystem-aware containment check now lives in `alint-core` so the
/// relation read cache and this module share one implementation. Re-exported
/// here so the existing `crate::pathsafe::resolved_within_root` call sites keep
/// resolving unchanged.
pub(crate) use alint_core::resolved_within_root;

/// Like [`confine`] but filesystem-aware: a lexically-confined path can
/// still escape the root through an in-repo symlink, so after the lexical
/// check we resolve `root.join(path)` on disk and re-verify containment, as
/// [`resolved_within_root`] does ([`alint_core::resolve_named_read`]). Use
/// this for every config-derived *read*; the bare lexical [`confine`] is
/// symlink-blind. Same In/AllowedEscape/Denied verdict, honouring
/// `allow_escape` for the symlink escape exactly as for the lexical one, and
/// [`Confined::GitDirectory`] for a path in a Git directory, which no rule
/// reads: by name before anything else, through a symlink from the same one
/// resolution.
pub(crate) fn confine_read(path: &Path, root: &Path, allow_escape: bool) -> Confined {
    use alint_core::NamedRead;
    if alint_core::path_names_git_directory(path) {
        return Confined::GitDirectory;
    }
    match confine(path, allow_escape) {
        // Lexically in-tree — but the lexical check is symlink-blind, so
        // re-verify on the filesystem before trusting it for a read. The one
        // resolution also tells whether a symlink leads into a Git directory.
        Confined::In(p) => match alint_core::resolve_named_read(root, &p) {
            NamedRead::InRoot => Confined::In(p),
            NamedRead::GitDirectory => Confined::GitDirectory,
            NamedRead::OutsideRoot if allow_escape => Confined::AllowedEscape(p),
            NamedRead::OutsideRoot => Confined::Denied,
        },
        // A permitted lexical escape can still lead back into the root's Git
        // directory through a symlink (`../repo/g/config`, `g -> .git`).
        Confined::AllowedEscape(p) if alint_core::reaches_git_directory(root, &p) => {
            Confined::GitDirectory
        }
        // Lexical escape already decided (permitted or denied) — pass through.
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn confine_read_rejects_escape_through_an_in_repo_symlink() {
        // H1 regression: `normalize_confined` is symlink-blind, so a
        // lexically-confined path (`link/secret`) can read out of the tree
        // when `link` is an in-repo symlink. `confine_read` must catch it.
        use super::{Confined, confine_read, normalize_confined, resolved_within_root};
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"top secret").unwrap();
        symlink(outside.path(), root.path().join("link")).unwrap();
        std::fs::write(root.path().join("README.md"), b"hi").unwrap();
        symlink(
            root.path().join("README.md"),
            root.path().join("readme-link"),
        )
        .unwrap();

        let escaping = Path::new("link/secret");
        // Lexically confined (all-Normal components) yet escapes via symlink:
        assert!(normalize_confined(escaping).is_some());
        assert!(!resolved_within_root(
            &root.path().join("link/secret"),
            root.path()
        ));

        assert!(matches!(
            confine_read(escaping, root.path(), false),
            Confined::Denied
        ));
        assert!(matches!(
            confine_read(escaping, root.path(), true),
            Confined::AllowedEscape(_)
        ));
        // Genuine in-root paths (direct, via an in-root symlink, or not yet
        // existing) stay In — confinement must not break legitimate reads.
        for ok in ["README.md", "readme-link", "docs/NOT-YET.md"] {
            assert!(
                matches!(
                    confine_read(Path::new(ok), root.path(), false),
                    Confined::In(_)
                ),
                "{ok} should be In"
            );
        }
    }

    /// No rule reads a Git directory: a named path there, in any spelling, is
    /// a load error, and one a symlink leads into is refused at read, whatever
    /// `allow_out_of_root` says. `.github` and a gitignored `.env` still read.
    #[cfg(unix)]
    #[test]
    fn named_reads_never_reach_a_git_directory() {
        use super::{Confined, confine_read, reject_git_directory_read};
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join(".git")).unwrap();
        std::fs::write(root.path().join(".git/config"), "x").unwrap();
        std::fs::write(root.path().join(".env"), "x").unwrap();
        std::os::unix::fs::symlink(".git/config", root.path().join("cfg.ini")).unwrap();
        std::os::unix::fs::symlink(".git", root.path().join("g")).unwrap();
        for named in [
            ".git/config",
            "./.git/config",
            "a/.GIT/x",
            "git~1/config",
            ".git./x",
        ] {
            let error = reject_git_directory_read("r", "source.file", Path::new(named))
                .unwrap_err()
                .to_string();
            assert_eq!(
                error,
                format!(
                    "rule \"r\": `source.file` {named} is in a Git directory; rules never read \
                     there"
                )
            );
        }
        for path in [".git/config", "cfg.ini", "g/config", "../repo/.git/config"] {
            for allow in [false, true] {
                assert!(
                    matches!(
                        confine_read(Path::new(path), root.path(), allow),
                        Confined::GitDirectory
                    ),
                    "{path}"
                );
            }
        }
        for path in [".env", ".github/workflows/ci.yml", ".gitignore"] {
            assert!(reject_git_directory_read("r", "f", Path::new(path)).is_ok());
            assert!(
                matches!(
                    confine_read(Path::new(path), root.path(), false),
                    Confined::In(_)
                ),
                "{path}"
            );
        }
    }

    /// A named read resolves its path once, for both the root and the
    /// Git-directory check: a second resolution per read cost the S3 bench
    /// thousands of `stat` calls. A lexical escape is resolved only when the
    /// rule may read it, and a name in a Git directory not at all.
    #[test]
    fn a_named_read_resolves_its_path_once() {
        use super::{Confined, confine_read};
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("sub")).unwrap();
        std::fs::write(root.path().join("sub/a.toml"), "x").unwrap();
        for (path, allow, resolutions) in [
            ("sub/a.toml", false, 1),
            ("sub/missing.toml", false, 1),
            ("../x", false, 0),
            ("../x", true, 1),
            (".git/config", true, 0),
        ] {
            let before = alint_core::path_check_costs();
            let verdict = confine_read(Path::new(path), root.path(), allow);
            let after = alint_core::path_check_costs();
            assert_eq!(
                (
                    after.resolutions - before.resolutions,
                    after.symlink_probes - before.symlink_probes
                ),
                (resolutions, 0),
                "{path}"
            );
            let expected = match (path, allow) {
                (".git/config", _) => matches!(verdict, Confined::GitDirectory),
                ("../x", false) => matches!(verdict, Confined::Denied),
                ("../x", true) => matches!(verdict, Confined::AllowedEscape(_)),
                _ => matches!(verdict, Confined::In(_)),
            };
            assert!(expected, "{path}");
        }
    }

    /// The `allow_out_of_root` decision surface -- the actual allow/deny
    /// gate. In-tree is always `In`; an escaping path is `Denied` without
    /// the permission and `AllowedEscape` with it.
    #[test]
    fn confine_honors_allow_out_of_root() {
        use super::{Confined, confine};
        assert!(matches!(confine(Path::new("a/b"), false), Confined::In(_)));
        assert!(matches!(confine(Path::new("a/b"), true), Confined::In(_)));
        assert!(matches!(
            confine(Path::new("../../etc"), false),
            Confined::Denied
        ));
        assert!(matches!(
            confine(Path::new("/etc/passwd"), false),
            Confined::Denied
        ));
        assert!(matches!(
            confine(Path::new("../../etc"), true),
            Confined::AllowedEscape(_)
        ));
        assert!(matches!(
            confine(Path::new("/etc/passwd"), true),
            Confined::AllowedEscape(_)
        ));
    }
}
