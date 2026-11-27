//! End-to-end CLI integration tests for v0.24 deliverables across real
//! `alint check`, `alint baseline`, `alint fix --changed`, and all output
//! formats (`human`, `json`, `sarif`, `junit`, `agent`).

use std::fmt::Write as _;
use std::path::Path;
use std::process::Output;

use serde_json::Value;

fn alint_bin() -> &'static str {
    env!("CARGO_BIN_EXE_alint")
}

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn run(root: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(alint_bin())
        .current_dir(root)
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("run alint")
}

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

#[test]
#[allow(clippy::too_many_lines)]
fn v024_phase3_constraints_and_no_duplicate_keys_across_formats_and_baseline() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    write(root, "teams.json", r#"{"teams":[{"name":"core"}]}"#);
    write(
        root,
        "services.json",
        r#"{
  "services": [
    { "name": "auth", "team": "ghost", "min_v": "2.0.0", "cur_v": "1.5.0", "range": ">=2.0.0" }
  ]
}
"#,
    );
    write(root, "dup.yaml", "a: 1\na: 2\n");
    write(
        root,
        ".alint.yml",
        r#"version: 2
relations:
  teams:
    from: { paths: teams.json }
    records: { format: json, at: "$.teams[*]" }
    fields: { name: "record: $.name" }
  services:
    from: { paths: services.json }
    records: { format: json, at: "$.services[*]" }
    fields:
      name: "record: $.name"
      team: "record: $.team"
      min_v: "record: $.min_v"
      cur_v: "record: $.cur_v"
      range: "record: $.range"
rules:
  - id: ref-team
    kind: constraint
    input: services
    assert:
      reference:
        fields: [team]
        to: { relation: teams, fields: [name] }
    level: error
  - id: count-min-two
    kind: constraint
    input: services
    assert:
      count: { min: 2 }
    level: error
  - id: cmp-ver
    kind: constraint
    input: services
    assert:
      compare: { field: cur_v, op: ge, to: { field: min_v }, order: semver }
    level: error
  - id: sat-ver
    kind: constraint
    input: services
    assert:
      satisfies: { version: cur_v, range: range, dialect: node }
    level: error
  - id: dup-keys
    kind: no_duplicate_keys
    paths: "dup.yaml"
    level: error
"#,
    );

    // Every major format succeeds with exit 1 and well-formed structured output.
    for format in ["human", "json", "sarif", "junit", "agent"] {
        let out = run(root, &["check", "--format", format]);
        assert_eq!(
            out.status.code(),
            Some(1),
            "format={format} stderr={}",
            String::from_utf8_lossy(&out.stderr)
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        match format {
            "json" | "agent" => {
                let parsed: Value = serde_json::from_str(&stdout).unwrap();
                assert_eq!(parsed["summary"]["total_violations"], 5, "{parsed}");
            }
            "sarif" => {
                let parsed: Value = serde_json::from_str(&stdout).unwrap();
                assert_eq!(
                    parsed["runs"][0]["results"].as_array().unwrap().len(),
                    5,
                    "{parsed}"
                );
            }
            "junit" => {
                assert!(stdout.contains("<testsuites") && stdout.contains("ref-team"));
            }
            "human" => {
                for id in [
                    "ref-team",
                    "count-min-two",
                    "cmp-ver",
                    "sat-ver",
                    "dup-keys",
                ] {
                    assert!(stdout.contains(id), "missing {id} in:\n{stdout}");
                }
            }
            _ => unreachable!(),
        }
    }

    // `alint baseline` writes a valid baseline that suppresses all 5 findings.
    let bl = run(root, &["baseline"]);
    assert_eq!(
        bl.status.code(),
        Some(0),
        "baseline stderr={}",
        String::from_utf8_lossy(&bl.stderr)
    );
    let check_bl = run(root, &["check", "--baseline", ".alint-baseline.json"]);
    assert_eq!(
        check_bl.status.code(),
        Some(0),
        "baselined check stderr={}",
        String::from_utf8_lossy(&check_bl.stderr)
    );
}

#[test]
fn large_over_64k_hcl_and_multi_rule_alias_heavy_yaml_succeed_through_cli() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    // >64 KiB HCL file (formerly rejected by v0.23's 64 KiB cap; now parses up to 4 MiB).
    let mut big_tf = String::with_capacity(80 * 1024);
    for i in 0..1_200 {
        let _ = writeln!(
            big_tf,
            "locals {{\n  key_{i:04} = \"value_{i:04}_padding_0123456789abcdef\"\n}}"
        );
    }
    big_tf.push_str("resource \"aws_s3_bucket\" \"logs\" {\n  bucket = \"acme-logs\"\n}\n");
    assert!(big_tf.len() > 65_536);
    write(root, "large.tf", &big_tf);

    // Multi-rule alias-heavy YAML exercising shared structured cache + RetainedLease (#548/#582).
    let mut alias_yaml = String::from("base: &b\n  env: prod\n  tier: web\nitems:\n");
    for i in 0..32 {
        let _ = writeln!(alias_yaml, "  s{i}:\n    <<: *b\n    name: svc-{i}");
    }
    write(root, "services.yaml", &alias_yaml);

    write(
        root,
        ".alint.yml",
        r#"version: 1
rules:
  - id: big-hcl-ok
    kind: hcl_path_equals
    paths: "large.tf"
    path: "$.resource.aws_s3_bucket.logs.bucket"
    equals: "acme-logs"
    level: error
  - id: yaml-env
    kind: yaml_path_equals
    paths: "services.yaml"
    path: "$.items.s31.env"
    equals: "prod"
    level: error
  - id: yaml-tier
    kind: yaml_path_equals
    paths: "services.yaml"
    path: "$.items.s31.tier"
    equals: "web"
    level: error
  - id: yaml-name
    kind: yaml_path_matches
    paths: "services.yaml"
    path: "$.items.s0.name"
    matches: "^svc-0$"
    level: error
"#,
    );

    let out = run(root, &["check"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn doubly_nested_for_each_and_templated_scope_filter_manifest_prepare_dependencies() {
    // Audit F9 + F10:
    // 1. Nested rule with templated `scope_filter.include_manifest_paths.source: "{path}/manifest.json"`
    //    must prepare real manifest paths instead of placeholder `_path_/manifest.json`.
    // 2. Doubly-nested `for_each_dir` with inner `select: "{path}/pkgs/*"` must prepare
    //    grandchild `scope_filter` dependencies cleanly.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    write(
        root,
        "groups/g1/manifest.json",
        r#"{"files": ["pkgs/p1/keep.txt"]}"#,
    );
    write(root, "groups/g1/pkgs/p1/keep.txt", "BAD_TOKEN\n");
    write(root, "groups/g1/pkgs/p1/ignored.txt", "BAD_TOKEN\n");
    write(
        root,
        ".alint.yml",
        r#"version: 1
rules:
  - id: templated-manifest-scope
    kind: for_each_dir
    select: "groups/*"
    level: error
    require:
      - kind: file_content_forbidden
        paths: "{path}/**/*.txt"
        pattern: "BAD_TOKEN"
        scope_filter:
          include_manifest_paths:
            source: "{path}/manifest.json"
            extract: { json: "$.files[*]" }
  - id: doubly-nested-scope
    kind: for_each_dir
    select: "groups/*"
    level: error
    require:
      - kind: for_each_dir
        select: "{path}/pkgs/*"
        require:
          - kind: file_content_forbidden
            paths: "{path}/**/*.txt"
            pattern: "BAD_TOKEN"
            scope_filter:
              include_manifest_paths:
                source: "groups/g1/manifest.json"
                extract: { json: "$.files[*]" }
"#,
    );

    let out = run(root, &["check", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let parsed: Value = serde_json::from_slice(&out.stdout).unwrap();
    for rule_id in ["templated-manifest-scope", "doubly-nested-scope"] {
        let r = parsed["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == rule_id)
            .unwrap();
        let paths: Vec<&str> = r["violations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v["path"].as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["groups/g1/pkgs/p1/keep.txt"],
            "rule {rule_id} must match ONLY manifest-listed file: {r}"
        );
    }
}

#[test]
fn fix_changed_runs_full_index_markdown_paths_resolve_even_when_target_file_changed() {
    // Audit F7: `fix --changed` must not skip a rule with `requires_full_index() == true`
    // (such as `markdown_paths_resolve`) when a non-markdown target file is deleted!
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();

    write(root, "docs/guide.md", "See `src/helper.rs` for details.\n");
    write(root, "src/helper.rs", "pub fn help() {}\n");
    write(
        root,
        ".alint.yml",
        r#"version: 1
rules:
  - id: md-paths
    kind: markdown_paths_resolve
    paths: "docs/**/*.md"
    prefixes: ["src/"]
    level: error
"#,
    );
    git(root, &["init", "-q"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);

    // Delete the referenced target file without touching docs/guide.md.
    std::fs::remove_file(root.join("src/helper.rs")).unwrap();

    let check_out = run(root, &["check", "--changed", "--format", "json"]);
    assert_eq!(check_out.status.code(), Some(1));
    let fix_out = run(root, &["fix", "--changed", "--dry-run", "--format", "json"]);
    let parsed: Value = serde_json::from_slice(&fix_out.stdout).unwrap();
    let items: usize = parsed["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["id"] == "md-paths")
        .map(|r| r["items"].as_array().unwrap().len())
        .sum();
    assert_eq!(
        items, 1,
        "fix --changed must evaluate full-index markdown_paths_resolve on target deletion: {parsed}"
    );
}
