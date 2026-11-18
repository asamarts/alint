use super::*;

#[test]
fn collect_returns_none_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    // `git ls-files` in a non-git directory exits non-zero;
    // we report None. Tests that need a populated set
    // construct a real repo via fixtures elsewhere.
    let result = collect_tracked_paths(tmp.path());
    assert!(result.is_none());
}

#[test]
fn collect_changed_returns_none_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    // Both diff modes shell out to git; both should report
    // None outside a repo so callers can decide between
    // hard-error (CLI's `--changed`) and silent fallback.
    assert!(collect_changed_paths(tmp.path(), None).is_none());
    assert!(collect_changed_paths(tmp.path(), Some("main")).is_none());
}

#[test]
fn collect_changed_without_base_unions_staged_unstaged_and_untracked() {
    let repo = make_repo_with_commits(&["init"]);
    let root = repo.path();
    std::fs::write(root.join("staged.txt"), "old\n").unwrap();
    std::fs::write(root.join("unstaged.txt"), "old\n").unwrap();
    let run = |args: &[&str]| {
        let output = crate::git::test_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["add", "staged.txt", "unstaged.txt"]);
    run(&["commit", "-m", "add fixtures"]);

    std::fs::write(root.join("staged.txt"), "staged edit\n").unwrap();
    run(&["add", "staged.txt"]);
    std::fs::write(root.join("unstaged.txt"), "unstaged edit\n").unwrap();
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();

    let changed = collect_changed_paths(root, None).expect("changed paths");
    assert_eq!(
        changed,
        HashSet::from([
            PathBuf::from("staged.txt"),
            PathBuf::from("unstaged.txt"),
            PathBuf::from("untracked.txt"),
        ])
    );
}

#[test]
fn collect_changed_without_base_drains_a_staged_list_larger_than_a_pipe() {
    // The staged diff runs in the background while `ls-files` runs. Its
    // output here (1,500 paths of ~86 bytes, ~126 KiB) overflows a 64 KiB
    // pipe buffer, so the child blocks on write until it's drained: the
    // result must still be complete, with no hang. (Waiting on the child
    // before reading its stdout would deadlock here.)
    let repo = make_repo_with_commits(&["init"]);
    let root = repo.path();
    let dir = root.join("a-directory-name-long-enough-to-grow-each-path");
    std::fs::create_dir(&dir).unwrap();
    let mut expected = HashSet::new();
    for i in 0..1_500 {
        let name = format!("staged-file-with-a-padded-name-{i:05}.txt");
        std::fs::write(dir.join(&name), "x\n").unwrap();
        expected.insert(Path::new("a-directory-name-long-enough-to-grow-each-path").join(name));
    }
    let status = crate::git::test_git()
        .arg("-C")
        .arg(root)
        .args(["add", "-A"])
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::write(root.join("untracked.txt"), "new\n").unwrap();
    expected.insert(PathBuf::from("untracked.txt"));

    let changed = collect_changed_paths(root, None).expect("changed paths");
    assert_eq!(changed, expected);
}

#[test]
#[cfg(unix)]
fn path_from_git_chunk_preserves_non_utf8() {
    use std::os::unix::ffi::OsStrExt as _;
    // M6: a non-UTF-8 tracked/changed path must survive as its exact bytes,
    // not collapse the whole set to None (which made `git_tracked_only`
    // over-permissive by not recognizing the file as tracked).
    let raw: &[u8] = b"dir/\xff\xfe_file";
    let p = path_from_git_chunk(raw);
    assert_eq!(p.as_os_str().as_bytes(), raw);
}

#[test]
fn head_commit_time_uses_fixed_committer_timestamp_and_detached_head() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    assert_eq!(
        head_commit_time(root),
        Err(AnalysisTimeUnavailable::HeadUnavailable)
    );
    assert!(
        crate::git::test_git()
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        head_commit_time(root),
        Err(AnalysisTimeUnavailable::HeadUnavailable)
    );
    std::fs::write(root.join("tracked"), "x\n").unwrap();
    assert!(
        crate::git::test_git()
            .args(["add", "tracked"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        crate::git::test_git()
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "fixed",
            ])
            .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
            .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z")
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    let expected =
        "2024-01-02".parse::<AnalysisTime>().unwrap().unix_seconds() + 3 * 3_600 + 4 * 60 + 5;
    assert_eq!(head_commit_time(root).unwrap().unix_seconds(), expected);
    assert!(
        crate::git::test_git()
            .args(["checkout", "-q", "--detach", "HEAD"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(head_commit_time(root).unwrap().unix_seconds(), expected);
}

#[test]
fn git_queries_ignore_foreign_repository_environment() {
    const ROOT_ENV: &str = "ALINT_TEST_HEAD_TIME_ROOT";
    const EXPECTED_ENV: &str = "ALINT_TEST_HEAD_TIME_EXPECTED";
    if let Some(root) = std::env::var_os(ROOT_ENV) {
        let expected = std::env::var(EXPECTED_ENV).unwrap().parse::<u64>().unwrap();
        assert_eq!(
            head_commit_time(Path::new(&root)).unwrap().unix_seconds(),
            expected
        );
        let tracked = collect_tracked_paths(Path::new(&root)).unwrap();
        assert!(tracked.contains(Path::new("tracked.txt")));
        let blame = blame_lines(Path::new(&root), Path::new("tracked.txt")).unwrap();
        assert_eq!(blame.len(), 1);
        assert_eq!(blame[0].content, "2024-01-02T03:04:05Z");
        return;
    }

    let make_repo = |date: &str| {
        let repo = tempfile::tempdir().unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["config", "user.email", "test@example.com"][..],
            &["config", "user.name", "Test"][..],
            &["config", "commit.gpgsign", "false"][..],
        ] {
            assert!(
                crate::git::test_git()
                    .arg("-C")
                    .arg(repo.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(repo.path().join("tracked.txt"), format!("{date}\n")).unwrap();
        assert!(
            crate::git::test_git()
                .arg("-C")
                .arg(repo.path())
                .args(["add", "tracked.txt"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            crate::git::test_git()
                .arg("-C")
                .arg(repo.path())
                .args(["commit", "-qm", "fixed"])
                .env("GIT_AUTHOR_DATE", date)
                .env("GIT_COMMITTER_DATE", date)
                .status()
                .unwrap()
                .success()
        );
        repo
    };
    let target = make_repo("2024-01-02T03:04:05Z");
    let foreign = make_repo("2030-06-07T08:09:10Z");
    let expected =
        "2024-01-02".parse::<AnalysisTime>().unwrap().unix_seconds() + 3 * 3_600 + 4 * 60 + 5;

    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "git::tests::git_queries_ignore_foreign_repository_environment",
            "--nocapture",
        ])
        .env(ROOT_ENV, target.path())
        .env(EXPECTED_ENV, expected.to_string())
        .env("GIT_DIR", foreign.path().join(".git"))
        .env("GIT_WORK_TREE", foreign.path())
        .env("GIT_INDEX_FILE", foreign.path().join(".git/index"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "isolated child failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn scoped_process_environment_filters_repository_redirectors() {
    let root = tempfile::tempdir().unwrap();
    let environment = ProcessEnvironment::from([
        ("GIT_DIR".to_string(), "/foreign/repository".to_string()),
        ("SAFE_VALUE".to_string(), "retained".to_string()),
    ]);
    let _guard = scoped_process_environment(root.path(), environment);
    repository::tests::init_repository(root.path(), &[]);

    let command = git_command(root.path()).expect("a repository to name");
    let git_dir = command
        .get_envs()
        .find(|(name, _)| *name == "GIT_DIR")
        .map(|(_, value)| value);
    let safe_value = command
        .get_envs()
        .find(|(name, _)| *name == "SAFE_VALUE")
        .map(|(_, value)| value);

    assert_eq!(git_dir, None);
    assert_eq!(
        safe_value,
        Some(Some(std::ffi::OsStr::new("retained"))),
        "unrelated scoped values must reach Git"
    );
}

/// A ceiling only narrows discovery: alint's own search for the repository
/// stops below one the embedder's scoped environment sets (`alint test` keeps
/// discovery in its sandbox with it), and it reaches Git, while the host's own
/// is still dropped.
#[test]
#[cfg(unix)]
fn a_scoped_ceiling_reaches_git_and_stops_discovery() {
    let outer = tempfile::tempdir().unwrap();
    let parent = outer.path().join("parent");
    let root = parent.join("sandbox/repository");
    std::fs::create_dir_all(&root).unwrap();
    let init = crate::git::test_git()
        .args(["init", "-q"])
        .arg(outer.path())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    assert!(
        collect_tracked_paths(&root).is_some(),
        "without a ceiling, discovery reaches the outer repository"
    );

    let ceiling = parent.to_string_lossy().into_owned();
    let environment: ProcessEnvironment = std::env::vars()
        .filter(|(name, _)| !GIT_LOCAL_ENV_VARS.contains(&name.as_str()))
        .chain([("GIT_CEILING_DIRECTORIES".to_string(), ceiling.clone())])
        .collect();
    let _guard = scoped_process_environment(&root, environment);
    assert!(
        git_command(&root).is_none(),
        "no repository below the ceiling"
    );
    assert_eq!(collect_tracked_paths(&root), None);

    repository::tests::init_repository(&root, &[]);
    let command = git_command(&root).expect("the sandbox's repository");
    assert_eq!(
        command
            .get_envs()
            .find(|(name, _)| *name == "GIT_CEILING_DIRECTORIES")
            .map(|(_, value)| value),
        Some(Some(std::ffi::OsStr::new(&ceiling)))
    );
}

#[test]
#[cfg(unix)]
fn git_log_queries_disable_configured_signature_verification() {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let run = |args: &[&str]| {
        let output = crate::git::test_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    run(&["init", "-q"]);
    std::fs::write(root.join("tracked"), "x\n").unwrap();
    run(&["add", "tracked"]);
    run(&[
        "-c",
        "user.name=Test",
        "-c",
        "user.email=test@example.com",
        "commit",
        "-qm",
        "base",
    ]);

    // Install a commit carrying a signature header without depending on a
    // real key. With log.showSignature enabled, Git would invoke the
    // configured verifier for otherwise read-only history queries.
    let tree = String::from_utf8(run(&["rev-parse", "HEAD^{tree}"]))
        .unwrap()
        .trim()
        .to_string();
    let commit = format!(
        "tree {tree}\n\
             author Test <test@example.com> 1704164645 +0000\n\
             committer Test <test@example.com> 1704164645 +0000\n\
             gpgsig -----BEGIN PGP SIGNATURE-----\n\
             \x20\n\
             \x20ZmFrZQ==\n\
             \x20-----END PGP SIGNATURE-----\n\
             \n\
             signed\n"
    );
    let mut hash_object = crate::git::test_git()
        .arg("-C")
        .arg(root)
        .args(["hash-object", "-t", "commit", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hash_object
        .stdin
        .take()
        .unwrap()
        .write_all(commit.as_bytes())
        .unwrap();
    let hash_output = hash_object.wait_with_output().unwrap();
    assert!(hash_output.status.success());
    let hash = String::from_utf8(hash_output.stdout).unwrap();
    run(&["update-ref", "HEAD", hash.trim()]);

    let verifier = root.join("gpg-probe.sh");
    std::fs::write(
        &verifier,
        "#!/bin/sh\ntouch \"$(dirname \"$0\")/gpg-invoked\"\nexit 1\n",
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&verifier).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&verifier, permissions).unwrap();
    run(&["config", "log.showSignature", "true"]);
    run(&[
        "config",
        "gpg.program",
        verifier.to_str().expect("temporary path is UTF-8"),
    ]);

    assert_eq!(
        head_commit_time(root).unwrap().unix_seconds(),
        1_704_164_645
    );
    assert_eq!(head_commit_message(root).as_deref(), Some("signed"));
    assert_eq!(
        head_commit_record(root).map(|record| record.message),
        Some("signed".to_string())
    );
    assert!(
        !root.join("gpg-invoked").exists(),
        "read-only log queries must not invoke a signature verifier"
    );
}

#[test]
fn git_history_queries_ignore_local_replace_refs() {
    use std::io::Write as _;

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let run = |args: &[&str]| {
        let output = crate::git::test_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    };
    run(&["init", "-q"]);
    std::fs::write(root.join("tracked"), "x\n").unwrap();
    run(&["add", "tracked"]);
    let output = crate::git::test_git()
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "base",
        ])
        .env("GIT_AUTHOR_DATE", "2024-01-02T03:04:05Z")
        .env("GIT_COMMITTER_DATE", "2024-01-02T03:04:05Z")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tree = String::from_utf8(run(&["rev-parse", "HEAD^{tree}"]))
        .unwrap()
        .trim()
        .to_string();
    let replacement = format!(
        "tree {tree}\n\
             author Test <test@example.com> 1749283750 +0000\n\
             committer Test <test@example.com> 1749283750 +0000\n\
             \n\
             replacement\n"
    );
    let mut hash_object = crate::git::test_git()
        .arg("-C")
        .arg(root)
        .args(["hash-object", "-t", "commit", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    hash_object
        .stdin
        .take()
        .unwrap()
        .write_all(replacement.as_bytes())
        .unwrap();
    let replacement_output = hash_object.wait_with_output().unwrap();
    assert!(replacement_output.status.success());
    let replacement_hash = String::from_utf8(replacement_output.stdout).unwrap();
    run(&["replace", "HEAD", replacement_hash.trim()]);

    assert_eq!(
        head_commit_time(root).unwrap().unix_seconds(),
        1_704_164_645,
        "local replace refs must not change HEAD's deterministic timestamp"
    );
    assert_eq!(
        head_commit_message(root).as_deref(),
        Some("base"),
        "local replace refs must not change HEAD's linted message"
    );
    assert_eq!(
        head_commit_record(root).map(|record| record.message),
        Some("base".to_string()),
        "local replace refs must not change HEAD's commit record"
    );
    let blame = blame_lines(root, Path::new("tracked")).expect("tracked file has blame");
    assert_eq!(blame.len(), 1);
    assert_eq!(
        blame[0]
            .author_time
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        1_704_164_645,
        "local replace refs must not change blame's deterministic author time"
    );
}

#[test]
fn head_commit_time_parser_rejects_empty_malformed_and_out_of_range_values() {
    for raw in [
        b"".as_slice(),
        b"\n",
        b" 123\n",
        b"-1\n",
        b"123 extra\n",
        b"253402300800\n",
    ] {
        assert_eq!(
            parse_head_commit_time(raw),
            Err(AnalysisTimeUnavailable::InvalidHeadTime),
            "{raw:?}"
        );
    }
    assert_eq!(
        parse_head_commit_time(b"0\n").unwrap(),
        "1970-01-01".parse().unwrap()
    );
}

#[test]
fn head_message_returns_none_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    // The low-level helper remains total outside Git. Engine capability
    // routing converts this repository-wide condition to an analysis
    // failure before a commit-message rule dispatches.
    assert!(head_commit_message(tmp.path()).is_none());
}

#[test]
fn parse_porcelain_two_lines_two_commits() {
    // Two source lines, each in its own porcelain block. The
    // first line is from an old commit (1700000000 = 2023-11-15);
    // the second is from a more recent one (1750000000 =
    // 2025-06-15). Both blocks repeat the full metadata per
    // line-porcelain semantics.
    let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 1 1 1
author Old Author
author-mail <old@example.com>
author-time 1700000000
author-tz +0000
committer Old Author
committer-mail <old@example.com>
committer-time 1700000000
committer-tz +0000
summary first commit
filename src/main.rs
\told line content
ef01ef01ef01ef01ef01ef01ef01ef01ef01ef01 2 2 1
author New Author
author-mail <new@example.com>
author-time 1750000000
author-tz +0000
committer New Author
committer-mail <new@example.com>
committer-time 1750000000
committer-tz +0000
summary recent commit
filename src/main.rs
\tnew line content
";
    let lines = parse_porcelain(porcelain);
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].line_number, 1);
    assert_eq!(lines[0].content, "old line content");
    assert_eq!(
        lines[0].author_time,
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    );
    assert_eq!(lines[1].line_number, 2);
    assert_eq!(lines[1].content, "new line content");
    assert_eq!(
        lines[1].author_time,
        UNIX_EPOCH + Duration::from_secs(1_750_000_000)
    );
}

#[test]
fn parse_porcelain_handles_previous_marker() {
    // The optional `previous <sha> <name>` line shows up when
    // the line was rewritten — the parser must not get
    // confused by it.
    let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 5 5 1
author X
author-mail <x@example.com>
author-time 1700000000
author-tz +0000
committer X
committer-mail <x@example.com>
committer-time 1700000000
committer-tz +0000
summary did a thing
previous 1111111111111111111111111111111111111111 src/old.rs
filename src/main.rs
\tline body
";
    let lines = parse_porcelain(porcelain);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].line_number, 5);
    assert_eq!(lines[0].content, "line body");
}

#[test]
fn parse_porcelain_skips_blocks_missing_metadata() {
    // A block whose author-time line is corrupt (non-numeric)
    // should drop that line rather than panic. The next valid
    // block still emits.
    let porcelain = "\
abcd1234abcd1234abcd1234abcd1234abcd1234 1 1 1
author X
author-time not-a-number
filename a.rs
\tbroken
ef01ef01ef01ef01ef01ef01ef01ef01ef01ef01 2 2 1
author Y
author-time 1700000000
filename a.rs
\tworks
";
    let lines = parse_porcelain(porcelain);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].content, "works");
}

#[test]
fn parse_porcelain_omits_uncommitted_zero_sha_lines() {
    let porcelain = "\
0000000000000000000000000000000000000000 1 1 1
author Not Committed Yet
author-time 1999999999
filename a.rs
\tworking tree line
abcd1234abcd1234abcd1234abcd1234abcd1234 2 2 1
author Committed
author-time 1700000000
filename a.rs
\tcommitted line
";
    let lines = parse_porcelain(porcelain);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].line_number, 2);
    assert_eq!(lines[0].content, "committed line");
}

#[test]
fn parse_porcelain_supports_sha256_headers_and_zero_ids() {
    let porcelain = "\
0000000000000000000000000000000000000000000000000000000000000000 1 1 1
author Not Committed Yet
author-time 1999999999
filename a.rs
\tworking tree line
abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234abcd1234 2 2 1
author Committed
author-time 1700000000
filename a.rs
\tcommitted line
";
    let lines = parse_porcelain(porcelain);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].line_number, 2);
    assert_eq!(lines[0].content, "committed line");
}

// One repository fixture exercises every trust boundary in sequence; keeping
// its commits together makes attribution failures diagnosable.
#[allow(clippy::too_many_lines)]
#[test]
fn blame_cache_uses_only_tracked_regular_default_ignore_revs_file() {
    const LOCAL_IGNORE_REVS_FILE: &str = ".local-blame-ignore-revs";
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let run = |args: &[&str]| {
        let output = crate::git::test_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    let commit_at = |message: &str, date: &str| {
        let output = crate::git::test_git()
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                message,
            ])
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git commit: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let author_time = || {
        run(&["show", "-s", "--format=%ct", "HEAD"])
            .parse::<u64>()
            .unwrap()
    };

    run(&["init", "-q"]);
    std::fs::write(root.join("tracked"), "original line here\n").unwrap();
    run(&["add", "tracked"]);
    commit_at("original", "2020-01-02T03:04:05Z");

    std::fs::write(root.join("tracked"), "formatted line here\n").unwrap();
    run(&["add", "tracked"]);
    commit_at("format-only", "2021-02-03T04:05:06Z");
    let locally_ignored = run(&["rev-parse", "HEAD"]);
    let expected_ignored_author_time = author_time();

    std::fs::write(root.join("tracked"), "substantive line here\n").unwrap();
    run(&["add", "tracked"]);
    commit_at("substantive change", "2024-06-07T08:09:10Z");
    let repository_ignored = run(&["rev-parse", "HEAD"]);
    let substantive_author_time = author_time();

    std::fs::write(
        root.join(DEFAULT_BLAME_IGNORE_REVS_FILE),
        format!("{repository_ignored}\n"),
    )
    .unwrap();
    std::fs::write(
        root.join(LOCAL_IGNORE_REVS_FILE),
        format!("{locally_ignored}\n"),
    )
    .unwrap();
    run(&["config", "blame.ignoreRevsFile", LOCAL_IGNORE_REVS_FILE]);

    let untracked_cache = BlameCache::new(root.to_path_buf());
    let untracked_blame = untracked_cache
        .get(Path::new("tracked"))
        .expect("tracked file has blame");
    assert_eq!(
        untracked_blame[0]
            .author_time
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        substantive_author_time,
        "neither an untracked default file nor clone-local config may alter blame"
    );

    run(&["add", DEFAULT_BLAME_IGNORE_REVS_FILE]);
    commit_at("record ignored revision", "2025-01-01T00:00:00Z");
    // A clone-local algorithm must not affect attribution.
    run(&["config", "diff.algorithm", "histogram"]);

    let cache = BlameCache::new(root.to_path_buf());
    let blame = cache
        .get(Path::new("tracked"))
        .expect("tracked file has blame");
    assert_eq!(blame.len(), 1);
    assert_eq!(
        blame[0]
            .author_time
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        expected_ignored_author_time,
        "the tracked repository file must apply, but clone-local config must not"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let target = root.join("ignore-target");
        std::fs::write(&target, format!("{repository_ignored}\n")).unwrap();
        std::fs::remove_file(root.join(DEFAULT_BLAME_IGNORE_REVS_FILE)).unwrap();
        symlink(&target, root.join(DEFAULT_BLAME_IGNORE_REVS_FILE)).unwrap();
        let symlink_cache = BlameCache::new(root.to_path_buf());
        let symlink_blame = symlink_cache
            .get(Path::new("tracked"))
            .expect("tracked file has blame");
        assert_eq!(
            symlink_blame[0]
                .author_time
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            substantive_author_time,
            "a tracked path replaced by a worktree symlink must not read external ignore data"
        );
    }
}

#[test]
fn blame_cache_pins_diff_algorithm() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let run = |args: &[&str]| {
        let output = crate::git::test_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let commit_at = |message: &str, date: &str| {
        let output = crate::git::test_git()
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qam",
                message,
            ])
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git commit: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };

    run(&["init", "-q"]);
    std::fs::write(root.join("tracked"), "A\nA\nA\nD\nC\nF\nC\nC\n").unwrap();
    run(&["add", "tracked"]);
    commit_at("base", "2020-01-01T00:00:00Z");
    std::fs::write(root.join("tracked"), "A\nA\nC\nA\nC\nB\nA\nC\nD\n").unwrap();
    commit_at("ambiguous edit", "2024-01-01T00:00:00Z");
    run(&["config", "diff.algorithm", "histogram"]);

    let blame = BlameCache::new(root.to_path_buf())
        .get(Path::new("tracked"))
        .expect("tracked file has blame");
    let author_times: Vec<_> = blame
        .iter()
        .map(|line| {
            line.author_time
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
        })
        .collect();
    assert_eq!(
        author_times,
        vec![
            1_577_836_800,
            1_577_836_800,
            1_577_836_800,
            1_704_067_200,
            1_577_836_800,
            1_704_067_200,
            1_704_067_200,
            1_577_836_800,
            1_704_067_200,
        ],
        "clone-local diff.algorithm must not change line ages"
    );
}

#[test]
fn blame_lines_returns_none_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    // No repo, so blame on anything (existing or not) fails.
    let result = blame_lines(tmp.path(), Path::new("missing.rs"));
    assert!(result.is_none());
}

#[test]
fn blame_cache_memoises_failure() {
    // Calling `get` twice on a non-existent file in a
    // non-git directory must short-circuit on the second
    // call. We can't observe the cache directly from outside,
    // but we can verify both calls return None and the cache
    // ends up with an entry for the path.
    let tmp = tempfile::tempdir().unwrap();
    let cache = BlameCache::new(tmp.path().to_path_buf());
    assert!(cache.get(Path::new("missing.rs")).is_none());
    assert!(cache.get(Path::new("missing.rs")).is_none());
    let guard = cache.inner.lock().unwrap();
    assert!(matches!(
        guard.get(Path::new("missing.rs")),
        Some(CacheEntry::Failed)
    ));
}

#[test]
fn dir_has_tracked_files_walks_prefix() {
    let mut set = HashSet::new();
    set.insert(PathBuf::from("src/main.rs"));
    set.insert(PathBuf::from("README.md"));
    assert!(dir_has_tracked_files(Path::new("src"), &set));
    assert!(!dir_has_tracked_files(Path::new("target"), &set));
    // `src` matches `src/main.rs` via prefix; `tar` does not
    // match `target/foo` because no tracked path is under
    // `tar/`.
    assert!(!dir_has_tracked_files(Path::new("tar"), &set));
}

// ----- commit_messages_in_range -----------------------------

#[test]
fn commit_range_rejects_dash_since_and_writes_no_file() {
    // Security regression (git arg-injection): a config-controlled
    // `since` starting with `-` (e.g. `--output=…`) must be rejected
    // before git runs — it must never write/truncate a file. (Affects
    // the released `git_commit_message` `since:` path.)
    let outdir = tempfile::tempdir().unwrap();
    let stem = outdir.path().join("sentinel");
    let would_write = outdir.path().join("sentinel..HEAD");
    let evil = format!("--output={}", stem.display());
    // In a repository of its own, so the check is reached wherever the test
    // runs (outside one, the function returns before it).
    let repo = make_repo_with_commits(&["one", "two"]);
    let err = commit_messages_in_range(repo.path(), &evil, false).unwrap_err();
    assert!(matches!(err, CommitRangeError::BadRange { .. }), "{err:?}");
    assert!(
        !would_write.exists(),
        "git must not have written {would_write:?}"
    );
}

#[test]
fn collect_changed_paths_dash_base_writes_no_file() {
    // The `--changed` / `changed_since` diff path: `--end-of-options`
    // forces a dash-leading base into the revision slot, so git never
    // parses `--output=…` and writes nothing.
    let outdir = tempfile::tempdir().unwrap();
    let stem = outdir.path().join("sentinel");
    let would_write = outdir.path().join("sentinel...HEAD");
    let evil = format!("--output={}", stem.display());
    // In a repository of its own: outside one, no Git runs at all.
    let repo = make_repo_with_commits(&["one", "two"]);
    let _ = collect_changed_paths(repo.path(), Some(&evil));
    assert!(
        !would_write.exists(),
        "git diff must not have written {would_write:?}"
    );
}

/// Build a temp dir into a git repo with the given list of
/// empty commits in order (commit N is HEAD~(len-1-N)). Returns
/// the tempdir so the caller controls its lifetime.
///
/// Uses `git commit --allow-empty` so the test doesn't need to
/// write fixture files. Disables GPG signing and sets a fixed
/// author so the commits are deterministic.
fn make_repo_with_commits(subjects: &[&str]) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let init_dir = tmp.path();
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
        vec!["config", "commit.gpgsign", "false"],
    ] {
        let out = crate::git::test_git()
            .arg("-C")
            .arg(init_dir)
            .args(&args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?} failed");
    }
    for subject in subjects {
        let out = crate::git::test_git()
            .arg("-C")
            .arg(init_dir)
            .args(["commit", "--allow-empty", "-m", subject])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git commit failed: stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    tmp
}

#[test]
fn head_commit_message_lossily_decodes_non_utf8() {
    // A non-UTF-8 HEAD commit message must NOT make `head_commit_message`
    // return `None` — that would silently no-op the `git_commit_message`
    // rule, letting a non-UTF-8 message bypass subject/pattern/length checks
    // (M6 fail-open, the sibling of the range-mode path already hardened).
    let tmp = make_repo_with_commits(&[]);
    let dir = tmp.path();
    let msg = dir.join("MSG");
    // Raw non-UTF-8 bytes (0xFF/0xFE are never valid UTF-8) in the subject.
    std::fs::write(&msg, b"fix: \xff\xfe non-utf8 subject\n").unwrap();
    let out = crate::git::test_git()
        .arg("-C")
        .arg(dir)
        .args(["commit", "--allow-empty", "-F"])
        .arg(&msg)
        .output()
        .unwrap();
    assert!(out.status.success(), "commit failed");
    let got = head_commit_message(dir);
    assert!(
        got.is_some(),
        "a non-UTF-8 HEAD message must decode lossily, not collapse to None"
    );
    assert!(
        got.unwrap().contains("non-utf8 subject"),
        "the (lossily-decoded) message body should survive"
    );
}

#[test]
fn parse_commit_log_empty_input() {
    assert_eq!(parse_commit_log(b""), Vec::new());
}

#[test]
fn parse_commit_log_single_commit() {
    // sha NUL name NUL email NUL body-with-trailing-newline RS.
    let raw =
        b"abc1234\0Jane Doe\0jane@example.com\0subject line\n\nbody line one\nbody line two\n\x1e";
    let records = parse_commit_log(raw);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].sha, "abc1234");
    assert_eq!(records[0].author_name, "Jane Doe");
    assert_eq!(records[0].author_email, "jane@example.com");
    assert_eq!(
        records[0].message,
        "subject line\n\nbody line one\nbody line two"
    );
}

#[test]
fn parse_commit_log_multiple_commits() {
    // Two commits, oldest first (matches --reverse). Between
    // records, git inserts a newline before the next SHA; the
    // parser strips it.
    let raw = b"a1\0A\0a@x.test\0first\n\x1e\nb2\0B\0b@x.test\0second\n\x1e";
    let records = parse_commit_log(raw);
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].sha, "a1");
    assert_eq!(records[0].author_email, "a@x.test");
    assert_eq!(records[0].message, "first");
    assert_eq!(records[1].sha, "b2");
    assert_eq!(records[1].message, "second");
}

#[test]
fn parse_commit_log_subject_only_no_body() {
    let raw = b"deadbef\0N\0n@x.test\0just the subject\n\x1e";
    let records = parse_commit_log(raw);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].message, "just the subject");
}

#[test]
fn parse_commit_log_preserves_blank_lines_in_body() {
    // A real commit body with multiple paragraphs survives the
    // round-trip unchanged.
    let raw = b"sha7777\0N\0n@x.test\0fix: thing\n\nfirst paragraph.\n\nsecond paragraph.\n\nthird.\n\x1e";
    let records = parse_commit_log(raw);
    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].message,
        "fix: thing\n\nfirst paragraph.\n\nsecond paragraph.\n\nthird."
    );
}

#[test]
fn parse_commit_log_lossily_decodes_invalid_utf8_rather_than_dropping() {
    // A record whose message field is invalid UTF-8. The parser must NOT
    // drop it — silently skipping the commit would let a contributor
    // bypass commit linting (conventional-subject / author-allowlist /
    // forbidden-pattern) just by using a non-UTF-8 message or author.
    // It now lossily decodes so the commit is still linted; the sha is
    // always hex, so it survives intact.
    let mut raw: Vec<u8> = b"abc1234\0N\0n@x.test\0".to_vec();
    raw.extend_from_slice(&[0xff, 0xfe, 0xfd]); // invalid UTF-8
    raw.push(0x1e);
    let records = parse_commit_log(&raw);
    assert_eq!(records.len(), 1, "the commit must be parsed, not dropped");
    assert_eq!(records[0].sha, "abc1234");
    assert_eq!(records[0].author_name, "N");
    // The invalid bytes become U+FFFD replacement chars (not silently lost).
    assert!(records[0].message.contains('\u{FFFD}'));
}

#[test]
fn commit_range_returns_none_outside_git() {
    let tmp = tempfile::tempdir().unwrap();
    // A non-Git directory produces the repository-unavailable variant,
    // distinct from a bad range inside a usable repository. Engine
    // capability routing turns this into a typed analysis failure before
    // dependent rules dispatch.
    let result = commit_messages_in_range(tmp.path(), "main", false);
    assert!(matches!(result, Ok(None)));
}

#[test]
fn commit_range_returns_empty_vec_for_head_to_head() {
    let repo = make_repo_with_commits(&["feat: first commit"]);
    let result = commit_messages_in_range(repo.path(), "HEAD", false).unwrap();
    // HEAD..HEAD is the empty range. Some(empty), not None.
    assert_eq!(result, Some(Vec::new()));
}

#[test]
fn commit_range_enumerates_real_commits_oldest_first() {
    // Four commits. Use the root commit's full SHA as the
    // `since` base; the range then yields the three later
    // commits, oldest first.
    let repo = make_repo_with_commits(&["root: zero", "feat: alpha", "fix: beta", "chore: gamma"]);
    let config = crate::git::test_git()
        .arg("-C")
        .arg(repo.path())
        .args(["config", "core.abbrev", "20"])
        .output()
        .unwrap();
    assert!(config.status.success());
    let root_sha = String::from_utf8(
        crate::git::test_git()
            .arg("-C")
            .arg(repo.path())
            .args(["rev-parse", "HEAD~3"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    let records = commit_messages_in_range(repo.path(), &root_sha, false)
        .unwrap()
        .unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0].message, "feat: alpha");
    assert_eq!(records[1].message, "fix: beta");
    assert_eq!(records[2].message, "chore: gamma");
    // The command pins seven characters instead of inheriting core.abbrev;
    // Git may extend only if a prefix is ambiguous.
    for r in &records {
        assert_eq!(r.sha.len(), 7);
        assert!(r.sha.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[test]
fn commit_range_skips_merges_by_default() {
    // Build the canonical PR-CI shape: a base branch with one
    // commit, a feature branch off it with two commits, then a
    // merge commit on the base branch. The merge is what
    // actions/checkout produces at HEAD on a pull_request
    // trigger.
    let repo = make_repo_with_commits(&["init commit on main"]);
    let root = repo.path();
    let run = |args: &[&str]| {
        let out = crate::git::test_git()
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    };
    let base_sha = run(&["rev-parse", "HEAD"]).trim().to_string();
    run(&["checkout", "-q", "-b", "feature"]);
    run(&["commit", "--allow-empty", "-m", "feat: A"]);
    run(&["commit", "--allow-empty", "-m", "fix: B"]);
    run(&["checkout", "-q", "main"]);
    run(&["merge", "--no-ff", "--no-edit", "feature"]);

    // Range main-base..HEAD: includes feat:A, fix:B, and the
    // merge commit. Default skips the merge.
    let records = commit_messages_in_range(root, &base_sha, false)
        .unwrap()
        .unwrap();
    let subjects: Vec<&str> = records.iter().map(|r| r.message.as_str()).collect();
    assert_eq!(subjects, vec!["feat: A", "fix: B"]);

    // Same range with include_merges: true picks up the merge.
    let with_merge = commit_messages_in_range(root, &base_sha, true)
        .unwrap()
        .unwrap();
    assert_eq!(with_merge.len(), 3);
    assert!(with_merge.iter().any(|r| r.message.starts_with("Merge ")));
}

#[test]
fn changed_paths_checked_none_outside_git_and_bad_range_inside() {
    // The low-level helper preserves unavailable-repository `None`; engine
    // capability routing turns it into an owning-rule analysis failure.
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        collect_changed_paths_checked(tmp.path(), "origin/main"),
        Ok(None)
    ));
    // Inside a repo, an unresolvable ref hard-errors.
    let repo = make_repo_with_commits(&["init"]);
    assert!(matches!(
        collect_changed_paths_checked(repo.path(), "no-such-ref"),
        Err(CommitRangeError::BadRange { .. })
    ));
}

#[test]
fn verify_commit_returns_false_for_unsigned_commit() {
    // make_repo_with_commits disables gpg signing, so HEAD is
    // unsigned; verify-commit exits non-zero → Some(false).
    let repo = make_repo_with_commits(&["init: unsigned commit"]);
    let head = String::from_utf8(
        crate::git::test_git()
            .arg("-C")
            .arg(repo.path())
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();
    assert_eq!(verify_commit(repo.path(), &head), Some(false));
}

#[test]
fn commit_range_returns_bad_range_for_unknown_ref() {
    let repo = make_repo_with_commits(&["init"]);
    let result = commit_messages_in_range(repo.path(), "does-not-exist-ref", false);
    match result {
        Err(CommitRangeError::BadRange { stderr }) => {
            // Git typically says "unknown revision or path not
            // in the working tree." We don't assert the exact
            // wording (varies across git versions); just that
            // we got a non-empty stderr.
            assert_ne!(stderr, "");
        }
        other => panic!("expected BadRange, got {other:?}"),
    }
}
