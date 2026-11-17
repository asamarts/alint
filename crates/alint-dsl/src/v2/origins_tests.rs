//! Unit tests of the field-origins marker (`v2/origins.rs`).

use std::path::{Path, PathBuf};

use ayaml::{Mapping, Value};

use super::*;

fn mapping(yaml: &str) -> Mapping {
    alint_core::yaml::from_str::<Value>(yaml)
        .expect("parse")
        .as_mapping()
        .expect("a mapping")
        .clone()
}

fn origins(mapping: &mut Mapping) -> Vec<(String, PathBuf)> {
    let mut origins = take_field_origins(mapping);
    origins.sort();
    origins
}

fn at(field: &str, source: &str) -> (String, PathBuf) {
    (field.to_string(), PathBuf::from(source))
}

#[test]
fn every_field_of_a_version_two_source_is_marked_as_written_by_it() {
    let mut rules = vec![mapping(
        "id: a\nkind: k\ninput: deps\n__alint_internal_x: 1\n",
    )];
    mark_field_origins(&mut rules, Path::new("/repo/.alint.yml"));
    assert_eq!(
        origins(&mut rules[0]),
        vec![
            at("id", "/repo/.alint.yml"),
            at("input", "/repo/.alint.yml"),
            at("kind", "/repo/.alint.yml"),
        ],
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
    mark_field_origins(&mut base, Path::new("/repo/base.yml"));
    let mut local = vec![mapping("id: a\ninput: base.deps\n")];
    mark_field_origins(&mut local, Path::new("/repo/.alint.yml"));
    let mut merged = base.remove(0);
    merge_field_origins(&mut merged, &local[0]);
    assert_eq!(
        origins(&mut merged),
        vec![
            at("id", "/repo/.alint.yml"),
            at("input", "/repo/.alint.yml"),
            at("kind", "/repo/base.yml"),
            at("level", "/repo/base.yml"),
        ]
    );
}

#[test]
fn a_field_a_version_one_source_writes_loses_its_origin() {
    let mut base = vec![mapping("id: a\nkind: k\ninput: deps\n")];
    mark_field_origins(&mut base, Path::new("/repo/base.yml"));
    let mut merged = base.remove(0);
    merge_field_origins(&mut merged, &mapping("id: a\ninput: other\n"));
    assert_eq!(origins(&mut merged), vec![at("kind", "/repo/base.yml")]);

    // A version 1 field overridden by a version 2 source takes its origin.
    let mut merged = mapping("id: a\nkind: k\ninput: x\n");
    let mut local = vec![mapping("input: deps\n")];
    mark_field_origins(&mut local, Path::new("/repo/.alint.yml"));
    merge_field_origins(&mut merged, &local[0]);
    assert_eq!(origins(&mut merged), vec![at("input", "/repo/.alint.yml")]);
}

#[test]
fn version_one_mappings_stay_unmarked() {
    let mut merged = mapping("id: a\nkind: k\n");
    merge_field_origins(&mut merged, &mapping("id: a\nlevel: error\n"));
    assert!(!merged.contains_key(FIELD_ORIGINS_MARKER));
    assert_eq!(take_field_origins(&mut merged), Vec::new());
}

#[test]
fn only_the_stub_kind_has_a_reference_field_in_this_build() {
    assert_eq!(reference_fields(REFERENCE_STUB_KIND), &["input"]);
    assert_eq!(reference_fields("file_exists"), &[] as &[&str]);
    assert_eq!(reference_fields("constraint"), &[] as &[&str]);
}
