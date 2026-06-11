//! Lexical path confinement — keep a config-author-controlled path
//! inside the repo root so a rule can never read or resolve a file
//! outside the tree (the untrusted-`extends:` threat the
//! `SPAWNING_RULE_KINDS` gate also defends against). Pure lexical, no
//! filesystem access. Design: `docs/design/v0.12/path-confinement.md`.

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
/// A `Some(_)` result is guaranteed root-relative: safe to
/// `root.join(..)` and to look up in the `FileIndex`.
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

    /// For every bounded component sequence: an absolute component
    /// always escapes (`None`), and a surviving path has a positive
    /// depth that never exceeds its `Normal` count — so the result can
    /// never reference a component the input didn't supply, and the
    /// `..` arithmetic never underflows/panics.
    #[kani::proof]
    #[kani::unwind(7)]
    fn confine_steps_is_sound() {
        let steps: [Step; 6] = kani::any();
        let result = confine_steps(steps);
        if steps.iter().any(|s| matches!(s, Step::AbsRoot)) {
            assert!(
                result.is_none(),
                "an absolute component must escape the root"
            );
        }
        if let Some(depth) = result {
            let normals = steps.iter().filter(|s| matches!(s, Step::Normal)).count();
            assert!(depth > 0, "a confined path is never the empty root");
            assert!(
                depth <= normals,
                "depth cannot exceed the Normal-component count"
            );
        }
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

    proptest! {
        /// The security invariant, as a property: whatever path the
        /// config author supplies, a `Some(_)` result is always
        /// root-confined — every component `Normal`, never empty — so
        /// `root.join(it)` can never escape the tree.
        #[test]
        fn confinement_invariant(s in r"[A-Za-z0-9_./\\-]{0,40}") {
            if let Some(out) = normalize_confined(Path::new(&s)) {
                prop_assert!(super::is_confined(&out));
            }
        }

        /// Normalisation is idempotent: a confined path is a fixed point,
        /// so re-normalising never changes it (a non-stable normaliser
        /// would make `equals`/index lookups order-dependent).
        #[test]
        fn normalize_confined_is_idempotent(s in r"[A-Za-z0-9_./\\-]{0,40}") {
            if let Some(out) = normalize_confined(Path::new(&s)) {
                prop_assert_eq!(normalize_confined(&out), Some(out.clone()));
            }
        }

        /// The real implementation agrees with the Kani-proven `model`:
        /// it returns `Some` with N components exactly when the model
        /// returns `Some(N)`. This ties the bounded proof to the actual
        /// `PathBuf`-building code.
        #[test]
        fn agrees_with_proven_model(s in r"[A-Za-z0-9_./\\-]{0,40}") {
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
