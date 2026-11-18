//! The key-parity shape as a pure contract analyzer (the R6a pilot),
//! checked against the rule.

use super::*;
use crate::test_support::{ctx, spec_yaml, tempdir_with_files};
use alint_core::Format;
use alint_core::plugin_api::{Analyzer, AnalyzerError, AnalyzerInput, Finding};
use alint_core::relation::{Adapter, ContractAnalyzer, EntryRowsAdapter, Limits, Row, RowState};

/// The key-parity shape as a pure contract analyzer: the first resource is
/// the reference, every other resource is a peer, and pointer segments are
/// the keys. This is the R6a pilot, not a replacement for the rule.
struct ParityPilot {
    reference: String,
}

impl Analyzer for ParityPilot {
    fn id(&self) -> &'static str {
        "key-parity-pilot"
    }

    fn analyze(&self, input: AnalyzerInput<'_>) -> Result<Vec<Finding>, AnalyzerError> {
        let keys = |path: &str| -> BTreeSet<String> {
            input
                .rows
                .iter()
                .filter(|row| row.resource.path == path)
                .filter_map(|row| {
                    row.pointer
                        .as_ref()
                        .map(|parts| render_contract_pointer(parts).unwrap())
                })
                .collect()
        };
        let reference = keys(&self.reference);
        let mut findings = Vec::new();
        for path in input
            .rows
            .iter()
            .map(|row| row.resource.path.as_str())
            .collect::<BTreeSet<_>>()
        {
            if path == self.reference {
                continue;
            }
            let peer = keys(path);
            for key in reference.difference(&peer) {
                findings.push(Finding {
                    path: Some(path.into()),
                    message: format!(
                        "{path}: missing key `{key}` (present in {})",
                        self.reference
                    ),
                    line: None,
                    column: None,
                    related: Vec::new(),
                    baseline_key: None,
                    sensitive: false,
                    analysis_failure: None,
                    failure_kind: None,
                    not_fixable: false,
                });
            }
            for key in peer.difference(&reference) {
                findings.push(Finding {
                    path: Some(path.into()),
                    message: format!("{path}: extra key `{key}` (absent from {})", self.reference),
                    line: None,
                    column: None,
                    related: Vec::new(),
                    baseline_key: None,
                    sensitive: false,
                    analysis_failure: None,
                    failure_kind: None,
                    not_fixable: false,
                });
            }
        }
        Ok(findings)
    }
}

/// Render a contract key path exactly as [`render_key`] renders the internal
/// one, so analyzer findings and rule-side keys compare as equal strings.
fn render_contract_pointer(pointer: &[PointerSegment]) -> Result<String, AnalyzerError> {
    let mut segments = Vec::with_capacity(pointer.len());
    for part in pointer {
        segments.push(match part {
            PointerSegment::Key(key) => Segment::Key(key.as_str().into()),
            PointerSegment::Index(index) => {
                Segment::Index(usize::try_from(*index).map_err(|_| {
                    AnalyzerError::InvalidInput(format!("key index {index} does not fit usize"))
                })?)
            }
        });
    }
    Ok(render_key(&segments))
}

fn key_row(path: &str, key: &str) -> Row {
    Row::new(
        Resource::literal(RelPath::parse(path).unwrap()),
        Some(Pointer::root().key(key)),
        0,
        RowState::Key,
    )
}

fn parsed_rows(path: &str, text: &str) -> Vec<Row> {
    EntryRowsAdapter::new(Format::Json)
        .unwrap()
        .extract(
            &Resource::literal(RelPath::parse(path).unwrap()),
            text.as_bytes(),
            &Limits::default(),
        )
        .unwrap()
        .rows
}

#[test]
fn pilot_matches_the_real_rule_on_parsed_key_rows() {
    // Top-level dotted keys need bracket notation: the oracle must render
    // them exactly as the rule does, or this comparison fails.
    let (tmp, index) = tempdir_with_files(&[
        ("en.json", br#"{"a":1,"b":2,"x.y":3}"#),
        ("fr.json", br#"{"a":1,"c":3,"x.z":4}"#),
    ]);
    index.enable_relation_caches();
    let rule = build(&spec_yaml(
        "id: t\nkind: key_parity\nreference: en.json\npeers: '*.json'\nlevel: error\n",
    ))
    .unwrap();
    let expected: Vec<String> = rule
        .evaluate(&ctx(tmp.path(), &index))
        .unwrap()
        .iter()
        .map(|finding| finding.message.to_string())
        .collect();
    assert!(
        expected.iter().any(|m| m.contains("missing key `['x.y']`")),
        "{expected:?}"
    );
    let mut rows = parsed_rows("en.json", r#"{"a":1,"b":2,"x.y":3}"#);
    rows.extend(parsed_rows("fr.json", r#"{"a":1,"c":3,"x.z":4}"#));
    let actual: Vec<String> = ContractAnalyzer::new(ParityPilot {
        reference: "en.json".into(),
    })
    .analyze("key-parity-pilot", &[], &rows, &Limits::default())
    .unwrap()
    .iter()
    .map(|finding| finding.message.to_string())
    .collect();
    let mut expected = expected;
    let mut actual = actual;
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
}

#[test]
fn key_parity_shape_runs_through_contract_analyzer() {
    let rows = [
        key_row("en.json", "a"),
        key_row("en.json", "b"),
        key_row("fr.json", "a"),
        key_row("fr.json", "c"),
    ];
    let findings = ContractAnalyzer::new(ParityPilot {
        reference: "en.json".into(),
    })
    .analyze("key-parity-pilot", &[], &rows, &Limits::default())
    .unwrap();
    let messages: Vec<String> = findings.iter().map(|f| f.message.to_string()).collect();
    assert!(messages.iter().any(|m| m.contains("missing key `b`")));
    assert!(messages.iter().any(|m| m.contains("extra key `c`")));
    assert_eq!(findings.len(), 2);
}
