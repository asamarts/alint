//! `key_parity`'s key paths are held once, by each file's decode, and what
//! the decodes and the comparison hold is taken from the run's bytes.

use super::*;
use crate::test_support::{ctx, spec_yaml, tempdir_with_files};
use alint_core::{FindingDisposition, Violation};

fn rule(yaml: &str) -> KeyParityRule {
    build_rule(&spec_yaml(yaml)).expect("rule builds")
}

fn messages(violations: &[Violation]) -> Vec<String> {
    violations.iter().map(|v| v.message.to_string()).collect()
}

/// `{"a":{"a":...{"k0":0,"k1":0,...}}}`, `depth` objects deep with `width`
/// keys at the bottom: #569's shape, W keys whose paths are D segments long.
fn deep_and_wide(depth: usize, width: usize) -> String {
    let keys: Vec<String> = (0..width).map(|key| format!("\"k{key}\":0")).collect();
    format!(
        "{}{{{}{}",
        "{\"a\":".repeat(depth),
        keys.join(","),
        "}".repeat(depth + 1)
    )
}

/// A file's keys borrow their paths from its decode, which holds each once,
/// with `strip_root` too, and a comparison's rows hold each key's identity,
/// not its path. The run's bytes take the decode's key paths as they are
/// built and what the file keeps beside them.
#[test]
fn a_files_keys_borrow_their_paths_from_its_decode() {
    let (tmp, index) =
        tempdir_with_files(&[("en.json", br#"{"en":{"nav":{"home":1},"title":2}}"#)]);
    let rule = rule("id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n");
    let (reads, decodes) = (ReadCache::new(), DecodeCache::new());
    let resource = Resource::literal(RelPath::parse("en.json").unwrap());
    let mut findings = Vec::new();
    let file = rule
        .read_file(
            &ctx(tmp.path(), &index),
            &reads,
            &decodes,
            &Limits::default(),
            &resource,
            KeyFormat::Json,
            true,
            &mut findings,
        )
        .unwrap();
    let held: Vec<&[Segment]> = file
        .decode
        .rows
        .iter()
        .filter_map(|row| row.pointer.as_ref().map(Pointer::segments))
        .collect();
    let pointers: usize = file
        .decode
        .rows
        .iter()
        .filter_map(|row| row.pointer.as_ref().map(Pointer::heap_bytes))
        .sum();
    assert_eq!(decodes.bytes_taken(), pointers + file.held_bytes());
    for (skip, expected) in [(0, ["en.nav.home", "en.title"]), (1, ["nav.home", "title"])] {
        let keys = base_keys(&file, skip == 1);
        assert_eq!(
            keys.keys().map(|key| render_key(key)).collect::<Vec<_>>(),
            expected
        );
        for key in keys.keys() {
            let Cow::Borrowed(path) = key else {
                panic!("{key:?} is a copy");
            };
            assert!(
                held.iter()
                    .any(|segments| segments[skip..].as_ptr_range() == path.as_ptr_range()),
                "{key:?} is not the decode's path"
            );
        }
        let rows = key_rows(&resource, &keys).unwrap();
        for (row, key) in rows.iter().zip(keys.keys()) {
            assert_eq!(
                row.pointer.as_ref().map(Pointer::segments),
                Some(&[Segment::Key(render_key(key).into())][..])
            );
        }
    }
}

/// Every file's key paths take the run's bytes as its decode builds them,
/// with what the comparison keeps: a reference and a peer of #569's shape,
/// scaled down, fit a budget that a second peer passes, or that leaves out
/// what the comparison keeps of the reference, and the run then fails with
/// the limit rather than holding more, or comparing in part.
#[test]
fn the_run_byte_budget_bounds_every_files_key_paths_together() {
    let text = deep_and_wide(30, 300);
    let rule = rule("id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n");
    // One file's key paths, as its decode builds them.
    let alone: usize = EntryRowsAdapter::new(alint_core::Format::Json)
        .unwrap()
        .extract(
            &Resource::literal(RelPath::parse("en.json").unwrap()),
            text.as_bytes(),
            &Limits::default(),
        )
        .unwrap()
        .rows
        .iter()
        .filter_map(|row| row.pointer.as_ref().map(Pointer::heap_bytes))
        .sum();

    let two = [("en.json", text.as_bytes()), ("fr.json", text.as_bytes())];
    let (tmp, index) = tempdir_with_files(&two);
    index.enable_relation_caches();
    assert_eq!(
        messages(&rule.evaluate(&ctx(tmp.path(), &index)).unwrap()),
        [""; 0]
    );
    let taken = index.decode_cache().unwrap().bytes_taken();
    assert!(
        taken > 2 * alone,
        "the run took {taken} bytes for two files of {alone} bytes of key paths"
    );
    // What the two files' reads take alone: the tables the comparison keeps
    // of the reference for its peers take the rest, and the run fails
    // without them.
    let reads = DecodeCache::new();
    rule.read_file(
        &ctx(tmp.path(), &index),
        &ReadCache::new(),
        &reads,
        &Limits::default(),
        &Resource::literal(RelPath::parse("en.json").unwrap()),
        KeyFormat::Json,
        true,
        &mut Vec::new(),
    )
    .unwrap();
    let read = reads.bytes_taken();
    assert!(read > alone && 2 * read < taken, "{read} of {taken}");
    let (tmp, index) = tempdir_with_files(&two);
    index.enable_relation_caches_under(
        &Limits::default()
            .with(alint_core::relation::LimitKind::BytesPerRun, 2 * read)
            .unwrap(),
    );
    let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
    assert_eq!(findings.len(), 1, "{:?}", messages(&findings));
    assert!(
        findings[0]
            .message
            .starts_with("en.json: relation limit `bytes_per_run`")
            && findings[0].message.contains("not compared"),
        "{:?}",
        messages(&findings)
    );

    let three = [
        ("en.json", text.as_bytes()),
        ("de.json", text.as_bytes()),
        ("fr.json", text.as_bytes()),
    ];
    let (tmp, index) = tempdir_with_files(&three);
    index.enable_relation_caches_under(
        &Limits::default()
            .with(alint_core::relation::LimitKind::BytesPerRun, taken)
            .unwrap(),
    );
    let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding.message.contains("relation limit `bytes_per_run`")),
        "{:?}",
        messages(&findings)
    );
    assert!(
        findings
            .iter()
            .all(|finding| matches!(finding.disposition, FindingDisposition::AnalysisFailure(_))),
        "{:?}",
        messages(&findings)
    );
    // The default budget holds all three.
    let (tmp, index) = tempdir_with_files(&three);
    index.enable_relation_caches();
    assert_eq!(
        messages(&rule.evaluate(&ctx(tmp.path(), &index)).unwrap()),
        [""; 0]
    );
}
