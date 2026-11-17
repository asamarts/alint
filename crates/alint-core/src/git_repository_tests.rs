use super::*;

/// A ceiling that keeps a search inside `dir`'s parent's subtree, so a stray
/// repository above the temporary directory never answers.
fn ceiling(dir: &Path) -> String {
    canonical(dir).unwrap().to_string_lossy().into_owned()
}

/// A repository Git made at `dir` (with `git init`; `args` go before the
/// directory). On Windows, where alint reads no owner, alint asks Git about
/// every repository it finds, and Git accepts only one it would discover
/// itself, so a bare `.git` directory is not enough there.
pub(in crate::git) fn init_repository(dir: &Path, args: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    let init = Command::new("git")
        .arg("init")
        .arg("-q")
        .args(args)
        .arg(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_CEILING_DIRECTORIES")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dir.join(".no-global-config"))
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
}

#[test]
fn the_nearest_git_directory_names_the_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let top = tmp.path().join("repo");
    init_repository(&top, &[]);
    std::fs::create_dir_all(top.join("a/b")).unwrap();
    let fence = ceiling(tmp.path());
    let found = find_repository(&top.join("a/b"), Some(&fence), None).unwrap();
    let top = canonical(&top).unwrap();
    assert_eq!(found.work_tree, top);
    assert_eq!(found.git_dir, top.join(".git"));
    let [git_dir, work_tree] = found.args();
    assert_eq!(
        git_dir,
        OsString::from(format!("--git-dir={}", top.join(".git").display()))
    );
    assert_eq!(
        work_tree,
        OsString::from(format!("--work-tree={}", top.display()))
    );
}

#[test]
fn a_gitfile_names_the_git_directory_relative_to_itself() {
    let tmp = tempfile::tempdir().unwrap();
    let store = tmp.path().join("store/wt.git");
    let top = tmp.path().join("checkout");
    std::fs::create_dir_all(tmp.path().join("store")).unwrap();
    let separate = format!("--separate-git-dir={}", store.display());
    init_repository(&top, &[&separate]);
    std::fs::create_dir_all(top.join("src")).unwrap();
    std::fs::write(top.join(".git"), "gitdir: ../store/wt.git\r\n").unwrap();
    let fence = ceiling(tmp.path());
    let found = find_repository(&top.join("src"), Some(&fence), None).unwrap();
    assert_eq!(found.work_tree, canonical(&top).unwrap());
    assert_eq!(
        found.git_dir,
        canonical(&tmp.path().join("store/wt.git")).unwrap()
    );

    // Any other content is no gitfile, and no repository.
    for text in ["", "gitdir:", "gitdir: \n", "ref: refs/heads/main\n"] {
        std::fs::write(top.join(".git"), text).unwrap();
        assert_eq!(find_repository(&top, Some(&fence), None), None, "{text:?}");
    }
}

/// A directory shaped like a bare repository is content: no directory is
/// ever taken as a repository itself, so the search goes on above it.
#[test]
fn a_bare_shaped_directory_is_never_a_repository() {
    let tmp = tempfile::tempdir().unwrap();
    let bare = tmp.path().join("outer/bare");
    std::fs::create_dir_all(bare.join("objects")).unwrap();
    std::fs::create_dir_all(bare.join("refs")).unwrap();
    std::fs::write(bare.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(bare.join("config"), "[core]\n\tworktree = .\n").unwrap();
    let fence = ceiling(tmp.path());
    assert_eq!(find_repository(&bare, Some(&fence), None), None);

    init_repository(&tmp.path().join("outer"), &[]);
    let found = find_repository(&bare, Some(&fence), None).unwrap();
    assert_eq!(
        found.work_tree,
        canonical(&tmp.path().join("outer")).unwrap()
    );
}

/// As `GIT_CEILING_DIRECTORIES` reads: no directory at or above the deepest
/// ceiling strictly above the start is read, and a relative entry is ignored.
#[test]
fn the_search_stops_below_the_ceiling() {
    let tmp = tempfile::tempdir().unwrap();
    init_repository(tmp.path(), &[]);
    let start = tmp.path().join("sandbox/repository");
    std::fs::create_dir_all(&start).unwrap();
    let sandbox = ceiling(&tmp.path().join("sandbox"));
    let separator = if cfg!(windows) { ';' } else { ':' };
    for ceilings in [
        sandbox.clone(),
        format!("relative{separator}{sandbox}"),
        format!("{}{separator}{sandbox}", ceiling(tmp.path())),
    ] {
        assert_eq!(
            find_repository(&start, Some(&ceilings), None),
            None,
            "{ceilings}"
        );
    }
    // The start itself is read even when it is a ceiling.
    init_repository(&start, &[]);
    let own = ceiling(&start);
    assert!(find_repository(&start, Some(&own), None).is_some());
    // Without a ceiling below it, the repository above is found.
    std::fs::remove_dir_all(start.join(".git")).unwrap();
    let above = ceiling(tmp.path().parent().unwrap());
    assert_eq!(
        find_repository(&start, Some(&above), None).map(|found| found.work_tree),
        Some(canonical(tmp.path()).unwrap())
    );
}

#[test]
fn repository_top_is_the_nearest_directory_holding_git() {
    let tmp = tempfile::tempdir().unwrap();
    let top = tmp.path().join("repo");
    std::fs::create_dir_all(top.join(".git")).unwrap();
    let vendored = top.join("node_modules/pkg");
    std::fs::create_dir_all(&vendored).unwrap();
    assert_eq!(repository_top(&vendored), Some(top.clone()));
    let submodule = top.join("sub");
    std::fs::create_dir_all(&submodule).unwrap();
    std::fs::write(submodule.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
    assert_eq!(repository_top(&submodule.join("x")), Some(submodule));
}

/// Another user's `.git`, or a `.git` in a directory another user owns, is
/// no top: the search goes on to the next one above.
#[test]
fn repository_top_takes_only_a_git_the_current_user_owns() {
    let tmp = tempfile::tempdir().unwrap();
    let outer = tmp.path().join("outer");
    let inner = outer.join("inner");
    std::fs::create_dir_all(outer.join(".git")).unwrap();
    std::fs::create_dir_all(inner.join("src")).unwrap();
    std::fs::write(inner.join(".git"), "").unwrap();
    let start = inner.join("src");
    let owned = |_: &Path| Some(true);
    assert_eq!(repository_top_with(&start, &owned), Some(inner.clone()));
    let foreign_entry = |path: &Path| Some(path != inner.join(".git"));
    assert_eq!(
        repository_top_with(&start, &foreign_entry),
        Some(outer.clone())
    );
    let foreign_directory = |path: &Path| Some(path != inner);
    assert_eq!(
        repository_top_with(&start, &foreign_directory),
        Some(outer.clone())
    );
    assert_eq!(repository_top_with(&start, &|_| Some(false)), None);
    // No owner read (Windows): any `.git`.
    assert_eq!(repository_top_with(&start, &|_| None), Some(inner));
}

/// The ownership check of [`trusted`]: a repository whose `.git`, work tree
/// and Git directory the current user owns is named without asking Git; for
/// one with any of the three another user's, Git decides (its ownership check
/// and `safe.directory`). `GIT_TEST_ASSUME_DIFFERENT_OWNER` makes Git take
/// every repository as another user's.
#[cfg(unix)]
#[test]
fn trusted_asks_git_unless_the_current_user_owns_all_three() {
    let tmp = tempfile::tempdir().unwrap();
    let top = tmp.path().join("repo");
    init_repository(&top, &[]);
    let repository = Repository {
        git_dir: canonical(&top.join(".git")).unwrap(),
        work_tree: canonical(&top).unwrap(),
    };
    let entry = top.join(".git");
    let safe = tmp.path().join("safe.gitconfig");
    std::fs::write(&safe, "[safe]\n\tdirectory = *\n").unwrap();
    let environment = |global: &Path| -> crate::git::ProcessEnvironment {
        std::env::vars()
            .filter(|(name, _)| !name.starts_with("GIT_"))
            .chain([
                ("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string()),
                (
                    "GIT_CONFIG_GLOBAL".to_string(),
                    global.to_string_lossy().into_owned(),
                ),
                (
                    "GIT_TEST_ASSUME_DIFFERENT_OWNER".to_string(),
                    "1".to_string(),
                ),
            ])
            .collect()
    };
    let refusing = environment(Path::new("/dev/null"));
    let accepting = environment(&safe);
    // Git's verdict is cached by Git directory; each case asks afresh.
    let ask = |owned: &dyn Fn(&Path) -> Option<bool>, environment| {
        VERDICTS.lock().unwrap().remove(&repository.git_dir);
        trusted(&repository, &entry, Some(environment), owned)
    };

    // All three the current user's: named, though Git would refuse.
    assert!(ask(&|_| Some(true), &refusing));
    // Any one another user's: Git refuses, so it is not named...
    for foreign in [
        entry.as_path(),
        repository.work_tree.as_path(),
        repository.git_dir.as_path(),
    ] {
        let owned = |path: &Path| Some(path != foreign);
        assert!(!ask(&owned, &refusing), "{}", foreign.display());
        // ...unless the user's `safe.directory` lets it through.
        assert!(ask(&owned, &accepting), "{}", foreign.display());
    }
    // No owner read (Windows): Git always decides.
    assert!(!ask(&|_| None, &refusing));
    assert!(ask(&|_| None, &accepting));
    VERDICTS.lock().unwrap().remove(&repository.git_dir);
}

#[cfg(unix)]
#[test]
fn the_current_user_owns_what_it_creates() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(owned_by_current_user(tmp.path()), Some(true));
    assert_eq!(
        owned_by_current_user(&tmp.path().join("missing")),
        Some(false)
    );
}

/// An explicit `--git-dir` skips Git's check that the current user owns the
/// repository (CVE-2022-24765), so for a repository another user owns, alint
/// asks Git to discover exactly it and names it only when Git accepts it.
/// `GIT_TEST_ASSUME_DIFFERENT_OWNER` makes Git take every repository as
/// another user's.
#[cfg(unix)]
#[test]
fn git_decides_on_a_repository_another_user_owns() {
    let tmp = tempfile::tempdir().unwrap();
    let top = tmp.path().join("repo");
    std::fs::create_dir_all(&top).unwrap();
    let init = Command::new("git")
        .args(["init", "-q"])
        .arg(&top)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    let repository = Repository {
        git_dir: canonical(&top.join(".git")).unwrap(),
        work_tree: canonical(&top).unwrap(),
    };
    let safe = tmp.path().join("safe.gitconfig");
    std::fs::write(&safe, "[safe]\n\tdirectory = *\n").unwrap();
    let environment = |global: &Path, extra: &[(&str, &str)]| -> crate::git::ProcessEnvironment {
        std::env::vars()
            .filter(|(name, _)| !name.starts_with("GIT_"))
            .chain([
                ("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string()),
                (
                    "GIT_CONFIG_GLOBAL".to_string(),
                    global.to_string_lossy().into_owned(),
                ),
            ])
            .chain(
                extra
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string())),
            )
            .collect()
    };
    let none = Path::new("/dev/null");
    assert!(git_accepts(&repository, Some(&environment(none, &[]))));
    let different = [("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1")];
    assert!(
        !git_accepts(&repository, Some(&environment(none, &different))),
        "Git refuses a repository another user owns"
    );
    // The user's own `safe.directory` lets it through, as for Git itself.
    assert!(git_accepts(
        &repository,
        Some(&environment(&safe, &different))
    ));
}
