use super::*;
use crate::relation::Value;

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

#[test]
fn leaves_stop_at_arrays_and_empty_objects() {
    let text = r#"{"en": {"nav": {"home": "Home", "empty": {}}, "days": ["Mo", {"x": 1}]}}"#;
    let output = run(Format::Json, &["$"], true, &[], text).unwrap();
    assert_eq!(
        rows(&output),
        [
            (None, "en.nav.home".into(), string("Home")),
            (None, "en.nav.empty".into(), error(OBJECT)),
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
