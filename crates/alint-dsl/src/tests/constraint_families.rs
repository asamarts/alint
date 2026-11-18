//! `constraint` rules run over trees by the engine (design
//! `v0.23/selectors-and-constraints.md` section 6): each family's findings
//! and related locations, comparison and nulls, every analysis failure, the
//! messages and their redaction, and the path-free identity. The loader and
//! the builder's refusals are tested in `constraint.rs`.

use std::fmt::Write as _;

use alint_core::{FileIndex, RuleResult, Violation, WalkOptions, walk};

use super::constraint::load_tree;

/// The results of running every rule of `config` over `files`.
fn run(config: &str, files: &[(&str, &str)]) -> Vec<RuleResult> {
    let (dir, loaded) = load_tree(config, files);
    let config = loaded.unwrap_or_else(|error| panic!("the config loads: {error}"));
    let index: FileIndex = walk(
        dir.path(),
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    let mut registry = alint_core::RuleRegistry::new();
    alint_rules::register_builtin(&mut registry);
    let entries = config
        .rules
        .iter()
        .map(|spec| {
            alint_core::RuleEntry::new(
                registry
                    .build(spec)
                    .unwrap_or_else(|error| panic!("{} builds: {error}", spec.id)),
            )
        })
        .collect();
    let engine = alint_core::Engine::from_entries(entries, registry);
    engine.run(dir.path(), &index).unwrap().results
}

/// Where a finding or related location sits: `path:line:column`, or the
/// path alone.
fn place(path: &std::path::Path, range: Option<&alint_core::SourceRange>) -> String {
    let path = path.display().to_string().replace('\\', "/");
    match range {
        Some(range) => format!("{path}:{}:{}", range.start_line(), range.start_column()),
        None => path,
    }
}

/// A finding as `place: message`, each related location on a line of its
/// own (`  ~ label: place`).
fn shown(violation: &Violation) -> String {
    let mut out = match &violation.path {
        Some(path) => format!(
            "{}: {}",
            place(path, violation.range.as_ref()),
            violation.message
        ),
        None => violation.message.to_string(),
    };
    for related in &violation.related_locations {
        let at = place(&related.path, related.range.as_ref());
        write!(out, "\n  ~ {}: {at}", related.label).unwrap();
    }
    if violation.related_locations_omitted > 0 {
        write!(
            out,
            "\n  ~ and {} more",
            violation.related_locations_omitted
        )
        .unwrap();
    }
    out
}

/// Rule `id`'s result.
fn result<'r>(results: &'r [RuleResult], id: &str) -> &'r RuleResult {
    results
        .iter()
        .find(|result| &*result.rule_id == id)
        .unwrap_or_else(|| panic!("no result for {id}"))
}

/// Rule `id`'s findings, shown.
fn findings(results: &[RuleResult], id: &str) -> Vec<String> {
    let result = result(results, id);
    assert!(
        result.analysis_failures.is_empty(),
        "{id} failed: {:?}",
        result.analysis_failures
    );
    result.violations.iter().map(shown).collect()
}

/// Rule `id`'s analysis failures, as `path: message`; it has no finding.
fn failures(results: &[RuleResult], id: &str) -> Vec<String> {
    let result = result(results, id);
    assert!(
        result.violations.is_empty(),
        "{id} has findings beside its failures: {:?}",
        result.violations
    );
    result
        .analysis_failures
        .iter()
        .map(|failure| match &failure.path {
            Some(path) => format!("{}: {}", place(path, None), failure.message),
            None => failure.message.to_string(),
        })
        .collect()
}

/// A version 2 config: relation `r` read from `owners.json`'s entries at
/// `at`, with `fields`, and rule `c` asserting `assert`.
fn entries(at: &str, fields: &str, assert: &str) -> String {
    format!(
        "version: 2\nrelations:\n  r:\n    from: {{ paths: \"**/owners.json\" }}\n    entries: \
         {{ format: json, at: {at} }}\n    fields: {fields}\nrules:\n  - id: c\n    kind: \
         constraint\n    input: r\n    assert: {assert}\n    level: error\n"
    )
}

const OWNERS: &str = "{ name: entry.key, owner: entry.value }";

// --- `unique` -------------------------------------------------------------

#[test]
fn unique_reports_each_row_that_repeats_a_key_and_no_null_key() {
    let results = run(
        &entries("\"$.owners\"", OWNERS, "{ unique: { key: [owner] } }"),
        &[(
            "owners.json",
            "{\"owners\": {\n\"a\": \"x\",\n\"b\": \"x\",\n\"c\": \"x\",\n\"d\": null,\n\"e\": \
             null,\n\"f\": \"y\"}}\n",
        )],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "owners.json:3:6: `owner` \"x\" is not unique: the first row with it is `owners.a` \
             in owners.json\n  ~ the first row with this key: owners.json:2:6",
            "owners.json:4:6: `owner` \"x\" is not unique: the first row with it is `owners.a` \
             in owners.json\n  ~ the first row with this key: owners.json:2:6",
        ]
    );
}

#[test]
fn a_key_of_several_fields_repeats_only_as_a_whole() {
    let config = entries(
        "[\"$.dev\", \"$.prod\"]",
        "{ env: entry.parent, name: entry.key, owner: entry.value }",
        "{ unique: { key: [env, owner] } }",
    );
    let results = run(
        &config,
        &[(
            "owners.json",
            "{\"dev\": {\"a\": \"x\", \"b\": \"y\"},\n\"prod\": {\"a\": \"x\", \"b\": \"x\"}}\n",
        )],
    );
    assert_eq!(
        findings(&results, "c"),
        // A key sits at its first field, `entry.parent`, which is at the entry's key.
        [
            "owners.json:2:20: `env` \"prod\", `owner` \"x\" is not unique: the first row with it \
          is `prod.a` in owners.json\n  ~ the first row with this key: owners.json:2:10"
        ]
    );
}

#[test]
fn determines_gives_one_finding_per_value_of_a_key_whose_rows_disagree() {
    let config = "version: 2\nrelations:\n  deps:\n    from: { paths: \"*/package.json\" }\n    \
                  entries: { format: json, at: \"$.dependencies\" }\n    fields: { dependency: \
                  entry.key, range: entry.value }\nrules:\n  - id: c\n    kind: constraint\n    \
                  input: deps\n    assert: { unique: { key: [dependency], determines: [range] } \
                  }\n    level: error\n";
    let manifest = |range: &str| format!("{{\"dependencies\": {{\"react\": {range}}}}}\n");
    let results = run(
        config,
        &[
            ("a/package.json", &manifest("\"^18\"")),
            ("b/package.json", &manifest("\"^17\"")),
            ("c/package.json", &manifest("\"^18\"")),
            ("d/package.json", &manifest("null")),
        ],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "a/package.json:1:28: rows with `dependency` \"react\" disagree on `range`: \"^18\" \
             here and in 1 other row; \"^17\" in 1 row; null in 1 row\n  ~ a row with another \
             value: b/package.json:1:28\n  ~ another row with this value: c/package.json:1:28\n  \
             ~ a row with another value: d/package.json:1:28",
            "b/package.json:1:28: rows with `dependency` \"react\" disagree on `range`: \"^17\" \
             here; \"^18\" in 2 rows; null in 1 row\n  ~ a row with another value: \
             a/package.json:1:28\n  ~ a row with another value: d/package.json:1:28",
            "d/package.json:1:28: rows with `dependency` \"react\" disagree on `range`: null \
             here; \"^18\" in 2 rows; \"^17\" in 1 row (null matches nothing, so a row without a \
             value agrees with no row)\n  ~ a row with another value: a/package.json:1:28\n  ~ a \
             row with another value: b/package.json:1:28",
        ]
    );
    // Two rows that agree only on null still disagree: null matches nothing.
    let results = run(
        config,
        &[
            ("a/package.json", &manifest("null")),
            ("b/package.json", &manifest("null")),
        ],
    );
    assert_eq!(findings(&results, "c").len(), 1);
}

#[test]
fn a_finding_lists_twenty_related_locations_and_counts_the_rest() {
    let mut body = String::from("{\"owners\": {");
    for index in 0..25 {
        write!(body, "\"k{index:02}\": \"x\", ").unwrap();
    }
    body.push_str("\"z\": \"y\"}, \"pinned\": {\"k00\": \"x\"}}");
    let config = entries("[\"$.owners\"]", OWNERS, "{ equal: { field: owner } }");
    let results = run(&config, &[("owners.json", &body)]);
    let result = result(&results, "c");
    assert_eq!(result.violations.len(), 2);
    let first = &result.violations[0];
    // 24 other rows with `x` and one row of `y`: 20 listed, 5 counted.
    assert_eq!(first.related_locations.len(), 20);
    assert_eq!(first.related_locations_omitted, 5);
}

// --- `equal` within groups ---------------------------------------------------

#[test]
fn equal_without_group_by_compares_every_row() {
    let config = entries("\"$.owners\"", OWNERS, "{ equal: { field: owner } }");
    let agreeing = run(
        &config,
        &[("owners.json", "{\"owners\": {\"a\": \"x\", \"b\": \"x\"}}")],
    );
    assert_eq!(findings(&agreeing, "c"), [] as [String; 0]);
    let results = run(
        &config,
        &[("owners.json", "{\"owners\": {\"a\": \"x\", \"b\": \"y\"}}")],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "owners.json:1:18: rows disagree on `owner`: \"x\" here; \"y\" in 1 row\n  ~ a row \
             with another value: owners.json:1:28",
            "owners.json:1:28: rows disagree on `owner`: \"y\" here; \"x\" in 1 row\n  ~ a row \
             with another value: owners.json:1:18",
        ]
    );
}

#[test]
fn equal_compares_within_each_group() {
    let config = entries(
        "[\"$.dev\", \"$.prod\"]",
        "{ env: entry.parent, name: entry.key, owner: entry.value }",
        "{ equal: { field: owner, group_by: env } }",
    );
    let results = run(
        &config,
        &[(
            "owners.json",
            "{\"dev\": {\"a\": \"x\", \"b\": \"x\"},\n\"prod\": {\"a\": \"y\", \"b\": \"z\"}}",
        )],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "owners.json:2:15: rows with `env` \"prod\" disagree on `owner`: \"y\" here; \"z\" in \
             1 row\n  ~ a row with another value: owners.json:2:25",
            "owners.json:2:25: rows with `env` \"prod\" disagree on `owner`: \"z\" here; \"y\" in \
             1 row\n  ~ a row with another value: owners.json:2:15",
        ]
    );
}

#[test]
fn values_of_two_types_differ_and_the_message_shows_both_types() {
    let config = entries("\"$.owners\"", OWNERS, "{ equal: { field: owner } }");
    let results = run(
        &config,
        &[("owners.json", "{\"owners\": {\"a\": 1, \"b\": \"1\"}}")],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "owners.json:1:18: rows disagree on `owner`: the number 1 here; the string \"1\" in 1 \
             row\n  ~ a row with another value: owners.json:1:26",
            "owners.json:1:26: rows disagree on `owner`: the string \"1\" here; the number 1 in 1 \
             row\n  ~ a row with another value: owners.json:1:18",
        ]
    );
    // A number compares by its lexeme: `1.0` is not `1`.
    let results = run(
        &config,
        &[("owners.json", "{\"owners\": {\"a\": 1, \"b\": 1.0}}")],
    );
    assert_eq!(findings(&results, "c").len(), 2);
}

#[test]
fn a_null_group_and_a_group_of_another_type_are_analysis_failures() {
    let config = entries(
        "\"$.owners\"",
        "{ name: entry.key, owner: entry.value }",
        "{ equal: { field: name, group_by: owner } }",
    );
    let results = run(
        &config,
        &[("owners.json", "{\"owners\": {\"a\": \"x\", \"b\": null}}")],
    );
    assert_eq!(
        failures(&results, "c"),
        [
            "owners.json: relation `r` (.alint.yml): `assert.equal.group_by`: `owner` has no value \
          at `owners.b` in owners.json; every row's group must have one"
        ]
    );
    let results = run(
        &config,
        &[("owners.json", "{\"owners\": {\"a\": \"x\", \"b\": true}}")],
    );
    assert_eq!(
        failures(&results, "c"),
        [
            "owners.json: relation `r` (.alint.yml): `assert.equal.group_by`: `owner` holds a \
          string at `owners.a` in owners.json and a boolean at `owners.b` in owners.json; the \
          comparison would cross types, so the assertion has no verdict"
        ]
    );
}

#[test]
fn a_cross_type_key_is_an_analysis_failure_naming_both_rows() {
    let config = entries("\"$.owners\"", OWNERS, "{ unique: { key: [owner] } }");
    let results = run(
        &config,
        &[(
            "owners.json",
            "{\"owners\": {\"a\": \"x\", \"b\": 1, \"c\": \"x\"}}",
        )],
    );
    assert_eq!(
        failures(&results, "c"),
        [
            "owners.json: relation `r` (.alint.yml): `assert.unique.key`: `owner` holds a string \
          at `owners.a` in owners.json and a number at `owners.b` in owners.json; the comparison \
          would cross types, so the assertion has no verdict"
        ]
    );
}

#[test]
fn a_list_or_a_record_never_reaches_an_assertion() {
    // The relation refuses a field that would hold one: the rule reports the
    // relation's failure.
    let config = entries("\"$.owners\"", OWNERS, "{ unique: { key: [owner] } }");
    let results = run(
        &config,
        &[("owners.json", "{\"owners\": {\"a\": [\"x\"]}}")],
    );
    assert_eq!(
        failures(&results, "c"),
        [
            "owners.json: relation `r` (.alint.yml): `owner` binds `entry.value`, which is an array \
          at `owners.a`; a field holds a scalar (null, a boolean, a number or a string), so \
          select the members below it with `at:`, or bind only `entry.key`"
        ]
    );
}

// --- `equal` against a source row, and within a row --------------------------

const PINS: &str = "version: 2\nrelations:\n  pins:\n    from: { paths: \"**/pins.json\" }\n    \
                    entries: { format: json, at: \"$\" }\n    fields: { tool: entry.key, version: \
                    entry.value, file: path }\nrules:\n  - id: c\n    kind: constraint\n    input: \
                    pins\n    assert: { equal: { field: version, to: { row: { file: ROW } } } }\n    \
                    level: error\n";

#[test]
fn equal_to_a_source_row_reports_each_row_that_differs() {
    let config = PINS.replace("ROW", "pins.json");
    let results = run(
        &config,
        &[
            ("pins.json", "{\"node\": \"20\"}"),
            ("a/pins.json", "{\"node\": \"20\"}"),
            ("b/pins.json", "{\"node\": \"18\"}"),
            ("c/pins.json", "{\"node\": null}"),
        ],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "b/pins.json:1:10: `version` is \"18\" here, but \"20\" in the source row (`file` \
             \"pins.json\", at `node` in pins.json)\n  ~ the source row: pins.json:1:10",
            "c/pins.json:1:10: `version` is null here, but \"20\" in the source row (`file` \
             \"pins.json\", at `node` in pins.json)\n  ~ the source row: pins.json:1:10",
        ]
    );
}

#[test]
fn a_source_row_must_exist_once_and_hold_a_value() {
    let config = PINS.replace("ROW", "pins.json");
    let none = run(&config, &[("a/pins.json", "{\"node\": \"20\"}")]);
    assert_eq!(
        failures(&none, "c"),
        [
            "relation `pins` (.alint.yml): `assert.equal.to.row`: no row has `file` \"pins.json\"; \
          the source row must exist"
        ]
    );
    let two = run(
        &config,
        &[("pins.json", "{\"node\": \"20\", \"npm\": \"10\"}")],
    );
    assert_eq!(
        failures(&two, "c"),
        [
            "pins.json: relation `pins` (.alint.yml): `assert.equal.to.row`: 2 rows have `file` \
          \"pins.json\", at `node` in pins.json and `npm` in pins.json; exactly one source row \
          may"
        ]
    );
    let null = run(
        &config,
        &[
            ("pins.json", "{\"node\": null}"),
            ("a/pins.json", "{\"node\": \"20\"}"),
        ],
    );
    assert_eq!(
        failures(&null, "c"),
        [
            "pins.json: relation `pins` (.alint.yml): `assert.equal.to.row`: the source row (`file` \
          \"pins.json\", at `node` in pins.json) holds no value of `version`"
        ]
    );
}

#[test]
fn a_literal_of_another_type_than_the_field_is_an_analysis_failure() {
    let config = PINS
        .replace("file: ROW", "version: true")
        .replace("field: version", "field: tool");
    let results = run(&config, &[("pins.json", "{\"node\": \"true\"}")]);
    assert_eq!(
        failures(&results, "c"),
        [
            "pins.json: relation `pins` (.alint.yml): `assert.equal.to.row.version`: the literal is \
          a boolean and `version` holds a string at `node` in pins.json; the comparison would \
          cross types, so the assertion has no verdict"
        ]
    );
}

#[test]
fn a_literal_from_the_environment_is_not_shown() {
    let config = PINS.replace(
        "ROW",
        "\"{{env.ALINT_TEST_UNSET_SOURCE | default('pins.json')}}\"",
    );
    let results = run(
        &config,
        &[
            ("pins.json", "{\"node\": \"20\"}"),
            ("a/pins.json", "{\"node\": \"18\"}"),
        ],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "a/pins.json:1:10: `version` is \"18\" here, but \"20\" in the source row (the literals \
          `to.row` names, at `node` in pins.json)\n  ~ the source row: pins.json:1:10"
        ]
    );
}

#[test]
fn equal_to_a_field_compares_two_fields_of_each_row() {
    let config = entries(
        "\"$.aliases\"",
        "{ name: entry.key, target: entry.value }",
        "{ equal: { field: target, to: { field: name } } }",
    );
    let results = run(
        &config,
        &[(
            "owners.json",
            "{\"aliases\": {\"a\": \"a\", \"b\": \"c\", \"d\": null}}",
        )],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "owners.json:1:29: `target` is \"c\" and `name` is \"b\" in this row; they must be \
             equal\n  ~ `name` in this row: owners.json:1:24",
            "owners.json:1:39: `target` is null and `name` is \"d\" in this row; they must be \
             equal (null matches nothing)\n  ~ `name` in this row: owners.json:1:34",
        ]
    );
}

// --- `set` ----------------------------------------------------------------

/// Relation `m` of `messages.json`'s locales, and rule `c` asserting a set
/// of `mode` against group `group`.
fn locales(mode: &str, group: &str) -> String {
    format!(
        "version: 2\nrelations:\n  m:\n    from: {{ paths: messages.json }}\n    entries: {{ \
         format: json, at: [\"$.en\", \"$.fr\", \"$.de\"] }}\n    fields: {{ locale: \
         entry.parent, message: entry.key, text: entry.value }}\nrules:\n  - id: c\n    kind: \
         constraint\n    input: m\n    assert: {{ set: {{ field: message, group_by: locale, \
         {mode}: {{ group: {group} }} }} }}\n    level: error\n"
    )
}

const MESSAGES: &str = "{\"en\": {\"home\": \"Home\", \"about\": \"About\"},\n\"fr\": {\"home\": \
                        \"Accueil\", \"faq\": \"FAQ\"},\n\"de\": {\"home\": \"Start\", \"about\": \
                        \"Info\"}}\n";

#[test]
fn equal_to_reports_each_missing_and_extra_value_per_group() {
    let results = run(&locales("equal_to", "en"), &[("messages.json", MESSAGES)]);
    assert_eq!(
        findings(&results, "c"),
        [
            "messages.json: `locale` \"fr\" lacks `message` \"about\", which `locale` \"en\" \
             holds\n  ~ the reference group's row with it: messages.json:1:25",
            "messages.json:2:27: `locale` \"fr\" holds `message` \"faq\", which `locale` \"en\" \
             lacks",
        ]
    );
}

#[test]
fn subset_of_reports_only_extra_values() {
    let results = run(&locales("subset_of", "en"), &[("messages.json", MESSAGES)]);
    assert_eq!(
        findings(&results, "c"),
        [
            "messages.json:2:27: `locale` \"fr\" holds `message` \"faq\", which `locale` \"en\" \
          lacks"
        ]
    );
}

#[test]
fn disjoint_from_reports_each_shared_value() {
    let results = run(
        &locales("disjoint_from", "fr"),
        &[("messages.json", MESSAGES)],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "messages.json:1:9: `locale` \"en\" and `locale` \"fr\" both hold `message` \
             \"home\"\n  ~ the reference group's row with it: messages.json:2:8",
            "messages.json:3:8: `locale` \"de\" and `locale` \"fr\" both hold `message` \
             \"home\"\n  ~ the reference group's row with it: messages.json:2:8",
        ]
    );
}

#[test]
fn a_set_without_its_reference_group_has_no_verdict() {
    let results = run(&locales("equal_to", "es"), &[("messages.json", MESSAGES)]);
    assert_eq!(
        failures(&results, "c"),
        [
            "relation `m` (.alint.yml): `assert.set.equal_to.group`: the reference group (`locale` \
          \"es\") has no rows; a set is compared with it, so it must have some"
        ]
    );
}

#[test]
fn a_set_whose_values_or_group_cross_types_has_no_verdict() {
    // A set's values are of one type across the relation.
    let config = locales("equal_to", "en").replace("message: entry.key", "message: entry.value");
    let results = run(
        &config,
        &[(
            "messages.json",
            "{\"en\": {\"a\": \"1\"}, \"fr\": {\"a\": 1}}",
        )],
    );
    assert_eq!(
        failures(&results, "c"),
        [
            "messages.json: relation `m` (.alint.yml): `assert.set.field`: `message` holds a string \
          at `en.a` in messages.json and a number at `fr.a` in messages.json; the comparison would \
          cross types, so the assertion has no verdict"
        ]
    );
    // The group literal is of the groups' type.
    let config = locales("equal_to", "'true'")
        .replace("locale: entry.parent", "locale: entry.value")
        .replace("text: entry.value", "text: entry.parent");
    let results = run(&config, &[("messages.json", "{\"en\": {\"a\": true}}")]);
    assert_eq!(
        failures(&results, "c"),
        [
            "messages.json: relation `m` (.alint.yml): `assert.set.equal_to.group`: the literal is \
          a string and `locale` holds a boolean at `en.a` in messages.json; the comparison would \
          cross types, so the assertion has no verdict"
        ]
    );
}

#[test]
fn null_is_in_no_set() {
    let config = locales("equal_to", "en").replace("message: entry.key", "message: entry.value");
    let results = run(
        &config,
        &[(
            "messages.json",
            "{\"en\": {\"a\": \"x\", \"b\": null}, \"fr\": {\"a\": \"x\", \"c\": null}}",
        )],
    );
    assert_eq!(findings(&results, "c"), [] as [String; 0]);
}

// --- Failures, messages and identity ------------------------------------------

#[test]
fn any_analysis_failure_leaves_the_rule_without_findings() {
    // One group is null; the other disagrees, which alone would be a finding.
    let config = entries(
        "[\"$.dev\", \"$.prod\"]",
        "{ env: entry.parent, name: entry.key, owner: entry.value }",
        "{ equal: { field: env, group_by: owner } }",
    );
    let results = run(
        &config,
        &[(
            "owners.json",
            "{\"dev\": {\"a\": \"x\"}, \"prod\": {\"a\": \"x\", \"b\": null}}",
        )],
    );
    assert_eq!(failures(&results, "c").len(), 1);
}

#[test]
fn a_relation_that_fails_gives_the_rule_its_failures() {
    let config = entries("\"$.owners\"", OWNERS, "{ unique: { key: [owner] } }");
    let results = run(&config, &[("owners.json", "{\"owners\": ")]);
    let failures = failures(&results, "c");
    assert_eq!(failures.len(), 1);
    assert!(
        failures[0].starts_with("owners.json: relation `r` (.alint.yml): could not parse"),
        "{failures:?}"
    );
}

#[test]
fn a_message_renders_the_reported_row_and_redacts_a_sensitive_field() {
    let config = "version: 2\nrelations:\n  r:\n    from: { paths: owners.json }\n    entries: { \
                  format: json, at: \"$.owners\" }\n    fields:\n      name: entry.key\n      \
                  owner: { source: entry.value, sensitive: true }\nrules:\n  - id: c\n    kind: \
                  constraint\n    input: r\n    assert: { unique: { key: [owner] } }\n    \
                  message: \"{{row.name}} repeats {{row.owner}}\"\n    level: error\n  - id: g\n    \
                  kind: constraint\n    input: r\n    assert: { unique: { key: [owner] } }\n    \
                  level: error\n";
    let results = run(
        config,
        &[(
            "owners.json",
            "{\"owners\": {\"a\": \"secret\", \"b\": \"secret\"}}",
        )],
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "owners.json:1:33: b repeats <redacted>\n  ~ the first row with this key: owners.json:1:18"
        ]
    );
    assert_eq!(
        findings(&results, "g"),
        [
            "owners.json:1:33: `owner` <redacted> is not unique: the first row with it is \
          `owners.a` in owners.json\n  ~ the first row with this key: owners.json:1:18"
        ]
    );
    // Neither the baseline key nor the message holds the value.
    for id in ["c", "g"] {
        for violation in &result(&results, id).violations {
            let key = violation.baseline_key.as_deref().unwrap();
            assert!(!key.contains("secret"), "{key}");
            assert!(!violation.message.contains("secret"));
        }
    }
}

#[test]
fn a_missing_or_null_value_renders_as_such() {
    let config = PINS.replace("ROW", "pins.json").replace(
        "    level: error\n",
        "    message: \"{{row.tool}} is {{row.version}}\"\n    level: error\n",
    );
    let results = run(
        &config,
        &[
            ("pins.json", "{\"node\": \"20\"}"),
            ("a/pins.json", "{\"node\": null}"),
        ],
    );
    assert_eq!(
        findings(&results, "c"),
        ["a/pins.json:1:10: node is null\n  ~ the source row: pins.json:1:10"]
    );
}

/// The fingerprints of rule `id`'s findings, sorted.
fn fingerprints(results: &[RuleResult], id: &str) -> Vec<String> {
    let mut prints: Vec<String> = result(results, id)
        .violations
        .iter()
        .map(|violation| alint_core::baseline::fingerprint(id, violation, None))
        .collect();
    prints.sort();
    prints
}

#[test]
fn a_finding_keeps_its_fingerprint_when_its_first_row_moves() {
    let config = "version: 2\nrelations:\n  deps:\n    from: { paths: \"*/package.json\" }\n    \
                  entries: { format: json, at: \"$.dependencies\" }\n    fields: { dependency: \
                  entry.key, range: entry.value }\n  names:\n    from: { paths: \
                  \"*/package.json\" }\n    read: { json: \"$.name\" }\n    fields: { name: value \
                  }\nrules:\n  - id: d\n    kind: constraint\n    input: deps\n    assert: { \
                  unique: { key: [dependency], determines: [range] } }\n    level: error\n  - id: \
                  e\n    kind: constraint\n    input: deps\n    assert: { equal: { field: range, \
                  group_by: dependency } }\n    level: error\n";
    let manifest = |range: &str| format!("{{\"dependencies\": {{\"react\": \"{range}\"}}}}");
    let before = run(
        config,
        &[
            ("b/package.json", &manifest("^18")),
            ("c/package.json", &manifest("^17")),
            ("d/package.json", &manifest("^18")),
        ],
    );
    // A row that sorts first, and the first resource renamed.
    let after = run(
        config,
        &[
            ("a/package.json", &manifest("^18")),
            ("bb/package.json", &manifest("^18")),
            ("c/package.json", &manifest("^17")),
            ("d/package.json", &manifest("^18")),
        ],
    );
    for id in ["d", "e"] {
        assert_eq!(fingerprints(&before, id).len(), 2);
        assert_eq!(fingerprints(&before, id), fingerprints(&after, id), "{id}");
        assert_ne!(
            result(&before, id).violations[0].path,
            result(&after, id).violations[0].path
        );
    }
    // Each finding is identified by its value: a third range is a new one.
    let third = run(
        config,
        &[
            ("b/package.json", &manifest("^18")),
            ("c/package.json", &manifest("^17")),
            ("d/package.json", &manifest("^16")),
        ],
    );
    let old = fingerprints(&before, "d");
    let new = fingerprints(&third, "d");
    assert_eq!(new.len(), 3);
    assert_eq!(new.iter().filter(|print| old.contains(print)).count(), 2);
}

#[test]
fn a_key_names_the_family_and_the_values_it_is_about() {
    let results = run(&locales("equal_to", "en"), &[("messages.json", MESSAGES)]);
    let keys: Vec<&str> = result(&results, "c")
        .violations
        .iter()
        .map(|violation| violation.baseline_key.as_deref().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "3:set6:string2:fr6:string5:about7:missing",
            "3:set6:string2:fr6:string3:faq5:extra",
        ]
    );
    for violation in &result(&results, "c").violations {
        assert!(violation.path_free_identity);
    }
}
