use super::*;
use crate::{FileEntry, Scope};

fn empty_index() -> FileIndex {
    FileIndex::default()
}

#[test]
fn range_builder_projects_start_to_legacy_point_fields() {
    let range = SourceRange::new(2, 3, 4, 5).unwrap();
    let finding = Violation::new("m").with_range(range.clone());
    assert_eq!(finding.line, Some(2));
    assert_eq!(finding.column, Some(3));
    assert_eq!(finding.range, Some(range));
}

#[test]
fn point_builder_after_range_clears_range_projection() {
    let finding = Violation::new("m")
        .with_range(SourceRange::new(2, 3, 4, 5).unwrap())
        .with_location(7, 8);
    assert_eq!((finding.line, finding.column), (Some(7), Some(8)));
    assert!(finding.range.is_none());
}

#[test]
fn related_locations_are_deduplicated_capped_and_order_independent() {
    fn collect(indices: impl Iterator<Item = usize>) -> Violation {
        Violation::new("m").with_related_locations(
            indices.map(|index| RelatedLocation::new(format!("path-{index:02}"), "secondary")),
        )
    }

    let forward = collect((0..25).chain([24, 24, 23]));
    let reverse = collect((0..25).rev().chain([24, 24, 23]));
    assert_eq!(forward.related_locations, reverse.related_locations);
    assert_eq!(
        forward.related_locations.len(),
        crate::MAX_RELATED_LOCATIONS
    );
    assert_eq!(forward.related_locations_omitted, 5);
    assert_eq!(reverse.related_locations_omitted, 5);
    assert_eq!(
        forward.related_locations.first().unwrap().path,
        Path::new("path-00")
    );
    assert_eq!(
        forward.related_locations.last().unwrap().path,
        Path::new("path-19")
    );
}

#[test]
fn public_sensitive_metadata_excludes_value_bearing_fields() {
    const SECRET: &str = "BASELINE_KEY_SECRET_DO_NOT_REPORT";
    let mut finding = Violation::new(format!("value {SECRET}"))
        .with_path(Path::new("secrets.env"))
        .with_baseline_key(format!("query\0{SECRET}"))
        .with_fix_key(format!("occurrence\0{SECRET}"))
        .with_related_location(RelatedLocation::new(SECRET, "declared target"));
    finding.redact_evidence("rule", "source\0secrets.env\0query", [SECRET]);

    // Remediation still sees the producer key until the public boundary.
    assert!(
        finding
            .baseline_key
            .as_deref()
            .is_some_and(|key| key.contains(SECRET))
    );
    assert!(
        finding
            .fix_key
            .as_deref()
            .is_some_and(|key| key.contains(SECRET))
    );
    finding.sanitize_sensitive_metadata("rule");

    let key = finding.baseline_key.as_deref().unwrap();
    assert!(key.starts_with("\0alint-sensitive\0"), "{key:?}");
    assert!(!key.contains(SECRET), "{key:?}");
    assert!(finding.fix_key.is_none(), "{finding:?}");
    assert!(!finding.message.contains(SECRET), "{finding:?}");
    assert!(finding.related_locations.is_empty(), "{finding:?}");
    assert_eq!(finding.related_locations_omitted, 0);
}

#[test]
fn sensitive_path_projection_preserves_internal_identity() {
    let path = Path::new("environment-derived-secret.txt");
    let violation = Violation::new("redacted").with_path(path);
    let note = Violation::new("redacted").with_path(path).as_note();
    let failure = Violation::new("redacted")
        .with_path(path)
        .as_analysis_failure(AnalysisFailureKind::Evaluation);
    let before = crate::baseline::fingerprint("rule", &violation, None);
    let mut result = RuleResult::new(
        "rule".into(),
        Level::Error,
        None,
        vec![violation, note, failure],
        false,
    );

    result.redact_sensitive_paths();

    let finding = &result.violations[0];
    assert_eq!(finding.path.as_deref(), Some(path));
    assert_eq!(finding.report_path(), Some(Path::new("<redacted>")));
    assert_eq!(
        crate::baseline::path_identity_fingerprint("rule", finding, None),
        before,
        "public path redaction must not change the engine's identity for the finding"
    );
    assert_ne!(
        crate::baseline::fingerprint("rule", finding, None),
        before,
        "the baseline fingerprint leaves a hidden path out (ADR-0030 decision 1)"
    );
    assert_eq!(result.notes[0].report_path(), Some(Path::new("<redacted>")));
    assert_eq!(
        result.analysis_failures[0].report_path(),
        Some(Path::new("<redacted>"))
    );
}

#[test]
fn a_hidden_path_stays_out_of_its_redaction_markers() {
    // A marker that hashed a path alint hides would tell anyone who can list
    // the repository's files which one it is.
    let redacted = |path: &str, hidden: bool| {
        let mut result = RuleResult::new(
            "rule".into(),
            Level::Error,
            None,
            vec![
                Violation::new("x").with_path(Path::new(path)),
                Violation::new("x")
                    .with_path(Path::new(path))
                    .as_analysis_failure(AnalysisFailureKind::Evaluation),
            ],
            false,
        );
        if hidden {
            result.redact_sensitive_paths();
        }
        result.redact_sensitive();
        result.sanitize_sensitive_metadata();
        let finding = &result.violations[0];
        (
            finding.message.to_string(),
            finding.baseline_key.clone(),
            result.analysis_failures[0].message.to_string(),
        )
    };
    assert_eq!(redacted("a/one.txt", true), redacted("b/two.txt", true));
    // A path that is shown still tells two findings apart.
    let [one, two] = [redacted("a/one.txt", false), redacted("b/two.txt", false)];
    assert_ne!(one.0, two.0);
    assert_ne!(one.1, two.1);
    assert_ne!(one.2, two.2);
}

#[test]
fn evidence_redaction_removes_raw_and_debug_escaped_values() {
    const SECRET: &str = "line one\n\"line two\"";
    let rendered = format!("source value {SECRET:?}; raw value {SECRET} does not match");
    let mut finding = Violation::new(rendered);
    finding.redact_evidence("rule", "source\0.env\0$.TOKEN", [SECRET]);

    assert!(!finding.message.contains("line one"), "{finding:?}");
    assert!(!finding.message.contains("line two"), "{finding:?}");
    assert!(finding.message.contains("<redacted:"), "{finding:?}");
    assert!(
        finding.message.contains("source value") && finding.message.contains("does not match"),
        "{finding:?}"
    );
}

// Unix-only: NAME_MAX is the unix limit this pins; on Windows the long
// absolute tempdir path would trip MAX_PATH instead, an unrelated limit.
#[cfg(unix)]
#[test]
fn write_atomic_handles_a_name_near_name_max() {
    // Audit 2026-10 finding 9: the temp name embedded the target's name
    // (`.{name}.alint-fix.{pid}.{n}`), so a 240-byte basename pushed it past
    // NAME_MAX (255) and the write failed with ENAMETOOLONG (os error 36).
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join(format!("{}.txt", "a".repeat(240)));
    std::fs::write(&p, b"old").unwrap();
    write_atomic(&p, b"new").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"new");
    let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(leftovers, 1, "no temp file left behind");
}

#[cfg(unix)]
#[test]
fn write_atomic_refuses_a_file_the_owner_cannot_write() {
    // `readonly()` is true only when ALL write bits are clear, so a 0464
    // file (owner may not write; group may) was silently replaced via
    // temp+rename although an in-place write fails with EACCES.
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("ro464.txt");
    std::fs::write(&p, b"old").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o464)).unwrap();
    if std::fs::OpenOptions::new().append(true).open(&p).is_ok() {
        // Running as root (DAC override): an in-place write would succeed
        // too, so there is nothing to refuse. Not the case under test.
        return;
    }
    let err = write_atomic(&p, b"new").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(err.to_string().contains("not writable in place"), "{err}");
    assert_eq!(std::fs::read(&p).unwrap(), b"old");
    let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o464);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn write_atomic_writes_through_a_symlink_and_keeps_it() {
    // A temp+rename on the link itself would replace it with a regular file.
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    std::fs::write(&target, b"old").unwrap();
    std::os::unix::fs::symlink("target.txt", &link).unwrap();
    write_atomic(&link, b"new").unwrap();
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
}

#[test]
fn write_atomic_writes_its_temporary_file_beside_the_target() {
    use super::fix_io::temp_dir_for;
    assert_eq!(temp_dir_for(Path::new("a/b.txt")), Path::new("a"));
    assert_eq!(temp_dir_for(Path::new("b.txt")), Path::new("."));
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(temp_dir_for(&dir.path().join("x.txt")), dir.path());
}

#[test]
fn write_atomic_lets_a_file_that_vanished_be_written() {
    // The file existed when its metadata was read and is gone by the
    // writability probe: nothing is left to protect.
    let dir = tempfile::tempdir().unwrap();
    let present = dir.path().join("present.txt");
    std::fs::write(&present, b"x").unwrap();
    let meta = std::fs::metadata(&present).unwrap();
    super::fix_io::refuse_unwritable(&dir.path().join("gone.txt"), &meta).unwrap();
}

#[test]
fn a_fixer_needs_no_existing_directory_by_default() {
    assert!(!TierEdgeFixer(Applicability::Safe).needs_existing_directory());
}

#[test]
fn write_atomic_refuses_a_read_only_file() {
    // Audit 2026-10 finding 11: temp+rename only needs DIRECTORY write
    // access, so a read-only file was silently replaced. It must be refused
    // (left byte-identical, mode intact) so the fix reports an error.
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("locked.txt");
    std::fs::write(&p, b"old").unwrap();
    let mut perms = std::fs::metadata(&p).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&p, perms).unwrap();
    let err = write_atomic(&p, b"new").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(std::fs::read(&p).unwrap(), b"old");
    assert!(std::fs::metadata(&p).unwrap().permissions().readonly());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    // Restore so the tempdir cleans up on every platform.
    let mut perms = std::fs::metadata(&p).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    std::fs::set_permissions(&p, perms).unwrap();
}

#[test]
fn violation_builder_sets_fields_via_chain() {
    let v = Violation::new("trailing whitespace")
        .with_path(Path::new("src/main.rs"))
        .with_location(12, 4);
    assert_eq!(v.message, "trailing whitespace");
    assert_eq!(v.path.as_deref(), Some(Path::new("src/main.rs")));
    assert_eq!(v.line, Some(12));
    assert_eq!(v.column, Some(4));
}

#[test]
fn violation_new_starts_with_no_path_or_location() {
    let v = Violation::new("global note");
    assert!(v.path.is_none());
    assert!(v.line.is_none());
    assert!(v.column.is_none());
}

/// A whole-file fixer at a configurable tier that emits a `DeleteFile` edit
/// and does NOT override `collect_edits` -- exercises the default adapter.
#[derive(Debug)]
struct TierEdgeFixer(Applicability);
impl Fixer for TierEdgeFixer {
    fn describe(&self) -> String {
        "tier-edge".to_string()
    }
    fn applicability(&self) -> Applicability {
        self.0
    }
    fn apply(&self, _v: &Violation, _ctx: &FixContext<'_>) -> Result<FixOutcome> {
        Ok(FixOutcome::Skipped("n/a".to_string()))
    }
    fn fix_edit(&self, v: &Violation, _bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        v.path.as_deref().map(|p| FixEdit::DeleteFile {
            path: p.to_path_buf(),
        })
    }
}

#[test]
fn default_collect_edits_carries_the_fixers_tier_not_safe() {
    // The default `collect_edits` adapter must tag each CollectedEdit with the
    // fixer's OWN applicability, not a hardcoded `Safe` -- otherwise an Unsafe
    // fixer routed through the (Phase-1) located path would have its edits
    // applied at the Safe threshold, bypassing the tier gate.
    let v = Violation::new("x").with_path(Path::new("junk.bak"));
    for tier in [
        Applicability::Safe,
        Applicability::Unsafe,
        Applicability::Suggestion,
        Applicability::Never,
    ] {
        let f = TierEdgeFixer(tier);
        let edits = f.collect_edits(
            std::slice::from_ref(&v),
            Path::new("junk.bak"),
            b"",
            Path::new("/repo"),
        );
        assert_eq!(edits.len(), 1, "tier {tier:?}");
        assert_eq!(
            edits[0].applicability, tier,
            "collect_edits must carry the fixer's tier ({tier:?}), not Safe"
        );
    }
}

#[test]
fn new_partitions_notes_and_failures_out_of_violations() {
    let raw = vec![
        Violation::new("real one"),
        Violation::new("a skipped entry").as_note(),
        Violation::new("analysis stopped at the byte limit").as_analysis_truncated_note(),
        Violation::new("could not parse input")
            .with_path(Path::new("data.json"))
            .with_location(3, 7)
            .as_analysis_failure(AnalysisFailureKind::ResourceParse),
        Violation::new("real two"),
    ];
    let r = RuleResult::new("x".into(), Level::Error, None, raw, true);
    assert_eq!(r.violations.len(), 2, "notes and failures excluded");
    assert_eq!(r.notes.len(), 2);
    assert_eq!(r.notes[0].message, "a skipped entry");
    assert_eq!(r.notes[0].note_kind(), Some(NoteKind::Informational));
    assert_eq!(r.notes[1].note_kind(), Some(NoteKind::AnalysisTruncated));
    assert_eq!(r.analysis_failures.len(), 1);
    let failure = &r.analysis_failures[0];
    assert_eq!(failure.kind, AnalysisFailureKind::ResourceParse);
    assert_eq!(failure.path.as_deref(), Some(Path::new("data.json")));
    assert_eq!(failure.line, Some(3));
    assert_eq!(failure.column, Some(7));
    assert!(!r.passed(), "real violations and failure present");

    // Notes alone pass; a failure alone does not.
    let only_notes = RuleResult::new(
        "y".into(),
        Level::Error,
        None,
        vec![Violation::new("just a note").as_note()],
        false,
    );
    assert!(only_notes.passed(), "notes-only result passes");
    assert!(only_notes.violations.is_empty());
    assert_eq!(only_notes.notes.len(), 1);
    let only_failure = RuleResult::new(
        "z".into(),
        Level::Info,
        None,
        vec![Violation::new("failed").as_analysis_failure(AnalysisFailureKind::Evaluation)],
        false,
    );
    assert!(!only_failure.passed(), "failure-only result fails");
    assert!(only_failure.violations.is_empty());
    assert_eq!(only_failure.analysis_failures.len(), 1);
}

#[test]
fn rule_result_passes_only_without_violations_or_failures() {
    let mut r = RuleResult {
        rule_id: "x".into(),
        level: Level::Error,
        policy_url: None,
        violations: Vec::new(),
        notes: Vec::new(),
        is_fixable: false,
        analysis_failures: Vec::new(),
    };
    assert!(r.passed());
    r.violations.push(Violation::new("oops"));
    assert!(!r.passed());
    r.violations.clear();
    r.analysis_failures.push(AnalysisFailure::new(
        AnalysisFailureKind::ResourceRead,
        "unreadable",
    ));
    assert!(!r.passed());
}

#[derive(Debug)]
struct MissingReadRule {
    scope: Scope,
}

impl PerFileRule for MissingReadRule {
    fn path_scope(&self) -> &Scope {
        &self.scope
    }

    fn evaluate_file(
        &self,
        _ctx: &Context<'_>,
        _path: &Path,
        _bytes: &[u8],
    ) -> Result<Vec<Violation>> {
        panic!("a vanished file must not reach rule evaluation");
    }
}

#[test]
fn per_file_fallback_reports_a_discovered_file_that_vanished() {
    let dir = tempfile::tempdir().unwrap();
    let index = FileIndex::from_entries(vec![FileEntry {
        path: Arc::<Path>::from(Path::new("gone.txt")),
        is_dir: false,
        size: 1,
    }]);
    let ctx = Context {
        root: dir.path(),
        index: &index,
        registry: None,
        facts: None,
        vars: None,
        git_tracked: None,
        git_blame: None,
        analysis_time: None,
        process_environment: None,
        condition_environment: None,
        trace: None,
        structured_cache: None,
        findings_per_rule_limit: None,
    };
    let rule = MissingReadRule {
        scope: Scope::from_patterns(&["**".to_string()]).unwrap(),
    };

    let findings = eval_per_file(&rule, &ctx).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].analysis_failure_kind(),
        Some(AnalysisFailureKind::ResourceRead)
    );
    assert_eq!(findings[0].path.as_deref(), Some(Path::new("gone.txt")));
}

#[test]
fn context_is_git_tracked_returns_false_outside_repo() {
    let idx = empty_index();
    let ctx = Context {
        root: Path::new("/tmp"),
        index: &idx,
        registry: None,
        facts: None,
        vars: None,
        git_tracked: None, // outside-a-repo / no rule opted in
        git_blame: None,
        analysis_time: None,
        process_environment: None,
        condition_environment: None,
        trace: None,
        structured_cache: None,
        findings_per_rule_limit: None,
    };
    assert!(!ctx.is_git_tracked(Path::new("anything.rs")));
    assert!(!ctx.dir_has_tracked_files(Path::new("src")));
}

#[test]
fn context_is_git_tracked_consults_set_when_present() {
    let mut tracked: std::collections::HashSet<std::path::PathBuf> =
        std::collections::HashSet::new();
    tracked.insert(std::path::PathBuf::from("src/main.rs"));
    let idx = empty_index();
    let ctx = Context {
        root: Path::new("/tmp"),
        index: &idx,
        registry: None,
        facts: None,
        vars: None,
        git_tracked: Some(&tracked),
        git_blame: None,
        analysis_time: None,
        process_environment: None,
        condition_environment: None,
        trace: None,
        structured_cache: None,
        findings_per_rule_limit: None,
    };
    assert!(ctx.is_git_tracked(Path::new("src/main.rs")));
    assert!(!ctx.is_git_tracked(Path::new("README.md")));
}

/// Stand-in `Rule` impl that returns the trait defaults.
/// Lets us assert the documented defaults without dragging
/// in a real registered rule.
#[derive(Debug)]
struct DefaultRule;

impl Rule for DefaultRule {
    fn id(&self) -> &'static str {
        "default"
    }
    fn level(&self) -> Level {
        Level::Warning
    }
    fn evaluate(&self, _ctx: &Context<'_>) -> Result<Vec<Violation>> {
        Ok(Vec::new())
    }
}

#[test]
fn rule_trait_defaults_are_safe_no_ops() {
    let r = DefaultRule;
    assert_eq!(r.policy_url(), None);
    assert_eq!(r.git_tracked_mode(), GitTrackedMode::Off);
    assert!(!r.wants_git_blame());
    assert!(!r.wants_analysis_time());
    assert!(!r.requires_full_index());
    assert!(r.path_scope().is_none());
    assert!(!r.sees_path_past_walk(Path::new("README.md")));
    assert!(r.fixer().is_none());
}

#[test]
fn fix_context_shows_a_sensitive_path_as_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctx = FixContext::new(dir.path(), false);
    assert_eq!(
        ctx.shown_path(Path::new("dir/canary.txt")),
        "dir/canary.txt"
    );
    ctx.paths_sensitive = true;
    assert_eq!(
        ctx.shown_path(Path::new("dir/canary.txt")),
        crate::CONFIGURATION_MASK
    );
}

#[test]
fn check_fix_size_returns_none_when_limit_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("a.txt");
    std::fs::write(&f, b"hello").unwrap();
    let ctx = FixContext::new(dir.path(), false);
    let outcome = check_fix_size(&f, Path::new("a.txt"), &ctx).unwrap();
    assert!(outcome.is_none());
}

#[test]
fn check_fix_size_allows_a_file_exactly_at_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("edge.txt");
    std::fs::write(&f, vec![b'x'; 64]).unwrap();
    let ctx = |limit| FixContext::new(dir.path(), false).with_fix_size_limit(Some(limit));
    let at = check_fix_size(&f, Path::new("edge.txt"), &ctx(64)).unwrap();
    assert!(at.is_none(), "{at:?}");
    let over = check_fix_size(&f, Path::new("edge.txt"), &ctx(63)).unwrap();
    assert!(matches!(over, Some(FixOutcome::Skipped(_))), "{over:?}");
}

#[test]
fn check_fix_size_skips_over_limit_files() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("big.txt");
    std::fs::write(&f, vec![b'x'; 1024]).unwrap();
    let ctx = FixContext::new(dir.path(), false).with_fix_size_limit(Some(64));
    let outcome = check_fix_size(&f, Path::new("big.txt"), &ctx).unwrap();
    match outcome {
        Some(FixOutcome::Skipped(reason)) => {
            assert!(reason.contains("exceeds fix_size_limit"));
            assert!(reason.contains("big.txt"));
            // The limit can come from the environment.
            assert!(!reason.contains("64"), "{reason}");
        }
        other => panic!("expected Skipped, got {other:?}"),
    }
}

/// A content fix's read of the file it edits is where it learns it may not
/// edit it: a symlink into `.git` (which the walk no longer indexes, but a
/// rule may still name) is skipped with the creators' reason, and the write
/// backstop refuses it too, in compose mode and out.
#[cfg(unix)]
#[test]
fn a_content_fix_never_edits_through_a_symlink_into_the_git_directory() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/config"), b"[core]\n").unwrap();
    std::fs::create_dir(root.join("src")).unwrap();
    std::os::unix::fs::symlink("../.git/config", root.join("src/evil.rs")).unwrap();
    std::fs::write(root.join("src/real.rs"), b"x\n").unwrap();
    std::os::unix::fs::symlink("real.rs", root.join("src/alias.rs")).unwrap();
    let evil = root.join("src/evil.rs");
    let buffer = std::cell::RefCell::new(std::collections::BTreeMap::new());
    // The walk does not index `evil.rs`, so it cannot vouch for it.
    let index = crate::walker::walk(root, &crate::walker::WalkOptions::test_default()).unwrap();
    for ctx in [
        FixContext::new(root, false),
        FixContext::new(root, false).with_compose(Some(&buffer)),
        FixContext::new(root, false).with_index(Some(&index)),
    ] {
        match read_for_fix(&evil, Path::new("src/evil.rs"), &ctx).unwrap() {
            ReadForFix::Skipped(FixOutcome::Skipped(reason)) => assert_eq!(
                reason,
                "src/evil.rs is in a Git directory; a fix never writes or reads there"
            ),
            other => panic!("expected the Git-directory skip, got {other:?}"),
        }
        let error = ctx.commit_write(&evil, b"# SPDX\n").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        // A link to an ordinary file is still edited through.
        let alias = root.join("src/alias.rs");
        assert!(matches!(
            read_for_fix(&alias, Path::new("src/alias.rs"), &ctx).unwrap(),
            ReadForFix::Bytes(_)
        ));
        ctx.commit_write(&alias, b"y\n").unwrap();
    }
    assert!(
        buffer
            .borrow()
            .keys()
            .all(|key| !key.starts_with(root.join(".git")))
    );
    assert_eq!(
        std::fs::read(root.join(".git/config")).unwrap(),
        b"[core]\n"
    );
    assert_eq!(std::fs::read(root.join("src/real.rs")).unwrap(), b"y\n");
}

/// A fix asks its path checks about a file the walk reached without a symlink
/// by name alone, and about any other without resolving it unless a symlink is
/// on the way. Resolving every file a fix edits made `alint fix` make several
/// times the `stat`-family calls it made before.
#[test]
fn a_fix_resolves_no_path_without_a_symlink_on_the_way() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::write(root.join("src/deep/a.rs"), b"x \n").unwrap();
    std::fs::write(root.join(".alint.yml"), b"version: 1\n").unwrap();
    let config = vec![root.join(".alint.yml").canonicalize().unwrap()];
    let index = crate::walker::walk(root, &crate::walker::WalkOptions::test_default()).unwrap();
    let file = root.join("src/deep/a.rs");
    // (resolutions, symlink probes) of reading, then writing, the file.
    let cost = |ctx: &FixContext<'_>| {
        let before = crate::pathsafe::path_check_costs();
        assert!(matches!(
            read_for_fix(&file, Path::new("src/deep/a.rs"), ctx).unwrap(),
            ReadForFix::Bytes(_)
        ));
        ctx.commit_write(&file, b"x\n").unwrap();
        let after = crate::pathsafe::path_check_costs();
        (
            after.resolutions - before.resolutions,
            after.symlink_probes - before.symlink_probes,
        )
    };
    let buffer = std::cell::RefCell::new(std::collections::BTreeMap::new());
    for extended in [false, true] {
        let ctx = FixContext::new(root, false)
            .with_compose(Some(&buffer))
            .with_extended_source(extended)
            .with_config_files(&config);
        assert_eq!(cost(&ctx.with_index(Some(&index))), (0, 0), "{extended}");
        let ctx = FixContext::new(root, false)
            .with_extended_source(extended)
            .with_config_files(&config);
        let (resolutions, probes) = cost(&ctx);
        assert_eq!(resolutions, 0, "{extended}");
        assert!(probes > 0, "{extended}");
    }
}

/// A fix of a rule an `extends:`'d ruleset shaped never writes alint's own
/// configuration: a name discovery accepts at any depth, anything under
/// `.alint.d/`, a file the configuration came from, or one a symlink leads to.
/// The user's own rules still may, and anyone may read one.
#[cfg(unix)]
#[test]
fn a_fix_from_an_extended_rule_never_writes_alint_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("vendor")).unwrap();
    std::fs::create_dir(root.join(".alint.d")).unwrap();
    std::fs::write(root.join(".alint.yml"), b"version: 1\n").unwrap();
    std::fs::write(root.join("vendor/policy.yml"), b"version: 1\n").unwrap();
    std::os::unix::fs::symlink(".alint.yml", root.join("link.yml")).unwrap();
    std::os::unix::fs::symlink(".alint.d", root.join("dropins")).unwrap();
    let files = [root.join("vendor/policy.yml").canonicalize().unwrap()];
    let own = FixContext::new(root, false).with_config_files(&files);
    let extended = FixContext::new(root, false)
        .with_config_files(&files)
        .with_extended_source(true);
    for path in [
        ".alint.yml",
        "alint.yaml",
        "sub/.alint.yaml",
        ".ALINT.YML",
        ".alint.yml.",
        ".alint.d/housekeeping.yml",
        ".alint.d",
        "vendor/policy.yml",
        "link.yml",
        "dropins/x.yml",
    ] {
        assert_eq!(
            extended.write_refusal(Path::new(path)).as_deref(),
            Some(
                format!(
                    "{path} is an alint configuration file; a fix from an extended ruleset \
                     never writes one"
                )
                .as_str()
            ),
            "{path}"
        );
        assert_eq!(own.write_refusal(Path::new(path)), None, "{path}");
    }
    for path in [
        "README.md",
        "vendor/other.yml",
        ".alint/templates/x",
        "alint.yml.bak",
    ] {
        assert_eq!(extended.write_refusal(Path::new(path)), None, "{path}");
    }
    let config = root.join(".alint.yml");
    let error = extended.commit_write(&config, b"rules: []\n").unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(matches!(
        read_for_fix(&config, Path::new(".alint.yml"), &extended).unwrap(),
        ReadForFix::Skipped(FixOutcome::Skipped(_))
    ));
    assert!(matches!(
        read_source_for_fix(&config, Path::new(".alint.yml"), &extended).unwrap(),
        ReadForFix::Bytes(_)
    ));
    own.commit_write(&config, b"version: 1\nrules: []\n")
        .unwrap();
    assert_eq!(std::fs::read(&config).unwrap(), b"version: 1\nrules: []\n");
}

#[test]
fn read_for_fix_returns_bytes_when_in_limit() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("a.txt");
    std::fs::write(&f, b"hello").unwrap();
    let ctx = FixContext::new(dir.path(), false).with_fix_size_limit(Some(1 << 20));
    match read_for_fix(&f, Path::new("a.txt"), &ctx).unwrap() {
        ReadForFix::Bytes(b) => assert_eq!(b, b"hello"),
        ReadForFix::Skipped(_) => panic!("expected Bytes, got Skipped"),
    }
}

#[test]
fn read_for_fix_returns_skipped_when_over_limit() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("big.txt");
    std::fs::write(&f, vec![b'x'; 1024]).unwrap();
    let ctx = FixContext::new(dir.path(), false).with_fix_size_limit(Some(64));
    match read_for_fix(&f, Path::new("big.txt"), &ctx).unwrap() {
        ReadForFix::Skipped(FixOutcome::Skipped(_)) => {}
        ReadForFix::Skipped(FixOutcome::Applied(_)) => {
            panic!("expected Skipped, got Skipped(Applied)")
        }
        ReadForFix::Bytes(_) => panic!("expected Skipped, got Bytes"),
    }
}

#[test]
fn read_for_fix_refuses_a_non_regular_file() {
    // A fixer can be handed a config-verbatim path that skips the walker's
    // special-file filter (a `sync_from` `targets:` list entry). A bare
    // `std::fs::read` of a FIFO would block `O_RDONLY` forever, hanging `fix`.
    // A directory is the portable, hang-free proxy for a non-regular file
    // (same `metadata().is_file() == false` branch as a FIFO; this crate takes
    // no libc dep, so it cannot `mkfifo(3)` here). The read must Skip, never
    // reach the blocking `std::fs::read`.
    let dir = tempfile::tempdir().unwrap();
    let subdir = dir.path().join("adir");
    std::fs::create_dir(&subdir).unwrap();
    let ctx = FixContext::new(dir.path(), false);
    match read_for_fix(&subdir, Path::new("adir"), &ctx).unwrap() {
        ReadForFix::Skipped(FixOutcome::Skipped(r)) => {
            assert!(r.contains("not a regular file"), "{r}");
        }
        other => panic!("a non-regular file must Skip, got {other:?}"),
    }
}

#[test]
fn fix_outcome_variants_are_constructible() {
    // Sanity: documented variant shapes haven't drifted.
    let _applied = FixOutcome::Applied("created LICENSE".into());
    let _skipped = FixOutcome::Skipped("already exists".into());
}

#[test]
#[should_panic(expected = "commit_write reached during a dry run")]
fn commit_write_asserts_against_a_dry_run_write() {
    // A fixer that reaches `commit_write` during a dry run is a bug -- it must
    // guard `ctx.dry_run` first. The `debug_assert!` catches it loudly in
    // debug/tests (this test); in release the early-return makes it a safe
    // no-op instead of a silent tree mutation during the cap-confirmation pass.
    let dir = tempfile::tempdir().unwrap();
    let ctx = FixContext::new(dir.path(), true);
    let _ = ctx.commit_write(&dir.path().join("x"), b"nope");
}

#[test]
fn compose_buffer_captures_writes_and_reads_them_back() {
    let dir = tempfile::tempdir().unwrap();
    let abs = dir.path().join("a.txt");
    std::fs::write(&abs, b"on disk").unwrap();
    let buf = RefCell::new(BTreeMap::new());
    let ctx = FixContext::new(dir.path(), false).with_compose(Some(&buf));
    let rel = Path::new("a.txt");
    // Compose mode: commit_write buffers; disk stays untouched.
    ctx.commit_write(&abs, b"composed").unwrap();
    assert_eq!(
        std::fs::read(&abs).unwrap(),
        b"on disk",
        "compose mode must not touch disk until flush"
    );
    // read_for_fix reads the buffered bytes back, so a later fixer in the
    // same pass composes on top of the earlier one.
    match read_for_fix(&abs, rel, &ctx).unwrap() {
        ReadForFix::Bytes(b) => assert_eq!(b, b"composed"),
        ReadForFix::Skipped(_) => panic!("expected buffered bytes"),
    }
    // A file not in the buffer falls through to disk.
    let abs2 = dir.path().join("b.txt");
    std::fs::write(&abs2, b"other").unwrap();
    match read_for_fix(&abs2, Path::new("b.txt"), &ctx).unwrap() {
        ReadForFix::Bytes(b) => assert_eq!(b, b"other"),
        ReadForFix::Skipped(_) => panic!("expected disk bytes"),
    }
}

#[test]
fn commit_write_direct_mode_writes_through_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let abs = dir.path().join("a.txt");
    std::fs::write(&abs, b"old").unwrap();
    let ctx = FixContext::new(dir.path(), false);
    // No compose buffer: commit_write is a direct atomic write, identical
    // to calling write_atomic.
    ctx.commit_write(&abs, b"new").unwrap();
    assert_eq!(std::fs::read(&abs).unwrap(), b"new");
}

#[cfg(unix)]
#[test]
fn compose_buffer_coalesces_symlink_and_target() {
    // R-audit: a symlink and its in-tree target must share ONE buffer
    // entry, so a fixer writing through the link and another reading the
    // target compose -- matching the direct-write path, which writes
    // through the link. Without this the two would report two independent
    // Applied edits instead of one Applied + one Skipped.
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("real.txt");
    std::fs::write(&target, b"orig").unwrap();
    let link = dir.path().join("link.txt");
    symlink(&target, &link).unwrap();
    let buf = RefCell::new(BTreeMap::new());
    let ctx = FixContext::new(dir.path(), false).with_compose(Some(&buf));
    // Write through the LINK; read through the TARGET must see the write.
    ctx.commit_write(&link, b"composed").unwrap();
    match read_for_fix(&target, Path::new("real.txt"), &ctx).unwrap() {
        ReadForFix::Bytes(b) => assert_eq!(b, b"composed"),
        ReadForFix::Skipped(_) => panic!("symlink and target must share one entry"),
    }
    assert_eq!(buf.borrow().len(), 1, "coalesced to a single keyed entry");
}

#[cfg(unix)]
#[test]
fn compose_buffer_coalesces_through_a_symlinked_dir() {
    // R-audit-3: a file reached through a symlinked DIRECTORY must coalesce
    // with the same file reached directly. This needs FULL canonicalization,
    // not just a final-component symlink check (the final component --
    // file.txt -- is a regular file in both spellings).
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("real")).unwrap();
    std::fs::write(dir.path().join("real/file.txt"), b"orig").unwrap();
    symlink(dir.path().join("real"), dir.path().join("link")).unwrap(); // link/ -> real/
    let buf = RefCell::new(BTreeMap::new());
    let ctx = FixContext::new(dir.path(), false).with_compose(Some(&buf));
    // Write through the symlinked dir; read through the real dir -> hit.
    ctx.commit_write(&dir.path().join("link/file.txt"), b"composed")
        .unwrap();
    match read_for_fix(
        &dir.path().join("real/file.txt"),
        Path::new("real/file.txt"),
        &ctx,
    )
    .unwrap()
    {
        ReadForFix::Bytes(b) => assert_eq!(b, b"composed"),
        ReadForFix::Skipped(_) => panic!("dir-symlink alias must coalesce"),
    }
    assert_eq!(
        buf.borrow().len(),
        1,
        "one entry for the real underlying file"
    );
}

#[test]
fn fix_context_new_defaults_and_setters() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = FixContext::new(dir.path(), true);
    assert!(ctx.dry_run);
    assert_eq!(ctx.fix_size_limit, None);
    assert!(!ctx.allow_out_of_root && !ctx.paths_sensitive);
    assert!(ctx.compose.is_none() && ctx.stage_ops.is_none());
    assert!(ctx.process_environment.is_none());

    let compose = std::cell::RefCell::new(std::collections::BTreeMap::new());
    let sink = std::cell::RefCell::new(Vec::new());
    let environment = std::collections::BTreeMap::new();
    let ctx = FixContext::new(dir.path(), false)
        .with_fix_size_limit(Some(7))
        .with_allow_out_of_root(true)
        .with_compose(Some(&compose))
        .with_stage_ops(Some(&sink))
        .with_process_environment(Some(&environment))
        .with_paths_sensitive(true);
    assert_eq!(ctx.fix_size_limit, Some(7));
    assert!(ctx.allow_out_of_root && ctx.paths_sensitive);
    assert!(ctx.compose.is_some() && ctx.stage_ops.is_some());
    assert!(ctx.process_environment.is_some());
}

/// A trace sink that keeps nothing, for checking that a context holds one.
#[derive(Debug)]
struct DiscardTrace;

impl crate::trace::TraceSink for DiscardTrace {
    fn record_kind(&self, _stage: crate::trace::TraceStage) {}

    fn record_extraction(&self, _observation: crate::trace::TraceExtractionObservation) {}

    fn preview_limit(&self) -> Option<usize> {
        None
    }

    fn evidence_sensitivity(&self, _producer_sensitive: bool) -> crate::trace::TraceSensitivity {
        crate::trace::TraceSensitivity::Unknown
    }

    fn paths_are_sensitive(&self) -> bool {
        false
    }

    fn record_preview(&self, _row: Result<crate::trace::TracePreviewRow>) {}
}

#[test]
fn every_context_setter_sets_its_field() {
    let root = Path::new("/repo");
    let index = empty_index();
    let registry = crate::RuleRegistry::new();
    let facts = FactValues::default();
    let vars = HashMap::from([("org".to_string(), "acme".to_string())]);
    let tracked = std::collections::HashSet::from([PathBuf::from("a.rs")]);
    let blame = crate::git::BlameCache::new(root.to_path_buf());
    let analysis_time = crate::AnalysisTimeResolution::available(
        crate::AnalysisTime::from_unix_seconds(86_400).unwrap(),
        crate::AnalysisTimeSource::Explicit,
    );
    let environment = BTreeMap::from([("PATH".to_string(), "/bin".to_string())]);
    let trace = DiscardTrace;

    let bare = Context::new(root, &index);
    assert_eq!(bare.root, root);
    assert!(std::ptr::eq(bare.index, &raw const index));
    assert!(bare.registry.is_none() && bare.facts.is_none() && bare.vars.is_none());
    assert!(bare.git_tracked.is_none() && bare.git_blame.is_none());
    assert!(bare.analysis_time.is_none() && bare.process_environment.is_none());
    assert!(bare.trace.is_none());

    let ctx = bare
        .with_registry(Some(&registry))
        .with_facts(Some(&facts))
        .with_vars(Some(&vars))
        .with_git_tracked(Some(&tracked))
        .with_git_blame(Some(&blame))
        .with_analysis_time(Some(&analysis_time))
        .with_process_environment(Some(&environment))
        .with_trace(Some(&trace));
    assert!(std::ptr::eq(ctx.registry.unwrap(), &raw const registry));
    assert!(std::ptr::eq(ctx.facts.unwrap(), &raw const facts));
    assert!(std::ptr::eq(ctx.vars.unwrap(), &raw const vars));
    assert!(std::ptr::eq(ctx.git_tracked.unwrap(), &raw const tracked));
    assert!(std::ptr::eq(ctx.git_blame.unwrap(), &raw const blame));
    assert!(std::ptr::eq(
        ctx.analysis_time.unwrap(),
        &raw const analysis_time
    ));
    assert!(std::ptr::eq(
        ctx.process_environment.unwrap(),
        &raw const environment
    ));
    assert!(ctx.trace.is_some());

    // A copy keeps every part but the one changed.
    let untraced = ctx.clone().with_trace(None);
    assert!(untraced.trace.is_none());
    assert!(std::ptr::eq(
        untraced.registry.unwrap(),
        &raw const registry
    ));
    assert!(std::ptr::eq(
        untraced.process_environment.unwrap(),
        &raw const environment
    ));
}
