use super::*;
use crate::test_support::{ctx, spec_yaml, tempdir_with_files};

#[test]
fn short_render_truncates_on_a_char_boundary_without_panicking() {
    // Regression: `short_render` byte-sliced `&raw[..80]`, which panics
    // when an untrusted matched value puts a multibyte codepoint across
    // byte 80 — crashing the whole parallel run (and the LSP). 78 ASCII
    // + `é`s: serde_json quotes the string, so the byte-80 boundary
    // lands mid-`é`. Must truncate (with the ellipsis), not abort.
    let value = Value::String(format!("{}{}", "a".repeat(78), "é".repeat(8)));
    let rendered = short_render(&value);
    assert!(rendered.ends_with('…'), "expected truncation: {rendered}");
    // A short non-ASCII value is returned whole (quoted JSON rendering).
    assert_eq!(short_render(&Value::String("café".to_string())), "\"café\"");
}

#[test]
fn dotenv_and_properties_mismatches_redact_only_extracted_values() {
    const SECRET: &str = "TOKEN_VALUE_DO_NOT_REPORT";
    for (kind, file, body) in [
        ("dotenv_path_equals", ".env", format!("TOKEN={SECRET}\n")),
        (
            "properties_path_equals",
            "app.properties",
            format!("TOKEN={SECRET}\n"),
        ),
    ] {
        let spec = spec_yaml(&format!(
            "id: t\nkind: {kind}\npaths: \"{file}\"\npath: \"$.TOKEN\"\n\
                 equals: \"configured-public-value\"\nlevel: error\n"
        ));
        let rule = crate::builtin_registry().build(&spec).unwrap();
        let (tmp, idx) = tempdir_with_files(&[(file, body.as_bytes())]);
        let findings = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
        assert_eq!(findings.len(), 1);
        let finding = &findings[0];
        assert!(finding.is_sensitive());
        assert!(!finding.message.contains(SECRET), "{finding:?}");
        assert!(finding.message.contains("<redacted:"), "{finding:?}");
        assert!(
            finding.message.contains("configured-public-value"),
            "configured prose stays visible: {finding:?}"
        );
    }
}

#[test]
fn json_mismatch_with_the_same_value_remains_visible() {
    const VALUE: &str = "TOKEN_VALUE_DO_NOT_REPORT";
    let spec = spec_yaml(
        "id: t\nkind: json_path_equals\npaths: \"a.json\"\npath: \"$.TOKEN\"\n\
             equals: \"configured-public-value\"\nlevel: error\n",
    );
    let rule = json_path_equals_build(&spec).unwrap();
    let body = format!(r#"{{"TOKEN":"{VALUE}"}}"#);
    let (tmp, idx) = tempdir_with_files(&[("a.json", body.as_bytes())]);
    let findings = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(findings.len(), 1);
    assert!(!findings[0].is_sensitive());
    assert!(findings[0].message.contains(VALUE), "{findings:?}");
}

// ─── build-path errors ────────────────────────────────────

#[test]
fn build_rejects_missing_paths() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_equals\n\
             path: \"$.name\"\n\
             equals: \"x\"\n\
             level: error\n",
    );
    assert!(json_path_equals_build(&spec).is_err());
}

#[test]
fn build_rejects_invalid_jsonpath() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$..[invalid\"\n\
             equals: \"x\"\n\
             level: error\n",
    );
    assert!(json_path_equals_build(&spec).is_err());
}

#[test]
fn build_rejects_invalid_regex_in_matches() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_matches\n\
             paths: \"package.json\"\n\
             path: \"$.version\"\n\
             matches: \"[unterminated\"\n\
             level: error\n",
    );
    // Must fail in the regex-compile path (not via
    // deny_unknown_fields on a typo'd `pattern:` key — the
    // latent bug this previously had).
    let e = json_path_matches_build(&spec).unwrap_err().to_string();
    assert!(e.contains("regex"), "expected a regex error, got: {e}");
}

#[test]
fn path_matches_replace_requires_a_pattern() {
    // `*_path_matches` + `replace` needs its OWN search `pattern:` (the rule's
    // `matches:` is the check target, not the search) -> config error without it.
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_matches\n\
             paths: \"**/*.json\"\n\
             path: \"$.v\"\n\
             matches: \"^v\"\n\
             level: error\n\
             fix: { replace: { replacement: \"v\" } }\n",
    );
    let e = json_path_matches_build(&spec).unwrap_err().to_string();
    assert!(
        e.contains("pattern"),
        "expected a missing-pattern error, got: {e}"
    );
}

#[test]
fn path_matches_replace_with_a_pattern_builds() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_matches\n\
             paths: \"**/*.json\"\n\
             path: \"$.v\"\n\
             matches: \"^v\"\n\
             level: error\n\
             fix: { replace: { pattern: \"^\", replacement: \"v\" } }\n",
    );
    assert!(json_path_matches_build(&spec).is_ok());
}

#[test]
fn path_matches_rejects_a_non_replace_fix() {
    // Only `replace` is compatible with `*_path_matches`.
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_matches\n\
             paths: \"**/*.json\"\n\
             path: \"$.v\"\n\
             matches: \"^v\"\n\
             level: error\n\
             fix: { set_value: {} }\n",
    );
    let e = json_path_matches_build(&spec).unwrap_err().to_string();
    assert!(
        e.contains("not compatible") && e.contains("replace"),
        "expected an incompatibility error naming `replace`, got: {e}"
    );
}

// ─── json_path_equals ─────────────────────────────────────

#[test]
fn json_path_equals_passes_when_value_matches() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"demo\"\n\
             level: error\n",
    );
    let rule = json_path_equals_build(&spec).unwrap();
    let (tmp, idx) =
        tempdir_with_files(&[("package.json", br#"{"name":"demo","version":"1.0.0"}"#)]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "matching value should pass: {v:?}");
}

#[test]
fn numeric_equals_compares_by_value_in_every_format() {
    // Regression: `equals: 1` failed against a document `1.0` (and vice
    // versa) because serde_json's `Value` equality distinguishes an integer
    // from a float. Numbers now compare by value, recursively, in every
    // format -- no string coercion, and a different value still fails.
    let check = |kind: &str, file: &str, body: &str, equals: &str| -> usize {
        let rule = crate::builtin_registry()
            .build(&spec_yaml(&format!(
                "id: t\nkind: {kind}\npaths: \"{file}\"\npath: \"$.v\"\n\
                     equals: {equals}\nlevel: error\n"
            )))
            .unwrap();
        let (tmp, idx) = tempdir_with_files(&[(file, body.as_bytes())]);
        rule.evaluate(&ctx(tmp.path(), &idx)).unwrap().len()
    };
    assert_eq!(check("json_path_equals", "a.json", r#"{"v": 1.0}"#, "1"), 0);
    assert_eq!(check("json_path_equals", "a.json", r#"{"v": 1}"#, "1.0"), 0);
    assert_eq!(
        check("json_path_equals", "a.json", r#"{"v": 1e2}"#, "100"),
        0
    );
    assert_eq!(
        check(
            "json_path_equals",
            "a.json",
            r#"{"v": [1.0, {"b": 2}]}"#,
            "[1, {b: 2.0}]"
        ),
        0
    );
    assert_eq!(check("yaml_path_equals", "a.yaml", "v: 1.0\n", "1"), 0);
    assert_eq!(check("toml_path_equals", "a.toml", "v = 1.0\n", "1"), 0);
    assert_eq!(check("hcl_path_equals", "a.hcl", "v = 1\n", "1.0"), 0);
    // Different values (or a numeric string) still fail.
    assert_eq!(check("json_path_equals", "a.json", r#"{"v": 1.5}"#, "1"), 1);
    assert_eq!(check("json_path_equals", "a.json", r#"{"v": "1"}"#, "1"), 1);
    assert_eq!(
        check("json_path_equals", "a.json", r#"{"v": [1, 2]}"#, "[1]"),
        1
    );
}

#[test]
fn integer_against_float_compares_exactly() {
    // Regression: an int-vs-float pair went through `as_f64`, so integers
    // beyond 2^53 equalled a nearby float they don't (main flagged these).
    let n = |s: &str| serde_json::from_str::<Value>(s).unwrap();
    assert!(!values_equal(
        &n("9007199254740993"),
        &n("9007199254740992.0")
    ));
    assert!(values_equal(
        &n("9007199254740992"),
        &n("9007199254740992.0")
    ));
    assert!(!values_equal(
        &n("18446744073709551615"),
        &n("1.8446744073709552e19")
    ));
    assert!(!values_equal(
        &n("9223372036854775807"),
        &n("9.223372036854775807e18")
    ));
    assert!(values_equal(&n("-3"), &n("-3.0")));
    assert!(values_equal(&n("3.0"), &n("3")));
    assert!(!values_equal(&n("3"), &n("3.5")));
    assert!(values_equal(&n("1.5"), &n("1.5")));
}

#[test]
fn non_utf8_file_is_analyzed_not_silently_skipped() {
    // A non-UTF-8 file (here a Latin-1 `.properties` with 0xE9 = é, the
    // format's historical default encoding) must be lossy-decoded and
    // ANALYZED, not silently dropped: the ASCII `server.port` still resolves,
    // so the mismatch fires. Before, strict `from_utf8` skipped the whole file
    // and this passed clean -- a silent false-negative.
    let spec = spec_yaml(
        "id: t\n\
             kind: properties_path_equals\n\
             paths: \"application.properties\"\n\
             path: \"$['server.port']\"\n\
             equals: \"9999\"\n\
             level: error\n",
    );
    let rule = properties_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
        "application.properties",
        b"server.port=8080\nadmin.name=Ren\xe9\n",
    )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(
        v.len(),
        1,
        "the Latin-1 file must be analyzed (port 8080 != 9999): {v:?}"
    );
}

#[test]
fn json_path_equals_fires_on_mismatch() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"demo\"\n\
             level: error\n",
    );
    let rule = json_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"name":"other"}"#)]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(v.len(), 1);
}

#[test]
fn json_path_equals_fires_on_missing_path() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"demo\"\n\
             level: error\n",
    );
    let rule = json_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"version":"1.0"}"#)]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(v.len(), 1, "missing path should fire");
}

#[test]
fn json_path_if_present_silent_on_missing() {
    // `if_present: true` → missing path is silent.
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"demo\"\n\
             if_present: true\n\
             level: error\n",
    );
    let rule = json_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"version":"1.0"}"#)]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "if_present should silence: {v:?}");
}

// ─── json_path_matches ────────────────────────────────────

#[test]
fn json_path_matches_passes_on_pattern_hit() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_matches\n\
             paths: \"package.json\"\n\
             path: \"$.version\"\n\
             matches: \"^\\\\d+\\\\.\\\\d+\\\\.\\\\d+$\"\n\
             level: error\n",
    );
    let rule = json_path_matches_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"version":"1.2.3"}"#)]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "matching version should pass: {v:?}");
}

#[test]
fn json_path_matches_fires_on_pattern_miss() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_matches\n\
             paths: \"package.json\"\n\
             path: \"$.version\"\n\
             matches: \"^\\\\d+\\\\.\\\\d+\\\\.\\\\d+$\"\n\
             level: error\n",
    );
    let rule = json_path_matches_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("package.json", br#"{"version":"v1.x"}"#)]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(v.len(), 1);
}

// ─── yaml_path_* ─────────────────────────────────────────

#[test]
fn yaml_path_equals_passes_when_value_matches() {
    let spec = spec_yaml(
        "id: t\n\
             kind: yaml_path_equals\n\
             paths: \".github/workflows/*.yml\"\n\
             path: \"$.name\"\n\
             equals: \"CI\"\n\
             level: error\n",
    );
    let rule = yaml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
        ".github/workflows/ci.yml",
        b"name: CI\non: push\njobs: {}\n",
    )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "matching name should pass: {v:?}");
}

#[test]
fn yaml_path_matches_uses_bracket_notation_for_dashed_keys() {
    // Per the memory note: dashed YAML keys need bracket
    // notation (`$.foo['dashed-key']`) because the JSONPath
    // dot-form can't parse them.
    let spec = spec_yaml(
        "id: t\n\
             kind: yaml_path_matches\n\
             paths: \"action.yml\"\n\
             path: \"$.runs['using']\"\n\
             matches: \"^node\\\\d+$\"\n\
             level: error\n",
    );
    let rule = yaml_path_matches_build(&spec).unwrap();
    let (tmp, idx) =
        tempdir_with_files(&[("action.yml", b"runs:\n  using: node20\n  main: index.js\n")]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "bracket notation should match: {v:?}");
}

// ─── yaml_path_absent ────────────────────────────────────

#[test]
fn yaml_path_absent_passes_when_query_matches_nothing() {
    // The path the rule forbids isn't there -> pass.
    let spec = spec_yaml(
        "id: t\n\
             kind: yaml_path_absent\n\
             paths: \".github/workflows/*.yml\"\n\
             path: \"$.permissions\"\n\
             level: error\n",
    );
    let rule = yaml_path_absent_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
        ".github/workflows/ci.yml",
        b"name: CI\non: push\njobs: {}\n",
    )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "absent path should pass: {v:?}");
}

#[test]
fn yaml_path_absent_fires_once_when_query_matches() {
    // The forbidden path exists -> exactly one file-level violation.
    let spec = spec_yaml(
        "id: t\n\
             kind: yaml_path_absent\n\
             paths: \".github/workflows/*.yml\"\n\
             path: \"$.permissions\"\n\
             level: error\n",
    );
    let rule = yaml_path_absent_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
        ".github/workflows/ci.yml",
        b"name: CI\npermissions: write-all\non: push\njobs: {}\n",
    )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(v.len(), 1, "present path should fire once: {v:?}");
}

#[test]
fn dotenv_path_absent_marks_the_value_driven_finding_sensitive() {
    const SECRET: &str = "ABSENT_SECRET_DO_NOT_REPORT";
    let spec = spec_yaml(
        "id: t\n\
             kind: dotenv_path_absent\n\
             paths: \".env\"\n\
             path: \"$.TOKEN\"\n\
             level: error\n\
             fix: { remove_value: {} }\n",
    );
    let rule = dotenv_path_absent_build(&spec).unwrap();
    let body = format!("TOKEN={SECRET}\n");
    let (tmp, idx) = tempdir_with_files(&[(".env", body.as_bytes())]);
    let findings = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();

    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].is_sensitive(), "{findings:?}");
    assert!(!findings[0].message.contains(SECRET), "{findings:?}");
}

#[test]
fn yaml_path_absent_filter_fanout_collapses_to_one_violation() {
    // A root-level filter `$[?…]` selects *every* top-level key when the
    // predicate holds (N nodes). Absent-mode must still yield exactly ONE
    // file-level violation -- this is the whole point of the kind (vs. a
    // `yaml_path_equals` + sentinel filter, which fans out to N warnings).
    let spec = spec_yaml(
        "id: t\n\
             kind: yaml_path_absent\n\
             paths: \"w.yml\"\n\
             path: \"$[?($.permissions == 'write-all')]\"\n\
             level: error\n",
    );
    let rule = yaml_path_absent_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
        "w.yml",
        b"name: CI\npermissions: write-all\non: push\njobs: {}\n",
    )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(
        v.len(),
        1,
        "filter fan-out must collapse to one file-level violation: {v:?}"
    );
}

#[test]
fn yaml_path_absent_rejects_value_and_if_present_options() {
    // `if_present` / `equals` / `matches` are not valid on an absent rule
    // (deny_unknown_fields).
    let spec = spec_yaml(
        "id: t\n\
             kind: yaml_path_absent\n\
             paths: \"w.yml\"\n\
             path: \"$.x\"\n\
             if_present: true\n\
             level: error\n",
    );
    assert!(
        yaml_path_absent_build(&spec).is_err(),
        "if_present must be rejected on yaml_path_absent"
    );
}

// ─── toml_path_* ─────────────────────────────────────────

#[test]
fn toml_path_equals_passes_when_value_matches() {
    let spec = spec_yaml(
        "id: t\n\
             kind: toml_path_equals\n\
             paths: \"Cargo.toml\"\n\
             path: \"$.package.edition\"\n\
             equals: \"2024\"\n\
             level: error\n",
    );
    let rule = toml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
        "Cargo.toml",
        b"[package]\nname = \"x\"\nedition = \"2024\"\n",
    )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "matching edition should pass: {v:?}");
}

#[test]
fn toml_path_matches_fires_on_floating_version() {
    // Common policy: deps must be tilde-pinned, not bare.
    let spec = spec_yaml(
        "id: t\n\
             kind: toml_path_matches\n\
             paths: \"Cargo.toml\"\n\
             path: \"$.dependencies.serde\"\n\
             matches: \"^[~=]\"\n\
             level: error\n",
    );
    let rule = toml_path_matches_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
        "Cargo.toml",
        b"[package]\nname = \"x\"\n[dependencies]\nserde = \"1\"\n",
    )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(v.len(), 1, "floating `serde = \"1\"` should fire");
}

// ─── xml_path_* ──────────────────────────────────────────

#[test]
fn xml_path_equals_passes_on_csproj_leaf() {
    let spec = spec_yaml(
        "id: t\n\
             kind: xml_path_equals\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project.PropertyGroup.TargetFramework\"\n\
             equals: \"net8.0\"\n\
             level: error\n",
    );
    let rule = xml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
            "App.csproj",
            br#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>"#,
        )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "leaf element should match: {v:?}");
}

#[test]
fn xml_path_equals_fires_on_csproj_mismatch() {
    let spec = spec_yaml(
        "id: t\n\
             kind: xml_path_equals\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project.PropertyGroup.TargetFramework\"\n\
             equals: \"net8.0\"\n\
             level: error\n",
    );
    let rule = xml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
            "App.csproj",
            br"<Project><PropertyGroup><TargetFramework>net6.0</TargetFramework></PropertyGroup></Project>",
        )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(v.len(), 1);
}

#[test]
fn xml_path_matches_on_packageref_attribute_array() {
    // Repeated <PackageReference> → array; `@Version`
    // attribute reached via bracket notation; every match
    // must be a non-empty version-ish string.
    let spec = spec_yaml(
        "id: t\n\
             kind: xml_path_matches\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project.ItemGroup.PackageReference[*]['@Version']\"\n\
             matches: \"^\\\\d\"\n\
             level: error\n",
    );
    let rule = xml_path_matches_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
            "App.csproj",
            br#"<Project><ItemGroup><PackageReference Include="A" Version="1.2.3"/><PackageReference Include="B" Version="4.0.0"/></ItemGroup></Project>"#,
        )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "both @Version attrs should match: {v:?}");
}

#[test]
fn xml_pom_namespace_flattened_and_repeated_dependency_array() {
    // Maven default namespace must not leak into the query;
    // repeated <dependency> must be an array.
    let pom = br#"<project xmlns="http://maven.apache.org/POM/4.0.0"><modelVersion>4.0.0</modelVersion><dependencies><dependency><artifactId>guava</artifactId></dependency><dependency><artifactId>junit</artifactId></dependency></dependencies></project>"#;
    let eq = spec_yaml(
        "id: t\n\
             kind: xml_path_equals\n\
             paths: \"pom.xml\"\n\
             path: \"$.project.modelVersion\"\n\
             equals: \"4.0.0\"\n\
             level: error\n",
    );
    let (tmp, idx) = tempdir_with_files(&[("pom.xml", pom)]);
    assert!(
        xml_path_equals_build(&eq)
            .unwrap()
            .evaluate(&ctx(tmp.path(), &idx))
            .unwrap()
            .is_empty(),
        "namespace-flattened modelVersion should match"
    );
    let m = spec_yaml(
        "id: t\n\
             kind: xml_path_matches\n\
             paths: \"pom.xml\"\n\
             path: \"$.project.dependencies.dependency[*].artifactId\"\n\
             matches: \"^[a-z]+$\"\n\
             level: error\n",
    );
    let v = xml_path_matches_build(&m)
        .unwrap()
        .evaluate(&ctx(tmp.path(), &idx))
        .unwrap();
    assert!(v.is_empty(), "both deps' artifactId should match: {v:?}");
}

#[test]
fn xml_path_if_present_silences_missing() {
    let spec = spec_yaml(
        "id: t\n\
             kind: xml_path_equals\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project.PropertyGroup.Nullable\"\n\
             equals: \"enable\"\n\
             if_present: true\n\
             level: error\n",
    );
    let rule = xml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[(
            "App.csproj",
            br"<Project><PropertyGroup><TargetFramework>net8.0</TargetFramework></PropertyGroup></Project>",
        )]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert!(v.is_empty(), "if_present should silence missing: {v:?}");
}

#[test]
fn xml_malformed_emits_one_analysis_failure() {
    let spec = spec_yaml(
        "id: t\n\
             kind: xml_path_equals\n\
             paths: \"App.csproj\"\n\
             path: \"$.Project\"\n\
             equals: \"x\"\n\
             level: error\n",
    );
    let rule = xml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("App.csproj", b"<Project><Unclosed></Project>")]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(v.len(), 1, "not-well-formed XML should fail analysis once");
    assert!(v[0].message.contains("XML"), "{:?}", v[0].message);
}

#[test]
fn xml_deeply_nested_is_a_parse_error_not_an_abort() {
    // P1 regression: unbounded recursion would `abort()` the
    // whole process. The `MAX_XML_DEPTH` guard must instead
    // yield exactly one `resource_parse` analysis failure for
    // the file (no panic, no abort, per-file contained).
    let depth = alint_core::MAX_XML_DEPTH + 50;
    let xml = format!("{}deep{}", "<a>".repeat(depth), "</a>".repeat(depth));
    let spec = spec_yaml(
        "id: t\n\
             kind: xml_path_equals\n\
             paths: \"deep.xml\"\n\
             path: \"$.a\"\n\
             equals: \"x\"\n\
             level: error\n",
    );
    let rule = xml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("deep.xml", xml.as_bytes())]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(
        v.len(),
        1,
        "deeply-nested XML must yield exactly one parse analysis failure: {v:?}"
    );
    assert!(
        v[0].message.contains("not a valid XML") && v[0].message.contains("depth"),
        "expected a depth parse-error message, got: {}",
        v[0].message
    );
}

#[test]
fn xml_depth_beyond_parse_recursion_limit_is_rejected_pre_parse_not_aborted() {
    // Rejecting very deeply nested XML upfront in `xml_within_parse_limits`
    // avoids building a huge DOM arena only for `element_to_value` to reject it
    // (or overflowing worker stacks during DOM conversion / drop), returning a
    // contained parse analysis failure instead.
    let depth = 100_000;
    let xml = format!("{}deep{}", "<a>".repeat(depth), "</a>".repeat(depth));
    let spec = spec_yaml(
        "id: t\nkind: xml_path_equals\npaths: \"deep.xml\"\n\
             path: \"$.a\"\nequals: \"x\"\nlevel: error\n",
    );
    let rule = xml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("deep.xml", xml.as_bytes())]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(
        v.len(),
        1,
        "must be one contained parse-error, no abort: {v:?}"
    );
    assert!(v[0].message.contains("depth"), "{}", v[0].message);
}

#[test]
fn yaml_flow_depth_bomb_is_rejected_pre_parse_not_hung() {
    // W2 wiring regression: a `yaml_path_*` rule over a deeply-nested FLOW
    // document (`[[[…`) must be rejected as a contained parse-error before
    // any parser reads it (the parser v0.22 used hung on it). This exercises the `flow_depth_within_limit` guard *at its
    // structured-query call site* — the yaml_depth unit tests only cover the
    // scanner in isolation, so without this a deleted guard here would pass
    // CI while reopening the DoS.
    let depth = 5000;
    let yaml = format!("x: {}1{}", "[".repeat(depth), "]".repeat(depth));
    let spec = spec_yaml(
        "id: t\nkind: yaml_path_equals\npaths: \"bomb.yml\"\n\
             path: \"$.x\"\nequals: \"1\"\nlevel: error\n",
    );
    let rule = yaml_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("bomb.yml", yaml.as_bytes())]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(
        v.len(),
        1,
        "must be one contained parse-error, no hang: {v:?}"
    );
    assert!(
        v[0].message.contains("depth"),
        "the flow-depth guard message should mention depth: {}",
        v[0].message
    );
}

#[test]
fn xml_leaf_values_are_string_typed() {
    // Documented gotcha: every XML leaf is a string. A
    // quoted `equals: "8"` matches; a bare `equals: 8`
    // (a YAML integer) does not.
    let xml: &[u8] = b"<Config><n>8</n></Config>";
    let as_str = spec_yaml(
        "id: t\n\
             kind: xml_path_equals\n\
             paths: \"c.xml\"\n\
             path: \"$.Config.n\"\n\
             equals: \"8\"\n\
             level: error\n",
    );
    let (tmp, idx) = tempdir_with_files(&[("c.xml", xml)]);
    assert!(
        xml_path_equals_build(&as_str)
            .unwrap()
            .evaluate(&ctx(tmp.path(), &idx))
            .unwrap()
            .is_empty(),
        "string 8 should match the string-typed leaf"
    );
    let as_int = spec_yaml(
        "id: t\n\
             kind: xml_path_equals\n\
             paths: \"c.xml\"\n\
             path: \"$.Config.n\"\n\
             equals: 8\n\
             level: error\n",
    );
    let v = xml_path_equals_build(&as_int)
        .unwrap()
        .evaluate(&ctx(tmp.path(), &idx))
        .unwrap();
    assert_eq!(v.len(), 1, "integer 8 must NOT equal string \"8\"");
}

#[test]
fn xml_empty_element_is_null() {
    // Design-doc promise (was untested): an empty element
    // maps to JSON null — `equals: null` matches; `equals:
    // ""` does not (it is null, not an empty string).
    let xml: &[u8] = b"<Config><empty/></Config>";
    let (tmp, idx) = tempdir_with_files(&[("c.xml", xml)]);
    let as_null = spec_yaml(
        "id: t\nkind: xml_path_equals\npaths: \"c.xml\"\n\
             path: \"$.Config.empty\"\nequals: null\nlevel: error\n",
    );
    assert!(
        xml_path_equals_build(&as_null)
            .unwrap()
            .evaluate(&ctx(tmp.path(), &idx))
            .unwrap()
            .is_empty(),
        "an empty element must equal null"
    );
    let as_empty_str = spec_yaml(
        "id: t\nkind: xml_path_equals\npaths: \"c.xml\"\n\
             path: \"$.Config.empty\"\nequals: \"\"\nlevel: error\n",
    );
    assert_eq!(
        xml_path_equals_build(&as_empty_str)
            .unwrap()
            .evaluate(&ctx(tmp.path(), &idx))
            .unwrap()
            .len(),
        1,
        "null must NOT equal the empty string"
    );
}

// ─── parse error path ─────────────────────────────────────

#[test]
fn evaluate_fires_on_malformed_input() {
    let spec = spec_yaml(
        "id: t\n\
             kind: json_path_equals\n\
             paths: \"package.json\"\n\
             path: \"$.name\"\n\
             equals: \"x\"\n\
             level: error\n",
    );
    let rule = json_path_equals_build(&spec).unwrap();
    let (tmp, idx) = tempdir_with_files(&[("package.json", b"{not valid json")]);
    let v = rule.evaluate(&ctx(tmp.path(), &idx)).unwrap();
    assert_eq!(v.len(), 1, "malformed JSON should fire one violation");
}

#[test]
fn selected_trace_aggregates_structured_extraction_and_cardinality_without_reparsing() {
    let spec = std::sync::Arc::new(spec_yaml(
        "id: traced\nkind: json_path_equals\npaths: \"**/*.json\"\n\
         path: \"$.items[*]\"\nequals: 1\nlevel: error\n",
    ));
    let registry = crate::builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = alint_core::Engine::from_entries(
        vec![alint_core::RuleEntry::new(rule).with_spec(spec)],
        registry,
    );
    let (tmp, index) = tempdir_with_files(&[
        ("a.json", br#"{"items":[1,2]}"#),
        ("b.json", br#"{"other":true}"#),
    ]);

    let trace = engine.trace_rule("traced", tmp.path(), &index).unwrap();
    let extraction = trace
        .stages
        .iter()
        .find(|stage| stage.class == alint_core::TraceStageClass::Extraction)
        .unwrap();
    assert_eq!(extraction.status, alint_core::TraceStatus::Satisfied);
    assert_eq!(extraction.counts.inputs, 2);
    assert_eq!(extraction.counts.values, 2);
    let cardinality = trace
        .stages
        .iter()
        .find(|stage| stage.class == alint_core::TraceStageClass::Cardinality)
        .unwrap();
    assert_eq!(cardinality.status, alint_core::TraceStatus::Violated);
    assert_eq!(cardinality.counts.inputs, 2);
    assert_eq!(cardinality.counts.values, 2);
    assert_eq!(cardinality.counts.violations, 1);
    assert_eq!(trace.result.as_ref().unwrap().violations.len(), 2);
}

#[test]
fn selected_trace_preserves_structured_parse_failure() {
    let spec = std::sync::Arc::new(spec_yaml(
        "id: traced\nkind: json_path_equals\npaths: \"bad.json\"\n\
         path: \"$.name\"\nequals: \"x\"\nlevel: error\n",
    ));
    let registry = crate::builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = alint_core::Engine::from_entries(
        vec![alint_core::RuleEntry::new(rule).with_spec(spec)],
        registry,
    );
    let (tmp, index) = tempdir_with_files(&[("bad.json", b"{not json")]);

    let trace = engine.trace_rule("traced", tmp.path(), &index).unwrap();
    assert_eq!(trace.outcome, alint_core::TraceStatus::AnalysisFailure);
    assert!(trace.stages.iter().any(|stage| {
        stage.class == alint_core::TraceStageClass::Extraction
            && stage.status == alint_core::TraceStatus::AnalysisFailure
            && stage.reason == alint_core::TraceReason::ParseFailed
            && stage.counts.analysis_failures == 1
    }));
    assert!(trace.stages.iter().any(|stage| {
        stage.class == alint_core::TraceStageClass::Cardinality
            && stage.status == alint_core::TraceStatus::NotEvaluated
            && stage.reason == alint_core::TraceReason::ParseFailed
            && stage.counts.inputs == 0
    }));
    assert_eq!(
        trace.result.as_ref().unwrap().analysis_failures[0].kind,
        alint_core::AnalysisFailureKind::ResourceParse
    );
}

#[test]
fn selected_trace_keeps_pre_extraction_read_failure_as_analysis_failure() {
    let spec = std::sync::Arc::new(spec_yaml(
        "id: traced\nkind: json_path_equals\npaths: \"gone.json\"\n\
         path: \"$.name\"\nequals: \"x\"\nlevel: error\n",
    ));
    let registry = crate::builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = alint_core::Engine::from_entries(
        vec![alint_core::RuleEntry::new(rule).with_spec(spec)],
        registry,
    );
    let (tmp, index) = tempdir_with_files(&[("gone.json", br#"{"name":"x"}"#)]);
    std::fs::remove_file(tmp.path().join("gone.json")).unwrap();

    let trace = engine.trace_rule("traced", tmp.path(), &index).unwrap();
    assert_eq!(trace.outcome, alint_core::TraceStatus::AnalysisFailure);
    assert!(
        !trace
            .stages
            .iter()
            .any(|stage| stage.class == alint_core::TraceStageClass::Extraction)
    );
    assert_eq!(
        trace.result.as_ref().unwrap().analysis_failures[0].kind,
        alint_core::AnalysisFailureKind::ResourceRead
    );
}

#[test]
fn selected_trace_allows_conditional_extraction_to_skip_binary_input() {
    let spec = std::sync::Arc::new(spec_yaml(
        "id: traced\nkind: json_path_equals\npaths: \"binary.json\"\n\
         path: \"$.name\"\nequals: \"x\"\nlevel: error\n",
    ));
    let registry = crate::builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let engine = alint_core::Engine::from_entries(
        vec![alint_core::RuleEntry::new(rule).with_spec(spec)],
        registry,
    );
    let (tmp, index) = tempdir_with_files(&[("binary.json", b"{\0}")]);

    let trace = engine.trace_rule("traced", tmp.path(), &index).unwrap();
    assert_eq!(trace.outcome, alint_core::TraceStatus::Satisfied);
    assert!(
        !trace
            .stages
            .iter()
            .any(|stage| stage.class == alint_core::TraceStageClass::Extraction)
    );
}

#[test]
fn duplicate_literal_paths_match_file_major_and_rule_major_counts() {
    let spec = std::sync::Arc::new(spec_yaml(
        "id: dup\nkind: json_path_equals\npaths: [\"a.json\", \"a.json\"]\n\
         path: \"$.x\"\nequals: 1\nlevel: error\n",
    ));
    let registry = crate::builtin_registry();
    let rule = registry.build(&spec).unwrap();
    let (tmp, index) = tempdir_with_files(&[("a.json", br#"{"x":2}"#)]);
    let rule_major = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
    let engine = alint_core::Engine::from_entries(
        vec![alint_core::RuleEntry::new(rule).with_spec(spec)],
        registry,
    );
    let report = engine.run(tmp.path(), &index).unwrap();
    assert_eq!(
        rule_major.len(),
        1,
        "rule-major must deduplicate literal paths"
    );
    assert_eq!(report.results[0].violations.len(), 1);
}

#[test]
#[allow(clippy::too_many_lines)]
fn all_eight_formats_sweep_equals_matches_and_absent_pass_and_fail() {
    struct Case<'a> {
        prefix: &'a str,
        filename: &'a str,
        content: &'a str,
        present_path: &'a str,
        absent_path: &'a str,
        equals_ok: &'a str,
        equals_bad: &'a str,
        matches_ok: &'a str,
        matches_bad: &'a str,
    }
    let registry = crate::builtin_registry();
    let cases = [
        Case {
            prefix: "json",
            filename: "a.json",
            content: r#"{"env": "prod", "secret": "x"}"#,
            present_path: "$.env",
            absent_path: "$.secret",
            equals_ok: "\"prod\"",
            equals_bad: "\"dev\"",
            matches_ok: "^prod$",
            matches_bad: "^staging$",
        },
        Case {
            prefix: "yaml",
            filename: "a.yml",
            content: "env: prod\nsecret: x\n",
            present_path: "$.env",
            absent_path: "$.secret",
            equals_ok: "\"prod\"",
            equals_bad: "\"dev\"",
            matches_ok: "^prod$",
            matches_bad: "^staging$",
        },
        Case {
            prefix: "toml",
            filename: "a.toml",
            content: "[app]\nenv = \"prod\"\nsecret = \"x\"\n",
            present_path: "$.app.env",
            absent_path: "$.app.secret",
            equals_ok: "\"prod\"",
            equals_bad: "\"dev\"",
            matches_ok: "^prod$",
            matches_bad: "^staging$",
        },
        Case {
            prefix: "xml",
            filename: "a.xml",
            content: "<root><env>prod</env><secret>x</secret></root>",
            present_path: "$.root.env",
            absent_path: "$.root.secret",
            equals_ok: "\"prod\"",
            equals_bad: "\"dev\"",
            matches_ok: "^prod$",
            matches_bad: "^staging$",
        },
        Case {
            prefix: "dotenv",
            filename: ".env",
            content: "ENV=prod\nSECRET=x\n",
            present_path: "$.ENV",
            absent_path: "$.SECRET",
            equals_ok: "\"prod\"",
            equals_bad: "\"dev\"",
            matches_ok: "^prod$",
            matches_bad: "^staging$",
        },
        Case {
            prefix: "properties",
            filename: "a.properties",
            content: "app.env=prod\napp.secret=x\n",
            present_path: "$['app.env']",
            absent_path: "$['app.secret']",
            equals_ok: "\"prod\"",
            equals_bad: "\"dev\"",
            matches_ok: "^prod$",
            matches_bad: "^staging$",
        },
        Case {
            prefix: "ini",
            filename: "a.ini",
            content: "[app]\nenv = prod\nsecret = x\n",
            present_path: "$['app']['env']",
            absent_path: "$['app']['secret']",
            equals_ok: "\"prod\"",
            equals_bad: "\"dev\"",
            matches_ok: "^prod$",
            matches_bad: "^staging$",
        },
        Case {
            prefix: "hcl",
            filename: "a.tf",
            content: "locals {\n  env = \"prod\"\n  secret = \"x\"\n}\n",
            present_path: "$.locals.env",
            absent_path: "$.locals.secret",
            equals_ok: "\"prod\"",
            equals_bad: "\"dev\"",
            matches_ok: "^prod$",
            matches_bad: "^staging$",
        },
    ];

    for c in cases {
        let (tmp, idx) = tempdir_with_files(&[(c.filename, c.content.as_bytes())]);

        // *_path_equals pass & fail
        let eq_ok = spec_yaml(&format!(
            "id: t\nkind: {0}_path_equals\npaths: \"{1}\"\npath: \"{2}\"\nequals: {3}\nlevel: error\n",
            c.prefix, c.filename, c.present_path, c.equals_ok
        ));
        assert!(
            registry
                .build(&eq_ok)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .is_empty(),
            "{}_path_equals pass failed",
            c.prefix
        );
        let eq_bad = spec_yaml(&format!(
            "id: t\nkind: {0}_path_equals\npaths: \"{1}\"\npath: \"{2}\"\nequals: {3}\nlevel: error\n",
            c.prefix, c.filename, c.present_path, c.equals_bad
        ));
        assert_eq!(
            registry
                .build(&eq_bad)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .len(),
            1,
            "{}_path_equals fail should fire",
            c.prefix
        );

        // *_path_matches pass & fail
        let m_ok = spec_yaml(&format!(
            "id: t\nkind: {0}_path_matches\npaths: \"{1}\"\npath: \"{2}\"\nmatches: \"{3}\"\nlevel: error\n",
            c.prefix, c.filename, c.present_path, c.matches_ok
        ));
        assert!(
            registry
                .build(&m_ok)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .is_empty(),
            "{}_path_matches pass failed",
            c.prefix
        );
        let m_bad = spec_yaml(&format!(
            "id: t\nkind: {0}_path_matches\npaths: \"{1}\"\npath: \"{2}\"\nmatches: \"{3}\"\nlevel: error\n",
            c.prefix, c.filename, c.present_path, c.matches_bad
        ));
        assert_eq!(
            registry
                .build(&m_bad)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .len(),
            1,
            "{}_path_matches fail should fire",
            c.prefix
        );

        // *_path_absent pass (missing path) & fail (present path)
        let abs_ok = spec_yaml(&format!(
            "id: t\nkind: {0}_path_absent\npaths: \"{1}\"\npath: \"$.definitely_not_here\"\nlevel: error\n",
            c.prefix, c.filename
        ));
        assert!(
            registry
                .build(&abs_ok)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .is_empty(),
            "{}_path_absent pass failed",
            c.prefix
        );
        let abs_bad = spec_yaml(&format!(
            "id: t\nkind: {0}_path_absent\npaths: \"{1}\"\npath: \"{2}\"\nlevel: error\n",
            c.prefix, c.filename, c.absent_path
        ));
        assert_eq!(
            registry
                .build(&abs_bad)
                .unwrap()
                .evaluate(&ctx(tmp.path(), &idx))
                .unwrap()
                .len(),
            1,
            "{}_path_absent fail should fire",
            c.prefix
        );
    }
}
