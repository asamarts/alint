//! The files alint reads whole besides a config are read only when they are
//! regular files of bounded size, and a baseline the repository names only
//! inside it.
//!
//! The walk's ignore files: `ignore` reads every `.ignore`, `.gitignore`,
//! `info/exclude` (and a linked worktree's `.git` file and `commondir` on the
//! way to one) and the global excludes file whole, through symlinks, so a
//! `.gitignore` committed as a symlink to `/dev/zero` made the walk take memory
//! until the run died, and one to a FIFO blocked it. The baseline file was read
//! the same way, wherever a config's `baseline:` key led.
//!
//! Each run here has a deadline and, on Linux, a memory limit: macOS refuses
//! to set one (`ulimit -v` fails with "Invalid argument"), so there a run has
//! only its deadline.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The address space each run may take: far more than a walk of these trees
/// needs, far less than an unbounded read reaches before the deadline.
const MEMORY_LIMIT_KIB: u64 = 4 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(60);

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn symlink(target: &Path, link: &Path) {
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// A sparse regular file far past the cap, which takes no space on disk.
fn sparse(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::File::create(path)
        .unwrap()
        .set_len(64 << 30)
        .unwrap();
}

fn fifo(path: &Path) -> bool {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    Command::new("mkfifo")
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

/// `alint check` on `root` ([`run`]).
fn check(root: &Path, home: &Path, gitconfig: &Path) -> Option<(Option<i32>, String)> {
    run(root, &["check", "--format", "json"], home, gitconfig)
}

/// `alint ARGS` in `root` under the memory limit (on Linux), with git's
/// global config at `gitconfig` and the excludes files `ignore` falls back to
/// in an empty `home`: its exit code and stderr, or `None` past the deadline.
fn run(root: &Path, args: &[&str], home: &Path, gitconfig: &Path) -> Option<(Option<i32>, String)> {
    let limit = if cfg!(target_os = "linux") {
        format!("ulimit -v {MEMORY_LIMIT_KIB} && ")
    } else {
        String::new()
    };
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(format!("{limit}exec \"$0\" \"$@\""))
        .arg(alint())
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", gitconfig)
        .env("GIT_CONFIG_SYSTEM", home.join("no-system-config"))
        .env("XDG_CONFIG_HOME", home.join("config"))
        .env("HOME", home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn alint");
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > DEADLINE {
            child.kill().ok();
            child.wait().ok();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut stderr = String::new();
    std::io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut stderr).unwrap();
    Some((status.code(), stderr))
}

const CONFIG: &str = "version: 1\nrules:\n  - id: readme\n    kind: file_exists\n    \
                      paths: README.md\n    level: error\n";

/// Each case: the tree, the linted root inside it, and the file named.
struct Case {
    name: &'static str,
    /// Builds the hostile tree under the temporary directory; returns the
    /// root to lint and the file the error must name. `None` when this host
    /// cannot build it.
    build: fn(&Path, &Path) -> Option<(PathBuf, PathBuf)>,
}

fn plain_root(tmp: &Path) -> PathBuf {
    let root = tmp.join("repo");
    write(&root, ".alint.yml", CONFIG);
    write(&root, "README.md", "# x\n");
    write(&root, "src/lib.rs", "\n");
    root
}

fn repository_root(tmp: &Path) -> PathBuf {
    let root = plain_root(tmp);
    std::fs::create_dir_all(root.join(".git/info")).unwrap();
    root
}

// One entry per case.
#[allow(clippy::too_many_lines)]
fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "a .gitignore below the root that is a symlink to /dev/zero",
            build: |tmp, _| {
                let root = plain_root(tmp);
                let file = root.join("src/.gitignore");
                symlink(Path::new("/dev/zero"), &file);
                Some((root, file))
            },
        },
        Case {
            name: "an .ignore below the root that is a symlink to /dev/zero",
            build: |tmp, _| {
                let root = plain_root(tmp);
                let file = root.join("src/.ignore");
                symlink(Path::new("/dev/zero"), &file);
                Some((root, file))
            },
        },
        Case {
            name: "the root's .gitignore, a symlink to /dev/zero",
            build: |tmp, _| {
                let root = plain_root(tmp);
                let file = root.join(".gitignore");
                symlink(Path::new("/dev/zero"), &file);
                Some((root, file))
            },
        },
        Case {
            name: "a .gitignore below the root that is a FIFO",
            build: |tmp, _| {
                let root = plain_root(tmp);
                let file = root.join("src/.gitignore");
                fifo(&file).then_some((root, file))
            },
        },
        Case {
            name: "a .gitignore below the root that is a sparse file past the cap",
            build: |tmp, _| {
                let root = plain_root(tmp);
                let file = root.join("src/.gitignore");
                sparse(&file);
                Some((root, file))
            },
        },
        Case {
            name: "a repository's info/exclude, a symlink to /dev/zero",
            build: |tmp, _| {
                let root = repository_root(tmp);
                let file = root.join(".git/info/exclude");
                symlink(Path::new("/dev/zero"), &file);
                Some((root, file))
            },
        },
        Case {
            name: "a nested repository's info/exclude, a symlink to /dev/zero",
            build: |tmp, _| {
                let root = repository_root(tmp);
                let file = root.join("src/vendored/.git/info/exclude");
                symlink(Path::new("/dev/zero"), &file);
                Some((root, file))
            },
        },
        Case {
            name: "a linked worktree's commondir, a symlink to /dev/zero",
            build: |tmp, _| {
                let root = plain_root(tmp);
                let git_dir = tmp.join("main.git/worktrees/repo");
                std::fs::create_dir_all(&git_dir).unwrap();
                write(&root, ".git", &format!("gitdir: {}\n", git_dir.display()));
                let file = git_dir.join("commondir");
                symlink(Path::new("/dev/zero"), &file);
                Some((root, file))
            },
        },
        Case {
            name: "a linked worktree's .git file, a sparse file past the cap",
            build: |tmp, _| {
                let root = plain_root(tmp);
                let file = root.join(".git");
                sparse(&file);
                Some((root, file))
            },
        },
        Case {
            name: "a .gitignore above the root, a symlink to /dev/zero",
            build: |tmp, _| {
                let file = tmp.join(".gitignore");
                symlink(Path::new("/dev/zero"), &file);
                Some((plain_root(tmp), file))
            },
        },
        Case {
            name: "the global excludes file, a sparse file past the cap",
            build: |tmp, gitconfig| {
                let file = tmp.join("global-excludes");
                sparse(&file);
                std::fs::write(
                    gitconfig,
                    format!("[core]\n\texcludesFile = {}\n", file.display()),
                )
                .unwrap();
                Some((plain_root(tmp), file))
            },
        },
    ]
}

#[test]
fn the_walk_refuses_an_ignore_file_it_cannot_read_in_bounds() {
    let mut failures = Vec::new();
    for case in cases() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let gitconfig = home.path().join("gitconfig");
        std::fs::write(&gitconfig, "").unwrap();
        let Some((root, file)) = (case.build)(tmp.path(), &gitconfig) else {
            eprintln!("skipped on this host: {}", case.name);
            continue;
        };
        match check(&root, home.path(), &gitconfig) {
            None => failures.push(format!("{}: ran past the deadline", case.name)),
            Some((code, stderr))
                if code != Some(2)
                    || !stderr.contains("the walk does not read the ignore file")
                    || !stderr.contains(&file.display().to_string()) =>
            {
                failures.push(format!("{}: exit {code:?}: {stderr}", case.name));
            }
            Some(_) => {}
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The positive control: the same trees with each file a small regular file
/// (or, for the FIFO, a regular file) lint as usual.
#[test]
fn a_small_regular_ignore_file_is_read_as_before() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let gitconfig = home.path().join("gitconfig");
    let excludes = tmp.path().join("global-excludes");
    std::fs::write(&excludes, "*.tmp\n").unwrap();
    std::fs::write(
        &gitconfig,
        format!("[core]\n\texcludesFile = {}\n", excludes.display()),
    )
    .unwrap();
    let root = repository_root(tmp.path());
    write(&root, "src/.gitignore", "*.log\n");
    write(&root, "src/.ignore", "*.bak\n");
    write(&root, ".git/info/exclude", "*.out\n");
    write(tmp.path(), ".gitignore", "*\n");
    // `.gitignore` as a symlink to a regular file is read, as before.
    symlink(Path::new("src/.gitignore"), &root.join(".gitignore"));
    let (code, stderr) = check(&root, home.path(), &gitconfig).expect("within the deadline");
    assert_eq!(code, Some(0), "{stderr}");
}

/// A repository with a violation a baseline grandfathers, a valid baseline
/// for it inside (`base.jsonl`), in its Git directory and outside it, and a
/// FIFO, a sparse file past the cap and a symlink to `/dev/zero` in it.
fn baseline_repository(tmp: &Path, home: &Path, gitconfig: &Path) -> (PathBuf, bool) {
    let root = tmp.join("repo");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    write(
        &root,
        ".alint.yml",
        "version: 1\nrules:\n  - id: no-todo\n    kind: file_content_forbidden\n    \
         paths: '*.txt'\n    pattern: TODO\n    level: error\n",
    );
    write(&root, "a.txt", "TODO\n");
    let (code, stderr) = run(
        &root,
        &["baseline", "--output", "base.jsonl"],
        home,
        gitconfig,
    )
    .expect("within the deadline");
    assert_eq!(code, Some(0), "{stderr}");
    let valid = std::fs::read(root.join("base.jsonl")).unwrap();
    std::fs::write(root.join(".git/base.jsonl"), &valid).unwrap();
    std::fs::write(tmp.join("outside.jsonl"), &valid).unwrap();
    symlink(Path::new("/dev/zero"), &root.join("zero.jsonl"));
    symlink(Path::new("../outside.jsonl"), &root.join("link.jsonl"));
    sparse(&root.join("sparse.jsonl"));
    let fifo = fifo(&root.join("fifo.jsonl"));
    (root, fifo)
}

fn set_baseline_key(root: &Path, baseline: &str) {
    let config = std::fs::read_to_string(root.join(".alint.yml")).unwrap();
    let rules = config.find("rules:").unwrap();
    write(
        root,
        ".alint.yml",
        &format!("version: 1\nbaseline: {baseline}\n{}", &config[rules..]),
    );
}

/// A config's `baseline:` key names a regular file within the cap, in the
/// repository and outside its Git directories; `--baseline` names a regular
/// file within the cap anywhere. `check` and `fix` read it alike.
#[test]
fn a_baseline_file_is_read_only_when_regular_bounded_and_confined_to_its_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let gitconfig = home.path().join("gitconfig");
    std::fs::write(&gitconfig, "").unwrap();
    let (root, fifo) = baseline_repository(tmp.path(), home.path(), &gitconfig);
    let outside = tmp.path().join("outside.jsonl");
    let outside = outside.to_str().unwrap();

    let mut failures = Vec::new();
    // (`baseline:` key, flag, refusal); `None` for a run that must pass.
    let cases: [(Option<&str>, Option<&str>, Option<&str>); 10] = [
        (
            Some("zero.jsonl"),
            None,
            Some("resolves outside the repository"),
        ),
        (
            Some("../outside.jsonl"),
            None,
            Some("resolves outside the repository"),
        ),
        (
            Some("link.jsonl"),
            None,
            Some("resolves outside the repository"),
        ),
        (Some(".git/base.jsonl"), None, Some("is in a Git directory")),
        (Some("sparse.jsonl"), None, Some("larger than 256 MiB")),
        (Some("fifo.jsonl"), None, Some("not a regular file")),
        (None, Some("/dev/zero"), Some("not a regular file")),
        (None, Some("sparse.jsonl"), Some("larger than 256 MiB")),
        // The user's own flag names a file anywhere, as before.
        (Some("zero.jsonl"), Some(outside), None),
        (Some("base.jsonl"), None, None),
    ];
    for (key, flag, refusal) in cases {
        if key == Some("fifo.jsonl") && !fifo {
            continue;
        }
        set_baseline_key(&root, key.unwrap_or("base.jsonl"));
        for command in ["check", "fix"] {
            let mut args = vec![command];
            if let Some(flag) = flag {
                args.extend(["--baseline", flag]);
            }
            let case = format!("{command} with baseline: {key:?} and --baseline {flag:?}");
            let Some((code, stderr)) = run(&root, &args, home.path(), &gitconfig) else {
                failures.push(format!("{case}: ran past the deadline"));
                continue;
            };
            let passed = match refusal {
                Some(refusal) => code == Some(2) && stderr.contains(refusal),
                None => code == Some(0),
            };
            if !passed {
                failures.push(format!("{case}: exit {code:?}: {stderr}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
