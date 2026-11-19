//! `alint check --changed` and `--base` with `constraint` rules, in a real Git
//! repository (design `v0.23/selectors-and-constraints.md` section 9). A rule
//! runs when a changed path, added, modified or deleted, is one the selectors
//! of its relations read: its input's, a join's other side's, or the relation
//! its `set` compares against. It then judges the whole snapshot and reports
//! every finding, also those in unchanged files. A change outside those
//! selectors, or to `.alint.yml` alone, runs no `constraint` rule.

use std::path::{Path, PathBuf};
use std::process::Output;

use serde_json::Value;

/// A locale policy, a pin policy over a join, and a catalog policy over a
/// `set` against another relation.
const CONFIG: &str = r#"version: 2
selectors:
  locales:
    paths: "locales/*.json"
  manifests:
    paths: "packages/*/package.json"
relations:
  messages:
    from: locales
    entries: { format: json, at: "$" }
    fields: { locale: stem, key: entry.key }
  deps:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { dependency: entry.key, range: entry.value }
  pins:
    from: { paths: pins.json }
    entries: { format: json, at: "$" }
    fields: { name: entry.key, pin: entry.value }
  pinned:
    join: { left: deps, right: pins, on: { dependency: name }, unique: right }
    fields: { range: left.range, pin: right.pin }
  names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value }
  catalog:
    from: { paths: catalog.json }
    entries: { format: json, at: "$.packages" }
    fields: { name: entry.key }
rules:
  - id: locales-match-english
    kind: constraint
    input: messages
    assert: { set: { field: key, group_by: locale, equal_to: { group: en } } }
    level: error
  - id: ranges-match-pins
    kind: constraint
    input: pinned
    assert: { equal: { field: range, to: { field: pin } } }
    level: error
  - id: names-in-catalog
    kind: constraint
    input: names
    assert: { set: { field: name, subset_of: { relation: catalog, field: name } } }
    level: error
"#;

fn git(dir: &Path, args: &[&str]) {
    let out = crate::hermetic::git()
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

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn manifest(name: &str, react: &str) -> String {
    format!("{{\"name\": \"{name}\", \"dependencies\": {{\"react\": \"{react}\"}}}}\n")
}

/// A committed repository whose policies all pass, but for `fr`'s missing
/// `about` when `fr_complete` is false.
fn repository(fr_complete: bool) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, ".alint.yml", CONFIG);
    write(
        root,
        "locales/en.json",
        "{\"home\": \"Home\", \"about\": \"About\"}\n",
    );
    write(
        root,
        "locales/de.json",
        "{\"home\": \"Start\", \"about\": \"Info\"}\n",
    );
    let fr = if fr_complete {
        "{\"home\": \"Accueil\", \"about\": \"A propos\"}\n"
    } else {
        "{\"home\": \"Accueil\"}\n"
    };
    write(root, "locales/fr.json", fr);
    write(
        root,
        "packages/web/package.json",
        &manifest("web", "^18.2.0"),
    );
    write(
        root,
        "packages/api/package.json",
        &manifest("api", "^18.2.0"),
    );
    write(root, "pins.json", "{\"react\": \"^18.2.0\"}\n");
    write(
        root,
        "catalog.json",
        "{\"packages\": {\"web\": {}, \"api\": {}}}\n",
    );
    write(root, "README.md", "# repo\n");
    git(root, &["init", "-q"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    tmp
}

fn run(root: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(root)
        .env("NO_COLOR", "1")
        .output()
        .expect("run alint")
}

/// `alint check --format json` with `args`: its exit code, and each rule
/// that ran with the paths of its findings, in order.
fn check(root: &Path, args: &[&str]) -> (Option<i32>, Vec<(String, Vec<PathBuf>)>) {
    let mut command = vec!["check", "--format", "json"];
    command.extend_from_slice(args);
    let output = run(root, &command);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let rules = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| {
            assert_eq!(
                result["analysis_failures"],
                serde_json::json!([]),
                "{result}"
            );
            let paths = result["violations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|violation| PathBuf::from(violation["path"].as_str().unwrap()))
                .collect();
            (result["id"].as_str().unwrap().to_string(), paths)
        })
        .collect();
    (output.status.code(), rules)
}

/// Rule `id` with findings at `paths`, each written with `/` and built with
/// [`Path::join`], so it compares equal to the report's on Windows too.
fn ran(id: &str, paths: &[&str]) -> (String, Vec<PathBuf>) {
    let path = |written: &str| {
        written
            .split('/')
            .fold(PathBuf::new(), |path, part| path.join(part))
    };
    (
        id.to_string(),
        paths.iter().map(|written| path(written)).collect(),
    )
}

#[test]
fn a_change_to_one_locale_fires_findings_in_the_others() {
    let tmp = repository(true);
    let root = tmp.path();
    // English gains a key; French and German, unchanged, now lack it.
    write(
        root,
        "locales/en.json",
        "{\"home\": \"Home\", \"about\": \"About\", \"contact\": \"Contact\"}\n",
    );
    let expected = vec![ran(
        "locales-match-english",
        &["locales/de.json", "locales/fr.json"],
    )];
    assert_eq!(check(root, &["--changed"]), (Some(1), expected.clone()));

    // The same change committed on a branch, against its base.
    git(root, &["branch", "base"]);
    git(root, &["commit", "-qam", "contact"]);
    assert_eq!(check(root, &["--base", "base"]), (Some(1), expected));
}

#[test]
fn a_change_outside_every_selector_runs_no_constraint_rule() {
    let tmp = repository(false);
    let root = tmp.path();
    write(root, "README.md", "# repo, edited\n");
    assert_eq!(check(root, &["--changed"]), (Some(0), Vec::new()));
    // The finding is there for a full check.
    assert_eq!(
        check(root, &[]).1[0],
        ran("locales-match-english", &["locales/fr.json"])
    );
}

#[test]
fn a_deleted_file_in_the_scope_runs_the_rule_over_the_whole_snapshot() {
    let tmp = repository(false);
    let root = tmp.path();
    std::fs::remove_file(root.join("locales/de.json")).unwrap();
    assert_eq!(
        check(root, &["--changed"]),
        (
            Some(1),
            vec![ran("locales-match-english", &["locales/fr.json"])]
        )
    );
}

#[test]
fn a_change_to_a_joins_other_side_runs_the_rule() {
    let tmp = repository(true);
    let root = tmp.path();
    // Only the pin moves; the manifests, unchanged, now disagree with it.
    write(root, "pins.json", "{\"react\": \"^19.0.0\"}\n");
    assert_eq!(
        check(root, &["--changed"]),
        (
            Some(1),
            vec![ran(
                "ranges-match-pins",
                &["packages/api/package.json", "packages/web/package.json"]
            )]
        )
    );
}

#[test]
fn a_change_to_the_relation_a_set_compares_against_runs_the_rule() {
    let tmp = repository(true);
    let root = tmp.path();
    // The catalog drops `api`; its manifest, unchanged, is now outside it.
    write(root, "catalog.json", "{\"packages\": {\"web\": {}}}\n");
    assert_eq!(
        check(root, &["--changed"]),
        (
            Some(1),
            vec![ran("names-in-catalog", &["packages/api/package.json"])]
        )
    );
}

#[test]
fn a_change_to_the_configuration_alone_runs_no_constraint_rule() {
    let tmp = repository(false);
    let root = tmp.path();
    write(
        root,
        ".alint.yml",
        &CONFIG.replace("level: error", "level: warning"),
    );
    assert_eq!(check(root, &["--changed"]), (Some(0), Vec::new()));
    // A change in the scope beside it runs the rule, at its new level.
    write(
        root,
        "locales/de.json",
        "{\"about\": \"Info\", \"home\": \"Start\"}\n",
    );
    assert_eq!(
        check(root, &["--changed"]),
        (
            Some(0),
            vec![ran("locales-match-english", &["locales/fr.json"])]
        )
    );
}
