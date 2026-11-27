//! What the `constraint` kind's messages and places carry into each output
//! format: a key or value holding a line break, an escape or a bidi
//! override is shown escaped, in a generated message and through
//! `{{row.FIELD}}`, so it can neither add a line to human output nor carry
//! raw control and bidi characters into JSON; and a place at a sensitive
//! cell shows its start alone, since its end would give away the value's
//! length.

use std::path::Path;
use std::process::Output;

use serde_json::Value;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(dir: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("COLUMNS", "200")
        .output()
        .unwrap()
}

fn json(dir: &Path, format: &str) -> Value {
    serde_json::from_slice(&run(dir, &["check", "--format", format]).stdout).unwrap()
}

/// Rule `id`'s JSON violations.
fn violations<'j>(json: &'j Value, id: &str) -> &'j Vec<Value> {
    json["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["id"] == id)
        .unwrap_or_else(|| panic!("no result for {id}"))["violations"]
        .as_array()
        .unwrap()
}

/// Rule `generated` with the kind's own message, and rule `custom` with a
/// `message:` showing the reported row's key and value, over `data.json`'s
/// top-level entries, whose values repeat.
const ESCAPES: &str = r#"version: 2
relations:
  r:
    from: { paths: data.json }
    entries: { format: json, at: "$" }
    fields: { k: entry.key, v: entry.value }
rules:
  - id: generated
    kind: constraint
    input: r
    assert: { unique: { key: [v] } }
    level: error
  - id: custom
    kind: constraint
    input: r
    assert: { unique: { key: [v] } }
    message: "{{row.k}} holds {{row.v}}"
    level: error
"#;

/// A first key and a repeated value that each try to forge a related line
/// (one word, so that wrapping cannot start a line with it) and hold an
/// escape and a right-to-left override.
const FORGING: &str = "{\"a\\nrelated:forged\\u001b[31m\u{202e}\": \"v\\nrelated:forged\u{202e}\", \
                       \"b\u{202e}\": \"v\\nrelated:forged\u{202e}\"}";

#[test]
fn a_key_path_and_row_text_reach_human_and_json_output_escaped() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".alint.yml", ESCAPES);
    write(dir.path(), "data.json", FORGING);

    let human = run(dir.path(), &["check"]);
    assert_eq!(human.status.code(), Some(1));
    let human = String::from_utf8(human.stdout).unwrap();
    for escaped in [
        "`['a\\nrelated:forged\\u{1b}[31m\\u{202e}']`",
        "b\\u{202e} holds v\\nrelated:forged\\u{202e}",
    ] {
        assert!(human.contains(escaped), "{escaped} in\n{human}");
    }
    for line in human.lines() {
        assert!(
            !line.trim_start().starts_with("related:forged"),
            "a forged line: {line:?} in\n{human}"
        );
        assert!(
            !line.chars().any(|c| c == '\u{1b}' || c == '\u{202e}'),
            "{line:?}"
        );
    }

    let json = json(dir.path(), "json");
    let messages: Vec<&str> = ["generated", "custom"]
        .iter()
        .flat_map(|id| violations(&json, id))
        .map(|violation| violation["message"].as_str().unwrap())
        .collect();
    assert_eq!(
        messages,
        [
            "`v` \"v\\nrelated:forged\\u{202e}\" is not unique: the first row with it is \
             `['a\\nrelated:forged\\u{1b}[31m\\u{202e}']` in data.json",
            "b\\u{202e} holds v\\nrelated:forged\\u{202e}",
        ]
    );
    for message in messages {
        assert!(
            !message.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "{message:?}"
        );
    }

    file_names_reach_human_json_and_sarif_output_escaped();
}

/// Rule `read` over `d/*.json`'s `$.v` values, a `read:` row naming its
/// file alone, and rule `entries` over the same files' `v` entries, a row
/// naming its key path in its file; and rule `types` over the entries of
/// `t/*.json`, whose `v` holds a string in one file and a number in the
/// other, an analysis failure naming both.
const FILE_NAMES: &str = r#"version: 2
relations:
  read:
    from: { paths: "d/*.json" }
    read: { json: "$.v" }
    fields: { v: value }
  entries:
    from: { paths: "d/*.json" }
    entries: { format: json, at: "$" }
    fields: { v: entry.value }
  types:
    from: { paths: "t/*.json" }
    entries: { format: json, at: "$" }
    fields: { v: entry.value }
rules:
  - id: read
    kind: constraint
    input: read
    assert: { unique: { key: [v] } }
    level: error
  - id: entries
    kind: constraint
    input: entries
    assert: { unique: { key: [v] } }
    level: error
  - id: types
    kind: constraint
    input: types
    assert: { unique: { key: [v] } }
    level: error
"#;

/// A file whose name holds a right-to-left override, and on Unix one whose
/// name holds an escape (Windows refuses control characters in file names),
/// each the first row of a value another file repeats, and of a type
/// another file's value is not: each message names the file escaped, as
/// `{{row.FIELD}}` shows its path, in human, JSON and SARIF output.
fn file_names_reach_human_json_and_sarif_output_escaped() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), ".alint.yml", FILE_NAMES);
    let mut names = vec![("a\u{202e}", "a\\u{202e}")];
    if cfg!(unix) {
        names.push(("b\u{1b}[31m", "b\\u{1b}[31m"));
    }
    for (index, (name, _)) in names.iter().enumerate() {
        let value = format!("{{\"v\": \"x{index}\"}}");
        write(dir.path(), &format!("d/{name}.json"), &value);
        write(dir.path(), &format!("d/z{index}.json"), &value);
    }
    write(
        dir.path(),
        &format!("t/{}.json", names[0].0),
        "{\"v\": \"1\"}",
    );
    write(dir.path(), "t/z.json", "{\"v\": 1}");

    let mut expected = Vec::new();
    for (index, (_, shown)) in names.iter().enumerate() {
        expected.push(format!(
            "`v` \"x{index}\" is not unique: the first row with it is d/{shown}.json"
        ));
        expected.push(format!(
            "`v` \"x{index}\" is not unique: the first row with it is `v` in d/{shown}.json"
        ));
    }
    let types = format!(
        "relation `types` (.alint.yml): `assert.unique.key`: `v` holds a string at `v` in \
         t/{}.json and a number at `v` in t/z.json; the comparison would cross types, so the \
         assertion has no verdict",
        names[0].1
    );

    let raw = |text: &str| text.chars().any(|c| c.is_control() || c == '\u{202e}');
    let human = run(dir.path(), &["check"]);
    let human = String::from_utf8(human.stdout).unwrap();
    for line in human.lines() {
        assert!(!raw(line), "{line:?} in\n{human}");
    }
    let json = json(dir.path(), "json");
    let mut messages: Vec<String> = ["read", "entries"]
        .iter()
        .flat_map(|id| violations(&json, id))
        .map(|violation| violation["message"].as_str().unwrap().to_string())
        .collect();
    messages.sort();
    expected.sort();
    assert_eq!(messages, expected);
    let failures: Vec<&str> = json["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["id"] == "types")
        .unwrap()["analysis_failures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|failure| failure["message"].as_str().unwrap())
        .collect();
    assert_eq!(failures, [types.as_str()]);
    let sarif = json_sarif(dir.path());
    let texts: Vec<&str> = sarif["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["message"]["text"].as_str().unwrap())
        .collect();
    assert!(texts.len() >= expected.len(), "{texts:?}");
    for text in texts {
        assert!(!raw(text), "{text:?}");
    }
}

/// `data.json`'s two entries share a value, of field `v`, sensitive when
/// `sensitive`.
fn secrets(dir: &Path, sensitive: bool) {
    let v = if sensitive {
        "{ source: entry.value, sensitive: true }"
    } else {
        "entry.value"
    };
    write(
        dir,
        ".alint.yml",
        &format!(
            "version: 2\nrelations:\n  r:\n    from: {{ paths: data.json }}\n    entries: {{ \
             format: json, at: \"$\" }}\n    fields: {{ k: entry.key, v: {v} }}\nrules:\n  - id: \
             c\n    kind: constraint\n    input: r\n    assert: {{ unique: {{ key: [v] }} }}\n    \
             level: error\n"
        ),
    );
    write(dir, "data.json", "{\"a\": \"secret\", \"b\": \"secret\"}");
}

/// A related location at a sensitive cell has no length in JSON (its end is
/// its start), and the finding's SARIF region no end, where a value that is
/// not sensitive keeps both ends.
#[test]
fn a_place_at_a_sensitive_cell_shows_no_length_in_json_or_sarif() {
    for sensitive in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        secrets(dir.path(), sensitive);
        let json = json(dir.path(), "json");
        let found = violations(&json, "c");
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(
            (&found[0]["line"], &found[0]["column"]),
            (&1.into(), &22.into())
        );
        let related = &found[0]["related_locations"][0];
        assert_eq!(
            (&related["line"], &related["column"]),
            (&1.into(), &7.into())
        );
        let end = (&related["end_line"], &related["end_column"]);
        let sarif = json_sarif(dir.path());
        let region = &sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startColumn"], 22);
        if sensitive {
            assert_eq!(end, (&1.into(), &7.into()), "no length: {related}");
            assert!(region.get("endColumn").is_none(), "{region}");
        } else {
            assert_eq!(end, (&1.into(), &15.into()), "{related}");
            assert_eq!(region["endColumn"], 30, "{region}");
        }
    }
}

fn json_sarif(dir: &Path) -> Value {
    json(dir, "sarif")
}

/// A `constraint` note's message starts with its file's path, escaped;
/// `--show-notes` prints the path once, `/`-separated on every platform (a
/// note's path is the resource's), escaped for the terminal, then the rest
/// of the message, not the path and then the message's escaped path. The
/// file's name holds an escape sequence and a line break (on Unix:
/// Windows refuses control characters in file names), a combining accent,
/// which shows as written, a Hangul filler and a right-to-left override.
#[test]
fn show_notes_names_a_note_file_once_and_escaped() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        ".alint.yml",
        "version: 2\nrelations:\n  r:\n    from: { paths: \"d/*.json\" }\n    read: { json: \
         \"$.v\" }\n    fields: { v: value }\nrules:\n  - id: n\n    kind: constraint\n    \
         input: r\n    assert: { unique: { key: [v] } }\n    level: error\n",
    );
    let (name, shown) = if cfg!(unix) {
        (
            "a\u{1b}[31m\ne\u{301}\u{3164}\u{202e}Z2.json",
            "a\\x1b[31m\\x0ae\u{301}\\u{3164}\\u{202e}Z2.json",
        )
    } else {
        (
            "ae\u{301}\u{3164}\u{202e}Z2.json",
            "ae\u{301}\\u{3164}\\u{202e}Z2.json",
        )
    };
    write(dir.path(), &format!("d/{name}"), "{\"v\": \"${VAR}\"}");
    let out = run(dir.path(), &["check", "--show-notes"]);
    let stderr = String::from_utf8(out.stderr).unwrap();
    let notes: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("  note: "))
        .collect();
    assert_eq!(
        notes,
        [format!(
            "  note: d/{shown}: skipped non-literal value \"${{VAR}}\""
        )],
        "{stderr}"
    );
}

#[test]
fn value_placeholder_truncates_escapes_single_pass_and_redacts_sensitive() {
    let dir = tempfile::tempdir().unwrap();
    let long_val = format!("{}{}", "a".repeat(80), "{{row.k}}");
    write(
        dir.path(),
        "en.json",
        &format!(
            "{{\"{long_val}\": \"1\", \"evil\\n\\u001b[31m\u{202e}\": \"2\", \"sec\": \"3\"}}"
        ),
    );
    write(dir.path(), "fr.json", "{}\n");
    write(
        dir.path(),
        ".alint.yml",
        r#"version: 2
relations:
  plain_keys:
    from: { paths: "*.json" }
    entries: { format: json, at: "$" }
    fields: { locale: stem, k: entry.key }
  secret_keys:
    from: { paths: "*.json" }
    entries: { format: json, at: "$" }
    fields:
      locale: stem
      k: { source: entry.key, sensitive: true }
rules:
  - id: plain-msg
    kind: constraint
    input: plain_keys
    assert:
      set: { field: k, group_by: locale, equal_to: { group: en } }
    message: "missing {{ value }} (not re-expanded)"
    level: error
  - id: secret-msg
    kind: constraint
    input: secret_keys
    assert:
      set: { field: k, group_by: locale, equal_to: { group: en } }
    message: "missing {{value}}"
    level: error
"#,
    );

    let parsed = json(dir.path(), "json");
    let plain_msgs: Vec<&str> = violations(&parsed, "plain-msg")
        .iter()
        .map(|v| v["message"].as_str().unwrap())
        .collect();
    // 1. Long value is truncated at 64 chars with `...` and never re-expands `{{row.k}}`.
    assert!(
        plain_msgs.iter().any(|m| m.contains(&"a".repeat(64))
            && m.contains("...")
            && !m.contains("a".repeat(65).as_str())),
        "expected 64-char truncation: {plain_msgs:?}"
    );
    // 2. Control and bidi chars are escaped in `{{value}}`.
    assert!(
        plain_msgs
            .iter()
            .any(|m| m.contains("evil\\n\\u{1b}[31m\\u{202e}")),
        "expected escaped invisible chars: {plain_msgs:?}"
    );
    // 3. Sensitive field redacts `{{value}}` to `<redacted>`.
    let secret_msgs: Vec<&str> = violations(&parsed, "secret-msg")
        .iter()
        .map(|v| v["message"].as_str().unwrap())
        .collect();
    assert_ne!(secret_msgs.len(), 0, "expected at least one finding");
    for m in &secret_msgs {
        assert_eq!(*m, "missing <redacted>", "{secret_msgs:?}");
    }
}
