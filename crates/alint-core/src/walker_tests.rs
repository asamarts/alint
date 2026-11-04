use super::*;

fn td() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("alint-walker-test-")
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

fn paths(idx: &FileIndex) -> Vec<String> {
    // Normalise to forward slashes so assertions can compare
    // against literal `"src/foo.rs"` regardless of host OS.
    // Windows' Path::display() emits `src\foo.rs`.
    idx.entries
        .iter()
        .map(|e| e.path.display().to_string().replace('\\', "/"))
        .collect()
}

#[test]
fn fileindex_files_filters_directories_out() {
    let idx = FileIndex::from_entries(vec![
        FileEntry {
            path: Path::new("a").into(),
            is_dir: true,
            size: 0,
        },
        FileEntry {
            path: Path::new("a/x.rs").into(),
            is_dir: false,
            size: 5,
        },
    ]);
    let files: Vec<_> = idx.files().collect();
    assert_eq!(files.len(), 1);
    assert_eq!(&*files[0].path, Path::new("a/x.rs"));
}

#[cfg(unix)]
#[test]
fn walk_prunes_symlinks_that_escape_the_root() {
    // Security (untrusted-PR content): a committed symlink whose
    // target escapes the repo root must NOT be indexed — otherwise a
    // content rule would read out-of-tree bytes via `root.join(link)`.
    // In-tree symlinks are still followed.
    use std::os::unix::fs::symlink;
    let outside = td();
    touch(outside.path(), "secret.txt", b"TOPSECRET");
    touch(outside.path(), "secretdir/inner.txt", b"INNER");

    let root = td();
    touch(root.path(), "real.txt", b"in-tree");
    symlink(
        outside.path().join("secret.txt"),
        root.path().join("link-file"),
    )
    .unwrap();
    symlink(outside.path(), root.path().join("link-dir")).unwrap();
    symlink(root.path().join("real.txt"), root.path().join("link-in")).unwrap();

    let idx = walk(root.path(), &WalkOptions::default()).unwrap();
    let p = paths(&idx);

    assert!(
        !p.iter().any(|x| x == "link-file"),
        "escaping symlink-to-file was indexed: {p:?}"
    );
    assert!(
        !p.iter().any(|x| x.starts_with("link-dir")),
        "escaping symlink-to-dir was descended into: {p:?}"
    );
    assert!(
        p.iter().any(|x| x == "link-in"),
        "in-tree symlink should still be followed/indexed: {p:?}"
    );
    assert!(p.iter().any(|x| x == "real.txt"), "{p:?}");

    // M4: the escaping symlinks are pruned from `entries` (above) but recorded
    // on the side-list so `no_symlinks` can still flag them.
    let esc: Vec<String> = idx
        .escaping_symlinks()
        .iter()
        .map(|x| x.to_string_lossy().into_owned())
        .collect();
    assert!(
        esc.iter().any(|x| x == "link-file"),
        "escaping symlink-to-file must be on the side-list: {esc:?}"
    );
    assert!(
        esc.iter().any(|x| x == "link-dir"),
        "escaping symlink-to-dir must be on the side-list: {esc:?}"
    );
    assert!(
        !esc.iter().any(|x| x == "link-in"),
        "in-tree symlink must NOT be on the side-list: {esc:?}"
    );
}

#[test]
fn fileindex_dirs_filters_files_out() {
    let idx = FileIndex::from_entries(vec![
        FileEntry {
            path: Path::new("a").into(),
            is_dir: true,
            size: 0,
        },
        FileEntry {
            path: Path::new("a/x.rs").into(),
            is_dir: false,
            size: 5,
        },
    ]);
    let dirs: Vec<_> = idx.dirs().collect();
    assert_eq!(dirs.len(), 1);
    assert_eq!(&*dirs[0].path, Path::new("a"));
}

#[test]
fn fileindex_total_size_sums_files_only() {
    let idx = FileIndex::from_entries(vec![
        FileEntry {
            path: Path::new("a").into(),
            is_dir: true,
            size: 999, // dirs report 0 in `walk`, but defensively excluded here
        },
        FileEntry {
            path: Path::new("a/x.rs").into(),
            is_dir: false,
            size: 100,
        },
        FileEntry {
            path: Path::new("a/y.rs").into(),
            is_dir: false,
            size: 50,
        },
    ]);
    // total_size sums via `files()` so the directory's
    // bogus size is ignored.
    assert_eq!(idx.total_size(), 150);
}

#[test]
fn fileindex_find_file_returns_match_or_none() {
    let idx = FileIndex::from_entries(vec![
        FileEntry {
            path: Path::new("a/x.rs").into(),
            is_dir: false,
            size: 0,
        },
        FileEntry {
            path: Path::new("b").into(),
            is_dir: true,
            size: 0,
        },
    ]);
    assert!(idx.find_file(Path::new("a/x.rs")).is_some());
    assert!(idx.find_file(Path::new("missing.rs")).is_none());
    // find_file filters dirs — querying a known directory
    // returns None.
    assert!(idx.find_file(Path::new("b")).is_none());
}

#[test]
fn fileindex_contains_path_includes_files_and_directories() {
    let idx = FileIndex::from_entries(vec![
        FileEntry {
            path: Path::new("a/x.rs").into(),
            is_dir: false,
            size: 0,
        },
        FileEntry {
            path: Path::new("a").into(),
            is_dir: true,
            size: 0,
        },
    ]);
    assert!(idx.contains_path(Path::new("a/x.rs")));
    assert!(idx.contains_path(Path::new("a")));
    assert!(!idx.contains_path(Path::new("missing")));
}

#[test]
fn walk_excludes_dot_git_directory() {
    let tmp = td();
    touch(tmp.path(), "README.md", b"# demo\n");
    // Fake `.git/` content — should never appear in the index.
    touch(tmp.path(), ".git/config", b"[core]\n");
    touch(tmp.path(), ".git/HEAD", b"ref: refs/heads/main\n");

    let idx = walk(
        tmp.path(),
        &WalkOptions {
            respect_gitignore: false,
            extra_ignores: Vec::new(),
        },
    )
    .unwrap();

    let p = paths(&idx);
    assert!(p.contains(&"README.md".into()), "missing README.md: {p:?}");
    assert!(
        !p.iter().any(|s| s.starts_with(".git")),
        ".git was not excluded: {p:?}",
    );
}

#[test]
fn walk_respects_gitignore_when_enabled() {
    let tmp = td();
    touch(tmp.path(), ".gitignore", b"target/\nignored.txt\n");
    touch(tmp.path(), "src/main.rs", b"fn main() {}\n");
    touch(tmp.path(), "target/debug/build.log", b"junk");
    touch(tmp.path(), "ignored.txt", b"junk");

    let idx = walk(
        tmp.path(),
        &WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        },
    )
    .unwrap();

    let p = paths(&idx);
    assert!(p.contains(&"src/main.rs".into()));
    assert!(
        !p.iter().any(|s| s.starts_with("target")),
        "target/ should be ignored: {p:?}",
    );
    assert!(
        !p.contains(&"ignored.txt".into()),
        "ignored.txt should be filtered: {p:?}",
    );
}

#[test]
fn walk_includes_gitignored_paths_when_respect_gitignore_false() {
    let tmp = td();
    touch(tmp.path(), ".gitignore", b"ignored.txt\n");
    touch(tmp.path(), "ignored.txt", b"x");
    touch(tmp.path(), "kept.txt", b"y");

    let idx = walk(
        tmp.path(),
        &WalkOptions {
            respect_gitignore: false,
            extra_ignores: Vec::new(),
        },
    )
    .unwrap();
    let p = paths(&idx);
    assert!(
        p.contains(&"ignored.txt".into()),
        "respect_gitignore=false should include it: {p:?}",
    );
    assert!(p.contains(&"kept.txt".into()));
}

#[test]
fn walk_applies_extra_ignores_as_excludes() {
    let tmp = td();
    touch(tmp.path(), "src/keep.rs", b"x");
    touch(tmp.path(), "vendor/skip.rs", b"y");

    let idx = walk(
        tmp.path(),
        &WalkOptions {
            respect_gitignore: false,
            extra_ignores: vec!["vendor/**".to_string()],
        },
    )
    .unwrap();
    let p = paths(&idx);
    assert!(p.contains(&"src/keep.rs".into()));
    // `vendor/**` excludes the contents but the dir entry
    // itself may still appear; the rule layer's `path_scope`
    // covers the dir-vs-file distinction. What matters here
    // is that no FILE under vendor/ was indexed.
    let file_paths: Vec<&FileEntry> = idx.files().collect();
    assert!(
        !file_paths.iter().any(|e| e.path.starts_with("vendor")),
        "no file under vendor/ should be indexed: {p:?}",
    );
}

#[test]
fn walk_invalid_extra_ignore_pattern_surfaces_error() {
    let tmp = td();
    touch(tmp.path(), "a.txt", b"x");
    let err = walk(
        tmp.path(),
        &WalkOptions {
            respect_gitignore: false,
            extra_ignores: vec!["[unterminated".to_string()],
        },
    );
    assert!(err.is_err(), "bad pattern should fail: {err:?}");
}

#[test]
fn walk_emits_files_with_correct_size() {
    let tmp = td();
    touch(tmp.path(), "a.txt", &[0u8; 1024]);
    let idx = walk(tmp.path(), &WalkOptions::default()).unwrap();
    let entry = idx
        .files()
        .find(|e| &*e.path == Path::new("a.txt"))
        .expect("a.txt entry");
    assert_eq!(entry.size, 1024);
    assert!(!entry.is_dir);
}

#[test]
fn default_walk_options_respects_gitignore_and_no_extra_ignores() {
    let opts = WalkOptions::default();
    assert!(opts.respect_gitignore);
    assert_eq!(opts.extra_ignores, Vec::<String>::new());
}

#[test]
fn walk_output_is_deterministic_across_runs() {
    // Parallel walker scheduling order is non-deterministic;
    // the deterministic post-sort by relative path is what
    // makes snapshot tests + formatters stable. Two runs over
    // the same tree must produce byte-identical FileIndex
    // outputs — guards against a forgotten sort.
    let tmp = td();
    for i in 0..50 {
        touch(
            tmp.path(),
            &format!("dir_{}/file_{i}.rs", i % 5),
            b"// hello\n",
        );
    }
    let opts = WalkOptions::default();
    let a = walk(tmp.path(), &opts).unwrap();
    let b = walk(tmp.path(), &opts).unwrap();
    assert_eq!(paths(&a), paths(&b));
}

#[test]
fn walk_output_is_alphabetically_sorted() {
    // The post-sort uses path-natural ordering. We don't
    // depend on the exact ordering — just that the output IS
    // sorted, in some total order over PathBuf, so callers
    // can rely on consecutive runs returning the same shape.
    let tmp = td();
    touch(tmp.path(), "z.txt", b"z");
    touch(tmp.path(), "a.txt", b"a");
    touch(tmp.path(), "m.txt", b"m");
    touch(tmp.path(), "sub/b.txt", b"b");
    touch(tmp.path(), "sub/a.txt", b"a");

    let idx = walk(tmp.path(), &WalkOptions::default()).unwrap();
    let actual: Vec<_> = idx.entries.iter().map(|e| e.path.clone()).collect();
    let mut expected = actual.clone();
    expected.sort_unstable();
    assert_eq!(actual, expected, "walker output must be path-sorted");
}

#[test]
fn walk_handles_thousand_files() {
    // Concurrency stress: enough files to land entries on
    // most worker threads on multi-core hosts. Asserts (a)
    // the count is exactly N and (b) the post-sort produces
    // a stable, total ordering matching what we'd compute
    // by sorting a manual list of expected paths.
    let tmp = td();
    let n = 1_000usize;
    for i in 0..n {
        touch(tmp.path(), &format!("d{}/f{i:04}.txt", i % 16), b"x");
    }
    let idx = walk(tmp.path(), &WalkOptions::default()).unwrap();

    let file_paths: Vec<_> = idx.files().map(|e| e.path.clone()).collect();
    assert_eq!(
        file_paths.len(),
        n,
        "expected {n} files, got {}",
        file_paths.len(),
    );

    let mut expected = file_paths.clone();
    expected.sort_unstable();
    assert_eq!(
        file_paths, expected,
        "concurrent walker output must remain path-sorted",
    );
}

// ── v0.9.8: parent_to_children + descendants_of ─────────────

/// Build a synthetic [`FileIndex`] with explicit `(path, is_dir)`
/// entries — sidesteps the filesystem walker so the
/// `children_of` / `descendants_of` tests can target exact tree
/// shapes without per-test tempdir scaffolding.
fn synthetic_index(entries: &[(&str, bool)]) -> FileIndex {
    let entries = entries
        .iter()
        .map(|(p, is_dir)| FileEntry {
            path: Arc::<Path>::from(Path::new(p)),
            is_dir: *is_dir,
            size: 0,
        })
        .collect();
    FileIndex::from_entries(entries)
}

#[test]
fn children_of_empty_index_returns_empty() {
    let idx = FileIndex::default();
    assert_eq!(idx.children_of(Path::new("anything")), &[] as &[usize]);
}

#[test]
fn children_of_root_with_top_level_files() {
    let idx = synthetic_index(&[("a.rs", false), ("b.rs", false), ("README.md", false)]);
    let children: Vec<&str> = idx
        .children_of(Path::new(""))
        .iter()
        .map(|&i| idx.entries[i].path.to_str().unwrap())
        .collect();
    assert_eq!(children.len(), 3);
    assert!(children.contains(&"a.rs"));
    assert!(children.contains(&"b.rs"));
    assert!(children.contains(&"README.md"));
}

#[test]
fn children_of_nested_dir_returns_only_direct_children() {
    let idx = synthetic_index(&[
        ("crates", true),
        ("crates/api", true),
        ("crates/api/Cargo.toml", false),
        ("crates/api/src", true),
        ("crates/api/src/main.rs", false),
        ("crates/api/src/lib.rs", false),
        ("crates/api/src/utils.rs", false),
    ]);
    let children: Vec<&str> = idx
        .children_of(Path::new("crates/api/src"))
        .iter()
        .map(|&i| idx.entries[i].path.to_str().unwrap())
        .collect();
    assert_eq!(children.len(), 3);
    assert!(children.contains(&"crates/api/src/main.rs"));
    assert!(children.contains(&"crates/api/src/lib.rs"));
    assert!(children.contains(&"crates/api/src/utils.rs"));
}

#[test]
fn children_of_dir_not_in_index_returns_empty() {
    let idx = synthetic_index(&[("a.rs", false)]);
    assert_eq!(
        idx.children_of(Path::new("nonexistent/dir")),
        &[] as &[usize]
    );
}

#[test]
fn children_of_is_memoised() {
    let idx = synthetic_index(&[("a.rs", false), ("b.rs", false)]);
    // First call builds the index. Second call must return the
    // same slice from the cache (same pointer indicates the
    // OnceLock initialised exactly once).
    let first = idx.children_of(Path::new(""));
    let second = idx.children_of(Path::new(""));
    assert_eq!(first.as_ptr(), second.as_ptr());
}

#[test]
fn children_of_build_is_linear_in_the_dir_count() {
    // Audit 2026-10 finding 6: the parent -> children build did a linear
    // `entries.iter().find(..)` per parent dir, O(dirs x N): 20k dirs took
    // ~40s in a debug build. Linear now; the bound is generous (a debug build
    // on a loaded CI box), orders of magnitude above the fixed cost and well
    // below the quadratic one.
    const DIRS: usize = 20_000;
    let mut entries = Vec::with_capacity(DIRS * 2);
    for i in 0..DIRS {
        let d = format!("d{i}");
        entries.push(FileEntry {
            path: Path::new(&d).into(),
            is_dir: true,
            size: 0,
        });
        entries.push(FileEntry {
            path: Path::new(&format!("{d}/f.rs")).into(),
            is_dir: false,
            size: 1,
        });
    }
    let idx = FileIndex::from_entries(entries);
    let start = std::time::Instant::now();
    assert_eq!(idx.children_of(Path::new("")).len(), DIRS);
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "children_of build took {elapsed:?} for {DIRS} dirs (quadratic?)"
    );
    // Correctness: each dir maps to exactly its own child.
    for i in [0, DIRS / 2, DIRS - 1] {
        let kids = idx.children_of(Path::new(&format!("d{i}")));
        assert_eq!(kids.len(), 1);
        assert_eq!(
            &*idx.entries[kids[0]].path,
            Path::new(&format!("d{i}/f.rs"))
        );
    }
}

#[test]
fn file_basenames_of_filters_subdirs() {
    let idx = synthetic_index(&[
        ("pkg", true),
        ("pkg/Cargo.toml", false),
        ("pkg/README.md", false),
        ("pkg/src", true), // subdir — NOT a file basename
    ]);
    let basenames: Vec<&str> = idx.file_basenames_of(Path::new("pkg")).collect();
    assert_eq!(basenames.len(), 2);
    assert!(basenames.contains(&"Cargo.toml"));
    assert!(basenames.contains(&"README.md"));
    assert!(!basenames.contains(&"src"));
}

#[test]
fn descendants_of_root_yields_all_entries_depth_first() {
    let idx = synthetic_index(&[
        ("crates", true),
        ("crates/api", true),
        ("crates/api/lib.rs", false),
        ("crates/web", true),
        ("crates/web/lib.rs", false),
        ("README.md", false),
    ]);
    let descendants: Vec<&str> = idx
        .descendants_of(Path::new(""))
        .map(|e| e.path.to_str().unwrap())
        .collect();
    // Must include every entry whose parent chain reaches root.
    // Order depends on insertion order into the parent_to_children
    // map; assert membership rather than position.
    assert_eq!(descendants.len(), 6);
    for expected in [
        "crates",
        "crates/api",
        "crates/api/lib.rs",
        "crates/web",
        "crates/web/lib.rs",
        "README.md",
    ] {
        assert!(
            descendants.contains(&expected),
            "missing {expected:?} in {descendants:?}",
        );
    }
}

#[test]
fn descendants_of_nested_dir_skips_outside_subtree() {
    let idx = synthetic_index(&[
        ("crates", true),
        ("crates/api", true),
        ("crates/api/lib.rs", false),
        ("crates/web", true),
        ("crates/web/lib.rs", false),
        ("README.md", false),
    ]);
    let descendants: Vec<&str> = idx
        .descendants_of(Path::new("crates/api"))
        .map(|e| e.path.to_str().unwrap())
        .collect();
    assert_eq!(descendants, vec!["crates/api/lib.rs"]);
}

#[test]
fn would_walk_skip_agrees_with_the_walk() {
    // `fix --dry-run` predicts with this whether a file a fix would create
    // is ever indexed; it must agree with `walk` on the same tree.
    let tmp = td();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), ".env\nbuild/\n*.log\n").unwrap();
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("sub/.gitignore"), "!keep.log\nlocal.txt\n").unwrap();
    std::fs::write(root.join(".ignore"), "scratch.txt\n").unwrap();
    let opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: vec!["vendor/**".into()],
    };
    let cases = [
        (".env", true),
        ("build/out.txt", true),
        ("a.log", true),
        ("sub/keep.log", false),
        ("sub/local.txt", true),
        ("local.txt", false),
        ("scratch.txt", true),
        ("vendor/x.rs", true),
        ("src/main.rs", false),
        ("./README.md", false),
    ];
    for (rel, _) in cases {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    let idx = walk(root, &opts).unwrap();
    for (rel, skipped) in cases {
        let clean: PathBuf = Path::new(rel)
            .components()
            .filter(|c| !matches!(c, std::path::Component::CurDir))
            .collect();
        assert_eq!(
            idx.contains_file(&clean),
            !skipped,
            "walk disagrees with the expectation for {rel}"
        );
        assert_eq!(
            would_walk_skip(root, &opts, Path::new(rel), false),
            skipped,
            "{rel}"
        );
    }
    // Without `respect_gitignore`, only `ignore:` (and `.git`) exclude.
    let opts = WalkOptions {
        respect_gitignore: false,
        extra_ignores: vec!["vendor/**".into()],
    };
    assert!(!would_walk_skip(root, &opts, Path::new(".env"), false));
    assert!(would_walk_skip(
        root,
        &opts,
        Path::new("vendor/x.rs"),
        false
    ));
    assert!(would_walk_skip(
        root,
        &opts,
        Path::new(".git/config"),
        false
    ));
}

#[test]
fn would_walk_skip_reads_a_linked_worktrees_shared_exclude() {
    // A linked worktree's `.git` is a file naming its private git dir,
    // whose `commondir` leads to the repository's shared `info/exclude`.
    // `walk` (ignore 0.4.34) honors that file, so `fix --dry-run` must too.
    let tmp = td();
    let repo_git = tmp.path().join("main/.git");
    std::fs::create_dir_all(repo_git.join("info")).unwrap();
    std::fs::write(repo_git.join("info/exclude"), "secret.txt\nscratch/\n").unwrap();
    let private = repo_git.join("worktrees/wt");
    std::fs::create_dir_all(&private).unwrap();
    let root = tmp.path().join("wt");
    std::fs::create_dir_all(root.join("scratch")).unwrap();
    std::fs::write(
        root.join(".git"),
        format!("gitdir: {}\n", private.display()),
    )
    .unwrap();
    for rel in ["secret.txt", "scratch/notes.md", "kept.txt"] {
        std::fs::write(root.join(rel), b"x").unwrap();
    }
    let opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    // An absolute and a relative `commondir` both name the shared dir.
    for commondir in [repo_git.display().to_string(), "../..".to_string()] {
        std::fs::write(private.join("commondir"), format!("{commondir}\n")).unwrap();
        let idx = walk(&root, &opts).unwrap();
        for (rel, skipped) in [
            ("secret.txt", true),
            ("scratch/notes.md", true),
            ("kept.txt", false),
        ] {
            assert_eq!(
                idx.contains_file(Path::new(rel)),
                !skipped,
                "walk, {rel}, commondir {commondir}"
            );
            assert_eq!(
                would_walk_skip(&root, &opts, Path::new(rel), false),
                skipped,
                "would_walk_skip, {rel}, commondir {commondir}"
            );
        }
    }
    // A `.git` file without a `commondir` (a submodule) reads no exclude.
    std::fs::remove_file(private.join("commondir")).unwrap();
    assert!(
        walk(&root, &opts)
            .unwrap()
            .contains_file(Path::new("secret.txt"))
    );
    assert!(!would_walk_skip(
        &root,
        &opts,
        Path::new("secret.txt"),
        false
    ));
}

/// Every file and directory below `root`, root-relative, with whether it is
/// a directory. Symlinks are not followed (the fixtures have none).
fn tree_entries(root: &Path) -> Vec<(PathBuf, bool)> {
    let mut entries = Vec::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(root.join(&dir)).unwrap() {
            let entry = entry.unwrap();
            let rel = dir.join(entry.file_name());
            let is_dir = entry.file_type().unwrap().is_dir();
            if is_dir {
                pending.push(rel.clone());
            }
            entries.push((rel, is_dir));
        }
    }
    entries.sort();
    entries
}

/// `would_walk_skip` agrees with `walk` on every file and directory below
/// `root`, and gives `expected` for the listed paths (so a fixture that
/// excluded nothing could not pass by agreeing on nothing).
fn assert_prediction_matches_walk(root: &Path, opts: &WalkOptions, expected: &[(&str, bool)]) {
    let index = walk(root, opts).unwrap();
    let indexed: std::collections::HashSet<(PathBuf, bool)> = index
        .entries
        .iter()
        .map(|entry| (entry.path.to_path_buf(), entry.is_dir))
        .collect();
    let entries = tree_entries(root);
    for (rel, is_dir) in &entries {
        let walked = indexed.contains(&(rel.clone(), *is_dir));
        assert_eq!(
            would_walk_skip(root, opts, rel, *is_dir),
            !walked,
            "would_walk_skip disagrees with the walk on {} (respect_gitignore: {})",
            rel.display(),
            opts.respect_gitignore,
        );
    }
    for (rel, skipped) in expected {
        let is_dir = entries
            .iter()
            .any(|(path, is_dir)| *is_dir && path == Path::new(rel));
        assert_eq!(
            would_walk_skip(root, opts, Path::new(rel), is_dir),
            *skipped,
            "{rel} (respect_gitignore: {})",
            opts.respect_gitignore,
        );
    }
}

#[test]
fn would_walk_skip_matches_the_walk_on_every_entry() {
    let tmp = td();
    let root = tmp.path().join("repo");
    // The repository's own exclude file.
    touch(
        &root,
        ".git/info/exclude",
        b"excluded.txt\nreincluded.txt\n",
    );
    // `.ignore` outranks `.gitignore`, which outranks the exclude file.
    touch(
        &root,
        ".gitignore",
        b".env\nbuild/\n*.log\n!scratch.txt\n!reincluded.txt\n",
    );
    touch(&root, ".ignore", b"scratch.txt\n");
    // A directory's own ignore file applies below it, never to the directory
    // itself: `*` keeps `logs/` walked and only its other files out. The
    // nearest file wins: `d/e` re-includes a `.log` the root excludes.
    let keep = b"*\n!.gitignore\n!keep.txt\n!keep.log\n";
    touch(&root, "logs/.gitignore", keep);
    touch(&root, "d/e/.gitignore", keep);
    touch(&root, "star/.ignore", b"*\n!keep.txt\n");
    // A nested repository, and a linked worktree nested in the tree whose
    // shared exclude lives outside it: each exclude is anchored at its own
    // directory.
    touch(&root, "nested/.git/info/exclude", b"/nsecret.txt\n");
    let shared = tmp.path().join("outer.git");
    touch(&shared, "info/exclude", b"/notes.md\n");
    touch(&shared, "worktrees/wt/commondir", b"../..\n");
    touch(
        &root,
        "wt/.git",
        format!("gitdir: {}\n", shared.join("worktrees/wt").display()).as_bytes(),
    );
    for rel in [
        ".env",
        "a.log",
        "build/out.txt",
        // A file named like a directory-only pattern (`build/`) is walked.
        "tools/build",
        "scratch.txt",
        "excluded.txt",
        "reincluded.txt",
        "kept.txt",
        "d/e/keep.log",
        "logs/keep.txt",
        "logs/other.txt",
        "d/e/keep.txt",
        "d/e/other.txt",
        "star/keep.txt",
        "star/other.txt",
        "nested/nsecret.txt",
        "nested/other.txt",
        "nested/deeper/nsecret.txt",
        "wt/notes.md",
        "wt/kept.md",
        "vendor/x.rs",
    ] {
        touch(&root, rel, b"x");
    }
    let opts = |respect_gitignore| WalkOptions {
        respect_gitignore,
        extra_ignores: vec!["vendor/**".into()],
    };
    assert_prediction_matches_walk(
        &root,
        &opts(true),
        &[
            ("scratch.txt", true),
            ("excluded.txt", true),
            ("reincluded.txt", false),
            ("kept.txt", false),
            ("a.log", true),
            ("build", true),
            ("tools/build", false),
            ("d/e/keep.log", false),
            ("logs", false),
            ("logs/keep.txt", false),
            ("logs/other.txt", true),
            ("d/e/keep.txt", false),
            ("d/e/other.txt", true),
            ("star", false),
            ("star/keep.txt", false),
            ("star/other.txt", true),
            ("nested/nsecret.txt", true),
            ("nested/deeper/nsecret.txt", false),
            ("wt/notes.md", true),
            ("wt/kept.md", false),
            ("vendor/x.rs", true),
        ],
    );
    // Without `respect_gitignore` only `ignore:` and `.git` exclude.
    assert_prediction_matches_walk(
        &root,
        &opts(false),
        &[
            ("excluded.txt", false),
            ("logs/other.txt", false),
            ("wt/notes.md", false),
            ("vendor/x.rs", true),
            (".git/info/exclude", true),
        ],
    );
}

#[test]
fn would_walk_skip_reads_the_ignore_files_above_the_root() {
    // A package of a monorepo linted from its own directory: the walk reads
    // the repository's `.gitignore` and `info/exclude` above the root.
    let tmp = td();
    let repo = tmp.path().join("mono");
    touch(&repo, ".gitignore", b".env\n*.tmp\n");
    touch(&repo, ".git/info/exclude", b"/pkg/excluded.md\n");
    let root = repo.join("pkg");
    // The nearest file wins: the package re-includes one `.tmp`.
    touch(&root, ".gitignore", b"!keep.tmp\n");
    for rel in [".env", "a.tmp", "keep.tmp", "excluded.md", "ok.md"] {
        touch(&root, rel, b"x");
    }
    let opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    assert_prediction_matches_walk(
        &root,
        &opts,
        &[
            (".env", true),
            ("a.tmp", true),
            ("keep.tmp", false),
            ("excluded.md", true),
            ("ok.md", false),
        ],
    );
}

#[cfg(unix)]
#[test]
fn would_walk_skip_matches_the_walk_from_a_relative_root() {
    // `ignore` drops a leading `./` from an ignore file's directory but not
    // from the path checked against it; the prediction must not depend on it.
    let tmp = td();
    let root = tmp.path().canonicalize().unwrap();
    touch(&root, ".gitignore", b"*.log\n");
    touch(&root, "sub/.gitignore", b"!keep.log\n");
    for rel in ["a.log", "sub/keep.log", "sub/b.log"] {
        touch(&root, rel, b"x");
    }
    // `./` and `..` up to `/` from the working directory, then down to `root`.
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    let mut relative = PathBuf::from(".");
    for _ in cwd.components().skip(1) {
        relative.push("..");
    }
    relative.push(root.strip_prefix("/").unwrap());
    let opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    assert_prediction_matches_walk(
        &relative,
        &opts,
        &[
            ("a.log", true),
            ("sub/keep.log", false),
            ("sub/b.log", true),
        ],
    );
}

#[test]
fn descendants_of_short_circuits_on_take() {
    let idx = synthetic_index(&[
        ("a", true),
        ("a/b", true),
        ("a/b/c", true),
        ("a/b/c/d", true),
        ("a/b/c/d/e.rs", false),
    ]);
    // take(2) consumes only the first two yielded entries; the
    // iterator state stops descending past that. Documents the
    // "no full materialisation" contract.
    let head: Vec<&str> = idx
        .descendants_of(Path::new(""))
        .take(2)
        .map(|e| e.path.to_str().unwrap())
        .collect();
    assert_eq!(head.len(), 2);
}

#[test]
fn children_of_independent_index_caches_independently() {
    // Two FileIndexes built from different entries must NOT
    // share their parent_to_children OnceLock — each instance
    // builds its own cache. Important for `--changed`-mode
    // filtered indices that live alongside the full index.
    let idx_a = synthetic_index(&[("a.rs", false)]);
    let idx_b = synthetic_index(&[("b.rs", false)]);
    let a_children = idx_a.children_of(Path::new(""));
    let b_children = idx_b.children_of(Path::new(""));
    assert_eq!(a_children.len(), 1);
    assert_eq!(b_children.len(), 1);
    let a_path = idx_a.entries[a_children[0]].path.to_str().unwrap();
    let b_path = idx_b.entries[b_children[0]].path.to_str().unwrap();
    assert_eq!(a_path, "a.rs");
    assert_eq!(b_path, "b.rs");
}

#[test]
fn children_of_only_indexes_walker_known_dirs() {
    // The walker emits both files AND dirs (per the existing
    // FileEntry::is_dir field). children_of indexes by parent
    // path regardless of whether the parent itself is a known
    // entry — so a deep tree where intermediate dirs aren't
    // explicitly in entries still indexes correctly.
    let idx = synthetic_index(&[("deep/nested/a.rs", false), ("deep/nested/b.rs", false)]);
    let children = idx.children_of(Path::new("deep/nested"));
    assert_eq!(children.len(), 2);
}

#[test]
fn read_capped_or_skip_gates_on_the_passed_size() {
    // M3: the cap uses the size argument (the walk-time index size), not
    // the file — so a tiny, readable file "claimed" to be over the cap is
    // skipped without reading, and one under the cap is read.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("small.txt");
    std::fs::write(&p, b"tiny").unwrap();
    assert!(
        read_capped_or_skip(&p, MAX_ANALYZE_BYTES + 1).is_none(),
        "over-cap size must skip"
    );
    assert_eq!(
        read_capped_or_skip(&p, 4).unwrap(),
        b"tiny",
        "under-cap size must read"
    );
}

#[cfg(unix)]
#[test]
fn walk_skips_a_fifo_special_file() {
    // A FIFO (named pipe) must NOT be indexed as a file: opening one
    // `O_RDONLY` blocks until a writer appears, so a per-file content rule
    // reading it would hang the whole run forever. Regular files alongside
    // it are still indexed.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("normal.txt"), b"hi").unwrap();
    let fifo = root.join("pipe.txt");
    let ok = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|s| s.success());
    if !ok || !fifo.exists() {
        eprintln!("mkfifo unavailable; skipping FIFO walk test");
        return;
    }

    let idx = walk(root, &WalkOptions::default()).unwrap();
    let names: Vec<_> = idx.entries.iter().map(|e| e.path.to_path_buf()).collect();
    assert!(
        names.iter().any(|p| p.as_os_str() == "normal.txt"),
        "regular file must still be indexed: {names:?}"
    );
    assert!(
        !names.iter().any(|p| p.as_os_str() == "pipe.txt"),
        "FIFO must be skipped, not indexed: {names:?}"
    );
}

#[test]
#[cfg(unix)]
fn dangling_symlink_is_skipped_not_fatal() {
    // Round-5 audit: with `follow_links(true)` a broken (dangling) symlink
    // surfaced as a FATAL walk error, aborting the whole run before any rule
    // ran -- so a repo containing one dangling link was entirely un-lintable
    // (and `file_remove` could not clean it up). It must be skipped, leaving
    // the rest of the tree indexed.
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("real.txt"), b"hi\n").unwrap();
    symlink("/does/not/exist/at/all", root.join("dangling")).unwrap();

    let idx =
        walk(root, &WalkOptions::default()).expect("a dangling symlink must not abort the walk");
    let names: Vec<_> = idx.entries.iter().map(|e| e.path.to_path_buf()).collect();
    assert!(
        names.iter().any(|p| p.as_os_str() == "real.txt"),
        "the real file must still be indexed: {names:?}"
    );
}

#[test]
#[cfg(unix)]
fn symlink_cycles_are_skipped_not_fatal() {
    // Round-7 (5a): a symlink CYCLE surfaces as `ignore::Error::Loop`
    // (ancestor-based: `..`, mutual) or a kernel ELOOP (a direct self-loop) --
    // neither is `NotFound`, so before this a single stray cycle aborted the
    // ENTIRE walk (exit 2), making the whole repo un-lintable / un-fixable.
    // Each cycle shape must be skipped, leaving the rest of the tree indexed.
    use std::os::unix::fs::symlink;
    for shape in ["parent", "selfdir", "mutual", "selfref"] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("real.txt"), b"hi\n").unwrap();
        match shape {
            "parent" => {
                std::fs::create_dir(root.join("sub")).unwrap();
                symlink("..", root.join("sub/up")).unwrap();
            }
            "selfdir" => symlink(".", root.join("selfloop")).unwrap(),
            "mutual" => {
                symlink("cyc_y", root.join("cyc_x")).unwrap();
                symlink("cyc_x", root.join("cyc_y")).unwrap();
            }
            "selfref" => symlink("loop", root.join("loop")).unwrap(),
            _ => unreachable!(),
        }
        let idx = walk(root, &WalkOptions::default())
            .unwrap_or_else(|e| panic!("cycle shape {shape:?} must not abort the walk: {e}"));
        assert!(
            idx.entries.iter().any(|e| e.path.as_os_str() == "real.txt"),
            "cycle shape {shape:?}: the real file must still be indexed"
        );
    }
}

#[test]
fn read_capped_or_skip_missing_file_is_none() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(read_capped_or_skip(&tmp.path().join("nope"), 0).is_none());
}

#[test]
fn read_bounded_bounds_the_actual_read_toctou() {
    // M3-F2: the read is bounded by the file's real bytes AT READ TIME, not
    // a (possibly stale) walk-time size — a file whose ACTUAL size exceeds
    // the cap is skipped, so concurrent growth past the cap can't force an
    // unbounded read. read_capped_or_skip delegates to this; the internal
    // cap is 256 MiB (a fixture that large is infeasible), so the bound is
    // exercised here with a small cap. The `size_hint` arg is strictly a
    // preallocation hint: a wrong hint must never change what is accepted.
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().join("grew.txt");
    std::fs::write(&p, vec![b'x'; 10]).unwrap();
    assert!(
        // Hint LIES that the 10-byte file fits the cap (4). The take(cap+1)
        // bound, not the hint, decides — the over-cap file is still skipped.
        read_bounded(&p, 4, 4).is_none(),
        "actual size over the cap must skip (never trust the size hint)"
    );
    assert_eq!(
        // Hint understates (0 = no preallocation); the whole file still reads.
        read_bounded(&p, 100, 0).unwrap().len(),
        10,
        "under the cap reads the whole file regardless of the hint"
    );
    assert!(
        read_bounded(&tmp.path().join("nope"), 100, 50).is_none(),
        "missing file is None"
    );
}
