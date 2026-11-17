//! The experimental `version: 2` config on the command line: how it names
//! the files it loads, the values it reads from the environment, and what it
//! says about versions it does not take.

use std::path::Path;
use std::process::{Command, Output};

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_alint"))
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    assert!(!output.status.success(), "expected a failure");
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// With `allow_out_of_root: true`, two `extends:` targets outside the
/// top-level config's directory with the same file name were both named by
/// it (`rules.yml`), in listings and in errors. Each is named by its path
/// from that directory.
#[test]
fn a_config_outside_the_root_is_named_by_its_path_from_it() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("repo");
    write(&repo, "README.md", "hi\n");
    for side in ["a", "b"] {
        write(
            tmp.path(),
            &format!("{side}/rules.yml"),
            "version: 2\nselectors:\n  docs: { paths: README.md }\nrules: []\n",
        );
    }
    write(
        &repo,
        ".alint.yml",
        "version: 2\nallow_out_of_root: true\nextends:\n  - url: ../a/rules.yml\n    as: a\n  \
         - url: ../b/rules.yml\n    as: b\nrules: []\n",
    );
    let outside = |side: &str| {
        Path::new("..")
            .join(side)
            .join("rules.yml")
            .display()
            .to_string()
    };
    let (a, b) = (outside("a"), outside("b"));
    let listed = stdout(&run(&repo, &["list"], &[]));
    assert!(
        listed.ends_with(&format!(
            "\nselector docs  {a}\nselector docs  {b}\naliases {a}  as a in .alint.yml\n\
             aliases {b}  as b in .alint.yml\n"
        )),
        "{listed}"
    );

    // A refusal names it as its `extends:` entry spells it, in either version
    // and on every platform.
    for version in [1, 2] {
        write(
            tmp.path(),
            "a/rules.yml",
            &format!("version: {version}\nrules: []\n"),
        );
        write(
            tmp.path(),
            "b/rules.yml",
            &format!("version: {version}\nrules:\n  - {{ id: r, kind: nope, level: error }}\n"),
        );
        write(
            &repo,
            ".alint.yml",
            &format!(
                "version: {version}\nallow_out_of_root: true\nextends:\n  - ../a/rules.yml\n  \
                 - ../b/rules.yml\nrules: []\n"
            ),
        );
        let error = stderr(&run(&repo, &["validate-config"], &[]));
        assert!(
            error.contains("extended config (../b/rules.yml)"),
            "version {version}: {error}"
        );
    }
    // A version 2 error found in it names it by its path from the root.
    write(
        tmp.path(),
        "b/rules.yml",
        "version: 2\nrules: []\nnot_a_field: 1\n",
    );
    let error = stderr(&run(&repo, &["validate-config"], &[]));
    assert!(
        error.contains(&format!("{b}: unknown key `not_a_field`")),
        "{error}"
    );
}
