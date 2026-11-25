//! Which parts of a version 2 source's selectors and relations take a value
//! from the environment (`{{env.X}}`), read off the raw document before
//! interpolation erases the difference, as the loader marks a rule's fields
//! (`SENSITIVE_FIELDS`). Names and references stay literal in a version 2
//! source (ADR-0037 decision 3), so only values can: a `where:` literal or
//! pattern, a transform, an `at:` or `read:` path, a selector's globs.
//!
//! - A field whose `where:` or `transform:` entry reads the environment is
//!   sensitive: its values never reach a message, the JSON report or a
//!   baseline, nor do the literals it is compared with. So is every field
//!   that shows its value ([`family`]): a filter on an entry's key would
//!   echo in its path, its parent's and its root.
//! - Any part of a relation's definition, or of a named selector it reads,
//!   that reads it makes the relation's analysis failures show no detail
//!   (an `at:` path from the environment would echo in the key path it
//!   selects), and a rule's findings over it show no path: the rows the
//!   environment selects, through a `where:` or `transform:` entry too, say
//!   where its value occurs. `explain` hides the values of a definition that
//!   reads it outside those entries; inside them, the fields' values only.

use std::collections::{HashMap, HashSet};

use ayaml::Value;

use super::decode::Locus;
use super::model::{Field, FieldSource, JoinSide, Rows, SourceObjects};

/// What one source's selectors and relations read from the environment, by
/// name, so marking a source of many objects costs one lookup per object.
#[derive(Debug, Clone, Default)]
pub(crate) struct Environment {
    selectors: HashSet<String>,
    relations: HashMap<String, RelationEnvironment>,
}

#[derive(Debug, Clone, Default)]
struct RelationEnvironment {
    /// Whether its definition reads the environment outside its fields'
    /// `where:` and `transform:` entries.
    reading: bool,
    /// The fields whose `where:` or `transform:` entry reads it.
    fields: HashSet<String>,
}

impl Environment {
    /// Read `document`, a source's raw (uninterpolated) YAML.
    pub(crate) fn of(document: &Value) -> Self {
        let mut environment = Self::default();
        let named = |key: &str| {
            document
                .get(key)
                .and_then(Value::as_mapping)
                .into_iter()
                .flatten()
                .filter_map(|(name, definition)| Some((name.as_str()?, definition)))
        };
        for (name, definition) in named("selectors") {
            if reads_environment(definition) {
                environment.selectors.insert(name.to_string());
            }
        }
        for (name, definition) in named("relations") {
            let mut relation = RelationEnvironment::default();
            for (key, value) in definition.as_mapping().into_iter().flatten() {
                match key.as_str() {
                    Some("where" | "transform") => {
                        for (field, entry) in value.as_mapping().into_iter().flatten() {
                            if reads_environment(entry)
                                && let Some(field) = field.as_str()
                            {
                                relation.fields.insert(field.to_string());
                            }
                        }
                    }
                    _ => relation.reading |= reads_environment(value),
                }
            }
            if relation.reading || !relation.fields.is_empty() {
                environment.relations.insert(name.to_string(), relation);
            }
        }
        environment
    }

    /// Whether the definition at `locus` (a selector or relation) takes any
    /// value from the environment.
    pub(crate) fn reads(&self, locus: &Locus) -> bool {
        match locus {
            Locus::Entry { key, name } if key == "selectors" => self.selectors.contains(name),
            Locus::Entry { key, name } if key == "relations" => self.relations.contains_key(name),
            _ => false,
        }
    }

    /// Mark `objects`, the source's decoded selectors and relations.
    pub(crate) fn mark(&self, objects: &mut SourceObjects) {
        for (name, selector) in &mut objects.selectors {
            selector.reads_environment = self.selectors.contains(name);
        }
        for (name, relation) in &mut objects.relations {
            let Some(environment) = self.relations.get(name) else {
                continue;
            };
            relation.reads_environment = environment.reading;
            relation.filters_read_environment = !environment.fields.is_empty();
            let mut sensitive_names: HashSet<String> = environment.fields.clone();
            if let Rows::Union(parts) = &mut relation.rows
                && parts.iter().any(|part| part.fields.is_some())
            {
                // Propagate environment-driven sensitivity across every part's
                // source families until fixpoint, then sync canonical `fields`.
                loop {
                    let before = sensitive_names.len();
                    for part in parts.iter() {
                        if let Some(fields) = &part.fields {
                            propagate_field_family(fields, &mut sensitive_names);
                        }
                    }
                    if sensitive_names.len() == before {
                        break;
                    }
                }
                for part in parts.iter_mut() {
                    if let Some(fields) = &mut part.fields {
                        for (field, definition) in fields.iter_mut() {
                            definition.sensitive |= sensitive_names.contains(field);
                        }
                    }
                }
                for (field, definition) in &mut relation.fields {
                    definition.sensitive |= sensitive_names.contains(field);
                }
            } else {
                propagate_field_family(&relation.fields, &mut sensitive_names);
                for (field, definition) in &mut relation.fields {
                    definition.sensitive |= sensitive_names.contains(field);
                }
            }
        }
    }
}

fn propagate_field_family(fields: &[(String, Field)], sensitive_names: &mut HashSet<String>) {
    let marked: HashSet<Family<'_>> = fields
        .iter()
        .filter(|(field, _)| sensitive_names.contains(field))
        .map(|(_, definition)| family(&definition.source))
        .collect();
    for (field, definition) in fields {
        if marked.contains(&family(&definition.source)) {
            sensitive_names.insert(field.clone());
        }
    }
}

/// The fields whose values show one another's, so that one the environment
/// filters makes all of its family sensitive: an entry's key, path, parent,
/// root and value (a key is in its path, its parent's and its root, and a
/// value is read at its key); a resource's path and its tokens; and fields
/// bound to one value, one `bound:` path, one `record:` path or one side's
/// field.
#[derive(Debug, PartialEq, Eq, Hash)]
enum Family<'a> {
    Entry,
    Path,
    Value,
    Bound(&'a str),
    Record(&'a str),
    Side(bool, &'a str),
}

fn family(source: &FieldSource) -> Family<'_> {
    match source {
        FieldSource::EntryKey
        | FieldSource::EntryValue
        | FieldSource::EntryParent
        | FieldSource::EntryRoot
        | FieldSource::EntryPath => Family::Entry,
        FieldSource::Path(_) => Family::Path,
        FieldSource::Value => Family::Value,
        FieldSource::Bound { query, .. } => Family::Bound(query),
        FieldSource::Record { query, .. } => Family::Record(query),
        FieldSource::Side(side, field) => Family::Side(*side == JoinSide::Left, field),
    }
}

fn reads_environment(value: &Value) -> bool {
    match value {
        Value::String(text) => crate::interp::reads_env(text),
        Value::Sequence(values) => values.iter().any(reads_environment),
        Value::Mapping(mapping) => mapping.values().any(reads_environment),
        _ => false,
    }
}
