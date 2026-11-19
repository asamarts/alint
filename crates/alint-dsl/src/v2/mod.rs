//! `version: 2` of the configuration language: experimental in v0.23
//! (ADR-0034 decision 1, as ADR-0037 decision 3 corrects it).
//!
//! The loader reads a source's top-level `version:` before any typed decode
//! and dispatches on it ([`dispatch`]). A v2 source accepts every v1 key with
//! its v1 meaning, plus `selectors:`, `relations:` and an `extends:` entry's
//! `as:`; in a v1 source each of those is an error that names the version it
//! needs. v2 sources are local files: the top-level config, local `extends:`
//! targets and `.alint.d` fragments. Bundled and remote sources, nested
//! configs and `alint_dsl::parse` input must be `version: 1`, and a v1 source
//! never extends a v2 one (ADR-0022's direction rule).
//!
//! The decoded selectors and relations ride on the loader's merged config
//! with the source that declared each ([`Objects`]). Once every source has
//! loaded, finalize resolves every reference in its source's namespace
//! ([`resolve`]), checks the plan ([`plan_check`]) and carries the result on
//! the loaded `Config` (`relation_plan`). See
//! `docs/design/v0.23/selectors-and-constraints.md`, sections 3 to 5, 8 and
//! 10.

mod decode;
mod document;
mod environment;
mod literal;
mod model;
mod origins;
mod plan_check;
mod resolve;
#[cfg(test)]
pub(crate) mod schema_parity;

use alint_core::{ConfigSourceClass, Error, Result};
use ayaml::Value;

#[cfg(test)]
pub(crate) use decode::source_objects;
pub(crate) use decode::{
    Locus, Position, decode_objects, decode_twins, hoist_inline_inputs, take_inline_input,
};
pub(crate) use document::{Document, ExtendsEntry};
pub(crate) use environment::Environment;
pub(crate) use literal::reject_interpolated_names;
pub(crate) use model::{Objects, SourceId};
pub(crate) use origins::{
    FIELD_ORIGINS_MARKER, mark_field_origins, merge_field_origins, reference_fields,
    take_field_origins,
};
pub(crate) use resolve::{Resolved, resolve};

#[cfg(test)]
pub(crate) use model::{Read, Reader, Rows};
#[cfg(test)]
pub(crate) use origins::REFERENCE_STUB_KIND;

/// The experimental configuration version.
pub(crate) const VERSION: u32 = 2;

/// Which configuration language a source decodes as.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Surface {
    #[default]
    V1,
    V2,
}

/// What the loader knows about a source before it reads it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Origin<'a> {
    pub(crate) class: ConfigSourceClass,
    /// How version errors name the source, as the compatibility checks do.
    pub(crate) display: &'a str,
    /// The version of the config this source is read for: the root's for a
    /// drop-in or a nested config, the extending config's for an `extends:`
    /// target.
    pub(crate) consumer_version: Option<u32>,
    /// How that config is named.
    pub(crate) consumer_display: Option<&'a str>,
}

impl<'a> Origin<'a> {
    pub(crate) const fn new(class: ConfigSourceClass, display: &'a str) -> Self {
        Self {
            class,
            display,
            consumer_version: None,
            consumer_display: None,
        }
    }

    #[must_use]
    pub(crate) const fn consumer(mut self, version: Option<u32>, display: Option<&'a str>) -> Self {
        self.consumer_version = version;
        self.consumer_display = display;
        self
    }
}

/// How a source writes its top-level `version:`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Declared {
    /// The plain integer `2`, written as itself.
    Two,
    /// 2 written any other way: quoted, tagged, through an alias, or as `+2`,
    /// `0x2`, `0o2` or `2.0`.
    OtherTwo,
    /// An integer other than 2, as written.
    Integer(String),
    /// Read from the environment (`{{env.V}}`): version 1, whatever it reads.
    Environment,
    /// Anything else: null, a string, a collection, or a number that is not
    /// an integer.
    NonLiteral,
    /// No `version:` key.
    Absent,
    /// Not a YAML mapping, or not YAML: the v1 decode reports it.
    Unreadable,
}

fn declared_version(document: Option<&Value>, contents: &str) -> Declared {
    let Some(map) = document.and_then(Value::as_mapping) else {
        return Declared::Unreadable;
    };
    let Some(version) = map.get("version") else {
        return Declared::Absent;
    };
    if !denotes_two(version) {
        return match version {
            Value::Number(number) if version.is_u64() || version.is_i64() => {
                Declared::Integer(number.to_string())
            }
            Value::String(text) if text.contains("{{") => Declared::Environment,
            _ => Declared::NonLiteral,
        };
    }
    // Only a 2 written as itself, a plain scalar without a tag or an anchor,
    // selects v2; the value cannot tell `2` from `+2` or `0x2`, the text can.
    // A decimal that only rounds to 2 (`1.9999999999999999`) is no 2.
    match alint_core::yaml_depth::top_level_plain_scalar(contents, "version") {
        Ok(Some(Some(text))) if text == "2" => Declared::Two,
        Ok(Some(Some(text)))
            if matches!(version, Value::Number(_))
                && !(version.is_u64() || version.is_i64())
                && decimal(&text).is_some_and(|exact| exact != ("2".to_string(), 0)) =>
        {
            Declared::NonLiteral
        }
        _ => Declared::OtherTwo,
    }
}

/// A plain decimal number's exact value, as its significant digits and the
/// power of ten they are scaled by (`20e-1` and `2.0` are `("2", 0)`); `None`
/// for any other text (`0x2`, `.inf`).
fn decimal(text: &str) -> Option<(String, i64)> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (mantissa, exponent) = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i64>().ok()?),
        None => (unsigned, 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let scale = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
    let significant = digits.trim_start_matches('0');
    let trimmed = significant.trim_end_matches('0');
    if trimmed.is_empty() {
        return Some((String::new(), 0));
    }
    let zeros = i64::try_from(significant.len() - trimmed.len()).ok()?;
    let sign = if text.starts_with('-') { "-" } else { "" };
    Some((format!("{sign}{trimmed}"), scale.checked_add(zeros)?))
}

/// Whether `version` is 2 in any spelling: a number equal to 2 or a string
/// that reads `2`, tagged or not.
fn denotes_two(version: &Value) -> bool {
    match version {
        Value::Tagged(tagged) => denotes_two(&tagged.value),
        Value::Number(number) => number
            .as_f64()
            .is_some_and(|number| (number - f64::from(VERSION)).abs() < f64::EPSILON),
        Value::String(text) => text.trim() == "2",
        _ => false,
    }
}

/// Whether a source of `class` may be version 2 in this build.
const fn may_be_v2(class: ConfigSourceClass) -> bool {
    matches!(
        class,
        ConfigSourceClass::Root | ConfigSourceClass::DropIn | ConfigSourceClass::LocalExtend
    )
}

/// A source class that must be version 1, as a message names it.
fn v1_only_class(class: ConfigSourceClass) -> &'static str {
    match class {
        ConfigSourceClass::Remote => "a remote `extends:` source",
        ConfigSourceClass::Bundled => "a bundled ruleset",
        ConfigSourceClass::Nested => "a nested config (`nested_configs:`)",
        ConfigSourceClass::Inline => "a config parsed from a string (`alint_dsl::parse`)",
        ConfigSourceClass::Root | ConfigSourceClass::DropIn | ConfigSourceClass::LocalExtend => {
            "a local config"
        }
    }
}

/// `contents` parsed, for the dispatch to read its `version:` from, or `None`
/// when it does not parse: the version 1 decode then reports the parse error
/// as it always has. A source that declares version 2 (or inherits it) and
/// does not parse gets the parse error here, refused first if it may not be
/// version 2, rather than the version 1 decode's misreading of its version 2
/// keys.
pub(crate) fn read_document(contents: &str, origin: &Origin<'_>) -> Result<Option<Value>> {
    match crate::loader::decode_text::<Value>(contents) {
        Ok(document) => Ok(Some(document)),
        Err(parse_error) => {
            let declared = alint_core::yaml_depth::top_level_plain_scalar(contents, "version");
            let two = match declared {
                Ok(Some(Some(text))) => text == "2",
                Ok(None) => inherits_two(origin),
                Ok(Some(None)) | Err(_) => false,
            };
            if !two {
                return Ok(None);
            }
            refuse_v2_edge(origin)?;
            Err(Error::Yaml(parse_error))
        }
    }
}

/// Whether a source with no `version:` is version 2: a fragment of a version 2
/// root.
fn inherits_two(origin: &Origin<'_>) -> bool {
    origin.class == ConfigSourceClass::DropIn && origin.consumer_version == Some(VERSION)
}

/// Whether a source that decoded as `config`, with no v2-only key taken aside,
/// is version 1 for certain, so the dispatch would read it as version 1 without
/// a word: no `extends:` entry has an alias and its `version:`, as it decoded,
/// is not 2 (a 2 in any spelling decodes as 2, or does not decode), nor absent
/// in a fragment of a version 2 root. The loader then needs no second parse to
/// read the version (a version 1 config parses once, as before version 2).
pub(crate) fn plainly_v1(config: &crate::RawConfig, origin: &Origin<'_>) -> bool {
    let version_one = match config.version {
        Some(version) => version != VERSION,
        None => !inherits_two(origin),
    };
    version_one && config.extends.iter().all(|entry| entry.alias.is_none())
}

/// Why this build refuses a config of `version`, after the source's name: a
/// version other than 1 and 2 is unsupported, and names version 2 as the
/// experiment it is; a 2 that reaches the check was not read as version 2
/// (read from the environment, or in a source that must be version 1), and
/// must be written as the plain `version: 2` in a local config file.
pub(crate) fn unsupported_version(version: u32) -> String {
    if version == VERSION {
        "unsupported config version 2 as written: only a literal `version: 2` in a local config \
         file selects the experimental version 2 (a version read from the environment is \
         version 1)"
            .to_string()
    } else {
        format!(
            "unsupported config version {version} (this build supports version 1, and version 2 \
             as an experiment)"
        )
    }
}

/// Whether the dispatch could read a source whose version 1 decode failed as
/// anything but version 1 (version 2, or an error about version 2): it
/// inherits version 2, or its text could hold a v2-only key or a `version:`
/// that is 2 in some spelling. A text that cannot keeps its version 1 decode
/// error without the parse the dispatch reads. Written out, each of those
/// needs a word the text holds (`selectors`, `relations`, `extends` with an
/// `as`, or a `version` whose value is not a plain decimal integer other than
/// 2); an escape in a double-quoted scalar can spell any key, an anchor lets
/// an alias repeat one, and an explicit key (`? `) can be a block scalar, so a
/// text with any of those is parsed too.
pub(crate) fn may_select_version_two(contents: &str, origin: &Origin<'_>) -> bool {
    inherits_two(origin)
        || contents.contains('&')
        || contents.contains("selectors")
        || contents.contains("relations")
        || (contents.contains("extends") && words(contents, "as").next().is_some())
        || has_escape(contents)
        || has_explicit_key(contents)
        || words(contents, "version").any(|after| version_may_be_two(&contents[after..]))
}

/// The byte offsets just past each occurrence of `word` in `text` that is no
/// part of a longer name (`subversion`, `min_version`, `version-x`).
fn words<'t>(text: &'t str, word: &'t str) -> impl Iterator<Item = usize> + 't {
    let name = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    text.match_indices(word).filter_map(move |(at, _)| {
        let end = at + word.len();
        let before = text[..at].chars().next_back().is_some_and(name);
        let after = text[end..].chars().next().is_some_and(name);
        (!before && !after).then_some(end)
    })
}

/// A double-quoted scalar's escape that writes a character by its code, or an
/// escaped line break, which joins two lines into one word.
fn has_escape(text: &str) -> bool {
    text.match_indices('\\').any(|(at, _)| {
        matches!(
            text[at + 1..].chars().next(),
            Some('x' | 'u' | 'U' | '\n' | '\r')
        )
    })
}

/// An explicit-key indicator: a `?` followed by a blank or the end.
fn has_explicit_key(text: &str) -> bool {
    text.match_indices('?').any(|(at, _)| {
        text[at + 1..]
            .chars()
            .next()
            .is_none_or(|next| matches!(next, ' ' | '\t' | '\n' | '\r'))
    })
}

/// Whether the text after a word `version` could make it a key whose value is
/// 2: it is followed (past a closing quote and any blanks or line breaks) by
/// a `:` whose value is not a plain decimal integer other than 2 on the same
/// line. A `version` that no `:` follows is no key; a value on a later line,
/// quoted, tagged, anchored, aliased or written another way (`+2`, `2.0`,
/// `0x2`) may be 2.
fn version_may_be_two(after: &str) -> bool {
    let after = after.strip_prefix(['"', '\'']).unwrap_or(after);
    let Some(value) = after.trim_start().strip_prefix(':') else {
        return false;
    };
    let value = value.trim_start_matches([' ', '\t']);
    let token = value
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '}' | ']' | '#'))
        .next()
        .unwrap_or_default();
    let plain_integer = !token.is_empty()
        && token.bytes().all(|byte| byte.is_ascii_digit())
        && (token == "0" || !token.starts_with('0'));
    !plain_integer || token == "2"
}

/// Read `document`'s top-level `version:` (`document` is `contents` parsed,
/// or `None` when it does not parse) and decide which language it decodes
/// as, refusing a version 2 source where one may not be, a 2 written other
/// than as the plain integer, and v2-only keys in a version 1 source.
pub(crate) fn dispatch(
    document: Option<&Value>,
    contents: &str,
    origin: &Origin<'_>,
) -> Result<Surface> {
    let declared = declared_version(document, contents);
    let inherits_two = declared == Declared::Absent && inherits_two(origin);
    if declared == Declared::Two || inherits_two {
        refuse_v2_edge(origin)?;
        return Ok(Surface::V2);
    }
    if declared == Declared::OtherTwo {
        refuse_v2_edge(origin)?;
        return Err(Error::Other(format!(
            "{}: `version:` is 2 written other than as the plain integer `2` (quoted, tagged, \
             through an alias, or as `+2`, `0x2` or `2.0`); write `version: 2` to select \
             version 2",
            origin.display
        )));
    }
    if let Some(key) = v2_only_key(document) {
        return Err(needs_v2(origin, &declared, &key));
    }
    Ok(Surface::V1)
}

/// Refuse a `version: 2` source of a class that must be version 1, or on an
/// edge a version 1 config does not cross.
fn refuse_v2_edge(origin: &Origin<'_>) -> Result<()> {
    let display = origin.display;
    if !may_be_v2(origin.class) {
        return Err(Error::Other(format!(
            "{display}: {} must be `version: 1` in this build; `version: 2` is accepted only \
             in local files (the top-level config, a local `extends:` target or an \
             `.alint.d` fragment)",
            v1_only_class(origin.class)
        )));
    }
    match (origin.class, origin.consumer_version) {
        // The wording of the drop-in check every version shares.
        (ConfigSourceClass::DropIn, Some(consumer)) if consumer != VERSION => {
            Err(Error::Other(format!(
                "{display}: drop-in config version {VERSION} must equal root config version \
                 {consumer}"
            )))
        }
        (ConfigSourceClass::LocalExtend, Some(consumer))
            if !crate::loader::source_version_allowed(consumer, VERSION) =>
        {
            let consumer_display = origin.consumer_display.unwrap_or("the extending config");
            Err(Error::Other(format!(
                "{display}: a version {consumer} config ({consumer_display}) cannot extend this \
                 version {VERSION} config; give {consumer_display} `version: {VERSION}`, or make \
                 {display} `version: {consumer}`"
            )))
        }
        _ => Ok(()),
    }
}

/// The first v2-only key of `document`, in document order, as a message
/// names it: `selectors:`, `relations:` or an `extends:` entry's `as:`. A key
/// neither version knows before it is reported first, by the version 1
/// decode, so this is `None` then.
fn v2_only_key(document: Option<&Value>) -> Option<String> {
    let v1_keys = document::v1_keys();
    for (key, value) in document?.as_mapping()? {
        let known = key
            .as_str()
            .is_some_and(|key| v1_keys.contains(&key) || document::V2_ONLY_KEYS.contains(&key));
        if !known {
            return None;
        }
        match key.as_str() {
            Some(key @ ("selectors" | "relations")) => return Some(format!("`{key}:`")),
            Some("extends") => {
                // An `extends:` that is not a list has no alias; the keys
                // after it are still read.
                let aliased = value.as_sequence().and_then(|entries| {
                    entries
                        .iter()
                        .position(|entry| entry.as_mapping().is_some_and(|m| m.contains_key("as")))
                });
                if let Some(index) = aliased {
                    return Some(format!("`extends[{index}].as`"));
                }
            }
            _ => {}
        }
    }
    None
}

fn needs_v2(origin: &Origin<'_>, declared: &Declared, key: &str) -> Error {
    let display = origin.display;
    let why = if may_be_v2(origin.class) {
        match declared {
            Declared::Integer(version) => format!("; this config is version {version}"),
            Declared::Absent if origin.class == ConfigSourceClass::DropIn => format!(
                "; this config is version {}, as its root config is",
                origin.consumer_version.unwrap_or(1)
            ),
            Declared::Absent => "; this config declares no `version:`".to_string(),
            Declared::Environment => "; this config's `version:` is not the integer `2` (a \
                                       version read from the environment is version 1)"
                .to_string(),
            Declared::NonLiteral | Declared::Two | Declared::OtherTwo | Declared::Unreadable => {
                "; this config's `version:` is not the integer `2`".to_string()
            }
        }
    } else {
        format!(
            ", which {} cannot use in this build: it must be `version: 1`",
            v1_only_class(origin.class)
        )
    };
    Error::Other(format!("{display}: {key} needs `version: {VERSION}`{why}"))
}

/// Check a version 2 `document` (as written, before interpolation) where its
/// typed decode cannot: every top-level key and every `extends:` entry key is
/// one v2 knows, so an unknown key is reported against the v2 surface, and no
/// name, alias or reference is interpolated.
pub(crate) fn check_document(document: &Value, display: &str) -> Result<()> {
    let Some(root) = document.as_mapping() else {
        return Ok(());
    };
    let v1_keys = document::v1_keys();
    let known = |key: &str| v1_keys.contains(&key) || document::V2_ONLY_KEYS.contains(&key);
    for key in root.keys() {
        if !key.as_str().is_some_and(known) {
            let expected: Vec<&str> = v1_keys
                .iter()
                .chain(document::V2_ONLY_KEYS)
                .copied()
                .collect();
            return Err(Error::Other(format!(
                "{display}: unknown key `{}` in a version 2 config; expected one of {}",
                decode::key_text(key),
                decode::listed(&expected)
            )));
        }
    }
    let entries = root
        .get("extends")
        .and_then(Value::as_sequence)
        .map_or(&[][..], Vec::as_slice);
    for (index, entry) in entries.iter().enumerate() {
        let Some(entry) = entry.as_mapping() else {
            continue;
        };
        for key in entry.keys() {
            if !key
                .as_str()
                .is_some_and(|key| document::EXTENDS_ENTRY_KEYS.contains(&key))
            {
                return Err(Error::Other(format!(
                    "{display}: `extends[{index}]`: unknown key `{}` in an `extends:` entry; \
                     expected one of {}",
                    decode::key_text(key),
                    decode::listed(document::EXTENDS_ENTRY_KEYS)
                )));
            }
        }
        if let Some(alias) = entry.get("as")
            && !alias.is_string()
        {
            return Err(Error::Other(format!(
                "{display}: `extends[{index}].as`: expected an alias name, found {}",
                decode::describe(alias)
            )));
        }
    }
    reject_interpolated_names(document, display)
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
