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
    /// In-tree (lexically normalised) — read as today.
    In(PathBuf),
    /// Escapes the root, but the rule is permitted to read it. The
    /// caller reads `root.join(path)` (absolute → itself; `../../x` →
    /// up) and emits an informational note via [`out_of_root_note`].
    AllowedEscape(PathBuf),
    /// Escapes the root and the rule is not permitted — the caller
    /// emits an "escapes the repo root" violation and does not read.
    Denied,
}

/// Confine a config-derived read path, honouring an
/// `allow_out_of_root` permission. `allow_escape` is the per-rule
/// flag the loader resolved from the top-level policy; it is `false`
/// for every rule unless the user's own top-level config opted the
/// rule (or its kind) in.
pub(crate) fn confine(path: &Path, allow_escape: bool) -> Confined {
    match normalize_confined(path) {
        Some(p) => Confined::In(p),
        None if allow_escape => Confined::AllowedEscape(path.to_path_buf()),
        None => Confined::Denied,
    }
}

/// The informational-note message for a permitted out-of-root read.
pub(crate) fn out_of_root_note(path: &Path) -> String {
    format!(
        "reading out-of-root path {} - permitted by `allow_out_of_root`",
        path.display()
    )
}

/// True iff `candidate` (a `root`-joined, lexically-confined path) really
/// resolves *inside* `root` once symlinks are followed. [`normalize_confined`]
/// is purely lexical and therefore symlink-blind, so an in-repo symlink
/// (`link -> /etc`) makes a lexically-in path (`link/secret`) escape the
/// tree at read time — defeating confinement and the `allow_out_of_root`
/// gate. We canonicalize the deepest *existing* ancestor (the final
/// component may legitimately not exist, e.g. an existence check, and a
/// non-existent component can't be a symlink), re-attach the non-existent
/// tail, and confirm the result stays under the real root. Fails closed if
/// the root itself can't be resolved.
pub(crate) fn resolved_within_root(candidate: &Path, root: &Path) -> bool {
    let Ok(root_real) = root.canonicalize() else {
        return false;
    };
    let mut tail = PathBuf::new();
    let mut cur = candidate.to_path_buf();
    loop {
        if let Ok(real) = cur.canonicalize() {
            let full = if tail.as_os_str().is_empty() {
                real
            } else {
                real.join(&tail)
            };
            return full.starts_with(&root_real);
        }
        // `cur` doesn't resolve yet; fold its last component into `tail`
        // and retry against the parent. A component that doesn't exist
        // can't be a symlink, so it cannot introduce an escape.
        let Some(name) = cur.file_name().map(std::ffi::OsString::from) else {
            return false;
        };
        tail = Path::new(&name).join(&tail);
        if !cur.pop() {
            return false;
        }
    }
}

/// Like [`confine`] but filesystem-aware: a lexically-confined path can
/// still escape the root through an in-repo symlink, so after the lexical
/// check we resolve `root.join(path)` on disk and re-verify containment via
/// [`resolved_within_root`]. Use this for every config-derived *read*; the
/// bare lexical [`confine`] is symlink-blind. Same In/AllowedEscape/Denied
/// verdict, honouring `allow_escape` for the symlink escape exactly as for
/// the lexical one.
pub(crate) fn confine_read(path: &Path, root: &Path, allow_escape: bool) -> Confined {
    match confine(path, allow_escape) {
        // Lexically in-tree — but the lexical check is symlink-blind, so
        // re-verify on the filesystem before trusting it for a read.
        Confined::In(p) => {
            if resolved_within_root(&root.join(&p), root) {
                Confined::In(p)
            } else if allow_escape {
                Confined::AllowedEscape(p)
            } else {
                Confined::Denied
            }
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
