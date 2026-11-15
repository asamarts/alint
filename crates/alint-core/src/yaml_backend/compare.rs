//! Compare mode (ADR-0041). ayaml decodes, and its result is what the caller
//! gets. libyaml reads the same input, and so does saphyr, which reads YAML
//! 1.2. A difference between ayaml and libyaml must be explained, by saphyr
//! reading the input as ayaml does or by an entry of the differential
//! harness's allowlist, exactly as the harness explains one. Otherwise the
//! decode panics, with the input and the three results.
//!
//! A typed decode is compared by whether each parser succeeds, since typed
//! values have no common form, and only where the value trees agree: a
//! difference in the trees already explains one in a decode. A decode into
//! [`crate::Config`] goes through the harness's `Config` entry point. Any
//! other type's difference is put down to a core tag in the input, the one
//! cause the allowlist documents (`typed-decode-core-tags`), and the log
//! says the cause is unchecked; with no core tag, the decode panics.
//!
//! With `ALINT_YAML_COMPARE_LOG` naming a file, each explained difference
//! appends one JSON line to it, for a report.

use std::any::type_name;
use std::io::Write as _;

use serde::Deserialize;

use crate::yaml::Error;
use crate::yaml_differential::{self as harness, Case, Entry};

/// The variable naming the file explained differences are logged to.
pub(crate) const LOG_VARIABLE: &str = "ALINT_YAML_COMPARE_LOG";

/// [`crate::yaml::from_str`] in compare mode.
pub(super) fn from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    let result = crate::yaml::ayaml_from_str::<T>(input);
    let libyaml = super::libyaml_from_str::<T>(input).is_ok();
    let trees_differ = check(input, Entry::Value);
    if !trees_differ && result.is_ok() != libyaml {
        typed_difference::<T>(input, result.is_ok());
    }
    result
}

/// [`super::json_tree`] in compare mode.
pub(super) fn json_tree(input: &str) -> Result<serde_json::Value, Error> {
    let result = super::ayaml_json_tree(input);
    check(input, Entry::Json);
    result
}

/// Check `input` at `entry`, and whether its results differ, explained.
fn check(input: &str, entry: Entry) -> bool {
    let (case, panicked) = harness::compare(input, entry);
    if panicked {
        fail(&case, "a parser panicked");
    }
    if !case.class.is_difference() {
        return false;
    }
    if harness::saphyr_agrees(&case) {
        log(&case, "saphyr", &[]);
    } else if let Some(ids) = harness::explain(&case) {
        log(&case, "allowlist", &ids);
    } else {
        fail(
            &case,
            "nothing explains this difference: saphyr does not read the input as ayaml does, \
             and no allowlist entry covers it",
        );
    }
    true
}

/// A typed decode into `T` that one parser refuses, where the value trees
/// agree.
fn typed_difference<T>(input: &str, ayaml_succeeded: bool) {
    if type_name::<T>() == type_name::<crate::Config>() {
        if !check(input, Entry::Config) {
            let (case, _) = harness::compare(input, Entry::Config);
            fail(
                &case,
                "the decodes into Config differ, but the harness finds no difference",
            );
        }
        return;
    }
    let (case, _) = harness::compare(input, Entry::Value);
    let who = if ayaml_succeeded { "libyaml" } else { "ayaml" };
    if input.contains("!!") {
        log(
            &case,
            "typed-decode-core-tags (unchecked)",
            &[type_name::<T>()],
        );
    } else {
        fail(
            &case,
            &format!(
                "only {who} refuses the decode into {}, the value trees agree, and the input \
                 holds no core tag",
                type_name::<T>()
            ),
        );
    }
}

/// Append a line about an explained `case` to the log, if there is one.
fn log(case: &Case<'_>, verdict: &str, ids: &[&str]) {
    if let Some(path) = std::env::var_os(LOG_VARIABLE) {
        append(std::path::Path::new(&path), case, verdict, ids);
    }
}

/// Append a line about an explained `case` to the file at `path`.
fn append(path: &std::path::Path, case: &Case<'_>, verdict: &str, ids: &[&str]) {
    let mut line = serde_json::json!({
        "verdict": verdict,
        "entry": format!("{:?}", case.entry),
        "class": format!("{:?}", case.class),
        "ids": ids,
        "input": harness::shortened(case.input),
    })
    .to_string();
    line.push('\n');
    // One write of one short line, to a file opened to append, so that the
    // processes of a test run do not interleave their lines.
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
    if let Err(error) = written {
        panic!(
            "{LOG_VARIABLE}: cannot append to {}: {error}",
            path.display()
        );
    }
}

/// Panic with `reason` and the three parsers' results for `case`.
fn fail(case: &Case<'_>, reason: &str) -> ! {
    let saphyr = if case.entry == Entry::Value {
        format!("{:?}", harness::run_saphyr(case.input))
    } else {
        "(no vote at this entry point)".to_string()
    };
    panic!(
        "ALINT_YAML_PARSER=compare: {reason}\n  entry point: {:?}\n  class: {:?}\n  input: {}\n  \
         ayaml: {:?}\n  libyaml: {:?}\n  saphyr: {saphyr}",
        case.entry,
        case.class,
        harness::shortened(case.input),
        case.ayaml,
        case.oracle,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
        payload
            .downcast_ref::<String>()
            .cloned()
            .unwrap_or_default()
    }

    /// libyaml reads NEL as a line break, which leaves an implicit key
    /// without a `:` (`yaml-1.2-line-breaks`).
    const NEL: &str = "a: x\u{85}y\n";

    #[test]
    fn the_caller_gets_ayamls_result() {
        let value: crate::yaml::Value = from_str(NEL).unwrap();
        assert_eq!(value["a"].as_str(), Some("x\u{85}y"));
        assert_eq!(json_tree(NEL).unwrap()["a"], "x\u{85}y");
        // A difference in a typed decode where the value trees agree, which
        // the harness's `Config` entry point explains.
        let config = "version: !!int '1'\nrules: []\n";
        let ayaml = crate::yaml::ayaml_from_str::<crate::Config>(config).is_ok();
        assert_eq!(from_str::<crate::Config>(config).is_ok(), ayaml);
    }

    #[test]
    fn a_decode_both_parsers_agree_on_needs_no_explanation() {
        // Both read it, and both refuse it as a `u8`: nothing to explain,
        // where a typed difference without a core tag would panic.
        let value: crate::yaml::Value = from_str("a: 1\n").unwrap();
        assert_eq!(value["a"].as_u64(), Some(1));
        assert!(from_str::<u8>("a: 1\n").is_err());
    }

    /// Set in the child of [`the_log_names_each_explained_difference`]: the
    /// file it logs to.
    const CHILD_LOG: &str = "ALINT_YAML_COMPARE_TEST_CHILD";

    /// The log is read from the environment, which a test sets only in a
    /// child process of this binary.
    #[test]
    fn the_log_names_each_explained_difference() {
        if let Some(path) = std::env::var_os(CHILD_LOG) {
            assert_eq!(std::env::var_os(LOG_VARIABLE), Some(path));
            assert!(check(NEL, Entry::Value));
            assert!(!check("a: 1\n", Entry::Value));
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compare.jsonl");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "yaml_backend::compare::tests::the_log_names_each_explained_difference",
                "--nocapture",
            ])
            .env(CHILD_LOG, &path)
            .env(LOG_VARIABLE, &path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(text.contains("\"entry\":\"Value\""), "{text}");
    }

    #[test]
    fn only_a_difference_is_explained() {
        assert!(check(NEL, Entry::Value));
        assert!(check(NEL, Entry::Json));
        assert!(!check("a: 1\n", Entry::Value));
        assert!(!check("a: [1\n", Entry::Value));
    }

    #[test]
    fn a_typed_difference_needs_a_cause() {
        // A core tag: logged as unchecked.
        typed_difference::<u8>("a: !!int '1'\n", true);
        let payload =
            std::panic::catch_unwind(|| typed_difference::<u8>("a: 1\n", false)).unwrap_err();
        let message = panic_message(&*payload);
        assert!(
            message.contains("only ayaml refuses the decode into u8")
                && message.contains("no core tag"),
            "{message}"
        );
        // A decode into `Config` the harness reads alike.
        let payload = std::panic::catch_unwind(|| {
            typed_difference::<crate::Config>("version: 1\nrules: []\n", true);
        })
        .unwrap_err();
        let message = panic_message(&*payload);
        assert!(
            message.contains("the decodes into Config differ, but the harness finds no difference"),
            "{message}"
        );
    }

    #[test]
    fn a_failure_names_the_input_and_each_parsers_result() {
        let (case, _) = harness::compare(NEL, Entry::Value);
        let payload = std::panic::catch_unwind(|| fail(&case, "the reason")).unwrap_err();
        let message = panic_message(&*payload);
        for part in [
            "ALINT_YAML_PARSER=compare: the reason",
            "entry point: Value",
            "class: OnlyOracleRejects",
            "input: \"a: x\\u{85}y\\n\"",
            "ayaml: Ok(",
            "libyaml: Err(",
            "saphyr: Ok(",
        ] {
            assert!(message.contains(part), "{part}: {message}");
        }
        let (case, _) = harness::compare(NEL, Entry::Json);
        let payload = std::panic::catch_unwind(|| fail(&case, "the reason")).unwrap_err();
        assert!(panic_message(&*payload).contains("saphyr: (no vote at this entry point)"));
    }

    #[test]
    fn each_explained_difference_appends_a_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("compare.jsonl");
        let (case, _) = harness::compare(NEL, Entry::Value);
        append(&path, &case, "allowlist", &["yaml-1.2-line-breaks"]);
        append(&path, &case, "saphyr", &[]);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{text}");
        assert_eq!(lines[0]["verdict"], "allowlist");
        assert_eq!(lines[0]["entry"], "Value");
        assert_eq!(lines[0]["class"], "OnlyOracleRejects");
        assert_eq!(lines[0]["ids"], serde_json::json!(["yaml-1.2-line-breaks"]));
        assert_eq!(lines[0]["input"], "\"a: x\\u{85}y\\n\"");
        assert_eq!(lines[1]["verdict"], "saphyr");
        // A log that cannot be written fails the run, not the line.
        let payload = std::panic::catch_unwind(|| {
            append(dir.path(), &case, "saphyr", &[]);
        })
        .unwrap_err();
        assert!(panic_message(&*payload).contains("cannot append to"));
    }
}
