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

use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
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

impl WalkOutput {
    fn empty() -> Self {
        Self {
            entries: Vec::new(),
            escaping_symlinks: Vec::new(),
            followed_symlinks: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

/// One node in the immutable in-tree ignore chain. Only allocated for
/// directories that actually contribute an ignore file or a repository
/// boundary marker (`.git` / `.jj`), so directories without ignore files
/// borrow their parent's chain with zero heap allocation.
struct IgnoreNode {
    dir: DirIgnores,
    parent: Option<Arc<IgnoreNode>>,
}

impl Drop for IgnoreNode {
    fn drop(&mut self) {
        let mut cur = self.parent.take();
        while let Some(arc) = cur {
            match Arc::try_unwrap(arc) {
                Ok(mut node) => cur = node.parent.take(),
                Err(_) => break,
            }
        }
    }
}

/// One node in a parent-linked canonical ancestor chain used once a symlink
/// has been followed on a branch.
struct ExplicitAncestor {
    canonical: PathBuf,
    parent: Option<Arc<ExplicitAncestor>>,
}

impl Drop for ExplicitAncestor {
    fn drop(&mut self) {
        let mut cur = self.parent.take();
        while let Some(arc) = cur {
            match Arc::try_unwrap(arc) {
                Ok(mut node) => cur = node.parent.take(),
                Err(_) => break,
            }
        }
    }
}

/// Canonical ancestor directory chain for symlink-loop detection.
///
/// While no symlink has been followed on the path from the walk root
/// (`Linear`), every visited ancestor's canonical path is a prefix between
/// `canonical_walk_root` and `canonical_walk_root.join(rel_dir)`, requiring
/// zero allocations and zero syscalls per normal directory. Once a symlinked
/// directory is entered, we switch to `Explicit`, a parent-linked chain where
/// `push_normal` and `push_symlink` are both O(1), tracking each visited
/// canonical directory on that branch so mutual directory symlinks and
/// `alias/deep/up -> ..` loops are detected without blocking distinct aliases
/// (`real/` and `alias/`) from both being walked.
#[derive(Clone)]
enum Ancestors {
    Linear,
    Explicit(Option<Arc<ExplicitAncestor>>),
}

impl Ancestors {
    fn contains(&self, canonical_walk_root: Option<&Path>, rel_dir: &Path, target: &Path) -> bool {
        match self {
            Self::Linear => {
                let Some(root) = canonical_walk_root else {
                    return false;
                };
                if !target.starts_with(root) {
                    return false;
                }
                let current: Cow<'_, Path> = if rel_dir.as_os_str().is_empty() {
                    Cow::Borrowed(root)
                } else {
                    Cow::Owned(root.join(rel_dir))
                };
                current.starts_with(target)
            }
            Self::Explicit(head) => {
                let mut cur = head.as_deref();
                while let Some(node) = cur {
                    if node.canonical == target {
                        return true;
                    }
                    cur = node.parent.as_deref();
                }
                false
            }
        }
    }

    fn push_normal(&self, name: &OsStr) -> Self {
        match self {
            Self::Linear => Self::Linear,
            Self::Explicit(head) => {
                let next = head.as_ref().map(|prev| {
                    Arc::new(ExplicitAncestor {
                        canonical: prev.canonical.join(name),
                        parent: Some(Arc::clone(prev)),
                    })
                });
                Self::Explicit(next)
            }
        }
    }

    fn push_symlink(
        &self,
        canonical_walk_root: Option<&Path>,
        rel_dir: &Path,
        target: PathBuf,
    ) -> Self {
        let parent = match self {
            Self::Linear => canonical_walk_root.map(|root| {
                let mut cur_path = root.to_path_buf();
                let mut head = Arc::new(ExplicitAncestor {
                    canonical: cur_path.clone(),
                    parent: None,
                });
                for comp in rel_dir.components() {
                    if let Component::Normal(name) = comp {
                        cur_path.push(name);
                        head = Arc::new(ExplicitAncestor {
                            canonical: cur_path.clone(),
                            parent: Some(head),
                        });
                    }
                }
                head
            }),
            Self::Explicit(head) => head.clone(),
        };
        Self::Explicit(Some(Arc::new(ExplicitAncestor {
            canonical: target,
            parent,
        })))
    }
}

/// Shared read-only state and synchronized sinks for one parallel walk.
struct WalkCtx {
    canonical_walk_root: Option<PathBuf>,
    canonical_confinement_root: Option<PathBuf>,
    /// Compiled `ignore::overrides::Override` when `extra_ignores` is non-empty;
    /// `None` uses the exact-name `.git` fast path with zero globset build or
    /// matching overhead.
    overrides: Option<Override>,
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
    /// Per-worker entry buckets indexed by `rayon::current_thread_index()` so
    /// concurrent flushes avoid contending on one global mutex.
    buckets: Vec<Mutex<Vec<FileEntry>>>,
}

impl WalkCtx {
    fn new(
        canonical_walk_root: Option<PathBuf>,
        canonical_confinement_root: Option<PathBuf>,
        overrides: Option<Override>,
        respect_gitignore: bool,
        bounded: bool,
        parents: Vec<DirIgnores>,
        global: Option<Gitignore>,
    ) -> Self {
        let bucket_count = rayon::current_num_threads().max(1);
        let buckets = (0..bucket_count).map(|_| Mutex::new(Vec::new())).collect();
        Self {
            canonical_walk_root,
            canonical_confinement_root,
            overrides,
            respect_gitignore,
            bounded,
            parents,
            global,
            cancelled: AtomicBool::new(false),
            error_slot: Mutex::new(None),
            escaping: Mutex::new(Vec::new()),
            followed: Mutex::new(Vec::new()),
            buckets,
        }
    }

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
        let idx = rayon::current_thread_index().unwrap_or(0) % self.buckets.len();
        if let Ok(mut bucket) = self.buckets[idx].lock() {
            if bucket.is_empty() {
                *bucket = local;
            } else {
                bucket.extend(local);
            }
        }
    }

    fn finish(self, warnings: Vec<String>) -> Result<WalkOutput> {
        if let Some(err) = self
            .error_slot
            .into_inner()
            .expect("walker error slot lock")
        {
            return Err(err);
        }

        let chunks: Vec<Vec<FileEntry>> = self
            .buckets
            .into_iter()
            .map(|b| b.into_inner().expect("walker bucket lock"))
            .filter(|v| !v.is_empty())
            .collect();
        let mut entries = if chunks.len() == 1 {
            chunks.into_iter().next().unwrap_or_default()
        } else {
            let total_entries: usize = chunks.iter().map(Vec::len).sum();
            let mut merged = Vec::with_capacity(total_entries);
            for chunk in chunks {
                merged.extend(chunk);
            }
            merged
        };
        entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        entries.dedup_by(|a, b| a.path == b.path);

        let mut escaping_symlinks = self.escaping.into_inner().expect("walker escaping lock");
        escaping_symlinks.sort_unstable();
        escaping_symlinks.dedup();

        let mut followed_symlinks = self.followed.into_inner().expect("walker followed lock");
        followed_symlinks.sort_unstable();
        followed_symlinks.dedup();

        Ok(WalkOutput {
            entries,
            escaping_symlinks,
            followed_symlinks,
            warnings,
        })
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
        match &self.overrides {
            Some(overrides) => match overrides.matched(abs, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            },
            None => {
                if abs.file_name() == Some(OsStr::new(".git")) {
                    return true;
                }
            }
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
    name: OsString,
    entry: std::fs::DirEntry,
    file_type: std::fs::FileType,
}

/// A subdirectory queued for descent.
struct PendingDir {
    rel: PathBuf,
    abs: PathBuf,
    ancestors: Ancestors,
}

/// An already-opened subdirectory ready to be inspected and descended.
struct ReadyDir {
    rel: PathBuf,
    abs: PathBuf,
    listing: Vec<RawChild>,
    chain: Option<Arc<IgnoreNode>>,
    ancestors: Ancestors,
}

/// Shared walk initialization anchored at `root` and confined to
/// `confinement_root`. Returns `Ok(None)` when `root` is a regular file
/// (preserving parity with `ignore::WalkBuilder`).
#[allow(clippy::type_complexity)]
fn init_walk(
    root: &Path,
    confinement_root: &Path,
    opts: &WalkOptions,
) -> Result<Option<(WalkCtx, Vec<RawChild>, Option<Arc<IgnoreNode>>, Vec<String>)>> {
    let overrides = if opts.extra_ignores.is_empty() {
        None
    } else {
        Some(build_overrides(root, opts)?)
    };

    // Canonicalize once up front (reused for parent ignores, symlink
    // confinement, and cycle detection).
    let canonical_confinement_root = confinement_root.canonicalize().ok();
    let canonical_walk_root = if confinement_root == root {
        canonical_confinement_root.clone()
    } else {
        root.canonicalize().ok()
    };

    // List the walk root first: if the root itself is missing or unreadable,
    // fail immediately; if it is a regular file, return empty (matching
    // `ignore::WalkBuilder`); otherwise inspect its ignore files and repository
    // markers directly from its listing with zero extra `stat` probes.
    let root_listing = match read_directory_listing(root) {
        Ok(listing) => listing,
        Err(Error::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotADirectory
                || std::fs::metadata(root).is_ok_and(|m| m.is_file()) =>
        {
            return Ok(None);
        }
        Err(err) => return Err(err),
    };

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

    let ctx = WalkCtx::new(
        canonical_walk_root,
        canonical_confinement_root,
        overrides,
        opts.respect_gitignore,
        bounded,
        parents,
        global,
    );
    Ok(Some((ctx, root_listing, root_ignores, warnings)))
}

/// Walk `root`, confining followed symlinks to `confinement_root`.
pub(super) fn walk_tree(
    root: &Path,
    confinement_root: &Path,
    opts: &WalkOptions,
) -> Result<WalkOutput> {
    let Some((ctx, root_listing, root_ignores, warnings)) =
        init_walk(root, confinement_root, opts)?
    else {
        return Ok(WalkOutput::empty());
    };

    let mut initial_entries = Vec::new();
    let root_dir = ReadyDir {
        rel: PathBuf::new(),
        abs: root.to_path_buf(),
        listing: root_listing,
        chain: root_ignores,
        ancestors: Ancestors::Linear,
    };
    if rayon::current_num_threads() <= 1 {
        process_directory_sequential(&ctx, root_dir, &mut initial_entries);
    } else {
        rayon::scope(|scope| {
            process_directory_parallel(&ctx, scope, root_dir, &mut initial_entries);
        });
    }
    ctx.flush_entries(initial_entries);
    ctx.finish(warnings)
}

/// Cached listing names and ignore chain for an intermediate directory along
/// the path from `root` to a requested subtree.
struct CachedDir {
    children: HashMap<OsString, std::fs::FileType>,
    chain: Option<Arc<IgnoreNode>>,
}

/// Walk only `subtrees` (normalized, deduplicated, root-relative paths) under
/// `root`, anchored at `root` with a single [`WalkCtx`] so exact on-disk
/// casing, lexical ancestor ignores, repository boundaries, `extra_ignores`,
/// cycle detection, strict inside-root ignore refusals, walk warnings, and
/// symlink side-lists all match [`walk_tree`] (#584, #573).
#[allow(clippy::too_many_lines)]
pub(super) fn walk_subtrees_from(
    root: &Path,
    subtrees: &[PathBuf],
    opts: &WalkOptions,
) -> Result<WalkOutput> {
    if subtrees.is_empty() {
        return Ok(WalkOutput::empty());
    }
    if subtrees.iter().any(|s| s.as_os_str().is_empty()) {
        return walk_tree(root, root, opts);
    }

    let Some((ctx, root_listing, root_ignores, warnings)) = init_walk(root, root, opts)? else {
        return Ok(WalkOutput::empty());
    };

    let root_children: HashMap<OsString, std::fs::FileType> = root_listing
        .into_iter()
        .map(|c| (c.name, c.file_type))
        .collect();
    let mut dir_cache: HashMap<PathBuf, CachedDir> = HashMap::new();
    dir_cache.insert(
        PathBuf::new(),
        CachedDir {
            children: root_children,
            chain: root_ignores,
        },
    );

    let mut initial_entries = Vec::new();
    let mut queued_roots: Vec<ReadyDir> = Vec::new();

    'subtree: for sub in subtrees {
        let components: Vec<&OsStr> = sub
            .components()
            .filter_map(|c| match c {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .collect();
        if components.is_empty() {
            continue;
        }

        let mut cur_rel = PathBuf::new();
        let mut cur_abs = root.to_path_buf();
        let mut ancestors = Ancestors::Linear;
        let last_idx = components.len() - 1;

        for (idx, comp) in components.into_iter().enumerate() {
            let is_last = idx == last_idx;
            // Exact listing spelling check: on case-insensitive filesystems,
            // a requested prefix like `docs` never aliases an on-disk `Docs`
            // directory (preventing `generated_file_fresh` from treating
            // committed files under `Docs/` as newly created under `docs/`).
            let (ft, chain) = {
                let Some(cached) = dir_cache.get(&cur_rel) else {
                    continue 'subtree;
                };
                let Some(&ft) = cached.children.get(comp) else {
                    continue 'subtree;
                };
                (ft, cached.chain.clone())
            };

            let next_rel = cur_rel.join(comp);
            let next_abs = cur_abs.join(comp);
            let next_canonical = (!ctx.parents.is_empty())
                .then(|| ctx.canonical_walk_root.as_ref().map(|r| r.join(&next_rel)))
                .flatten();

            if ft.is_symlink() {
                let target = match next_abs.canonicalize() {
                    Ok(target) => target,
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                        ) || is_eloop_io(&err) =>
                    {
                        continue 'subtree;
                    }
                    Err(_) => {
                        if !ctx.is_ignored(
                            &next_abs,
                            next_canonical.as_deref(),
                            false,
                            chain.as_deref(),
                        ) {
                            ctx.push_escaping(Arc::from(next_rel.as_path()));
                        }
                        continue 'subtree;
                    }
                };
                let meta = match std::fs::metadata(&target) {
                    Ok(meta) => meta,
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                        ) || is_eloop_io(&err) =>
                    {
                        continue 'subtree;
                    }
                    Err(_) => {
                        if !ctx.is_ignored(
                            &next_abs,
                            next_canonical.as_deref(),
                            false,
                            chain.as_deref(),
                        ) {
                            ctx.push_escaping(Arc::from(next_rel.as_path()));
                        }
                        continue 'subtree;
                    }
                };
                let is_dir = meta.is_dir();
                if ctx.is_ignored(
                    &next_abs,
                    next_canonical.as_deref(),
                    is_dir,
                    chain.as_deref(),
                ) {
                    continue 'subtree;
                }
                let keep = ctx
                    .canonical_confinement_root
                    .as_deref()
                    .is_some_and(|real_root| {
                        crate::pathsafe::walk_follows_symlink(real_root, &target)
                    });
                if !keep {
                    ctx.push_escaping(Arc::from(next_rel.as_path()));
                    continue 'subtree;
                }
                if is_dir
                    && ancestors.contains(ctx.canonical_walk_root.as_deref(), &cur_rel, &target)
                {
                    continue 'subtree;
                }
                if is_dir {
                    let rel_arc: Arc<Path> = Arc::from(next_rel.as_path());
                    ctx.push_followed(Arc::clone(&rel_arc));
                    let next_ancestors = ancestors.push_symlink(
                        ctx.canonical_walk_root.as_deref(),
                        &cur_rel,
                        target,
                    );
                    if is_last {
                        let Some((listing, next_chain)) =
                            open_subtree_dir(&ctx, &next_abs, chain.as_ref())?
                        else {
                            continue 'subtree;
                        };
                        initial_entries.push(FileEntry {
                            path: rel_arc,
                            is_dir: true,
                            size: 0,
                        });
                        queued_roots.push(ReadyDir {
                            rel: next_rel,
                            abs: next_abs,
                            listing,
                            chain: next_chain,
                            ancestors: next_ancestors,
                        });
                    } else {
                        if !dir_cache.contains_key(&next_rel) {
                            let Some((listing, next_chain)) =
                                open_subtree_dir(&ctx, &next_abs, chain.as_ref())?
                            else {
                                continue 'subtree;
                            };
                            let children =
                                listing.into_iter().map(|c| (c.name, c.file_type)).collect();
                            dir_cache.insert(
                                next_rel.clone(),
                                CachedDir {
                                    children,
                                    chain: next_chain,
                                },
                            );
                        }
                        cur_rel = next_rel;
                        cur_abs = next_abs;
                        ancestors = next_ancestors;
                    }
                } else if meta.is_file() {
                    if is_last {
                        let rel_arc: Arc<Path> = Arc::from(next_rel.as_path());
                        ctx.push_followed(Arc::clone(&rel_arc));
                        initial_entries.push(FileEntry {
                            path: rel_arc,
                            is_dir: false,
                            size: meta.len(),
                        });
                    }
                    continue 'subtree;
                } else {
                    continue 'subtree;
                }
            } else if ft.is_dir() {
                if ctx.is_ignored(&next_abs, next_canonical.as_deref(), true, chain.as_deref()) {
                    continue 'subtree;
                }
                let next_ancestors = ancestors.push_normal(comp);
                if is_last {
                    let Some((listing, next_chain)) =
                        open_subtree_dir(&ctx, &next_abs, chain.as_ref())?
                    else {
                        continue 'subtree;
                    };
                    initial_entries.push(FileEntry {
                        path: Arc::from(next_rel.as_path()),
                        is_dir: true,
                        size: 0,
                    });
                    queued_roots.push(ReadyDir {
                        rel: next_rel,
                        abs: next_abs,
                        listing,
                        chain: next_chain,
                        ancestors: next_ancestors,
                    });
                } else {
                    if !dir_cache.contains_key(&next_rel) {
                        let Some((listing, next_chain)) =
                            open_subtree_dir(&ctx, &next_abs, chain.as_ref())?
                        else {
                            continue 'subtree;
                        };
                        let children = listing.into_iter().map(|c| (c.name, c.file_type)).collect();
                        dir_cache.insert(
                            next_rel.clone(),
                            CachedDir {
                                children,
                                chain: next_chain,
                            },
                        );
                    }
                    cur_rel = next_rel;
                    cur_abs = next_abs;
                    ancestors = next_ancestors;
                }
            } else if ft.is_file() {
                if !is_last {
                    // Regular file at an intermediate segment (`file/child`):
                    // cleanly absent rather than surfacing `ENOTDIR`.
                    continue 'subtree;
                }
                if !ctx.is_ignored(
                    &next_abs,
                    next_canonical.as_deref(),
                    false,
                    chain.as_deref(),
                ) {
                    match std::fs::metadata(&next_abs) {
                        Ok(meta) if meta.is_file() => {
                            initial_entries.push(FileEntry {
                                path: Arc::from(next_rel.as_path()),
                                is_dir: false,
                                size: meta.len(),
                            });
                        }
                        Ok(_) => {}
                        Err(err)
                            if matches!(
                                err.kind(),
                                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                            ) => {}
                        Err(source) => {
                            return Err(Error::Io {
                                path: next_abs,
                                source,
                            });
                        }
                    }
                }
                continue 'subtree;
            } else {
                // Special file (FIFO, device, socket): silently skipped.
                continue 'subtree;
            }
        }
    }

    if !queued_roots.is_empty() {
        if rayon::current_num_threads() <= 1 {
            for ready in queued_roots {
                process_directory_sequential(&ctx, ready, &mut initial_entries);
            }
        } else {
            rayon::scope(|scope| {
                let tail = queued_roots.pop();
                for ready in queued_roots {
                    if ctx.is_cancelled() {
                        break;
                    }
                    scope.spawn(|inner_scope| {
                        let mut task_entries = Vec::new();
                        process_directory_parallel(&ctx, inner_scope, ready, &mut task_entries);
                        ctx.flush_entries(task_entries);
                    });
                }
                if let Some(tail) = tail
                    && !ctx.is_cancelled()
                {
                    process_directory_parallel(&ctx, scope, tail, &mut initial_entries);
                }
            });
        }
    }

    ctx.flush_entries(initial_entries);
    ctx.finish(warnings)
}

/// Listing and updated ignore chain produced by opening one subdirectory.
type OpenedSubdir = (Vec<RawChild>, Option<Arc<IgnoreNode>>);

/// Open an in-root directory along a subtree path and load its ignore chain
/// strictly (matching [`open_subdir`]). Returns `Ok(None)` if the directory
/// vanished or ceased to be a directory concurrently.
fn open_subtree_dir(
    ctx: &WalkCtx,
    abs: &Path,
    parent_chain: Option<&Arc<IgnoreNode>>,
) -> Result<Option<OpenedSubdir>> {
    let listing = match read_directory_listing(abs) {
        Ok(listing) => listing,
        Err(Error::Io { source, .. })
            if matches!(
                source.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(err) => return Err(err),
    };
    let next_chain = if ctx.respect_gitignore {
        match load_dir_ignores_from_listing(abs, &listing)? {
            Some(ignores) => Some(Arc::new(IgnoreNode {
                dir: ignores,
                parent: parent_chain.cloned(),
            })),
            None => parent_chain.cloned(),
        }
    } else {
        None
    };
    Ok(Some((listing, next_chain)))
}

/// Read all entries of `dir` along with their `FileType` (from `d_type` on
/// Unix / `WIN32_FIND_DATA` on Windows: zero extra `stat` calls on common
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
                rel_dir,
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
    rel_dir: &Path,
    rel: &Path,
    abs: &Path,
    canonical_abs: Option<&Path>,
    chain: Option<&IgnoreNode>,
    ancestors: &Ancestors,
    local_entries: &mut Vec<FileEntry>,
    subdirs: &mut Vec<PendingDir>,
) {
    // Resolve the symlink target first. A broken (dangling) symlink, an
    // `ENOTDIR` through a non-directory segment, or a kernel `ELOOP` direct
    // self-cycle is skipped without aborting the walk, matching
    // `ignore::WalkBuilder` with `follow_links(true)`.
    let target = match abs.canonicalize() {
        Ok(target) => target,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) || is_eloop_io(&err) =>
        {
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
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) || is_eloop_io(&err) =>
        {
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
    if is_dir && ancestors.contains(ctx.canonical_walk_root.as_deref(), rel_dir, &target) {
        // Ancestor symlink cycle (`loop -> .`, `sub/up -> ..`, mutual directory
        // symlinks): skip cleanly without indexing or recursing.
        return;
    }
    if is_dir {
        let rel_arc: Arc<Path> = Arc::from(rel);
        ctx.push_followed(Arc::clone(&rel_arc));
        let next_ancestors =
            ancestors.push_symlink(ctx.canonical_walk_root.as_deref(), rel_dir, target);
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
        let rel_arc: Arc<Path> = Arc::from(rel);
        ctx.push_followed(Arc::clone(&rel_arc));
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
        Err(Error::Io { source, .. })
            if matches!(
                source.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
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

/// Single-threaded iterative traversal using an explicit LIFO stack so deep
/// directory trees never overflow the call stack.
fn process_directory_sequential(ctx: &WalkCtx, root: ReadyDir, local_entries: &mut Vec<FileEntry>) {
    let mut stack: Vec<(PendingDir, Option<Arc<IgnoreNode>>)> = Vec::new();
    let subdirs = inspect_children(
        ctx,
        &root.rel,
        &root.abs,
        root.listing,
        root.chain.as_deref(),
        &root.ancestors,
        local_entries,
    );
    for sub in subdirs.into_iter().rev() {
        stack.push((sub, root.chain.clone()));
    }
    while let Some((sub, parent_chain)) = stack.pop() {
        if let Some((sub_listing, sub_chain)) = open_subdir(ctx, &sub, parent_chain.as_ref()) {
            let children = inspect_children(
                ctx,
                &sub.rel,
                &sub.abs,
                sub_listing,
                sub_chain.as_deref(),
                &sub.ancestors,
                local_entries,
            );
            for child in children.into_iter().rev() {
                stack.push((child, sub_chain.clone()));
            }
        }
    }
}

/// Work-stealing parallel traversal over `rayon::scope`. Loops iteratively on
/// the last subdirectory (O(1) call-stack depth even on arbitrarily deep trees)
/// and spawns sibling subtrees onto the pool, avoiding spawns altogether for
/// leaf and single-child directories.
fn process_directory_parallel<'scope>(
    ctx: &'scope WalkCtx,
    scope: &rayon::Scope<'scope>,
    mut current: ReadyDir,
    local_entries: &mut Vec<FileEntry>,
) {
    loop {
        let mut subdirs = inspect_children(
            ctx,
            &current.rel,
            &current.abs,
            current.listing,
            current.chain.as_deref(),
            &current.ancestors,
            local_entries,
        );
        let Some(tail) = subdirs.pop() else {
            return;
        };
        for sub in subdirs {
            if ctx.is_cancelled() {
                return;
            }
            let parent_chain = current.chain.clone();
            scope.spawn(move |inner_scope| {
                if let Some((sub_listing, sub_chain)) =
                    open_subdir(ctx, &sub, parent_chain.as_ref())
                {
                    let mut task_entries = Vec::new();
                    process_directory_parallel(
                        ctx,
                        inner_scope,
                        ReadyDir {
                            rel: sub.rel,
                            abs: sub.abs,
                            listing: sub_listing,
                            chain: sub_chain,
                            ancestors: sub.ancestors,
                        },
                        &mut task_entries,
                    );
                    ctx.flush_entries(task_entries);
                }
            });
        }
        let Some((tail_listing, tail_chain)) = open_subdir(ctx, &tail, current.chain.as_ref())
        else {
            return;
        };
        current = ReadyDir {
            rel: tail.rel,
            abs: tail.abs,
            listing: tail_listing,
            chain: tail_chain,
            ancestors: tail.ancestors,
        };
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
            std::fs::metadata(dir.join(".jj")).is_ok_and(|m| m.is_dir() || m.is_file())
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
/// marks a repository and, when present, its `info/exclude` matcher. A
/// non-regular `.git` (FIFO, device, socket, or symlink to one) or unreadable
/// symlink fails strictly per ADR-0048 decision 5b (#568).
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
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok((false, None));
            }
            Err(err) => {
                return Err(IgnoreRefusal::io(dot_git, err).into_error());
            }
        };
        if meta.is_dir() {
            let exclude = read_inside_exclude_file(dir, &dot_git.join("info/exclude"))?;
            return Ok((true, exclude));
        }
        if meta.is_file() {
            if meta.len() > ignore_files::MAX_IGNORE_FILE_BYTES {
                return Err(IgnoreRefusal::too_large(dot_git).into_error());
            }
            let bytes = match ignore_files::open_and_read_bounded(dot_git) {
                IgnoreFileRead::Bytes(bytes) => bytes,
                IgnoreFileRead::Absent => return Ok((false, None)),
                IgnoreFileRead::Refused(refused) => return Err(refused.into_error()),
            };
            let exclude = read_inside_worktree_exclude(dir, &bytes)?;
            return Ok((true, exclude));
        }
        return Err(IgnoreRefusal::non_regular(dot_git).into_error());
    }
    Err(IgnoreRefusal::non_regular(dot_git).into_error())
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
/// ignore file (or `.git` marker) above the checked root is skipped with a
/// visible warning recorded in `warnings` (and logged via `tracing::warn!`).
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
    // probed for repository markers. A non-regular or unreadable `.git` above
    // root emits a warning and stops outer `.git` discovery there rather than
    // climbing into an unrelated enclosing repository.
    let mut repo_match: Option<(usize, Option<std::fs::Metadata>)> = None;
    for (idx, dir) in ancestors.iter().enumerate() {
        let dot_git = dir.join(".git");
        match std::fs::metadata(&dot_git) {
            Ok(meta) if meta.is_dir() || meta.is_file() => {
                repo_match = Some((idx, Some(meta)));
                break;
            }
            Ok(_) => {
                record_parent_warning(IgnoreRefusal::non_regular(dot_git), warnings);
                repo_match = Some((idx, None));
                break;
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) => {}
            Err(err) => {
                record_parent_warning(IgnoreRefusal::io(dot_git, err), warnings);
                repo_match = Some((idx, None));
                break;
            }
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

fn record_parent_warning(refused: IgnoreRefusal, warnings: &mut Vec<String>) {
    tracing::warn!(
        path = %refused.path().display(),
        reason = %refused.reason(),
        "skipping ignore file above root"
    );
    warnings.push(refused.into_warning());
}

fn read_parent_file(path: PathBuf, warnings: &mut Vec<String>) -> Option<Vec<u8>> {
    match ignore_files::read_by_path(path) {
        IgnoreFileRead::Bytes(bytes) => Some(bytes),
        IgnoreFileRead::Absent => None,
        IgnoreFileRead::Refused(refused) => {
            record_parent_warning(refused, warnings);
            None
        }
    }
}

/// Whether `err` is a kernel symlink-loop error (`ELOOP` on Unix,
/// `ERROR_CANT_RESOLVE_FILENAME` on Windows), produced by `canonicalize()` /
/// `metadata()` on a direct symlink cycle (`ln -s loop loop`, mutual links).
/// `ErrorKind::FilesystemLoop` is still unstable in Rust 1.88, so match the
/// platform error code directly.
#[cfg(unix)]
fn is_eloop_io(err: &std::io::Error) -> bool {
    const ELOOP: i32 = if cfg!(any(target_os = "linux", target_os = "android")) {
        if cfg!(any(target_arch = "mips", target_arch = "mips64")) {
            90
        } else if cfg!(any(target_arch = "sparc", target_arch = "sparc64")) {
            92
        } else {
            40
        }
    } else if cfg!(any(target_os = "solaris", target_os = "illumos")) {
        90
    } else {
        62
    };
    err.raw_os_error() == Some(ELOOP)
}

#[cfg(windows)]
fn is_eloop_io(err: &std::io::Error) -> bool {
    const ERROR_CANT_RESOLVE_FILENAME: i32 = 1921;
    err.raw_os_error() == Some(ERROR_CANT_RESOLVE_FILENAME)
}

#[cfg(not(any(unix, windows)))]
fn is_eloop_io(_err: &std::io::Error) -> bool {
    false
}

#[cfg(test)]
#[path = "walker_parallel_tests.rs"]
mod tests;
