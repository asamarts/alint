//! What a `constraint` finding's message and places show of the files it
//! reads: key paths cut and escaped as values are, `{{row.FIELD}}` escaped,
//! no end column at a sensitive cell, every related row shown or counted,
//! missing told from null, and a `union:` part named where it read nothing.

use std::fmt::Write as _;

use alint_core::relation::{LimitKind, Limits};

use alint_core::{SourceRange, Violation};

use super::{failures, result, run, run_under};

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

/// A rule over every entry of [`padded`]'s files asserting `assert`.
fn over_padded(assert: &str) -> String {
    format!(
        "version: 2\nrelations:\n  r:\n    from: {{ paths: \"data/*.json\" }}\n    entries: {{ \
         format: json, at: \"$\" }}\n    fields: {{ l: entry.value }}\nrules:\n  - id: c\n    \
         kind: constraint\n    input: r\n    assert: {assert}\n    level: error\n"
    )
}

fn run_padded(
    assert: &str,
    count: usize,
    limits: Option<&Limits>,
) -> (Vec<alint_core::RuleResult>, usize) {
    let files = padded(count);
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, body)| (path.as_str(), body.as_str()))
        .collect();
    run_under(&over_padded(assert), &files, limits, false)
}

/// Placing the findings of eight padded files holds one file's text at a
/// time, which the run's bytes take once: what the run takes for them is
/// one file's bytes, as for two files, where keeping each file and a table
/// of its lines until the rule ended held 2.4 GB for eight files of 32 MB,
/// none of it taken.
#[test]
fn placing_findings_takes_one_files_bytes_however_many_files_it_reads() {
    let (passing, relation) = run_padded("{ equal: { field: l } }", 8, None);
    assert!(result(&passing, "c").violations.is_empty());
    let (results, eight) = run_padded("{ unique: { key: [l] } }", 8, None);
    // 16 rows of `dup`: 15 repeats, in each of the eight files.
    assert_eq!(result(&results, "c").violations.len(), 15);
    // One file's text, and the offsets asked of it.
    let one_file = PADDING..PADDING + 4096;
    let placing = eight - relation;
    assert!(
        one_file.contains(&placing),
        "placing took {placing} bytes for eight files of {PADDING}"
    );
    let (_, two) = run_padded("{ unique: { key: [l] } }", 2, None);
    let (_, relation_two) = run_padded("{ equal: { field: l } }", 2, None);
    let placing = two - relation_two;
    assert!(
        one_file.contains(&placing),
        "placing took {placing} bytes for two files of {PADDING}"
    );
}

/// A run whose bytes cannot hold a file a finding sits in has no verdict:
/// the rule's analysis failure says so, as `key_parity`'s does, rather
/// than findings without their places.
#[test]
fn a_run_whose_bytes_cannot_hold_a_file_to_place_fails_the_rule() {
    let (_, taken) = run_padded("{ unique: { key: [l] } }", 8, None);
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, taken - 1)
        .unwrap();
    let (results, _) = run_padded("{ unique: { key: [l] } }", 8, Some(&limits));
    let failed = failures(&results, "c");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(
        failed[0].starts_with("data/a")
            && failed[0].contains("relation `r` (.alint.yml): relation limit `bytes_per_run`")
            && failed[0].contains(
                "so the rule reports no finding rather than findings without their places"
            ),
        "{failed:?}"
    );
    // At the bytes it took, the run holds them.
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, taken)
        .unwrap();
    let (results, _) = run_padded("{ unique: { key: [l] } }", 8, Some(&limits));
    assert_eq!(result(&results, "c").violations.len(), 15);
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
/// counted of 49 rows).
#[test]
fn related_rows_at_one_place_are_shown_once_and_counted() {
    let config = "version: 2\nrelations:\n  r:\n    from: { paths: lines.txt }\n    read: { lines: \
                  {} }\n    fields: { v: value }\nrules:\n  - id: c\n    kind: constraint\n    \
                  input: r\n    assert: { equal: { field: v } }\n    level: error\n";
    let mut body = "a\n".repeat(25);
    for index in 0..25 {
        writeln!(body, "b{index}").unwrap();
    }
    let results = run(config, &[("lines.txt", &body)]);
    let violations = &result(&results, "c").violations;
    // `a` and each of the 25 others.
    assert_eq!(violations.len(), 26);
    let first = &violations[0];
    assert!(
        first.message.contains("\"a\" here and in 24 other rows"),
        "{}",
        first.message
    );
    // 24 other rows with `a`, and a row of each of the 25 other values.
    assert_eq!(
        first.related_locations.len() + first.related_locations_omitted,
        49
    );
    for other in &violations[1..] {
        assert_eq!(
            other.related_locations.len() + other.related_locations_omitted,
            25,
            "{other:?}"
        );
    }
}
