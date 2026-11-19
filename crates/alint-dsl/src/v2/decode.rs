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
    SourceObjects, Transform, Twins,
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

/// Where a value sits in a version 2 `selectors:` or `relations:` value, as
/// far as its interpolation needs to know: whether it lands in a field that
/// takes a boolean ([`BOOLEAN_FIELDS`]), where a value substituted from the
/// environment reads as one. Everywhere else it stays text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Position {
    /// The value of `relations:`.
    Relations,
    /// A relation's definition.
    Relation,
    /// A union's list of parts.
    Union,
    /// A union's part.
    Part,
    /// A relation's or a part's `entries:`.
    Entries,
    /// A relation's `fields:`.
    Fields,
    /// One field's definition.
    Field,
    /// A field that takes a boolean.
    Boolean,
    /// Anywhere else, all of `selectors:` too: text, or lists and maps of it.
    Text,
}

impl Position {
    /// The position of the value under `key` (`None` for a list's item) of
    /// the value at this one.
    pub(crate) fn below(self, key: Option<&str>) -> Self {
        match (self, key) {
            (_, Some(key)) if BOOLEAN_FIELDS.contains(&(self, key)) => Self::Boolean,
            (Self::Relations, Some(_)) => Self::Relation,
            (Self::Relation, Some("union")) => Self::Union,
            (Self::Union, None) => Self::Part,
            (Self::Relation | Self::Part, Some("entries")) => Self::Entries,
            (Self::Relation, Some("fields")) => Self::Fields,
            (Self::Fields, Some(_)) => Self::Field,
            _ => Self::Text,
        }
    }
}

/// Each field of a selector or relation that takes a boolean, with where it
/// sits; the decode reads each with `boolean()`. No field takes a number, and
/// every other one takes text (or a list or map of it), as an `in:` list's
/// members compare as text.
pub(crate) const BOOLEAN_FIELDS: &[(Position, &str)] = &[
    (Position::Relation, "optional"),
    (Position::Part, "optional"),
    (Position::Entries, "leaves"),
    (Position::Field, "sensitive"),
];
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

/// The key of a `read:` form that reads no JSON, YAML or TOML document, which
/// a `bound:` field reads its value from; `None` for one that does.
const fn unstructured_form(read: &Read) -> Option<&'static str> {
    match read {
        Read::Json(_) | Read::Yaml(_) | Read::Toml(_) => None,
        Read::Regex(_) => Some("regex"),
        Read::Lines { .. } => Some("lines"),
        Read::WholeFile => Some("whole_file"),
    }
}

/// Why a `read:` with `form` (`regex`, `lines` or `whole_file`) binds no
/// `bound:` field. The plan check says the same of a union's part.
pub(super) fn unbound_by(form: &str) -> String {
    format!(
        "a `bound:` field reads a value from a JSON, YAML or TOML document, and a `read:` with \
         `{form}:` reads the file as text"
    )
}

/// Decode one source's objects: its `selectors:` and `relations:` (the values
/// the document decode took aside) and its `extends:` aliases.
#[cfg(test)]
pub(crate) fn source_objects(
    source: SourceId,
    v2_keys: &[(String, Value)],
    extends: &[super::ExtendsEntry],
) -> Result<SourceObjects> {
    decode_objects(source, v2_keys, extends).map_err(|undecoded| undecoded.error)
}

/// The error a source's objects failed to decode with, and the object it is
/// about: `relations.titles`, `extends[0].as`, or `selectors` or `relations`
/// as a whole. The object's key path is written in the config as map keys,
/// which interpolation leaves as written, so it never holds a value read from
/// the environment, and a redacted error can still say where it was.
pub(crate) struct Undecoded {
    pub(crate) object: String,
    /// Where the object is in the source, for the loader to rerun the decode
    /// with its values alone masked.
    pub(crate) locus: Locus,
    pub(crate) error: Error,
}

/// Where an object is in a source's document: what the loader masks the
/// environment's values in, to rerun a decode or a plan check on. Its keys
/// are written in the config, which interpolation leaves as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Locus {
    /// The whole value of a top-level key: `selectors:` or `relations:`.
    Key(String),
    /// The definition of the selector or relation `name` under `key`.
    Entry { key: String, name: String },
    /// The `extends:` entry at this index.
    Extends(usize),
}

impl Locus {
    fn entry(key: &str, name: &str) -> Self {
        Self::Entry {
            key: key.to_string(),
            name: name.to_string(),
        }
    }

    /// The value at this locus of `document`, a source's whole document.
    pub(crate) fn find<'d>(&self, document: &'d Value) -> Option<&'d Value> {
        match self {
            Self::Key(key) => document.get(key.as_str()),
            Self::Entry { key, name } => document.get(key.as_str())?.get(name.as_str()),
            Self::Extends(index) => document.get("extends")?.get(*index),
        }
    }

    /// Where the value at this locus sits, for its interpolation: a
    /// selector's or relation's values stay text, but in a field that takes a
    /// boolean ([`Position`]).
    pub(crate) fn scope(&self) -> crate::interp::Scope {
        use crate::interp::Scope;
        match self {
            Self::Key(key) if key == "relations" => Scope::Objects(Position::Relations),
            Self::Entry { key, .. } if key == "relations" => Scope::Objects(Position::Relation),
            Self::Key(_) | Self::Entry { .. } => Scope::Objects(Position::Text),
            Self::Extends(_) => Scope::Plain,
        }
    }

    /// [`Locus::find`], to change.
    pub(crate) fn find_mut<'d>(&self, document: &'d mut Value) -> Option<&'d mut Value> {
        match self {
            Self::Key(key) => document.get_mut(key.as_str()),
            Self::Entry { key, name } => document.get_mut(key.as_str())?.get_mut(name.as_str()),
            Self::Extends(index) => document.get_mut("extends")?.get_mut(*index),
        }
    }
}

fn undecoded(object: &str, locus: Locus, error: Error) -> Box<Undecoded> {
    Box::new(Undecoded {
        object: object.to_string(),
        locus,
        error,
    })
}

/// Decode one source's objects: its `selectors:` and `relations:` (the values
/// the document decode took aside) and its `extends:` aliases, failing with
/// the object the error is about.
pub(crate) fn decode_objects(
    source: SourceId,
    v2_keys: &[(String, Value)],
    extends: &[super::ExtendsEntry],
) -> std::result::Result<SourceObjects, Box<Undecoded>> {
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
        let whole = |error| undecoded(key, Locus::Key(key.clone()), error);
        decoder.untagged(value, key).map_err(whole)?;
        let map = decoder
            .mapping(value, key, "a map from each name to its definition")
            .map_err(whole)?;
        for (name, definition) in map {
            let path = decoder.child_key(key, name, "a name").map_err(whole)?;
            let text = name.as_str().unwrap_or_default();
            let entry = |error| undecoded(&path, Locus::entry(key, text), error);
            let declared = decoder.declared_name(name, &path).and_then(|name| {
                names.declare(&decoder, &name, &path)?;
                Ok(name)
            });
            let name = declared.map_err(entry)?;
            if key == "selectors" {
                let selector = decoder.selector(definition, &path).map_err(entry)?;
                objects.selectors.push((name, selector));
            } else {
                let relation = decoder.relation(definition, &path).map_err(entry)?;
                objects.relations.push((name, relation));
            }
        }
    }
    for (index, extends) in extends.iter().enumerate() {
        let Some(alias) = &extends.alias else {
            continue;
        };
        let path = format!("extends[{index}].as");
        let about = |error| undecoded(&path, Locus::Extends(index), error);
        decoder
            .name(alias, &path)
            .and_then(|()| names.declare(&decoder, alias, &path))
            .map_err(about)?;
        let url = extends.entry.url();
        // An entry with a refused scheme gets the loader's error for that.
        if matches!(
            crate::extends::EntryKind::of(url),
            crate::extends::EntryKind::Https | crate::extends::EntryKind::Bundled(_)
        ) {
            return Err(about(decoder.error(
                &path,
                "an alias names a local `extends:` source, whose selectors and relations \
                 the config can then reach as `alias.name`; a bundled or remote source must \
                 be `version: 1` and declares none, so remove `as:` from this entry",
            )));
        }
        objects.aliases.push((
            alias.clone(),
            Alias {
                url: url.to_string(),
                entry: index,
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

/// The private key the loader puts in an inline relation that a rule or
/// template writes as its `input:` (a map), holding the name the relation was
/// declared under in its source ([`hoist_inline_inputs`]). The loader refuses
/// a `__alint_internal_*` key a config writes, so no config can forge it.
pub(crate) const INLINE_INPUT_MARKER: &str = "__alint_internal_v2_inline_input";

/// Whether `name`, a relation's name in a plan, is that of an inline `input:`
/// relation ([`hoist_inline_inputs`]): one no config can write, since every
/// declared name is a valid name ([`is_name`]).
pub(crate) fn is_inline_name(name: &str) -> bool {
    // `NamedRelation::is_inline` reads a plan's names with the same
    // predicate.
    !is_name(name)
}

/// How an error names relation `name` of a source: `relations.<name>` for a
/// named one, or where an inline `input:` relation is written
/// (`rules[3].input`).
pub(crate) fn relation_path(name: &str) -> String {
    if is_inline_name(name) {
        name.to_string()
    } else {
        format!("relations.{name}")
    }
}

/// Decode the inline relation each of a source's rules (`rules`) and
/// templates (`templates`) writes as its `input:` (a map, the `constraint`
/// kind's anonymous input), whatever its `kind:` (an override by id writes
/// none), as a relation of the source, and mark the map with the relation's
/// name ([`INLINE_INPUT_MARKER`]). The name is where the map is written
/// (`rules[3].input`, `templates[0].input`): no config can write one as a
/// name (it is not a valid name) or as a reference (a reference is a name or
/// `alias.name`), so nothing reaches it but the rule whose `input:` the
/// loader marked, wherever the rule merges or a template expands. The map
/// stays the rule's `input:`, which `list` and `explain` show; the plan holds
/// the relation, planned and checked as a named one is. An inline relation
/// is literal (`super::literal` refuses `{{...}}` in one), so it reads
/// nothing from the environment.
pub(crate) fn hoist_inline_inputs(
    objects: &mut SourceObjects,
    rules: &mut [Mapping],
    templates: &mut [Mapping],
) -> Result<()> {
    let display = objects.source.display.clone();
    let decoder = Decoder { source: &display };
    for (section, mappings) in [("rules", rules), ("templates", templates)] {
        for (index, mapping) in mappings.iter_mut().enumerate() {
            let Some(Value::Mapping(input)) = mapping.get_mut("input") else {
                continue;
            };
            let name = format!("{section}[{index}].input");
            let definition = Value::Mapping(input.clone());
            decoder.untagged(&definition, &name)?;
            let relation = decoder.relation(&definition, &name)?;
            input.insert(INLINE_INPUT_MARKER.into(), Value::from(name.as_str()));
            objects.relations.push((name, relation));
        }
    }
    Ok(())
}

/// Take the marker [`hoist_inline_inputs`] put in a rule's `input:` map off
/// it, so the rule's options show the relation as written: the name of the
/// relation the plan holds for it, or `None` when `input:` is no marked map.
pub(crate) fn take_inline_input(rule: &mut Mapping) -> Option<String> {
    let input = rule.get_mut("input")?.as_mapping_mut()?;
    input
        .remove(INLINE_INPUT_MARKER)
        .and_then(|name| name.as_str().map(str::to_string))
}

/// The name [`hoist_inline_inputs`] marked a rule's `input:` map with, if it
/// did.
pub(crate) fn inline_input(value: &Value) -> Option<&str> {
    value
        .as_mapping()?
        .get(INLINE_INPUT_MARKER)
        .and_then(Value::as_str)
}

/// Give each of `objects`' selectors and relations whose definition takes a
/// value from the environment (`reads`) the copies a plan-check error about
/// it is rerun on ([`Twins`]): its definition as `masked` gives it, with each
/// such value masked and numbers moved each way, decoded as it was. Each
/// object is decoded once more for each direction, alone, so the work is
/// linear in the objects.
pub(crate) fn decode_twins(
    objects: &mut SourceObjects,
    reads: impl Fn(&Locus) -> bool,
    masked: impl Fn(&Locus, alint_core::NumberMask) -> Option<Value>,
) {
    let display = objects.source.display.clone();
    let decoder = Decoder { source: &display };
    for (name, selector) in &mut objects.selectors {
        let locus = Locus::entry("selectors", name);
        if reads(&locus) {
            let path = format!("selectors.{name}");
            selector.twins = Twins::Masked(alint_core::NumberMask::BOTH.map(|numbers| {
                let definition = masked(&locus, numbers)?;
                decoder.selector(&definition, &path).ok().map(Box::new)
            }));
        }
    }
    for (name, relation) in &mut objects.relations {
        let locus = Locus::entry("relations", name);
        if reads(&locus) {
            let path = format!("relations.{name}");
            relation.twins = Twins::Masked(alint_core::NumberMask::BOTH.map(|numbers| {
                let definition = masked(&locus, numbers)?;
                decoder.relation(&definition, &path).ok().map(Box::new)
            }));
        }
    }
}

/// The names one source has declared so far, each with the key path that
/// declared it. Selectors, relations and `extends:` aliases share one
/// namespace per source, so `deps` cannot be both an alias and a relation.
/// Held by name: a source of many names declares each in constant time.
#[derive(Default)]
struct Names(std::collections::HashMap<String, String>);

impl Names {
    fn declare(&mut self, decoder: &Decoder<'_>, name: &str, path: &str) -> Result<()> {
        match self.0.entry(name.to_string()) {
            std::collections::hash_map::Entry::Occupied(first) => Err(decoder.error(
                path,
                format!(
                    "`{name}` is already declared at `{}`; a name or alias is declared once in \
                     a config (selectors, relations and `extends:` aliases share one \
                     namespace), so rename one of them",
                    first.get()
                ),
            )),
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(path.to_string());
                Ok(())
            }
        }
    }
}

// Whether a text is a valid name: an ASCII letter or `_`, then ASCII
// letters, digits, `_` and `-`. It is the plan's own predicate, by which
// `NamedRelation::is_inline` tells an inline `input:` relation, so a declared
// name is never read as an inline one.
pub(crate) use alint_core::relation::plan::is_name;

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
            twins: Twins::Unneeded,
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
        let spec = crate::core_yaml::decode::<alint_core::ScopeFilterSpec>(value.clone())
            .map_err(|error| self.error(path, error))?;
        // What a rule's filter may also hold (`changed_since:`) is refused
        // above, so these are a selector's own words for an empty filter.
        if spec.has_ancestor.is_none()
            && spec.include_manifest_paths.is_none()
            && spec.exclude_manifest_paths.is_none()
        {
            return Err(self.error(
                path,
                "a selector's `scope_filter:` sets at least one of `has_ancestor:`, \
                 `include_manifest_paths:` and `exclude_manifest_paths:`; set one, or remove \
                 `scope_filter:`",
            ));
        }
        match spec.has_ancestor.as_deref() {
            Some([]) => Err(self.error(
                &format!("{path}.has_ancestor"),
                "`has_ancestor:` lists no file name; list at least one, as in `has_ancestor: \
                 package.json`, or remove it",
            )),
            Some(names) if names.iter().any(String::is_empty) => Err(self.error(
                &format!("{path}.has_ancestor"),
                "`has_ancestor:` names a file with an empty string; write the file's name, as \
                 in `has_ancestor: package.json`",
            )),
            _ => Ok(spec),
        }
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
            twins: Twins::Unneeded,
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
            (
                Rows::Part(Part {
                    reader: Reader::Read { read, .. },
                    ..
                }),
                FieldSource::Bound(_),
            ) => {
                if let Some(form) = unstructured_form(read) {
                    return Err(self.error(
                        path,
                        format!(
                            "{}; read the file with `entries:`, or with `read:` and `json:`, \
                             `yaml:` or `toml:`",
                            unbound_by(form)
                        ),
                    ));
                }
                None
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
