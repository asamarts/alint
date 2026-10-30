//! Parser-independent behavior contract for alint's user-facing YAML surfaces.
//!
//! These assertions intentionally name observable YAML semantics, not the parser
//! implementation. A future parser migration must preserve them or document and
//! approve a compatibility change.

use ayaml::Value;

#[test]
fn scalar_resolution_tags_merges_and_aliases_keep_the_supported_semantics() {
    let document: Value = ayaml::from_str(
        "plain_bool_word: yes\n\
         legacy_octal: 0123\n\
         binary: 0b11\n\
         hex: 0x1f\n\
         exponent: 1e3\n\
         date_like: 2026-10-19\n\
         null_value: ~\n\
         infinity: .inf\n\
         core_string: !!str 123\n\
         tagged: !Custom {x: 1}\n\
         base: &base {x: 1}\n\
         alias: *base\n\
         merged: {<<: *base, y: 2}\n",
    )
    .expect("contract fixture parses");

    assert_eq!(document["plain_bool_word"].as_str(), Some("yes"));
    assert_eq!(document["legacy_octal"].as_str(), Some("0123"));
    assert_eq!(document["binary"].as_u64(), Some(3));
    assert_eq!(document["hex"].as_u64(), Some(31));
    assert_eq!(document["exponent"].as_f64(), Some(1_000.0));
    assert_eq!(document["date_like"].as_str(), Some("2026-10-19"));
    assert!(document["null_value"].is_null());
    assert!(document["infinity"].as_f64().is_some_and(f64::is_infinite));
    assert_eq!(document["core_string"].as_str(), Some("123"));

    let Value::Tagged(tagged) = &document["tagged"] else {
        panic!("custom tags must remain represented in the generic value model");
    };
    assert_eq!(tagged.tag.to_string(), "!Custom");
    assert_eq!(tagged.value["x"].as_u64(), Some(1));

    assert_eq!(document["alias"], document["base"]);
    assert_eq!(document["merged"]["<<"], document["base"]);
    assert_eq!(document["merged"]["y"].as_u64(), Some(2));
}

#[test]
fn duplicate_keys_multi_document_streams_and_malformed_locations_remain_errors() {
    let duplicate = ayaml::from_str::<Value>("a: 1\na: 2\n")
        .expect_err("duplicate mapping keys are ambiguous and must fail")
        .to_string();
    assert!(
        duplicate.contains("duplicate entry with key \"a\""),
        "unexpected duplicate-key diagnostic: {duplicate}"
    );

    let multiple = ayaml::from_str::<Value>("a: 1\n---\nb: 2\n")
        .expect_err("single-document entry points must reject YAML streams")
        .to_string();
    assert!(
        multiple.contains("more than one document"),
        "unexpected multi-document diagnostic: {multiple}"
    );

    let malformed =
        ayaml::from_str::<Value>("a: [1,\n").expect_err("malformed flow sequence must fail");
    let location = malformed
        .location()
        .expect("syntax diagnostics retain a line and column");
    assert_eq!((location.line(), location.column()), (2, 1));
}
