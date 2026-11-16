//! The differential YAML harness (#482, ADR-0035): each input goes through
//! ayaml, alint's parser, and through `yaml_serde` 0.10.7, the parser v0.22
//! used, at the entry points alint decodes through, and each result pair is
//! classified. A difference fails unless an entry of [`allowlist::DEVIATIONS`]
//! or [`allowlist::LOCATIONS`] explains it (see [`explanation`]).
//!
//! The entry points:
//! - [`Entry::Value`]: a document decoded into a value tree, which every
//!   option, `extends:` source and `alint test` package passes through;
//! - [`Entry::Json`]: the `serde_json` tree the structured-format rules
//!   (`yaml_path_*`, `json_schema_passes`, front matter) query, through alint's
//!   own seed, which applies merge keys and drops custom tags;
//! - [`Entry::Config`]: a typed decode into [`crate::Config`];
//! - [`Entry::Rule`]: each entry of `rules:` decoded from its value tree into
//!   a [`crate::RuleSpec`] with `from_value`, as alint's loader decodes rules,
//!   each rule a case of its own.
//!
//! alint reads no ayaml events in v0.23 (source spans still come from
//! saphyr), so the event stream joins the harness with the saphyr exit in
//! v0.24 (ADR-0036).
//!
//! The inputs are generated token soup, generated documents, generated
//! configurations and mutations of them, the repository's own YAML files, and
//! mutations of those files. The same seed always gives the same inputs, and
//! each file's mutations come from its own seed, so adding a file changes no
//! other input.
//!
//! The bounded run is an ordinary test of `cargo test`, which CI's test job
//! runs when Rust code changes. The long run is
//! `cargo test --release -p alint-core --lib yaml_differential:: -- --ignored`,
//! sized by `ALINT_YAML_DIFF_INPUTS` (300,000), `ALINT_YAML_DIFF_SEED` (1),
//! `ALINT_YAML_DIFF_MUTATIONS` (20 per corpus file) and
//! `ALINT_YAML_DIFF_SPECIALS` (`0` leaves out NEL, LS, PS and the byte-order
//! mark), with a Markdown report written to `ALINT_YAML_DIFF_REPORT` when set.
//! A value that is not a number fails the run.

mod allowlist;
#[cfg(test)]
mod configuration;
#[cfg(test)]
mod generate;
mod neutralize;
mod predicates;
mod rule_errors;

use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
#[cfg(test)]
use std::path::Path;

use serde::de::DeserializeSeed;

use allowlist::{Hidden, Trigger};
#[cfg(test)]
use generate::Rng;

/// Where alint decodes YAML.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Entry {
    Value,
    Json,
    Config,
    Rule,
}

impl Entry {
    #[cfg(test)]
    const ALL: [Self; 4] = [Self::Value, Self::Json, Self::Config, Self::Rule];
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
    #[cfg(test)]
    const ALL: [Self; 6] = [
        Self::SameValue,
        Self::DifferentValue,
        Self::RejectedAtSameLocation,
        Self::RejectedAtDifferentLocation,
        Self::OnlyAyamlRejects,
        Self::OnlyOracleRejects,
    ];

    /// Whether a result of this class needs an allowlist entry.
    pub(crate) fn is_difference(self) -> bool {
        !matches!(self, Self::SameValue | Self::RejectedAtSameLocation)
    }
}

/// Why one parser refused an input.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Rejection {
    pub(crate) message: String,
    /// The one-based line and column, when the error has a place. At
    /// [`Entry::Rule`], the line is the failing rule's position in `rules:`
    /// and the column is 0.
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
    /// At [`Entry::Rule`], the index in `rules:` of the rule the case
    /// decodes; 0 elsewhere.
    pub(crate) rule: usize,
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

/// saphyr's value as a [`Node`], or why it has none: an alias it did not
/// resolve, or a scalar its tag does not fit, which ayaml rejects.
fn from_saphyr(value: &saphyr::Yaml<'_>) -> Result<Node, String> {
    use saphyr::{Scalar, Yaml};
    Ok(match value {
        Yaml::Value(Scalar::Null) => Node::Null,
        Yaml::Value(Scalar::Boolean(value)) => Node::Bool(*value),
        Yaml::Value(Scalar::Integer(value)) => Node::Integer(value.to_string()),
        Yaml::Value(Scalar::FloatingPoint(value)) => float(value.into_inner()),
        Yaml::Value(Scalar::String(value)) => Node::String(value.to_string()),
        Yaml::Sequence(values) => {
            Node::Sequence(values.iter().map(from_saphyr).collect::<Result<_, _>>()?)
        }
        Yaml::Mapping(mapping) => Node::Mapping(
            mapping
                .iter()
                .map(|(key, value)| Ok((from_saphyr(key)?, from_saphyr(value)?)))
                .collect::<Result<_, String>>()?,
        ),
        Yaml::Tagged(tag, value) => Node::Tagged(
            format!("{}{}", tag.handle, tag.suffix),
            Box::new(from_saphyr(value)?),
        ),
        Yaml::Representation(..) => return Err("an unresolved scalar".to_string()),
        Yaml::Alias(_) => return Err("an unresolved alias".to_string()),
        Yaml::BadValue => return Err("a scalar its tag does not fit".to_string()),
    })
}

/// saphyr's value tree for `input`, as the value entry point reads it: no
/// document is null, and more than one is an error.
pub(crate) fn run_saphyr(input: &str) -> Side {
    use saphyr::LoadableYamlNode as _;
    let rejection = |message: String| Rejection {
        message,
        location: None,
        kind: None,
    };
    let documents =
        saphyr::Yaml::load_from_str(input).map_err(|error| rejection(error.to_string()))?;
    match documents.as_slice() {
        [] => Ok(format!("{:?}", Node::Null)),
        [document] => from_saphyr(document)
            .map(|node| format!("{node:?}"))
            .map_err(rejection),
        _ => Err(rejection("more than one document".to_string())),
    }
}

/// Whether saphyr, which reads YAML 1.2, sides with ayaml on a difference
/// at the value entry point: it reads the value ayaml reads, or rejects the
/// input ayaml rejects (ADR-0041). It does not vote on where an error is,
/// nor on the other entry points, which saphyr has no serde support for.
pub(crate) fn saphyr_agrees(case: &Case<'_>) -> bool {
    if case.entry != Entry::Value {
        return false;
    }
    let voting = matches!(
        case.class,
        Class::DifferentValue | Class::OnlyAyamlRejects | Class::OnlyOracleRejects
    );
    if !voting {
        return false;
    }
    let (saphyr, panicked) = guarded(|| run_saphyr(case.input));
    if panicked {
        return false;
    }
    match (&case.ayaml, &saphyr) {
        (Ok(ayaml), Ok(saphyr)) => ayaml == saphyr,
        (Err(_), Err(_)) => true,
        _ => false,
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
    Rejection {
        // The oracle's Display holds its own location, and the context's
        // when it differs; the ayaml message leaves its own out.
        message: crate::yaml_backend::without_marks(&error.to_string()),
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

/// A rule's decoded spec, written with `{:?}`, or its error, placed at the
/// rule's position `index` in `rules:`.
fn rule_form(index: usize, spec: Result<crate::RuleSpec, Rejection>) -> Side {
    spec.map(|spec| format!("{spec:?}"))
        .map_err(|rejection| Rejection {
            location: Some((index + 1, 0)),
            ..rejection
        })
}

/// One parser's results for the rules of an input: each rule's, or the
/// document's error.
type RuleSides = Result<Vec<Side>, Rejection>;

/// ayaml's result for each rule of `rules:` in `input`, decoded one by one
/// from the document's value tree.
fn ayaml_rules(input: &str) -> RuleSides {
    let value = crate::yaml::ayaml_from_str::<crate::yaml::Value>(input)
        .map_err(|error| ayaml_rejection(&error.into_parser()))?;
    let rules = value
        .get("rules")
        .and_then(crate::yaml::Value::as_sequence)
        .map(|rules| rules.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    Ok(rules
        .into_iter()
        .enumerate()
        .map(|(index, rule)| {
            let spec = crate::yaml::from_value(rule)
                .map_err(|error| ayaml_rejection(&error.into_parser()));
            rule_form(index, spec)
        })
        .collect())
}

/// The oracle's result for each rule of `rules:` in `input`.
fn oracle_rules(input: &str) -> RuleSides {
    let value = yaml_serde::from_str::<yaml_serde::Value>(input)
        .map_err(|error| oracle_rejection(&error))?;
    let rules = value
        .get("rules")
        .and_then(yaml_serde::Value::as_sequence)
        .cloned()
        .unwrap_or_default();
    Ok(rules
        .into_iter()
        .enumerate()
        .map(|(index, rule)| {
            let spec = yaml_serde::from_value(rule).map_err(|error| oracle_rejection(&error));
            rule_form(index, spec)
        })
        .collect())
}

/// The result for the rule at `index` among a parser's `rules`: each rule is
/// a case of its own, so that a rule one parser rejects hides no difference
/// in another. An input without that rule is rejected without a place.
fn rule_side(rules: &RuleSides, index: usize) -> Side {
    match rules {
        Ok(sides) => sides.get(index).cloned().unwrap_or_else(|| {
            Err(Rejection {
                message: format!("no rule {}", index + 1),
                location: None,
                kind: None,
            })
        }),
        Err(rejection) => Err(rejection.clone()),
    }
}

/// The case of each rule of `input`, with the document decoded once for all
/// of them, and whether a parser panicked; none for an input without rules,
/// unless a parser panicked.
#[cfg(test)]
fn rule_cases(input: &str) -> Vec<(Case<'_>, bool)> {
    let (ayaml, ayaml_panicked) = guarded(|| ayaml_rules(input));
    let (oracle, oracle_panicked) = guarded(|| oracle_rules(input));
    let panicked = ayaml_panicked || oracle_panicked;
    let count = [&ayaml, &oracle]
        .iter()
        .filter_map(|rules| rules.as_ref().ok().map(Vec::len))
        .max()
        .unwrap_or(0)
        .max(usize::from(panicked));
    (0..count)
        .map(|rule| {
            let (ayaml, oracle) = (rule_side(&ayaml, rule), rule_side(&oracle, rule));
            let mut case = Case {
                input,
                entry: Entry::Rule,
                rule,
                class: classify(&ayaml, &oracle),
                ayaml,
                oracle,
            };
            settle_same_location(&mut case);
            (case, panicked)
        })
        .collect()
}

/// Run `parse`, turning a panic into a rejection the run then fails on.
fn guarded<T>(parse: impl FnOnce() -> Result<T, Rejection>) -> (Result<T, Rejection>, bool) {
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

/// ayaml's result for `input` at `entry`, and at [`Entry::Rule`] for the
/// rule at index `rule`, with any defect a test planted in it.
fn run_ayaml(input: &str, entry: Entry, rule: usize) -> Side {
    planted::apply(input, entry, ayaml_side(input, entry, rule))
}

/// ayaml's result for `input` at `entry` (see [`run_ayaml`]).
fn ayaml_side(input: &str, entry: Entry, rule: usize) -> Side {
    match entry {
        Entry::Value => crate::yaml::ayaml_from_str::<crate::yaml::Value>(input)
            .map(|value| format!("{:?}", from_ayaml(value.parser())))
            .map_err(|error| ayaml_rejection(&error.into_parser())),
        Entry::Json => {
            crate::yaml::ayaml_from_str_seed(input, crate::structured_format::json_seed(input))
                .map(|value| format!("{value:?}"))
                .map_err(|error| ayaml_rejection(&error))
        }
        Entry::Config => crate::yaml::ayaml_from_str::<crate::Config>(input)
            .map(config_form)
            .map_err(|error| ayaml_rejection(&error.into_parser())),
        Entry::Rule => rule_side(&ayaml_rules(input), rule),
    }
}

/// The oracle's result for `input` at `entry`, and at [`Entry::Rule`] for
/// the rule at index `rule`.
fn run_oracle(input: &str, entry: Entry, rule: usize) -> Side {
    crate::yaml_backend::before_libyaml_decode();
    match entry {
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
        Entry::Rule => rule_side(&oracle_rules(input), rule),
    }
}

/// Both parsers' results for `input` at `entry` (and `rule`), and whether
/// either panicked.
fn run(input: &str, entry: Entry, rule: usize) -> (Side, Side, bool) {
    let (ayaml, ayaml_panicked) = guarded(|| run_ayaml(input, entry, rule));
    let (oracle, oracle_panicked) = guarded(|| run_oracle(input, entry, rule));
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

/// The case for `input` at `entry`, and whether a parser panicked; at
/// [`Entry::Rule`], the case of the first rule.
pub(crate) fn compare(input: &str, entry: Entry) -> (Case<'_>, bool) {
    compare_at(input, entry, 0)
}

/// The case for `input` at `entry`, and at [`Entry::Rule`] for the rule at
/// index `rule`, and whether a parser panicked.
pub(crate) fn compare_at(input: &str, entry: Entry, rule: usize) -> (Case<'_>, bool) {
    let (ayaml, oracle, panicked) = run(input, entry, rule);
    let class = classify(&ayaml, &oracle);
    let mut case = Case {
        input,
        entry,
        rule,
        class,
        ayaml,
        oracle,
    };
    settle_same_location(&mut case);
    (case, panicked)
}

/// Two errors at one place are one only when both are about the syntax or
/// neither is: the oracle reports a control character at 1:1, where ayaml
/// reports a duplicate key of the root mapping. A rule's errors are both
/// at the rule's position, and are one only when they agree on the field
/// and the problem (see [`rule_errors::same_rule_error`]).
fn settle_same_location(case: &mut Case<'_>) {
    if case.class == Class::RejectedAtSameLocation
        && if case.entry == Entry::Rule {
            !rule_errors::same_rule_error(case)
        } else {
            one_rejects_the_syntax(case).is_some()
        }
    {
        case.class = Class::RejectedAtDifferentLocation;
    }
}

/// What explains `case`: no id for a result that needs no explanation, the
/// ids of the deviations and location class that explain a difference, or
/// `None` when nothing does.
///
/// A rejection at another place, where ayaml's error has a place, is
/// explained by the first location class that covers ayaml's error. Any
/// other difference goes through the deviations in order:
/// - the first whose narrow check covers the case explains it;
/// - an input-wide entry that covers it rewrites the input without its
///   trigger, and the rewritten input is compared again at the same entry
///   point. The case is explained if the rewritten one no longer differs or
///   is explained in turn, by these same steps, with the entries not yet
///   applied on the way to it. So an input-wide entry explains a difference
///   only when its trigger causes it: removing the trigger must remove the
///   difference, or leave one that another entry explains.
///
/// A rewrite is a fair counterfactual only if it hides nothing the case
/// showed. Each parser that read the case must read the rewritten input.
/// Each that rejected it must read the rewritten input, reject it with the
/// same message wherever the error now is, or reject it for another reason
/// no earlier in the input: removing the trigger of the first error may show
/// one after it, but an error the rewrite puts in front of the old one is
/// one the rewrite made. A rewrite after which a parser rejects what it
/// read, or rejects earlier for another reason, could put a documented error
/// in front of an undocumented difference, and is not used, unless the other
/// parser rejected the case and still gives that error at the place where
/// this one now rejects, which the trigger had kept it from; neither is one
/// after which both reject, unless the parser that rejected the case rejects
/// it with the same message and the other now gives the same value error,
/// as a duplicate key once a core tag no longer tells two keys apart, which
/// explains the case. An entry whose rewrite changes neither result is not
/// listed among the ids.
///
/// One exception: an entry whose trigger is documented to hide an ayaml
/// error ([`allowlist::Deviation::hides`]) may make ayaml reject with that
/// error where the oracle rejected the case, so that no two values were
/// compared.
fn explanation(case: &Case<'_>) -> Option<Vec<&'static str>> {
    explanation_without(case, &[])
}

/// [`explanation`], for the parser facade's compare mode.
#[cfg(feature = "yaml-reference")]
pub(crate) fn explain(case: &Case<'_>) -> Option<Vec<&'static str>> {
    explanation(case)
}

/// [`explanation`] with the deviations of `excluded` left out. The search
/// credits the first rewrite after which the case is explained, so an entry
/// whose rewrite changed nothing that mattered may be among the ids, as the
/// line separator entry is for `- 'x'#c` before a quoted scalar with a line
/// separator, which both parsers read alike. So each entry of an
/// explanation of more than one is left out in turn, and the explanation the
/// search then finds kept when its entries are among the others.
fn explanation_without(case: &Case<'_>, excluded: &[&'static str]) -> Option<Vec<&'static str>> {
    let search = |excluded: &[&'static str]| {
        let mut search = Search {
            tried: excluded.to_vec(),
            ids: Vec::new(),
            comparisons: 0,
            complete: false,
        };
        search.explain(case).then_some(search.ids)
    };
    let mut excluded = excluded.to_vec();
    let mut ids = search(&excluded)?;
    let mut needed: Vec<&'static str> = Vec::new();
    while let Some(&id) = ids.iter().find(|id| !needed.contains(id)) {
        if ids.iter().all(|other| *other == id) {
            break;
        }
        excluded.push(id);
        match search(&excluded) {
            Some(fewer)
                if !fewer.contains(&id) && fewer.iter().all(|other| ids.contains(other)) =>
            {
                ids = fewer;
            }
            _ => {
                excluded.pop();
                needed.push(id);
            }
        }
    }
    // A case split in two may need an entry on each side: it is named once.
    let mut named = Vec::with_capacity(ids.len());
    ids.retain(|id| {
        let first = !named.contains(id);
        named.push(*id);
        first
    });
    Some(ids)
}

/// The most comparisons one explanation makes. A case that needs more is
/// left unexplained, so a run fails rather than slows down.
const MAX_COMPARISONS: usize = 64;

/// The state of one [`explanation`].
#[derive(Clone)]
struct Search {
    /// The deviations applied on the way to this case, or left out.
    tried: Vec<&'static str>,
    /// The ids that explain the case so far.
    ids: Vec<&'static str>,
    comparisons: usize,
    /// Whether a rewrite on the way revealed an error the case's error hid
    /// ([`Fairness::Revealed`]), so that only a case both parsers read alike
    /// ends the search.
    complete: bool,
}

/// How fair a counterfactual a rewritten case is, the fairest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Fairness {
    /// Each parser that read the case reads the rewritten input, or, where
    /// the other parser rejected the case and still gives that error,
    /// rejects it at the same place. Each that rejected the case reads the
    /// rewritten input, or rejects it with the same message, with a value
    /// error or one about an alias, which a parser reports where the node
    /// starts, or for another reason no earlier than the line of its error.
    Kept,
    /// As [`Fairness::Kept`], but a parser that rejected the case rejects the
    /// rewritten input for another reason on an earlier line: removing the
    /// trigger showed an error that the case's error hid, as a tab that a
    /// NEL, read as a line break, had made part of a plain scalar. The text
    /// between the two errors is compared only if the search then reaches a
    /// case both parsers read alike, which it must.
    Revealed,
    /// One parser rejected the case and rejects the rewritten input with the
    /// same message, and the other, which read the case or rejected it for
    /// another reason, now gives the same value error, or, outside a rule,
    /// an error at the same place.
    BothAgree,
}

/// Whether `rejection` is about a value, not the syntax: a duplicate key,
/// or a decode error of ayaml's or of serde's.
fn value_error(rejection: &Rejection) -> bool {
    rejection.kind == Some(ayaml::ErrorKind::Serde)
        || [
            "duplicate entry",
            "invalid value",
            "invalid type",
            "invalid length",
            "unknown variant",
            "unknown field",
            "missing field",
        ]
        .iter()
        .any(|message| rejection.message.contains(message))
}

/// Whether `rejection` is one of ayaml's errors about a value, which it
/// reports only once the whole stream's syntax is read.
fn ayaml_value_error(rejection: &Rejection) -> bool {
    rejection.kind.is_some_and(|kind| {
        matches!(
            kind,
            ayaml::ErrorKind::Serde
                | ayaml::ErrorKind::DuplicateKey
                | ayaml::ErrorKind::Resolution
                | ayaml::ErrorKind::Composition
                | ayaml::ErrorKind::Limit
        )
    })
}

/// Whether `rejection` is one of ayaml's errors about the syntax: its
/// scanner's and its parser's.
fn ayaml_syntax_error(rejection: &Rejection) -> bool {
    rejection
        .kind
        .is_some_and(|kind| matches!(kind, ayaml::ErrorKind::Scanner | ayaml::ErrorKind::Parser))
}

/// Whether `rejection` is about an alias: ayaml's, which it gives as it
/// builds values, and the oracle's, which it gives as it reads.
fn alias_error(rejection: &Rejection) -> bool {
    let messages = if rejection.kind.is_some() {
        AYAML_ALIAS_MESSAGES
    } else {
        ORACLE_ALIAS_MESSAGES
    };
    messages
        .iter()
        .any(|message| rejection.message.contains(message))
}

/// How fair one parser's result on the rewritten input `next` is to its
/// result on `case` (see [`Fairness`]), or `None` when it hides something
/// the case showed. `side` picks the parser, and `other` the other one.
fn side_fairness(
    case: &Case<'_>,
    next: &Case<'_>,
    side: for<'c> fn(&'c Case<'_>) -> &'c Side,
    other: for<'c> fn(&'c Case<'_>) -> &'c Side,
) -> Option<Fairness> {
    let place = |case: &Case<'_>, side: for<'c> fn(&'c Case<'_>) -> &'c Side| {
        predicates::error_offset(case.input, side(case))
    };
    let moved = |at| original_offset(case.input, next.input, at);
    match (side(case), side(next)) {
        (_, Ok(_)) => Some(Fairness::Kept),
        // ayaml reads the whole stream's syntax before it builds a value, so
        // a syntax error the rewrite makes hides the value error the case
        // showed, wherever each is. The oracle's errors have no kind. A
        // second document is the exception: ayaml finds it only once it
        // built the first one's value, which then held no error.
        (Err(before), Err(after))
            if ayaml_value_error(before)
                && ayaml_syntax_error(after)
                && !after.message.contains(AYAML_SECOND_DOCUMENT) =>
        {
            None
        }
        // The other parser rejected the case with the error it still gives,
        // and this one now gives an error at the same place: the trigger made
        // this parser read what the other could not. An error the other
        // parser gives only now is one the rewrite made, which could hide a
        // difference after it. A rule's place is only its position, which
        // says nothing of where.
        (Ok(_), Err(_)) => (matches!(
            (other(case), other(next)),
            (Err(before), Err(after)) if before.message == after.message
        ) && next.class == Class::RejectedAtSameLocation
            && case.entry != Entry::Rule)
            .then_some(Fairness::Kept),
        // A value error, or one about an alias, is reported where its node
        // starts, not where the parser stopped reading.
        (Err(before), Err(after))
            if before.message == after.message || value_error(after) || alias_error(after) =>
        {
            Some(Fairness::Kept)
        }
        (Err(_), Err(_)) => match (place(case, side), place(next, side)) {
            (Some(before), Some(after)) => {
                let line = case.input[..before]
                    .rfind(['\n', '\r'])
                    .map_or(0, |offset| offset + 1);
                Some(if moved(after) >= line {
                    Fairness::Kept
                } else {
                    Fairness::Revealed
                })
            }
            // An error with no place, as the oracle's "repetition limit
            // exceeded" or the rule a rule's error is in, gives nothing to
            // compare.
            _ => Some(Fairness::Kept),
        },
    }
}

/// Where byte `at` of `rewritten`, a rewrite of `input`, was in `input`. The
/// two are walked together; past each edit, where they differ, they meet
/// again at the nearest place where the next few characters agree. A byte
/// inside an edit was where the edit starts.
fn original_offset(input: &str, rewritten: &str, at: usize) -> usize {
    /// How many characters must agree for the texts to meet again; fewer
    /// at the end.
    const AGREE: usize = 3;
    let old: Vec<(usize, char)> = input.char_indices().collect();
    let new: Vec<(usize, char)> = rewritten.char_indices().collect();
    let offset = |chars: &[(usize, char)], index: usize, text: &str| {
        chars.get(index).map_or(text.len(), |(offset, _)| *offset)
    };
    let agree = |i: usize, j: usize| {
        let count = AGREE.min(old.len() - i).min(new.len() - j);
        (count > 0 || (i == old.len() && j == new.len()))
            && (0..count).all(|k| old[i + k].1 == new[j + k].1)
    };
    let (mut i, mut j) = (0, 0);
    loop {
        if i == old.len() && j == new.len() {
            return input.len();
        }
        if i < old.len() && j < new.len() && old[i].1 == new[j].1 {
            if new[j].0 >= at {
                return old[i].0;
            }
            i += 1;
            j += 1;
            continue;
        }
        let rest = (old.len() - i) + (new.len() - j);
        let (skip, gap) = (1..=rest)
            .flat_map(|distance| (0..=distance).map(move |skip| (skip, distance - skip)))
            .find(|&(skip, gap)| {
                i + skip <= old.len() && j + gap <= new.len() && agree(i + skip, j + gap)
            })
            .unwrap_or((old.len() - i, new.len() - j));
        if gap > 0 && offset(&new, j + gap, rewritten) > at {
            return offset(&old, i, input);
        }
        i += skip;
        j += gap;
    }
}

/// Whether ayaml's and the oracle's errors are the same value error: a
/// duplicate key, which the two word differently for a null key, or a
/// decode error of ayaml's (not a syntax error) whose message ends as the
/// oracle's does after the path to the value.
fn same_value_error(ayaml: &Rejection, oracle: &Rejection) -> bool {
    let problem = |message: &str| message.rsplit(": ").next().unwrap_or("").to_string();
    (ayaml.message.contains("duplicate entry") && oracle.message.contains("duplicate entry"))
        || (ayaml.kind == Some(ayaml::ErrorKind::Serde)
            && problem(&ayaml.message) == problem(&oracle.message))
}

/// How fair a counterfactual of `case` the rewritten case `next` is, or
/// `None` when it is not fair. `hides` names the ayaml errors the rewrite's
/// trigger is documented to hide.
fn fairness(case: &Case<'_>, next: &Case<'_>, hides: &Hidden) -> Option<Fairness> {
    let shown = case.ayaml.is_ok() && case.oracle.is_err() && hides.shown(case, next);
    let ayaml = match side_fairness(case, next, |case| &case.ayaml, |case| &case.oracle) {
        Some(Fairness::Revealed) if hides.revealed(case, next) => Some(Fairness::Kept),
        fairness => fairness,
    }
    .or(shown.then_some(Fairness::Kept));
    let oracle = side_fairness(case, next, |case| &case.oracle, |case| &case.ayaml);
    // The two errors are one: the same value error, or, outside a rule,
    // where a place is only the rule's position, the same place.
    let one = |ayaml: &Rejection, oracle: &Rejection| {
        same_value_error(ayaml, oracle)
            || (next.class == Class::RejectedAtSameLocation && case.entry != Entry::Rule)
    };
    let both_agree = match (&case.ayaml, &case.oracle, &next.ayaml, &next.oracle) {
        (Err(before), _, Err(ayaml), Err(oracle)) if before.message == ayaml.message => {
            one(ayaml, oracle)
        }
        (_, Err(before), Err(ayaml), Err(oracle)) if before.message == oracle.message => {
            one(ayaml, oracle)
        }
        _ => false,
    };
    match (ayaml, oracle) {
        // A value error is reported where its node or collection starts,
        // which can come before the error it takes the place of: the
        // parser that moved now gives the error the other kept, which hides
        // nothing.
        (Some(ayaml), Some(oracle)) if ayaml.max(oracle) == Fairness::Revealed && both_agree => {
            Some(Fairness::BothAgree)
        }
        (Some(ayaml), Some(oracle)) => Some(ayaml.max(oracle)),
        _ => both_agree.then_some(Fairness::BothAgree),
    }
}

/// ayaml's error about a second document at an entry point that reads one,
/// which it reports after the first document's value errors, as for
/// `a: !!int x` then `---` and `b`.
const AYAML_SECOND_DOCUMENT: &str = "more than one document in stream";

/// ayaml's messages about aliases, of kind Composition: ayaml 0.1.2 gives
/// no error of kind Alias, which names the same errors.
const AYAML_ALIAS_MESSAGES: &[&str] = &[
    "unknown anchor",
    "an alias cannot refer to a collection that contains it",
];

/// The oracle's messages about aliases, which it checks as it reads the
/// syntax and ayaml only when it builds values.
const ORACLE_ALIAS_MESSAGES: &[&str] = &[
    "unknown anchor",
    "recursion limit exceeded",
    "repetition limit exceeded",
];

/// For a case both parsers reject at different places, the class it has
/// when one parser's error is about the syntax and the other reads the
/// syntax: ayaml checks it before it builds any value, and the oracle
/// builds values as it reads, so either can report a value error where the
/// other reports a syntax error that the first may not share. A location
/// class would take the two for one error found at two places, and hide a
/// difference in the syntax. The oracle reads the syntax when it reads the
/// input into no value; its error is about the syntax when it gives the same
/// error so, and is not about an alias.
fn one_rejects_the_syntax(case: &Case<'_>) -> Option<Class> {
    let (Err(ayaml), Err(oracle)) = (&case.ayaml, &case.oracle) else {
        return None;
    };
    let syntax = yaml_serde::from_str::<serde::de::IgnoredAny>(case.input)
        .map(|_| ())
        .map_err(|error| oracle_rejection(&error));
    // ayaml 0.1.2 reports a malformed tag as a scanner error, and gives no
    // error of kind Tag.
    let ayaml_syntax = ayaml_syntax_error(ayaml);
    if ayaml_syntax {
        return syntax.is_ok().then_some(Class::OnlyAyamlRejects);
    }
    let about_syntax = syntax.as_ref().is_err_and(|error| error == oracle)
        && !ORACLE_ALIAS_MESSAGES
            .iter()
            .any(|message| oracle.message.contains(message));
    about_syntax.then_some(Class::OnlyOracleRejects)
}

/// What the prefix through the line of a case's first error shows (see
/// [`Search::split`]).
#[derive(Debug)]
enum Split<'a> {
    /// That the first error may stand for the case: for two errors, that
    /// they may be one, found at two places.
    One,
    /// That the case holds two: the prefix's case holds the first error
    /// alone, and the rest is a case of its own: the rest of the input,
    /// which the parser that gave the first error never read, or, where the
    /// oracle's error is in a flow collection ayaml read, the input with
    /// that collection empty, in which ayaml gives its error as it is (see
    /// [`Search::split_at_a_flow_collection`]).
    Two(Case<'a>, String),
    /// That the case holds two, and the rest of the input is no case of its
    /// own: the oracle read the first error's line, ayaml's, in its first
    /// document before it found a second, which has no place, or ayaml read
    /// the line the oracle rejected.
    Apart,
    /// That the case holds two, and the prefix's case, through the line of
    /// ayaml's error, which the oracle reads, holds that error as it is:
    /// ayaml rejects the input there, before the oracle's error, so that an
    /// explanation of the prefix's case is one of ayaml's result, and the
    /// text after it, which ayaml never read, can neither show nor hide one
    /// of ayaml's differences.
    Through(Case<'a>),
    /// Nothing, since a parser panicked or the search ran out of comparisons:
    /// no location class may explain the case.
    Unknown,
}

/// Whether the oracle's error is its "more than one document".
fn oracle_more_than_one_document(case: &Case<'_>) -> bool {
    predicates::rejects(&case.oracle, "more than one document")
}

/// The byte offset of the oracle's error `oracle` in `input`: where it
/// reports it, or, for a character its reader does not allow, which it
/// reports at line 1, column 1, the first such character.
pub(super) fn oracle_error_offset(input: &str, oracle: &Side) -> Option<usize> {
    if predicates::rejects(oracle, predicates::ORACLE_READER_ERROR) {
        let allowed = |ch: char| {
            matches!(ch, '\t' | '\n' | '\r' | ' '..='~' | '\u{85}' | '\u{a0}'..='\u{d7ff}')
                || matches!(ch, '\u{e000}'..='\u{fffd}' | '\u{10000}'..)
        };
        return input.find(|ch| !allowed(ch));
    }
    predicates::error_offset(input, oracle)
}

/// Where the first error of `case` is, as a byte offset in its input, and
/// whether it is ayaml's: ayaml's place stands for an oracle error that has
/// none, and the first character the oracle's reader does not allow for its
/// error about one (see [`oracle_error_offset`]). None at a typed decode,
/// where a prefix or the rest of a configuration lacks the fields before or
/// after it, and a rule's error is placed at the rule's position, and for
/// an error without a place.
fn first_error(case: &Case<'_>) -> Option<(usize, bool)> {
    if matches!(case.entry, Entry::Config | Entry::Rule) {
        return None;
    }
    let ayaml = predicates::error_offset(case.input, &case.ayaml);
    let oracle = oracle_error_offset(case.input, &case.oracle);
    match (ayaml, oracle) {
        (Some(ayaml), Some(oracle)) if oracle < ayaml => Some((oracle, false)),
        (Some(ayaml), _) => Some((ayaml, true)),
        (None, Some(oracle)) if case.ayaml.is_ok() => Some((oracle, false)),
        _ => None,
    }
}

/// The byte offset where the line that holds byte `at` of `input` starts.
fn line_start(input: &str, at: usize) -> usize {
    input[..at]
        .rfind(['\n', '\r'])
        .map_or(0, |offset| offset + 1)
}

/// The byte offset where the line before the one that starts at byte
/// `start`, past 0, of `input` starts.
fn previous_line_start(input: &str, start: usize) -> usize {
    let at = start
        - if input[..start].ends_with("\r\n") {
            2
        } else {
            1
        };
    line_start(input, at)
}

/// Whether the line that starts at byte `at` of `input` starts at the top,
/// outside a construct a line before it opened and it continues: a flow
/// collection, a quoted scalar, a block scalar, a plain scalar or a comment,
/// as [`neutralize::places`] reads them into `places`, or a mapping entry
/// whose `:` starts the line, as `? a` then `: b`. A prefix cut at such a
/// line would cut the construct, and the rest would read its end out of
/// place. A document marker ends any of them.
fn at_the_top(input: &str, places: &[neutralize::Place], at: usize) -> bool {
    let after = |text: &str, marker: &str| {
        text.strip_prefix(marker)
            .is_some_and(|rest| rest.chars().next().is_none_or(char::is_whitespace))
    };
    if ["---", "..."]
        .iter()
        .any(|marker| after(&input[at..], marker))
    {
        return true;
    }
    !after(input[at..].trim_start_matches(' '), ":")
        && places
            .get(at)
            .is_none_or(|place| place.depth == 0 && !place.literal && !place.plain)
}

/// Whether the lines of `input` from byte `start`, the start of a line at
/// the top (see [`at_the_top`]), read as they would after the lines before
/// them: those lines hold no node, only spaces, tabs and comments, or no
/// line at the top is indented less than the first, which a line before
/// `start` would have held. Blank lines and comments do not count.
fn stands_alone(input: &str, places: &[neutralize::Place], start: usize) -> bool {
    let blank = |line: &str| {
        let text = line
            .trim_end_matches(['\n', '\r'])
            .trim_matches([' ', '\t']);
        text.is_empty() || text.starts_with('#')
    };
    if input[..start].split_inclusive(['\n', '\r']).all(blank) {
        return true;
    }
    let mut first = None;
    let mut at = start;
    while at < input.len() {
        let end = line_end(input, at);
        let line = &input[at..end];
        let text = line.trim_start_matches(' ');
        let content = !text.trim().is_empty() && !text.starts_with('#');
        if content && at_the_top(input, places, at) {
            let indent = line.len() - text.len();
            if indent < *first.get_or_insert(indent) {
                return false;
            }
        }
        at = end;
    }
    true
}

/// The byte offset where the line that holds byte `at` of `input` ends,
/// past its line break: a line feed, a carriage return, or both.
fn line_end(input: &str, at: usize) -> usize {
    match input[at..].find(['\n', '\r']) {
        Some(offset) => {
            let end = at + offset;
            end + if input[end..].starts_with("\r\n") {
                2
            } else {
                1
            }
        }
        None => input.len(),
    }
}

/// `input` without the text of its first `end` bytes, whose line breaks
/// stay, so that the rest keeps its lines.
fn after_prefix(input: &str, end: usize) -> String {
    input[..end]
        .chars()
        .filter(|ch| matches!(ch, '\n' | '\r'))
        .chain(input[end..].chars())
        .collect()
}

impl Search {
    /// Whether the split of `case`, which both parsers reject at different
    /// places, shows its two errors to be two (see [`Search::split`]), or, as
    /// `Err`, whether the split settles the case: the prefix and the rest
    /// explain it each on its own, or the split cannot tell.
    fn errors_apart(&mut self, case: &Case<'_>) -> Result<bool, bool> {
        match self.split(case) {
            Split::Unknown => Err(false),
            Split::One => Ok(false),
            Split::Apart => Ok(true),
            Split::Two(prefix, rest) => {
                if self.explain_both(case, &prefix, &rest) {
                    Err(true)
                } else {
                    Ok(true)
                }
            }
            Split::Through(prefix) => {
                let mut branch = self.clone();
                if branch.explain(&prefix) {
                    *self = branch;
                    return Err(true);
                }
                self.comparisons = branch.comparisons;
                Ok(true)
            }
        }
    }

    fn explain(&mut self, case: &Case<'_>) -> bool {
        if self.complete {
            if case.class == Class::SameValue {
                return true;
            }
        } else if !case.class.is_difference() {
            return true;
        }
        if !self.complete
            && case.class == Class::RejectedAtDifferentLocation
            && case.entry != Entry::Rule
            && let Err(Rejection {
                location: Some(_),
                kind: Some(kind),
                message,
            }) = &case.ayaml
        {
            let as_class = |class| Case {
                input: case.input,
                entry: case.entry,
                rule: case.rule,
                class,
                ayaml: case.ayaml.clone(),
                oracle: case.oracle.clone(),
            };
            if let Some(class) = one_rejects_the_syntax(case) {
                return self.explain(&as_class(class));
            }
            // A location class takes the two errors for one. The prefix
            // through the earlier error's line, which the parser with the
            // later error reads and the other rejects, shows them to be two
            // (see [`Search::split`]): the case is then explained by the
            // prefix and the rest of the input, each on its own, or by the
            // rewrites, or not at all. So does removing a trigger, of an
            // entry that covers either error as if the other parser had read
            // the case, after which that parser reads the input while the
            // other keeps its error. A location class says nothing of the
            // lines before the earlier error (see [`Search::explain_before`]).
            let mut separated = match self.errors_apart(case) {
                Ok(separated) => separated,
                Err(explained) => return explained,
            };
            for deviation in allowlist::DEVIATIONS {
                let Trigger::Input { neutralize, .. } = deviation.trigger else {
                    continue;
                };
                let covers = [
                    Class::RejectedAtDifferentLocation,
                    Class::OnlyAyamlRejects,
                    Class::OnlyOracleRejects,
                ]
                .into_iter()
                .any(|class| deviation.covers(&as_class(class)));
                if self.tried.contains(&deviation.id) || !covers {
                    continue;
                }
                let rewrites = neutralize(case.input);
                if self.explain_rewrites(case, deviation, &rewrites, &mut separated) {
                    return true;
                }
            }
            // Failing that, a narrow entry that covers the case as it is, with
            // both rejecting, may explain it, as below.
            if let Some(location) = allowlist::locate_case(case, *kind, message)
                && !separated
                && self.explain_before(case)
            {
                self.ids.push(location.id);
                return true;
            }
        }
        for deviation in allowlist::DEVIATIONS {
            if self.tried.contains(&deviation.id) || !deviation.covers(case) {
                continue;
            }
            match deviation.trigger {
                // A narrow entry explains the case's error, not the text
                // after a revealed one.
                Trigger::Narrow { .. } if self.complete => {}
                // Nor the lines before the error, or after its line, where
                // the case holds two (see [`Search::split`]); a rewrite of
                // another entry may yet explain the case.
                Trigger::Narrow { .. } => match self.split(case) {
                    Split::Unknown => return false,
                    Split::Two(prefix, rest) => {
                        if self.explain_both(case, &prefix, &rest) {
                            return true;
                        }
                    }
                    Split::One | Split::Apart | Split::Through(_) => {
                        let mut branch = self.clone();
                        if branch.explain_before(case) {
                            branch.ids.push(deviation.id);
                            *self = branch;
                            return true;
                        }
                        self.comparisons = branch.comparisons;
                    }
                },
                Trigger::Input { neutralize, .. } => {
                    let rewrites = neutralize(case.input);
                    if self.explain_rewrites(case, deviation, &rewrites, &mut false) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Whether the prefix of `case`, through the line of its first error
    /// (see [`first_error`]) and the construct the error is in, holds that
    /// error alone: the parser that gave it rejects the prefix, and the
    /// other reads it. For a case both parsers reject, the other error is
    /// then another; for a case one rejects, the rest of the input is one
    /// that parser never read. A prefix that both parsers reject, as one
    /// that ends in a quoted scalar or a flow collection, shows nothing.
    /// Neither does one that both read, as when the first error is about
    /// what follows its line, but the oracle's "more than one document",
    /// which has no place, is then not ayaml's error found elsewhere: the
    /// oracle read ayaml's error's line in its first document. Nor a prefix
    /// whose rest reads otherwise without it (see [`stands_alone`]). Where
    /// that prefix shows nothing, the prefix through the error's line alone,
    /// whatever construct goes on past it, may (see
    /// [`Search::split_after_line`]).
    fn split<'a>(&mut self, case: &Case<'a>) -> Split<'a> {
        let Some((earlier, by_ayaml)) = first_error(case) else {
            return Split::One;
        };
        let places = neutralize::places(case.input);
        let line = line_end(case.input, earlier);
        let mut end = line;
        while end < case.input.len() && !at_the_top(case.input, &places, end) {
            end = line_end(case.input, end);
        }
        let compared = end < case.input.len() && stands_alone(case.input, &places, end);
        let split = if compared {
            self.split_after_construct(case, end, by_ayaml)
        } else {
            Split::One
        };
        match split {
            Split::One if line < case.input.len() && !(compared && end == line) => {
                self.split_after_line(case, line, by_ayaml, &places)
            }
            split => split,
        }
    }

    /// [`Search::split`] at `end`, past the construct that the case's first
    /// error, ayaml's if `by_ayaml`, is in.
    fn split_after_construct<'a>(
        &mut self,
        case: &Case<'a>,
        end: usize,
        by_ayaml: bool,
    ) -> Split<'a> {
        let oracle = oracle_error_offset(case.input, &case.oracle);
        let later_error = if by_ayaml {
            oracle
        } else {
            predicates::error_offset(case.input, &case.ayaml)
        };
        if self.comparisons == MAX_COMPARISONS {
            return Split::Unknown;
        }
        self.comparisons += 1;
        let (prefix, panicked) = compare_at(&case.input[..end], case.entry, case.rule);
        let (earlier, later) = if by_ayaml {
            (&prefix.ayaml, &prefix.oracle)
        } else {
            (&prefix.oracle, &prefix.ayaml)
        };
        // Both parsers reject the prefix's syntax at one place, and the
        // case's other error is past it: ayaml reads the whole stream's
        // tokens before it parses, so it reports a scanner error there ahead
        // of the error the two share. A value error the two share is one
        // ayaml reports after any syntax error, as it should.
        let shared = prefix.class == Class::RejectedAtSameLocation
            && matches!(&prefix.ayaml, Err(rejection) if ayaml_syntax_error(rejection))
            && later_error.is_some_and(|later| later >= end);
        if panicked {
            Split::Unknown
        } else if (earlier.is_err() && later.is_ok()) || shared {
            Split::Two(prefix, after_prefix(case.input, end))
        } else if oracle.is_none() && oracle_more_than_one_document(case) && prefix.oracle.is_ok() {
            Split::Apart
        } else {
            Split::One
        }
    }

    /// [`Search::split`] at `end`, the end of the line of the case's first
    /// error, ayaml's if `by_ayaml`, where the prefix through the construct
    /// it is in showed nothing. The rest of the input is no case of its own
    /// past a construct cut in two, so the prefix shows two errors apart, or
    /// that ayaml's error is all of ayaml's result (see [`Split::Through`]),
    /// or nothing, when the parser with the later error rejects it too, as
    /// when the cut leaves a construct open. `places` are the input's (see
    /// [`neutralize::places`]).
    fn split_after_line<'a>(
        &mut self,
        case: &Case<'a>,
        end: usize,
        by_ayaml: bool,
        places: &[neutralize::Place],
    ) -> Split<'a> {
        if self.comparisons == MAX_COMPARISONS {
            return Split::Unknown;
        }
        self.comparisons += 1;
        let (prefix, panicked) = compare_at(&case.input[..end], case.entry, case.rule);
        let holds = |part: &Side, whole: &Side| part.is_err() && part == whole;
        // The oracle's "more than one document" has no place: ayaml's error
        // is then the first.
        let multiple_documents = oracle_more_than_one_document(case);
        if panicked {
            Split::Unknown
        } else if by_ayaml && prefix.oracle.is_ok() && holds(&prefix.ayaml, &case.ayaml) {
            Split::Through(prefix)
        } else if !by_ayaml && prefix.ayaml.is_ok() && holds(&prefix.oracle, &case.oracle) {
            self.split_at_a_flow_collection(case, prefix, places)
                .unwrap_or(Split::Apart)
        } else if multiple_documents && prefix.oracle.is_ok() {
            Split::Apart
        } else {
            Split::One
        }
    }

    /// Where `prefix`, the lines of `case` through its first error, the
    /// oracle's, holds that error alone, and the error is in a flow
    /// collection that ends on the prefix's last line, the split of `case`
    /// into the prefix and the input with the collection empty (`[]` or
    /// `{}`, with the line breaks it held), in which ayaml gives its error as
    /// it is. The oracle must read the prefix with the collection empty: its
    /// error was the collection's alone, as an empty one both parsers read
    /// alike, and it then reads the lines around it as ayaml read them in the
    /// case. None where that does not hold, as when the search ran out of
    /// comparisons. `places` are the input's.
    fn split_at_a_flow_collection<'a>(
        &mut self,
        case: &Case<'a>,
        prefix: Case<'a>,
        places: &[neutralize::Place],
    ) -> Option<Split<'a>> {
        let input = case.input;
        let end = prefix.input.len();
        let earlier = oracle_error_offset(input, &case.oracle)?;
        if places[earlier].depth == 0 {
            return None;
        }
        // `places` gives a flow collection's brackets the depth around it,
        // and the bytes between them more, so these are its brackets.
        let open = (0..earlier).rev().find(|&at| places[at].depth == 0)?;
        let close = (earlier..input.len()).find(|&at| places[at + 1].depth == 0)?;
        if line_end(input, close) != end {
            return None;
        }
        let breaks: String = input[open..close]
            .chars()
            .filter(|ch| matches!(ch, '\n' | '\r'))
            .collect();
        let emptied = format!(
            "{}{}{breaks}{}",
            &input[..=open],
            &input[close..=close],
            &input[close + 1..end]
        );
        if self.comparisons == MAX_COMPARISONS {
            return None;
        }
        self.comparisons += 1;
        let (alone, panicked) = compare_at(&emptied, case.entry, case.rule);
        if panicked || alone.oracle.is_err() || self.comparisons == MAX_COMPARISONS {
            return None;
        }
        self.comparisons += 1;
        let emptied = emptied + &input[end..];
        let (rest, panicked) = compare_at(&emptied, case.entry, case.rule);
        (!panicked && rest.ayaml == case.ayaml).then_some(Split::Two(prefix, emptied))
    }

    /// Whether the lines of `case` before the line of its first error (see
    /// [`first_error`]), or before the construct the error is in, which both
    /// parsers read before either rejected the input, are explained: an
    /// entry that explains the error says nothing of them. Lines that both
    /// parsers read alike need nothing, and lines that a parser rejects
    /// without the rest show nothing.
    fn explain_before(&mut self, case: &Case<'_>) -> bool {
        let Some((first, _)) = first_error(case) else {
            return true;
        };
        let places = neutralize::places(case.input);
        let mut start = line_start(case.input, first);
        while start > 0 && !at_the_top(case.input, &places, start) {
            start = previous_line_start(case.input, start);
        }
        let before = &case.input[..start];
        if before.trim().is_empty() {
            return true;
        }
        if self.comparisons == MAX_COMPARISONS {
            return false;
        }
        self.comparisons += 1;
        let (before, panicked) = compare_at(before, case.entry, case.rule);
        !panicked && (before.ayaml.is_err() || before.oracle.is_err() || self.explain(&before))
    }

    /// Whether `prefix`, which holds the first error of `case` alone (see
    /// [`Search::split`]), and `rest`, the rest of the input after it or
    /// the input without it, are each explained, the rest with the entries
    /// the case may use, whichever explained the prefix.
    fn explain_both(&mut self, case: &Case<'_>, prefix: &Case<'_>, rest: &str) -> bool {
        let mut branch = self.clone();
        if !branch.explain(prefix) {
            self.comparisons = branch.comparisons;
            return false;
        }
        let mut search = Search {
            ids: branch.ids,
            comparisons: branch.comparisons,
            ..self.clone()
        };
        if search.explain_rest(case, rest) {
            *self = search;
            return true;
        }
        self.comparisons = search.comparisons;
        false
    }

    /// Whether `rest`, the rest of `case`'s input after a prefix that holds
    /// one of its errors, or its input without that error (see
    /// [`Split::Two`]), is explained.
    fn explain_rest(&mut self, case: &Case<'_>, rest: &str) -> bool {
        if self.comparisons == MAX_COMPARISONS {
            return false;
        }
        self.comparisons += 1;
        let (rest, panicked) = compare_at(rest, case.entry, case.rule);
        !panicked && self.explain(&rest)
    }

    /// Whether one of `rewrites` of `case`, which remove the trigger of
    /// `deviation`, explains it. `separated` is set when one of them, fair
    /// to the case, leaves one parser's error as it was, wherever the edits
    /// moved it, and the other parser reading the input.
    fn explain_rewrites(
        &mut self,
        case: &Case<'_>,
        deviation: &allowlist::Deviation,
        rewrites: &[String],
        separated: &mut bool,
    ) -> bool {
        let id = deviation.id;
        let mut compared = Vec::new();
        for input in rewrites {
            if self.comparisons == MAX_COMPARISONS {
                break;
            }
            self.comparisons += 1;
            let (next, panicked) = compare_at(input, case.entry, case.rule);
            if !panicked {
                let kept = |before: &Side, after: &Side| matches!((before, after), (Err(before), Err(after)) if before.message == after.message);
                *separated |= fairness(case, &next, &deviation.hides()) == Some(Fairness::Kept)
                    && ((kept(&case.ayaml, &next.ayaml) && next.oracle.is_ok())
                        || (kept(&case.oracle, &next.oracle) && next.ayaml.is_ok()));
                compared.push(next);
            }
        }
        for tier in [Fairness::Kept, Fairness::Revealed, Fairness::BothAgree] {
            for next in &compared {
                let both_agree = tier == Fairness::BothAgree;
                if fairness(case, next, &deviation.hides()) != Some(tier)
                    || (both_agree && self.complete)
                {
                    continue;
                }
                let mut branch = self.clone();
                branch.tried.push(id);
                branch.complete |= tier == Fairness::Revealed;
                // The oracle's error about a character it does not allow,
                // the same at line 1, column 1, is about the first: the
                // rewrite changed it if it removed that one.
                let about = |case: &Case<'_>| oracle_error_offset(case.input, &case.oracle);
                if next.ayaml != case.ayaml
                    || next.oracle != case.oracle
                    || about(next).map(|at| original_offset(case.input, next.input, at))
                        != about(case)
                {
                    branch.ids.push(id);
                }
                if both_agree || branch.explain(next) {
                    *self = branch;
                    return true;
                }
                self.comparisons = branch.comparisons;
            }
        }
        false
    }
}

/// The counts of a run, and the cases it fails on.
#[derive(Default)]
#[cfg(test)]
struct Report {
    inputs: usize,
    classes: BTreeMap<(Entry, Class), usize>,
    /// How often each deviation or location class helped explain a
    /// difference that saphyr's vote did not.
    explained: BTreeMap<(Entry, &'static str), usize>,
    /// The differences saphyr's vote explained (see [`saphyr_agrees`]).
    voted: BTreeMap<(Entry, Class), usize>,
    /// How often each deviation or location class would also have explained
    /// a difference the vote explained: an entry with no other count is one
    /// the vote makes redundant.
    also_voted: BTreeMap<&'static str, usize>,
    /// The differences only the vote explained, which no entry would: where
    /// a defect ayaml shares with saphyr would pass, so the report lists the
    /// shortest few.
    vote_only: usize,
    vote_only_inputs: Vec<String>,
    /// The shortest input each deviation or location class helped explain,
    /// and where.
    examples: BTreeMap<&'static str, (Entry, String)>,
    /// Inputs a typed decode did not compare: because the value entry point
    /// did not read them alike, or, for rules, because there was no rule.
    /// Each rule of an input is a case of its own, so the rule decode's
    /// counts are of rules, not inputs.
    skipped: BTreeMap<Entry, usize>,
    /// Unexplained differences and panics, the shortest few of each kind.
    failures: BTreeMap<(Entry, Class, bool), Vec<String>>,
    failure_count: usize,
    /// The generated configurations, unmutated, and their cases by entry
    /// point and class, also counted in `classes`: the floors on the typed
    /// decodes read these, which the corpus's configurations cannot meet for
    /// them.
    configurations: usize,
    configuration_classes: BTreeMap<(Entry, Class), usize>,
}

#[cfg(test)]
const KEPT_FAILURES: usize = 8;

/// How the report names saphyr's vote.
#[cfg(test)]
const SAPHYR_VOTE: &str = "saphyr's vote";

#[cfg(test)]
impl Report {
    fn add(&mut self, input: &str) {
        self.add_input(input, false);
    }

    /// [`Report::add`] for a generated configuration, whose cases are also
    /// counted on their own.
    fn add_configuration(&mut self, input: &str) {
        self.configurations += 1;
        self.add_input(input, true);
    }

    fn add_input(&mut self, input: &str, configuration: bool) {
        self.inputs += 1;
        let mut value_alike = false;
        for entry in Entry::ALL {
            // A typed decode is compared only for a document both parsers
            // read alike, so its result is about the decode, not the syntax.
            let typed = matches!(entry, Entry::Config | Entry::Rule);
            let cases = match entry {
                _ if typed && !value_alike => Vec::new(),
                Entry::Rule => rule_cases(input),
                _ => vec![compare(input, entry)],
            };
            if cases.is_empty() {
                *self.skipped.entry(entry).or_default() += 1;
            }
            for (case, panicked) in &cases {
                if entry == Entry::Value {
                    value_alike = case.class == Class::SameValue;
                }
                self.record(case, *panicked);
                if configuration {
                    *self
                        .configuration_classes
                        .entry((entry, case.class))
                        .or_default() += 1;
                }
            }
        }
    }

    fn record(&mut self, case: &Case<'_>, panicked: bool) {
        *self.classes.entry((case.entry, case.class)).or_default() += 1;
        let voted = case.class.is_difference() && saphyr_agrees(case);
        let explained = explanation(case);
        if voted {
            *self.voted.entry((case.entry, case.class)).or_default() += 1;
            self.example(SAPHYR_VOTE, case);
            if explained.is_none() {
                self.vote_only += 1;
                self.vote_only_inputs.push(format!(
                    "{:?}\n      ayaml:  {}\n      oracle: {}",
                    case.input,
                    describe(&case.ayaml),
                    describe(&case.oracle)
                ));
                self.vote_only_inputs.sort_by_key(String::len);
                self.vote_only_inputs.dedup();
                self.vote_only_inputs.truncate(KEPT_FAILURES);
            }
        }
        for id in explained.iter().flatten() {
            if voted {
                *self.also_voted.entry(id).or_default() += 1;
            } else {
                *self.explained.entry((case.entry, id)).or_default() += 1;
                self.example(id, case);
            }
        }
        if panicked || (explained.is_none() && !voted) {
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

    /// Keep `case`'s input as `id`'s example if it is the shortest yet.
    fn example(&mut self, id: &'static str, case: &Case<'_>) {
        let example = self
            .examples
            .entry(id)
            .or_insert_with(|| (case.entry, case.input.to_string()));
        if case.input.chars().count() < example.1.chars().count() {
            *example = (case.entry, case.input.to_string());
        }
    }

    fn count(&self, entry: Entry, class: Class) -> usize {
        self.classes.get(&(entry, class)).copied().unwrap_or(0)
    }

    fn markdown(&self, title: &str) -> String {
        let skipped = |entry| self.skipped.get(&entry).copied().unwrap_or(0);
        let mut out = format!(
            "### {title}\n\n{} inputs. The typed decode into `Config` skipped {} that the value \
             entry point did not read alike, and the rule decode {} that it did not read alike \
             or that have no rules. The Rules column counts rules, each a case of its own.\n\n",
            self.inputs,
            skipped(Entry::Config),
            skipped(Entry::Rule)
        );
        out.push_str("| Class | Value | JSON | Config | Rules |\n|---|---:|---:|---:|---:|\n");
        for class in Class::ALL {
            let _ = write!(out, "| {class:?} |");
            for entry in Entry::ALL {
                let _ = write!(out, " {} |", self.count(entry, class));
            }
            out.push('\n');
        }
        out.push_str(
            "\nsaphyr's vote explains a difference at the value entry point when saphyr reads \
             the input as ayaml does. An entry's \"Also voted\" counts differences it explains \
             that the vote explained first; an entry with no other count is one the vote makes \
             redundant.\n\n| Explained by | Value | JSON | Config | Rules | Also voted |\n\
             |---|---:|---:|---:|---:|---:|\n",
        );
        let voted = |entry| {
            Class::ALL
                .iter()
                .map(|class| self.voted.get(&(entry, *class)).copied().unwrap_or(0))
                .sum::<usize>()
        };
        let _ = write!(out, "| {SAPHYR_VOTE} |");
        for entry in Entry::ALL {
            let _ = write!(out, " {} |", voted(entry));
        }
        out.push_str(" |\n");
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
            let also = self.also_voted.get(id).copied().unwrap_or(0);
            let _ = writeln!(out, " {also} |");
        }
        let _ = writeln!(
            out,
            "\n{} differences only the vote explains, which no entry does.",
            self.vote_only
        );
        for input in &self.vote_only_inputs {
            let _ = writeln!(out, "  - {input}");
        }
        out.push_str("\nThe shortest input each one explained:\n\n");
        for (id, (entry, input)) in &self.examples {
            let _ = writeln!(out, "- `{id}`, {entry:?}: `` {} ``", shortened(input));
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

/// `input` written with `{:?}`, cut to 160 characters.
pub(crate) fn shortened(input: &str) -> String {
    let mut text = format!("{input:?}");
    if let Some((cut, _)) = text.char_indices().nth(160) {
        text.truncate(cut);
        text.push('…');
    }
    text
}

/// The allowlist as Markdown, for the long run's report.
#[cfg(test)]
fn allowlist_markdown() -> String {
    let mut out = String::from("\n### Documented differences\n");
    for deviation in allowlist::DEVIATIONS {
        let how = match deviation.trigger {
            Trigger::Narrow { because, .. } => format!("Narrow: {because}"),
            Trigger::Input { .. } => "Explains a case only if removing its trigger from the \
                                      input removes the difference"
                .to_string(),
        };
        let _ = write!(
            out,
            "\n- `{}` ({:?}): ayaml {}; the oracle {}. Reason: {}. {how}. Example: `{}`.\n",
            deviation.id,
            deviation.classes,
            deviation.ayaml,
            deviation.oracle,
            deviation.reason,
            shortened(deviation.example)
        );
    }
    out.push_str("\n### Documented error locations\n");
    for location in allowlist::LOCATIONS {
        let _ = write!(
            out,
            "\n- `{}`: ayaml reports {}; the oracle reports {}. Example: `{:?}` ({:?}).\n",
            location.id, location.ayaml, location.oracle, location.example.0, location.example.1
        );
    }
    out
}

#[cfg(test)]
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
#[cfg(test)]
fn repository() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The committed YAML files of the repository, by path, with their line
/// breaks as committed whatever Git's `core.autocrlf`; none outside a Git
/// checkout.
#[cfg(test)]
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
#[cfg(test)]
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The generated input of `index`: of each eight, two are token soup, two
/// documents, two mutated documents, one a configuration and one a mutated
/// configuration. When `specials` is set, every other input may hold
/// [`generate::SPECIALS`].
#[cfg(test)]
fn generated(rng: &mut Rng, index: usize, specials: bool) -> String {
    let specials = specials && index % 2 == 1;
    match index % 8 {
        0 | 1 => generate::token_soup(rng, specials),
        2 | 3 => generate::document(rng, specials),
        4 | 5 => {
            let document = generate::document(rng, specials);
            generate::mutate(rng, &document, specials)
        }
        6 => generate::configuration(rng),
        _ => {
            let configuration = generate::configuration(rng);
            generate::mutate(rng, &configuration, specials)
        }
    }
}

/// A run over `inputs` generated inputs from `seed`, the corpus, and
/// `mutations` mutations of each corpus file. When `specials` is set, every
/// other generated input and corpus mutation may hold [`generate::SPECIALS`]
/// (see `docs/design/yaml-parser.md` for how many do); none does when it is
/// not.
#[cfg(test)]
fn differential_run(seed: u64, inputs: usize, mutations: usize, specials: bool) -> Report {
    let mut rng = Rng::new(seed);
    let mut report = Report::default();
    for index in 0..inputs {
        let input = generated(&mut rng, index, specials);
        if index % 8 == 6 {
            report.add_configuration(&input);
        } else {
            report.add(&input);
        }
    }
    for_each_corpus_input(seed, mutations, specials, |input, _| report.add(input));
    report
}

/// Each corpus file, then `mutations` mutations of it from a seed of its
/// own, passed to `add` with whether it is a mutation. When `specials` is
/// set, every other mutation may hold [`generate::SPECIALS`].
#[cfg(test)]
fn for_each_corpus_input(
    seed: u64,
    mutations: usize,
    specials: bool,
    mut add: impl FnMut(&str, bool),
) {
    for (path, text) in corpus() {
        add(&text, false);
        let mut rng = Rng::new(seed ^ fnv1a(&path));
        for mutation in 0..mutations {
            let input = generate::mutate(&mut rng, &text, specials && mutation % 2 == 1);
            add(&input, true);
        }
    }
}

/// The number in the environment variable `name`, with `_` allowed between
/// digits, or `default` when it is not set.
///
/// # Panics
///
/// When it is set to something else, so that a mistyped size or seed fails
/// the run instead of running the default.
#[cfg(test)]
fn env_number(name: &str, default: u64) -> u64 {
    number_setting(name, std::env::var(name).ok().as_deref(), default)
}

/// The number `value` of the setting `name`, or `default` without one.
#[cfg(test)]
fn number_setting(name: &str, value: Option<&str>, default: u64) -> u64 {
    value.map_or(default, |value| {
        value
            .replace('_', "")
            .parse()
            .unwrap_or_else(|_| panic!("{name}={value:?} is not a number"))
    })
}

#[cfg(test)]
#[path = "yaml_differential_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "yaml_differential_findings.rs"]
mod findings;

#[cfg(test)]
#[path = "yaml_differential_planted.rs"]
mod planted;

/// Outside the tests, which plant defects in ayaml's results to check that
/// the harness catches them, ayaml's results as they are.
#[cfg(not(test))]
mod planted {
    pub(super) fn apply(_: &str, _: super::Entry, side: super::Side) -> super::Side {
        side
    }
}
