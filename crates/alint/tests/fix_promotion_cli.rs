//! A fix's `applicability` from an `extends:`'d ruleset or a nested config,
//! end to end: such a config may demote a fix but never promote one to
//! auto-apply, and a tier is a string. serde writes an enum variant as a
//! one-entry mapping (`{safe: ~}`) or a tag (`!safe ~`); either one, for every
//! fix op, is refused at load, and a bare `alint fix` leaves the file.

use std::path::Path;
use std::process::Output;

fn run(dir: &Path, args: &[&str]) -> Output {
    crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

/// A rule whose fix is `op` with `applicability` written as `written`.
fn rule(op: &str, written: &str) -> String {
    format!(
        "rules:\n  - id: no-junk\n    kind: file_absent\n    paths: junk.log\n    \
         level: error\n    fix:\n      {op}:\n        applicability: {written}\n"
    )
}

/// Whether `stderr` refuses the fix for its `applicability`, or, for an op
/// that spawns a process, which such a config may not use at all, for that.
fn refused(op: &str, stderr: &str) -> bool {
    stderr.contains("applicability")
        || (alint_dsl::SPAWNING_FIX_OPS.contains(&op)
            && stderr.contains(&format!("`fix.{op}` spawns a process")))
}

/// Write the config and the file `junk` names under `dir`, run a bare
/// `alint fix`, and say what went wrong, if anything.
fn promotion_failure(
    dir: &Path,
    files: &[(&str, String)],
    junk: &str,
    op: &str,
    case: &str,
) -> Option<String> {
    for (path, content) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }
    std::fs::write(dir.join(junk), "x\n").unwrap();
    let out = run(dir, &["fix"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !dir.join(junk).exists() {
        return Some(format!("{case}: the file was removed"));
    }
    if out.status.success() || !refused(op, &stderr) {
        return Some(format!(
            "{case}: not refused for its applicability: {stderr}"
        ));
    }
    None
}

#[test]
fn an_extended_or_nested_config_cannot_promote_a_fix_with_a_variant_shape() {
    // Every case runs, so a failure lists each op and shape that got through.
    let mut failures = Vec::new();
    for op in alint_core::FixSpec::ALL_OP_NAMES {
        for written in ["{safe: ~}", "!safe ~"] {
            // From a local `extends:` file.
            let dir = tempfile::tempdir().unwrap();
            failures.extend(promotion_failure(
                dir.path(),
                &[
                    (".alint.yml", "version: 1\nextends:\n  - base.yml\n".into()),
                    ("base.yml", format!("version: 1\n{}", rule(op, written))),
                ],
                "junk.log",
                op,
                &format!("extends {op} {written}"),
            ));
            // From a nested config.
            let dir = tempfile::tempdir().unwrap();
            failures.extend(promotion_failure(
                dir.path(),
                &[
                    (".alint.yml", "version: 1\nnested_configs: true\n".into()),
                    (
                        "sub/.alint.yml",
                        format!("version: 1\n{}", rule(op, written)),
                    ),
                ],
                "sub/junk.log",
                op,
                &format!("nested {op} {written}"),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn the_users_own_config_still_promotes_with_the_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("junk.log"), "x\n").unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        format!("version: 1\n{}", rule("file_remove", "safe")),
    )
    .unwrap();
    let out = run(dir.path(), &["fix"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!dir.path().join("junk.log").exists());
}
