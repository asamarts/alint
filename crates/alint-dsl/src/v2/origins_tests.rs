//! Unit tests of the field-origins marker (`v2/origins.rs`).

use ayaml::{Mapping, Value};

use super::*;

fn mapping(yaml: &str) -> Mapping {
    alint_core::yaml::from_str::<Value>(yaml)
        .expect("parse")
        .as_mapping()
        .expect("a mapping")
        .clone()
}

/// The configuration files' indices of `base.yml` and `.alint.yml`.
const BASE: usize = 0;
const ROOT: usize = 1;

fn origins(mapping: &mut Mapping) -> Vec<(String, usize)> {
    let mut origins = take_field_origins(mapping);
    origins.sort();
    origins
}

fn at(field: &str, source: usize) -> (String, usize) {
    (field.to_string(), source)
}

#[test]
fn every_field_of_a_version_two_source_is_marked_as_written_by_it() {
    let mut rules = vec![mapping(
        "id: a\nkind: k\ninput: deps\n__alint_internal_x: 1\n",
    )];
    mark_field_origins(&mut rules, ROOT);
    assert_eq!(
        origins(&mut rules[0]),
        vec![at("id", ROOT), at("input", ROOT), at("kind", ROOT),],
        "an internal marker is no field"
    );
    assert!(
        !rules[0].contains_key(FIELD_ORIGINS_MARKER),
        "taking removes it"
    );
}

#[test]
fn a_later_source_takes_the_origin_of_each_field_it_writes() {
    let mut base = vec![mapping("id: a\nkind: k\ninput: deps\nlevel: error\n")];
    mark_field_origins(&mut base, BASE);
    let mut local = vec![mapping("id: a\ninput: base.deps\n")];
    mark_field_origins(&mut local, ROOT);
    let mut merged = base.remove(0);
    merge_field_origins(&mut merged, &local[0]);
    assert_eq!(
        origins(&mut merged),
        vec![
            at("id", ROOT),
            at("input", ROOT),
            at("kind", BASE),
            at("level", BASE),
        ]
    );
}

#[test]
fn a_field_a_version_one_source_writes_loses_its_origin() {
    let mut base = vec![mapping("id: a\nkind: k\ninput: deps\n")];
    mark_field_origins(&mut base, BASE);
    let mut merged = base.remove(0);
    merge_field_origins(&mut merged, &mapping("id: a\ninput: other\n"));
    assert_eq!(origins(&mut merged), vec![at("kind", BASE)]);

    // A version 1 field overridden by a version 2 source takes its origin.
    let mut merged = mapping("id: a\nkind: k\ninput: x\n");
    let mut local = vec![mapping("input: deps\n")];
    mark_field_origins(&mut local, ROOT);
    merge_field_origins(&mut merged, &local[0]);
    assert_eq!(origins(&mut merged), vec![at("input", ROOT)]);
}

#[test]
fn version_one_mappings_stay_unmarked() {
    let mut merged = mapping("id: a\nkind: k\n");
    merge_field_origins(&mut merged, &mapping("id: a\nlevel: error\n"));
    assert!(!merged.contains_key(FIELD_ORIGINS_MARKER));
    assert_eq!(take_field_origins(&mut merged), Vec::new());
}

#[test]
fn only_the_constraint_kind_and_the_stub_have_a_reference_field() {
    assert_eq!(reference_fields(REFERENCE_STUB_KIND), &["input"]);
    assert_eq!(reference_fields("constraint"), &["input"]);
    assert_eq!(reference_fields("file_exists"), &[] as &[&str]);
    // Every other kind of the catalogue has none.
    let registry = alint_rules::builtin_registry();
    for kind in registry.known_kinds() {
        if kind != "constraint" {
            assert_eq!(reference_fields(kind), &[] as &[&str], "{kind}");
        }
    }
}
