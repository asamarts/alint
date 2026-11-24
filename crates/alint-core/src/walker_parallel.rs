//! Owned parallel directory walker built on `ignore::gitignore::Gitignore`
//! and `rayon::scope` (#573, #568, ADR-0048 decision 5).
//!
//! Keeps directory traversal behind a clean internal module boundary so every
//! directory is listed once (`std::fs::read_dir`) and its ignore files
//! (`.ignore`, `.gitignore`, `.git` -> `info/exclude`) are discovered directly
//! from that listing and read via open-then-`fstat` bounded reads, adding zero
//! per-directory `stat` probes (retiring ADR-0045's probe allowance).
//!
//! Ignore-file policy (ADR-0048 decision 5b, #568):
//! - Inside the checked root: an unreadable, non-regular (FIFO, device, socket,
//!   or symlink to one), or oversize (> 32 MiB) ignore file (or `.git` file /
//!   `commondir` on the way to `info/exclude`) fails the walk.
//! - Above the checked root (in parent directories): the same bad ignore file
//!   is skipped with a visible warning rather than failing the walk.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ignore::Match;
use ignore::gitignore::Gitignore;
use ignore::overrides::Override;

use crate::error::{Error, Result};

use super::ignore_files::{self, IgnoreFileRead, IgnoreRefusal};
use super::{DirIgnores, FileEntry, WalkOptions, build_overrides, gitignore_from};

/// Everything produced by one parallel tree walk.
pub(super) struct WalkOutput {
    pub(super) entries: Vec<FileEntry>,
    pub(super) escaping_symlinks: Vec<Arc<Path>>,
    pub(super) followed_symlinks: Vec<Arc<Path>>,
    pub(super) warnings: Vec<String>,
}

/// One node in the immutable in-tree ignore chain. Only allocated for
/// directories that actually contribute an ignore file or a repository
/// boundary marker (`.git` / `.jj`), so directories without ignore files
/// borrow their parent's chain with zero heap allocation.
struct IgnoreNode {
    dir: DirIgnores,
    parent: Option<Arc<IgnoreNode>>,
}

/// Canonical ancestor directory chain for symlink-loop detection.
///
/// While no symlink has been followed on the path from the walk root
/// (`Linear`), every visited ancestor's canonical path is a prefix between
/// `root_canonical` and `current_canonical`, requiring zero allocations and
/// zero syscalls per normal directory. Once a symlinked directory is entered,
/// we switch to `Explicit`, tracking each visited canonical directory on that
/// branch so mutual directory symlinks and `alias/deep/up -> ..` loops are
/// still detected without blocking distinct aliases (`real/` and `alias/`)
/// from both being walked.
#[derive(Clone)]
enum Ancestors {
    Linear {
        root: Option<PathBuf>,
        current: Option<PathBuf>,
    },
    Explicit(Arc<Vec<PathBuf>>),
}

impl Ancestors {
    fn at_root(canonical_root: Option<PathBuf>) -> Self {
        Self::Linear {
            current: canonical_root.clone(),
            root: canonical_root,
        }
    }

    fn contains(&self, target: &Path) -> bool {
        match self {
            Self::Linear {
                root: Some(root),
                current: Some(current),
            } => current.starts_with(target) && target.starts_with(root),
            Self::Linear { .. } => false,
            Self::Explicit(chain) => chain.iter().any(|dir| dir == target),
        }
    }

    fn push_normal(&self, name: &OsStr) -> Self {
        match self {
            Self::Linear { root, current } => Self::Linear {
                root: root.clone(),
                current: current.as_ref().map(|cur| cur.join(name)),
            },
            Self::Explicit(chain) => {
                let mut next = (**chain).clone();
                if let Some(last) = next.last() {
                    next.push(last.join(name));
                }
                Self::Explicit(Arc::new(next))
            }
        }
    }

    fn push_symlink(&self, target: PathBuf) -> Self {
        let mut next = match self {
            Self::Linear {
                root: Some(root),
                current: Some(current),
            } => {
                let mut chain = Vec::new();
                if let Ok(suffix) = current.strip_prefix(root) {
                    let mut cur = root.clone();
                    chain.push(cur.clone());
                    for comp in suffix.components() {
                        cur.push(comp);
                        chain.push(cur.clone());
                    }
                } else {
                    chain.push(root.clone());
                }
                chain
            }
            Self::Linear { .. } => Vec::new(),
            Self::Explicit(chain) => (**chain).clone(),
        };
        next.push(target);
        Self::Explicit(Arc::new(next))
    }
}

/// Shared read-only state and synchronized sinks for one parallel walk.
struct WalkCtx {
    canonical_walk_root: Option<PathBuf>,
    canonical_confinement_root: Option<PathBuf>,
    overrides: Override,
    respect_gitignore: bool,
    bounded: bool,
    /// Ignore files of directories above `canonical_walk_root`, nearest first.
    /// Non-empty only when `respect_gitignore && bounded`.
    parents: Vec<DirIgnores>,
    global: Option<Gitignore>,
    cancelled: AtomicBool,
    error_slot: Mutex<Option<Error>>,
    escaping: Mutex<Vec<Arc<Path>>>,
    followed: Mutex<Vec<Arc<Path>>>,
    chunks: Mutex<Vec<Vec<FileEntry>>>,
}

impl WalkCtx {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    fn fail(&self, err: Error) {
        self.cancelled.store(true, Ordering::Relaxed);
        let mut slot = self.error_slot.lock().expect("walker error slot lock");
        if slot.is_none() {
            *slot = Some(err);
        }
    }

    fn push_escaping(&self, rel: Arc<Path>) {
        if let Ok(mut list) = self.escaping.lock() {
            list.push(rel);
        }
    }

    fn push_followed(&self, rel: Arc<Path>) {
        if let Ok(mut list) = self.followed.lock() {
            list.push(rel);
        }
    }

    fn flush_entries(&self, local: Vec<FileEntry>) {
        if local.is_empty() {
            return;
        }
        if let Ok(mut chunks) = self.chunks.lock() {
            chunks.push(local);
        }
    }

    /// Whether `abs` (`root.join(rel)`) is excluded by overrides or active
    /// ignore files. Mirrors `ignore` 0.4.34 and `WalkSkips::skips` precedence:
    /// 1. Overrides (`.git` + config `extra_ignores`): `Ignore` excludes,
    ///    `Whitelist` includes immediately.
    /// 2. Nearest `.ignore` (in-tree chain, then parent dirs above root when
    ///    `bounded`).
    /// 3. Nearest `.gitignore` (in-tree chain stopping at nearest repo marker
    ///    when `bounded`, then parents up to the repo root).
    /// 4. Nearest `info/exclude` (same repository boundary as `.gitignore`).
    /// 5. Global git excludes file.
    fn is_ignored(
        &self,
        abs: &Path,
        canonical_abs: Option<&Path>,
        is_dir: bool,
        chain: Option<&IgnoreNode>,
    ) -> bool {
        match self.overrides.matched(abs, is_dir) {
            Match::Ignore(_) => return true,
            Match::Whitelist(_) => return false,
            Match::None => {}
        }
        if !self.respect_gitignore {
            return false;
        }

        let nearest = |kind: fn(&DirIgnores) -> Option<&Gitignore>, stops: bool| {
            let mut cursor = chain;
            while let Some(node) = cursor {
                if let Some(gi) = kind(&node.dir) {
                    let m = gi.matched(abs, is_dir);
                    if !m.is_none() {
                        return Some(m);
                    }
                }
                if stops && node.dir.repository {
                    return None;
                }
                cursor = node.parent.as_deref();
            }
            if let Some(candidate) = canonical_abs {
                for dir in &self.parents {
                    if let Some(gi) = kind(dir) {
                        let m = gi.matched(candidate, is_dir);
                        if !m.is_none() {
                            return Some(m);
                        }
                    }
                    if stops && dir.repository {
                        break;
                    }
                }
            }
            None
        };

        let found = nearest(|d| d.dot_ignore.as_ref(), false)
            .or_else(|| nearest(|d| d.git_ignore.as_ref(), self.bounded))
            .or_else(|| nearest(|d| d.exclude.as_ref(), self.bounded))
            .or_else(|| self.global.as_ref().map(|gi| gi.matched(abs, is_dir)));
        matches!(found, Some(Match::Ignore(_)))
    }
}

/// One raw child entry captured from `std::fs::read_dir`.
struct RawChild {
    name: std::ffi::OsString,
    entry: std::fs::DirEntry,
    file_type: std::fs::FileType,
}

/// A subdirectory queued for descent.
struct PendingDir {
    rel: PathBuf,
    abs: PathBuf,
    ancestors: Ancestors,
}

/// Walk `root`, confining followed symlinks to `confinement_root`.
pub(super) fn walk_tree(
    root: &Path,
    confinement_root: &Path,
    opts: &WalkOptions,
) -> Result<WalkOutput> {
    let overrides = build_overrides(root, opts)?;

    // Canonicalize once up front (reused for parent ignores, symlink
    // confinement, and cycle detection).
    let canonical_confinement_root = confinement_root.canonicalize().ok();
    let canonical_walk_root = if confinement_root == root {
        canonical_confinement_root.clone()
    } else {
        root.canonicalize().ok()
    };

    // List the walk root first: if the root itself is unreadable or missing,
    // fail immediately; otherwise we inspect its ignore files and repository
    // markers directly from its listing with zero extra `stat` probes.
    let root_listing = read_directory_listing(root)?;

    let mut warnings = Vec::new();
    let (root_ignores, bounded, parents, global) = if opts.respect_gitignore {
        let root_dir_ignores = load_dir_ignores_from_listing(root, &root_listing)?;
        let root_has_repo = root_dir_ignores.as_ref().is_some_and(|d| d.repository);
        let (bounded, parents) =
            load_parent_ignores(canonical_walk_root.as_deref(), root_has_repo, &mut warnings);
        let global = if opts.uses_global_git_excludes() {
            ignore_files::load_global_excludes()?
        } else {
            None
        };
        let chain = root_dir_ignores.map(|dir| Arc::new(IgnoreNode { dir, parent: None }));
        (chain, bounded, parents, global)
    } else {
        (None, false, Vec::new(), None)
    };

    let ctx = WalkCtx {
        canonical_walk_root: canonical_walk_root.clone(),
        canonical_confinement_root,
        overrides,
        respect_gitignore: opts.respect_gitignore,
        bounded,
        parents,
        global,
        cancelled: AtomicBool::new(false),
        error_slot: Mutex::new(None),
        escaping: Mutex::new(Vec::new()),
        followed: Mutex::new(Vec::new()),
        chunks: Mutex::new(Vec::new()),
    };

    let root_ancestors = Ancestors::at_root(canonical_walk_root);
    let mut initial_entries = Vec::new();
    if rayon::current_num_threads() <= 1 {
        process_directory_sequential(
            &ctx,
            Path::new(""),
            root,
            root_listing,
            root_ignores.as_ref(),
            &root_ancestors,
            &mut initial_entries,
        );
    } else {
        rayon::scope(|scope| {
            process_directory_parallel(
                &ctx,
                scope,
                Path::new(""),
                root,
                root_listing,
                root_ignores.as_ref(),
                &root_ancestors,
                &mut initial_entries,
            );
        });
    }
    ctx.flush_entries(initial_entries);

    if let Some(err) = ctx.error_slot.into_inner().expect("walker error slot lock") {
        return Err(err);
    }

    let chunks = ctx.chunks.into_inner().expect("walker chunks lock");
    let total_entries: usize = chunks.iter().map(Vec::len).sum();
    let mut entries = Vec::with_capacity(total_entries);
    for chunk in chunks {
        entries.extend(chunk);
    }
    entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));

    let mut escaping_symlinks = ctx.escaping.into_inner().expect("walker escaping lock");
    escaping_symlinks.sort_unstable();
    escaping_symlinks.dedup();

    let mut followed_symlinks = ctx.followed.into_inner().expect("walker followed lock");
    followed_symlinks.sort_unstable();
    followed_symlinks.dedup();

    Ok(WalkOutput {
        entries,
        escaping_symlinks,
        followed_symlinks,
        warnings,
    })
}

/// Read all entries of `dir` along with their `FileType` (from `d_type` on
/// Unix / `WIN32_FIND_DATA` on Windows — zero extra `stat` calls on common
/// filesystems).
fn read_directory_listing(dir: &Path) -> Result<Vec<RawChild>> {
    let rd = std::fs::read_dir(dir).map_err(|source| Error::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut children = Vec::new();
    for item in rd {
        let entry = match item {
            Ok(entry) => entry,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(Error::Io {
                    path: dir.to_path_buf(),
                    source,
                });
            }
        };
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(Error::Io {
                    path: entry.path(),
                    source,
                });
            }
        };
        children.push(RawChild {
            name: entry.file_name(),
            entry,
            file_type,
        });
    }
    Ok(children)
}

/// Inspect `children` of `dir` (whose ignore chain is `chain`), emitting kept
/// entries into `local_entries` and returning subdirectories to descend into.
///
/// Reuses one `abs_buf`, one `rel_buf`, and (when parent ignore files exist)
/// one `canonical_buf` per directory via `push`/`pop` so regular files incur
/// zero intermediate `PathBuf` allocations.
fn inspect_children(
    ctx: &WalkCtx,
    rel_dir: &Path,
    abs_dir: &Path,
    children: Vec<RawChild>,
    chain: Option<&IgnoreNode>,
    ancestors: &Ancestors,
    local_entries: &mut Vec<FileEntry>,
) -> Vec<PendingDir> {
    let mut subdirs = Vec::new();
    let mut rel_buf = rel_dir.to_path_buf();
    let mut abs_buf = abs_dir.to_path_buf();
    let mut canonical_buf = (!ctx.parents.is_empty())
        .then(|| {
            ctx.canonical_walk_root.as_ref().map(|r| {
                if rel_dir.as_os_str().is_empty() {
                    r.clone()
                } else {
                    r.join(rel_dir)
                }
            })
        })
        .flatten();

    for child in children {
        if ctx.is_cancelled() {
            break;
        }
        let ft = child.file_type;
        let is_symlink = ft.is_symlink();
        let is_dir = !is_symlink && ft.is_dir();
        let is_file = !is_symlink && ft.is_file();
        if !is_symlink && !is_dir && !is_file {
            // Special files (FIFO, socket, char/block device): silently skipped.
            continue;
        }

        rel_buf.push(&child.name);
        abs_buf.push(&child.name);
        if let Some(buf) = &mut canonical_buf {
            buf.push(&child.name);
        }

        if is_symlink {
            handle_symlink(
                ctx,
                &rel_buf,
                &abs_buf,
                canonical_buf.as_deref(),
                chain,
                ancestors,
                local_entries,
                &mut subdirs,
            );
        } else if is_dir {
            if !ctx.is_ignored(&abs_buf, canonical_buf.as_deref(), true, chain) {
                let next_ancestors = ancestors.push_normal(&child.name);
                local_entries.push(FileEntry {
                    path: Arc::from(rel_buf.as_path()),
                    is_dir: true,
                    size: 0,
                });
                subdirs.push(PendingDir {
                    rel: rel_buf.clone(),
                    abs: abs_buf.clone(),
                    ancestors: next_ancestors,
                });
            }
        } else if !ctx.is_ignored(&abs_buf, canonical_buf.as_deref(), false, chain) {
            match child.entry.metadata() {
                Ok(meta) if meta.is_file() => {
                    local_entries.push(FileEntry {
                        path: Arc::from(rel_buf.as_path()),
                        is_dir: false,
                        size: meta.len(),
                    });
                }
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(source) => {
                    ctx.fail(Error::Io {
                        path: abs_buf.clone(),
                        source,
                    });
                    break;
                }
            }
        }

        rel_buf.pop();
        abs_buf.pop();
        if let Some(buf) = &mut canonical_buf {
            buf.pop();
        }
    }
    subdirs
}

/// Resolve, filter, and confine a symlink entry.
#[allow(clippy::too_many_arguments)]
fn handle_symlink(
    ctx: &WalkCtx,
    rel: &Path,
    abs: &Path,
    canonical_abs: Option<&Path>,
    chain: Option<&IgnoreNode>,
    ancestors: &Ancestors,
    local_entries: &mut Vec<FileEntry>,
    subdirs: &mut Vec<PendingDir>,
) {
    // Resolve the symlink target first. A broken (dangling) symlink or a
    // kernel ELOOP direct self-cycle is skipped without aborting the walk,
    // matching `ignore::WalkBuilder` with `follow_links(true)` +
    // `is_skippable_walk_error`.
    let target = match abs.canonicalize() {
        Ok(target) => target,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound || is_eloop_io(&err) => {
            return;
        }
        Err(_) => {
            if !ctx.is_ignored(abs, canonical_abs, false, chain) {
                ctx.push_escaping(Arc::from(rel));
            }
            return;
        }
    };
    let meta = match std::fs::metadata(&target) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound || is_eloop_io(&err) => {
            return;
        }
        Err(_) => {
            if !ctx.is_ignored(abs, canonical_abs, false, chain) {
                ctx.push_escaping(Arc::from(rel));
            }
            return;
        }
    };
    let is_dir = meta.is_dir();
    if ctx.is_ignored(abs, canonical_abs, is_dir, chain) {
        return;
    }
    let keep = ctx
        .canonical_confinement_root
        .as_deref()
        .is_some_and(|real_root| crate::pathsafe::walk_follows_symlink(real_root, &target));
    if !keep {
        ctx.push_escaping(Arc::from(rel));
        return;
    }
    if is_dir && ancestors.contains(&target) {
        // Ancestor symlink cycle (`loop -> .`, `sub/up -> ..`, mutual directory
        // symlinks): skip cleanly without indexing or recursing.
        return;
    }
    let rel_arc: Arc<Path> = Arc::from(rel);
    ctx.push_followed(Arc::clone(&rel_arc));
    if is_dir {
        let next_ancestors = ancestors.push_symlink(target);
        local_entries.push(FileEntry {
            path: rel_arc,
            is_dir: true,
            size: 0,
        });
        subdirs.push(PendingDir {
            rel: rel.to_path_buf(),
            abs: abs.to_path_buf(),
            ancestors: next_ancestors,
        });
    } else if meta.is_file() {
        local_entries.push(FileEntry {
            path: rel_arc,
            is_dir: false,
            size: meta.len(),
        });
    }
}

/// Descend into a subdirectory, reading its listing and (when enabled) its own
/// ignore files from that listing. Returns `None` if the directory vanished or
/// the walk failed / was cancelled.
fn open_subdir(
    ctx: &WalkCtx,
    dir: &PendingDir,
    parent_chain: Option<&Arc<IgnoreNode>>,
) -> Option<(Vec<RawChild>, Option<Arc<IgnoreNode>>)> {
    if ctx.is_cancelled() {
        return None;
    }
    let listing = match read_directory_listing(&dir.abs) {
        Ok(listing) => listing,
        Err(Error::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return None;
        }
        Err(err) => {
            ctx.fail(err);
            return None;
        }
    };
    let next_chain = if ctx.respect_gitignore {
        match load_dir_ignores_from_listing(&dir.abs, &listing) {
            Ok(Some(ignores)) => Some(Arc::new(IgnoreNode {
                dir: ignores,
                parent: parent_chain.cloned(),
            })),
            Ok(None) => parent_chain.cloned(),
            Err(err) => {
                ctx.fail(err);
                return None;
            }
        }
    } else {
        None
    };
    Some((listing, next_chain))
}

/// Single-threaded recursive traversal: zero `rayon::scope` overhead when
/// `RAYON_NUM_THREADS=1`.
fn process_directory_sequential(
    ctx: &WalkCtx,
    rel_dir: &Path,
    abs_dir: &Path,
    listing: Vec<RawChild>,
    chain: Option<&Arc<IgnoreNode>>,
    ancestors: &Ancestors,
    local_entries: &mut Vec<FileEntry>,
) {
    let subdirs = inspect_children(
        ctx,
        rel_dir,
        abs_dir,
        listing,
        chain.map(Arc::as_ref),
        ancestors,
        local_entries,
    );
    for sub in subdirs {
        if let Some((sub_listing, sub_chain)) = open_subdir(ctx, &sub, chain) {
            process_directory_sequential(
                ctx,
                &sub.rel,
                &sub.abs,
                sub_listing,
                sub_chain.as_ref(),
                &sub.ancestors,
                local_entries,
            );
        }
    }
}

/// Work-stealing parallel traversal over `rayon::scope`. Reuses the current
/// worker's `local_entries` buffer for the last subdirectory (tail-call) and
/// spawns sibling subtrees onto the pool, avoiding spawns altogether for leaf
/// and single-child directories.
#[allow(clippy::too_many_arguments)]
fn process_directory_parallel<'scope>(
    ctx: &'scope WalkCtx,
    scope: &rayon::Scope<'scope>,
    rel_dir: &Path,
    abs_dir: &Path,
    listing: Vec<RawChild>,
    chain: Option<&Arc<IgnoreNode>>,
    ancestors: &Ancestors,
    local_entries: &mut Vec<FileEntry>,
) {
    let mut subdirs = inspect_children(
        ctx,
        rel_dir,
        abs_dir,
        listing,
        chain.map(Arc::as_ref),
        ancestors,
        local_entries,
    );
    let Some(tail) = subdirs.pop() else {
        return;
    };
    for sub in subdirs {
        if ctx.is_cancelled() {
            return;
        }
        let parent_chain = chain.cloned();
        scope.spawn(move |inner_scope| {
            let mut task_entries = Vec::new();
            if let Some((sub_listing, sub_chain)) = open_subdir(ctx, &sub, parent_chain.as_ref()) {
                process_directory_parallel(
                    ctx,
                    inner_scope,
                    &sub.rel,
                    &sub.abs,
                    sub_listing,
                    sub_chain.as_ref(),
                    &sub.ancestors,
                    &mut task_entries,
                );
            }
            ctx.flush_entries(task_entries);
        });
    }
    if let Some((tail_listing, tail_chain)) = open_subdir(ctx, &tail, chain) {
        process_directory_parallel(
            ctx,
            scope,
            &tail.rel,
            &tail.abs,
            tail_listing,
            tail_chain.as_ref(),
            &tail.ancestors,
            local_entries,
        );
    }
}

/// Inspect `listing` of `dir` for exact-name ignore files (`.ignore`,
/// `.gitignore`) and repository markers (`.git`, `.jj`), reading any present
/// files strictly through bounded open-then-`fstat` checks. Returns `Ok(None)`
/// when the directory contributes no ignore rules and no repository boundary.
fn load_dir_ignores_from_listing(dir: &Path, listing: &[RawChild]) -> Result<Option<DirIgnores>> {
    let mut dot_ignore_entry = None;
    let mut git_ignore_entry = None;
    let mut dot_git_entry = None;
    let mut dot_jj_entry = None;

    for child in listing {
        let name = child.name.as_os_str();
        if name == OsStr::new(".ignore") {
            dot_ignore_entry = Some(child.file_type);
        } else if name == OsStr::new(".gitignore") {
            git_ignore_entry = Some(child.file_type);
        } else if name == OsStr::new(".git") {
            dot_git_entry = Some(child.file_type);
        } else if name == OsStr::new(".jj") {
            dot_jj_entry = Some(child.file_type);
        }
    }

    if dot_ignore_entry.is_none()
        && git_ignore_entry.is_none()
        && dot_git_entry.is_none()
        && dot_jj_entry.is_none()
    {
        return Ok(None);
    }

    let dot_ignore = match dot_ignore_entry {
        Some(ft) => load_inside_gitignore(dir, &dir.join(".ignore"), ft)?,
        None => None,
    };
    let git_ignore = match git_ignore_entry {
        Some(ft) => load_inside_gitignore(dir, &dir.join(".gitignore"), ft)?,
        None => None,
    };

    let mut repository = false;
    let mut exclude = None;
    if let Some(ft) = dot_git_entry {
        let dot_git = dir.join(".git");
        let (is_repo, git_exclude) = load_inside_git_exclude(dir, dot_git, ft)?;
        repository |= is_repo;
        exclude = git_exclude;
    }
    if let Some(ft) = dot_jj_entry {
        repository |= if ft.is_symlink() {
            dir.join(".jj").exists()
        } else {
            ft.is_dir() || ft.is_file()
        };
    }

    if dot_ignore.is_none() && git_ignore.is_none() && exclude.is_none() && !repository {
        return Ok(None);
    }
    Ok(Some(DirIgnores {
        dot_ignore,
        git_ignore,
        exclude,
        repository,
    }))
}

fn load_inside_gitignore(
    dir: &Path,
    path: &Path,
    ft: std::fs::FileType,
) -> Result<Option<Gitignore>> {
    match ignore_files::read_from_entry(path.to_path_buf(), ft) {
        IgnoreFileRead::Bytes(content) => Ok(gitignore_from(dir, path, &content)),
        IgnoreFileRead::Absent => Ok(None),
        IgnoreFileRead::Refused(refused) => Err(refused.into_error()),
    }
}

/// Resolve a listed `.git` entry inside the checked root: returns whether it
/// marks a repository and, when present, its `info/exclude` matcher.
fn load_inside_git_exclude(
    dir: &Path,
    dot_git: PathBuf,
    ft: std::fs::FileType,
) -> Result<(bool, Option<Gitignore>)> {
    if ft.is_dir() {
        let exclude = read_inside_exclude_file(dir, &dot_git.join("info/exclude"))?;
        return Ok((true, exclude));
    }
    if ft.is_file() {
        let bytes = match ignore_files::read_from_entry(dot_git, ft) {
            IgnoreFileRead::Bytes(bytes) => bytes,
            IgnoreFileRead::Absent => return Ok((false, None)),
            IgnoreFileRead::Refused(refused) => return Err(refused.into_error()),
        };
        let exclude = read_inside_worktree_exclude(dir, &bytes)?;
        return Ok((true, exclude));
    }
    if ft.is_symlink() {
        let meta = match std::fs::metadata(&dot_git) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok((false, None)),
            Err(err) => {
                return Err(IgnoreRefusal::new(dot_git, err.to_string()).into_error());
            }
        };
        if meta.is_dir() {
            let exclude = read_inside_exclude_file(dir, &dot_git.join("info/exclude"))?;
            return Ok((true, exclude));
        }
        if meta.is_file() {
            let bytes = match ignore_files::read_by_path(dot_git) {
                IgnoreFileRead::Bytes(bytes) => bytes,
                IgnoreFileRead::Absent => return Ok((false, None)),
                IgnoreFileRead::Refused(refused) => return Err(refused.into_error()),
            };
            let exclude = read_inside_worktree_exclude(dir, &bytes)?;
            return Ok((true, exclude));
        }
        return Ok((false, None));
    }
    Ok((false, None))
}

fn read_inside_worktree_exclude(dir: &Path, dot_git_bytes: &[u8]) -> Result<Option<Gitignore>> {
    let Some(first) = ignore_files::first_line_from_bytes(dot_git_bytes) else {
        return Ok(None);
    };
    let Some(git_dir_str) = first.strip_prefix("gitdir: ") else {
        return Ok(None);
    };
    let git_dir = PathBuf::from(git_dir_str);
    let commondir_path = git_dir.join("commondir");
    let commondir_bytes = match ignore_files::read_by_path(commondir_path) {
        IgnoreFileRead::Bytes(bytes) => bytes,
        IgnoreFileRead::Absent => return Ok(None),
        IgnoreFileRead::Refused(refused) => return Err(refused.into_error()),
    };
    let Some(common) = ignore_files::first_line_from_bytes(&commondir_bytes) else {
        return Ok(None);
    };
    let common_dir = if common.starts_with('.') {
        git_dir.join(common)
    } else {
        PathBuf::from(common)
    };
    read_inside_exclude_file(dir, &common_dir.join("info/exclude"))
}

fn read_inside_exclude_file(dir: &Path, path: &Path) -> Result<Option<Gitignore>> {
    match ignore_files::read_by_path(path.to_path_buf()) {
        IgnoreFileRead::Bytes(content) => Ok(gitignore_from(dir, path, &content)),
        IgnoreFileRead::Absent => Ok(None),
        IgnoreFileRead::Refused(refused) => Err(refused.into_error()),
    }
}

/// Load ignore files from directories above `canonical_walk_root` in a single
/// pass. Returns `(bounded, parents)` where `parents` is ordered nearest-first.
///
/// Per ADR-0048 decision 5b (#568), any unreadable, non-regular, or oversize
/// ignore file above the checked root is skipped with a visible warning
/// recorded in `warnings` (and logged via `tracing::warn!`).
fn load_parent_ignores(
    canonical_walk_root: Option<&Path>,
    root_has_repo: bool,
    warnings: &mut Vec<String>,
) -> (bool, Vec<DirIgnores>) {
    let Some(canon_root) = canonical_walk_root else {
        return (root_has_repo, Vec::new());
    };
    let ancestors: Vec<&Path> = canon_root.ancestors().skip(1).collect();
    if ancestors.is_empty() {
        return (root_has_repo, Vec::new());
    }

    if root_has_repo {
        // The checked root itself holds a repository boundary: `.gitignore` and
        // `info/exclude` stop at the root, so above the root only `.ignore`
        // files apply (one probe per ancestor, zero `.git`/`.jj`/`.gitignore`
        // probes).
        let mut parents = Vec::new();
        for dir in ancestors {
            let path = dir.join(".ignore");
            if let Some(content) = read_parent_file(path.clone(), warnings)
                && let Some(gi) = gitignore_from(dir, &path, &content)
            {
                parents.push(DirIgnores {
                    dot_ignore: Some(gi),
                    git_ignore: None,
                    exclude: None,
                    repository: false,
                });
            }
        }
        return (true, parents);
    }

    // Locate the nearest ancestor holding a repository marker (`.git` or
    // `.jj`), stopping as soon as one is found so outer ancestors are not
    // probed for repository markers.
    let mut repo_match: Option<(usize, Option<std::fs::Metadata>)> = None;
    for (idx, dir) in ancestors.iter().enumerate() {
        if let Ok(meta) = std::fs::metadata(dir.join(".git"))
            && (meta.is_dir() || meta.is_file())
        {
            repo_match = Some((idx, Some(meta)));
            break;
        }
        if let Ok(meta) = std::fs::metadata(dir.join(".jj"))
            && (meta.is_dir() || meta.is_file())
        {
            repo_match = Some((idx, None));
            break;
        }
    }

    let Some((repo_idx, dot_git_meta)) = repo_match else {
        // Outside any repository: the checked root is the boundary and no
        // ignore file above it applies (ADR-0028).
        return (false, Vec::new());
    };

    let mut parents = Vec::new();
    for (idx, &dir) in ancestors.iter().enumerate() {
        let dot_ignore = {
            let path = dir.join(".ignore");
            read_parent_file(path.clone(), warnings)
                .and_then(|content| gitignore_from(dir, &path, &content))
        };
        let git_ignore = if idx <= repo_idx {
            let path = dir.join(".gitignore");
            read_parent_file(path.clone(), warnings)
                .and_then(|content| gitignore_from(dir, &path, &content))
        } else {
            None
        };
        let exclude = if idx == repo_idx {
            dot_git_meta
                .as_ref()
                .and_then(|meta| read_parent_git_exclude(dir, meta, warnings))
        } else {
            None
        };
        let repository = idx == repo_idx;
        if dot_ignore.is_some() || git_ignore.is_some() || exclude.is_some() || repository {
            parents.push(DirIgnores {
                dot_ignore,
                git_ignore,
                exclude,
                repository,
            });
        }
    }
    (true, parents)
}

fn read_parent_git_exclude(
    dir: &Path,
    dot_git_meta: &std::fs::Metadata,
    warnings: &mut Vec<String>,
) -> Option<Gitignore> {
    let dot_git = dir.join(".git");
    if dot_git_meta.is_dir() {
        let path = dot_git.join("info/exclude");
        let content = read_parent_file(path.clone(), warnings)?;
        return gitignore_from(dir, &path, &content);
    }
    if dot_git_meta.is_file() {
        let dot_git_bytes = read_parent_file(dot_git, warnings)?;
        let first = ignore_files::first_line_from_bytes(&dot_git_bytes)?;
        let git_dir = PathBuf::from(first.strip_prefix("gitdir: ")?);
        let commondir_bytes = read_parent_file(git_dir.join("commondir"), warnings)?;
        let common = ignore_files::first_line_from_bytes(&commondir_bytes)?;
        let common_dir = if common.starts_with('.') {
            git_dir.join(common)
        } else {
            PathBuf::from(common)
        };
        let exclude_path = common_dir.join("info/exclude");
        let content = read_parent_file(exclude_path.clone(), warnings)?;
        return gitignore_from(dir, &exclude_path, &content);
    }
    None
}

fn read_parent_file(path: PathBuf, warnings: &mut Vec<String>) -> Option<Vec<u8>> {
    match ignore_files::read_by_path(path) {
        IgnoreFileRead::Bytes(bytes) => Some(bytes),
        IgnoreFileRead::Absent => None,
        IgnoreFileRead::Refused(refused) => {
            tracing::warn!(
                path = %refused.path().display(),
                reason = %refused.reason(),
                "skipping ignore file above root"
            );
            warnings.push(refused.into_warning());
            None
        }
    }
}

/// Whether `err` is a kernel `ELOOP` (too many levels of symbolic links),
/// produced by `canonicalize()` / `metadata()` on a direct symlink cycle
/// (`ln -s loop loop`, mutual links). `ErrorKind::FilesystemLoop` is still
/// unstable in Rust 1.88, so match the platform errno directly.
#[cfg(unix)]
fn is_eloop_io(err: &std::io::Error) -> bool {
    const ELOOP: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        40
    } else {
        62
    };
    err.raw_os_error() == Some(ELOOP)
}

#[cfg(not(unix))]
fn is_eloop_io(_err: &std::io::Error) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::walker::{in_repository, walk};

    fn td() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("alint-walker-parallel-")
            .tempdir()
            .unwrap()
    }

    fn touch(root: &Path, rel: &str, content: &[u8]) {
        let abs = root.join(rel);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(abs, content).unwrap();
    }

    /// Reference walk using `ignore::WalkBuilder::build_parallel` with the
    /// exact flags alint used before #573, used by parity tests to confirm the
    /// owned parallel walker yields identical entries, directory flags, and
    /// sizes (including exact-case ignore-file matching on case-insensitive
    /// filesystems, which `WalkParallel` uses whereas sequential `Walk` probes
    /// by path).
    fn reference_ignore_walk(root: &Path, opts: &WalkOptions) -> Vec<(String, bool, u64)> {
        let mut builder = ignore::WalkBuilder::new(root);
        builder
            .standard_filters(opts.respect_gitignore)
            .git_global(opts.uses_global_git_excludes())
            .hidden(false)
            .follow_links(true);
        if opts.respect_gitignore {
            let bounded = in_repository(root);
            builder.require_git(bounded).parents(bounded);
        }
        builder.overrides(build_overrides(root, opts).unwrap());
        let canonical_root = root.canonicalize().ok();
        builder.filter_entry(move |entry| {
            if !entry.path_is_symlink() {
                return true;
            }
            match (&canonical_root, entry.path().canonicalize()) {
                (Some(real_root), Ok(target)) => {
                    crate::pathsafe::walk_follows_symlink(real_root, &target)
                }
                _ => false,
            }
        });
        let out: Arc<Mutex<Vec<(String, bool, u64)>>> = Arc::new(Mutex::new(Vec::new()));
        let root_buf = root.to_path_buf();
        builder.build_parallel().run(|| {
            let out = Arc::clone(&out);
            let root = root_buf.clone();
            Box::new(move |item| {
                if let Ok(entry) = item
                    && let Ok(rel) = entry.path().strip_prefix(&root)
                    && !rel.as_os_str().is_empty()
                    && let Ok(meta) = entry.metadata()
                    && (meta.is_dir() || meta.is_file())
                {
                    out.lock().unwrap().push((
                        rel.display().to_string().replace('\\', "/"),
                        meta.is_dir(),
                        if meta.is_file() { meta.len() } else { 0 },
                    ));
                }
                ignore::WalkState::Continue
            })
        });
        let mut out = out.lock().unwrap().clone();
        out.sort();
        out
    }

    #[test]
    fn owned_walker_matches_ignore_walk_builder_on_complex_tree() {
        let tmp = td();
        let outer = tmp.path().join("outer");
        touch(&outer, ".ignore", b"*.outer_ignored\n");
        touch(&outer, ".gitignore", b"*\n");
        let repo = outer.join("repo");
        touch(
            &repo,
            ".git/info/exclude",
            b"excluded.txt\nreincluded.txt\n",
        );
        touch(
            &repo,
            ".gitignore",
            b".env\nbuild/\n*.log\n!scratch.txt\n!reincluded.txt\n",
        );
        touch(&repo, ".ignore", b"scratch.txt\n");
        touch(&repo, "logs/.gitignore", b"*\n!.gitignore\n!keep.txt\n");
        touch(&repo, "d/e/.gitignore", b"!keep.log\n");
        touch(&repo, "nested/.git/info/exclude", b"/nsecret.txt\n");
        touch(&repo, "nested/.gitignore", b"*.ntmp\n");
        let shared = tmp.path().join("shared.git");
        touch(&shared, "info/exclude", b"/wt_secret.md\n");
        touch(&shared, "worktrees/wt/commondir", b"../..\n");
        touch(
            &repo,
            "wt/.git",
            format!("gitdir: {}\n", shared.join("worktrees/wt").display()).as_bytes(),
        );
        touch(&repo, "case/.GITIGNORE", b"*.keep_case\n");
        for rel in [
            ".env",
            "a.log",
            "a.outer_ignored",
            "build/out.txt",
            "tools/build",
            "scratch.txt",
            "excluded.txt",
            "reincluded.txt",
            "kept.txt",
            "logs/keep.txt",
            "logs/drop.txt",
            "d/e/keep.log",
            "d/e/drop.log",
            "nested/a.log",
            "nested/nsecret.txt",
            "nested/deeper/nsecret.txt",
            "nested/x.ntmp",
            "wt/wt_secret.md",
            "wt/kept.md",
            "case/file.keep_case",
            "vendor/lib.rs",
        ] {
            touch(&repo, rel, b"content");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("kept.txt", repo.join("link_in.txt")).unwrap();
            std::os::unix::fs::symlink("tools", repo.join("tools_alias")).unwrap();
            std::os::unix::fs::symlink(shared.join("info/exclude"), repo.join("escape.txt"))
                .unwrap();
            std::os::unix::fs::symlink("missing", repo.join("dangling")).unwrap();
            std::os::unix::fs::symlink(".", repo.join("self_loop")).unwrap();
        }

        for respect_gitignore in [true, false] {
            let opts = WalkOptions {
                respect_gitignore,
                extra_ignores: vec!["vendor/**".into()],
                global_git_excludes: false,
            };
            for target in [&repo, &repo.join("d")] {
                let owned = walk(target, &opts).unwrap();
                let mut got: Vec<(String, bool, u64)> = owned
                    .entries
                    .iter()
                    .map(|e| {
                        (
                            e.path.display().to_string().replace('\\', "/"),
                            e.is_dir,
                            e.size,
                        )
                    })
                    .collect();
                got.sort();
                let expected = reference_ignore_walk(target, &opts);
                assert_eq!(
                    got,
                    expected,
                    "parity mismatch at {} (respect_gitignore={respect_gitignore})",
                    target.display()
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn bad_ignore_files_above_root_warn_and_skip_while_inside_root_fail() {
        use std::os::unix::fs::PermissionsExt as _;

        // 1. Non-regular (symlink to /dev/zero) `.gitignore` in enclosing repo
        //    above the checked subdirectory: skipped with a visible warning (#568).
        let tmp = td();
        let repo = tmp.path().join("mono");
        touch(&repo, ".git/HEAD", b"ref: refs/heads/main\n");
        std::os::unix::fs::symlink("/dev/zero", repo.join(".gitignore")).unwrap();
        let pkg = repo.join("pkg");
        touch(&pkg, "README.md", b"hi\n");
        let idx = walk(&pkg, &WalkOptions::test_default()).unwrap();
        assert!(idx.contains_file(Path::new("README.md")));
        assert!(
            idx.walk_warnings()
                .iter()
                .any(|w| w.contains(".gitignore") && w.contains("not a regular file")),
            "expected warning naming parent .gitignore: {:?}",
            idx.walk_warnings()
        );

        // 2. Oversize (> 32 MiB) `.ignore` above a repository: skipped with a
        //    visible warning (#568).
        let tmp2 = td();
        let outer = tmp2.path().join("outer");
        std::fs::create_dir_all(&outer).unwrap();
        std::fs::File::create(outer.join(".ignore"))
            .unwrap()
            .set_len(2 * ignore_files::MAX_IGNORE_FILE_BYTES)
            .unwrap();
        let inner = outer.join("repo");
        touch(&inner, ".git/HEAD", b"ref: refs/heads/main\n");
        touch(&inner, "lib.rs", b"fn f() {}\n");
        let idx2 = walk(&inner, &WalkOptions::test_default()).unwrap();
        assert!(idx2.contains_file(Path::new("lib.rs")));
        assert!(
            idx2.walk_warnings()
                .iter()
                .any(|w| w.contains(".ignore") && w.contains("32 MiB")),
            "expected warning naming oversize parent .ignore: {:?}",
            idx2.walk_warnings()
        );

        // 3. Unreadable `.gitignore` above vs inside root (#568).
        let tmp3 = td();
        let mono3 = tmp3.path().join("mono");
        touch(&mono3, ".git/HEAD", b"ref: refs/heads/main\n");
        touch(&mono3, ".gitignore", b"*.log\n");
        let pkg3 = mono3.join("pkg");
        touch(&pkg3, "a.rs", b"x\n");
        std::fs::set_permissions(
            mono3.join(".gitignore"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        let running_as_root = std::fs::read(mono3.join(".gitignore")).is_ok();
        if !running_as_root {
            let idx3 = walk(&pkg3, &WalkOptions::test_default()).unwrap();
            assert!(idx3.contains_file(Path::new("a.rs")));
            assert!(
                idx3.walk_warnings()
                    .iter()
                    .any(|w| w.contains(".gitignore")),
                "expected warning for unreadable parent .gitignore: {:?}",
                idx3.walk_warnings()
            );
            // Same unreadable `.gitignore` INSIDE the checked root fails strictly.
            let err = walk(&mono3, &WalkOptions::test_default()).unwrap_err();
            assert!(
                err.to_string()
                    .contains("the walk does not read the ignore file"),
                "expected strict refusal inside root, got: {err}"
            );
        }
        std::fs::set_permissions(
            mono3.join(".gitignore"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
    }
}
