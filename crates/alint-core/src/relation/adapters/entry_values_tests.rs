use super::*;
use crate::relation::Value;

std::thread_local! {
    /// How many member rows `Document::members` has yielded on this
    /// thread: the work an `at:` walk does.
    pub(super) static MEMBER_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn resource(path: &str) -> Resource {
    Resource::for_test(0, path)
}

fn run(
    format: Format,
    at: &[&str],
    leaves: bool,
    bound: &[&str],
    text: &str,
) -> Result<AdapterOutput, AdapterError> {
    let at: Vec<String> = at.iter().map(ToString::to_string).collect();
    let bound: Vec<String> = bound.iter().map(ToString::to_string).collect();
    EntryValuesAdapter::new(format, &at, leaves, &bound)
        .unwrap()
        .extract(&resource("doc"), text.as_bytes(), &Limits::default())
}

/// Each row as `(label, pointer, state)`.
fn rows(output: &AdapterOutput) -> Vec<(Option<String>, String, RowState)> {
    output
        .rows
        .iter()
        .map(|row| {
            (
                row.label.as_deref().map(str::to_string),
                row.pointer
                    .as_ref()
                    .map(Pointer::render)
                    .unwrap_or_default(),
                row.state.clone(),
            )
        })
        .collect()
}

fn number(lexeme: &str) -> RowState {
    RowState::Value(Value::Number(
        crate::relation::Number::from_lexeme(lexeme).unwrap(),
    ))
}

fn string(text: &str) -> RowState {
    RowState::Value(Value::String(text.into()))
}

fn error(kind: &str) -> RowState {
    RowState::Error(kind.into())
}

#[test]
fn json_members_keep_number_lexemes_and_spans() {
    let text = r#"{"deps": {"a": 1.10, "b": "^1", "c": true, "d": null, "e": -0, "f": 1e3}}"#;
    let output = run(Format::Json, &["$.deps"], false, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [
            (None, "deps.a".into(), number("1.10")),
            (None, "deps.b".into(), string("^1")),
            (None, "deps.c".into(), RowState::Value(Value::Bool(true))),
            (None, "deps.d".into(), RowState::Value(Value::Null)),
            (None, "deps.e".into(), number("-0")),
            (None, "deps.f".into(), number("1e3")),
        ]
    );
    let a = &output.rows[0];
    assert_eq!(&text[a.span.unwrap().as_range()], "\"a\"");
    assert_eq!(&text[a.value_span.unwrap().as_range()], "1.10");
    let b = &output.rows[1];
    assert_eq!(&text[b.value_span.unwrap().as_range()], "\"^1\"");
}

#[test]
fn members_follow_the_at_paths_in_order_and_each_entry_comes_once() {
    let text = r#"{"b": {"x": 1}, "a": {"y": 2}}"#;
    let output = run(Format::Json, &["$.a", "$.b", "$.a"], false, &[], text).unwrap();
    let pointers: Vec<String> = rows(&output).into_iter().map(|row| row.1).collect();
    assert_eq!(pointers, ["a.y", "b.x"]);
    // A path the document lacks selects nothing.
    let none = run(Format::Json, &["$.missing"], false, &[], text).unwrap();
    assert_eq!(rows(&none), Vec::new());
}

#[test]
fn a_member_holding_an_object_or_array_names_it() {
    let text = r#"{"a": {"o": {"k": 1}, "l": [1, 2]}}"#;
    let output = run(Format::Json, &["$.a"], false, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [
            (None, "a.o".into(), error(OBJECT)),
            (None, "a.l".into(), error(ARRAY)),
        ]
    );
}

#[test]
fn an_at_path_that_selects_no_object_is_reported_where_it_points() {
    let text = r#"{"a": [1], "s": "x"}"#;
    let output = run(Format::Json, &["$.a", "$.s"], false, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [
            (Some("at:$.a".into()), "a".into(), error(ARRAY)),
            (Some("at:$.s".into()), "s".into(), error("a string")),
        ]
    );
}

/// An object or array with nothing in it is told apart from one with
/// members or elements, in each format, and so is null: `strip-root` reads
/// a top-level key holding one as a file with no keys.
#[test]
fn a_member_holding_nothing_says_so() {
    let documents = [
        (
            Format::Json,
            r#"{"o": {}, "a": [], "n": null, "p": {"k": 1}, "q": [1], "s": ""}"#,
        ),
        (
            Format::Yaml,
            "o: {}\na: []\nn:\np: { k: 1 }\nq: [1]\ns: ''\n",
        ),
        (
            Format::Toml,
            "o = {}\na = []\np = { k = 1 }\nq = [1]\ns = ''\n",
        ),
    ];
    for (format, text) in documents {
        let output = run(format, &["$"], false, &[], text).unwrap();
        let mut expected = vec![
            (None, "o".to_string(), error(EMPTY_OBJECT)),
            (None, "a".to_string(), error(EMPTY_ARRAY)),
            (None, "n".to_string(), RowState::Value(Value::Null)),
            (None, "p".to_string(), error(OBJECT)),
            (None, "q".to_string(), error(ARRAY)),
            (None, "s".to_string(), string("")),
        ];
        if format == Format::Toml {
            // TOML has no null.
            expected.remove(2);
        }
        assert_eq!(rows(&output), expected, "{format:?}");
        let nothing: Vec<bool> = output
            .rows
            .iter()
            .map(|row| holds_nothing(&row.state))
            .collect();
        let mut held = vec![true, true, true, false, false, false];
        if format == Format::Toml {
            held.remove(2);
        }
        assert_eq!(nothing, held, "{format:?}");
    }
}

#[test]
fn leaves_stop_at_arrays_and_empty_objects() {
    let text = r#"{"en": {"nav": {"home": "Home", "empty": {}}, "days": ["Mo", {"x": 1}]}}"#;
    let output = run(Format::Json, &["$"], true, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [
            (None, "en.nav.home".into(), string("Home")),
            (None, "en.nav.empty".into(), error(EMPTY_OBJECT)),
            (None, "en.days".into(), error(ARRAY)),
        ]
    );
}

#[test]
fn yaml_scalars_are_typed_as_the_core_schema_reads_them() {
    let text = "a: 1.10\nb: '1.10'\nc: ~\nd:\ne: true\nf: 0x1F\ng: !!str 12\nh: |\n  block\ni: !!int 0x1F\n";
    let output = run(Format::Yaml, &["$"], false, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [
            (None, "a".into(), number("1.10")),
            (None, "b".into(), string("1.10")),
            (None, "c".into(), RowState::Value(Value::Null)),
            (None, "d".into(), RowState::Value(Value::Null)),
            (None, "e".into(), RowState::Value(Value::Bool(true))),
            (None, "f".into(), number("0x1F")),
            (None, "g".into(), string("12")),
            (None, "h".into(), string("block\n")),
            (None, "i".into(), error(UNREADABLE)),
        ]
    );
    // A block scalar has no value span; a plain one has.
    let h = &output.rows[7];
    assert_eq!(h.value_span, None);
    assert_eq!(&text[output.rows[0].value_span.unwrap().as_range()], "1.10");
}

#[test]
fn yaml_aliases_merge_keys_and_complex_keys_are_parse_failures() {
    for (text, message) in [
        (
            "a: &x 1\nb: *x\n",
            "YAML aliases are not supported for key extraction",
        ),
        (
            "base: &b {x: 1}\nc:\n  <<: *b\n",
            "YAML merge keys are not supported for key extraction",
        ),
        (
            "? [a, b]\n: 1\n",
            "YAML complex (non-scalar) mapping keys are not supported for key extraction",
        ),
    ] {
        assert_eq!(
            run(Format::Yaml, &["$"], false, &[], text).unwrap_err(),
            AdapterError::Parse(message.into()),
            "{text}"
        );
    }
}

#[test]
fn toml_values_keep_lexemes_and_name_datetimes() {
    let text =
        "[t]\ni = 0x1F\nu = 1_000\nf = +inf\ns = \"x\"\nd = 1979-05-27\na = [1]\nn = { k = 1 }\n";
    let output = run(Format::Toml, &["$.t"], false, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [
            (None, "t.i".into(), number("0x1F")),
            (None, "t.u".into(), number("1_000")),
            (None, "t.f".into(), number("+inf")),
            (None, "t.s".into(), string("x")),
            (None, "t.d".into(), error(DATETIME)),
            (None, "t.a".into(), error(ARRAY)),
            (None, "t.n".into(), error(OBJECT)),
        ]
    );
    assert_eq!(&text[output.rows[0].value_span.unwrap().as_range()], "0x1F");
    // A date selected by `at:` is not an object.
    let at_date = run(Format::Toml, &["$.t.d"], false, &[], text).unwrap();
    assert_eq!(
        rows(&at_date),
        [(Some("at:$.t.d".into()), "t.d".into(), error(DATETIME))]
    );
}

#[test]
fn a_toml_table_in_an_array_of_tables_is_not_read_as_entries() {
    let text = "[[bin]]\nname = \"a\"\n";
    let output = run(Format::Toml, &["$.bin[0]"], false, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [(
            Some("at:$.bin[0]".into()),
            "bin[0]".into(),
            error(ARRAY_TABLE)
        )]
    );
}

#[test]
fn bound_paths_report_each_match_up_to_two() {
    let label = |query: &str| Some(format!("{BOUND_LABEL}{query}"));
    let text = r#"{"name": "app", "list": ["x", 1], "dup": 1, "dup": 2}"#;
    let output = run(
        Format::Json,
        &[],
        false,
        &[
            "$.name",
            "$.missing",
            "$.list[0]",
            "$.list[1]",
            "$.dup",
            "$.list[*]",
        ],
        text,
    )
    .unwrap();
    assert_eq!(
        rows(&output),
        [
            (label("$.name"), "name".into(), string("app")),
            (label("$.list[0]"), "list[0]".into(), string("x")),
            (label("$.list[1]"), "list[1]".into(), error(ARRAY_NUMBER)),
            (label("$.dup"), "dup".into(), number("1")),
            (label("$.dup"), "dup".into(), number("2")),
            (label("$.list[*]"), "list[0]".into(), string("x")),
            (label("$.list[*]"), "list[1]".into(), error(ARRAY_NUMBER)),
        ]
    );
}

#[test]
fn the_row_limit_bounds_the_walk() {
    let limits = Limits::default()
        .with(LimitKind::RowsPerExtractor, 2)
        .unwrap();
    let adapter = EntryValuesAdapter::new(Format::Json, &["$".into()], false, &[]).unwrap();
    let error = adapter
        .extract(&resource("doc"), br#"{"a": 1, "b": 2, "c": 3}"#, &limits)
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "relation limit `rows_per_extractor` (2) reached: 3 rows or values"
    );
}

#[test]
fn the_id_holds_the_specification() {
    let one = EntryValuesAdapter::new(Format::Json, &["$.a".into()], false, &[]).unwrap();
    let other = EntryValuesAdapter::new(Format::Json, &["$.b".into()], false, &[]).unwrap();
    let leaves = EntryValuesAdapter::new(Format::Json, &["$.a".into()], true, &[]).unwrap();
    assert_eq!(one.id(), r#"entry-values-json:["$.a"]:false:[]"#);
    assert_ne!(one.id(), other.id());
    assert_ne!(one.id(), leaves.id());
    assert!(EntryValuesAdapter::new(Format::Dotenv, &[], false, &[]).is_err());
}

/// Each row as `(label, pointer, state)` with the spans left out, for
/// comparing formats.
fn shapes(
    format: Format,
    at: &[&str],
    leaves: bool,
    text: &str,
) -> Vec<(Option<String>, String, RowState)> {
    rows(&run(format, at, leaves, &[], text).unwrap())
}

#[test]
fn a_dotted_key_in_a_toml_inline_table_reads_as_json_s_nested_object() {
    let toml = "[en]\nnav = { home.title = \"Home\", about = \"About\" }\n";
    let json = r#"{"en": {"nav": {"home": {"title": "Home"}, "about": "About"}}}"#;
    for (at, leaves) in [
        ("$", true),
        ("$", false),
        ("$.en.nav", false),
        ("$.en.nav.home", false),
    ] {
        assert_eq!(
            shapes(Format::Toml, &[at], leaves, toml),
            shapes(Format::Json, &[at], leaves, json),
            "at {at}, leaves {leaves}"
        );
    }
    assert_eq!(
        shapes(Format::Toml, &["$"], true, toml),
        [
            (None, "en.nav.home.title".into(), string("Home")),
            (None, "en.nav.about".into(), string("About")),
        ]
    );
    assert_eq!(
        shapes(Format::Toml, &["$.en.nav"], false, toml),
        [
            (None, "en.nav.home".into(), error(OBJECT)),
            (None, "en.nav.about".into(), string("About")),
        ]
    );
    // A dotted key's table is a table, not one in an array of tables, and the
    // key above it is not a leaf.
    let dotted = "a = { b.c = \"x\" }\n";
    let nested = r#"{"a": {"b": {"c": "x"}}}"#;
    for (at, leaves) in [("$.a", false), ("$", true), ("$.a.b", false)] {
        assert_eq!(
            shapes(Format::Toml, &[at], leaves, dotted),
            shapes(Format::Json, &[at], leaves, nested),
            "at {at}, leaves {leaves}"
        );
    }
    assert_eq!(
        shapes(Format::Toml, &["$.a"], false, dotted),
        [(None, "a.b".into(), error(OBJECT))]
    );
    // The middle key's span is its own.
    let output = run(Format::Toml, &["$.en.nav"], false, &[], toml).unwrap();
    assert_eq!(&toml[output.rows[0].span.unwrap().as_range()], "home");
}

#[test]
fn a_null_document_has_no_entries_and_a_scalar_one_fails() {
    for text in [
        "",
        "# only a comment\n",
        "---\n",
        "~\n",
        "null\n",
        "--- # c\n...\n",
    ] {
        for leaves in [false, true] {
            assert_eq!(
                shapes(Format::Yaml, &["$"], leaves, text),
                [],
                "{text:?}, leaves {leaves}"
            );
        }
    }
    assert_eq!(shapes(Format::Json, &["$"], false, "null"), []);
    assert_eq!(
        shapes(Format::Yaml, &["$"], false, "hello\n"),
        [(Some("at:$".into()), String::new(), error("a string"))]
    );
    // A null below the root is selected, and is not an object.
    assert_eq!(
        shapes(Format::Yaml, &["$.fr"], false, "fr:\n"),
        [(Some("at:$.fr".into()), "fr".into(), error("null"))]
    );
}

/// The tree holds an object's members in key order; an `at:` path that
/// selects several objects visits them in the document's.
#[test]
fn selected_objects_are_visited_in_document_order() {
    let documents = [
        (
            Format::Json,
            r#"{"z": {"a": 1}, "b": {"c": 2}, "m": {"d": 3}}"#,
        ),
        (Format::Yaml, "z:\n  a: 1\nb:\n  c: 2\nm:\n  d: 3\n"),
        (Format::Toml, "[z]\na = 1\n[b]\nc = 2\n[m]\nd = 3\n"),
    ];
    for (format, text) in documents {
        let pointers = |at: &str, leaves: bool| -> Vec<String> {
            shapes(format, &[at], leaves, text)
                .into_iter()
                .map(|row| row.1)
                .collect()
        };
        assert_eq!(pointers("$.*", false), ["z.a", "b.c", "m.d"], "{format:?}");
        assert_eq!(pointers("$", true), ["z.a", "b.c", "m.d"], "{format:?}");
    }
    // An array's elements keep their order, and an object in one sits where
    // the array does.
    let text = r#"{"z": [{"y": 1}, {"x": 2}], "b": {"c": 3}}"#;
    let pointers: Vec<String> = shapes(Format::Json, &["$.z[*]", "$.b"], false, text)
        .into_iter()
        .map(|row| row.1)
        .collect();
    assert_eq!(pointers, ["z[0].y", "z[1].x", "b.c"]);
    // A `bound:` path's matches, the first two of which are rows, too.
    let text = r#"{"z": {"v": 1}, "b": {"v": 2}, "m": {"v": 3}}"#;
    let bound = run(Format::Json, &[], false, &["$.*.v"], text).unwrap();
    let pointers: Vec<String> = rows(&bound).into_iter().map(|row| row.1).collect();
    assert_eq!(pointers, ["z.v", "b.v"]);
}

/// `$..*` with `leaves: true` selects every object of a document 127 levels
/// deep and 50,000 keys wide, each inside the last: each is walked once, not
/// once per object it is inside.
#[test]
fn a_leaves_walk_visits_each_member_once_however_many_paths_select_it() {
    let (depth, width) = (125, 50_000);
    let mut text = "{\"a\":".repeat(depth);
    text.push('{');
    let leaves: Vec<String> = (0..width).map(|key| format!("\"k{key}\":{{}}")).collect();
    text.push_str(&leaves.join(","));
    text.push_str(&"}".repeat(depth + 1));
    MEMBER_STEPS.with(|steps| steps.set(0));
    let limits = Limits::default()
        .with(LimitKind::KeyPathBytesPerDecode, 128 << 20)
        .unwrap();
    let adapter = EntryValuesAdapter::new(Format::Json, &["$..*".into()], true, &[]).unwrap();
    let output = adapter
        .extract(&resource("doc"), text.as_bytes(), &limits)
        .unwrap();
    let steps = MEMBER_STEPS.with(std::cell::Cell::get);
    assert_eq!(output.rows.len(), width);
    let entries = depth + width;
    assert!(
        steps <= 2 * entries,
        "{steps} member steps for {entries} entries"
    );
}

/// Only the rows an adapter emits build a key path, and each one's bytes
/// are charged as it is built: a deep, wide document fails at the run's
/// bytes when its leaves are emitted, and costs nothing when no `at:` path
/// selects them.
#[test]
fn emitted_rows_charge_their_key_paths_as_they_are_built() {
    let mut text = "{\"a\":".repeat(100);
    text.push('{');
    let leaves: Vec<String> = (0..2_000).map(|key| format!("\"k{key}\":0")).collect();
    text.push_str(&leaves.join(","));
    text.push_str(&"}".repeat(101));
    let limits = Limits::default()
        .with(LimitKind::BytesPerRun, 256 * 1024)
        .unwrap();
    let extract = |at: &str, leaves: bool| {
        EntryValuesAdapter::new(Format::Json, &[at.into()], leaves, &[])
            .unwrap()
            .extract(&resource("doc"), text.as_bytes(), &limits)
    };
    let Err(AdapterError::Limit(limit)) = extract("$", true) else {
        panic!("the leaves' key paths were not charged");
    };
    assert_eq!(limit.kind, LimitKind::BytesPerRun);
    assert!(limit.reached <= 2 * 256 * 1024, "{limit}");
    assert_eq!(extract("$.version", false).unwrap().rows.len(), 0);
}

/// A block scalar key has no token span: its row sits where its text
/// starts rather than being dropped.
#[test]
fn a_yaml_block_scalar_key_is_a_row() {
    let text = "deps:\n  ? |\n    block\n  : 1\n  plain: 2\n";
    let output = run(Format::Yaml, &["$.deps"], false, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [
            (None, "deps.block\n".into(), number("1")),
            (None, "deps.plain".into(), number("2")),
        ]
    );
    let block = output.rows[0].span.unwrap();
    assert!(
        block.is_empty() && text[block.start..].starts_with("block"),
        "{block:?}"
    );
}
