//! `alint fix --changed` blast-radius gate. A full-index rule (existence /
//! cross-file) evaluates the whole tree for its check verdict even under
//! `--changed`, but the FIX must not touch files outside the working-tree diff.
//! Regression for the Phase-0 audit finding where `file_absent` + `file_remove`
//! under `--changed` DELETED unchanged committed files that the diff never
//! touched.

use std::path::{Path, PathBuf};
use std::process::Command;

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("run git")
        .status;
    assert!(status.success(), "git {args:?} failed");
}

const CONFIG: &str = "\
version: 1
rules:
  - id: no-logs
    kind: file_absent
    paths: \"**/*.log\"
    level: error
    fix: { file_remove: { applicability: safe } }
";

#[test]
fn fix_changed_does_not_touch_files_outside_the_diff() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::create_dir_all(root.join("logs")).unwrap();
    std::fs::write(root.join("build/old.log"), "old\n").unwrap();
    std::fs::write(root.join("logs/today.log"), "today\n").unwrap();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    // Modify only logs/today.log -> it is the sole working-tree-diff file.
    std::fs::write(root.join("logs/today.log"), "edited\n").unwrap();

    let out = Command::new(alint())
        .args(["fix", "--changed", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed");
    // Exit 0 or 1 both acceptable (a removed error-level violation may remain
    // as residual for the file it did remove); what matters is the blast radius.
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));

    assert!(
        root.join("build/old.log").exists(),
        "fix --changed must NOT remove the unchanged, out-of-diff build/old.log"
    );
    assert!(
        !root.join("logs/today.log").exists(),
        "the changed logs/today.log is in-scope and should be removed"
    );
}

#[test]
fn full_fix_still_removes_every_match() {
    // Sanity: WITHOUT --changed, the same rule removes both matching files, so
    // the blast-radius guard above only narrows the --changed path.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::write(root.join("build/old.log"), "old\n").unwrap();
    std::fs::write(root.join("a.log"), "a\n").unwrap();
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();

    let out = Command::new(alint())
        .args(["fix", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix");
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));
    assert!(
        !root.join("build/old.log").exists(),
        "full fix removes all matches"
    );
    assert!(!root.join("a.log").exists(), "full fix removes all matches");
}

const CREATE_CONFIG: &str = "\
version: 1
rules:
  - id: need-readme
    kind: file_exists
    paths: README.md
    root_only: true
    level: error
    fix: { file_create: { content: \"# R\\n\" } }
";

#[test]
fn fix_changed_recreates_a_required_file_deleted_in_the_diff() {
    // A `file_exists` create violation is PATHLESS (its target comes from config,
    // not the violation). The --changed blast-radius filter must NOT drop it, or
    // `fix --changed` would never create a required file -- not even one deleted
    // in the very diff being fixed (which IS in the changed set and re-fires the
    // existence rule). Regression guard: the round-2 filter's `is_some_and`
    // wrongly dropped pathless violations.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("README.md"), "# R\n").unwrap();
    std::fs::write(root.join(".alint.yml"), CREATE_CONFIG).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    // Delete README.md -> it is in the working-tree diff (changed set).
    std::fs::remove_file(root.join("README.md")).unwrap();

    let out = Command::new(alint())
        .args(["fix", "--changed", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed");
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));
    assert!(
        root.join("README.md").exists(),
        "fix --changed must re-create a required file deleted in the diff"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("README.md")).unwrap(),
        "# R\n"
    );
}

const CONFINE_CASCADE_CONFIG: &str = "\
version: 1
rules:
  - id: has-req
    kind: file_exists
    paths: req.md
    root_only: true
    level: error
    fix: { file_create: { content: \"# req   \\n\" } }
  - id: no-ws
    kind: no_trailing_whitespace
    paths: \"**/*.md\"
    level: error
    fix: { file_trim_trailing_whitespace: {} }
";

#[test]
fn fix_changed_confinement_holds_across_the_multipass_fixpoint() {
    // The Phase-1 fixpoint re-walks after each pass. Under --changed the changed
    // set is frozen (computed once, before the loop), so a per-file content rule
    // stays confined to it on EVERY pass. This gates two things at once:
    //   * an IN-scope file's create-then-content cascade completes ACROSS passes
    //     (req.md is deleted-in-diff, so it is in the changed set: pass 1 recreates
    //     it with trailing whitespace, pass 2's re-walk lets no-ws trim it);
    //   * an OUT-of-scope file is never touched, even though every re-walk sees it
    //     (other.md has trailing whitespace but is not in the diff).
    // Regression guard for the design's "2a is safe under --changed" claim -- a
    // re-walk that leaked newly-seen files into the per-file filtered index would
    // trim other.md here. (Audit finding: multi-pass --changed was ungated.)
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("req.md"), "# req\n").unwrap();
    std::fs::write(root.join("other.md"), "x   \n").unwrap();
    std::fs::write(root.join(".alint.yml"), CONFINE_CASCADE_CONFIG).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    // Delete req.md -> the sole working-tree-diff file (the changed set).
    std::fs::remove_file(root.join("req.md")).unwrap();

    let out = Command::new(alint())
        .args(["fix", "--changed", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed");
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));
    assert_eq!(
        std::fs::read_to_string(root.join("req.md")).unwrap(),
        "# req\n",
        "the in-scope create-then-trim cascade must complete across the re-walk"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("other.md")).unwrap(),
        "x   \n",
        "fix --changed must not trim an out-of-diff file on any pass"
    );
}

#[test]
fn fix_changed_demotes_out_of_scope_write_to_a_suggestion() {
    // 2b: a full-index rule with SOME in-scope target still RUNS under `--changed`,
    // but a fix whose target is OUTSIDE the changed set is DEMOTED to a Suggestion --
    // surfaced (so the user can see and apply it) rather than APPLIED (which would
    // widen the blast radius to an untouched committed file) or silently DROPPED
    // (the pre-2b behavior, which let `fix --changed` exit 0 while `check --changed`
    // reported the same file and exited 1). This closes that check/fix divergence.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::create_dir_all(root.join("logs")).unwrap();
    std::fs::write(root.join("build/old.log"), "old\n").unwrap(); // committed, OUT of diff
    std::fs::write(root.join("logs/today.log"), "today\n").unwrap(); // will be edited (IN diff)
    std::fs::write(root.join(".alint.yml"), CONFIG).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    std::fs::write(root.join("logs/today.log"), "edited\n").unwrap();

    let out = Command::new(alint())
        .args(["fix", "--changed", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed");
    let stdout = String::from_utf8_lossy(&out.stdout);

    // In-scope removal applied; out-of-scope removal only suggested.
    assert!(
        !root.join("logs/today.log").exists(),
        "the in-scope logs/today.log is removed"
    );
    assert!(
        root.join("build/old.log").exists(),
        "the out-of-scope build/old.log must NOT be removed -- only suggested"
    );
    assert!(
        stdout.contains("1 suggested") && stdout.to_lowercase().contains("scope"),
        "the out-of-scope removal must surface as a Suggestion naming the scope; got:\n{stdout}"
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "a standing out-of-scope Suggestion is unresolved -> exit 1 (not a silent exit 0)"
    );

    // Agreement: on the resulting tree, `check --changed` still reports the standing
    // out-of-scope violation and exits 1 -- the same verdict `fix` reached.
    let check = Command::new(alint())
        .args(["check", "--changed", "."])
        .current_dir(root)
        .output()
        .expect("run alint check --changed");
    assert_eq!(
        check.status.code(),
        Some(1),
        "check --changed agrees: the out-of-scope violation still stands"
    );
    assert!(
        root.join("build/old.log").exists(),
        "check does not mutate; build/old.log is still there"
    );
}

#[test]
fn fix_changed_confinement_wins_over_unsafe_fixes() {
    // 2b invariant: `--changed` confinement is INDEPENDENT of the safety tier. The
    // out-of-scope demote is checked BEFORE the tier arms, so even `--unsafe-fixes`
    // (which raises the applied tier to Unsafe) must NOT apply an out-of-scope
    // write. Here `file_remove` is Unsafe by DEFAULT (no `applicability: safe`), so
    // `--unsafe-fixes` applies the in-scope removal -- but the out-of-scope one is
    // still only suggested. Guards against a regression that reordered the status
    // arms so the tier check ran first.
    const UNSAFE_CONFIG: &str = "\
version: 1
rules:
  - id: no-logs
    kind: file_absent
    paths: \"**/*.log\"
    level: error
    fix: { file_remove: {} }
";
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::create_dir_all(root.join("logs")).unwrap();
    std::fs::write(root.join("build/old.log"), "old\n").unwrap(); // OUT of diff
    std::fs::write(root.join("logs/today.log"), "today\n").unwrap(); // IN diff
    std::fs::write(root.join(".alint.yml"), UNSAFE_CONFIG).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    std::fs::write(root.join("logs/today.log"), "edited\n").unwrap();

    let out = Command::new(alint())
        .args(["fix", "--changed", "--unsafe-fixes", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed --unsafe-fixes");
    // In-scope Unsafe removal APPLIES (--unsafe-fixes); out-of-scope stays suggested.
    assert!(
        !root.join("logs/today.log").exists(),
        "in-scope Unsafe removal applies under --unsafe-fixes"
    );
    assert!(
        root.join("build/old.log").exists(),
        "out-of-scope removal must NOT be applied even with --unsafe-fixes (confinement \
         wins over the tier)"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_lowercase().contains("scope"),
        "the out-of-scope removal is surfaced as an out-of-scope suggestion; got:\n{stdout}"
    );
}

#[test]
fn fix_changed_renames_only_in_scope_files() {
    // 2b (rename interaction): `filename_case` + `file_rename` writes a NEW name,
    // which `writes_outside_changed` does not directly inspect. But the rename is
    // confined the RIGHT way regardless: `filename_case` gets the filtered index
    // (`requires_full_index == false`), so it only EVALUATES in-diff (+ created)
    // files -- an out-of-diff mis-named file is never seen, so never renamed. The
    // in-scope rename's new name is a "file a fix in scope created" (spec-permitted)
    // and cannot clobber an existing file (the fixer's collision guard). Guards
    // against a regression that leaked out-of-diff files into the rename path.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("Src.md"), "a\n").unwrap(); // PascalCase, will be edited (IN diff)
    std::fs::write(root.join("Docs.md"), "b\n").unwrap(); // PascalCase, committed (OUT of diff)
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: kebab\n    kind: filename_case\n    paths: \"**/*.md\"\n    \
         case: kebab\n    level: error\n    fix: { file_rename: {} }\n",
    )
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    std::fs::write(root.join("Src.md"), "a2\n").unwrap(); // Src.md now IN diff

    let out = Command::new(alint())
        .args(["fix", "--changed", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed");
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));
    // In-scope Src.md renamed to src.md (its new name is an allowed created file).
    assert!(
        root.join("src.md").exists(),
        "in-scope Src.md is renamed to src.md"
    );
    assert!(
        !root.join("Src.md").exists(),
        "the in-scope original is gone"
    );
    // Out-of-diff Docs.md is never evaluated, so never renamed or created.
    assert!(
        root.join("Docs.md").exists(),
        "out-of-diff Docs.md must NOT be renamed under --changed"
    );
    assert!(
        !root.join("docs.md").exists(),
        "no out-of-diff rename target may be created"
    );
}

#[test]
fn located_replace_under_changed_is_confined_to_the_diff() {
    // 2b + Phase-1 located regime (whole-phase audit, F1/LG-1): the ONLY located
    // fixer, `replace` on the per-file `file_content_forbidden`, must respect the
    // `--changed` blast radius. Unlike whole-file fixers it is NOT gated by
    // `writes_outside_changed`; its confinement comes entirely from the FILTERED
    // INDEX (`pick_ctx` hands a per-file rule the changed-filtered ctx), so an
    // out-of-diff file is never even evaluated, hence never spliced. This is the
    // first committed test of a located fixer under `--changed`, and it exercises
    // the located branch's `debug_assert!(changed_paths.is_none() ||
    // as_per_file().is_some())` on the PASSING path (a real per-file host under
    // `--changed`). `replace` defaults to Unsafe, so `--unsafe-fixes` arms it.
    const REPLACE_CONFIG: &str = "\
version: 1
rules:
  - id: no-todo
    kind: file_content_forbidden
    paths: \"**/*.txt\"
    pattern: TODO
    level: error
    fix: { replace: { replacement: DONE } }
";
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("in_scope.txt"), "TODO here\n").unwrap();
    std::fs::write(root.join("out_of_diff.txt"), "TODO there\n").unwrap();
    std::fs::write(root.join(".alint.yml"), REPLACE_CONFIG).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    // Edit ONLY in_scope.txt (still contains TODO) -> it is the sole diff file.
    std::fs::write(root.join("in_scope.txt"), "TODO here, edited\n").unwrap();

    let out = Command::new(alint())
        .args(["fix", "--changed", "--unsafe-fixes", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed --unsafe-fixes");
    assert!(out.status.code() == Some(0) || out.status.code() == Some(1));

    // In-diff file: the located replace applied (TODO -> DONE).
    assert_eq!(
        std::fs::read_to_string(root.join("in_scope.txt")).unwrap(),
        "DONE here, edited\n",
        "the in-diff located replace must apply under --changed --unsafe-fixes"
    );
    // Out-of-diff file: never evaluated (filtered index), so its TODO survives.
    assert_eq!(
        std::fs::read_to_string(root.join("out_of_diff.txt")).unwrap(),
        "TODO there\n",
        "the out-of-diff file must NOT be spliced -- located confinement via the \
         filtered index, independent of --unsafe-fixes"
    );
}

#[test]
fn fix_changed_demotes_an_out_of_scope_value_propagation_to_a_suggestion() {
    // The value-propagation `sync_from` (cross_file relation: equals) is a
    // WHOLE-FILE apply fixer on a `requires_full_index` rule; the "no engine change
    // was needed" design rests on the whole-file path's `writes_outside_changed`
    // demote applying to it. Assert it: with only the SOURCE changed, a drifting
    // OUT-of-diff target is SUGGESTED, never silently written (a blast-radius
    // escape). (audit gap: no test covered the value fixer under --changed.)
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir_all(root.join("crates/pkg")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace.package]\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    // Committed IN SYNC, so it is out of the diff after we bump only the source.
    std::fs::write(
        root.join("crates/pkg/Cargo.toml"),
        "[package]\nname = \"pkg\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n  - id: ver\n    kind: cross_file\n    relation: equals\n    \
         source: { file: Cargo.toml, extract: { toml: \"$.workspace.package.version\" } }\n    \
         targets: { files: \"crates/*/Cargo.toml\", extract: { toml: \"$.package.version\" } }\n    \
         level: error\n    fix: { sync_from: {} }\n",
    )
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", "base"]);
    // Bump ONLY the source; crates/pkg/Cargo.toml is now out-of-diff and drifting.
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace.package]\nversion = \"2.0.0\"\n",
    )
    .unwrap();

    let out = Command::new(alint())
        .args(["fix", "--changed", "--unsafe-fixes", "."])
        .current_dir(root)
        .output()
        .expect("run alint fix --changed --unsafe-fixes");
    let stdout = String::from_utf8_lossy(&out.stdout);

    // The out-of-scope target must NOT be written -- only suggested.
    assert_eq!(
        std::fs::read_to_string(root.join("crates/pkg/Cargo.toml")).unwrap(),
        "[package]\nname = \"pkg\"\nversion = \"1.0.0\"\n",
        "the out-of-scope value propagation must NOT be applied -- only suggested"
    );
    assert!(
        stdout.contains("1 suggested") && stdout.to_lowercase().contains("scope"),
        "the out-of-scope value propagation must surface as a scope-naming Suggestion; got:\n{stdout}"
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "a standing out-of-scope Suggestion is unresolved -> exit 1"
    );
}
