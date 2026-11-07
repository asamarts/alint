//! What `cross_file` reports for each way reading a side's values can go,
//! across the relations that read them (`equals`, the set relations,
//! `resolves`, `registered`), named and glob-matched files, and both sides.
//!
//! Each case lists, per finding, its disposition and a fragment of its path and
//! message. With `ALINT_READER_DUMP` set to a directory, every case also writes
//! its complete findings there, so two builds can be compared byte for byte.

use std::path::Path;

use alint_core::{FileEntry, FileIndex, FindingDisposition, Violation};

use crate::test_support::{ctx, spec_yaml};

/// A tree on disk plus its index. `ghosts` are index entries with no file
/// behind them, as when a file vanishes after the walk.
fn fixture(files: &[(&str, &[u8])], ghosts: &[&str]) -> (tempfile::TempDir, FileIndex) {
    let tmp = tempfile::Builder::new()
        .prefix("alint-reader-")
        .tempdir()
        .unwrap();
    let mut entries = Vec::new();
    for (rel, content) in files {
        let abs = tmp.path().join(rel);
        std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
        std::fs::write(&abs, content).unwrap();
        entries.push(entry(Path::new(rel), content.len()));
    }
    for rel in ghosts {
        entries.push(entry(Path::new(rel), 1));
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    (tmp, FileIndex::from_entries(entries))
}

fn entry(path: &Path, size: usize) -> FileEntry {
    FileEntry {
        path: path.into(),
        is_dir: false,
        size: size as u64,
    }
}

fn run(name: &str, rule_yaml: &str, root: &Path, index: &FileIndex) -> Vec<Violation> {
    let yaml = format!("id: t\nlevel: error\n{rule_yaml}");
    let rule = super::super::build(&spec_yaml(&yaml)).unwrap();
    let findings = rule.evaluate(&ctx(root, index)).unwrap();
    if let Some(dir) = std::env::var_os("ALINT_READER_DUMP") {
        std::fs::write(Path::new(&dir).join(name), format!("{findings:#?}\n")).unwrap();
    }
    findings
}

fn render(finding: &Violation) -> String {
    let kind = match finding.disposition {
        FindingDisposition::Violation => "violation".to_string(),
        FindingDisposition::Note(_) => "note".to_string(),
        FindingDisposition::AnalysisFailure(kind) => format!("failure:{kind:?}"),
    };
    let path = finding.path.as_ref().map_or(String::new(), |path| {
        path.to_string_lossy().replace('\\', "/")
    });
    format!("{kind} {path}: {}", finding.message)
}

/// Per finding, in order: its kind and a fragment of its rendering.
type Expected<'a> = &'a [(&'a str, &'a str)];

/// Each finding, in order, starts with its expected kind and contains the
/// expected fragment.
#[track_caller]
fn expect(findings: &[Violation], expected: &[(&str, &str)]) {
    let rendered: Vec<String> = findings.iter().map(render).collect();
    assert_eq!(
        rendered.len(),
        expected.len(),
        "finding count differs: {rendered:#?}"
    );
    for (line, (kind, fragment)) in rendered.iter().zip(expected) {
        assert!(
            line.starts_with(&format!("{kind} ")) && line.contains(fragment),
            "expected {kind} containing {fragment:?}, got {line:?}\nall: {rendered:#?}"
        );
    }
}

const EQUALS_JSON: &str = "kind: cross_file\nrelation: equals\n\
    source: { file: SRC, extract: { json: \"$.v\" } }\n\
    targets: [{ file: t.json, extract: { json: \"$.v\" } }]\n";

fn equals_from(source: &str) -> String {
    EQUALS_JSON.replace("SRC", source)
}

#[test]
fn a_named_source_that_cannot_be_read() {
    let (tmp, index) = fixture(&[("t.json", br#"{"v":"1"}"#)], &[]);
    std::fs::create_dir(tmp.path().join("adir")).unwrap();
    let root = tmp.path();
    let cases: [(&str, &str, (&str, &str)); 4] = [
        (
            "missing",
            "absent.json",
            ("violation", "absent.json: source file is missing: "),
        ),
        (
            "absolute",
            "/etc/hostname",
            ("failure:ResourceRead", "path escapes the repo root"),
        ),
        (
            "parent",
            "../x.json",
            ("failure:ResourceRead", "path escapes the repo root"),
        ),
        (
            "directory",
            "adir",
            (
                "failure:ResourceRead",
                "adir: could not read file: not a regular file",
            ),
        ),
    ];
    for (name, source, expected) in cases {
        let findings = run(
            &format!("named-source-{name}"),
            &equals_from(source),
            root,
            &index,
        );
        expect(&findings, &[expected]);
    }
}

#[cfg(unix)]
#[test]
fn a_named_source_that_escapes_through_a_symlink_or_is_too_large() {
    let (tmp, index) = fixture(&[("t.json", br#"{"v":"1"}"#)], &[]);
    std::os::unix::fs::symlink("/etc", tmp.path().join("link")).unwrap();
    let big = std::fs::File::create(tmp.path().join("big.json")).unwrap();
    big.set_len(alint_core::MAX_ANALYZE_BYTES + 1).unwrap();
    let root = tmp.path();
    let findings = run(
        "named-source-symlink",
        &equals_from("link/hostname"),
        root,
        &index,
    );
    expect(
        &findings,
        &[("failure:ResourceRead", "path escapes the repo root")],
    );
    let findings = run(
        "named-source-too-large",
        &equals_from("big.json"),
        root,
        &index,
    );
    expect(
        &findings,
        &[(
            "failure:ResourceRead",
            "big.json: file is too large to analyze",
        )],
    );
}

#[test]
fn a_named_source_that_decodes_badly() {
    let non_string = "s.json: source extract matched non-string value(s) (number: 1)";
    let cases: [(&str, &str, &[u8], Expected<'_>); 5] = [
        (
            "parse",
            "$.v",
            b"{not json",
            &[("failure:ResourceParse", "s.json: source extract failed: ")],
        ),
        (
            "non-strings-only",
            "$.v",
            br#"{"v":1}"#,
            &[("violation", non_string)],
        ),
        (
            // Non-strings next to only computed strings: the computed ones are
            // noted and skipped, and the source then has no literal value.
            "non-strings-and-computed",
            "$.v[*]",
            br#"{"v":[1,"${X}"]}"#,
            &[
                ("violation", non_string),
                ("note", "skipped non-literal source value \"${X}\""),
                ("violation", "canonical value not found"),
            ],
        ),
        (
            "computed-only",
            "$.v",
            br#"{"v":"${X}"}"#,
            &[
                ("note", "skipped non-literal source value \"${X}\""),
                ("violation", "canonical value not found"),
            ],
        ),
        (
            "several",
            "$.v[*]",
            br#"{"v":["1","2"]}"#,
            &[("violation", "source must resolve to exactly one value")],
        ),
    ];
    for (name, query, source, expected) in cases {
        let (tmp, index) = fixture(&[("s.json", source), ("t.json", br#"{"v":"1"}"#)], &[]);
        let rule = equals_from("s.json").replacen("\"$.v\"", &format!("{query:?}"), 1);
        let findings = run(&format!("named-source-{name}"), &rule, tmp.path(), &index);
        expect(&findings, expected);
    }
}

#[test]
fn the_value_limit_applies_to_both_sides() {
    let many = format!(
        "{{\"v\":[{}]}}",
        vec!["\"1\""; super::super::MAX_EXTRACTED_VALUES + 1].join(",")
    );
    let rule = "kind: cross_file\nrelation: subset\n\
        source: { file: s.json, extract: { json: \"$.v[*]\" } }\n\
        targets: [{ file: t.json, extract: { json: \"$.v[*]\" } }]\n";
    let limit = format!(
        "extract failed: extract selected more than the {}-value safety limit",
        super::super::MAX_EXTRACTED_VALUES
    );
    let (tmp, index) = fixture(
        &[("s.json", many.as_bytes()), ("t.json", br#"{"v":["1"]}"#)],
        &[],
    );
    let findings = run("limit-source", rule, tmp.path(), &index);
    expect(
        &findings,
        &[("failure:ResourceParse", &format!("s.json: source {limit}"))],
    );
    let (tmp, index) = fixture(
        &[("s.json", br#"{"v":["1"]}"#), ("t.json", many.as_bytes())],
        &[],
    );
    let findings = run("limit-target", rule, tmp.path(), &index);
    expect(
        &findings,
        &[("failure:ResourceParse", &format!("t.json: target {limit}"))],
    );
}

#[test]
fn targets_that_cannot_be_read_or_decoded() {
    let files: &[(&str, &[u8])] = &[
        ("s.json", br#"{"v":"1"}"#),
        ("t/bad.json", b"{oops"),
        ("t/num.json", br#"{"v":2}"#),
        ("t/ok.json", br#"{"v":"2"}"#),
        ("t/var.json", br#"{"v":"${X}"}"#),
    ];
    let (tmp, index) = fixture(files, &["t/gone.json"]);
    let glob = "kind: cross_file\nrelation: equals\n\
        source: { file: s.json, extract: { json: \"$.v\" } }\n\
        targets: { files: \"t/*.json\", extract: { json: \"$.v\" } }\n";
    let findings = run("glob-targets", glob, tmp.path(), &index);
    expect(
        &findings,
        &[
            (
                "failure:ResourceParse",
                "t/bad.json: target extract failed: ",
            ),
            ("violation", "t/gone.json: target file is missing"),
            (
                "violation",
                "t/num.json: target extract matched non-string value(s) (number: 1)",
            ),
            ("violation", "t/ok.json: "),
            (
                "note",
                "t/var.json: skipped non-literal target value \"${X}\"",
            ),
            ("violation", "t/var.json: no literal value to compare"),
        ],
    );
    let allowed = format!("{glob}allow_missing_target: true\n");
    let findings = run("glob-targets-allow-missing", &allowed, tmp.path(), &index);
    expect(
        &findings,
        &[
            (
                "failure:ResourceParse",
                "t/bad.json: target extract failed: ",
            ),
            (
                "violation",
                "t/num.json: target extract matched non-string value(s) (number: 1)",
            ),
            ("violation", "t/ok.json: "),
            (
                "note",
                "t/var.json: skipped non-literal target value \"${X}\"",
            ),
        ],
    );
    let list = "kind: cross_file\nrelation: equals\n\
        source: { file: s.json, extract: { json: \"$.v\" } }\n\
        targets:\n  - { file: t/absent.json, extract: { json: \"$.v\" } }\n  \
        - { file: /etc/hostname, extract: { json: \"$.v\" } }\n  \
        - { file: ./t/ok.json, extract: { json: \"$.v\" } }\n";
    let findings = run("list-targets", list, tmp.path(), &index);
    expect(
        &findings,
        &[
            ("violation", "t/absent.json: target file is missing"),
            ("failure:ResourceRead", "path escapes the repo root"),
            ("violation", "t/ok.json: "),
        ],
    );
}

#[test]
fn target_regexes_keep_the_syntax_filter_and_the_computed_skip() {
    let files: &[(&str, &[u8])] = &[
        ("VERSION", b"3.12\n"),
        (
            "Dockerfile",
            b"# FROM python:9.9\nFROM python:3.11\nFROM python:${PY}\n",
        ),
    ];
    let (tmp, index) = fixture(files, &[]);
    let rule = "kind: cross_file\nrelation: equals\n\
        source: { file: VERSION, extract: { lines: {} } }\n\
        targets: [{ file: Dockerfile, extract: { regex: 'FROM python:(\\S+)' } }]\n";
    let findings = run("target-regex", rule, tmp.path(), &index);
    expect(
        &findings,
        &[
            (
                "note",
                "Dockerfile: skipped non-literal target value \"${PY}\"",
            ),
            ("violation", "Dockerfile: "),
        ],
    );
    assert!(
        findings[1].message.contains("3.11"),
        "{:?}",
        findings[1].message
    );
}

#[test]
fn glob_sources_read_every_match() {
    let files: &[(&str, &[u8])] = &[
        ("doc/a.txt", b"x=a\n"),
        ("doc/b.txt", b"x=b\nx=${B}\n"),
        ("list.txt", b"a\nb\nc\n"),
    ];
    let (tmp, index) = fixture(files, &["doc/gone.txt"]);
    let rule = "kind: cross_file\nrelation: set_equals\n\
        source: { files: \"doc/*.txt\", extract: { regex: 'x=(.*)' } }\n\
        targets: [{ file: list.txt, extract: { lines: {} } }]\n";
    let findings = run("glob-source", rule, tmp.path(), &index);
    expect(
        &findings,
        &[
            (
                "note",
                "doc/b.txt: skipped non-literal source value \"${B}\"",
            ),
            (
                "failure:ResourceRead",
                "doc/gone.txt: file vanished after repository discovery",
            ),
            ("violation", "list.txt: "),
        ],
    );
    let none = rule.replace("doc/*.txt", "none/*.txt");
    let findings = run("glob-source-no-match", &none, tmp.path(), &index);
    expect(
        &findings,
        &[("violation", "`source.files` glob matched no files")],
    );
}

#[test]
fn resolves_and_registered_read_through_the_same_reader() {
    let files: &[(&str, &[u8])] = &[
        ("paths.json", br#"{"p":["a.txt","missing.txt","${X}"]}"#),
        ("a.txt", b""),
        ("members.json", b"{bad"),
    ];
    let (tmp, index) = fixture(files, &[]);
    let resolves = "kind: cross_file\nrelation: resolves\n\
        source: { file: paths.json, extract: { json: \"$.p[*]\" } }\n";
    let findings = run("resolves", resolves, tmp.path(), &index);
    expect(
        &findings,
        &[
            (
                "note",
                "paths.json: skipped non-literal source value \"${X}\"",
            ),
            (
                "violation",
                "declared path \"missing.txt\" does not resolve",
            ),
        ],
    );
    let missing = resolves.replace("paths.json", "absent.json");
    let findings = run("resolves-missing", &missing, tmp.path(), &index);
    expect(
        &findings,
        &[("violation", "absent.json: source file is missing: ")],
    );
    let registered = "kind: cross_file\nrelation: registered\n\
        source: { file: a.txt }\n\
        targets:\n  - { file: members.json, extract: { json: \"$.m[*]\" } }\n  \
        - { file: absent.json, extract: { json: \"$.m[*]\" } }\n";
    let findings = run("registered", registered, tmp.path(), &index);
    expect(
        &findings,
        &[
            (
                "failure:ResourceParse",
                "members.json: target extract failed: ",
            ),
            ("violation", "absent.json: target file is missing"),
        ],
    );
}

#[test]
fn whole_file_and_lines_keep_their_decoding() {
    let files: &[(&str, &[u8])] = &[
        ("s.txt", b"${NOT_SKIPPED}\n"),
        ("t.txt", b"${NOT_SKIPPED}\n"),
        ("l.txt", b"# comment\n  one  \n\ntwo\n"),
        ("m.txt", b"one\nthree\n"),
    ];
    let (tmp, index) = fixture(files, &[]);
    let whole = "kind: cross_file\nrelation: equals\n\
        source: { file: s.txt, extract: { whole_file: {} } }\n\
        targets: [{ file: t.txt, extract: { whole_file: {} } }]\n";
    expect(&run("whole-file", whole, tmp.path(), &index), &[]);
    let lines = "kind: cross_file\nrelation: subset\n\
        source: { file: l.txt, extract: { lines: { comment: \"#\" } } }\n\
        targets: [{ file: m.txt, extract: { lines: {} } }]\n";
    let findings = run("lines", lines, tmp.path(), &index);
    expect(&findings, &[("violation", "m.txt: ")]);
    assert!(
        findings[0].message.contains("two"),
        "{:?}",
        findings[0].message
    );
}

/// A glob-matched file whose name is not UTF-8 is read and compared like any
/// other.
#[cfg(target_os = "linux")]
#[test]
fn a_matched_file_whose_name_is_not_utf8() {
    use std::os::unix::ffi::OsStrExt;
    let (tmp, index) = fixture(&[("s.json", br#"{"v":"1"}"#)], &[]);
    let name = Path::new("t").join(std::ffi::OsStr::from_bytes(b"\xff.json"));
    std::fs::create_dir(tmp.path().join("t")).unwrap();
    std::fs::write(tmp.path().join(&name), br#"{"v":"2"}"#).unwrap();
    let mut entries: Vec<FileEntry> = index.entries.clone();
    entries.push(entry(&name, 9));
    let index = FileIndex::from_entries(entries);
    let rule = "kind: cross_file\nrelation: equals\n\
        source: { file: s.json, extract: { json: \"$.v\" } }\n\
        targets: { files: \"t/*\", extract: { json: \"$.v\" } }\n";
    let findings = run("non-utf8-target", rule, tmp.path(), &index);
    assert_eq!(findings.len(), 1, "{findings:#?}");
    assert_eq!(findings[0].path.as_deref(), Some(name.as_path()));
    assert!(
        findings[0].message.contains('2'),
        "{:?}",
        findings[0].message
    );
}

/// A file the config names is read and decoded once for every rule that names
/// it; a file a glob matches is read each time and never kept.
#[test]
fn named_files_share_the_run_caches_and_matches_are_not_kept() {
    let files: &[(&str, &[u8])] = &[
        ("s.json", br#"{"v":"1"}"#),
        ("t/a.json", br#"{"v":"1"}"#),
        ("t/b.json", br#"{"v":"1"}"#),
    ];
    let (tmp, index) = fixture(files, &[]);
    index.enable_relation_caches();
    let rule = "kind: cross_file\nrelation: equals\n\
        source: { file: s.json, extract: { json: \"$.v\" } }\n\
        targets: { files: \"t/*.json\", extract: { json: \"$.v\" } }\n";
    for _ in 0..2 {
        expect(&run("caches", rule, tmp.path(), &index), &[]);
    }
    let (reads, decodes) = (index.read_cache().unwrap(), index.decode_cache().unwrap());
    // One read of the source, then two per pass for the matches.
    assert_eq!(reads.reads(), 1 + 2 * 2);
    assert_eq!(
        decodes.decodes(),
        1,
        "only the named source is decoded into the cache"
    );
    let rendered = format!("{reads:?}");
    assert!(rendered.contains("entries: 1"), "{rendered}");
}

#[test]
fn every_relation_but_identical_uses_the_relation_layer() {
    let source = "source: { file: s.json, extract: { json: \"$.v\" } }\n";
    let target = "targets: [{ file: t.json, extract: { json: \"$.v[*]\" } }]\n";
    for (relation, uses) in [
        ("equals", true),
        ("subset", true),
        ("superset", true),
        ("set_equals", true),
        ("resolves", true),
        ("registered", true),
        ("identical", false),
    ] {
        let yaml = match relation {
            "resolves" => format!("kind: cross_file\nrelation: resolves\n{source}"),
            "registered" => format!("kind: cross_file\nrelation: registered\nsource: {{ file: a }}\n{target}"),
            "identical" => "kind: cross_file\nrelation: identical\nsource: { file: a }\ntargets: [{ file: b }]\n".to_string(),
            _ => format!("kind: cross_file\nrelation: {relation}\n{source}{target}"),
        };
        let rule =
            super::super::build(&spec_yaml(&format!("id: t\nlevel: error\n{yaml}"))).unwrap();
        assert_eq!(rule.uses_relations(), uses, "{relation}");
    }
}

#[test]
fn repeated_mismatches_key_apart_and_an_empty_targets_glob_is_reported() {
    let files: &[(&str, &[u8])] = &[
        ("s.json", br#"{"v":"1"}"#),
        ("t.json", br#"{"v":["2","2","3"]}"#),
    ];
    let (tmp, index) = fixture(files, &[]);
    let yaml = "kind: cross_file\nrelation: equals\n\
        source: { file: s.json, extract: { json: \"$.v\" } }\n\
        targets: [{ file: t.json, extract: { json: \"$.v[*]\" } }]\n";
    let findings = run("repeated-mismatches", yaml, tmp.path(), &index);
    // The value fixer finds each occurrence of a repeated value by its count.
    let keys: Vec<&str> = findings
        .iter()
        .map(|finding| finding.fix_key.as_deref().unwrap())
        .collect();
    assert_eq!(
        keys,
        [
            "cross_file_equals\u{0}0",
            "cross_file_equals\u{0}1",
            "cross_file_equals\u{0}0"
        ]
    );

    let glob = "kind: cross_file\nrelation: equals\n\
        source: { file: s.json, extract: { json: \"$.v\" } }\n\
        targets: { files: \"none/*.json\", extract: { json: \"$.v\" } }\n";
    let findings = run("empty-targets-glob", glob, tmp.path(), &index);
    expect(
        &findings,
        &[("violation", "s.json: targets glob matched no files")],
    );
    let allowed = format!("{glob}allow_missing_target: true\n");
    let findings = run("empty-targets-glob-allowed", &allowed, tmp.path(), &index);
    expect(&findings, &[]);
}
