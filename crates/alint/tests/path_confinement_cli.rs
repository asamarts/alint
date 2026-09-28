//! Path-confinement scenarios, end-to-end through the real binary.
//!
//! Two post-v0.13 audit fixes closed read/disclosure oracles reachable from an
//! untrusted config. Both were *symlink-blind* before the fix — lexical
//! confinement passes a path whose components are all `Normal`, even when an
//! in-repo symlink makes it resolve outside the tree. Each fix added a
//! `canonicalize` + re-containment check. Existing coverage exercises the
//! confinement *helpers* and the *lexical* escape (`../`, absolute paths); what
//! was missing is the **symlink** escape driven through the real rule / loader
//! — the attacker's actual surface. These fill that gap:
//!
//! * **H1** — a direct-read rule (`pair_hash`, one of the config-derived-read
//!   sites the fix hardened) whose `target:` resolves out of the repo via an
//!   in-repo symlink must NOT read it (reports "escapes the repo root"), and
//!   reads it only under a top-level `allow_out_of_root`. The out-of-root
//!   manifest deliberately carries the matching digest, so the confinement
//!   decision is the *only* thing that flips pass/fail — proving the no-flag
//!   run refused to read rather than merely mismatched. (`json_schema_passes`,
//!   `cross_file`, `registry_paths_resolve` share the same guarded read path.)
//! * **M2** — a local `extends:` whose target resolves out of the config's
//!   directory via an in-tree symlink is rejected at load (exit 2), liftable
//!   only by a top-level `allow_out_of_root`.
//!
//! `#![cfg(unix)]` — the escape vector is a symlink.

#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Output;

/// `sha256("hello")` — the digest the `pair_hash` `target:` manifest carries.
const HELLO_SHA256: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

fn alint_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn run(dir: &Path, args: &[&str]) -> Output {
    std::process::Command::new(alint_bin())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

/// `base/{repo,outside}` where `repo/link` is a symlink to `outside` (out of
/// the repo root). Returns `(base_guard, repo, outside)`; keep `base_guard`
/// alive for the test's duration.
fn repo_with_symlink_out() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let base = tempfile::Builder::new()
        .prefix("alint-confine-")
        .tempdir()
        .unwrap();
    let repo = base.path().join("repo");
    let outside = base.path().join("outside");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    symlink(&outside, repo.join("link")).unwrap();
    (base, repo, outside)
}

fn out_text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

// ─── H1 — a direct-read rule is confined against a symlink escape ─────

#[test]
fn h1_pair_hash_symlink_target_is_confined_not_read() {
    let (_base, repo, outside) = repo_with_symlink_out();
    std::fs::write(repo.join("a.txt"), "hello").unwrap();
    // The out-of-root manifest DOES carry a.txt's digest, so reading it would
    // make the check pass. Confinement is the only lever on the outcome.
    std::fs::write(outside.join("pin.txt"), format!("HASH = {HELLO_SHA256}\n")).unwrap();

    let config = |allow: bool| {
        format!(
            "version: 1\n{}rules:\n  - id: pin-check\n    kind: pair_hash\n    \
             source: \"a.txt\"\n    target: \"link/pin.txt\"\n    algorithm: sha256\n    \
             format: contains\n    level: error\n",
            if allow {
                "allow_out_of_root: true\n"
            } else {
                ""
            }
        )
    };

    // No flag → the symlinked target resolves out of root, so pair_hash reports
    // the escape and never reads it: exit 1, "escapes the repo root". (Pre-fix,
    // symlink-blind, it would have read the matching digest and passed — a
    // silent out-of-root read.)
    std::fs::write(repo.join(".alint.yml"), config(false)).unwrap();
    let out = run(&repo, &["check", ".", "--format", "human"]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected a violations exit; got {:?}\n{}",
        out.status.code(),
        out_text(&out)
    );
    assert!(
        out_text(&out).contains("escapes the repo root"),
        "expected an out-of-root escape violation, not a silent read:\n{}",
        out_text(&out)
    );

    // Top-level allow_out_of_root → the same target IS read; the digest matches
    // → exit 0. The flag is the only difference, so this proves the no-flag run
    // refused to read (rather than read-and-mismatched).
    std::fs::write(repo.join(".alint.yml"), config(true)).unwrap();
    let out = run(&repo, &["check", ".", "--format", "human"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "with allow_out_of_root the matching digest should pass:\n{}",
        out_text(&out)
    );
}

// ─── M2 — a local `extends:` is confined against a symlink escape ─────

#[test]
fn m2_local_extends_through_in_tree_symlink_is_rejected() {
    let (_base, repo, outside) = repo_with_symlink_out();
    // The out-of-root ruleset a traversal/exfil oracle would target.
    std::fs::write(
        outside.join("secret.yml"),
        "version: 1\nrules:\n  - id: inherited\n    kind: file_exists\n    paths: X.md\n    level: warning\n",
    )
    .unwrap();

    // extends: through the in-tree symlink resolves out of the config dir →
    // rejected at load (exit 2), naming the escape. This is the symlink vector
    // the fix's `canonicalize` specifically catches (existing tests cover only
    // the `../` form).
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nextends: [link/secret.yml]\nrules: []\n",
    )
    .unwrap();
    let out = run(&repo, &["check", "."]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "a symlinked extends escape must be a config error (exit 2); got {:?}\n{}",
        out.status.code(),
        out_text(&out)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("outside the lint root"),
        "expected an out-of-root extends rejection:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Top-level allow_out_of_root lifts it (the deliberate escape hatch): the
    // extends resolves and the inherited rule loads, so the run is no longer a
    // config error (exit != 2).
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nextends: [link/secret.yml]\nrules: []\n",
    )
    .unwrap();
    let out = run(&repo, &["check", "."]);
    assert_ne!(
        out.status.code(),
        Some(2),
        "allow_out_of_root should lift the extends confinement:\n{}",
        out_text(&out)
    );
}

// ─── FIXER confinement — a config-declared fix path can't escape the root ─────
//
// The post-v0.13 audit confined every rule *read* against an untrusted
// `extends:`'d ruleset, but the *fixer* write/read paths were left unguarded:
// `alint fix` on a `file_create` whose `path:` (or `content_from:`) is `../…`
// wrote/read outside the repo root. These lock that down: the escape is refused
// unless the user's own top-level config opts in with `allow_out_of_root`.

/// A `file_create` whose target escapes the root must NOT be written on
/// `alint fix` (no top-level `allow_out_of_root`) — the fix is skipped and the
/// out-of-root file never appears.
#[test]
fn fixer_file_create_out_of_root_target_is_refused() {
    let base = tempfile::Builder::new()
        .prefix("alint-fixesc-")
        .tempdir()
        .unwrap();
    let repo = base.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nrules:\n  - id: seed\n    kind: file_exists\n    level: error\n    \
         paths: [\"../PWNED.txt\"]\n    fix:\n      file_create:\n        \
         path: \"../PWNED.txt\"\n        content: \"pwned\\n\"\n",
    )
    .unwrap();

    let out = run(&repo, &["fix", "."]);
    let pwned = base.path().join("PWNED.txt");
    assert!(
        !pwned.exists(),
        "fixer wrote OUTSIDE the repo root — confinement escape: {}\n{}",
        pwned.display(),
        out_text(&out)
    );
    // The human formatter wraps the skip reason across lines, so normalize
    // whitespace before matching the phrase.
    let normalized = out_text(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        normalized.contains("escapes the repo root"),
        "the refusal should say why; got:\n{}",
        out_text(&out)
    );
}

/// The escape hatch works: with a top-level `allow_out_of_root: true`, the same
/// `file_create` IS permitted to write out of the root (the intended opt-in).
#[test]
fn fixer_file_create_out_of_root_allowed_with_top_level_opt_in() {
    let base = tempfile::Builder::new()
        .prefix("alint-fixok-")
        .tempdir()
        .unwrap();
    let repo = base.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nrules:\n  - id: seed\n    kind: file_exists\n    \
         level: error\n    paths: [\"../ALLOWED.txt\"]\n    fix:\n      file_create:\n        \
         path: \"../ALLOWED.txt\"\n        content: \"ok\\n\"\n",
    )
    .unwrap();

    run(&repo, &["fix", "."]);
    let allowed = base.path().join("ALLOWED.txt");
    assert!(
        allowed.exists(),
        "top-level allow_out_of_root must permit the out-of-root create (escape hatch)"
    );
}

/// `content_from` pointing out of the root must NOT read an out-of-tree secret
/// into an in-repo file (exfiltration) without the opt-in.
#[test]
fn fixer_content_from_out_of_root_is_refused() {
    let base = tempfile::Builder::new()
        .prefix("alint-exfil-")
        .tempdir()
        .unwrap();
    let repo = base.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(base.path().join("secret.txt"), "TOPSECRET\n").unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nrules:\n  - id: exfil\n    kind: file_exists\n    level: error\n    \
         paths: [\"stolen.txt\"]\n    fix:\n      file_create:\n        \
         path: \"stolen.txt\"\n        content_from: \"../secret.txt\"\n",
    )
    .unwrap();

    run(&repo, &["fix", "."]);
    let stolen = repo.join("stolen.txt");
    let leaked = std::fs::read_to_string(&stolen).unwrap_or_default();
    assert!(
        !leaked.contains("TOPSECRET"),
        "content_from exfiltrated an out-of-root secret into the repo: {leaked:?}"
    );
}

#[test]
fn content_providing_fixers_refuse_out_of_root_content_from() {
    // AUDIT M3: `file_create`'s content_from confinement is covered above; the
    // sibling content-providing ops (`file_prepend`, `insert_header`, and, sharing
    // the identical `resolve_source_bytes` path, `file_append`) must refuse an
    // out-of-root `content_from:` too -- never prepend/insert a secret into a file.
    let base = tempfile::Builder::new()
        .prefix("alint-exfil2-")
        .tempdir()
        .unwrap();
    let repo = base.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(base.path().join("secret.txt"), "TOPSECRET\n").unwrap();
    std::fs::write(repo.join("pre.txt"), "body\n").unwrap();
    std::fs::write(repo.join("hdr.txt"), "body\n").unwrap();
    std::fs::write(
        repo.join(".alint.yml"),
        "version: 1\nrules:\n  \
         - id: pre\n    kind: file_header\n    paths: [\"pre.txt\"]\n    pattern: \"NEEDLE\"\n    \
         level: error\n    fix: { file_prepend: { content_from: \"../secret.txt\" } }\n  \
         - id: hdr\n    kind: file_header\n    paths: [\"hdr.txt\"]\n    pattern: \"NEEDLE\"\n    \
         level: error\n    fix: { insert_header: { content_from: \"../secret.txt\" } }\n",
    )
    .unwrap();

    run(&repo, &["fix", "."]);
    for f in ["pre.txt", "hdr.txt"] {
        let content = std::fs::read_to_string(repo.join(f)).unwrap_or_default();
        assert!(
            !content.contains("TOPSECRET"),
            "{f}: content_from exfiltrated an out-of-root secret: {content:?}"
        );
    }
}
