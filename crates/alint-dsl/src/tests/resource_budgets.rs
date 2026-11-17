//! A config is untrusted for what it costs to load: the reader, the
//! `requires:` check, `extends:` merging and template expansion are bounded.

use std::time::Duration;

use super::*;

/// Run `job` on a thread with a stack of `stack` bytes, and fail unless it
/// returns within `limit` (a hang leaves the thread behind, but fails the test).
fn on_thread<T: Send + 'static>(
    stack: usize,
    limit: Duration,
    job: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || drop(sender.send(job())))
        .unwrap();
    receiver
        .recv_timeout(limit)
        .unwrap_or_else(|_| panic!("did not finish within {limit:?}"))
}

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
    assert!(error.contains("the merged config holds"), "{error}");
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
            let text = std::fs::read_to_string(&path).unwrap();
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
