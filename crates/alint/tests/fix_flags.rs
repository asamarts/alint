//! CLI behavior for `alint fix` tier flags. `--fix-only` reports only what was
//! applied (residual findings suppressed) and exits 0 unless a fix errored;
//! the plain `fix` reports residuals and exits nonzero on an unfixable error.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

/// `alint` with git's global and system config, and the excludes files
/// `ignore` falls back to, pointed into an empty `home`, so a developer's
/// global excludes file (`.env` is a common entry) cannot change the walk.
fn isolated_alint(home: &Path) -> Command {
    let config = home.join("gitconfig");
    std::fs::write(&config, "").unwrap();
    let mut command = Command::new(alint());
    command
        .env("GIT_CONFIG_GLOBAL", &config)
        .env("GIT_CONFIG_SYSTEM", &config)
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("HOME", home)
        .env("USERPROFILE", home);
    command
}

fn run(dir: &Path, args: &[&str]) -> Output {
    let home = tempfile::tempdir().unwrap();
    isolated_alint(home.path())
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn alint")
}

// A fixable rule (trailing whitespace) plus an unfixable one (a forbidden
// pattern with no fix): the fix applies the first and leaves the second as an
// error-level residual.
const CONFIG: &str = "\
version: 1
rules:
  - id: no-ws
    kind: no_trailing_whitespace
    paths: \"**/*.rs\"
    level: error
    fix:
      file_trim_trailing_whitespace: {}
  - id: no-todo
    kind: file_content_forbidden
    paths: \"**/*.rs\"
    pattern: TODO
    level: error
";

fn setup() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".alint.yml"), CONFIG).unwrap();
    std::fs::write(tmp.path().join("a.rs"), "fn a() {}   \nTODO\n").unwrap();
    tmp
}

#[test]
fn dry_run_reports_the_same_skip_as_the_real_run_for_an_oversized_file() {
    // Round-4 audit: `fix --dry-run` must run the same read/size/binary guards as
    // the real `fix` and report Skipped for files the real run skips -- not a
    // false optimistic "would apply". A gate using `--dry-run` must not
    // false-green. Here `fix_size_limit` makes the only file oversized.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nfix_size_limit: 100\nrules:\n  - id: no-ws\n    \
         kind: no_trailing_whitespace\n    paths: \"**/*.rs\"\n    level: error\n    \
         fix:\n      file_trim_trailing_whitespace: {}\n",
    )
    .unwrap();
    let big = "let _ = 1;   \n".repeat(50); // > 100 bytes, all with trailing ws
    std::fs::write(root.join("big.rs"), &big).unwrap();

    let dry = run(root, &["fix", "--dry-run", "."]);
    let real = run(root, &["fix", "."]);
    let dry_out = String::from_utf8_lossy(&dry.stdout);
    let real_out = String::from_utf8_lossy(&real.stdout);

    // Both must report the file as SKIPPED (not applied), and agree on the exit
    // code -- the dry run cannot be optimistic where the real run declines.
    assert!(
        dry_out.contains("0 applied") && dry_out.contains("1 skipped"),
        "dry-run must skip the oversized file, not report would-apply; got:\n{dry_out}"
    );
    assert!(
        real_out.contains("0 applied") && real_out.contains("1 skipped"),
        "real run skips the oversized file; got:\n{real_out}"
    );
    assert_eq!(
        dry.status.code(),
        real.status.code(),
        "dry-run and real fix must agree on the exit code"
    );
    // The tree is untouched by both.
    assert_eq!(std::fs::read_to_string(root.join("big.rs")).unwrap(), big);
}

#[test]
fn fix_only_suppresses_residual_and_exits_zero() {
    let tmp = setup();
    let out = run(tmp.path(), &["fix", "--fix-only", "."]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The fixable rule applied (file trimmed)...
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
        "fn a() {}\nTODO\n"
    );
    assert!(
        stdout.contains("no-ws"),
        "applied rule shown; stdout:\n{stdout}"
    );
    // ...the unfixable residual is suppressed...
    assert!(
        !stdout.contains("no-todo"),
        "residual must be suppressed under --fix-only; stdout:\n{stdout}"
    );
    // ...and with no fix error, the exit is success.
    assert_eq!(
        out.status.code(),
        Some(0),
        "fix-only exits 0 absent a fix error"
    );
}

#[test]
fn plain_fix_reports_residual_and_exits_nonzero() {
    let tmp = setup();
    let out = run(tmp.path(), &["fix", "."]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("no-todo"),
        "plain fix reports the unfixable residual; stdout:\n{stdout}"
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "an unfixable error exits nonzero without --fix-only"
    );
}

/// M3 audit: an editless spawning op (a `command` fix) has no worktree hunk, so
/// `fix --diff` shows nothing -- but `fix --dry-run` must still SURFACE it as
/// "would run", never silently drop it (indistinguishable from nothing-to-fix),
/// and must NOT actually spawn the command.
#[test]
#[cfg(unix)]
fn dry_run_previews_an_editless_command_fix_as_would_run() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let config = r#"version: 1
rules:
  - id: js
    kind: command
    paths: "**/*.js"
    command: ["sh", "-c", "grep -q FIXED \"$1\"", "_", "{path}"]
    level: error
    fix:
      command:
        run: ["sh", "-c", "touch RAN_MARKER", "_", "{path}"]
"#;
    std::fs::write(root.join(".alint.yml"), config).unwrap();
    std::fs::write(root.join("a.js"), "x\n").unwrap();
    let out = run(root, &["fix", "--dry-run", "--unsafe-fixes", "."]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("would run"),
        "dry-run must surface the editless command fix; stdout: {stdout}"
    );
    assert!(
        !root.join("RAN_MARKER").exists(),
        "dry-run must NOT spawn the command"
    );
}

/// Audit 2026-10 finding 8: `fix --dry-run --format json` was shaped exactly
/// like a real run. The top-level `dry_run` flag now tells them apart
/// (statuses stay `applied` for compatibility).
#[test]
fn json_fix_report_marks_dry_runs() {
    let tmp = setup();
    let o = run(tmp.path(), &["fix", "--dry-run", "--format", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["dry_run"], true, "{v}");
    assert_eq!(v["summary"]["applied"], 1, "{v}");
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("a.rs")).unwrap(),
        "fn a() {}   \nTODO\n",
        "dry run writes nothing"
    );

    let o = run(tmp.path(), &["fix", "--format", "json"]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["dry_run"], false, "{v}");
}

/// A `file_exists` rule on `checked` whose `file_create` writes `created`.
fn create_config(checked: &str, created: &str) -> String {
    format!(
        "version: 1\nrules:\n  - id: env\n    kind: file_exists\n    paths: \"{checked}\"\n    \
         level: error\n    fix:\n      file_create:\n        path: \"{created}\"\n        \
         content: \"X=1\\n\"\n"
    )
}

fn only_item(o: &Output) -> serde_json::Value {
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    v["results"][0]["items"][0].clone()
}

#[test]
fn dry_run_agrees_with_fix_when_a_create_would_not_stick() {
    // A `file_create` into a gitignored path: the real `fix` creates it, the
    // walk never indexes it, and the re-check reports it unresolved (exit 1).
    // `--dry-run` used to say "1 applied", exit 0.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), ".env\n").unwrap();
    std::fs::write(root.join(".alint.yml"), create_config(".env", ".env")).unwrap();

    let dry = run(root, &["fix", "--dry-run", "--format", "json"]);
    assert!(!root.join(".env").exists(), "a dry run writes nothing");
    let item = only_item(&dry);
    assert_eq!(item["status"], "skipped", "{item}");
    assert_eq!(item["skip_kind"], "unresolved", "{item}");
    assert_eq!(dry.status.code(), Some(1));

    let real = run(root, &["fix", "--format", "json"]);
    let item = only_item(&real);
    assert_eq!(item["status"], "skipped", "{item}");
    assert_eq!(item["skip_kind"], "unresolved", "{item}");
    assert_eq!(real.status.code(), Some(1));
}

// The walk also leaves out what git's global excludes file lists, so the dry
// run predicts the same. This guards behavior the prediction already had.
// Unix only: `ignore` expands every `~` in the configured path, which mangles
// a Windows short temp path (`RUNNER~1`).
#[cfg(unix)]
#[test]
fn dry_run_agrees_with_fix_under_the_global_excludes_file() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join(".alint.yml"), create_config(".env", ".env")).unwrap();
    let excludes = tmp.path().join("global-ignore");
    std::fs::write(&excludes, ".env\n").unwrap();
    let gitconfig = tmp.path().join("gitconfig");
    std::fs::write(
        &gitconfig,
        format!("[core]\n\texcludesFile = {}\n", excludes.display()),
    )
    .unwrap();

    for args in [
        &["fix", "--dry-run", "--format", "json"][..],
        &["fix", "--format", "json"],
    ] {
        let home = tempfile::tempdir().unwrap();
        let output = isolated_alint(home.path())
            .args(args)
            .current_dir(&root)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .output()
            .expect("spawn alint");
        let item = only_item(&output);
        assert_eq!(item["status"], "skipped", "{args:?}: {item}");
        assert_eq!(item["skip_kind"], "unresolved", "{args:?}: {item}");
        assert_eq!(output.status.code(), Some(1), "{args:?}");
    }
    assert!(root.join(".env").exists(), "the real fix creates the file");
}

#[test]
fn dry_run_flags_a_create_outside_the_rules_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".alint.yml"), create_config("a.txt", "b.txt")).unwrap();
    let dry = run(root, &["fix", "--dry-run", "--format", "json"]);
    let item = only_item(&dry);
    assert_eq!(item["skip_kind"], "unresolved", "{item}");
    assert!(
        item["detail"]
            .as_str()
            .unwrap()
            .contains("outside the rule's `paths:`"),
        "{item}"
    );
    assert_eq!(dry.status.code(), Some(1));
    // Control: a create that does land is still previewed as applied.
    std::fs::write(root.join(".alint.yml"), create_config("a.txt", "a.txt")).unwrap();
    let dry = run(root, &["fix", "--dry-run", "--format", "json"]);
    assert_eq!(only_item(&dry)["status"], "applied");
    assert_eq!(dry.status.code(), Some(0));
}

#[test]
fn dry_run_leaves_a_sensitive_rules_target_out_of_its_reason() {
    // The reason a create would not resolve named its target, which a
    // `sensitive: true` rule or a `fix:` read from the environment can hold.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    let configs = [
        (
            "sensitive: true",
            "canary-target.txt",
            "the target is outside the rule's `paths:`",
        ),
        (
            "sensitive: false",
            "{{env.CREATED}}",
            "the target is outside the rule's `paths:`",
        ),
        (
            "sensitive: true",
            "../canary-target.txt",
            "the target is outside the root",
        ),
    ];
    for (sensitive, created, reason) in configs {
        std::fs::write(
            root.join(".alint.yml"),
            format!(
                "version: 1\nallow_out_of_root: true\nrules:\n\
                 - {{id: env, kind: file_exists, paths: a.txt, level: error, {sensitive}, \
                 fix: {{file_create: {{path: \"{created}\", content: \"x\\n\"}}}}}}\n"
            ),
        )
        .unwrap();
        for format in ["json", "human"] {
            let home = tempfile::tempdir().unwrap();
            let dry = isolated_alint(home.path())
                .args(["fix", "--dry-run", "--format", format])
                .current_dir(&root)
                .env("CREATED", "canary-target.txt")
                .output()
                .expect("spawn alint");
            let output = format!(
                "{}{}",
                String::from_utf8_lossy(&dry.stdout),
                String::from_utf8_lossy(&dry.stderr)
            );
            assert_eq!(dry.status.code(), Some(1), "{created} {format}: {output}");
            assert!(!output.contains("canary"), "{created} {format}: {output}");
            if format == "json" {
                let item = only_item(&dry);
                assert_eq!(item["skip_kind"], "unresolved", "{item}");
                assert!(item["detail"].as_str().unwrap().contains(reason), "{item}");
            }
        }
    }
    // Control: a rule that is not sensitive still names the target.
    std::fs::write(root.join(".alint.yml"), create_config("a.txt", "b.txt")).unwrap();
    let item = only_item(&run(&root, &["fix", "--dry-run", "--format", "json"]));
    assert!(
        item["detail"]
            .as_str()
            .unwrap()
            .contains("b.txt is outside the rule's `paths:`"),
        "{item}"
    );
}

/// One run's outcome per rule, `(rule id, "applied" or the skip kind)`, and
/// its exit code.
type RunOutcome = (Vec<(String, String)>, Option<i32>);

/// The outcome of `fix --dry-run` and then of `fix` in `root` (JSON, with
/// `env` set).
fn dry_and_real(root: &Path, env: &[(&str, &Path)]) -> [RunOutcome; 2] {
    let outcome = |args: &[&str]| {
        let home = tempfile::tempdir().unwrap();
        let output = isolated_alint(home.path())
            .args(args)
            .current_dir(root)
            .envs(env.iter().copied())
            .output()
            .expect("spawn alint");
        let v: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let items = v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|result| {
                let item = &result["items"][0];
                let outcome = if item["status"] == "applied" {
                    "applied"
                } else {
                    item["skip_kind"].as_str().unwrap()
                };
                (
                    result["id"].as_str().unwrap().to_string(),
                    outcome.to_string(),
                )
            })
            .collect();
        (items, output.status.code())
    };
    [
        outcome(&["fix", "--dry-run", "--format", "json"]),
        outcome(&["fix", "--format", "json"]),
    ]
}

fn outcomes(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected
        .iter()
        .map(|(id, outcome)| (id.to_string(), outcome.to_string()))
        .collect()
}

#[test]
fn dry_run_agrees_with_fix_when_a_create_is_an_ignore_file_or_a_directory() {
    // A created `.gitignore` of `*` leaves itself out; a created `.ignore`
    // leaves out a file another rule creates in the same pass; `dir_create`
    // makes a gitignored directory; a `file_create` path with `..` is written
    // where it normalizes to, which is not ignored although `out/` is.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), "build/\nout/\n").unwrap();
    std::fs::create_dir(root.join("cache")).unwrap();
    std::fs::write(root.join("cache/data.bin"), "x").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n\
         - {id: cache-ignore, kind: file_exists, paths: cache/.gitignore, level: error, \
         fix: {file_create: {content: \"*\\n\"}}}\n\
         - {id: logs-ignore, kind: file_exists, paths: .ignore, level: error, \
         fix: {file_create: {content: \"*.log\\n\"}}}\n\
         - {id: log, kind: file_exists, paths: x.log, level: error, \
         fix: {file_create: {content: \"x\\n\"}}}\n\
         - {id: build, kind: dir_exists, paths: build, level: error, fix: {dir_create: {}}}\n\
         - {id: readme, kind: file_exists, paths: README.md, level: error, \
         fix: {file_create: {path: out/../README.md, content: \"hi\\n\"}}}\n",
    )
    .unwrap();
    let expected = outcomes(&[
        ("cache-ignore", "unresolved"),
        ("logs-ignore", "applied"),
        ("log", "unresolved"),
        ("build", "unresolved"),
        ("readme", "applied"),
    ]);
    let [dry, real] = dry_and_real(root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
    assert!(root.join("README.md").is_file());
}

#[cfg(unix)]
#[test]
fn dry_run_agrees_with_fix_below_a_symlink_loop() {
    // `loop/.env` is written to `./.env`; the walk does not enter `loop`.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::os::unix::fs::symlink(".", root.join("loop")).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        create_config("loop/.env", "loop/.env"),
    )
    .unwrap();
    let expected = outcomes(&[("env", "unresolved")]);
    let [dry, real] = dry_and_real(root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
}

// Unix only, as `dry_run_agrees_with_fix_under_the_global_excludes_file`.
#[cfg(unix)]
#[test]
fn info_exclude_outranks_the_global_excludes_file() {
    // The global excludes file leaves out `*.md`; the repository's
    // `info/exclude` takes `keep.md` back, and wins.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(root.join(".git/info")).unwrap();
    std::fs::write(root.join(".git/info/exclude"), "!keep.md\n").unwrap();
    let excludes = tmp.path().join("global-ignore");
    std::fs::write(&excludes, "*.md\n").unwrap();
    let gitconfig = tmp.path().join("gitconfig");
    std::fs::write(
        &gitconfig,
        format!("[core]\n\texcludesFile = {}\n", excludes.display()),
    )
    .unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n\
         - {id: keep, kind: file_exists, paths: keep.md, level: error, \
         fix: {file_create: {content: \"x\\n\"}}}\n\
         - {id: other, kind: file_exists, paths: other.md, level: error, \
         fix: {file_create: {content: \"x\\n\"}}}\n",
    )
    .unwrap();
    let expected = outcomes(&[("keep", "applied"), ("other", "unresolved")]);
    let [dry, real] = dry_and_real(&root, &[("GIT_CONFIG_GLOBAL", &gitconfig)]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
}

#[test]
fn dry_run_agrees_with_fix_when_the_rule_looks_past_the_walk() {
    // `respect_gitignore: false` on a literal `file_exists` path stats the file
    // itself, so a create the walk leaves out still resolves it (the
    // `.bazelversion` pattern). The dry run used to call it unresolved.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), ".bazelversion\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n\
         - {id: bazel, kind: file_exists, paths: .bazelversion, level: error, \
         respect_gitignore: false, fix: {file_create: {content: \"7.0.0\\n\"}}}\n\
         - {id: walked, kind: file_exists, paths: .other, level: error, \
         fix: {file_create: {path: .bazelversion, content: \"7.0.0\\n\"}}}\n",
    )
    .unwrap();
    // `walked` creates the same ignored file for a rule that only sees the
    // walk; it is skipped once `bazel` has written it.
    let expected = outcomes(&[("bazel", "applied"), ("walked", "unresolved")]);
    let [dry, real] = dry_and_real(root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real.0[0], expected[0], "real fix");
    assert!(root.join(".bazelversion").is_file());
}

#[test]
fn dry_run_agrees_with_fix_for_a_create_outside_the_root() {
    // `allow_out_of_root` lets `file_create` write `../x.txt`, which no walk
    // of the root reaches. The dry run used to call it applied.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nrules:\n\
         - {id: up, kind: file_exists, paths: x.txt, level: error, \
         fix: {file_create: {path: ../x.txt, content: \"x\\n\"}}}\n",
    )
    .unwrap();
    let expected = outcomes(&[("up", "unresolved")]);
    let [dry, real] = dry_and_real(&root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
    assert!(tmp.path().join("x.txt").is_file(), "the real fix writes it");
}

#[cfg(unix)]
#[test]
fn dry_run_agrees_with_fix_below_a_symlink_out_of_the_root() {
    // `out` leads out of the root; `allow_out_of_root` lets a create through
    // it, which the walk (pruning `out`) and the rule's own stat both miss.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(tmp.path().join("elsewhere")).unwrap();
    std::fs::create_dir(&root).unwrap();
    std::os::unix::fs::symlink(tmp.path().join("elsewhere"), root.join("out")).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nrules:\n\
         - {id: walked, kind: file_exists, paths: out/a.txt, level: error, \
         fix: {file_create: {content: \"x\\n\"}}}\n\
         - {id: stat, kind: file_exists, paths: out/b.txt, level: error, \
         respect_gitignore: false, fix: {file_create: {content: \"x\\n\"}}}\n",
    )
    .unwrap();
    let expected = outcomes(&[("walked", "unresolved"), ("stat", "unresolved")]);
    let [dry, real] = dry_and_real(&root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
}

#[cfg(unix)]
#[test]
fn dry_run_agrees_with_fix_for_an_ignore_file_created_through_a_symlink() {
    // `alias` leads to `real`. `alias/.gitignore` lands in `real/`, where it
    // leaves out the `real/x.txt` another rule creates. Of two creates of one
    // ignore file, the first is written and the second skipped.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("real")).unwrap();
    std::os::unix::fs::symlink("real", root.join("alias")).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n\
         - {id: ignore, kind: file_exists, paths: alias/.gitignore, level: error, \
         fix: {file_create: {content: \"x.txt\\n\"}}}\n\
         - {id: x, kind: file_exists, paths: real/x.txt, level: error, \
         fix: {file_create: {content: \"x\\n\"}}}\n\
         - {id: first, kind: file_exists, paths: .gitignore, level: error, \
         fix: {file_create: {content: \"other\\n\"}}}\n\
         - {id: second, kind: file_exists, paths: .ignore, level: error, \
         fix: {file_create: {path: .gitignore, content: \"y.txt\\n\"}}}\n\
         - {id: y, kind: file_exists, paths: y.txt, level: error, \
         fix: {file_create: {content: \"y\\n\"}}}\n",
    )
    .unwrap();
    let [dry, real] = dry_and_real(root, &[]);
    for (id, outcome) in [("ignore", "applied"), ("x", "unresolved"), ("y", "applied")] {
        let expected = (id.to_string(), outcome.to_string());
        assert!(dry.0.contains(&expected), "dry run: {dry:?}");
        assert!(real.0.contains(&expected), "real fix: {real:?}");
    }
}

#[test]
fn dry_run_agrees_with_fix_for_an_ignore_file_read_from_outside_the_root() {
    // `allow_out_of_root` lets `file_create` take its content from
    // `../template`; the created `cache/.gitignore` of `*` then leaves out
    // itself and the `cache/README.md` another rule creates. The dry run read
    // the content as an editor would, which never leaves the root, so it saw
    // no ignore file at all.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir_all(root.join("cache")).unwrap();
    std::fs::write(tmp.path().join("template"), "*\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nrules:\n\
         - {id: cache-ignore, kind: file_exists, paths: cache/.gitignore, level: error, \
         fix: {file_create: {content_from: ../template}}}\n\
         - {id: cache-readme, kind: file_exists, paths: cache/README.md, level: error, \
         fix: {file_create: {content: \"x\\n\"}}}\n",
    )
    .unwrap();
    let expected = outcomes(&[
        ("cache-ignore", "unresolved"),
        ("cache-readme", "unresolved"),
    ]);
    let [dry, real] = dry_and_real(&root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
}

#[test]
fn dry_run_agrees_with_fix_for_a_member_created_outside_the_root() {
    // `allow_out_of_root` lets `create_and_register` create a named member
    // outside the root, which no walk of the root reaches.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nrules:\n\
         - id: member\n\
         \x20 kind: cross_file\n\
         \x20 relation: registered\n\
         \x20 source: {file: ../shared/Cargo.toml}\n\
         \x20 register_as: \"{dir}\"\n\
         \x20 targets:\n\
         \x20   - {file: Cargo.toml, extract: {toml: \"$.workspace.members[*]\"}}\n\
         \x20 level: error\n\
         \x20 fix:\n\
         \x20   create_and_register: {content: \"[package]\\n\", applicability: safe}\n",
    )
    .unwrap();
    let expected = outcomes(&[("member", "unresolved")]);
    let [dry, real] = dry_and_real(&root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
    assert!(
        tmp.path().join("shared/Cargo.toml").is_file(),
        "the real fix creates it"
    );
}

#[cfg(unix)]
#[test]
fn dry_run_agrees_with_fix_for_a_rule_without_a_paths_scope_through_a_symlink() {
    // `dir_exists` and `cross_file` look for the path as written, not for
    // where a symlink makes it land: `alias` (ignored) leads to `real`, and
    // `deep/x/up` back to `deep`, which the walk does not enter from below.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("real")).unwrap();
    std::os::unix::fs::symlink("real", root.join("alias")).unwrap();
    std::fs::create_dir_all(root.join("deep/x")).unwrap();
    std::os::unix::fs::symlink("..", root.join("deep/x/up")).unwrap();
    std::fs::write(root.join(".gitignore"), "alias\n").unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n\
         - {id: ignored-alias, kind: dir_exists, paths: alias/m, level: error, \
         fix: {dir_create: {}}}\n\
         - {id: up-link, kind: dir_exists, paths: deep/x/up/n, level: error, \
         fix: {dir_create: {}}}\n\
         - id: member\n\
         \x20 kind: cross_file\n\
         \x20 relation: registered\n\
         \x20 source: {file: alias/a/Cargo.toml}\n\
         \x20 register_as: \"{dir}\"\n\
         \x20 targets:\n\
         \x20   - {file: Cargo.toml, extract: {toml: \"$.workspace.members[*]\"}}\n\
         \x20 level: error\n\
         \x20 fix:\n\
         \x20   create_and_register: {content: \"[package]\\n\", applicability: safe}\n",
    )
    .unwrap();
    let expected = outcomes(&[
        ("ignored-alias", "unresolved"),
        ("up-link", "unresolved"),
        ("member", "unresolved"),
    ]);
    let [dry, real] = dry_and_real(root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
}

#[test]
fn dry_run_agrees_with_fix_for_a_member_written_back_into_the_root() {
    // `../repo/crates/b` leads back into the root, but `cross_file` compares
    // a member it cannot read as a path inside the root as written, so the
    // created member never matches.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nrules:\n\
         - id: member\n\
         \x20 kind: cross_file\n\
         \x20 relation: registered\n\
         \x20 source: {file: ../repo/crates/b/Cargo.toml}\n\
         \x20 register_as: \"{dir}\"\n\
         \x20 targets:\n\
         \x20   - {file: Cargo.toml, extract: {toml: \"$.workspace.members[*]\"}}\n\
         \x20 level: error\n\
         \x20 fix:\n\
         \x20   create_and_register: {content: \"[package]\\n\", applicability: safe}\n",
    )
    .unwrap();
    let expected = outcomes(&[("member", "unresolved")]);
    let [dry, real] = dry_and_real(&root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
    assert!(
        root.join("crates/b/Cargo.toml").is_file(),
        "the real fix creates it"
    );
}

#[cfg(unix)]
#[test]
fn dry_run_agrees_with_fix_for_a_member_written_with_a_backslash() {
    // On Unix `crates\b` is one name, which the rule looks for, but the
    // finding's key writes it `crates/b`. Creating that would leave the
    // finding standing, so both runs decline and write nothing.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n\
         - id: member\n\
         \x20 kind: cross_file\n\
         \x20 relation: registered\n\
         \x20 source: {file: 'crates\\b/Cargo.toml'}\n\
         \x20 register_as: \"{dir}\"\n\
         \x20 targets:\n\
         \x20   - {file: Cargo.toml, extract: {toml: \"$.workspace.members[*]\"}}\n\
         \x20 level: error\n\
         \x20 fix:\n\
         \x20   create_and_register: {content: \"[package]\\n\", applicability: safe}\n",
    )
    .unwrap();
    let expected = outcomes(&[("member", "declined")]);
    let [dry, real] = dry_and_real(root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
    assert!(!root.join("crates").exists(), "nothing is created");
}

#[test]
fn dry_run_agrees_with_fix_for_a_create_through_a_directory_it_makes() {
    // Written out of the root (`../repo/...`), the path is used as given, so
    // `newdir/..` is the root once `create_parents` makes `newdir` and the
    // file lands at `x.txt`; the dry run used to place it outside the root.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nallow_out_of_root: true\nrules:\n\
         - {id: x, kind: file_exists, paths: x.txt, level: error, \
         fix: {file_create: {path: ../repo/newdir/../x.txt, content: \"x\\n\", \
         create_parents: true}}}\n",
    )
    .unwrap();
    let expected = outcomes(&[("x", "applied")]);
    let [dry, real] = dry_and_real(&root, &[]);
    assert_eq!(dry, (expected.clone(), Some(0)), "dry run");
    assert_eq!(real, (expected, Some(0)), "real fix");
    assert!(root.join("x.txt").is_file());
}

/// `git` with an empty global and system config, so a developer's settings
/// (commit signing, hooks) cannot fail or change the setup.
fn git(dir: &Path, args: &[&str]) {
    let config = tempfile::NamedTempFile::new().unwrap();
    let status = Command::new("git")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", config.path())
        .env("GIT_CONFIG_SYSTEM", config.path())
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .expect("run git")
        .status;
    assert!(status.success(), "git {args:?} failed");
}

#[test]
fn dry_run_agrees_with_fix_for_a_git_tracked_only_rule() {
    // A `git_tracked_only` rule sees only tracked files: a created file is
    // untracked and never resolves it, unless git still tracks the path (a
    // tracked file deleted from the worktree).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    std::fs::write(root.join("gone.txt"), "x\n").unwrap();
    git(root, &["add", "gone.txt"]);
    git(root, &["commit", "-qm", "base"]);
    std::fs::remove_file(root.join("gone.txt")).unwrap();
    std::fs::write(
        root.join(".alint.yml"),
        "version: 1\nrules:\n\
         - {id: new, kind: file_exists, paths: new.txt, level: error, \
         git_tracked_only: true, fix: {file_create: {content: \"x\\n\"}}}\n\
         - {id: gone, kind: file_exists, paths: gone.txt, level: error, \
         git_tracked_only: true, fix: {file_create: {content: \"x\\n\"}}}\n",
    )
    .unwrap();
    let expected = outcomes(&[("new", "unresolved"), ("gone", "applied")]);
    let [dry, real] = dry_and_real(root, &[]);
    assert_eq!(dry, (expected.clone(), Some(1)), "dry run");
    assert_eq!(real, (expected, Some(1)), "real fix");
}
