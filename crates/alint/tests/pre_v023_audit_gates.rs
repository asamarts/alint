//! Regression gates for the pre-v0.23 comprehensive audit:
//! - v1-into-v2 `constraint` `assert:` literal-name enforcement
//! - `close_sensitivity` sibling propagation in non-join relations and
//!   transitive multi-pair joins (`on: { a: x, b: x }`)
//! - v2 selector secret-bearing manifest `scope_filter` redaction
//! - `constraint` `visit_sensitive_paths` (neither under-protecting nor
//!   over-suppressing unrelated files in `fix --diff` / `path_contains_sensitive_evidence`)
//! - `for_each_dir` nested rules with secret-bearing manifest `scope_filter`
//! - `constraint` `assert.set` sensitive end-column span hiding
//! - SARIF + `--baseline` repeated-fingerprint ordering parity (`a.json` vs `a/b.json`)
//! - `extract::yaml_rows` block-scalar start offsets on non-ASCII UTF-8 files

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use alint_core::baseline::{
    Baseline, FingerprintedViolation, apply as apply_baseline, fingerprint as baseline_fingerprint,
};
use alint_core::{Applicability, Engine, RuleEntry, WalkOptions, walk};
use alint_dsl::{EnvironmentProvider, LoadOptions, load_with};
use alint_output::{
    BaselineMarks, ResultMarks, SuppressedFinding, write_sarif_with_baseline,
    write_sarif_with_fingerprints,
};

#[derive(Debug)]
struct MapEnv(BTreeMap<String, String>);

impl MapEnv {
    fn from_pairs<const N: usize>(pairs: [(&str, &str); N]) -> Self {
        Self(
            pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        )
    }
}

impl EnvironmentProvider for MapEnv {
    fn get(&self, name: &str) -> Option<String> {
        self.0.get(name).cloned()
    }
}

fn build_engine(root: &Path) -> Engine {
    let registry = alint_rules::builtin_registry();
    let opts = LoadOptions::default()
        .with_kind_catalog(Some(registry.catalog()))
        .with_global_git_excludes(false);
    let cfg = load_with(&root.join(".alint.yml"), &opts).unwrap();
    let mut entries = Vec::with_capacity(cfg.rules.len());
    for spec in &cfg.rules {
        let mut rule = registry.build(spec).unwrap();
        let allow_out_of_root = cfg.allow_out_of_root.allows_rule(spec);
        rule.set_allow_out_of_root(allow_out_of_root);
        let entry = RuleEntry::new(rule)
            .with_spec(Arc::new(spec.clone()))
            .with_allow_out_of_root(allow_out_of_root);
        entries.push(entry);
    }
    Engine::from_entries(entries, registry)
        .with_facts(cfg.facts)
        .with_vars(cfg.vars)
}

fn load_and_run(root: &Path) -> alint_core::Report {
    let engine = build_engine(root);
    let index = walk(
        root,
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    engine.run(root, &index).unwrap()
}

#[test]
fn v1_config_cannot_inject_constraint_assert_into_v2_config() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("legacy.yml"),
        r#"version: 1
templates:
  - id: bad-tpl
    assert:
      unique:
        key: ["{{env.SECRET_KEY}}"]
"#,
    )
    .unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        r#"version: 2
extends:
  - url: legacy.yml
selectors:
  files:
    paths: "*.json"
relations:
  entries:
    from: files
    entries: { format: json, at: "$" }
    fields:
      name: entry.key
rules:
  - id: composed-constraint
    extends_template: bad-tpl
    kind: constraint
    input: entries
    level: error
"#,
    )
    .unwrap();
    let registry = alint_rules::builtin_registry();
    let env = MapEnv::from_pairs([("SECRET_KEY", "name")]);
    let opts = LoadOptions::default()
        .with_kind_catalog(Some(registry.catalog()))
        .with_environment(Arc::new(env))
        .with_global_git_excludes(false);
    let err = load_with(&tmp.path().join(".alint.yml"), &opts)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("assert") && err.contains("version 2"),
        "expected v1 assert rejection, got: {err}"
    );
}

#[test]
fn close_sensitivity_propagates_to_non_join_siblings_and_multi_pair_joins() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.json"), r#"{"k": "SUPER_SECRET_123"}"#).unwrap();
    std::fs::write(tmp.path().join("b.json"), r#"{"k": "SUPER_SECRET_123"}"#).unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        r#"version: 2
selectors:
  files:
    paths: "*.json"
relations:
  siblings:
    from: files
    entries: { format: json, at: "$" }
    fields:
      secret_val:
        source: entry.value
        sensitive: true
      copy_val: entry.value
      key: entry.key
  left_rel:
    from: files
    entries: { format: json, at: "$" }
    fields:
      a:
        source: entry.value
        sensitive: true
      b: entry.value
      file: path
  right_rel:
    from: files
    entries: { format: json, at: "$" }
    fields:
      x: entry.value
      file: path
  joined:
    join:
      left: left_rel
      right: right_rel
      on:
        a: x
        b: x
      unique: right
    fields:
      leaked_b: left.b
      leaked_x: right.x
rules:
  - id: sibling-unique
    kind: constraint
    input: siblings
    assert:
      unique:
        key: [copy_val]
    level: error
  - id: join-unique
    kind: constraint
    input: joined
    assert:
      unique:
        key: [leaked_b]
    level: error
"#,
    )
    .unwrap();

    let registry = alint_rules::builtin_registry();
    let opts = LoadOptions::default()
        .with_kind_catalog(Some(registry.catalog()))
        .with_global_git_excludes(false);
    let cfg = load_with(&tmp.path().join(".alint.yml"), &opts).unwrap();
    let plan = cfg.relation_plan.as_ref().expect("v2 plan present");
    let siblings = plan
        .relations
        .iter()
        .find(|r| r.name == "siblings")
        .unwrap();
    assert!(
        siblings.relation.field("copy_val").unwrap().sensitive,
        "sibling field bound to same EntryValue must inherit sensitivity"
    );
    assert!(
        !siblings.relation.field("key").unwrap().sensitive,
        "entry.key must not be over-tainted when only entry.value is sensitive"
    );
    let joined = plan.relations.iter().find(|r| r.name == "joined").unwrap();
    assert!(
        joined.relation.field("leaked_b").unwrap().sensitive,
        "multi-pair join must transitively propagate sensitivity to left.b"
    );
    assert!(
        joined.relation.field("leaked_x").unwrap().sensitive,
        "multi-pair join must propagate sensitivity to right.x"
    );

    let report = load_and_run(tmp.path());
    let rendered = format!("{report:?}");
    assert!(
        !rendered.contains("SUPER_SECRET_123"),
        "sensitive value leaked into constraint report: {rendered}"
    );
}

#[test]
fn v2_selector_with_env_manifest_scope_filter_redacts_constraint_findings() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".env"), "TARGET=secret_svc.json\n").unwrap();
    std::fs::write(tmp.path().join("allowed.json"), r#"{"ok": "allowed"}"#).unwrap();
    std::fs::write(tmp.path().join("secret_svc.json"), r#"{"name": "bad"}"#).unwrap();
    std::fs::write(tmp.path().join("other.json"), r#"{"name": "bad"}"#).unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        r#"version: 2
selectors:
  from_env:
    paths: "*.json"
    scope_filter:
      include_manifest_paths:
        source: .env
        extract:
          dotenv: "$.TARGET"
  allowed_file:
    paths: "allowed.json"
relations:
  names:
    from: from_env
    entries: { format: json, at: "$" }
    fields:
      k: entry.key
      v: entry.value
  allowed:
    from: allowed_file
    entries: { format: json, at: "$" }
    fields:
      val: entry.value
rules:
  - id: env-scoped-constraint
    kind: constraint
    input: names
    assert:
      set:
        field: v
        subset_of:
          relation: allowed
          field: val
    level: error
"#,
    )
    .unwrap();

    let report = load_and_run(tmp.path());
    let result = &report.results[0];
    assert_eq!(result.violations.len(), 1, "{result:?}");
    let v = &result.violations[0];
    assert!(
        v.path_is_sensitive(),
        "path must be marked sensitive: {v:?}"
    );
    assert_eq!(v.report_path(), Some(Path::new("<redacted>")));
    assert_eq!(v.report_line(), None);
    assert!(!v.message.contains("secret_svc.json"), "{v:?}");
}

#[test]
fn constraint_sensitive_path_gating_neither_over_suppresses_nor_under_protects() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("secret.json"),
        r#"{"tok": "SECRET_VAL_LONG"}"#,
    )
    .unwrap();
    std::fs::write(tmp.path().join("allowed.json"), r#"{"ok": "OK"}"#).unwrap();
    std::fs::write(tmp.path().join("plain.txt"), "hello   \n").unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        r#"version: 2
selectors:
  secrets:
    paths: "secret.json"
  allowed_file:
    paths: "allowed.json"
relations:
  tokens:
    from: secrets
    entries: { format: json, at: "$" }
    fields:
      tok:
        source: entry.value
        sensitive: true
  allowed:
    from: allowed_file
    entries: { format: json, at: "$" }
    fields:
      val: entry.value
rules:
  - id: check-tokens
    kind: constraint
    input: tokens
    assert:
      set:
        field: tok
        subset_of:
          relation: allowed
          field: val
    level: error
  - id: trim-plain
    kind: no_trailing_whitespace
    paths: "plain.txt"
    level: warning
    fix:
      file_trim_trailing_whitespace: {}
"#,
    )
    .unwrap();

    let engine = build_engine(tmp.path());
    let index = walk(
        tmp.path(),
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();

    // `secret.json` IS read by the sensitive relation -> must be protected.
    assert!(
        engine.path_contains_sensitive_evidence(tmp.path(), &index, Path::new("secret.json")),
        "secret.json must be gated as sensitive"
    );
    // `plain.txt` is NOT matched by the constraint's trigger scope -> must NOT be over-suppressed!
    assert!(
        !engine.path_contains_sensitive_evidence(tmp.path(), &index, Path::new("plain.txt")),
        "unrelated plain.txt must not be over-suppressed by a sensitive constraint rule"
    );
    // Consequently, `fix --diff` staging still emits the diff for `plain.txt`.
    let (_, staged) = engine
        .stage_fixes(tmp.path(), &index, Applicability::Safe)
        .unwrap();
    assert_eq!(staged.len(), 1, "plain.txt diff should not be suppressed");
    assert_eq!(staged[0].path, Path::new("plain.txt"));

    // Also verify `assert.set` hides the sensitive value's end column (start-only point).
    let report = engine.run(tmp.path(), &index).unwrap();
    let v = &report.results[0].violations[0];
    assert!(
        v.range.is_none(),
        "sensitive set finding must not expose value length range: {v:?}"
    );
}

#[test]
fn nested_rule_with_env_manifest_scope_filter_redacts_path_and_marker() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("pkgs/alpha")).unwrap();
    std::fs::create_dir_all(tmp.path().join("pkgs/beta")).unwrap();
    std::fs::write(tmp.path().join(".env"), "ONLY=pkgs/alpha/bad.txt\n").unwrap();
    std::fs::write(tmp.path().join("pkgs/alpha/bad.txt"), "TODO secret\n").unwrap();
    std::fs::write(tmp.path().join("pkgs/beta/bad.txt"), "TODO secret\n").unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: per-pkg
    kind: for_each_dir
    select: "pkgs/*"
    require:
      - kind: file_content_forbidden
        paths: "{path}/*.txt"
        pattern: "TODO"
        scope_filter:
          include_manifest_paths:
            source: .env
            extract:
              regex: "ONLY=(\\S+)"
    level: error
"#,
    )
    .unwrap();

    let report = load_and_run(tmp.path());
    let result = &report.results[0];
    assert_eq!(result.violations.len(), 1, "{result:?}");
    let v = &result.violations[0];
    assert!(
        v.path_is_sensitive(),
        "nested manifest-scoped finding must hide its path: {v:?}"
    );
    assert_eq!(v.report_path(), Some(Path::new("<redacted>")));
    assert!(!v.message.contains("pkgs/alpha/bad.txt"), "{v:?}");
}

#[test]
fn sarif_repeated_fingerprints_with_baseline_do_not_swap_dot_vs_slash_paths() {
    let make_violation = |p: &str| {
        let mut v = alint_core::Violation::new("dup")
            .with_path(Path::new(p))
            .with_baseline_key("same-key");
        v.path_free_identity = true;
        v
    };
    let report = alint_core::Report {
        results: vec![alint_core::RuleResult::new(
            "r".into(),
            alint_core::Level::Error,
            None,
            vec![make_violation("a.json"), make_violation("a/b.json")],
            false,
        )],
    };
    let canonical_fp = baseline_fingerprint("r", &report.results[0].violations[0], None);
    assert_eq!(
        canonical_fp,
        baseline_fingerprint("r", &report.results[0].violations[1], None),
        "both findings must share the canonical path-free fingerprint"
    );

    // Without baseline
    let fps = vec![vec![canonical_fp.clone(), canonical_fp.clone()]];
    let mut raw_without = Vec::new();
    write_sarif_with_fingerprints(&report, Some(&fps), &mut raw_without).unwrap();
    let sarif_without: serde_json::Value = serde_json::from_slice(&raw_without).unwrap();
    let fp_of = |sarif: &serde_json::Value, uri: &str| -> String {
        sarif["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["locations"][0]["physicalLocation"]["artifactLocation"]["uri"] == uri)
            .unwrap()["partialFingerprints"]["alint/v1"]
            .as_str()
            .unwrap()
            .to_string()
    };

    // Baseline with count = 1 for the shared fingerprint
    let baseline = Baseline::from_fingerprints(
        Some("0.23.0".into()),
        [FingerprintedViolation {
            rule_id: "r".into(),
            path: Some("a.json".into()),
            fingerprint: canonical_fp.clone(),
            message: Some("dup".into()),
            path_free_identity: false,
        }],
    );
    let applied = apply_baseline(&report, &baseline, |rid, v| {
        baseline_fingerprint(rid, v, None)
    });
    assert_eq!(applied.suppressed_total, 1);
    assert_eq!(
        applied.suppressed[0].violation.path.as_deref(),
        Some(Path::new("a.json"))
    );
    let marks = BaselineMarks {
        suppressed_total: applied.suppressed_total,
        per_result: vec![ResultMarks {
            live_fingerprints: applied.live_fingerprints[0].clone(),
            suppressed: applied
                .suppressed
                .into_iter()
                .map(|s| SuppressedFinding {
                    violation: s.violation,
                    fingerprint: s.fingerprint,
                })
                .collect(),
        }],
    };
    let mut raw_with = Vec::new();
    write_sarif_with_baseline(&applied.live, Some(&marks), &mut raw_with).unwrap();
    let sarif_with: serde_json::Value = serde_json::from_slice(&raw_with).unwrap();

    assert_eq!(
        fp_of(&sarif_without, "a.json"),
        fp_of(&sarif_with, "a.json"),
        "a.json fingerprint must not swap under --baseline"
    );
    assert_eq!(
        fp_of(&sarif_without, "a/b.json"),
        fp_of(&sarif_with, "a/b.json"),
        "a/b.json fingerprint must not swap under --baseline"
    );
}

#[test]
fn yaml_entry_rows_preserve_block_scalar_offsets_on_non_ascii_files() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("notes.yml"),
        "title: \"café\"\nbody: |\n  hello world\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("allowed.yml"), "title: ok\n").unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        r#"version: 2
selectors:
  ymls:
    paths: "notes.yml"
  allowed_file:
    paths: "allowed.yml"
relations:
  entries:
    from: ymls
    entries: { format: yaml, at: "$" }
    fields:
      k: entry.key
      v: entry.value
  allowed:
    from: allowed_file
    entries: { format: yaml, at: "$" }
    fields:
      k: entry.key
rules:
  - id: check-body
    kind: constraint
    input: entries
    assert:
      set:
        field: k
        subset_of:
          relation: allowed
          field: k
    level: error
"#,
    )
    .unwrap();

    let report = load_and_run(tmp.path());
    let v = &report.results[0].violations[0];
    assert_eq!(
        v.line,
        Some(2),
        "block-scalar entry key line must survive non-ASCII UTF-8 offset mapping: {v:?}"
    );
}
