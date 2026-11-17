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

pub(crate) use decode::source_objects;
pub(crate) use document::{Document, ExtendsEntry};
pub(crate) use environment::Environment;
pub(crate) use literal::reject_interpolated_names;
pub(crate) use model::{Objects, SourceId};
pub(crate) use origins::{
    FIELD_ORIGINS_MARKER, mark_field_origins, merge_field_origins, take_field_origins,
};
pub(crate) use resolve::resolve;

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
    /// The literal integer 2.
    Two,
    /// An integer other than 2, as written.
    Integer(String),
    /// Anything else: read from the environment, quoted, tagged, an alias, or
    /// not an integer.
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
    let Value::Number(number) = version else {
        return Declared::NonLiteral;
    };
    if !(version.is_u64() || version.is_i64()) {
        return Declared::NonLiteral;
    }
    if version.as_u64() != Some(u64::from(VERSION)) {
        return Declared::Integer(number.to_string());
    }
    // Only a 2 written as itself selects v2: not an alias of one, nor a
    // tagged one (a quoted `'2'` is a string, and `{{env.V}}` is one too).
    if alint_core::yaml_depth::mapping_subtree_is_literal(contents, &["version"]) == Ok(true) {
        Declared::Two
    } else {
        Declared::NonLiteral
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

/// Read `document`'s top-level `version:` (`document` is `contents` parsed,
/// or `None` when it does not parse) and decide which language it decodes
/// as, refusing a version 2 source where one may not be and v2-only keys in a
/// version 1 source.
pub(crate) fn dispatch(
    document: Option<&Value>,
    contents: &str,
    origin: &Origin<'_>,
) -> Result<Surface> {
    let declared = declared_version(document, contents);
    let inherits_two = declared == Declared::Absent
        && origin.class == ConfigSourceClass::DropIn
        && origin.consumer_version == Some(VERSION);
    if declared == Declared::Two || inherits_two {
        refuse_v2_edge(origin)?;
        return Ok(Surface::V2);
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
/// names it: `selectors:`, `relations:` or an `extends:` entry's `as:`.
fn v2_only_key(document: Option<&Value>) -> Option<String> {
    for (key, value) in document?.as_mapping()? {
        match key.as_str() {
            Some(key @ ("selectors" | "relations")) => return Some(format!("`{key}:`")),
            Some("extends") => {
                let aliased = value
                    .as_sequence()?
                    .iter()
                    .position(|entry| entry.as_mapping().is_some_and(|m| m.contains_key("as")));
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
            Declared::NonLiteral | Declared::Two | Declared::Unreadable => {
                "; this config's `version:` is not the literal integer `2` (a version read \
                 from the environment, quoted, tagged or written as an alias is version 1)"
                    .to_string()
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
