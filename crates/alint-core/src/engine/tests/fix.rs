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

// ---- `fix --dry-run` predicts the creates a real fix demotes ----

/// Requires `checks` and creates `path` (a file with `content`, or a directory
/// when `dir`), as `file_create` and `dir_create` do: under the root joined
/// with the lexically normalized path, and only when nothing is there yet.
// Each flag mirrors one rule option a test turns on.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug)]
struct CreateRule {
    id: &'static str,
    checks: PathBuf,
    path: PathBuf,
    content: &'static [u8],
    dir: bool,
    /// The rule's `paths:`; without one it looks for `checks` as written, as
    /// `dir_exists` and `cross_file` do.
    scope: Option<Scope>,
    /// Also looks for `checks` on disk, as `file_exists` does for a literal
    /// path with `respect_gitignore: false`.
    past_walk: bool,
    /// Sees only git-tracked files, as with `git_tracked_only`.
    tracked_only: bool,
    /// Creates the file only in a directory that exists, as `file_create`
    /// with `create_parents: false` does.
    needs_directory: bool,
}

fn create_rule(
    id: &'static str,
    checks: &str,
    path: &str,
    content: &'static [u8],
    dir: bool,
) -> Box<dyn Rule> {
    Box::new(CreateRule {
        id,
        checks: PathBuf::from(checks),
        path: PathBuf::from(path),
        content,
        dir,
        scope: Some(Scope::from_patterns(&[checks.to_string()]).unwrap()),
        past_walk: false,
        tracked_only: false,
        needs_directory: false,
    })
}

/// A `file_create` of `path` for a rule with no `paths:` scope that requires
/// `checks`.
fn scopeless_create_rule(id: &'static str, checks: &str, path: &str) -> Box<dyn Rule> {
    Box::new(CreateRule {
        id,
        checks: PathBuf::from(checks),
        path: PathBuf::from(path),
        content: b"x",
        dir: false,
        scope: None,
        past_walk: false,
        tracked_only: false,
        needs_directory: false,
    })
}

/// A `file_create` of `path` for a rule that looks past the walk for it, or
/// sees only tracked files.
fn create_rule_seeing(
    id: &'static str,
    path: &str,
    past_walk: bool,
    tracked_only: bool,
) -> Box<dyn Rule> {
    Box::new(CreateRule {
        id,
        checks: PathBuf::from(path),
        path: PathBuf::from(path),
        content: b"x",
        dir: false,
        scope: Some(Scope::from_patterns(&[path.to_string()]).unwrap()),
        past_walk,
        tracked_only,
        needs_directory: false,
    })
}

/// A `file_create` of `path` with `create_parents: false`.
fn create_rule_into_existing_directory(id: &'static str, path: &str) -> Box<dyn Rule> {
    Box::new(CreateRule {
        id,
        checks: PathBuf::from(path),
        path: PathBuf::from(path),
        content: b"x",
        dir: false,
        scope: Some(Scope::from_patterns(&[path.to_string()]).unwrap()),
        past_walk: false,
        tracked_only: false,
        needs_directory: true,
    })
}

impl crate::rule::Fixer for CreateRule {
    fn describe(&self) -> String {
        format!("create {}", self.path.display())
    }
    fn apply(&self, _v: &Violation, ctx: &FixContext<'_>) -> crate::error::Result<FixOutcome> {
        let target = ctx.root.join(
            crate::pathsafe::normalize_confined(&self.path).unwrap_or_else(|| self.path.clone()),
        );
        if target.exists() {
            return Ok(FixOutcome::Skipped("already there".to_string()));
        }
        let previewing = ctx.dry_run || ctx.stage_ops.is_some();
        if self.needs_directory && !previewing && !target.parent().unwrap().is_dir() {
            return Ok(FixOutcome::Skipped("no directory".to_string()));
        }
        if let Some(sink) = ctx.stage_ops {
            if !self.dir {
                sink.borrow_mut().push(FixEdit::CreateFile {
                    path: self.path.clone(),
                    content: self.content.to_vec(),
                });
            }
        } else if !ctx.dry_run {
            if self.dir {
                std::fs::create_dir_all(&target).unwrap();
            } else {
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(&target, self.content).unwrap();
            }
        }
        Ok(FixOutcome::Applied(format!(
            "create {}",
            self.path.display()
        )))
    }
    // Like `file_create`, no editor form for a target outside the root.
    fn fix_edit(&self, _v: &Violation, _bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        crate::pathsafe::normalize_confined(&self.path)?;
        (!self.dir).then(|| FixEdit::CreateFile {
            path: self.path.clone(),
            content: self.content.to_vec(),
        })
    }
    fn created_dir(&self) -> Option<&Path> {
        self.dir.then_some(self.path.as_path())
    }
    fn created_file(
        &self,
        _v: &Violation,
        _root: &Path,
        _allow_out_of_root: bool,
    ) -> Option<(PathBuf, Vec<u8>)> {
        (!self.dir).then(|| (self.path.clone(), self.content.to_vec()))
    }
    fn needs_existing_directory(&self) -> bool {
        self.needs_directory
    }
}

impl Rule for CreateRule {
    fn id(&self) -> &str {
        self.id
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn path_scope(&self) -> Option<&Scope> {
        self.scope.as_ref()
    }
    fn sees_path_past_walk(&self, path: &Path) -> bool {
        self.past_walk && path == self.checks
    }
    fn git_tracked_mode(&self) -> crate::GitTrackedMode {
        if self.tracked_only {
            crate::GitTrackedMode::FileOnly
        } else {
            crate::GitTrackedMode::Off
        }
    }
    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let found = ctx.index.contains_path(&self.checks)
            || (self.past_walk && ctx.root.join(&self.checks).is_file());
        Ok(if found {
            Vec::new()
        } else {
            vec![Violation::new("missing")]
        })
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(self)
    }
}

#[test]
fn a_preview_declines_a_create_into_a_missing_directory_only_when_nothing_makes_it() {
    // `fix` creates the file once another create has made its directory, in
    // the same pass or the next, so the dry run and `--diff` report it then,
    // whatever the rules' order; with no such create, every mode declines it.
    type Rules = fn() -> Vec<Box<dyn Rule>>;
    let cases: [(&str, Rules, bool); 5] = [
        (
            "alone",
            || vec![create_rule_into_existing_directory("needs", "docs/i.md")],
            false,
        ),
        (
            "directory first",
            || {
                vec![
                    create_rule("dir", "docs", "docs", b"", true),
                    create_rule_into_existing_directory("needs", "docs/i.md"),
                ]
            },
            true,
        ),
        (
            "directory after",
            || {
                vec![
                    create_rule_into_existing_directory("needs", "docs/i.md"),
                    create_rule("dir", "docs", "docs", b"", true),
                ]
            },
            true,
        ),
        (
            "parents of another file",
            || {
                vec![
                    create_rule("other", "docs/sub/o.md", "docs/sub/o.md", b"x", false),
                    create_rule_into_existing_directory("needs", "docs/i.md"),
                ]
            },
            true,
        ),
        (
            "another directory",
            || {
                vec![
                    create_rule("dir", "other", "other", b"", true),
                    create_rule_into_existing_directory("needs", "docs/i.md"),
                ]
            },
            false,
        ),
    ];
    let opts = crate::WalkOptions::default();
    for (case, rules, expected) in cases {
        let applied = |report: &FixReport| {
            let result = report
                .results
                .iter()
                .find(|r| &*r.rule_id == "needs")
                .unwrap();
            matches!(result.items[0].status, FixStatus::Applied(_))
        };
        for dry_run in [true, false] {
            let tmp = tempfile::tempdir().unwrap();
            let index = crate::walk(tmp.path(), &opts).unwrap();
            let report = Engine::new(rules(), RuleRegistry::new())
                .fix(tmp.path(), &index, &opts, dry_run, Applicability::Safe)
                .unwrap();
            assert_eq!(
                applied(&report),
                expected,
                "{case}, dry_run={dry_run}: {report:?}"
            );
            assert_eq!(
                tmp.path().join("docs/i.md").is_file(),
                expected && !dry_run,
                "{case}"
            );
        }
        let tmp = tempfile::tempdir().unwrap();
        let index = crate::walk(tmp.path(), &opts).unwrap();
        let (report, staged) = Engine::new(rules(), RuleRegistry::new())
            .stage_fixes(tmp.path(), &index, Applicability::Safe)
            .unwrap();
        assert_eq!(applied(&report), expected, "{case}, --diff");
        assert_eq!(
            staged.iter().any(|fix| fix.path == Path::new("docs/i.md")),
            expected,
            "{case}, --diff: {staged:?}"
        );
    }
}

#[test]
fn dry_run_predicts_each_create_the_real_fix_finds_unresolved() {
    // Each rule's dry-run outcome (applied or not) is the real fix's.
    let outcomes = |dry_run: bool| -> Vec<(String, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let root = &tmp.path().join("root");
        std::fs::create_dir_all(root.join("cache")).unwrap();
        std::fs::write(root.join(".gitignore"), "build/\nout/\n").unwrap();
        let rules = vec![
            // A created ignore file that leaves itself out.
            create_rule(
                "self-ignoring",
                "cache/.gitignore",
                "cache/.gitignore",
                b"*\n",
                false,
            ),
            // One that does not, and leaves out another create of the pass.
            create_rule("logs-ignore", ".ignore", ".ignore", b"*.log\n", false),
            create_rule("log", "x.log", "x.log", b"x", false),
            // A directory the root `.gitignore` leaves out.
            create_rule("build-dir", "build", "build", b"", true),
            // Written to `README.md`, though `out/` is ignored.
            create_rule("normalized", "README.md", "out/../README.md", b"x", false),
            create_rule("outside-paths", "b.txt", "a.txt", b"x", false),
            // Written outside the root, as `allow_out_of_root` permits.
            create_rule("outside-root", "escaped.txt", "../escaped.txt", b"x", false),
            create_rule("plain", "keep.txt", "keep.txt", b"x", false),
        ];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        report
            .results
            .iter()
            .map(|result| {
                assert_eq!(result.items.len(), 1, "{result:?}");
                let applied = matches!(result.items[0].status, FixStatus::Applied(_));
                (result.rule_id.to_string(), applied)
            })
            .collect()
    };
    let expected: Vec<(String, bool)> = [
        ("self-ignoring", false),
        ("logs-ignore", true),
        ("log", false),
        ("build-dir", false),
        ("normalized", true),
        ("outside-paths", false),
        ("outside-root", false),
        ("plain", true),
    ]
    .into_iter()
    .map(|(id, applied)| (id.to_string(), applied))
    .collect();
    assert_eq!(outcomes(true), expected, "dry run");
    assert_eq!(outcomes(false), expected, "real fix");
}

#[test]
fn dry_run_takes_the_first_of_two_creates_of_one_ignore_file() {
    // The real `fix` skips a create whose file is already there, so the first
    // `.gitignore` written is the one the walk reads and `x.txt` stays walked.
    let outcome = |dry_run: bool| -> bool {
        let tmp = tempfile::tempdir().unwrap();
        let root = &tmp.path().join("root");
        std::fs::create_dir_all(root).unwrap();
        let rules = vec![
            create_rule("first", ".gitignore", ".gitignore", b"other\n", false),
            create_rule("second", ".gitignore", ".gitignore", b"x.txt\n", false),
            create_rule("x", "x.txt", "x.txt", b"x", false),
        ];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        let x = report
            .results
            .iter()
            .find(|result| &*result.rule_id == "x")
            .unwrap();
        matches!(x.items[0].status, FixStatus::Applied(_))
    };
    assert!(outcome(false), "real fix");
    assert!(outcome(true), "dry run");
}

#[test]
fn dry_run_reads_an_absolute_create_inside_the_root_as_relative() {
    // `allow_out_of_root` lets a fixer take an absolute path, which may still
    // name a file in the tree; the walk indexes it there.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    let absolute = root.join("keep.txt").to_string_lossy().into_owned();
    let rules = vec![create_rule("absolute", "keep.txt", &absolute, b"x", false)];
    let opts = crate::WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = crate::walk(&root, &opts).unwrap();
    let report = Engine::new(rules, RuleRegistry::new())
        .fix(&root, &index, &opts, true, Applicability::Safe)
        .unwrap();
    assert!(
        matches!(report.results[0].items[0].status, FixStatus::Applied(_)),
        "{:?}",
        report.results[0].items[0].status
    );
}

#[test]
fn dry_run_trusts_a_rule_that_looks_past_the_walk() {
    // Both files are gitignored. The rule that also looks on disk finds the
    // created one anyway; the one that reads only the walk does not.
    let outcomes = |dry_run: bool| -> Vec<(String, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let root = &tmp.path().join("root");
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join(".gitignore"), "seen.txt\nunseen.txt\n").unwrap();
        let rules = vec![
            create_rule_seeing("past-walk", "seen.txt", true, false),
            create_rule_seeing("walk-only", "unseen.txt", false, false),
        ];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        report
            .results
            .iter()
            .map(|result| {
                let applied = matches!(result.items[0].status, FixStatus::Applied(_));
                (result.rule_id.to_string(), applied)
            })
            .collect()
    };
    let expected = vec![
        ("past-walk".to_string(), true),
        ("walk-only".to_string(), false),
    ];
    assert_eq!(outcomes(true), expected, "dry run");
    assert_eq!(outcomes(false), expected, "real fix");
}

#[test]
fn dry_run_resolves_a_tracked_only_rule_only_with_a_tracked_path() {
    // git still tracks `gone.txt`, deleted from the worktree; `new.txt` was
    // never added. A rule that sees only tracked files is resolved by a create
    // of the first, not of the second; a rule that sees the walk, by either.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    std::fs::write(root.join("gone.txt"), "x").unwrap();
    git(&["add", "gone.txt"]);
    git(&["commit", "-qm", "base"]);
    std::fs::remove_file(root.join("gone.txt")).unwrap();
    let rules = vec![
        create_rule_seeing("tracked-gone", "gone.txt", false, true),
        create_rule_seeing("tracked-new", "new.txt", false, true),
        create_rule_seeing("walked-new", "new.txt", false, false),
    ];
    let opts = crate::WalkOptions {
        respect_gitignore: true,
        extra_ignores: Vec::new(),
    };
    let index = crate::walk(root, &opts).unwrap();
    let report = Engine::new(rules, RuleRegistry::new())
        .fix(root, &index, &opts, true, Applicability::Safe)
        .unwrap();
    let applied: Vec<(String, bool)> = report
        .results
        .iter()
        .map(|result| {
            let applied = matches!(result.items[0].status, FixStatus::Applied(_));
            (result.rule_id.to_string(), applied)
        })
        .collect();
    assert_eq!(
        applied,
        [
            ("tracked-gone".to_string(), true),
            ("tracked-new".to_string(), false),
            ("walked-new".to_string(), true),
        ]
    );
}

#[cfg(unix)]
#[test]
#[allow(clippy::too_many_lines)]
fn dry_run_judges_a_create_by_where_it_lands() {
    // A create through a symlink, or by an absolute or `..` path, lands where
    // the filesystem resolves it; the walk lists it there, and under the path
    // as written when the walk enters that name.
    use std::os::unix::fs::symlink;
    let outcomes = |dry_run: bool| -> Vec<(String, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let root = &base.join("root");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(root.join("real/deep")).unwrap();
        std::fs::create_dir_all(root.join("hidden")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(root.join(".gitignore"), "alias\nhidden\nveiled\n").unwrap();
        // A loop the walk does not enter, but which leads back into the root.
        symlink("..", root.join("sub/up")).unwrap();
        // Ignored names for directories the walk does or does not list.
        symlink("real", root.join("alias")).unwrap();
        symlink("hidden", root.join("veiled")).unwrap();
        // A name for the root from outside it, and a way out of it.
        symlink("root", base.join("root-link")).unwrap();
        symlink("../outside", root.join("out")).unwrap();
        // A `..` after it steps up from `real/deep`, not back to the root.
        symlink("real/deep", root.join("deeplink")).unwrap();
        let path = |p: &Path| p.to_string_lossy().into_owned();
        let rules = vec![
            create_rule("loop", "x.txt", "sub/up/x.txt", b"x", false),
            create_rule("ignored-alias", "real/y.txt", "alias/y.txt", b"x", false),
            create_rule("dotted-alias", "real/z.txt", "./alias/z.txt", b"x", false),
            create_rule("ignored-both", "hidden/n.txt", "veiled/n.txt", b"x", false),
            create_rule(
                "absolute-via-link",
                "w.txt",
                &path(&base.join("root-link/w.txt")),
                b"x",
                false,
            ),
            create_rule(
                "absolute-dotdot",
                "v.txt",
                &path(&base.join("root/../root/v.txt")),
                b"x",
                false,
            ),
            create_rule("relative-dotdot", "u.txt", "../root/u.txt", b"x", false),
            create_rule("lands-outside", "out/o.txt", "out/o.txt", b"x", false),
            // Written as given, so the OS resolves `deeplink/..` to `real`.
            create_rule(
                "absolute-link-up",
                "t.txt",
                &path(&root.join("deeplink/../t.txt")),
                b"x",
                false,
            ),
            // Out and back in through a directory the create makes, then a
            // symlink: `new/..` cancels and `alias` still leads to `real`.
            create_rule(
                "back-in-alias",
                "real/s.txt",
                "../root/new/../alias/s.txt",
                b"x",
                false,
            ),
            create_rule(
                "back-in-link-up",
                "r.txt",
                "../root/new/../deeplink/../r.txt",
                b"x",
                false,
            ),
            // Written as given, and the `..` cancels a real directory, so the
            // walk lists the file under the symlink that follows.
            create_rule(
                "absolute-back-in-link",
                "deeplink/q.txt",
                &path(&root.join("sub/../deeplink/q.txt")),
                b"x",
                false,
            ),
        ];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        report
            .results
            .iter()
            .map(|result| {
                assert_eq!(result.items.len(), 1, "{result:?}");
                let applied = matches!(result.items[0].status, FixStatus::Applied(_));
                (result.rule_id.to_string(), applied)
            })
            .collect()
    };
    let expected: Vec<(String, bool)> = [
        ("loop", true),
        ("ignored-alias", true),
        ("dotted-alias", true),
        ("ignored-both", false),
        ("absolute-via-link", true),
        ("absolute-dotdot", true),
        ("relative-dotdot", true),
        ("lands-outside", false),
        ("absolute-link-up", false),
        ("back-in-alias", true),
        ("back-in-link-up", false),
        ("absolute-back-in-link", true),
    ]
    .into_iter()
    .map(|(id, applied)| (id.to_string(), applied))
    .collect();
    assert_eq!(outcomes(true), expected, "dry run");
    assert_eq!(outcomes(false), expected, "real fix");
}

#[cfg(unix)]
#[test]
fn dry_run_names_an_absolute_create_through_the_canonical_root() {
    // The root given through a symlink, and an absolute target written from
    // its canonical form through an in-tree symlink the walk follows: the walk
    // lists the file under the symlink's name, which only the canonical form
    // of the written path gives.
    use std::os::unix::fs::symlink;
    let outcome = |dry_run: bool| -> bool {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join("root/real")).unwrap();
        symlink("real", base.join("root/alias")).unwrap();
        symlink("root", base.join("root-link")).unwrap();
        let root = &base.join("root-link");
        let target = base.join("root/alias/x.txt").to_string_lossy().into_owned();
        let rules = vec![create_rule(
            "canonical",
            "alias/x.txt",
            &target,
            b"x",
            false,
        )];
        let opts = crate::WalkOptions {
            respect_gitignore: true,
            extra_ignores: Vec::new(),
        };
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        matches!(report.results[0].items[0].status, FixStatus::Applied(_))
    };
    assert!(outcome(false), "real fix");
    assert!(outcome(true), "dry run");
}

#[test]
#[cfg(unix)]
fn a_create_has_one_name_when_it_lands_where_it_is_written() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("real")).unwrap();
    symlink("real", root.join("alias")).unwrap();
    let names = |path: &str| Engine::created_names(root, Path::new(path));
    assert_eq!(names("real/x.txt"), Some(vec![PathBuf::from("real/x.txt")]));
    assert_eq!(
        names("./real/x.txt"),
        Some(vec![PathBuf::from("real/x.txt")])
    );
    assert_eq!(
        names("alias/x.txt"),
        Some(vec![
            PathBuf::from("alias/x.txt"),
            PathBuf::from("real/x.txt")
        ])
    );
    assert_eq!(names("../outside.txt"), None);
}

#[test]
fn engine_fix_warnings_show_a_hidden_file_as_redacted() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("dir")).unwrap();
    std::fs::write(tmp.path().join("dir/secret.txt"), "x").unwrap();
    std::fs::write(tmp.path().join("plain.txt"), "x").unwrap();
    let hidden: HashSet<PathBuf> = std::iter::once(Engine::normalized_write_target(
        tmp.path(),
        Path::new("dir/secret.txt"),
    ))
    .collect();
    // The same file under another spelling is still hidden.
    for spelling in [
        "dir/secret.txt",
        "./dir/secret.txt",
        "dir/../dir/secret.txt",
    ] {
        assert_eq!(
            Engine::shown_path(tmp.path(), &hidden, Path::new(spelling)),
            crate::CONFIGURATION_MASK,
            "{spelling}"
        );
    }
    assert_eq!(
        Engine::shown_path(tmp.path(), &hidden, Path::new("plain.txt")),
        "plain.txt"
    );
}

#[test]
fn dry_run_judges_a_rule_without_a_paths_scope_by_the_path_as_written() {
    // A rule with no `paths:` finds nothing under another name: a create
    // written out of the root and back in lands on `back.txt` but never
    // matches `../root/back.txt`, while `out/../keep.txt` reads as `keep.txt`.
    let outcomes = |dry_run: bool| -> Vec<(String, bool)> {
        let tmp = tempfile::tempdir().unwrap();
        let root = &tmp.path().join("root");
        std::fs::create_dir_all(root).unwrap();
        let rules = vec![
            scopeless_create_rule("back-in", "../root/back.txt", "../root/back.txt"),
            scopeless_create_rule("normalized", "keep.txt", "out/../keep.txt"),
        ];
        let opts = crate::WalkOptions::default();
        let index = crate::walk(root, &opts).unwrap();
        let report = Engine::new(rules, RuleRegistry::new())
            .fix(root, &index, &opts, dry_run, Applicability::Safe)
            .unwrap();
        report
            .results
            .iter()
            .map(|result| {
                let applied = matches!(result.items[0].status, FixStatus::Applied(_));
                (result.rule_id.to_string(), applied)
            })
            .collect()
    };
    let expected = vec![
        ("back-in".to_string(), false),
        ("normalized".to_string(), true),
    ];
    assert_eq!(outcomes(true), expected, "dry run");
    assert_eq!(outcomes(false), expected, "real fix");
}

/// Creates `../outside.txt` only when its rule's `allow_out_of_root` lets it
/// leave the root, with no editor form, as `file_create` does there.
#[derive(Debug)]
struct OutOfRootCreate;

impl crate::rule::Fixer for OutOfRootCreate {
    fn describe(&self) -> String {
        "create ../outside.txt".to_string()
    }
    fn apply(&self, _v: &Violation, _ctx: &FixContext<'_>) -> crate::error::Result<FixOutcome> {
        Ok(FixOutcome::Applied("create ../outside.txt".to_string()))
    }
    fn created_file(
        &self,
        _v: &Violation,
        _root: &Path,
        allow_out_of_root: bool,
    ) -> Option<(PathBuf, Vec<u8>)> {
        allow_out_of_root.then(|| (PathBuf::from("../outside.txt"), b"x".to_vec()))
    }
}

impl Rule for OutOfRootCreate {
    fn id(&self) -> &'static str {
        "escape"
    }
    fn level(&self) -> Level {
        Level::Error
    }
    fn evaluate(&self, _ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        Ok(vec![Violation::new("missing")])
    }
    fn fixer(&self) -> Option<&dyn crate::rule::Fixer> {
        Some(self)
    }
}

#[test]
fn diff_hides_a_sensitive_create_outside_the_root() {
    // `--diff` hides every file a sensitive item writes. A create out of the
    // root has no editor form, so the target comes from `created_file` under
    // the rule's own `allow_out_of_root`.
    let tmp = tempfile::tempdir().unwrap();
    let root = &tmp.path().join("root");
    std::fs::create_dir_all(root).unwrap();
    let report = |sensitive: bool| FixReport {
        results: vec![FixRuleResult {
            rule_id: "escape".into(),
            level: Level::Error,
            items: vec![FixItem {
                violation: if sensitive {
                    Violation::new("missing").with_redacted_message("escape")
                } else {
                    Violation::new("missing")
                },
                status: FixStatus::Applied("create ../outside.txt".to_string()),
            }],
            analysis_failures: Vec::new(),
        }],
        non_convergent: false,
    };
    let engine = |allow: bool| {
        Engine::from_entries(
            vec![RuleEntry::new(Box::new(OutOfRootCreate)).with_allow_out_of_root(allow)],
            RuleRegistry::new(),
        )
    };
    let outside = Engine::normalized_write_target(root, Path::new("../outside.txt"));
    assert_eq!(
        engine(true).sensitive_fix_targets(&report(true), root),
        std::iter::once(outside).collect::<HashSet<_>>()
    );
    assert!(
        engine(true)
            .sensitive_fix_targets(&report(false), root)
            .is_empty()
    );
    assert!(
        engine(false)
            .sensitive_fix_targets(&report(true), root)
            .is_empty()
    );
}

static PATH_SELECTION_SELECTORS: &[crate::SelectorMeta] = &[crate::SelectorMeta {
    name: std::borrow::Cow::Borrowed("targets.files"),
    selector_type: crate::SelectorType::Glob,
    rerootable: false,
}];

fn path_selection_meta(
    selectors: &'static [crate::SelectorMeta],
    nested_rules: bool,
) -> crate::KindMeta {
    crate::KindMeta {
        dispatch: crate::DispatchClass::FullIndex { path_scope: false },
        paths: crate::PathsSupport::Optional,
        scope_filter: crate::ScopeFilterSupport::Full,
        git_tracked_only: crate::GitTrackedSupport::None,
        respect_gitignore: false,
        root_only: false,
        selectors: std::borrow::Cow::Borrowed(selectors),
        since: crate::SinceSupport::None,
        capabilities: std::borrow::Cow::Borrowed(&[]),
        content: crate::ContentAccess::PathOnly,
        formats: std::borrow::Cow::Borrowed(&[]),
        dependencies: std::borrow::Cow::Borrowed(&[]),
        fix_ops: std::borrow::Cow::Borrowed(&[]),
        expect_matches: false,
        nested_rules,
        trace: crate::TraceMeta::SELECTION,
    }
}

#[test]
fn a_path_selection_read_from_the_environment_is_sensitive() {
    let mut registry = RuleRegistry::new();
    for (kind, selectors, nested) in [
        ("selecting", PATH_SELECTION_SELECTORS, false),
        ("nesting", &[][..], true),
        ("flat", &[][..], false),
    ] {
        registry.register_owned(
            kind,
            std::sync::Arc::new(path_selection_meta(selectors, nested)),
            |_| unreachable!("never built"),
        );
    }
    let entry = |kind: &str, field: Option<&str>| {
        let mut spec: crate::RuleSpec =
            yaml_serde::from_str(&format!("id: r\nkind: {kind}\nlevel: error\n")).unwrap();
        spec.set_sensitive_fields(field.map(str::to_string).into_iter().collect());
        RuleEntry::new(stub("r", "**/*.txt")).with_spec(std::sync::Arc::new(spec))
    };
    let cases = [
        (entry("flat", None), false),
        (entry("flat", Some("paths")), true),
        (entry("flat", Some("scope_filter")), true),
        (entry("flat", Some("message")), false),
        // `require:` and `when_iter:` choose the paths of a nested kind only.
        (entry("nesting", Some("require")), true),
        (entry("flat", Some("require")), false),
        (entry("nesting", Some("when_iter")), true),
        (entry("flat", Some("when_iter")), false),
        // The environment marks a selector's top-level field.
        (entry("selecting", Some("targets")), true),
        (entry("flat", Some("targets")), false),
        (RuleEntry::new(stub("r", "**/*.txt")), false),
    ];
    let expected: Vec<bool> = cases.iter().map(|(_, sensitive)| *sensitive).collect();
    let engine = Engine::from_entries(cases.into_iter().map(|(e, _)| e).collect(), registry);
    let actual: Vec<bool> = engine
        .entries
        .iter()
        .map(|entry| engine.has_sensitive_path_selection(entry))
        .collect();
    assert_eq!(actual, expected);
}
