use super::*;
use crate::test_support::{ctx, spec_yaml, tempdir_with_files};
use alint_core::{FindingDisposition, Violation};

fn run(yaml: &str, files: &[(&str, &[u8])]) -> Vec<Violation> {
    let (tmp, index) = tempdir_with_files(files);
    index.enable_relation_caches();
    let rule = build(&spec_yaml(yaml)).expect("rule builds");
    rule.evaluate(&ctx(tmp.path(), &index))
        .expect("rule evaluates")
}

fn messages(violations: &[Violation]) -> Vec<String> {
    violations.iter().map(|v| v.message.to_string()).collect()
}

#[test]
fn format_inference_and_models() {
    assert_eq!(infer_format(Path::new("a.json")), Some(KeyFormat::Json));
    assert_eq!(infer_format(Path::new("a.yml")), Some(KeyFormat::Yaml));
    assert_eq!(
        infer_format(Path::new(".env.example")),
        Some(KeyFormat::Dotenv)
    );
    assert_eq!(
        infer_format(Path::new("a.properties")),
        Some(KeyFormat::Properties)
    );
    assert_eq!(infer_format(Path::new("a.ini")), Some(KeyFormat::Ini));
    assert_eq!(infer_format(Path::new("a.toml")), Some(KeyFormat::Toml));
    assert_eq!(infer_format(Path::new("a.xml")), None);
    assert_eq!(
        infer_format(Path::new("Strings.fr.resx")),
        Some(KeyFormat::Resx)
    );
    assert_eq!(KeyFormat::Json.model(), KeyModel::Tree);
    assert_eq!(KeyFormat::Toml.model(), KeyModel::Tree);
    assert_eq!(KeyFormat::Dotenv.model(), KeyModel::Flat);
    assert_eq!(KeyFormat::AndroidStrings.model(), KeyModel::AndroidStrings);
    assert_eq!(KeyFormat::Resx.model(), KeyModel::Resx);
}

#[test]
fn leaf_keys_drop_containers_and_array_members() {
    let mut keys = BTreeMap::new();
    for (parts, span) in [
        (vec![Segment::Key("nav".into())], ByteSpan::new(0, 1)),
        (
            vec![Segment::Key("nav".into()), Segment::Key("home".into())],
            ByteSpan::new(2, 3),
        ),
        (
            vec![
                Segment::Key("list".into()),
                Segment::Index(0),
                Segment::Key("x".into()),
            ],
            ByteSpan::new(4, 5),
        ),
    ] {
        keys.insert(parts, span);
    }
    let keys = leaf_keys(keys);
    assert_eq!(
        keys.keys().map(|key| render_key(key)).collect::<Vec<_>>(),
        ["nav.home"]
    );
}

/// One file's keys with the CLDR plural groups it forms on its own.
fn merged_alone(keys: &Keys) -> Keys {
    let nested = merge_nested_groups(keys, &nested_plural_groups(&[keys]));
    merge_i18next_groups(&nested, &i18next_plural_groups(&[&nested]))
}

#[test]
fn plural_groups_merge_nested_and_i18next_keys() {
    let mut keys = BTreeMap::new();
    for (parts, span) in [
        (
            vec![Segment::Key("item".into()), Segment::Key("one".into())],
            ByteSpan::new(0, 1),
        ),
        (
            vec![Segment::Key("item".into()), Segment::Key("other".into())],
            ByteSpan::new(2, 3),
        ),
        (vec![Segment::Key("msg_one".into())], ByteSpan::new(4, 5)),
        (vec![Segment::Key("msg_other".into())], ByteSpan::new(6, 7)),
    ] {
        keys.insert(parts, span);
    }
    let keys = merged_alone(&keys);
    assert_eq!(
        keys.keys().map(|key| render_key(key)).collect::<Vec<_>>(),
        ["item", "msg"]
    );
}

#[test]
fn ranges_use_a_line_index_and_ignore_the_bom_column() {
    let text = "\u{feff}a\r\nbb\nç\rx";
    let file = KeyFile {
        keys: BTreeMap::from([
            (vec![Segment::Key("a".into())], ByteSpan::new(3, 4)),
            (vec![Segment::Key("bb".into())], ByteSpan::new(6, 8)),
            (vec![Segment::Key("ç".into())], ByteSpan::new(9, 11)),
            (vec![Segment::Key("x".into())], ByteSpan::new(12, 13)),
        ]),
        text: Arc::from(text),
        line_starts: line_starts(text),
    };
    let range = |key: &str| {
        file.range(&[Segment::Key(key.into())])
            .expect("range resolves")
    };
    assert_eq!((range("a").start_line(), range("a").start_column()), (1, 1));
    assert_eq!(
        (range("bb").start_line(), range("bb").start_column()),
        (2, 1)
    );
    assert_eq!((range("ç").start_line(), range("ç").start_column()), (3, 1));
    assert_eq!((range("x").start_line(), range("x").start_column()), (4, 1));
    assert_eq!(range("ç").end_column(), 2);
}

#[test]
fn ignore_globs_respect_segment_boundaries() {
    assert!(key_glob_match("nav.*", "nav.home"));
    assert!(!key_glob_match("nav.*", "nav.home.x"));
    assert!(key_glob_match("nav.**", "nav.home.x"));
    assert!(key_glob_match("nav.**", "nav."));
}

#[test]
fn build_rejects_an_unconfined_reference() {
    let spec = spec_yaml(
        "id: t\nkind: key_parity\nreference: ../en.json\npeers: '*.json'\nlevel: error\n",
    );
    let error = build(&spec).unwrap_err().to_string();
    assert!(error.contains("confined"), "{error}");
}

#[test]
fn build_rejects_an_uninferable_reference_format() {
    let spec = spec_yaml(
        "id: t\nkind: key_parity\nreference: locales/en\npeers: 'locales/*'\nlevel: error\n",
    );
    let error = build(&spec).unwrap_err().to_string();
    assert!(error.contains("format"), "{error}");
}

#[test]
fn build_rejects_a_literal_peer_in_another_key_model() {
    let error = |yaml: &str| build(&spec_yaml(yaml)).err().map(|e| e.to_string());
    let mismatch =
        error("id: t\nkind: key_parity\nreference: en.json\npeers: fr.properties\nlevel: error\n");
    assert!(
        mismatch
            .as_deref()
            .is_some_and(|e| e.contains("different key model")),
        "{mismatch:?}"
    );
    let uninferable =
        error("id: t\nkind: key_parity\nreference: en.json\npeers: locales/fr\nlevel: error\n");
    assert!(
        uninferable
            .as_deref()
            .is_some_and(|e| e.contains("literal `peers` path")),
        "{uninferable:?}"
    );
    // The same model, or an explicit `format` that every file shares, builds.
    for yaml in [
        "id: t\nkind: key_parity\nreference: en.json\npeers: fr.yaml\nlevel: error\n",
        "id: t\nkind: key_parity\nreference: .env.example\npeers: .env\nlevel: error\n",
        "id: t\nkind: key_parity\nreference: en.json\npeers: locales/fr\nformat: json\nlevel: error\n",
    ] {
        assert_eq!(error(yaml), None, "{yaml}");
    }
}

#[test]
fn reports_missing_and_extra_keys() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
        &[
            (
                "locales/en.json",
                br#"{"nav":{"home":"Home","about":"About"}}"#,
            ),
            (
                "locales/fr.json",
                br#"{"nav":{"home":"Accueil","contact":"Contact"}}"#,
            ),
        ],
    );
    let messages = messages(&findings);
    assert!(
        messages
            .iter()
            .any(|m| m.contains("missing key `nav.about`")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("extra key `nav.contact`")),
        "{messages:?}"
    );
    assert!(
        findings
            .iter()
            .all(|v| v.disposition == FindingDisposition::Violation)
    );
}

#[test]
fn direction_selects_one_side() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\ndirection: missing\nlevel: error\n",
        &[
            ("locales/en.json", br#"{"a":1}"#),
            ("locales/fr.json", br#"{"a":1,"b":2}"#),
        ],
    );
    assert!(findings.is_empty(), "{:?}", messages(&findings));
}

#[test]
fn strip_root_compares_below_the_locale_key() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: locales/en.yml\npeers: 'locales/*.yml'\nstrip_root: true\nlevel: error\n",
        &[
            (
                "locales/en.yml",
                b"en:\n  title: Title\n  nav:\n    home: Home\n",
            ),
            ("locales/fr.yml", b"fr:\n  title: Titre\n"),
        ],
    );
    let messages = messages(&findings);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(
        messages[0].contains("missing key `nav.home`"),
        "{messages:?}"
    );
}

#[test]
fn cldr_plural_groups_are_one_key() {
    let exact = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
        &[
            (
                "locales/en.json",
                br#"{"item":{"one":"one","other":"other"}}"#,
            ),
            (
                "locales/fr.json",
                br#"{"item":{"one":"un","other":"autre"}}"#,
            ),
        ],
    );
    assert!(exact.is_empty(), "{:?}", messages(&exact));
    let cldr = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nplurals: cldr\nlevel: error\n",
        &[
            (
                "locales/en.json",
                br#"{"item":{"one":"one","other":"other"}}"#,
            ),
            (
                "locales/fr.json",
                br#"{"item":{"one":"un","other":"autre"}}"#,
            ),
        ],
    );
    assert!(cldr.is_empty(), "{:?}", messages(&cldr));
}

#[test]
fn ignore_keys_omits_a_known_gap() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nignore_keys: ['nav.about']\nlevel: error\n",
        &[
            (
                "locales/en.json",
                br#"{"nav":{"home":"Home","about":"About"}}"#,
            ),
            ("locales/fr.json", br#"{"nav":{"home":"Accueil"}}"#),
        ],
    );
    assert!(findings.is_empty(), "{:?}", messages(&findings));
}

#[test]
fn flat_and_ini_formats_compare() {
    let dotenv = run(
        "id: t\nkind: key_parity\nreference: .env.example\npeers: '.env'\nlevel: error\n",
        &[(".env.example", b"A=1\nB=2\n"), (".env", b"A=1\n")],
    );
    assert!(
        messages(&dotenv)
            .iter()
            .any(|m| m.contains("missing key `B`")),
        "{:?}",
        messages(&dotenv)
    );
    let ini = run(
        "id: t\nkind: key_parity\nreference: a.ini\npeers: 'b.ini'\nlevel: error\n",
        &[("a.ini", b"[s]\na=1\nb=2\n"), ("b.ini", b"[s]\na=1\n")],
    );
    assert!(
        messages(&ini)
            .iter()
            .any(|m| m.contains("missing key `s.b`")),
        "{:?}",
        messages(&ini)
    );
}

#[test]
fn a_sensitive_peer_parse_failure_is_redacted() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: .env.example\npeers: '.env'\nlevel: error\n",
        &[
            (".env.example", b"A=1\n"),
            (".env", b"A=\"top-secret-value\n"),
        ],
    );
    assert!(
        findings
            .iter()
            .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
        "{:?}",
        messages(&findings)
    );
    assert!(
        findings
            .iter()
            .all(|v| !v.message.contains("top-secret-value")),
        "{:?}",
        messages(&findings)
    );
}

#[test]
fn toml_android_and_resx_formats_compare() {
    let toml = run(
        "id: t\nkind: key_parity\nreference: locales/en.toml\npeers: 'locales/*.toml'\nlevel: error\n",
        &[
            (
                "locales/en.toml",
                b"[nav]\nhome = \"Home\"\nabout = \"About\"\n",
            ),
            ("locales/fr.toml", b"[nav]\nhome = \"Accueil\"\n"),
        ],
    );
    assert!(
        messages(&toml)
            .iter()
            .any(|m| m.contains("missing key `nav.about`")),
        "{:?}",
        messages(&toml)
    );

    let android = run(
        "id: t\nkind: key_parity\nreference: values/strings.xml\npeers: 'values-*/*.xml'\nformat: android-strings\nlevel: error\n",
        &[
            ("values/strings.xml", b"<resources><string name=\"app\">App</string><string name=\"build\" translatable=\"false\">1</string></resources>"),
            ("values-fr/strings.xml", b"<resources><string name=\"app\">App</string></resources>"),
        ],
    );
    assert!(android.is_empty(), "{:?}", messages(&android));

    let resx = run(
        "id: t\nkind: key_parity\nreference: Strings.resx\npeers: 'Strings.*.resx'\nformat: resx\nlevel: error\n",
        &[
            ("Strings.resx", b"<root><data name=\"Greeting\"><value>Hello</value></data><data name=\"Icon\" type=\"Bitmap\"/></root>"),
            ("Strings.fr.resx", b"<root><data name=\"Greeting\"><value>Bonjour</value></data></root>"),
        ],
    );
    assert!(resx.is_empty(), "{:?}", messages(&resx));
}

#[test]
fn missing_reference_parse_failure_and_empty_peers_fail_loudly() {
    let missing = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
        &[("locales/fr.json", br#"{"a":1}"#)],
    );
    assert!(
        missing
            .iter()
            .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
        "{:?}",
        messages(&missing)
    );
    let parse = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
        &[
            ("locales/en.json", br#"{"a":1}"#),
            ("locales/fr.json", b"{"),
        ],
    );
    assert!(
        parse
            .iter()
            .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
        "{:?}",
        messages(&parse)
    );
    let empty = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
        &[("locales/en.json", br#"{"a":1}"#)],
    );
    assert!(
        empty
            .iter()
            .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
        "{:?}",
        messages(&empty)
    );
}

#[test]
fn a_reference_that_is_a_directory_is_reported_without_an_absolute_path() {
    // `locales/en.json/` is a directory, so the literal read refuses it. The
    // finding names the repo-relative path only: an absolute one would make
    // findings, fingerprints, and the corpus replay depend on the checkout.
    let violations = run(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.yml'\nlevel: error\n",
        &[("locales/en.json/keep", b""), ("locales/fr.yml", b"a: 1\n")],
    );
    assert_eq!(
        messages(&violations),
        vec!["locales/en.json: could not read file: not a regular file".to_string()],
    );
    assert!(matches!(
        violations[0].disposition,
        FindingDisposition::AnalysisFailure(_)
    ));
}

#[test]
fn the_rule_declares_the_relation_cache_and_full_index() {
    let spec = spec_yaml(
        "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n",
    );
    let rule = build(&spec).unwrap();
    assert!(rule.uses_relations());
    assert!(rule.requires_full_index());
    assert!(rule.path_scope().is_some());
    assert!(!rule.supports_expect_matches());
}

fn rule(yaml: &str) -> KeyParityRule {
    build_rule(&spec_yaml(yaml)).expect("rule builds")
}

fn summary(violations: &[Violation]) -> Vec<String> {
    let mut out: Vec<String> = violations
        .iter()
        .map(|v| {
            format!(
                "{:?}|{}|{:?}|{:?}|{:?}",
                v.disposition,
                v.message,
                v.path,
                v.range.as_ref().map(|r| (r.start_line(), r.start_column())),
                v.baseline_key
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn dotted_and_bracketed_keys_are_compared_by_identity() {
    let properties = run(
        "id: t\nkind: key_parity\nreference: en.properties\npeers: '*.properties'\nlevel: error\n",
        &[
            ("en.properties", b"app.title=Hello\napp.name=X\nother=1\n"),
            ("fr.properties", b"other=1\nmenu.item=Y\n"),
        ],
    );
    // A single-name format shows each key as written (ADR-0025), and the
    // baseline key carries the same form.
    let msgs = messages(&properties);
    for expected in [
        "missing key `app.title`",
        "missing key `app.name`",
        "extra key `menu.item`",
    ] {
        assert!(
            msgs.iter().any(|m| m.contains(expected)),
            "{expected}: {msgs:?}"
        );
    }
    assert_eq!(msgs.len(), 3, "{msgs:?}");
    assert!(
        properties
            .iter()
            .filter_map(|v| v.baseline_key.as_deref())
            .any(|key| key.contains("\u{0}app.title\u{0}")),
        "{:?}",
        properties
            .iter()
            .map(|v| &v.baseline_key)
            .collect::<Vec<_>>()
    );

    // A flat dotted key and a nested key are different keys (V21-4).
    let json = run(
        "id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n",
        &[
            (
                "en.json",
                br#"{"nav":{"home":"Home"},"x":{"":"e","q'":"q"}}"#,
            ),
            (
                "fr.json",
                br#"{"nav.home":"Accueil","x":{"":"e","q'":"q"}}"#,
            ),
        ],
    );
    let msgs = messages(&json);
    assert!(
        msgs.iter().any(|m| m.contains("missing key `nav.home`")),
        "{msgs:?}"
    );
    assert!(
        msgs.iter().any(|m| m.contains("extra key `['nav.home']`")),
        "{msgs:?}"
    );
    assert_eq!(msgs.len(), 2, "{msgs:?}");

    let resx = run(
        "id: t\nkind: key_parity\nreference: Form.resx\npeers: 'Form.*.resx'\nlevel: error\n",
        &[
            ("Form.resx", b"<root><data name=\"button1.Text\"><value>OK</value></data><data name=\"Title\"><value>T</value></data></root>"),
            ("Form.fr.resx", b"<root><data name=\"Title\"><value>T</value></data></root>"),
        ],
    );
    assert!(
        messages_of(&resx)
            .iter()
            .any(|m| m.contains("button1.Text")),
        "{:?}",
        messages_of(&resx)
    );
}

#[test]
fn dotted_dotenv_and_ini_keys_compare_by_identity() {
    // Dotted dotenv names and INI keys inside a section compare by identity
    // too; an INI key keeps its section path.
    let dotenv = messages(&run(
        "id: t\nkind: key_parity\nreference: .env.example\npeers: .env\nlevel: error\n",
        &[
            (".env.example", b"APP.NAME=x\nAPP=y\n"),
            (".env", b"APP=y\n"),
        ],
    ));
    assert_eq!(dotenv.len(), 1, "{dotenv:?}");
    assert!(dotenv[0].contains("missing key `APP.NAME`"), "{dotenv:?}");
    let ini = messages(&run(
        "id: t\nkind: key_parity\nreference: en.ini\npeers: '*.ini'\nlevel: error\n",
        &[
            ("en.ini", b"[Session]\nsession.save_path=/tmp\n"),
            ("fr.ini", b"[Session.session]\nsave_path=/tmp\n"),
        ],
    ));
    assert_eq!(ini.len(), 2, "{ini:?}");
    assert!(
        ini.iter()
            .any(|m| m.contains("missing key `Session['session.save_path']`")),
        "{ini:?}"
    );
    assert!(
        ini.iter()
            .any(|m| m.contains("extra key `['Session.session'].save_path`")),
        "{ini:?}"
    );
}

#[test]
fn single_name_ignore_patterns_match_the_name() {
    // `ignore_keys` matches the shown form: a dotted flat name by its name,
    // where `*` stops at a dot and `**` crosses it.
    let ignored = |pattern: &str| {
        let yaml = format!(
            "id: t\nkind: key_parity\nreference: en.properties\npeers: '*.properties'\nignore_keys: [\"{pattern}\"]\nlevel: error\n"
        );
        messages(&run(
            &yaml,
            &[
                (
                    "en.properties",
                    b"app.title=Hello\napp.menu.item=M\nother=1\n",
                ),
                ("fr.properties", b"other=1\n"),
            ],
        ))
    };
    assert_eq!(ignored("app.**"), Vec::<String>::new());
    assert_eq!(ignored("app.*").len(), 1, "`*` stops at the dot");
    assert_eq!(
        ignored("['app.*']").len(),
        1,
        "a bracketed pattern is the name it brackets"
    );
}

#[test]
fn bracket_shown_single_names_are_ignored_as_shown() {
    // A name that is empty or starts with `[` is shown in bracket form, and
    // `ignore_keys` takes the form the finding shows.
    let run_with = |ignore: &str| {
        messages(&run(
            &format!(
                "id: t\nkind: key_parity\nreference: en.properties\npeers: fr.properties\nignore_keys: {ignore}\nlevel: error\n"
            ),
            &[
                ("en.properties", b"[x=1\n=2\nkept=3\n"),
                ("fr.properties", b"kept=3\n"),
            ],
        ))
    };
    let shown = run_with("[]");
    assert!(shown.iter().any(|m| m.contains("`['[x']`")), "{shown:?}");
    assert!(shown.iter().any(|m| m.contains("`['']`")), "{shown:?}");
    assert_eq!(run_with(r#"["['[x']", "['']"]"#), Vec::<String>::new());
    let error = build(&spec_yaml(
        "id: t\nkind: key_parity\nreference: en.properties\npeers: fr.properties\nignore_keys: [\"['a'].b\"]\nlevel: error\n",
    ))
    .expect_err("one bracketed name only");
    assert!(error.to_string().contains("one `['...']`"), "{error}");
}

#[test]
fn index_patterns_and_padded_peers_are_config_errors() {
    for (options, expected) in [
        (
            "peers: '*.json'\nignore_keys: ['items[0]']",
            "never matches",
        ),
        ("peers: ' *.json'", "whitespace"),
    ] {
        let error = build(&spec_yaml(&format!(
            "id: t\nkind: key_parity\nreference: en.json\n{options}\nlevel: error\n"
        )))
        .expect_err(options);
        assert!(error.to_string().contains(expected), "{options}: {error}");
    }
}

#[test]
fn glob_matchers_compare_bytes_like_characters() {
    assert!(key_glob_match("é*", "éa"));
    assert!(key_glob_match("*é", "aé"));
    assert!(!key_glob_match("a*b", "a.b"));
    assert!(key_glob_match("a**b", "a.b"));
    assert!(key_glob_match("app.**", "app.menu.item"));
    assert!(!key_glob_match("app.*", "app.menu.item"));
    assert!(key_glob_match("", ""));
    assert!(!key_glob_match("", "x"));
    assert!(wildcard_match("*é*", "aéb"));
    assert!(!wildcard_match("é", "e"));
    assert!(wildcard_match("a*", "a"));
}

#[test]
fn parse_failures_never_quote_values() {
    const SECRET: &str = "MIIEowIBAAKCAQEAsecretmaterial";
    let ini = format!("[s]\npassword = -----BEGIN RSA PRIVATE KEY-----\n{SECRET}\n");
    let android = format!("<resources><string name=\"a\">&{SECRET};</string></resources>");
    let ini_reference: (&str, &[u8]) = ("en.ini", b"[s]\npassword=x\n");
    let android_reference: (&str, &[u8]) = (
        "values/strings.xml",
        b"<resources><string name=\"a\">A</string></resources>",
    );
    for (yaml, reference, peer, bytes) in [
        (
            "reference: en.ini\npeers: fr.ini",
            ini_reference,
            "fr.ini",
            ini.as_bytes(),
        ),
        (
            "reference: values/strings.xml\npeers: 'values-*/strings.xml'\nformat: android-strings",
            android_reference,
            "values-fr/strings.xml",
            android.as_bytes(),
        ),
    ] {
        let findings = run(
            &format!("id: t\nkind: key_parity\n{yaml}\nlevel: error\n"),
            &[reference, (peer, bytes)],
        );
        assert!(
            findings
                .iter()
                .any(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_))),
            "{peer}: {:?}",
            messages(&findings)
        );
        for message in messages(&findings) {
            assert!(!message.contains(SECRET), "{peer}: {message}");
        }
    }
}

#[test]
fn plural_groups_are_decided_over_both_files() {
    let cldr = |reference: &str, peer: &str, files: &[(&str, &[u8])]| {
        messages(&run(
            &format!(
                "id: t\nkind: key_parity\nreference: {reference}\npeers: {peer}\nplurals: cldr\nlevel: error\n"
            ),
            files,
        ))
    };
    // A locale with fewer categories keeps the group.
    assert_eq!(
        cldr(
            "en.json",
            "fr.json",
            &[
                ("en.json", br#"{"item_one":"a","item_other":"b"}"#),
                ("fr.json", br#"{"item_one":"a"}"#),
            ]
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        cldr(
            "en.yml",
            "fr.yml",
            &[
                ("en.yml", b"count:\n  one: a\n  other: b\n"),
                ("fr.yml", b"count:\n  one: a\n"),
            ]
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        cldr(
            "values/strings.xml",
            "values-fr/strings.xml\nformat: android-strings",
            &[
                (
                    "values/strings.xml",
                    b"<resources><string name=\"share_one\">a</string><string name=\"share_other\">b</string></resources>"
                ),
                (
                    "values-fr/strings.xml",
                    b"<resources><string name=\"share_one\">a</string></resources>"
                ),
            ]
        ),
        Vec::<String>::new()
    );
    // A base that is a real key in either file is never merged, so the
    // difference is the real key.
    let missing = cldr(
        "en.json",
        "fr.json",
        &[
            ("en.json", br#"{"msg":"m","msg_one":"a","msg_other":"b"}"#),
            ("fr.json", br#"{"msg_one":"a","msg_other":"b"}"#),
        ],
    );
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(missing[0].contains("missing key `msg`"), "{missing:?}");
    let extra = cldr(
        "en.json",
        "fr.json",
        &[
            ("en.json", br#"{"msg_one":"a","msg_other":"b"}"#),
            ("fr.json", br#"{"msg":"m","msg_one":"a","msg_other":"b"}"#),
        ],
    );
    assert_eq!(extra.len(), 1, "{extra:?}");
    assert!(extra[0].contains("extra key `msg`"), "{extra:?}");
}

#[test]
fn strip_root_needs_one_object_root_in_every_file() {
    let strip = |files: &[(&str, &[u8])]| {
        run(
            "id: t\nkind: key_parity\nreference: en.yml\npeers: '*.yml'\nstrip_root: true\nlevel: error\n",
            files,
        )
    };
    // A lone leaf is not a root to strip below.
    let leaf = strip(&[
        ("en.yml", b"greeting: Hello\n"),
        ("fr.yml", b"farewell: Bye\n"),
    ]);
    assert_eq!(leaf.len(), 1, "{:?}", messages(&leaf));
    assert!(matches!(
        leaf[0].disposition,
        FindingDisposition::AnalysisFailure(_)
    ));
    assert!(leaf[0].message.contains("an object"), "{}", leaf[0].message);
    // One file without a single root fails alone; the others still compare.
    let stray = strip(&[
        ("en.yml", b"en:\n  a: A\n  b: B\n"),
        ("de.yml", b"de:\n  a: A\n  b: B\n"),
        ("fr.yml", b"fr:\n  a: A\n  b: B\nstray: x\n"),
    ]);
    assert_eq!(stray.len(), 1, "{:?}", messages(&stray));
    assert_eq!(stray[0].path.as_deref(), Some(Path::new("fr.yml")));
    assert!(
        stray[0].message.contains("one top-level key"),
        "{}",
        stray[0].message
    );
    let error = build(&spec_yaml(
        "id: t\nkind: key_parity\nreference: en.properties\npeers: '*.properties'\nstrip_root: true\nlevel: error\n",
    ))
    .expect_err("strip_root needs a tree format");
    assert!(
        error.to_string().contains("JSON, YAML, and TOML"),
        "{error}"
    );
}

#[test]
fn an_explicit_format_still_refuses_another_key_model() {
    let error = build(&spec_yaml(
        "id: t\nkind: key_parity\nreference: en.yml\npeers: fr.properties\nformat: yaml\nlevel: error\n",
    ))
    .expect_err("a literal peer in another model");
    assert!(error.to_string().contains("(properties)"), "{error}");
    let findings = run(
        "id: t\nkind: key_parity\nreference: locales/en.yml\npeers: 'locales/*'\nformat: yaml\nlevel: error\n",
        &[
            ("locales/en.yml", b"a: 1\n"),
            ("locales/fr.properties", b"a=1\n"),
        ],
    );
    assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
    assert!(matches!(
        findings[0].disposition,
        FindingDisposition::AnalysisFailure(_)
    ));
    assert!(
        findings[0]
            .message
            .contains("format properties has a different key model"),
        "{}",
        findings[0].message
    );
}

#[test]
fn yaml_complex_keys_fail_instead_of_moving_values() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: en.yml\npeers: fr.yml\nlevel: error\n",
        &[
            ("en.yml", b"nav:\n  home: Home\n"),
            ("fr.yml", b"? [complex, key]\n: nav:\n    home: Accueil\n"),
        ],
    );
    assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
    assert!(matches!(
        findings[0].disposition,
        FindingDisposition::AnalysisFailure(_)
    ));
    assert!(
        findings[0].message.contains("complex"),
        "{}",
        findings[0].message
    );
}

fn messages_of(violations: &[Violation]) -> Vec<String> {
    messages(violations)
}

#[test]
fn an_empty_or_fully_ignored_peer_reports_every_reference_key() {
    let empty = run(
        "id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n",
        &[("en.json", br#"{"a":1,"b":{"c":2}}"#), ("fr.json", b"{}")],
    );
    let msgs = messages(&empty);
    assert!(
        msgs.iter().any(|m| m.contains("missing key `a`")),
        "{msgs:?}"
    );
    assert!(
        msgs.iter().any(|m| m.contains("missing key `b.c`")),
        "{msgs:?}"
    );
    assert_eq!(msgs.len(), 2, "{msgs:?}");

    let ignored = run(
        "id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nignore_keys: ['legacy.**']\nlevel: error\n",
        &[
            ("en.json", br#"{"a":1,"legacy":{"x":1}}"#),
            ("fr.json", br#"{"legacy":{"y":1}}"#),
        ],
    );
    let msgs = messages(&ignored);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("missing key `a`"), "{msgs:?}");

    let empty_resx = run(
        "id: t\nkind: key_parity\nreference: S.resx\npeers: 'S.*.resx'\nlevel: error\n",
        &[
            (
                "S.resx",
                b"<root><data name=\"Greeting\"><value>Hi</value></data></root>",
            ),
            ("S.fr.resx", b"<root/>"),
        ],
    );
    assert!(
        messages(&empty_resx)
            .iter()
            .any(|m| m.contains("missing key `Greeting`")),
        "{:?}",
        messages(&empty_resx)
    );
}

#[test]
fn a_comparison_error_is_an_analysis_failure_not_a_pass() {
    let (tmp, index) =
        tempdir_with_files(&[("en.json", br#"{"a":1,"b":2}"#), ("fr.json", br#"{"c":3}"#)]);
    index.enable_relation_caches();
    let rule = rule("id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n");
    let context = ctx(tmp.path(), &index);
    let read_cache = index.read_cache().unwrap();
    let decode_cache = index.decode_cache().unwrap();
    let limits = Limits::default();
    let mut findings = Vec::new();
    let load = |path: &str, findings: &mut Vec<Violation>| {
        let resource = Resource::literal(RelPath::parse(path).unwrap());
        let file = rule
            .read_file(
                &context,
                read_cache,
                decode_cache,
                &limits,
                &resource,
                KeyFormat::Json,
                false,
                findings,
            )
            .unwrap();
        (resource, file)
    };
    let (reference, reference_file) = load("en.json", &mut findings);
    let (peer, peer_file) = load("fr.json", &mut findings);
    let reference_keys = base_keys(&reference_file, false);
    let peer_keys = base_keys(&peer_file, false);
    let rows = key_rows(&reference, &reference_keys).unwrap();
    let compared = Compared {
        file: &reference_file,
        keys: &reference_keys,
        rows: &rows,
    };
    // Three differences against a one-finding limit: the bridge refuses
    // the analyzer's output, and the rule must say so.
    let tight = Limits::default()
        .with(alint_core::relation::LimitKind::RowsPerExtractor, 1)
        .unwrap();
    rule.compare(
        &tight,
        &compared,
        &peer,
        &peer_file,
        &peer_keys,
        &mut findings,
    );
    assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
    assert!(
        matches!(
            findings[0].disposition,
            FindingDisposition::AnalysisFailure(_)
        ),
        "{:?}",
        messages(&findings)
    );
    assert!(
        findings[0].message.contains("key comparison failed"),
        "{:?}",
        messages(&findings)
    );

    // A row from a third file is refused rather than silently compared.
    let mut foreign = rows.clone();
    foreign.extend(
        key_rows(
            &Resource::literal(RelPath::parse("de.json").unwrap()),
            &peer_keys,
        )
        .unwrap(),
    );
    let error = ContractAnalyzer::new(KeySetAnalyzer {
        reference: "en.json".into(),
        peer: "fr.json".into(),
        direction: Direction::Both,
    })
    .analyze("t", &[], &foreign, &Limits::default())
    .unwrap_err();
    assert!(
        error.to_string().contains("neither compared file"),
        "{error}"
    );
}

#[test]
fn a_literal_peer_is_read_even_when_the_index_omits_it() {
    let (tmp, full) = tempdir_with_files(&[
        (".env.example", b"A=1\nB=2\n"),
        (".env", b"A=1\n"),
        (".gitignore", b".env\n"),
    ]);
    // The walked index honours .gitignore, so it never lists `.env`.
    let index = alint_core::FileIndex::from_entries(
        full.entries
            .iter()
            .filter(|entry| entry.path.as_ref() != Path::new(".env"))
            .cloned()
            .collect(),
    );
    index.enable_relation_caches();
    let yaml = "id: t\nkind: key_parity\nreference: .env.example\npeers: .env\nlevel: error\n";
    let findings = rule(yaml).evaluate(&ctx(tmp.path(), &index)).unwrap();
    let msgs = messages(&findings);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("missing key `B`"), "{msgs:?}");

    std::fs::remove_file(tmp.path().join(".env")).unwrap();
    let index = alint_core::FileIndex::from_entries(index.entries.clone());
    index.enable_relation_caches();
    let findings = rule(yaml).evaluate(&ctx(tmp.path(), &index)).unwrap();
    assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
    assert!(
        matches!(
            findings[0].disposition,
            FindingDisposition::AnalysisFailure(_)
        ),
        "{:?}",
        messages(&findings)
    );
    assert!(
        findings[0].message.contains("peer file is missing"),
        "{:?}",
        messages(&findings)
    );

    // A literal peer naming the reference is a config error.
    let error = build(&spec_yaml(
        "id: t\nkind: key_parity\nreference: a.json\npeers: './a.json'\nlevel: error\n",
    ))
    .err()
    .map(|e| e.to_string());
    assert!(
        error
            .as_deref()
            .is_some_and(|e| e.contains("other than the reference")),
        "{error:?}"
    );
}

#[test]
fn toml_nested_inline_tables_match_their_table_form() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: en.toml\npeers: '*.toml'\nlevel: error\n",
        &[
            ("en.toml", b"[x.a]\nb = 1\n"),
            ("fr.toml", b"x = { a = { b = 1 } }\n"),
        ],
    );
    assert!(findings.is_empty(), "{:?}", messages(&findings));
}

#[test]
fn strip_root_and_cldr_findings_keep_their_locations() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: en.yml\npeers: '*.yml'\nstrip_root: true\nplurals: cldr\nlevel: error\n",
        &[
            (
                "en.yml",
                b"en:\n  title: Title\n  items:\n    one: item\n    other: items\n",
            ),
            ("fr.yml", b"fr:\n  title: Titre\n  extra: x\n"),
        ],
    );
    let missing = findings
        .iter()
        .find(|v| v.message.contains("missing key `items`"))
        .unwrap_or_else(|| panic!("{:?}", messages(&findings)));
    let related = &missing.related_locations;
    assert_eq!(related.len(), 1, "{:?}", messages(&findings));
    let range = related[0]
        .range
        .clone()
        .expect("the reference key keeps its range");
    assert_eq!((range.start_line(), range.start_column()), (5, 5));
    let extra = findings
        .iter()
        .find(|v| v.message.contains("extra key `extra`"))
        .unwrap_or_else(|| panic!("{:?}", messages(&findings)));
    let range = extra.range.clone().expect("the peer key keeps its range");
    assert_eq!((range.start_line(), range.start_column()), (3, 3));
}

#[test]
fn only_whole_plural_objects_merge_and_merges_never_hide_keys() {
    let keys = |entries: &[&[&str]]| -> BTreeMap<Vec<Segment>, ByteSpan> {
        entries
            .iter()
            .enumerate()
            .map(|(i, parts)| {
                (
                    parts.iter().map(|p| Segment::Key((*p).into())).collect(),
                    ByteSpan::new(i, i + 1),
                )
            })
            .collect()
    };
    let render = |keys: BTreeMap<Vec<Segment>, ByteSpan>| {
        keys.keys().map(|key| render_key(key)).collect::<Vec<_>>()
    };
    // `other` plus a non-category sibling: not a plural group.
    let mixed = keys(&[&["item", "one"], &["item", "other"], &["item", "label"]]);
    assert_eq!(
        render(merged_alone(&mixed)),
        ["item.label", "item.one", "item.other"]
    );
    // A container child also disqualifies the parent.
    let nested = keys(&[&["item", "other"], &["item", "one", "x"]]);
    assert_eq!(render(merged_alone(&nested)), ["item.one.x", "item.other"]);
    // An i18next base that is already a key stays unmerged.
    let colliding = keys(&[&["msg"], &["msg_one"], &["msg_other"]]);
    assert_eq!(
        render(merged_alone(&colliding)),
        ["msg", "msg_one", "msg_other"]
    );
    // So does an empty base.
    let empty_base = keys(&[&["_one"], &["_other"]]);
    assert_eq!(render(merged_alone(&empty_base)), ["_one", "_other"]);
}

#[test]
fn a_reference_with_glob_metacharacters_matches_its_own_changed_edit() {
    // The reference sits outside the peers glob, so only the escaped
    // reference pattern can match its edit.
    let rule = rule(
        "id: t\nkind: key_parity\nreference: 'ref/[id].json'\npeers: 'locales/*.json'\nlevel: error\n",
    );
    let index = crate::test_support::index(&["ref/[id].json", "ref/i.json"]);
    let scope = rule.path_scope().expect("a path scope");
    assert!(scope.matches(Path::new("ref/[id].json"), &index));
    assert!(!scope.matches(Path::new("ref/i.json"), &index));
    assert!(scope.matches(Path::new("locales/fr.json"), &index));
    assert!(!scope.matches(Path::new("other/i.txt"), &index));
}

#[test]
fn a_literal_peer_is_evaluated_whenever_anything_changed() {
    // Git cannot report an edit to a gitignored `.env`, and `./x` and `x`
    // name the same peer, so a literal peer's scope is every path.
    let index = crate::test_support::index(&["src/main.rs"]);
    for (reference, peers) in [
        (".env.example", ".env"),
        ("locales/en.json", "./locales/fr.json"),
    ] {
        let rule = rule(&format!(
            "id: t\nkind: key_parity\nreference: {reference}\npeers: '{peers}'\nlevel: error\n"
        ));
        let scope = rule.path_scope().expect("a path scope");
        assert!(scope.matches(Path::new("src/main.rs"), &index), "{peers}");
        assert!(
            scope.matches(Path::new("locales/fr.json"), &index),
            "{peers}"
        );
    }
}

#[test]
fn tree_ignore_patterns_match_by_segment() {
    let keys = |pattern: &str| {
        let pattern = KeyPattern::parse(pattern, KeyModel::Tree).unwrap();
        [
            vec!["nav"],
            vec!["nav", "home"],
            vec!["nav", "a.b"],
            vec!["nav", ""],
            vec!["nav", "x", "y"],
            vec!["debug.flag"],
            vec!["navbar"],
        ]
        .into_iter()
        .filter(|key| {
            let key: Vec<Segment> = key.iter().map(|s| Segment::Key((*s).into())).collect();
            pattern.matches(&key)
        })
        .map(|key| key.join("/"))
        .collect::<Vec<_>>()
    };
    // `*` is one whole segment, dots and all; a trailing `**` needs at least
    // one segment below its prefix.
    assert_eq!(keys("nav.*"), ["nav/home", "nav/a.b", "nav/"]);
    assert_eq!(keys("nav.**"), ["nav/home", "nav/a.b", "nav/", "nav/x/y"]);
    assert_eq!(keys("nav*"), ["nav", "navbar"]);
    assert_eq!(keys("nav['a.b']"), ["nav/a.b"]);
    assert_eq!(keys("nav.['a.b']"), ["nav/a.b"]);
    assert_eq!(keys("nav['']"), ["nav/"]);
    assert_eq!(keys("['debug.*']"), ["debug.flag"]);
    assert_eq!(keys("**.y"), ["nav/x/y"]);
    assert_eq!(keys("**"), keys("**.**"));
    assert_eq!(keys("**").len(), 7);
}

#[test]
fn malformed_ignore_patterns_are_config_errors() {
    for pattern in [
        "nav.",
        ".nav",
        "nav..home",
        "nav['a",
        "nav['a'",
        "nav[x]",
        "['a']b",
    ] {
        let yaml = format!(
            "id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nignore_keys: [\"{pattern}\"]\nlevel: error\n"
        );
        let error = build(&spec_yaml(&yaml)).err().map(|e| e.to_string());
        assert!(
            error.as_deref().is_some_and(|e| e.contains("ignore_keys")),
            "{pattern}: {error:?}"
        );
    }
    // In a single-name format a pattern is a plain glob, never parsed as a path.
    assert!(
        build(&spec_yaml(
            "id: t\nkind: key_parity\nreference: en.properties\npeers: '*.properties'\nignore_keys: ['a..b[']\nlevel: error\n"
        ))
        .is_ok()
    );
}

#[test]
fn single_name_keys_display_as_written_and_stay_injective() {
    let key = |name: &str| vec![Segment::Key(name.into())];
    assert_eq!(display_key(KeyModel::Flat, &key("app.title")), "app.title");
    assert_eq!(
        display_key(KeyModel::Resx, &key("button1.Text")),
        "button1.Text"
    );
    assert_eq!(display_key(KeyModel::Flat, &key("")), "['']");
    assert_eq!(display_key(KeyModel::Flat, &key("[x")), "['[x']");
    assert_eq!(
        display_key(KeyModel::Tree, &key("app.title")),
        "['app.title']"
    );
    let names = ["", "[x", "['[x']", "['']", "a.b", "['a.b']", "x"];
    let shown: BTreeSet<String> = names
        .iter()
        .map(|name| display_key(KeyModel::Flat, &key(name)))
        .collect();
    assert_eq!(shown.len(), names.len(), "{shown:?}");
}

#[test]
fn a_root_plural_object_is_not_merged() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nplurals: cldr\nlevel: error\n",
        &[
            ("en.json", br#"{"one":"a","other":"b"}"#),
            ("fr.json", br#"{"one":"a"}"#),
        ],
    );
    assert_eq!(
        messages(&findings),
        ["fr.json: missing key `other` (present in en.json)"]
    );
}

#[test]
fn an_empty_peer_does_not_turn_off_strip_root() {
    let findings = run(
        "id: t\nkind: key_parity\nreference: en.yml\npeers: '*.yml'\nstrip_root: true\nlevel: error\n",
        &[
            ("en.yml", b"en:\n  title: T\n"),
            ("fr.yml", b"fr:\n  title: T\n"),
            ("de.yml", b"{}\n"),
        ],
    );
    assert_eq!(
        messages(&findings),
        ["de.yml: missing key `title` (present in en.yml)"]
    );
}

#[test]
fn allow_missing_peers_turns_an_absent_peer_into_a_note() {
    let note = |findings: &[Violation]| {
        findings.len() == 1 && matches!(findings[0].disposition, FindingDisposition::Note(_))
    };
    let literal = "id: t\nkind: key_parity\nreference: .env.example\npeers: .env\nlevel: error\n";
    let glob = "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n";
    let files: &[(&str, &[u8])] = &[(".env.example", b"A=1\n"), ("locales/en.json", b"{}")];
    for yaml in [literal, glob] {
        let strict = run(yaml, files);
        assert!(
            strict.len() == 1
                && matches!(
                    strict[0].disposition,
                    FindingDisposition::AnalysisFailure(_)
                ),
            "{:?}",
            messages(&strict)
        );
        let allowed = run(&format!("{yaml}allow_missing_peers: true\n"), files);
        assert!(note(&allowed), "{:?}", messages(&allowed));
    }
    // A present peer is still compared, and a missing reference still fails.
    let compared = run(
        &format!("{literal}allow_missing_peers: true\n"),
        &[(".env.example", b"A=1\nB=2\n"), (".env", b"A=1\n")],
    );
    assert_eq!(messages(&compared).len(), 1);
    assert!(messages(&compared)[0].contains("missing key `B`"));
    let no_reference = run(
        &format!("{literal}allow_missing_peers: true\n"),
        &[(".env", b"A=1\n")],
    );
    assert!(
        matches!(
            no_reference[0].disposition,
            FindingDisposition::AnalysisFailure(_)
        ),
        "{:?}",
        messages(&no_reference)
    );
}

#[test]
fn the_analyzer_and_rule_refuse_malformed_comparisons() {
    // A row without a key path is an analyzer error, never a skipped key.
    let mut row = key_rows(
        &Resource::literal(RelPath::parse("en.json").unwrap()),
        &BTreeMap::from([(vec![Segment::Key("a".into())], ByteSpan::new(0, 1))]),
    )
    .unwrap();
    row[0].pointer = None;
    let error = ContractAnalyzer::new(KeySetAnalyzer {
        reference: "en.json".into(),
        peer: "fr.json".into(),
        direction: Direction::Both,
    })
    .analyze("t", &[], &row, &Limits::default())
    .unwrap_err();
    assert!(error.to_string().contains("no key path"), "{error}");

    // A finding the rule cannot classify is an error, never dropped.
    assert_eq!(
        classify(&[
            Violation::new("missing\u{0}a"),
            Violation::new("extra\u{0}b")
        ]),
        Ok((
            BTreeSet::from(["a".to_string()]),
            BTreeSet::from(["b".to_string()])
        ))
    );
    assert!(classify(&[Violation::new("changed\u{0}a")]).is_err());
    assert!(classify(&[Violation::new("no separator")]).is_err());
}

/// Invariant 4: the same tree gives the same findings, in the same order,
/// whatever the index order and however many threads evaluate it at once
/// against the shared read and decode caches.
#[test]
fn findings_are_identical_across_index_orders_and_thread_counts() {
    let files: &[(&str, &[u8])] = &[
        (
            "locales/en.json",
            br#"{"nav":{"home":"H","about":"A"},"a.b":1,"z":2}"#,
        ),
        ("locales/fr.json", br#"{"nav":{"home":"H"},"extra":1}"#),
        ("locales/de.json", b"{}"),
        (
            "locales/es.json",
            br#"{"nav":{"home":"H","about":"A","x":1},"a.b":1,"z":2}"#,
        ),
        ("locales/it.json", b"{"),
        ("other/readme.md", b"# x\n"),
    ];
    let (tmp, base) = tempdir_with_files(files);
    let yaml = "id: t\nkind: key_parity\nreference: locales/en.json\npeers: 'locales/*.json'\nlevel: error\n";
    let evaluate = |entries: Vec<alint_core::FileEntry>, threads: usize| -> Vec<Vec<String>> {
        let index = alint_core::FileIndex::from_entries(entries);
        index.enable_relation_caches();
        let rule = rule(yaml);
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
                        findings
                            .iter()
                            .map(|v| summary(std::slice::from_ref(v)).remove(0))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        })
    };
    let entries = base.entries.clone();
    let expected = evaluate(entries.clone(), 1).remove(0);
    assert!(expected.len() >= 6, "{expected:?}");
    let mut orders = vec![entries.iter().rev().cloned().collect::<Vec<_>>()];
    for rotation in 1..entries.len() {
        let mut rotated = entries.clone();
        rotated.rotate_left(rotation);
        orders.push(rotated);
    }
    // A seeded Fisher-Yates shuffle for orders no rotation produces.
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    for _ in 0..8 {
        let mut shuffled = entries.clone();
        for i in (1..shuffled.len()).rev() {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1);
            let j = usize::try_from(state >> 33).unwrap() % (i + 1);
            shuffled.swap(i, j);
        }
        orders.push(shuffled);
    }
    for (n, order) in orders.into_iter().enumerate() {
        for threads in [1, 2, 8] {
            for run in evaluate(order.clone(), threads) {
                assert_eq!(run, expected, "order {n}, {threads} threads");
            }
        }
    }
}
