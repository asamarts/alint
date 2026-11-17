//! Unit tests of the `extends:` loader (`loader.rs`).

use super::*;

#[test]
fn display_path_strips_the_windows_verbatim_prefix() {
    // `canonicalize` on Windows yields `\\?\C:\...`; error messages must
    // show the path the way the user would write it.
    assert_eq!(
        display_path(Path::new(r"\\?\C:\repo\.alint.yml")),
        r"C:\repo\.alint.yml"
    );
    assert_eq!(
        display_path(Path::new(r"\\?\UNC\server\share\.alint.yml")),
        r"\\server\share\.alint.yml"
    );
    assert_eq!(
        display_path(Path::new("/repo/.alint.yml")),
        "/repo/.alint.yml"
    );
}

#[test]
fn remote_source_display_removes_credentials_query_and_fragment() {
    assert_eq!(
        remote_source_display(
            "https://user:secret@example.test/rules.yml?token=secret#sha256-deadbeef"
        ),
        "https://example.test/rules.yml"
    );
}

#[test]
fn requirement_must_be_direct_literal_metadata() {
    for body in [
        "version: 1\nrequires:\n  alint: '{{env.ALINT_REQUIRED}}'\nrules: []\n",
        "version: 1\nvars: { minimum: &v '0.19.0' }\nrequires:\n  alint: *v\nrules: []\n",
        "version: 1\nrequires:\n  alint: &v '0.19.0'\nrules: []\n",
        "version: 1\nrequires:\n  alint: !release '0.19.0'\nrules: []\n",
        "version: 1\nrequires:\n  <<: { alint: '0.19.0' }\nrules: []\n",
        "version: 1\nvars: { requirement_key: &k requires }\n? *k\n:\n  alint: '0.19.0'\nrules: []\n",
        "version: 1\nvars: { minimum: &v '0.19.0' }\n\"requi\\x72es\":\n  alint: *v\nrules: []\n",
    ] {
        let error = parse_config_interpolated_inner(body, Path::new(".alint.yml"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("direct literal stable version"), "{error}");
    }
}

#[test]
fn unrelated_yaml_metadata_does_not_taint_a_literal_requirement() {
    let config = parse_config_interpolated_inner(
        "version: 1\nvars: { reusable: &value literal }\nrequires:\n  alint: '0.19.0'\nrules: []\n",
        Path::new(".alint.yml"),
    )
    .unwrap();
    assert_eq!(
        config.requires.unwrap().alint,
        alint_core::StableVersion::new(0, 19, 0)
    );
}
#[test]
fn every_complete_source_class_requires_an_explicit_version() {
    let running = RunningVersion::new(alint_core::StableVersion::new(0, 19, 0), false);
    for source_class in [
        ConfigSourceClass::Root,
        ConfigSourceClass::LocalExtend,
        ConfigSourceClass::Bundled,
        ConfigSourceClass::Remote,
        ConfigSourceClass::Nested,
        ConfigSourceClass::Inline,
    ] {
        let mut config = RawConfig::default();
        let error =
            validate_source_compatibility(&mut config, "source.yml", source_class, None, running)
                .unwrap_err()
                .to_string();
        assert!(
            error.contains("missing required config `version`"),
            "{source_class:?}: {error}"
        );
    }
}

#[test]
fn drop_in_inherits_omitted_version_and_records_compatibility() {
    let mut config: RawConfig = alint_core::yaml::from_str("rules: []").unwrap();
    validate_source_compatibility(
        &mut config,
        ".alint.d/10-team.yml",
        ConfigSourceClass::DropIn,
        Some(1),
        RunningVersion::new(alint_core::StableVersion::new(0, 19, 0), false),
    )
    .unwrap();
    assert_eq!(config.version, Some(1));
    assert_eq!(config.compatibility.len(), 1);
    assert_eq!(config.compatibility[0].consumer_version, Some(1));
}

#[test]
fn drop_in_explicit_version_must_equal_root() {
    let running = RunningVersion::new(alint_core::StableVersion::new(0, 19, 0), false);
    let mut matching: RawConfig = alint_core::yaml::from_str("version: 1\nrules: []").unwrap();
    validate_source_compatibility(
        &mut matching,
        ".alint.d/10-team.yml",
        ConfigSourceClass::DropIn,
        Some(1),
        running,
    )
    .unwrap();
    assert_eq!(matching.compatibility[0].consumer_version, Some(1));

    let mut mismatching: RawConfig = alint_core::yaml::from_str("version: 2\nrules: []").unwrap();
    let error = validate_source_compatibility(
        &mut mismatching,
        ".alint.d/10-team.yml",
        ConfigSourceClass::DropIn,
        Some(1),
        running,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("drop-in config version 2 must equal root config version 1"),
        "{error}"
    );
}

#[test]
fn local_source_requirement_fails_before_child_resolution() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join(".alint.yml");
    std::fs::write(
        &path,
        "version: 1\nrequires:\n  alint: '999.0.0'\nextends:\n  - https://invalid.invalid/rules.yml#sha256-deadbeef\nrules:\n  - id: forbidden-command\n    kind: command\n    command: [sh, -c, 'exit 0']\n    level: error\n",
    )
    .unwrap();
    let error = crate::load(&path).unwrap_err().to_string();
    assert!(error.contains("requires alint 999.0.0 or newer"), "{error}");
    assert!(!error.contains("fetch"), "{error}");
    assert!(!error.contains("forbidden-command"), "{error}");
}

#[test]
fn source_version_direction_is_not_symmetric() {
    for (consumer, source, allowed) in [(1, 1, true), (1, 2, false), (2, 1, true), (2, 2, true)] {
        assert_eq!(
            source_version_allowed(consumer, source),
            allowed,
            "{consumer} -> {source}"
        );
    }
    assert!(
        source_version_allowed(2, 1) && !source_version_allowed(1, 2),
        "a v2 -> v1 -> v2 chain must fail at its second edge"
    );
}

#[test]
fn memo_clones_a_config_only_once_it_is_seen_twice() {
    let key = || {
        MemoKey::new(
            Path::new("/repo/base.yml"),
            Some(Path::new("/repo")),
            &[],
            Some(1),
            ConfigSourceClass::LocalExtend,
        )
    };
    let config = RawConfig {
        version: Some(1),
        ..RawConfig::default()
    };
    let mut state = LoadState::default();
    // A linear chain loads each file once: nothing is cloned or served.
    state.remember(key(), &config);
    assert!(state.cached(Some(&key())).is_none());
    // A second load of the same file (a diamond) stores it; later loads hit.
    state.remember(key(), &config);
    assert_eq!(state.cached(Some(&key())).map(|c| c.version), Some(Some(1)));
}

#[test]
fn memoization_separates_consumer_versions() {
    let path = Path::new("/repo/base.yml");
    let v1 = MemoKey::new(
        path,
        Some(Path::new("/repo")),
        &[],
        Some(1),
        ConfigSourceClass::LocalExtend,
    );
    let v2 = MemoKey::new(
        path,
        Some(Path::new("/repo")),
        &[],
        Some(2),
        ConfigSourceClass::LocalExtend,
    );
    assert!(v1 != v2);
}

#[test]
fn confine_none_disables_the_check() {
    // A programmatic caller (or `allow_out_of_root: true`) hands `None`:
    // even a blatant escape target is permitted.
    assert!(
        confine_extends_target(
            Path::new("/etc/shadow"),
            "/etc/shadow",
            None,
            &crate::FilesystemConfigSource,
        )
        .is_ok()
    );
}

#[test]
fn confine_missing_target_is_not_an_escape() {
    // A non-existent target can't be canonicalized; it is left for the
    // caller's not-found path, NOT reported as out-of-root (nothing to read).
    let tmp = tempfile::tempdir().unwrap();
    let res = confine_extends_target(
        &tmp.path().join("nope.yml"),
        "../nope.yml",
        Some(tmp.path()),
        &crate::FilesystemConfigSource,
    );
    assert!(
        res.is_ok(),
        "missing target must defer to the not-found path"
    );
}

#[test]
fn an_absolute_rooted_or_network_extends_is_told_apart_by_its_text() {
    for entry in [
        "/etc/alint.yml",
        "//server/share/x.yml",
        r"\\server\share\x.yml",
        r"\\?\UNC\server\share\x.yml",
        r"\\.\pipe\x",
        r"\x.yml",
        r"C:\x.yml",
        "C:/x.yml",
        "c:x.yml",
    ] {
        assert!(names_a_path_of_its_own(entry), "{entry:?}");
    }
    for entry in [
        "base.yml",
        "./base.yml",
        "../base.yml",
        "sub/base.yml",
        r"sub\base.yml",
        "1:x.yml",
        ":x.yml",
        "",
    ] {
        assert!(!names_a_path_of_its_own(entry), "{entry:?}");
    }
}

/// A config source that records every path the loader asks it about.
#[derive(Debug, Default)]
struct RecordingSource {
    asked: std::sync::Mutex<Vec<PathBuf>>,
}

impl crate::ConfigSourceProvider for RecordingSource {
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.asked.lock().unwrap().push(path.to_path_buf());
        crate::FilesystemConfigSource.canonicalize(path)
    }
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        self.asked.lock().unwrap().push(path.to_path_buf());
        crate::FilesystemConfigSource.read_to_string(path)
    }
    fn yaml_files(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        self.asked.lock().unwrap().push(dir.to_path_buf());
        crate::FilesystemConfigSource.yaml_files(dir)
    }
}

/// A confined load refuses an absolute, rooted or network `extends:` before
/// it asks the file system anything about it; with `allow_out_of_root`, the
/// user may name one.
#[test]
fn an_absolute_or_network_extends_is_refused_before_any_file_system_call() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join(".alint.yml");
    for entry in [
        "//attacker/share/x.yml",
        r"\\attacker\share\x.yml",
        r"C:\attacker\x.yml",
        "/attacker/x.yml",
    ] {
        std::fs::write(
            &config,
            format!("version: 1\nextends: ['{entry}']\nrules: []\n"),
        )
        .unwrap();
        let source = std::sync::Arc::new(RecordingSource::default());
        let options = crate::LoadOptions {
            config_source: source.clone(),
            ..crate::LoadOptions::default()
        };
        let error = crate::load_with(&config, &options).unwrap_err().to_string();
        assert!(
            error.contains("absolute, rooted or network path"),
            "{entry}: {error}"
        );
        let asked = source.asked.lock().unwrap();
        assert!(
            asked
                .iter()
                .all(|path| !path.to_string_lossy().contains("attacker")),
            "{entry}: {asked:?}"
        );
    }
    // The user's own opt-out lets an absolute path through, as before.
    let base = tmp.path().join("elsewhere.yml");
    std::fs::write(&base, "version: 1\nrules: []\n").unwrap();
    std::fs::write(
        &config,
        format!(
            "version: 1\nallow_out_of_root: true\nextends: ['{}']\nrules: []\n",
            base.display()
        ),
    )
    .unwrap();
    crate::load(&config).unwrap();
}

#[test]
fn confine_rejects_target_outside_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let outside = tmp.path().join("outside.yml");
    std::fs::write(&outside, "x").unwrap();
    let err = confine_extends_target(
        &outside,
        "../outside.yml",
        Some(&root),
        &crate::FilesystemConfigSource,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("outside the lint root"), "{err}");
}

#[test]
fn confine_allows_target_inside_root() {
    let tmp = tempfile::tempdir().unwrap();
    let inside = tmp.path().join("base.yml");
    std::fs::write(&inside, "x").unwrap();
    assert!(
        confine_extends_target(
            &inside,
            "./base.yml",
            Some(tmp.path()),
            &crate::FilesystemConfigSource,
        )
        .is_ok()
    );
}

#[test]
fn extends_chain_depth_is_capped() {
    // L5: an acyclic chain deeper than the cap is rejected (not a stack
    // overflow). c0 -> c1 -> ... all within one dir (so confinement passes).
    let tmp = tempfile::tempdir().unwrap();
    let n = MAX_EXTENDS_DEPTH + 5;
    for i in 0..n {
        let body = if i + 1 < n {
            format!("version: 1\nextends: [./c{}.yml]\nrules: []\n", i + 1)
        } else {
            "version: 1\nrules: []\n".to_string()
        };
        std::fs::write(tmp.path().join(format!("c{i}.yml")), body).unwrap();
    }
    // Loading recurses once per level, so run it on a stack of a set size
    // rather than the test thread's 2 MiB: in a release build, 1 MiB (a
    // Windows main thread, the smallest a user runs alint on; the chain
    // needs about 330 KiB); in a debug build, whose frames are about six
    // times larger, 4 MiB (it needs about 2 MiB).
    let stack = if cfg!(debug_assertions) {
        4 << 20
    } else {
        1 << 20
    };
    let root = tmp.path().join("c0.yml");
    let err = std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || crate::load(&root).unwrap_err().to_string())
        .unwrap()
        .join()
        .unwrap();
    assert!(err.contains("maximum depth"), "{err}");
}

#[test]
fn extends_chain_within_depth_cap_loads() {
    // A short chain (well under the cap) still composes fine.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("base.yml"), "version: 1\nrules: []\n").unwrap();
    std::fs::write(
        tmp.path().join(".alint.yml"),
        "version: 1\nextends: [./base.yml]\nrules: []\n",
    )
    .unwrap();
    assert!(crate::load(&tmp.path().join(".alint.yml")).is_ok());
}

#[test]
fn diamond_extends_chain_loads_each_file_once() {
    // Audit 2026-10: `cN` extends `[./cN+1.yml, ./cN+1.yml]` at every level,
    // so without memoization the bottom file loads 2^depth times (depth 18
    // measured at 18s; depth 20 is minutes). Each level also contributes a
    // rule so the composed result is checked, not just the runtime.
    let tmp = tempfile::tempdir().unwrap();
    let depth = 20;
    for i in 0..=depth {
        let extends = if i < depth {
            format!("extends: [./c{n}.yml, ./c{n}.yml]\n", n = i + 1)
        } else {
            String::new()
        };
        let body = format!(
            "version: 1\n{extends}rules:\n  - id: r{i}\n    kind: file_exists\n    \
                 paths: README.md\n    level: warning\n"
        );
        std::fs::write(tmp.path().join(format!("c{i}.yml")), body).unwrap();
    }
    let started = std::time::Instant::now();
    let registry = alint_rules::builtin_registry();
    let cfg = crate::load_with(
        &tmp.path().join("c0.yml"),
        &crate::LoadOptions::with_catalog(registry.catalog()),
    )
    .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "diamond chain took {:?}",
        started.elapsed()
    );
    assert_eq!(cfg.rules.len(), depth + 1);
    assert_eq!(
        cfg.compatibility.len(),
        depth + 1,
        "diamond references must retain one compatibility record per canonical source/context"
    );
}
#[derive(Debug)]
struct SensitiveFieldEnvironment;

impl crate::EnvironmentProvider for SensitiveFieldEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        (name == "SECRET").then(|| "canary-secret".to_string())
    }
}
/// Values interpolation re-types as numbers, which an error quotes as one.
#[derive(Debug)]
struct NumericEnvironment;
impl crate::EnvironmentProvider for NumericEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            "SECRET" => Some("123456789012".to_string()),
            "HEX" => Some("0xDEADBEEF".to_string()),
            _ => None,
        }
    }
}
#[derive(Debug)]
struct InvalidGlobEnvironment;
impl crate::EnvironmentProvider for InvalidGlobEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        (name == "SECRET").then(|| "canary-secret[".to_string())
    }
}
/// A distinct value per name, so a test can tell which were recorded.
#[derive(Debug)]
struct NamedEnvironment;
impl crate::EnvironmentProvider for NamedEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        Some(format!("value-of-{}", name.to_lowercase()))
    }
}
#[derive(Debug)]
struct InvalidRegexEnvironment;
impl crate::EnvironmentProvider for InvalidRegexEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        (name == "SECRET").then(|| "[builder-canary".to_string())
    }
}

#[test]
fn environment_provenance_survives_templates_and_last_wins_overrides() {
    let raw = parse_config_interpolated_inner_with_environment(
        r#"version: 1
templates:
  - id: scan
    kind: file_content_forbidden
    paths: "**/*.txt"
    pattern: "{{vars.needle}}"
    level: warning
rules:
  - id: direct
    kind: file_content_forbidden
    paths: "**/*.txt"
    pattern: "{{env.SECRET}}"
    message: safe prose
    level: warning
  - id: through-template
    extends_template: scan
    vars: { needle: "{{env.SECRET}}" }
  - id: literal-override
    extends_template: scan
    vars: { needle: "{{env.SECRET}}" }
    pattern: literal
  - id: condition
    kind: file_exists
    paths: README.md
    when: "'{{env.SECRET}}' == 'canary-secret'"
    level: warning
  - id: url-only
    kind: file_exists
    paths: README.md
    policy_url: "https://example.test/{{env.SECRET}}"
    level: warning
"#,
        Path::new(".alint.yml"),
        &SensitiveFieldEnvironment,
        &TEST_ROOT,
    )
    .unwrap();
    let registry = alint_rules::builtin_registry();
    let config = raw.finalize(Some(registry.catalog())).unwrap();
    let rule = |id: &str| config.rules.iter().find(|rule| rule.id == id).unwrap();

    assert!(rule("direct").field_is_sensitive("pattern"));
    assert!(!rule("direct").field_is_sensitive("message"));
    assert!(rule("through-template").field_is_sensitive("pattern"));
    assert!(!rule("literal-override").field_is_sensitive("pattern"));
    assert!(rule("condition").field_is_sensitive("when"));
    assert!(rule("url-only").field_is_sensitive("policy_url"));
    assert!(!rule("url-only").has_sensitive_evidence());
}
#[test]
fn sensitive_typed_and_builder_errors_never_echo_interpolated_values() {
    let typed = parse_config_interpolated_inner_with_environment(
        "version: 1\nrules:\n  - id: typed\n    kind: file_content_forbidden\n    paths: '**/*.txt'\n    pattern: x\n    expect_matches: '{{env.SECRET}}'\n    level: warning\n",
        Path::new(".alint.yml"),
        &SensitiveFieldEnvironment,
        &TEST_ROOT,
    )
    .unwrap();
    let registry = alint_rules::builtin_registry();
    let error = typed
        .finalize(Some(registry.catalog()))
        .unwrap_err()
        .to_string();
    assert!(error.contains("<redacted>"), "{error}");
    assert!(!error.contains("canary-secret"), "{error}");

    let raw = parse_config_interpolated_inner_with_environment(
        "version: 1\nrules:\n  - id: builder\n    kind: file_content_forbidden\n    paths: '**/*.txt'\n    pattern: '{{env.SECRET}}'\n    level: warning\n",
        Path::new(".alint.yml"),
        &InvalidRegexEnvironment,
        &TEST_ROOT,
    )
    .unwrap();
    let config = raw.finalize(Some(registry.catalog())).unwrap();
    let error = registry.build(&config.rules[0]).unwrap_err().to_string();
    assert!(error.contains("<redacted>"), "{error}");
    assert!(!error.contains("builder-canary"), "{error}");
}

/// The first error loading `yaml` (read with `environment`), finalizing it,
/// and building each rule with its nested rules, as `alint check` does.
fn first_configuration_error(yaml: &str, environment: &dyn crate::EnvironmentProvider) -> String {
    let registry = alint_rules::builtin_registry();
    parse_config_interpolated_inner_with_environment(
        yaml,
        Path::new(".alint.yml"),
        environment,
        &TEST_ROOT,
    )
    .and_then(|raw| raw.finalize(Some(registry.catalog())))
    .and_then(|config| {
        for spec in &config.rules {
            let rule = registry.build(spec)?;
            registry.validate_nested(spec, rule.as_ref())?;
        }
        Ok(())
    })
    .unwrap_err()
    .to_string()
}

#[test]
fn structural_errors_stay_readable_beside_a_secret() {
    // Each error names only fields, kinds or option keys, never a value, so
    // a rule that also reads the environment or sets `sensitive: true` must
    // not hide it.
    for (yaml, expected) in [
        (
            // The facebook/react shape: an unsupported `scope_filter`
            // predicate on a rule whose message reads the environment.
            "version: 1\nrules:\n  - id: fixups\n    kind: git_commit_no_fixup\n    level: error\n    message: 'fixup ({{env.SECRET}})'\n    scope_filter: {changed_since: main}\n",
            "`scope_filter.changed_since` is not supported by `git_commit_no_fixup`",
        ),
        (
            "version: 1\nrules:\n  - id: exists\n    kind: file_exists\n    paths: README.md\n    level: error\n    sensitive: true\n    expect_matches: true\n",
            "`expect_matches` is not supported by `file_exists`",
        ),
        (
            // An unknown option, caught by the rule builder.
            "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*.txt'\n    pattern: '{{env.SECRET}}'\n    colour: red\n    level: warning\n",
            "unknown field `colour`",
        ),
        (
            // A nested rule's unknown option, beside a secret in the parent.
            "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    message: '{{env.SECRET}}'\n    level: error\n    require:\n      - kind: file_exists\n        paths: '{path}/Cargo.toml'\n        colour: red\n",
            "unknown field `colour`",
        ),
        (
            // A top-level key error in a file that reads the environment.
            "version: 1\nrulez: []\nrules:\n  - id: r\n    kind: file_exists\n    paths: '{{env.SECRET}}'\n    level: error\n",
            "unknown field `rulez`",
        ),
        (
            // A nested rule's unknown option under a parent marked
            // sensitive: masking keeps the nested `kind`, so the rerun
            // reaches the same error.
            "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    level: error\n    sensitive: true\n    require:\n      - kind: file_exists\n        paths: '{path}/Cargo.toml'\n        colour: red\n",
            "unknown field `colour`",
        ),
        (
            // The same, with the secret in the nested rule itself.
            "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    level: error\n    require:\n      - kind: file_exists\n        paths: '{path}/{{env.SECRET}}'\n        colour: red\n",
            "unknown field `colour`",
        ),
        (
            // A nested rule's unknown kind under a parent marked sensitive.
            "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    level: error\n    sensitive: true\n    require:\n      - kind: file_exits\n        paths: '{path}/Cargo.toml'\n",
            "file_exits",
        ),
    ] {
        let error = first_configuration_error(yaml, &SensitiveFieldEnvironment);
        assert!(error.contains(expected), "{yaml}\n=> {error}");
        assert!(!error.contains("<redacted>"), "{error}");
        assert!(!error.contains("canary-secret"), "{error}");
    }
}

#[test]
fn errors_that_quote_a_secret_stay_redacted() {
    for (yaml, environment, secret) in [
        (
            // A literal regex in a rule marked sensitive.
            "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*.txt'\n    pattern: '[literal-canary'\n    sensitive: true\n    level: warning\n",
            &SensitiveFieldEnvironment as &dyn crate::EnvironmentProvider,
            "literal-canary",
        ),
        (
            // A nested rule's invalid regex read from the environment.
            "version: 1\nrules:\n  - id: each\n    kind: for_each_dir\n    select: 'crates/*'\n    level: error\n    require:\n      - kind: file_content_forbidden\n        paths: '{path}/*.txt'\n        pattern: '{{env.SECRET}}'\n",
            &InvalidRegexEnvironment,
            "builder-canary",
        ),
        (
            // A typed value read from the environment, beside a key error
            // the masked copy would reach first.
            "version: '{{env.SECRET}}'\nrulez: []\n",
            &SensitiveFieldEnvironment,
            "canary-secret",
        ),
        (
            // A single `paths:` glob read from the environment.
            "version: 1\nrules:\n  - id: exists\n    kind: file_exists\n    paths: '{{env.SECRET}}'\n    level: error\n",
            &InvalidGlobEnvironment,
            "canary-secret",
        ),
    ] {
        let error = first_configuration_error(yaml, environment);
        assert!(error.contains("<redacted>"), "{yaml}\n=> {error}");
        assert!(!error.contains(secret), "{error}");
    }
}

#[test]
fn a_numeric_value_never_reaches_an_error() {
    // Interpolation re-types a numeric value, so masking only strings would
    // let the masked copy fail with the same error, quoting the number.
    for yaml in [
        // Rejected by the rule builder.
        "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*'\n    pattern: '{{env.SECRET}}'\n    level: error\n",
        // Rejected when the merged rule is deserialized.
        "version: 1\nrules:\n  - id: exists\n    kind: file_exists\n    paths: README.md\n    message: '{{env.SECRET}}'\n    level: error\n",
        // A bool field given a number.
        "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*'\n    pattern: x\n    expect_matches: '{{env.SECRET}}'\n    level: error\n",
        // A hex value, which an error prints in decimal.
        "version: 1\nrules:\n  - id: exists\n    kind: file_exists\n    paths: README.md\n    message: '{{env.HEX}}'\n    level: error\n",
        // A literal number in a rule marked sensitive.
        "version: 1\nrules:\n  - id: scan\n    kind: file_content_forbidden\n    paths: '**/*'\n    pattern: 123456789012\n    sensitive: true\n    level: error\n",
    ] {
        let error = first_configuration_error(yaml, &NumericEnvironment);
        for secret in ["123456789012", "3735928559", "DEADBEEF"] {
            assert!(!error.contains(secret), "{yaml}\n=> {error}");
        }
    }
}

#[test]
fn top_level_environment_values_are_recorded_and_scrubbed_from_load_errors() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".alint.d")).unwrap();
    std::fs::write(
        root.path().join(".alint.yml"),
        "version: 1\nignore: ['{{env.IGNORED}}']\nfacts:\n  - id: f\n    any_file_exists: ['{{env.FACT}}']\nvars:\n  v: '{{env.VAR}}'\nrules:\n  - id: r\n    kind: file_exists\n    paths: '{{env.RULE}}'\n    level: error\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".alint.d/10-extra.yml"),
        "version: 1\nbaseline: '{{env.BASELINE}}'\n",
    )
    .unwrap();
    let registry = alint_rules::builtin_registry();
    let mut options = crate::LoadOptions::with_catalog(registry.catalog());
    options.environment = std::sync::Arc::new(NamedEnvironment);
    let config = crate::load_with(&root.path().join(".alint.yml"), &options).unwrap();
    // Rules, templates and vars are redacted per rule, so only the other
    // top-level fields are recorded, including a drop-in's.
    assert_eq!(
        config.environment_values,
        ["value-of-ignored", "value-of-fact", "value-of-baseline"]
    );
    assert_eq!(
        config.redact_environment_values("bad glob value-of-fact"),
        "bad glob <redacted>"
    );

    std::fs::remove_dir_all(root.path().join(".alint.d")).unwrap();

    // An error met while resolving `extends:` (before the configs merge) is
    // scrubbed too.
    std::fs::write(
        root.path().join(".alint.yml"),
        "version: 1\nextends: ['./{{env.MISSING}}.yml']\nrules: []\n",
    )
    .unwrap();
    let error = crate::load_with(&root.path().join(".alint.yml"), &options)
        .unwrap_err()
        .to_string();
    assert!(error.contains("<redacted>"), "{error}");
    assert!(!error.contains("value-of-missing"), "{error}");

    // So is one met before the merge that is not about `extends:` (a
    // version check runs per source)...
    std::fs::write(
        root.path().join(".alint.yml"),
        "version: '{{env.VALUE}}'\nrules: []\n",
    )
    .unwrap();
    let error = crate::load_with(&root.path().join(".alint.yml"), &value_options("987654"))
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("unsupported config version <redacted>"),
        "{error}"
    );
    assert!(!error.contains("987654"), "{error}");

    // ...and one met after it: nested-config discovery walks the tree with
    // the top-level `ignore:` patterns.
    std::fs::write(
        root.path().join(".alint.yml"),
        "version: 1\nnested_configs: true\nignore: ['{{env.VALUE}}']\nrules: []\n",
    )
    .unwrap();
    let error = crate::load_with(
        &root.path().join(".alint.yml"),
        &value_options("canary-secret["),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("<redacted>"), "{error}");
    assert!(!error.contains("canary-secret"), "{error}");

    // An `extends:` entry read from the environment is scrubbed in the form
    // the loader normalizes it to, which does not contain it as written:
    // a parse error names the extended file by its canonical path...
    std::fs::create_dir(root.path().join("sub")).unwrap();
    std::fs::write(
        root.path().join("canary-secret.yml"),
        "version: 1\nrules: [\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join(".alint.yml"),
        "version: 1\nextends: ['{{env.VALUE}}']\nrules: []\n",
    )
    .unwrap();
    let error = crate::load_with(
        &root.path().join(".alint.yml"),
        &value_options("sub/../canary-secret.yml"),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("<redacted>"), "{error}");
    assert!(!error.contains("canary-secret"), "{error}");

    // ...while a literal entry beside a value read from the environment
    // stays readable.
    std::fs::write(
        root.path().join(".alint.yml"),
        "version: 1\nignore: ['{{env.VALUE}}']\nextends: ['./plain-missing.yml']\nrules: []\n",
    )
    .unwrap();
    let error = crate::load_with(
        &root.path().join(".alint.yml"),
        &value_options("canary-ignored"),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("plain-missing.yml"), "{error}");
    assert!(!error.contains("<redacted>"), "{error}");
}

#[test]
fn an_extends_entry_read_from_the_environment_is_scrubbed_in_the_forms_it_is_printed_in() {
    let root = tempfile::tempdir().unwrap();
    for name in ["value-of-shared.yml", "value-of-ignored-rules.yml"] {
        std::fs::write(root.path().join(name), "version: 1\nrules: []\n").unwrap();
    }
    // `ignore:` reads a value the literal entry happens to contain.
    std::fs::write(
        root.path().join(".alint.yml"),
        "version: 1\nignore: ['{{env.IGNORED}}']\n\
             extends: ['./{{env.SHARED}}.yml', './value-of-ignored-rules.yml']\nrules: []\n",
    )
    .unwrap();
    let options = crate::LoadOptions {
        environment: std::sync::Arc::new(NamedEnvironment),
        ..Default::default()
    };
    let config = crate::load_with(&root.path().join(".alint.yml"), &options).unwrap();
    let forms = |file: &str| {
        config
            .environment_values
            .iter()
            .filter(|value| value.ends_with(file))
            .count()
    };
    // The joined and canonical paths of the entry read from the
    // environment; none of the literal one.
    assert!(
        forms("value-of-shared.yml") >= 2,
        "{:?}",
        config.environment_values
    );
    assert_eq!(
        forms("value-of-ignored-rules.yml"),
        0,
        "{:?}",
        config.environment_values
    );

    // A remote entry is printed without its credentials, query and
    // fragment, so not as the value reads.
    for policy in [crate::RemotePolicy::Deny, crate::RemotePolicy::Allow] {
        std::fs::write(
            root.path().join(".alint.yml"),
            "version: 1\nextends: ['{{env.VALUE}}']\nrules: []\n",
        )
        .unwrap();
        let mut options =
            value_options("https://token@example.invalid/canary-path.yml?q=1#sha256-zz");
        options.remote_policy = policy;
        let error = crate::load_with(&root.path().join(".alint.yml"), &options)
            .unwrap_err()
            .to_string();
        assert!(error.contains("<redacted>"), "{error}");
        assert!(!error.contains("canary-path"), "{error}");
    }
}

/// Options whose environment gives `VALUE` the value `value`.
fn value_options(value: &'static str) -> crate::LoadOptions<'static> {
    #[derive(Debug)]
    struct ValueEnvironment(&'static str);
    impl crate::EnvironmentProvider for ValueEnvironment {
        fn get(&self, name: &str) -> Option<String> {
            (name == "VALUE").then(|| self.0.to_string())
        }
    }
    crate::LoadOptions {
        environment: std::sync::Arc::new(ValueEnvironment(value)),
        ..Default::default()
    }
}

#[test]
fn raw_config_type_errors_never_echo_interpolated_values() {
    let error = parse_config_interpolated_inner_with_environment(
        "version: '{{env.SECRET}}'\nrules: []\n",
        Path::new(".alint.yml"),
        &SensitiveFieldEnvironment,
        &TEST_ROOT,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("<redacted>"), "{error}");
    assert!(!error.contains("canary-secret"), "{error}");
}
#[test]
fn merge_sensitivity_follows_the_winning_field() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("base.yml"),
        r#"version: 1
rules:
  - id: cleared
    kind: file_content_forbidden
    paths: "**/*.txt"
    pattern: "{{env.SECRET}}"
    level: warning
  - id: tainted
    kind: file_content_forbidden
    paths: "**/*.txt"
    pattern: literal
    level: warning
"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join(".alint.yml"),
        r#"version: 1
extends: [./base.yml]
rules:
  - id: cleared
    pattern: literal
  - id: tainted
    pattern: "{{env.SECRET}}"
"#,
    )
    .unwrap();
    let registry = alint_rules::builtin_registry();
    let mut options = crate::LoadOptions::with_catalog(registry.catalog());
    options.environment = std::sync::Arc::new(SensitiveFieldEnvironment);
    let config = crate::load_with(&root.path().join(".alint.yml"), &options).unwrap();
    let rule = |id: &str| config.rules.iter().find(|rule| rule.id == id).unwrap();
    assert!(!rule("cleared").field_is_sensitive("pattern"));
    assert!(rule("tainted").field_is_sensitive("pattern"));
}

#[test]
fn a_bundled_ruleset_must_be_version_one() {
    assert!(decode_bundled("version: 1\nrules: []\n", "x@v1").is_ok());
    for (body, expected) in [
        (
            "version: 2\nrules: []\n",
            "alint://bundled/x@v1: a bundled ruleset must be `version: 1` in this build; \
             `version: 2` is accepted only in local files (the top-level config, a local \
             `extends:` target or an `.alint.d` fragment)",
        ),
        (
            "version: 1\nselectors: {}\n",
            "alint://bundled/x@v1: `selectors:` needs `version: 2`, which a bundled ruleset \
             cannot use in this build: it must be `version: 1`",
        ),
        (
            "version: 1\nextends:\n  - url: ./a.yml\n    as: a\n",
            "alint://bundled/x@v1: `extends[0].as` needs `version: 2`, which a bundled ruleset \
             cannot use in this build: it must be `version: 1`",
        ),
        (
            "version: 2\nrules:\n  - {id: a, id: b}\n",
            "alint://bundled/x@v1: a bundled ruleset must be `version: 1` in this build",
        ),
    ] {
        let error = decode_bundled(body, "x@v1").unwrap_err();
        assert!(matches!(error, Error::Internal(_)), "{error:?}");
        assert!(error.to_string().contains(expected), "{error}");
    }
}

/// Every ruleset this build ships is version 1 with nothing of version 2, and
/// decodes once: the dispatch never needs to parse one to read its version.
#[test]
fn every_bundled_ruleset_is_plainly_version_one() {
    for (name, rev) in crate::bundled::catalog() {
        let spec = format!("{name}@{rev}");
        let body = crate::bundled::resolve(&spec).unwrap();
        let display = format!("alint://bundled/{spec}");
        let config = deserialize_raw_config(body, &display).unwrap();
        let origin = crate::v2::Origin::new(ConfigSourceClass::Bundled, &display);
        assert!(crate::v2::plainly_v1(&config, &origin), "{spec}");
        assert_eq!(
            crate::v2::dispatch(decode_text(body).ok().as_ref(), body, &origin).unwrap(),
            crate::v2::Surface::V1,
            "{spec}"
        );
    }
}

/// A version 2 source's selectors and relations count toward the merged
/// config's size, and the error says so.
#[test]
fn a_merged_config_counts_its_selectors_and_relations() {
    let mut config = parse_config_interpolated_inner(
        "version: 2\nselectors:\n  a: { paths: a }\nrelations:\n  r:\n    from: a\n    \
         read: { json: $.x }\n    fields: { x: value }\nrules: []\n",
        Path::new("/repo/.alint.yml"),
    )
    .unwrap();
    config.ignore = (0..crate::MAX_MERGED_ENTRIES - 1)
        .map(|i| i.to_string())
        .collect();
    let error = crate::merge(RawConfig::default(), config)
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        format!(
            "the merged config holds {} entries (rules, templates, facts, vars, `ignore:` \
             patterns, selectors and relations), more than the {} supported; flatten the \
             `extends:` chain or split the ruleset",
            crate::MAX_MERGED_ENTRIES + 1,
            crate::MAX_MERGED_ENTRIES
        )
    );
}

/// A `JSONPath` read from the environment: a dashed key the error's hint
/// rewrites (`$['canary-secret']`), or one that is fine.
#[derive(Debug)]
struct JsonPathEnvironment;
impl crate::EnvironmentProvider for JsonPathEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            "AT" => Some("$.canary-secret".to_string()),
            "FINE" => Some("$.fine".to_string()),
            _ => None,
        }
    }
}

fn v2_error(yaml: &str) -> String {
    parse_config_interpolated_inner_with_environment(
        yaml,
        Path::new(".alint.yml"),
        &JsonPathEnvironment,
        &TEST_ROOT,
    )
    .unwrap_err()
    .to_string()
}

/// D3: a version 2 object's decode error stands only when the source with
/// every environment value masked fails the same way; otherwise it is
/// redacted, naming the object. A `JSONPath`'s hint quotes the path rewritten,
/// which removing the value as written would miss.
#[test]
fn a_v2_object_error_that_could_quote_the_environment_is_redacted() {
    let relation = |body: &str| {
        format!(
            "version: 2\nselectors:\n  docs: {{ paths: a.json }}\nrelations:\n  b:\n    from: \
             docs\n{body}rules: []\n"
        )
    };
    for body in [
        "    entries: { format: json, at: '{{env.AT | default(\"$.x\")}}' }\n    fields: { k: \
         entry.key }\n",
        "    read: { json: '{{env.AT | default(\"$.x\")}}' }\n    fields: { v: value }\n",
        "    read: { json: $.x }\n    fields: { v: value, n: 'bound: {{env.AT | \
         default(\"$.x\")}}' }\n",
    ] {
        let error = v2_error(&relation(body));
        assert_eq!(
            error,
            ".alint.yml: `relations.b`: <redacted> (the error could quote a value read from the \
             environment)",
            "{body}"
        );
    }

    // An error the masked source repeats quotes no value: it stays readable.
    let error = v2_error(&relation(
        "    entries: { format: json, at: '{{env.FINE | default(\"$.x\")}}' }\n    fields: { k: \
         entry.key }\n    colour: red\n",
    ));
    assert_eq!(
        error,
        ".alint.yml: `relations.b`: unknown key `colour` in a relation; expected one of `from`, \
         `entries`, `read`, `optional`, `union`, `join`, `fields`, `transform`, `where`"
    );
    // And so does one that reads nothing from the environment.
    let error = v2_error(&relation(
        "    entries: { format: json, at: '$.canary-secret' }\n    fields: { k: entry.key }\n",
    ));
    assert_eq!(
        error,
        ".alint.yml: `relations.b.entries.at`: bad JSONPath \"$.canary-secret\": at position 8, \
         parser error; JSONPath dot-notation requires identifier-shape keys (RFC 9535). For \
         dashed keys, use bracket notation: `$['canary-secret']` instead of `$.canary-secret` (or \
         `@['canary-secret']` instead of `@.canary-secret` inside a filter). See \
         `docs/development/CONFIG-AUTHORING.md` § 10."
    );
}

/// Values read from the environment that are valid where they are written and
/// that a mask (`<redacted>`) is not: a format, a regex with a group, a join
/// side and a transform.
#[derive(Debug)]
struct FieldEnvironment;
impl crate::EnvironmentProvider for FieldEnvironment {
    fn get(&self, name: &str) -> Option<String> {
        match name {
            "FORMAT" => Some("json".to_string()),
            "PATTERN" => Some("x(.*)".to_string()),
            "UNIQUE" => Some("left".to_string()),
            "TRANSFORM" => Some("trim".to_string()),
            "AT" => Some("$.canary-secret".to_string()),
            _ => None,
        }
    }
}

fn v2_field_error(yaml: &str) -> String {
    parse_config_interpolated_inner_with_environment(
        yaml,
        Path::new(".alint.yml"),
        &FieldEnvironment,
        &TEST_ROOT,
    )
    .unwrap_err()
    .to_string()
}

/// The masked source a decode error is rerun on masks only the values of the
/// object the error is about. An environment value in another object, where a
/// mask is no valid value (a `format:`, a regex with a group, a join's
/// `unique:`, a transform), fails that object first in a source masked
/// whole, and redacted an error that quotes no value; it now reads as it
/// would with no value read from the environment. An error about the object
/// whose own value cannot be masked stays redacted.
#[test]
fn a_v2_object_error_beside_an_unmaskable_value_elsewhere_stays_readable() {
    let typo = "  b:\n    from: s\n    read: { json: $.a }\n    fieldz: { v: value }\n";
    let typo_message = ".alint.yml: `relations.b`: unknown key `fieldz` in a relation; expected \
                        one of `from`, `entries`, `read`, `optional`, `union`, `join`, \
                        `fields`, `transform`, `where`";
    for elsewhere in [
        "  a:\n    from: s\n    entries: { format: '{{env.FORMAT | default(\"x\")}}', at: $ }\n    \
         fields: { k: entry.key }\n",
        "  a:\n    from: s\n    read: { regex: '{{env.PATTERN | default(\"x\")}}' }\n    fields: \
         { v: value }\n",
        "  a:\n    from: s\n    read: { json: $.a }\n    fields: { v: value }\n    transform: { \
         v: ['{{env.TRANSFORM | default(\"x\")}}'] }\n",
        "  a:\n    from: s\n    read: { json: $.a }\n    fields: { v: value }\n  c:\n    from: \
         s\n    read: { json: $.c }\n    fields: { w: value }\n  j:\n    join: { left: a, \
         right: c, on: { v: w }, unique: '{{env.UNIQUE | default(\"x\")}}' }\n    fields: { x: \
         left.v }\n",
    ] {
        let source = format!(
            "version: 2\nselectors:\n  s: {{ paths: a.json }}\nrelations:\n{elsewhere}{typo}\
             rules: []\n"
        );
        assert_eq!(v2_field_error(&source), typo_message, "{elsewhere}");
    }
    // The erroring object's own value: its masked copy fails elsewhere in it
    // (no format is `<redacted>`), so the error is redacted.
    let source = "version: 2\nselectors:\n  s: { paths: a.json }\nrelations:\n  b:\n    from: \
                  s\n    entries: { format: '{{env.FORMAT | default(\"x\")}}', at: '{{env.AT | \
                  default(\"$.x\")}}' }\n    fields: { k: entry.key }\nrules: []\n";
    assert_eq!(
        v2_field_error(source),
        ".alint.yml: `relations.b`: <redacted> (the error could quote a value read from the \
         environment)"
    );
}
