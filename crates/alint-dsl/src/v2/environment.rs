//! Which parts of a version 2 source's selectors and relations take a value
//! from the environment (`{{env.X}}`), read off the raw document before
//! interpolation erases the difference, as the loader marks a rule's fields
//! (`SENSITIVE_FIELDS`). Names and references stay literal in a version 2
//! source (ADR-0037 decision 3), so only values can: a `where:` literal or
//! pattern, a transform, an `at:` or `read:` path, a selector's globs.
//!
//! - A field whose `where:` or `transform:` entry reads the environment is
//!   sensitive: its values never reach a message, the JSON report or a
//!   baseline, nor do the literals it is compared with.
//! - Any other part of a relation's definition, or of a named selector, that
//!   reads it makes the relation's analysis failures show no detail: an
//!   `at:` path from the environment would echo in the key path it selects.

use std::collections::{HashMap, HashSet};

use ayaml::Value;

use super::decode::Locus;
use super::model::SourceObjects;

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
            for (field, definition) in &mut relation.fields {
                if environment.fields.contains(field) {
                    definition.sensitive = true;
                }
            }
        }
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
