//! End-to-end integration tests for top-level `limits:` configuration,
//! per-rule finding caps across `key_parity` and `constraint` families,
//! per-decode key-path byte caps across INI / Android XML / `.resx` / v2
//! `entries: { leaves: true }`, and CLI / baseline failure semantics.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Output;

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
        .output()
        .expect("run alint")
}

#[test]
fn top_level_limits_validation_and_trust_boundaries() {
    let root = tempfile::tempdir().unwrap();
    write(root.path(), "en.json", r#"{"a":"1","b":"2","c":"3"}"#);
    write(root.path(), "fr.json", "{}");

    // 1. `findings_per_rule: 0` is rejected at load.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nlimits:\n  findings_per_rule: 0\nrules: []\n",
    );
    let out = run(root.path(), &["validate-config"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("findings_per_rule"), "{stderr}");

    // 2. Exceeding hard cap (> 1,000,000) is rejected at load.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nlimits:\n  findings_per_rule: 1000001\nrules: []\n",
    );
    let out = run(root.path(), &["validate-config"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("findings_per_rule"), "{stderr}");

    // 3. Refused in `extends:`.
    write(
        root.path(),
        "base.yml",
        "version: 1\nlimits:\n  findings_per_rule: 5\nrules: []\n",
    );
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nextends: ['./base.yml']\nrules: []\n",
    );
    let out = run(root.path(), &["validate-config"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("limits"), "{stderr}");

    // 4. Refused in nested configs.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nnested_configs: true\nrules: []\n",
    );
    write(
        root.path(),
        "sub/.alint.yml",
        "version: 1\nlimits:\n  findings_per_rule: 5\nrules: []\n",
    );
    let out = run(root.path(), &["validate-config"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("limits"), "{stderr}");

    // 5. Merged field-by-field across `.alint.d/` drop-ins.
    std::fs::remove_dir_all(root.path().join("sub")).unwrap();
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nlimits:\n  findings_per_rule: 1\nrules:\n  - id: kp\n    kind: key_parity\n    \
         reference: en.json\n    peers: fr.json\n    level: warning\n",
    );
    write(
        root.path(),
        ".alint.d/01-limits.yml",
        "version: 1\nlimits:\n  key_path_bytes_per_decode: 65536\nrules: []\n",
    );
    let out = run(root.path(), &["check", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "truncation analysis failure must exit 1"
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rule = &parsed["results"][0];
    assert_eq!(rule["violations"].as_array().unwrap().len(), 1);
    let failures = rule["analysis_failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1);
    assert!(
        failures[0]["message"]
            .as_str()
            .unwrap()
            .contains("emitted 1 finding and suppressed 2 more"),
        "{failures:?}"
    );
}

#[test]
fn finding_cap_breach_fails_cli_refuses_baseline_and_cannot_be_baselined_away() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "en.json",
        r#"{"a":"1","b":"2","c":"3","d":"4"}"#,
    );
    write(root.path(), "fr.json", "{}");
    // Warning-level rule: without the analysis failure it would exit 0, so
    // exit 1 proves the truncation analysis failure always fails the gate.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nlimits:\n  findings_per_rule: 2\nrules:\n  - id: kp\n    kind: key_parity\n    \
         reference: en.json\n    peers: fr.json\n    level: warning\n",
    );

    let check = run(root.path(), &["check", "--format", "json"]);
    assert_eq!(check.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&check.stdout).unwrap();
    assert_eq!(
        parsed["results"][0]["violations"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        parsed["results"][0]["analysis_failures"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // `alint baseline` refuses to write a baseline when a finding-cap breach
    // emits an analysis failure.
    let bl = run(root.path(), &["baseline"]);
    assert_eq!(bl.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&bl.stderr);
    assert!(stderr.contains("refusing to write baseline"), "{stderr}");
    assert!(!root.path().join(".alint-baseline.json").exists());

    // Even if a pre-existing baseline grandfathers all emitted findings, the
    // truncation analysis failure cannot be suppressed.
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nlimits:\n  findings_per_rule: 10\nrules:\n  - id: kp\n    kind: key_parity\n    \
         reference: en.json\n    peers: fr.json\n    level: warning\n",
    );
    let bl_ok = run(root.path(), &["baseline"]);
    assert!(bl_ok.status.success(), "{bl_ok:?}");

    write(
        root.path(),
        ".alint.yml",
        "version: 1\nlimits:\n  findings_per_rule: 2\nrules:\n  - id: kp\n    kind: key_parity\n    \
         reference: en.json\n    peers: fr.json\n    level: warning\n",
    );
    let check_bl = run(
        root.path(),
        &[
            "check",
            "--baseline",
            ".alint-baseline.json",
            "--format",
            "json",
        ],
    );
    assert_eq!(check_bl.status.code(), Some(1));
    let parsed: serde_json::Value = serde_json::from_slice(&check_bl.stdout).unwrap();
    assert_eq!(
        parsed["results"][0]["analysis_failures"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn nested_key_parity_and_cldr_plurals_and_segment_order_honor_limits() {
    let root = tempfile::tempdir().unwrap();
    // Bracketed key (`a.b` -> `['a.b']`) sorts BEFORE uppercase `Z` in
    // `[Segment]` order (`.` / `a` vs `Z`: `'Z' < 'a'`, wait: `'!' < 'Z' < 'a'`),
    // so key `"!first"` (renders as `"!first"`), `"A.dot"` (renders as
    // `"['A.dot']"` where `'[' > 'Z'`), and `"Z"` prove `[Segment]` order
    // (`"A.dot" < "Z"`) instead of string order (`"Z" < "['A.dot']"`).
    write(
        root.path(),
        "pkgs/one/en.json",
        r#"{"A.dot":"1","Z":"2","items_one":"1","items_other":"2"}"#,
    );
    write(root.path(), "pkgs/one/fr.json", "{}");
    write(
        root.path(),
        ".alint.yml",
        "version: 1\nlimits:\n  findings_per_rule: 1\nrules:\n  - id: nested-kp\n    \
         kind: for_each_dir\n    select: 'pkgs/*'\n    level: error\n    require:\n      \
         - kind: key_parity\n        reference: '{path}/en.json'\n        peers: '{path}/fr.json'\n        \
         plurals: cldr\n",
    );
    let out = run(root.path(), &["check", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let violations = parsed["results"][0]["violations"].as_array().unwrap();
    assert_eq!(
        violations.len(),
        1,
        "nested key_parity must honor configured cap: {parsed}"
    );
    // In `[Segment]` order, `"A.dot"` comes before `"Z"`, even though
    // `"['A.dot']"` sorts after `"Z"` lexicographically.
    assert!(
        violations[0]["message"]
            .as_str()
            .unwrap()
            .contains("['A.dot']"),
        "expected segment-ordered first key `['A.dot']`, got: {}",
        violations[0]["message"]
    );
    let failures = parsed["results"][0]["analysis_failures"]
        .as_array()
        .unwrap();
    assert_eq!(failures.len(), 1, "{parsed}");
    assert!(
        failures[0]["message"]
            .as_str()
            .unwrap()
            .contains("emitted 1 finding and suppressed 2 more"),
        "{failures:?}"
    );
}

#[test]
fn constraint_unique_equal_and_set_families_enforce_configured_finding_cap() {
    let root = tempfile::tempdir().unwrap();
    write(
        root.path(),
        "items.json",
        r#"{"a":{"v":"x","w":"1"},"b":{"v":"x","w":"2"},"c":{"v":"x","w":"3"},"d":{"v":"x","w":"4"}}"#,
    );
    write(
        root.path(),
        ".alint.yml",
        concat!(
            "version: 2\n",
            "limits:\n",
            "  findings_per_rule: 2\n",
            "selectors:\n",
            "  s:\n",
            "    paths: 'items.json'\n",
            "relations:\n",
            "  r:\n",
            "    from: s\n",
            "    entries: { format: json, at: '$.*' }\n",
            "    fields: { k: entry.key, v: entry.value }\n",
            "rules:\n",
            "  - id: unique-cap\n",
            "    kind: constraint\n",
            "    input: r\n",
            "    assert:\n",
            "      unique:\n",
            "        key: [v]\n",
            "    level: error\n",
            "  - id: equal-field-cap\n",
            "    kind: constraint\n",
            "    input: r\n",
            "    assert:\n",
            "      equal:\n",
            "        field: k\n",
            "        to: { field: v }\n",
            "    level: error\n",
        ),
    );
    let out = run(root.path(), &["check", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let results = parsed["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "{parsed}");
    for r in results {
        let violations = r["violations"].as_array().unwrap();
        let failures = r["analysis_failures"].as_array().unwrap();
        assert_eq!(violations.len(), 2, "rule {:?}: {r}", r["rule_id"]);
        assert_eq!(failures.len(), 1, "rule {:?}: {r}", r["rule_id"]);
        assert!(
            failures[0]["message"]
                .as_str()
                .unwrap()
                .contains("relation limit `findings_per_rule` (2) reached"),
            "{failures:?}"
        );
    }
}

#[test]
fn key_path_bytes_per_decode_bounds_ini_xml_resx_and_v2_entries_leaves() {
    let root = tempfile::tempdir().unwrap();

    // 1. INI
    let mut ini = String::from("[very_long_section_name_for_budget_check]\n");
    for i in 0..30 {
        let _ = writeln!(ini, "key_{i:02} = val");
    }
    write(root.path(), "a.ini", &ini);
    write(root.path(), "b.ini", "");

    // 2. Android strings XML
    let mut android = String::from("<resources>\n");
    for i in 0..30 {
        let _ = writeln!(
            android,
            "  <string name=\"long_string_key_{i:02}\">v</string>"
        );
    }
    android.push_str("</resources>\n");
    write(root.path(), "strings-en.xml", &android);
    write(root.path(), "strings-fr.xml", "<resources/>");

    // 3. .NET .resx XML
    let mut resx = String::from("<root>\n");
    for i in 0..30 {
        let _ = writeln!(
            resx,
            "  <data name=\"LongResxKeyName_{i:02}\"><value>v</value></data>"
        );
    }
    resx.push_str("</root>\n");
    write(root.path(), "en.resx", &resx);
    write(root.path(), "fr.resx", "<root/>");

    // 4. v2 `entries: { leaves: true }` over JSON
    let mut json = String::from("{\"deep_section\":{");
    for i in 0..30 {
        if i > 0 {
            json.push(',');
        }
        let _ = write!(json, "\"leaf_key_name_{i:02}\":\"v\"");
    }
    json.push_str("}}");
    write(root.path(), "leaves.json", &json);

    write(
        root.path(),
        ".alint.yml",
        concat!(
            "version: 2\n",
            "limits:\n",
            "  key_path_bytes_per_decode: 128\n",
            "selectors:\n",
            "  j:\n",
            "    paths: 'leaves.json'\n",
            "relations:\n",
            "  r:\n",
            "    from: j\n",
            "    entries: { format: json, at: '$', leaves: true }\n",
            "    fields: { p: entry.path }\n",
            "rules:\n",
            "  - id: kp-ini\n",
            "    kind: key_parity\n",
            "    reference: a.ini\n",
            "    peers: b.ini\n",
            "    level: error\n",
            "  - id: kp-android\n",
            "    kind: key_parity\n",
            "    format: android-strings\n",
            "    reference: strings-en.xml\n",
            "    peers: strings-fr.xml\n",
            "    level: error\n",
            "  - id: kp-resx\n",
            "    kind: key_parity\n",
            "    reference: en.resx\n",
            "    peers: fr.resx\n",
            "    level: error\n",
            "  - id: v2-leaves\n",
            "    kind: constraint\n",
            "    input: r\n",
            "    assert: { unique: { key: [p] } }\n",
            "    level: error\n",
        ),
    );
    let out = run(root.path(), &["check", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let results = parsed["results"].as_array().unwrap();
    assert_eq!(results.len(), 4, "{parsed}");
    for r in results {
        let failures = r["analysis_failures"].as_array().unwrap();
        assert!(
            !failures.is_empty(),
            "expected key_path_bytes_per_decode failure for {:?}: {r}",
            r["rule_id"]
        );
        let msg = failures[0]["message"].as_str().unwrap();
        assert!(
            msg.contains("key_path_bytes_per_decode"),
            "rule {:?} unexpected failure: {msg}",
            r["rule_id"]
        );
    }
}
