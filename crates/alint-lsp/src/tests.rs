use super::config::{build_session, config_for_document};
use super::diagnostics::{
    TextSource, byte_offset_to_position, group_findings, related_information, severity_of,
    utf16_column, violation_range,
};
use super::render::{kind_description, rule_docs_url};
use super::*;
use alint_core::{FindingDisposition, FixEdit, Level, RelatedLocation, SourceRange};
use std::borrow::Cow;
use tower_lsp::lsp_types::{DocumentChangeOperation, DocumentChanges, NumberOrString, ResourceOp};

fn test_backend(client: Client) -> Backend {
    Backend::new(client, LspOptions::default())
}

/// Wire a built session into the state as the governing config of `uri`.
fn install_session(st: &mut State, root: &Path, uri: &Url, session: Session) {
    st.folders = vec![root.to_path_buf()];
    st.doc_config
        .insert(uri.clone(), session.config_path.clone());
    st.sessions
        .insert(session.config_path.clone(), Arc::new(session));
}

fn violation(line: Option<usize>, column: Option<usize>) -> Violation {
    Violation {
        path: None,
        message: Cow::Borrowed("boom"),
        line,
        column,
        range: None,
        related_locations: Vec::new(),
        related_locations_omitted: 0,
        disposition: FindingDisposition::default(),
        baseline_key: None,
        fix_key: None,
        sensitive: false,
        is_fixable: false,
        not_fixable: false,
        proposed_edits: Vec::new(),
        path_sensitive: false,
    }
}

fn finding(policy_url: Option<&str>) -> Finding {
    Finding {
        range: Range::new(Position::new(3, 6), Position::new(3, 7)),
        severity: DiagnosticSeverity::ERROR,
        rule_id: "my-rule".to_string(),
        message: "boom".to_string(),
        // 1-indexed location matching the 0-indexed range above.
        line: Some(4),
        column: Some(7),
        policy_url: policy_url.map(ToString::to_string),
        fixable: false,
        sensitive: false,
        per_file: true,
        description: None,
        docs_url: None,
        related_information: Vec::new(),
    }
}

#[test]
fn severity_maps_levels_and_drops_off() {
    assert_eq!(severity_of(Level::Error), Some(DiagnosticSeverity::ERROR));
    assert_eq!(
        severity_of(Level::Warning),
        Some(DiagnosticSeverity::WARNING)
    );
    assert_eq!(
        severity_of(Level::Info),
        Some(DiagnosticSeverity::INFORMATION)
    );
    assert_eq!(severity_of(Level::Off), None);
}

#[test]
fn violation_range_converts_one_indexed_to_zero_indexed() {
    let r = violation_range(&violation(Some(4), Some(7)), None);
    assert_eq!(r.start, Position::new(3, 6));
    assert_eq!(r.end, Position::new(3, 7));
}

#[test]
fn violation_range_without_line_anchors_at_file_start() {
    let r = violation_range(&violation(None, None), None);
    assert_eq!(r.start, Position::new(0, 0));
    assert_eq!(r.end, Position::new(0, 1));
}

#[test]
fn finding_to_diagnostic_carries_rule_and_policy_link() {
    let d = finding_to_diagnostic(&finding(Some("https://example.com/policy")), true);
    assert_eq!(d.code, Some(NumberOrString::String("my-rule".to_string())));
    assert_eq!(d.source.as_deref(), Some("alint"));
    assert_eq!(d.message, "boom");
    assert_eq!(
        d.code_description.unwrap().href.as_str(),
        "https://example.com/policy"
    );
}

#[test]
fn finding_to_diagnostic_omits_code_description_for_non_url_policy() {
    let d = finding_to_diagnostic(&finding(Some("not a url")), true);
    assert!(d.code_description.is_none());
}

#[test]
fn range_contains_uses_half_open_end_and_empty_ranges_match_nothing() {
    let r = Range::new(Position::new(3, 6), Position::new(3, 7));
    assert!(range_contains(r, Position::new(3, 6)));
    assert!(!range_contains(r, Position::new(3, 7)));
    assert!(!range_contains(r, Position::new(3, 8)));
    assert!(!range_contains(r, Position::new(2, 6)));
    let empty = Range::new(Position::new(3, 6), Position::new(3, 6));
    assert!(!range_contains(empty, Position::new(3, 6)));
}
#[test]
fn ranged_locations_use_utf16_and_related_information_respects_capability() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let primary = root.join("primary.txt");
    let related = root.join("related.txt");
    let text = "a😀z\n";
    let mut overlays = std::collections::HashMap::new();
    overlays.insert(related.clone(), text.to_string());
    let mut texts = TextSource::with_overlays(overlays);
    let violation = Violation::new("boom")
        .with_path(Path::new("primary.txt"))
        .with_range(SourceRange::new(1, 1, 1, 2).unwrap())
        .with_related_location(
            RelatedLocation::new("related.txt", "secondary")
                .with_range(SourceRange::new(1, 2, 1, 4).unwrap()),
        );
    let primary_range = violation_range(&violation, Some(text));
    let information = related_information(root, &primary, primary_range, &violation, &mut texts);
    assert_eq!(information.len(), 1);
    assert_eq!(
        information[0].location.uri,
        Url::from_file_path(related).unwrap()
    );
    assert_eq!(information[0].location.range.start, Position::new(0, 1));
    assert_eq!(information[0].location.range.end, Position::new(0, 4));
    assert_eq!(information[0].message, "secondary");

    let mut finding = finding(None);
    finding.related_information = information;
    assert_eq!(
        finding_to_diagnostic(&finding, true)
            .related_information
            .unwrap()
            .len(),
        1
    );
    assert!(
        finding_to_diagnostic(&finding, false)
            .related_information
            .is_none()
    );
}

#[test]
fn bom_marked_disk_and_overlay_locations_use_logical_text_coordinates() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let primary = root.join("primary.txt");
    let text = "a😀x\r\nb";
    let mut utf8_bytes = b"\xef\xbb\xbf".to_vec();
    utf8_bytes.extend_from_slice(text.as_bytes());
    let mut little_endian_bytes = b"\xff\xfe".to_vec();
    let mut big_endian_bytes = b"\xfe\xff".to_vec();
    for unit in text.encode_utf16() {
        little_endian_bytes.extend_from_slice(&unit.to_le_bytes());
        big_endian_bytes.extend_from_slice(&unit.to_be_bytes());
    }

    for (name, bytes) in [
        ("utf8.txt", utf8_bytes),
        ("utf16le.txt", little_endian_bytes),
        ("utf16be.txt", big_endian_bytes),
    ] {
        std::fs::write(root.join(name), bytes).unwrap();
        let violation = Violation::new("boom").with_related_location(
            RelatedLocation::new(name, "secondary")
                .with_range(SourceRange::new(1, 2, 1, 3).unwrap()),
        );
        let mut texts = TextSource::default();
        let information = related_information(
            root,
            &primary,
            Range::new(Position::new(0, 0), Position::new(0, 1)),
            &violation,
            &mut texts,
        );
        assert_eq!(information.len(), 1, "{name}");
        assert_eq!(
            information[0].location.range,
            Range::new(Position::new(0, 1), Position::new(0, 3)),
            "{name}"
        );
    }

    let overlay = root.join("overlay.txt");
    let violation = Violation::new("boom").with_related_location(
        RelatedLocation::new("overlay.txt", "secondary")
            .with_range(SourceRange::new(1, 2, 1, 3).unwrap()),
    );
    let mut texts = TextSource::with_overlays(std::collections::HashMap::from([(
        overlay,
        format!("\u{feff}{text}"),
    )]));
    let information = related_information(
        root,
        &primary,
        Range::new(Position::new(0, 0), Position::new(0, 1)),
        &violation,
        &mut texts,
    );
    assert_eq!(
        information[0].location.range,
        Range::new(Position::new(0, 1), Position::new(0, 3)),
        "UTF-8 BOM in an editor overlay"
    );
}

#[tokio::test]
async fn initialize_records_related_information_capability() {
    use tower_lsp::lsp_types::{
        ClientCapabilities, PublishDiagnosticsClientCapabilities, TextDocumentClientCapabilities,
    };

    for supported in [false, true] {
        let (service, _socket) = LspService::new(test_backend);
        let backend = service.inner();
        let params = InitializeParams {
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                        related_information: Some(supported),
                        ..PublishDiagnosticsClientCapabilities::default()
                    }),
                    ..TextDocumentClientCapabilities::default()
                }),
                ..ClientCapabilities::default()
            },
            ..InitializeParams::default()
        };

        backend.initialize(params).await.unwrap();
        assert_eq!(
            backend.state.lock().related_information_supported,
            supported
        );
    }
}

#[test]
fn render_finding_includes_rule_message_and_policy() {
    let md = render_finding(&finding(Some("https://example.com/p")));
    assert!(md.contains("my-rule"), "{md}");
    assert!(md.contains("(error)"), "{md}");
    assert!(md.contains("boom"), "{md}");
    assert!(md.contains("https://example.com/p"), "{md}");
}

#[test]
fn render_finding_omits_policy_link_when_absent() {
    let md = render_finding(&finding(None));
    assert!(!md.contains("Policy"), "{md}");
}

#[test]
fn build_session_returns_none_when_no_config() {
    let dir = tempfile::tempdir().unwrap();
    assert!(build_session(dir.path()).unwrap().is_none());
}

#[test]
fn build_session_preserves_expect_matches_findings() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".alint.yml");
    std::fs::write(
        &config_path,
        r"version: 1
rules:
  - id: required-doc
    kind: file_content_forbidden
    paths: required.md
    pattern: stale
    expect_matches: true
    level: error
",
    )
    .unwrap();

    let session = build_session(dir.path())
        .expect("build_session succeeds")
        .expect("config present");
    let report = session
        .engine
        .run(&session.root, &session.index)
        .expect("scope assertion evaluates");
    assert_eq!(report.results.len(), 1, "{report:?}");
    assert_eq!(report.results[0].violations.len(), 1, "{report:?}");
    assert!(
        report.results[0].violations[0]
            .message
            .contains("matched none"),
        "{report:?}"
    );

    let by_path = group_findings(
        &session.group_ctx(),
        &report.results,
        &mut TextSource::default(),
    );
    assert_eq!(by_path[&config_path].len(), 1, "{by_path:?}");
    assert!(!by_path[&config_path][0].fixable);
}

#[test]
fn pattern_hints_are_warnings_on_the_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".alint.yml");
    std::fs::write(dir.path().join("a.txt"), "test in first line\n").unwrap();
    std::fs::write(
        &config_path,
        r"version: 1
rules:
  - id: anchored
    kind: file_content_forbidden
    paths: '*.txt'
    pattern: '^test'
    level: warning
  - id: line-anchored
    kind: file_content_forbidden
    paths: '*.txt'
    pattern: '(?m)^test'
    level: warning
",
    )
    .unwrap();

    let check = super::config::check_workspace(
        &[dir.path().to_path_buf()],
        &[],
        None,
        &alint_rules::builtin_registry(),
    );
    let [(_, run)] = &check.runs[..] else {
        panic!("one config run: {:?}", check.runs.len());
    };
    let run = run.as_ref().expect("the config runs");
    let hints: Vec<&Finding> = run.by_path[&config_path]
        .iter()
        .filter(|finding| finding.severity == DiagnosticSeverity::WARNING)
        .collect();
    assert_eq!(hints.len(), 1, "{hints:?}");
    assert_eq!(hints[0].rule_id, "anchored");
    assert_eq!(hints[0].line, Some(3), "points at the rule's `id:` line");
    assert!(hints[0].message.contains("`(?m)^`"), "{}", hints[0].message);
    assert!(
        !hints[0].per_file,
        "kept across keystrokes, like cross-file findings"
    );
}

#[test]
fn lsp_diagnostic_redacts_dotenv_evidence() {
    const SECRET: &str = "LSP_TOKEN_VALUE_DO_NOT_REPORT";
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".alint.yml");
    let env_path = dir.path().join(".env");
    std::fs::write(&env_path, format!("TOKEN={SECRET}\n")).unwrap();
    std::fs::write(
        &config_path,
        r"version: 1
rules:
  - id: token
    kind: dotenv_path_equals
    paths: .env
    path: $.TOKEN
    equals: expected
    level: error
    fix: { set_value: {} }
",
    )
    .unwrap();

    let session = build_session(dir.path())
        .expect("build_session succeeds")
        .expect("config present");
    let report = session
        .engine
        .run(&session.root, &session.index)
        .expect("rule evaluates");
    let by_path = group_findings(
        &session.group_ctx(),
        &report.results,
        &mut TextSource::default(),
    );
    let diagnostics = by_path.get(&env_path).expect("finding is anchored to .env");
    assert_eq!(diagnostics.len(), 1);
    assert!(!diagnostics[0].message.contains(SECRET));
    assert!(diagnostics[0].message.contains("<redacted:"));
    assert!(diagnostics[0].fixable);
    assert!(diagnostics[0].sensitive);
}

#[test]
fn lsp_incremental_diagnostic_applies_explicit_sensitivity() {
    const SECRET: &str = "LSP_EXPLICIT_VALUE_DO_NOT_REPORT";
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join(".alint.yml");
    let file_path = dir.path().join("secret.txt");
    std::fs::write(&file_path, SECRET).unwrap();
    std::fs::write(
        &config_path,
        format!(
            r"version: 1
rules:
  - id: secret
    kind: file_content_forbidden
    paths: secret.txt
    pattern: {SECRET}
    message: evidence={SECRET}
    sensitive: true
    level: error
"
        ),
    )
    .unwrap();

    let session = build_session(dir.path())
        .expect("build_session succeeds")
        .expect("config present");
    let results = session
        .engine
        .run_for_file(
            &session.root,
            &session.index,
            Path::new("secret.txt"),
            SECRET.as_bytes(),
        )
        .expect("incremental rule evaluates");
    let by_path = group_findings(&session.group_ctx(), &results, &mut TextSource::default());
    let diagnostics = by_path
        .get(&file_path)
        .expect("finding is anchored to secret.txt");

    assert_eq!(diagnostics.len(), 1);
    assert!(!diagnostics[0].message.contains(SECRET));
    assert!(diagnostics[0].message.contains("<redacted:"));
    assert!(diagnostics[0].sensitive);
}

#[test]
fn build_session_rejects_invalid_deeply_nested_rule() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        r#"version: 1
rules:
  - id: outer
    kind: for_each_dir
    select: packages/*
    require:
      - kind: for_each_dir
        select: "{path}/*"
        require:
          - kind: file_exists
            paths: "{path}/README.md"
            expect_matches: true
    level: error
"#,
    )
    .unwrap();

    let error = build_session(dir.path()).expect_err("invalid descendant must fail loading");
    assert!(error.contains("expect_matches"), "{error}");
}

#[test]
fn group_findings_anchors_pathless_to_config() {
    use alint_core::{Engine, RuleResult};
    let engine = Engine::new(vec![], alint_core::RuleRegistry::new());
    let root = repo_root();
    let config = root.join(".alint.yml");
    let results = vec![RuleResult {
        rule_id: std::sync::Arc::from("missing-license"),
        level: Level::Error,
        policy_url: None,
        violations: vec![Violation::new("LICENSE is missing")],
        notes: Vec::new(),
        is_fixable: false,
        analysis_failures: Vec::new(),
    }];
    let kinds = HashMap::new();
    let ctx = GroupCtx {
        root: &root,
        engine: &engine,
        config_path: &config,
        kinds: &kinds,
    };
    let by_path = group_findings(&ctx, &results, &mut TextSource::default());
    // The path-less violation is anchored to the config file.
    assert!(
        by_path.contains_key(&config),
        "anchored to config: {by_path:?}"
    );
    let finding = &by_path[&config][0];
    assert!(
        !finding.per_file,
        "unknown/cross-file rule tagged per_file=false"
    );
    assert_eq!(finding.message, "LICENSE is missing");
    // A path-less finding has no source location.
    assert_eq!(finding.line, None);
    assert_eq!(finding.column, None);
}

#[test]
fn group_findings_anchors_protected_paths_to_config_without_locations() {
    use alint_core::{Engine, RuleResult};

    let engine = Engine::new(vec![], alint_core::RuleRegistry::new());
    let root = repo_root();
    let config = root.join(".alint.yml");
    let mut violation = Violation::new("redacted")
        .with_path(std::path::PathBuf::from("secret-selector.rs"))
        .with_location(12, 3);
    violation.path_sensitive = true;
    let results = vec![RuleResult::new(
        "protected-path".into(),
        Level::Error,
        None,
        vec![violation],
        false,
    )];
    let kinds = HashMap::new();
    let ctx = GroupCtx {
        root: &root,
        engine: &engine,
        config_path: &config,
        kinds: &kinds,
    };

    let by_path = group_findings(&ctx, &results, &mut TextSource::default());

    assert_eq!(by_path.len(), 1);
    assert!(by_path.contains_key(&config), "{by_path:?}");
    let finding = &by_path[&config][0];
    assert_eq!(finding.line, None);
    assert_eq!(finding.column, None);
    assert_eq!(finding.related_information, Vec::new());
}

#[test]
fn group_findings_escalates_and_anchors_analysis_failures() {
    use alint_core::{AnalysisFailure, AnalysisFailureKind, Engine, RuleResult};

    let engine = Engine::new(vec![], alint_core::RuleRegistry::new());
    let root = repo_root();
    let config = root.join(".alint.yml");
    let results = vec![RuleResult {
        rule_id: std::sync::Arc::from("broken-warning"),
        level: Level::Warning,
        policy_url: None,
        violations: Vec::new(),
        notes: Vec::new(),
        is_fixable: true,
        analysis_failures: vec![AnalysisFailure::new(
            AnalysisFailureKind::Evaluation,
            "sentinel failure",
        )],
    }];
    let kinds = HashMap::new();
    let ctx = GroupCtx {
        root: &root,
        engine: &engine,
        config_path: &config,
        kinds: &kinds,
    };

    let by_path = group_findings(&ctx, &results, &mut TextSource::default());
    let finding = &by_path[&config][0];
    assert_eq!(finding.severity, DiagnosticSeverity::ERROR);
    assert_eq!(
        finding.message,
        "analysis failure (evaluation): sentinel failure",
    );
    assert_eq!(finding.line, None);
    assert_eq!(finding.column, None);
    assert!(!finding.fixable);
    assert!(!finding.per_file);
}

#[test]
fn group_findings_threads_violation_line_and_column() {
    use alint_core::{Engine, RuleResult};
    let engine = Engine::new(vec![], alint_core::RuleRegistry::new());
    let root = repo_root();
    let config = root.join(".alint.yml");
    let results = vec![RuleResult {
        rule_id: std::sync::Arc::from("line-rule"),
        level: Level::Warning,
        policy_url: None,
        violations: vec![
            Violation::new("bad line")
                .with_path(std::path::PathBuf::from("src/x.rs"))
                .with_location(12, 3),
        ],
        notes: Vec::new(),
        is_fixable: true,
        analysis_failures: Vec::new(),
    }];
    let kinds = HashMap::new();
    let ctx = GroupCtx {
        root: &root,
        engine: &engine,
        config_path: &config,
        kinds: &kinds,
    };
    let by_path = group_findings(&ctx, &results, &mut TextSource::default());
    let finding = &by_path[&root.join("src/x.rs")][0];
    // The reported location is carried onto the finding (so a
    // range-scoped code-action fixer sees it), and drives the
    // 1-indexed -> 0-indexed LSP range.
    assert_eq!(finding.line, Some(12));
    assert_eq!(finding.column, Some(3));
    assert_eq!(finding.range.start.line, 11);
}

#[test]
fn ranges_overlap_uses_half_open_ends_and_cursor_selections() {
    let a = Range::new(Position::new(2, 0), Position::new(2, 10));
    assert!(ranges_overlap(
        a,
        Range::new(Position::new(2, 5), Position::new(2, 6))
    ));
    assert!(ranges_overlap(
        a,
        Range::new(Position::new(0, 0), Position::new(5, 0))
    ));
    assert!(!ranges_overlap(
        a,
        Range::new(Position::new(2, 10), Position::new(2, 11))
    ));
    assert!(!ranges_overlap(
        a,
        Range::new(Position::new(3, 0), Position::new(3, 1))
    ));
    assert!(ranges_overlap(
        a,
        Range::new(Position::new(2, 5), Position::new(2, 5))
    ));
    assert!(!ranges_overlap(
        a,
        Range::new(Position::new(2, 10), Position::new(2, 10))
    ));
}

/// An absolute root valid on the test's OS — `Url::from_file_path`
/// requires absolute, and `/repo` is NOT absolute on Windows.
fn repo_root() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\repo")
    } else {
        PathBuf::from("/repo")
    }
}

#[test]
fn set_content_maps_to_full_document_text_edit() {
    let root = repo_root();
    let edit = FixEdit::SetContent {
        path: PathBuf::from("a.txt"),
        content: b"fixed\n".to_vec(),
    };
    let ws = fix_edit_to_workspace_edit(&edit, &root).unwrap();
    let changes = ws.changes.expect("content edit uses the changes map");
    let uri = Url::from_file_path(root.join("a.txt")).unwrap();
    let edits = &changes[&uri];
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "fixed\n");
    assert_eq!(edits[0].range.start, Position::new(0, 0));
    assert!(ws.document_changes.is_none());
}

#[test]
fn delete_maps_to_a_resource_operation() {
    let edit = FixEdit::DeleteFile {
        path: PathBuf::from("debug.log"),
    };
    let ws = fix_edit_to_workspace_edit(&edit, &repo_root()).unwrap();
    assert!(ws.changes.is_none());
    let Some(DocumentChanges::Operations(ops)) = ws.document_changes else {
        panic!("delete must use resource operations");
    };
    assert_eq!(ops.len(), 1);
    assert!(matches!(
        ops[0],
        DocumentChangeOperation::Op(ResourceOp::Delete(_))
    ));
}

#[test]
fn create_maps_to_create_op_plus_insert_edit() {
    let edit = FixEdit::CreateFile {
        path: PathBuf::from("LICENSE"),
        content: b"Apache-2.0\n".to_vec(),
    };
    let ws = fix_edit_to_workspace_edit(&edit, &repo_root()).unwrap();
    let Some(DocumentChanges::Operations(ops)) = ws.document_changes else {
        panic!("create must use resource operations");
    };
    assert_eq!(ops.len(), 2);
    assert!(matches!(
        ops[0],
        DocumentChangeOperation::Op(ResourceOp::Create(_))
    ));
    assert!(matches!(ops[1], DocumentChangeOperation::Edit(_)));
}

#[test]
fn rename_maps_to_rename_op() {
    let edit = FixEdit::RenameFile {
        from: PathBuf::from("FooBar.rs"),
        to: PathBuf::from("foo_bar.rs"),
    };
    let ws = fix_edit_to_workspace_edit(&edit, &repo_root()).unwrap();
    let Some(DocumentChanges::Operations(ops)) = ws.document_changes else {
        panic!("rename must use resource operations");
    };
    assert!(matches!(
        ops[0],
        DocumentChangeOperation::Op(ResourceOp::Rename(_))
    ));
}

#[test]
fn set_content_with_non_utf8_yields_no_edit() {
    let edit = FixEdit::SetContent {
        path: PathBuf::from("a.bin"),
        content: vec![0xff, 0xfe],
    };
    assert!(fix_edit_to_workspace_edit(&edit, &repo_root()).is_none());
}

#[test]
fn byte_offset_to_position_counts_utf16_code_units() {
    // ASCII, single line.
    assert_eq!(byte_offset_to_position("hello", 0), Position::new(0, 0));
    assert_eq!(byte_offset_to_position("hello", 3), Position::new(0, 3));
    assert_eq!(byte_offset_to_position("hello", 5), Position::new(0, 5)); // clamps at end
    // Multi-line: the offset just after '\n' is line 1, character 0.
    let two = "ab\ncd";
    assert_eq!(byte_offset_to_position(two, 2), Position::new(0, 2)); // before '\n'
    assert_eq!(byte_offset_to_position(two, 3), Position::new(1, 0)); // 'c'
    assert_eq!(byte_offset_to_position(two, 5), Position::new(1, 2)); // end of "cd"
    // R-UTF16: U+1F600 is 4 UTF-8 bytes but TWO UTF-16 code units, so a byte- or
    // `char`-based column would misplace an edit after it.
    let emoji = "a\u{1F600}b";
    assert_eq!(byte_offset_to_position(emoji, 1), Position::new(0, 1)); // before the emoji
    assert_eq!(
        byte_offset_to_position(emoji, 5),
        Position::new(0, 3),
        "the emoji is two UTF-16 units, so 'b' is at character 3"
    );
    // CRLF: '\r' counts as an ordinary character; a position after the CRLF is
    // line 1, character 0 (so a match on the next line lands correctly).
    let crlf = "ab\r\ncd";
    assert_eq!(byte_offset_to_position(crlf, 2), Position::new(0, 2)); // end of line-0 text
    assert_eq!(byte_offset_to_position(crlf, 4), Position::new(1, 0)); // 'c' after \r\n
    // Lone CR is also a physical line ending, matching SourceRange.
    let cr = "ab\r\u{1F600}x";
    assert_eq!(byte_offset_to_position(cr, 3), Position::new(1, 0));
    assert_eq!(byte_offset_to_position(cr, 7), Position::new(1, 2));
    // Empty text: only the origin is reachable.
    assert_eq!(byte_offset_to_position("", 0), Position::new(0, 0));
}

#[test]
fn located_edits_map_to_utf16_text_edits() {
    // A non-BMP char BEFORE the edit range: a byte- or char-based column would be
    // wrong. The located edit replaces the 4-byte "TODO" with "DONE".
    let root = repo_root();
    let text = "x\u{1F600} TODO\n";
    let todo = text.find("TODO").unwrap();
    let edit = CollectedEdit {
        edit: FixEdit::ReplaceRange {
            path: PathBuf::from("a.txt"),
            range: todo..todo + 4,
            content: b"DONE".to_vec(),
        },
        applicability: alint_core::Applicability::Unsafe,
        verify: alint_core::EditVerifier::None,
        isolation_group: None,
    };
    let ws = located_edits_to_workspace_edit(&[edit], text, Path::new("a.txt"), &root).unwrap();
    let changes = ws.changes.expect("a located edit uses the changes map");
    let uri = Url::from_file_path(root.join("a.txt")).unwrap();
    let edits = &changes[&uri];
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].new_text, "DONE");
    // 'x'=0, emoji=cols 1-2 (two units), ' '=3, so "TODO" starts at character 4.
    assert_eq!(
        edits[0].range.start,
        Position::new(0, 4),
        "the emoji counts as two UTF-16 units"
    );
    assert_eq!(edits[0].range.end, Position::new(0, 8));
    assert!(ws.document_changes.is_none());
}

#[test]
fn located_edit_spanning_a_newline_maps_to_a_multi_line_range() {
    // A `(?s)`-style pattern can match across a line break; the resulting
    // `TextEdit` range must cross lines. Replace "foo\nbar" (bytes 0..7) with "X".
    let root = repo_root();
    let text = "foo\nbar\n";
    let edit = CollectedEdit {
        edit: FixEdit::ReplaceRange {
            path: PathBuf::from("a.txt"),
            range: 0..7,
            content: b"X".to_vec(),
        },
        applicability: alint_core::Applicability::Unsafe,
        verify: alint_core::EditVerifier::None,
        isolation_group: None,
    };
    let ws = located_edits_to_workspace_edit(&[edit], text, Path::new("a.txt"), &root).unwrap();
    let uri = Url::from_file_path(root.join("a.txt")).unwrap();
    let te = &ws.changes.unwrap()[&uri][0];
    assert_eq!(te.range.start, Position::new(0, 0));
    assert_eq!(
        te.range.end,
        Position::new(1, 3),
        "the range crosses into line 1, character 3 (past 'bar')"
    );
    assert_eq!(te.new_text, "X");
}

#[test]
fn located_edits_with_no_range_edits_yield_none() {
    // A located fixer that collected nothing offers NO action (not an empty one).
    assert!(
        located_edits_to_workspace_edit(&[], "abc", Path::new("a.txt"), &repo_root()).is_none()
    );
}

#[tokio::test]
async fn code_action_offers_the_located_replace_fix_end_to_end() {
    // Drive the real async `code_action` for a located fixer: a genuine
    // `file_content_forbidden` + `replace` session, an open buffer with the
    // forbidden pattern, and a diagnostic over it. The response must carry a
    // quick-fix whose `WorkspaceEdit` replaces the match with a UTF-16 `TextEdit`.
    // Gates the routing (`collects_located_edits` branch) + state handling that
    // the mapper unit tests don't reach. `code_action` touches only `self.state`
    // (never `self.client`), so it runs without a live LSP socket.
    use tower_lsp::lsp_types::{
        CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
    };

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
             paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n    \
             fix: { replace: { replacement: \"DONE\" } }\n",
    )
    .unwrap();
    let session = build_session(&root)
        .expect("build_session ok")
        .expect("config present");

    let (service, _socket) = LspService::new(test_backend);
    let backend = service.inner();

    let uri = Url::from_file_path(root.join("a.txt")).unwrap();
    let finding = Finding {
        range: Range::new(Position::new(0, 2), Position::new(0, 3)),
        severity: DiagnosticSeverity::ERROR,
        rule_id: "no-todo".to_string(),
        message: "forbidden".to_string(),
        line: Some(1),
        column: Some(3),
        policy_url: None,
        fixable: true,
        sensitive: false,
        per_file: true,
        description: None,
        docs_url: None,
        related_information: Vec::new(),
    };
    {
        let mut st = backend.state.lock();
        install_session(&mut st, &root, &uri, session);
        st.open.insert(uri.clone());
        st.documents.insert(uri.clone(), "x TODO\n".to_string());
        st.diagnostics.insert(uri.clone(), vec![finding]);
    }

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range::new(Position::new(0, 2), Position::new(0, 3)),
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let resp = backend
        .code_action(params)
        .await
        .expect("code_action ok")
        .expect("an action is offered");
    assert_eq!(
        resp.len(),
        1,
        "exactly one quick-fix for the replace violation"
    );
    let CodeActionOrCommand::CodeAction(action) = &resp[0] else {
        panic!("expected a CodeAction, not a Command");
    };
    let ws = action
        .edit
        .as_ref()
        .expect("the action carries a workspace edit");
    let changes = ws
        .changes
        .as_ref()
        .expect("a located edit uses the changes map");
    let tes = &changes[&uri];
    assert_eq!(tes.len(), 1, "one TextEdit for the single TODO");
    assert_eq!(tes[0].new_text, "DONE");
    // "x TODO": 'x'=0, ' '=1, "TODO" occupies characters 2..6.
    assert_eq!(tes[0].range.start, Position::new(0, 2));
    assert_eq!(tes[0].range.end, Position::new(0, 6));
}

#[tokio::test]
async fn code_action_omits_sensitive_replacement_bytes() {
    use tower_lsp::lsp_types::{
        CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
    };

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: token\n    kind: dotenv_path_equals\n    \
         paths: .env\n    path: $.TOKEN\n    equals: expected\n    level: error\n    \
         fix: { set_value: {} }\n",
    )
    .unwrap();
    let session = build_session(&root)
        .expect("build_session ok")
        .expect("config present");
    let (service, _socket) = LspService::new(test_backend);
    let backend = service.inner();
    let uri = Url::from_file_path(root.join(".env")).unwrap();
    let finding = Finding {
        range: Range::new(Position::new(0, 6), Position::new(0, 12)),
        severity: DiagnosticSeverity::ERROR,
        rule_id: "token".to_string(),
        message: "sensitive evidence <redacted:000000000000>".to_string(),
        line: Some(1),
        column: Some(7),
        policy_url: None,
        fixable: true,
        sensitive: true,
        per_file: true,
        description: None,
        docs_url: None,
        related_information: Vec::new(),
    };
    {
        let mut state = backend.state.lock();
        install_session(&mut state, &root, &uri, session);
        state.open.insert(uri.clone());
        state
            .documents
            .insert(uri.clone(), "TOKEN=secret\n".to_string());
        state.diagnostics.insert(uri.clone(), vec![finding]);
    }
    let response = backend
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 6), Position::new(0, 12)),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code_action succeeds");
    assert!(
        response.is_none(),
        "sensitive findings must not expose replacement bytes: {response:?}"
    );
}

#[tokio::test]
async fn code_action_omits_a_sibling_fix_for_a_sensitive_input_file() {
    use tower_lsp::lsp_types::{
        CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
    };

    const SECRET: &str = "LSP_PASSING_SECRET_DO_NOT_REPORT";
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    let env_text = format!("TOKEN={SECRET}");
    std::fs::write(root.join(".env"), &env_text).unwrap();
    std::fs::write(root.join("target.properties"), format!("TOKEN={SECRET}\n")).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        r#"version: 1
rules:
  - id: token
    kind: cross_file
    source: { file: .env, extract: { dotenv: "$.TOKEN" } }
    targets:
      - { file: target.properties, extract: { properties: "$.TOKEN" } }
    relation: equals
    level: error
  - id: append
    kind: file_content_matches
    paths: .env
    pattern: NEVER
    level: error
    fix: { file_append: { content: "\n" } }
"#,
    )
    .unwrap();
    let session = build_session(&root)
        .expect("build_session ok")
        .expect("config present");
    let (service, _socket) = LspService::new(test_backend);
    let backend = service.inner();
    let uri = Url::from_file_path(root.join(".env")).unwrap();
    let finding = Finding {
        range: Range::new(Position::new(0, 0), Position::new(0, 1)),
        severity: DiagnosticSeverity::ERROR,
        rule_id: "append".to_string(),
        message: "required pattern is missing".to_string(),
        line: Some(1),
        column: Some(1),
        policy_url: None,
        fixable: true,
        sensitive: false,
        per_file: true,
        description: None,
        docs_url: None,
        related_information: Vec::new(),
    };
    {
        let mut state = backend.state.lock();
        install_session(&mut state, &root, &uri, session);
        state.open.insert(uri.clone());
        state.documents.insert(uri.clone(), env_text);
        state.diagnostics.insert(uri.clone(), vec![finding]);
    }

    let response = backend
        .code_action(CodeActionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            range: Range::new(Position::new(0, 0), Position::new(0, 1)),
            context: CodeActionContext {
                diagnostics: vec![],
                only: None,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("code_action succeeds");
    assert!(
        response.is_none(),
        "a sibling fix must not serialize bytes from a sensitive input: {response:?}"
    );
}

#[tokio::test]
async fn code_action_offers_a_structured_set_value_fix() {
    // Follow-up 4: the structured located fixers (`set_value`/`remove_value`/
    // `replace` on the `*_path_*` kinds) reach the editor through the SAME
    // `collects_located_edits` branch as `replace`. Drive `code_action` for a
    // real `json_path_equals` + `set_value` session: the response must carry a
    // quick-fix whose `TextEdit` rewrites just the value span.
    use tower_lsp::lsp_types::{
        CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: port\n    kind: json_path_equals\n    \
             paths: \"*.json\"\n    path: \"$.port\"\n    equals: 9090\n    level: error\n    \
             fix: { set_value: {} }\n",
    )
    .unwrap();
    let session = build_session(&root)
        .expect("build_session ok")
        .expect("config present");
    let (service, _socket) = LspService::new(test_backend);
    let backend = service.inner();
    let uri = Url::from_file_path(root.join("app.json")).unwrap();
    let finding = Finding {
        range: Range::new(Position::new(0, 9), Position::new(0, 13)),
        severity: DiagnosticSeverity::ERROR,
        rule_id: "port".to_string(),
        message: "value at path does not equal expected".to_string(),
        line: Some(1),
        column: Some(10),
        policy_url: None,
        fixable: true,
        sensitive: false,
        per_file: true,
        description: None,
        docs_url: None,
        related_information: Vec::new(),
    };
    {
        let mut st = backend.state.lock();
        install_session(&mut st, &root, &uri, session);
        st.open.insert(uri.clone());
        st.documents
            .insert(uri.clone(), "{\"port\": 8080}".to_string());
        st.diagnostics.insert(uri.clone(), vec![finding]);
    }
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range::new(Position::new(0, 9), Position::new(0, 13)),
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let resp = backend
        .code_action(params)
        .await
        .expect("code_action ok")
        .expect("an action is offered");
    let CodeActionOrCommand::CodeAction(action) = &resp[0] else {
        panic!("expected a CodeAction");
    };
    let ws = action.edit.as_ref().expect("workspace edit");
    let tes = &ws.changes.as_ref().expect("changes map")[&uri];
    assert_eq!(tes.len(), 1, "one TextEdit for the value span");
    assert_eq!(tes[0].new_text, "9090");
    // `{"port": 8080}`: `8080` occupies UTF-16 columns 9..13.
    assert_eq!(tes[0].range.start, Position::new(0, 9));
    assert_eq!(tes[0].range.end, Position::new(0, 13));
}

#[tokio::test]
async fn code_action_withholds_a_fix_the_pipeline_would_demote() {
    // Audit HIGH: the LSP must not offer a quick-fix the engine refuses. It
    // now routes located edits through `apply_file_edits` (the same verify /
    // overlap / tier pipeline `alint fix` uses) and offers only survivors. A
    // `remove_value` batch that can only partially remove is demoted
    // all-or-nothing (the `Absent` verify fails), so `alint fix` writes
    // nothing -- the code action must offer nothing, never a partial (here:
    // secret-leaking) removal.
    use tower_lsp::lsp_types::{
        CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-secret\n    kind: hcl_path_absent\n    \
             paths: \"*.tf\"\n    path: \"$..secret\"\n    level: error\n    \
             fix: { remove_value: { applicability: safe } }\n",
    )
    .unwrap();
    let session = build_session(&root)
        .expect("build_session ok")
        .expect("config present");
    let (service, _socket) = LspService::new(test_backend);
    let backend = service.inner();
    let uri = Url::from_file_path(root.join("main.tf")).unwrap();
    // A top-level secret + two block secrets: the block members can't be
    // removed, so the whole batch demotes.
    let content = "secret = \"top\"\nitem {\n  secret = \"a\"\n}\nitem {\n  secret = \"b\"\n}\n";
    let finding = Finding {
        range: Range::new(Position::new(0, 0), Position::new(0, 6)),
        severity: DiagnosticSeverity::ERROR,
        rule_id: "no-secret".to_string(),
        message: "secret present".to_string(),
        line: Some(1),
        column: Some(1),
        policy_url: None,
        fixable: true,
        sensitive: false,
        per_file: true,
        description: None,
        docs_url: None,
        related_information: Vec::new(),
    };
    {
        let mut st = backend.state.lock();
        install_session(&mut st, &root, &uri, session);
        st.open.insert(uri.clone());
        st.documents.insert(uri.clone(), content.to_string());
        st.diagnostics.insert(uri.clone(), vec![finding]);
    }
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range::new(Position::new(0, 0), Position::new(0, 6)),
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let resp = backend.code_action(params).await.expect("code_action ok");
    assert!(
        resp.is_none(),
        "a fix the pipeline demotes must not be offered; got: {resp:?}"
    );
}

#[tokio::test]
async fn code_action_labels_an_unsafe_fix_and_does_not_prefer_it() {
    // An Unsafe fix (here `replace`, default Unsafe) IS offered as a quick-fix
    // (human-in-the-loop), but its title is labeled `(unsafe)` and it is NOT
    // marked preferred -- so clicking it is a visible, deliberate opt-in (the
    // LSP analogue of `--unsafe-fixes`), never an editor auto-apply.
    use tower_lsp::lsp_types::{
        CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
             paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n    \
             fix: { replace: { replacement: \"DONE\" } }\n",
    )
    .unwrap();
    let session = build_session(&root)
        .expect("build_session ok")
        .expect("config present");
    let (service, _socket) = LspService::new(test_backend);
    let backend = service.inner();
    let uri = Url::from_file_path(root.join("a.txt")).unwrap();
    let finding = Finding {
        range: Range::new(Position::new(0, 2), Position::new(0, 6)),
        severity: DiagnosticSeverity::ERROR,
        rule_id: "no-todo".to_string(),
        message: "forbidden".to_string(),
        line: Some(1),
        column: Some(3),
        policy_url: None,
        fixable: true,
        sensitive: false,
        per_file: true,
        description: None,
        docs_url: None,
        related_information: Vec::new(),
    };
    {
        let mut st = backend.state.lock();
        install_session(&mut st, &root, &uri, session);
        st.open.insert(uri.clone());
        st.documents.insert(uri.clone(), "x TODO\n".to_string());
        st.diagnostics.insert(uri.clone(), vec![finding]);
    }
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        range: Range::new(Position::new(0, 2), Position::new(0, 6)),
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let resp = backend
        .code_action(params)
        .await
        .expect("code_action ok")
        .expect("an action is offered");
    let CodeActionOrCommand::CodeAction(action) = &resp[0] else {
        panic!("expected a CodeAction");
    };
    assert_eq!(action.title, "alint: fix `no-todo` (unsafe)");
    assert_ne!(
        action.is_preferred,
        Some(true),
        "an Unsafe fix must not be auto-preferred"
    );
}

#[test]
fn located_lsp_path_maps_a_real_replace_fixer_to_utf16_text_edits() {
    // End-to-end for the located `code_action` path: a REAL `file_content_forbidden`
    // + `replace` rule, its `ReplaceFixer::collect_edits`, mapped to UTF-16
    // `TextEdit`s -- the exact sequence `code_action` runs for a located fixer.
    // A non-BMP char precedes the matches so the UTF-16 column mapping is
    // load-bearing, and there are TWO occurrences so the per-match multi-edit
    // behavior (not just the diagnostic's first match) is exercised.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
             paths: \"*.txt\"\n    pattern: \"TODO\"\n    level: error\n    \
             fix: { replace: { replacement: \"DONE\" } }\n",
    )
    .unwrap();
    let session = build_session(root)
        .expect("build_session succeeds")
        .expect("a config is present");
    let fixer = session
        .engine
        .fixer_for("no-todo")
        .expect("no-todo declares a fixer");
    assert!(
        fixer.collects_located_edits(),
        "replace is a located fixer, so code_action takes the collect_edits path"
    );
    let text = "x\u{1F600} TODO and TODO\n";
    let violation = Violation::new("forbidden").with_path(PathBuf::from("a.txt"));
    let edits = fixer.collect_edits(
        std::slice::from_ref(&violation),
        Path::new("a.txt"),
        text.as_bytes(),
        root,
    );
    let ws = located_edits_to_workspace_edit(&edits, text, Path::new("a.txt"), root)
        .expect("the located edits map to a workspace edit");
    let changes = ws.changes.expect("located edits use the changes map");
    let uri = Url::from_file_path(root.join("a.txt")).unwrap();
    let tes = &changes[&uri];
    assert_eq!(tes.len(), 2, "both TODO occurrences become TextEdits");
    assert!(tes.iter().all(|te| te.new_text == "DONE"));
    // 'x'=0, emoji=cols 1-2 (two UTF-16 units), ' '=3 -> first TODO at character 4.
    assert_eq!(tes[0].range.start, Position::new(0, 4));
}

#[test]
fn code_action_offers_every_match_for_a_path_matches_replace() {
    // Regression (audit 2026-09-20): the located `replace` fixer now correlates
    // its edits to the violation SET (W4). `code_action` synthesizes a
    // positional violation with NO baseline_key, so the fixer falls back to
    // fixing every occurrence -- otherwise the empty correlation budget would
    // leave the LSP offering NO `*_path_matches` fix.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: v-pins\n    kind: json_path_matches\n    \
             paths: \"*.json\"\n    path: \"$.deps.*\"\n    matches: \"^v\"\n    level: error\n    \
             fix: { replace: { pattern: \"^\", replacement: \"v\", applicability: safe } }\n",
    )
    .unwrap();
    let session = build_session(root)
        .expect("build_session succeeds")
        .expect("a config is present");
    let fixer = session
        .engine
        .fixer_for("v-pins")
        .expect("v-pins declares a fixer");
    assert!(fixer.collects_located_edits());
    let text = "{\"deps\": {\"a\": \"1.0\", \"b\": \"2.0\"}}";
    // As `code_action` builds it: a positional path-bearing violation, NO key.
    let violation = Violation::new("v-pins").with_path(PathBuf::from("app.json"));
    let edits = fixer.collect_edits(
        std::slice::from_ref(&violation),
        Path::new("app.json"),
        text.as_bytes(),
        root,
    );
    let ws = located_edits_to_workspace_edit(&edits, text, Path::new("app.json"), root)
        .expect("the located edits map to a workspace edit");
    let changes = ws.changes.expect("located edits use the changes map");
    let uri = Url::from_file_path(root.join("app.json")).unwrap();
    assert_eq!(
        changes[&uri].len(),
        2,
        "both failing nodes are offered (the keyless fix-all fallback)"
    );
}

#[test]
fn violation_range_converts_char_columns_to_utf16() {
    // Audit HIGH: alint columns count `char`s; LSP positions are UTF-16
    // units. Two emoji (2 units each) + 'x' put the ZWSP (char column 4)
    // at UTF-16 character 5 — not 3.
    let text = "\u{1F600}\u{1F600}x\u{200B}y\n";
    let r = violation_range(&violation(Some(1), Some(4)), Some(text));
    assert_eq!(r.start, Position::new(0, 5));
    assert_eq!(r.end, Position::new(0, 6));
    // A marker ON a non-BMP char spans both of its units.
    let r = violation_range(&violation(Some(1), Some(2)), Some(text));
    assert_eq!((r.start, r.end), (Position::new(0, 2), Position::new(0, 4)));
    // Second line, CRLF-free; a column past EOL stays one unit per char.
    let r = violation_range(&violation(Some(2), Some(3)), Some("ab\n\u{1F600}"));
    assert_eq!(r.start, Position::new(1, 3));
    assert_eq!(utf16_column("ab", 5), (4, 1));

    // SourceRange treats lone CR as a physical separator. The second-line
    // emoji occupies two UTF-16 units, so scalar-column fallback is incorrect.
    let cr = "a\r\u{1F600}x";
    let x = cr.find('x').unwrap();
    let ranged =
        Violation::new("boom").with_range(SourceRange::from_byte_range(cr, x..x + 1).unwrap());
    let r = violation_range(&ranged, Some(cr));
    assert_eq!((r.start, r.end), (Position::new(1, 2), Position::new(1, 3)));
}

#[test]
fn render_finding_shows_description_fix_availability_and_docs_link() {
    let mut f = finding(None);
    f.fixable = true;
    f.description = kind_description("no_zero_width_chars");
    f.docs_url = rule_docs_url("no_zero_width_chars");
    let md = render_finding(&f);
    assert!(md.contains("Fix: available"), "{md}");
    assert!(
        md.contains("https://alint.org/docs/rules/") && md.contains("/no_zero_width_chars/"),
        "{md}"
    );
    assert!(f.description.is_some_and(|d| md.contains(d)), "{md}");
    f.fixable = false;
    assert!(render_finding(&f).contains("Fix: none"));
}

#[test]
fn rule_docs_url_resolves_aliases_to_the_canonical_page() {
    let (alias, canonical) = alint_rules::categories::ALIAS_TO_CANONICAL[0];
    let url = rule_docs_url(alias).expect("alias has a docs page");
    assert!(url.ends_with(&format!("/{canonical}/")), "{url}");
    assert!(rule_docs_url("no-such-kind").is_none());
}

#[test]
fn render_notes_counts_by_default_and_lists_with_show_notes() {
    let config = Path::new("/r/.alint.yml");
    assert_eq!(render_notes(config, &[], true), "");
    let notes = vec!["a.txt: skipped \u{1b}[31m${MODULE}".to_string()];
    let short = render_notes(config, &notes, false);
    assert!(short.contains("1 informational note(s)"), "{short}");
    assert!(short.contains("--show-notes"), "{short}");
    let long = render_notes(config, &notes, true);
    assert!(long.contains("note: a.txt: skipped"), "{long}");
    assert!(!long.contains('\u{1b}'), "control chars escaped: {long:?}");
}

#[test]
fn exit_code_follows_the_lsp_spec() {
    assert_eq!(exit_code(true, true), 0, "exit after shutdown");
    assert_eq!(exit_code(true, false), 1, "exit without shutdown");
    assert_eq!(exit_code(false, false), 0, "stream closed");
}

#[test]
fn workspace_folders_keeps_every_root() {
    use tower_lsp::lsp_types::WorkspaceFolder;
    let a = repo_root().join("a");
    let b = repo_root().join("b");
    let params = InitializeParams {
        workspace_folders: Some(vec![
            WorkspaceFolder {
                uri: Url::from_file_path(&a).unwrap(),
                name: "a".into(),
            },
            WorkspaceFolder {
                uri: Url::from_file_path(&b).unwrap(),
                name: "b".into(),
            },
        ]),
        ..InitializeParams::default()
    };
    assert_eq!(workspace_folders(&params), vec![a, b]);
}

#[test]
fn config_for_document_ignores_files_outside_every_folder() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), "version: 1\n").unwrap();
    let doc = tmp.path().join("a.txt");
    assert_eq!(
        config_for_document(&doc, &[tmp.path().to_path_buf()]),
        Some(tmp.path().join(".alint.yml"))
    );
    assert_eq!(config_for_document(&doc, &[repo_root()]), None);
}

#[tokio::test]
async fn code_action_withholds_fixes_over_the_fix_size_limit() {
    // The LSP ignored `fix_size_limit`: `alint fix` skips an oversized
    // file, so the editor must not offer to fix it either.
    use tower_lsp::lsp_types::{
        CodeActionContext, PartialResultParams, TextDocumentIdentifier, WorkDoneProgressParams,
    };
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nfix_size_limit: 4\nrules:\n  - id: no-todo\n    \
             kind: file_content_forbidden\n    paths: \"*.txt\"\n    pattern: \"TODO\"\n    \
             level: error\n    fix: { replace: { replacement: \"DONE\" } }\n",
    )
    .unwrap();
    let session = build_session(&root).unwrap().unwrap();
    assert_eq!(session.engine.fix_size_limit(), Some(4));
    let (service, _socket) = LspService::new(test_backend);
    let backend = service.inner();
    let uri = Url::from_file_path(root.join("a.txt")).unwrap();
    let mut f = finding(None);
    f.rule_id = "no-todo".to_string();
    f.fixable = true;
    f.range = Range::new(Position::new(0, 2), Position::new(0, 3));
    {
        let mut st = backend.state.lock();
        install_session(&mut st, &root, &uri, session);
        st.open.insert(uri.clone());
        st.documents.insert(uri.clone(), "x TODO\n".to_string());
        st.diagnostics.insert(uri.clone(), vec![f]);
    }
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::new(Position::new(0, 2), Position::new(0, 3)),
        context: CodeActionContext::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    };
    let resp = backend.code_action(params).await.expect("code_action ok");
    assert!(resp.is_none(), "oversized buffer gets no fix: {resp:?}");
}

#[test]
fn build_session_rejects_a_missing_baseline() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nbaseline: missing.json\nrules: []\n",
    )
    .unwrap();
    let err = build_session(dir.path()).expect_err("missing baseline is a config error");
    assert!(err.contains("baseline"), "{err}");
}
