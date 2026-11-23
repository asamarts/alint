//! The check of a structured edit against the whole document: re-read the
//! edited bytes and require the original tree with exactly the intended
//! changes, so an edit that also changed a node it was not aimed at declines.
//!
//! The engine's per-edit verifier ([`EditVerifier::Structured`]) re-runs one
//! query, so it sees the target and nothing else. A YAML line removal can
//! change a neighbour that still parses: the blank line after the removed one
//! joins a block scalar kept with `|+`, an explicit `?` key takes the next
//! entry's value, an emptied block mapping reads as null. The span resolvers
//! decline the shapes they know; this check catches the rest, for the formats
//! whose tree is the whole of what the rules read ([`checks_whole_document`]).
//!
//! [`EditVerifier::Structured`]: crate::rule::EditVerifier::Structured

use super::{PathSeg, resolve_value_spans, serialize_scalar};
use crate::structured_format::Format;
use serde_json::Value;
use std::cmp::Ordering;
use std::ops::Range;

/// What a structured edit means to do at one path.
#[derive(Debug, Clone, PartialEq)]
pub enum Intent {
    /// Make the node at the path this value.
    Set(Value),
    /// Remove the node at the path from its mapping or sequence.
    Remove,
}

/// Whether an edit in `format` is checked against the whole document: YAML,
/// JSON, TOML, and `.properties`, whose tree is what the structured rules
/// query. The other formats' trees fold their source (XML's repeated elements
/// become an array, one of them an object; INI folds duplicates into arrays),
/// so a correct edit can change a node's shape.
#[must_use]
pub fn checks_whole_document(format: Format) -> bool {
    matches!(
        format,
        Format::Yaml | Format::Json | Format::Toml | Format::Properties
    )
}

/// `before` with each of `intents` applied: the sets first, then the
/// removals from the last path back, so a removal from a sequence shifts
/// no path still to come. `None` when a set's path does not lead to a node; a
/// removal whose node is already gone (a repeated path) changes nothing.
#[must_use]
pub fn intended(before: &Value, intents: &[(Vec<PathSeg>, Intent)]) -> Option<Value> {
    let mut after = before.clone();
    for (path, intent) in intents {
        if let Intent::Set(value) = intent {
            *node_mut(&mut after, path)? = value.clone();
        }
    }
    let mut removals: Vec<&[PathSeg]> = intents
        .iter()
        .filter(|(_, intent)| *intent == Intent::Remove)
        .map(|(path, _)| path.as_slice())
        .collect();
    removals.sort_by(|a, b| compare_paths(b, a));
    removals.dedup();
    for path in removals {
        let (last, parents) = path.split_last()?;
        match (node_mut(&mut after, parents), last) {
            (Some(Value::Object(members)), PathSeg::Key(key)) => {
                members.remove(key);
            }
            (Some(Value::Array(elements)), PathSeg::Index(i)) if *i < elements.len() => {
                elements.remove(*i);
            }
            _ => {}
        }
    }
    Some(after)
}

/// Whether `after`, read as `format`, is `before` with exactly `intents`
/// applied ([`intended`]), and nothing else changed.
#[must_use]
pub fn is_exact_edit(
    format: Format,
    before: &Value,
    after: &[u8],
    intents: &[(Vec<PathSeg>, Intent)],
) -> bool {
    let Some(want) = intended(before, intents) else {
        return false;
    };
    std::str::from_utf8(after)
        .ok()
        .and_then(|text| format.parse(text).ok())
        .is_some_and(|read| read == want)
}

/// The edits `set_value` and `replace` make: for each `(path, value)`, the
/// range of the scalar at `path` ([`super::resolve_value_spans`]) and `value`
/// serialized for `format`, `None` where either is missing. For a format
/// checked against the whole document, every edit is `None` unless the edits
/// together leave `before`, the tree of `bytes`, changed exactly at their
/// paths, to their values ([`is_exact_edit`]).
#[must_use]
pub fn checked_value_edits(
    format: Format,
    bytes: &[u8],
    before: &Value,
    sets: &[(Vec<PathSeg>, Value)],
) -> Vec<Option<(Range<usize>, Vec<u8>)>> {
    let edits: Vec<Option<(Range<usize>, Vec<u8>)>> =
        if let (Format::Properties, Ok(text)) = (format, std::str::from_utf8(bytes)) {
            super::properties_value_edits(text, sets)
        } else {
            let paths: Vec<Vec<PathSeg>> = sets.iter().map(|(path, _)| path.clone()).collect();
            resolve_value_spans(format, bytes, &paths)
                .into_iter()
                .zip(sets)
                .map(|(span, (_, value))| Some((span?, serialize_scalar(format, value)?)))
                .collect()
        };
    let intents: Vec<_> = sets
        .iter()
        .zip(&edits)
        .filter(|(_, edit)| edit.is_some())
        .map(|((path, value), _)| (path.clone(), Intent::Set(value.clone())))
        .collect();
    checked(format, bytes, before, edits, &intents)
}

/// The edits `remove_value` makes for each of `paths`
/// ([`super::resolve_removal_edits`]), checked as [`checked_value_edits`]
/// checks its own: every edit is `None` unless together they remove exactly
/// the nodes at their paths.
#[must_use]
pub fn checked_removal_edits(
    format: Format,
    bytes: &[u8],
    before: &Value,
    paths: &[Vec<PathSeg>],
) -> Vec<Option<(Range<usize>, Vec<u8>)>> {
    let edits = super::resolve_removal_edits(format, bytes, paths);
    let intents: Vec<_> = paths
        .iter()
        .zip(&edits)
        .filter(|(_, edit)| edit.is_some())
        .map(|(path, _)| (path.clone(), Intent::Remove))
        .collect();
    checked(format, bytes, before, edits, &intents)
}

/// `edits`, or none of them when `format` is checked against the whole
/// document and the edits spliced into `bytes` are not exactly `intents`.
fn checked(
    format: Format,
    bytes: &[u8],
    before: &Value,
    edits: Vec<Option<(Range<usize>, Vec<u8>)>>,
    intents: &[(Vec<PathSeg>, Intent)],
) -> Vec<Option<(Range<usize>, Vec<u8>)>> {
    if !checks_whole_document(format) || intents.is_empty() {
        return edits;
    }
    let exact = splice(bytes, edits.iter().flatten())
        .is_some_and(|after| is_exact_edit(format, before, &after, intents));
    if exact {
        edits
    } else {
        vec![None; edits.len()]
    }
}

/// `bytes` with each edit's range replaced by its content. An edit repeated
/// (the same range and content, from a path a query selected twice) applies
/// once; `None` when two edits otherwise overlap or one is out of range.
pub(crate) fn splice<'e>(
    bytes: &[u8],
    edits: impl IntoIterator<Item = &'e (Range<usize>, Vec<u8>)>,
) -> Option<Vec<u8>> {
    let mut edits: Vec<&(Range<usize>, Vec<u8>)> = edits.into_iter().collect();
    edits.sort_by_key(|(range, _)| (range.start, range.end));
    edits.dedup();
    let mut out = Vec::with_capacity(bytes.len());
    let mut copied = 0;
    for (range, content) in edits {
        if range.start < copied || range.start > range.end || range.end > bytes.len() {
            return None;
        }
        out.extend_from_slice(&bytes[copied..range.start]);
        out.extend_from_slice(content);
        copied = range.end;
    }
    out.extend_from_slice(&bytes[copied..]);
    Some(out)
}

/// The node at `path` in `value`.
fn node_mut<'v>(value: &'v mut Value, path: &[PathSeg]) -> Option<&'v mut Value> {
    path.iter().try_fold(value, |node, seg| match (node, seg) {
        (Value::Object(members), PathSeg::Key(key)) => members.get_mut(key),
        (Value::Array(elements), PathSeg::Index(i)) => elements.get_mut(*i),
        _ => None,
    })
}

/// An order of paths, segment by segment, in which a sequence's elements come
/// by index and a path comes before the paths under it.
fn compare_paths(a: &[PathSeg], b: &[PathSeg]) -> Ordering {
    for (a, b) in a.iter().zip(b) {
        let order = match (a, b) {
            (PathSeg::Key(a), PathSeg::Key(b)) => a.cmp(b),
            (PathSeg::Index(a), PathSeg::Index(b)) => a.cmp(b),
            (PathSeg::Key(_), PathSeg::Index(_)) => Ordering::Less,
            (PathSeg::Index(_), PathSeg::Key(_)) => Ordering::Greater,
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
#[path = "exact_tests.rs"]
mod tests;
