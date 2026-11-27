//! The draft version 2 schema (`schemas/v2/config.json`, experimental) against
//! the version 2 configs of the end-to-end corpora (design
//! `v0.23/selectors-and-constraints.md` section 11). alint-dsl's unit tests
//! hold the schema to the decode on every config they decode and on chosen
//! refusals; this file adds the configs the users of alint see:
//! - every policy test whose config declares `version: 2`, or uses what only
//!   version 2 has, validates against the schema for its version when it
//!   loads; when it is refused, [`REFUSALS`] says whether the schema refuses
//!   it too (its shape) or leaves it to the loader;
//! - the version 2 files those tests extend or add as `.alint.d` fragments;
//! - the version 2 configs of the CLI snapshots (`crates/alint/tests/cli`),
//!   which the schema accepts exactly when they load;
//! - every example of the version 2 docs page
//!   (`docs/site/configuration/version-2.md`), which loads and validates.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value as Json;

use crate::support;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn schema(relative: &str) -> jsonschema::Validator {
    let path = workspace().join(relative);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let schema: Json = serde_json::from_str(&text).expect("a schema is JSON");
    jsonschema::validator_for(&schema).expect("the schema compiles")
}

fn draft() -> &'static jsonschema::Validator {
    static DRAFT: OnceLock<jsonschema::Validator> = OnceLock::new();
    DRAFT.get_or_init(|| schema("schemas/v2/config.json"))
}

fn version_one() -> &'static jsonschema::Validator {
    static ONE: OnceLock<jsonschema::Validator> = OnceLock::new();
    ONE.get_or_init(|| schema("schemas/v1/config.json"))
}

fn json_of(yaml: &str) -> Json {
    let value: ayaml::Value = alint_core::yaml::from_str(yaml).expect("the YAML parses");
    serde_json::to_value(value).expect("the YAML has a JSON form")
}

/// Whether `config` uses version 2: it declares `version: 2`, or a key or an
/// `extends:` alias only version 2 has.
fn uses_version_two(config: &Json) -> bool {
    config["version"] == 2
        || config.get("selectors").is_some()
        || config.get("relations").is_some()
        || config["extends"]
            .as_array()
            .is_some_and(|entries| entries.iter().any(|entry| entry.get("as").is_some()))
}

/// The schema's errors for `config`, by the version it declares: the draft
/// for `version: 2`, the version 1 schema otherwise.
fn schema_errors(config: &Json) -> Vec<String> {
    let validator = if config["version"] == 2 {
        draft()
    } else {
        version_one()
    };
    validator
        .iter_errors(config)
        .map(|error| format!("{error} at `{}`", error.instance_path()))
        .collect()
}

/// Who refuses a refused version 2 policy test's config.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Refusal {
    /// Its shape: the schema for its version refuses it, as the loader does.
    Shape,
    /// What only the loader sees (a name, a cycle, the plan check, the source
    /// a config is): the schema accepts it.
    Loader,
}

/// Each version 2 policy test that expects a configuration error, by its
/// path below `scenarios/`.
const REFUSALS: &[(&str, Refusal)] = &[
    (
        "check/version_two/a_chained_name_is_refused.yml",
        Refusal::Shape,
    ),
    // The draft refuses a `constraint` rule under `require:`.
    (
        "check/version_two/a_constraint_cannot_be_nested.yml",
        Refusal::Shape,
    ),
    ("check/version_two/a_cycle_is_refused.yml", Refusal::Loader),
    (
        "check/version_two/a_name_cannot_be_interpolated.yml",
        Refusal::Shape,
    ),
    // The nested config in its tree is version 2; the top-level one is valid.
    (
        "check/version_two/a_nested_config_must_be_version_one.yml",
        Refusal::Loader,
    ),
    (
        "check/version_two/a_re_export_is_refused.yml",
        Refusal::Loader,
    ),
    (
        "check/version_two/a_relation_is_checked_at_load.yml",
        Refusal::Shape,
    ),
    (
        "check/version_two/a_union_part_must_bind_every_field.yml",
        Refusal::Loader,
    ),
    (
        "check/version_two/a_where_reference_names_a_field.yml",
        Refusal::Loader,
    ),
    // A version 1 config with `as:`: the version 1 schema refuses it.
    (
        "check/version_two/an_alias_needs_version_two.yml",
        Refusal::Shape,
    ),
    (
        "check/version_two/an_unknown_alias_is_refused.yml",
        Refusal::Loader,
    ),
    (
        "check/version_two/an_unknown_key_lists_the_version_two_keys.yml",
        Refusal::Shape,
    ),
    (
        "check/version_two/an_unknown_name_is_refused.yml",
        Refusal::Loader,
    ),
    (
        "check/version_two/message_value_placeholder_refused_on_non_set_reference.yml",
        Refusal::Loader,
    ),
    // A version 1 config with `selectors:`: the version 1 schema refuses it.
    (
        "check/version_two/selectors_need_version_two.yml",
        Refusal::Shape,
    ),
];

/// Each policy test below `scenarios/` that uses version 2: its path below
/// there, its config and whether it expects a configuration error.
fn version_two_policy_tests() -> Vec<(String, Json, bool)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    let mut tests = Vec::new();
    for (path, yaml) in support::policy_test_sources(&root) {
        let test = json_of(&yaml);
        let Some(config) = test["given"]["config"].as_str() else {
            continue;
        };
        let config = json_of(config);
        if !uses_version_two(&config) {
            continue;
        }
        let refused = test["expect"].as_array().is_some_and(|steps| {
            steps
                .iter()
                .any(|step| step["exit"] == "configuration_error")
        });
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        tests.push((relative, config, refused));
    }
    tests.sort_by(|a, b| a.0.cmp(&b.0));
    tests
}

#[test]
fn each_version_two_policy_test_validates_as_it_loads() {
    let refusals: BTreeMap<&str, Refusal> = REFUSALS.iter().copied().collect();
    let tests = version_two_policy_tests();
    let mut loads = 0;
    for (path, config, refused) in &tests {
        let errors = schema_errors(config);
        if !refused {
            assert!(
                errors.is_empty(),
                "{path} loads, and the schema refuses its config:\n{}",
                errors.join("\n")
            );
            loads += 1;
            continue;
        }
        let refusal = refusals.get(path.as_str()).unwrap_or_else(|| {
            panic!(
                "{path} expects a configuration error: add it to `REFUSALS`, as `Shape` if the \
                 schema refuses its config ({errors:?}) and `Loader` if it does not"
            )
        });
        match refusal {
            Refusal::Shape => assert!(
                !errors.is_empty(),
                "{path}: the schema accepts a config refused for its shape"
            ),
            Refusal::Loader => assert!(
                errors.is_empty(),
                "{path}: the schema refuses a config only the loader should:\n{}",
                errors.join("\n")
            ),
        }
    }
    for (path, _) in REFUSALS {
        assert!(
            tests
                .iter()
                .any(|(test, _, refused)| test == path && *refused),
            "`REFUSALS` lists {path}, which is not a refused version 2 policy test"
        );
    }
    assert!(loads >= 3, "{loads} version 2 policy tests load");
}

/// Every YAML file below `root` that is not a policy test.
fn other_yaml(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            other_yaml(&path, out);
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("yml" | "yaml")
        ) {
            let text = std::fs::read_to_string(&path).unwrap();
            if !alint_testkit::has_policy_test_discriminator(&text) {
                out.push(path);
            }
        }
    }
}

#[test]
fn the_version_two_files_policy_tests_extend_validate() {
    let mut files = Vec::new();
    other_yaml(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios"),
        &mut files,
    );
    let mut checked = 0;
    for path in files {
        let mut config = json_of(&std::fs::read_to_string(&path).unwrap());
        if !config.is_object() || !uses_version_two(&config) {
            continue;
        }
        // An `.alint.d` fragment without a `version:` is read as its root's.
        let fragment = path
            .parent()
            .is_some_and(|parent| parent.ends_with(".alint.d"));
        if fragment && config.get("version").is_none() {
            config["version"] = Json::from(2);
        }
        let errors = schema_errors(&config);
        assert!(
            errors.is_empty(),
            "{}:\n{}",
            path.display(),
            errors.join("\n")
        );
        checked += 1;
    }
    assert!(checked >= 4, "{checked} version 2 files");
}

#[test]
fn the_version_two_cli_snapshots_validate_as_they_load() {
    let cli = workspace().join("crates/alint/tests/cli");
    let mut checked = 0;
    for entry in std::fs::read_dir(&cli).unwrap() {
        let input = entry.unwrap().path();
        if input.extension().and_then(|extension| extension.to_str()) != Some("in") {
            continue;
        }
        let mut files = Vec::new();
        other_yaml(&input, &mut files);
        for path in files {
            // Some snapshots test YAML that does not parse.
            let text = std::fs::read_to_string(&path).unwrap();
            let Ok(value) = alint_core::yaml::from_str::<ayaml::Value>(&text) else {
                continue;
            };
            let config = serde_json::to_value(value).expect("the YAML has a JSON form");
            if !config.is_object() || !uses_version_two(&config) {
                continue;
            }
            let errors = schema_errors(&config);
            // A top-level config validates exactly when it loads; a file it
            // extends loads with it.
            if path.file_name().is_some_and(|name| name == ".alint.yml") {
                let loaded = support::load(&path);
                assert_eq!(
                    loaded.is_ok(),
                    errors.is_empty(),
                    "{}: loads: {:?}; schema: {errors:?}",
                    path.display(),
                    loaded.err().map(|error| error.to_string())
                );
            } else {
                assert!(
                    errors.is_empty(),
                    "{}:\n{}",
                    path.display(),
                    errors.join("\n")
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= 6, "{checked} version 2 CLI configs");
}

/// The fenced `yaml` blocks of `markdown`, each with the line it starts on.
fn yaml_blocks(markdown: &str) -> Vec<(usize, String)> {
    let mut blocks = Vec::new();
    let mut lines = markdown.lines().enumerate();
    while let Some((number, line)) = lines.next() {
        if line.trim() != "```yaml" {
            continue;
        }
        let mut body = String::new();
        for (_, line) in lines.by_ref() {
            if line.trim() == "```" {
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        blocks.push((number + 1, body));
    }
    blocks
}

/// Every `yaml` example of the version 2 page loads and validates against the
/// schema for its declared version (version 1 for the specialized comparison
/// spellings, the draft version 2 schema for the rest). A block whose first
/// line is `# file: <path>` is a file the others extend (or an `.alint.d/`
/// fragment): it is written there, beside the `.alint.yml` each other block is
/// loaded as.
#[test]
fn the_version_two_page_examples_load_and_validate() {
    let page = workspace().join("docs/site/configuration/version-2.md");
    let text = std::fs::read_to_string(&page).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let (files, configs): (Vec<_>, Vec<_>) = yaml_blocks(&text)
        .into_iter()
        .partition(|(_, body)| body.starts_with("# file: "));
    for (line, body) in &files {
        let relative = body.lines().next().unwrap().trim_start_matches("# file: ");
        let path = tmp.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        let mut config = json_of(body);
        if relative.starts_with(".alint.d/") && config.get("version").is_none() {
            config["version"] = Json::from(2);
        }
        let errors = schema_errors(&config);
        assert!(errors.is_empty(), "line {line}:\n{}", errors.join("\n"));
    }
    let root = tmp.path().join(".alint.yml");
    let v1_tmp = tempfile::tempdir().unwrap();
    let v1_root = v1_tmp.path().join(".alint.yml");
    let mut v1_configs = 0;
    let mut v2_configs = 0;
    for (line, body) in &configs {
        let config = json_of(body);
        let target = match config["version"].as_u64() {
            Some(1) => {
                v1_configs += 1;
                &v1_root
            }
            Some(2) => {
                v2_configs += 1;
                &root
            }
            other => panic!("line {line}: unexpected config version {other:?}"),
        };
        std::fs::write(target, body).unwrap();
        if let Err(error) = support::load(target) {
            panic!("the example at line {line} does not load: {error}\n{body}");
        }
        let errors = schema_errors(&config);
        assert!(errors.is_empty(), "line {line}:\n{}", errors.join("\n"));
    }
    assert_eq!((files.len(), v1_configs, v2_configs), (2, 2, 13));
}

#[test]
fn bundled_version_two_rulesets_and_tests_validate_against_draft_schema() {
    let rulesets_v2 = workspace().join("crates/alint-dsl/rulesets/v2");
    let mut ruleset_files = Vec::new();
    other_yaml(&rulesets_v2, &mut ruleset_files);
    assert!(
        !ruleset_files.is_empty(),
        "expected at least one bundled v2 ruleset under {}",
        rulesets_v2.display()
    );
    for path in &ruleset_files {
        let config = json_of(&std::fs::read_to_string(path).unwrap());
        assert_eq!(config["version"], 2, "{}", path.display());
        let errors = schema_errors(&config);
        assert!(
            errors.is_empty(),
            "bundled v2 ruleset {} failed schema validation:\n{}",
            path.display(),
            errors.join("\n")
        );
    }

    let ruleset_tests_v2 = workspace().join("crates/alint-dsl/ruleset-tests/v2");
    let tests = support::policy_test_sources(&ruleset_tests_v2);
    assert!(
        !tests.is_empty(),
        "expected at least one bundled v2 ruleset policy test under {}",
        ruleset_tests_v2.display()
    );
    for (path, yaml) in &tests {
        let test = json_of(yaml);
        let Some(config_str) = test["given"]["config"].as_str() else {
            continue;
        };
        let config = json_of(config_str);
        assert!(uses_version_two(&config), "{}", path.display());
        let errors = schema_errors(&config);
        assert!(
            errors.is_empty(),
            "bundled v2 ruleset test {} failed schema validation:\n{}",
            path.display(),
            errors.join("\n")
        );
    }
}
