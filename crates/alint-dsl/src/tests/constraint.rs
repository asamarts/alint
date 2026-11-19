//! The `constraint` kind through the loader and its builder (design
//! `v0.23/selectors-and-constraints.md`, sections 6, 10 and 11): the version
//! gate, the refusal under `require:`, an inline `input:`, a qualified one,
//! the names that stay literal, and the builder's check of the fields and
//! literals an assertion names. What a rule finds over a tree is tested in
//! `constraint_families.rs`.

use std::path::{Path, PathBuf};

use super::*;

pub(super) fn write(root: &Path, relative: &str, body: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    path
}

/// `config` written as `.alint.yml` with `files` beside it, loaded.
pub(super) fn load_tree(
    config: &str,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, alint_core::Result<Config>) {
    let dir = tempfile::tempdir().unwrap();
    for (path, body) in files {
        write(dir.path(), path, body);
    }
    let loaded = load(&write(dir.path(), ".alint.yml", config));
    (dir, loaded)
}

/// The error loading `config` gives, without the directory it sits in.
fn load_error(config: &str, files: &[(&str, &str)]) -> String {
    let (dir, loaded) = load_tree(config, files);
    let error = loaded.expect_err("the config is refused").to_string();
    error.replace(
        &format!("{}{}", dir.path().display(), std::path::MAIN_SEPARATOR),
        "",
    )
}

/// The error building rule `id` of `config` gives.
fn build_error(config: &str, id: &str) -> String {
    let (_dir, loaded) = load_tree(config, &[]);
    let config = loaded.expect("the config loads");
    let spec = config
        .rules
        .iter()
        .find(|spec| spec.id == id)
        .expect("the rule is declared");
    match test_registry().build(spec) {
        Ok(_) => panic!("rule {id} built"),
        Err(error) => error.to_string(),
    }
}

/// Every rule of `config` builds.
fn builds(config: &str, files: &[(&str, &str)]) -> Config {
    let (_dir, loaded) = load_tree(config, files);
    let config = loaded.unwrap_or_else(|error| panic!("the config loads: {error}"));
    for spec in &config.rules {
        if let Err(error) = test_registry().build(spec) {
            panic!("rule {} does not build: {error}", spec.id);
        }
    }
    config
}

const NAMES: &str = "relations:\n  names:\n    from: { paths: \"packages/*/package.json\" }\n    \
                     read: { json: \"$.name\" }\n    fields: { name: value, manifest: path }\n";

const DEPS: &str = "relations:\n  deps:\n    from: { paths: \"**/package.json\" }\n    \
                    entries: { format: json, at: \"$.dependencies\" }\n    \
                    fields: { dependency: entry.key, range: entry.value, at: path, \
                    file: basename }\n";

/// A version 2 config with `objects` and one `constraint` rule, `c`, whose
/// `input:` and `assert:` are `rest` (indented for a rule's fields).
fn v2(objects: &str, rest: &str) -> String {
    format!(
        "version: 2\n{objects}rules:\n  - id: c\n    kind: constraint\n    level: error\n{rest}"
    )
}

// --- The version gate (section 11) -----------------------------------------

#[test]
fn a_version_one_config_cannot_use_the_constraint_kind() {
    let config = "version: 1\nrules:\n  - id: c\n    kind: constraint\n    input: names\n    \
                  assert: { unique: { key: [name] } }\n    level: error\n";
    assert_eq!(
        load_error(config, &[]),
        ".alint.yml: `rules[0]`: the `constraint` rule kind needs config `version: 2` (it is \
         experimental), and this config is version 1; declare `version: 2` to use it"
    );
}

#[test]
fn a_version_one_source_a_version_two_config_extends_cannot_use_it_either() {
    let base = "version: 1\nrules:\n  - id: c\n    kind: constraint\n    input: names\n    \
                assert: { unique: { key: [name] } }\n    level: error\n";
    let error = load_error(
        &format!("version: 2\nextends: [./base.yml]\n{NAMES}rules: []\n"),
        &[("base.yml", base)],
    );
    assert!(
        error.contains(
            "`rules[0]`: the `constraint` rule kind needs config `version: 2` (it is \
             experimental), and this config is version 1"
        ),
        "{error}"
    );
    assert!(error.contains("base.yml"), "{error}");
}

#[test]
fn a_template_cannot_bring_the_kind_into_a_rule_of_version_one() {
    // A version 1 template whose kind is `constraint`, instantiated by a
    // version 1 rule: the rule's kind came from no version 2 source.
    let config = "version: 1\ntemplates:\n  - id: t\n    kind: constraint\n    input: names\n    \
                  assert: { unique: { key: [name] } }\n    level: error\nrules:\n  - \
                  extends_template: t\n    id: c\n";
    let error = load_error(config, &[]);
    assert!(
        error.contains("the `constraint` rule kind needs config `version: 2`"),
        "{error}"
    );
}

/// A rule takes its kind from the source that declares the kind: a version 1
/// file may instantiate a template of kind `constraint` that the version 2
/// root declares, and the rule reads the root's plan; a version 1 file may
/// not declare such a template, wherever it is instantiated.
#[test]
fn a_template_of_the_kind_is_declared_in_a_source_of_version_two() {
    let template = "templates:\n  - id: t\n    kind: constraint\n    input: names\n    \
                    assert: { unique: { key: [name] } }\n    level: error\n";
    let base = "version: 1\nrules:\n  - id: x\n    extends_template: t\n";
    let config = builds(
        &format!("version: 2\nextends: [./base.yml]\n{NAMES}{template}rules: []\n"),
        &[("base.yml", base)],
    );
    let spec = config.rules.iter().find(|spec| spec.id == "x").unwrap();
    assert_eq!(spec.kind, "constraint");
    assert!(spec.relation_plan.is_some());

    let base = format!("version: 1\n{template}");
    let error = load_error(
        &format!(
            "version: 2\nextends: [./base.yml]\n{NAMES}rules:\n  - id: x\n    \
             extends_template: t\n"
        ),
        &[("base.yml", &base)],
    );
    assert!(
        error.contains("the `constraint` rule kind needs config `version: 2`")
            && error.contains("base.yml"),
        "{error}"
    );
}

/// Every source declares its version (an `.alint.d/` fragment takes its
/// root's), and no `{{...}}` can supply a kind, so the check of each
/// source's literal kinds covers every rule.
#[test]
fn a_fragment_takes_its_roots_version_and_no_variable_supplies_a_kind() {
    let rule = |relation: &str| {
        format!(
            "{relation}rules:\n  - id: c\n    kind: constraint\n    input: names\n    \
             assert: {{ unique: {{ key: [name] }} }}\n    level: error\n"
        )
    };
    // The loader names a fragment as the platform spells its path.
    let fragment = std::path::Path::new(".alint.d").join("c.yml");
    assert_eq!(
        load_error("version: 1\nrules: []\n", &[(".alint.d/c.yml", &rule(""))]),
        format!(
            "{}: `rules[0]`: the `constraint` rule kind needs config `version: 2` (it is \
             experimental), and this config is version 1; declare `version: 2` to use it",
            fragment.display()
        )
    );
    builds(
        "version: 2\nrules: []\n",
        &[(".alint.d/c.yml", &rule(NAMES))],
    );
    let template = "version: 1\ntemplates:\n  - id: t\n    kind: \"{{vars.k}}\"\n    level: \
                    error\nrules:\n  - extends_template: t\n    id: c\n    vars: { k: \
                    constraint }\n";
    assert!(
        load_error(template, &[]).contains("`kind` must be a literal value"),
        "{}",
        load_error(template, &[])
    );
}

#[test]
fn every_other_kind_is_of_version_one() {
    let registry = test_registry();
    for kind in registry.known_kinds() {
        let expected = u32::from(kind == "constraint") + 1;
        assert_eq!(registry.min_config_version(kind), expected, "{kind}");
    }
    assert_eq!(
        registry.min_config_version("constraint"),
        alint_rules::constraint::MIN_CONFIG_VERSION
    );
    // A `constraint` rule reads what its relation's readers decode.
    assert_eq!(
        registry.meta("constraint").unwrap().formats.as_ref(),
        [
            alint_core::Format::Json,
            alint_core::Format::Yaml,
            alint_core::Format::Toml,
        ]
    );
}

// --- Nesting (section 11) ----------------------------------------------------

#[test]
fn a_constraint_cannot_be_nested_under_require() {
    for (kind, select) in [
        ("for_each_dir", "select: \"packages/*\""),
        ("for_each_file", "select: \"packages/*/package.json\""),
        ("every_matching_has", "select: \"packages/*\""),
    ] {
        let config = format!(
            "version: 2\n{NAMES}rules:\n  - id: each\n    kind: {kind}\n    {select}\n    \
             require:\n      - kind: constraint\n        input: names\n        assert: {{ \
             unique: {{ key: [name] }} }}\n    level: error\n"
        );
        let error = load_error(&config, &[]);
        assert!(
            error.contains(
                "a `constraint` rule reads a relation, which a rule nested under `require:` \
                 cannot; declare it as a top-level rule"
            ),
            "{kind}: {error}"
        );
    }
}

// --- `input:` --------------------------------------------------------------

#[test]
fn an_inline_input_is_planned_under_a_name_no_config_can_write() {
    let config = builds(
        &v2(
            "",
            "    input:\n      from: { paths: \"packages/*/package.json\" }\n      read: { \
             json: \"$.name\" }\n      fields: { name: value }\n    assert: { unique: { key: \
             [name] } }\n",
        ),
        &[],
    );
    let plan = config.relation_plan.as_ref().expect("a plan");
    let inline: Vec<&str> = plan
        .relations
        .iter()
        .filter(|named| named.is_inline())
        .map(|named| named.name.as_str())
        .collect();
    assert_eq!(inline, ["rules[0].input"]);
    let rule = plan.rule("c").expect("the rule's references");
    assert_eq!(rule.references.len(), 1);
    assert_eq!(
        plan.relation(rule.references[0].relation.id).name,
        "rules[0].input"
    );
    // The rule keeps no trace of the marker the loader planned it under.
    let spec = &config.rules[0];
    assert!(
        spec.extra
            .get("input")
            .is_some_and(alint_core::yaml::Value::is_mapping)
    );
    assert!(!format!("{:?}", spec.extra).contains("__alint_internal"));
    // A config cannot write that name: `.` and `[` are not in a name.
    let error = load_error(
        &v2(
            "relations:\n  \"rules[0].input\":\n    from: { paths: x }\n    read: { \
             whole_file: {} }\n    fields: { f: path }\n",
            "    input: names\n    assert: { unique: { key: [f] } }\n",
        ),
        &[],
    );
    assert!(error.contains("rules[0].input"), "{error}");
    // Nor the marker.
    let error = load_error(
        &v2(
            "",
            "    input:\n      __alint_internal_v2_inline_input: x\n      from: { paths: x }\n      \
             read: { whole_file: {} }\n      fields: { f: path }\n    assert: { unique: { key: \
             [f] } }\n",
        ),
        &[],
    );
    assert!(
        error.contains("__alint_internal_v2_inline_input"),
        "{error}"
    );
}

/// A declared name may start with `_` and hold `_` and `-`: such a relation
/// is no inline one, by the one predicate the loader checks names with, so
/// a rule's messages name it as ``relation `NAME` ``, whichever of its
/// messages names it (a `set`'s subject, an analysis failure's prefix).
#[test]
fn a_declared_name_with_underscores_and_hyphens_is_not_inline() {
    let config = "version: 2\nrelations:\n  _dep-ranges:\n    from: { paths: \
                  \"**/package.json\" }\n    entries: { format: json, at: \"$.dependencies\" \
                  }\n    fields: { dependency: entry.key, file: path }\n  dep_ranges:\n    from: \
                  { paths: wanted.json }\n    entries: { format: json, at: \"$\" }\n    fields: \
                  { dependency: entry.key, file: path }\nrules:\n  - id: lacks-wanted\n    kind: \
                  constraint\n    input: _dep-ranges\n    assert: { set: { field: dependency, \
                  equal_to: { relation: dep_ranges, field: dependency } } }\n    level: error\n  \
                  - id: lacks-used\n    kind: constraint\n    input: dep_ranges\n    assert: { \
                  set: { field: dependency, equal_to: { relation: _dep-ranges, field: dependency \
                  } } }\n    level: error\n  - id: no-reference\n    kind: constraint\n    \
                  input: _dep-ranges\n    assert: { set: { field: dependency, group_by: file, \
                  equal_to: { group: none.json } } }\n    level: error\n  - id: no-reference-2\n    \
                  kind: constraint\n    input: dep_ranges\n    assert: { set: { field: \
                  dependency, group_by: file, equal_to: { group: none.json } } }\n    level: \
                  error\n";
    let files = &[
        (
            "a/package.json",
            "{\"dependencies\": {\"react\": \"1\", \"left-pad\": \"1\"}}\n",
        ),
        ("wanted.json", "{\"react\": true, \"vue\": true}\n"),
    ];
    let config_loaded = builds(config, files);
    let plan = config_loaded.relation_plan.as_ref().expect("a plan");
    let inline: Vec<(&str, bool)> = plan
        .relations
        .iter()
        .map(|named| (named.name.as_str(), named.is_inline()))
        .collect();
    assert_eq!(inline, [("_dep-ranges", false), ("dep_ranges", false)]);
    let results = super::constraint_families::run(config, files);
    let first_line = |shown: &[String]| -> Vec<String> {
        shown
            .iter()
            .map(|shown| shown.lines().next().unwrap_or_default().to_string())
            .collect()
    };
    assert_eq!(
        first_line(&super::constraint_families::findings(
            &results,
            "lacks-wanted"
        )),
        [
            "a/package.json: relation `_dep-ranges` lacks `dependency` \"vue\", which \
             `dependency` of relation `dep_ranges` holds",
            "a/package.json:1:33: relation `_dep-ranges` holds `dependency` \"left-pad\", which \
             `dependency` of relation `dep_ranges` lacks"
        ]
    );
    assert_eq!(
        first_line(&super::constraint_families::findings(
            &results,
            "lacks-used"
        )),
        [
            "wanted.json: relation `dep_ranges` lacks `dependency` \"left-pad\", which \
             `dependency` of relation `_dep-ranges` holds",
            "wanted.json:1:17: relation `dep_ranges` holds `dependency` \"vue\", which \
             `dependency` of relation `_dep-ranges` lacks"
        ]
    );
    for (rule, relation) in [
        ("no-reference", "_dep-ranges"),
        ("no-reference-2", "dep_ranges"),
    ] {
        let failures = super::constraint_families::failures(&results, rule);
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(
            failures[0].starts_with(&format!("relation `{relation}` (.alint.yml): ")),
            "{failures:?}"
        );
    }
}

/// A built rule says it reads relations, so the engine turns the run's
/// relation caches on for it (each file a relation reads is read once),
/// evaluates its inputs before the rules run and hands it the full index.
#[test]
fn a_built_rule_reads_its_relations_through_the_runs_caches() {
    let config = builds(
        &v2(
            NAMES,
            "    input: names\n    assert: { unique: { key: [name] } }\n",
        ),
        &[],
    );
    let rule = test_registry()
        .build(&config.rules[0])
        .expect("the rule builds");
    assert!(rule.uses_relations());
    assert!(rule.requires_full_index());
    let (plan, inputs) = rule.relation_inputs().expect("the rule's inputs");
    let names: Vec<&str> = inputs
        .iter()
        .map(|&id| plan.relation(id).name.as_str())
        .collect();
    assert_eq!(names, ["names"]);
}

#[test]
fn an_inline_input_of_a_template_is_planned_too() {
    let template = "version: 2\ntemplates:\n  - id: t\n    kind: constraint\n    input:\n      \
                    from: { paths: \"DIR/*/package.json\" }\n      read: { json: \"$.name\" }\n      \
                    fields: { name: value }\n    assert: { unique: { key: [name] } }\n    \
                    level: error\nrules:\n  - extends_template: t\n    id: c\n    vars: { dir: \
                    packages }\n";
    let config = builds(&template.replace("DIR", "packages"), &[]);
    let plan = config.relation_plan.as_ref().expect("a plan");
    let named = plan.relation(plan.rule("c").unwrap().references[0].relation.id);
    assert_eq!(named.name, "templates[0].input");
    // An inline relation is literal throughout, a template's `{{vars.X}}`
    // included: one relation per template, the same for every instance.
    assert_eq!(
        load_error(&template.replace("DIR", "{{vars.dir}}"), &[]),
        ".alint.yml: `templates[0].input.from.paths`: `{{...}}` is not allowed in an inline \
         `input:` relation; declare the relation under `relations:`, where a value may read \
         the environment, and name it"
    );
}

#[test]
fn an_inline_input_reads_no_environment() {
    let error = load_error(
        &v2(
            "",
            "    input:\n      from: { paths: \"{{env.DIR}}/*.json\" }\n      read: { json: \
             \"$.name\" }\n      fields: { name: value }\n    assert: { unique: { key: [name] \
             } }\n",
        ),
        &[],
    );
    assert_eq!(
        error,
        ".alint.yml: `rules[0].input.from.paths`: `{{...}}` is not allowed in an inline \
         `input:` relation; declare the relation under `relations:`, where a value may read \
         the environment, and name it"
    );
}

#[test]
fn a_qualified_input_resolves_through_an_extends_alias() {
    let base = format!("version: 2\n{NAMES}rules: []\n");
    let config = builds(
        "version: 2\nextends:\n  - { url: ./base.yml, as: base }\nrules:\n  - id: c\n    kind: \
         constraint\n    input: base.names\n    assert: { unique: { key: [name] } }\n    \
         level: error\n",
        &[("base.yml", &base)],
    );
    let plan = config.relation_plan.as_ref().expect("a plan");
    let reference = &plan.rule("c").unwrap().references[0];
    assert_eq!(plan.relation(reference.relation.id).name, "names");
    // The bare name is refused: the root does not declare it.
    let (_dir, loaded) = load_tree(
        "version: 2\nextends:\n  - { url: ./base.yml, as: base }\nrules:\n  - id: c\n    kind: \
         constraint\n    input: names\n    assert: { unique: { key: [name] } }\n    level: \
         error\n",
        &[("base.yml", &base)],
    );
    let error = loaded.expect_err("a bare name").to_string();
    assert!(error.contains("base.names"), "{error}");
}

#[test]
fn an_input_names_a_relation_not_a_selector() {
    let error = load_error(
        &v2(
            "selectors:\n  manifests: { paths: \"**/package.json\" }\n",
            "    input: manifests\n    assert: { unique: { key: [name] } }\n",
        ),
        &[],
    );
    assert!(error.contains("manifests"), "{error}");
    assert!(error.contains("selector"), "{error}");
}

// --- Names stay literal (section 10) -----------------------------------------

#[test]
fn a_name_in_an_assertion_cannot_be_interpolated() {
    for (rest, at) in [
        (
            "    input: \"{{env.R}}\"\n    assert: { unique: { key: [name] } }\n",
            "rules[0].input",
        ),
        (
            "    input: names\n    assert: { unique: { key: [\"{{env.K}}\"] } }\n",
            "rules[0].assert.unique.key[0]",
        ),
        (
            "    input: names\n    assert: { unique: { key: [name], determines: \
             [\"{{env.K}}\"] } }\n",
            "rules[0].assert.unique.determines[0]",
        ),
        (
            "    input: names\n    assert: { equal: { field: \"{{env.F}}\" } }\n",
            "rules[0].assert.equal.field",
        ),
        (
            "    input: names\n    assert: { equal: { field: name, group_by: \"{{env.G}}\" } \
             }\n",
            "rules[0].assert.equal.group_by",
        ),
        (
            "    input: names\n    assert: { equal: { field: name, to: { field: \"{{env.G}}\" \
             } } }\n",
            "rules[0].assert.equal.to.field",
        ),
        (
            "    input: names\n    assert: { equal: { field: name, to: { row: { \"{{env.G}}\": \
             x } } } }\n",
            "rules[0].assert.equal.to.row.{{env.G}}",
        ),
        (
            "    input: names\n    assert: { set: { field: name, group_by: \"{{env.G}}\", \
             equal_to: { group: en } } }\n",
            "rules[0].assert.set.group_by",
        ),
        (
            "    input: names\n    assert: { set: { field: name, subset_of: { relation: \
             \"{{env.R}}\", field: name } } }\n",
            "rules[0].assert.set.subset_of.relation",
        ),
        (
            "    input: names\n    assert: { set: { field: name, subset_of: { relation: names, \
             field: \"{{env.F}}\" } } }\n",
            "rules[0].assert.set.subset_of.field",
        ),
    ] {
        assert_eq!(
            load_error(&v2(NAMES, rest), &[]),
            format!(
                ".alint.yml: `{at}`: `{{{{...}}}}` is not allowed in a name, alias or reference \
                 of a version 2 config, which must name the same thing in every environment; \
                 write it literally"
            ),
            "{rest}"
        );
    }
}

#[test]
fn a_literal_may_read_the_environment() {
    // `to.row`'s literal and a set's group are values, not names. What a
    // message shows of one is tested in `constraint_families.rs`.
    builds(
        &v2(
            NAMES,
            "    input: names\n    assert: { equal: { field: name, to: { row: { manifest: \
             \"{{env.ALINT_TEST_UNSET_ROOT | default('package.json')}}\" } } } }\n",
        ),
        &[],
    );
    builds(
        &v2(
            DEPS,
            "    input: deps\n    assert: { set: { field: dependency, group_by: range, \
             equal_to: { group: \"{{env.ALINT_TEST_UNSET_GROUP | default('x')}}\" } } }\n",
        ),
        &[],
    );
}

// --- The builder's check (section 8, "plan check") --------------------------

#[test]
fn every_field_an_assertion_names_is_a_field_of_its_relation() {
    let unknown = |rest: &str, at: &str, name: &str| {
        assert_eq!(
            build_error(&v2(NAMES, &format!("    input: names\n{rest}")), "c"),
            format!(
                "rule \"c\": `{at}`: `{name}` is not a field of relation `names` (.alint.yml); \
                 its fields are `name`, `manifest`"
            ),
            "{rest}"
        );
    };
    unknown(
        "    assert: { unique: { key: [nmae] } }\n",
        "assert.unique.key[0]",
        "nmae",
    );
    unknown(
        "    assert: { unique: { key: [name], determines: [vresion] } }\n",
        "assert.unique.determines[0]",
        "vresion",
    );
    unknown(
        "    assert: { equal: { field: nmae } }\n",
        "assert.equal.field",
        "nmae",
    );
    unknown(
        "    assert: { equal: { field: name, group_by: dir } }\n",
        "assert.equal.group_by",
        "dir",
    );
    unknown(
        "    assert: { equal: { field: name, to: { field: other } } }\n",
        "assert.equal.to.field",
        "other",
    );
    unknown(
        "    assert: { equal: { field: name, to: { row: { file: x } } } }\n",
        "assert.equal.to.row.file",
        "file",
    );
    unknown(
        "    assert: { set: { field: nmae, group_by: manifest, equal_to: { group: x } } }\n",
        "assert.set.field",
        "nmae",
    );
    unknown(
        "    assert: { set: { field: name, group_by: locale, equal_to: { group: x } } }\n",
        "assert.set.group_by",
        "locale",
    );
    unknown(
        "    assert: { unique: { key: [name] } }\n    message: \"{{row.nmae}} again\"\n",
        "message",
        "nmae",
    );
    // A placeholder of one character or none before it is skipped whole.
    for before in ["{{x}}", "{{}}"] {
        unknown(
            &format!(
                "    assert: {{ unique: {{ key: [name] }} }}\n    message: \"{before} \
                 {{{{row.nmae}}}}\"\n"
            ),
            "message",
            "nmae",
        );
    }
}

#[test]
fn an_inline_relation_is_named_by_where_it_is_written() {
    assert_eq!(
        build_error(
            &v2(
                "",
                "    input:\n      from: { paths: x }\n      read: { whole_file: {} }\n      \
                 fields: { f: path }\n    assert: { unique: { key: [g] } }\n",
            ),
            "c",
        ),
        "rule \"c\": `assert.unique.key[0]`: `g` is not a field of the inline relation at \
         `rules[0].input` (.alint.yml); its fields are `f`"
    );
}

#[test]
fn an_unknown_option_and_a_missing_one_are_refused() {
    let error = build_error(
        &v2(
            NAMES,
            "    input: names\n    assert: { unique: { key: [name], keys: [name] } }\n",
        ),
        "c",
    );
    assert!(error.contains("unknown field `keys`"), "{error}");
    let error = build_error(
        &v2(NAMES, "    input: names\n    assrt: { unique: {} }\n"),
        "c",
    );
    assert!(error.contains("unknown field `assrt`"), "{error}");
}

#[test]
fn the_assertion_is_one_family_of_one_shape() {
    let refused = |rest: &str, expected: &str| {
        assert_eq!(
            build_error(&v2(DEPS, &format!("    input: deps\n{rest}")), "c"),
            format!("rule \"c\": {expected}"),
            "{rest}"
        );
    };
    refused(
        "    assert: {}\n",
        "`assert`: holds exactly one of `equal`, `unique` and `set`; found none",
    );
    refused(
        "    assert: { unique: { key: [dependency] }, equal: { field: range } }\n",
        "`assert`: holds exactly one of `equal`, `unique` and `set`; found `equal` and `unique`",
    );
    refused(
        "    assert: { unique: { key: [] } }\n",
        "`assert.unique.key`: `key:` needs at least one field name",
    );
    refused(
        "    assert: { unique: { key: [dependency, dependency] } }\n",
        "`assert.unique.key[1]`: `dependency` is listed twice",
    );
    refused(
        "    assert: { unique: { key: [dependency], determines: [dependency] } }\n",
        "`assert.unique.determines[0]`: `dependency` is part of the key, which determines it \
         trivially",
    );
    refused(
        "    assert: { equal: { field: range, group_by: range } }\n",
        "`assert.equal.group_by`: groups by `range`, the field it compares, so every group agrees",
    );
    refused(
        "    assert: { equal: { field: range, group_by: dependency, to: { field: at } } }\n",
        "`assert.equal.group_by`: an `equal` with `to:` compares every row with one row or \
         field, and takes no `group_by:`",
    );
    refused(
        "    assert: { equal: { field: range, to: { field: range } } }\n",
        "`assert.equal.to.field`: compares `range` with itself",
    );
    refused(
        "    assert: { equal: { field: range, to: { row: {} } } }\n",
        "`assert.equal.to.row`: names the source row by one or more fields and their literals; \
         it is empty",
    );
    refused(
        "    assert: { set: { field: dependency, equal_to: { group: x } } }\n",
        "`assert.set.equal_to`: a reference group (`{ group: ... }`) is a value of the \
         `group_by:` field, and this `set` has no `group_by:`",
    );
    refused(
        "    assert: { set: { field: dependency, group_by: dependency, equal_to: { group: x } \
         } }\n",
        "`assert.set.group_by`: groups by `dependency`, the field whose values it compares",
    );
    refused(
        "    assert: { set: { field: dependency, group_by: range, equal_to: { group: x }, \
         subset_of: { group: y } } }\n",
        "`assert.set`: holds exactly one of `equal_to:`, `subset_of:` and `disjoint_from:`",
    );
    refused(
        "    assert: { set: { field: dependency, group_by: range, equal_to: {} } }\n",
        "`assert.set.equal_to`: names the other set: `{ group: literal }` (a group of this \
         relation) or `{ relation: R, field: F }` (every value of another relation's field)",
    );
}

/// `refused(rest, at, message)`: rule `c` over `objects`, with `rest` as its
/// `input:` and `assert:`, is refused at `at` with `message`.
fn refused_with(objects: &str) -> impl Fn(&str, &str, &str) + '_ {
    move |rest: &str, at: &str, message: &str| {
        assert_eq!(
            build_error(&v2(objects, rest), "c"),
            format!("rule \"c\": `{at}`: {message}"),
            "{rest}"
        );
    }
}

/// Relation `keys` of each locale file's leaves: `key` below the locale,
/// `lowered` lowercased too.
const KEYS: &str = "relations:\n  keys:\n    from: { paths: \"locales/*.json\" }\n    entries: { \
                    format: json, at: \"$\", leaves: true }\n    fields: { locale: entry.root, key: \
                    entry.path, lowered: entry.path, name: entry.key }\n    transform: { key: \
                    [strip-root], lowered: [strip-root, lower] }\n  joined:\n    join: { left: \
                    keys, right: keys, on: { name: name }, unique: right }\n    fields: { locale: \
                    left.locale, key: left.key }\n";

#[test]
fn plurals_cldr_merges_the_key_paths_of_an_entry_path_field_against_a_group() {
    let refused = refused_with(KEYS);
    refused(
        "    input: keys\n    assert: { set: { field: name, group_by: locale, equal_to: { group: \
         en }, plurals: cldr } }\n",
        "assert.set.plurals",
        "`plurals: cldr` merges key paths, so `field:` must be an `entry.path` field (or a \
         join's copy of one), and `name` is not",
    );
    refused(
        "    input: keys\n    assert: { set: { field: lowered, group_by: locale, equal_to: { \
         group: en }, plurals: cldr } }\n",
        "assert.set.plurals",
        "`plurals: cldr` merges key paths by their segments, and `transform:` applies `lower` \
         to `lowered` in relation `keys` (.alint.yml), which changes its text but not its \
         segments; only `strip-root` may apply to the field",
    );
    refused(
        "    input: keys\n    assert: { set: { field: key, subset_of: { relation: keys, field: \
         key }, plurals: cldr } }\n",
        "assert.set.plurals",
        "`plurals: cldr` decides each plural group over the key paths of two groups of one \
         relation, so it takes a `{ group: }` target, not `{ relation: R, field: F }`",
    );
    // An `entry.path` field, after `strip-root`, or a join's copy of one.
    for input in ["keys", "joined"] {
        builds(
            &v2(
                KEYS,
                &format!(
                    "    input: {input}\n    assert: {{ set: {{ field: key, group_by: locale, \
                     equal_to: {{ group: en }}, plurals: cldr }} }}\n"
                ),
            ),
            &[],
        );
    }
}

#[test]
fn groups_lists_literals_of_the_group_by_field_once_each() {
    let refused = refused_with(DEPS);
    let set = |rest: &str| {
        format!("    input: deps\n    assert: {{ set: {{ field: dependency, {rest} }} }}\n")
    };
    refused(
        &set("subset_of: { relation: deps, field: dependency }, groups: [x]"),
        "assert.set.groups",
        "names groups, which are values of the `group_by:` field, and this `set` has no \
         `group_by:`",
    );
    // `subset_of:` and `disjoint_from:` find nothing in a group without
    // rows, so a group that must exist needs `equal_to:`.
    for mode in ["subset_of", "disjoint_from"] {
        refused(
            &set(&format!(
                "group_by: range, {mode}: {{ group: x }}, groups: [y]"
            )),
            "assert.set.groups",
            &format!(
                "names groups that must exist, which only `equal_to:` checks: under `{mode}:` a \
                 group without rows holds no value, so it passes; compare with `equal_to:`, or \
                 drop `groups:`"
            ),
        );
    }
    refused(
        &set("group_by: range, equal_to: { group: x }, groups: [y, z, y]"),
        "assert.set.groups[2]",
        "names the group `assert.set.groups[0]` names",
    );
    refused(
        &set("group_by: range, equal_to: { group: x }, groups: [y, x]"),
        "assert.set.groups[1]",
        "names the reference group, which must have rows anyway (a reference group without \
         rows is an analysis failure); list only the groups compared with it",
    );
    refused(
        &set("group_by: range, equal_to: { group: x }, groups: [1.10]"),
        "assert.set.groups[0]",
        "a number cannot be written as a literal in this build: a value compares by the text \
         it is written with (`1.10` is not `1.1`), which the config's YAML does not keep; write \
         it as a string (`'1.10'`) to match a string value",
    );
    refused(
        &set("group_by: at, equal_to: { group: x }, groups: [true]"),
        "assert.set.groups[0]",
        "`at` holds strings, and a boolean never equals a string; quote it (`'true'`) to match \
         that text",
    );
    // A boolean names a group of a field that can hold one, and an empty
    // list names none.
    builds(
        &v2(
            DEPS,
            &set("group_by: range, equal_to: { group: x }, groups: [true, y]"),
        ),
        &[],
    );
    builds(
        &v2(
            DEPS,
            &set("group_by: range, equal_to: { group: x }, groups: []"),
        ),
        &[],
    );
}

#[test]
fn a_set_against_another_relation_names_the_relation_and_its_field() {
    let objects = format!("{DEPS}{}", &NAMES["relations:\n".len()..]);
    let refused = refused_with(&objects);
    let set = |target: &str| {
        format!(
            "    input: deps\n    assert: {{ set: {{ field: dependency, subset_of: {target} }} }}\n"
        )
    };
    refused(
        &set("{ relation: names }"),
        "assert.set.subset_of",
        "names another relation's field as `{ relation: R, field: F }`, and `field:` is missing",
    );
    refused(
        &set("{ field: name }"),
        "assert.set.subset_of",
        "names another relation's field as `{ relation: R, field: F }`, and `relation:` is \
         missing",
    );
    refused(
        &set("{ group: x, relation: names, field: name }"),
        "assert.set.subset_of",
        "names one other set: `{ group: literal }` (a group of this relation) or `{ relation: \
         R, field: F }` (every value of another relation's field), not both",
    );
    refused(
        &set("{ relation: names, field: nmae }"),
        "assert.set.subset_of.field",
        "`nmae` is not a field of relation `names` (.alint.yml); its fields are `name`, \
         `manifest`",
    );
    // The loader resolves the relation as it resolves `input:`.
    let error = load_error(&v2(&objects, &set("{ relation: nmaes, field: name }")), &[]);
    assert!(
        error.contains("`assert.set.subset_of.relation` (written by .alint.yml)")
            && error.contains("nmaes"),
        "{error}"
    );
    assert_eq!(
        load_error(
            &v2(
                &objects,
                &set(
                    "{ relation: { from: { paths: x }, read: { whole_file: {} }, fields: { f: \
                      path } }, field: f }"
                )
            ),
            &[]
        ),
        "rule \"c\": `assert.set.subset_of.relation` (written by .alint.yml): names a relation, \
         bare or qualified by an `extends:` alias; an inline relation is not allowed here, so \
         declare it under `relations:` and name it"
    );
    assert_eq!(
        load_error(
            &v2(&objects, &set("{ relation: [names], field: name }")),
            &[]
        ),
        "rule \"c\": `assert.set.subset_of.relation` (written by .alint.yml): expected a \
         relation name, found a list"
    );
    // The rule reads the relation, which the plan lists as its reference.
    let config = builds(&v2(&objects, &set("{ relation: names, field: name }")), &[]);
    let plan = config.relation_plan.as_ref().expect("a plan");
    let references: Vec<(&str, &str)> = plan
        .rule("c")
        .unwrap()
        .references
        .iter()
        .map(|reference| {
            (
                reference.field.as_str(),
                plan.relation(reference.relation.id).name.as_str(),
            )
        })
        .collect();
    assert_eq!(
        references,
        [
            ("input", "deps"),
            ("assert.set.subset_of.relation", "names")
        ]
    );
}

/// A `set` against a sensitive field of another relation makes the compared
/// field sensitive in the plan `explain` and `list` show, with its family
/// (every entry field, here), and a relation that reads it follows.
#[test]
fn a_set_against_a_sensitive_field_makes_the_compared_family_sensitive() {
    let objects = format!(
        "{DEPS}  denied:\n    from: {{ paths: denied.json }}\n    entries: {{ format: json, at: \
         \"$\" }}\n    fields: {{ name: {{ source: entry.key, sensitive: true }} }}\n  later:\n    \
         from: {{ paths: later.json }}\n    entries: {{ format: json, at: \"$\" }}\n    fields: \
         {{ name: entry.key, where_from: path }}\n    where: {{ name: {{ in: [{{ relation: deps, \
         field: dependency }}] }} }}\n"
    );
    let sensitive = |config: &Config, relation: &str| -> Vec<String> {
        let plan = config.relation_plan.as_ref().expect("a plan");
        let named = plan
            .relations
            .iter()
            .find(|named| named.name == relation)
            .unwrap();
        named
            .relation
            .fields
            .iter()
            .filter(|(_, field)| field.sensitive)
            .map(|(name, _)| name.clone())
            .collect()
    };
    let rest = "    input: deps\n    assert: { set: { field: dependency, disjoint_from: { \
                relation: denied, field: name } } }\n";
    let config = builds(&v2(&objects, rest), &[]);
    assert_eq!(sensitive(&config, "deps"), ["dependency", "range"]);
    assert_eq!(sensitive(&config, "later"), ["name"]);
    // A group target, or another relation's field that is not sensitive,
    // changes nothing.
    let rest = "    input: deps\n    assert: { set: { field: dependency, group_by: at, \
                disjoint_from: { group: x } } }\n";
    assert_eq!(
        sensitive(&builds(&v2(&objects, rest), &[]), "deps"),
        [] as [String; 0]
    );
}

#[test]
fn a_set_names_a_relation_qualified_by_an_extends_alias() {
    let base = format!("version: 2\n{NAMES}rules: []\n");
    let config = format!(
        "version: 2\nextends:\n  - {{ url: ./base.yml, as: base }}\n{DEPS}rules:\n  - id: c\n    \
         kind: constraint\n    input: deps\n    assert: {{ set: {{ field: dependency, \
         disjoint_from: {{ relation: RELATION, field: name }} }} }}\n    level: error\n"
    );
    let loaded = builds(
        &config.replace("RELATION", "base.names"),
        &[("base.yml", &base)],
    );
    let plan = loaded.relation_plan.as_ref().expect("a plan");
    let reference = &plan.rule("c").unwrap().references[1];
    assert_eq!(plan.relation(reference.relation.id).name, "names");
    assert_eq!(reference.relation.alias.as_deref(), Some("base"));
    // The bare name is not the root's.
    let error = load_error(&config.replace("RELATION", "names"), &[("base.yml", &base)]);
    assert!(error.contains("base.names"), "{error}");
}

#[test]
fn a_set_grouped_by_a_path_token_reads_the_resources_its_relation_lists() {
    let objects = format!(
        "{DEPS}{}  linked:\n    join: {{ left: deps, right: names, on: {{ dependency: name }}, \
         unique: right }}\n    fields: {{ dependency: left.dependency, mine: left.at, theirs: \
         right.manifest }}\n",
        &NAMES["relations:\n".len()..]
    );
    let set = |group_by: &str| {
        format!(
            "    input: linked\n    assert: {{ set: {{ field: dependency, group_by: {group_by}, \
             subset_of: {{ relation: names, field: name }} }} }}\n"
        )
    };
    refused_with(&objects)(
        &set("theirs"),
        "assert.set.group_by",
        "`theirs` is a path token that relation `linked` (.alint.yml) copies from its \
         `unique:` side; a `set` grouped by a path token gives every resource the relation \
         reads a group, and a join reads the resources of its other side, so group by a field \
         of that side",
    );
    builds(&v2(&objects, &set("mine")), &[]);
    // A field copied from the `unique:` side that is no path token groups
    // as any field does.
    let named = objects.replace(
        "theirs: right.manifest",
        "theirs: right.manifest, name: right.name",
    );
    builds(&v2(&named, &set("name")), &[]);
    // A join's copy of a join's copy of a path token is one too, from
    // whichever side of the inner join it came.
    let outer = format!(
        "{objects}  outer:\n    join: {{ left: names, right: linked, on: {{ name: dependency }}, \
         unique: right }}\n    fields: {{ name: left.name, copied: right.mine }}\n"
    );
    refused_with(&outer)(
        "    input: outer\n    assert: { set: { field: name, group_by: copied, subset_of: { \
         relation: names, field: name } } }\n",
        "assert.set.group_by",
        "`copied` is a path token that relation `outer` (.alint.yml) copies from its `unique:` \
         side; a `set` grouped by a path token gives every resource the relation reads a group, \
         and a join reads the resources of its other side, so group by a field of that side",
    );
    // What a join copies holds what its side's field holds: a boolean never
    // equals a path.
    refused_with(&objects)(
        "    input: linked\n    assert: { equal: { field: dependency, to: { row: { theirs: true \
         } } } }\n",
        "assert.equal.to.row.theirs",
        "`theirs` holds strings, and a boolean never equals a string; quote it (`'true'`) to \
         match that text",
    );
}

#[test]
fn a_literal_is_a_string_or_a_boolean_that_can_match() {
    let refused = |literal: &str, field: &str, expected: &str| {
        let rest = format!(
            "    input: deps\n    assert: {{ equal: {{ field: dependency, to: {{ row: {{ \
             {field}: {literal} }} }} }} }}\n"
        );
        let field_at = format!("assert.equal.to.row.{field}");
        assert_eq!(
            build_error(&v2(DEPS, &rest), "c"),
            format!("rule \"c\": `{field_at}`: {expected}"),
            "{literal}"
        );
    };
    refused(
        "1.10",
        "range",
        "a number cannot be written as a literal in this build: a value compares by the text \
         it is written with (`1.10` is not `1.1`), which the config's YAML does not keep; write \
         it as a string (`'1.10'`) to match a string value",
    );
    refused(
        "~",
        "range",
        "null matches nothing, so no row ever holds it; write a string or a boolean",
    );
    refused(
        "true",
        "at",
        "`at` holds strings, and a boolean never equals a string; quote it (`'true'`) to match \
         that text",
    );
    refused("[x]", "range", "expected a literal: a string or a boolean");
    // The loader refuses a tag other than a core-schema one before the
    // builder sees it.
    let error = load_error(
        &v2(
            DEPS,
            "    input: deps\n    assert: { equal: { field: dependency, to: { row: { range: \
             !custom x } } } }\n",
        ),
        &[],
    );
    assert!(error.contains("has the YAML tag `!custom`"), "{error}");
    // A boolean can match a value an entry holds; a string matches anything
    // that holds strings.
    for (literal, field) in [
        ("true", "range"),
        ("'true'", "at"),
        ("x", "dependency"),
        ("!!str 1.10", "range"),
    ] {
        builds(
            &v2(
                DEPS,
                &format!(
                    "    input: deps\n    assert: {{ equal: {{ field: dependency, to: {{ row: {{ \
                     {field}: {literal} }} }} }} }}\n"
                ),
            ),
            &[],
        );
    }
    // The set's group is checked as a literal of `group_by:`.
    assert_eq!(
        build_error(
            &v2(
                DEPS,
                "    input: deps\n    assert: { set: { field: range, group_by: dependency, \
                 equal_to: { group: 2 } } }\n",
            ),
            "c",
        ),
        "rule \"c\": `assert.set.equal_to.group`: a number cannot be written as a literal in this \
         build: a value compares by the text it is written with (`1.10` is not `1.1`), which \
         the config's YAML does not keep; write it as a string (`'1.10'`) to match a string \
         value"
    );
}

#[test]
fn a_rule_built_without_a_plan_names_version_two() {
    // A spec no loader resolved (an embedder's, or a test's) has no plan.
    let spec: alint_core::RuleSpec = alint_core::yaml::from_str(
        "id: c\nkind: constraint\nlevel: error\ninput: names\nassert: { unique: { key: [name] } \
         }\n",
    )
    .unwrap();
    let error = match test_registry().build(&spec) {
        Ok(_) => panic!("built without a plan"),
        Err(error) => error.to_string(),
    };
    assert_eq!(
        error,
        "rule \"c\": a `constraint` rule reads a relation, which only a config of `version: 2` \
         declares (the kind is experimental); a version 1 config, a rule nested under \
         `require:` and a rule built outside a configuration have none"
    );
}
