//! `explain` on a `constraint` rule shows its resolved plan (design
//! `v0.23/selectors-and-constraints.md` sections 5, 9, 10 and 11), in human
//! and JSON output: the assertion, every relation it reads in evaluation
//! order with its source, reader, fields and `where:`, the config source that
//! declared each, and the selectors that trigger it under `--changed`. What
//! the environment wrote stays redacted, a sensitive field's literals show
//! as `<redacted>`, no value read from a file is shown, and a version 1
//! rule's `explain` is unchanged.

use std::path::Path;
use std::process::Output;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = crate::hermetic::command(env!("CARGO_BIN_EXE_alint"));
    command
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("COLUMNS", "200");
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().unwrap()
}

/// `explain rule` in human and JSON output.
fn explain(dir: &Path, rule: &str, env: &[(&str, &str)]) -> (String, serde_json::Value) {
    let human = run(dir, &["explain", rule], env);
    assert!(
        human.status.success(),
        "{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let json = run(dir, &["explain", rule, "--format", "json"], env);
    assert!(
        json.status.success(),
        "{}",
        String::from_utf8_lossy(&json.stderr)
    );
    (
        String::from_utf8(human.stdout).unwrap(),
        serde_json::from_slice(&json.stdout).unwrap(),
    )
}

/// The `plan:` block that ends the human output.
fn plan_block(human: &str) -> &str {
    let start = human
        .find("\nplan:")
        .unwrap_or_else(|| panic!("no plan:\n{human}"));
    &human[start + 1..]
}

/// `path` as the loader names a config source: relative to the root, with
/// the platform's separators.
fn source(path: &str) -> String {
    path.split('/')
        .fold(std::path::PathBuf::new(), |joined, part| joined.join(part))
        .display()
        .to_string()
}

/// `lines`, each ended by a newline.
fn lines(lines: &[&str]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line);
        text.push('\n');
    }
    text
}

fn manifest(name: &str, range: &str) -> String {
    format!("{{\"name\": \"{name}\", \"dependencies\": {{\"react\": \"{range}\"}}}}\n")
}

/// A selector source: a named selector with its paths, `scope_filter:` and
/// the manifest it resolved to, an `entries:` reader, transforms and
/// `where:`. The ranges the manifests hold are never shown.
#[test]
fn a_selector_source_shows_its_paths_scope_filter_and_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        r#"version: 2
selectors:
  manifests:
    paths: ["packages/*/package.json", "!packages/legacy/package.json"]
    scope_filter:
      has_ancestor: package.json
      include_manifest_paths:
        source: package.json
        extract: { json: "$.workspaces[*]" }
relations:
  ranges:
    from: manifests
    entries: { format: json, at: ["$.dependencies", "$.devDependencies"], leaves: true }
    fields: { manifest: path, dependency: entry.key, range: entry.value }
    transform: { range: [trim, lower] }
    where: { range: { not_matches: "^workspace:", not_in: ["*"] } }
rules:
  - id: one-range
    kind: constraint
    input: ranges
    assert: { unique: { key: [dependency], determines: [range] } }
    level: error
"#,
    );
    write(
        root,
        "package.json",
        "{\"name\": \"root\", \"workspaces\": [\"packages/a\", \"packages/b\"]}\n",
    );
    write(root, "packages/a/package.json", &manifest("a", "^18.2.0"));
    write(root, "packages/b/package.json", &manifest("b", "^17.0.2"));
    let (human, json) = explain(root, "one-range", &[]);
    let scope = "paths: packages/*/package.json  (excluding packages/legacy/package.json)";
    let manifests = "include_manifest_paths: package.json -> packages/a, packages/b";
    assert_eq!(
        plan_block(&human),
        lines(&[
            "plan:",
            "  assert: unique: key [dependency], determines [range]",
            "  relations, in evaluation order:",
            "    relation ranges  .alint.yml  (input)",
            "      from: selector manifests  .alint.yml",
            &format!("        {scope}"),
            "        scope_filter:",
            "          has_ancestor:  package.json",
            &format!("          {manifests}"),
            "      entries: json at $.dependencies, $.devDependencies, leaves",
            "      fields:",
            "        manifest: path",
            "        dependency: entry.key",
            "        range: entry.value  (transform: trim, lower)",
            "      where:",
            "        range not_matches \"^workspace:\"",
            "        range not_in [\"*\"]",
            "  trigger: under --changed, the rule runs on a change to a path these selectors pick, \
             to an ignore file, submodule or nested repository where they can match, or to a \
             marker or manifest their scope_filter: consults",
            "    selector manifests  .alint.yml",
            &format!("      {scope}"),
            "      scope_filter:",
            "        has_ancestor:  package.json",
            &format!("        {manifests}"),
            "  cost: linear in the rows of each relation it reads",
        ])
    );
    let selector = serde_json::json!({
        "selector": "manifests",
        "declared_in": ".alint.yml",
        "paths": {
            "include": ["packages/*/package.json"],
            "exclude": ["packages/legacy/package.json"],
        },
        "scope_filter": {
            "changed_since": null,
            "has_ancestor": ["package.json"],
            "manifest_scopes": [{
                "predicate": "include_manifest_paths",
                "source": "package.json",
                "paths": ["packages/a", "packages/b"],
            }],
        },
    });
    assert_eq!(
        json["plan"],
        serde_json::json!({
            "input": 0,
            "assertion": {
                "family": "unique",
                "text": "unique: key [dependency], determines [range]",
            },
            "relations": [{
                "id": 0,
                "name": "ranges",
                "written": ["ranges"],
                "inline": false,
                "roles": ["input"],
                "declared_in": ".alint.yml",
                "from": selector,
                "entries": {
                    "format": "json",
                    "at": ["$.dependencies", "$.devDependencies"],
                    "leaves": true,
                },
                "fields": [
                    { "name": "manifest", "source": "path", "transforms": [], "sensitive": false },
                    {
                        "name": "dependency",
                        "source": "entry.key",
                        "transforms": [],
                        "sensitive": false,
                    },
                    {
                        "name": "range",
                        "source": "entry.value",
                        "transforms": ["trim", "lower"],
                        "sensitive": false,
                    },
                ],
                "where": [
                    { "field": "range", "predicate": "not_matches", "pattern": "^workspace:" },
                    { "field": "range", "predicate": "not_in", "members": ["*"] },
                ],
            }],
            "trigger": [selector],
            "cost": "linear in the rows of each relation it reads",
        })
    );
    let json = json.to_string();
    for value in ["^18.2.0", "^17.0.2", "react"] {
        assert!(!human.contains(value), "{value}:\n{human}");
        assert!(!json.contains(value), "{value}: {json}");
    }
}

const JOINED: &str = r#"version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  joined:
    join: { left: ranges, right: names, on: { dependency: name }, unique: right }
    fields: { dependency: left.dependency, range: left.range, manifest: right.manifest }
  ranges:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { dependency: entry.key, range: entry.value }
  names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value, manifest: path }
rules:
  - id: one-range
    kind: constraint
    input: joined
    assert: { unique: { key: [dependency], determines: [range] } }
    level: error
"#;

/// A join: both sides before it, in evaluation order, the join with its
/// `left`, `right`, `on` and `unique`, and the trigger through both sides.
/// A walk that reads no ignore files gives a change to one no say, so the
/// trigger names none: with `respect_gitignore: false`, and with
/// `--no-gitignore`.
#[test]
fn the_trigger_names_ignore_files_only_when_the_walk_reads_them() {
    let config = "version: 2\nrelations:\n  names:\n    from: { paths: \"packages/*/package.json\" }\n    \
                  read: { json: \"$.name\" }\n    fields: { name: value }\nrules:\n  - id: names\n    \
                  kind: constraint\n    input: names\n    assert: { unique: { key: [name] } }\n    \
                  level: error\n";
    let trigger = |ignore_file: &str| {
        format!(
            "  trigger: under --changed, the rule runs on a change to a path these selectors \
             pick, to {ignore_file}submodule or nested repository where they can match, or to \
             a marker or manifest their scope_filter: consults\n"
        )
    };
    let read = trigger("an ignore file, ");
    let unread = trigger("a ");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "packages/a/package.json", &manifest("a", "1.0.0"));
    for (config, args, expected) in [
        (config.to_string(), &[][..], &read),
        (config.to_string(), &["--no-gitignore"][..], &unread),
        (
            format!("respect_gitignore: false\n{config}"),
            &[][..],
            &unread,
        ),
    ] {
        write(root, ".alint.yml", &config);
        let mut command = args.to_vec();
        command.extend(["explain", "names"]);
        let output = run(root, &command, &[]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let human = String::from_utf8(output.stdout).unwrap();
        assert!(
            plan_block(&human).contains(expected.as_str()),
            "{args:?}: {human}"
        );
    }
}

#[test]
fn a_join_shows_both_sides_first_and_its_keys() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".alint.yml", JOINED);
    write(root, "packages/a/package.json", &manifest("a", "^18.2.0"));
    write(
        root,
        "packages/react/package.json",
        &manifest("react", "1.0.0"),
    );
    let (human, json) = explain(root, "one-range", &[]);
    assert_eq!(
        plan_block(&human),
        lines(&[
            "plan:",
            "  assert: unique: key [dependency], determines [range]",
            "  relations, in evaluation order:",
            "    relation ranges  .alint.yml",
            "      from: selector manifests  .alint.yml",
            "        paths: packages/*/package.json",
            "      entries: json at $.dependencies",
            "      fields:",
            "        dependency: entry.key",
            "        range: entry.value",
            "    relation names  .alint.yml",
            "      from: selector manifests  .alint.yml",
            "        paths: packages/*/package.json",
            "      read: json $.name",
            "      fields:",
            "        name: value",
            "        manifest: path",
            "    relation joined  .alint.yml  (input)",
            "      join: left ranges, right names, on dependency = name, unique right",
            "      fields:",
            "        dependency: left.dependency",
            "        range: left.range",
            "        manifest: right.manifest",
            "  trigger: under --changed, the rule runs on a change to a path these selectors pick, \
             to an ignore file, submodule or nested repository where they can match, or to a \
             marker or manifest their scope_filter: consults",
            "    selector manifests  .alint.yml",
            "      paths: packages/*/package.json",
            "  cost: linear in the rows of each relation it reads",
        ])
    );
    let plan = &json["plan"];
    let names: Vec<&str> = plan["relations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|relation| relation["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["ranges", "names", "joined"]);
    assert_eq!(
        plan["relations"][2]["join"],
        serde_json::json!({
            "left": 0,
            "right": 1,
            "on": [{ "left": "dependency", "right": "name" }],
            "unique": "right",
        })
    );
    let ids: Vec<&serde_json::Value> = plan["relations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|relation| &relation["id"])
        .collect();
    assert_eq!(ids, [0, 1, 2]);
    assert_eq!((&plan["input"], plan.get("set_target")), (&2.into(), None));
    assert_eq!(plan["relations"][2]["roles"], serde_json::json!(["input"]));
    assert_eq!(plan["relations"][0]["roles"], serde_json::json!([]));
    assert_eq!(
        plan["relations"][1]["read"],
        serde_json::json!({ "json": "$.name" })
    );
    assert_eq!(plan["relations"][1]["optional"], false);
    assert_eq!(plan["trigger"].as_array().unwrap().len(), 1);
    let json = json.to_string();
    for value in ["^18.2.0", "1.0.0", "\"a\""] {
        assert!(!human.contains(value), "{value}:\n{human}");
        assert!(!json.contains(value), "{value}: {json}");
    }
}

/// A union: each part with its own selector and reader, in the order
/// written, and each inline selector in the trigger by its part, each named
/// by its place from 0 (`union[0]`), as a message that names a part does.
#[test]
fn a_union_shows_each_part_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        r##"version: 2
relations:
  pins:
    union:
      - from: { paths: .nvmrc }
        read: { regex: "^v?(\\S+)" }
      - from: { paths: ["package.json", "packages/*/package.json"] }
        read: { json: "$.engines.node" }
        optional: true
      - from: { paths: .tool-versions }
        read: { lines: { comment: "#" } }
    fields: { source: path, version: value }
rules:
  - id: one-node
    kind: constraint
    input: pins
    assert: { equal: { field: version, to: { row: { source: .nvmrc } } } }
    level: error
"##,
    );
    write(root, ".nvmrc", "v20.11.1\n");
    let (human, json) = explain(root, "one-node", &[]);
    assert_eq!(
        plan_block(&human),
        lines(&[
            "plan:",
            "  assert: equal: field version, to the source row where source = \".nvmrc\"",
            "  relations, in evaluation order:",
            "    relation pins  .alint.yml  (input)",
            "      union[0]:",
            "        from: inline selector  .alint.yml",
            "          paths: .nvmrc",
            "        read: regex \"^v?(\\\\S+)\"",
            "      union[1]:",
            "        from: inline selector  .alint.yml",
            "          paths: package.json, packages/*/package.json",
            "        read: json $.engines.node, optional",
            "      union[2]:",
            "        from: inline selector  .alint.yml",
            "          paths: .tool-versions",
            "        read: lines, comment \"#\"",
            "      fields:",
            "        source: path",
            "        version: value",
            "  trigger: under --changed, the rule runs on a change to a path these selectors pick, \
             to an ignore file, submodule or nested repository where they can match, or to a \
             marker or manifest their scope_filter: consults",
            "    the inline selector of relation pins, union[0]  .alint.yml",
            "      paths: .nvmrc",
            "    the inline selector of relation pins, union[1]  .alint.yml",
            "      paths: package.json, packages/*/package.json",
            "    the inline selector of relation pins, union[2]  .alint.yml",
            "      paths: .tool-versions",
            "  cost: linear in the rows of each relation it reads",
        ])
    );
    let union = &json["plan"]["relations"][0]["union"];
    assert_eq!(union.as_array().unwrap().len(), 3);
    assert_eq!(
        union[0]["read"],
        serde_json::json!({ "regex": "^v?(\\S+)" })
    );
    assert_eq!(union[1]["optional"], true);
    assert_eq!(
        union[2]["read"],
        serde_json::json!({ "lines": { "comment": "#" } })
    );
    assert_eq!(union[0]["from"]["selector"], serde_json::Value::Null);
    assert_eq!(json["plan"]["trigger"][2]["relation"], 0);
    assert_eq!(json["plan"]["trigger"][2]["part"], 2);
    assert!(!human.contains("20.11.1") && !json.to_string().contains("20.11.1"));
}

/// An inline `input:` is marked, named where it is written.
#[test]
fn an_inline_input_is_marked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrules:\n  - id: names-unique\n    kind: constraint\n    input:\n      \
         from: { paths: \"packages/*/package.json\" }\n      read: { json: \"$.name\" }\n      \
         fields: { name: value }\n    assert: { unique: { key: [name] } }\n    level: error\n",
    );
    let (human, json) = explain(root, "names-unique", &[]);
    assert_eq!(
        plan_block(&human),
        lines(&[
            "plan:",
            "  assert: unique: key [name]",
            "  relations, in evaluation order:",
            "    relation rules[0].input  .alint.yml  (inline input)",
            "      from: inline selector  .alint.yml",
            "        paths: packages/*/package.json",
            "      read: json $.name",
            "      fields:",
            "        name: value",
            "  trigger: under --changed, the rule runs on a change to a path these selectors pick, \
             to an ignore file, submodule or nested repository where they can match, or to a \
             marker or manifest their scope_filter: consults",
            "    the inline selector of relation rules[0].input  .alint.yml",
            "      paths: packages/*/package.json",
            "  cost: linear in the rows of each relation it reads",
        ])
    );
    let relation = &json["plan"]["relations"][0];
    assert_eq!(relation["inline"], true);
    assert_eq!(relation["name"], "rules[0].input");
    assert_eq!(relation["written"], serde_json::json!(["rules[0].input"]));
    assert_eq!(relation["roles"], serde_json::json!(["input"]));
    assert!(!human.contains("__alint_internal"), "{human}");
}

/// An input reached through an `extends:` alias is named as the rule writes
/// it, qualified, with the source that declares it and its selector.
#[test]
fn a_qualified_input_names_its_alias_and_source() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "policies/deps.yml",
        "version: 2\nselectors:\n  manifests: { paths: \"packages/*/package.json\" }\n\
         relations:\n  ranges:\n    from: manifests\n    entries: { format: json, at: \
         \"$.dependencies\" }\n    fields: { dependency: entry.key, range: entry.value }\n",
    );
    write(
        root,
        ".alint.yml",
        "version: 2\nextends:\n  - { url: ./policies/deps.yml, as: deps }\nrules:\n  - id: \
         one-range\n    kind: constraint\n    input: deps.ranges\n    assert: { unique: { key: \
         [dependency], determines: [range] } }\n    level: error\n",
    );
    let (human, json) = explain(root, "one-range", &[]);
    let deps = source("policies/deps.yml");
    assert_eq!(
        plan_block(&human),
        lines(&[
            "plan:",
            "  assert: unique: key [dependency], determines [range]",
            "  relations, in evaluation order:",
            &format!("    relation deps.ranges  {deps}  (input)"),
            &format!("      from: selector manifests  {deps}"),
            "        paths: packages/*/package.json",
            "      entries: json at $.dependencies",
            "      fields:",
            "        dependency: entry.key",
            "        range: entry.value",
            "  trigger: under --changed, the rule runs on a change to a path these selectors pick, \
             to an ignore file, submodule or nested repository where they can match, or to a \
             marker or manifest their scope_filter: consults",
            &format!("    selector manifests  {deps}"),
            "      paths: packages/*/package.json",
            "  cost: linear in the rows of each relation it reads",
        ])
    );
    let relation = &json["plan"]["relations"][0];
    assert_eq!(relation["name"], "ranges");
    assert_eq!(relation["written"], serde_json::json!(["deps.ranges"]));
    assert_eq!(relation["declared_in"], deps.as_str());
    assert_eq!(relation["from"]["declared_in"], deps.as_str());
    assert_eq!(json["plan"]["trigger"][0]["declared_in"], deps.as_str());
}

/// A `set` against another relation's field reads that relation as its
/// target, in plan order with the input, and through a membership list.
#[test]
fn a_set_against_a_relation_marks_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        r#"version: 2
relations:
  workspaces:
    from: { paths: "packages/*/package.json" }
    read: { json: "$.name" }
    fields: { name: value }
  ignored:
    from: { paths: ignored.json }
    entries: { format: json, at: "$" }
    fields: { name: entry.key }
  deps:
    from: { paths: "packages/*/package.json" }
    entries: { format: json, at: "$.dependencies" }
    fields: { manifest: path, dependency: entry.key }
    where: { dependency: { not_in: ["@types/node", { relation: ignored, field: name }] } }
rules:
  - id: deps-are-workspaces
    kind: constraint
    input: deps
    assert:
      set:
        field: dependency
        group_by: manifest
        subset_of: { relation: workspaces, field: name }
    level: error
"#,
    );
    let (human, json) = explain(root, "deps-are-workspaces", &[]);
    assert!(
        human.contains(
            "  assert: set: values of dependency per group of manifest, subset_of field name of \
             relation workspaces\n"
        ),
        "{human}"
    );
    assert!(
        human.contains("    relation workspaces  .alint.yml  (set target)\n"),
        "{human}"
    );
    assert!(
        human.contains(
            "      where:\n        dependency not_in [\"@types/node\", { relation: ignored, \
             field: name }]\n"
        ),
        "{human}"
    );
    let plan = &json["plan"];
    let relations: Vec<(&str, &serde_json::Value)> = plan["relations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|relation| (relation["name"].as_str().unwrap(), &relation["roles"]))
        .collect();
    assert_eq!(
        relations,
        [
            ("workspaces", &serde_json::json!(["set_target"])),
            ("ignored", &serde_json::json!([])),
            ("deps", &serde_json::json!(["input"])),
        ]
    );
    assert_eq!(
        plan["relations"][2]["where"][0]["members"],
        serde_json::json!(["@types/node", { "relation": 1, "field": "name" }])
    );
    let trigger: Vec<u64> = plan["trigger"]
        .as_array()
        .unwrap()
        .iter()
        .map(|selector| selector["relation"].as_u64().unwrap())
        .collect();
    // The trigger lists selectors in walk order: the input's, the relation
    // its list probes, then the target's.
    assert_eq!(trigger, [2, 1, 0]);
    assert_eq!(
        (&plan["input"], &plan["set_target"]),
        (&2.into(), &0.into())
    );
    assert_eq!(
        plan["assertion"]["text"],
        "set: values of dependency per group of manifest, subset_of field name of relation \
         workspaces"
    );
}

/// One relation two `extends:` aliases reach is listed once, with each
/// spelling, and every reference to it reads as the list calls it.
#[test]
fn a_relation_two_aliases_reach_is_listed_once_with_each_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "policies/base.yml",
        "version: 2\nrelations:\n  names:\n    from: { paths: \"packages/*/package.json\" }\n    \
         read: { json: \"$.name\" }\n    fields: { name: value }\n",
    );
    for (file, alias, relation) in [("a.yml", "base", "j"), ("b.yml", "common", "k")] {
        write(
            root,
            &format!("policies/{file}"),
            &format!(
                "version: 2\nextends:\n  - {{ url: ./base.yml, as: {alias} }}\nrelations:\n  \
                 {relation}:\n    join: {{ left: {alias}.names, right: {alias}.names, on: {{ \
                 name: name }}, unique: right }}\n    fields: {{ name: left.name }}\n"
            ),
        );
    }
    write(
        root,
        ".alint.yml",
        "version: 2\nextends:\n  - { url: ./policies/a.yml, as: a }\n  - { url: \
         ./policies/b.yml, as: b }\nrules:\n  - id: c\n    kind: constraint\n    input: a.j\n    \
         assert: { set: { field: name, subset_of: { relation: b.k, field: name } } }\n    level: \
         error\n",
    );
    let (human, json) = explain(root, "c", &[]);
    let (base, a, b) = (
        source("policies/base.yml"),
        source("policies/a.yml"),
        source("policies/b.yml"),
    );
    assert_eq!(
        plan_block(&human),
        lines(&[
            "plan:",
            "  assert: set: values of name, subset_of field name of relation b.k",
            "  relations, in evaluation order:",
            &format!("    relation common.names  {base}"),
            "      written: common.names, base.names",
            &format!("      from: inline selector  {base}"),
            "        paths: packages/*/package.json",
            "      read: json $.name",
            "      fields:",
            "        name: value",
            &format!("    relation a.j  {a}  (input)"),
            "      join: left common.names, right common.names, on name = name, unique right",
            "      fields:",
            "        name: left.name",
            &format!("    relation b.k  {b}  (set target)"),
            "      join: left common.names, right common.names, on name = name, unique right",
            "      fields:",
            "        name: left.name",
            "  trigger: under --changed, the rule runs on a change to a path these selectors pick, \
             to an ignore file, submodule or nested repository where they can match, or to a \
             marker or manifest their scope_filter: consults",
            &format!("    the inline selector of relation common.names  {base}"),
            "      paths: packages/*/package.json",
            "  cost: linear in the rows of each relation it reads",
        ])
    );
    let plan = &json["plan"];
    assert_eq!(
        plan["relations"][0]["written"],
        serde_json::json!(["common.names", "base.names"])
    );
    for (id, relation) in plan["relations"].as_array().unwrap().iter().enumerate() {
        assert_eq!(relation["id"], id);
    }
    for at in [1, 2] {
        let join = &plan["relations"][at]["join"];
        assert_eq!((&join["left"], &join["right"]), (&0.into(), &0.into()));
    }
    assert_eq!(
        (&plan["input"], &plan["set_target"]),
        (&1.into(), &2.into())
    );
    assert_eq!(plan["trigger"][0]["relation"], 0);
}

/// Two relations of one name, from two config sources, read differently
/// wherever they are named: each with the source that declares it.
#[test]
fn two_relations_of_one_name_read_differently() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "policies/deps.yml",
        "version: 2\nrelations:\n  ranges:\n    from: { paths: \"packages/*/package.json\" }\n    \
         entries: { format: json, at: \"$.dependencies\" }\n    fields: { dependency: \
         entry.key, range: entry.value }\n  names:\n    from: { paths: \
         \"packages/*/package.json\" }\n    read: { json: \"$.name\" }\n    fields: { name: \
         value }\n  joined:\n    join: { left: ranges, right: names, on: { dependency: name }, \
         unique: right }\n    fields: { dependency: left.dependency }\n",
    );
    write(
        root,
        ".alint.yml",
        "version: 2\nextends:\n  - { url: ./policies/deps.yml, as: deps }\nrelations:\n  \
         ranges:\n    from: { paths: allowed.json }\n    entries: { format: json, at: \"$\" }\n    \
         fields: { dependency: entry.key }\n  kept:\n    from: { paths: kept.json }\n    \
         entries: { format: json, at: \"$\" }\n    fields: { dependency: entry.key }\n    where: \
         { dependency: { in: [{ relation: ranges, field: dependency }] } }\nrules:\n  - id: c\n    \
         kind: constraint\n    input: deps.joined\n    assert: { set: { field: dependency, \
         subset_of: { relation: kept, field: dependency } } }\n    level: error\n",
    );
    let (human, json) = explain(root, "c", &[]);
    let (deps, own) = (source("policies/deps.yml"), source(".alint.yml"));
    for expected in [
        format!("    relation ranges ({deps})  {deps}\n"),
        format!("    relation ranges ({own})  {own}\n"),
        format!(
            "      join: left ranges ({deps}), right names, on dependency = name, unique right\n"
        ),
        format!("        dependency in [{{ relation: ranges ({own}), field: dependency }}]\n"),
        format!("    the inline selector of relation ranges ({deps})  {deps}\n"),
        format!("    the inline selector of relation ranges ({own})  {own}\n"),
    ] {
        assert!(human.contains(&expected), "{expected}\n{human}");
    }
    // No two relations listed read alike.
    let listed: Vec<&str> = human
        .lines()
        .filter_map(|line| line.strip_prefix("    relation "))
        .collect();
    let mut distinct = listed.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(listed.len(), distinct.len(), "{listed:?}");
    let plan = &json["plan"];
    let relations = plan["relations"].as_array().unwrap();
    let named = |id: &serde_json::Value| {
        let relation = &relations[usize::try_from(id.as_u64().unwrap()).unwrap()];
        (
            relation["name"].as_str().unwrap(),
            relation["declared_in"].as_str().unwrap(),
        )
    };
    let joined = relations.iter().find(|r| r["name"] == "joined").unwrap();
    assert_eq!(named(&joined["join"]["left"]), ("ranges", deps.as_str()));
    let kept = relations.iter().find(|r| r["name"] == "kept").unwrap();
    assert_eq!(
        named(&kept["where"][0]["members"][0]["relation"]),
        ("ranges", own.as_str())
    );
    assert_eq!(named(&plan["set_target"]), ("kept", own.as_str()));
    assert_eq!(named(&plan["input"]), ("joined", deps.as_str()));
}

/// `secret` appears in neither output.
fn assert_hidden(human: &str, json: &serde_json::Value, secrets: &[&str]) {
    let json = json.to_string();
    for secret in secrets {
        assert!(!human.contains(secret), "{secret}:\n{human}");
        assert!(!json.contains(secret), "{secret}: {json}");
    }
}

/// A named selector whose `paths:` the environment writes shows them as
/// `<redacted>`, in its relation and in the trigger.
#[test]
fn an_environment_written_selector_path_is_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nselectors:\n  chosen:\n    paths: \"{{env.SECRET_DIR}}/*/package.json\"\n\
         relations:\n  names:\n    from: chosen\n    read: { json: \"$.name\" }\n    fields: { \
         name: value }\nrules:\n  - id: names-unique\n    kind: constraint\n    input: names\n    \
         assert: { unique: { key: [name] } }\n    level: error\n",
    );
    let (human, json) = explain(root, "names-unique", &[("SECRET_DIR", "canary-dir")]);
    assert_hidden(&human, &json, &["canary-dir"]);
    assert_eq!(human.matches("paths: <redacted>\n").count(), 2, "{human}");
    assert_eq!(json["plan"]["relations"][0]["from"]["paths"], "<redacted>");
    assert_eq!(json["plan"]["trigger"][0]["paths"], "<redacted>");
    assert_eq!(json["plan"]["relations"][0]["read"]["json"], "$.name");
}

/// A relation whose own definition the environment writes shows each value
/// it holds as `<redacted>`: an inline selector, its reader, a `bound:` path
/// and a join's `unique:`; its names stay.
#[test]
fn an_environment_written_relation_definition_is_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  ranges:\n    from: { paths: \
         \"{{env.SECRET_DIR}}/*/package.json\" }\n    entries: { format: json, at: \
         \"{{env.AT}}\" }\n    fields: { dependency: entry.key, owner: \"bound: \
         {{env.BOUND}}\" }\n  names:\n    from: { paths: \"packages/*/package.json\" }\n    \
         read: { json: \"$.name\" }\n    fields: { name: value }\n  joined:\n    join: { left: \
         ranges, right: names, on: { dependency: name }, unique: \"{{env.SIDE}}\" }\n    fields: \
         { dependency: left.dependency, owner: left.owner }\nrules:\n  - id: owners\n    kind: \
         constraint\n    input: joined\n    assert: { equal: { field: owner, group_by: \
         dependency } }\n    level: error\n",
    );
    let env = [
        ("SECRET_DIR", "canary-dir"),
        ("AT", "$.canary_at"),
        ("BOUND", "$.canary_bound"),
        ("SIDE", "right"),
    ];
    let (human, json) = explain(root, "owners", &env);
    assert_hidden(&human, &json, &["canary"]);
    for expected in [
        "    relation ranges  .alint.yml\n      from: inline selector  .alint.yml\n        \
         paths: <redacted>\n      entries: <redacted>\n      fields:\n        dependency: \
         entry.key\n        owner: bound: <redacted>\n",
        "      join: left ranges, right names, on dependency = name, unique <redacted>\n",
        // The relation that reads it holds no value the environment wrote.
        "    relation names  .alint.yml\n      from: inline selector  .alint.yml\n        \
         paths: packages/*/package.json\n      read: json $.name\n",
    ] {
        assert!(human.contains(expected), "{expected}\n{human}");
    }
    let relations = &json["plan"]["relations"];
    assert_eq!(relations[0]["entries"], "<redacted>");
    assert_eq!(relations[0]["from"]["paths"], "<redacted>");
    assert_eq!(relations[0]["fields"][1]["source"], "bound: <redacted>");
    assert_eq!(relations[2]["join"]["unique"], "<redacted>");
    assert_eq!(relations[2]["join"]["left"], 0);

    // A `read:` and its `optional:`, whether `optional:` is written or not.
    for optional in ["\n    optional: true", ""] {
        write(
            root,
            ".alint.yml",
            &format!(
                "version: 2\nrelations:\n  names:\n    from: {{ paths: \"packages/*/package.json\" \
                 }}\n    read: {{ json: \"{{{{env.QUERY}}}}\" }}{optional}\n    fields: {{ name: \
                 value }}\nrules:\n  - id: names\n    kind: constraint\n    input: names\n    \
                 assert: {{ unique: {{ key: [name] }} }}\n    level: error\n"
            ),
        );
        let (human, json) = explain(root, "names", &[("QUERY", "$.canary_query")]);
        assert_hidden(&human, &json, &["canary"]);
        assert!(
            human.contains("      read: <redacted>, optional: <redacted>\n"),
            "{optional:?}\n{human}"
        );
        let relation = &json["plan"]["relations"][0];
        assert_eq!(relation["read"], "<redacted>", "{optional:?}");
        assert_eq!(relation["optional"], "<redacted>", "{optional:?}");
    }
}

/// An assertion whose literal the environment writes is `<redacted>` whole,
/// as its option is.
#[test]
fn an_environment_written_assertion_literal_is_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  pins:\n    from: { paths: \"packages/*/package.json\" }\n    \
         read: { json: \"$.engines.node\" }\n    fields: { manifest: path, node: value }\n\
         rules:\n  - id: one-node\n    kind: constraint\n    input: pins\n    assert: { equal: { \
         field: node, to: { row: { manifest: \"{{env.ROW}}\" } } } }\n    level: error\n",
    );
    let (human, json) = explain(root, "one-node", &[("ROW", "canary-row")]);
    assert_hidden(&human, &json, &["canary"]);
    assert!(human.contains("\n  assert: <redacted>\n"), "{human}");
    assert!(
        human.contains("            assert: <redacted>\n"),
        "{human}"
    );
    assert_eq!(json["plan"]["assertion"], "<redacted>");
    assert_eq!(json["options"]["assert"], "<redacted>");
}

/// A config source an environment-chosen `extends:` entry reached is shown
/// as `list` shows it: `<redacted>`.
#[test]
fn an_environment_chosen_extends_entry_is_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "canary-dir/deps.yml",
        "version: 2\nselectors:\n  manifests: { paths: \"packages/*/package.json\" }\n\
         relations:\n  names:\n    from: manifests\n    read: { json: \"$.name\" }\n    fields: \
         { name: value }\n",
    );
    write(
        root,
        ".alint.yml",
        "version: 2\nextends:\n  - { url: \"{{env.POLICY}}\", as: deps }\nrules:\n  - id: \
         names-unique\n    kind: constraint\n    input: deps.names\n    assert: { unique: { key: \
         [name] } }\n    level: error\n",
    );
    let (human, json) = explain(root, "names-unique", &[("POLICY", "./canary-dir/deps.yml")]);
    assert_hidden(&human, &json, &["canary"]);
    assert!(
        human.contains(
            "    relation deps.names  <redacted>  (input)\n      from: selector manifests  \
             <redacted>\n"
        ),
        "{human}"
    );
    assert_eq!(json["plan"]["relations"][0]["declared_in"], "<redacted>");
    assert_eq!(json["plan"]["trigger"][0]["declared_in"], "<redacted>");
}

/// A `where:` literal or transform the environment writes makes its field
/// sensitive, with every field that shows its value: their `where:`
/// literals and transforms show as `<redacted>`.
#[test]
fn an_environment_written_filter_is_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  ranges:\n    from: { paths: \"packages/*/package.json\" }\n    \
         entries: { format: json, at: \"$.dependencies\" }\n    fields: { dependency: entry.key, \
         range: entry.value, manifest: path }\n    transform: { range: [\"{{env.T}}\"] }\n    \
         where: { dependency: { not_in: [\"{{env.SKIP}}\"], not_matches: \"^canary-pattern\" } \
         }\nrules:\n  - id: one-range\n    kind: constraint\n    input: ranges\n    assert: { \
         unique: { key: [dependency], determines: [range] } }\n    level: error\n",
    );
    let (human, json) = explain(root, "one-range", &[("T", "trim"), ("SKIP", "canary-skip")]);
    assert_hidden(&human, &json, &["canary"]);
    assert!(
        human.contains(
            "      fields:\n        dependency: entry.key  (sensitive)\n        range: \
             entry.value  (transform: <redacted>; sensitive)\n        manifest: path\n      \
             where:\n        dependency not_in [<redacted>]\n        dependency not_matches \
             <redacted>\n"
        ),
        "{human}"
    );
    let relation = &json["plan"]["relations"][0];
    assert_eq!(relation["fields"][1]["transforms"], "<redacted>");
    assert_eq!(
        relation["where"][0]["members"],
        serde_json::json!(["<redacted>"])
    );
    assert_eq!(relation["where"][1]["pattern"], "<redacted>");
}

/// A sensitive field's literals show as `<redacted>` wherever they are
/// written: in `where:` lists and patterns, in `to: { row: }`, in a `set`'s
/// `{ group: }` and `groups:`, in the plan and in the rule's options, and in
/// an inline input's options.
#[test]
fn a_sensitive_fields_literals_are_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        r#"version: 2
relations:
  tokens:
    from: { paths: "secrets/*.json" }
    entries: { format: json, at: "$" }
    fields: { key: entry.key, token: { source: entry.value, sensitive: true }, file: path }
    transform: { token: [trim] }
    where: { token: { not_in: ["canary-list"], not_matches: "^canary-pattern" } }
rules:
  - id: source-row
    kind: constraint
    input: tokens
    assert: { equal: { field: key, to: { row: { token: "canary-row" } } } }
    level: error
  - id: groups
    kind: constraint
    input: tokens
    assert:
      set:
        field: key
        group_by: token
        equal_to: { group: "canary-group" }
        groups: ["canary-other"]
    level: error
  - id: inline
    kind: constraint
    input:
      from: { paths: "secrets/*.json" }
      entries: { format: json, at: "$" }
      fields: { key: entry.key, token: { source: entry.value, sensitive: true } }
      transform: { token: [trim] }
      where: { token: { in: ["canary-inline"], matches: "canary-match" } }
    assert: { unique: { key: [token] } }
    level: error
"#,
    );
    write(root, "secrets/a.json", "{\"api\": \"hunter2\"}\n");
    for rule in ["source-row", "groups", "inline"] {
        let (human, json) = explain(root, rule, &[]);
        assert_hidden(&human, &json, &["canary", "hunter2"]);
        assert!(
            human.contains("        token: entry.value  (transform: <redacted>; sensitive)\n"),
            "{rule}:\n{human}"
        );
        let relation = &json["plan"]["relations"][0];
        assert_eq!(relation["fields"][1]["transforms"], "<redacted>", "{rule}");
        assert_eq!(relation["fields"][1]["sensitive"], true, "{rule}");
        assert_eq!(
            relation["where"][0]["members"],
            serde_json::json!(["<redacted>"]),
            "{rule}"
        );
        assert_eq!(relation["where"][1]["pattern"], "<redacted>", "{rule}");
    }
    let (human, json) = explain(root, "source-row", &[]);
    assert!(
        human.contains("  assert: equal: field key, to the source row where token = <redacted>\n"),
        "{human}"
    );
    assert_eq!(
        json["options"]["assert"]["equal"]["to"]["row"]["token"],
        "<redacted>"
    );
    let (human, json) = explain(root, "groups", &[]);
    assert!(
        human.contains(
            "  assert: set: values of key per group of token, equal_to group <redacted>, groups \
             [<redacted>]\n"
        ),
        "{human}"
    );
    let set = &json["options"]["assert"]["set"];
    assert_eq!(set["equal_to"]["group"], "<redacted>");
    assert_eq!(set["groups"], serde_json::json!(["<redacted>"]));
    let (human, json) = explain(root, "inline", &[]);
    let input = &json["options"]["input"];
    assert_eq!(
        input["where"]["token"]["in"],
        serde_json::json!(["<redacted>"])
    );
    assert_eq!(input["where"]["token"]["matches"], "<redacted>");
    assert_eq!(input["transform"]["token"], "<redacted>");
    assert!(human.contains("\"in\":[\"<redacted>\"]"), "{human}");
}

/// A rule whose fields are all hidden (`sensitive: true`) shows its options
/// as `<redacted>`, and so its plan.
#[test]
fn a_sensitive_rule_shows_no_plan() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\nrelations:\n  canary-names:\n    from: { paths: \"packages/*/package.json\" \
         }\n    read: { json: \"$.name\" }\n    fields: { name: value }\nrules:\n  - id: \
         names-unique\n    kind: constraint\n    input: canary-names\n    assert: { unique: { \
         key: [name] } }\n    level: error\n    sensitive: true\n",
    );
    let (human, json) = explain(root, "names-unique", &[]);
    assert_hidden(&human, &json, &["canary"]);
    assert!(human.ends_with("\nplan:       <redacted>\n"), "{human}");
    assert_eq!(json["plan"], "<redacted>");
}

/// A version 1 rule's `explain`, in a version 1 config and in a version 2
/// one, is byte for byte what it was before `explain` showed a plan: the
/// snapshots were written by the build before that change.
#[test]
fn a_version_one_rules_explain_is_unchanged() {
    let snapshot = |name: &str| {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/snapshots")
                .join(format!("explain-{name}.txt")),
        )
        .unwrap()
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        r#"version: 1
facts:
  - id: has_rust
    any_file_exists: [Cargo.toml]
rules:
  - id: readme-title
    kind: file_content_matches
    paths:
      include: ["packages/*/README.md", "!packages/legacy/README.md"]
      exclude: ["packages/vendor/**"]
    scope_filter:
      has_ancestor: package.json
      include_manifest_paths:
        source: package.json
        extract: { json: "$.workspaces[*]" }
    pattern: "^# "
    level: warning
    message: |
      every README starts with a title
      (see the style guide)
    policy_url: https://example.com/style
    when: facts.has_rust
  - id: no-tabs
    kind: no_trailing_whitespace
    paths: "**/*.md"
    level: error
    fix: { file_trim_trailing_whitespace: {} }
"#,
    );
    write(
        root,
        "package.json",
        "{\"name\":\"root\",\"workspaces\":[\"packages/a\",\"packages/b\"]}\n",
    );
    write(root, "packages/a/package.json", "{}\n");
    write(root, "packages/a/README.md", "# A\n");
    let stdout = |args: &[&str]| {
        let output = run(root, args, &[]);
        assert!(output.status.success(), "{args:?}");
        String::from_utf8(output.stdout).unwrap()
    };
    for (rule, name) in [("readme-title", "v1-readme"), ("no-tabs", "v1-notabs")] {
        assert_eq!(
            stdout(&["explain", rule]),
            snapshot(&format!("{name}-human")),
            "{rule}"
        );
        assert_eq!(
            stdout(&["explain", rule, "--format", "json"]),
            snapshot(&format!("{name}-json")),
            "{rule}"
        );
    }
    write(
        root,
        ".alint.yml",
        "version: 2\nselectors:\n  manifests:\n    paths: \"packages/*/package.json\"\n\
         relations:\n  names:\n    from: manifests\n    read: { json: \"$.name\" }\n    fields: \
         { name: value }\nrules:\n  - id: license-present\n    kind: file_exists\n    paths: \
         [LICENSE, LICENSE.md]\n    level: error\n    message: add a license\n",
    );
    assert_eq!(
        stdout(&["explain", "license-present"]),
        snapshot("v2v1-human")
    );
    assert_eq!(
        stdout(&["explain", "license-present", "--format", "json"]),
        snapshot("v2v1-json")
    );
}

/// Rules whose plans hold every key and every `<redacted>` form a plan has:
/// a named selector with a `scope_filter:` and manifests, each reader and
/// `read:` form, a union, a join, a `set`'s target, `where:` lists with
/// literals and a relation's field, a sensitive field, and what the
/// environment wrote.
const EVERY_PLAN_SHAPE: &str = r##"version: 2
selectors:
  manifests:
    paths: ["packages/*/package.json", "!packages/legacy/package.json"]
    scope_filter:
      has_ancestor: package.json
      include_manifest_paths:
        source: package.json
        extract: { json: "$.workspaces[*]" }
  chosen:
    paths: "{{env.DIR}}/*/package.json"
    scope_filter: { has_ancestor: package.json }
relations:
  ranges:
    from: manifests
    entries: { format: json, at: ["$.dependencies", "$.devDependencies"], leaves: true }
    fields: { manifest: path, dependency: entry.key, range: entry.value }
    transform: { range: [trim, lower] }
    where: { range: { not_matches: "^workspace:", not_in: ["*"] } }
  names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value, manifest: path }
  joined:
    join: { left: ranges, right: names, on: { manifest: manifest }, unique: right }
    fields: { dependency: left.dependency, range: left.range, owner: right.name }
  pins:
    union:
      - from: { paths: .nvmrc }
        read: { regex: "^v?(\\S+)" }
      - from: { paths: package.json }
        read: { json: "$.engines.node" }
        optional: true
      - from: { paths: .tool-versions }
        read: { lines: { comment: "#" } }
      - from: { paths: .node-version }
        read: { lines: {} }
      - from: { paths: node.yaml }
        read: { yaml: "$.node" }
      - from: { paths: node.toml }
        read: { toml: "$.node" }
      - from: { paths: NODE }
        read: { whole_file: {} }
    fields: { source: path, version: value }
  hidden:
    from: chosen
    read: { json: "{{env.QUERY}}" }
    optional: true
    fields: { name: value }
  hidden-entries:
    from: { paths: "{{env.DIR}}/*.json" }
    entries: { format: json, at: "{{env.AT}}" }
    fields: { key: entry.key, owner: "bound: {{env.BOUND}}" }
  workspaces:
    from: { paths: "packages/*/package.json" }
    entries: { format: yaml, at: "$.deps" }
    fields: { name: entry.key }
  hidden-joined:
    join: { left: hidden-entries, right: workspaces, on: { key: name }, unique: "{{env.SIDE}}" }
    fields: { key: left.key, owner: left.owner }
  tokens:
    from: { paths: "secrets/*.json" }
    entries: { format: toml, at: "$" }
    fields:
      key: entry.key
      token: { source: entry.value, sensitive: true }
      flag: { source: "bound: $.private", optional: true, on_missing: missing }
    transform: { token: [trim] }
    where:
      token: { not_in: ["x", { relation: workspaces, field: name }], matches: "^t" }
      flag: { in: [true] }
      key: { in: ["k"] }
rules:
  - id: joined
    kind: constraint
    input: joined
    assert: { unique: { key: [dependency], determines: [range] } }
    level: error
  - id: pins
    kind: constraint
    input: pins
    assert: { equal: { field: version, to: { row: { source: .nvmrc } } } }
    level: error
  - id: hidden
    kind: constraint
    input: hidden
    assert: { equal: { field: name, to: { row: { name: "{{env.ROW}}" } } } }
    level: error
  - id: hidden-joined
    kind: constraint
    input: hidden-joined
    assert: { equal: { field: owner, group_by: key } }
    level: error
  - id: tokens
    kind: constraint
    input: tokens
    assert: { set: { field: key, subset_of: { relation: workspaces, field: name } } }
    level: error
  - id: inline
    kind: constraint
    input:
      from: { paths: "x/*.json" }
      read: { json: "$.v" }
      fields: { v: value }
    assert: { unique: { key: [v] } }
    level: error
  - id: sensitive
    kind: constraint
    input: names
    assert: { unique: { key: [name] } }
    level: error
    sensitive: true
"##;

/// The `explain --trace --format json` document of each rule of
/// [`EVERY_PLAN_SHAPE`].
fn every_plan_shape() -> Vec<(&'static str, serde_json::Value)> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".alint.yml", EVERY_PLAN_SHAPE);
    write(
        root,
        "package.json",
        "{\"name\": \"root\", \"workspaces\": [\"packages/a\"]}\n",
    );
    write(root, "packages/a/package.json", &manifest("a", "^18.2.0"));
    let env = [
        ("DIR", "canary-dir"),
        ("QUERY", "$.canary_query"),
        ("AT", "$.canary_at"),
        ("BOUND", "$.canary_bound"),
        ("SIDE", "right"),
        ("ROW", "canary-row"),
    ];
    [
        "joined",
        "pins",
        "hidden",
        "hidden-joined",
        "tokens",
        "inline",
        "sensitive",
    ]
    .into_iter()
    .map(|rule| {
        let output = run(
            root,
            &["explain", rule, "--trace", "--format", "json"],
            &env,
        );
        assert!(output.status.success(), "{rule}: {output:?}");
        let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            !document.to_string().contains("canary"),
            "{rule}: {document}"
        );
        (rule, document)
    })
    .collect()
}

/// The JSON pointer of every object in `value`, below `at`.
fn objects(value: &serde_json::Value, at: &str, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            out.push(at.to_string());
            for (key, child) in map {
                objects(child, &format!("{at}/{key}"), out);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                objects(item, &format!("{at}/{index}"), out);
            }
        }
        _ => {}
    }
}

/// Each key in `value`, and each key (or `key[]` for a list's item) whose
/// value is `<redacted>`.
fn keys(
    value: &serde_json::Value,
    key: &str,
    seen: &mut std::collections::BTreeSet<String>,
    redacted: &mut std::collections::BTreeSet<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (name, child) in map {
                seen.insert(name.clone());
                if child == "<redacted>" {
                    redacted.insert(name.clone());
                }
                keys(child, name, seen, redacted);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if item == "<redacted>" {
                    redacted.insert(format!("{key}[]"));
                }
                keys(item, key, seen, redacted);
            }
        }
        _ => {}
    }
}

/// Each property name the schema `value` declares.
fn declared(value: &serde_json::Value, out: &mut std::collections::BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(properties) = map.get("properties").and_then(|p| p.as_object()) {
                out.extend(properties.keys().cloned());
            }
            map.values().for_each(|child| declared(child, out));
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| declared(item, out)),
        _ => {}
    }
}

/// `explain --trace --format json` carries the plan in its rule document,
/// which the experimental trace schema describes key by key: every real plan
/// validates, whole or `<redacted>`; a key renamed or added anywhere in one
/// fails; and the plans name every key the schema declares for a plan, and
/// take each `<redacted>` form.
#[test]
fn the_trace_schema_describes_the_plan_key_by_key() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../schemas/experimental/rule-trace.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors = |document: &serde_json::Value| -> Vec<String> {
        validator
            .iter_errors(document)
            .map(|error| error.to_string())
            .collect()
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut redacted = std::collections::BTreeSet::new();
    let mut checked = 0;
    for (rule, document) in every_plan_shape() {
        assert!(document["rule"].get("plan").is_some(), "{rule}: {document}");
        assert!(
            errors(&document).is_empty(),
            "{rule}: {:#?}",
            errors(&document)
        );
        let plan = &document["rule"]["plan"];
        keys(plan, "plan", &mut seen, &mut redacted);
        if plan == "<redacted>" {
            redacted.insert("plan".into());
        }
        // A plan is an object or `<redacted>`, never another string.
        let mut other = document.clone();
        other["rule"]["plan"] = serde_json::Value::from("plan");
        assert!(!validator.is_valid(&other), "{other}");
        let mut at = Vec::new();
        objects(plan, "/rule/plan", &mut at);
        for pointer in at {
            let names: Vec<String> = document
                .pointer(&pointer)
                .and_then(serde_json::Value::as_object)
                .unwrap()
                .keys()
                .cloned()
                .collect();
            for name in names.iter().map(Some).chain([None]) {
                let mut changed = document.clone();
                let object = changed
                    .pointer_mut(&pointer)
                    .and_then(serde_json::Value::as_object_mut)
                    .unwrap();
                let change = if let Some(name) = name {
                    let value = object.remove(name).unwrap();
                    object.insert(format!("{name}_renamed"), value);
                    format!("`{name}` renamed")
                } else {
                    object.insert("unexpected".into(), serde_json::Value::from(1));
                    "a key added".into()
                };
                assert!(
                    !validator.is_valid(&changed),
                    "{rule}: the schema accepts {change} at {pointer}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 500, "{checked}");
    let mut plan_schema = std::collections::BTreeSet::new();
    declared(
        &schema["properties"]["rule"]["properties"]["plan"],
        &mut plan_schema,
    );
    assert_eq!(seen, plan_schema, "the plans' keys and the schema's");
    let every_form: std::collections::BTreeSet<String> = [
        "assertion",
        "entries",
        "members[]",
        "optional",
        "paths",
        "pattern",
        "plan",
        "read",
        "scope_filter",
        "transforms",
        "unique",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(redacted, every_form);
}

#[test]
fn an_optional_bound_field_shows_its_mode_and_redacts_when_env_driven() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        ".alint.yml",
        "version: 2\n\
         relations:\n\
         \x20 deps:\n\
         \x20   from: { paths: 'packages/*/package.json' }\n\
         \x20   entries: { format: json, at: $.dependencies }\n\
         \x20   fields:\n\
         \x20     dep: entry.key\n\
         \x20     pkg: { source: 'bound: $.name', optional: true }\n\
         \x20     license: { source: 'bound: $.license', optional: true, on_missing: missing, sensitive: true }\n\
         \x20 env_deps:\n\
         \x20   from: { paths: 'packages/*/package.json' }\n\
         \x20   entries: { format: json, at: $.dependencies }\n\
         \x20   fields:\n\
         \x20     dep: entry.key\n\
         \x20     pkg: { source: 'bound: $.name', optional: '{{env.IS_OPT}}', on_missing: '{{env.MODE}}' }\n\
         rules:\n\
         \x20 - id: plain\n\
         \x20   kind: constraint\n\
         \x20   input: deps\n\
         \x20   assert: { unique: { key: [dep] } }\n\
         \x20   level: error\n\
         \x20 - id: env-driven\n\
         \x20   kind: constraint\n\
         \x20   input: env_deps\n\
         \x20   assert: { unique: { key: [dep] } }\n\
         \x20   level: error\n",
    );
    let env = [("IS_OPT", "true"), ("MODE", "missing")];
    let (human, json) = explain(root, "plain", &env);
    assert!(
        human.contains("        pkg: bound: $.name  (optional)\n"),
        "{human}"
    );
    assert!(
        human.contains(
            "        license: bound: $.license  (optional, on_missing: missing; sensitive)\n"
        ),
        "{human}"
    );
    let fields = &json["plan"]["relations"][0]["fields"];
    assert_eq!(fields[1]["source"], "bound: $.name");
    assert_eq!(fields[1]["optional"], true);
    assert_eq!(fields[1]["on_missing"], "skip");
    assert_eq!(fields[2]["source"], "bound: $.license");
    assert_eq!(fields[2]["optional"], true);
    assert_eq!(fields[2]["on_missing"], "missing");
    assert_eq!(fields[2]["sensitive"], true);

    // When the relation definition reads the environment (here via `optional:`
    // and `on_missing:`), neither `optional` nor `on_missing` leaks in human or
    // JSON `explain` output.
    let (env_human, env_json) = explain(root, "env-driven", &env);
    assert!(
        env_human.contains("        pkg: bound: <redacted>\n"),
        "{env_human}"
    );
    assert!(!env_human.contains("on_missing"), "{env_human}");
    assert!(!env_human.contains("(optional)"), "{env_human}");
    let env_field = &env_json["plan"]["relations"][0]["fields"][1];
    assert_eq!(env_field["source"], "bound: <redacted>");
    assert!(env_field.get("optional").is_none(), "{env_field}");
    assert!(env_field.get("on_missing").is_none(), "{env_field}");
}
