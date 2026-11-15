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
    let Some(path) = std::env::var_os(LOG_VARIABLE) else {
        return;
    };
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
        .open(&path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
    if let Err(error) = written {
        panic!(
            "{LOG_VARIABLE}: cannot append to {}: {error}",
            path.to_string_lossy()
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
