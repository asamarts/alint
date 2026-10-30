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
    let keys = merge_i18next_groups(&merge_plural_groups(&keys));
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
    let msgs = messages(&properties);
    for expected in [
        "missing key `['app.title']`",
        "missing key `['app.name']`",
        "extra key `['menu.item']`",
    ] {
        assert!(
            msgs.iter().any(|m| m.contains(expected)),
            "{expected}: {msgs:?}"
        );
    }
    assert_eq!(msgs.len(), 3, "{msgs:?}");

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
    let reference_keys = transform(&reference_file, Plurals::Exact, false, &[]);
    let peer_keys = transform(&peer_file, Plurals::Exact, false, &[]);
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
    .analyze("t", &foreign, &Limits::default())
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

    // A literal peer naming the reference is a config error, as before.
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
        render(merge_plural_groups(&mixed)),
        ["item.label", "item.one", "item.other"]
    );
    // A container child also disqualifies the parent.
    let nested = keys(&[&["item", "other"], &["item", "one", "x"]]);
    assert_eq!(
        render(merge_plural_groups(&nested)),
        ["item.one.x", "item.other"]
    );
    // An i18next base that is already a key stays unmerged.
    let colliding = keys(&[&["msg"], &["msg_one"], &["msg_other"]]);
    assert_eq!(
        render(merge_i18next_groups(&colliding)),
        ["msg", "msg_one", "msg_other"]
    );
    // So does an empty base.
    let empty_base = keys(&[&["_one"], &["_other"]]);
    assert_eq!(
        render(merge_i18next_groups(&empty_base)),
        ["_one", "_other"]
    );
}

#[test]
fn a_reference_with_glob_metacharacters_matches_its_own_changed_edit() {
    let rule = rule(
        "id: t\nkind: key_parity\nreference: 'locales/[id].json'\npeers: 'locales/*.json'\nlevel: error\n",
    );
    let index = crate::test_support::index(&["locales/[id].json"]);
    let scope = rule.path_scope().expect("a path scope");
    assert!(scope.matches(Path::new("locales/[id].json"), &index));
    assert!(!scope.matches(Path::new("other/i.txt"), &index));
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
