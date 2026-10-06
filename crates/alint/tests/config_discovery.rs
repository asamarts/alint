use std::path::PathBuf;
use std::process::Command;

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

    let check = Command::new(alint())
        .arg("check")
        .current_dir(&nested)
        .output()
        .expect("run alint check from nested directory");
    assert!(
        check.status.success(),
        "ancestor config must be evaluated at its own root: {}",
        String::from_utf8_lossy(&check.stderr)
    );

    let validate = Command::new(alint())
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

    let output = Command::new(alint())
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

    let output = Command::new(alint())
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

    let output = Command::new(alint())
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
