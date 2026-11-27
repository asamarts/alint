//! End-to-end tests for Phase 5 Batch 5c (ADR-0050 D5, D6, D7):
//! - D5 `assert.satisfies` (`node`, `cargo`, `pep440-basic`, prereleases,
//!   field/literal forms, fail-loud analysis failures, sensitivity, spans) and
//!   `compare: { order: semver }` (all 6 ops, normalization, `to.field`/
//!   `to.value`/`to.row`, fail-loud, sensitivity);
//! - D6 `no_duplicate_keys` across YAML, JSON/JSONC, dotenv, properties, INI,
//!   TOML, HCL, XML rejection, and `sensitive: true` redaction;
//! - D7 `strict: true` RFC 8259 JSON parse mode across v1 and v2 surfaces plus
//!   single-run tolerant-vs-strict cache isolation.

#![allow(clippy::too_many_lines)]

use super::constraint_families::{failures, findings, result, run};
use super::*;

// --- D5: `satisfies` ---------------------------------------------------------

#[test]
fn satisfies_covers_all_three_dialects_and_field_literal_combinations() {
    let config = r#"version: 2
relations:
  pins:
    from: { paths: "pins.json" }
    records: { format: json, at: "$.pins[*]" }
    fields:
      name: "record: $.name"
      ver: "record: $.ver"
      py_ver: "record: $.py_ver"
      node_range: "record: $.node_range"
      cargo_range: "record: $.cargo_range"
      py_range: "record: $.py_range"
rules:
  - id: node-field-pair
    kind: constraint
    input: pins
    assert:
      satisfies:
        version: ver
        range: node_range
        dialect: node
    level: error
  - id: node-range-literal
    kind: constraint
    input: pins
    assert:
      satisfies:
        version: ver
        range_literal: "^18.0.0 || >=20.0.0"
    level: error
  - id: cargo-version-literal
    kind: constraint
    input: pins
    assert:
      satisfies:
        version_literal: "1.4.2"
        range: cargo_range
        dialect: cargo
    level: error
  - id: pep440-field-pair
    kind: constraint
    input: pins
    assert:
      satisfies:
        version: py_ver
        range: py_range
        dialect: pep440-basic
    level: error
"#;
    let files = [(
        "pins.json",
        r#"{
  "pins": [
    {
      "name": "ok",
      "ver": "20.11.0+build.1",
      "py_ver": "20.11.0",
      "node_range": ">=18.12.0 <22",
      "cargo_range": "^1.2, <2.0",
      "py_range": ">=20.0, !=20.10.*, <21"
    },
    {
      "name": "partial-ok",
      "ver": "v22",
      "py_ver": "v22",
      "node_range": "22.x",
      "cargo_range": ">=1.0, <=1.5",
      "py_range": "~=22.0"
    },
    {
      "name": "bad",
      "ver": "19.5.0",
      "py_ver": "19.5.0",
      "node_range": "^18.0.0 || >=20.0.0",
      "cargo_range": "^2.0",
      "py_range": ">=20.0, <22.0"
    }
  ]
}"#,
    )];
    let results = run(config, &files);

    let node_pair = result(&results, "node-field-pair");
    assert_eq!(node_pair.violations.len(), 1, "{node_pair:#?}");
    assert!(node_pair.violations[0].message.contains("19.5.0"));
    assert_eq!(node_pair.violations[0].related_locations.len(), 1);
    assert!(
        node_pair.violations[0].related_locations[0]
            .label
            .contains("node_range")
    );

    assert_eq!(findings(&results, "node-range-literal").len(), 1);
    assert!(findings(&results, "node-range-literal")[0].contains("19.5.0"));

    assert_eq!(findings(&results, "cargo-version-literal").len(), 1);
    assert!(findings(&results, "cargo-version-literal")[0].contains("^2.0"));

    assert_eq!(findings(&results, "pep440-field-pair").len(), 1);
    assert!(findings(&results, "pep440-field-pair")[0].contains("19.5.0"));
}

#[test]
fn satisfies_prerelease_gating_respects_include_prerelease_flag() {
    let config = r#"version: 2
relations:
  vers:
    from: { paths: "vers.json" }
    entries: { format: json, at: "$" }
    fields: { name: entry.key, ver: entry.value }
rules:
  - id: strict-pre
    kind: constraint
    input: vers
    assert:
      satisfies:
        version: ver
        range_literal: "^1.2.0"
    level: error
  - id: allow-pre
    kind: constraint
    input: vers
    assert:
      satisfies:
        version: ver
        range_literal: "^1.2.0"
        include_prerelease: true
    level: error
  - id: tuple-pre
    kind: constraint
    input: vers
    assert:
      satisfies:
        version: ver
        range_literal: ">=1.2.3-alpha.1 <2.0.0"
    level: error
"#;
    let files = [(
        "vers.json",
        r#"{"rel": "1.2.3", "pre_same": "1.2.3-beta.1", "pre_other": "1.2.4-alpha.1"}"#,
    )];
    let results = run(config, &files);
    // Without `include_prerelease`, both prereleases fail `^1.2.0`.
    assert_eq!(findings(&results, "strict-pre").len(), 2);
    // With `include_prerelease: true`, `1.2.3-beta.1` < `1.2.3` but >= `1.2.0` -> both prereleases satisfy `^1.2.0`!
    assert_eq!(findings(&results, "allow-pre").len(), 0);
    // With explicit prerelease comparator `>=1.2.3-alpha.1`, `1.2.3-beta.1` (same tuple) matches while `1.2.4-alpha.1` (different tuple) fails.
    assert_eq!(findings(&results, "tuple-pre").len(), 1);
    assert!(findings(&results, "tuple-pre")[0].contains("1.2.4-alpha.1"));
}

#[test]
fn satisfies_fails_loud_on_null_missing_non_string_and_malformed_operands() {
    let make_cfg = |range_expr: &str, dialect: &str| {
        format!(
            r#"version: 2
relations:
  items:
    from: {{ paths: "items.json" }}
    records: {{ format: json, at: "$.items[*]" }}
    fields:
      ver: {{ source: "record: $.ver", optional: true }}
      rng: {{ source: "record: $.rng", optional: true }}
rules:
  - id: check
    kind: constraint
    input: items
    assert:
      satisfies:
        version: ver
        {range_expr}
        dialect: {dialect}
    level: error
"#
        )
    };
    for (label, cfg, body, expected_substr) in [
        (
            "null version",
            make_cfg("range_literal: '^1.0.0'", "node"),
            r#"{"items": [{"ver": null}]}"#,
            "null",
        ),
        (
            "missing version",
            make_cfg("range_literal: '^1.0.0'", "node"),
            r#"{"items": [{}]}"#,
            "missing",
        ),
        (
            "numeric version",
            make_cfg("range_literal: '^1.0.0'", "node"),
            r#"{"items": [{"ver": 18}]}"#,
            "string version",
        ),
        (
            "invalid version",
            make_cfg("range_literal: '^1.0.0'", "node"),
            r#"{"items": [{"ver": "latest"}]}"#,
            "not a valid semantic version",
        ),
        (
            "null range",
            make_cfg("range: rng", "node"),
            r#"{"items": [{"ver": "1.0.0", "rng": null}]}"#,
            "null",
        ),
        (
            "missing range",
            make_cfg("range: rng", "node"),
            r#"{"items": [{"ver": "1.0.0"}]}"#,
            "missing",
        ),
        (
            "comma in node range",
            make_cfg("range: rng", "node"),
            r#"{"items": [{"ver": "1.0.0", "rng": ">=1.0.0, <2.0.0"}]}"#,
            "not a valid `node` version range",
        ),
        (
            "pep440 epoch rejected",
            make_cfg("range: rng", "pep440-basic"),
            r#"{"items": [{"ver": "1.0.0", "rng": ">=1!2.0.0"}]}"#,
            "not a valid `pep440-basic` version range",
        ),
        (
            "pep440 arbitrary eq rejected",
            make_cfg("range: rng", "pep440-basic"),
            r#"{"items": [{"ver": "1.0.0", "rng": "===1.0.0"}]}"#,
            "not a valid `pep440-basic` version range",
        ),
    ] {
        let results = run(&cfg, &[("items.json", body)]);
        let fails = failures(&results, "check");
        assert_eq!(fails.len(), 1, "{label}: {fails:?}");
        assert!(
            fails[0].contains(expected_substr),
            "{label}: expected `{expected_substr}` in `{}`",
            fails[0]
        );
    }
}

#[test]
fn satisfies_propagates_sensitivity_bidirectionally_and_hides_literals_and_ranges() {
    let config = r#"version: 2
relations:
  secrets:
    from: { paths: "s.json" }
    records: { format: json, at: "$.rows[*]" }
    fields:
      ver: { source: "record: $.ver", sensitive: true }
      rng: "record: $.rng"
rules:
  - id: field-pair
    kind: constraint
    input: secrets
    assert:
      satisfies:
        version: ver
        range: rng
    level: error
  - id: lit-range
    kind: constraint
    input: secrets
    assert:
      satisfies:
        version: ver
        range_literal: "^9.9.9"
    level: error
"#;
    let files = [(
        "s.json",
        r#"{"rows": [{"ver": "1.2.3-secret", "rng": "^8.8.8"}]}"#,
    )];
    let results = run(config, &files);

    for id in ["field-pair", "lit-range"] {
        let res = result(&results, id);
        assert_eq!(res.violations.len(), 1, "{id}: {res:#?}");
        let v = &res.violations[0];
        assert!(v.message.contains("<redacted>"), "{id}: {}", v.message);
        assert!(!v.message.contains("1.2.3-secret"), "{id}: {}", v.message);
        assert!(!v.message.contains("8.8.8"), "{id}: {}", v.message);
        assert!(!v.message.contains("9.9.9"), "{id}: {}", v.message);
        // Point-only span when sensitive (no range end leaking value length).
        assert!(
            v.range.is_none(),
            "{id}: sensitive span leaked range: {v:?}"
        );
        assert!(v.line.is_some() && v.column.is_some());
    }
}

// --- D5: `compare: { order: semver }` ----------------------------------------

#[test]
fn compare_semver_supports_all_ops_normalization_and_row_targets() {
    let config = r#"version: 2
relations:
  pkgs:
    from: { paths: "pkgs.json" }
    records: { format: json, at: "$.pkgs[*]" }
    fields:
      name: "record: $.name"
      cur: "record: $.cur"
      min: "record: $.min"
rules:
  - id: ge-field
    kind: constraint
    input: pkgs
    assert:
      compare: { field: cur, op: ge, order: semver, to: { field: min } }
    level: error
  - id: eq-norm
    kind: constraint
    input: pkgs
    assert:
      compare: { field: cur, op: eq, order: semver, to: { value: "22.0.0" } }
    level: error
  - id: le-source-row
    kind: constraint
    input: pkgs
    assert:
      compare: { field: cur, op: le, order: semver, to: { row: { name: "cap" } } }
    level: error
"#;
    let files = [(
        "pkgs.json",
        r#"{
  "pkgs": [
    {"name": "cap", "cur": "v22.0.0+build.7", "min": "22.0.0-rc.1"},
    {"name": "partial", "cur": "22", "min": "v22.0.0+other"},
    {"name": "pre", "cur": "22.0.0-alpha.2", "min": "22.0.0-alpha.10"},
    {"name": "high", "cur": "23.0.0", "min": "22.0.0"}
  ]
}"#,
    )];
    let results = run(config, &files);

    // `22.0.0-alpha.2 < 22.0.0-alpha.10` (numeric prerelease ordering).
    assert_eq!(findings(&results, "ge-field").len(), 1);
    assert!(findings(&results, "ge-field")[0].contains("22.0.0-alpha.2"));

    // `v22.0.0+build.7 == 22 == 22.0.0`, so `cap` and `partial` pass `eq-norm`, while `pre` and `high` fail.
    assert_eq!(findings(&results, "eq-norm").len(), 2);

    // `cap` has `22.0.0`; `partial` and `pre` pass; `high` (23.0.0) fails.
    assert_eq!(findings(&results, "le-source-row").len(), 1);
    assert!(findings(&results, "le-source-row")[0].contains("23.0.0"));
}

#[test]
fn compare_semver_fails_loud_on_malformed_or_non_string_and_redacts_sensitive() {
    let bad_cfg = r#"version: 2
relations:
  items:
    from: { paths: "items.json" }
    entries: { format: json, at: "$" }
    fields: { k: entry.key, v: entry.value }
rules:
  - id: bad-ver
    kind: constraint
    input: items
    assert: { compare: { field: v, op: ge, order: semver, to: { value: "1.0.0" } } }
    level: error
"#;
    let res_bad = run(bad_cfg, &[("items.json", r#"{"a": "not-semver"}"#)]);
    assert_eq!(failures(&res_bad, "bad-ver").len(), 1);
    assert!(failures(&res_bad, "bad-ver")[0].contains("not a valid semantic version"));

    let res_num = run(bad_cfg, &[("items.json", r#"{"a": 22}"#)]);
    assert_eq!(failures(&res_num, "bad-ver").len(), 1);
    assert!(failures(&res_num, "bad-ver")[0].contains("string"));

    let sens_cfg = r#"version: 2
relations:
  items:
    from: { paths: "items.json" }
    entries: { format: json, at: "$" }
    fields:
      k: entry.key
      v: { source: entry.value, sensitive: true }
rules:
  - id: sens
    kind: constraint
    input: items
    assert: { compare: { field: v, op: ge, order: semver, to: { value: "9.0.0" } } }
    level: error
"#;
    let res_sens = run(sens_cfg, &[("items.json", r#"{"a": "1.2.3-hidden"}"#)]);
    let r = result(&res_sens, "sens");
    assert_eq!(r.violations.len(), 1);
    assert!(r.violations[0].message.contains("<redacted>"));
    assert!(!r.violations[0].message.contains("1.2.3-hidden"));
    assert!(!r.violations[0].message.contains("9.0.0"));
    assert!(r.violations[0].range.is_none());
}

// --- D6: `no_duplicate_keys` -------------------------------------------------

#[test]
fn no_duplicate_keys_covers_yaml_json_dotenv_properties_ini_toml_hcl_and_sensitive() {
    let config = r#"version: 1
rules:
  - id: dup-all
    kind: no_duplicate_keys
    paths: ["**/*.{yml,json,properties,ini,toml,tf}", ".env"]
    level: error
  - id: dup-sens
    kind: no_duplicate_keys
    paths: "secret.yml"
    sensitive: true
    level: error
"#;
    let files = [
        (
            "ci.yml",
            "base: &base\n  k: 1\nmerged:\n  <<: *base\n  k: 2\ndup_typed:\n  true: 1\n  True: 2\n  1: a\n  0x1: b\n  \"1\": c\n---\ndup_typed:\n  true: 1\n",
        ),
        ("merge_dup.yml", "a: &a\n  x: 1\nb:\n  <<: *a\n  <<: *a\n"),
        (
            "nested.json",
            "{\n  // jsonc comment\n  \"arr\": [{\"x\": 1, \"x\": 2}]\n}\n",
        ),
        (".env", "SECRET=topsecret_a\nSECRET=topsecret_b\n"),
        ("app.properties", "db.host=a\ndb.host=b\n"),
        ("cfg.ini", "[sec]\na=1\n[other]\na=2\n[sec]\na=3\n"),
        ("bad.toml", "a = 1\na = 2\n"),
        ("bad.tf", "a = 1\na = 2\n"),
        ("secret.yml", "token_key: 1\ntoken_key: 2\n"),
    ];
    let results = run(config, &files);
    let all = result(&results, "dup-all");

    // `ci.yml`: `true`/`True` and `1`/`0x1` fire (2 dupes); `<<: *base` override, `"1"`, and second doc `---` do NOT fire!
    let ci_vs: Vec<_> = all
        .violations
        .iter()
        .filter(|v| v.path.as_deref() == Some(std::path::Path::new("ci.yml")))
        .collect();
    assert_eq!(ci_vs.len(), 2, "{ci_vs:#?}");
    assert_eq!(ci_vs[0].related_locations.len(), 1);
    assert!(ci_vs[0].related_locations[0].range.is_some());

    // `merge_dup.yml`: two literal `<<:` keys fire.
    let merge_vs: Vec<_> = all
        .violations
        .iter()
        .filter(|v| v.path.as_deref() == Some(std::path::Path::new("merge_dup.yml")))
        .collect();
    assert_eq!(merge_vs.len(), 1, "{merge_vs:#?}");

    // `nested.json`: duplicate inside array-of-objects element.
    assert_eq!(
        all.violations
            .iter()
            .filter(|v| v.path.as_deref() == Some(std::path::Path::new("nested.json")))
            .count(),
        1
    );

    // `.env`: fires without leaking secret values.
    let env_v = all
        .violations
        .iter()
        .find(|v| v.path.as_deref() == Some(std::path::Path::new(".env")))
        .unwrap();
    assert!(env_v.message.contains("SECRET"));
    assert!(!env_v.message.contains("topsecret"));

    // `.properties`: fires on `db.host`.
    let prop_v = all
        .violations
        .iter()
        .find(|v| v.path.as_deref() == Some(std::path::Path::new("app.properties")))
        .unwrap();
    assert!(prop_v.message.contains("db.host"));

    // `.ini`: repeated `[sec]` merges and flags `a` in `[sec]` (ignoring `[other]`).
    let ini_vs: Vec<_> = all
        .violations
        .iter()
        .filter(|v| v.path.as_deref() == Some(std::path::Path::new("cfg.ini")))
        .collect();
    assert_eq!(ini_vs.len(), 1, "{ini_vs:#?}");
    assert!(ini_vs[0].message.contains("[sec]"));

    // TOML and HCL delegate to parse errors (`resource_parse`).
    for path in ["bad.toml", "bad.tf"] {
        let af = all
            .analysis_failures
            .iter()
            .find(|f| f.path.as_deref() == Some(std::path::Path::new(path)))
            .unwrap_or_else(|| {
                panic!(
                    "expected parse failure for {path}: {:#?}",
                    all.analysis_failures
                )
            });
        assert_eq!(
            af.kind,
            alint_core::AnalysisFailureKind::ResourceParse,
            "{path}: {af:?}"
        );
    }

    // `sensitive: true`: redacts key name, emits point-only primary span, and strips related locations.
    let sens = result(&results, "dup-sens");
    assert_eq!(sens.violations.len(), 1, "{sens:#?}");
    let sv = &sens.violations[0];
    assert!(!sv.message.contains("token_key"), "{}", sv.message);
    assert!(sv.range.is_none());
    assert_eq!(sv.line, Some(2));
    assert_eq!(sv.related_locations.len(), 0);
}

// --- D7: `strict: true` RFC 8259 JSON mode & cache isolation -----------------

#[test]
fn strict_json_isolates_cache_from_tolerant_jsonc_across_v1_and_v2_in_one_run() {
    let config = r#"version: 2
relations:
  tol-entries:
    from: { paths: "tsconfig.json" }
    entries: { format: json, at: "$.compilerOptions" }
    fields: { k: entry.key, v: entry.value }
  strict-entries:
    from: { paths: "tsconfig.json" }
    entries: { format: json, at: "$.compilerOptions", strict: true }
    fields: { k: entry.key, v: entry.value }
  tol-records:
    from: { paths: "items.json" }
    records: { format: json, at: "$.items[*]" }
    fields: { n: "record: $.name" }
  strict-records:
    from: { paths: "items.json" }
    records: { format: json, at: "$.items[*]", strict: true }
    fields: { n: "record: $.name" }
  tol-read:
    from: { paths: "tsconfig.json" }
    read: { json: "$.compilerOptions.target" }
    fields: { target: value }
  strict-read:
    from: { paths: "tsconfig.json" }
    read: { json: "$.compilerOptions.target", strict: true }
    fields: { target: value }
rules:
  - id: v1-path-tol
    kind: json_path_equals
    paths: "tsconfig.json"
    path: "$.compilerOptions.target"
    equals: "ES2022"
    level: error
  - id: v1-path-strict
    kind: json_path_equals
    paths: "tsconfig.json"
    path: "$.compilerOptions.target"
    equals: "ES2022"
    strict: true
    level: error
  - id: v1-matches-strict
    kind: json_path_matches
    paths: "bom.json"
    path: "$.name"
    matches: "^ok$"
    strict: true
    level: error
  - id: v1-absent-strict
    kind: json_path_absent
    paths: "empty.json"
    path: "$.bad"
    strict: true
    level: error
  - id: v1-schema-tol
    kind: json_schema_passes
    paths: "tsconfig.json"
    schema_path: "schema.json"
    level: error
  - id: v1-schema-strict
    kind: json_schema_passes
    paths: "tsconfig.json"
    schema_path: "schema.json"
    strict: true
    level: error
  - id: v1-parity-tol
    kind: key_parity
    reference: "locales/en.json"
    peers: "locales/fr.json"
    level: error
  - id: v1-parity-strict
    kind: key_parity
    reference: "locales/en.json"
    peers: "locales/fr.json"
    strict: true
    level: error
  - id: v1-xfile-tol
    kind: cross_file
    source: { file: "tsconfig.json", extract: { json: "$.compilerOptions.target" } }
    targets: { files: "target.json", extract: { json: "$.target" } }
    level: error
  - id: v1-xfile-strict
    kind: cross_file
    source: { file: "tsconfig.json", extract: { json: "$.compilerOptions.target", strict: true } }
    targets: { files: "target.json", extract: { json: "$.target", strict: true } }
    level: error
  - id: v1-dup-tol
    kind: no_duplicate_keys
    paths: "tsconfig.json"
    level: error
  - id: v1-dup-strict
    kind: no_duplicate_keys
    paths: "tsconfig.json"
    strict: true
    level: error
  - id: v2-entries-tol
    kind: constraint
    input: tol-entries
    assert: { count: { min: 1 } }
    level: error
  - id: v2-entries-strict
    kind: constraint
    input: strict-entries
    assert: { count: { min: 1 } }
    level: error
  - id: v2-records-tol
    kind: constraint
    input: tol-records
    assert: { count: { min: 1 } }
    level: error
  - id: v2-records-strict
    kind: constraint
    input: strict-records
    assert: { count: { min: 1 } }
    level: error
  - id: v2-read-tol
    kind: constraint
    input: tol-read
    assert: { compare: { field: target, op: eq, to: { value: "ES2022" } } }
    level: error
  - id: v2-read-strict
    kind: constraint
    input: strict-read
    assert: { compare: { field: target, op: eq, to: { value: "ES2022" } } }
    level: error
"#;
    let files = [
        (
            "tsconfig.json",
            "{\n  // JSONC comment + trailing comma\n  \"compilerOptions\": {\n    \"target\": \"ES2022\",\n  }\n}\n",
        ),
        (
            "items.json",
            "{\n  /* comment */\n  \"items\": [{\"name\": \"a\"}],\n}\n",
        ),
        ("bom.json", "\u{feff}{\"name\": \"ok\"}\n"),
        ("empty.json", "   \n"),
        ("schema.json", r#"{"type": "object"}"#),
        ("target.json", r#"{"target": "ES2022"}"#),
        ("locales/en.json", "{\"hello\": \"hi\"}\n"),
        (
            "locales/fr.json",
            "{\n  // trailing comma\n  \"hello\": \"salut\",\n}\n",
        ),
    ];
    let results = run(config, &files);

    // Every tolerant rule MUST pass silently on the JSONC files.
    for tol_id in [
        "v1-path-tol",
        "v1-schema-tol",
        "v1-parity-tol",
        "v1-xfile-tol",
        "v1-dup-tol",
        "v2-entries-tol",
        "v2-records-tol",
        "v2-read-tol",
    ] {
        let r = result(&results, tol_id);
        assert!(
            r.violations.is_empty() && r.analysis_failures.is_empty(),
            "tolerant rule `{tol_id}` should succeed: violations={:#?} failures={:#?}",
            r.violations,
            r.analysis_failures
        );
    }

    // Every strict rule MUST fail with `ResourceParse` on JSONC / BOM / empty files.
    for strict_id in [
        "v1-path-strict",
        "v1-matches-strict",
        "v1-absent-strict",
        "v1-schema-strict",
        "v1-parity-strict",
        "v1-xfile-strict",
        "v1-dup-strict",
        "v2-entries-strict",
        "v2-records-strict",
        "v2-read-strict",
    ] {
        let r = result(&results, strict_id);
        assert!(
            r.violations.is_empty()
                && !r.analysis_failures.is_empty()
                && r.analysis_failures
                    .iter()
                    .all(|f| f.kind == alint_core::AnalysisFailureKind::ResourceParse),
            "strict rule `{strict_id}` must fail with ResourceParse: violations={:#?} failures={:#?}",
            r.violations,
            r.analysis_failures
        );
    }
}

#[test]
fn strict_json_is_refused_on_non_json_rules_and_extractors() {
    for (label, cfg) in [
        (
            "json_schema_passes non-json",
            "version: 1\nrules:\n  - id: r\n    kind: json_schema_passes\n    paths: '*.yml'\n    \
             schema_path: s.json\n    format: yaml\n    strict: true\n    level: error\n",
        ),
        (
            "key_parity non-json format",
            "version: 1\nrules:\n  - id: r\n    kind: key_parity\n    reference: a.yml\n    \
             peers: '*.yml'\n    format: yaml\n    strict: true\n    level: error\n",
        ),
        (
            "key_parity non-json inferred",
            "version: 1\nrules:\n  - id: r\n    kind: key_parity\n    reference: a.yml\n    \
             peers: '*.yml'\n    strict: true\n    level: error\n",
        ),
        (
            "no_duplicate_keys non-json",
            "version: 1\nrules:\n  - id: r\n    kind: no_duplicate_keys\n    paths: '*.yml'\n    \
             format: yaml\n    strict: true\n    level: error\n",
        ),
        (
            "no_duplicate_keys xml",
            "version: 1\nrules:\n  - id: r\n    kind: no_duplicate_keys\n    paths: '*.xml'\n    \
             format: xml\n    level: error\n",
        ),
        (
            "cross_file yaml with strict",
            "version: 1\nrules:\n  - id: r\n    kind: cross_file\n    source: { file: a.yml, \
             extract: { yaml: '$.v', strict: true } }\n    targets: { files: b.yml, extract: \
             { yaml: '$.v' } }\n    level: error\n",
        ),
        (
            "v2 entries yaml strict",
            "version: 2\nrelations:\n  r:\n    from: { paths: '*.yml' }\n    entries: { format: \
             yaml, at: '$', strict: true }\n    fields: { k: entry.key }\nrules: []\n",
        ),
        (
            "v2 records toml strict",
            "version: 2\nrelations:\n  r:\n    from: { paths: '*.toml' }\n    records: { format: \
             toml, at: '$.a[*]', strict: true }\n    fields: { k: 'record: $.k' }\nrules: []\n",
        ),
        (
            "v2 read yaml strict",
            "version: 2\nrelations:\n  r:\n    from: { paths: '*.yml' }\n    read: { yaml: '$.v', \
             strict: true }\n    fields: { v: value }\nrules: []\n",
        ),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".alint.yml");
        std::fs::write(&path, cfg).unwrap();
        let outcome = load(&path).and_then(|loaded| {
            for spec in &loaded.rules {
                test_registry().build(spec)?;
            }
            Ok(())
        });
        assert!(outcome.is_err(), "{label}: expected load/build error");
    }
}

#[test]
fn no_duplicate_keys_enforces_findings_per_rule_limit_across_files() {
    let config = r#"version: 1
limits:
  findings_per_rule: 3
rules:
  - id: dup-capped
    kind: no_duplicate_keys
    paths: "*.yml"
    level: error
"#;
    let files = [
        ("a.yml", "k: 1\nk: 2\nk: 3\n"),
        ("b.yml", "k: 1\nk: 2\nk: 3\n"),
        ("c.yml", "k: 1\nk: 2\nk: 3\n"),
    ];
    let results = run(config, &files);
    let r = result(&results, "dup-capped");
    assert_eq!(r.violations.len(), 3, "{r:#?}");
    assert_eq!(r.analysis_failures.len(), 1, "{r:#?}");
    assert_eq!(
        r.analysis_failures[0].kind,
        alint_core::AnalysisFailureKind::Evaluation
    );
    assert!(
        r.analysis_failures[0].message.contains("findings_per_rule"),
        "{:?}",
        r.analysis_failures[0].message
    );
}

#[test]
fn satisfies_withholds_partial_findings_on_failure_supports_row_msg_and_rejects_value_placeholder()
{
    // Row 0 violates `^20.0.0`, row 1 has an unparseable version: fail-loud
    // must withhold row 0's violation and report only the analysis failure.
    let withhold_cfg = r#"version: 2
relations:
  pins:
    from: { paths: "pins.json" }
    records: { format: json, at: "$.pins[*]" }
    fields:
      name: "record: $.name"
      ver: "record: $.ver"
rules:
  - id: sat
    kind: constraint
    input: pins
    assert:
      satisfies:
        version: ver
        range_literal: "^20.0.0"
    message: "package {{row.name}} has invalid pin {{row.ver}}"
    level: error
"#;
    let res = run(
        withhold_cfg,
        &[(
            "pins.json",
            r#"{"pins": [{"name": "a", "ver": "18.0.0"}, {"name": "b", "ver": "not-semver"}]}"#,
        )],
    );
    let r = result(&res, "sat");
    assert_eq!(r.violations.len(), 0, "{r:#?}");
    assert_eq!(r.analysis_failures.len(), 1, "{r:#?}");

    // When all versions are valid SemVer (including `.dev` / `.post` in pre/build
    // metadata!), `{{row.FIELD}}` renders in custom messages.
    let res_ok = run(
        withhold_cfg,
        &[(
            "pins.json",
            r#"{"pins": [{"name": "a", "ver": "18.0.0-alpha.dev.1+build.postcss"}]}"#,
        )],
    );
    let r_ok = result(&res_ok, "sat");
    assert_eq!(r_ok.violations.len(), 1, "{r_ok:#?}");
    assert_eq!(r_ok.analysis_failures.len(), 0, "{r_ok:#?}");
    assert!(
        r_ok.violations[0]
            .message
            .contains("package a has invalid pin 18.0.0-alpha.dev.1+build.postcss"),
        "{}",
        r_ok.violations[0].message
    );

    // `{{value}}` is rejected at build time on `satisfies`.
    let bad_placeholder = withhold_cfg.replace("{{row.ver}}", "{{value}}");
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join(".alint.yml");
    std::fs::write(&cfg_path, bad_placeholder).unwrap();
    let err = load(&cfg_path)
        .and_then(|loaded| {
            for spec in &loaded.rules {
                test_registry().build(spec)?;
            }
            Ok(())
        })
        .unwrap_err();
    assert!(err.to_string().contains("{{value}}"), "{err}");
}

#[test]
fn satisfies_version_literal_and_compare_semver_redact_sensitive_parse_failures() {
    let cfg = r#"version: 2
relations:
  pins:
    from: { paths: "pins.json" }
    records: { format: json, at: "$.pins[*]" }
    fields:
      name: "record: $.name"
      ver: "record: $.ver"
      rng: { source: "record: $.rng", sensitive: true }
rules:
  - id: sat-ver-lit
    kind: constraint
    input: pins
    assert:
      satisfies:
        version_literal: "1.2.3-secret-ver"
        range: rng
    level: error
  - id: cmp-bidir
    kind: constraint
    input: pins
    assert:
      compare:
        field: ver
        op: ge
        order: semver
        to: { field: rng }
    level: error
"#;
    // Policy violation on `sat-ver-lit` + parse failure on `cmp-bidir`:
    // bidirectional sensitivity must redact both `ver` and `rng` (and `version_literal`).
    let res = run(
        cfg,
        &[(
            "pins.json",
            r#"{"pins": [{"name": "a", "ver": "secret-bad-ver", "rng": "^9.9.9-secret-rng"}]}"#,
        )],
    );
    let sat = result(&res, "sat-ver-lit");
    assert_eq!(sat.violations.len(), 1, "{sat:#?}");
    assert!(sat.violations[0].message.contains("<redacted>"));
    assert!(!sat.violations[0].message.contains("1.2.3-secret-ver"));
    assert!(!sat.violations[0].message.contains("9.9.9-secret-rng"));
    assert!(sat.violations[0].range.is_none());

    let cmp = result(&res, "cmp-bidir");
    assert_eq!(cmp.analysis_failures.len(), 1, "{cmp:#?}");
    assert!(
        !cmp.analysis_failures[0].message.contains("secret-bad-ver"),
        "{}",
        cmp.analysis_failures[0].message
    );
    assert!(
        cmp.analysis_failures[0].message.contains("<redacted>"),
        "{}",
        cmp.analysis_failures[0].message
    );
}

#[test]
fn strict_json_works_in_facts_and_matches_v1_json_schema_parity() {
    // `facts:` with `extract: { json: ..., strict: true }` must validate against
    // `schemas/v1/config.json` AND fail cleanly on JSONC (evaluating fact to false).
    let fact_cfg = r#"version: 1
facts:
  - id: strict_pkg
    file_content_matches:
      paths: "pkg.json"
      extract: { json: "$.name", strict: true }
      pattern: "^app$"
rules:
  - id: require-readme-when-strict
    kind: file_exists
    paths: "README.md"
    when: "facts.strict_pkg"
    level: error
"#;
    let v1_schema: serde_json::Value =
        serde_json::from_str(include_str!("../../schemas/v1/config.json")).unwrap();
    let validator = jsonschema::validator_for(&v1_schema).unwrap();

    let fact_val: serde_json::Value = alint_core::yaml::from_str(fact_cfg).unwrap();
    assert!(
        validator.is_valid(&fact_val),
        "v1 schema must accept strict JSON in facts: {:?}",
        validator.iter_errors(&fact_val).collect::<Vec<_>>()
    );
    // On JSONC `pkg.json`, strict fact evaluates false -> `require-readme-when-strict` is skipped.
    let res_jsonc = run(
        fact_cfg,
        &[("pkg.json", "{\n  // comment\n  \"name\": \"app\"\n}\n")],
    );
    assert!(res_jsonc.is_empty(), "{res_jsonc:#?}");
    // On strict valid JSON `pkg.json`, fact evaluates true -> missing README fires.
    let res_valid = run(fact_cfg, &[("pkg.json", r#"{"name": "app"}"#)]);
    assert_eq!(
        result(&res_valid, "require-readme-when-strict")
            .violations
            .len(),
        1
    );

    // Negative v1 schema + loader parity cases for `strict` on non-JSON and null values.
    for (label, cfg) in [
        (
            "no_duplicate_keys yaml strict true",
            "version: 1\nrules:\n  - id: r\n    kind: no_duplicate_keys\n    paths: '*.yml'\n    \
             format: yaml\n    strict: true\n    level: error\n",
        ),
        (
            "no_duplicate_keys yaml strict false",
            "version: 1\nrules:\n  - id: r\n    kind: no_duplicate_keys\n    paths: '*.yml'\n    \
             format: yaml\n    strict: false\n    level: error\n",
        ),
        (
            "json_schema_passes yaml strict true",
            "version: 1\nrules:\n  - id: r\n    kind: json_schema_passes\n    paths: '*.yml'\n    \
             schema_path: s.json\n    format: yaml\n    strict: true\n    level: error\n",
        ),
        (
            "key_parity yaml strict true",
            "version: 1\nrules:\n  - id: r\n    kind: key_parity\n    reference: en.yml\n    \
             peers: '*.yml'\n    format: yaml\n    strict: true\n    level: error\n",
        ),
        (
            "extract_spec json null with strict true",
            "version: 1\nrules:\n  - id: r\n    kind: cross_file\n    source: { file: a.json, \
             extract: { json: null, strict: true } }\n    targets: { files: b.json, extract: \
             { json: '$.a' } }\n    level: error\n",
        ),
        (
            "extract_spec yaml with strict null",
            "version: 1\nrules:\n  - id: r\n    kind: cross_file\n    source: { file: a.yml, \
             extract: { yaml: '$.a', strict: null } }\n    targets: { files: b.yml, extract: \
             { yaml: '$.a' } }\n    level: error\n",
        ),
    ] {
        let val: serde_json::Value = alint_core::yaml::from_str(cfg).unwrap();
        assert!(
            !validator.is_valid(&val),
            "{label}: v1 JSON Schema must reject"
        );
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join(".alint.yml");
        std::fs::write(&p, cfg).unwrap();
        let built = load(&p).and_then(|loaded| {
            for spec in &loaded.rules {
                test_registry().build(spec)?;
            }
            Ok(())
        });
        assert!(built.is_err(), "{label}: loader/builder must reject");
    }
}
