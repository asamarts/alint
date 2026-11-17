//! `--untrusted`: the language server's untrusted mode, for the command line.
//! Beyond what `--no-spawn` refuses (`no_spawn.rs` runs every one of its cases
//! with both flags), a remote `extends:` is refused before anything is
//! fetched, and the config reads an empty environment: `{{env.NAME}}` in it,
//! and `env.*` in a `when:`.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// `alint ARGS` in `dir`, with `env` set, git's global config and the
/// remote-ruleset cache kept in `home`.
fn alint(dir: &Path, home: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
    let mut command = crate::hermetic::command(env!("CARGO_BIN_EXE_alint"));
    command
        .args(args)
        .current_dir(dir)
        .env("HOME", home)
        .env("XDG_CACHE_HOME", home.join("cache"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("GIT_CONFIG_GLOBAL", home.join("gitconfig"))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .envs(env.iter().copied());
    command
}

fn run(dir: &Path, home: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    alint(dir, home, args, env).output().expect("spawn alint")
}

fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

/// Whether `listener` (non-blocking) accepted a connection within `wait`.
/// The connection is dropped at once, so a client waiting on it fails fast.
fn connected(listener: &TcpListener, wait: Duration) -> bool {
    let deadline = Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok(_) => return true,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => panic!("listener failed: {error}"),
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A config that extends a ruleset at the listener, its path read from the
/// environment (with a default, so it resolves with none).
fn remote_config(port: u16) -> String {
    format!(
        "version: 1\nextends:\n  - \"https://127.0.0.1:{port}/\
         {{{{env.ALINT_UNTRUSTED_RULESET | default('base')}}}}.yml#sha256-{}\"\n\
         rules:\n  - id: readme\n    kind: file_exists\n    paths: README.md\n    \
         level: error\n",
        "0".repeat(64)
    )
}

const ENV: [(&str, &str); 1] = [("ALINT_UNTRUSTED_RULESET", "leaked")];

/// Every subcommand that loads a config refuses the remote `extends:` under
/// `--untrusted`, and nothing connects to the host it names.
#[test]
fn untrusted_refuses_a_remote_extends_before_connecting() {
    let (listener, port) = listener();
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".alint.yml"), remote_config(port)).unwrap();
    std::fs::write(dir.path().join("README.md"), "# x\n").unwrap();
    for args in [
        &["check", "--untrusted"][..],
        &["--untrusted"],
        &["fix", "--untrusted"],
        &["baseline", "--untrusted", "--output", "base.json"],
        &["facts", "--untrusted"],
        &["list", "--untrusted"],
        &["explain", "readme", "--untrusted"],
        &["validate-config", "--untrusted"],
    ] {
        let out = run(dir.path(), home.path(), args, &ENV);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?}: {stderr}");
        assert!(
            stderr.contains("remote `extends` is disabled for this load"),
            "{args:?}: {stderr}"
        );
        assert!(!stderr.contains("leaked"), "{args:?}: {stderr}");
        assert!(!connected(&listener, Duration::ZERO), "{args:?} connected");
    }
    assert!(!connected(&listener, Duration::from_millis(200)));

    // The positive control: without the flag, alint connects to fetch it.
    let mut child = alint(dir.path(), home.path(), &["check"], &ENV)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn alint");
    let reached = connected(&listener, Duration::from_secs(30));
    child.kill().ok();
    child.wait().ok();
    assert!(reached, "the positive control never connected");
}

/// The rule ids that reported a violation.
fn failing_rules(output: &Output) -> Vec<String> {
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)));
    let mut ids: Vec<String> = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|result| result["passed"] == false)
        .map(|result| result["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    ids
}

/// `{{env.NAME}}` in the config and `env.*` in a `when:` read an empty
/// environment under `--untrusted`, as in an untrusted language-server
/// session: `named` looks for `absent.txt`, and `conditional` does not run.
#[test]
fn untrusted_reads_an_empty_environment() {
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("SECRET.txt"), "x\n").unwrap();
    std::fs::write(
        dir.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: named\n    kind: file_exists\n    \
         paths: \"{{env.ALINT_UNTRUSTED_NAME | default('absent')}}.txt\"\n    level: error\n  \
         - id: conditional\n    kind: file_exists\n    paths: never-there.txt\n    \
         when: 'env.ALINT_UNTRUSTED_NAME == \"SECRET\"'\n    level: error\n",
    )
    .unwrap();
    let env = [("ALINT_UNTRUSTED_NAME", "SECRET")];
    let trusted = run(
        dir.path(),
        home.path(),
        &["check", "--format", "json"],
        &env,
    );
    assert_eq!(failing_rules(&trusted), ["conditional"]);
    for flags in [&["--untrusted"][..], &["--untrusted", "--no-spawn"]] {
        let mut args = vec!["check", "--format", "json"];
        args.extend_from_slice(flags);
        let untrusted = run(dir.path(), home.path(), &args, &env);
        assert_eq!(failing_rules(&untrusted), ["named"], "{flags:?}");
        assert!(
            !String::from_utf8_lossy(&untrusted.stdout).contains("SECRET"),
            "{flags:?}"
        );
    }
    // `--no-spawn` alone reads the environment, as before.
    let no_spawn = run(
        dir.path(),
        home.path(),
        &["check", "--format", "json", "--no-spawn"],
        &env,
    );
    assert_eq!(failing_rules(&no_spawn), ["conditional"]);
}
