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

/// #596: 1,000 empty peer files against a 1,000-key reference would otherwise
/// emit 1,000,000 findings. Under the default `findings_per_rule` cap (10,000),
/// the rule emits 10,000 findings plus one non-baselinable analysis failure
/// naming the cap and suppressed count.
#[test]
fn empty_peers_are_bounded_by_findings_per_rule() {
    use std::fmt::Write as _;
    let mut reference = String::from("{");
    for i in 0..1_000 {
        if i > 0 {
            reference.push(',');
        }
        write!(reference, "\"k{i:04}\":\"v\"").unwrap();
    }
    reference.push('}');
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(1_001);
    files.push(("en.json".to_string(), reference.into_bytes()));
    for i in 0..1_000 {
        files.push((format!("p{i:04}.json"), b"{}".to_vec()));
    }
    let refs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    let (tmp, index) = tempdir_with_files(&refs);
    index.enable_relation_caches();
    let rule = rule("id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n");
    let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
    let cap = LimitKind::FindingsPerRule.default_value();
    let normal: Vec<&Violation> = findings
        .iter()
        .filter(|v| matches!(v.disposition, FindingDisposition::Violation))
        .collect();
    let failures: Vec<&Violation> = findings
        .iter()
        .filter(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_)))
        .collect();
    assert_eq!(normal.len(), cap, "emits exactly the default cap");
    assert_eq!(failures.len(), 1, "emits one truncation analysis failure");
    assert!(
        failures[0].message.contains(&format!(
            "relation limit `findings_per_rule` ({cap}) reached"
        )) && failures[0].message.contains("suppressed 990000 more"),
        "truncation message: {}",
        failures[0].message
    );
}

/// Below or at `findings_per_rule`, findings and baseline keys stay untouched;
/// one finding past a configured cap emits the cap plus one analysis failure.
#[test]
fn configured_findings_per_rule_preserves_exact_boundary_and_triggers_one_past() {
    let files: &[(&str, &[u8])] = &[
        ("en.json", br#"{"a":1,"b":2,"c":3}"#),
        ("fr.json", b"{}"),
        ("de.json", b"{}"),
    ];
    let rule = rule("id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n");
    let (tmp, default_index) = tempdir_with_files(files);
    default_index.enable_relation_caches();
    let baseline_findings = rule.evaluate(&ctx(tmp.path(), &default_index)).unwrap();
    assert_eq!(baseline_findings.len(), 6);

    // At the exact cap (6): identical findings and baseline keys, no failure.
    let (tmp, exact_index) = tempdir_with_files(files);
    exact_index.enable_relation_caches_under(
        &Limits::default()
            .with(LimitKind::FindingsPerRule, 6)
            .unwrap(),
    );
    let exact_findings = rule.evaluate(&ctx(tmp.path(), &exact_index)).unwrap();
    assert_eq!(exact_findings.len(), 6);
    assert_eq!(
        exact_findings
            .iter()
            .map(|v| v.baseline_key.clone())
            .collect::<Vec<_>>(),
        baseline_findings
            .iter()
            .map(|v| v.baseline_key.clone())
            .collect::<Vec<_>>()
    );
    assert!(
        exact_findings
            .iter()
            .all(|v| matches!(v.disposition, FindingDisposition::Violation))
    );

    // One below (5): 5 findings + 1 analysis failure saying 1 suppressed.
    let (tmp, tight_index) = tempdir_with_files(files);
    tight_index.enable_relation_caches_under(
        &Limits::default()
            .with(LimitKind::FindingsPerRule, 5)
            .unwrap(),
    );
    let tight_findings = rule.evaluate(&ctx(tmp.path(), &tight_index)).unwrap();
    let normal_count = tight_findings
        .iter()
        .filter(|v| matches!(v.disposition, FindingDisposition::Violation))
        .count();
    let failure_msgs: Vec<&str> = tight_findings
        .iter()
        .filter(|v| matches!(v.disposition, FindingDisposition::AnalysisFailure(_)))
        .map(|v| v.message.as_ref())
        .collect();
    assert_eq!(normal_count, 5);
    assert_eq!(failure_msgs.len(), 1, "{failure_msgs:?}");
    assert!(
        failure_msgs[0].contains("relation limit `findings_per_rule` (5) reached")
            && failure_msgs[0].contains("suppressed 1 more"),
        "{failure_msgs:?}"
    );
}

/// #569: under `Limits::default()`, a deep, wide JSON file fails at
/// `key_path_bytes_per_decode` (32 MiB) rather than holding ~300 MiB of
/// key-path pointers under the 1 GiB `bytes_per_run` cap.
#[test]
fn deep_wide_document_fails_at_default_key_path_decode_cap() {
    let text = deep_and_wide(120, 20_000);
    let files: &[(&str, &[u8])] = &[("en.json", text.as_bytes()), ("fr.json", b"{}")];
    let (tmp, index) = tempdir_with_files(files);
    index.enable_relation_caches();
    let rule = rule("id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n");
    let findings = rule.evaluate(&ctx(tmp.path(), &index)).unwrap();
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
        findings[0]
            .message
            .contains("relation limit `key_path_bytes_per_decode`"),
        "{:?}",
        messages(&findings)
    );
}
