use std::path::PathBuf;

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

#[test]
fn commands_discover_ancestor_config_and_use_its_directory_as_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let nested = root.join("packages/app/src");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(root.join("README.md"), "# Repository\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: root-readme\n    kind: file_exists\n    paths: README.md\n    root_only: true\n    level: error\n",
    )
    .unwrap();

    let check = crate::hermetic::command(alint())
        .arg("check")
        .current_dir(&nested)
        .output()
        .expect("run alint check from nested directory");
    assert!(
        check.status.success(),
        "ancestor config must be evaluated at its own root: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    let validate = crate::hermetic::command(alint())
        .arg("validate-config")
        .current_dir(&nested)
        .output()
        .expect("run validate-config from nested directory");
    assert!(
        validate.status.success(),
        "validate-config must discover the ancestor config: {}",
        String::from_utf8_lossy(&validate.stderr)
    );
}

#[test]
fn fix_from_a_subdirectory_mutates_the_discovered_repository_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let nested = root.join("packages/app");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(root.join("README.md"), "trailing   \n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: trim-readme\n    kind: no_trailing_whitespace\n    paths: README.md\n    level: error\n    fix: { file_trim_trailing_whitespace: {} }\n",
    )
    .unwrap();

    let output = crate::hermetic::command(alint())
        .arg("fix")
        .current_dir(&nested)
        .output()
        .expect("run alint fix from nested directory");
    assert!(
        output.status.success(),
        "fix failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "trailing\n"
    );
}

#[test]
fn baseline_from_a_subdirectory_writes_to_the_discovered_repository_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let nested = root.join("packages/app");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: missing\n    kind: file_exists\n    paths: REQUIRED.md\n    level: error\n",
    )
    .unwrap();

    let output = crate::hermetic::command(alint())
        .arg("baseline")
        .current_dir(&nested)
        .output()
        .expect("run alint baseline from nested directory");
    assert!(
        output.status.success(),
        "baseline failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join(".alint-baseline.json").is_file());
    assert!(!nested.join(".alint-baseline.json").exists());
}

#[test]
fn facts_from_a_subdirectory_evaluate_the_discovered_repository_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let nested = root.join("packages/app");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(root.join("README.md"), "# Repository\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nfacts:\n  - id: has_root_readme\n    any_file_exists: [README.md]\nrules: []\n",
    )
    .unwrap();

    let output = crate::hermetic::command(alint())
        .args(["facts", "--format", "json"])
        .current_dir(&nested)
        .output()
        .expect("run alint facts from nested directory");
    assert!(
        output.status.success(),
        "facts failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["facts"][0]["id"], "has_root_readme");
    assert_eq!(body["facts"][0]["value"], true);
}

/// Inside a repository, discovery stops at its top (the nearest directory
/// holding a `.git` directory or file): a config above it, in a directory
/// someone else may write (`/tmp`, a shared home), never governs it. Every
/// command that discovers a config stops there. From a subdirectory, the
/// repository's own config is still found.
#[test]
fn discovery_stops_at_the_repository_top() {
    let tmp = tempfile::tempdir().unwrap();
    let canary = tmp.path().join("canary");
    std::fs::write(
        tmp.path().join(".alint.yml"),
        format!(
            "version: 1\nrules:\n  - id: planted\n    kind: command\n    paths: \"*.txt\"\n    \
             command: [\"sh\", \"-c\", \"touch '{}'\"]\n    level: error\n",
            canary.display()
        ),
    )
    .unwrap();
    let repo = tmp.path().join("repo");
    let nested = repo.join("src");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir(repo.join(".git")).unwrap();
    std::fs::write(nested.join("a.txt"), "x\n").unwrap();

    for args in [
        &["check"][..],
        &["fix"],
        &["list"],
        &["validate-config"],
        &["export-agents-md"],
    ] {
        let output = crate::hermetic::command(alint())
            .args(args)
            .current_dir(&nested)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{args:?}: {stderr}");
        assert!(stderr.contains("no .alint.yml found"), "{args:?}: {stderr}");
        assert!(!canary.exists(), "{args:?} ran the parent config's command");
    }

    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nrules:\n  - id: has-a\n    kind: file_exists\n    paths: src/a.txt\n    \
         level: error\n",
    )
    .unwrap();
    let output = crate::hermetic::command(alint())
        .arg("check")
        .current_dir(&nested)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!canary.exists());
}
