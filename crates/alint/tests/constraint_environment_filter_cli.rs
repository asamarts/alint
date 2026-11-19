//! A `constraint` rule over a relation whose `where:` or `transform:` entry
//! takes a value from the environment (`{{env.X}}`): the rows that entry
//! keeps say where the value occurs, so the rule's findings show no path or
//! value on any surface, as for a relation whose selector the environment
//! writes (design `v0.23/selectors-and-constraints.md` section 10).

use std::path::Path;
use std::process::Output;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
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

/// Each surface a finding reaches, as text: human, JSON, SARIF and GitHub
/// output (stdout and stderr), and the baseline file `alint baseline`
/// writes; and the JSON report and the baseline's entries (a line each,
/// after its header), parsed.
struct Surfaces {
    shown: Vec<(&'static str, String)>,
    json: serde_json::Value,
    baseline: Vec<serde_json::Value>,
}

fn surfaces(root: &Path, env: &[(&str, &str)]) -> Surfaces {
    let mut shown = Vec::new();
    let mut json = serde_json::Value::Null;
    for format in ["human", "json", "sarif", "github"] {
        let output = run(root, &["check", "--format", format], env);
        let stdout = String::from_utf8(output.stdout.clone()).unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(1), "{format}: {stdout}{stderr}");
        if format == "json" {
            json = serde_json::from_str(&stdout).unwrap();
        }
        shown.push((format, format!("{stdout}{stderr}")));
    }
    let output = run(root, &["baseline"], env);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(root.join(".alint-baseline.json")).unwrap();
    let baseline = text
        .lines()
        .skip(1)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    shown.push(("baseline", text));
    std::fs::remove_file(root.join(".alint-baseline.json")).unwrap();
    Surfaces {
        shown,
        json,
        baseline,
    }
}

/// Every finding of the JSON report and every baseline entry has the path
/// `<redacted>`, and no finding has a related location; there is at least
/// one of each.
fn assert_paths_redacted(surfaces: &Surfaces) {
    let mut findings = 0;
    for result in surfaces.json["results"].as_array().unwrap() {
        for violation in result["violations"].as_array().unwrap() {
            findings += 1;
            assert_eq!(violation["path"], "<redacted>", "{violation}");
            assert!(
                violation["related_locations"]
                    .as_array()
                    .is_none_or(Vec::is_empty),
                "{violation}"
            );
        }
    }
    assert!(findings > 0, "{}", surfaces.json);
    assert_ne!(surfaces.baseline.len(), 0);
    for entry in &surfaces.baseline {
        assert_eq!(entry["path"], "<redacted>", "{entry}");
    }
}

/// A `where:` on a `path` field from the environment picks the one file the
/// rule reads: its findings name neither that file nor its directory.
#[test]
fn a_filter_from_the_environment_on_a_path_field_hides_the_findings_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  values:\n    from: { paths: \"**/*.json\" }\n    entries: { \
         format: json, at: \"$\" }\n    fields: { file: path, val: entry.value }\n    where: { \
         file: { in: [\"{{env.TARGET_FILE}}\"] } }\nrules:\n  - id: one-value\n    kind: \
         constraint\n    input: values\n    assert: { equal: { field: val } }\n    level: \
         error\n",
    );
    write(
        root,
        "secretdir/internal-codename-falcon.json",
        "{\"a\": \"first\", \"b\": \"second\"}\n",
    );
    write(root, "plain/other.json", "{\"a\": \"third\"}\n");
    let env = [("TARGET_FILE", "secretdir/internal-codename-falcon.json")];
    let surfaces = surfaces(root, &env);
    for (surface, shown) in &surfaces.shown {
        for secret in ["secretdir", "internal-codename-falcon", "first", "second"] {
            assert!(!shown.contains(secret), "{surface} shows {secret}: {shown}");
        }
    }
    assert_paths_redacted(&surfaces);
}

/// A `where:` on a field that is no path, an entry's key, from the
/// environment: the rows it keeps still say which files hold that key, so
/// the findings name none of them, nor the key.
#[test]
fn a_filter_from_the_environment_on_another_field_hides_the_findings_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  tokens:\n    from: { paths: \"**/*.json\" }\n    entries: { \
         format: json, at: \"$\" }\n    fields: { key: entry.key, val: entry.value }\n    \
         transform: { val: [trim] }\n    where: { key: { in: [\"{{env.SECRET_KEY}}\"] } \
         }\nrules:\n  - id: one-token\n    kind: constraint\n    input: tokens\n    assert: { \
         equal: { field: val } }\n    level: error\n",
    );
    write(
        root,
        "secretdir/a.json",
        "{\"falcon_token\": \"one\", \"other\": \"x\"}\n",
    );
    write(root, "secretdir/b.json", "{\"falcon_token\": \"two\"}\n");
    write(root, "plain/c.json", "{\"other\": \"y\"}\n");
    let env = [("SECRET_KEY", "falcon_token")];
    let surfaces = surfaces(root, &env);
    for (surface, shown) in &surfaces.shown {
        for secret in ["secretdir", "falcon_token", "\"one\"", "\"two\""] {
            assert!(!shown.contains(secret), "{surface} shows {secret}: {shown}");
        }
    }
    assert_paths_redacted(&surfaces);
}

/// A `transform:` from the environment changes the values a rule compares,
/// and so which rows it reports: its findings show no path either.
#[test]
fn a_transform_from_the_environment_hides_the_findings_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  names:\n    from: { paths: \"**/*.json\" }\n    entries: { \
         format: json, at: \"$\" }\n    fields: { key: entry.key, val: entry.value }\n    \
         transform: { val: [\"{{env.FOLD}}\"] }\nrules:\n  - id: unique-names\n    kind: \
         constraint\n    input: names\n    assert: { unique: { key: [val] } }\n    level: \
         error\n",
    );
    write(root, "secretdir/a.json", "{\"n\": \"Falcon\"}\n");
    write(root, "secretdir/b.json", "{\"n\": \"FALCON\"}\n");
    let env = [("FOLD", "lower")];
    let surfaces = surfaces(root, &env);
    for (surface, shown) in &surfaces.shown {
        for secret in ["secretdir", "alcon", "ALCON"] {
            assert!(!shown.contains(secret), "{surface} shows {secret}: {shown}");
        }
    }
    assert_paths_redacted(&surfaces);
}

/// A relation that reads one whose `where:` the environment writes, as a
/// join side, through a membership reference, or as a `set`'s target, may
/// hold rows that filter chose: a rule over it shows no path either.
#[test]
fn a_relation_reading_one_the_environment_filters_hides_the_findings_paths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  secret:\n    from: { paths: \"secretdir/*.json\" }\n    \
         entries: { format: json, at: \"$\" }\n    fields: { key: entry.key, val: entry.value \
         }\n    where: { key: { in: [\"{{env.SECRET_KEY}}\"] } }\n  plain:\n    from: { paths: \
         \"plain/*.json\" }\n    entries: { format: json, at: \"$\" }\n    fields: { key: \
         entry.key, val: entry.value, file: path }\n  joined:\n    join: { left: plain, right: \
         secret, on: { val: val }, unique: right }\n    fields: { val: left.val, file: left.file \
         }\n  member:\n    from: { paths: \"plain/*.json\" }\n    entries: { format: json, at: \
         \"$\" }\n    fields: { val: entry.value, file: path }\n    where: { val: { in: [ { \
         relation: secret, field: val } ] } }\nrules:\n  - id: joined\n    kind: constraint\n    \
         input: joined\n    assert: { unique: { key: [val] } }\n    level: error\n  - id: \
         member\n    kind: constraint\n    input: member\n    assert: { unique: { key: [val] } \
         }\n    level: error\n  - id: target\n    kind: constraint\n    input: plain\n    \
         assert: { set: { field: val, disjoint_from: { relation: secret, field: val } } }\n    \
         level: error\n",
    );
    write(root, "secretdir/a.json", "{\"falcon_token\": \"hush\"}\n");
    write(root, "plain/b.json", "{\"x\": \"hush\", \"y\": \"hush\"}\n");
    let env = [("SECRET_KEY", "falcon_token")];
    let surfaces = surfaces(root, &env);
    for (surface, shown) in &surfaces.shown {
        for secret in ["secretdir", "falcon_token", "plain/b.json", "hush"] {
            assert!(!shown.contains(secret), "{surface} shows {secret}: {shown}");
        }
    }
    assert_paths_redacted(&surfaces);
    let mut rules: Vec<&str> = surfaces.json["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["id"].as_str().unwrap_or_default())
        .collect();
    rules.sort_unstable();
    assert_eq!(rules, ["joined", "member", "target"], "{}", surfaces.json);
}
