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
    count_check_cost(|costs| costs.resolutions += 1);
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
/// before the location exists. A symlink that leads nowhere is missing too,
/// unless it leads into a directory `create_dir_all` made earlier in the same
/// path (`b/../a/x` with `a -> b` lands in `b`).
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
    let real = loop {
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
    let mut rest = &components[end..];
    // A symlink met after a `..` can lead into a directory made before it,
    // so only then are the made directories worth keeping.
    let mut landing = Landing {
        real,
        pending: Vec::new(),
        made: rest
            .iter()
            .any(|c| matches!(c, Component::ParentDir))
            .then(Vec::new),
    };
    // Unless the walk stopped at the first `..`, the next component is the
    // one a longer prefix did not resolve through: it is missing.
    if let Some((Component::Normal(name), after)) = rest.split_first() {
        landing.make(name);
        rest = after;
    }
    for component in rest {
        landing.step(*component);
    }
    Some(landing.position())
}

/// A create's walk along its path, as `create_dir_all` then a write take it:
/// the real directory reached, the names below it the create makes, and, when
/// a symlink could still lead into them, every directory made so far.
struct Landing {
    real: PathBuf,
    pending: Vec<std::ffi::OsString>,
    made: Option<Vec<PathBuf>>,
}

/// How many symlinks a lookup follows before giving up, as the OS does.
const MAX_SYMLINKS: usize = 40;

impl Landing {
    fn step(&mut self, component: Component<'_>) {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if self.pending.pop().is_none() {
                    // `real` is canonical, so its parent is where `..` leads;
                    // at the filesystem root it stays put, as `/..` does.
                    self.real.pop();
                }
            }
            Component::Normal(name) => {
                if self.pending.is_empty() {
                    let next = self.real.join(name);
                    if let Ok(real) = next.canonicalize() {
                        self.real = real;
                        return;
                    }
                    if let Some((real, pending)) = self.through_dangling(&next, 0) {
                        self.real = real;
                        self.pending = pending;
                        return;
                    }
                }
                self.make(name);
            }
        }
    }

    fn make(&mut self, name: &std::ffi::OsStr) {
        self.pending.push(name.to_owned());
        if self.made.is_some() {
            let position = self.position();
            if let Some(made) = &mut self.made {
                made.push(position);
            }
        }
    }

    fn position(&self) -> PathBuf {
        self.pending
            .iter()
            .fold(self.real.clone(), |path, name| path.join(name))
    }

    /// Where `link`, a symlink that does not resolve, leads now that the
    /// directories this create made exist: a real directory and made names
    /// below it. `None` when it is not a symlink or leads anywhere else.
    fn through_dangling(
        &self,
        link: &Path,
        followed: usize,
    ) -> Option<(PathBuf, Vec<std::ffi::OsString>)> {
        let made = self.made.as_deref().filter(|made| !made.is_empty())?;
        if followed >= MAX_SYMLINKS {
            return None;
        }
        let target = std::fs::read_link(link).ok()?;
        // `link` sits in a canonical directory, which a relative target
        // starts from.
        let mut real = if target.has_root() {
            PathBuf::new()
        } else {
            link.parent()?.to_path_buf()
        };
        let mut pending: Vec<std::ffi::OsString> = Vec::new();
        for component in target.components() {
            match component {
                Component::Prefix(_) | Component::RootDir => real.push(component.as_os_str()),
                Component::CurDir => {}
                Component::ParentDir => {
                    if pending.pop().is_none() {
                        real.pop();
                    }
                }
                Component::Normal(name) => {
                    if pending.is_empty() {
                        let next = real.join(name);
                        if let Ok(resolved) = next.canonicalize() {
                            real = resolved;
                            continue;
                        }
                        if let Some(inner) = self.through_dangling(&next, followed + 1) {
                            (real, pending) = inner;
                            continue;
                        }
                    }
                    pending.push(name.to_owned());
                    let reached = pending.iter().fold(real.clone(), |path, n| path.join(n));
                    if !made.contains(&reached) {
                        return None;
                    }
                }
            }
        }
        (!pending.is_empty()).then_some((real, pending))
    }
}

/// Whether a file system could open the entry `name` as `.git`, a Git
/// directory. Beyond `.git` in any case, Git's own checks name three more
/// spellings: Windows drops trailing dots and spaces (`.git.`, `.git `), reads
/// `name:stream` as a stream of `name` (`.git::$INDEX_ALLOCATION`) and `\` as
/// a separator (`a\.git`), and its 8.3 short name for `.git` is `GIT~1` (or
/// `GIT~2`, ..., when collisions occur), which drops them too (`git~1.`);
/// macOS's HFS+ ignores some invisible code points (`.g\u{200c}it`).
///
/// A fix never writes or reads a path with such a component: a file it puts
/// there (`.git/config`, `.git/commondir`, a `.git` file) is configuration
/// Git runs commands from. No rule reads one either, since it can hold
/// credentials (an `http.*.extraheader`): a rule that names one is refused at
/// load, and a named read that reaches one through a symlink is refused when
/// it happens ([`git_directory_read_refusal`]).
#[must_use]
pub fn names_git_directory(name: &str) -> bool {
    opens_as(name, |file| {
        file.eq_ignore_ascii_case(".git") || short_name_of(file, ".git")
    })
}

/// Whether a file system could open the entry `name` as a name `matches`
/// accepts, reading it as Git's `is_ntfs_dotgit` and `is_hfs_dotgit` do (see
/// [`names_git_directory`]): without the code points HFS+ ignores, each
/// `\`-separated piece without its `:stream` suffix and trailing dots and
/// spaces.
fn opens_as(name: &str, matches: impl Fn(&str) -> bool) -> bool {
    const HFS_IGNORED: [std::ops::RangeInclusive<char>; 4] = [
        '\u{200c}'..='\u{200f}',
        '\u{202a}'..='\u{202e}',
        '\u{206a}'..='\u{206f}',
        '\u{feff}'..='\u{feff}',
    ];
    let visible: String = name
        .chars()
        .filter(|c| !HFS_IGNORED.iter().any(|range| range.contains(c)))
        .collect();
    visible.split('\\').any(|piece| {
        let file = piece
            .split(':')
            .next()
            .unwrap_or_default()
            .trim_end_matches(['.', ' ']);
        matches(file)
    })
}

/// Whether a component of `path` names a Git directory
/// ([`names_git_directory`]).
#[must_use]
pub fn path_names_git_directory(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => names_git_directory(&name.to_string_lossy()),
        _ => false,
    })
}

/// Whether `path`, relative to `root`, reaches a Git directory: a component
/// of it names one ([`path_names_git_directory`]), or the place it leads to
/// once the OS follows the symlinks in it (an in-repo `g -> .git`) is inside
/// one below `root`.
///
/// Only a path with a symlink on the way pays for resolving it; any other
/// costs one `lstat` per component, until one does not exist.
#[must_use]
pub fn reaches_git_directory(root: &Path, path: &Path) -> bool {
    reaches_git_directory_walked(root, path, None)
}

/// [`reaches_git_directory`], asking `index`, the walk of `root`, first: a file
/// it reached without a symlink is in a Git directory only by name, which
/// needs no file system call to tell.
pub(crate) fn reaches_git_directory_walked(
    root: &Path,
    path: &Path,
    index: Option<&crate::walker::FileIndex>,
) -> bool {
    if path_names_git_directory(path) {
        return true;
    }
    if follows_no_symlink(root, path, index) {
        return false;
    }
    count_check_cost(|costs| costs.resolutions += 1);
    let Ok(root_real) = root.canonicalize() else {
        return false;
    };
    resolve_existing_prefix(&root.join(path))
        .is_some_and(|real| enters_git_directory(&root_real, &real))
}

/// Whether the OS reaches `path`, relative to `root`, without following a
/// symlink below `root`, so that it lands at `path` itself below the real
/// root: `index`, the walk of `root`, indexed it that way
/// ([`FileIndex::walked_without_symlink`](crate::walker::FileIndex)), or no
/// component of it is a symlink, asked from the root down with one `lstat`
/// each until one does not exist (nothing below it does either). A path with
/// a component other than a name (`..`, an absolute one) gets no answer.
pub(crate) fn follows_no_symlink(
    root: &Path,
    path: &Path,
    index: Option<&crate::walker::FileIndex>,
) -> bool {
    if !path
        .components()
        .all(|component| matches!(component, Component::Normal(_)))
    {
        return false;
    }
    if index.is_some_and(|index| index.walked_without_symlink(path)) {
        return true;
    }
    count_check_cost(|costs| costs.symlink_probes += 1);
    let mut at = root.to_path_buf();
    for component in path.components() {
        at.push(component);
        match std::fs::symlink_metadata(&at) {
            Ok(metadata) if !metadata.file_type().is_symlink() => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return true,
            _ => return false,
        }
    }
    true
}

/// What the path checks in this module cost the calling thread so far: how
/// many paths they resolved through their symlinks (a `canonicalize` of the
/// root and of the path, one call per component of each), and how many they
/// probed for a symlink below the root (`follows_no_symlink`). Test and
/// diagnostic only: a check that resolves every path it is asked about makes
/// thousands of calls a run, which only a count shows.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PathCheckCosts {
    /// Paths resolved through their symlinks.
    pub resolutions: usize,
    /// Paths probed for a symlink, one `lstat` per component.
    pub symlink_probes: usize,
}

thread_local! {
    static CHECK_COSTS: std::cell::Cell<PathCheckCosts> =
        const { std::cell::Cell::new(PathCheckCosts { resolutions: 0, symlink_probes: 0 }) };
}

fn count_check_cost(add: impl FnOnce(&mut PathCheckCosts)) {
    CHECK_COSTS.with(|cell| {
        let mut costs = cell.get();
        add(&mut costs);
        cell.set(costs);
    });
}

/// [`PathCheckCosts`] for the calling thread.
#[doc(hidden)]
#[must_use]
pub fn path_check_costs() -> PathCheckCosts {
    CHECK_COSTS.with(std::cell::Cell::get)
}

/// Where a rule's read of `path`, relative to `root`, lands once the OS
/// follows the symlinks in it ([`resolve_named_read`]).
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedRead {
    /// Inside the root, and outside any Git directory there.
    InRoot,
    /// Inside a Git directory below the root, by name or through a symlink:
    /// no rule reads there ([`git_directory_read_refusal`]).
    GitDirectory,
    /// Outside the root, or the root does not resolve.
    OutsideRoot,
}

/// Resolve a path a rule reads by name once, for both checks a named read
/// makes: that it stays in the root ([`resolved_within_root`]) and out of a
/// Git directory there ([`reaches_git_directory`]). One resolution answers
/// both, so the Git-directory check costs a named read nothing more.
#[doc(hidden)]
#[must_use]
pub fn resolve_named_read(root: &Path, path: &Path) -> NamedRead {
    if path_names_git_directory(path) {
        return NamedRead::GitDirectory;
    }
    count_check_cost(|costs| costs.resolutions += 1);
    let Ok(root_real) = root.canonicalize() else {
        return NamedRead::OutsideRoot;
    };
    match resolve_existing_prefix(&root.join(path)) {
        Some(real) if enters_git_directory(&root_real, &real) => NamedRead::GitDirectory,
        Some(real) if real.starts_with(&root_real) => NamedRead::InRoot,
        _ => NamedRead::OutsideRoot,
    }
}

/// Whether `real`, a resolved path, enters a Git directory beyond the longest
/// common ancestor prefix shared with `root_real`. Covers both `.git` inside
/// `root_real` and a Git directory reached outside `root_real` (for instance a
/// parent repo's `../.git` or a linked worktree's common `.git`), without
/// false-positiving if `root_real` itself sits under a host directory whose
/// ancestor happens to be named `.git`.
fn enters_git_directory(root_real: &Path, real: &Path) -> bool {
    let mut root_comps = root_real.components().peekable();
    let mut real_comps = real.components().peekable();
    while root_comps.peek().is_some() && root_comps.peek() == real_comps.peek() {
        root_comps.next();
        real_comps.next();
    }
    real_comps.any(|component| match component {
        Component::Normal(name) => names_git_directory(&name.to_string_lossy()),
        _ => false,
    })
}

/// Whether `real`, a resolved path, is inside a Git directory below
/// `root_real`, the resolved root ([`path_names_git_directory`] on the part
/// below it). A path outside the root is not.
pub(crate) fn in_git_directory(root_real: &Path, real: &Path) -> bool {
    real.strip_prefix(root_real)
        .is_ok_and(path_names_git_directory)
}

/// Whether the walk follows a symlink that resolves to `target` from the
/// root that resolves to `root_real`: one that stays inside the root and does
/// not lead into a Git directory there. A link into `.git` (`evil.rs ->
/// .git/config`, `gitdata -> .git`) would hand every rule, and every fix, the
/// repository's own files under another name.
pub(crate) fn walk_follows_symlink(root_real: &Path, target: &Path) -> bool {
    target.starts_with(root_real) && !in_git_directory(root_real, target)
}

/// Why a fix declines a path in a Git directory, or one a symlink leads into
/// ([`reaches_git_directory`]).
#[doc(hidden)]
#[must_use]
pub fn git_directory_refusal(path: &Path) -> String {
    format!(
        "{} is in a Git directory; a fix never writes or reads there",
        path.display()
    )
}

/// Why a rule may not read a path it names (a `cross_file` `source.file`, a
/// `json_schema_passes` `schema_path`): the path is in a Git directory, by
/// name or through a symlink ([`reaches_git_directory`]), and a file there
/// can hold credentials.
#[doc(hidden)]
#[must_use]
pub fn git_directory_read_refusal(path: &Path) -> String {
    format!(
        "{} is in a Git directory; rules never read there",
        path.display()
    )
}

/// The file names alint's configuration discovery accepts, in the order it
/// tries them in each directory.
pub const CONFIG_FILE_NAMES: [&str; 4] = [".alint.yml", ".alint.yaml", "alint.yml", "alint.yaml"];

/// The directory beside the top-level configuration whose `*.yml` and
/// `*.yaml` drop-ins load with it, with the same trust.
pub const CONFIG_DROP_IN_DIRECTORY: &str = ".alint.d";

/// The directory `alint test` reads policy tests from when it is given no
/// paths, below the directory it runs in. A policy test holds a configuration
/// and what it expects of it.
pub const POLICY_TEST_DIRECTORY: &str = "alint-tests";

/// Whether `path` names alint's own configuration: its last component is one
/// a file system could open as a name configuration discovery accepts
/// ([`CONFIG_FILE_NAMES`], at any depth, as a nested config or the
/// configuration of a run from that directory loads there), or any component
/// is one it could open as the drop-in directory (`.alint.d`) or the policy
/// tests' directory ([`POLICY_TEST_DIRECTORY`]). Spellings are read as
/// [`names_git_directory`] reads them, any case matches, and so does the
/// 8.3 short name Windows may give each (`short_name_of`: `ALINT~1.YML`,
/// `ALINT~1.D`, `ALINT-~1`).
#[must_use]
pub fn path_names_alint_config(path: &Path) -> bool {
    let names: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let is = |file: &str, long: &str| file.eq_ignore_ascii_case(long) || short_name_of(file, long);
    names.iter().any(|name| {
        opens_as(name, |file| {
            is(file, CONFIG_DROP_IN_DIRECTORY) || is(file, POLICY_TEST_DIRECTORY)
        })
    }) || names.last().is_some_and(|name| {
        opens_as(name, |file| {
            CONFIG_FILE_NAMES.iter().any(|config| is(file, config))
        })
    })
}

/// Whether `file` could be the 8.3 short name Windows gives the entry `long`
/// (`.alint.yml` as `ALINT~1.YML`, `alint-tests` as `ALINT-~1`): the first
/// characters of `long`'s name before its last dot, without leading dots and
/// with no dot or space, as many as fit in eight with `~` and a number; then
/// the first three characters of its extension, if it has one. Any case and
/// any number match. Past `~4` Windows hashes the name instead, which this
/// does not follow, as [`names_git_directory`] follows only `git~1`.
fn short_name_of(file: &str, long: &str) -> bool {
    let long = long.trim_start_matches('.');
    let (base, extension) = long.rsplit_once('.').unwrap_or((long, ""));
    let base: String = base.chars().filter(|c| !matches!(c, '.' | ' ')).collect();
    let extension: String = extension.chars().take(3).collect();
    let (short_base, short_extension) = file.rsplit_once('.').unwrap_or((file, ""));
    let Some((prefix, number)) = short_base.split_once('~') else {
        return false;
    };
    let kept = base.len().min(8usize.saturating_sub(1 + number.len()));
    short_extension.eq_ignore_ascii_case(&extension)
        && !number.is_empty()
        && !number.starts_with('0')
        && number.bytes().all(|byte| byte.is_ascii_digit())
        && prefix.len() == kept
        && base
            .get(..kept)
            .is_some_and(|kept| kept.eq_ignore_ascii_case(prefix))
}

/// Whether `path`, relative to `root`, reaches alint's own configuration: it
/// names some ([`path_names_alint_config`]), or the place it leads to once the
/// OS follows the symlinks in it does (below `root`), or is one of
/// `config_files`, the resolved files a loaded configuration came from (its
/// top-level file, even one outside the root, and its local `extends:`
/// targets). A configuration file runs the commands its rules name, so a fix
/// from a source that may not run commands must not write one.
#[must_use]
pub fn reaches_alint_config(root: &Path, path: &Path, config_files: &[PathBuf]) -> bool {
    reaches_alint_config_walked(root, path, config_files, None)
}

/// Whether a component uses Windows/HFS+ alternate-spelling syntax that can
/// alias a simpler filename (trailing `.`/` `, NTFS `:stream`, 8.3 `~`, `\`
/// separator, or non-ASCII / HFS-ignored code points).
fn has_alternate_spelling(path: &Path) -> bool {
    path.components().any(|component| match component {
        Component::Normal(name) => match name.to_str() {
            Some(text) => {
                text.ends_with(['.', ' '])
                    || text.bytes().any(|b| matches!(b, b':' | b'~' | b'\\'))
                    || !text.is_ascii()
            }
            None => true,
        },
        _ => false,
    })
}

/// Whether a single filename component `candidate` could be opened by a file
/// system as `target` (same NTFS/HFS+/8.3 rules as [`path_names_alint_config`]).
fn component_opens_as(candidate: &str, target: &str) -> bool {
    candidate.eq_ignore_ascii_case(target)
        || opens_as(candidate, |file| {
            file.eq_ignore_ascii_case(target) || short_name_of(file, target)
        })
}

/// Whether `candidate` (a resolved path) could open the same file as `target`
/// (a canonical config file path), accounting for case-insensitivity and
/// Windows/HFS+ alternate spellings (`policy.yml.`, `policy.yml::$DATA`,
/// `POLICY~1.YML`, `pol\u{200c}icy.yml`, or `\`-separated pieces).
fn path_opens_as_file(candidate: &Path, target: &Path) -> bool {
    enum Piece {
        /// Non-filename path component (`Prefix` such as `\\?\C:`, `RootDir`,
        /// etc.): compared literally (case-insensitively) rather than through
        /// NTFS stream / 8.3 filename rules so `:` in a drive prefix is never
        /// stripped as an alternate data stream delimiter.
        Structural(String),
        Normal(String),
    }

    if target
        .as_os_str()
        .eq_ignore_ascii_case(candidate.as_os_str())
    {
        return true;
    }
    if !has_alternate_spelling(candidate) {
        return false;
    }
    // Split any literal `\` inside Normal components on Unix so `dir\policy.yml.`
    // aligns component-for-component with `dir/policy.yml`.
    let expand = |path: &Path| -> Vec<Piece> {
        let mut out = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(name) => {
                    for piece in name.to_string_lossy().split('\\') {
                        if !piece.is_empty() {
                            out.push(Piece::Normal(piece.to_owned()));
                        }
                    }
                }
                other => out.push(Piece::Structural(
                    other.as_os_str().to_string_lossy().into_owned(),
                )),
            }
        }
        out
    };
    let cand = expand(candidate);
    let targ = expand(target);
    cand.len() == targ.len()
        && cand.iter().zip(&targ).all(|(c, t)| match (c, t) {
            (Piece::Structural(c), Piece::Structural(t)) => c.eq_ignore_ascii_case(t),
            (Piece::Normal(c), Piece::Normal(t)) => component_opens_as(c, t),
            _ => false,
        })
}

/// [`reaches_alint_config`], asking `index`, the walk of `root`, first. A path
/// reached without a symlink ([`follows_no_symlink`]) lands at itself below
/// the real root, where its name is the whole answer unless a loaded file ends
/// in the same names (or `path` carries a Windows/HFS+ alternate spelling);
/// only then, or with a symlink on the way, is it resolved.
pub(crate) fn reaches_alint_config_walked(
    root: &Path,
    path: &Path,
    config_files: &[PathBuf],
    index: Option<&crate::walker::FileIndex>,
) -> bool {
    if path_names_alint_config(path) {
        return true;
    }
    let ends_in_path = |file: &PathBuf| {
        let mut file = file.components().rev();
        path.components().rev().all(|component| {
            file.next()
                .is_some_and(|name| name.as_os_str().eq_ignore_ascii_case(component))
        })
    };
    let alternate = has_alternate_spelling(path);
    if !alternate && follows_no_symlink(root, path, index) && !config_files.iter().any(ends_in_path)
    {
        return false;
    }
    count_check_cost(|costs| costs.resolutions += 1);
    let Some(real) = resolve_existing_prefix(&root.join(path)) else {
        return false;
    };
    // Any case or Windows/HFS+ alternate spelling: `POLICY.yml`, `policy.yml.`,
    // `policy.yml::$DATA`, `POLICY~1.YML`, `pol\u{200c}icy.yml` can all open
    // `policy.yml`.
    if config_files
        .iter()
        .any(|file| path_opens_as_file(&real, file))
    {
        return true;
    }
    root.canonicalize().is_ok_and(|root_real| {
        real.strip_prefix(&root_real)
            .is_ok_and(path_names_alint_config)
    })
}

/// Why a fix from an `extends:`'d ruleset or a nested config declines a path
/// that reaches alint's configuration ([`reaches_alint_config`]).
#[doc(hidden)]
#[must_use]
pub fn alint_config_refusal(path: &Path) -> String {
    format!(
        "{} is an alint configuration file; a fix from an extended ruleset never writes one",
        path.display()
    )
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
    fn a_loop_of_dangling_symlinks_stops_at_the_link_limit() {
        use super::resolve_existing_prefix;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::os::unix::fs::symlink("b", root.join("a")).unwrap();
        std::os::unix::fs::symlink("a", root.join("b")).unwrap();
        // After `made/..` the resolver follows dangling symlinks, and `a` and
        // `b` lead only to each other: it gives up at the link limit rather
        // than recursing without end, and takes `a` as a name still to make.
        assert_eq!(
            resolve_existing_prefix(&root.join("made/../a/x.txt")),
            Some(root.join("a/x.txt"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn resolve_existing_prefix_lands_where_the_os_creates() {
        // Every path of up to three components over a small tree (directories,
        // a file, a symlinked directory, a missing name, dangling symlinks,
        // `..`), then a file name: wherever `create_dir_all` and a write
        // succeed, the file lands where the resolver said it would.
        use super::resolve_existing_prefix;
        const NAMES: [&str; 10] = ["d", "e", "f", "l", "m", "dl", "dn", "dc", "dd", ".."];
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
            // Dangling symlinks: to a name a path here makes (`m/../dl/x.txt`
            // lands in `m`), directly, through another link, and through `..`;
            // and to a place no path here makes.
            std::os::unix::fs::symlink("m", root.join("dl")).unwrap();
            std::os::unix::fs::symlink("dl", root.join("dc")).unwrap();
            std::os::unix::fs::symlink("./d/../m", root.join("dd")).unwrap();
            std::os::unix::fs::symlink("nowhere/zz", root.join("dn")).unwrap();
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

    /// Every spelling a file system opens as `.git` names a Git directory,
    /// as Git's own `is_ntfs_dotgit` and `is_hfs_dotgit` read them.
    #[test]
    fn every_spelling_of_git_names_a_git_directory() {
        use super::{names_git_directory, path_names_git_directory};
        for name in [
            ".git",
            ".GIT",
            ".git.",
            ".git ",
            ".git. .",
            ".git::$INDEX_ALLOCATION",
            ".git:stream",
            ".git\\x",
            "a\\.git",
            "GIT~1",
            "git~1",
            "git~2",
            "git~1.",
            "GIT~1 ",
            "git~1. .",
            "git~1:stream",
            ".g\u{200c}it",
            "\u{feff}.git",
        ] {
            assert!(names_git_directory(name), "{name:?}");
        }
        for name in [
            ".github",
            ".gitignore",
            "git",
            ".git-blame",
            "git~0",
            "git~1x",
            "a.git",
            ".gi t",
            ".git .x",
        ] {
            assert!(!names_git_directory(name), "{name:?}");
        }
        for path in [
            ".git/x",
            ".GIT/x",
            "sub/.git/x",
            "git~1/x",
            ".git./x",
            ".git /x",
        ] {
            assert!(path_names_git_directory(Path::new(path)), "{path:?}");
        }
        assert!(!path_names_git_directory(Path::new("sub/.github/x")));
    }

    /// Every name discovery, the drop-in directory or `alint test` reads a
    /// configuration from names alint's configuration, as does the 8.3 short
    /// name Windows may give it.
    #[test]
    fn every_spelling_of_a_config_name_names_alint_configuration() {
        use super::path_names_alint_config;
        for path in [
            ".alint.yml",
            "sub/alint.yaml",
            ".ALINT.YML.",
            ".alint.d/x.yml",
            "alint-tests/case.yml",
            "sub/alint-tests/fixtures/a.txt",
            "ALINT-TESTS/case.yml",
            "alint-tests",
            // 8.3 short names.
            "ALINT~1.YML",
            "alint~1.yml",
            "sub/ALINT~2.YAM",
            "ALINT~1.D/x.yml",
            "ALINT~1.d",
            "ALINT-~1/case.yml",
            "ALINT-~3/x",
            "ALINT~10/x",
            "ALINT~1.YML.",
            "ALINT~1.YML:stream",
        ] {
            assert!(path_names_alint_config(Path::new(path)), "{path:?}");
        }
        for path in [
            "README.md",
            "alint.yml.bak",
            "alint-tests.md",
            "src/alint-tests-helper.rs",
            "ALINT~1.TXT",
            "ALINT~0.YML",
            "ALINT~.YML",
            "ALINT~1X.YML",
            "ALIN~1.YML",
            "ALINTX~1.YML",
            "ALINT~1.YML/x",
            "ALINT~1.DX/x",
            "ALINT~1/x",
        ] {
            assert!(!path_names_alint_config(Path::new(path)), "{path:?}");
        }
    }

    /// A path reaches a Git directory through an in-repo symlink to one too.
    #[test]
    #[cfg(unix)]
    fn a_symlink_to_the_git_directory_reaches_it() {
        use super::reaches_git_directory;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::os::unix::fs::symlink(".git", root.join("g")).unwrap();
        assert!(reaches_git_directory(root, Path::new("g/config")));
        assert!(reaches_git_directory(root, Path::new("g")));
        assert!(reaches_git_directory(root, Path::new(".git/commondir")));
        assert!(!reaches_git_directory(root, Path::new("src/config")));
        assert!(!reaches_git_directory(root, Path::new("new/config")));
    }

    /// The Git-directory and configuration checks resolve a path through its
    /// symlinks only when one is on the way: otherwise they cost an `lstat` per
    /// component, and nothing for a file the walk reached without one. Resolving
    /// every path made the S3 bench run thousands more `stat` calls and
    /// `alint fix` several times as many; the counts show such a resolution.
    #[test]
    #[cfg(unix)]
    fn only_a_symlink_on_the_way_makes_a_path_check_resolve() {
        use super::{
            NamedRead, path_check_costs, reaches_alint_config, reaches_alint_config_walked,
            reaches_git_directory, reaches_git_directory_walked, resolve_named_read,
        };
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(root.join("src/deep/a.rs"), "").unwrap();
        std::fs::write(root.join("policy.yml"), "").unwrap();
        std::fs::write(root.join(".gitignore"), "hidden.rs\n").unwrap();
        std::fs::write(root.join("hidden.rs"), "").unwrap();
        std::os::unix::fs::symlink(".git", root.join("g")).unwrap();
        std::os::unix::fs::symlink("src", root.join("lnk")).unwrap();
        std::os::unix::fs::symlink("src/deep/a.rs", root.join("alias.rs")).unwrap();
        let config = vec![root.canonicalize().unwrap().join("policy.yml")];
        let index = crate::walker::walk(root, &crate::walker::WalkOptions::test_default()).unwrap();
        // (answer, resolutions, symlink probes) of one check.
        let cost = |check: &dyn Fn() -> bool| {
            let before = path_check_costs();
            let answer = check();
            let after = path_check_costs();
            (
                answer,
                after.resolutions - before.resolutions,
                after.symlink_probes - before.symlink_probes,
            )
        };
        let p = Path::new;
        // No symlink on the way: one probe, whether the path exists or not.
        for path in ["src/deep/a.rs", "new/dir/x"] {
            assert_eq!(
                cost(&|| reaches_git_directory(root, p(path))),
                (false, 0, 1)
            );
        }
        assert_eq!(
            cost(&|| reaches_alint_config(root, p("src/deep/a.rs"), &config)),
            (false, 0, 1)
        );
        // A file the walk reached without one: no file system call at all.
        let walked = Some(&index);
        assert!(index.walked_without_symlink(p("src/deep/a.rs")));
        assert_eq!(
            cost(&|| reaches_git_directory_walked(root, p("src/deep/a.rs"), walked)),
            (false, 0, 0)
        );
        assert_eq!(
            cost(&|| reaches_alint_config_walked(root, p("src/deep/a.rs"), &config, walked)),
            (false, 0, 0)
        );
        // The walk cannot vouch for a path through a symlink it followed, one
        // it did not index, or an index built from entries.
        for path in [
            "lnk/deep/a.rs",
            "alias.rs",
            "hidden.rs",
            "new.rs",
            "src/deep",
        ] {
            assert!(!index.walked_without_symlink(p(path)), "{path}");
        }
        let built = crate::walker::FileIndex::from_entries(index.entries.clone());
        assert!(!built.walked_without_symlink(p("src/deep/a.rs")));
        // A symlink on the way, or a loaded file that could be the path, is
        // resolved; a name decides first, for nothing.
        assert_eq!(
            cost(&|| reaches_git_directory(root, p("g/config"))),
            (true, 1, 1)
        );
        assert_eq!(
            cost(&|| reaches_git_directory_walked(root, p("lnk/deep/a.rs"), walked)),
            (false, 1, 1)
        );
        assert_eq!(
            cost(&|| reaches_git_directory(root, p("a/../g/x"))),
            (true, 1, 0)
        );
        assert_eq!(
            cost(&|| reaches_alint_config(root, p("POLICY.yml"), &config)),
            (true, 1, 1)
        );
        assert_eq!(
            cost(&|| reaches_git_directory(root, p(".git/x"))),
            (true, 0, 0)
        );
        // A named read resolves once for both of its checks.
        for (path, landing) in [
            ("src/deep/a.rs", NamedRead::InRoot),
            ("g/config", NamedRead::GitDirectory),
            ("../x", NamedRead::OutsideRoot),
        ] {
            let before = path_check_costs();
            assert_eq!(resolve_named_read(root, p(path)), landing, "{path}");
            let after = path_check_costs();
            assert_eq!(
                (
                    after.resolutions - before.resolutions,
                    after.symlink_probes - before.symlink_probes
                ),
                (1, 0)
            );
        }
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

    #[test]
    #[cfg(unix)]
    fn out_of_root_git_directory_is_still_refused_as_git_directory() {
        use super::{NamedRead, reaches_git_directory, resolve_named_read};
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git/config"), "[core]\n").unwrap();
        let repo = tmp.path().join("subrepo");
        std::fs::create_dir_all(&repo).unwrap();
        std::os::unix::fs::symlink("../.git/config", repo.join("cfg")).unwrap();
        assert_eq!(
            resolve_named_read(&repo, Path::new("cfg")),
            NamedRead::GitDirectory
        );
        assert_eq!(
            resolve_named_read(&repo, Path::new("../.git/config")),
            NamedRead::GitDirectory
        );
        assert!(reaches_git_directory(&repo, Path::new("cfg")));
        assert!(reaches_git_directory(&repo, Path::new("../.git/config")));
    }

    #[test]
    fn reaches_alint_config_catches_alternate_spellings_of_extended_config() {
        use super::reaches_alint_config;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("policy.yml"), "version: 1\nrules: []\n").unwrap();
        let config = vec![root.canonicalize().unwrap().join("policy.yml")];
        for spelling in [
            "policy.yml",
            "POLICY.YML",
            "policy.yml.",
            "policy.yml ",
            "policy.yml::$DATA",
            "POLICY~1.YML",
            "pol\u{200c}icy.yml",
        ] {
            assert!(
                reaches_alint_config(root, Path::new(spelling), &config),
                "alternate spelling {spelling:?} must be refused"
            );
        }
        assert!(!reaches_alint_config(root, Path::new("other.yml"), &config));
        assert!(super::path_opens_as_file(
            Path::new(r"\\?\C:\repo\POLICY~1.YML"),
            Path::new(r"\\?\C:\repo\policy.yml"),
        ));
        assert!(super::path_opens_as_file(
            Path::new("/repo/sub:dir/POLICY~1.YML"),
            Path::new("/repo/sub:dir/policy.yml"),
        ));
    }
}
