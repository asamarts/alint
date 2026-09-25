use super::*;

fn rule(comparator: Comparator, unique: bool) -> OrderedBlockRule {
    OrderedBlockRule {
        id: "t".into(),
        level: Level::Warning,
        policy_url: None,
        message: None,
        scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
        start: Some("# keep-sorted start".into()),
        end: Some("# keep-sorted end".into()),
        comparator,
        unique,
        select: None,
        require: Vec::new(),
        fixer: None,
    }
}

fn markerless_rule(
    start: Option<&str>,
    end: Option<&str>,
    comparator: Comparator,
) -> OrderedBlockRule {
    OrderedBlockRule {
        id: "t".into(),
        level: Level::Warning,
        policy_url: None,
        message: None,
        scope: Scope::from_patterns(&["**/*".to_string()]).unwrap(),
        start: start.map(Into::into),
        end: end.map(Into::into),
        comparator,
        unique: false,
        select: None,
        require: Vec::new(),
        fixer: None,
    }
}

fn eval(r: &OrderedBlockRule, text: &str) -> Vec<Violation> {
    let ctx = Context {
        root: Path::new("/"),
        index: &alint_core::FileIndex::from_entries(Vec::new()),
        registry: None,
        facts: None,
        vars: None,
        git_tracked: None,
        git_blame: None,
    };
    r.evaluate_file(&ctx, Path::new("f.txt"), text.as_bytes())
        .unwrap()
}

#[test]
fn sorted_block_passes() {
    let t = "x\n# keep-sorted start\nalpha\nbravo\ncharlie\n# keep-sorted end\ny\n";
    assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
}

#[test]
fn unsorted_block_fails_once_at_the_offending_line() {
    let t = "# keep-sorted start\nalpha\ncharlie\nbravo\ndelta\n# keep-sorted end\n";
    let v = eval(&rule(Comparator::Lexical, false), t);
    assert_eq!(v.len(), 1, "one violation per block: {v:?}");
    // `bravo` (line 4) is out of order after `charlie`.
    assert_eq!(v[0].line, Some(4));
    assert!(v[0].message.contains("bravo"));
}

#[test]
fn absent_markers_in_delimited_mode_is_silent() {
    // A delimited rule (both markers set) over a file that contains
    // NEITHER marker forms no block -> silent. (Markerless mode is
    // covered by `markerless_sorts_the_whole_file`.)
    let t = "just\nsome\nunsorted\nlines\nz\na\n";
    assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
}

#[test]
fn unique_flags_duplicate_only_when_set() {
    let t = "# keep-sorted start\nalpha\nalpha\nbravo\n# keep-sorted end\n";
    // Non-decreasing: a duplicate is fine without `unique`.
    assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
    let v = eval(&rule(Comparator::Lexical, true), t);
    assert_eq!(v.len(), 1);
    assert!(v[0].message.contains("duplicate"));
}

#[test]
fn lexical_ci_and_numeric_comparators() {
    // Bravo < alpha lexically (uppercase), but ci-sorted.
    let ci = "# keep-sorted start\nalpha\nBravo\ncharlie\n# keep-sorted end\n";
    assert!(eval(&rule(Comparator::LexicalCi, false), ci).is_empty());
    // Numeric: "9" before "10" (lexical would flip them).
    let num = "# keep-sorted start\n2\n9\n10\n100\n# keep-sorted end\n";
    assert!(eval(&rule(Comparator::Numeric, false), num).is_empty());
    let bad = "# keep-sorted start\n10\n9\n# keep-sorted end\n";
    assert_eq!(eval(&rule(Comparator::Numeric, false), bad).len(), 1);
}

#[test]
fn multiple_blocks_checked_independently() {
    let t = "# keep-sorted start\na\nb\n# keep-sorted end\nmid\n\
                 # keep-sorted start\nz\nq\n# keep-sorted end\n";
    let v = eval(&rule(Comparator::Lexical, false), t);
    assert_eq!(v.len(), 1, "only the 2nd block (z, q) is unsorted: {v:?}");
}

#[test]
fn markerless_sorts_the_whole_file() {
    // No start/end: every line is a sortable entry.
    let sorted = "alpha\nbravo\ncharlie\n";
    assert!(eval(&markerless_rule(None, None, Comparator::Lexical), sorted).is_empty());
    let unsorted = "banana\napple\ncherry\n";
    let v = eval(&markerless_rule(None, None, Comparator::Lexical), unsorted);
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].line, Some(2), "apple (line 2) is out of order");
    assert!(v[0].message.contains("apple"));
}

#[test]
fn start_only_sorts_to_eof_and_is_never_unclosed() {
    // `start` but no `end`: sort from the marker to EOF; an open
    // block at EOF is intentional, not an "unclosed" violation.
    let r = markerless_rule(Some("# sorted below"), None, Comparator::Lexical);
    let ok = "preamble\n# sorted below\nalpha\nbravo\n";
    assert!(
        eval(&r, ok).is_empty(),
        "no unclosed at EOF: {:?}",
        eval(&r, ok)
    );
    let bad = "# sorted below\nbravo\nalpha\n";
    let v = eval(&r, bad);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("alpha"));
}

#[test]
fn start_only_repeated_marker_reopens_a_fresh_block() {
    // C3: a repeated `start` in start-only mode delimits a NEW
    // section (re-opens), it is not flagged as out-of-order data.
    let r = markerless_rule(Some("# S"), None, Comparator::Lexical);
    // Two independently-sorted sections, the marker re-opening each.
    let ok = "# S\nalpha\nbravo\n# S\nyak\nzed\n";
    assert!(eval(&r, ok).is_empty(), "{:?}", eval(&r, ok));
    // Only the second section is unsorted -> exactly one violation,
    // anchored on the second section's start (the marker itself is
    // never reported as an entry).
    let bad = "# S\nalpha\nbravo\n# S\nzed\nyak\n";
    let v = eval(&r, bad);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("yak"));
    assert!(
        !v.iter().any(|x| x.message.contains("\"# S\"")),
        "marker must not be an entry"
    );
}

#[test]
fn delimited_repeated_start_flags_unclosed() {
    // In fully-delimited mode a 2nd `start` before the `end` means the
    // first block was never closed — flag it, don't silently swallow
    // it. The 2nd block here is properly closed.
    let t = "# keep-sorted start\na\n# keep-sorted start\nb\n# keep-sorted end\n";
    let v = eval(&rule(Comparator::Lexical, false), t);
    assert_eq!(v.len(), 1, "the first unterminated block is flagged: {v:?}");
    assert!(v[0].message.contains("unclosed"), "{}", v[0].message);
}

#[test]
fn end_only_sorts_from_bof_until_the_marker() {
    // No `start`, with `end`: the block opens at BOF and closes at
    // the end marker; lines after it are not entries.
    let r = markerless_rule(None, Some("# end"), Comparator::Lexical);
    let ok = "alpha\nbravo\n# end\nzeta\naardvark\n"; // post-`end` unsorted, ignored
    assert!(eval(&r, ok).is_empty());
    let bad = "bravo\nalpha\n# end\n";
    assert_eq!(eval(&r, bad).len(), 1);
}

#[test]
fn end_only_with_no_marker_present_sorts_to_eof() {
    // No `start`, `end` configured but the marker never appears:
    // the BOF-opened block runs to EOF (no "unclosed" — end is the
    // only marker, and start is absent).
    let r = markerless_rule(None, Some("# end"), Comparator::Lexical);
    assert!(eval(&r, "alpha\nbravo\ncherry\n").is_empty());
    assert_eq!(eval(&r, "cherry\nalpha\n").len(), 1);
}

#[test]
fn empty_and_all_blank_files_are_silent() {
    let r = markerless_rule(None, None, Comparator::Lexical);
    assert!(eval(&r, "").is_empty(), "empty file");
    assert!(eval(&r, "\n\n\n").is_empty(), "all-blank file");
}

#[test]
fn crlf_lines_sort_like_lf() {
    // `str::lines()` strips the trailing `\r`, so CRLF content
    // compares the same as LF.
    let r = markerless_rule(None, None, Comparator::Lexical);
    assert!(eval(&r, "apple\r\nbanana\r\ncherry\r\n").is_empty());
    assert_eq!(eval(&r, "banana\r\napple\r\n").len(), 1);
}

#[test]
fn unclosed_start_is_a_violation() {
    let t = "before\n# keep-sorted start\nalpha\nbravo\n";
    let v = eval(&rule(Comparator::Lexical, false), t);
    assert_eq!(v.len(), 1);
    assert!(v[0].message.contains("unclosed"));
    assert_eq!(v[0].line, Some(2));
}

#[test]
fn blank_lines_inside_a_block_are_ignored() {
    let t = "# keep-sorted start\nalpha\n\nbravo\n\ncharlie\n# keep-sorted end\n";
    assert!(eval(&rule(Comparator::Lexical, false), t).is_empty());
}

#[test]
fn select_sorts_only_matching_lines() {
    let mut r = rule(Comparator::Lexical, false);
    // Only `require '…'` lines must be sorted; other lines pass.
    r.select = Some(Regex::new(r"^\s*require ").unwrap());
    // The `require` lines are in order; the interleaved comments
    // and the out-of-order `gem` line are ignored.
    let ok = "# keep-sorted start\n\
                  require 'a'\n# a comment\nrequire 'b'\ngem 'z'\nrequire 'c'\n\
                  # keep-sorted end\n";
    assert!(eval(&r, ok).is_empty(), "{:?}", eval(&r, ok));
    // Out-of-order `require` lines fire even with non-require
    // lines interleaved.
    let bad = "# keep-sorted start\nrequire 'c'\ngem 'z'\nrequire 'a'\n# keep-sorted end\n";
    let v = eval(&r, bad);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].message.contains("require 'a'"));
}

#[test]
fn build_requires_paths() {
    use crate::test_support::spec_yaml;
    let no_paths = "id: t\nkind: ordered_block\nlevel: error\n";
    assert!(build(&spec_yaml(no_paths)).is_err(), "paths is required");
}

#[test]
fn build_rejects_an_empty_marker() {
    use crate::test_support::spec_yaml;
    let bad = "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: ''\nlevel: error\n";
    let err = build(&spec_yaml(bad)).unwrap_err();
    assert!(err.to_string().contains("must not be empty"), "{err}");
}

#[test]
fn build_rejects_identical_start_and_end_markers() {
    use crate::test_support::spec_yaml;
    let bad = "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: SAME\nend: SAME\nlevel: error\n";
    let err = build(&spec_yaml(bad)).unwrap_err();
    assert!(err.to_string().contains("must differ"), "{err}");
}

#[test]
fn build_rejects_invalid_select_regex() {
    use crate::test_support::spec_yaml;
    let bad = "id: t\nkind: ordered_block\npaths: [\"x\"]\nselect: '(unclosed'\nlevel: error\n";
    let err = build(&spec_yaml(bad)).unwrap_err();
    assert!(err.to_string().contains("select"), "{err}");
}

// ----- the `sort` fix -----------------------------------------------

fn sort_fixer(comparator: Comparator, unique: bool) -> OrderedBlockSortFixer {
    OrderedBlockSortFixer {
        start: Some("# keep-sorted start".into()),
        end: Some("# keep-sorted end".into()),
        comparator,
        unique,
        select: None,
        applicability: Applicability::Safe,
    }
}

/// A delimited rule WITH a `sort` fixer attached, so its findings are keyed
/// (the fixable-rule path). Mirrors `rule()`'s markers.
fn rule_with_sort(comparator: Comparator, unique: bool) -> OrderedBlockRule {
    let mut r = rule(comparator, unique);
    r.fixer = Some(Box::new(sort_fixer(comparator, unique)));
    r
}

/// A MARKERLESS rule with `require:` lines + an `insert_line` fixer.
fn rule_with_insert_line(require: &[&str]) -> OrderedBlockRule {
    let mut r = markerless_rule(None, None, Comparator::Lexical);
    r.require = require.iter().map(|s| s.trim().to_string()).collect();
    r.fixer = Some(Box::new(OrderedBlockInsertLineFixer {
        comparator: Comparator::Lexical,
        select: None,
        applicability: Applicability::Safe,
    }));
    r
}

fn markerless_sort_fixer(
    start: Option<&str>,
    end: Option<&str>,
    comparator: Comparator,
    unique: bool,
    select: Option<&str>,
) -> OrderedBlockSortFixer {
    OrderedBlockSortFixer {
        start: start.map(Into::into),
        end: end.map(Into::into),
        comparator,
        unique,
        select: select.map(|p| Regex::new(p).unwrap()),
        applicability: Applicability::Safe,
    }
}

#[test]
fn split_lines_round_trips_every_ending() {
    for text in [
        "",
        "a",
        "a\n",
        "a\nb",
        "a\nb\n",
        "a\r\nb\r\n",
        "a\r\nb\n",     // mixed
        "\n\n",         // blank lines
        "a\r",          // lone trailing CR (no LF) stays in body
        "# start\nx\n", // markers
    ] {
        let rebuilt: String = split_lines(text)
            .iter()
            .flat_map(|(b, e)| [*b, *e])
            .collect();
        assert_eq!(rebuilt, text, "round-trip failed for {text:?}");
    }
}

#[test]
fn split_lines_bodies_match_str_lines() {
    for text in ["a\nb\n", "a\nb", "\n", "", "a\r\nb", "x\r"] {
        let bodies: Vec<&str> = split_lines(text).iter().map(|(b, _)| *b).collect();
        let stdlib: Vec<&str> = text.lines().collect();
        assert_eq!(
            bodies, stdlib,
            "bodies diverge from str::lines() for {text:?}"
        );
    }
}

#[test]
fn sorted_reorders_a_delimited_block_leaving_surroundings() {
    let t = "head\n# keep-sorted start\ncharlie\nalpha\nbravo\n# keep-sorted end\ntail\n";
    let out = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
    assert_eq!(
        out,
        "head\n# keep-sorted start\nalpha\nbravo\ncharlie\n# keep-sorted end\ntail\n"
    );
}

#[test]
fn sorted_returns_none_when_already_sorted() {
    let t = "# keep-sorted start\nalpha\nbravo\n# keep-sorted end\n";
    assert!(sort_fixer(Comparator::Lexical, false).sorted(t).is_none());
}

#[test]
fn sorted_is_idempotent() {
    let t = "# keep-sorted start\ncharlie\nalpha\nbravo\n# keep-sorted end\n";
    let once = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
    // A second pass finds nothing to do (the engine's per-violation apply loop
    // relies on this to converge).
    assert!(
        sort_fixer(Comparator::Lexical, false)
            .sorted(&once)
            .is_none()
    );
}

#[test]
fn sorted_preserves_crlf_per_line() {
    let t = "# keep-sorted start\r\ncharlie\r\nalpha\r\n# keep-sorted end\r\n";
    let out = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
    assert_eq!(
        out,
        "# keep-sorted start\r\nalpha\r\ncharlie\r\n# keep-sorted end\r\n"
    );
}

#[test]
fn sorted_preserves_missing_final_newline() {
    // Markerless whole-file sort where the last entry has no terminator: the
    // file must still end without a newline (the ending is positional).
    let out = markerless_sort_fixer(None, None, Comparator::Lexical, false, None)
        .sorted("charlie\nalpha\nbravo")
        .unwrap();
    assert_eq!(out, "alpha\nbravo\ncharlie");
}

#[test]
fn sorted_numeric_comparator() {
    let t = "# keep-sorted start\n10\n2\n1\n# keep-sorted end\n";
    let out = sort_fixer(Comparator::Numeric, false).sorted(t).unwrap();
    assert_eq!(out, "# keep-sorted start\n1\n2\n10\n# keep-sorted end\n");
}

#[test]
fn leading_int_parses_beyond_i64() {
    // F3: u64-range identifiers (snowflakes / ns timestamps) must parse, so
    // they sort numerically rather than falling back to a wrong lexical order.
    assert_eq!(
        leading_int("9999999999999999999"),
        Some(9_999_999_999_999_999_999)
    );
    assert_eq!(
        leading_int("100000000000000000000"),
        Some(100_000_000_000_000_000_000)
    );
    assert_eq!(leading_int("-42x"), Some(-42));
    assert!(leading_int("abc").is_none());
    // A 40-digit integer overflows even i128 -> None (lexical fallback).
    assert!(leading_int(&"9".repeat(40)).is_none());
}

#[test]
fn sorted_numeric_orders_u64_range_ids() {
    // F3 end-to-end: `9999999999999999999` (~1e19) < `100000000000000000000`
    // (1e20) numerically. i64 parse fails on both -> the OLD code fell back to
    // lexical ("1…" < "9…") and left this "sorted"; i128 sorts it correctly.
    let t = "# keep-sorted start\n100000000000000000000\n9999999999999999999\n# keep-sorted end\n";
    let out = sort_fixer(Comparator::Numeric, false).sorted(t).unwrap();
    assert_eq!(
        out,
        "# keep-sorted start\n9999999999999999999\n100000000000000000000\n# keep-sorted end\n"
    );
}

#[test]
fn sorted_unique_drops_duplicate_slots() {
    let t = "# keep-sorted start\nbravo\nalpha\nbravo\n# keep-sorted end\n";
    let out = sort_fixer(Comparator::Lexical, true).sorted(t).unwrap();
    // One `bravo` remains; the block shrank by exactly the duplicate.
    assert_eq!(
        out,
        "# keep-sorted start\nalpha\nbravo\n# keep-sorted end\n"
    );
}

#[test]
fn sorted_unique_deleting_last_line_preserves_no_final_newline() {
    // F2: markerless `unique` where the deleted duplicate is the LAST physical
    // line (no terminator). The result must NOT gain a trailing newline.
    let lf = markerless_sort_fixer(None, None, Comparator::Lexical, true, None)
        .sorted("b\na\nb") // no final newline; `b` duplicated
        .unwrap();
    assert_eq!(lf, "a\nb", "must stay no-final-newline");
    let crlf = markerless_sort_fixer(None, None, Comparator::Lexical, true, None)
        .sorted("b\r\na\r\nb")
        .unwrap();
    assert_eq!(crlf, "a\r\nb", "CRLF: no spurious trailing terminator");
    // A file that DID end with a newline keeps it (interior dedup).
    let kept = markerless_sort_fixer(None, None, Comparator::Lexical, true, None)
        .sorted("b\na\nb\n")
        .unwrap();
    assert_eq!(kept, "a\nb\n", "a final newline is preserved");
}

#[test]
fn sorted_select_reorders_only_matching_lines_in_place() {
    // Non-matching lines (the `# group` comment) stay at their slot; only
    // `dep:`-prefixed entries are reordered around them.
    let f = markerless_sort_fixer(
        Some("# start"),
        Some("# end"),
        Comparator::Lexical,
        false,
        Some("^dep:"),
    );
    let t = "# start\ndep:z\n# group comment\ndep:a\n# end\n";
    let out = f.sorted(t).unwrap();
    assert_eq!(out, "# start\ndep:a\n# group comment\ndep:z\n# end\n");
}

#[test]
fn sorted_handles_multiple_blocks_each_independently() {
    let t = "# keep-sorted start\nb\na\n# keep-sorted end\nx\n# keep-sorted start\nd\nc\n# keep-sorted end\n";
    let out = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
    assert_eq!(
        out,
        "# keep-sorted start\na\nb\n# keep-sorted end\nx\n# keep-sorted start\nc\nd\n# keep-sorted end\n"
    );
}

#[test]
fn sort_fix_correlates_with_the_check() {
    // The load-bearing invariant: `sort` resolves exactly the entry
    // violations `check` flags. Sort every unsorted fixture, then re-check
    // and assert no entry (non-`unclosed`) violation survives.
    for t in [
        "# keep-sorted start\ncharlie\nalpha\nbravo\n# keep-sorted end\n",
        "# keep-sorted start\nz\ny\nx\nw\n# keep-sorted end\n",
        "head\n# keep-sorted start\nb\na\n# keep-sorted end\nmid\n# keep-sorted start\nd\nc\n# keep-sorted end\n",
    ] {
        let rule = rule(Comparator::Lexical, false);
        assert!(!eval(&rule, t).is_empty(), "fixture should flag: {t:?}");
        let out = sort_fixer(Comparator::Lexical, false).sorted(t).unwrap();
        let after: Vec<_> = eval(&rule, &out)
            .into_iter()
            .filter(|v| {
                v.baseline_key
                    .as_deref()
                    .is_none_or(|k| !k.starts_with(UNCLOSED_KEY_PREFIX))
            })
            .collect();
        assert!(
            after.is_empty(),
            "entry violations survived sort: {after:?}"
        );
    }
}

#[test]
fn sort_fix_unique_correlates_with_the_check() {
    let rule = rule(Comparator::Lexical, true);
    let t = "# keep-sorted start\nbravo\nalpha\nbravo\n# keep-sorted end\n";
    assert!(!eval(&rule, t).is_empty());
    let out = sort_fixer(Comparator::Lexical, true).sorted(t).unwrap();
    assert!(eval(&rule, &out).is_empty(), "dup/order survived: {out:?}");
}

#[test]
fn can_fix_declines_only_the_unclosed_finding() {
    let f = sort_fixer(Comparator::Lexical, false);
    let entry = Violation::new("x is out of order");
    assert!(f.can_fix(&entry), "an entry violation is sort-fixable");
    let unclosed = Violation::new("unclosed ordered_block")
        .with_baseline_key(format!("{UNCLOSED_KEY_PREFIX}7"));
    assert!(
        !f.can_fix(&unclosed),
        "the unclosed finding is not sort-fixable"
    );
}

#[test]
fn fixable_rule_marks_the_unclosed_finding_with_the_sentinel_key() {
    // A start with no end, on a FIXABLE rule: the check emits the unclosed
    // finding carrying the sentinel, so the engine tags it non-fixable.
    let t = "# keep-sorted start\nalpha\nbravo\n";
    let v = eval(&rule_with_sort(Comparator::Lexical, false), t);
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(
        v[0].baseline_key
            .as_deref()
            .is_some_and(|k| k.starts_with(UNCLOSED_KEY_PREFIX)),
        "unclosed finding must carry the sentinel key: {:?}",
        v[0].baseline_key
    );
}

#[test]
fn check_only_rule_leaves_every_finding_key_less() {
    // With NO fixer attached, findings stay key-less so their baseline
    // fingerprint remains the offending line -- adding a `sort` fix is the
    // only thing that re-baselines an ordered_block (no silent churn).
    let entry = eval(
        &rule(Comparator::Lexical, false),
        "# keep-sorted start\nbravo\nalpha\n# keep-sorted end\n",
    );
    assert_eq!(entry.len(), 1, "{entry:?}");
    assert!(
        entry[0].baseline_key.is_none(),
        "entry finding must be key-less"
    );
    let unclosed = eval(
        &rule(Comparator::Lexical, false),
        "# keep-sorted start\nalpha\n",
    );
    assert_eq!(unclosed.len(), 1, "{unclosed:?}");
    assert!(
        unclosed[0].baseline_key.is_none(),
        "unclosed finding must be key-less without a fixer"
    );
}

#[test]
fn fixable_rule_keys_two_blocks_distinctly() {
    // F4 regression (found by CLI probing): a fixable ordered_block over a
    // file with TWO out-of-order blocks emits two findings that MUST carry
    // distinct baseline_keys -- otherwise the fixpoint merge keys both to
    // `(rule_id, path)` and drops one (a debug panic in `alint fix`).
    let t = "# keep-sorted start\nb\na\n# keep-sorted end\nMID\n# keep-sorted start\nd\nc\n# keep-sorted end\n";
    let v = eval(&rule_with_sort(Comparator::Lexical, false), t);
    assert_eq!(v.len(), 2, "one finding per block: {v:?}");
    let k0 = v[0].baseline_key.as_deref();
    let k1 = v[1].baseline_key.as_deref();
    assert!(k0.is_some() && k1.is_some(), "both findings keyed: {v:?}");
    assert_ne!(k0, k1, "the two blocks' findings must have distinct keys");
    assert!(
        k0.unwrap().starts_with(ENTRY_KEY_PREFIX) && k1.unwrap().starts_with(ENTRY_KEY_PREFIX),
        "both are entry (sortable) findings: {v:?}"
    );
}

#[test]
fn apply_sorts_the_file_on_disk() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    let t = "# keep-sorted start\ncharlie\nalpha\nbravo\n# keep-sorted end\n";
    std::fs::write(tmp.path().join("f.txt"), t).unwrap();
    let ctx = FixContext {
        root: tmp.path(),
        dry_run: false,
        fix_size_limit: None,
        allow_out_of_root: false,
        compose: None,
        stage_ops: None,
    };
    let outcome = sort_fixer(Comparator::Lexical, false)
        .apply(&Violation::new("x").with_path(Path::new("f.txt")), &ctx)
        .unwrap();
    assert!(matches!(outcome, FixOutcome::Applied(_)), "{outcome:?}");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
        "# keep-sorted start\nalpha\nbravo\ncharlie\n# keep-sorted end\n"
    );
}

#[test]
fn apply_dry_run_leaves_disk_untouched() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    let t = "# keep-sorted start\nb\na\n# keep-sorted end\n";
    std::fs::write(tmp.path().join("f.txt"), t).unwrap();
    let ctx = FixContext {
        root: tmp.path(),
        dry_run: true,
        fix_size_limit: None,
        allow_out_of_root: false,
        compose: None,
        stage_ops: None,
    };
    let outcome = sort_fixer(Comparator::Lexical, false)
        .apply(&Violation::new("x").with_path(Path::new("f.txt")), &ctx)
        .unwrap();
    assert!(matches!(outcome, FixOutcome::Applied(_)), "{outcome:?}");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
        t
    );
}

#[test]
fn apply_skips_an_already_sorted_file() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    let t = "# keep-sorted start\na\nb\n# keep-sorted end\n";
    std::fs::write(tmp.path().join("f.txt"), t).unwrap();
    let ctx = FixContext {
        root: tmp.path(),
        dry_run: false,
        fix_size_limit: None,
        allow_out_of_root: false,
        compose: None,
        stage_ops: None,
    };
    let outcome = sort_fixer(Comparator::Lexical, false)
        .apply(&Violation::new("x").with_path(Path::new("f.txt")), &ctx)
        .unwrap();
    assert!(
        matches!(&outcome, FixOutcome::Skipped(s) if s.contains("already sorted")),
        "{outcome:?}"
    );
}

#[test]
fn apply_declines_an_unclosed_finding_even_with_an_unsorted_block() {
    // F1 GUARD: the engine calls `apply` for every violation regardless of
    // `can_fix`. For an UNCLOSED finding (which sort cannot repair), `apply`
    // must decline -- NOT sort the file's other blocks and return `Applied`
    // (which would let the engine lock the key and drop the unclosed residual).
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    // An unsorted block: without the guard, apply(unclosed) would sort it.
    let t = "# keep-sorted start\nb\na\n# keep-sorted end\n";
    std::fs::write(tmp.path().join("f.txt"), t).unwrap();
    let ctx = FixContext {
        root: tmp.path(),
        dry_run: false,
        fix_size_limit: None,
        allow_out_of_root: false,
        compose: None,
        stage_ops: None,
    };
    let outcome = sort_fixer(Comparator::Lexical, false)
        .apply(
            &Violation::new("x")
                .with_path(Path::new("f.txt"))
                .with_baseline_key(format!("{UNCLOSED_KEY_PREFIX}0")),
            &ctx,
        )
        .unwrap();
    assert!(
        matches!(&outcome, FixOutcome::Skipped(s) if s.contains("unclosed")),
        "{outcome:?}"
    );
    // The block was NOT sorted (the guard declined the whole apply).
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
        t
    );
}

#[test]
fn fix_edit_emits_set_content_and_declines_unclosed() {
    let f = sort_fixer(Comparator::Lexical, false);
    let bytes = b"# keep-sorted start\nb\na\n# keep-sorted end\n";
    let edit = f
        .fix_edit(
            &Violation::new("x").with_path(Path::new("f.txt")),
            bytes,
            Path::new("/"),
        )
        .expect("an entry violation yields an edit");
    match edit {
        FixEdit::SetContent { content, .. } => assert_eq!(
            content,
            b"# keep-sorted start\na\nb\n# keep-sorted end\n".to_vec()
        ),
        other => panic!("expected SetContent, got {other:?}"),
    }
    // The unclosed finding is declined even though other blocks could sort.
    let unclosed = Violation::new("unclosed")
        .with_path(Path::new("f.txt"))
        .with_baseline_key(format!("{UNCLOSED_KEY_PREFIX}1"));
    assert!(f.fix_edit(&unclosed, bytes, Path::new("/")).is_none());
}

#[test]
fn build_wires_the_sort_fixer() {
    use crate::test_support::spec_yaml;
    let yaml = "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: '# s'\nend: '# e'\nlevel: error\nfix: { sort: {} }\n";
    let rule = build(&spec_yaml(yaml)).unwrap();
    assert!(rule.fixer().is_some(), "sort fix should wire a fixer");
}

#[test]
fn build_defaults_the_deleting_unique_sort_to_unsafe() {
    use crate::test_support::spec_yaml;
    // A pure reorder is behavior-preserving -> Safe (a bare `alint fix`
    // applies it). `unique` DELETES lines (equality on the trimmed / folded
    // value), so it defaults to Unsafe -- a bare fix only suggests it.
    let pure = build(&spec_yaml(
            "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: '# s'\nend: '# e'\nlevel: error\nfix: { sort: {} }\n",
        ))
        .unwrap();
    assert_eq!(
        pure.fixer().unwrap().applicability(),
        Applicability::Safe,
        "a pure reorder is Safe"
    );
    let uniq = build(&spec_yaml(
            "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: '# s'\nend: '# e'\nunique: true\nlevel: error\nfix: { sort: {} }\n",
        ))
        .unwrap();
    assert_eq!(
        uniq.fixer().unwrap().applicability(),
        Applicability::Unsafe,
        "a deleting `unique` sort defaults to Unsafe"
    );
    // An explicit tier always wins -- a user can opt the dedup back to Safe.
    let promoted = build(&spec_yaml(
            "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: '# s'\nend: '# e'\nunique: true\nlevel: error\nfix:\n  sort:\n    applicability: safe\n",
        ))
        .unwrap();
    assert_eq!(
        promoted.fixer().unwrap().applicability(),
        Applicability::Safe,
        "an explicit `applicability: safe` overrides the unique default"
    );
}

#[test]
fn build_rejects_an_incompatible_fix_op() {
    use crate::test_support::spec_yaml;
    let yaml =
        "id: t\nkind: ordered_block\npaths: [\"x\"]\nlevel: error\nfix: { file_remove: {} }\n";
    let err = build(&spec_yaml(yaml)).unwrap_err();
    assert!(err.to_string().contains("not compatible"), "{err}");
}

// ----- the `insert_line` fix (require:) ------------------------------

fn insert_fixer(comparator: Comparator) -> OrderedBlockInsertLineFixer {
    OrderedBlockInsertLineFixer {
        comparator,
        select: None,
        applicability: Applicability::Safe,
    }
}

#[test]
fn require_flags_a_missing_line_keyed_per_line() {
    let r = rule_with_insert_line(&["bravo", "delta"]);
    // File has bravo (present) but not delta (missing).
    let v = eval(&r, "alpha\nbravo\ncharlie\n");
    assert_eq!(v.len(), 1, "only delta is missing: {v:?}");
    assert_eq!(
        v[0].baseline_key.as_deref(),
        Some(format!("{REQUIRE_KEY_PREFIX}delta").as_str())
    );
}

#[test]
fn require_is_satisfied_when_all_present() {
    let r = rule_with_insert_line(&["alpha", "bravo"]);
    assert!(eval(&r, "alpha\nbravo\ncharlie\n").is_empty());
}

#[test]
fn require_check_is_key_less_without_a_fixer() {
    // A check-only require: rule keeps line-content fingerprints (no key).
    let mut r = markerless_rule(None, None, Comparator::Lexical);
    r.require = vec!["zzz".to_string()];
    let v = eval(&r, "alpha\n");
    assert_eq!(v.len(), 1);
    assert!(v[0].baseline_key.is_none());
}

#[test]
fn inserted_splices_at_the_sorted_position() {
    let f = insert_fixer(Comparator::Lexical);
    // middle
    assert_eq!(
        f.inserted("alpha\ncharlie\n", "bravo").unwrap(),
        "alpha\nbravo\ncharlie\n"
    );
    // start
    assert_eq!(
        f.inserted("bravo\ncharlie\n", "alpha").unwrap(),
        "alpha\nbravo\ncharlie\n"
    );
    // end
    assert_eq!(
        f.inserted("alpha\nbravo\n", "charlie").unwrap(),
        "alpha\nbravo\ncharlie\n"
    );
}

#[test]
fn inserted_is_idempotent_when_present() {
    let f = insert_fixer(Comparator::Lexical);
    assert_eq!(f.inserted("alpha\nbravo\n", "bravo"), None);
}

#[test]
fn inserted_preserves_crlf_and_missing_final_newline() {
    let f = insert_fixer(Comparator::Lexical);
    // CRLF, middle insert.
    assert_eq!(
        f.inserted("alpha\r\ncharlie\r\n", "bravo").unwrap(),
        "alpha\r\nbravo\r\ncharlie\r\n"
    );
    // No final newline, append at end: the previous last line gains a
    // terminator and the new last line stays bare.
    assert_eq!(
        f.inserted("alpha\nbravo", "charlie").unwrap(),
        "alpha\nbravo\ncharlie"
    );
    // No final newline, middle insert.
    assert_eq!(
        f.inserted("alpha\ncharlie", "bravo").unwrap(),
        "alpha\nbravo\ncharlie"
    );
    // Empty file -> the line with a trailing newline.
    assert_eq!(f.inserted("", "solo").unwrap(), "solo\n");
}

#[test]
fn inserted_numeric_comparator_positions_correctly() {
    let f = insert_fixer(Comparator::Numeric);
    assert_eq!(f.inserted("1\n10\n", "2").unwrap(), "1\n2\n10\n");
}

#[test]
fn insert_line_can_fix_only_a_require_finding() {
    let f = insert_fixer(Comparator::Lexical);
    let req = Violation::new("x").with_baseline_key(format!("{REQUIRE_KEY_PREFIX}a"));
    let entry = Violation::new("x").with_baseline_key(format!("{ENTRY_KEY_PREFIX}0"));
    assert!(f.can_fix(&req));
    assert!(!f.can_fix(&entry));
}

#[test]
fn sort_can_fix_declines_a_require_finding() {
    // A require finding is NOT sort-fixable (sort reorders, it does not add).
    let s = sort_fixer(Comparator::Lexical, false);
    let req = Violation::new("x").with_baseline_key(format!("{REQUIRE_KEY_PREFIX}a"));
    assert!(!s.can_fix(&req));
}

#[test]
fn insert_line_apply_declines_a_non_require_finding() {
    // F1 GUARD: apply is called per-violation; an ENTRY (sortedness) finding
    // must be declined, not spliced.
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("f.txt"), "b\na\n").unwrap();
    let ctx = FixContext {
        root: tmp.path(),
        dry_run: false,
        fix_size_limit: None,
        allow_out_of_root: false,
        compose: None,
        stage_ops: None,
    };
    let outcome = insert_fixer(Comparator::Lexical)
        .apply(
            &Violation::new("x")
                .with_path(Path::new("f.txt"))
                .with_baseline_key(format!("{ENTRY_KEY_PREFIX}0")),
            &ctx,
        )
        .unwrap();
    assert!(
        matches!(&outcome, FixOutcome::Skipped(s) if s.contains("not a missing-required")),
        "{outcome:?}"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("f.txt")).unwrap(),
        "b\na\n"
    );
}

#[test]
fn insert_line_apply_splices_on_disk() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("CODEOWNERS"), "*.md @docs\n*.rs @rust\n").unwrap();
    let ctx = FixContext {
        root: tmp.path(),
        dry_run: false,
        fix_size_limit: None,
        allow_out_of_root: false,
        compose: None,
        stage_ops: None,
    };
    let outcome = insert_fixer(Comparator::Lexical)
        .apply(
            &Violation::new("x")
                .with_path(Path::new("CODEOWNERS"))
                .with_baseline_key(format!("{REQUIRE_KEY_PREFIX}*.py @py")),
            &ctx,
        )
        .unwrap();
    assert!(matches!(outcome, FixOutcome::Applied(_)), "{outcome:?}");
    // `*.py @py` sorts before `*.rs @rust` and after `*.md @docs`.
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("CODEOWNERS")).unwrap(),
        "*.md @docs\n*.py @py\n*.rs @rust\n"
    );
}

#[test]
fn insert_line_fix_correlates_with_the_check() {
    // check flags the missing line -> insert -> re-check clean (converges).
    let r = rule_with_insert_line(&["bravo"]);
    let text = "alpha\ncharlie\n";
    assert_eq!(eval(&r, text).len(), 1);
    let out = insert_fixer(Comparator::Lexical)
        .inserted(text, "bravo")
        .unwrap();
    assert!(eval(&r, &out).is_empty(), "converged: {out:?}");
}

#[test]
fn build_wires_insert_line_and_rejects_bad_configs() {
    use crate::test_support::spec_yaml;
    // Markerless + require: + insert_line wires the fixer at Safe.
    let ok = build(&spec_yaml(
            "id: t\nkind: ordered_block\npaths: [\"CODEOWNERS\"]\nrequire: [\"* @team\"]\nlevel: error\nfix: { insert_line: {} }\n",
        ))
        .unwrap();
    // Default tier is Unsafe (a computed-position insert can change meaning in an
    // order-sensitive file); an explicit `applicability:` overrides.
    assert_eq!(ok.fixer().unwrap().applicability(), Applicability::Unsafe);
    let safe = build(&spec_yaml(
            "id: t\nkind: ordered_block\npaths: [\"CODEOWNERS\"]\nrequire: [\"* @team\"]\nlevel: error\nfix: { insert_line: { applicability: safe } }\n",
        ))
        .unwrap();
    assert_eq!(safe.fixer().unwrap().applicability(), Applicability::Safe);
    // require: with a marker is rejected.
    let markered = build(&spec_yaml(
            "id: t\nkind: ordered_block\npaths: [\"x\"]\nstart: '# s'\nend: '# e'\nrequire: [\"a\"]\nlevel: error\n",
        ))
        .unwrap_err();
    assert!(markered.to_string().contains("MARKERLESS"), "{markered}");
    // insert_line without require: is rejected.
    let no_require = build(&spec_yaml(
        "id: t\nkind: ordered_block\npaths: [\"x\"]\nlevel: error\nfix: { insert_line: {} }\n",
    ))
    .unwrap_err();
    assert!(no_require.to_string().contains("require"), "{no_require}");
    // an empty require line is rejected.
    let empty = build(&spec_yaml(
        "id: t\nkind: ordered_block\npaths: [\"x\"]\nrequire: [\"  \"]\nlevel: error\n",
    ))
    .unwrap_err();
    assert!(empty.to_string().contains("must not be empty"), "{empty}");
    // AUDIT (insert_line HIGH): a require line that `select:` would EXCLUDE is
    // unsatisfiable -- inserting it never makes it an entry, so the fixpoint would
    // re-insert it every pass (unbounded duplication). Reject at load.
    let unselectable = build(&spec_yaml(
        "id: t\nkind: ordered_block\npaths: [\"deps.txt\"]\nselect: \"^dep:\"\nrequire: [\"config\"]\nlevel: error\nfix: { insert_line: {} }\n",
    ))
    .unwrap_err();
    assert!(
        unselectable
            .to_string()
            .contains("does not match `select:`")
            && unselectable.to_string().contains("could never be an entry"),
        "{unselectable}"
    );
    // ... but a require line that DOES match `select:` builds fine.
    let selectable = build(&spec_yaml(
        "id: t\nkind: ordered_block\npaths: [\"deps.txt\"]\nselect: \"^dep:\"\nrequire: [\"dep:z\"]\nlevel: error\nfix: { insert_line: {} }\n",
    ));
    assert!(selectable.is_ok(), "{:?}", selectable.err());
    // AUDIT (insert_line HIGH, embedded-newline trigger): a require line with an
    // interior line break splits into multiple physical lines on re-read, so it is
    // never "present" -> the same unbounded-duplication runaway. Reject at load.
    let multiline = build(&spec_yaml(
        "id: t\nkind: ordered_block\npaths: [\"x\"]\nrequire: [\"mid\\nline\"]\nlevel: error\nfix: { insert_line: {} }\n",
    ))
    .unwrap_err();
    assert!(
        multiline.to_string().contains("embedded line break"),
        "{multiline}"
    );
}

#[test]
fn insert_line_select_mismatch_cannot_run_away() {
    // End-to-end guard for the HIGH finding: with the load-time rejection in
    // place, a select-mismatched require line can never reach the fixer, so there
    // is no config that produces the unbounded-duplication fixpoint. (The unit
    // above asserts the rejection; this documents WHY it is load-time, not a
    // runtime clamp: `is_entry_line` gates BOTH the presence scan and the insert
    // idempotence check, so a non-entry line is invisible to both and would loop.)
    use crate::test_support::spec_yaml;
    let err = build(&spec_yaml(
        "id: t\nkind: ordered_block\npaths: [\"deps.txt\"]\nselect: \"^dep:\"\nrequire: [\"config\", \"dep:ok\"]\nlevel: error\nfix: { insert_line: {} }\n",
    ))
    .unwrap_err();
    // The offending line is named so the fix is obvious.
    assert!(err.to_string().contains("\"config\""), "{err}");
}

#[test]
fn sort_apply_reports_the_require_specific_skip_reason() {
    // AUDIT (sort MED false-diagnostic): `can_fix` declines BOTH unclosed and
    // require findings, but the skip reason must match the finding -- a markerless
    // `require:` rule must NOT be told to hunt for a nonexistent `end` marker.
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("f.txt"), "b\na\n").unwrap();
    let ctx = FixContext {
        root: tmp.path(),
        dry_run: false,
        fix_size_limit: None,
        allow_out_of_root: false,
        compose: None,
        stage_ops: None,
    };
    let outcome = sort_fixer(Comparator::Lexical, false)
        .apply(
            &Violation::new("required line \"m\" is missing")
                .with_path(Path::new("f.txt"))
                .with_baseline_key(format!("{REQUIRE_KEY_PREFIX}m")),
            &ctx,
        )
        .unwrap();
    let FixOutcome::Skipped(s) = &outcome else {
        panic!("expected a Skipped outcome, got {outcome:?}");
    };
    assert!(s.contains("insert_line"), "{s}");
    assert!(
        !s.contains("unclosed"),
        "must not mention a nonexistent marker: {s}"
    );
}

#[test]
fn require_presence_is_exact_string_not_comparator_equality() {
    // AUDIT (insert_line LOW, BY-DESIGN): presence is EXACT-string, independent of
    // `comparator`. Under lexical-ci, `Bravo` present does NOT satisfy require
    // `bravo` -- the distinct case-variant is flagged missing and inserted. This
    // pins the intentional contract (a required line is inserted verbatim, so it
    // must match itself exactly) and, crucially, that it CONVERGES (no runaway).
    let mut r = rule_with_insert_line(&["bravo"]);
    r.comparator = Comparator::LexicalCi;
    r.fixer = Some(Box::new(insert_fixer(Comparator::LexicalCi)));
    let text = "Bravo\ncharlie\n";
    // `Bravo` is present but `bravo` (exact) is not -> one finding.
    assert_eq!(eval(&r, text).len(), 1);
    let out = insert_fixer(Comparator::LexicalCi)
        .inserted(text, "bravo")
        .unwrap();
    // Inserted at its ci-sorted slot; both case-variants now present.
    assert_eq!(out, "Bravo\nbravo\ncharlie\n");
    // Converges: re-check is clean (the exact line is now present).
    assert!(eval(&r, &out).is_empty(), "converged: {out:?}");
}
