//! Write-side confinement (audit 2026-10, finding 5): commands that WRITE
//! files must not be steered outside the repository by repo content — a
//! `baseline:` key pointing out of the tree, a committed symlink at the
//! default baseline path, or a dangling `.alint.yml` symlink at `init`.
//! Each must fail with exit 2 and leave the outside file untouched.
//!
//! `#![cfg(unix)]` — symlink creation.

#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::Output;

fn run(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

const CONFIG: &str = "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
                      paths: \"*.txt\"\n    pattern: TODO\n    level: error\n";

/// `repo/` (with a TODO violation) beside `out/` (the attacker's target).
fn layout() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    let out = tmp.path().join("out");
    std::fs::create_dir(&repo).unwrap();
    std::fs::create_dir(&out).unwrap();
    std::fs::write(repo.join("a.txt"), "TODO\n").unwrap();
    (tmp, repo, out)
}

#[test]
fn baseline_key_escaping_the_repo_is_refused() {
    let (_tmp, repo, out) = layout();
    std::fs::write(
        repo.join(".alint.yml"),
        format!("{CONFIG}baseline: ../out/victim.txt\n"),
    )
    .unwrap();
    std::fs::write(out.join("victim.txt"), "precious\n").unwrap();

    let o = run(&repo, &["baseline", "--accept-new"]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("outside the repository"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(out.join("victim.txt")).unwrap(),
        "precious\n"
    );
}

#[test]
fn baseline_through_a_committed_symlink_is_refused() {
    let (_tmp, repo, out) = layout();
    std::fs::write(repo.join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(out.join("victim2.txt"), "precious\n").unwrap();
    symlink("../out/victim2.txt", repo.join(".alint-baseline.json")).unwrap();

    let o = run(&repo, &["baseline", "--accept-new"]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(
        String::from_utf8_lossy(&o.stderr).contains("symlink"),
        "{o:?}"
    );
    assert_eq!(
        std::fs::read_to_string(out.join("victim2.txt")).unwrap(),
        "precious\n"
    );
    assert!(
        std::fs::symlink_metadata(repo.join(".alint-baseline.json"))
            .unwrap()
            .file_type()
            .is_symlink(),
        "the symlink itself is left alone"
    );
}

#[test]
fn baseline_in_a_symlinked_directory_escaping_the_repo_is_refused() {
    let (_tmp, repo, out) = layout();
    symlink("../out", repo.join("link")).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        format!("{CONFIG}baseline: link/victim3.txt\n"),
    )
    .unwrap();

    let o = run(&repo, &["baseline", "--accept-new"]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(!out.join("victim3.txt").exists());
}

#[test]
fn baseline_inside_the_repo_still_writes() {
    let (_tmp, repo, _out) = layout();
    std::fs::create_dir(repo.join("ci")).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        format!("{CONFIG}baseline: ci/baseline.json\n"),
    )
    .unwrap();
    let o = run(&repo, &["baseline"]);
    assert!(o.status.success(), "{o:?}");
    assert!(repo.join("ci/baseline.json").is_file());
    // Regenerating replaces the regular file in place (temp + rename).
    let o = run(&repo, &["baseline"]);
    assert!(o.status.success(), "{o:?}");
    let o = run(&repo, &["check"]);
    assert!(o.status.success(), "baseline honored: {o:?}");
}

#[test]
fn baseline_explicit_output_flag_may_point_anywhere() {
    // `--output` is the user's own explicit choice on the command line, not
    // repo content, so it is not root-confined.
    let (_tmp, repo, out) = layout();
    std::fs::write(repo.join(".alint.yml"), CONFIG).unwrap();
    let target = out.join("b.json");
    let o = run(&repo, &["baseline", "--output", target.to_str().unwrap()]);
    assert!(o.status.success(), "{o:?}");
    assert!(target.is_file());
}

#[test]
fn init_refuses_a_dangling_config_symlink() {
    let (_tmp, repo, out) = layout();
    symlink("../out/created.yml", repo.join(".alint.yml")).unwrap();

    let o = run(&repo, &["init"]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(
        !out.join("created.yml").exists(),
        "init must not create a file outside the repo through the symlink"
    );
}

#[test]
fn baseline_through_symlink_dotdot_is_refused() {
    // `link/..` cancels lexically, but the OS resolves it through the
    // symlink: `link -> ../out/deep` makes `link/../x` land in `out/`.
    let (_tmp, repo, out) = layout();
    std::fs::create_dir(out.join("deep")).unwrap();
    symlink("../out/deep", repo.join("link")).unwrap();
    std::fs::write(out.join("victim4.json"), "precious\n").unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        format!("{CONFIG}baseline: link/../victim4.json\n"),
    )
    .unwrap();

    let o = run(&repo, &["baseline", "--accept-new"]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert_eq!(
        std::fs::read_to_string(out.join("victim4.json")).unwrap(),
        "precious\n"
    );
    std::fs::write(
        repo.join(".alint.yml"),
        format!("{CONFIG}baseline: link/../pwned.json\n"),
    )
    .unwrap();
    let o = run(&repo, &["baseline"]);
    assert_eq!(o.status.code(), Some(2), "{o:?}");
    assert!(!out.join("pwned.json").exists());
    assert!(!repo.join("pwned.json").exists());
}

#[test]
fn baseline_regeneration_preserves_file_mode() {
    use std::os::unix::fs::PermissionsExt as _;
    let (_tmp, repo, _out) = layout();
    std::fs::write(repo.join(".alint.yml"), CONFIG).unwrap();
    let b = repo.join(".alint-baseline.json");
    let o = run(&repo, &["baseline"]);
    assert!(o.status.success(), "{o:?}");
    std::fs::set_permissions(&b, std::fs::Permissions::from_mode(0o600)).unwrap();
    let o = run(&repo, &["baseline", "--accept-new"]);
    assert!(o.status.success(), "{o:?}");
    let mode = std::fs::metadata(&b).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "mode {mode:o}");
}
