//! Path confinement — lexical primitives that keep a
//! config-author-controlled path inside the repo root (the untrusted
//! `extends:` threat the `SPAWNING_RULE_KINDS` gate also defends
//! against).
//!
//! [`normalize_confined`] is the *lexical* gate: pure, no filesystem
//! access, and the subject of the Kani proof below. It rejects absolute
//! and `..`-escaping paths but is **symlink-blind** — a
//! lexically-confined `link/x` still resolves outside the tree if `link`
//! is an in-repo symlink pointing out of it. The filesystem-aware half,
//! [`resolved_within_root`], lives in this module; `alint-rules` layers its
//! `allow_out_of_root` policy on top as `confine_read`.
//!
//! [`derive_target`] applies a `from`→`to` capture template to a path
//! and confines the result — the `file_graph` `derive_target` edge.
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
/// [`resolved_within_root`] (or `alint-rules`' `confine_read`), never this
/// result alone.
pub fn normalize_confined(p: &Path) -> Option<PathBuf> {
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

/// True iff `candidate` (a `root`-joined, lexically-confined path) really
/// resolves *inside* `root` once symlinks are followed.
///
/// [`normalize_confined`] is purely lexical and therefore symlink-blind, so an
/// in-repo symlink (`link -> /etc`) makes a lexically-in path (`link/secret`)
/// escape the tree at read time. We canonicalize the deepest *existing*
/// ancestor (the final component may legitimately not exist, e.g. an existence
/// check, and a non-existent component cannot be a symlink), re-attach the
/// non-existent tail, and confirm the result stays under the real root. Fails
/// closed if the root itself cannot be resolved.
///
/// This lives in `alint-core` because the relation layer's read cache and
/// `alint-rules`' `confine_read` must share one implementation; a second copy
/// would let the two read paths diverge.
#[must_use]
pub fn resolved_within_root(candidate: &Path, root: &Path) -> bool {
    let Ok(root_real) = root.canonicalize() else {
        return false;
    };
    resolve_existing_prefix(candidate).is_some_and(|real| real.starts_with(&root_real))
}

/// `path` with its existing components resolved as the OS resolves them and
/// the rest re-attached, or `None` when its root (or, for a relative path, the
/// working directory) does not resolve. The walk runs forward: an existing
/// component resolves against the real location so far (through a symlink,
/// to where it leads), and a `..` steps up from that real location. A
/// component that does not exist cannot be a symlink, so it and every one
/// after it wait in a pending tail, where a `..` cancels the last of them
/// (`new/../x`, as `create_dir_all` then a write resolve it); once the tail
/// empties, the next component may exist again (`new/../link/x`). Two names
/// for one location (through an in-repo symlinked directory) resolve alike
/// before the location exists. A symlink that leads nowhere counts as missing,
/// though `create_dir_all` can make the place it leads to on the way
/// (`b/../a/x` with `a -> b`); the dry run then predicts the wrong place.
pub(crate) fn resolve_existing_prefix(path: &Path) -> Option<PathBuf> {
    use std::path::Component;
    // A path that exists resolves in one call.
    if let Ok(real) = path.canonicalize() {
        return Some(real);
    }
    // The deepest existing ancestor before the first `..` also resolves in
    // one call, so the walk starts there rather than at the root: a missing
    // file in an existing directory costs two calls, not one per component.
    let components: Vec<Component<'_>> = path.components().collect();
    let base = components
        .iter()
        .take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
        .count();
    let first_up = components
        .iter()
        .position(|c| matches!(c, Component::ParentDir))
        .unwrap_or(components.len());
    // The whole path, when it has no `..`, is known not to resolve.
    let mut end = if first_up == components.len() {
        first_up.checked_sub(1)?
    } else {
        first_up
    };
    let mut real = loop {
        let prefix: PathBuf = components[..end].iter().collect();
        let resolved = if prefix.as_os_str().is_empty() {
            Path::new(".").canonicalize()
        } else {
            prefix.canonicalize()
        };
        if let Ok(real) = resolved {
            break real;
        }
        if end <= base {
            return None;
        }
        end -= 1;
    };
    let mut pending: Vec<&std::ffi::OsStr> = Vec::new();
    let mut rest = &components[end..];
    // Unless the walk stopped at the first `..`, the next component is the
    // one a longer prefix did not resolve through: it is missing.
    if let Some((Component::Normal(name), after)) = rest.split_first() {
        pending.push(name);
        rest = after;
    }
    for component in rest {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if pending.pop().is_none() {
                    // `real` is canonical, so its parent is where `..` leads;
                    // at the filesystem root it stays put, as `/..` does.
                    real.pop();
                }
            }
            Component::Normal(name) => {
                if pending.is_empty()
                    && let Ok(next) = real.join(name).canonicalize()
                {
                    real = next;
                    continue;
                }
                pending.push(name);
            }
        }
    }
    Some(pending.into_iter().fold(real, |path, name| path.join(name)))
}

/// Where a create of `path` lands once the OS resolves it, for the checks a
/// creating fixer makes before it writes (already there, a symlink). A `..`
/// after a directory the create makes steps back out of it, so the path as
/// written names nothing until that directory exists, while the place it
/// leads to can hold anything. A path without `..` is its own landing, as is
/// one whose directory does not resolve.
#[doc(hidden)]
#[must_use]
pub fn create_landing(path: &Path) -> PathBuf {
    if !path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return path.to_path_buf();
    }
    path.parent()
        .zip(path.file_name())
        .and_then(|(parent, name)| Some(resolve_existing_prefix(parent)?.join(name)))
        .unwrap_or_else(|| path.to_path_buf())
}

/// Derive a target path from `path` by matching it against the `from`
/// regex and expanding its captures into the `to` replacement template,
/// then lexically confining the result to the repo root (via
/// [`normalize_confined`]).
///
/// Returns `None` in two cases the caller may need to tell apart: `path`
/// did not match `from` (no derived edge at all), or it matched but the
/// derived path escapes the repo root (an out-of-repo target that must
/// never be read). The `file_graph` `derive_target` edge (source path →
/// generated file) is the caller.
pub fn derive_target(from: &regex::Regex, to: &str, path: &str) -> Option<PathBuf> {
    let caps = from.captures(path)?;
    let mut derived = String::new();
    caps.expand(to, &mut derived);
    normalize_confined(Path::new(&derived))
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

    #[test]
    fn a_dotdot_after_a_missing_directory_cancels_it() {
        use super::{resolve_existing_prefix, resolved_within_root};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        let real = root.canonicalize().unwrap();
        let resolve = |rel: &str| resolve_existing_prefix(&root.join(rel));
        // `create_dir_all` makes `new`, so `new/..` is the root.
        assert_eq!(resolve("new/../x.txt"), Some(real.join("x.txt")));
        assert_eq!(resolve("a/new/../../x.txt"), Some(real.join("x.txt")));
        assert_eq!(resolve("new/../sub/y"), Some(real.join("sub/y")));
        assert!(resolved_within_root(&root.join("new/../x.txt"), &root));
        // A `..` left over steps up from the root's real location.
        assert_eq!(
            resolve("new/../../x.txt"),
            Some(real.parent().unwrap().join("x.txt"))
        );
        assert!(!resolved_within_root(&root.join("new/../../x.txt"), &root));
        assert!(!resolved_within_root(
            &root.join("a/b/../../../etc/passwd"),
            &root
        ));
    }

    #[test]
    #[cfg(unix)]
    fn a_leftover_dotdot_steps_up_from_where_a_symlink_lands() {
        use super::resolve_existing_prefix;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("elsewhere/deep")).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere/deep"), root.join("link")).unwrap();
        // `link/new/../..` is `elsewhere`, the parent of where `link` lands.
        assert_eq!(
            resolve_existing_prefix(&root.join("link/new/../../x")),
            Some(root.join("elsewhere/x"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_symlink_after_a_cancelled_missing_directory_still_resolves() {
        use super::{resolve_existing_prefix, resolved_within_root};
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::fs::create_dir_all(root.join("elsewhere/deep")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("alias")).unwrap();
        std::os::unix::fs::symlink(root.join("elsewhere/deep"), root.join("link")).unwrap();
        // Once `new/..` cancels, `alias` exists and leads to `real`.
        assert_eq!(
            resolve_existing_prefix(&root.join("new/../alias/x.txt")),
            Some(root.join("real/x.txt"))
        );
        // And `link/..` steps up from where `link` lands, not back to the root.
        assert_eq!(
            resolve_existing_prefix(&root.join("new/../link/../x.txt")),
            Some(root.join("elsewhere/x.txt"))
        );
        // A symlink out of the root after a cancelled directory is outside it.
        std::os::unix::fs::symlink("/", root.join("real/out")).unwrap();
        assert!(!resolved_within_root(
            &root.join("real/new/../out/etc/passwd"),
            &root.join("real")
        ));
    }

    #[test]
    #[cfg(unix)]
    fn resolve_existing_prefix_lands_where_the_os_creates() {
        // Every path of up to three components over a small tree (directories,
        // a file, a symlinked directory, a missing name, a dangling symlink,
        // `..`), then a file name: wherever `create_dir_all` and a write
        // succeed, the file lands where the resolver said it would.
        use super::resolve_existing_prefix;
        const NAMES: [&str; 7] = ["d", "e", "f", "l", "m", "dl", ".."];
        let mut paths: Vec<Vec<&str>> = vec![Vec::new()];
        let mut frontier = paths.clone();
        for _ in 0..3 {
            frontier = frontier
                .iter()
                .flat_map(|path| {
                    NAMES.iter().map(move |name| {
                        let mut longer = path.clone();
                        longer.push(*name);
                        longer
                    })
                })
                .collect();
            paths.extend(frontier.iter().cloned());
        }
        let mut created = 0;
        for components in &paths {
            let tmp = tempfile::tempdir().unwrap();
            // Deep enough that three `..` stay inside the temporary directory.
            let root = tmp.path().canonicalize().unwrap().join("a/b/c/root");
            std::fs::create_dir_all(root.join("d/e")).unwrap();
            std::fs::write(root.join("f"), "").unwrap();
            std::os::unix::fs::symlink("d/e", root.join("l")).unwrap();
            // A dangling symlink to a place no path here creates.
            std::os::unix::fs::symlink("nowhere/zz", root.join("dl")).unwrap();
            let path = components
                .iter()
                .fold(root.clone(), |path, name| path.join(name))
                .join("x.txt");
            let predicted = resolve_existing_prefix(&path);
            let written = path
                .parent()
                .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
                && std::fs::write(&path, "").is_ok();
            if written {
                assert_eq!(
                    predicted,
                    path.canonicalize().ok(),
                    "{}",
                    components.join("/")
                );
                created += 1;
            }
        }
        assert!(created > 100, "only {created} paths could be created");
    }

    #[test]
    #[cfg(unix)]
    fn create_landing_is_where_a_dot_dot_after_a_new_directory_leads() {
        use super::create_landing;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::write(root.join("KEEP.md"), "keep").unwrap();
        // `new` does not exist, so the path as written names nothing.
        assert!(!root.join("new/../KEEP.md").exists());
        assert_eq!(
            create_landing(&root.join("new/../KEEP.md")),
            root.join("KEEP.md")
        );
        // Without `..`, a path is its own landing.
        assert_eq!(
            create_landing(&root.join("new/x.md")),
            root.join("new/x.md")
        );
    }
}
