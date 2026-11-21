//! A config is untrusted for what it costs to load: the reader, the
//! `requires:` check, `extends:` merging and template expansion are bounded.

use std::time::Duration;

use super::*;

/// A tempdir holding `files`, kept alive with the path of its `.alint.yml`.
fn repo(files: &[(&str, &str)]) -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    for (name, body) in files {
        std::fs::write(tmp.path().join(name), body).unwrap();
    }
    let config = tmp.path().join(".alint.yml");
    (tmp, config)
}

/// ROB-4: the `requires:` check walked the whole first document as saphyr read
/// it, recursively and without a depth bound, so a value saphyr reads 100,000
/// levels deep overflowed the stack (ayaml reads it as a scalar). It now reads
/// the document as alint decodes it, with a stack of its own.
#[test]
fn the_requires_check_does_not_recurse_through_the_document() {
    let deep = "- ".repeat(100_000);
    let with = format!(
        "version: 1\nrequires:\n  alint: \"0.1.0\"\nrules: []\nvars:\n  x: &ak:---\n    {deep}x\n"
    );
    let without = with.replace("requires:\n  alint: \"0.1.0\"\n", "");
    let outcomes = on_thread(2 << 20, Duration::from_secs(20), move || {
        [parse(&with), parse(&without)].map(|outcome| outcome.map(drop).map_err(|e| e.to_string()))
    });
    assert_eq!(outcomes[0], outcomes[1]);
    // What ayaml itself reads that deep is an error, never a deep stack.
    let nested =
        format!("version: 1\nrequires:\n  alint: \"0.1.0\"\nrules: []\nvars:\n  x:\n    {deep}x\n");
    assert!(on_thread(2 << 20, Duration::from_secs(20), move || parse(
        &nested
    )
    .is_err()));
}

/// YAML-15: with `requires:`, a config ayaml reads and saphyr refuses failed
/// with saphyr's message; without it, the same config loaded.
#[test]
fn requires_does_not_make_a_config_depend_on_another_parser() {
    let without = "version: 1\nvars:\n  a:\tx\nrules: []\n";
    let with = "version: 1\nrequires:\n  alint: '0.1.0'\nvars:\n  a:\tx\nrules: []\n";
    assert_eq!(parse(without).unwrap().vars["a"], "x");
    assert_eq!(parse(with).unwrap().vars["a"], "x");
    let (_tmp, config) = repo(&[(".alint.yml", with)]);
    assert_eq!(load(&config).unwrap().vars["a"], "x");
}

/// ROB-9: after a decode error, the `requires:` check parses the text again
/// only when it spells `requires`, and still names a non-literal requirement.
#[test]
fn a_decode_error_still_names_a_non_literal_requirement() {
    let error = parse("version: 1\nrequires:\n  alint: '${ALINT}'\nrules: []\n")
        .unwrap_err()
        .to_string();
    assert!(error.contains("direct literal stable version"), "{error}");
    let error = parse("version: 1\nrules: 3\n").unwrap_err().to_string();
    assert!(!error.contains("direct literal"), "{error}");
}

/// ROB-5: a chain of 30 configs that each list the next twice doubled
/// `ignore:` at every level (2^30 entries, 5.8 GB before it aborted).
#[test]
fn an_extends_diamond_merges_each_entry_once() {
    let mut files: Vec<(String, String)> = (0..30)
        .map(|level| {
            let next = format!("./l{}.yml", level + 1);
            (
                format!("l{level}.yml"),
                format!("version: 1\nextends: [{next}, {next}]\n"),
            )
        })
        .collect();
    files.push((
        "l30.yml".into(),
        "version: 1\nignore: [\"zzz/**\"]\n".into(),
    ));
    files.push((
        ".alint.yml".into(),
        "version: 1\nextends: [./l0.yml]\n".into(),
    ));
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect();
    let (tmp, config) = repo(&files);
    let ignore = on_thread(8 << 20, Duration::from_secs(1), move || {
        load(&config).map(|config| config.ignore)
    })
    .unwrap();
    drop(tmp);
    assert_eq!(ignore, ["zzz/**"]);

    // Two sources that extend one more: its rule, and the rule's id, once.
    let (_tmp, config) = repo(&[
        (
            "c.yml",
            "version: 1\nignore: [x]\nrules:\n  - {id: r, kind: file_exists, paths: a, level: error}\n",
        ),
        ("a.yml", "version: 1\nextends: [./c.yml]\nignore: [y]\n"),
        ("b.yml", "version: 1\nextends: [./c.yml]\nignore: [x]\n"),
        (".alint.yml", "version: 1\nextends: [./a.yml, ./b.yml]\n"),
    ]);
    let loaded = load(&config).unwrap();
    assert_eq!(loaded.ignore, ["x", "y"]);
    assert_eq!(loaded.rules.len(), 1);
}

/// ROB-5: past dedup, the merged config is held to a size.
#[test]
fn a_merged_config_is_held_to_a_size() {
    let big = RawConfig {
        ignore: (0..=MAX_MERGED_ENTRIES).map(|i| i.to_string()).collect(),
        ..RawConfig::default()
    };
    let error = merge(RawConfig::default(), big).unwrap_err().to_string();
    assert_eq!(
        error,
        format!(
            "the merged config holds {} entries (rules, templates, facts, vars and `ignore:` \
             patterns), more than the {MAX_MERGED_ENTRIES} supported; flatten the `extends:` \
             chain or split the ruleset",
            MAX_MERGED_ENTRIES + 1
        )
    );
}

/// ROB-6: a template copied by every instance, and a var rendered at every
/// placeholder, expanded a config of a megabyte to gigabytes.
#[test]
fn template_expansion_is_held_to_a_budget() {
    use std::fmt::Write as _;

    let template = |message: &str| {
        format!(
            "version: 1\ntemplates:\n  - id: t\n    kind: file_exists\n    paths: x\n    \
             level: error\n    message: \"{message}\"\nrules:\n"
        )
    };
    let mut copies = template(&"m".repeat(1 << 20));
    for instance in 0..4000 {
        writeln!(copies, "  - {{id: r{instance}, extends_template: t}}").unwrap();
    }
    let (tmp, config) = repo(&[(".alint.yml", &copies)]);
    let error = on_thread(8 << 20, Duration::from_secs(5), move || {
        load(&config).unwrap_err().to_string()
    });
    drop(tmp);
    assert!(error.contains("template expansion budget"), "{error}");
    assert!(error.contains("template `t`"), "{error}");

    let mut placeholders = template(&"{{vars.x}}".repeat(100_000));
    writeln!(
        placeholders,
        "  - id: r\n    extends_template: t\n    vars: {{x: \"{}\"}}",
        "v".repeat(20_000)
    )
    .unwrap();
    let (tmp, config) = repo(&[(".alint.yml", &placeholders)]);
    let error = on_thread(8 << 20, Duration::from_secs(5), move || {
        load(&config).unwrap_err().to_string()
    });
    drop(tmp);
    assert!(
        error.contains("renders a string of 2000000000 bytes"),
        "{error}"
    );
    assert!(error.contains("rule \"r\""), "{error}");
}

/// The documented template example expands far inside the budget.
#[test]
fn the_documented_template_example_expands_well_inside_the_budget() {
    let template: Mapping = alint_core::yaml::from_str::<ayaml::Mapping>(
        "{id: dir-has-readme, kind: file_exists, paths: [\"{{vars.dir}}/README.md\"], \
         level: warning, message: \"{{vars.dir}} is missing a top-level README\"}",
    )
    .unwrap();
    let per_instance = template_budget::size_of_mapping(&template);
    assert!(per_instance < 512, "{per_instance}");
    // 295 bytes: a config would need some 220,000 such instances to spend it.
    assert!(per_instance * 100_000 < template_budget::MAX_TEMPLATE_EXPANSION_BYTES);
}

/// ROB-11: an `extends:` target that is a FIFO hung the load forever.
#[cfg(unix)]
#[test]
fn a_fifo_extends_target_is_refused_not_read() {
    let (tmp, config) = repo(&[(".alint.yml", "version: 1\nextends: [./f.yml]\n")]);
    let fifo = tmp.path().join("f.yml");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        eprintln!("mkfifo unavailable; skipping");
        return;
    }
    let error = on_thread(2 << 20, Duration::from_secs(2), move || {
        load(&config).unwrap_err().to_string()
    });
    assert!(error.contains("not a regular file"), "{error}");
}

/// ROB-11: a config symlinked to `/dev/zero` was read until memory ran out.
#[cfg(unix)]
#[test]
fn a_config_symlinked_to_a_device_is_refused_not_read() {
    let (_tmp, config) = repo(&[]);
    std::os::unix::fs::symlink("/dev/zero", &config).unwrap();
    let error = on_thread(2 << 20, Duration::from_secs(2), move || {
        load(&config).unwrap_err().to_string()
    });
    assert!(error.contains("not a regular file"), "{error}");
}

/// YAML-5: a config past the 32 MiB cap is refused with the data-file
/// wording and its path, not ayaml's limit message with a line and column.
#[test]
fn a_config_past_the_size_cap_names_the_cap_and_the_file() {
    let wording = format!(
        "input exceeds the maximum supported size for a structured document ({} bytes)",
        alint_core::MAX_STRUCTURED_BYTES
    );
    // One byte past the cap, and past what the reader takes for a cap and a
    // byte order mark: the decoder and the reader refuse them in those words.
    for len in [1, 4] {
        let (_tmp, config) = repo(&[]);
        std::fs::File::create(&config)
            .unwrap()
            .set_len((alint_core::MAX_STRUCTURED_BYTES + len) as u64)
            .unwrap();
        let error = load(&config).unwrap_err().to_string();
        assert!(error.contains(&wording), "{error}");
        assert!(error.contains(".alint.yml"), "{error}");
        assert!(
            !error.contains("line 1") && !error.contains("I/O"),
            "{error}"
        );
    }

    let mut inline = String::from("version: 1\n# ");
    inline.push_str(&"x".repeat(alint_core::MAX_STRUCTURED_BYTES));
    let error = parse(&inline).unwrap_err().to_string();
    assert!(
        error.contains(&wording) && error.contains("<inline>"),
        "{error}"
    );
}

/// YAML-6: a byte order mark before `---` or `%YAML` was read as content.
#[test]
fn a_leading_byte_order_mark_before_a_document_marker_is_dropped() {
    for text in [
        "\u{feff}---\nversion: 1\nrules: []\n",
        "\u{feff}%YAML 1.2\n---\nversion: 1\nrules: []\n",
    ] {
        assert!(parse(text).is_ok(), "{text:?}");
        let (_tmp, config) = repo(&[(".alint.yml", text)]);
        assert!(load(&config).is_ok(), "{text:?}");
    }
}

/// YAML-7: an integer past 64 bits in a rule failed only when the merged rule
/// was decoded, naming neither the file nor the field.
#[test]
fn an_integer_past_64_bits_in_a_rule_names_the_file_and_the_field() {
    let wide = "99999999999999999999999";
    for (field, line) in [
        ("message", format!("    message: {wide}\n")),
        ("id", format!("    id: {wide}\n")),
        ("paths[1]", format!("    paths: [a, {wide}]\n")),
    ] {
        let mut body =
            String::from("version: 1\nrules:\n  - kind: file_exists\n    level: error\n");
        if field != "id" {
            body.push_str("    id: a\n");
        }
        if !field.starts_with("paths") {
            body.push_str("    paths: README.md\n");
        }
        body.push_str(&line);
        let (_tmp, config) = repo(&[(".alint.yml", &body)]);
        let error = load(&config).unwrap_err().to_string();
        assert!(error.contains(".alint.yml"), "{error}");
        assert!(error.contains(&format!("`rules[0].{field}`")), "{error}");
    }
}

/// Loading a version 1 config parses each of its sources once: the version
/// dispatch reads `version:` from the one typed decode rather than parsing
/// the document a second time (which cost a v1 load a quarter more
/// instructions and a third more memory). That holds for the top-level
/// config, a fragment, a local `extends:` target that reads the environment,
/// a bundled ruleset, a remote one and a string, and a version 2 source still
/// parses at most twice.
#[test]
fn each_source_of_a_version_one_config_parses_once() {
    let (_tmp, config) = repo(&[
        (
            ".alint.yml",
            "version: 1\nextends:\n  - ./base.yml\n  - alint://bundled/hygiene/lockfiles@v1\n\
             rules:\n  - id: readme\n    kind: file_exists\n    paths: README.md\n    \
             level: info\n",
        ),
        (
            "base.yml",
            "version: 1\nrules:\n  - id: notice\n    kind: file_exists\n    paths: NOTICE\n    \
             level: info\n    message: \"{{env.ALINT_PARSE_COUNT | default('x')}}\"\n",
        ),
    ]);
    let drop_ins = config.parent().unwrap().join(".alint.d");
    std::fs::create_dir(&drop_ins).unwrap();
    std::fs::write(drop_ins.join("10.yml"), "rules: []\n").unwrap();
    let (loaded, parses) = crate::loader::parses_during(|| load(&config));
    assert!(loaded.is_ok(), "{loaded:?}");
    assert_eq!(
        parses, 4,
        "the root, its fragment, base.yml and the bundled ruleset"
    );

    let (_, parses) =
        crate::loader::parses_during(|| load_extending("version: 1\nrules: []\n", ""));
    assert_eq!(parses, 2, "the root and the remote");
    let (inline, parses) = crate::loader::parses_during(|| parse("version: 1\nrules: []\n"));
    assert!(inline.is_ok(), "{inline:?}");
    assert_eq!(parses, 1);

    // A version 2 source's `version:` needs the parsed document.
    let (_tmp, config) = repo(&[(
        ".alint.yml",
        "version: 2\nselectors:\n  docs:\n    paths: docs/**\nrules: []\n",
    )]);
    let (loaded, parses) = crate::loader::parses_during(|| load(&config));
    assert!(loaded.is_ok(), "{loaded:?}");
    assert_eq!(parses, 2);

    // One that does not open with `version: 2` decodes as version 1 reads it
    // first: that decode is kept when the source holds no v2-only key, and one
    // that does fails it and costs a parse more. Each loads as written.
    for (body, expected, selectors) in [
        ("# v2\nversion: 2\nrules: []\n", 2, 0),
        (
            "# v2\nversion: 2\nselectors:\n  docs:\n    paths: docs/**\nrules: []\n",
            3,
            1,
        ),
    ] {
        let (_tmp, config) = repo(&[(".alint.yml", body)]);
        let (loaded, parses) = crate::loader::parses_during(|| load(&config));
        let plan = loaded.unwrap().relation_plan.expect("a version 2 plan");
        assert_eq!(plan.selectors.len(), selectors, "{body}");
        assert_eq!(parses, expected, "{body}");
    }
}

/// A version 1 config that fails to decode was parsed a second time, to read
/// its `version:` for the dispatch, whose answer for such a text is version 1
/// and the same error. It parses once now, unless its text could hold
/// something of version 2, and the error is the one it was.
#[test]
fn a_version_one_config_that_fails_to_decode_parses_once() {
    for (body, parses, error) in [
        ("version: 1\nnope: 1\n", 1, "unknown field `nope`"),
        (
            "version: 1\nrules:\n  - id: a\n    kind: file_exists\n    paths: [\n",
            1,
            "",
        ),
        (
            "version: '2'\nnope: 1\n",
            2,
            "`version:` is 2 written other than as the plain integer `2`",
        ),
        (
            "version: 1\nselectors: {}\nnope: 1\n",
            2,
            "`selectors:` needs `version: 2`",
        ),
    ] {
        let (_tmp, config) = repo(&[(".alint.yml", body)]);
        let (loaded, counted) = crate::loader::parses_during(|| load(&config));
        let shown = loaded.unwrap_err().to_string();
        assert!(shown.contains(error), "{body:?}: {shown}");
        assert_eq!(counted, parses, "{body:?}: {shown}");
    }
}

/// No code in the loader parses a whole document but through the counted
/// `decode_text`, so the test above sees every parse. (`interp.rs` re-types
/// one interpolated scalar.)
#[test]
fn every_whole_document_parse_goes_through_the_counted_decode() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    let mut pending = vec![src.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "tests" {
                    pending.push(path);
                }
                continue;
            }
            if name.ends_with("_tests.rs") || name == "tests.rs" || name == "schema_parity.rs" {
                continue;
            }
            // A Windows checkout may end its lines with CRLF.
            let text = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            // The crate's own code, before any inline test module.
            let code = text.split("\n#[cfg(test)]\nmod tests").next().unwrap();
            for _ in code.matches("alint_core::yaml::from_str") {
                let relative = path.strip_prefix(&src).unwrap().to_string_lossy();
                found.push(relative.replace('\\', "/"));
            }
        }
    }
    found.sort();
    assert_eq!(found, ["interp.rs", "loader.rs"]);
}

/// The top-level config at `path` with its `extends:` chain and fragments
/// merged, before `finalize`: the rules and templates with their markers.
fn raw_at(path: &std::path::Path) -> RawConfig {
    load_top_level_raw(path, &LoadOptions::with_catalog(test_catalog())).expect("loads")
}

/// The estimated size of `mapping`'s `marker`, as the template budget counts.
fn marker_size(mapping: &Mapping, marker: &str) -> usize {
    let held = mapping
        .get(marker)
        .and_then(ayaml::Value::as_mapping)
        .unwrap_or_else(|| panic!("no {marker} on {mapping:?}"));
    template_budget::size_of_mapping(held)
}

/// V2L-4: every field of a version 2 source's rules named the source by its
/// whole path (20,000 rules at a 3.4 KB path peaked at 380 MB, against 94 MB
/// as version 1), and every field of a rule an `extends:` entry brings in
/// named the entry by its whole text. Each marker names its source by an
/// index now, the same size whatever the path.
#[test]
fn a_rule_marker_does_not_grow_with_its_source_path() {
    const RULE: &str = "rules:\n  - {id: r, kind: file_exists, paths: x, level: error, \
                        message: m}\n";
    let origins_at = |dir: &str| {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(dir).join(".alint.yml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, format!("version: 2\n{RULE}")).unwrap();
        marker_size(&raw_at(&config).rules[0], crate::v2::FIELD_ORIGINS_MARKER)
    };
    assert_eq!(origins_at("s"), origins_at(&deep_dir()));

    // The entry's text, written as long as the platform's paths allow, names
    // an extended file's fields.
    let sources_of = |entry: &str| {
        let (_tmp, config) = repo(&[
            (".alint.yml", &format!("version: 1\nextends: ['{entry}']\n")),
            ("base.yml", &format!("version: 1\n{RULE}")),
        ]);
        let raw = raw_at(&config);
        // Its text is still what an error about a field quotes.
        let mut rule = raw.rules[0].clone();
        crate::trust::name_field_sources(&mut rule, &raw.field_source_names);
        assert_eq!(configured_field_source(&rule, "paths"), Some(entry));
        marker_size(&raw.rules[0], FIELD_SOURCES_MARKER)
    };
    assert_eq!(
        sources_of("./base.yml"),
        sources_of(&format!("{}base.yml", "./".repeat(deep_dir().len() / 2)))
    );
}

/// V2L-5: the template budget counted the loader's private markers, so a
/// template a version 2 source declares (each of its fields marked with the
/// source's path) cost more than the same template in version 1, and a config
/// that loaded as version 1 overran the budget as version 2. A template costs
/// its body alone.
#[test]
fn a_template_costs_the_same_in_either_version_and_from_any_source() {
    const TEMPLATE: &str = "templates:\n  - {id: t, kind: file_exists, paths: x, level: error, \
                            message: m}\nrules: []\n";
    let size = |files: &[(&str, &str)]| {
        let (_tmp, config) = repo(files);
        template_budget::size_of_template(&raw_at(&config).templates[0])
    };
    let one = size(&[(".alint.yml", &format!("version: 1\n{TEMPLATE}"))]);
    assert_eq!(
        one,
        size(&[(".alint.yml", &format!("version: 2\n{TEMPLATE}"))])
    );
    // Extended, with its provenance and field sources marked.
    assert_eq!(
        one,
        size(&[
            (".alint.yml", "version: 1\nextends: [./base.yml]\n"),
            ("base.yml", &format!("version: 1\n{TEMPLATE}")),
        ])
    );
    let bare: Mapping = alint_core::yaml::from_str(
        "{id: t, kind: file_exists, paths: x, level: error, message: m}",
    )
    .unwrap();
    assert_eq!(one, template_budget::size_of_mapping(&bare));
}

/// V2L-5, as reported: a template expanded by its instances to within the
/// budget loads as version 2 too, from a deep path.
#[test]
fn a_template_within_the_budget_loads_in_either_version() {
    use std::fmt::Write as _;

    // About 62 KB an instance, 1,000 of them: 62 MB of the 64 MiB budget.
    let mut body = format!(
        "templates:\n  - id: t\n    kind: file_exists\n    paths: x\n    level: error\n    \
         message: \"{}\"\nrules:\n",
        "m".repeat(62_000)
    );
    for instance in 0..1_000 {
        writeln!(body, "  - {{id: r{instance}, extends_template: t}}").unwrap();
    }
    for version in [1, 2] {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join(deep_dir()).join(".alint.yml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, format!("version: {version}\n{body}")).unwrap();
        let loaded = on_thread(8 << 20, Duration::from_secs(60), move || {
            load(&config)
                .map(|config| config.rules.len())
                .map_err(|e| e.to_string())
        });
        assert_eq!(loaded, Ok(1_000), "version {version}");
    }
}

/// V2L-5 where the loader charges it: finalize charges each instance its
/// template's body, not the markers the loader put on the template, so
/// instances whose bodies fit the budget load, though with the markers they
/// would not.
#[test]
fn an_instance_is_charged_its_templates_body_alone() {
    use std::fmt::Write as _;

    let base = format!(
        "version: 1\ntemplates:\n  - id: t\n    kind: file_exists\n    paths: x\n    \
         level: error\n    message: \"{}\"\n",
        "m".repeat(60_000)
    );
    let (_tmp, config) = repo(&[
        (".alint.yml", "version: 1\nextends: [./base.yml]\n"),
        ("base.yml", &base),
    ]);
    let template = &raw_at(&config).templates[0];
    let (body, marked) = (
        template_budget::size_of_template(template),
        template_budget::size_of_mapping(template),
    );
    let budget = template_budget::MAX_TEMPLATE_EXPANSION_BYTES;
    let instances = budget / marked + 1;
    assert!(
        body * instances <= budget,
        "{body} and {marked} bytes, {instances} instances"
    );

    let mut root = "version: 1\nextends: [./base.yml]\nrules:\n".to_string();
    for instance in 0..instances {
        writeln!(root, "  - {{id: r{instance}, extends_template: t}}").unwrap();
    }
    let (_tmp, config) = repo(&[(".alint.yml", &root), ("base.yml", &base)]);
    let loaded = on_thread(8 << 20, Duration::from_secs(60), move || {
        load(&config)
            .map(|config| config.rules.len())
            .map_err(|e| e.to_string())
    });
    assert_eq!(loaded, Ok(instances));
}

/// `config` loaded as `load` loads its chain (without its fragments), with the
/// work its `extends:` merges were charged, in entries.
fn merged_with_charge(config: &std::path::Path) -> (RawConfig, usize) {
    let mut state = loader::LoadState::default();
    let opts = LoadOptions::with_catalog(test_catalog());
    let raw = loader::load_recursive(
        config,
        &mut state,
        &opts,
        config.parent(),
        true,
        &loader::TrustedExtends::default(),
        loader::SourceContext::new(alint_core::ConfigSourceClass::Root, None),
    )
    .expect("loads");
    (raw, state.extends_entries)
}

/// A ruleset of `count` rules `<prefix>N`, each at `level`.
fn ruleset(prefix: &str, count: usize, level: &str) -> String {
    let rules = (0..count)
        .map(|index| {
            format!("  - {{id: {prefix}{index}, kind: file_exists, paths: x, level: {level}}}\n")
        })
        .collect::<Vec<_>>()
        .concat();
    format!("version: 1\nrules:\n{rules}")
}

/// Two thousand `extends:` entries of one ruleset of two thousand rules
/// loaded, gated and merged it two thousand times (152 seconds). An entry
/// that names a source an earlier entry of its list loaded reuses it, and one
/// that repeats the entry before it merges nothing; the merges' work is
/// charged against a budget ([`an_extends_merge_budget_bounds_a_load`]).
#[test]
fn an_extends_list_that_repeats_a_source_loads_it_once() {
    let list = |entries: &[&str], times: usize| {
        let entries = entries
            .iter()
            .map(|entry| format!("  - {entry}\n"))
            .collect::<Vec<_>>()
            .concat()
            .repeat(times);
        format!("version: 1\nextends:\n{entries}")
    };
    let a = ruleset("a", 50, "error");
    let b = ruleset("b", 50, "error");
    // Fifty entries that each keep another rule of one file: each loads all
    // of it.
    let filters: Vec<String> = (0..50)
        .map(|i| format!("{{url: ./a.yml, only: [a{i}]}}"))
        .collect();
    let filters: Vec<&str> = filters.iter().map(String::as_str).collect();
    // An entry that loads its source is charged the source's entries, and
    // each merge what it brings in, and each rule it merges onto one an
    // other source merged last once more.
    for (entries, times, parses, charged, rules) in [
        // The first entry loads (50) and merges (50); each of the 999
        // consecutive repeats skips the merge and charges 1 entry for itself.
        (&["./a.yml"][..], 1_000, 2, 50 + 50 + 999, 50),
        (
            &["./a.yml", "./b.yml"][..],
            500,
            3,
            (50 + 50) + (50 + 50) + 998 * 50,
            100,
        ),
        // A filter makes another source of the same file, whose `a1` each
        // entry after the first merges onto the other's.
        (
            &["./a.yml", "{url: ./a.yml, only: [a1]}"][..],
            500,
            3,
            (50 + 50) + (50 + 1) + 499 * 50 + 499 + 999,
            50,
        ),
        (&filters[..], 1, 3, 50 * 50 + 50, 50),
    ] {
        let root = list(entries, times);
        let (_tmp, config) = repo(&[(".alint.yml", &root), ("a.yml", &a), ("b.yml", &b)]);
        let ((raw, charge), parsed) = crate::loader::parses_during(|| merged_with_charge(&config));
        assert_eq!(parsed, parses, "{entries:?}: the root and each file once");
        assert_eq!(charge, charged, "{entries:?}");
        assert_eq!(raw.rules.len(), rules, "{entries:?}");
    }
}

/// A version 2 source counts in the `extends:` budget though it declares
/// nothing, as one entry: a merge looks for each among the sources it holds.
#[test]
fn a_version_two_source_counts_in_the_extends_budget() {
    let (_tmp, config) = repo(&[
        (".alint.yml", "version: 2\nextends: [./a.yml, ./b.yml]\n"),
        ("a.yml", "version: 2\n"),
        ("b.yml", "version: 2\n"),
    ]);
    let (raw, charge) = merged_with_charge(&config);
    assert_eq!(raw.v2.sources.len(), 3, "the root, `a` and `b`");
    // Loading `a` and merging it, then loading `b` and merging it.
    assert_eq!(charge, 1 + 1 + 1 + 1);
}

/// A source an entry repeats still overrides what came between, as when it
/// was loaded again: the last entry to set a field wins, and rules keep the
/// order they first appear in.
#[test]
fn a_repeated_extends_entry_still_overrides_the_entries_before_it() {
    let shared = |level: &str| {
        format!(
            "version: 1\nrules:\n  - {{id: r, kind: file_exists, paths: x, level: {level}}}\n  \
             - {{id: {level}-only, kind: file_exists, paths: x, level: info}}\n"
        )
    };
    for (entries, level, order) in [
        (
            "[./a.yml, ./b.yml, ./a.yml]",
            "error",
            ["r", "error-only", "warning-only"],
        ),
        (
            "[./a.yml, ./b.yml, ./b.yml]",
            "warning",
            ["r", "error-only", "warning-only"],
        ),
        (
            "[./b.yml, ./a.yml, ./a.yml, ./b.yml]",
            "warning",
            ["r", "warning-only", "error-only"],
        ),
    ] {
        let (_tmp, config) = repo(&[
            (".alint.yml", &format!("version: 1\nextends: {entries}\n")),
            ("a.yml", &shared("error")),
            ("b.yml", &shared("warning")),
        ]);
        let loaded = load(&config).unwrap();
        let ids: Vec<&str> = loaded.rules.iter().map(|rule| rule.id.as_str()).collect();
        assert_eq!(ids, order, "{entries}");
        assert_eq!(loaded.rules[0].level.as_str(), level, "{entries}");
    }
}

/// What the `extends:` merges of one load handle, in all, is held to a
/// budget, so a list that alternates two sources cannot merge without end.
/// Each merge is charged what it brings in, and at least one entry. (The
/// loader charges every merge of a list:
/// [`an_extends_list_that_repeats_a_source_loads_it_once`].)
#[test]
fn an_extends_merge_budget_bounds_a_load() {
    let entries = |count: usize| RawConfig {
        ignore: (0..count).map(|i| i.to_string()).collect(),
        ..RawConfig::default()
    };
    let mut state = loader::LoadState::default();
    let large = entries(loader::MAX_EXTENDS_MERGE_WORK / 2);
    state.charge_extends(&large).expect("half the budget");
    state
        .charge_extends(&entries(loader::MAX_EXTENDS_MERGE_WORK / 2 - 1))
        .expect("all of it but one entry");
    // A merge of nothing still counts.
    state
        .charge_extends(&RawConfig::default())
        .expect("all of it");
    assert_eq!(
        state
            .charge_extends(&RawConfig::default())
            .unwrap_err()
            .to_string(),
        format!(
            "the `extends:` entries and `.alint.d/` drop-ins of this config load and merge more \
             than {} entries in all (rules, templates, facts, vars, `ignore:` patterns, the \
             `vars.` reads in an untrusted remote's `when:`, and each version 2 source, selector \
             and relation, counted for each source an entry loads, again for what each entry or \
             drop-in brings in to a merge, and again for each rule or template a merge combines \
             with one of its id, but not one an earlier entry naming the same source, with the \
             same filters, wrote last); list each source once, remove repeated rule and template \
             ids within a source, extend fewer sources, use fewer drop-ins, or flatten the \
             `extends:` chain",
            loader::MAX_EXTENDS_MERGE_WORK
        )
    );
}
