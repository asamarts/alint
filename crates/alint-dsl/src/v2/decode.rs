//! Decode a version 2 source's `selectors:`, `relations:` and `extends:`
//! aliases into [`super::model`], with errors that name the source, the key
//! path and what to write instead.
//!
//! The decode works on values the loader has already interpolated; the names
//! in them were checked literal before that ([`super::literal`]). It checks
//! syntax only: resolution and the plan check come once every source is
//! loaded.

use std::fmt::Write as _;

use alint_core::{Error, Result};
use ayaml::{Mapping, Value};

use super::model::{
    Alias, Entries, EntriesFormat, Field, FieldSource, Join, JoinSide, Member, Part, PathToken,
    Predicate, Read, Reader, Reference, Relation, Rows, Selector, SelectorRef, SourceId,
    SourceObjects, Transform,
};

// The keys and values each part of a selector or relation takes. The draft
// schema (`schemas/v2/config.json`) lists the same ones, which
// `schema_parity` checks.
pub(super) const SELECTOR_KEYS: &[&str] = &["paths", "scope_filter"];
/// `scope_filter:` in a selector: a rule's predicates but `changed_since`.
pub(super) const SELECTOR_FILTER_KEYS: &[&str] = &[
    "has_ancestor",
    "include_manifest_paths",
    "exclude_manifest_paths",
];
pub(super) const RELATION_KEYS: &[&str] = &[
    "from",
    "entries",
    "read",
    "optional",
    "union",
    "join",
    "fields",
    "transform",
    "where",
];
/// A relation has exactly one of these.
pub(super) const ROW_SOURCES: &[&str] = &["entries", "read", "union", "join"];
pub(super) const PART_KEYS: &[&str] = &["from", "entries", "read", "optional"];
pub(super) const ENTRIES_KEYS: &[&str] = &["format", "at", "leaves"];
pub(super) const READ_KEYS: &[&str] = &["json", "yaml", "toml", "regex", "lines", "whole_file"];
/// Extract forms `cross_file` has that a relation cannot read in v0.23.
const UNREADABLE_FORMATS: &[&str] = &["xml", "dotenv", "properties", "ini", "hcl"];
pub(super) const JOIN_KEYS: &[&str] = &["left", "right", "on", "unique"];
pub(super) const FIELD_KEYS: &[&str] = &["source", "sensitive"];
pub(super) const PREDICATE_KEYS: &[&str] = &["in", "not_in", "matches", "not_matches"];
pub(super) const MEMBER_KEYS: &[&str] = &["relation", "field"];
pub(super) const TRANSFORMS: &[&str] = &[
    "trim",
    "lower",
    "semver-major",
    "semver-minor",
    "strip-root",
];
/// The field sources written as a bare name; `bound:`, `left.` and `right.`
/// take an argument.
pub(super) const NAMED_FIELD_SOURCES: &[(&str, FieldSource)] = &[
    ("path", FieldSource::Path(PathToken::Path)),
    ("dir", FieldSource::Path(PathToken::Dir)),
    ("basename", FieldSource::Path(PathToken::Basename)),
    ("stem", FieldSource::Path(PathToken::Stem)),
    ("ext", FieldSource::Path(PathToken::Ext)),
    ("parent_name", FieldSource::Path(PathToken::ParentName)),
    ("entry.key", FieldSource::EntryKey),
    ("entry.value", FieldSource::EntryValue),
    ("entry.parent", FieldSource::EntryParent),
    ("entry.root", FieldSource::EntryRoot),
    ("entry.path", FieldSource::EntryPath),
    ("value", FieldSource::Value),
];
const FIELD_SOURCES: &str = "`path`, a path token (`dir`, `basename`, `stem`, `ext`, \
     `parent_name`), `entry.key`, `entry.value`, `entry.parent`, `entry.root`, `entry.path`, \
     `value`, `bound: <JSONPath>`, or in a join's output `left.<field>` or `right.<field>`";

/// Decode one source's objects: its `selectors:` and `relations:` (the values
/// the document decode took aside) and its `extends:` aliases.
pub(crate) fn source_objects(
    source: SourceId,
    v2_keys: &[(String, Value)],
    extends: &[super::ExtendsEntry],
) -> Result<SourceObjects> {
    let display = source.display.clone();
    let decoder = Decoder { source: &display };
    let mut names = Names::default();
    let mut objects = SourceObjects {
        source,
        selectors: Vec::new(),
        relations: Vec::new(),
        aliases: Vec::new(),
        extended: Vec::new(),
    };
    for (key, value) in v2_keys {
        decoder.untagged(value, key)?;
        let map = decoder.mapping(value, key, "a map from each name to its definition")?;
        for (name, definition) in map {
            let path = decoder.child_key(key, name, "a name")?;
            let name = decoder.declared_name(name, &path)?;
            names.declare(&decoder, &name, &path)?;
            if key == "selectors" {
                let selector = decoder.selector(definition, &path)?;
                objects.selectors.push((name, selector));
            } else {
                let relation = decoder.relation(definition, &path)?;
                objects.relations.push((name, relation));
            }
        }
    }
    for (index, extends) in extends.iter().enumerate() {
        let Some(alias) = &extends.alias else {
            continue;
        };
        let path = format!("extends[{index}].as");
        decoder.name(alias, &path)?;
        names.declare(&decoder, alias, &path)?;
        let url = extends.entry.url();
        // An entry with a refused scheme gets the loader's error for that.
        if matches!(
            crate::extends::EntryKind::of(url),
            crate::extends::EntryKind::Https | crate::extends::EntryKind::Bundled(_)
        ) {
            return Err(decoder.error(
                &path,
                "an alias names a local `extends:` source, whose selectors and relations \
                 the config can then reach as `alias.name`; a bundled or remote source must \
                 be `version: 1` and declares none, so remove `as:` from this entry",
            ));
        }
        objects.aliases.push((
            alias.clone(),
            Alias {
                url: url.to_string(),
                target: None,
            },
        ));
    }
    // In this crate's unit tests, the draft schema must accept every source
    // this decode accepts.
    #[cfg(test)]
    super::schema_parity::assert_accepts(v2_keys, extends);
    Ok(objects)
}

/// The names one source has declared so far, each with the key path that
/// declared it. Selectors, relations and `extends:` aliases share one
/// namespace per source, so `deps` cannot be both an alias and a relation.
#[derive(Default)]
struct Names(Vec<(String, String)>);

impl Names {
    fn declare(&mut self, decoder: &Decoder<'_>, name: &str, path: &str) -> Result<()> {
        if let Some((_, first)) = self.0.iter().find(|(held, _)| held == name) {
            return Err(decoder.error(
                path,
                format!(
                    "`{name}` is already declared at `{first}`; a name or alias is declared \
                     once in a config (selectors, relations and `extends:` aliases share one \
                     namespace), so rename one of them"
                ),
            ));
        }
        self.0.push((name.to_string(), path.to_string()));
        Ok(())
    }
}

/// Whether `text` is a valid name: an ASCII letter or `_`, then ASCII
/// letters, digits, `_` and `-`. A `.` would read as a qualified reference.
pub(crate) fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// What a value is, for an error that says what was found.
pub(crate) fn describe(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(_) => "a boolean".to_string(),
        Value::Number(_) => "a number".to_string(),
        Value::String(_) => "a string".to_string(),
        Value::Sequence(_) => "a list".to_string(),
        Value::Mapping(_) => "a map".to_string(),
        Value::Tagged(tagged) => format!("a value with the YAML tag `{}`", tagged.tag),
    }
}

/// `items` as "`a`, `b`, `c`".
pub(crate) fn listed(items: &[&str]) -> String {
    let mut out = String::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        write!(out, "`{item}`").expect("writing to a String cannot fail");
    }
    out
}

/// A map key as an error quotes it.
pub(crate) fn key_text(key: &Value) -> String {
    match key {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Null => "~".to_string(),
        other => format!("<{}>", describe(other)),
    }
}

/// Read `text` as a reference: a name, or `alias.name`. The error says what
/// to write instead.
pub(crate) fn parse_reference(text: &str) -> std::result::Result<Reference, String> {
    let parts: Vec<&str> = text.split('.').collect();
    let valid = parts.len() <= 2 && parts.iter().all(|part| is_name(part));
    if !valid {
        let mut message = format!(
            "`{text}` is not a valid reference: write a name, or `alias.name` for an \
             object of a source this config extends with `as: alias`; a name starts with \
             an ASCII letter or `_` and holds only ASCII letters, digits, `_` and `-`"
        );
        if parts.len() > 2 {
            message.push_str(" (there is no chained name such as `a.b.c`)");
        }
        return Err(message);
    }
    Ok(match parts.as_slice() {
        [alias, name] => Reference {
            alias: Some((*alias).to_string()),
            name: (*name).to_string(),
        },
        _ => Reference {
            alias: None,
            name: text.to_string(),
        },
    })
}

struct Decoder<'s> {
    source: &'s str,
}

impl Decoder<'_> {
    fn error(&self, path: &str, message: impl std::fmt::Display) -> Error {
        Error::Other(format!("{}: `{path}`: {message}", self.source))
    }

    /// Refuse a YAML tag anywhere in `value`: ayaml reads through one, so
    /// `!relation { ... }` would otherwise decode as if the tag were not there,
    /// and no tag means anything in a selector or relation.
    fn untagged(&self, value: &Value, path: &str) -> Result<()> {
        match value {
            Value::Tagged(tagged) => Err(self.error(
                path,
                format!(
                    "a YAML tag (`{}`) has no meaning in a selector or relation; remove it",
                    tagged.tag
                ),
            )),
            Value::Sequence(items) => items
                .iter()
                .enumerate()
                .try_for_each(|(index, item)| self.untagged(item, &format!("{path}[{index}]"))),
            Value::Mapping(map) => map.iter().try_for_each(|(key, item)| {
                let item_path = format!("{path}.{}", key_text(key));
                self.untagged(key, &item_path)?;
                self.untagged(item, &item_path)
            }),
            _ => Ok(()),
        }
    }

    fn mapping<'v>(&self, value: &'v Value, path: &str, expected: &str) -> Result<&'v Mapping> {
        value.as_mapping().ok_or_else(|| {
            self.error(
                path,
                format!("expected {expected}, found {}", describe(value)),
            )
        })
    }

    /// `map`, each of whose keys must be one of `known`; `what` names the map
    /// in the error (`a relation`).
    fn known_keys(&self, map: &Mapping, path: &str, what: &str, known: &[&str]) -> Result<()> {
        for key in map.keys() {
            let text = key.as_str().filter(|text| known.contains(text));
            if text.is_none() {
                return Err(self.error(
                    path,
                    format!(
                        "unknown key `{}` in {what}; expected one of {}",
                        key_text(key),
                        listed(known)
                    ),
                ));
            }
        }
        Ok(())
    }

    /// The key path of `key` under `parent`, when `key` is a string.
    fn child_key(&self, parent: &str, key: &Value, what: &str) -> Result<String> {
        match key.as_str() {
            Some(text) => Ok(format!("{parent}.{text}")),
            None => Err(self.error(
                parent,
                format!(
                    "`{}` is not {what}: expected a string, found {}",
                    key_text(key),
                    describe(key)
                ),
            )),
        }
    }

    fn string<'v>(&self, value: &'v Value, path: &str, expected: &str) -> Result<&'v str> {
        value.as_str().ok_or_else(|| {
            self.error(
                path,
                format!("expected {expected}, found {}", describe(value)),
            )
        })
    }

    fn boolean(&self, value: &Value, path: &str) -> Result<bool> {
        value.as_bool().ok_or_else(|| {
            self.error(
                path,
                format!("expected `true` or `false`, found {}", describe(value)),
            )
        })
    }

    fn name(&self, text: &str, path: &str) -> Result<()> {
        if is_name(text) {
            return Ok(());
        }
        let mut message = format!(
            "`{text}` is not a valid name: a name starts with an ASCII letter or `_` and \
             holds only ASCII letters, digits, `_` and `-`"
        );
        if text.contains('.') {
            message.push_str(" (a `.` separates an alias from a name in a reference)");
        }
        Err(self.error(path, message))
    }

    /// Refuse a `JSONPath` that does not parse, which would fail every
    /// resource the relation reads.
    fn json_path(&self, query: &str, path: &str) -> Result<()> {
        alint_core::Extract::structured(alint_core::Format::Json, query)
            .map(drop)
            .map_err(|error| {
                let hint = alint_core::jsonpath_diagnostics::diagnose_path(query)
                    .map(|hint| format!("; {hint}"))
                    .unwrap_or_default();
                self.error(path, format!("{error}{hint}"))
            })
    }

    /// Compile `pattern`, which a filter or a `read:` matches.
    fn regex(&self, pattern: &str, path: &str) -> Result<regex::Regex> {
        regex::Regex::new(pattern)
            .map_err(|error| self.error(path, format!("`{pattern}` is not a valid regex: {error}")))
    }

    /// A declared name: a map key under `selectors:` or `relations:`.
    fn declared_name(&self, key: &Value, path: &str) -> Result<String> {
        let text = key.as_str().unwrap_or_default();
        self.name(text, path)?;
        Ok(text.to_string())
    }

    /// A field name written as a map key (`fields:`, `transform:`, `where:`,
    /// `on:`).
    fn field_key(&self, key: &Value, parent: &str) -> Result<(String, String)> {
        let path = self.child_key(parent, key, "a field name")?;
        let text = key.as_str().unwrap_or_default();
        self.name(text, &path)?;
        Ok((text.to_string(), path))
    }

    fn reference(&self, text: &str, path: &str) -> Result<Reference> {
        parse_reference(text).map_err(|message| self.error(path, message))
    }

    fn selector(&self, value: &Value, path: &str) -> Result<Selector> {
        let map = self.mapping(
            value,
            path,
            "a selector: a map with `paths:` and an optional `scope_filter:`",
        )?;
        self.known_keys(map, path, "a selector", SELECTOR_KEYS)?;
        let paths_path = format!("{path}.paths");
        let paths = map.get("paths").ok_or_else(|| {
            self.error(
                path,
                "a selector needs `paths:` (a glob, a list of globs with `!` excludes, or an \
                 `{ include, exclude }` map)",
            )
        })?;
        // A rule's `paths:` grammar, without the keys it would ignore.
        if let Some(map) = paths.as_mapping() {
            self.known_keys(map, &paths_path, "`paths:`", &["include", "exclude"])?;
        }
        let paths = crate::core_yaml::decode::<alint_core::PathsSpec>(paths.clone())
            .map_err(|error| self.error(&paths_path, error))?;
        let scope_filter = map
            .get("scope_filter")
            .map(|filter| self.selector_filter(filter, &format!("{path}.scope_filter")))
            .transpose()?;
        Ok(Selector {
            paths,
            scope_filter,
            reads_environment: false,
        })
    }

    fn selector_filter(&self, value: &Value, path: &str) -> Result<alint_core::ScopeFilterSpec> {
        let map = self.mapping(value, path, "a `scope_filter:` map")?;
        if map.contains_key("changed_since") {
            return Err(self.error(
                &format!("{path}.changed_since"),
                "a selector cannot use `changed_since`: the rules that read a selector judge \
                 the whole tree, under `--changed` too, so what a selector picks must not \
                 depend on the diff; remove it (a selector may use `has_ancestor`, \
                 `include_manifest_paths` and `exclude_manifest_paths`)",
            ));
        }
        self.known_keys(
            map,
            path,
            "a selector's `scope_filter:`",
            SELECTOR_FILTER_KEYS,
        )?;
        crate::core_yaml::decode::<alint_core::ScopeFilterSpec>(value.clone())
            .map_err(|error| self.error(path, error))
    }

    fn selector_ref(&self, value: &Value, path: &str) -> Result<SelectorRef> {
        match value {
            Value::String(text) => Ok(SelectorRef::Named(self.reference(text, path)?)),
            Value::Mapping(_) => Ok(SelectorRef::Inline(Box::new(self.selector(value, path)?))),
            other => Err(self.error(
                path,
                format!(
                    "expected a selector name, or an inline selector map, found {}",
                    describe(other)
                ),
            )),
        }
    }

    fn relation(&self, value: &Value, path: &str) -> Result<Relation> {
        let map = self.mapping(
            value,
            path,
            "a relation: a map with `fields:` and one of `entries:`, `read:`, `union:` and \
             `join:`",
        )?;
        self.known_keys(map, path, "a relation", RELATION_KEYS)?;
        let present: Vec<&str> = ROW_SOURCES
            .iter()
            .copied()
            .filter(|key| map.contains_key(*key))
            .collect();
        let rows = match present.as_slice() {
            ["entries" | "read"] => Rows::Part(self.part(map, path)?),
            [kind @ ("union" | "join")] => {
                for key in ["from", "optional"] {
                    if map.contains_key(key) {
                        return Err(self.error(
                            &format!("{path}.{key}"),
                            format!(
                                "a relation with `{kind}:` takes no `{key}:`; {}",
                                if *kind == "union" {
                                    "each part of the union has its own"
                                } else {
                                    "a join reads the relations it names"
                                }
                            ),
                        ));
                    }
                }
                if *kind == "union" {
                    Rows::Union(self.union(&map["union"], &format!("{path}.union"))?)
                } else {
                    Rows::Join(self.join(&map["join"], &format!("{path}.join"))?)
                }
            }
            [] => {
                return Err(self.error(
                    path,
                    "a relation needs exactly one of `entries:`, `read:`, `union:` and \
                     `join:`; it has none",
                ));
            }
            many => {
                let named: Vec<String> = many.iter().map(|key| format!("`{key}:`")).collect();
                return Err(self.error(
                    path,
                    format!(
                        "a relation needs exactly one of `entries:`, `read:`, `union:` and \
                         `join:`; it has {}",
                        named.join(" and ")
                    ),
                ));
            }
        };
        let fields = map.get("fields").ok_or_else(|| {
            self.error(
                path,
                "a relation needs `fields:`, a map from each field's name to its source",
            )
        })?;
        let fields = self.fields(fields, &format!("{path}.fields"), &rows)?;
        let transforms = map
            .get("transform")
            .map(|value| self.transforms(value, &format!("{path}.transform")))
            .transpose()?
            .unwrap_or_default();
        let filters = map
            .get("where")
            .map(|value| self.filters(value, &format!("{path}.where")))
            .transpose()?
            .unwrap_or_default();
        Ok(Relation {
            rows,
            fields,
            transforms,
            filters,
            reads_environment: false,
        })
    }

    /// A relation with `entries:` or `read:`, or a union's part: `map` holds
    /// exactly one of the two (the caller checked).
    fn part(&self, map: &Mapping, path: &str) -> Result<Part> {
        let from = map.get("from").ok_or_else(|| {
            self.error(
                path,
                "`entries:` and `read:` need `from:`, a selector name or an inline selector map",
            )
        })?;
        let from = self.selector_ref(from, &format!("{path}.from"))?;
        let optional = map
            .get("optional")
            .map(|value| self.boolean(value, &format!("{path}.optional")))
            .transpose()?;
        let reader = if let Some(entries) = map.get("entries") {
            if optional.is_some() {
                return Err(self.error(
                    &format!("{path}.optional"),
                    "`optional:` applies to `read:` only (an `entries:` path a resource lacks \
                     yields no rows from it already); remove it",
                ));
            }
            Reader::Entries(self.entries(entries, &format!("{path}.entries"))?)
        } else {
            Reader::Read {
                read: self.read(&map["read"], &format!("{path}.read"))?,
                optional: optional.unwrap_or(false),
            }
        };
        Ok(Part { from, reader })
    }

    fn union(&self, value: &Value, path: &str) -> Result<Vec<Part>> {
        let Some(parts) = value.as_sequence() else {
            return Err(self.error(
                path,
                format!(
                    "expected a list of parts, each `{{ from, entries or read, optional }}`, \
                     found {}",
                    describe(value)
                ),
            ));
        };
        if parts.len() < 2 {
            return Err(self.error(
                path,
                "a union needs at least two parts; a relation with one part is a plain \
                 relation, so write its `from:` and `entries:` or `read:` on the relation",
            ));
        }
        let mut out = Vec::with_capacity(parts.len());
        for (index, part) in parts.iter().enumerate() {
            let part_path = format!("{path}[{index}]");
            let map = self.mapping(
                part,
                &part_path,
                "a union part: a map with `from:` and `entries:` or `read:`",
            )?;
            self.known_keys(map, &part_path, "a union part", PART_KEYS)?;
            match (map.contains_key("entries"), map.contains_key("read")) {
                (true, false) | (false, true) => {}
                (has_entries, _) => {
                    return Err(self.error(
                        &part_path,
                        format!(
                            "a union part needs exactly one of `entries:` and `read:`; it has {}",
                            if has_entries { "both" } else { "neither" }
                        ),
                    ));
                }
            }
            out.push(self.part(map, &part_path)?);
        }
        Ok(out)
    }

    fn entries(&self, value: &Value, path: &str) -> Result<Entries> {
        let map = self.mapping(value, path, "a map with `format:`, `at:` and `leaves:`")?;
        self.known_keys(map, path, "`entries:`", ENTRIES_KEYS)?;
        let format = map.get("format").ok_or_else(|| {
            self.error(
                path,
                "`entries:` needs `format:` (`json`, `yaml` or `toml`)",
            )
        })?;
        let format_path = format!("{path}.format");
        let format = match self.string(format, &format_path, "`json`, `yaml` or `toml`")? {
            "json" => EntriesFormat::Json,
            "yaml" => EntriesFormat::Yaml,
            "toml" => EntriesFormat::Toml,
            other => {
                return Err(self.error(
                    &format_path,
                    format!(
                        "unknown format `{other}`; expected one of `json`, `yaml`, `toml` (in \
                         this build entries are read from JSON, YAML and TOML only)"
                    ),
                ));
            }
        };
        let at = map.get("at").ok_or_else(|| {
            self.error(
                path,
                "`entries:` needs `at:`, a JSONPath or a list of them selecting the objects \
                 whose members become rows",
            )
        })?;
        let at = self.paths_list(at, &format!("{path}.at"))?;
        let leaves = map
            .get("leaves")
            .map(|value| self.boolean(value, &format!("{path}.leaves")))
            .transpose()?
            .unwrap_or(false);
        Ok(Entries { format, at, leaves })
    }

    /// A JSON path, or a non-empty list of them.
    fn paths_list(&self, value: &Value, path: &str) -> Result<Vec<String>> {
        let expected = "a JSONPath, or a non-empty list of them";
        match value {
            Value::String(text) => {
                self.json_path(text, path)?;
                Ok(vec![text.clone()])
            }
            Value::Sequence(items) if !items.is_empty() => items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let item_path = format!("{path}[{index}]");
                    let text = self.string(item, &item_path, "a JSONPath")?;
                    self.json_path(text, &item_path)?;
                    Ok(text.to_string())
                })
                .collect(),
            other => Err(self.error(
                path,
                format!("expected {expected}, found {}", describe(other)),
            )),
        }
    }

    fn read(&self, value: &Value, path: &str) -> Result<Read> {
        let map = self.mapping(
            value,
            path,
            "a map with one of `json`, `yaml`, `toml`, `regex`, `lines` and `whole_file`",
        )?;
        for key in map.keys() {
            if let Some(format) = key.as_str().filter(|key| UNREADABLE_FORMATS.contains(key)) {
                return Err(self.error(
                    path,
                    format!(
                        "`read:` cannot take `{format}` in this build: a relation reads JSON, \
                         YAML and TOML values, a regex, lines or a whole file (dotenv and \
                         properties values stay redacted, and XML, INI and HCL are not \
                         readable)"
                    ),
                ));
            }
        }
        self.known_keys(map, path, "`read:`", READ_KEYS)?;
        let mut forms = map.iter();
        let (Some((form, option)), None) = (forms.next(), forms.next()) else {
            return Err(self.error(
                path,
                format!(
                    "`read:` takes exactly one of {}; it has {}",
                    listed(READ_KEYS),
                    if map.is_empty() { "none" } else { "several" }
                ),
            ));
        };
        let form = form.as_str().unwrap_or_default();
        let option_path = format!("{path}.{form}");
        let text = |expected: &str| {
            self.string(option, &option_path, expected)
                .map(str::to_string)
        };
        let query = || {
            let query = text("a JSONPath")?;
            self.json_path(&query, &option_path)?;
            Ok::<_, Error>(query)
        };
        Ok(match form {
            "json" => Read::Json(query()?),
            "yaml" => Read::Yaml(query()?),
            "toml" => Read::Toml(query()?),
            "regex" => {
                let pattern = text("a regex whose capture group 1 is the value")?;
                let regex = self.regex(&pattern, &option_path)?;
                if regex.captures_len() < 2 {
                    return Err(self.error(
                        &option_path,
                        format!(
                            "a `read: {{ regex }}` takes each value from capture group 1, and \
                             `{pattern}` has no group; put the value in parentheses, as in \
                             `version: (.*)`"
                        ),
                    ));
                }
                Read::Regex(pattern)
            }
            "lines" => {
                let lines =
                    self.mapping(option, &option_path, "a map: `{}` or `{ comment: \"#\" }`")?;
                self.known_keys(lines, &option_path, "`lines:`", &["comment"])?;
                let comment = lines
                    .get("comment")
                    .map(|comment| {
                        self.string(comment, &format!("{option_path}.comment"), "a string")
                            .map(str::to_string)
                    })
                    .transpose()?;
                Read::Lines { comment }
            }
            _ => {
                let whole = self.mapping(option, &option_path, "an empty map, `{}`")?;
                if !whole.is_empty() {
                    return Err(self.error(
                        &option_path,
                        "`whole_file:` takes no options; write `whole_file: {}`",
                    ));
                }
                Read::WholeFile
            }
        })
    }

    fn join(&self, value: &Value, path: &str) -> Result<Join> {
        let map = self.mapping(
            value,
            path,
            "a map with `left:`, `right:`, `on:` and `unique:`",
        )?;
        self.known_keys(map, path, "a join", JOIN_KEYS)?;
        let side = |key: &str| -> Result<Reference> {
            let value = map.get(key).ok_or_else(|| {
                self.error(path, format!("a join needs `{key}:`, a relation name"))
            })?;
            let key_path = format!("{path}.{key}");
            let text = self.string(value, &key_path, "a relation name")?;
            self.reference(text, &key_path)
        };
        let left = side("left")?;
        let right = side("right")?;
        let on = map.get("on").ok_or_else(|| {
            self.error(
                path,
                "a join needs `on:`, a map from each left field to the right field it equals",
            )
        })?;
        let on_path = format!("{path}.on");
        let on_map = self.mapping(on, &on_path, "a map from a left field to a right field")?;
        if on_map.is_empty() {
            return Err(self.error(&on_path, "a join matches on at least one field"));
        }
        let mut pairs = Vec::with_capacity(on_map.len());
        for (left_field, right_field) in on_map {
            let (left_field, pair_path) = self.field_key(left_field, &on_path)?;
            let right_field = self.string(right_field, &pair_path, "a right field's name")?;
            self.name(right_field, &pair_path)?;
            pairs.push((left_field, right_field.to_string()));
        }
        let unique = map.get("unique").ok_or_else(|| {
            self.error(
                path,
                "a join needs `unique:` (`left` or `right`), the side that holds at most one \
                 row per join key; it has no default",
            )
        })?;
        let unique_path = format!("{path}.unique");
        let unique = match self.string(unique, &unique_path, "`left` or `right`")? {
            "left" => JoinSide::Left,
            "right" => JoinSide::Right,
            other => {
                return Err(self.error(
                    &unique_path,
                    format!("unknown side `{other}`; expected `left` or `right`"),
                ));
            }
        };
        Ok(Join {
            left,
            right,
            on: pairs,
            unique,
        })
    }

    fn fields(&self, value: &Value, path: &str, rows: &Rows) -> Result<Vec<(String, Field)>> {
        let map = self.mapping(value, path, "a map from each field's name to its source")?;
        if map.is_empty() {
            return Err(self.error(path, "a relation names at least one field"));
        }
        let mut fields = Vec::with_capacity(map.len());
        for (name, definition) in map {
            let (name, field_path) = self.field_key(name, path)?;
            let field = self.field(definition, &field_path)?;
            self.field_fits_rows(&field.source, rows, &field_path)?;
            fields.push((name, field));
        }
        Ok(fields)
    }

    fn field(&self, value: &Value, path: &str) -> Result<Field> {
        match value {
            Value::String(text) => Ok(Field {
                source: self.field_source(text, path)?,
                sensitive: false,
            }),
            Value::Mapping(map) => {
                if map.contains_key("bound") {
                    return Err(self.error(
                        path,
                        "unknown key `bound` in a field; a bound field's source is the string \
                         `\"bound: <JSONPath>\"`, as in `{ source: \"bound: $.name\" }`",
                    ));
                }
                self.known_keys(map, path, "a field", FIELD_KEYS)?;
                let source = map.get("source").ok_or_else(|| {
                    self.error(path, format!("a field needs `source:`: {FIELD_SOURCES}"))
                })?;
                let source_path = format!("{path}.source");
                let source = self.string(source, &source_path, "a field source")?;
                let sensitive = map
                    .get("sensitive")
                    .map(|value| self.boolean(value, &format!("{path}.sensitive")))
                    .transpose()?
                    .unwrap_or(false);
                Ok(Field {
                    source: self.field_source(source, &source_path)?,
                    sensitive,
                })
            }
            other => Err(self.error(
                path,
                format!(
                    "expected a field source or a `{{ source, sensitive }}` map, found {}",
                    describe(other)
                ),
            )),
        }
    }

    fn field_source(&self, text: &str, path: &str) -> Result<FieldSource> {
        if let Some((_, source)) = NAMED_FIELD_SOURCES.iter().find(|(name, _)| *name == text) {
            return Ok(source.clone());
        }
        Ok(if let Some(query) = text.strip_prefix("bound:") {
            let query = query.trim();
            if query.is_empty() {
                return Err(self.error(path, "`bound:` needs a JSONPath, as in `bound: $.name`"));
            }
            self.json_path(query, path)?;
            FieldSource::Bound(query.to_string())
        } else if let Some((side, field)) = text
            .strip_prefix("left.")
            .map(|field| (JoinSide::Left, field))
            .or_else(|| {
                text.strip_prefix("right.")
                    .map(|field| (JoinSide::Right, field))
            })
        {
            self.name(field, path)?;
            FieldSource::Side(side, field.to_string())
        } else {
            return Err(self.error(
                path,
                format!("`{text}` is not a field source; expected {FIELD_SOURCES}"),
            ));
        })
    }

    /// Refuse a source the relation's rows cannot have: a join's fields name
    /// a side; `entries:` rows have entries, `read:` rows a value. Which part
    /// of a union binds which field is the plan check's.
    fn field_fits_rows(&self, source: &FieldSource, rows: &Rows, path: &str) -> Result<()> {
        let entry = matches!(
            source,
            FieldSource::EntryKey
                | FieldSource::EntryValue
                | FieldSource::EntryParent
                | FieldSource::EntryRoot
                | FieldSource::EntryPath
        );
        let problem = match (rows, source) {
            (Rows::Join(_), FieldSource::Side(..)) => None,
            (Rows::Join(_), _) => Some(
                "a join's output fields each name a side: write `left.<field>` or \
                 `right.<field>` (nothing is copied implicitly)",
            ),
            (_, FieldSource::Side(..)) => Some(
                "`left.<field>` and `right.<field>` name the sides of a `join:`, which this \
                 relation does not have",
            ),
            (Rows::Part(part), FieldSource::Value) if matches!(part.reader, Reader::Entries(_)) => {
                Some("`value` is a `read:` row's value; an `entries:` row has `entry.value`")
            }
            (Rows::Part(part), _) if entry && matches!(part.reader, Reader::Read { .. }) => {
                Some("`entry.*` sources need `entries:`; a `read:` row has `value`")
            }
            _ => None,
        };
        match problem {
            Some(problem) => Err(self.error(path, problem)),
            None => Ok(()),
        }
    }

    fn transforms(&self, value: &Value, path: &str) -> Result<Vec<(String, Vec<Transform>)>> {
        let map = self.mapping(
            value,
            path,
            "a map from a field's name to a list of transforms",
        )?;
        let mut out = Vec::with_capacity(map.len());
        for (field, list) in map {
            let (field, field_path) = self.field_key(field, path)?;
            let items = match list.as_sequence() {
                Some(items) if !items.is_empty() => items,
                _ => {
                    return Err(self.error(
                        &field_path,
                        format!(
                            "expected a non-empty list of transforms ({}), found {}",
                            listed(TRANSFORMS),
                            describe(list)
                        ),
                    ));
                }
            };
            let mut transforms = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let item_path = format!("{field_path}[{index}]");
                let transform = match self.string(item, &item_path, "a transform name")? {
                    "trim" => Transform::Trim,
                    "lower" => Transform::Lower,
                    "semver-major" => Transform::SemverMajor,
                    "semver-minor" => Transform::SemverMinor,
                    "strip-root" => Transform::StripRoot,
                    "cldr-plurals" => {
                        return Err(self.error(
                            &item_path,
                            "`cldr-plurals` is not a transform: CLDR plural collapse is \
                             `plurals: cldr`, an option of a `set` assertion",
                        ));
                    }
                    other => {
                        return Err(self.error(
                            &item_path,
                            format!(
                                "unknown transform `{other}`; expected one of {}",
                                listed(TRANSFORMS)
                            ),
                        ));
                    }
                };
                transforms.push(transform);
            }
            out.push((field, transforms));
        }
        Ok(out)
    }

    fn filters(&self, value: &Value, path: &str) -> Result<Vec<(String, Vec<Predicate>)>> {
        let map = self.mapping(value, path, "a map from a field's name to its predicates")?;
        let mut out = Vec::with_capacity(map.len());
        for (field, predicates) in map {
            let (field, field_path) = self.field_key(field, path)?;
            let predicates_map = self.mapping(
                predicates,
                &field_path,
                "a map of predicates (`in`, `not_in`, `matches`, `not_matches`)",
            )?;
            if predicates_map.is_empty() {
                return Err(self.error(
                    &field_path,
                    format!("a filter needs at least one of {}", listed(PREDICATE_KEYS)),
                ));
            }
            self.known_keys(predicates_map, &field_path, "a filter", PREDICATE_KEYS)?;
            let mut list = Vec::with_capacity(predicates_map.len());
            for (predicate, operand) in predicates_map {
                let predicate = predicate.as_str().unwrap_or_default();
                let operand_path = format!("{field_path}.{predicate}");
                list.push(match predicate {
                    "in" => Predicate::In(self.members(operand, &operand_path)?),
                    "not_in" => Predicate::NotIn(self.members(operand, &operand_path)?),
                    "matches" => {
                        let pattern = self.string(operand, &operand_path, "a regex")?;
                        self.regex(pattern, &operand_path)?;
                        Predicate::Matches(pattern.to_string())
                    }
                    _ => {
                        let pattern = self.string(operand, &operand_path, "a regex")?;
                        self.regex(pattern, &operand_path)?;
                        Predicate::NotMatches(pattern.to_string())
                    }
                });
            }
            out.push((field, list));
        }
        Ok(out)
    }

    fn members(&self, value: &Value, path: &str) -> Result<Vec<Member>> {
        let items = match value.as_sequence() {
            Some(items) if !items.is_empty() => items,
            _ => {
                return Err(self.error(
                    path,
                    format!(
                        "expected a non-empty list of literals and `{{ relation, field }}` \
                         references, found {}",
                        describe(value)
                    ),
                ));
            }
        };
        let mut members = Vec::with_capacity(items.len());
        for (index, item) in items.iter().enumerate() {
            let item_path = format!("{path}[{index}]");
            members.push(match item {
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {
                    Member::Literal(item.clone())
                }
                Value::Mapping(map) => {
                    self.known_keys(map, &item_path, "a membership reference", MEMBER_KEYS)?;
                    let (Some(relation), Some(field)) = (map.get("relation"), map.get("field"))
                    else {
                        return Err(self.error(
                            &item_path,
                            "a membership reference needs `relation:` and `field:`",
                        ));
                    };
                    let relation_path = format!("{item_path}.relation");
                    let relation = self.string(relation, &relation_path, "a relation name")?;
                    let relation = self.reference(relation, &relation_path)?;
                    let field_path = format!("{item_path}.field");
                    let field = self.string(field, &field_path, "a field name")?;
                    self.name(field, &field_path)?;
                    Member::Relation {
                        relation,
                        field: field.to_string(),
                    }
                }
                other => {
                    return Err(self.error(
                        &item_path,
                        format!(
                            "expected a literal (null, a boolean, a number or a string) or a \
                             `{{ relation, field }}` reference, found {}",
                            describe(other)
                        ),
                    ));
                }
            });
        }
        Ok(members)
    }
}

#[cfg(test)]
#[path = "decode_tests.rs"]
mod tests;
