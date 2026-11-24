use super::*;
use crate::walker::{in_repository, walk, walk_subtrees};

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
        std::os::unix::fs::symlink(shared.join("info/exclude"), repo.join("escape.txt")).unwrap();
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

/// Deep directory chains (hundreds of nested directories, each with its
/// own `.gitignore`) and deep `IgnoreNode` / `ExplicitAncestor` chains must
/// traverse and drop iteratively without overflowing a small thread stack.
#[test]
fn deep_directory_tree_does_not_overflow_stack() {
    let tmp = td();
    let root = tmp.path().join("deep");
    let mut cur = root.clone();
    std::fs::create_dir_all(&cur).unwrap();
    // Stay safely inside each OS's `PATH_MAX` (Linux 4096, macOS 1024,
    // Windows 260 without verbatim prefix) while exercising deep traversal
    // on a 1 MiB stack.
    let depth: usize = if cfg!(windows) {
        50
    } else if cfg!(target_os = "linux") {
        600
    } else {
        350
    };
    for i in 0..depth {
        cur.push("d");
        std::fs::create_dir(&cur).unwrap();
        if i % 4 == 0 {
            std::fs::write(cur.join(".gitignore"), "*.tmp\n").unwrap();
        }
    }
    std::fs::write(cur.join("leaf.txt"), b"ok").unwrap();
    std::fs::write(cur.join("drop.tmp"), b"no").unwrap();

    let handle = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || {
            let idx = walk(&root, &WalkOptions::test_default()).unwrap();
            assert!(
                idx.entries
                    .iter()
                    .any(|e| { e.path.file_name() == Some(OsStr::new("leaf.txt")) && !e.is_dir })
            );
            assert!(
                !idx.entries
                    .iter()
                    .any(|e| { e.path.file_name() == Some(OsStr::new("drop.tmp")) })
            );

            // Also prove `IgnoreNode` and `ExplicitAncestor` drop iteratively
            // at depths far exceeding any OS `PATH_MAX`.
            let mut ignore_head: Option<Arc<IgnoreNode>> = None;
            let mut ancestor_head: Option<Arc<ExplicitAncestor>> = None;
            for _ in 0..20_000 {
                ignore_head = Some(Arc::new(IgnoreNode {
                    dir: DirIgnores {
                        dot_ignore: None,
                        git_ignore: None,
                        exclude: None,
                        repository: false,
                    },
                    parent: ignore_head,
                }));
                ancestor_head = Some(Arc::new(ExplicitAncestor {
                    canonical: PathBuf::from("d"),
                    parent: ancestor_head,
                }));
            }
            drop(ignore_head);
            drop(ancestor_head);
        })
        .unwrap();
    handle.join().unwrap();
}

#[test]
fn walk_on_single_file_returns_empty_index() {
    let tmp = td();
    let file = tmp.path().join("only.txt");
    std::fs::write(&file, b"hi").unwrap();
    let idx = walk(&file, &WalkOptions::test_default()).unwrap();
    assert!(idx.entries.is_empty());
}

#[test]
fn walk_subtrees_matches_full_walk_for_ignores_casing_cycles_and_errors() {
    let tmp = td();
    // Outer `.ignore` above a non-repo root must NOT apply even when a
    // nested `.git` exists under a requested subtree (ADR-0028).
    touch(tmp.path(), ".ignore", b"*.outer_only\n");
    let root = tmp.path().join("root");
    touch(&root, ".gitignore", b"ignored/\n*.log\n");
    touch(&root, "Docs/out.txt", b"committed\n");
    touch(&root, "ignored/secret/a.txt", b"no\n");
    touch(&root, "nested/.git/HEAD", b"ref: refs/heads/main\n");
    touch(&root, "nested/keep.outer_only", b"kept\n");
    touch(&root, "nested/drop.log", b"dropped\n");
    touch(&root, "regular_file", b"file\n");
    touch(&root, "real/sub/item.txt", b"item\n");
    touch(&root, "real/.gitignore", b"*.real_only\n");
    touch(&root, "real/sub/skip.real_only", b"x\n");
    touch(&root, "vendor/skip.txt", b"v\n");
    #[cfg(unix)]
    {
        // Symlinked subtree inside root: lexical ancestors apply, not
        // target's parent ignores (`real/.gitignore`).
        std::os::unix::fs::symlink("real/sub", root.join("alias")).unwrap();
        // Upward cycle targeting an ancestor between root and subtree.
        std::os::unix::fs::symlink("../..", root.join("real/sub/up")).unwrap();
        // Direct ELOOP symlink.
        std::os::unix::fs::symlink("eloop", root.join("eloop")).unwrap();
    }

    let opts = WalkOptions {
        respect_gitignore: true,
        extra_ignores: vec!["vendor/**".into()],
        global_git_excludes: false,
    };

    // 1. Exact casing check: asking for `docs` when disk has `Docs` yields
    //    zero entries even on case-insensitive APFS/NTFS.
    let wrong_case = walk_subtrees(&root, &[Path::new("docs")], &opts).unwrap();
    assert!(wrong_case.entries.is_empty());

    // 2. Regular file intermediate segment (`regular_file/child`), missing
    //    path, ignored dir, vendor/** extra_ignore target, and ELOOP skip
    //    cleanly.
    let skipped = walk_subtrees(
        &root,
        &[
            Path::new("regular_file/child"),
            Path::new("missing/dir"),
            Path::new("ignored/secret"),
            Path::new("vendor/skip.txt"),
            Path::new("eloop"),
        ],
        &opts,
    )
    .unwrap();
    assert!(skipped.entries.is_empty());
    // `vendor/**` excludes every file inside `vendor/` while leaving the
    // directory entry itself, matching `walk(&root)`.
    let vendor = walk_subtrees(&root, &[Path::new("vendor")], &opts).unwrap();
    assert!(!vendor.contains_file(Path::new("vendor/skip.txt")));

    // 3. Outside-repo root + nested `.git` does not load outer `.ignore`.
    let nested = walk_subtrees(&root, &[Path::new("nested")], &opts).unwrap();
    assert!(nested.contains_file(Path::new("nested/keep.outer_only")));
    assert!(!nested.contains_file(Path::new("nested/drop.log")));

    // 4. Upward cycle (`real/sub/up -> ../..`) stops cleanly, and symlinked
    //    subtree (`alias -> real/sub`) uses lexical ancestors (no
    //    `real/.gitignore`), matching `walk(&root)`.
    #[cfg(unix)]
    {
        let full = walk(&root, &opts).unwrap();
        let sub =
            walk_subtrees(&root, &[Path::new("alias"), Path::new("real/sub")], &opts).unwrap();
        let sub_paths: Vec<String> = sub
            .entries
            .iter()
            .map(|e| e.path.display().to_string())
            .collect();
        let full_sub_paths: Vec<String> = full
            .entries
            .iter()
            .filter(|e| {
                e.path.starts_with(Path::new("alias")) || e.path.starts_with(Path::new("real/sub"))
            })
            .map(|e| e.path.display().to_string())
            .collect();
        assert_eq!(sub_paths, full_sub_paths);
        assert!(sub.contains_file(Path::new("alias/skip.real_only")));
        assert!(!sub.contains_file(Path::new("real/sub/skip.real_only")));
        assert!(
            sub.followed_symlinks
                .as_ref()
                .is_some_and(|s| s.iter().any(|p| p.as_ref() == Path::new("alias")))
        );
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

    // 4. Non-regular `.git` (symlink to /dev/zero): fails strictly inside
    //    root, warns and stops outer `.git` discovery above root (#568).
    let tmp4 = td();
    let outer4 = tmp4.path().join("outer");
    touch(&outer4, ".git/HEAD", b"ref: refs/heads/main\n");
    touch(&outer4, ".gitignore", b"*.outer_hide\n");
    let repo4 = outer4.join("repo");
    std::fs::create_dir_all(&repo4).unwrap();
    std::os::unix::fs::symlink("/dev/zero", repo4.join(".git")).unwrap();
    let pkg4 = repo4.join("pkg");
    touch(&pkg4, "keep.outer_hide", b"kept\n");
    let err4 = walk(&repo4, &WalkOptions::test_default()).unwrap_err();
    assert!(
        err4.to_string()
            .contains("the walk does not read the ignore file")
            && err4.to_string().contains("not a regular file"),
        "expected strict refusal for non-regular .git inside root, got: {err4}"
    );
    let idx4 = walk(&pkg4, &WalkOptions::test_default()).unwrap();
    assert!(
        idx4.contains_file(Path::new("keep.outer_hide")),
        "outer repo .gitignore should not leak past bad .git boundary"
    );
    assert!(
        idx4.walk_warnings()
            .iter()
            .any(|w| w.contains(".git") && w.contains("not a regular file")),
        "expected warning for non-regular parent .git: {:?}",
        idx4.walk_warnings()
    );
}
