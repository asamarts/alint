use super::*;

// ---- Fixture for the located-edit regime (dormant in Phase 0) ----
//
// Exercises the engine wiring that no shipped fixer reaches yet: a fixer
// that opts into `collects_located_edits` and returns two byte-disjoint
// `ReplaceRange` edits tagged into ONE isolation group. The engine must
// collect them, splice through `located_fix`, write the result, and report.

#[derive(Debug)]
struct LocatedFixture {
    app: Applicability,
}

impl crate::rule::Fixer for LocatedFixture {
    fn describe(&self) -> String {
        "fixture located fixer".to_string()
    }
    fn apply(&self, _v: &Violation, _ctx: &FixContext<'_>) -> crate::error::Result<FixOutcome> {
        // Never called: the engine routes located fixers through collect_edits.
        Ok(FixOutcome::Skipped("unused".to_string()))
    }
    fn collects_located_edits(&self) -> bool {
        true
    }
    fn collect_edits(
        &self,
        _violations: &[Violation],
        file: &Path,
        _bytes: &[u8],
        _root: &Path,
    ) -> Vec<crate::rule::CollectedEdit> {
        let app = self.app;
        let mk = move |range: std::ops::Range<usize>, content: &str| crate::rule::CollectedEdit {
            edit: FixEdit::ReplaceRange {
                path: file.to_path_buf(),
                range,
                content: content.as_bytes().to_vec(),
            },
            applicability: app,
            verify: crate::rule::EditVerifier::None,
            isolation_group: Some(1),
        };
        // Disjoint, but same isolation group: the earlier-ordered edit wins.
        vec![mk(0..1, "X"), mk(4..5, "Y")]
    }
}

#[derive(Debug)]
struct LocatedRule {
    id: String,
    scope: Scope,
    fixer: LocatedFixture,
}

impl Rule for LocatedRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }
    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let mut out = Vec::new();
        for entry in ctx.index.files() {
            if self.scope.matches(&entry.path, ctx.index) {
                out.push(Violation::new("located hit").with_path(entry.path.clone()));
            }
        }
        Ok(out)
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(&self.fixer)
    }
}

fn located_rule_with(app: Applicability) -> Box<dyn Rule> {
    Box::new(LocatedRule {
        id: "loc".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        fixer: LocatedFixture { app },
    })
}

fn located_rule() -> Box<dyn Rule> {
    located_rule_with(Applicability::Safe)
}

#[test]
fn malformed_manifest_blocks_fixing_and_preserves_the_target() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("package.json"), b"{not json").unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"01234567").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let rule = Box::new(LocatedRule {
        id: "manifest-fix".into(),
        scope: manifest_scope(),
        fixer: LocatedFixture {
            app: Applicability::Safe,
        },
    });

    let report = Engine::new(vec![rule], RuleRegistry::new())
        .fix(
            tmp.path(),
            &index,
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();

    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"01234567"
    );
    assert_eq!(report.total_analysis_failures(), 1);
    assert!(report.results[0].items.is_empty());
    assert_eq!(
        report.results[0].analysis_failures[0].kind,
        AnalysisFailureKind::ResourceParse,
    );
}

// ---- Fixture for a MIXED real+identity located batch (Phase-2 pre-reqs) ----
//
// Two DISJOINT, non-grouped edits so both apply: one changes bytes, one is an
// identity rewrite (its replacement equals the bytes it spans). Exercises the
// per-edit no-op downgrade (pre-req 2) and the per-edit range keying (pre-req 1)
// that arm for a multi-node located fixer (Phase-2 `set_value`); `ReplaceFixer`
// triggers neither.
#[derive(Debug)]
struct MixedBatchFixture;

impl crate::rule::Fixer for MixedBatchFixture {
    fn describe(&self) -> String {
        "fixture mixed-batch located fixer".to_string()
    }
    fn apply(&self, _v: &Violation, _ctx: &FixContext<'_>) -> crate::error::Result<FixOutcome> {
        Ok(FixOutcome::Skipped("unused".to_string()))
    }
    fn collects_located_edits(&self) -> bool {
        true
    }
    fn collect_edits(
        &self,
        _violations: &[Violation],
        file: &Path,
        _bytes: &[u8],
        _root: &Path,
    ) -> Vec<crate::rule::CollectedEdit> {
        let mk = move |range: std::ops::Range<usize>, content: &str| crate::rule::CollectedEdit {
            edit: FixEdit::ReplaceRange {
                path: file.to_path_buf(),
                range,
                content: content.as_bytes().to_vec(),
            },
            applicability: Applicability::Safe,
            verify: crate::rule::EditVerifier::None,
            isolation_group: None, // disjoint + independent -> BOTH apply
        };
        // On "01234567": 0..1 -> "X" CHANGES a byte; 4..5 -> "4" is an IDENTITY
        // rewrite (file[4] == '4'). apply_file_edits applies both (disjoint, no
        // group), so the file DID change -- the old whole-file `batch_changed`
        // reported the identity edit `Applied` too; the per-edit check skips it.
        vec![mk(0..1, "X"), mk(4..5, "4")]
    }
}

#[derive(Debug)]
struct MixedBatchRule {
    id: String,
    scope: Scope,
    fixer: MixedBatchFixture,
}

impl Rule for MixedBatchRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn path_scope(&self) -> Option<&Scope> {
        Some(&self.scope)
    }
    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let mut out = Vec::new();
        for entry in ctx.index.files() {
            if self.scope.matches(&entry.path, ctx.index) {
                out.push(Violation::new("mixed hit").with_path(entry.path.clone()));
            }
        }
        Ok(out)
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(&self.fixer)
    }
}

#[test]
fn located_mixed_real_and_identity_batch_downgrades_only_the_identity_edit() {
    // Phase-2 pre-req 2 (per-edit no-op downgrade): a batch mixing a REAL edit
    // with an IDENTITY edit must report the real one Applied and the identity
    // one Skipped. The old whole-file `batch_changed` reported BOTH Applied
    // because the file DID change (only the real edit moved a byte). Pre-req 1
    // (per-edit range keying): the two items must carry DISTINCT `violation_key`s
    // so an Applied for one node cannot lock-mask a standing Skipped for another.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"01234567").unwrap();
    let mixed = || -> Box<dyn Rule> {
        Box::new(MixedBatchRule {
            id: "mixed".into(),
            scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
            fixer: MixedBatchFixture,
        })
    };
    let count = |r: &FixReport, applied: bool| {
        r.results
            .iter()
            .flat_map(|r| &r.items)
            .filter(|i| matches!(i.status, FixStatus::Applied(_)) == applied)
            .count()
    };
    // Within ONE pass (a dry run is single-pass): the real edit reports
    // Applied, the identity edit a per-edit no-op Skipped.
    let preview = Engine::new(vec![mixed()], RuleRegistry::new())
        .fix(
            tmp.path(),
            &idx(&["a.txt"]),
            &crate::WalkOptions::default(),
            true,
            Applicability::Safe,
        )
        .unwrap();
    assert_eq!(
        count(&preview, true),
        1,
        "only the real edit is Applied; the identity edit is a per-edit no-op"
    );
    assert_eq!(
        count(&preview, false),
        1,
        "the identity edit is downgraded to Skipped"
    );
    let report = Engine::new(vec![mixed()], RuleRegistry::new())
        .fix(
            tmp.path(),
            &idx(&["a.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();
    // The real edit landed; the identity edit changed nothing.
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"X1234567"
    );
    // The fixture rule flags every `.txt` file unconditionally, so the
    // converged pass still sees the 0..1 edit's key: the violation STANDS, so
    // the earlier Applied must not survive as a success (audit 2026-10
    // finding 1: never "applied" while the violation stands).
    assert_eq!(
        count(&report, true),
        0,
        "an Applied whose violation the converged pass still sees is demoted"
    );
    assert!(report.has_unfixable_errors());
    let items: Vec<_> = report.results.iter().flat_map(|r| &r.items).collect();
    // Pre-req 1: two edits on one file get DISTINCT keys (range-keyed), not a
    // single `(rule, path)` that could lock-mask one behind the other.
    let keys: std::collections::HashSet<String> = items
        .iter()
        .map(|it| Engine::violation_key("mixed", &it.violation))
        .collect();
    assert_eq!(
        keys.len(),
        2,
        "two located edits on one file must have distinct violation_keys"
    );
    // The identity edit is not read as progress, so the fixpoint converges.
    assert!(
        !report.non_convergent,
        "a mixed real+identity batch converges (the identity edit is a no-op)"
    );
}

#[test]
fn located_regime_applies_batch_and_excludes_isolation_group() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"01234567").unwrap();
    let engine = Engine::new(vec![located_rule()], RuleRegistry::new());
    let run = |dry_run: bool| {
        engine
            .fix(
                tmp.path(),
                &idx(&["a.txt"]),
                &crate::WalkOptions::default(),
                dry_run,
                Applicability::Safe,
            )
            .unwrap()
    };
    let count = |r: &FixReport, applied: bool| {
        r.results
            .iter()
            .flat_map(|r| &r.items)
            .filter(|i| matches!(i.status, FixStatus::Applied(_)) == applied)
            .count()
    };
    // Within ONE pass (dry run): the first edit of the group applies, the
    // second (same isolation group) is a conflict skip.
    let preview = run(true);
    assert_eq!(count(&preview, true), 1);
    assert_eq!(count(&preview, false), 1);
    let report = run(false);
    // First edit (0..1 -> X) applied; the second (same group) is a conflict.
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"X1234567"
    );
    // The fixture rule flags the file unconditionally, so the converged pass
    // still sees both edits' keys: nothing may be reported Applied while the
    // violation stands (audit 2026-10 finding 1), and both items remain.
    assert_eq!(count(&report, true), 0);
    assert_eq!(count(&report, false), 2);
    // The batch applies on pass 1 and the identity re-collect on pass 2 nets
    // no change (the no-op downgrade), so the loop CONVERGES -- it must not
    // churn to the cap.
    assert!(
        !report.non_convergent,
        "the located batch converges; it must not be flagged non-convergent"
    );
}

#[test]
fn located_identity_edit_reports_skip_and_converges() {
    // Gate for the located no-op downgrade (`engine/fix.rs` `batch_changed`): a
    // located batch that nets NO byte change (every edit is an identity
    // rewrite) must report `Skipped`, not `Applied`, so `applied()==0` and the
    // byte-level fixpoint converges. Without the downgrade the identity edit
    // reports `Applied` -> `applied()>0` -> re-walk forever -> the cap fires
    // (spurious non_convergent). Here the file already reads "X123Y567", so the
    // fixture's edits (0..1->"X", 4..5->"Y") are both identities.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"X123Y567").unwrap();
    let report = Engine::new(vec![located_rule()], RuleRegistry::new())
        .fix(
            tmp.path(),
            &idx(&["a.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();
    assert!(
        !report.non_convergent,
        "an all-identity located batch must converge (no-op), not cap"
    );
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"X123Y567",
        "an identity batch must leave the file byte-identical"
    );
    let items: Vec<_> = report.results.iter().flat_map(|r| &r.items).collect();
    assert!(
        !items.is_empty()
            && items
                .iter()
                .all(|i| matches!(i.status, FixStatus::Skipped { .. })),
        "every identity edit reports Skipped, none Applied"
    );
}

#[test]
fn located_regime_honors_the_size_guard_on_the_collect_step() {
    // A file over the fix_size_limit is not edited (invariant 4), AND the
    // violation on it must be REPORTED as a size-skip -- not silently dropped.
    // The whole-file path surfaces the same skip; without parity a large file
    // with a forbidden pattern would pass `fix` (exit 0) while `check` (which
    // reads to a far larger cap) flags it -- an audit-found false negative.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"01234567").unwrap();
    let engine =
        Engine::new(vec![located_rule()], RuleRegistry::new()).with_fix_size_limit(Some(4));
    let report = engine
        .fix(
            tmp.path(),
            &idx(&["a.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"01234567",
        "over-limit file must be left untouched"
    );
    // The size-skipped violation is reported exactly once, as a Skip (so it
    // drives the exit code), and the loop converges (nothing applies).
    let items: Vec<_> = report.results.iter().flat_map(|r| &r.items).collect();
    assert_eq!(
        items.len(),
        1,
        "the size-skipped violation is reported once"
    );
    assert!(
        matches!(items[0].status, FixStatus::Skipped { .. }),
        "reported as a Skip, not Applied or silently dropped"
    );
    assert!(
        !report.non_convergent,
        "a size-skip converges: nothing applies"
    );
}

#[test]
fn located_regime_dry_run_reports_but_does_not_write() {
    // R-audit-4: the located application must NOT write in --dry-run (a dry
    // run has no compose buffer, so an unguarded commit_write would hit
    // disk). It still reports the outcome, as "would edit".
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"01234567").unwrap();
    let engine = Engine::new(vec![located_rule()], RuleRegistry::new());
    let report = engine
        .fix(
            tmp.path(),
            &idx(&["a.txt"]),
            &crate::WalkOptions::default(),
            /* dry_run */ true,
            Applicability::Safe,
        )
        .unwrap();
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"01234567",
        "dry-run must not touch the file"
    );
    // Still reported: one "would edit" Applied + one conflict Skipped.
    let items: Vec<_> = report.results.iter().flat_map(|r| &r.items).collect();
    assert!(
        items
            .iter()
            .any(|i| matches!(&i.status, FixStatus::Applied(s) if s.starts_with("would edit")))
    );
    assert_eq!(
        items
            .iter()
            .filter(|i| matches!(i.status, FixStatus::Skipped { .. }))
            .count(),
        1
    );
}

#[test]
fn located_regime_threshold_gates_unsafe_edits_through_the_engine() {
    // The threshold reaches located_fix through the engine: an Unsafe edit
    // is a Suggestion under a Safe threshold (not written), and applies once
    // the caller opts into Unsafe.
    let mk = || {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.txt"), b"01234567").unwrap();
        tmp
    };

    // Safe threshold: both Unsafe edits are Suggestions; file untouched.
    let tmp = mk();
    let report = Engine::new(
        vec![located_rule_with(Applicability::Unsafe)],
        RuleRegistry::new(),
    )
    .fix(
        tmp.path(),
        &idx(&["a.txt"]),
        &crate::WalkOptions::default(),
        false,
        Applicability::Safe,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"01234567"
    );
    let items: Vec<_> = report.results.iter().flat_map(|r| &r.items).collect();
    assert_eq!(
        items
            .iter()
            .filter(|i| matches!(i.status, FixStatus::Suggested { .. }))
            .count(),
        2,
        "both Unsafe edits are suggestions below the Safe threshold"
    );

    // Unsafe threshold: the batch applies (one edit; the other is an
    // isolation conflict), and the file is written.
    let tmp = mk();
    Engine::new(
        vec![located_rule_with(Applicability::Unsafe)],
        RuleRegistry::new(),
    )
    .fix(
        tmp.path(),
        &idx(&["a.txt"]),
        &crate::WalkOptions::default(),
        false,
        Applicability::Unsafe,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"X1234567"
    );
}

#[test]
fn stage_fixes_composes_without_writing() {
    // stage_fixes (for --diff) runs the compose pass but does NOT flush:
    // it returns the composed (old, new) per file, and disk is untouched.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"01234567").unwrap();
    let engine = Engine::new(vec![located_rule()], RuleRegistry::new());
    let (_report, staged) = engine
        .stage_fixes(tmp.path(), &idx(&["a.txt"]), Applicability::Safe)
        .unwrap();
    // Disk is unchanged: nothing was flushed.
    assert_eq!(
        std::fs::read(tmp.path().join("a.txt")).unwrap(),
        b"01234567",
        "stage_fixes must not write"
    );
    // One staged change carries old + the composed new bytes.
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].path, std::path::Path::new("a.txt"));
    assert_eq!(staged[0].old, b"01234567");
    assert_eq!(staged[0].new, b"X1234567");
    // A content edit is an in-place modify.
    assert_eq!(staged[0].kind, StagedKind::Modify);
}

// ---- Fixture for the byte-level fixpoint: convergence + cap (Phase 1) ----
//
// A rule whose fix always applies (appends a byte) and never self-guards. With
// `stop_at: None` it never stops -> the pure non-convergent config the cap
// exists to bound. With `stop_at: Some(n)` it stops firing once the file
// reaches n bytes -> a fixer that makes PROGRESS over several passes and then
// converges, which is exactly the multi-pass case a premature "one apply per
// file" cutoff would have wrongly abandoned. The violation is PATHLESS with a
// constant message; termination is byte-level (a pass that changes nothing
// converges), so the message only groups report items, it does not gate the
// loop.
#[derive(Debug)]
struct GrowFixer {
    target: PathBuf,
}
impl crate::rule::Fixer for GrowFixer {
    fn describe(&self) -> String {
        "append one byte".to_string()
    }
    fn apply(&self, _v: &Violation, ctx: &FixContext<'_>) -> crate::error::Result<FixOutcome> {
        // Direct append: each application grows the file by one byte, so a pass
        // that runs it changes the tree (the loop's progress signal). Honour
        // `dry_run` like a real fixer -- report "would apply" but do NOT write
        // (the fixpoint's non-mutating cap-confirmation pass relies on this).
        if !ctx.dry_run {
            let mut bytes = std::fs::read(&self.target).unwrap_or_default();
            bytes.push(b'x');
            std::fs::write(&self.target, &bytes).unwrap();
        }
        Ok(FixOutcome::Applied("grew by one byte".to_string()))
    }
    fn fix_edit(&self, _v: &Violation, _bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        None
    }
}

#[derive(Debug)]
struct GrowRule {
    id: String,
    fixer: GrowFixer,
    // `None`: always violates -> the fixer applies every pass -> the cap fires.
    // `Some(n)`: violates only while the file is under n bytes, so the fixer
    // makes progress for n passes and then the pass finds nothing to do and
    // the loop converges.
    stop_at: Option<usize>,
}
impl Rule for GrowRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn path_scope(&self) -> Option<&Scope> {
        None
    }
    fn evaluate(&self, _ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let len = std::fs::read(&self.fixer.target).map_or(0, |b| b.len());
        if self.stop_at.is_some_and(|n| len >= n) {
            return Ok(Vec::new()); // resolved: nothing left to grow
        }
        Ok(vec![Violation::new("keeps growing")])
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(&self.fixer)
    }
}

#[derive(Debug)]
struct LengthFailureRule {
    target: PathBuf,
    fail_when: fn(usize) -> bool,
}

impl Rule for LengthFailureRule {
    fn id(&self) -> &'static str {
        "length-failure"
    }

    fn level(&self) -> Level {
        Level::Warning
    }

    fn path_scope(&self) -> Option<&Scope> {
        None
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let len = std::fs::read(&self.target).map_or(0, |bytes| bytes.len());
        if !(self.fail_when)(len) {
            return Ok(Vec::new());
        }
        Ok(vec![
            Violation::new("length-dependent analysis failed")
                .as_analysis_failure(AnalysisFailureKind::Evaluation),
        ])
    }
}

fn grow_engine(target: &Path, stop_at: Option<usize>) -> Engine {
    Engine::new(
        vec![Box::new(GrowRule {
            id: "grow".into(),
            fixer: GrowFixer {
                target: target.to_path_buf(),
            },
            stop_at,
        })],
        RuleRegistry::new(),
    )
}

#[derive(Debug)]
struct NotFixableRule {
    fixer: GrowFixer,
}
impl Rule for NotFixableRule {
    fn id(&self) -> &'static str {
        "nf"
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn path_scope(&self) -> Option<&Scope> {
        None
    }
    fn evaluate(&self, _ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        Ok(vec![Violation::new("stuck").with_not_fixable()])
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(&self.fixer)
    }
}

#[derive(Debug)]
struct NoteOnlyRule {
    fixer: GrowFixer,
}

impl Rule for NoteOnlyRule {
    fn id(&self) -> &'static str {
        "note-only"
    }

    fn level(&self) -> Level {
        Level::Error
    }

    fn evaluate(&self, _ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        Ok(vec![Violation::new("informational").as_note()])
    }

    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(&self.fixer)
    }
}

#[test]
fn fix_never_applies_a_fixer_to_a_note() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("grow.txt");
    std::fs::write(&target, b"").unwrap();
    let engine = Engine::new(
        vec![Box::new(NoteOnlyRule {
            fixer: GrowFixer {
                target: target.clone(),
            },
        })],
        RuleRegistry::new(),
    );

    let report = engine
        .fix(
            tmp.path(),
            &idx(&["grow.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();
    assert!(report.results.is_empty());
    assert_eq!(std::fs::read(target).unwrap(), b"");
}

#[test]
fn fixer_backed_note_is_never_marked_fixable() {
    let fixer = GrowFixer {
        target: PathBuf::from("unused"),
    };
    let (findings, rule_has_fixer) = mark_fixability(
        vec![Violation::new("informational").as_note()],
        Some(&fixer),
    );
    assert!(rule_has_fixer);
    assert!(!findings[0].is_fixable);
}

#[test]
fn a_not_fixable_finding_is_unfixable_in_check_and_fix() {
    // A finding the rule marks `not_fixable` (e.g. a UTF-16 BOM, a bidi
    // control in a binary file) is not tagged fixable by `check`, and `fix`
    // reports it `Unfixable` WITHOUT invoking the fixer (it used to call the
    // fixer and list a "skipped"), so the two verdicts agree.
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("grow.txt");
    std::fs::write(&target, b"").unwrap();
    let engine = Engine::new(
        vec![Box::new(NotFixableRule {
            fixer: GrowFixer {
                target: target.clone(),
            },
        })],
        RuleRegistry::new(),
    );
    let report = engine.run(tmp.path(), &idx(&["grow.txt"])).unwrap();
    assert!(!report.results[0].violations[0].is_fixable);
    let fix = engine
        .fix(
            tmp.path(),
            &idx(&["grow.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Unsafe,
        )
        .unwrap();
    assert_eq!(fix.unfixable(), 1);
    assert_eq!(fix.skipped() + fix.applied(), 0);
    assert!(
        std::fs::read(&target).unwrap().is_empty(),
        "the fixer must not run on a not-fixable finding"
    );
}

fn grow_fix(target: &Path, stop_at: Option<usize>) -> FixReport {
    std::fs::write(target, b"").unwrap();
    let root = target.parent().unwrap();
    grow_engine(target, stop_at)
        .fix(
            root,
            &idx(&["grow.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap()
}

#[test]
fn fix_bails_at_the_cap_on_a_nonconvergent_config() {
    // A config whose fix keeps changing the tree without settling must not loop
    // forever: the byte-level fixpoint hard-stops at MAX_PASSES (10) and flags
    // non-convergence (which the CLI maps to exit 2). Whatever landed before the
    // cap stays on disk -- the cap stops the loop, it does not roll back.
    // (docs/design/v0.17/fixpoint.md 2, 5.)
    let tmp = tempfile::tempdir().unwrap();
    let report = grow_fix(&tmp.path().join("grow.txt"), /* stop_at */ None);
    assert!(
        report.non_convergent,
        "a config that never settles must hit the cap and report non-convergence"
    );
    assert_eq!(
        std::fs::read(tmp.path().join("grow.txt")).unwrap().len(),
        10,
        "exactly MAX_PASSES (10) applications land before the cap stops the loop"
    );
}

#[test]
fn fix_converges_after_several_progressing_passes() {
    // The property the removed apply-once heuristic used to break: a fixer that
    // makes progress on the SAME file over MULTIPLE passes must be allowed to
    // keep going until it is done, not cut off after the first apply. Here the
    // fixer appends until the file reaches 3 bytes, then the rule stops firing;
    // the loop must CONVERGE (not cap) with the file fully grown. A "one apply
    // per file" cutoff would have stopped at 1 byte and either left the job
    // half-done or (if it still reported success) hidden it -- the F1 bug class.
    let tmp = tempfile::tempdir().unwrap();
    let report = grow_fix(&tmp.path().join("grow.txt"), /* stop_at */ Some(3));
    assert!(
        !report.non_convergent,
        "a fixer that finishes in a few progressing passes must converge, not cap"
    );
    assert_eq!(
        std::fs::read(tmp.path().join("grow.txt")).unwrap().len(),
        3,
        "the fix must run to completion across passes (3 bytes), not stop after one"
    );
    assert_eq!(
        report.applied(),
        1,
        "the multi-pass progressive fix on one violation is reported EXACTLY once \
             (Applied-wins locks the key; passes 2-3 do not duplicate it)"
    );
}

#[test]
fn fix_converges_exactly_at_the_pass_cap() {
    // Regression for the audit finding that a slow-but-convergent config which
    // reaches its fixed point on the LAST budgeted pass was branded
    // non-convergent (exit 2) even though the tree is CLEAN -- there was no
    // room in the budget for the confirming `applied()==0` pass. The
    // non-mutating dry-run cap-confirmation must recognise convergence here.
    // `stop_at == MAX_PASSES` needs exactly the whole budget of applying
    // passes and then is done.
    let tmp = tempfile::tempdir().unwrap();
    let report = grow_fix(&tmp.path().join("grow.txt"), /* stop_at */ Some(10));
    assert!(
        !report.non_convergent,
        "a config that reaches its fixed point exactly at the cap must NOT be \
             branded non-convergent (the tree is clean)"
    );
    assert_eq!(
        std::fs::read(tmp.path().join("grow.txt")).unwrap().len(),
        10,
        "the fix ran to completion (10 bytes); the dry-run confirmation did not write"
    );
}

#[test]
fn cap_confirmation_installs_new_failures_and_keeps_rule_order() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("grow.txt");
    std::fs::write(&target, b"").unwrap();
    let engine = Engine::new(
        vec![
            Box::new(LengthFailureRule {
                target: target.clone(),
                fail_when: |len| len >= 10,
            }),
            Box::new(GrowRule {
                id: "grow".into(),
                fixer: GrowFixer {
                    target: target.clone(),
                },
                stop_at: Some(10),
            }),
        ],
        RuleRegistry::new(),
    );

    let report = engine
        .fix(
            tmp.path(),
            &idx(&["grow.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();

    assert!(!report.non_convergent);
    assert_eq!(report.total_analysis_failures(), 1);
    assert_eq!(
        report
            .results
            .iter()
            .map(|result| result.rule_id.as_ref())
            .collect::<Vec<_>>(),
        ["length-failure", "grow"],
        "a result first seen during confirmation still follows configured rule order"
    );
}

#[test]
fn cap_confirmation_removes_failure_resolved_by_last_write() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("grow.txt");
    std::fs::write(&target, b"").unwrap();
    let engine = Engine::new(
        vec![
            Box::new(LengthFailureRule {
                target: target.clone(),
                fail_when: |len| len == 9,
            }),
            Box::new(GrowRule {
                id: "grow".into(),
                fixer: GrowFixer {
                    target: target.clone(),
                },
                stop_at: Some(10),
            }),
        ],
        RuleRegistry::new(),
    );

    let report = engine
        .fix(
            tmp.path(),
            &idx(&["grow.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();

    assert!(!report.non_convergent);
    assert_eq!(report.total_analysis_failures(), 0);
}

#[test]
fn cap_confirm_preserves_a_standing_unfixable_at_the_boundary() {
    // At the cap-confirmation boundary a GENUINE unfixable that still stands
    // must survive the residual reconciliation. `grow` reaches its fixed point
    // exactly at the cap (converged via the dry-run confirm), while a no-fixer
    // error rule leaves an unfixable standing on another file -- the confirm's
    // `final_keys` must include it, else it is dropped and `fix` exits 0 on a
    // dirty tree (false negative). Gates the confirm-path `final_keys`
    // population, which `fix_converges_exactly_at_the_pass_cap` (a clean tree,
    // Applied-only) does not exercise.
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("grow.txt");
    std::fs::write(&target, b"").unwrap();
    std::fs::write(tmp.path().join("keep.rs"), b"x").unwrap();
    let engine = Engine::new(
        vec![
            Box::new(GrowRule {
                id: "grow".into(),
                fixer: GrowFixer {
                    target: target.clone(),
                },
                stop_at: Some(10),
            }),
            stub("nofix", "**/*.rs"), // Error level, no fixer -> Unfixable
        ],
        RuleRegistry::new(),
    );
    let report = engine
        .fix(
            tmp.path(),
            &idx(&["grow.txt", "keep.rs"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();
    assert!(
        !report.non_convergent,
        "grow reaches its fixed point at the cap; the run is not non-convergent"
    );
    assert!(
        report.has_unfixable_errors(),
        "the standing no-fixer error must survive reconciliation on the confirm path"
    );
    assert_eq!(std::fs::read(&target).unwrap().len(), 10);
}

// A rule that is its own fixer: it fires while `file` (relative to the run
// root) exists, and its fixer either ERRORS (a fix I/O error) or DELETES the
// file. Used to reach the exotic case the reconciliation's write-error
// carve-out guards: a fix error on a file that ANOTHER rule then removes.
#[derive(Debug)]
enum FixAction {
    Err,
    Delete,
}
#[derive(Debug)]
struct ActionRule {
    id: String,
    file: PathBuf, // relative to the run root
    action: FixAction,
}
impl crate::rule::Fixer for ActionRule {
    fn describe(&self) -> String {
        "test action".to_string()
    }
    fn apply(&self, _v: &Violation, ctx: &FixContext<'_>) -> crate::error::Result<FixOutcome> {
        match self.action {
            FixAction::Err => Err(crate::error::Error::Other("simulated fix I/O error".into())),
            FixAction::Delete => {
                std::fs::remove_file(ctx.root.join(&self.file)).ok();
                Ok(FixOutcome::Applied("deleted".to_string()))
            }
        }
    }
    fn fix_edit(&self, _v: &Violation, _bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        None
    }
}
impl Rule for ActionRule {
    fn id(&self) -> &str {
        &self.id
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn path_scope(&self) -> Option<&Scope> {
        None
    }
    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        if ctx.root.join(&self.file).exists() {
            Ok(vec![Violation::new("stands").with_path(self.file.clone())])
        } else {
            Ok(Vec::new()) // resolved once the file is gone
        }
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(self)
    }
}

#[test]
fn reconciliation_drops_a_moot_fix_error_when_the_file_is_removed() {
    // A fix that errored on X because ANOTHER rule removed X is MOOT: X is
    // gone, the tree is in the desired state, and `check` on it is clean. So
    // `fix` must also converge clean (exit 0), NOT strand a phantom I/O error
    // on the deleted file. A PERSISTENT error (X still standing) would instead
    // stay in `final_keys` and be kept -- that path is exercised by
    // fix_robustness.rs. Here `errs` errors on x.txt while `removes` deletes
    // it, so x.txt leaves the converged residual and the error is reconciled
    // away.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("x.txt"), b"hi").unwrap();
    let engine = Engine::new(
        vec![
            Box::new(ActionRule {
                id: "errs".into(),
                file: PathBuf::from("x.txt"),
                action: FixAction::Err,
            }),
            Box::new(ActionRule {
                id: "removes".into(),
                file: PathBuf::from("x.txt"),
                action: FixAction::Delete,
            }),
        ],
        RuleRegistry::new(),
    );
    let report = engine
        .fix(
            tmp.path(),
            &idx(&["x.txt"]),
            &crate::WalkOptions::default(),
            false,
            Applicability::Safe,
        )
        .unwrap();
    assert!(
        !report.non_convergent,
        "x.txt is removed -> the rules stop firing -> converges"
    );
    assert!(
        !report.had_fix_error(),
        "a fix error on a file another rule REMOVED is moot and must be dropped (fix agrees with check: exit 0)"
    );
    assert!(
        !report.has_unfixable_errors(),
        "no residual on the clean tree"
    );
}
