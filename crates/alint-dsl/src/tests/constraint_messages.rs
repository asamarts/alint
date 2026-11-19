//! What a `constraint` finding's message and places show of the files it
//! reads: key paths cut and escaped as values are, `{{row.FIELD}}` escaped,
//! no end column at a sensitive cell, every related row shown or counted,
//! missing told from null, and a `union:` part named where it read nothing.

use std::fmt::Write as _;

use super::{result, run};

/// A version 2 config: relation `r` reading `data.json`'s top-level entries
/// as `k` (the key) and `v` (the value), and rule `c` asserting `assert`,
/// with `extra` lines (`message:`, say) after it.
fn top_level(assert: &str, extra: &str) -> String {
    format!(
        "version: 2\nrelations:\n  r:\n    from: {{ paths: data.json }}\n    entries: {{ format: \
         json, at: \"$\" }}\n    fields: {{ k: entry.key, v: entry.value }}\nrules:\n  - id: c\n    \
         kind: constraint\n    input: r\n    assert: {assert}\n    level: error\n{extra}"
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
