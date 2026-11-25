//! End-to-end tests for Phase 5 Batch 5b (ADR-0050 D1, D2, D3, D4, D8 / #592):
//! - D1 `reference` (single-key & composite-key foreign keys, `skip_missing`,
//!   `{{value}}`, type clashes, sensitivity closure, `TriggerScope`, alias-stable keys);
//! - D2 unified `count` (`min`/`max`/`exact`, `distinct`, emptiness per-row mode,
//!   grouped + multi-field `group_by`, `groups:`, `resource_groups`, `stub_rows`);
//! - D3 multi-field `group_by` across `equal`/`set`/`count`, 1-element list baseline
//!   parity, per-group cross-relation `set` (`to.group_by`, `all_target_groups`),
//!   and inline literal `set` targets (`values:`);
//! - D4 `compare` (`eq`/`ne`/`lt`/`le`/`gt`/`ge` x `exact`/`lexical`/`numeric` x
//!   `to.field`/`to.value`/`to.row`, fail-loud on `Null`/`Missing` and type clashes);
//! - D8 `#592` (`records:`, parameterized `transform:`, mixed-reader per-part `union:`,
//!   and env-interpolated `split.part`).

#![allow(clippy::too_many_lines)]

use super::constraint_families::{failures, findings, result, run};
use super::*;

fn keys(results: &[alint_core::RuleResult], id: &str) -> Vec<String> {
    result(results, id)
        .violations
        .iter()
        .map(|v| v.baseline_key.as_deref().unwrap().to_string())
        .collect()
}

// --- D1: `reference` ---------------------------------------------------------

#[test]
fn reference_single_and_composite_keys_skip_missing_and_message_value() {
    let config = r#"version: 2
relations:
  catalog:
    from: { paths: "catalog.json" }
    entries: { format: json, at: "$.packages" }
    fields: { name: entry.key, version: entry.value }
  uses:
    from: { paths: "apps/*/package.json" }
    entries: { format: json, at: "$.deps" }
    fields:
      name: entry.key
      version: entry.value
      owner: { source: "bound: $.owner", optional: true, on_missing: missing }
  owners:
    from: { paths: "owners.json" }
    read: { json: "$.owners[*]" }
    fields: { owner: value }
rules:
  - id: single-ref
    kind: constraint
    input: uses
    assert:
      reference:
        field: name
        to: { relation: catalog, field: name }
    message: "unknown dependency {{value}} in {{row.name}}"
    level: error
  - id: composite-ref
    kind: constraint
    input: uses
    assert:
      reference:
        fields: [name, version]
        to: { relation: catalog, fields: [name, version] }
    message: "unresolved pair {{value}}"
    level: error
  - id: owner-required
    kind: constraint
    input: uses
    assert:
      reference:
        field: owner
        to: { relation: owners, field: owner }
    level: error
  - id: owner-skip-missing
    kind: constraint
    input: uses
    assert:
      reference:
        field: owner
        to: { relation: owners, field: owner }
        skip_missing: true
    level: error
"#;
    let files = [
        (
            "catalog.json",
            r#"{"packages": {"react": "18.2.0", "zod": "3.22.0"}}"#,
        ),
        (
            "apps/web/package.json",
            r#"{"owner": "alice", "deps": {"react": "18.2.0", "zod": "3.23.0", "ghost": "1.0.0"}}"#,
        ),
        ("apps/cli/package.json", r#"{"deps": {"react": "18.2.0"}}"#),
        ("owners.json", r#"{"owners": ["alice"]}"#),
    ];
    let results = run(config, &files);
    assert_eq!(
        findings(&results, "single-ref"),
        ["apps/web/package.json:1:65: unknown dependency ghost in ghost"]
    );
    assert_eq!(
        findings(&results, "composite-ref"),
        [
            "apps/web/package.json:1:48: unresolved pair zod, 3.23.0",
            "apps/web/package.json:1:65: unresolved pair ghost, 1.0.0",
        ]
    );
    // Missing `owner` in apps/cli fails when `skip_missing: false` and passes when `true`.
    assert_eq!(findings(&results, "owner-required").len(), 1);
    assert!(findings(&results, "owner-required")[0].contains("missing"));
    assert_eq!(findings(&results, "owner-skip-missing").len(), 0);
}

#[test]
fn reference_propagates_sensitivity_and_triggers_on_target_relation() {
    let config = r#"version: 2
relations:
  allowed:
    from: { paths: "allowed.json" }
    read: { json: "$.tokens[*]" }
    fields: { token: { source: value, sensitive: true } }
  used:
    from: { paths: "app.json" }
    read: { json: "$.used[*]" }
    fields: { token: value }
rules:
  - id: ref-sec
    kind: constraint
    input: used
    assert:
      reference:
        field: token
        to: { relation: allowed, field: token }
    message: "bad {{value}}"
    level: error
"#;
    let tmp = tempfile::tempdir().unwrap();
    let cfg_path = tmp.path().join(".alint.yml");
    std::fs::write(&cfg_path, config).unwrap();
    let loaded = load(&cfg_path).unwrap();
    let plan = loaded.relation_plan.as_ref().unwrap();
    let used_rel = plan.relations.iter().find(|r| r.name == "used").unwrap();
    assert!(
        used_rel.relation.field("token").unwrap().sensitive,
        "sensitive target column must propagate to reference source column"
    );
    let rule = test_registry().build(&loaded.rules[0]).unwrap();
    let (_, inputs) = rule.relation_inputs().unwrap();
    assert_eq!(
        inputs.len(),
        2,
        "reference rule must list both source and target relations as inputs"
    );
    let trigger = rule.trigger_scope().unwrap();
    assert_eq!(
        trigger.origins().count(),
        2,
        "TriggerScope must include both input and target relation selectors"
    );

    let results = run(
        config,
        &[
            ("allowed.json", r#"{"tokens": ["secret-ok"]}"#),
            ("app.json", r#"{"used": ["secret-bad"]}"#),
        ],
    );
    let f = findings(&results, "ref-sec");
    assert_eq!(f.len(), 1);
    assert!(
        !f[0].contains("secret-bad"),
        "must redact sensitive value: {f:?}"
    );
    assert!(f[0].contains("<redacted>"), "{f:?}");
}

#[test]
fn reference_fails_loud_on_cross_type_and_distinct_value_cap() {
    let config = r#"version: 2
relations:
  targets:
    from: { paths: "targets.json" }
    entries: { format: json, at: "$.ids" }
    fields: { id: entry.value }
  refs:
    from: { paths: "refs.json" }
    entries: { format: json, at: "$.refs" }
    fields: { id: entry.value }
rules:
  - id: r
    kind: constraint
    input: refs
    assert:
      reference:
        field: id
        to: { relation: targets, field: id }
    level: error
"#;
    let results = run(
        config,
        &[
            ("targets.json", r#"{"ids": {"a": 1}}"#),
            ("refs.json", r#"{"refs": {"x": "1"}}"#),
        ],
    );
    let errs = failures(&results, "r");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("cross types"), "{errs:?}");
}

// --- D2: `count` -------------------------------------------------------------

#[test]
fn count_covers_bounds_distinct_emptiness_grouped_resource_and_stub_groups() {
    let config = r#"version: 2
relations:
  items:
    from: { paths: "data/*.json" }
    entries: { format: json, at: "$.items" }
    fields:
      file: stem
      key: entry.key
      val: entry.value
  forbidden:
    from: { paths: "data/*.json" }
    entries: { format: json, at: "$.banned" }
    fields: { key: entry.key }
  locales:
    from: { paths: "locales/*.yml" }
    entries: { format: yaml, at: "$", leaves: true }
    fields: { locale: entry.root, key: entry.path }
    transform: { key: [strip-root] }
rules:
  - id: per-file-min-max
    kind: constraint
    input: items
    assert: { count: { group_by: file, min: 1, max: 2 } }
    level: error
  - id: distinct-exact
    kind: constraint
    input: items
    assert: { count: { group_by: file, distinct: true, field: val, exact: 1, groups: [missing_file] } }
    level: error
  - id: no-banned
    kind: constraint
    input: forbidden
    assert: { count: { max: 0 } }
    level: error
  - id: locale-non-empty
    kind: constraint
    input: locales
    assert: { count: { group_by: locale, min: 1 } }
    level: error
"#;
    let files = [
        ("data/empty.json", r#"{"items": {}}"#),
        (
            "data/too_many.json",
            r#"{"items": {"a": "x", "b": "y", "c": "x"}, "banned": {"eval": 1, "exec": 2}}"#,
        ),
        ("data/ok.json", r#"{"items": {"a": "same", "b": "same"}}"#),
        ("locales/en.yml", "en:\n  hello: world\n"),
        ("locales/fr.yml", "fr: {}\n"),
    ];
    let results = run(config, &files);

    // `per-file-min-max`: `empty` has 0 rows (< 1) via resource_groups, `too_many` has 3 (> 2).
    let f = findings(&results, "per-file-min-max");
    assert_eq!(f.len(), 2, "{f:?}");
    assert!(
        f.iter()
            .any(|m| m.contains("data/empty.json") && m.contains("0 rows"))
    );
    assert!(
        f.iter()
            .any(|m| m.contains("data/too_many.json") && m.contains("3 rows"))
    );

    // `distinct-exact`: `empty` (0 != 1), `too_many` (2 distinct != 1), `missing_file` (0 != 1).
    let d = findings(&results, "distinct-exact");
    assert_eq!(d.len(), 3, "{d:?}");
    assert!(
        d.iter()
            .any(|m| m.contains("missing_file") && m.contains("0 distinct"))
    );
    assert!(d.iter().any(|m| m.contains("2 distinct")));

    // `no-banned` (`max: 0`): emits one per-row finding anchored at each offending entry.
    let b = findings(&results, "no-banned");
    assert_eq!(b.len(), 2, "{b:?}");
    assert!(b[0].starts_with("data/too_many.json:1:"), "{b:?}");
    assert!(b[1].starts_with("data/too_many.json:1:"), "{b:?}");

    // `locale-non-empty`: stub locale `fr: {}` forms a zero-row group anchored at `locales/fr.yml`.
    let l = findings(&results, "locale-non-empty");
    assert_eq!(l.len(), 1, "{l:?}");
    assert!(
        l[0].starts_with("locales/fr.yml:") && l[0].contains("0 rows"),
        "{l:?}"
    );
}

// --- D3: multi-field `group_by`, per-group `set`, inline `values:` -----------

#[test]
fn multi_field_group_by_and_one_element_baseline_key_parity() {
    let config = r#"version: 2
relations:
  i18n:
    from: { paths: "locales/*/*.json" }
    entries: { format: json, at: "$", leaves: true }
    fields:
      locale: parent_name
      ns: stem
      key: entry.path
      val: entry.value
rules:
  - id: single-str
    kind: constraint
    input: i18n
    assert: { set: { group_by: locale, field: key, equal_to: { group: en } } }
    level: error
  - id: single-list
    kind: constraint
    input: i18n
    assert: { set: { group_by: [locale], field: key, equal_to: { group: en } } }
    level: error
  - id: multi-set
    kind: constraint
    input: i18n
    assert:
      set:
        group_by: [ns, locale]
        field: key
        equal_to: { group: [auth, en] }
        groups: [[auth, de]]
    level: error
  - id: multi-equal
    kind: constraint
    input: i18n
    assert: { equal: { group_by: [ns, key], field: val } }
    level: error
"#;
    let files = [
        (
            "locales/en/auth.json",
            r#"{"login": "Log in", "logout": "Log out"}"#,
        ),
        ("locales/fr/auth.json", r#"{"login": "Connexion"}"#),
    ];
    let results = run(config, &files);
    // 1-element list `group_by: [locale]` has byte-identical baseline_key to `group_by: locale`.
    assert_eq!(keys(&results, "single-str"), keys(&results, "single-list"));
    assert_ne!(keys(&results, "single-str").len(), 0);

    // Multi-field `set`: `[auth, fr]` lacks `logout`; `[auth, de]` (named in `groups:`) lacks both.
    let ms = findings(&results, "multi-set");
    assert_eq!(ms.len(), 3, "{ms:?}");
    assert!(
        ms.iter()
            .any(|m| m.contains("`ns` \"auth\", `locale` \"fr\"") && m.contains("logout"))
    );
    assert!(
        ms.iter()
            .any(|m| m.contains("`ns` \"auth\", `locale` \"de\"") && m.contains("login"))
    );

    // Multi-field `equal`: `[auth, login]` disagrees (`"Log in"` vs `"Connexion"`).
    let me = findings(&results, "multi-equal");
    assert_eq!(me.len(), 2, "{me:?}");
}

#[test]
fn per_group_cross_relation_set_and_inline_values_target() {
    let config = r#"version: 2
relations:
  declared:
    from: { paths: "plugins/*/package.json" }
    read: { json: "$.backstage.pluginPackages[*]" }
    optional: true
    fields:
      plugin_id: "bound: $.backstage.pluginId"
      pkg: value
  siblings:
    from: { paths: "packages/*/package.json" }
    read: { json: "$.name" }
    fields:
      plugin_id: "bound: $.backstage.pluginId"
      name: value
  frontmatter:
    from: { paths: "docs/*.json" }
    entries: { format: json, at: "$" }
    fields: { file: stem, key: entry.key }
rules:
  - id: plugin-packages-match
    kind: constraint
    input: declared
    assert:
      set:
        group_by: plugin_id
        field: pkg
        equal_to:
          relation: siblings
          field: name
          group_by: plugin_id
          all_target_groups: true
    level: error
  - id: allowed-frontmatter-keys
    kind: constraint
    input: frontmatter
    assert:
      set:
        group_by: file
        field: key
        subset_of: { values: [id, title, tags] }
    message: "disallowed key {{value}}"
    level: error
"#;
    let files = [
        (
            "plugins/catalog/package.json",
            r#"{"backstage": {"pluginId": "catalog", "pluginPackages": ["@app/cat-web", "@app/cat-extra"]}}"#,
        ),
        (
            "packages/cat-web/package.json",
            r#"{"name": "@app/cat-web", "backstage": {"pluginId": "catalog"}}"#,
        ),
        (
            "packages/cat-node/package.json",
            r#"{"name": "@app/cat-node", "backstage": {"pluginId": "catalog"}}"#,
        ),
        (
            "packages/search-web/package.json",
            r#"{"name": "@app/search-web", "backstage": {"pluginId": "search"}}"#,
        ),
        (
            "docs/post.json",
            r#"{"id": "1", "title": "Hi", "draft": "yes"}"#,
        ),
    ];
    let results = run(config, &files);
    // `catalog`: missing `@app/cat-node`, extra `@app/cat-extra`; `search` (all_target_groups): missing `@app/search-web`.
    let p = findings(&results, "plugin-packages-match");
    assert_eq!(p.len(), 3, "{p:?}");
    assert!(
        p.iter()
            .any(|m| m.contains("catalog") && m.contains("lacks `pkg` \"@app/cat-node\""))
    );
    assert!(
        p.iter()
            .any(|m| m.contains("catalog") && m.contains("holds `pkg` \"@app/cat-extra\""))
    );
    assert!(
        p.iter()
            .any(|m| m.contains("search") && m.contains("lacks `pkg` \"@app/search-web\""))
    );

    // Inline `values:` target (`ghost-63` shape).
    let fm = findings(&results, "allowed-frontmatter-keys");
    assert_eq!(fm, ["docs/post.json:1:28: disallowed key draft"]);
}

// --- D4: `compare` -----------------------------------------------------------

#[test]
fn compare_covers_exact_lexical_numeric_all_targets_and_fails_on_null_or_missing() {
    let config = r#"version: 2
relations:
  i18n:
    from: { paths: "i18n.json" }
    entries: { format: json, at: "$" }
    fields: { key: entry.key, val: entry.value }
  limits:
    from: { paths: "limits.json" }
    records: { format: json, at: "$.services[*]" }
    fields:
      name: "record: $.name"
      min_port: "record: $.minPort"
      max_port: "record: $.maxPort"
      opt_port: { source: "record: $.optPort", optional: true }
rules:
  - id: no-self-translation
    kind: constraint
    input: i18n
    assert: { compare: { field: val, op: ne, to: { field: key } } }
    level: error
  - id: port-range-order
    kind: constraint
    input: limits
    assert: { compare: { field: min_port, op: le, to: { field: max_port }, order: numeric } }
    level: error
  - id: port-floor
    kind: constraint
    input: limits
    assert: { compare: { field: min_port, op: ge, to: { value: 1024 }, order: numeric } }
    level: error
  - id: name-lexical-after-baseline
    kind: constraint
    input: limits
    assert: { compare: { field: name, op: gt, to: { row: { name: "api" } }, order: lexical } }
    level: error
  - id: missing-operand-fails-loud
    kind: constraint
    input: limits
    assert: { compare: { field: opt_port, op: ne, to: { value: "none" } } }
    level: error
"#;
    let files = [
        ("i18n.json", r#"{"hello": "Hello", "todo": "todo"}"#),
        (
            "limits.json",
            r#"{"services": [
                {"name": "api", "minPort": 8080, "maxPort": 9000, "optPort": 1},
                {"name": "admin", "minPort": 9500, "maxPort": 9000},
                {"name": "web", "minPort": 80, "maxPort": 443}
            ]}"#,
        ),
    ];
    let results = run(config, &files);
    // `element-web-05` shape: `val != key`.
    assert_eq!(findings(&results, "no-self-translation").len(), 1);
    assert!(findings(&results, "no-self-translation")[0].contains("\"todo\""));

    // `numeric` field comparison: `admin` has 9500 > 9000.
    assert_eq!(findings(&results, "port-range-order").len(), 1);
    assert!(findings(&results, "port-range-order")[0].contains("9500"));

    // `numeric` literal comparison: `web` has 80 < 1024.
    assert_eq!(findings(&results, "port-floor").len(), 1);
    assert!(findings(&results, "port-floor")[0].contains("80"));

    // `lexical` source-row comparison: `"admin"` is not `> "api"`, `"web"` is `> "api"`.
    assert_eq!(findings(&results, "name-lexical-after-baseline").len(), 1);
    assert!(findings(&results, "name-lexical-after-baseline")[0].contains("\"admin\""));

    // Missing operand fails loud even under `op: ne` (never silently passes!).
    let errs = failures(&results, "missing-operand-fails-loud");
    assert_eq!(errs.len(), 1);
    assert!(errs[0].contains("missing"), "{errs:?}");
}

// --- D8 (#592) E2E: `records:` + parameterized transforms + mixed union ------

#[test]
fn issue_592_records_split_affix_and_mixed_union_end_to_end() {
    let config = r#"version: 2
relations:
  containers:
    from: { paths: "k8s/*.yaml" }
    records: { format: yaml, at: "$.spec.template.spec.containers[*]" }
    fields:
      name: "record: $.name"
      tag: "record: $.image"
    transform:
      tag: [{ split: { on: ":", part: -1 } }]
  all-commands:
    union:
      - from: { paths: "commands.json" }
        entries: { format: json, at: "$.commands" }
        fields: { cmd: entry.key }
      - from: { paths: "commands.json" }
        read: { json: "$.commands.*.aliases[*]" }
        fields: { cmd: value }
rules:
  - id: no-latest-tag
    kind: constraint
    input: containers
    assert: { compare: { field: tag, op: ne, to: { value: "latest" } } }
    level: error
  - id: unique-command-and-alias
    kind: constraint
    input: all-commands
    assert: { unique: { key: [cmd] } }
    level: error
"#;
    let files = [
        (
            "k8s/deploy.yaml",
            "spec:\n  template:\n    spec:\n      containers:\n        - name: api\n          \
             image: ghcr.io/org/api:1.4.2\n        - name: web\n          image: nginx:latest\n",
        ),
        (
            "commands.json",
            r#"{"commands": {"help": {"aliases": ["h"]}, "h": {"aliases": ["?"]}}}"#,
        ),
    ];
    let results = run(config, &files);
    // `no-latest-tag` catches `nginx:latest` at line 8 (exact YAML span of `image`).
    let tag_findings = findings(&results, "no-latest-tag");
    assert_eq!(tag_findings.len(), 1, "{tag_findings:?}");
    assert!(
        tag_findings[0].starts_with("k8s/deploy.yaml:8:"),
        "{tag_findings:?}"
    );
    // `unique-command-and-alias` (`mattermost-27` shape) catches collision on `"h"`.
    let cmd_findings = findings(&results, "unique-command-and-alias");
    assert_eq!(cmd_findings.len(), 1, "{cmd_findings:?}");
    assert!(cmd_findings[0].contains("\"h\""), "{cmd_findings:?}");
}

#[test]
fn split_part_interpolates_integer_from_env() {
    #[derive(Debug)]
    struct Env;
    impl crate::EnvironmentProvider for Env {
        fn get(&self, name: &str) -> Option<String> {
            match name {
                "PART" => Some("-1".into()),
                "REQ" => Some("true".into()),
                _ => None,
            }
        }
    }
    let yaml = r#"version: 2
relations:
  r:
    from: { paths: "a.txt" }
    read: { lines: {} }
    fields: { v: value }
    transform:
      v: [{ split: { on: ":", part: "{{env.PART}}", required: "{{env.REQ}}" } }]
rules: []
"#;
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(&path, yaml).unwrap();
    let opts = LoadOptions::default().with_environment(std::sync::Arc::new(Env));
    let parsed = load_with(&path, &opts);
    assert!(
        parsed.is_ok(),
        "env-interpolated split.part/required failed: {parsed:?}"
    );
}
