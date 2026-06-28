//! Path confinement — keep a config-author-controlled path inside the
//! repo root (the untrusted-`extends:` threat the `SPAWNING_RULE_KINDS`
//! gate also defends against). Two layers, because a lexical check alone
//! is not enough:
//!
//! - [`normalize_confined`] / [`confine`] are the *lexical* gate: pure, no
//!   filesystem access, and the subject of the Kani proof below. They
//!   reject absolute and `..`-escaping paths but are **symlink-blind** — a
//!   lexically-confined `link/x` still resolves outside the tree if `link`
//!   is an in-repo symlink pointing out of it.
//! - [`confine_read`] / [`resolved_within_root`] add the *filesystem*
//!   gate: after joining under the root they follow symlinks and re-verify
//!   containment. Every config-derived **read** must go through these, not
//!   the bare lexical pass (the walker's `FileIndex` is separately
//!   symlink-pruned, so index *lookups* are already safe).
//!
//! Design: `docs/design/v0.12/path-confinement.md`.

use std::path::{Component, Path, PathBuf};

/// Normalise `p` lexically (collapsing `.` and `a/../b`) and return it
/// **only if it stays within the repo root**.
///
/// Returns `None` when the path escapes the root:
/// - an absolute component (`RootDir` / Windows `Prefix`) — because
///   `root.join(absolute)` discards `root`, so reading it would touch
///   an arbitrary host path;
/// - a `..` that cannot pop a real component (caught *during* the
///   walk, so `../../escape` and `a/../../x` are rejected, not merely
///   inspected after the fact);
/// - a result that collapses to empty (`.`, `a/..`) — the root itself
///   is never a valid edge / target / reference.
///
/// A `Some(_)` result is guaranteed root-relative *lexically*: safe to
/// look up in the (symlink-pruned) `FileIndex`. For a direct filesystem
/// **read** it is **not** sufficient — `root.join(result)` still follows
/// any in-repo symlink out of the tree — so reads must go through
/// [`confine_read`] / [`resolved_within_root`], never this result alone.
pub(crate) fn normalize_confined(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                // A `..` that can't pop a real component escapes root.
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(c) => out.push(c),
            // Absolute (Unix root or Windows prefix) escapes by
            // definition — never let it reach `root.join`.
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        return None;
    }
    debug_assert!(
        is_confined(&out),
        "normalize_confined produced a non-confined path: {}",
        out.display()
    );
    Some(out)
}

/// The confinement invariant every `Some(_)` from [`normalize_confined`]
/// upholds: a non-empty, purely root-relative path — every component is
/// `Normal` (no `..`, no `.`, no absolute `RootDir`/`Prefix`). Checked at
/// runtime by the `debug_assert` above and exhaustively-for-bounded-inputs
/// by the `confinement_invariant` property test. This is the security
/// contract: a value satisfying it is safe to `root.join(..)`.
fn is_confined(p: &Path) -> bool {
    !p.as_os_str().is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))
}

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
        "reading out-of-root path {} — permitted by `allow_out_of_root`",
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

/// A bounded, verifiable model of the confinement policy — the only
/// distinction the walk makes between path components, decoupled from
/// `std::path::Component` (which carries an unbounded `OsStr`) so the
/// policy is provable over fixed-size inputs. `normalize_confined` is the
/// real, `PathBuf`-building implementation this abstracts; the
/// `agrees_with_proven_model` property checks they never disagree.
#[cfg(any(test, kani))]
mod model {
    use std::path::Component;

    #[cfg_attr(kani, derive(kani::Arbitrary))]
    #[derive(Clone, Copy)]
    pub(super) enum Step {
        Normal,
        Parent,
        Cur,
        AbsRoot,
    }

    pub(super) fn step_of(c: &Component) -> Step {
        match c {
            Component::Normal(_) => Step::Normal,
            Component::CurDir => Step::Cur,
            Component::ParentDir => Step::Parent,
            Component::RootDir | Component::Prefix(_) => Step::AbsRoot,
        }
    }

    /// Fold component steps into the surviving root-relative depth, or
    /// `None` on any escape (an absolute component, or a `..` that
    /// underflows the root). `Some(depth)` requires `depth > 0` — the
    /// root itself is never a valid confined path.
    pub(super) fn confine_steps(steps: impl IntoIterator<Item = Step>) -> Option<usize> {
        let mut depth = 0usize;
        for s in steps {
            match s {
                Step::Normal => depth += 1,
                Step::Cur => {}
                Step::Parent => depth = depth.checked_sub(1)?,
                Step::AbsRoot => return None,
            }
        }
        (depth > 0).then_some(depth)
    }
}

/// Kani bounded proof of the confinement policy. Run with `cargo kani`
/// (not part of standard preflight — Kani is a separate, heavier
/// toolchain). See `docs/design/formal-methods.md`.
#[cfg(kani)]
mod kani_proofs {
    use super::model::{Step, confine_steps};

    /// For every bounded component sequence, `confine_steps` implements
    /// the confinement policy *exactly*: it escapes (`None`) on any
    /// absolute component or on a `..` that underflows the root, and
    /// otherwise yields the surviving depth `#Normal - #Parent` (with the
    /// empty root, depth 0, rejected). The proof checks this against an
    /// **independent counting formulation** of the spec — a different
    /// algorithm shape from the early-return fold under test — so it
    /// catches a real escape bug (e.g. a `..` that fails to pop), not just
    /// the weaker side-conditions (`AbsRoot => None`, `depth <= #Normal`)
    /// an earlier version proved. It also proves the `..` arithmetic never
    /// underflows or panics.
    #[kani::proof]
    #[kani::unwind(7)]
    fn confine_steps_is_sound() {
        let steps: [Step; 6] = kani::any();
        let result = confine_steps(steps);

        // Independent spec: walk the whole sequence counting a running
        // balance; the policy escapes iff any absolute component is
        // present or the balance ever goes negative (a `..` underflowing
        // the root). When no escape occurs, the depth is the final
        // balance, and depth 0 (the bare root) is not a valid path.
        let has_abs = steps.iter().any(|s| matches!(s, Step::AbsRoot));
        let mut balance: i64 = 0;
        let mut underflowed = false;
        for s in &steps {
            match s {
                Step::Normal => balance += 1,
                Step::Parent => {
                    balance -= 1;
                    if balance < 0 {
                        underflowed = true;
                    }
                }
                Step::Cur | Step::AbsRoot => {}
            }
        }
        let expected = if has_abs || underflowed || balance <= 0 {
            None
        } else {
            Some(balance as usize)
        };

        assert!(
            result == expected,
            "confine_steps must implement the confinement policy exactly"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_confined;
    use proptest::prelude::*;
    use std::path::{Path, PathBuf};

    fn confined(s: &str) -> Option<PathBuf> {
        normalize_confined(Path::new(s))
    }

    #[test]
    fn in_tree_paths_normalise_and_pass() {
        assert_eq!(confined("a/b.rs"), Some(PathBuf::from("a/b.rs")));
        assert_eq!(confined("./a/./b"), Some(PathBuf::from("a/b")));
        assert_eq!(confined("a/x/../b"), Some(PathBuf::from("a/b")));
        // pops back to root then descends — stays in-tree.
        assert_eq!(confined("a/../b"), Some(PathBuf::from("b")));
    }

    #[cfg(unix)]
    #[test]
    fn confine_read_rejects_escape_through_an_in_repo_symlink() {
        // H1 regression: `normalize_confined` is symlink-blind, so a
        // lexically-confined path (`link/secret`) can read out of the tree
        // when `link` is an in-repo symlink. `confine_read` must catch it.
        use super::{Confined, confine_read, resolved_within_root};
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

    #[test]
    fn absolute_paths_are_rejected() {
        // root.join(absolute) would discard root — the read-oracle.
        assert_eq!(confined("/etc/passwd"), None);
        assert_eq!(confined("/tmp/secret.txt"), None);
    }

    #[test]
    fn root_escaping_dotdot_is_rejected_including_cancellation() {
        assert_eq!(confined("../x"), None);
        // The double-dot-cancellation escape a first-component check
        // misses: `../../escape` must NOT collapse to in-tree `escape`.
        assert_eq!(confined("../../escape"), None);
        assert_eq!(confined("a/../../x"), None);
        assert_eq!(confined("a/b/../../../c"), None);
    }

    #[test]
    fn empty_or_root_collapse_is_rejected() {
        assert_eq!(confined(""), None);
        assert_eq!(confined("."), None);
        assert_eq!(confined("a/.."), None); // collapses to the root itself
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

    /// A path strategy that actually exercises the confinement logic: each
    /// component is a short name, `..`, or `.`, joined by `/` and
    /// optionally absolute. A flat character regex produces `..` in well
    /// under 0.01% of cases, so it almost never hits the `..`-underflow
    /// rejection (the headline cancellation attack); this makes escapes,
    /// cancellation, and root underflow common, so the properties stress
    /// the security-critical rejection paths, not just the in-tree case.
    fn confinement_paths() -> impl Strategy<Value = String> {
        let component = prop_oneof![
            3 => "[a-z]{1,4}",
            2 => Just("..".to_string()),
            1 => Just(".".to_string()),
        ];
        (any::<bool>(), prop::collection::vec(component, 0..8)).prop_map(|(absolute, comps)| {
            let body = comps.join("/");
            if absolute { format!("/{body}") } else { body }
        })
    }

    proptest! {
        /// The security invariant, as a property: whatever path the
        /// config author supplies, a `Some(_)` result is always
        /// root-confined — every component `Normal`, never empty — so
        /// `root.join(it)` can never escape the tree.
        #[test]
        fn confinement_invariant(s in confinement_paths()) {
            if let Some(out) = normalize_confined(Path::new(&s)) {
                prop_assert!(super::is_confined(&out));
            }
        }

        /// Normalisation is idempotent: a confined path is a fixed point,
        /// so re-normalising never changes it (a non-stable normaliser
        /// would make `equals`/index lookups order-dependent).
        #[test]
        fn normalize_confined_is_idempotent(s in confinement_paths()) {
            if let Some(out) = normalize_confined(Path::new(&s)) {
                prop_assert_eq!(normalize_confined(&out), Some(out.clone()));
            }
        }

        /// The real implementation agrees with the Kani-proven `model`:
        /// it returns `Some` with N components exactly when the model
        /// returns `Some(N)`. This ties the bounded proof to the actual
        /// `PathBuf`-building code.
        #[test]
        fn agrees_with_proven_model(s in confinement_paths()) {
            let p = Path::new(&s);
            let model = super::model::confine_steps(p.components().map(|c| super::model::step_of(&c)));
            let real = normalize_confined(p).map(|o| o.components().count());
            prop_assert_eq!(real, model);
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefix_and_unc_paths_are_rejected() {
        // On Windows a drive-letter `Prefix` or a UNC `\\server\share`
        // is absolute → escapes the root, same as a Unix `/etc`. (On
        // Unix these parse as ordinary `Normal` components, so the test
        // is Windows-only.)
        assert_eq!(confined(r"C:\Windows\System32"), None);
        assert_eq!(confined(r"\\server\share\x"), None);
    }
}
