//! `constraint` rules run over trees by the engine (design
//! `v0.23/selectors-and-constraints.md` section 6): each family's findings
//! and related locations, comparison and nulls, every analysis failure, the
//! messages and their redaction, and the path-free identity. The loader and
//! the builder's refusals are tested in `constraint.rs`.

use std::fmt::Write as _;

use alint_core::{FileIndex, RuleResult, Violation, WalkOptions, walk};

use super::constraint::load_tree;

/// The results of running every rule of `config` over `files`.
pub(super) fn run(config: &str, files: &[(&str, &str)]) -> Vec<RuleResult> {
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
pub(super) fn shown(violation: &Violation) -> String {
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
pub(super) fn result<'r>(results: &'r [RuleResult], id: &str) -> &'r RuleResult {
    results
        .iter()
        .find(|result| &*result.rule_id == id)
        .unwrap_or_else(|| panic!("no result for {id}"))
}

/// Rule `id`'s findings, shown.
pub(super) fn findings(results: &[RuleResult], id: &str) -> Vec<String> {
    let result = result(results, id);
    assert!(
        result.analysis_failures.is_empty(),
        "{id} failed: {:?}",
        result.analysis_failures
    );
    result.violations.iter().map(shown).collect()
}

/// Rule `id`'s analysis failures, as `path: message`; it has no finding.
pub(super) fn failures(results: &[RuleResult], id: &str) -> Vec<String> {
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
            "owners.json: relation `r` (.alint.yml): `owner` binds `entry.value`, which is an \
            array \
          at `owners.a`; a field holds a scalar (null, a boolean, a number or a string), so \
          select the members below it with `at:`, or bind only `entry.key`"
        ]
    );
}

// --- `equal` against a source row, and within a row --------------------------

const PINS: &str = "version: 2\nrelations:\n  pins:\n    from: { paths: \"**/pins.json\" }\n    \
                    entries: { format: json, at: \"$\" }\n    fields: { tool: entry.key, version: \
                    entry.value, file: path }\nrules:\n  - id: c\n    kind: constraint\n    input: \
                    pins\n    assert: { equal: { field: version, to: { row: { file: ROW } } } \
                    }\n    \
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
            "pins.json: relation `pins` (.alint.yml): `assert.equal.to.row`: the source row \
            (`file` \
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
            "pins.json: relation `pins` (.alint.yml): `assert.equal.to.row.version`: the literal \
            is \
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
            "a/pins.json:1:10: `version` is \"18\" here, but \"20\" in the source row (the \
            literals \
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
            "messages.json: relation `m` (.alint.yml): `assert.set.field`: `message` holds a \
            string \
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
                  message: \"{{row.name}} repeats {{row.owner}}\"\n    level: error\n  - id: \
                  g\n    \
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
pub(super) fn fingerprints(results: &[RuleResult], id: &str) -> Vec<String> {
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

// --- Bounds, places and identity (the step 1 review) -------------------------

/// `owners.json` with `count` owners, each of its own value.
fn distinct_owners(count: usize) -> String {
    let owners: Vec<String> = (0..count)
        .map(|index| format!("\"k{index}\": \"v{index}\""))
        .collect();
    format!("{{\"owners\": {{\n{}}}}}\n", owners.join(",\n"))
}

/// 3,000 distinct values give 3,000 findings, each relating to 20 rows and
/// counting the 2,979 others, without placing them: building every related
/// location first made the findings quadratic in the values.
#[test]
fn a_finding_places_no_more_related_locations_than_it_lists() {
    let config = entries("\"$.owners\"", OWNERS, "{ equal: { field: owner } }");
    let body = distinct_owners(3_000);
    let started = std::time::Instant::now();
    let results = run(&config, &[("owners.json", &body)]);
    let elapsed = started.elapsed();
    let result = result(&results, "c");
    assert_eq!(result.violations.len(), 3_000);
    for violation in &result.violations {
        assert_eq!(violation.related_locations.len(), 20);
        assert_eq!(violation.related_locations_omitted, 2_979);
    }
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "3,000 values took {elapsed:?}"
    );
}

#[test]
fn a_finding_relates_a_row_of_each_other_value_before_rows_of_its_own() {
    let config = "version: 2\nrelations:\n  deps:\n    from: { paths: \"*/package.json\" }\n    \
                  entries: { format: json, at: \"$.dependencies\" }\n    fields: { dependency: \
                  entry.key, range: entry.value }\nrules:\n  - id: c\n    kind: constraint\n    \
                  input: deps\n    assert: { unique: { key: [dependency], determines: [range] } \
                  }\n    level: error\n";
    let manifest = |range: &str| format!("{{\"dependencies\": {{\"react\": \"{range}\"}}}}\n");
    let (new, old) = (manifest("^18.2.0"), manifest("^17.0.2"));
    let mut files: Vec<(String, &str)> = (0..28)
        .map(|index| (format!("p{index:02}/package.json"), new.as_str()))
        .collect();
    files.push(("q1/package.json".into(), old.as_str()));
    files.push(("q2/package.json".into(), old.as_str()));
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), *body))
        .collect();
    let results = run(config, &files);
    let result = result(&results, "c");
    assert_eq!(result.violations.len(), 2);
    let labels = |violation: &Violation| -> Vec<String> {
        violation
            .related_locations
            .iter()
            .filter(|related| related.label == "a row with another value")
            .map(|related| related.path.display().to_string().replace('\\', "/"))
            .collect()
    };
    // 27 other rows at `^18.2.0`: 19 listed after the row of `^17.0.2`.
    let common = &result.violations[0];
    assert_eq!(labels(common), ["q1/package.json"]);
    assert_eq!(common.related_locations.len(), 20);
    assert_eq!(common.related_locations_omitted, 8);
    let rare = &result.violations[1];
    assert_eq!(labels(rare), ["p00/package.json"]);
    assert_eq!(rare.related_locations.len(), 2);
    assert_eq!(rare.related_locations_omitted, 0);
}

#[test]
fn rows_that_repeat_a_key_share_its_identity_and_keep_it_when_files_are_renamed() {
    let config = "version: 2\nrelations:\n  names:\n    from: { paths: \"p/*.json\" }\n    read: { \
                  json: \"$.name\" }\n    fields: { name: value }\nrules:\n  - id: c\n    kind: \
                  constraint\n    input: names\n    assert: { unique: { key: [name] } }\n    \
                  level: \
                  error\n";
    let dup = "{\"name\": \"dup\"}";
    let before = run(config, &[("p/a.json", dup), ("p/b.json", dup)]);
    let after = run(config, &[("p/b.json", dup), ("p/c.json", dup)]);
    assert_eq!(fingerprints(&before, "c"), fingerprints(&after, "c"));
    assert_eq!(
        result(&after, "c").violations[0].path.as_deref(),
        Some(std::path::Path::new("p/c.json"))
    );
    // Three rows with a key give two findings of one identity.
    let three = run(
        config,
        &[("p/a.json", dup), ("p/b.json", dup), ("p/c.json", dup)],
    );
    let prints = fingerprints(&three, "c");
    assert_eq!(prints.len(), 2);
    assert_eq!(prints[0], prints[1]);
    assert_eq!(prints[0], fingerprints(&before, "c")[0]);
}

#[test]
fn values_a_read_skipped_are_withheld_with_the_findings_of_a_rule_without_a_verdict() {
    let config = "version: 2\nrelations:\n  pins:\n    from: { paths: \"*/package.json\" \
                  }\n    read: { json: \"$.engines.node\" }\n    fields: { version: value, source: \
                  path }\nrules:\n  - id: c\n    kind: constraint\n    \
                  input: pins\n    assert: { equal: { field: version, to: { row: { source: \
                  .nvmrc-missing } } } }\n    level: error\n";
    let results = run(
        config,
        &[
            ("a/package.json", "{\"engines\": {\"node\": 20}}"),
            ("b/package.json", "{\"engines\": {\"node\": \"20\"}}"),
        ],
    );
    assert_eq!(
        failures(&results, "c"),
        [
            "relation `pins` (.alint.yml): `assert.equal.to.row`: no row has `source` \
             \".nvmrc-missing\"; the source row must exist"
        ]
    );
    // With a verdict, the skipped value is a finding.
    let config = config.replace(".nvmrc-missing", "b/package.json");
    let results = run(
        &config,
        &[
            ("a/package.json", "{\"engines\": {\"node\": 20}}"),
            ("b/package.json", "{\"engines\": {\"node\": \"20\"}}"),
        ],
    );
    let found = findings(&results, "c");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("non-string"), "{found:?}");
}

/// Three manifests depend on `shared`, joined with its manifest's name: a
/// finding about a field the join copies from its unique side sits at each
/// row's own entry, the dependency, never at the one name all three share.
#[test]
fn a_finding_about_a_field_of_a_joins_unique_side_sits_at_the_rows_own_entry() {
    let config = "version: 2\nrelations:\n  names:\n    from: { paths: \"*/package.json\" }\n    \
                  read: { json: \"$.name\" }\n    fields: { name: value }\n  deps:\n    from: { \
                  paths: \"*/package.json\" }\n    entries: { format: json, at: \
                  \"$.dependencies\" }\n    fields: { dependency: entry.key, range: entry.value \
                  }\n  linked:\n    join: { left: deps, right: names, on: { dependency: name }, \
                  unique: right }\n    fields: { range: left.range, wname: right.name }\nrules:\n  \
                  - id: u\n    kind: constraint\n    input: linked\n    assert: { unique: { key: \
                  [wname] } }\n    level: error\n  - id: e\n    kind: constraint\n    input: \
                  linked\n    assert: { equal: { field: wname, to: { field: range } } }\n    \
                  level: error\n";
    let dependent = "{\"name\": \"N\",\n\"dependencies\": {\"shared\": \"^1\"}}\n";
    let results = run(
        config,
        &[
            ("a/package.json", &dependent.replace('N', "a")),
            ("b/package.json", &dependent.replace('N', "b")),
            ("c/package.json", &dependent.replace('N', "c")),
            ("shared/package.json", "{\"name\": \"shared\"}"),
        ],
    );
    assert_eq!(
        findings(&results, "u"),
        [
            "b/package.json:2:28: `wname` \"shared\" is not unique: the first row with it is \
             `dependencies.shared` in a/package.json\n  ~ the first row with this key: \
             a/package.json:2:28",
            "c/package.json:2:28: `wname` \"shared\" is not unique: the first row with it is \
             `dependencies.shared` in a/package.json\n  ~ the first row with this key: \
             a/package.json:2:28",
        ]
    );
    let found = findings(&results, "e");
    assert_eq!(found.len(), 3);
    for (finding, manifest) in found.iter().zip(["a", "b", "c"]) {
        assert!(
            finding.starts_with(&format!(
                "{manifest}/package.json:2:28: `wname` is \"shared\""
            )),
            "{finding}"
        );
    }
    let prints = fingerprints(&results, "e");
    assert_eq!(prints.len(), 3);
    assert!(prints[0] != prints[1] && prints[1] != prints[2]);
    // A join that copies only its unique side's fields: a row's own entry
    // has no cell, so its finding sits at its file.
    let only = config.replace(
        "fields: { range: left.range, wname: right.name }",
        "fields: { wname: right.name }",
    );
    let only = only[..only.find("  - id: e\n").unwrap()].to_string();
    let results = run(
        &only,
        &[
            ("a/package.json", &dependent.replace('N', "a")),
            ("b/package.json", &dependent.replace('N', "b")),
            ("shared/package.json", "{\"name\": \"shared\"}"),
        ],
    );
    assert_eq!(
        findings(&results, "u"),
        [
            "b/package.json: `wname` \"shared\" is not unique: the first row with it is \
             `dependencies.shared` in a/package.json\n  ~ the first row with this key: \
             a/package.json"
        ]
    );
}

#[test]
fn a_long_value_is_cut_in_a_message_and_held_as_a_digest_in_a_key() {
    let long = "x".repeat(10_000);
    let config = locales("equal_to", "en").replace("message: entry.key", "message: entry.value");
    let config = config.replace(
        "    level: error\n",
        "    message: \"lacks {{row.message}}\"\n    level: error\n",
    );
    let body = format!("{{\"en\": {{\"a\": \"{long}\"}}, \"fr\": {{\"b\": \"y\"}}}}");
    let results = run(&config, &[("messages.json", &body)]);
    let result = result(&results, "c");
    assert_eq!(result.violations.len(), 2);
    for violation in &result.violations {
        let key = violation.baseline_key.as_deref().unwrap();
        assert!(key.len() < 200, "{key}");
        assert!(violation.message.len() < 200, "{}", violation.message);
    }
    let missing = &result.violations[0];
    assert_eq!(
        missing.message,
        format!("lacks {}... (10000 bytes)", "x".repeat(64))
    );
    assert!(
        missing
            .baseline_key
            .as_deref()
            .unwrap()
            .contains("13:string-sha256"),
        "{:?}",
        missing.baseline_key
    );
}

/// A sensitive `entry.key` makes every field that spells the key sensitive:
/// `entry.path` shows it whole.
#[test]
fn a_sensitive_key_hides_every_field_that_spells_it() {
    let config = "version: 2\nrelations:\n  r:\n    from: { paths: tokens.json }\n    entries: { \
                  format: json, at: \"$\" }\n    fields:\n      k: { source: entry.key, \
                  sensitive: true }\n      p: entry.path\n      v: entry.value\nrules:\n  - id: \
                  e\n    kind: constraint\n    input: r\n    assert: { equal: { field: p } }\n    \
                  level: error\n  - id: m\n    kind: constraint\n    input: r\n    assert: { \
                  unique: { key: [v] } }\n    message: \"key {{row.k}} at {{row.p}}\"\n    level: \
                  error\n";
    let results = run(
        config,
        &[(
            "tokens.json",
            "{\"SECRET_TOKEN_NAME_1\": \"x\", \"SECRET_TOKEN_NAME_2\": \"x\"}",
        )],
    );
    for id in ["e", "m"] {
        let result = result(&results, id);
        assert!(!result.violations.is_empty(), "{id}");
        for violation in &result.violations {
            let shown = shown(violation);
            let key = violation.baseline_key.as_deref().unwrap();
            assert!(!shown.contains("SECRET"), "{id}: {shown}");
            assert!(!key.contains("SECRET"), "{id}: {key}");
        }
    }
    assert_eq!(
        result(&results, "m").violations[0].message,
        "key <redacted> at <redacted>"
    );
}

/// A field that holds keys is sensitive, so no message or baseline key of
/// the relation's findings shows a key path, not even one of a row's own.
#[test]
fn a_sensitive_key_keeps_key_paths_out_of_messages_and_keys() {
    let config = "version: 2\nrelations:\n  r:\n    from: { paths: \"*/pins.json\" }\n    \
                  entries: { format: json, at: \"$\" }\n    fields:\n      tool: { source: \
                  entry.key, sensitive: true }\n      version: entry.value\n      file: \
                  path\nrules:\n  - id: s\n    kind: constraint\n    input: r\n    \
                  assert: { equal: { field: version, to: { row: { file: a/pins.json } } } }\n    \
                  level: error\n  - id: u\n    kind: constraint\n    input: r\n    assert: { \
                  unique: { key: [version] } }\n    level: error\n";
    let results = run(
        config,
        &[
            ("a/pins.json", "{\"SECRET_TOOL\": \"1\"}"),
            ("b/pins.json", "{\"SECRET_TOOL\": \"2\"}"),
            ("c/pins.json", "{\"SECRET_TOOL\": \"1\"}"),
        ],
    );
    for id in ["s", "u"] {
        let result = result(&results, id);
        assert!(result.analysis_failures.is_empty(), "{id}");
        assert_eq!(result.violations.len(), 1, "{id}");
        for violation in &result.violations {
            let shown = shown(violation);
            let key = violation.baseline_key.as_deref().unwrap();
            assert!(!shown.contains("SECRET"), "{id}: {shown}");
            assert!(
                shown.contains("a key path not shown (a field holding keys is sensitive)"),
                "{id}: {shown}"
            );
            assert!(!key.contains("SECRET"), "{id}: {key}");
        }
    }
}

#[test]
fn two_missing_or_null_values_of_a_row_are_not_equal() {
    let config = entries(
        "\"$\"",
        "{ a: entry.value, b: entry.value }",
        "{ equal: { field: a, to: { field: b } } }",
    );
    let results = run(&config, &[("owners.json", "{\"k\": null, \"j\": \"1\"}")]);
    assert_eq!(
        findings(&results, "c"),
        [
            "owners.json:1:7: `a` is null and `b` is null in this row; they must be equal (null \
             matches nothing)\n  ~ `b` in this row: owners.json:1:7"
        ]
    );
}

#[test]
fn a_key_with_a_null_field_is_never_a_repeat() {
    let config = entries(
        "\"$.owners\"",
        "{ name: entry.key, owner: entry.value, file: path }",
        "{ unique: { key: [file, owner] } }",
    );
    let results = run(
        &config,
        &[(
            "owners.json",
            "{\"owners\": {\"a\": null, \"b\": null, \"c\": \"x\"}}",
        )],
    );
    assert_eq!(findings(&results, "c"), [] as [String; 0]);
}

#[test]
fn a_missing_value_sits_at_the_groups_first_file() {
    // Group `fr` is read from two files: a value it lacks sits at the first.
    let config = "version: 2\nrelations:\n  m:\n    from: { paths: \"*.json\" }\n    entries: { \
                  format: json, at: \"$.*\" }\n    fields: { locale: entry.parent, message: \
                  entry.key }\nrules:\n  - id: c\n    kind: constraint\n    input: m\n    assert: \
                  { set: { field: message, group_by: locale, equal_to: { group: en } } }\n    \
                  level: error\n";
    let results = run(
        config,
        &[
            (
                "a.json",
                "{\"en\": {\"home\": 1, \"about\": 2}, \"fr\": {\"home\": 1}}",
            ),
            ("b.json", "{\"fr\": {\"faq\": 1}}"),
        ],
    );
    let found = findings(&results, "c");
    assert!(
        found.iter().any(|finding| finding.starts_with(
            "a.json: `locale` \"fr\" lacks `message` \"about\", which `locale` \"en\" holds"
        )),
        "{found:?}"
    );
}

/// With two determined fields, a finding sits at the first one the rows
/// disagree on: here `owner`, since the rows of key `a` share their file.
#[test]
fn a_finding_sits_at_the_first_determined_field_the_rows_disagree_on() {
    let config = entries(
        "[\"$.dev\", \"$.prod\"]",
        "{ name: entry.key, owner: entry.value, file: path }",
        "{ unique: { key: [name], determines: [file, owner] } }",
    );
    let results = run(
        &config,
        &[(
            "owners.json",
            "{\"dev\": {\"a\": \"x\"},\n\"prod\": {\"a\": \"y\"}}\n",
        )],
    );
    let found = findings(&results, "c");
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("owners.json:1:15:"), "{found:?}");
    assert!(found[1].starts_with("owners.json:2:15:"), "{found:?}");
}

/// A finding's place is read as the relation read its file: as a literal
/// path, never through a symlink out of the root that replaced the file
/// after the relation read it (or a FIFO, which would block the read).
#[cfg(unix)]
#[test]
fn a_place_is_read_as_the_relation_read_its_file() {
    let config = entries("\"$.owners\"", OWNERS, "{ unique: { key: [owner] } }");
    let body = "{\"owners\": {\"a\": \"x\", \"b\": \"x\"}}";
    let (dir, loaded) = load_tree(&config, &[("owners.json", body)]);
    let config = loaded.unwrap();
    let index = walk(
        dir.path(),
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    let mut registry = alint_core::RuleRegistry::new();
    alint_rules::register_builtin(&mut registry);
    let rule = registry.build(&config.rules[0]).unwrap();
    let ctx = alint_core::Context::new(dir.path(), &index);
    let first = rule.evaluate(&ctx).unwrap();
    assert_eq!(first.len(), 1);
    assert!(first[0].range.is_some());
    // The relation's rows are the run's; its file is now a symlink out.
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("o.json"), format!("\n\n{body}")).unwrap();
    std::fs::remove_file(dir.path().join("owners.json")).unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("o.json"),
        dir.path().join("owners.json"),
    )
    .unwrap();
    let second = rule.evaluate(&ctx).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].path, first[0].path);
    assert_eq!(second[0].range, None);
    assert!(
        second[0]
            .related_locations
            .iter()
            .all(|related| related.range.is_none())
    );
}
