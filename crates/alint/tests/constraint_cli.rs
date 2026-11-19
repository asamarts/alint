//! The `constraint` kind on the command line (design
//! `v0.23/selectors-and-constraints.md` sections 6 and 11): each family's
//! findings in human and JSON output, a baseline that keeps them silenced
//! when their first row moves, and how `list`, `explain` and `rules` show
//! the kind and an inline `input:`.

use std::path::Path;
use std::process::Output;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(dir: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("COLUMNS", "200")
        .output()
        .unwrap()
}

fn text(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// A rule of each family over one relation of dependency ranges, one of
/// manifest names, and one of locale messages.
const CONFIG: &str = r#"version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  ranges:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { dependency: entry.key, range: entry.value, manifest: path }
  names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value, manifest: path }
  messages:
    from: { paths: messages.json }
    entries: { format: json, at: ["$.en", "$.fr"] }
    fields: { locale: entry.parent, message: entry.key }
rules:
  - id: one-range
    kind: constraint
    input: ranges
    assert: { unique: { key: [dependency], determines: [range] } }
    level: error
  - id: equal-ranges
    kind: constraint
    input: ranges
    assert: { equal: { field: range, group_by: dependency } }
    level: warning
  - id: names-unique
    kind: constraint
    input: names
    assert: { unique: { key: [name] } }
    message: "the name {{row.name}} is taken"
    level: error
  - id: root-range
    kind: constraint
    input: ranges
    assert: { equal: { field: range, to: { row: { manifest: packages/app/package.json } } } }
    level: warning
  - id: locales
    kind: constraint
    input: messages
    assert: { set: { field: message, group_by: locale, equal_to: { group: en } } }
    level: error
"#;

fn manifest(name: &str, range: &str) -> String {
    format!(
        "{{\n  \"name\": \"{name}\",\n  \"dependencies\": {{\n    \"react\": \"{range}\"\n  }}\n}}\n"
    )
}

fn tree(root: &Path) {
    write(root, ".alint.yml", CONFIG);
    write(
        root,
        "packages/app/package.json",
        &manifest("app", "^18.2.0"),
    );
    write(
        root,
        "packages/web/package.json",
        &manifest("web", "^18.2.0"),
    );
    write(
        root,
        "packages/old/package.json",
        &manifest("app", "^17.0.2"),
    );
    write(
        root,
        "messages.json",
        "{\n  \"en\": {\"home\": \"Home\", \"about\": \"About\"},\n  \"fr\": {\"home\": \"Accueil\"}\n}\n",
    );
}

#[test]
fn each_family_reports_in_human_and_json_output() {
    let dir = tempfile::tempdir().unwrap();
    tree(dir.path());
    let human = run(dir.path(), &["check"]);
    assert_eq!(human.status.code(), Some(1));
    let human = text(&human);
    for expected in [
        "rows with `dependency` \"react\" disagree on `range`: \"^18.2.0\" here",
        "related: a row with another value: packages/old/package.json:4:14-4:23",
        "related: another row with this value: packages/web/package.json:4:14-4:23",
        "the name app is taken",
        "related: the first row with this key: packages/app/package.json",
        "`range` is \"^17.0.2\" here, but \"^18.2.0\" in the source row",
        "related: the source row: packages/app/package.json:4:14-4:23",
        "`locale` \"fr\" lacks `message` \"about\", which `locale` \"en\" holds",
        "related: the reference group's row with it: messages.json:2:26-2:33",
    ] {
        assert!(human.contains(expected), "{expected:?} in:\n{human}");
    }
    assert!(
        String::from_utf8_lossy(&run(dir.path(), &["check"]).stderr)
            .contains("`version: 2` is experimental"),
        "the experimental warning"
    );

    let json: serde_json::Value =
        serde_json::from_slice(&run(dir.path(), &["check", "--format", "json"]).stdout).unwrap();
    let findings = |id: &str| -> Vec<(String, Option<u64>, String)> {
        json["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|result| result["id"] == id)
            .unwrap_or_else(|| panic!("no result for {id}"))["violations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|violation| {
                (
                    violation["path"].as_str().unwrap().to_string(),
                    violation["line"].as_u64(),
                    violation["message"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    };
    assert_eq!(
        findings("one-range"),
        [
            (
                "packages/app/package.json".to_string(),
                Some(4),
                "rows with `dependency` \"react\" disagree on `range`: \"^18.2.0\" here and in 1 \
                 other row; \"^17.0.2\" in 1 row"
                    .to_string()
            ),
            (
                "packages/old/package.json".to_string(),
                Some(4),
                "rows with `dependency` \"react\" disagree on `range`: \"^17.0.2\" here; \
                 \"^18.2.0\" in 2 rows"
                    .to_string()
            ),
        ]
    );
    // `equal` within groups reports as `unique` with `determines:` does.
    assert_eq!(
        findings("equal-ranges")
            .iter()
            .map(|(path, ..)| path.as_str())
            .collect::<Vec<_>>(),
        ["packages/app/package.json", "packages/old/package.json"]
    );
    assert_eq!(
        findings("names-unique"),
        [(
            "packages/old/package.json".to_string(),
            None,
            "the name app is taken".to_string()
        )]
    );
    assert_eq!(
        findings("root-range"),
        [(
            "packages/old/package.json".to_string(),
            Some(4),
            "`range` is \"^17.0.2\" here, but \"^18.2.0\" in the source row (`manifest` \
             \"packages/app/package.json\", at `dependencies.react` in packages/app/package.json)"
                .to_string()
        )]
    );
    assert_eq!(
        findings("locales"),
        [(
            "messages.json".to_string(),
            None,
            "`locale` \"fr\" lacks `message` \"about\", which `locale` \"en\" holds".to_string()
        )]
    );
}

#[test]
fn a_baseline_keeps_constraint_findings_silenced_when_their_first_row_moves() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    tree(root);
    assert_eq!(run(root, &["check"]).status.code(), Some(1));
    assert_eq!(run(root, &["baseline"]).status.code(), Some(0));
    let check = || run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(check().status.code(), Some(0), "{}", text(&check()));

    // A row that sorts first, and the first resource renamed: every finding
    // moves to another row or file, and keeps its identity.
    write(
        root,
        "packages/aaa/package.json",
        &manifest("aaa", "^18.2.0"),
    );
    std::fs::rename(root.join("packages/app"), root.join("packages/apps")).unwrap();
    let config = CONFIG.replace("packages/app/package.json", "packages/apps/package.json");
    write(root, ".alint.yml", &config);
    let moved = run(root, &["check", "--format", "json"]);
    let json: serde_json::Value = serde_json::from_slice(&moved.stdout).unwrap();
    let one_range = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["id"] == "one-range")
        .unwrap();
    assert_eq!(
        one_range["violations"][0]["path"], "packages/aaa/package.json",
        "the finding moved to the new first row"
    );
    assert_eq!(check().status.code(), Some(0), "{}", text(&check()));

    // A third range is a new finding; the baselined ones stay silenced.
    write(
        root,
        "packages/new/package.json",
        &manifest("new", "^16.0.0"),
    );
    let output = check();
    assert_eq!(output.status.code(), Some(1));
    let output = text(&output);
    assert!(output.contains("\"^16.0.0\" here"), "{output}");
    assert!(!output.contains("\"^17.0.2\" here"), "{output}");
}

#[test]
fn list_and_explain_show_an_inline_input_as_written() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        ".alint.yml",
        "version: 2\nrules:\n  - id: names-unique\n    kind: constraint\n    input:\n      \
         from: { paths: \"packages/*/package.json\" }\n      read: { json: \"$.name\" }\n      \
         fields: { name: value }\n    assert: { unique: { key: [name] } }\n    level: error\n",
    );
    let list = text(&run(dir.path(), &["list"]));
    assert_eq!(list, "error    names-unique  constraint\n");
    let json: serde_json::Value =
        serde_json::from_slice(&run(dir.path(), &["list", "--format", "json"]).stdout).unwrap();
    assert_eq!(json["relations"], serde_json::json!([]));
    let explain = text(&run(dir.path(), &["explain", "names-unique"]));
    assert!(
        explain.contains(
            "options:    input: {\"fields\":{\"name\":\"value\"},\"from\":{\"paths\":\
             \"packages/*/package.json\"},\"read\":{\"json\":\"$.name\"}}"
        ),
        "{explain}"
    );
    assert!(!explain.contains("__alint_internal"), "{explain}");
}

#[test]
fn rules_list_and_show_mark_the_kind_version_two_only_and_experimental() {
    let dir = tempfile::tempdir().unwrap();
    let list = text(&run(
        dir.path(),
        &["rules", "list", "--search", "constraint"],
    ));
    assert!(
        list.contains("constraint  Cross-file  [version 2 only, experimental]"),
        "{list}"
    );
    let show = text(&run(dir.path(), &["rules", "show", "constraint"]));
    assert!(
        show.contains("config:       version 2 only, experimental"),
        "{show}"
    );
    let json: serde_json::Value =
        serde_json::from_slice(&run(dir.path(), &["rules", "list", "--format", "json"]).stdout)
            .unwrap();
    for rule in json["rules"].as_array().unwrap() {
        let constraint = rule["kind"] == "constraint";
        assert_eq!(
            rule["config_version"],
            if constraint { 2 } else { 1 },
            "{rule}"
        );
        assert_eq!(rule["experimental"], constraint, "{rule}");
    }
    let show: serde_json::Value = serde_json::from_slice(
        &run(
            dir.path(),
            &["rules", "show", "constraint", "--format", "json"],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(show["config_version"], 2);
    assert_eq!(show["experimental"], true);
    // A kind of version 1 has no mark.
    let show = text(&run(dir.path(), &["rules", "show", "file_exists"]));
    assert!(!show.contains("config:"), "{show}");
}

#[test]
fn a_version_one_config_that_uses_the_kind_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        ".alint.yml",
        "version: 1\nrules:\n  - id: c\n    kind: constraint\n    input: names\n    assert: { \
         unique: { key: [name] } }\n    level: error\n",
    );
    let output = run(dir.path(), &["check"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "`rules[0]`: the `constraint` rule kind needs config `version: 2` (it is \
             experimental), and this config is version 1; declare `version: 2` to use it"
        ),
        "{stderr}"
    );
}

fn run_env(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = crate::hermetic::command(env!("CARGO_BIN_EXE_alint"));
    command
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("COLUMNS", "200");
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

/// Every surface a finding reaches, as text: human, JSON and SARIF output,
/// and the baseline file `alint baseline` writes.
fn surfaces(root: &Path, env: &[(&str, &str)]) -> Vec<(&'static str, String)> {
    let mut shown = Vec::new();
    for (name, format) in [("human", "human"), ("json", "json"), ("sarif", "sarif")] {
        let output = run_env(root, &["check", "--format", format], env);
        assert_eq!(output.status.code(), Some(1), "{name}: {}", text(&output));
        let stderr = String::from_utf8_lossy(&output.stderr);
        shown.push((name, format!("{}{stderr}", text(&output))));
    }
    let output = run_env(root, &["baseline"], env);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output));
    let baseline = std::fs::read_to_string(root.join(".alint-baseline.json")).unwrap();
    shown.push(("baseline", baseline));
    shown
}

/// A relation whose selection the environment writes may have read rows,
/// and so paths and values, the environment chose: the findings of a rule
/// that reads it, or reads a relation that reads it (`linked`), show
/// neither, as a version 1 rule's whose `paths:` the environment writes.
#[test]
fn a_relation_the_environment_selects_shows_no_path_or_value_of_its_findings() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  ranges:\n    from: { \
        paths: \"{{env.SECRET_DIR}}/*/package.json\" \
         }\n    entries: { format: json, at: \"$.dependencies\" }\n    fields: { dependency: \
         entry.key, range: entry.value }\n  wanted:\n    from: { paths: wanted.json }\n    \
         entries: { format: json, at: \"$\" }\n    fields: { name: entry.key }\n  linked:\n    \
         join: { left: ranges, right: wanted, on: { dependency: name }, unique: right }\n    \
         fields: { range: left.range }\nrules:\n  - id: same\n    kind: constraint\n    input: \
         ranges\n    assert: { equal: { field: range, group_by: dependency } }\n    level: \
         error\n  - id: linked\n    kind: constraint\n    input: linked\n    assert: { equal: \
         { field: range } }\n    level: error\n",
    );
    write(root, "pkgs/a/package.json", &manifest("a", "^18.2.0"));
    write(root, "pkgs/b/package.json", &manifest("b", "^17.0.2"));
    write(root, "wanted.json", "{\"react\": true}\n");
    let env = [("SECRET_DIR", "pkgs")];
    for (surface, shown) in surfaces(root, &env) {
        for secret in ["pkgs", "react", "^18.2.0", "^17.0.2"] {
            assert!(!shown.contains(secret), "{surface} shows {secret}: {shown}");
        }
        if surface != "baseline" {
            assert!(shown.contains("sensitive evidence"), "{surface}: {shown}");
        }
    }
    let output = run_env(root, &["check", "--format", "json"], &env);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    for result in json["results"].as_array().unwrap() {
        let violations = result["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 2, "{result}");
        for violation in violations {
            assert_eq!(violation["path"], "<redacted>", "{violation}");
            assert!(
                violation["related_locations"]
                    .as_array()
                    .is_none_or(Vec::is_empty),
                "{violation}"
            );
        }
    }
}

/// A `set` against another relation's field reads that relation too: when
/// the environment selects it, the rule's findings show no path or value,
/// though the rule's own input reads no environment.
#[test]
fn a_set_against_a_relation_the_environment_selects_shows_no_path_or_value() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  ranges:\n    from: { paths: \"packages/*/package.json\" }\n    \
         entries: { format: json, at: \"$.dependencies\" }\n    fields: { manifest: path, \
         dependency: entry.key }\n  denied:\n    from: { paths: \"{{env.SECRET_DIR}}/denied.json\" \
         }\n    entries: { format: json, at: \"$\" }\n    fields: { name: entry.key }\nrules:\n  \
         - id: denied\n    kind: constraint\n    input: ranges\n    assert: { set: { field: \
         dependency, group_by: manifest, disjoint_from: { relation: denied, field: name } } \
         }\n    level: error\n  - id: needed\n    kind: constraint\n    input: ranges\n    \
         assert: { set: { field: dependency, group_by: manifest, equal_to: { relation: denied, \
         field: name } } }\n    level: error\n",
    );
    write(root, "packages/a/package.json", &manifest("a", "^18.2.0"));
    write(
        root,
        "secret/denied.json",
        "{\"react\": true, \"left-pad\": true}\n",
    );
    let env = [("SECRET_DIR", "secret")];
    for (surface, shown) in surfaces(root, &env) {
        for secret in ["secret", "react", "left-pad", "packages/a"] {
            assert!(!shown.contains(secret), "{surface} shows {secret}: {shown}");
        }
        if surface != "baseline" {
            assert!(shown.contains("sensitive evidence"), "{surface}: {shown}");
        }
    }
    let output = run_env(root, &["check", "--format", "json"], &env);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let results = json["results"].as_array().unwrap();
    // `denied`'s shared `react`, and `needed`'s missing `left-pad`.
    assert_eq!(results.len(), 2, "{json}");
    for result in results {
        let violations = result["violations"].as_array().unwrap();
        assert_eq!(violations.len(), 1, "{result}");
        assert_eq!(violations[0]["path"], "<redacted>", "{result}");
        assert!(
            violations[0]["related_locations"]
                .as_array()
                .is_none_or(Vec::is_empty),
            "{result}"
        );
    }
}

/// A `set` against a sensitive field of another relation makes the compared
/// field sensitive, with every field that spells the same key: no surface
/// shows a value the two share, through `{{row.FIELD}}` of any of them.
#[test]
fn a_set_against_a_sensitive_field_hides_every_field_that_spells_its_values() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  deps:\n    from: { paths: \"packages/*/package.json\" }\n    \
         entries: { format: json, at: \"$.dependencies\" }\n    fields: { manifest: path, \
         dependency: entry.key, full: entry.path, parent: entry.parent, root: entry.root \
         }\n  denied:\n    from: { \
         paths: denied.json }\n    entries: { format: json, at: \"$\" }\n    fields: { name: { \
         source: entry.key, sensitive: true } }\nrules:\n  - id: denied\n    kind: \
         constraint\n    input: deps\n    assert: { set: { field: dependency, disjoint_from: { \
         relation: denied, field: name } } }\n    message: \"{{row.dependency}} is denied \
         ({{row.full}}, {{row.parent}}, {{row.root}}, {{row.manifest}})\"\n    level: error\n",
    );
    write(
        root,
        "packages/a/package.json",
        "{\n  \"dependencies\": {\n    \"secret-pkg\": \"1\"\n  }\n}\n",
    );
    write(root, "denied.json", "{\"secret-pkg\": true}\n");
    for (surface, shown) in surfaces(root, &[]) {
        assert!(
            !shown.contains("secret-pkg"),
            "{surface} shows the value: {shown}"
        );
        if surface == "json" {
            assert!(
                shown.contains(
                    "<redacted> is denied (<redacted>, <redacted>, <redacted>, \
                     packages/a/package.json)"
                ),
                "{shown}"
            );
        }
    }
}

/// A sensitive `entry.key` hides an `entry.path` beside it, which spells
/// the key, in a generated message, in `{{row.FIELD}}` and in the baseline.
#[test]
fn a_sensitive_key_shows_in_no_field_that_spells_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  tokens:\n    from: { paths: tokens.json }\n    entries: { \
         format: json, at: \"$\" }\n    fields:\n      k: { source: entry.key, sensitive: true \
         }\n      \
         p: entry.path\n      v: entry.value\nrules:\n  - id: paths\n    kind: constraint\n    \
         input: tokens\n    assert: { equal: { field: p } }\n    level: error\n  - id: values\n    \
         kind: constraint\n    input: tokens\n    assert: { unique: { key: [v] } }\n    message: \
         \"key {{row.k}} at {{row.p}}\"\n    level: error\n",
    );
    write(
        root,
        "tokens.json",
        "{\"SECRET_TOKEN_NAME_1\": \"x\", \"SECRET_TOKEN_NAME_2\": \"x\"}\n",
    );
    for (surface, shown) in surfaces(root, &[]) {
        assert!(!shown.contains("SECRET"), "{surface}: {shown}");
    }
    let human = text(&run(root, &["check"]));
    assert!(human.contains("key <redacted> at <redacted>"), "{human}");
}

/// Every value of a sensitive field has one key word, `sensitive`, never a
/// digest, which would let anyone who reads the baseline try values: the
/// findings are told apart by their count, so a third against a baselined
/// two is one new finding.
#[test]
fn findings_about_sensitive_values_share_a_key_and_are_counted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  owners:\n    from: { paths: owners.json }\n    entries: { \
         format: json, at: \"$\" }\n    fields:\n      name: entry.key\n      owner: { source: \
         entry.value, sensitive: true }\nrules:\n  - id: one-owner\n    kind: constraint\n    \
         input: owners\n    assert: { equal: { field: owner } }\n    level: error\n",
    );
    write(root, "owners.json", "{\"a\": \"s1\", \"b\": \"s2\"}\n");
    assert_eq!(run(root, &["baseline"]).status.code(), Some(0));
    // A header line and one entry, counted twice.
    let baseline = std::fs::read_to_string(root.join(".alint-baseline.json")).unwrap();
    let entries: Vec<serde_json::Value> = baseline
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(entries.len(), 1, "{baseline}");
    assert_eq!(entries[0]["count"], 2, "{baseline}");
    let check = || {
        let output = run(
            root,
            &[
                "check",
                "--baseline",
                ".alint-baseline.json",
                "--format",
                "json",
            ],
        );
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        json["results"][0]["violations"].as_array().unwrap().len()
    };
    assert_eq!(check(), 0);
    write(
        root,
        "owners.json",
        "{\"a\": \"s1\", \"b\": \"s2\", \"c\": \"s3\"}\n",
    );
    assert_eq!(check(), 1);
}
