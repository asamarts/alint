//! `check --changed` must judge the changed files against the WHOLE repository.
//! Regressions from the v0.18.3 review: under `--changed` the per-file pass matched
//! scope and resolved references against the filtered (changed-only) index, so
//!
//! - a `scope_filter.has_ancestor` rule never fired when its marker (`Cargo.toml`) was
//!   unchanged, in `check --changed` and `fix --changed` alike;
//! - `markdown_paths_resolve` reported an unchanged referenced file as missing, and
//!   missed a deleted target referenced from an unchanged document;
//! - `json_schema_passes` was skipped when only its schema changed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?} failed: {out:?}");
}

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn commit_all(root: &Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(alint())
        .args(args)
        .current_dir(root)
        .output()
        .expect("run alint")
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn has_ancestor_rule_fires_under_changed_with_an_unchanged_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        ".alint.yml",
        "version: 1
rules:
  - id: rs-no-trailing
    kind: no_trailing_whitespace
    paths: \"**/*.rs\"
    scope_filter:
      has_ancestor: Cargo.toml
    level: error
    fix:
      file_trim_trailing_whitespace: {}
",
    );
    write(root, "crates/api/Cargo.toml", "[package]\nname = \"api\"\n");
    write(root, "crates/api/src/main.rs", "fn main() {}\n");
    commit_all(root);
    // Only the source file changes; the `Cargo.toml` marker is unchanged.
    write(root, "crates/api/src/main.rs", "fn main() {}   \n");

    let full = run(root, &["check", "--compact", "."]);
    assert_eq!(full.status.code(), Some(1), "full check: {}", text(&full));
    let changed = run(root, &["check", "--changed", "--compact", "."]);
    assert_eq!(
        changed.status.code(),
        Some(1),
        "check --changed must see the unchanged marker: {}",
        text(&changed)
    );
    assert!(
        text(&changed).contains("rs-no-trailing"),
        "{}",
        text(&changed)
    );

    let fix = run(root, &["fix", "--changed", "."]);
    assert!(fix.status.success(), "fix --changed: {}", text(&fix));
    assert_eq!(
        std::fs::read_to_string(root.join("crates/api/src/main.rs")).unwrap(),
        "fn main() {}\n",
        "fix --changed must fix the in-scope changed file"
    );
}

const MD_PATHS: &str = "version: 1
rules:
  - id: md-paths
    kind: markdown_paths_resolve
    paths: \"**/*.md\"
    prefixes: [\"docs/\"]
    level: error
";

#[test]
fn markdown_paths_resolve_sees_unchanged_targets_under_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, ".alint.yml", MD_PATHS);
    write(root, "docs/guide.md", "# guide\n");
    write(root, "README.md", "intro\n");
    commit_all(root);
    // Edit the document to reference an existing, unchanged file.
    write(root, "README.md", "See `docs/guide.md`.\n");

    let changed = run(root, &["check", "--changed", "--compact", "."]);
    assert_eq!(
        changed.status.code(),
        Some(0),
        "an unchanged existing target must resolve: {}",
        text(&changed)
    );
}

#[test]
fn markdown_paths_resolve_catches_a_deleted_target_under_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, ".alint.yml", MD_PATHS);
    write(root, "docs/guide.md", "# guide\n");
    write(root, "README.md", "See `docs/guide.md`.\n");
    commit_all(root);
    // Delete the target; the referencing document is unchanged.
    std::fs::remove_file(root.join("docs/guide.md")).unwrap();

    let changed = run(root, &["check", "--changed", "--compact", "."]);
    assert_eq!(
        changed.status.code(),
        Some(1),
        "deleting a referenced file must fail check --changed: {}",
        text(&changed)
    );
    assert!(text(&changed).contains("md-paths"), "{}", text(&changed));
}

#[test]
fn json_schema_passes_revalidates_when_only_the_schema_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        ".alint.yml",
        "version: 1
rules:
  - id: data-schema
    kind: json_schema_passes
    paths: \"data/*.json\"
    schema_path: schema.json
    level: error
",
    );
    write(root, "schema.json", "{\"type\": \"object\"}\n");
    write(root, "data/a.json", "{\"name\": \"a\"}\n");
    commit_all(root);
    // Tighten the schema only; the data file is unchanged.
    write(
        root,
        "schema.json",
        "{\"type\": \"object\", \"required\": [\"version\"]}\n",
    );

    let changed = run(root, &["check", "--changed", "--compact", "."]);
    assert_eq!(
        changed.status.code(),
        Some(1),
        "a schema-only change must re-validate the data: {}",
        text(&changed)
    );
    assert!(text(&changed).contains("data-schema"), "{}", text(&changed));
}

#[test]
fn rule_by_rule_has_ancestor_rule_fires_under_changed_with_an_unchanged_marker() {
    // `filename_case` is evaluated rule by rule on the `--changed` index (not in the
    // per-file pass, which matches scope on the full index), so its `has_ancestor`
    // lookup needs the shared tree file set. It is the only `has_ancestor` rule here,
    // so this also proves the engine decides to share it.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        ".alint.yml",
        "version: 1
rules:
  - id: rs-snake
    kind: filename_case
    paths: \"**/*.rs\"
    case: snake
    scope_filter:
      has_ancestor: Cargo.toml
    level: error
",
    );
    write(root, "crates/api/Cargo.toml", "[package]\nname = \"api\"\n");
    write(root, "crates/api/src/main.rs", "fn main() {}\n");
    commit_all(root);
    // A new badly named file; the `Cargo.toml` marker is unchanged.
    write(root, "crates/api/src/BadName.rs", "\n");

    let changed = run(root, &["check", "--changed", "--compact", "."]);
    assert_eq!(
        changed.status.code(),
        Some(1),
        "check --changed must see the unchanged marker: {}",
        text(&changed)
    );
    assert!(text(&changed).contains("rs-snake"), "{}", text(&changed));
}
