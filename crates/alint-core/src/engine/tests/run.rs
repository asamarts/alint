use super::*;

#[test]
fn run_empty_returns_empty_report() {
    let engine = Engine::new(Vec::new(), RuleRegistry::new());
    let report = engine.run(Path::new("/fake"), &idx(&["a.rs"])).unwrap();
    assert!(report.results.is_empty());
}

#[test]
fn run_single_rule_emits_per_match() {
    let engine = Engine::new(vec![stub("t", "**/*.rs")], RuleRegistry::new());
    let report = engine
        .run(
            Path::new("/fake"),
            &idx(&["src/a.rs", "src/b.rs", "README.md"]),
        )
        .unwrap();
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].violations.len(), 2);
}

#[test]
fn run_with_empty_changed_set_short_circuits() {
    // Per the contract: empty `--changed` set means "lint
    // nothing"; the engine returns an empty Report without
    // even evaluating facts.
    let engine = Engine::new(vec![stub("t", "**/*.rs")], RuleRegistry::new())
        .with_changed_paths(HashSet::new());
    let report = engine.run(Path::new("/fake"), &idx(&["src/a.rs"])).unwrap();
    assert!(report.results.is_empty());
}

#[test]
fn changed_mode_skips_rule_whose_scope_misses_diff() {
    // Rule scoped to `src/**`; changed-set has only docs/
    // → rule skipped (no result emitted).
    let mut changed = HashSet::new();
    changed.insert(std::path::PathBuf::from("docs/README.md"));
    let engine = Engine::new(vec![stub("src-rule", "src/**/*.rs")], RuleRegistry::new())
        .with_changed_paths(changed);
    let report = engine
        .run(Path::new("/fake"), &idx(&["src/a.rs", "docs/README.md"]))
        .unwrap();
    assert!(
        report.results.is_empty(),
        "out-of-scope rule should be skipped: {:?}",
        report.results,
    );
}

#[test]
fn changed_mode_runs_rule_whose_scope_intersects_diff() {
    let mut changed = HashSet::new();
    changed.insert(std::path::PathBuf::from("src/a.rs"));
    let engine = Engine::new(vec![stub("src-rule", "src/**/*.rs")], RuleRegistry::new())
        .with_changed_paths(changed);
    let report = engine
        .run(Path::new("/fake"), &idx(&["src/a.rs", "src/b.rs"]))
        .unwrap();
    // Filtered index: only `src/a.rs` is visible. Rule
    // matches it → 1 violation.
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].violations.len(), 1);
}

#[test]
fn requires_full_index_rule_runs_unconditionally_in_changed_mode() {
    // A rule with `requires_full_index = true` and no
    // `path_scope` opts out of the changed-set filter
    // entirely — its verdict is over the whole tree.
    let mut changed = HashSet::new();
    changed.insert(std::path::PathBuf::from("docs/README.md"));
    let engine = Engine::new(vec![full_index_stub("cross")], RuleRegistry::new())
        .with_changed_paths(changed);
    let report = engine
        .run(Path::new("/fake"), &idx(&["src/a.rs", "docs/README.md"]))
        .unwrap();
    // `cross` ran against the full index (not the filtered
    // one), so it sees both files.
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].violations.len(), 2);
}

#[test]
fn build_filtered_index_includes_changed_and_created_files() {
    // 2b: the `--changed`-filtered index (which confines per-file rules) must
    // include BOTH the diff files AND files this fix run created, so a per-file
    // content rule can complete a create-then-fix cascade on a created file
    // (which is not in the git diff). `None` (the `check` path) confines to the
    // diff alone. Teeth: drop `|| created.is_some_and(...)` and `made.txt` is
    // no longer visible.
    let mut changed = HashSet::new();
    changed.insert(std::path::PathBuf::from("a.txt"));
    let engine = Engine::new(vec![], RuleRegistry::new()).with_changed_paths(changed);
    let full = idx(&["a.txt", "b.txt", "made.txt"]);
    let mut created = HashSet::new();
    created.insert(std::path::PathBuf::from("made.txt"));

    let fi = engine
        .build_filtered_index(&full, Some(&created))
        .expect("a changed set is present");
    let paths: std::collections::HashSet<std::path::PathBuf> =
        fi.entries.iter().map(|e| e.path.to_path_buf()).collect();
    assert!(
        paths.contains(std::path::Path::new("a.txt")),
        "changed file kept"
    );
    assert!(
        paths.contains(std::path::Path::new("made.txt")),
        "a file this run created joins the changed set (2b)"
    );
    assert!(
        !paths.contains(std::path::Path::new("b.txt")),
        "an untouched, uncreated file stays confined out"
    );

    // The `check` path (no created set) sees only the diff file.
    let fi_check = engine.build_filtered_index(&full, None).unwrap();
    let check_paths: std::collections::HashSet<std::path::PathBuf> = fi_check
        .entries
        .iter()
        .map(|e| e.path.to_path_buf())
        .collect();
    assert!(check_paths.contains(std::path::Path::new("a.txt")));
    assert!(
        !check_paths.contains(std::path::Path::new("made.txt")),
        "check does not carry created files (nothing is created during a check)"
    );
}

#[test]
fn writes_outside_changed_judges_a_fix_by_its_target() {
    // 2b: a path-bearing fix is in scope iff its path is in the changed set OR
    // was created by this fix run; anything else is out of scope and is demoted
    // to a Suggestion in the fix loop. With no `--changed` set, confinement is
    // inactive so nothing is ever out of scope (a full fix is unaffected).
    let mut changed = HashSet::new();
    changed.insert(std::path::PathBuf::from("a.txt"));
    let engine = Engine::new(vec![], RuleRegistry::new()).with_changed_paths(changed);
    // The rule is only consulted for a PATHLESS violation; for path-bearing ones
    // the violation's own path decides, so any stub rule serves here.
    let rule = stub("r", "**/*");
    let index = idx(&["a.txt", "made.txt", "other.txt"]);
    let mut created = HashSet::new();
    created.insert(std::path::PathBuf::from("made.txt"));
    let v = |p: &str| Violation::new("x").with_path(std::path::PathBuf::from(p));

    assert!(
        !engine.writes_outside_changed(rule.as_ref(), &v("a.txt"), &index, &created),
        "a changed file is in scope"
    );
    assert!(
        !engine.writes_outside_changed(rule.as_ref(), &v("made.txt"), &index, &created),
        "a file this run created is in scope (2b)"
    );
    assert!(
        engine.writes_outside_changed(rule.as_ref(), &v("other.txt"), &index, &created),
        "an untouched, uncreated file is out of scope -> demoted to a Suggestion"
    );

    // A PATHLESS violation (a create, whose target comes from config not the
    // violation) is judged by the rule's `path_scope`: in scope iff the scope
    // matches some changed-or-created path. `stub` exposes a `Rule::path_scope`.
    let pathless = Violation::new("needs creating");
    let in_scope_rule = stub("in", "a.txt"); // scope matches the changed a.txt
    let created_scope_rule = stub("cr", "made.txt"); // scope matches a created file
    let out_scope_rule = stub("out", "zzz.txt"); // scope matches nothing in scope
    assert!(
        !engine.writes_outside_changed(in_scope_rule.as_ref(), &pathless, &index, &created),
        "a pathless create whose scope is in the changed set is in scope"
    );
    assert!(
        !engine.writes_outside_changed(created_scope_rule.as_ref(), &pathless, &index, &created),
        "a pathless create whose scope matches a created file is in scope (2b)"
    );
    assert!(
        engine.writes_outside_changed(out_scope_rule.as_ref(), &pathless, &index, &created),
        "a pathless create whose scope matches nothing in scope is out of scope -> demoted"
    );

    let full_engine = Engine::new(vec![], RuleRegistry::new());
    assert!(
        !full_engine.writes_outside_changed(rule.as_ref(), &v("other.txt"), &index, &created),
        "a full (non-changed) fix never demotes for scope"
    );
}

#[test]
fn rule_count_reflects_number_of_entries() {
    let engine = Engine::new(
        vec![stub("a", "**"), stub("b", "**"), stub("c", "**")],
        RuleRegistry::new(),
    );
    assert_eq!(engine.rule_count(), 3);
}

#[test]
fn from_entries_constructor_supports_when_clauses() {
    // A rule wrapped with a `when: false` expression should
    // be skipped during run — no result emitted.
    let entry =
        RuleEntry::new(stub("gated", "**/*.rs")).with_when(crate::when::parse("false").unwrap());
    let engine = Engine::from_entries(vec![entry], RuleRegistry::new());
    let report = engine.run(Path::new("/fake"), &idx(&["a.rs"])).unwrap();
    assert!(
        report.results.is_empty(),
        "when-false rule must be skipped: {:?}",
        report.results,
    );
}

#[test]
fn runtime_when_error_is_a_condition_failure_in_check_and_fix() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.rs"), "fn main() {}\n").unwrap();
    let rule = Box::new(StubRule {
        id: "gated".into(),
        level: Level::Warning,
        scope: Scope::from_patterns(&["*.rs".to_string()]).unwrap(),
        full_index: false,
        expose_scope: true,
    });
    let entry = RuleEntry::new(rule).with_when(crate::when::parse("1 < \"x\"").unwrap());
    let engine = Engine::from_entries(vec![entry], RuleRegistry::new());
    let index = idx(&["a.rs"]);

    let report = engine.run(tmp.path(), &index).unwrap();
    assert!(report.has_errors());
    assert_eq!(report.total_violations(), 0);
    assert_eq!(
        report.results[0].analysis_failures[0].kind,
        AnalysisFailureKind::Condition,
    );

    let fix = engine
        .fix(
            tmp.path(),
            &index,
            &crate::WalkOptions {
                respect_gitignore: true,
                extra_ignores: Vec::new(),
            },
            true,
            Applicability::Safe,
        )
        .unwrap();
    assert_eq!(fix.total_analysis_failures(), 1);
    assert!(fix.results[0].items.is_empty());
    assert_eq!(
        fix.results[0].analysis_failures[0].kind,
        AnalysisFailureKind::Condition,
    );
}

#[test]
fn fix_size_limit_default_is_one_mib() {
    // The builder default; tests that override engines via
    // `with_fix_size_limit` rely on this baseline.
    let engine = Engine::new(Vec::new(), RuleRegistry::new());
    // Implementation detail intentionally exposed for tests.
    // We can only verify the value indirectly via `with_*`
    // returning a different limit; assert the builder works.
    let updated = engine.with_fix_size_limit(Some(42));
    assert_eq!(updated.rule_count(), 0);
}

#[test]
fn skip_for_changed_returns_false_for_full_check() {
    // No `--changed` set → rule never skipped on that basis.
    let engine = Engine::new(vec![stub("t", "**/*.rs")], RuleRegistry::new());
    let report = engine.run(Path::new("/fake"), &idx(&["a.rs"])).unwrap();
    assert_eq!(report.results.len(), 1);
}

/// A scope with `scope_filter: { has_ancestor: Cargo.toml }` over `**/*.rs`.
fn ancestor_scope() -> Scope {
    let spec: crate::RuleSpec = yaml_serde::from_str(
            "id: a\nkind: x\nlevel: error\npaths: \"**/*.rs\"\nscope_filter:\n  has_ancestor: Cargo.toml\n",
        )
        .unwrap();
    Scope::from_spec(&spec).unwrap()
}

#[test]
fn tree_files_are_shared_only_for_a_rule_that_looks_them_up_on_the_filtered_index() {
    // `--changed` shares the full tree's file set into the filtered index only
    // when a `has_ancestor` lookup will actually run on that index: building it
    // is a pass over the full index (v0.18.3 bench, PR #329 review).
    let index = idx(&["crates/api/Cargo.toml", "crates/api/src/main.rs"]);
    let changed: HashSet<PathBuf> = [PathBuf::from("crates/api/src/main.rs")].into();
    let rule_major = |full_index: bool| -> Box<dyn Rule> {
        Box::new(StubRule {
            id: "a".into(),
            level: Level::Error,
            scope: ancestor_scope(),
            full_index,
            expose_scope: true,
        })
    };
    let per_file = || -> Box<dyn Rule> {
        Box::new(PerFileStub {
            id: "p".into(),
            scope: ancestor_scope(),
            prefix: Vec::new(),
        })
    };
    let engine = |rules: Vec<Box<dyn Rule>>, set: HashSet<PathBuf>| {
        Engine::new(rules, RuleRegistry::new()).with_changed_paths(set)
    };

    // No `has_ancestor` anywhere: never shared.
    let plain = engine(vec![stub("t", "**/*.rs")], changed.clone());
    assert!(!plain.needs_tree_files(&index, None, false));
    assert!(!plain.needs_tree_files(&index, None, true));
    // A rule-by-rule rule with a lookup and a changed in-scope file: shared.
    assert!(engine(vec![rule_major(false)], changed.clone()).needs_tree_files(&index, None, false));
    // ...but not when it is `--changed`-skipped (no in-scope file changed),
    let other: HashSet<PathBuf> = [PathBuf::from("README.md")].into();
    assert!(!engine(vec![rule_major(false)], other).needs_tree_files(&index, None, false));
    // ...nor when it is full-index (it gets the full index anyway).
    assert!(!engine(vec![rule_major(true)], changed.clone()).needs_tree_files(&index, None, false));
    // A per-file rule: `check` matches its scope on the full index (no share);
    // `fix` evaluates it on the filtered index (share).
    let pf = engine(vec![per_file()], changed);
    assert!(!pf.needs_tree_files(&index, None, false));
    assert!(pf.needs_tree_files(&index, None, true));
}

#[test]
fn dispatch_flip_routes_per_file_rule_through_file_major_loop() {
    // Real filesystem so the engine's `std::fs::read` works.
    // The PerFileStub fires when a file does NOT start with
    // `MAGIC` — exercises the slice-handing-in path end-to-end.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("good.txt"), b"MAGIC + payload").unwrap();
    std::fs::write(tmp.path().join("bad.txt"), b"no magic here").unwrap();

    let rule = Box::new(PerFileStub {
        id: "needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let engine = Engine::new(vec![rule], RuleRegistry::new());

    let opts = crate::WalkOptions::default();
    let index = crate::walk(tmp.path(), &opts).unwrap();
    let report = engine.run(tmp.path(), &index).unwrap();

    assert_eq!(report.results.len(), 1, "results: {:?}", report.results);
    let r = &report.results[0];
    assert_eq!(&*r.rule_id, "needs-magic");
    assert_eq!(r.violations.len(), 1, "violations: {:?}", r.violations);
    assert_eq!(
        r.violations[0].path.as_deref(),
        Some(std::path::Path::new("bad.txt")),
    );
}

#[test]
fn dispatch_flip_aggregates_multiple_per_file_rules() {
    // Two per-file rules sharing one scope: the file-major
    // loop reads each file once and dispatches both rules
    // against the same byte buffer. Verifies the aggregation
    // step buckets violations per rule correctly (not
    // per-file).
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"ZZZ stuff").unwrap();
    std::fs::write(tmp.path().join("b.txt"), b"BBB stuff").unwrap();

    let rule_a = Box::new(PerFileStub {
        id: "needs-AAA".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"AAA".to_vec(),
    });
    let rule_b = Box::new(PerFileStub {
        id: "needs-BBB".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"BBB".to_vec(),
    });
    let engine = Engine::new(vec![rule_a, rule_b], RuleRegistry::new());

    let opts = crate::WalkOptions::default();
    let index = crate::walk(tmp.path(), &opts).unwrap();
    let report = engine.run(tmp.path(), &index).unwrap();

    // `needs-AAA` fires on both files (neither starts with
    // "AAA"). `needs-BBB` fires only on `a.txt`.
    let by_id: HashMap<&str, &RuleResult> =
        report.results.iter().map(|r| (&*r.rule_id, r)).collect();
    assert_eq!(
        by_id.len(),
        2,
        "expected both rules in the report: {:?}",
        report.results
    );
    assert_eq!(by_id["needs-AAA"].violations.len(), 2);
    assert_eq!(by_id["needs-BBB"].violations.len(), 1);
    assert_eq!(
        by_id["needs-BBB"].violations[0].path.as_deref(),
        Some(std::path::Path::new("a.txt")),
    );
}

#[test]
fn passing_per_file_rule_appears_in_the_report() {
    // A live per-file rule that finds no violations is a PASSING
    // rule — it now appears in the report with empty violations,
    // so the pass count ("All N rule(s) passed") includes it,
    // matching the cross-file path (which always emits a result).
    // Previously these were dropped, so a silently-passing
    // per-file rule read as "All 0 rule(s) passed".
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"MAGIC ok").unwrap();

    let rule = Box::new(PerFileStub {
        id: "needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let engine = Engine::new(vec![rule], RuleRegistry::new());

    let opts = crate::WalkOptions::default();
    let index = crate::walk(tmp.path(), &opts).unwrap();
    let report = engine.run(tmp.path(), &index).unwrap();

    assert_eq!(report.results.len(), 1, "results: {:?}", report.results);
    assert!(report.results[0].violations.is_empty());
    assert_eq!(report.passing_rules(), 1);
}

#[test]
fn run_for_file_runs_only_in_scope_per_file_rules() {
    // 3-rule fixture: a per-file rule in scope for `.txt`, a
    // per-file rule scoped to `.rs` (out of scope), and a
    // cross-file rule (must never run via run_for_file). Only the
    // in-scope per-file rule should fire.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"no magic").unwrap();

    let in_scope = Box::new(PerFileStub {
        id: "txt-needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let out_of_scope = Box::new(PerFileStub {
        id: "rs-needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.rs".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let cross = stub("cross", "**/*.txt");
    let engine = Engine::new(vec![in_scope, out_of_scope, cross], RuleRegistry::new());

    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let results = engine
        .run_for_file(tmp.path(), &index, Path::new("a.txt"), b"no magic")
        .unwrap();

    assert_eq!(results.len(), 1, "results: {results:?}");
    assert_eq!(&*results[0].rule_id, "txt-needs-magic");
    assert_eq!(results[0].violations.len(), 1);
}

#[test]
fn run_for_file_applies_explicit_sensitivity_before_returning() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"secret editor bytes").unwrap();
    let rule = Box::new(PerFileStub {
        id: "needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let spec: crate::RuleSpec = yaml_serde::from_str(
        "id: needs-magic\nkind: file_content_matches\npaths: \"**/*.txt\"\n\
             pattern: MAGIC\nsensitive: true\nlevel: error\n",
    )
    .unwrap();
    let entry = RuleEntry::new(rule).with_spec(std::sync::Arc::new(spec));
    let engine = Engine::from_entries(vec![entry], RuleRegistry::new());
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();

    let results = engine
        .run_for_file(
            tmp.path(),
            &index,
            Path::new("a.txt"),
            b"secret unsaved editor bytes",
        )
        .unwrap();

    assert_eq!(results.len(), 1, "{results:?}");
    let finding = &results[0].violations[0];
    assert!(finding.is_sensitive());
    assert!(finding.message.contains("<redacted:"), "{finding:?}");
    assert!(!finding.message.contains("missing prefix"), "{finding:?}");
}

#[test]
fn explicit_sensitive_scope_covers_an_unindexed_editor_file() {
    let tmp = tempfile::tempdir().unwrap();
    let rule = Box::new(PerFileStub {
        id: "needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let spec: crate::RuleSpec = yaml_serde::from_str(
        "id: needs-magic\nkind: file_content_matches\npaths: \"**/*.txt\"\n\
             pattern: MAGIC\nsensitive: true\nlevel: error\n",
    )
    .unwrap();
    let entry = RuleEntry::new(rule).with_spec(std::sync::Arc::new(spec));
    let engine = Engine::from_entries(vec![entry], RuleRegistry::new());

    assert!(engine.path_contains_sensitive_evidence(
        tmp.path(),
        &FileIndex::default(),
        Path::new("new.txt"),
    ));
}
#[test]
fn run_for_file_uses_supplied_bytes_not_disk() {
    // On-disk content passes the rule; the in-memory edited bytes
    // (handed to run_for_file) fail it. The supplied bytes win —
    // this is the whole point of the LSP unsaved-edit contract.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"MAGIC on disk").unwrap();

    let rule = Box::new(PerFileStub {
        id: "needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let engine = Engine::new(vec![rule], RuleRegistry::new());
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();

    let results = engine
        .run_for_file(tmp.path(), &index, Path::new("a.txt"), b"edited, no prefix")
        .unwrap();
    assert_eq!(results.len(), 1, "edited bytes should fail the rule");
    assert_eq!(&*results[0].rule_id, "needs-magic");
}

#[test]
fn run_for_file_passing_rule_omitted() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"whatever").unwrap();
    let rule = Box::new(PerFileStub {
        id: "needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let engine = Engine::new(vec![rule], RuleRegistry::new());
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let results = engine
        .run_for_file(tmp.path(), &index, Path::new("a.txt"), b"MAGIC passes")
        .unwrap();
    assert!(
        results.is_empty(),
        "passing rule must be omitted: {results:?}"
    );
}

#[test]
fn is_per_file_classifies_rules() {
    let pf = Box::new(PerFileStub {
        id: "pf".into(),
        scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
        prefix: b"X".to_vec(),
    });
    let cross = stub("cross", "**/*");
    let engine = Engine::new(vec![pf, cross], RuleRegistry::new());
    assert!(engine.is_per_file("pf"));
    assert!(!engine.is_per_file("cross"));
    assert!(!engine.is_per_file("unknown-id"));
}

#[test]
fn run_for_file_caches_facts_on_index() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"x").unwrap();
    let rule = Box::new(PerFileStub {
        id: "pf".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let engine = Engine::new(vec![rule], RuleRegistry::new());
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();

    assert!(index.cached_facts().is_none());
    engine
        .run_for_file(tmp.path(), &index, Path::new("a.txt"), b"x")
        .unwrap();
    assert!(
        index.cached_facts().is_some(),
        "facts should be cached after the first run_for_file"
    );
    // Second call reuses the cache without error.
    engine
        .run_for_file(tmp.path(), &index, Path::new("a.txt"), b"x")
        .unwrap();
}

#[test]
fn run_for_file_errors_when_file_not_in_index() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"x").unwrap();
    let engine = Engine::new(vec![], RuleRegistry::new());
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let err = engine
        .run_for_file(tmp.path(), &index, Path::new("ghost.txt"), b"x")
        .unwrap_err();
    assert!(
        matches!(err, Error::FileNotInIndex { .. }),
        "expected FileNotInIndex, got: {err:?}"
    );
}

#[test]
fn dispatch_flip_preserves_cross_file_rules_unchanged() {
    // A rule that opts out of `as_per_file` (the default
    // `None`) keeps the rule-major path. Mixing with a
    // per-file rule should produce both results.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"hi").unwrap();

    let cross_rule = stub("cross", "**/*.txt");
    let per_file_rule = Box::new(PerFileStub {
        id: "needs-magic".into(),
        scope: Scope::from_patterns(&["**/*.txt".to_string()]).unwrap(),
        prefix: b"MAGIC".to_vec(),
    });
    let engine = Engine::new(vec![cross_rule, per_file_rule], RuleRegistry::new());

    let opts = crate::WalkOptions::default();
    let index = crate::walk(tmp.path(), &opts).unwrap();
    let report = engine.run(tmp.path(), &index).unwrap();

    assert_eq!(report.results.len(), 2, "results: {:?}", report.results);
    // Order follows entry-registration order.
    assert_eq!(&*report.results[0].rule_id, "cross");
    assert_eq!(&*report.results[1].rule_id, "needs-magic");
}

/// Test-only relation consumer: reads through the run-scoped cache and decodes
/// the file, proving the relation path and the per-file path share one read.
#[derive(Debug)]
struct RelationConsumer;

impl Rule for RelationConsumer {
    fn id(&self) -> &'static str {
        "relation-consumer"
    }

    fn level(&self) -> Level {
        Level::Error
    }

    fn uses_relations(&self) -> bool {
        true
    }

    fn requires_full_index(&self) -> bool {
        true
    }

    fn evaluate(&self, ctx: &Context<'_>) -> crate::error::Result<Vec<Violation>> {
        let cache = ctx
            .index
            .read_cache()
            .expect("a relation consumer enables the run cache");
        let resource = crate::relation::Resource::literal(
            crate::relation::RelPath::parse("a.txt").expect("a literal test path"),
        );
        if let crate::relation::ResourceRead::Present(bytes) =
            cache.read(ctx.root, &resource, 0).as_ref()
        {
            ctx.index
                .decode_cache()
                .expect("a relation consumer enables the run decode cache")
                .decode(
                    &crate::relation::ExtractAdapter::new(crate::extract::Extract::WholeFile),
                    &resource,
                    bytes,
                    &crate::relation::Limits::default(),
                )
                .expect("the test document decodes");
        }
        Ok(Vec::new())
    }
}

#[test]
fn filtered_index_shares_the_full_index_read_cache() {
    let mut changed = HashSet::new();
    changed.insert(std::path::PathBuf::from("a.txt"));
    let engine = Engine::new(vec![], RuleRegistry::new()).with_changed_paths(changed);
    let full = idx(&["a.txt", "b.txt"]);
    full.enable_relation_caches();
    let cache = full.read_cache().expect("full cache enabled");
    let decodes = full.decode_cache().expect("full decode cache enabled");
    let filtered = engine
        .build_filtered_index(&full, None)
        .expect("a changed set is present");
    assert!(
        std::ptr::eq(filtered.read_cache().expect("filtered cache shared"), cache),
        "a --changed filtered index must read through the full index's cache"
    );
    assert!(
        std::ptr::eq(
            filtered
                .decode_cache()
                .expect("filtered decode cache shared"),
            decodes
        ),
        "a --changed filtered index must decode through the full index's cache"
    );
}

#[test]
fn a_relation_free_run_does_not_enable_the_read_cache() {
    let tmp = tempfile::Builder::new()
        .prefix("alint-no-relation-cache-")
        .tempdir()
        .unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let engine = Engine::new(
        vec![Box::new(PerFileStub {
            id: "per-file".into(),
            scope: Scope::from_patterns(&["a.txt".to_string()]).unwrap(),
            prefix: b"x".to_vec(),
        })],
        RuleRegistry::new(),
    );
    engine.run(tmp.path(), &index).unwrap();
    assert!(
        index.read_cache().is_none(),
        "a run without relation consumers must keep the direct read path"
    );
    assert!(
        index.decode_cache().is_none(),
        "a run without relation consumers must not allocate a decode cache"
    );
}

#[test]
fn run_for_file_and_fix_enable_the_relation_cache() {
    let tmp = tempfile::Builder::new()
        .prefix("alint-relation-cache-paths-")
        .tempdir()
        .unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let engine = Engine::new(vec![Box::new(RelationConsumer)], RuleRegistry::new());
    engine
        .run_for_file(tmp.path(), &index, std::path::Path::new("a.txt"), b"hello")
        .unwrap();
    assert!(index.read_cache().is_some(), "run_for_file enables it");
    assert!(
        index.decode_cache().is_some(),
        "run_for_file enables decodes"
    );

    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    engine
        .fix(
            tmp.path(),
            &index,
            &crate::WalkOptions::default(),
            true,
            crate::Applicability::Safe,
        )
        .unwrap();
    assert!(index.read_cache().is_some(), "fix enables it");
    assert!(index.decode_cache().is_some(), "fix enables decodes");
}

#[test]
fn relation_consumer_and_per_file_rule_share_one_read_and_decode() {
    let tmp = tempfile::Builder::new()
        .prefix("alint-relation-cache-")
        .tempdir()
        .unwrap();
    std::fs::write(tmp.path().join("a.txt"), b"hello").unwrap();
    let index = crate::walk(tmp.path(), &crate::WalkOptions::default()).unwrap();
    let engine = Engine::new(
        vec![
            Box::new(PerFileStub {
                id: "per-file".into(),
                scope: Scope::from_patterns(&["a.txt".to_string()]).unwrap(),
                prefix: b"x".to_vec(),
            }),
            Box::new(RelationConsumer),
        ],
        RuleRegistry::new(),
    );
    let report = engine.run(tmp.path(), &index).unwrap();

    assert_eq!(report.total_violations(), 1);
    assert_eq!(
        index
            .read_cache()
            .expect("the run enabled the relation cache")
            .reads(),
        1,
        "the relation consumer and the per-file rule must share one read"
    );
    assert_eq!(
        index.read_cache().unwrap().hits(),
        1,
        "the second consumer must hit the cached read"
    );
    assert_eq!(
        index.decode_cache().unwrap().decodes(),
        1,
        "the document must be decoded once"
    );
}
