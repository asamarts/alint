//! The experimental `version: 2` config on the command line: how it names
//! the files it loads, the values it reads from the environment, and what it
//! says about versions it does not take.

use std::path::Path;
use std::process::Output;

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn run(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
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

/// A bundled `version: 2` ruleset (`alint://bundled/js/workspace-deps@v2`)
/// works end-to-end across `validate-config`, `list`, `explain`, and `check`,
/// supports `as:` aliasing for downstream rules, and fails with exit code 2
/// when extended from a `version: 1` config (ADR-0050 D10 / ADR-0022).
#[test]
fn bundled_version_two_ruleset_works_across_cli_commands() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write(
        repo,
        "package.json",
        "{\"name\": \"root\", \"private\": true}\n",
    );
    write(
        repo,
        "packages/a/package.json",
        "{\"name\": \"@demo/a\", \"dependencies\": {\"zod\": \"^3.23.0\"}}\n",
    );
    write(
        repo,
        "packages/b/package.json",
        "{\"name\": \"@demo/b\", \"dependencies\": {\"zod\": \"^3.24.0\"}}\n",
    );
    write(
        repo,
        ".alint.yml",
        "version: 2\nextends:\n  - url: alint://bundled/js/workspace-deps@v2\n    as: ws\n\
         relations:\n  banned:\n    from: ws.workspace-manifests\n    entries:\n      \
         format: json\n      at: [\"$.dependencies\"]\n    fields:\n      dep: entry.key\n    \
         where:\n      dep: { in: [\"left-pad\"] }\nrules:\n  - id: no-left-pad\n    \
         kind: constraint\n    input: banned\n    assert: { count: { max: 0 } }\n    \
         level: error\n",
    );

    // `validate-config` succeeds and prints the v0.25 experimental warning.
    let validate = run(repo, &["validate-config"], &[]);
    assert!(
        validate.status.success(),
        "validate-config failed: {}",
        String::from_utf8_lossy(&validate.stderr)
    );
    let validate_err = String::from_utf8_lossy(&validate.stderr);
    assert!(
        validate_err.contains("v0.25 at the earliest"),
        "expected v0.25 warning, got: {validate_err}"
    );

    // `list` shows the bundled URI as the declaring source and its `as: ws` alias.
    let listed = stdout(&run(repo, &["list"], &[]));
    assert!(
        listed.contains("js-workspace-deps-consistent-range"),
        "{listed}"
    );
    assert!(
        listed.contains("selector workspace-manifests  alint://bundled/js/workspace-deps@v2\n"),
        "{listed}"
    );
    assert!(
        listed.contains("aliases alint://bundled/js/workspace-deps@v2  as ws in .alint.yml\n"),
        "{listed}"
    );

    // `list --format json` includes the bundled source path and alias.
    let listed_json: serde_json::Value =
        serde_json::from_str(&stdout(&run(repo, &["list", "--format", "json"], &[]))).unwrap();
    let sources = listed_json["sources"].as_array().unwrap();
    assert!(
        sources
            .iter()
            .any(|s| s["path"] == "alint://bundled/js/workspace-deps@v2"),
        "{listed_json}"
    );

    // `explain` renders both the bundled rule and the local rule reusing `ws.workspace-manifests`.
    let explained_bundled = stdout(&run(
        repo,
        &["explain", "js-workspace-deps-consistent-range"],
        &[],
    ));
    assert!(
        explained_bundled.contains("alint://bundled/js/workspace-deps@v2"),
        "{explained_bundled}"
    );
    let explained_local = stdout(&run(repo, &["explain", "no-left-pad"], &[]));
    assert!(
        explained_local.contains("ws.workspace-manifests"),
        "{explained_local}"
    );
    assert!(
        explained_local.contains("alint://bundled/js/workspace-deps@v2"),
        "{explained_local}"
    );

    // `check` flags the `zod` version drift between packages/a and packages/b.
    let check = run(repo, &["check"], &[]);
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let check_out = String::from_utf8_lossy(&check.stdout);
    assert!(
        check_out.contains("js-workspace-deps-consistent-range"),
        "{check_out}"
    );
}

/// A `version: 1` config extending a bundled `version: 2` ruleset fails
/// `validate-config` with exit code 1 and `check` with config-error exit
/// code 2 (never internal-error exit code 3).
#[test]
fn version_one_config_extending_bundled_version_two_exits_two() {
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    write(
        repo,
        ".alint.yml",
        "version: 1\nextends:\n  - alint://bundled/js/workspace-deps@v2\nrules: []\n",
    );
    let refused_validate = run(repo, &["validate-config"], &[]);
    assert_eq!(
        refused_validate.status.code(),
        Some(1),
        "{refused_validate:?}"
    );
    let refused_check = run(repo, &["check"], &[]);
    assert_eq!(refused_check.status.code(), Some(2), "{refused_check:?}");
    let refused_err = String::from_utf8_lossy(&refused_check.stderr);
    assert!(
        refused_err.contains("cannot extend this version 2 bundled ruleset")
            && refused_err.contains("version: 2"),
        "{refused_err}"
    );
}
