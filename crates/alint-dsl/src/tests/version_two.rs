//! `version: 2` through the loader: dispatch at every source, the decoded
//! objects and their provenance, and the refusals (design
//! `v0.23/selectors-and-constraints.md`, sections 3 and 10).

use std::path::{Path, PathBuf};

use alint_core::ConfigSourceClass;

use super::*;

const OBJECTS: &str = r#"selectors:
  manifests:
    paths: "**/package.json"
relations:
  deps:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { name: entry.key, version: entry.value }
"#;

const README_RULE: &str = "rules:\n  - id: readme\n    kind: file_exists\n    paths: README.md\n    \
                           level: warning\n";

fn write(root: &Path, relative: &str, body: &str) -> PathBuf {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    path
}

fn options() -> LoadOptions<'static> {
    LoadOptions::with_catalog(test_catalog())
}

fn raw(path: &Path) -> Result<RawConfig> {
    load_top_level_raw(path, &options())
}

fn load_error(path: &Path) -> String {
    load(path).expect_err("expected a load error").to_string()
}

/// The names each source declared, by its display.
fn declared(raw: &RawConfig) -> Vec<(String, ConfigSourceClass, Vec<String>)> {
    raw.v2
        .sources
        .iter()
        .map(|source| {
            let names = source
                .selectors
                .iter()
                .map(|(name, _)| name.clone())
                .chain(source.relations.iter().map(|(name, _)| name.clone()))
                .chain(source.aliases.iter().map(|(name, _)| name.clone()))
                .collect();
            (source.source.display.clone(), source.source.class, names)
        })
        .collect()
}

#[test]
fn a_version_two_config_loads_and_keeps_its_rules() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        &format!("version: 2\n{OBJECTS}{README_RULE}"),
    );
    let config = load(&path).unwrap();
    assert_eq!(config.version, 2);
    assert_eq!(config.rules.len(), 1);
    assert_eq!(config.rules[0].id, "readme");
    assert_eq!(config.compatibility[0].config_version, 2);

    let raw = raw(&path).unwrap();
    assert_eq!(raw.surface, v2::Surface::V2);
    assert_eq!(
        declared(&raw),
        vec![(
            ".alint.yml".to_string(),
            ConfigSourceClass::Root,
            vec!["manifests".to_string(), "deps".to_string()]
        )]
    );
    assert_eq!(
        raw.v2.sources[0].source.path,
        path.canonicalize().unwrap(),
        "the source is kept as its resolved file"
    );
}

#[test]
fn a_version_two_config_reads_version_one_keys_as_version_one_does() {
    // The v1 part of a v2 document decodes through the same deserializer, so
    // an error in it reads the same, line and column included.
    let tmp = tempfile::tempdir().unwrap();
    let body = "rules:\n  - id: x\n    kind: file_exists\n    paths: [a\n";
    let one = write(tmp.path(), "one/.alint.yml", &format!("version: 1\n{body}"));
    let two = write(tmp.path(), "two/.alint.yml", &format!("version: 2\n{body}"));
    let strip =
        |error: String, dir: &str| error.replace(&tmp.path().join(dir).display().to_string(), "");
    assert_eq!(
        strip(load_error(&one), "one"),
        strip(load_error(&two), "two")
    );

    let body = "ignore: 3\nrules: []\n";
    let one = write(tmp.path(), "one/.alint.yml", &format!("version: 1\n{body}"));
    let two = write(tmp.path(), "two/.alint.yml", &format!("version: 2\n{body}"));
    let one_error = strip(load_error(&one), "one");
    assert!(one_error.contains("line 2 column 9"), "{one_error}");
    assert_eq!(one_error, strip(load_error(&two), "two"));

    // A v1 `extends:` entry error keeps its text (the v2 entry adds `as`).
    let one = write(
        tmp.path(),
        "one/.alint.yml",
        "version: 1\nextends:\n  - url: ./a.yml\n    excpet: [x]\n",
    );
    assert!(
        load_error(&one).ends_with(
            "YAML parse error: extends[0]: unknown field `excpet`, expected one of `url`, \
             `only`, `except` at line 3 column 5"
        ),
        "{}",
        load_error(&one)
    );
}

#[test]
fn a_version_one_config_is_refused_version_two_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", &format!("version: 1\n{OBJECTS}"));
    assert_eq!(
        load_error(&path),
        ".alint.yml: `selectors:` needs `version: 2`; this config is version 1"
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 1\nextends:\n  - url: ./base.yml\n    as: base\n",
    );
    assert_eq!(
        load_error(&path),
        ".alint.yml: `extends[0].as` needs `version: 2`; this config is version 1"
    );
}

#[test]
fn an_unknown_key_in_a_version_two_config_lists_the_version_two_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", "version: 2\nrelation: {}\n");
    assert_eq!(
        load_error(&path),
        ".alint.yml: unknown key `relation` in a version 2 config; expected one of `version`, \
         `requires`, `extends`, `ignore`, `respect_gitignore`, `vars`, `facts`, `templates`, \
         `rules`, `fix_size_limit`, `nested_configs`, `allow_out_of_root`, `baseline`, \
         `trusted_extends`, `selectors`, `relations`"
    );
}

#[test]
fn a_version_read_from_the_environment_is_version_one() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: '{{env.ALINT_V2_TEST_VERSION | default(\"2\")}}'\nrules: []\n",
    );
    assert_eq!(
        load_error(&path),
        ".alint.yml: unsupported config version 2 (this build supports 1 through 1)"
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: '{{env.ALINT_V2_TEST_VERSION | default(\"1\")}}'\nselectors: {}\n",
    );
    assert_eq!(
        load_error(&path),
        ".alint.yml: `selectors:` needs `version: 2`; this config's `version:` is not the \
         literal integer `2` (a version read from the environment, quoted, tagged or written \
         as an alias is version 1)"
    );
}

#[derive(Debug)]
struct Environment(&'static [(&'static str, &'static str)]);

impl crate::EnvironmentProvider for Environment {
    fn get(&self, name: &str) -> Option<String> {
        self.0
            .iter()
            .find(|(held, _)| *held == name)
            .map(|(_, value)| (*value).to_string())
    }
}

fn load_in(path: &Path, environment: &'static [(&'static str, &'static str)]) -> Result<Config> {
    let options = LoadOptions {
        environment: std::sync::Arc::new(Environment(environment)),
        ..options()
    };
    load_with(path, &options)
}

#[test]
fn a_version_two_value_interpolates_and_a_name_does_not() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nselectors:\n  docs:\n    paths: '{{env.DOCS}}/**'\n\
         relations:\n  titles:\n    from: docs\n    read: { regex: '{{env.TITLE}}' }\n    \
         fields: { title: value }\n",
    );
    let raw = {
        let options = LoadOptions {
            environment: std::sync::Arc::new(Environment(&[
                ("DOCS", "handbook"),
                ("TITLE", "^# (.*)"),
            ])),
            ..options()
        };
        load_top_level_raw(&path, &options).unwrap()
    };
    let source = &raw.v2.sources[0];
    assert_eq!(
        format!("{:?}", source.selectors[0].1.paths),
        format!("{:?}", alint_core::PathsSpec::Single("handbook/**".into()))
    );
    let v2::Rows::Part(part) = &source.relations[0].1.rows else {
        panic!("not a plain relation");
    };
    let v2::Reader::Read { read, .. } = &part.reader else {
        panic!("not a read");
    };
    assert_eq!(read, &v2::Read::Regex("^# (.*)".into()));

    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nrelations:\n  titles:\n    from: '{{env.SELECTOR}}'\n    \
         read: { whole_file: {} }\n    fields: { text: value }\n",
    );
    assert_eq!(
        load_in(&path, &[("SELECTOR", "docs")])
            .unwrap_err()
            .to_string(),
        ".alint.yml: `relations.titles.from`: `{{...}}` is not allowed in a name, alias or \
         reference of a version 2 config, which must name the same thing in every environment; \
         write it literally"
    );
}

#[test]
fn a_version_two_error_leaves_out_a_value_from_the_environment() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nrelations:\n  r:\n    from: { paths: '*.json' }\n    \
         entries: { format: '{{env.FORMAT}}', at: $ }\n    fields: { k: entry.key }\n",
    );
    let error = load_in(&path, &[("FORMAT", "hunter2")])
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        ".alint.yml: `relations.r.entries.format`: unknown format `<redacted>`; expected one of \
         `json`, `yaml`, `toml` (in this build entries are read from JSON, YAML and TOML only)"
    );
    // A load the environment fed carries the value for the embedder too.
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nselectors:\n  s: { paths: '{{env.DIR}}/**' }\nrules: []\n",
    );
    let config = load_in(&path, &[("DIR", "secret-dir")]).unwrap();
    assert!(
        config
            .environment_values
            .iter()
            .any(|value| value == "secret-dir"),
        "{:?}",
        config.environment_values
    );
}

#[test]
fn a_fragment_without_a_version_inherits_version_two() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", &format!("version: 2\n{OBJECTS}"));
    write(
        tmp.path(),
        ".alint.d/10-docs.yml",
        &format!(
            "selectors:\n  docs:\n    paths: docs/**\n  manifests:\n    paths: x\n{README_RULE}"
        ),
    );
    let raw = raw(&path).unwrap();
    assert_eq!(
        declared(&raw),
        vec![
            (
                ".alint.yml".to_string(),
                ConfigSourceClass::Root,
                vec!["manifests".to_string(), "deps".to_string()]
            ),
            (
                ".alint.d/10-docs.yml".to_string(),
                ConfigSourceClass::DropIn,
                // Each source has its own namespace: the fragment may reuse a
                // name its root declares.
                vec!["docs".to_string(), "manifests".to_string()]
            ),
        ]
    );
    assert_eq!(load(&path).unwrap().rules.len(), 1);
}

#[test]
fn a_fragment_follows_a_version_one_root() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(tmp.path(), ".alint.yml", "version: 1\nrules: []\n");
    let fragment = write(tmp.path(), ".alint.d/10.yml", "selectors: {}\n");
    assert_eq!(
        load_error(&path),
        ".alint.d/10.yml: `selectors:` needs `version: 2`; this config is version 1, as its \
         root config is"
    );
    std::fs::write(&fragment, "version: 2\nrules: []\n").unwrap();
    assert_eq!(
        load_error(&path),
        ".alint.d/10.yml: drop-in config version 2 must equal root config version 1"
    );
}

#[test]
fn a_version_two_config_extends_local_sources_through_aliases() {
    let tmp = tempfile::tempdir().unwrap();
    let base = write(
        tmp.path(),
        "rules/base.yml",
        &format!("version: 2\n{OBJECTS}"),
    );
    write(
        tmp.path(),
        "rules/plain.yml",
        &format!("version: 1\n{README_RULE}"),
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends:\n  - url: ./rules/base.yml\n    as: base\n  - ./rules/plain.yml\n\
         relations:\n  pinned:\n    from: base.manifests\n    read: { json: $.version }\n    \
         fields: { version: value }\n",
    );
    let raw = raw(&path).unwrap();
    assert_eq!(
        declared(&raw),
        vec![
            (
                "rules/base.yml".to_string(),
                ConfigSourceClass::LocalExtend,
                vec!["manifests".to_string(), "deps".to_string()]
            ),
            (
                ".alint.yml".to_string(),
                ConfigSourceClass::Root,
                vec!["pinned".to_string(), "base".to_string()]
            ),
        ]
    );
    let root = &raw.v2.sources[1];
    assert_eq!(
        root.aliases[0].1.target.as_deref(),
        Some(base.canonicalize().unwrap().as_path()),
        "an alias leads to the file it extends"
    );
    let config = load(&path).unwrap();
    assert_eq!(config.version, 2);
    assert_eq!(config.rules.len(), 1);
}

#[test]
fn a_source_reached_twice_declares_its_objects_once() {
    let tmp = tempfile::tempdir().unwrap();
    write(tmp.path(), "shared.yml", &format!("version: 2\n{OBJECTS}"));
    write(tmp.path(), "a.yml", "version: 2\nextends: [./shared.yml]\n");
    write(tmp.path(), "b.yml", "version: 2\nextends: [./shared.yml]\n");
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends: [./a.yml, ./b.yml]\n",
    );
    let raw = raw(&path).unwrap();
    assert_eq!(
        declared(&raw),
        vec![(
            "shared.yml".to_string(),
            ConfigSourceClass::LocalExtend,
            vec!["manifests".to_string(), "deps".to_string()]
        )]
    );
}

#[test]
fn a_version_one_config_cannot_extend_a_version_two_one() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        tmp.path(),
        "rules/base.yml",
        &format!("version: 2\n{OBJECTS}"),
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 1\nextends: [./rules/base.yml]\n",
    );
    let base = Path::new("rules").join("base.yml").display().to_string();
    assert_eq!(
        load_error(&path),
        format!(
            "{base}: a version 1 config (.alint.yml) cannot extend this version 2 config; give \
             .alint.yml `version: 2`, or make {base} `version: 1`"
        )
    );
    // Deeper in a chain, the extending config is named as the chain shows it.
    write(
        tmp.path(),
        "rules/middle.yml",
        "version: 1\nextends: [./base.yml]\n",
    );
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nextends: [./rules/middle.yml]\n",
    );
    let middle = Path::new("rules").join("middle.yml").display().to_string();
    assert_eq!(
        load_error(&path),
        format!(
            "{base}: a version 1 config ({middle}) cannot extend this version 2 config; give \
             {middle} `version: 2`, or make {base} `version: 1`"
        )
    );
}

#[test]
fn a_remote_source_must_be_version_one() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let path = tmp.path().join(".alint.yml");
    let refused = [
        (
            "version: 2\nrules: []\n",
            "https://example.invalid/remote.yml: a remote `extends:` source must be `version: 1` \
             in this build; `version: 2` is accepted only in local files (the top-level config, \
             a local `extends:` target or an `.alint.d` fragment)",
        ),
        (
            "version: 1\nrelations: {}\n",
            "https://example.invalid/remote.yml: `relations:` needs `version: 2`, which a remote \
             `extends:` source cannot use in this build: it must be `version: 1`",
        ),
    ];
    for (body, expected) in refused {
        let url = seed_remote(&cache, body);
        std::fs::write(&path, format!("version: 2\nextends: [\"{url}\"]\n")).unwrap();
        let error = load_with(&path, &LoadOptions::with_cache(cache.clone()))
            .unwrap_err()
            .to_string();
        assert_eq!(error, expected);
    }
    // An alias on a remote entry is refused before anything is fetched.
    std::fs::write(
        &path,
        "version: 2\nextends:\n  - url: https://example.invalid/r.yml#sha256-AAAA\n    as: r\n",
    )
    .unwrap();
    assert!(
        load_error(&path).starts_with(".alint.yml: `extends[0].as`: an alias names a local"),
        "{}",
        load_error(&path)
    );
}

#[test]
fn a_version_two_config_extends_a_version_one_remote_and_bundled_ruleset() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = extends::Cache::at(tmp.path().join("cache"));
    let url = seed_remote(&cache, &format!("version: 1\n{README_RULE}"));
    let path = tmp.path().join(".alint.yml");
    std::fs::write(
        &path,
        format!("version: 2\nextends: [\"{url}\", alint://bundled/oss-baseline@v1]\n{OBJECTS}"),
    )
    .unwrap();
    let config = load_with(&path, &LoadOptions::with_cache(cache)).unwrap();
    assert_eq!(config.version, 2);
    assert!(config.rules.iter().any(|rule| rule.id == "readme"));
    assert!(config.rules.len() > 1, "the bundled rules load too");
}

#[test]
fn a_nested_config_must_be_version_one() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write(
        tmp.path(),
        ".alint.yml",
        "version: 2\nnested_configs: true\n",
    );
    let nested = write(tmp.path(), "pkg/.alint.yml", "version: 2\nrules: []\n");
    let error = load_error(&path);
    assert!(
        error.ends_with(
            ": a nested config (`nested_configs:`) must be `version: 1` in this build; \
             `version: 2` is accepted only in local files (the top-level config, a local \
             `extends:` target or an `.alint.d` fragment)"
        ),
        "{error}"
    );
    assert!(error.contains("pkg"), "{error}");
    std::fs::write(&nested, "version: 1\nselectors: {}\n").unwrap();
    let error = load_error(&path);
    assert!(
        error.ends_with(
            ": `selectors:` needs `version: 2`, which a nested config (`nested_configs:`) \
             cannot use in this build: it must be `version: 1`"
        ),
        "{error}"
    );
    // A version 1 nested config under a version 2 root loads as before.
    std::fs::write(&nested, format!("version: 1\n{README_RULE}")).unwrap();
    assert_eq!(load(&path).unwrap().rules.len(), 1);
}

#[test]
fn a_parsed_string_must_be_version_one() {
    assert_eq!(
        parse("version: 2\nrules: []\n").unwrap_err().to_string(),
        "<inline>: a config parsed from a string (`alint_dsl::parse`) must be `version: 1` in \
         this build; `version: 2` is accepted only in local files (the top-level config, a \
         local `extends:` target or an `.alint.d` fragment)"
    );
    assert_eq!(
        parse("version: 1\nselectors: {}\n")
            .unwrap_err()
            .to_string(),
        "<inline>: `selectors:` needs `version: 2`, which a config parsed from a string \
         (`alint_dsl::parse`) cannot use in this build: it must be `version: 1`"
    );
    assert_eq!(parse("version: 1\nrules: []\n").unwrap().version, 1);
}

#[test]
fn objects_are_dropped_when_the_config_is_finalized() {
    // Nothing reads selectors and relations before the `constraint` kind
    // (phase 7): a v2 config's rules are its v1 rules.
    let tmp = tempfile::tempdir().unwrap();
    let with = write(
        tmp.path(),
        "a/.alint.yml",
        &format!("version: 2\n{OBJECTS}{README_RULE}"),
    );
    let without = write(
        tmp.path(),
        "b/.alint.yml",
        &format!("version: 2\n{README_RULE}"),
    );
    let (with, without) = (load(&with).unwrap(), load(&without).unwrap());
    assert_eq!(
        with.rules.iter().map(|rule| &rule.id).collect::<Vec<_>>(),
        without
            .rules
            .iter()
            .map(|rule| &rule.id)
            .collect::<Vec<_>>()
    );
}
