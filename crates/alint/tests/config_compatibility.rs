use std::path::PathBuf;
use std::process::Command;

fn alint() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_alint"))
}

#[test]
fn every_config_consuming_cli_command_fails_on_requirements_before_evaluation() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join(".alint.yml"),
        "version: 1\nrequires:\n  alint: '999.0.0'\nrules:\n  - id: should-not-build\n    kind: definitely_unknown\n    level: error\n",
    )
    .unwrap();

    let cases: &[(&[&str], i32)] = &[
        (&["check"], 2),
        (&["fix", "--dry-run"], 2),
        (&["list"], 2),
        (&["explain", "should-not-build"], 2),
        (&["facts"], 2),
        (&["baseline"], 2),
        (&["validate-config"], 1),
        (&["export-agents-md"], 2),
    ];
    for (args, expected_exit) in cases {
        let output = Command::new(alint())
            .args(*args)
            .current_dir(temp.path())
            .output()
            .unwrap_or_else(|error| panic!("run alint {args:?}: {error}"));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(*expected_exit),
            "alint {args:?} must report a config error before evaluation: {stderr}"
        );
        assert!(
            stderr.contains("requires alint 999.0.0 or newer"),
            "alint {args:?}: {stderr}"
        );
        assert!(
            !stderr.contains("definitely_unknown"),
            "alint {args:?} evaluated the rule before compatibility: {stderr}"
        );
    }
}

/// A nested `key_parity` whose `reference:` takes its file name from a path
/// token loads in every command that reads the configuration; its format is
/// inferred per iteration (#504). It was rejected at load with "cannot infer
/// the key format from `reference`".
#[test]
fn a_nested_key_parity_with_a_templated_reference_loads_everywhere() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(
        temp.path().join(".alint.yml"),
        "version: 1\nrules:\n  - id: locale-parity\n    kind: for_each_file\n    \
         select: \"en.json\"\n    level: error\n    require:\n      - kind: key_parity\n        \
         reference: \"{path}\"\n        peers: \"fr.json\"\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("en.json"), "{\"a\": 1, \"b\": 2}\n").unwrap();
    std::fs::write(temp.path().join("fr.json"), "{\"a\": 1}\n").unwrap();

    let cases: &[(&[&str], i32)] = &[
        (&["validate-config"], 0),
        (&["check"], 1),
        (&["list"], 0),
        (&["explain", "locale-parity"], 0),
    ];
    for (args, expected_exit) in cases {
        let output = Command::new(alint())
            .args(*args)
            .current_dir(temp.path())
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(*expected_exit),
            "{args:?}\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(!stderr.contains("cannot infer"), "{args:?}: {stderr}");
        if args == &["check"] {
            assert!(stdout.contains("missing key `b`"), "{stdout}");
        }
    }
}
