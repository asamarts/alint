//! Which `constraint` rules run under `--changed` (design
//! `v0.23/selectors-and-constraints.md` section 9): a rule's trigger scope is
//! the union of the selectors of every relation its plan reads, followed
//! through joins, membership filters and a `set`'s other relation, and
//! narrowed by each selector's `scope_filter:` as the selector is. A change
//! to what a predicate consults (a marker, a manifest) triggers too. The
//! command line is tested in `crates/alint/tests/constraint_changed_cli.rs`.

use std::collections::HashSet;
use std::path::PathBuf;

use alint_core::{FileIndex, WalkOptions, walk};

use super::constraint::load_tree;

const CONFIG: &str = r#"version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
  marked:
    paths: "**/module.json"
    scope_filter: { has_ancestor: module.marker }
  members:
    paths: "workspace/**/member.json"
    scope_filter:
      include_manifest_paths:
        source: members.json
        extract: { json: "$.members[*]" }
relations:
  names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value }
  allowed:
    from: { paths: allowed.txt }
    read: { lines: {} }
    fields: { name: value }
  banned:
    from: { paths: banned.txt }
    read: { lines: {} }
    fields: { name: value }
  kept:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value }
    where:
      name: { in: [{ relation: allowed, field: name }] }
  dropped:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value }
    where:
      name: { not_in: [{ relation: banned, field: name }] }
  owners:
    from: { paths: "owners/*.json" }
    read: { json: "$.package" }
    fields: { package: value }
  linked:
    join: { left: names, right: owners, on: { name: package }, unique: right }
    fields: { name: left.name, package: right.package }
  catalog:
    from: { paths: catalog.json }
    entries: { format: json, at: "$.packages" }
    fields: { name: entry.key }
  either:
    union:
      - { from: { paths: "a/*.txt" }, read: { lines: {} } }
      - { from: { paths: { include: "b/**/*.txt", exclude: "b/skip/**" } }, read: { lines: {} } }
    fields: { line: value }
  modules:
    from: marked
    read: { json: "$.name" }
    fields: { name: value }
  workspace:
    from: members
    read: { json: "$.name" }
    fields: { name: value }
rules:
  - { id: input, kind: constraint, input: names, assert: { unique: { key: [name] } }, level: error }
  - { id: join, kind: constraint, input: linked, assert: { unique: { key: [name] } }, level: error }
  - { id: in, kind: constraint, input: kept, assert: { unique: { key: [name] } }, level: error }
  - { id: not-in, kind: constraint, input: dropped, assert: { unique: { key: [name] } }, level: error }
  - id: set
    kind: constraint
    input: names
    assert: { set: { field: name, subset_of: { relation: catalog, field: name } } }
    level: error
  - { id: union, kind: constraint, input: either, assert: { unique: { key: [line] } }, level: error }
  - { id: marked, kind: constraint, input: modules, assert: { unique: { key: [name] } }, level: error }
  - id: members
    kind: constraint
    input: workspace
    assert: { unique: { key: [name] } }
    level: error
"#;

const TREE: &[(&str, &str)] = &[
    ("packages/a/package.json", r#"{"name": "a"}"#),
    ("packages/b/package.json", r#"{"name": "b"}"#),
    ("allowed.txt", "a\n"),
    ("banned.txt", "b\n"),
    ("owners/a.json", r#"{"package": "a"}"#),
    ("catalog.json", r#"{"packages": {"a": {}, "b": {}}}"#),
    ("a/one.txt", "x\n"),
    ("b/two.txt", "y\n"),
    ("b/skip/three.txt", "y\n"),
    ("mods/module.marker", ""),
    ("mods/a/module.json", r#"{"name": "a"}"#),
    ("loose/module.json", r#"{"name": "loose"}"#),
    ("members.json", r#"{"members": ["workspace/w1"]}"#),
    ("workspace/w1/member.json", r#"{"name": "w1"}"#),
    ("workspace/w2/member.json", r#"{"name": "w2"}"#),
    ("src/main.rs", "fn main() {}\n"),
];

/// The ids of the rules of `config` that run over `files` when `changed`
/// changed, in config order, each with whether it reported an analysis
/// failure.
fn ran(config: &str, files: &[(&str, &str)], changed: &[&str]) -> Vec<(String, bool)> {
    let (dir, loaded) = load_tree(config, files);
    let config = loaded.unwrap_or_else(|error| panic!("the config loads: {error}"));
    let index: FileIndex = walk(
        dir.path(),
        &WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    let registry = alint_rules::builtin_registry();
    let entries = config
        .rules
        .iter()
        .map(|spec| alint_core::RuleEntry::new(registry.build(spec).unwrap()))
        .collect();
    let changed: HashSet<PathBuf> = changed.iter().map(PathBuf::from).collect();
    let engine = alint_core::Engine::from_entries(entries, registry).with_changed_paths(changed);
    let report = engine.run(dir.path(), &index).unwrap();
    report
        .results
        .iter()
        .map(|result| {
            (
                result.rule_id.to_string(),
                !result.analysis_failures.is_empty(),
            )
        })
        .collect()
}

/// No rule.
const NONE: [&str; 0] = [];

/// The ids of the rules that run when `changed` changed in [`TREE`].
fn triggered(changed: &[&str]) -> Vec<String> {
    let mut ids: Vec<String> = ran(CONFIG, TREE, changed)
        .into_iter()
        .map(|(id, failed)| {
            assert!(!failed, "{id} reported an analysis failure");
            id
        })
        .collect();
    ids.sort();
    ids
}

#[test]
fn a_change_to_the_input_selector_runs_the_rules_that_read_it() {
    assert_eq!(
        triggered(&["packages/a/package.json"]),
        ["in", "input", "join", "not-in", "set"]
    );
    // Added or deleted, a path the selector reads is a change to the rows.
    assert_eq!(
        triggered(&["packages/new/package.json"]),
        ["in", "input", "join", "not-in", "set"]
    );
}

#[test]
fn each_relation_the_plan_reads_is_in_the_trigger_scope() {
    // A join's other side.
    assert_eq!(triggered(&["owners/a.json"]), ["join"]);
    assert_eq!(triggered(&["owners/gone.json"]), ["join"]);
    // The relations an `in` and a `not_in` probe.
    assert_eq!(triggered(&["allowed.txt"]), ["in"]);
    assert_eq!(triggered(&["banned.txt"]), ["not-in"]);
    // The relation a `set` compares against.
    assert_eq!(triggered(&["catalog.json"]), ["set"]);
    // Each part of a union, with its excludes.
    assert_eq!(triggered(&["a/one.txt"]), ["union"]);
    assert_eq!(triggered(&["b/two.txt"]), ["union"]);
    assert_eq!(triggered(&["b/skip/three.txt"]), NONE);
}

#[test]
fn a_change_outside_every_selector_runs_nothing() {
    assert_eq!(triggered(&["src/main.rs"]), NONE);
    assert_eq!(triggered(&["docs/gone.md"]), NONE);
    // A configuration source alone triggers no rule, as in version 1.
    assert_eq!(triggered(&[".alint.yml"]), NONE);
}

#[test]
fn has_ancestor_narrows_the_trigger_and_a_changed_marker_triggers() {
    assert_eq!(triggered(&["mods/a/module.json"]), ["marked"]);
    // No marker above it: the selector does not read it.
    assert_eq!(triggered(&["loose/module.json"]), NONE);
    // A marker added or removed changes what the selector reads.
    assert_eq!(triggered(&["loose/module.marker"]), ["marked"]);
    assert_eq!(triggered(&["mods/module.marker"]), ["marked"]);
}

#[test]
fn manifest_membership_narrows_the_trigger_and_a_changed_manifest_triggers() {
    assert_eq!(triggered(&["workspace/w1/member.json"]), ["members"]);
    // Not a member: the selector does not read it.
    assert_eq!(triggered(&["workspace/w2/member.json"]), NONE);
    assert_eq!(triggered(&["members.json"]), ["members"]);
}

#[test]
fn a_manifest_that_cannot_be_read_keeps_every_path_its_globs_match() {
    let tree: Vec<(&str, &str)> = TREE
        .iter()
        .map(|&(path, body)| {
            if path == "members.json" {
                (path, "{")
            } else {
                (path, body)
            }
        })
        .collect();
    // The rule runs, and reports why it cannot read the selector.
    assert_eq!(
        ran(CONFIG, &tree, &["workspace/w2/member.json"]),
        [("members".to_string(), true)]
    );
}
