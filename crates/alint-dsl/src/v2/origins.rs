//! Which source wrote each field of a rule, so a field that names a relation
//! resolves in that source's namespace (design section 10: "a rule that a
//! consumer overrides by id resolves each overridden field in the source
//! that wrote it").
//!
//! The loader's field-source marker (`FIELD_SOURCES_MARKER`) cannot answer
//! this: it names the immediate `extends:` entry for an error message, leaves
//! the user's own fields unattributed, and an `extends:` chain overwrites it
//! with the nearer entry. This marker records the file that wrote each
//! top-level field of a rule or template of a version 2 source, follows the
//! same last-wins field merge and template expansion, and is taken off before
//! the rule is deserialized. A version 1 source writes no origin, so a field
//! it overrides loses the one it had: a version 1 field names no relation.
//!
//! A source is named by its index among the files the configuration was
//! loaded from (`Config::config_files`, in load order), not by its path: every
//! field of every rule carries the marker, and a copy of a deep path in each
//! cost several times the config.

use ayaml::{Mapping, Value};

/// The private marker: a mapping from each top-level field to the index,
/// among the configuration's files, of the version 2 source that wrote it.
pub(crate) const FIELD_ORIGINS_MARKER: &str = "__alint_internal_v2_field_origins";

fn is_internal(key: &Value) -> bool {
    key.as_str()
        .is_some_and(|key| key.starts_with("__alint_internal_"))
}

/// Mark every field of each of `mappings` (the rules or templates of the
/// version 2 source that is the configuration's file `source`) as written by
/// it.
pub(crate) fn mark_field_origins(mappings: &mut [Mapping], source: usize) {
    for mapping in mappings {
        let mut origins = Mapping::new();
        for field in mapping.keys().filter(|field| !is_internal(field)) {
            origins.insert(field.clone(), Value::from(source));
        }
        if !origins.is_empty() {
            mapping.insert(FIELD_ORIGINS_MARKER.into(), origins.into());
        }
    }
}

/// Carry the origins of `incoming`'s fields onto `existing`, which takes
/// those fields last-wins: a field `incoming` writes takes its origin there,
/// or loses its origin when `incoming` has none for it (a version 1 source
/// wrote it). A mapping neither side marked stays unmarked.
pub(crate) fn merge_field_origins(existing: &mut Mapping, incoming: &Mapping) {
    let theirs = incoming
        .get(FIELD_ORIGINS_MARKER)
        .and_then(Value::as_mapping);
    let ours = existing.remove(FIELD_ORIGINS_MARKER);
    if theirs.is_none() && ours.is_none() {
        return;
    }
    let mut merged = ours
        .and_then(|value| value.as_mapping().cloned())
        .unwrap_or_default();
    for field in incoming.keys().filter(|field| !is_internal(field)) {
        match theirs.and_then(|origins| origins.get(field)) {
            Some(origin) => {
                merged.insert(field.clone(), origin.clone());
            }
            None => {
                merged.remove(field);
            }
        }
    }
    if !merged.is_empty() {
        existing.insert(FIELD_ORIGINS_MARKER.into(), merged.into());
    }
}

/// Take the marker off `mapping`: the index, among the configuration's
/// files, of the file that wrote each field it records.
pub(crate) fn take_field_origins(mapping: &mut Mapping) -> Vec<(String, usize)> {
    mapping
        .remove(FIELD_ORIGINS_MARKER)
        .and_then(|value| value.as_mapping().cloned())
        .into_iter()
        .flatten()
        .filter_map(|(field, origin)| {
            let origin = usize::try_from(origin.as_u64()?).ok()?;
            Some((field.as_str()?.to_string(), origin))
        })
        .collect()
}

/// The top-level fields of a rule of `kind` that name a relation, bare or
/// qualified. No rule kind of this build has one: the `constraint` kind
/// (phase 7) adds `input:` and the relations its `assert:` names. The unit
/// tests register a stub kind with one, `input:`.
pub(crate) fn reference_fields(kind: &str) -> &'static [&'static str] {
    #[cfg(test)]
    if kind == REFERENCE_STUB_KIND {
        return &["input"];
    }
    let _ = kind;
    &[]
}

/// The rule kind the unit tests give a reference field.
#[cfg(test)]
pub(crate) const REFERENCE_STUB_KIND: &str = "v2_reference_stub";

#[cfg(test)]
#[path = "origins_tests.rs"]
mod tests;
