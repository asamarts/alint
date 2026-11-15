//! The differential YAML harness (#482, ADR-0035): each input goes through
//! ayaml, alint's parser, and through `yaml_serde` 0.10.7, the parser v0.22
//! used, at the entry points alint decodes through, and each result pair is
//! classified. A difference fails unless an entry of [`allowlist::DEVIATIONS`]
//! documents it.
//!
//! The entry points:
//! - [`Entry::Value`]: a document decoded into a value tree, which every
//!   option, `extends:` source and `alint test` package passes through;
//! - [`Entry::Json`]: the `serde_json` tree the structured-format rules
//!   (`yaml_path_*`, `json_schema_passes`, front matter) query, through alint's
//!   own seed, which applies merge keys and drops custom tags;
//! - [`Entry::Config`]: a typed decode into [`crate::Config`].
//!
//! alint reads no ayaml events in v0.23 (source spans still come from
//! saphyr), so the event stream joins the harness with the saphyr exit in
//! v0.24 (ADR-0036).
//!
//! The inputs are generated token soup, generated documents and mutations of
//! them, the repository's own YAML files, and mutations of those files. The
//! same seed always gives the same inputs, and each file's mutations come from
//! its own seed, so adding a file changes no other input.
//!
//! The bounded run is an ordinary test, so every pull request runs it. The
//! long run is `cargo test --release -p alint-core --lib yaml_differential::
//! -- --ignored`, sized by `ALINT_YAML_DIFF_INPUTS` (300,000),
//! `ALINT_YAML_DIFF_SEED` (1), `ALINT_YAML_DIFF_MUTATIONS` (20 per corpus
//! file) and `ALINT_YAML_DIFF_SPECIALS` (`0` leaves out NEL, LS, PS and the
//! byte-order mark), with a Markdown report written to
//! `ALINT_YAML_DIFF_REPORT` when set.

mod allowlist;
mod generate;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use serde::de::DeserializeSeed;

use generate::Rng;

/// Where alint decodes YAML.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Entry {
    Value,
    Json,
    Config,
}

impl Entry {
    const ALL: [Self; 3] = [Self::Value, Self::Json, Self::Config];
}

/// How the two parsers' results relate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Class {
    /// Both accept, with the same value.
    SameValue,
    /// Both accept, with different values.
    DifferentValue,
    /// Both reject, at the same line and column.
    RejectedAtSameLocation,
    /// Both reject, at different places, or one without a place.
    RejectedAtDifferentLocation,
    /// ayaml rejects what the oracle accepts.
    OnlyAyamlRejects,
    /// The oracle rejects what ayaml accepts.
    OnlyOracleRejects,
}

impl Class {
    const ALL: [Self; 6] = [
        Self::SameValue,
        Self::DifferentValue,
        Self::RejectedAtSameLocation,
        Self::RejectedAtDifferentLocation,
        Self::OnlyAyamlRejects,
        Self::OnlyOracleRejects,
    ];

    /// Whether a result of this class needs an allowlist entry.
    fn is_difference(self) -> bool {
        !matches!(self, Self::SameValue | Self::RejectedAtSameLocation)
    }
}

/// Why one parser refused an input.
#[derive(Clone, Debug)]
pub(crate) struct Rejection {
    pub(crate) message: String,
    /// The one-based line and column, when the error has a place.
    pub(crate) location: Option<(usize, usize)>,
    /// ayaml's kind of error; `None` for the oracle's.
    pub(crate) kind: Option<ayaml::ErrorKind>,
}

/// One parser's result: the decoded value, written with `{:?}` after it is
/// put in a form both parsers share, or the rejection.
pub(crate) type Side = Result<String, Rejection>;

/// One input at one entry point.
#[derive(Debug)]
pub(crate) struct Case<'a> {
    pub(crate) input: &'a str,
    pub(crate) entry: Entry,
    pub(crate) class: Class,
    pub(crate) ayaml: Side,
    pub(crate) oracle: Side,
}

/// A value tree both parsers' values convert into: numbers as written by
/// Rust, every NaN alike, mappings in order, tags kept.
#[derive(Debug, PartialEq)]
enum Node {
    Null,
    Bool(bool),
    Integer(String),
    Float(u64),
    String(String),
    Sequence(Vec<Node>),
    Mapping(Vec<(Node, Node)>),
    Tagged(String, Box<Node>),
}

fn float(value: f64) -> Node {
    Node::Float(if value.is_nan() { f64::NAN } else { value }.to_bits())
}

fn from_ayaml(value: &ayaml::Value) -> Node {
    match value {
        ayaml::Value::Null => Node::Null,
        ayaml::Value::Bool(value) => Node::Bool(*value),
        ayaml::Value::Number(number) if number.is_f64() => float(number.as_f64().unwrap()),
        ayaml::Value::Number(number) => Node::Integer(
            number
                .as_i128()
                .map(|value| value.to_string())
                .or_else(|| number.as_u128().map(|value| value.to_string()))
                .unwrap(),
        ),
        ayaml::Value::String(value) => Node::String(value.clone()),
        ayaml::Value::Sequence(values) => Node::Sequence(values.iter().map(from_ayaml).collect()),
        ayaml::Value::Mapping(mapping) => Node::Mapping(
            mapping
                .iter()
                .map(|(key, value)| (from_ayaml(key), from_ayaml(value)))
                .collect(),
        ),
        ayaml::Value::Tagged(tagged) => {
            Node::Tagged(tagged.tag.to_string(), Box::new(from_ayaml(&tagged.value)))
        }
    }
}

fn from_oracle(value: &yaml_serde::Value) -> Node {
    match value {
        yaml_serde::Value::Null => Node::Null,
        yaml_serde::Value::Bool(value) => Node::Bool(*value),
        yaml_serde::Value::Number(number) if number.is_f64() => float(number.as_f64().unwrap()),
        yaml_serde::Value::Number(number) => Node::Integer(
            number
                .as_i64()
                .map(|value| value.to_string())
                .or_else(|| number.as_u64().map(|value| value.to_string()))
                .unwrap(),
        ),
        yaml_serde::Value::String(value) => Node::String(value.clone()),
        yaml_serde::Value::Sequence(values) => {
            Node::Sequence(values.iter().map(from_oracle).collect())
        }
        yaml_serde::Value::Mapping(mapping) => Node::Mapping(
            mapping
                .iter()
                .map(|(key, value)| (from_oracle(key), from_oracle(value)))
                .collect(),
        ),
        yaml_serde::Value::Tagged(tagged) => {
            Node::Tagged(tagged.tag.to_string(), Box::new(from_oracle(&tagged.value)))
        }
    }
}

fn ayaml_rejection(error: &ayaml::Error) -> Rejection {
    Rejection {
        message: error.message().to_string(),
        location: error
            .location()
            .map(|location| (location.line(), location.column())),
        kind: Some(error.kind()),
    }
}

fn oracle_rejection(error: &yaml_serde::Error) -> Rejection {
    let message = error.to_string();
    // The oracle's Display ends with its own location; drop it, as the
    // ayaml message leaves its own out.
    let message = match message.rfind(" at line ") {
        Some(at) if error.location().is_some() => message[..at].to_string(),
        _ => message,
    };
    Rejection {
        message,
        location: error
            .location()
            .map(|location| (location.line(), location.column())),
        kind: None,
    }
}

/// `config` written with `{:?}`, with its hashed collections in order.
fn config_form(mut config: crate::Config) -> String {
    let vars: BTreeMap<String, String> = std::mem::take(&mut config.vars).into_iter().collect();
    let allow = match std::mem::take(&mut config.allow_out_of_root) {
        crate::config::AllowOutOfRoot::Selective { kinds, rules } => format!(
            "Selective {:?} {:?}",
            kinds.into_iter().collect::<BTreeSet<_>>(),
            rules.into_iter().collect::<BTreeSet<_>>()
        ),
        other => format!("{other:?}"),
    };
    format!("{config:?} vars: {vars:?} allow_out_of_root: {allow}")
}

/// Run `parse`, turning a panic into a rejection the run then fails on.
fn guarded(parse: impl FnOnce() -> Side) -> (Side, bool) {
    match catch_unwind(AssertUnwindSafe(parse)) {
        Ok(side) => (side, false),
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(ToString::to_string)
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            (
                Err(Rejection {
                    message: format!("panic: {message}"),
                    location: None,
                    kind: None,
                }),
                true,
            )
        }
    }
}

/// Both parsers' results for `input` at `entry`, and whether either panicked.
fn run(input: &str, entry: Entry) -> (Side, Side, bool) {
    let (ayaml, ayaml_panicked) = guarded(|| match entry {
        Entry::Value => crate::yaml::from_str::<crate::yaml::Value>(input)
            .map(|value| format!("{:?}", from_ayaml(value.parser())))
            .map_err(|error| ayaml_rejection(&error.into_parser())),
        Entry::Json => {
            crate::yaml::from_str_seed(input, crate::structured_format::json_seed(input))
                .map(|value| format!("{value:?}"))
                .map_err(|error| ayaml_rejection(&error))
        }
        Entry::Config => crate::yaml::from_str::<crate::Config>(input)
            .map(config_form)
            .map_err(|error| ayaml_rejection(&error.into_parser())),
    });
    let (oracle, oracle_panicked) = guarded(|| match entry {
        Entry::Value => yaml_serde::from_str::<yaml_serde::Value>(input)
            .map(|value| format!("{:?}", from_oracle(&value)))
            .map_err(|error| oracle_rejection(&error)),
        Entry::Json => crate::structured_format::json_seed(input)
            .deserialize(yaml_serde::Deserializer::from_str(input))
            .map(|value| format!("{value:?}"))
            .map_err(|error| oracle_rejection(&error)),
        Entry::Config => yaml_serde::from_str::<crate::Config>(input)
            .map(config_form)
            .map_err(|error| oracle_rejection(&error)),
    });
    (ayaml, oracle, ayaml_panicked || oracle_panicked)
}

fn classify(ayaml: &Side, oracle: &Side) -> Class {
    match (ayaml, oracle) {
        (Ok(left), Ok(right)) if left == right => Class::SameValue,
        (Ok(_), Ok(_)) => Class::DifferentValue,
        (Err(left), Err(right)) if left.location.is_some() && left.location == right.location => {
            Class::RejectedAtSameLocation
        }
        (Err(_), Err(_)) => Class::RejectedAtDifferentLocation,
        (Err(_), Ok(_)) => Class::OnlyAyamlRejects,
        (Ok(_), Err(_)) => Class::OnlyOracleRejects,
    }
}

/// The case for `input` at `entry`, and whether a parser panicked.
pub(crate) fn compare(input: &str, entry: Entry) -> (Case<'_>, bool) {
    let (ayaml, oracle, panicked) = run(input, entry);
    let class = classify(&ayaml, &oracle);
    (
        Case {
            input,
            entry,
            class,
            ayaml,
            oracle,
        },
        panicked,
    )
}

/// The counts of a run, and the cases it fails on.
#[derive(Default)]
struct Report {
    inputs: usize,
    classes: BTreeMap<(Entry, Class), usize>,
    /// How often each deviation or location class explained a difference.
    explained: BTreeMap<(Entry, &'static str), usize>,
    /// Inputs whose typed decode was not compared, because the value entry
    /// point did not read them alike.
    config_skipped: usize,
    /// Unexplained differences and panics, the shortest few of each kind.
    failures: BTreeMap<(Entry, Class, bool), Vec<String>>,
    failure_count: usize,
}

const KEPT_FAILURES: usize = 8;

/// What explains `case`: `Some("")` for a result that needs nothing, the id
/// of the deviation or location class that covers a difference, or `None`.
fn explanation(case: &Case<'_>) -> Option<&'static str> {
    match case.class {
        Class::SameValue | Class::RejectedAtSameLocation => Some(""),
        Class::RejectedAtDifferentLocation => match &case.ayaml {
            Err(Rejection {
                location: Some(_),
                kind: Some(kind),
                message,
            }) => allowlist::locate(*kind, message).map(|location| location.id),
            _ => allowlist::explain(case).map(|deviation| deviation.id),
        },
        _ => allowlist::explain(case).map(|deviation| deviation.id),
    }
}

impl Report {
    fn add(&mut self, input: &str) {
        self.inputs += 1;
        let mut value_alike = false;
        for entry in Entry::ALL {
            // A typed decode is compared only for a document both parsers
            // read alike, so its result is about the decode, not the syntax.
            if entry == Entry::Config && !value_alike {
                self.config_skipped += 1;
                continue;
            }
            let (case, panicked) = compare(input, entry);
            if entry == Entry::Value {
                value_alike = case.class == Class::SameValue;
            }
            self.record(&case, panicked);
        }
    }

    fn record(&mut self, case: &Case<'_>, panicked: bool) {
        *self.classes.entry((case.entry, case.class)).or_default() += 1;
        let explained = explanation(case);
        if let Some(id) = explained.filter(|id| !id.is_empty()) {
            *self.explained.entry((case.entry, id)).or_default() += 1;
        }
        if panicked || explained.is_none() {
            self.failure_count += 1;
            let kept = self
                .failures
                .entry((case.entry, case.class, panicked))
                .or_default();
            kept.push(format!(
                "{:?}\n      ayaml:  {}\n      oracle: {}",
                case.input,
                describe(&case.ayaml),
                describe(&case.oracle)
            ));
            kept.sort_by_key(String::len);
            kept.dedup();
            kept.truncate(KEPT_FAILURES);
        }
    }

    fn count(&self, entry: Entry, class: Class) -> usize {
        self.classes.get(&(entry, class)).copied().unwrap_or(0)
    }

    fn markdown(&self, title: &str) -> String {
        let mut out = format!(
            "### {title}\n\n{} inputs; the typed decode skipped {} that the value entry \
             point did not read alike.\n\n",
            self.inputs, self.config_skipped
        );
        out.push_str("| Class | Value | JSON | Config |\n|---|---:|---:|---:|\n");
        for class in Class::ALL {
            let _ = write!(out, "| {class:?} |");
            for entry in Entry::ALL {
                let _ = write!(out, " {} |", self.count(entry, class));
            }
            out.push('\n');
        }
        out.push_str("\n| Explained by | Value | JSON | Config |\n|---|---:|---:|---:|\n");
        let ids = allowlist::DEVIATIONS
            .iter()
            .map(|deviation| deviation.id)
            .chain(allowlist::LOCATIONS.iter().map(|location| location.id));
        for id in ids {
            let _ = write!(out, "| `{id}` |");
            for entry in Entry::ALL {
                let count = self.explained.get(&(entry, id)).copied().unwrap_or(0);
                let _ = write!(out, " {count} |");
            }
            out.push('\n');
        }
        let _ = writeln!(
            out,
            "\n{} unexplained differences or panics.",
            self.failure_count
        );
        for ((entry, class, panicked), inputs) in &self.failures {
            let what = if *panicked { "panic" } else { "unexplained" };
            let _ = writeln!(out, "\n{what}, {entry:?}, {class:?}:");
            for input in inputs {
                let _ = writeln!(out, "  - {input}");
            }
        }
        out
    }
}

/// The allowlist as Markdown, for the long run's report.
fn allowlist_markdown() -> String {
    let mut out = String::from("\n### Documented differences\n");
    for deviation in allowlist::DEVIATIONS {
        let _ = write!(
            out,
            "\n- `{}` ({:?}): ayaml {}; the oracle {}. Reason: {}. Example: `{:?}`.\n",
            deviation.id,
            deviation.classes,
            deviation.ayaml,
            deviation.oracle,
            deviation.reason,
            deviation.example
        );
    }
    out.push_str("\n### Documented error locations\n");
    for location in allowlist::LOCATIONS {
        let _ = write!(
            out,
            "\n- `{}`: ayaml reports {}; the oracle reports {}. Example: `{:?}`.\n",
            location.id, location.ayaml, location.oracle, location.example
        );
    }
    out
}

fn describe(side: &Side) -> String {
    match side {
        Ok(value) => {
            let mut value = value.clone();
            if let Some((cut, _)) = value.char_indices().nth(160) {
                value.truncate(cut);
                value.push('…');
            }
            format!("ok {value}")
        }
        Err(rejection) => match rejection.location {
            Some((line, column)) => format!("error at {line}:{column}: {}", rejection.message),
            None => format!("error: {}", rejection.message),
        },
    }
}

/// The repository's root.
fn repository() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The committed YAML files of the repository, by path, with their line
/// breaks as committed whatever Git's `core.autocrlf`; none outside a Git
/// checkout.
fn corpus() -> Vec<(String, String)> {
    let root = repository();
    let Ok(output) = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z", "--", "*.yml", "*.yaml"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .filter_map(|path| {
            let path = String::from_utf8_lossy(path).into_owned();
            let text = String::from_utf8(std::fs::read(root.join(&path)).ok()?).ok()?;
            Some((path, text.replace("\r\n", "\n")))
        })
        .collect()
}

/// FNV-1a, to give each corpus file a seed of its own.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A run over `inputs` generated inputs from `seed`, the corpus, and
/// `mutations` mutations of each corpus file. Half the generated inputs and
/// mutations hold [`generate::SPECIALS`] when `specials` is set, and none
/// when it is not.
fn differential_run(seed: u64, inputs: usize, mutations: usize, specials: bool) -> Report {
    let mut rng = Rng::new(seed);
    let mut report = Report::default();
    for index in 0..inputs {
        let specials = specials && index % 2 == 1;
        let input = match index % 6 {
            0 | 1 => generate::token_soup(&mut rng, specials),
            2 | 3 => generate::document(&mut rng, specials),
            _ => {
                let document = generate::document(&mut rng, specials);
                generate::mutate(&mut rng, &document, specials)
            }
        };
        report.add(&input);
    }
    for (path, text) in corpus() {
        report.add(&text);
        let mut rng = Rng::new(seed ^ fnv1a(&path));
        for mutation in 0..mutations {
            report.add(&generate::mutate(
                &mut rng,
                &text,
                specials && mutation % 2 == 1,
            ));
        }
    }
    report
}

fn env_number(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.replace('_', "").parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
#[path = "yaml_differential_tests.rs"]
mod tests;
