use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use tempfile::TempDir;

fn ctx(tmp: &TempDir, dry_run: bool) -> FixContext<'_> {
    FixContext::new(tmp.path(), dry_run)
}

fn write(tmp: &TempDir, rel: &str, content: &[u8]) {
    let abs = tmp.path().join(rel);
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(abs, content).unwrap();
}

fn read(tmp: &TempDir, rel: &str) -> Vec<u8> {
    std::fs::read(tmp.path().join(rel)).unwrap()
}

fn viol(target: &str) -> Violation {
    Violation::new("x").with_path(PathBuf::from(target))
}

fn equals_viol(target: &str, value: &str) -> Violation {
    equals_viol_occurrence(target, value, 0)
}

fn equals_viol_occurrence(target: &str, value: &str, occurrence: usize) -> Violation {
    Violation::new("x")
        .with_path(PathBuf::from(target))
        .with_baseline_key(format!("equals\0{target}\0{value}"))
        .with_fix_key(format!("cross_file_equals\0{occurrence}"))
}

#[test]
fn overwrites_a_drifted_target_with_the_source() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "canon.txt", b"canonical\n");
    write(&tmp, "copy.txt", b"stale\n");
    let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
        .apply(&viol("copy.txt"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    assert_eq!(read(&tmp, "copy.txt"), b"canonical\n", "target now mirrors");
    assert_eq!(read(&tmp, "canon.txt"), b"canonical\n", "source untouched");
}

#[test]
fn is_idempotent_when_already_identical() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "canon.txt", b"same\n");
    write(&tmp, "copy.txt", b"same\n");
    let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
        .apply(&viol("copy.txt"), &ctx(&tmp, false))
        .unwrap();
    match out {
        FixOutcome::Skipped(reason) => {
            assert!(reason.contains("already identical"), "{reason}");
        }
        FixOutcome::Applied(s) => panic!("expected an idempotent skip, got Applied({s})"),
    }
}

#[test]
fn mirrors_binary_content() {
    // The `identical` relation compares bytes, so `sync_from` mirrors binary
    // content too (a vendored asset), unlike the text-only hygiene fixers.
    let tmp = TempDir::new().unwrap();
    let bin = [0u8, 159, 146, 150, b'\n'];
    write(&tmp, "asset.bin", &bin);
    write(&tmp, "vendored.bin", b"old\n");
    let out = SyncFromFixer::new(PathBuf::from("asset.bin"), Applicability::Unsafe)
        .apply(&viol("vendored.bin"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    assert_eq!(read(&tmp, "vendored.bin"), bin, "binary mirrored verbatim");
}

#[test]
fn dry_run_reports_but_does_not_write() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "canon.txt", b"canonical\n");
    write(&tmp, "copy.txt", b"stale\n");
    let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
        .apply(&viol("copy.txt"), &ctx(&tmp, true))
        .unwrap();
    match out {
        FixOutcome::Applied(s) => {
            assert!(s.starts_with("would sync"), "dry-run summary: {s}");
            assert!(s.contains("copy.txt") && s.contains("canon.txt"), "{s}");
        }
        FixOutcome::Skipped(r) => panic!("expected a would-sync report, got Skipped({r})"),
    }
    assert_eq!(read(&tmp, "copy.txt"), b"stale\n", "dry-run must not write");
}

#[test]
fn confines_an_absolute_source_read() {
    // SECURITY: an absolute `source` must NOT read out of the repo root
    // (`root.join("/abs")` discards the base) -- else an untrusted ruleset
    // could copy `/etc/passwd` into a tracked file. Confined -> Skip, no write.
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret");
    std::fs::write(&secret, b"TOP SECRET\n").unwrap();
    write(&tmp, "copy.txt", b"stale\n");
    let out = SyncFromFixer::new(secret.clone(), Applicability::Unsafe)
        .apply(&viol("copy.txt"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("escapes the repo root")),
        "an absolute source must be confined, got {out:?}"
    );
    assert_eq!(
        read(&tmp, "copy.txt"),
        b"stale\n",
        "must NOT copy out-of-root bytes in"
    );
}

#[test]
fn confines_an_absolute_target_write() {
    // SECURITY: an absolute target (violation path) must NOT be written outside
    // the root.
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let victim = outside.path().join("victim");
    std::fs::write(&victim, b"original\n").unwrap();
    write(&tmp, "canon.txt", b"canonical\n");
    let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
        .apply(&viol(victim.to_str().unwrap()), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("escapes the repo root")),
        "an absolute target must be confined, got {out:?}"
    );
    assert_eq!(
        std::fs::read(&victim).unwrap(),
        b"original\n",
        "out-of-root victim untouched"
    );
}

#[test]
fn skips_a_missing_source_cleanly() {
    // A missing canonical source is a clean Skip (not a hard fix error): `check`
    // already reports it as a source-side violation.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "copy.txt", b"stale\n");
    let out = SyncFromFixer::new(PathBuf::from("gone.txt"), Applicability::Unsafe)
        .apply(&viol("copy.txt"), &ctx(&tmp, false))
        .unwrap();
    match out {
        FixOutcome::Skipped(reason) => {
            assert!(reason.contains("missing or unreadable"), "{reason}");
        }
        FixOutcome::Applied(s) => panic!("expected a missing-source skip, got Applied({s})"),
    }
}

#[test]
fn skips_a_non_regular_target_without_hanging() {
    // SECURITY/DoS (audit HIGH): a `targets:` LIST entry is a config-verbatim
    // path that skips the walker's special-file filter. A bare read of a FIFO
    // target would block `fix` forever. A directory is the portable, hang-free
    // proxy for a non-regular file; the fixer must Skip (via read_for_fix's
    // guard), never reach a blocking read.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "canon.txt", b"canonical\n");
    std::fs::create_dir(tmp.path().join("a_dir")).unwrap();
    let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
        .apply(&viol("a_dir"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("not a regular file")),
        "a non-regular target must Skip cleanly, got {out:?}"
    );
}

#[test]
fn skips_a_non_regular_source_without_hanging() {
    // Symmetric guard for the `source:` read (also config-verbatim).
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("src_dir")).unwrap();
    write(&tmp, "copy.txt", b"stale\n");
    let out = SyncFromFixer::new(PathBuf::from("src_dir"), Applicability::Unsafe)
        .apply(&viol("copy.txt"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("not a regular file")),
        "a non-regular source must Skip cleanly, got {out:?}"
    );
    assert_eq!(read(&tmp, "copy.txt"), b"stale\n", "target untouched");
}

#[test]
fn skips_a_self_referential_target() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "canon.txt", b"x\n");
    let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
        .apply(&viol("canon.txt"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("same file")),
        "got {out:?}"
    );
}

#[test]
fn stage_mode_composes_without_writing() {
    // `--diff`: a content fixer routes through the compose buffer (dry_run
    // false, compose Some, stage sink present but unused by a content op), and
    // the buffer is diffed without flushing -- so disk is untouched.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "canon.txt", b"canonical\n");
    write(&tmp, "copy.txt", b"stale\n");
    let compose = RefCell::new(BTreeMap::new());
    let sink = RefCell::new(Vec::new());
    let fix_ctx = FixContext::new(tmp.path(), false)
        .with_compose(Some(&compose))
        .with_stage_ops(Some(&sink));
    let out = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
        .apply(&viol("copy.txt"), &fix_ctx)
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    assert_eq!(
        read(&tmp, "copy.txt"),
        b"stale\n",
        "stage must NOT flush to disk"
    );
    // The composed write is captured for the diff (keyed by resolved target).
    let buffered = compose.borrow();
    let composed = buffered
        .iter()
        .find(|(k, _)| k.ends_with("copy.txt"))
        .map(|(_, v)| v.clone());
    assert_eq!(
        composed,
        Some(b"canonical\n".to_vec()),
        "the sync is composed, got {:?}",
        *buffered
    );
    assert!(
        sink.borrow().is_empty(),
        "a content op records no whole-file stage edit"
    );
}

#[test]
fn allow_out_of_root_permits_an_out_of_root_source() {
    // Parity with the same-file content fixers: `allow_out_of_root` opts into
    // reading a source outside the tree (a top-level policy escape hatch).
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let src = outside.path().join("canon");
    std::fs::write(&src, b"external canonical\n").unwrap();
    write(&tmp, "copy.txt", b"stale\n");
    let fix_ctx = FixContext::new(tmp.path(), false).with_allow_out_of_root(true);
    let out = SyncFromFixer::new(src, Applicability::Unsafe)
        .apply(&viol("copy.txt"), &fix_ctx)
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    assert_eq!(read(&tmp, "copy.txt"), b"external canonical\n");
}

#[test]
fn fix_edit_sets_the_target_to_the_source_bytes() {
    // The editor / SARIF / agent form: a `SetContent` of the target with the
    // source's bytes, produced even with EMPTY `_bytes` (the suggestion path).
    let tmp = TempDir::new().unwrap();
    write(&tmp, "canon.txt", b"canonical\n");
    write(&tmp, "copy.txt", b"stale\n");
    let edit = SyncFromFixer::new(PathBuf::from("canon.txt"), Applicability::Unsafe)
        .fix_edit(&viol("copy.txt"), &[], tmp.path())
        .expect("a proposed edit");
    match edit {
        FixEdit::SetContent { path, content } => {
            assert_eq!(path, PathBuf::from("copy.txt"));
            assert_eq!(content, b"canonical\n");
        }
        other => panic!("expected SetContent, got {other:?}"),
    }
}

#[test]
fn fix_edit_declines_an_out_of_root_source() {
    // `fix_edit` confines strictly (no `allow_out_of_root`): an out-of-root
    // source yields NO proposed edit.
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let src = outside.path().join("canon");
    std::fs::write(&src, b"x\n").unwrap();
    write(&tmp, "copy.txt", b"stale\n");
    assert!(
        SyncFromFixer::new(src, Applicability::Unsafe)
            .fix_edit(&viol("copy.txt"), &[], tmp.path())
            .is_none(),
        "an out-of-root source must not produce an editor edit"
    );
}

#[test]
fn carries_its_tier() {
    assert_eq!(
        SyncFromFixer::new(PathBuf::from("s"), Applicability::Unsafe).applicability(),
        Applicability::Unsafe
    );
    assert_eq!(
        SyncFromFixer::new(PathBuf::from("s"), Applicability::Safe).applicability(),
        Applicability::Safe
    );
}

// ─── CrossFileValueFixer (relation: equals value propagation) ────────

// Source and target share `fmt` (the common test case); the source extract
// uses the same format as the target so a JSON source parses as JSON, a TOML
// source as TOML, etc.
fn value_fixer(source: &str, source_q: &str, fmt: Format, target_q: &str) -> CrossFileValueFixer {
    CrossFileValueFixer::new(
        PathBuf::from(source),
        Extract::structured(fmt, source_q.to_string()).unwrap(),
        ValueTargets::Glob(Extract::structured(fmt, target_q.to_string()).unwrap()),
        Vec::new(),
        Applicability::Unsafe,
    )
}

#[test]
fn value_propagates_a_scalar_into_the_target_node() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp,
        "Cargo.toml",
        b"[workspace.package]\nversion = \"2.0.0\"\n",
    );
    write(
        &tmp,
        "crate/Cargo.toml",
        b"[package]\nname = \"a\"\nversion = \"1.0.0\"\n",
    );
    let out = value_fixer(
        "Cargo.toml",
        "$.workspace.package.version",
        Format::Toml,
        "$.package.version",
    )
    .apply(&equals_viol("crate/Cargo.toml", "1.0.0"), &ctx(&tmp, false))
    .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    let after = String::from_utf8(read(&tmp, "crate/Cargo.toml")).unwrap();
    assert!(after.contains("version = \"2.0.0\""), "{after}");
    assert!(
        after.contains("name = \"a\""),
        "other keys preserved: {after}"
    );
}

#[test]
fn value_is_idempotent_when_the_target_already_equals() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp,
        "Cargo.toml",
        b"[workspace.package]\nversion = \"2.0.0\"\n",
    );
    write(
        &tmp,
        "crate/Cargo.toml",
        b"[package]\nversion = \"2.0.0\"\n",
    );
    let out = value_fixer(
        "Cargo.toml",
        "$.workspace.package.version",
        Format::Toml,
        "$.package.version",
    )
    .apply(&equals_viol("crate/Cargo.toml", "2.0.0"), &ctx(&tmp, false))
    .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("already equals")),
        "an already-equal target must Skip, got {out:?}"
    );
}

#[test]
fn value_skips_when_the_source_is_not_exactly_one_value() {
    let tmp = TempDir::new().unwrap();
    // The source query matches nothing.
    write(&tmp, "Cargo.toml", b"[workspace.package]\nname = \"ws\"\n");
    write(
        &tmp,
        "crate/Cargo.toml",
        b"[package]\nversion = \"1.0.0\"\n",
    );
    let out = value_fixer(
        "Cargo.toml",
        "$.workspace.package.version",
        Format::Toml,
        "$.package.version",
    )
    .apply(&viol("crate/Cargo.toml"), &ctx(&tmp, false))
    .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("exactly one")),
        "a 0-match source must Skip, got {out:?}"
    );
}

#[test]
fn value_fixer_declines_a_non_string_source_without_writing() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp,
        "source.json",
        br#"{"version":42,"secret":"do-not-report"}"#,
    );
    write(&tmp, "target.json", br#"{"version":"old"}"#);

    let out = value_fixer("source.json", "$.version", Format::Json, "$.version")
        .apply(&viol("target.json"), &ctx(&tmp, false))
        .unwrap();

    assert!(
        matches!(&out, FixOutcome::Skipped(reason) if reason == "source extract matched a non-string value"),
        "a non-string source must be declined, got {out:?}"
    );
    assert_eq!(read(&tmp, "target.json"), br#"{"version":"old"}"#);
}

#[test]
fn value_dry_run_reports_without_writing() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp,
        "Cargo.toml",
        b"[workspace.package]\nversion = \"2.0.0\"\n",
    );
    write(
        &tmp,
        "crate/Cargo.toml",
        b"[package]\nversion = \"1.0.0\"\n",
    );
    let out = value_fixer(
        "Cargo.toml",
        "$.workspace.package.version",
        Format::Toml,
        "$.package.version",
    )
    .apply(&equals_viol("crate/Cargo.toml", "1.0.0"), &ctx(&tmp, true))
    .unwrap();
    assert!(
        matches!(out, FixOutcome::Applied(ref s) if s.starts_with("would set")),
        "got {out:?}"
    );
    assert!(
        String::from_utf8(read(&tmp, "crate/Cargo.toml"))
            .unwrap()
            .contains("1.0.0"),
        "dry-run must not write"
    );
}

#[test]
fn value_confines_an_absolute_target() {
    // SECURITY: an absolute target must not be written outside the root.
    let tmp = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let victim = outside.path().join("victim.toml");
    std::fs::write(&victim, b"[package]\nversion = \"1.0.0\"\n").unwrap();
    write(
        &tmp,
        "Cargo.toml",
        b"[workspace.package]\nversion = \"2.0.0\"\n",
    );
    let out = CrossFileValueFixer::new(
        PathBuf::from("Cargo.toml"),
        Extract::structured(Format::Toml, "$.workspace.package.version".to_string()).unwrap(),
        ValueTargets::List(vec![(
            victim.clone(),
            Extract::structured(Format::Toml, "$.package.version").unwrap(),
        )]),
        Vec::new(),
        Applicability::Unsafe,
    )
    .apply(&viol(victim.to_str().unwrap()), &ctx(&tmp, false))
    .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("escapes the repo root")),
        "an absolute target must be confined, got {out:?}"
    );
    assert!(
        std::fs::read_to_string(&victim).unwrap().contains("1.0.0"),
        "out-of-root victim untouched"
    );
}

#[test]
fn value_carries_its_tier() {
    assert_eq!(
        value_fixer("s", "$.a", Format::Toml, "$.b").applicability(),
        Applicability::Unsafe
    );
}

#[test]
fn value_declines_a_numeric_target_node_no_coercion() {
    // Audit F1: the propagated value is always a STRING, so setting a NUMERIC
    // target node would silently change its type to a quoted string (and the
    // `equals` check, comparing only string leaves, would keep firing). Decline
    // instead of coercing.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.json", br#"{"port":"8080"}"#);
    write(&tmp, "svc.json", br#"{"port": 9090, "host": "x"}"#);
    let out = value_fixer("src.json", "$.port", Format::Json, "$.port")
        .apply(&equals_viol("svc.json", "9090"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("numeric node")),
        "a numeric target node must decline, got {out:?}"
    );
    assert_eq!(
        read(&tmp, "svc.json"),
        br#"{"port": 9090, "host": "x"}"#,
        "the numeric node must NOT be coerced to a string"
    );
}

#[test]
fn value_declines_a_boolean_target_node() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.json", br#"{"flag":"true"}"#);
    write(&tmp, "t.json", br#"{"flag": false}"#);
    let out = value_fixer("src.json", "$.flag", Format::Json, "$.flag")
        .apply(&equals_viol("t.json", "false"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("boolean node")),
        "a boolean target node must decline, got {out:?}"
    );
    assert_eq!(read(&tmp, "t.json"), br#"{"flag": false}"#, "unchanged");
}

#[test]
fn value_skips_a_non_scalar_target_node() {
    // An object/array target node is not a single scalar -> StructuredFixer::set
    // emits no edit -> clean Skip, byte-unchanged.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.json", br#"{"v":"3.0"}"#);
    write(&tmp, "obj.json", br#"{"v": {"nested": 1}}"#);
    let out = value_fixer("src.json", "$.v", Format::Json, "$.v")
        .apply(&equals_viol("obj.json", "object"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Skipped(_)), "got {out:?}");
    assert_eq!(
        read(&tmp, "obj.json"),
        br#"{"v": {"nested": 1}}"#,
        "unchanged"
    );
}

#[test]
fn value_fix_edit_produces_a_set_content_for_the_editor() {
    // Audit 4c: value propagation HAS a worktree-edit form, so the LSP can offer
    // it. `fix_edit` (real buffer bytes) returns a SetContent with the value
    // already propagated into the node.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.json", br#"{"v":"3.0"}"#);
    let buffer = br#"{"name":"x","v":"1.0"}"#;
    write(&tmp, "t.json", buffer);
    let edit = value_fixer("src.json", "$.v", Format::Json, "$.v")
        .fix_edit(&equals_viol("t.json", "1.0"), buffer, tmp.path())
        .expect("a proposed edit");
    match edit {
        FixEdit::SetContent { path, content } => {
            assert_eq!(path, PathBuf::from("t.json"));
            let s = String::from_utf8(content).unwrap();
            assert!(s.contains(r#""v":"3.0""#), "value propagated: {s}");
            assert!(s.contains(r#""name":"x""#), "other keys preserved: {s}");
        }
        other => panic!("expected SetContent, got {other:?}"),
    }
}

#[test]
fn value_fix_edit_declines_empty_bytes() {
    // The two engine SUGGESTION sites call fix_edit with EMPTY bytes; the target
    // re-parse fails, so it declines (None). Only the LSP passes a real buffer.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.json", br#"{"v":"3.0"}"#);
    assert!(
        value_fixer("src.json", "$.v", Format::Json, "$.v")
            .fix_edit(&viol("t.json"), &[], tmp.path())
            .is_none(),
        "empty bytes -> no proposed edit"
    );
}

#[test]
fn value_structured_fix_filters_union_nodes_and_correlates_the_finding() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"22.14.0\n");
    write(
        &tmp,
        "package.json",
        br#"{"engines":{"node":">=20 <23"},"volta":{"node":"22.13.1"}}"#,
    );
    let exact = regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$").unwrap();
    let fixer = CrossFileValueFixer::new(
        PathBuf::from("VERSION"),
        Extract::Regex(r"^([0-9.]+)".to_string()),
        ValueTargets::Glob(
            Extract::structured(Format::Json, "$['engines','volta'].node".to_string()).unwrap(),
        ),
        Vec::new(),
        Applicability::Unsafe,
    )
    .with_target_matches(ValueTargetMatches::Glob(Some(exact)));

    let out = fixer
        .apply(&equals_viol("package.json", "22.13.1"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    assert_eq!(
        read(&tmp, "package.json"),
        br#"{"engines":{"node":">=20 <23"},"volta":{"node":"22.14.0"}}"#,
        "the excluded range stays untouched while the exact live value is repaired"
    );
}

#[test]
fn value_structured_fix_leaves_a_same_value_baselined_sibling_untouched() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"22.14.0\n");
    write(
        &tmp,
        "package.json",
        br#"{"engines":{"node":"22.13.1"},"volta":{"node":"22.13.1"}}"#,
    );
    let fixer = CrossFileValueFixer::new(
        PathBuf::from("VERSION"),
        Extract::Regex(r"^([0-9.]+)".to_string()),
        ValueTargets::Glob(
            Extract::structured(Format::Json, "$['engines','volta'].node".to_string()).unwrap(),
        ),
        Vec::new(),
        Applicability::Unsafe,
    );

    let out = fixer
        .apply(
            &equals_viol_occurrence("package.json", "22.13.1", 1),
            &ctx(&tmp, false),
        )
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    assert_eq!(
        read(&tmp, "package.json"),
        br#"{"engines":{"node":"22.13.1"},"volta":{"node":"22.14.0"}}"#,
    );
}

// ─── regex-extract value propagation (Phase 2) ───────────────────────

// A value fixer with a regex source pattern + a (possibly different) regex
// target pattern.
fn value_fixer_regex(source_pat: &str, target_pat: &str) -> CrossFileValueFixer {
    CrossFileValueFixer::new(
        PathBuf::from("VERSION"),
        Extract::Regex(source_pat.to_string()),
        ValueTargets::Glob(Extract::Regex(target_pat.to_string())),
        Vec::new(),
        Applicability::Unsafe,
    )
}

#[test]
fn value_propagates_a_regex_capture_preserving_the_pattern() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"2.5.0\n");
    write(
        &tmp,
        "README.md",
        b"# Proj\n![v](https://x/badge/version-1.0.0-blue)\n",
    );
    let out = value_fixer_regex("^([0-9.]+)", "version-([0-9.]+)-")
        .apply(&equals_viol("README.md", "1.0.0"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    let after = String::from_utf8(read(&tmp, "README.md")).unwrap();
    assert!(
        after.contains("version-2.5.0-blue"),
        "capture rewritten: {after}"
    );
    assert!(
        after.contains("# Proj") && after.contains("![v]"),
        "rest preserved: {after}"
    );
}

#[test]
fn value_regex_fix_honors_the_target_match_filter() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"2.5.0\n");
    write(&tmp, "pins.txt", b"range=>=2 <3\nexact=1.0.0\n");
    let exact = regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$").unwrap();
    let fixer = value_fixer_regex(r"^([0-9.]+)", r"(?m)^(?:range|exact)=([^\r\n]+)$")
        .with_target_matches(ValueTargetMatches::Glob(Some(exact)));

    let out = fixer
        .apply(&equals_viol("pins.txt", "1.0.0"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    assert_eq!(
        read(&tmp, "pins.txt"),
        b"range=>=2 <3\nexact=2.5.0\n",
        "excluded captures must remain byte-identical"
    );
}

#[test]
fn value_regex_fix_leaves_a_same_value_baselined_sibling_untouched() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"2.5.0\n");
    write(
        &tmp,
        "Dockerfile",
        b"FROM node:1.0.0\nFROM node:1.5.0\nFROM node:1.5.0\n",
    );
    let fixer = value_fixer_regex(r"^([0-9.]+)", r"(?m)^FROM node:([0-9.]+)$");

    let out = fixer
        .apply(
            &equals_viol_occurrence("Dockerfile", "1.5.0", 1),
            &ctx(&tmp, false),
        )
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    assert_eq!(
        read(&tmp, "Dockerfile"),
        b"FROM node:1.0.0\nFROM node:1.5.0\nFROM node:2.5.0\n",
        "the suppressed same-value occurrence must remain byte-identical"
    );
}

#[test]
fn value_regex_fix_declines_projected_output_over_the_limit() {
    let tmp = TempDir::new().unwrap();
    let source = format!("{}\n", "y".repeat(100));
    write(&tmp, "VERSION", source.as_bytes());
    let target = "v=x\n".repeat(20);
    write(&tmp, "pins.txt", target.as_bytes());
    let mut limited = ctx(&tmp, false);
    limited.fix_size_limit = Some(128);

    let out = value_fixer_regex(r"^([a-z]+)", r"(?m)^v=([a-z]+)$")
        .apply(&equals_viol("pins.txt", "x"), &limited)
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref reason)
            if reason.contains("output safety limit") && !reason.contains("128")),
        "got {out:?}"
    );
    assert_eq!(read(&tmp, "pins.txt"), target.as_bytes());
}

#[test]
fn value_regex_fix_declines_excessive_capture_count() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"y\n");
    let target = "v=x\n".repeat(crate::cross_file::MAX_EXTRACTED_VALUES + 1);
    write(&tmp, "pins.txt", target.as_bytes());

    let out = value_fixer_regex(r"^([a-z])", r"(?m)^v=([a-z])$")
        .apply(&equals_viol("pins.txt", "x"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref reason) if reason.contains("value safety limit")),
        "got {out:?}"
    );
    assert_eq!(read(&tmp, "pins.txt"), target.as_bytes());
}

#[test]
fn value_fix_declines_a_source_outside_the_target_match_filter() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"latest\n");
    write(&tmp, "pins.txt", b"exact=1.0.0\n");
    let exact = regex::Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$").unwrap();
    let fixer = value_fixer_regex(r"^([a-z]+)", r"exact=([^\r\n]+)")
        .with_target_matches(ValueTargetMatches::Glob(Some(exact)));

    let out = fixer.apply(&viol("pins.txt"), &ctx(&tmp, false)).unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref reason) if reason.contains("does not satisfy")),
        "got {out:?}"
    );
    assert_eq!(read(&tmp, "pins.txt"), b"exact=1.0.0\n");
}

#[test]
fn value_regex_declines_when_the_value_breaks_the_pattern() {
    // Re-extract verify: a source value that cannot satisfy the target's capture
    // (here a digit-bearing value into a `[a-z]+` capture) must be declined --
    // never write a value the `equals` check would still reject.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.txt", b"val=DIGITS123\n");
    write(&tmp, "f.txt", b"v=old\n");
    let fixer = CrossFileValueFixer::new(
        PathBuf::from("src.txt"),
        Extract::Regex("val=(.+)".to_string()),
        ValueTargets::Glob(Extract::Regex("v=([a-z]+)".to_string())),
        Vec::new(),
        Applicability::Unsafe,
    );
    let out = fixer
        .apply(&equals_viol("f.txt", "old"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("would not satisfy the regex")),
        "a value that breaks the pattern must decline, got {out:?}"
    );
    assert_eq!(read(&tmp, "f.txt"), b"v=old\n", "target unchanged");
}

#[test]
fn value_regex_is_idempotent_when_already_equal() {
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.txt", b"tag: 9.9\n");
    write(&tmp, "t.txt", b"pinned to 9.9 here\n");
    let fixer = CrossFileValueFixer::new(
        PathBuf::from("src.txt"),
        Extract::Regex("tag: ([0-9.]+)".to_string()),
        ValueTargets::Glob(Extract::Regex("pinned to ([0-9.]+) ".to_string())),
        Vec::new(),
        Applicability::Unsafe,
    );
    let out = fixer
        .apply(&equals_viol("t.txt", "9.9"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("nothing to propagate")),
        "an already-equal capture must Skip, got {out:?}"
    );
}

#[test]
fn value_regex_declines_a_missing_capture_group() {
    // A target regex with no group 1 has nothing to set.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.txt", b"tag: 9.9\n");
    write(&tmp, "t.txt", b"nogroup 1.0\n");
    let fixer = CrossFileValueFixer::new(
        PathBuf::from("src.txt"),
        Extract::Regex("tag: ([0-9.]+)".to_string()),
        ValueTargets::Glob(Extract::Regex("nogroup".to_string())),
        Vec::new(),
        Applicability::Unsafe,
    );
    let out = fixer
        .apply(&equals_viol("t.txt", "1.0"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("no capture group 1")),
        "got {out:?}"
    );
}

#[test]
fn value_regex_declines_a_no_match_distinctly() {
    // Audit F-1: a regex that HAS a group 1 but matches NOTHING must say
    // "matched nothing", not the misleading "no capture group 1".
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"tag: 9.9\n");
    write(&tmp, "t.txt", b"unrelated content\n");
    let out = value_fixer_regex("tag: ([0-9.]+)", "v=([0-9.]+)")
        .apply(&equals_viol("t.txt", "1.0"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("matched nothing")),
        "got {out:?}"
    );
}

#[test]
fn value_regex_skips_a_non_literal_capture() {
    // Audit HIGH: an interpolated `${VERSION}` capture is a template the check
    // skips (as a note); the fixer must NOT clobber it. Here the ONLY capture is
    // non-literal -> nothing to propagate, file untouched.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"2.5.0\n");
    write(&tmp, "t.txt", b"image: myapp:${VERSION}\n");
    let out = value_fixer_regex("([0-9.]+)", "myapp:(\\S+)")
        .apply(&equals_viol("t.txt", "${VERSION}"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Skipped(_)), "got {out:?}");
    assert_eq!(
        read(&tmp, "t.txt"),
        b"image: myapp:${VERSION}\n",
        "the interpolated template must NOT be clobbered"
    );
}

#[test]
fn value_regex_clobbers_only_the_literal_drift_beside_a_template() {
    // A literal drift AND a `${VERSION}` template under one pattern: rewrite the
    // literal, LEAVE the template (audit HIGH).
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"2.5.0\n");
    write(
        &tmp,
        "t.md",
        b"badge/version-1.0.0-blue and badge/version-${VERSION}-green\n",
    );
    let out = value_fixer_regex("([0-9.]+)", "version-(\\S+?)-")
        .apply(&equals_viol("t.md", "1.0.0"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    let after = String::from_utf8(read(&tmp, "t.md")).unwrap();
    assert!(
        after.contains("version-2.5.0-blue"),
        "literal drift fixed: {after}"
    );
    assert!(
        after.contains("version-${VERSION}-green"),
        "the template must survive: {after}"
    );
}

#[test]
fn value_regex_respects_normalize() {
    // Audit HIGH/MED: under `normalize: semver-minor`, a capture in the same
    // band as the source is NOT drift -- do not clobber it. Source 2.5.0 (band
    // 2.5): "2.4.0" drifts (band 2.4), "2.5.7" does not (band 2.5).
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.txt", b"2.5.0\n");
    write(&tmp, "t.txt", b"min v([0-9.]+) ... v2.4.0 and v2.5.7\n");
    let fixer = CrossFileValueFixer::new(
        PathBuf::from("src.txt"),
        Extract::Regex("([0-9.]+)".to_string()),
        ValueTargets::Glob(Extract::Regex("v([0-9.]+)".to_string())),
        vec![Normalize::SemverMinor],
        Applicability::Unsafe,
    );
    let out = fixer
        .apply(&equals_viol("t.txt", "2.4.0"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "got {out:?}");
    let after = String::from_utf8(read(&tmp, "t.txt")).unwrap();
    assert!(
        after.contains("v2.5.0 and"),
        "the drifting 2.4.0 -> 2.5.0: {after}"
    );
    assert!(
        after.contains("v2.5.7"),
        "the in-band 2.5.7 must NOT be clobbered: {after}"
    );
}

#[test]
fn value_regex_declines_a_non_utf8_target() {
    // Audit F-2: a target with invalid UTF-8 declines cleanly (byte-offset
    // safety), never mis-splices.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "VERSION", b"9\n");
    write(&tmp, "t.txt", b"v=1 \xff\xfe raw\n");
    let out = value_fixer_regex("([0-9]+)", "v=([0-9]+)")
        .apply(&equals_viol("t.txt", "1"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("not valid UTF-8")),
        "got {out:?}"
    );
    assert_eq!(read(&tmp, "t.txt"), b"v=1 \xff\xfe raw\n", "untouched");
}

#[test]
fn value_structured_skips_a_non_literal_node() {
    // Audit HIGH: a JSON string node `${VERSION}` is a template the check skips;
    // the structured path must NOT clobber it either.
    let tmp = TempDir::new().unwrap();
    write(&tmp, "src.json", br#"{"v":"2.5.0"}"#);
    write(&tmp, "t.json", br#"{"v":"${VERSION}"}"#);
    let out = value_fixer("src.json", "$.v", Format::Json, "$.v")
        .apply(&equals_viol("t.json", "${VERSION}"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(out, FixOutcome::Skipped(ref r) if r.contains("non-literal template")),
        "got {out:?}"
    );
    assert_eq!(
        read(&tmp, "t.json"),
        br#"{"v":"${VERSION}"}"#,
        "template survives"
    );
}

// ─── CreateAndRegisterFixer: the create half (Phase 2) ───────────

fn register_fixer(content: Option<&str>) -> CreateAndRegisterFixer {
    CreateAndRegisterFixer::new(
        ValueTargets::Glob(Extract::structured(Format::Toml, "$.a").unwrap()),
        content.map(|c| alint_core::ContentSourceSpec::Inline(c.to_string())),
        Applicability::Unsafe,
    )
}

fn existence_viol(member: &str) -> Violation {
    Violation::new("missing")
        .with_path(Path::new(member))
        .with_baseline_key(format!("registered\u{0}exists\u{0}{member}"))
}

#[test]
fn create_and_register_existence_member_parses_the_key() {
    let ex = existence_viol("crates/d/Cargo.toml");
    assert_eq!(
        CreateAndRegisterFixer::existence_member(&ex),
        Some(PathBuf::from("crates/d/Cargo.toml"))
    );
    // A registration finding is NOT an existence finding, and vice versa.
    let reg =
        Violation::new("x").with_baseline_key("registered\u{0}member\u{0}Cargo.toml\u{0}crates/d");
    assert_eq!(CreateAndRegisterFixer::existence_member(&reg), None);
    assert_eq!(
        CreateAndRegisterFixer::missing_members(&reg),
        vec!["crates/d".to_string()]
    );
    assert_eq!(
        CreateAndRegisterFixer::missing_members(&ex),
        Vec::<String>::new()
    );
}

#[cfg(unix)]
#[test]
fn create_and_register_declines_a_member_its_key_names_differently() {
    // The rule checks `crates\b/...`, one name on Unix; the key slashes it.
    let tmp = TempDir::new().unwrap();
    let fixer = register_fixer(Some("[package]\n"));
    let finding = Violation::new("missing")
        .with_path(Path::new("crates\\b/Cargo.toml"))
        .with_baseline_key("registered\u{0}exists\u{0}crates/b/Cargo.toml");
    for dry_run in [true, false] {
        let out = fixer.apply(&finding, &ctx(&tmp, dry_run)).unwrap();
        let FixOutcome::Skipped(reason) = out else {
            panic!("expected a decline, got {out:?}");
        };
        assert!(
            reason.contains("write the member path with `/`"),
            "{reason}"
        );
    }
    assert!(!tmp.path().join("crates").exists());
    assert!(fixer.fix_edit(&finding, &[], tmp.path()).is_none());
    assert!(fixer.created_file(&finding, tmp.path(), false).is_none());
    // The same member written with `/` is created.
    let slashed = existence_viol("crates/b/Cargo.toml");
    assert!(fixer.fix_edit(&slashed, &[], tmp.path()).is_some());
    assert!(fixer.created_file(&slashed, tmp.path(), false).is_some());
}

#[test]
fn create_and_register_creates_a_missing_member() {
    let tmp = TempDir::new().unwrap();
    let out = register_fixer(Some("[package]\nname = \"x\"\n"))
        .apply(&existence_viol("crates/new/Cargo.toml"), &ctx(&tmp, false))
        .unwrap();
    assert!(matches!(out, FixOutcome::Applied(_)), "{out:?}");
    assert_eq!(
        read(&tmp, "crates/new/Cargo.toml"),
        b"[package]\nname = \"x\"\n"
    );
}

#[test]
fn create_and_register_create_guards() {
    let tmp = TempDir::new().unwrap();
    // A `..` member escapes the root -> confined skip, no out-of-root write.
    let out = register_fixer(Some("x"))
        .apply(&existence_viol("../evil/Cargo.toml"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(&out, FixOutcome::Skipped(m) if m.contains("escapes")),
        "{out:?}"
    );
    // No content -> skip (never a phantom-driving create).
    let out = register_fixer(None)
        .apply(&existence_viol("crates/new/Cargo.toml"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(&out, FixOutcome::Skipped(m) if m.contains("no `content`")),
        "{out:?}"
    );
    assert!(!tmp.path().join("crates/new/Cargo.toml").exists());
    // No-clobber: an existing file is left intact.
    write(&tmp, "crates/have/Cargo.toml", b"keep");
    let out = register_fixer(Some("x"))
        .apply(&existence_viol("crates/have/Cargo.toml"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(&out, FixOutcome::Skipped(m) if m.contains("already exists")),
        "{out:?}"
    );
    assert_eq!(read(&tmp, "crates/have/Cargo.toml"), b"keep");
    // A file where the member's directory should be: `create_dir_all` would
    // fail, so every mode declines.
    write(&tmp, "crates/flat", b"a file");
    for dry_run in [true, false] {
        let out = register_fixer(Some("x"))
            .apply(
                &existence_viol("crates/flat/Cargo.toml"),
                &ctx(&tmp, dry_run),
            )
            .unwrap();
        assert!(
            matches!(&out, FixOutcome::Skipped(m) if m.contains("under a file")),
            "dry_run={dry_run}: {out:?}"
        );
    }
    assert_eq!(read(&tmp, "crates/flat"), b"a file");
}

#[cfg(unix)]
#[test]
fn create_and_register_refuses_a_symlink_target() {
    use std::os::unix::fs::symlink;
    let tmp = TempDir::new().unwrap();
    // A broken symlink at the member path -> refuse to create through it.
    symlink("nonexistent", tmp.path().join("member.toml")).unwrap();
    let out = register_fixer(Some("x"))
        .apply(&existence_viol("member.toml"), &ctx(&tmp, false))
        .unwrap();
    assert!(
        matches!(&out, FixOutcome::Skipped(m) if m.contains("symlink")),
        "{out:?}"
    );
}

#[test]
fn create_and_register_dry_run_creates_nothing() {
    let tmp = TempDir::new().unwrap();
    let out = register_fixer(Some("x"))
        .apply(&existence_viol("crates/new/Cargo.toml"), &ctx(&tmp, true))
        .unwrap();
    assert!(
        matches!(&out, FixOutcome::Applied(m) if m.starts_with("would create")),
        "{out:?}"
    );
    assert!(
        !tmp.path().join("crates/new/Cargo.toml").exists(),
        "dry-run must not create the file"
    );
}

#[test]
fn create_and_register_can_fix_gates_existence_on_content() {
    // An existence finding is fixable only when the rule carries content.
    let ex = existence_viol("crates/new/Cargo.toml");
    assert!(register_fixer(Some("x")).can_fix(&ex), "content -> fixable");
    assert!(
        !register_fixer(None).can_fix(&ex),
        "no content -> not fixable"
    );
}

#[test]
fn create_and_register_names_the_member_it_creates_for_the_dry_run() {
    // `fix --dry-run` checks the created member against the walk; `fix_edit`
    // declines a member outside the root that `allow_out_of_root` lets
    // `create_member` write.
    let tmp = TempDir::new().unwrap();
    let fixer = register_fixer(Some("x"));
    let outside = existence_viol("../shared/member.toml");
    assert_eq!(
        fixer.created_file(&outside, tmp.path(), true),
        Some((PathBuf::from("../shared/member.toml"), b"x".to_vec()))
    );
    assert!(fixer.fix_edit(&outside, &[], tmp.path()).is_none());
    // A registration finding creates nothing, and neither does a rule
    // without content.
    let reg =
        Violation::new("x").with_baseline_key("registered\u{0}member\u{0}Cargo.toml\u{0}crates/d");
    assert_eq!(fixer.created_file(&reg, tmp.path(), true), None);
    assert_eq!(
        register_fixer(None).created_file(&outside, tmp.path(), true),
        None
    );
}
