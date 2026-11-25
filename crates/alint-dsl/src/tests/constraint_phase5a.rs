//! End-to-end tests for Phase 5 Batch 5a (`#593`, `#594`, `#595`, `#598`):
//! optional `bound:` fields, `{{value}}` in `constraint` `message:`, structural
//! target-file `set` placement, and stub locale root groups under
//! `group_by: entry.root`.

use super::{failures, findings, result, run};

// ---------------------------------------------------------------------------
// #593: Optional `bound:` queries
// ---------------------------------------------------------------------------

#[test]
fn optional_bound_skip_excludes_resource_from_rows_and_resource_groups() {
    let config = "\
version: 2
selectors:
  manifests: { paths: 'packages/*/package.json' }
relations:
  deps:
    from: manifests
    entries: { format: json, at: $.dependencies }
    fields:
      pkg: { source: 'bound: $.name', optional: true }
      dir: parent_name
      dep: entry.key
rules:
  - id: parity
    kind: constraint
    input: deps
    assert:
      set:
        field: dep
        group_by: dir
        equal_to: { group: app }
    level: error
";
    // `packages/root-meta/package.json` has no `name`, so `optional: true`
    // (default `on_missing: skip`) skips it completely -- it produces zero rows
    // AND does not form a zero-row `resource_groups` group for `dir = "root-meta"`.
    let results = run(
        config,
        &[
            (
                "packages/app/package.json",
                r#"{"name": "app", "dependencies": {"react": "18.0.0"}}"#,
            ),
            (
                "packages/web/package.json",
                r#"{"name": "web", "dependencies": {"react": "18.0.0"}}"#,
            ),
            (
                "packages/root-meta/package.json",
                r#"{"private": true, "dependencies": {}}"#,
            ),
        ],
    );
    assert_eq!(findings(&results, "parity"), Vec::<String>::new());
}

#[test]
fn optional_bound_on_missing_missing_keeps_rows_with_missing_cell() {
    let config = "\
version: 2
selectors:
  manifests: { paths: '*/package.json' }
relations:
  deps:
    from: manifests
    entries: { format: json, at: $.dependencies }
    fields:
      license: { source: 'bound: $.license', optional: true, on_missing: missing }
      dep: entry.key
rules:
  - id: same-license
    kind: constraint
    input: deps
    assert:
      equal:
        field: license
    level: error
";
    let results = run(
        config,
        &[
            (
                "a/package.json",
                r#"{"license": "MIT", "dependencies": {"react": "18.0.0"}}"#,
            ),
            ("b/package.json", r#"{"dependencies": {"react": "18.0.0"}}"#),
        ],
    );
    let found = findings(&results, "same-license");
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found
            .iter()
            .any(|f| f.contains("b/package.json") && f.contains("missing")),
        "{found:?}"
    );
}

#[test]
fn non_optional_bound_still_fails_loud_and_hints_optional_true() {
    let config = "\
version: 2
selectors:
  manifests: { paths: 'package.json' }
relations:
  deps:
    from: manifests
    entries: { format: json, at: $.dependencies }
    fields:
      pkg: 'bound: $.name'
      dep: entry.key
rules:
  - id: c
    kind: constraint
    input: deps
    assert: { unique: { key: [dep] } }
    level: error
";
    let results = run(
        config,
        &[("package.json", r#"{"dependencies": {"react": "18.0.0"}}"#)],
    );
    let errs = failures(&results, "c");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(
        errs[0].contains("mark the field `optional: true`"),
        "{errs:?}"
    );
}

// ---------------------------------------------------------------------------
// #594: `{{value}}` placeholder in `constraint` `message:`
// ---------------------------------------------------------------------------

#[test]
fn set_message_renders_value_for_missing_extra_shared_relation_target_and_plurals() {
    // 1. Missing + extra with `{ group: }`, whitespace `{{ value }}`, and literal `{{name}}`.
    let config = "\
version: 2
selectors:
  locales: { paths: 'locales/*.json' }
relations:
  keys:
    from: locales
    entries: { format: json, at: $, leaves: true }
    fields:
      locale: stem
      key: entry.path
rules:
  - id: parity
    kind: constraint
    input: keys
    assert:
      set:
        field: key
        group_by: locale
        equal_to: { group: en }
    message: 'locale {{row.locale}} mismatch on {{value}} / {{ value }} (keep {{name}})'
    level: error
";
    let results = run(
        config,
        &[
            ("locales/en.json", r#"{"home": "Home", "about": "About"}"#),
            ("locales/fr.json", r#"{"home": "Accueil", "extra": "X"}"#),
        ],
    );
    let found = findings(&results, "parity");
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0]
            .starts_with("locales/fr.json: locale en mismatch on about / about (keep {{name}})"),
        "{found:?}"
    );
    assert!(
        found[1].starts_with(
            "locales/fr.json:1:21: locale fr mismatch on extra / extra (keep {{name}})"
        ),
        "{found:?}"
    );
}

#[test]
fn set_message_renders_value_for_relation_target_shared_and_plurals() {
    // Cross-relation target (`{ relation: R, field: F }`): missing has `<no row>`
    // for `{{row.FIELD}}` yet `{{value}}` renders the missing value cleanly;
    // `disjoint_from` renders the shared value.
    let cross_config = "\
version: 2
relations:
  allowed:
    from: { paths: 'allowed.json' }
    entries: { format: json, at: $ }
    fields: { name: entry.key }
  used:
    from: { paths: 'used.json' }
    entries: { format: json, at: $ }
    fields: { name: entry.key }
rules:
  - id: exact
    kind: constraint
    input: used
    assert:
      set:
        field: name
        equal_to: { relation: allowed, field: name }
    message: 'row={{row.name}} value={{value}}'
    level: error
  - id: disjoint
    kind: constraint
    input: used
    assert:
      set:
        field: name
        disjoint_from: { relation: allowed, field: name }
    message: 'shared {{value}}'
    level: error
";
    let results = run(
        cross_config,
        &[
            ("allowed.json", r#"{"keep": 1, "missing_one": 1}"#),
            ("used.json", r#"{"keep": 1}"#),
        ],
    );
    let exact = findings(&results, "exact");
    assert_eq!(exact.len(), 1, "{exact:?}");
    assert!(
        exact[0].starts_with("used.json: row=<no row> value=missing_one"),
        "{exact:?}"
    );
    let disjoint = findings(&results, "disjoint");
    assert_eq!(disjoint.len(), 1, "{disjoint:?}");
    assert!(
        disjoint[0].starts_with("used.json:1:2: shared keep"),
        "{disjoint:?}"
    );

    // 3. `plurals: cldr` renders the merged plural group key path in `{{value}}`.
    let plural_config = "\
version: 2
relations:
  keys:
    from: { paths: 'locales/*.json' }
    entries: { format: json, at: $, leaves: true }
    fields: { locale: stem, key: entry.path }
rules:
  - id: plurals
    kind: constraint
    input: keys
    assert:
      set:
        field: key
        group_by: locale
        equal_to: { group: en }
        plurals: cldr
    message: 'plural key {{value}}'
    level: error
";
    let results = run(
        plural_config,
        &[
            (
                "locales/en.json",
                r#"{"inbox": {"one": "1", "other": "n"}}"#,
            ),
            ("locales/fr.json", "{}"),
        ],
    );
    let found = findings(&results, "plurals");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("locales/fr.json: plural key inbox"),
        "{found:?}"
    );
}

#[test]
fn set_message_value_redacts_sensitive_escapes_invisible_truncates_and_is_single_pass() {
    // 1. Sensitive target relation field redacts `{{value}}` to `<redacted>`.
    let sens_config = "\
version: 2
relations:
  secrets:
    from: { paths: 'secrets.json' }
    entries: { format: json, at: $ }
    fields:
      token: { source: entry.value, sensitive: true }
  used:
    from: { paths: 'used.json' }
    entries: { format: json, at: $ }
    fields:
      token: entry.value
rules:
  - id: no-secret
    kind: constraint
    input: used
    assert:
      set:
        field: token
        disjoint_from: { relation: secrets, field: token }
    message: 'leaked {{value}}'
    level: error
";
    let results = run(
        sens_config,
        &[
            ("secrets.json", r#"{"a": "super-secret-value"}"#),
            ("used.json", r#"{"b": "super-secret-value"}"#),
        ],
    );
    let found = findings(&results, "no-secret");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("leaked <redacted>"), "{found:?}");
    assert!(!found[0].contains("super-secret-value"), "{found:?}");

    // 2. Single-pass injection defense + invisible-char escaping + 64-char truncation.
    let long_val = "z".repeat(80);
    let config = "\
version: 2
relations:
  items:
    from: { paths: 'items.json' }
    entries: { format: json, at: ['$.ref', '$.peer'] }
    fields:
      group: entry.parent
      val: entry.value
      secret: { source: 'bound: $.secret', sensitive: true }
rules:
  - id: check-val
    kind: constraint
    input: items
    assert:
      set:
        field: val
        group_by: group
        equal_to: { group: ref }
    message: 'got={{value}}'
    level: error
";
    let body = format!(
        "{{\"secret\": \"TOPSECRET\", \"ref\": {{\"a\": \"ok\"}}, \"peer\": {{\"a\": \"ok\", \
         \"inj\": \"{{{{row.secret}}}} {{{{value}}}}\", \
         \"bidi\": \"bad\\n\\u202ehidden\", \
         \"long\": \"{long_val}\"}}}}"
    );
    let results = run(config, &[("items.json", &body)]);
    let found = findings(&results, "check-val");
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(
        found
            .iter()
            .any(|f| f.contains("got={{row.secret}} {{value}}")),
        "single-pass must keep literal placeholder text unexpanded: {found:?}"
    );
    assert!(
        found.iter().all(|f| !f.contains("TOPSECRET")),
        "secret must never leak: {found:?}"
    );
    assert!(
        found.iter().any(|f| f.contains(r"got=bad\n\u{202e}hidden")),
        "newline and bidi control must be escaped: {found:?}"
    );
    let expected_trunc = format!("got={}... (80 bytes)", "z".repeat(64));
    assert!(
        found.iter().any(|f| f.contains(&expected_trunc)),
        "values > 64 chars must truncate: {found:?}"
    );
}

// ---------------------------------------------------------------------------
// #595: Target-file placement for missing-value `set` findings
// ---------------------------------------------------------------------------

#[test]
fn set_missing_places_at_unique_structural_counterpart_and_falls_back_on_ambiguity_or_absence() {
    // 1. Multi-file locale dirs (`parent_name`), including `en/en.json` ambiguity immunity
    //    and case-folding (`transform: { locale: [lower] }`).
    let config = "\
version: 2
relations:
  keys:
    from: { paths: 'locales/*/*.json' }
    entries: { format: json, at: $, leaves: true }
    fields:
      locale: parent_name
      key: entry.path
    transform:
      locale: [lower]
rules:
  - id: i18n
    kind: constraint
    input: keys
    assert:
      set:
        field: key
        group_by: locale
        equal_to: { group: en }
    level: error
";
    let results = run(
        config,
        &[
            ("locales/EN/auth.json", r#"{"login": "Log in"}"#),
            ("locales/EN/en.json", r#"{"self_key": "Self"}"#),
            ("locales/EN/errors.json", r#"{"not_found": "Not found"}"#),
            (
                "locales/EN/only_en.json",
                r#"{"orphan_file_key": "Orphan"}"#,
            ),
            ("locales/fr/auth.json", r#"{"login": "Connexion"}"#),
            ("locales/fr/en.json", "{}"),
            ("locales/fr/errors.json", "{}"),
        ],
    );
    let found = findings(&results, "i18n");
    assert_eq!(found.len(), 3, "{found:?}");
    // Key from `locales/EN/en.json` -> `locales/fr/en.json` (not `auth.json`).
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("locales/fr/en.json: ") && f.contains("\"self_key\"")),
        "{found:?}"
    );
    // Key from `locales/EN/errors.json` -> `locales/fr/errors.json` (not `auth.json`).
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("locales/fr/errors.json: ") && f.contains("\"not_found\"")),
        "{found:?}"
    );
    // Key from `locales/EN/only_en.json` (no `locales/fr/only_en.json`) -> falls back
    // to `group.first` (`locales/fr/auth.json`).
    assert!(
        found
            .iter()
            .any(|f| f.starts_with("locales/fr/auth.json: ") && f.contains("\"orphan_file_key\"")),
        "{found:?}"
    );

    // Baseline keys remain path-free: adding `locales/fr/00_aaa.json` does NOT change
    // the baseline_key of `not_found`!
    let orig_key = result(&results, "i18n")
        .violations
        .iter()
        .find(|v| v.message.contains("\"not_found\""))
        .unwrap()
        .baseline_key
        .clone();
    let results_with_earlier = run(
        config,
        &[
            ("locales/EN/auth.json", r#"{"login": "Log in"}"#),
            ("locales/EN/errors.json", r#"{"not_found": "Not found"}"#),
            ("locales/fr/00_aaa.json", "{}"),
            ("locales/fr/auth.json", r#"{"login": "Connexion"}"#),
            ("locales/fr/errors.json", "{}"),
        ],
    );
    let new_v = result(&results_with_earlier, "i18n")
        .violations
        .iter()
        .find(|v| v.message.contains("\"not_found\""))
        .unwrap();
    assert_eq!(new_v.baseline_key, orig_key);
    assert_eq!(
        new_v
            .path
            .as_deref()
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/"),
        "locales/fr/errors.json"
    );
}

#[test]
fn set_missing_falls_back_on_ambiguous_counterpart_and_supports_ext_counterpart() {
    let config = "\
version: 2
relations:
  keys:
    from: { paths: 'locales/*/*.json' }
    entries: { format: json, at: $, leaves: true }
    fields:
      locale: parent_name
      key: entry.path
    transform:
      locale: [lower]
rules:
  - id: i18n
    kind: constraint
    input: keys
    assert:
      set:
        field: key
        group_by: locale
        equal_to: { group: en }
    level: error
";
    // Ambiguous counterpart (both `locales/FR/errors.json` and `locales/fr/errors.json`
    // collapse into group `fr`): falls back to `group.first` (`locales/FR/auth.json`).
    let ambig = run(
        config,
        &[
            ("locales/EN/auth.json", r#"{"login": "Log in"}"#),
            ("locales/EN/errors.json", r#"{"not_found": "Not found"}"#),
            ("locales/FR/auth.json", r#"{"login": "Connexion"}"#),
            ("locales/FR/errors.json", "{}"),
            ("locales/fr/errors.json", "{}"),
        ],
    );
    let ambig_found = findings(&ambig, "i18n");
    assert_eq!(ambig_found.len(), 1, "{ambig_found:?}");
    assert!(
        ambig_found[0].starts_with("locales/FR/auth.json: "),
        "ambiguous counterparts must fall back to group.first: {ambig_found:?}"
    );

    // 3. `ext` token counterpart (`schemas/user.json` -> `schemas/user.yaml`, not `a.yaml`).
    let ext_config = "\
version: 2
relations:
  fields:
    union:
      - { from: { paths: 'schemas/*.json' }, entries: { format: json, at: $ } }
      - { from: { paths: 'schemas/*.yaml' }, entries: { format: yaml, at: $ } }
    fields:
      format: ext
      field: entry.key
rules:
  - id: schema-parity
    kind: constraint
    input: fields
    assert:
      set:
        field: field
        group_by: format
        equal_to: { group: json }
    level: error
";
    let ext_res = run(
        ext_config,
        &[
            ("schemas/alpha.json", r#"{"a": 1}"#),
            ("schemas/user.json", r#"{"email": 1}"#),
            ("schemas/alpha.yaml", "a: 1\n"),
            ("schemas/user.yaml", "{}\n"),
        ],
    );
    let ext_found = findings(&ext_res, "schema-parity");
    assert_eq!(ext_found.len(), 1, "{ext_found:?}");
    assert!(
        ext_found[0].starts_with("schemas/user.yaml: "),
        "{ext_found:?}"
    );
}

// ---------------------------------------------------------------------------
// #598: Stub locale root naming its group under `group_by: entry.root`
// ---------------------------------------------------------------------------

#[test]
fn strip_root_stub_locale_files_form_zero_row_groups_under_entry_root() {
    let config = "\
version: 2
relations:
  keys:
    from: { paths: 'locales/*.yml' }
    entries: { format: yaml, at: $, leaves: true }
    fields:
      locale: entry.root
      key: entry.path
    transform:
      key: [strip-root]
      locale: [lower]
    where:
      locale: { not_in: [test] }
      key: { not_matches: '^_' }
rules:
  - id: i18n
    kind: constraint
    input: keys
    assert:
      set:
        field: key
        group_by: locale
        equal_to: { group: en }
    level: error
";
    let results = run(
        config,
        &[
            ("locales/en.yml", "en:\n  home: Home\n  _internal: skip\n"),
            ("locales/fr.yml", "fr:\n"),
            ("locales/de.yml", "de: {}\n"),
            ("locales/es.yml", "es: []\n"),
            ("locales/pt_upper.yml", "PT: {}\n"),
            ("locales/test.yml", "test: {}\n"),
            ("locales/empty.yml", "{}\n"),
        ],
    );
    let found = findings(&results, "i18n");
    // `fr`, `de`, `es`, `pt` (lowercased from `PT`) each lack `home`, anchored
    // at their own stub file; `test` was filtered by `where.locale`, `empty.yml`
    // has no root key, and `_internal` was filtered by `where.key`.
    assert_eq!(found.len(), 4, "{found:?}");
    for (file, locale) in [
        ("locales/fr.yml", "fr"),
        ("locales/de.yml", "de"),
        ("locales/es.yml", "es"),
        ("locales/pt_upper.yml", "pt"),
    ] {
        assert!(
            found.iter().any(|f| {
                f.starts_with(&format!(
                    "{file}: `locale` \"{locale}\" lacks `key` \"home\""
                ))
            }),
            "expected stub group {locale} at {file}: {found:?}"
        );
    }
}

#[test]
fn strip_root_stub_locale_files_interact_with_groups_and_reject_multi_root_stubs() {
    // With explicit `groups: [fr, ja]`, `fr` anchors at `locales/fr.yml` while
    // absent `ja` has no file path (`Place::Nowhere`).
    let with_groups = "\
version: 2
relations:
  keys:
    from: { paths: 'locales/*.yml' }
    entries: { format: yaml, at: $, leaves: true }
    fields:
      locale: entry.root
      key: entry.path
    transform:
      key: [strip-root]
rules:
  - id: i18n
    kind: constraint
    input: keys
    assert:
      set:
        field: key
        group_by: locale
        equal_to: { group: en }
        groups: [fr, ja]
    level: error
";
    let res = run(
        with_groups,
        &[
            ("locales/en.yml", "en:\n  home: Home\n"),
            ("locales/fr.yml", "fr: {}\n"),
        ],
    );
    let vs = &result(&res, "i18n").violations;
    assert_eq!(vs.len(), 2, "{vs:?}");
    let fr_v = vs.iter().find(|v| v.message.contains("\"fr\"")).unwrap();
    let ja_v = vs.iter().find(|v| v.message.contains("\"ja\"")).unwrap();
    assert_eq!(
        fr_v.path
            .as_deref()
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/"),
        "locales/fr.yml"
    );
    assert_eq!(ja_v.path, None);

    // Multi-root stub file (`fr: {}\nde: {}`) STILL fails loud per ADR-0037 decision 2!
    let multi = run(
        with_groups,
        &[
            ("locales/en.yml", "en:\n  home: Home\n"),
            ("locales/bad.yml", "fr: {}\nde: {}\n"),
        ],
    );
    let errs = failures(&multi, "i18n");
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("locales/bad.yml"), "{errs:?}");
    assert!(errs[0].contains("`fr`, `de`"), "{errs:?}");
}
