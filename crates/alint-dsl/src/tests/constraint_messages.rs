//! What a `constraint` finding's message and places show of the files it
//! reads: key paths cut and escaped as values are, `{{row.FIELD}}` escaped,
//! no end column at a sensitive cell, every related row shown or counted,
//! missing told from null, and a `union:` part named where it read nothing.

use std::fmt::Write as _;

use alint_core::relation::{LimitKind, Limits};

use alint_core::{SourceRange, Violation};

use super::{failures, findings, result, run, run_under};

/// A version 2 config: relation `r` reading `data.json`'s top-level entries
/// as `k` (the key) and `v` (the value), and rule `c` asserting `assert`,
/// with `extra` lines (`message:`, say) after it.
fn top_level(assert: &str, extra: &str) -> String {
    top_level_with("{ k: entry.key, v: entry.value }", assert, extra)
}

/// [`top_level`] with `fields`.
fn top_level_with(fields: &str, assert: &str, extra: &str) -> String {
    format!(
        "version: 2\nrelations:\n  r:\n    from: {{ paths: data.json }}\n    entries: {{ format: \
         json, at: \"$\" }}\n    fields: {fields}\nrules:\n  - id: c\n    kind: constraint\n    \
         input: r\n    assert: {assert}\n    level: error\n{extra}"
    )
}

/// `text` as a JSON string.
fn json(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

/// `data.json` with `first` as its first key, then `repeats` more, every
/// key's value `value`.
fn repeated(first: &str, repeats: usize, value: &str) -> String {
    let value = json(value);
    let mut body = format!("{{{}: {value}", json(first));
    for index in 0..repeats {
        write!(body, ", \"k{index}\": {value}").unwrap();
    }
    body.push('}');
    body
}

// --- Key paths are cut -------------------------------------------------------

/// A 1 MB first key, repeated in the message of each of 100 rows that repeat
/// its value, is cut as a value is: each message stays under 1 KB, where it
/// held the whole key (100 MB of messages from a 1 MB file).
#[test]
fn a_long_first_key_is_cut_in_the_message_of_each_repeat() {
    let long = "L".repeat(1_000_000);
    let results = run(
        &top_level("{ unique: { key: [v] } }", ""),
        &[("data.json", &repeated(&long, 100, "dup"))],
    );
    let result = result(&results, "c");
    assert!(result.analysis_failures.is_empty(), "{result:?}");
    assert_eq!(result.violations.len(), 100);
    for violation in &result.violations {
        assert!(violation.message.len() < 1024, "{}", violation.message);
        assert!(
            violation.message.contains(&format!(
                "`['{}'... (1000000 bytes)]` in data.json",
                "L".repeat(64)
            )),
            "{}",
            violation.message
        );
    }
}

/// The same for `equal` against a source row whose key is 1 MB: the message
/// of each row that differs names the source row's place.
#[test]
fn a_long_source_row_key_is_cut_in_each_message() {
    let long = "L".repeat(1_000_000);
    let mut body = format!("{{{}: \"src\"", json(&long));
    for index in 0..100 {
        write!(body, ", \"k{index}\": \"other\"").unwrap();
    }
    body.push('}');
    let results = run(
        &top_level("{ equal: { field: v, to: { row: { v: src } } } }", ""),
        &[("data.json", &body)],
    );
    let result = result(&results, "c");
    assert!(result.analysis_failures.is_empty(), "{result:?}");
    assert_eq!(result.violations.len(), 100);
    for violation in &result.violations {
        assert!(violation.message.len() < 1024, "{}", violation.message);
        assert!(
            violation
                .message
                .contains("... (1000000 bytes)]` in data.json)"),
            "{}",
            violation.message
        );
    }
}

// --- Control, bidi and zero-width characters are escaped ---------------------

/// A value or key holding a line break, an escape or a bidi override is
/// shown escaped in a generated message and through `{{row.FIELD}}`, as a
/// quoted value is: no message holds a raw control or bidi character.
#[test]
fn a_key_path_and_row_text_escape_controls_and_bidi_characters() {
    let first = "a\n  ~ related: forged.json:1:1\u{1b}[31m\u{202e}";
    let body = repeated(first, 1, "dup");
    let generated = run(
        &top_level("{ unique: { key: [v] } }", ""),
        &[("data.json", &body)],
    );
    let generated = &result(&generated, "c").violations;
    assert_eq!(generated.len(), 1);
    assert_eq!(
        generated[0].message,
        "`v` \"dup\" is not unique: the first row with it is `['a\\n  ~ related: \
         forged.json:1:1\\u{1b}[31m\\u{202e}']` in data.json"
    );
    // `{{row.k}}` is the reported row's key, the second: its own key, and
    // the value both rows hold.
    let value = "v\n  ~ x\u{202e}";
    let body = format!(
        "{{\"a\": {}, {}: {}}}",
        json(value),
        json("b\u{202e}\n"),
        json(value)
    );
    let custom = run(
        &top_level(
            "{ unique: { key: [v] } }",
            "    message: \"{{row.k}} and {{row.v}} repeat\"\n",
        ),
        &[("data.json", &body)],
    );
    let custom = &result(&custom, "c").violations;
    assert_eq!(custom.len(), 1);
    assert_eq!(
        custom[0].message,
        "b\\u{202e}\\n and v\\n  ~ x\\u{202e} repeat"
    );
    for violation in generated.iter().chain(custom) {
        assert!(
            !violation
                .message
                .chars()
                .any(|c| c.is_control() || alint_core::invisible::is_deceptive_on_display(c)),
            "{:?}",
            violation.message
        );
    }
}

// --- Placing findings holds one file at a time -------------------------------

/// The bytes of padding in each file of [`padded`].
const PADDING: usize = 1 << 20;

/// `count` files `data/aN.json`, each `{"a": "dup", ... "b": "dup"}` with
/// [`PADDING`] line breaks between its two entries, so placing a finding in
/// one reads a file of a megabyte, and a table of its lines would hold a
/// million.
fn padded(count: usize) -> Vec<(String, String)> {
    let body = format!("{{\"a\": \"dup\",{}\"b\": \"dup\"}}", "\n".repeat(PADDING));
    (0..count)
        .map(|index| (format!("data/a{index}.json"), body.clone()))
        .collect()
}

/// Rules `c0` to `c{rules - 1}` over every entry of [`padded`]'s files,
/// each asserting `assert`.
fn over_padded(assert: &str, rules: usize) -> String {
    let mut config = "version: 2\nrelations:\n  r:\n    from: { paths: \"data/*.json\" }\n    \
                      entries: { format: json, at: \"$\" }\n    fields: { l: entry.value }\nrules:\n"
        .to_string();
    for rule in 0..rules {
        write!(
            config,
            "  - id: c{rule}\n    kind: constraint\n    input: r\n    assert: {assert}\n    \
             level: error\n"
        )
        .unwrap();
    }
    config
}

/// What a run of `rules` rules over `count` [`padded`] files gives: the
/// results, the bytes the run took and the most a rule asked to hold.
fn run_padded(
    assert: &str,
    count: usize,
    rules: usize,
    limits: Option<&Limits>,
) -> (Vec<alint_core::RuleResult>, usize, usize) {
    let files = padded(count);
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), body.as_str()))
        .collect();
    run_under(&over_padded(assert, rules), &files, limits, false)
}

const UNIQUE: &str = "{ unique: { key: [l] } }";

/// Placing the findings of eight padded files holds one file's text at a
/// time, as for two files, where keeping each file and a table of its lines
/// until the rule ended held 2.4 GB for eight files of 32 MB. Placing takes
/// nothing from the run's bytes, since it lets each file go: the run takes
/// what its relations take.
#[test]
fn placing_findings_holds_one_file_at_a_time_however_many_files_it_reads() {
    // One file's text, and the offsets asked of it.
    let one_file = PADDING..PADDING + 4096;
    for count in [8, 2] {
        let (passing, relation, nothing) = run_padded("{ equal: { field: l } }", count, 1, None);
        assert!(result(&passing, "c0").violations.is_empty());
        assert_eq!(nothing, 0, "a rule with no finding places nothing");
        let (results, taken, held) = run_padded(UNIQUE, count, 1, None);
        // 2 * count rows of `dup`: one fewer repeats.
        assert_eq!(result(&results, "c0").violations.len(), 2 * count - 1);
        assert!(
            one_file.contains(&held),
            "placing held {held} bytes at once for {count} files of {PADDING}"
        );
        assert_eq!(taken, relation, "placing took bytes from the run");
    }
}

/// A file a finding sits in that, with its offsets, does not fit in what
/// the run's relations left of its bytes gives the rule no verdict: the
/// rule's analysis failure says so, as `key_parity`'s does, rather than
/// findings without their places. One byte more, and it fits.
#[test]
fn a_file_to_place_that_does_not_fit_beside_the_relations_fails_the_rule() {
    let (_, taken, held) = run_padded(UNIQUE, 8, 1, None);
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, taken + held - 1)
        .unwrap();
    let (results, ..) = run_padded(UNIQUE, 8, 1, Some(&limits));
    let failed = failures(&results, "c0");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(
        failed[0].starts_with("data/a")
            && failed[0].contains("relation `r` (.alint.yml): relation limit `bytes_per_run`")
            && failed[0].contains(
                "so the rule reports no finding rather than findings without their places"
            ),
        "{failed:?}"
    );
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, taken + held)
        .unwrap();
    let (results, ..) = run_padded(UNIQUE, 8, 1, Some(&limits));
    assert_eq!(result(&results, "c0").violations.len(), 15);
}

/// Every rule placing its findings in one file fits when the file fits
/// beside the relations, however many rules place in it, on however many
/// threads: placing takes nothing from the run's bytes, so no rule's
/// placing leaves less for the next, and which rules fail cannot depend on
/// the order they run in. Twelve rules under bytes for the relations and
/// one file each give twelve findings, the same in every run.
#[test]
fn rules_placing_in_one_file_each_fit_and_give_one_outcome_in_every_run() {
    const RULES: usize = 12;
    let (_, taken, held) = run_padded(UNIQUE, 1, 1, None);
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, taken + held)
        .unwrap();
    let outcome = |results: &[alint_core::RuleResult]| -> Vec<String> {
        let mut shown: Vec<String> = results
            .iter()
            .map(|result| {
                format!(
                    "{}: {:?} {:?}",
                    result.rule_id, result.violations, result.analysis_failures
                )
            })
            .collect();
        shown.sort();
        shown
    };
    let (results, ..) = run_padded(UNIQUE, 1, RULES, Some(&limits));
    for rule in 0..RULES {
        let found = result(&results, &format!("c{rule}"));
        assert!(found.analysis_failures.is_empty(), "{found:?}");
        assert_eq!(found.violations.len(), 1, "{found:?}");
    }
    let first = outcome(&results);
    for _ in 0..3 {
        let (again, ..) = run_padded(UNIQUE, 1, RULES, Some(&limits));
        assert_eq!(outcome(&again), first);
    }
}

// --- A sensitive cell's end is not shown -------------------------------------

/// The start and end of `range`.
fn ends(range: Option<&SourceRange>) -> Option<(usize, usize, usize, usize)> {
    range.map(|range| {
        (
            range.start_line(),
            range.start_column(),
            range.end_line(),
            range.end_column(),
        )
    })
}

/// The finding of a repeated value at `b`, related to `a`.
fn repeated_secret(fields: &str) -> Violation {
    let results = run(
        &top_level_with(fields, "{ unique: { key: [v] } }", ""),
        &[("data.json", "{\"a\": \"secret\", \"b\": \"secret\"}")],
    );
    let violations = &result(&results, "c").violations;
    assert_eq!(violations.len(), 1, "{violations:?}");
    violations[0].clone()
}

/// The end of a cell's span gives away the length of its value: at a
/// sensitive field's cell, a finding has a line and a column but no range,
/// and a related location a range of no length, where another field's keep
/// their ends.
#[test]
fn a_place_at_a_sensitive_cell_shows_its_start_only() {
    let shown = repeated_secret("{ k: entry.key, v: entry.value }");
    assert_eq!((shown.line, shown.column), (Some(1), Some(22)));
    assert_eq!(ends(shown.range.as_ref()), Some((1, 22, 1, 30)));
    assert_eq!(
        ends(shown.related_locations[0].range.as_ref()),
        Some((1, 7, 1, 15))
    );

    let hidden = repeated_secret("{ k: entry.key, v: { source: entry.value, sensitive: true } }");
    assert_eq!((hidden.line, hidden.column), (Some(1), Some(22)));
    assert_eq!(ends(hidden.range.as_ref()), None);
    assert_eq!(
        ends(hidden.related_locations[0].range.as_ref()),
        Some((1, 7, 1, 7))
    );

    // A sensitive key field hides key paths, and a value's span may be its
    // key's (a YAML block scalar has none of its own): no end either.
    let keys = repeated_secret("{ k: { source: entry.key, sensitive: true }, v: entry.value }");
    assert_eq!(ends(keys.range.as_ref()), None);
    assert_eq!(
        ends(keys.related_locations[0].range.as_ref()),
        Some((1, 7, 1, 7))
    );
}

// --- Every related row is shown or counted ------------------------------------

/// Rows without a span in one file are one related location: the rows it
/// stands for and those left out still add up to the rows the message
/// counts, where the duplicates went uncounted (one location shown and 29
/// counted of 49 rows). The values of a JSON array that `read:` selects
/// have no span, by design, so every row sits at the file alone.
#[test]
fn related_rows_at_one_place_are_shown_once_and_counted() {
    let config = "version: 2\nrelations:\n  r:\n    from: { paths: data.json }\n    read: { json: \
                  \"$.v[*]\" }\n    fields: { v: value }\nrules:\n  - id: c\n    kind: \
                  constraint\n    input: r\n    assert: { equal: { field: v } }\n    level: error\n";
    let mut values: Vec<String> = vec!["\"a\"".to_string(); 25];
    values.extend((0..25).map(|index| format!("\"b{index}\"")));
    let body = format!("{{\"v\": [{}]}}", values.join(", "));
    let results = run(config, &[("data.json", &body)]);
    let violations = &result(&results, "c").violations;
    // `a` and each of the 25 others.
    assert_eq!(violations.len(), 26);
    let first = &violations[0];
    assert!(
        first.message.contains("\"a\" here and in 24 other rows"),
        "{}",
        first.message
    );
    // The rows have no span: one location, at the file, and the other 48
    // of the 24 other rows with `a` and a row of each of the 25 other
    // values counted.
    assert!(first.range.is_none() && first.line.is_none(), "{first:?}");
    assert_eq!(
        (
            first.related_locations.len(),
            first.related_locations_omitted
        ),
        (1, 48),
        "{first:?}"
    );
    for other in &violations[1..] {
        assert_eq!(
            (
                other.related_locations.len(),
                other.related_locations_omitted
            ),
            (1, 24),
            "{other:?}"
        );
    }
}

// --- Missing is not null -------------------------------------------------------

/// Relation `p` reading `$.v` of each `p/*.json` as `v`, beside its path as
/// `src`, and rule `c` asserting `assert`.
fn read_v(assert: &str) -> String {
    format!(
        "version: 2\nrelations:\n  p:\n    from: {{ paths: \"p/*.json\" }}\n    read: {{ json: \
         \"$.v\" }}\n    fields: {{ src: path, v: value }}\nrules:\n  - id: c\n    kind: \
         constraint\n    input: p\n    assert: {assert}\n    level: error\n"
    )
}

const READ_V: [(&str, &str); 2] = [("p/a.json", "{\"v\": \"1\"}"), ("p/b.json", "{}")];

/// A file a `read:` matches nothing in has a missing value, which every
/// family calls missing, and a null value null.
#[test]
fn a_missing_value_is_called_missing_and_a_null_null_by_every_family() {
    let results = run(&read_v("{ equal: { field: v } }"), &READ_V);
    assert_eq!(
        findings(&results, "c"),
        [
            "p/a.json:1:7: rows disagree on `v`: \"1\" here; missing in 1 row\n  ~ a row with \
             another value: p/b.json",
            "p/b.json: rows disagree on `v`: missing here; \"1\" in 1 row (a missing value \
             matches nothing, so a row without a value agrees with no row)\n  ~ a row with \
             another value: p/a.json:1:7",
        ]
    );
    let results = run(
        &read_v("{ equal: { field: v, to: { row: { src: p/a.json } } } }"),
        &READ_V,
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "p/b.json: `v` is missing here, but \"1\" in the source row (`src` \"p/a.json\", at \
             p/a.json)\n  ~ the source row: p/a.json:1:7"
        ]
    );
    let results = run(
        &read_v("{ equal: { field: v, to: { field: src } } }"),
        &READ_V,
    );
    assert_eq!(
        findings(&results, "c")[1],
        "p/b.json: `v` is missing and `src` is \"p/b.json\" in this row; they must be equal (a \
         missing value matches nothing)\n  ~ `src` in this row: p/b.json"
    );

    // An entry's null value is null in each.
    let null = [("data.json", "{\"a\": \"1\", \"b\": null}")];
    let results = run(&top_level("{ equal: { field: v } }", ""), &null);
    assert_eq!(
        findings(&results, "c")[1],
        "data.json:1:17: rows disagree on `v`: null here; \"1\" in 1 row (null matches nothing, \
         so a row without a value agrees with no row)\n  ~ a row with another value: \
         data.json:1:7"
    );
    let results = run(
        &top_level("{ equal: { field: v, to: { row: { k: a } } } }", ""),
        &null,
    );
    assert_eq!(
        findings(&results, "c"),
        [
            "data.json:1:17: `v` is null here, but \"1\" in the source row (`k` \"a\", at `a` in \
             data.json)\n  ~ the source row: data.json:1:7"
        ]
    );
    let results = run(
        &top_level("{ equal: { field: v, to: { field: k } } }", ""),
        &null,
    );
    assert_eq!(
        findings(&results, "c")[1],
        "data.json:1:17: `v` is null and `k` is \"b\" in this row; they must be equal (null \
         matches nothing)\n  ~ `k` in this row: data.json:1:12"
    );
}

// --- A `union:` part that read nothing is named --------------------------------

/// The node-pins example of `version-2.md`: one row per place a Node version
/// is pinned, `version` written as `version`.
fn node_pins(version: &str) -> String {
    format!(
        "version: 2\nrelations:\n  node-pins:\n    union:\n      - {{ from: {{ paths: .nvmrc }}, \
         read: {{ regex: \"^v?(\\\\S+)\" }} }}\n      - {{ from: {{ paths: package.json }}, read: \
         {{ json: \"$.engines.node\" }} }}\n      - {{ from: {{ paths: package.json }}, read: {{ \
         json: \"$.devEngines.runtime.version\" }} }}\n      - {{ from: {{ paths: package.json }}, \
         read: {{ json: \"$.volta.node\" }} }}\n    fields: {{ source: path, version: {version} \
         }}\nrules:\n  - id: node-pins-agree\n    kind: constraint\n    input: node-pins\n    \
         assert: {{ equal: {{ field: version, to: {{ row: {{ source: .nvmrc }} }} }} }}\n    \
         level: error\n"
    )
}

const PINNED: [(&str, &str); 2] = [
    (".nvmrc", "v18\n"),
    ("package.json", "{\"engines\": {\"node\": \"18\"}}"),
];

/// Two parts that read nothing in one file give two findings at the file,
/// each naming its part by its place in the `union:` and its query.
#[test]
fn a_union_part_that_read_nothing_is_named() {
    let results = run(&node_pins("value"), &PINNED);
    assert_eq!(
        findings(&results, "node-pins-agree"),
        [
            "package.json: `version` is missing here (`relations.node-pins.union[2]`, \
             `read.json: $.devEngines.runtime.version`, matched nothing), but \"18\" in the \
             source row (`source` \".nvmrc\", at .nvmrc)\n  ~ the source row: .nvmrc:1:2",
            "package.json: `version` is missing here (`relations.node-pins.union[3]`, \
             `read.json: $.volta.node`, matched nothing), but \"18\" in the source row (`source` \
             \".nvmrc\", at .nvmrc)\n  ~ the source row: .nvmrc:1:2",
        ]
    );
    // A sensitive field's part is named by its place alone.
    let results = run(&node_pins("{ source: value, sensitive: true }"), &PINNED);
    let shown = findings(&results, "node-pins-agree");
    assert_eq!(shown.len(), 2);
    assert!(
        shown[0].contains("(`relations.node-pins.union[2]` matched nothing)")
            && shown[1].contains("(`relations.node-pins.union[3]` matched nothing)")
            && !shown.iter().any(|finding| finding.contains("read.json")),
        "{shown:?}"
    );
    // So do rows of a `union:` that disagree within groups.
    let results = run(
        &node_pins("value").replace(
            "{ equal: { field: version, to: { row: { source: .nvmrc } } } }",
            "{ equal: { field: version } }",
        ),
        &PINNED,
    );
    let shown = findings(&results, "node-pins-agree");
    assert!(
        shown.iter().any(|finding| finding.contains(
            "missing here (`relations.node-pins.union[2]`, `read.json: \
             $.devEngines.runtime.version`, matched nothing) and in 1 other row"
        )),
        "{shown:?}"
    );
}

// --- What the environment writes ---------------------------------------------

/// A relation whose `at:` the environment writes reads the environment: its
/// key paths stay out of the keys the rule makes, before the engine redacts
/// the findings, and a finding's place shows no end.
#[test]
fn a_relation_that_reads_the_environment_keeps_key_paths_out_of_its_keys() {
    let config = top_level("{ equal: { field: v, to: { row: { k: a } } } }", "").replace(
        "at: \"$\"",
        "at: \"{{env.ALINT_TEST_UNSET_AT | default('$')}}\"",
    );
    let files = [("data.json", "{\"a\": \"1\", \"b\": \"2\"}")];
    let (dir, loaded) = super::load_tree(&config, &files);
    let loaded = loaded.unwrap();
    let mut registry = alint_core::RuleRegistry::new();
    alint_rules::register_builtin(&mut registry);
    let rule = registry.build(&loaded.rules[0]).unwrap();
    let index = alint_core::walk(
        dir.path(),
        &alint_core::WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    let found = rule
        .evaluate(&alint_core::Context::new(dir.path(), &index))
        .unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    // The row's file, then `sensitive` for its key path `b`.
    assert_eq!(
        found[0].baseline_key.as_deref(),
        Some("5:equal6:source9:data.json9:sensitive")
    );
    assert!(
        found[0].message.contains("a key path not shown"),
        "{}",
        found[0].message
    );
    // Through the engine, every message is redacted, and the finding is at
    // its start alone.
    let results = run(&config, &files);
    let violation = &result(&results, "c").violations[0];
    assert!(violation.message.starts_with("sensitive evidence"));
    assert_eq!((violation.line, violation.column), (Some(1), Some(17)));
    assert!(violation.range.is_none());
}

/// A literal the environment writes in `assert:` redacts every message of
/// the rule, as a version 1 rule's evidence is when its configuration reads
/// the environment: the kind's own hiding of that literal (`<redacted>`)
/// shows only without the rule's spec, which no run lacks.
#[test]
fn an_environment_literal_in_assert_redacts_every_message_of_the_rule() {
    let env = |name: &str, default: &str| format!("\"{{{{env.{name} | default('{default}')}}}}\"");
    let locales = |set: &str| {
        format!(
            "version: 2\nrelations:\n  m:\n    from: {{ paths: messages.json }}\n    entries: {{ \
             format: json, at: [\"$.en\", \"$.fr\"] }}\n    fields: {{ locale: entry.parent, key: \
             entry.key }}\nrules:\n  - id: c\n    kind: constraint\n    input: m\n    assert: {{ \
             set: {{ field: key, group_by: locale, {set} }} }}\n    level: error\n"
        )
    };
    let messages = [(
        "messages.json",
        "{\"en\": {\"home\": \"Home\", \"about\": \"About\"},\n\"fr\": {\"home\": \"Accueil\"}}",
    )];
    let group = locales(&format!(
        "equal_to: {{ group: {} }}",
        env("ALINT_TEST_UNSET_REFERENCE", "en")
    ));
    let groups = locales(&format!(
        "equal_to: {{ group: en }}, groups: [{}]",
        env("ALINT_TEST_UNSET_GROUP", "es")
    ));
    let row = top_level(
        &format!(
            "{{ equal: {{ field: v, to: {{ row: {{ k: {} }} }} }} }}",
            env("ALINT_TEST_UNSET_ROW", "a")
        ),
        "",
    );
    let pins = [("data.json", "{\"a\": \"1\", \"b\": \"2\"}")];
    for (position, config, files) in [
        ("{ group: }", &group, &messages[..]),
        ("groups:", &groups, &messages[..]),
        ("to: { row: }", &row, &pins[..]),
    ] {
        let (results, ..) = run_under(config, files, None, true);
        let violations = &result(&results, "c").violations;
        assert!(!violations.is_empty(), "{position}");
        for violation in violations {
            assert!(
                violation.message.starts_with("sensitive evidence")
                    && violation.related_locations.is_empty(),
                "{position}: {violation:?}"
            );
        }
        // Without the spec, the kind hides the literal itself.
        let shown = findings(&run(config, files), "c");
        assert!(
            shown.iter().any(|finding| finding.contains("<redacted>")
                || finding.contains("the literals `to.row` names")),
            "{position}: {shown:?}"
        );
    }
}

// --- Paths in notes and labels are escaped -----------------------------------

/// A file a note names and the file that declares a relation reach messages
/// escaped, as a row's path does: a `${VAR}` value skipped in a file whose
/// name holds a right-to-left override (rule `n`), and a relation declared
/// in an `.alint.d/` fragment whose name holds one, whose rule `c` fails.
#[test]
fn a_note_and_a_relation_label_name_their_files_escaped() {
    let config = "version: 2\nrules: []\n";
    let fragment = "relations:\n  r:\n    from: { paths: \"n/*.json\" }\n    read: { json: \
                    \"$.v\" }\n    fields: { v: value, file: path }\nrules:\n  - id: c\n    \
                    kind: constraint\n    input: r\n    assert: { equal: { field: v, to: { row: \
                    { file: none.json } } } }\n    level: error\n  - id: n\n    kind: \
                    constraint\n    input: r\n    assert: { unique: { key: [v] } }\n    level: \
                    error\n";
    let files = [
        (".alint.d/x\u{202e}.yml", fragment),
        ("n/a\u{202e}.json", "{\"v\": \"${VAR}\"}"),
        ("n/b.json", "{\"v\": \"x\"}"),
    ];
    let results = run(config, &files);
    let shown: Vec<&str> = ["c", "n"]
        .into_iter()
        .flat_map(|id| {
            let found = result(&results, id);
            found
                .violations
                .iter()
                .chain(&found.notes)
                .map(|violation| violation.message.as_ref())
                .chain(
                    found
                        .analysis_failures
                        .iter()
                        .map(|failure| failure.message.as_ref()),
                )
        })
        .collect();
    assert!(
        shown.iter().any(|message| message
            .starts_with("relation `r` (.alint.d/x\\u{202e}.yml): `assert.equal.to.row`")),
        "{shown:?}"
    );
    assert!(
        shown
            .iter()
            .any(|message| message.starts_with("n/a\\u{202e}.json: skipped non-literal value")),
        "{shown:?}"
    );
    for message in shown {
        assert!(!message.contains('\u{202e}'), "{message:?}");
    }
}
