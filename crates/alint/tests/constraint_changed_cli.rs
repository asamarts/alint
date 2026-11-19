//! `alint check --changed` and `--base` with `constraint` rules, in a real Git
//! repository (design `v0.23/selectors-and-constraints.md` section 9). A rule
//! runs when a changed path, added, modified or deleted, is one the selectors
//! of its relations read: its input's, a join's other side's, or the relation
//! its `set` compares against. It then judges the whole snapshot and reports
//! every finding, also those in unchanged files. A change outside those
//! selectors, or to `.alint.yml` alone, runs no `constraint` rule. A rename
//! changes both its paths, so a file renamed out of a selector runs the rule.

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
/// that ran with the paths of its findings, in order (a pathless finding's as
/// an empty path).
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
                .map(|violation| PathBuf::from(violation["path"].as_str().unwrap_or_default()))
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

/// A catalog whose every package must be a manifest's name, beside a
/// version 1 kind that reads the full index through a path scope.
const RENAME_CONFIG: &str = r#"version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value }
  catalog:
    from: { paths: catalog.json }
    entries: { format: json, at: "$.packages" }
    fields: { name: entry.key }
rules:
  - id: catalog-names-packages
    kind: constraint
    input: catalog
    assert: { set: { field: name, subset_of: { relation: names, field: name } } }
    level: error
  - id: license-exists
    kind: file_exists
    paths: LICENSE
    level: error
"#;

/// A committed repository, on a branch `base` as well, whose policies in
/// [`RENAME_CONFIG`] pass.
fn rename_repository() -> tempfile::TempDir {
    let tmp = repository(true);
    let root = tmp.path();
    write(root, ".alint.yml", RENAME_CONFIG);
    write(root, "LICENSE", "MIT\n");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "rename policies"]);
    git(root, &["branch", "base"]);
    tmp
}

#[test]
fn a_file_renamed_out_of_a_selector_runs_the_rule_that_reads_it() {
    let tmp = rename_repository();
    let root = tmp.path();
    // `api` leaves `packages/`, so the catalog names a package no manifest
    // there declares. Git lists the rename under its new path alone, which no
    // selector reads; its old path is the one in `manifests`.
    std::fs::create_dir_all(root.join("legacy").join("api")).unwrap();
    git(
        root,
        &["mv", "packages/api/package.json", "legacy/api/package.json"],
    );
    let expected = vec![ran("catalog-names-packages", &["catalog.json"])];
    assert_eq!(
        check(root, &["--changed"]),
        (Some(1), expected.clone()),
        "staged"
    );
    git(root, &["commit", "-qm", "move api"]);
    assert_eq!(
        check(root, &["--base", "base"]),
        (Some(1), expected),
        "committed"
    );
}

#[test]
fn a_renamed_required_file_runs_a_version_one_full_index_rule() {
    let tmp = rename_repository();
    let root = tmp.path();
    // `file_exists` reads the whole index but runs under `--changed` only
    // when a changed path is in its `paths:`: here the rename's old path.
    git(root, &["mv", "LICENSE", "LICENSE.txt"]);
    let expected = vec![ran("license-exists", &[""])];
    assert_eq!(
        check(root, &["--changed"]),
        (Some(1), expected.clone()),
        "staged"
    );
    git(root, &["commit", "-qm", "rename the license"]);
    assert_eq!(
        check(root, &["--base", "base"]),
        (Some(1), expected),
        "committed"
    );
}

/// A locale policy over the locales of `dir`.
fn locales_below(dir: &str) -> String {
    format!(
        "version: 2\nrelations:\n  messages:\n    from: {{ paths: \"{dir}/locales/*.json\" }}\n    \
         entries: {{ format: json, at: \"$\" }}\n    fields: {{ locale: stem, key: entry.key }}\n\
         rules:\n  - id: locales-match-english\n    kind: constraint\n    input: messages\n    \
         assert: {{ set: {{ field: key, group_by: locale, equal_to: {{ group: en }} }} }}\n    \
         level: error\n"
    )
}

/// A committed repository at `root` of English and German locales that
/// agree.
fn locale_repository(root: &Path) {
    write(root, "locales/en.json", "{\"home\": \"Home\"}\n");
    write(root, "locales/de.json", "{\"home\": \"Start\"}\n");
    git(root, &["init", "-q"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "locales"]);
}

/// English gains a key in the repository at `root`, committed there.
fn english_gains_a_key(root: &Path) {
    write(
        root,
        "locales/en.json",
        "{\"home\": \"Home\", \"about\": \"About\"}\n",
    );
    git(root, &["commit", "-qam", "about"]);
}

/// A policy that the keys `used.json` names are keys of English in the
/// submodule `sub`.
const KEYS_IN_SUBMODULE: &str = "version: 2\nrelations:\n  used:\n    from: { paths: used.json }\n    \
     entries: { format: json, at: \"$\" }\n    fields: { key: entry.key }\n  english:\n    \
     from: { paths: sub/locales/en.json }\n    entries: { format: json, at: \"$\" }\n    \
     fields: { key: entry.key }\nrules:\n  - id: keys-in-english\n    kind: constraint\n    \
     input: used\n    assert: { set: { field: key, subset_of: { relation: english, field: key } } }\n    \
     level: error\n";

/// Git lists a submodule whose commit moved, or that was removed, by its path
/// alone, though the walk reads (or read) the files in it: the change runs
/// every rule whose selectors can match below it, and no other.
#[test]
fn a_submodule_change_runs_the_rules_that_read_below_it() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("source");
    locale_repository(&source);
    let root = tmp.path().join("main");
    write(&root, ".alint.yml", &locales_below("sub"));
    git(&root, &["init", "-q"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    let source = source.to_str().unwrap();
    for path in ["sub", "vendor"] {
        git(
            &root,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                source,
                path,
            ],
        );
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);
    git(&root, &["branch", "base"]);

    // `vendor` moves: no selector reads below it.
    english_gains_a_key(&root.join("vendor"));
    git(&root, &["add", "vendor"]);
    assert_eq!(check(&root, &["--changed"]), (Some(0), Vec::new()));

    // `sub` moves: German, unchanged, now lacks the key English gained.
    // Checked out, then staged, then committed.
    english_gains_a_key(&root.join("sub"));
    let expected = vec![ran("locales-match-english", &["sub/locales/de.json"])];
    assert_eq!(check(&root, &["--changed"]), (Some(1), expected.clone()));
    git(&root, &["add", "sub"]);
    assert_eq!(check(&root, &["--changed"]), (Some(1), expected.clone()));
    git(&root, &["commit", "-qm", "move sub"]);
    assert_eq!(check(&root, &["--base", "base"]), (Some(1), expected));

    // `sub` goes, which the walk then holds nothing of: the keys `used.json`
    // names are no longer English's. Deleted, then removed from the index,
    // then committed.
    write(&root, ".alint.yml", KEYS_IN_SUBMODULE);
    write(&root, "used.json", "{\"home\": 1}\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "use"]);
    git(&root, &["branch", "used"]);
    assert_eq!(
        check(&root, &[]),
        (Some(0), vec![ran("keys-in-english", &[])])
    );
    std::fs::remove_dir_all(root.join("sub")).unwrap();
    let expected = vec![ran("keys-in-english", &["used.json"])];
    assert_eq!(check(&root, &["--changed"]), (Some(1), expected.clone()));
    git(&root, &["rm", "-q", "sub"]);
    assert_eq!(check(&root, &["--changed"]), (Some(1), expected.clone()));
    git(&root, &["commit", "-qm", "remove sub"]);
    assert_eq!(check(&root, &["--base", "used"]), (Some(1), expected));
}

/// Git lists an untracked repository as `dir/`, and none of its files: the
/// change runs every rule whose selectors can match below it, and no other.
#[test]
fn an_untracked_repository_runs_the_rules_that_read_below_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, ".alint.yml", &locales_below("nested"));
    git(root, &["init", "-q"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);

    let elsewhere = root.join("elsewhere");
    locale_repository(&elsewhere);
    english_gains_a_key(&elsewhere);
    assert_eq!(check(root, &["--changed"]), (Some(0), Vec::new()));

    let nested = root.join("nested");
    locale_repository(&nested);
    english_gains_a_key(&nested);
    assert_eq!(
        check(root, &["--changed"]),
        (
            Some(1),
            vec![ran("locales-match-english", &["nested/locales/de.json"])]
        )
    );
}

/// A manifest that a trigger scope reads only to decide whether its rule runs
/// warns of nothing when it lists no paths: the warning that a rule's own
/// manifest scope selects nothing is not about a selector's, which no full
/// check warns of either.
#[test]
fn a_trigger_scopes_empty_manifest_warns_of_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nselectors:\n  members:\n    paths: \"workspace/*/member.json\"\n    \
         scope_filter:\n      include_manifest_paths:\n        source: \"{{env.MANIFEST}}\"\n        \
         extract: { json: \"$.members[*]\" }\nrelations:\n  names:\n    from: members\n    \
         read: { json: \"$.name\" }\n    fields: { name: value }\nrules:\n  - id: names-unique\n    \
         kind: constraint\n    input: names\n    assert: { unique: { key: [name] } }\n    \
         level: error\n",
    );
    write(root, "members.json", "{\"members\": []}\n");
    write(root, "workspace/a/member.json", "{\"name\": \"a\"}\n");
    write(root, "workspace/b/member.json", "{\"name\": \"a\"}\n");
    git(root, &["init", "-q"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    // A path the selector's globs match, outside the manifest's members: the
    // trigger scope reads the manifest to find that the rule does not run.
    write(
        root,
        "workspace/a/member.json",
        "{\"name\": \"a\", \"x\": 1}\n",
    );
    for args in [&["check", "--changed"][..], &["check"]] {
        let output = crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
            .args(args)
            .current_dir(root)
            .env("NO_COLOR", "1")
            .env("MANIFEST", "members.json")
            .output()
            .expect("run alint");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(0), "{args:?}: {stderr}");
        assert!(
            !stderr.contains("resolved to no paths"),
            "{args:?}: {stderr}"
        );
    }
}
