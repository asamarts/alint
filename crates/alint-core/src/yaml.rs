//! YAML for alint: the entry points every alint crate decodes through, and the
//! value and error types alint's public API uses.
//!
//! Every document is read under the same parser limits: ayaml's defaults,
//! without its caps on events, collection entries, anchors and alias uses. The
//! input cap (32 MiB), the depth cap (128) and the alias-expansion allowance
//! stay.
//!
//! [`Value`], [`Mapping`], [`Sequence`], [`Error`] and [`Location`] are alint's
//! own types. Each wraps the parser's, which stays private, so a change of
//! parser does not change alint's API (ADR-0038).

use std::fmt;

use ref_cast::{RefCastCustom, ref_cast_custom};
use serde::{Deserialize, Serialize};

/// The parser limits every decode runs under: ayaml's defaults, without its
/// caps on events, collection entries, anchors and alias uses. The parser
/// v0.22 used had none of those caps, and ayaml's default of 1,000,000 events
/// refuses a valid file of a few megabytes, far below the input cap. The
/// input cap (32 MiB, as [`crate::MAX_STRUCTURED_BYTES`]), the depth cap
/// (128) and the alias-expansion budget stay, and together they bound what
/// one parse can cost.
fn limits() -> ayaml::Limits {
    ayaml::Limits::default()
        .with_max_events(usize::MAX)
        .with_max_collection_entries(usize::MAX)
        .with_max_anchors(usize::MAX)
        .with_max_alias_dereferences(usize::MAX)
}

/// Decode one YAML document into `T`, under alint's parser limits.
///
/// # Errors
///
/// When the input is not one valid YAML document, exceeds a limit, or does
/// not decode into `T`.
pub fn from_str<'de, T: Deserialize<'de>>(input: &'de str) -> Result<T, Error> {
    ayaml::compat::from_str_with_limits(input, &limits()).map_err(Error)
}

/// Decode one YAML document through `seed`, under alint's parser limits.
pub(crate) fn from_str_seed<'de, S: serde::de::DeserializeSeed<'de>>(
    input: &'de str,
    seed: S,
) -> Result<S::Value, ayaml::Error> {
    ayaml::compat::from_str_seed_with_limits(input, &limits(), seed)
}

/// Decode `value` into `T`.
///
/// # Errors
///
/// When `value` does not decode into `T`.
pub fn from_value<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, Error> {
    ayaml::from_value(value.0).map_err(Error)
}

/// The YAML value of `value`.
///
/// # Errors
///
/// When `value` cannot be represented in YAML.
pub fn to_value<T: Serialize + ?Sized>(value: &T) -> Result<Value, Error> {
    ayaml::to_value(value).map(Value).map_err(Error)
}

/// `value` written as a YAML document.
///
/// # Errors
///
/// When `value` cannot be represented in YAML.
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> Result<String, Error> {
    ayaml::to_string(value).map_err(Error)
}

/// A YAML value: null, a boolean, a number, a string, a [`Sequence`], a
/// [`Mapping`], or one of those with a tag.
///
/// The accessors look through a tag, so `!Celsius 21` is a number. A value
/// prints with `{:?}` as the parser's value does.
#[derive(Clone, PartialEq, Eq, Hash, RefCastCustom)]
#[repr(transparent)]
pub struct Value(ayaml::Value);

/// What [`Mapping`]'s and [`Value`]'s indexing return for a missing key.
static NULL: Value = Value(ayaml::Value::Null);

impl Value {
    #[ref_cast_custom]
    pub(crate) fn from_parser_ref(value: &ayaml::Value) -> &Self;

    #[ref_cast_custom]
    pub(crate) fn from_parser_mut(value: &mut ayaml::Value) -> &mut Self;

    #[cfg(test)]
    pub(crate) fn from_parser(value: ayaml::Value) -> Self {
        Self(value)
    }

    pub(crate) fn into_parser(self) -> ayaml::Value {
        self.0
    }

    pub(crate) fn parser(&self) -> &ayaml::Value {
        &self.0
    }

    pub(crate) fn parser_mut(&mut self) -> &mut ayaml::Value {
        &mut self.0
    }

    /// Whether this is null.
    #[must_use]
    pub fn is_null(&self) -> bool {
        self.0.is_null()
    }

    /// The boolean, if this is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        self.0.as_bool()
    }

    /// The number as an `i64`, if this is an integer in its range.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        self.0.as_i64()
    }

    /// The number as a `u64`, if this is an integer in its range.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        self.0.as_u64()
    }

    /// The number as an `f64`, if this is a number.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        self.0.as_f64()
    }

    /// The string, if this is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        self.0.as_str()
    }

    /// The sequence, if this is one.
    #[must_use]
    pub fn as_sequence(&self) -> Option<&Sequence> {
        self.0.as_sequence().map(Sequence::from_parser_ref)
    }

    /// The sequence, mutably, if this is one.
    #[must_use]
    pub fn as_sequence_mut(&mut self) -> Option<&mut Sequence> {
        self.0.as_sequence_mut().map(Sequence::from_parser_mut)
    }

    /// The mapping, if this is one.
    #[must_use]
    pub fn as_mapping(&self) -> Option<&Mapping> {
        self.0.as_mapping().map(Mapping::from_parser_ref)
    }

    /// The mapping, mutably, if this is one.
    #[must_use]
    pub fn as_mapping_mut(&mut self) -> Option<&mut Mapping> {
        self.0.as_mapping_mut().map(Mapping::from_parser_mut)
    }

    /// The value under `key`, if this is a mapping that has one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        self.as_mapping().and_then(|mapping| mapping.get(key))
    }
}

/// Null.
impl Default for Value {
    fn default() -> Self {
        Self(ayaml::Value::Null)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ayaml::Value::deserialize(deserializer).map(Self)
    }
}

/// `value[key]` is the value under `key`, or null when this is not a mapping
/// or has no such key.
impl std::ops::Index<&str> for Value {
    type Output = Self;

    fn index(&self, key: &str) -> &Self {
        self.get(key).unwrap_or(&NULL)
    }
}

/// `value[index]` is the value at `index`, or null when this is not a sequence
/// or has no such index.
impl std::ops::Index<usize> for Value {
    type Output = Self;

    fn index(&self, index: usize) -> &Self {
        self.as_sequence()
            .and_then(|sequence| sequence.get(index))
            .unwrap_or(&NULL)
    }
}

macro_rules! value_from {
    ($($from:ty),* $(,)?) => {$(
        impl From<$from> for Value {
            fn from(value: $from) -> Self {
                Self(ayaml::Value::from(value))
            }
        }
    )*};
}

value_from!(bool, i64, u64, f64, String, &str);

impl From<Mapping> for Value {
    fn from(mapping: Mapping) -> Self {
        Self(ayaml::Value::Mapping(mapping.0))
    }
}

impl From<Sequence> for Value {
    fn from(sequence: Sequence) -> Self {
        Self(ayaml::Value::Sequence(sequence.0))
    }
}

impl From<Vec<Value>> for Value {
    fn from(values: Vec<Value>) -> Self {
        Self::from(values.into_iter().collect::<Sequence>())
    }
}

macro_rules! value_eq {
    ($($other:ty),* $(,)?) => {$(
        impl PartialEq<$other> for Value {
            fn eq(&self, other: &$other) -> bool {
                self.0 == *other
            }
        }
    )*};
}

value_eq!(str, &str, String, bool, i64, u64);

impl PartialEq<i32> for Value {
    fn eq(&self, other: &i32) -> bool {
        self.0 == i64::from(*other)
    }
}

/// A YAML sequence: values in order.
#[derive(Clone, Default, PartialEq, Eq, Hash, RefCastCustom)]
#[repr(transparent)]
pub struct Sequence(Vec<ayaml::Value>);

impl Sequence {
    #[ref_cast_custom]
    fn from_parser_ref(values: &Vec<ayaml::Value>) -> &Self;

    #[ref_cast_custom]
    fn from_parser_mut(values: &mut Vec<ayaml::Value>) -> &mut Self;

    /// An empty sequence.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many values it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it holds no values.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The value at `index`, if there is one.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Value> {
        self.0.get(index).map(Value::from_parser_ref)
    }

    /// Add `value` at the end.
    pub fn push(&mut self, value: impl Into<Value>) {
        self.0.push(value.into().0);
    }

    /// The values, in order.
    pub fn iter(&self) -> SequenceIter<'_> {
        SequenceIter(self.0.iter())
    }

    /// The values, mutably, in order.
    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = &mut Value> + DoubleEndedIterator {
        self.0.iter_mut().map(Value::from_parser_mut)
    }
}

impl fmt::Debug for Sequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Sequence {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Sequence {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<ayaml::Value>::deserialize(deserializer).map(Self)
    }
}

impl FromIterator<Value> for Sequence {
    fn from_iter<I: IntoIterator<Item = Value>>(values: I) -> Self {
        Self(values.into_iter().map(Value::into_parser).collect())
    }
}

impl<'a> IntoIterator for &'a Sequence {
    type Item = &'a Value;
    type IntoIter = SequenceIter<'a>;

    fn into_iter(self) -> SequenceIter<'a> {
        self.iter()
    }
}

/// The values of a [`Sequence`], in order.
#[derive(Clone, Debug)]
pub struct SequenceIter<'a>(std::slice::Iter<'a, ayaml::Value>);

impl<'a> Iterator for SequenceIter<'a> {
    type Item = &'a Value;

    fn next(&mut self) -> Option<&'a Value> {
        self.0.next().map(Value::from_parser_ref)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for SequenceIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(Value::from_parser_ref)
    }
}

impl ExactSizeIterator for SequenceIter<'_> {}

/// A YAML mapping: keys and their values, in the order they were written or
/// inserted.
#[derive(Clone, Default, PartialEq, Eq, Hash, RefCastCustom)]
#[repr(transparent)]
pub struct Mapping(ayaml::Mapping);

impl Mapping {
    #[ref_cast_custom]
    pub(crate) fn from_parser_ref(mapping: &ayaml::Mapping) -> &Self;

    #[ref_cast_custom]
    pub(crate) fn from_parser_mut(mapping: &mut ayaml::Mapping) -> &mut Self;

    pub(crate) fn from_parser(mapping: ayaml::Mapping) -> Self {
        Self(mapping)
    }

    pub(crate) fn parser(&self) -> &ayaml::Mapping {
        &self.0
    }

    /// An empty mapping.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many keys it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it holds no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether it holds the string key `key`.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.0.get_str(key).is_some()
    }

    /// The value under the string key `key`, if there is one.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get_str(key).map(Value::from_parser_ref)
    }

    /// The value under the string key `key`, mutably, if there is one.
    #[must_use]
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.get_str_mut(key).map(Value::from_parser_mut)
    }

    /// Set `key` to `value`, returning the value it replaced. A new key goes
    /// last; a replaced one keeps its place.
    pub fn insert(&mut self, key: impl Into<Value>, value: impl Into<Value>) -> Option<Value> {
        self.0.insert(key.into().0, value.into().0).map(Value)
    }

    /// Remove the string key `key`, returning its value. The keys after it
    /// keep their order.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.shift_remove(key).map(Value)
    }

    /// The keys and their values, in order.
    pub fn iter(&self) -> MappingIter<'_> {
        MappingIter((&self.0).into_iter())
    }

    /// The keys and their values, mutably, in order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&Value, &mut Value)> {
        self.0
            .iter_mut()
            .map(|(key, value)| (Value::from_parser_ref(key), Value::from_parser_mut(value)))
    }

    /// The keys, in order.
    pub fn keys(&self) -> impl Iterator<Item = &Value> {
        self.iter().map(|(key, _)| key)
    }

    /// The values, in order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.iter().map(|(_, value)| value)
    }
}

impl fmt::Debug for Mapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for Mapping {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Mapping {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ayaml::Mapping::deserialize(deserializer).map(Self)
    }
}

/// `mapping[key]` is the value under `key`, or null when there is none.
impl std::ops::Index<&str> for Mapping {
    type Output = Value;

    fn index(&self, key: &str) -> &Value {
        self.get(key).unwrap_or(&NULL)
    }
}

impl FromIterator<(Value, Value)> for Mapping {
    fn from_iter<I: IntoIterator<Item = (Value, Value)>>(entries: I) -> Self {
        Self(
            entries
                .into_iter()
                .map(|(key, value)| (key.0, value.0))
                .collect(),
        )
    }
}

impl<'a> IntoIterator for &'a Mapping {
    type Item = (&'a Value, &'a Value);
    type IntoIter = MappingIter<'a>;

    fn into_iter(self) -> MappingIter<'a> {
        self.iter()
    }
}

/// The keys of a [`Mapping`] and their values, in order.
#[derive(Clone, Debug)]
pub struct MappingIter<'a>(<&'a ayaml::Mapping as IntoIterator>::IntoIter);

impl<'a> Iterator for MappingIter<'a> {
    type Item = (&'a Value, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0
            .next()
            .map(|(key, value)| (Value::from_parser_ref(key), Value::from_parser_ref(value)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl DoubleEndedIterator for MappingIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0
            .next_back()
            .map(|(key, value)| (Value::from_parser_ref(key), Value::from_parser_ref(value)))
    }
}

impl ExactSizeIterator for MappingIter<'_> {}

/// Why a YAML document did not parse, or a value did not decode or encode.
///
/// It prints a message and, where there is one, the location.
pub struct Error(ayaml::Error);

impl Error {
    pub(crate) fn from_parser(error: ayaml::Error) -> Self {
        Self(error)
    }

    pub(crate) fn into_parser(self) -> ayaml::Error {
        self.0
    }

    /// The message, without the location.
    #[must_use]
    pub fn message(&self) -> &str {
        self.0.message()
    }

    /// Where in the document the error is, if it has a place there.
    #[must_use]
    pub fn location(&self) -> Option<Location> {
        self.0.location().map(|location| Location {
            line: location.line(),
            column: location.column(),
        })
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.source()
    }
}

/// An error a [`Deserialize`] or [`Serialize`] implementation reports with
/// this message.
impl serde::de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(ayaml::Error::new(
            ayaml::ErrorKind::Serde,
            message.to_string(),
        ))
    }
}

impl serde::ser::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self(ayaml::Error::new(
            ayaml::ErrorKind::Serde,
            message.to_string(),
        ))
    }
}

/// A place in a YAML document: a line and a column, both counted from one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Location {
    line: usize,
    column: usize,
}

impl Location {
    /// The line, counted from one.
    #[must_use]
    pub fn line(self) -> usize {
        self.line
    }

    /// The column, counted from one.
    #[must_use]
    pub fn column(self) -> usize {
        self.column
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    /// A valid document with more events, entries, anchors and alias uses
    /// than ayaml's defaults allow (1,000,000, 1,000,000, 100,000 and
    /// 100,000) decodes, as it did in v0.22. Each count sits just past its
    /// default, so dropping any one override fails here.
    #[test]
    fn a_large_valid_document_decodes() {
        // 1,000,001 entries in one flow sequence: past the entries default
        // and, with the sequence's own events, the events default.
        let mut entries = String::with_capacity(2_100_000);
        entries.push('[');
        for i in 0..1_000_001 {
            if i > 0 {
                entries.push(',');
            }
            entries.push('0');
        }
        entries.push(']');
        let decoded: Vec<u8> = from_str(&entries).unwrap();
        assert_eq!(decoded.len(), 1_000_001);

        // 100,001 anchors, and 100,001 uses of one of them.
        let mut anchors = String::with_capacity(4_000_000);
        anchors.push_str("a: &x 0\nanchored: [");
        for i in 0..100_001 {
            if i > 0 {
                anchors.push(',');
            }
            write!(anchors, "&a{i} 0").unwrap();
        }
        anchors.push_str("]\nused: [");
        for i in 0..100_001 {
            if i > 0 {
                anchors.push(',');
            }
            anchors.push_str("*x");
        }
        anchors.push_str("]\n");
        let decoded: std::collections::BTreeMap<String, ayaml::Value> = from_str(&anchors).unwrap();
        assert_eq!(decoded.len(), 3);
    }

    /// The accessors read each kind of value, and look through a tag.
    #[test]
    fn value_accessors_read_each_kind() {
        let value: Value = from_str(concat!(
            "s: text\nt: true\nb: false\nn: -3\nu: 18446744073709551615\nf: 0.5\nz: ~\n",
            "q: [a, 1]\nm: {k: v}\ntagged: !celsius 21\n",
        ))
        .unwrap();
        assert_eq!(value.get("s").and_then(Value::as_str), Some("text"));
        assert_eq!(value.get("t").and_then(Value::as_bool), Some(true));
        assert_eq!(value.get("b").and_then(Value::as_bool), Some(false));
        assert_eq!(value.get("s").and_then(Value::as_bool), None);
        assert_eq!(value.get("n").and_then(Value::as_i64), Some(-3));
        assert_eq!(value.get("n").and_then(Value::as_u64), None);
        assert_eq!(value.get("u").and_then(Value::as_u64), Some(u64::MAX));
        assert_eq!(value.get("u").and_then(Value::as_i64), None);
        assert_eq!(value.get("f").and_then(Value::as_f64), Some(0.5));
        assert!(value.get("z").is_some_and(Value::is_null));
        assert!(!value["s"].is_null());
        assert_eq!(value["tagged"].as_u64(), Some(21));
        let sequence = value["q"].as_sequence().unwrap();
        assert_eq!(sequence.len(), 2);
        assert!(!sequence.is_empty());
        assert_eq!(sequence.get(0).and_then(Value::as_str), Some("a"));
        assert_eq!(sequence.get(1).and_then(Value::as_u64), Some(1));
        assert!(sequence.get(2).is_none());
        assert_eq!(value["q"][1], 1);
        assert_eq!(value["m"].as_mapping().map(Mapping::len), Some(1));
        assert_eq!(value["m"]["k"], "v");
        // A value of another kind answers `None`, and indexing it is null.
        assert!(value["s"].as_mapping().is_none());
        assert!(value["s"].as_sequence().is_none());
        assert!(value["s"].get("k").is_none());
        assert!(value["s"]["k"].is_null());
        assert!(value["s"][0].is_null());
        assert!(value["q"][2].is_null());
        assert!(value["missing"].is_null());
        assert!(Value::default().is_null());
    }

    /// A mapping keeps its keys in order: a new key goes last, a replaced
    /// one keeps its place, and removing one keeps the others' order.
    #[test]
    fn mapping_edits_keep_key_order() {
        let mut mapping = Mapping::new();
        assert!(mapping.is_empty());
        assert_eq!(mapping.insert("a", 1_u64), None);
        assert_eq!(mapping.insert("b", true), None);
        assert_eq!(mapping.insert("c", "x"), None);
        assert_eq!(mapping.insert("a", 2_u64), Some(Value::from(1_u64)));
        assert_eq!(mapping.len(), 3);
        assert!(!mapping.is_empty());
        let keys = |mapping: &Mapping| -> Vec<String> {
            mapping
                .keys()
                .map(|key| key.as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(keys(&mapping), ["a", "b", "c"]);
        assert_eq!(mapping["a"], 2);
        assert_eq!(mapping.remove("a"), Some(Value::from(2_u64)));
        assert_eq!(mapping.remove("a"), None);
        assert_eq!(keys(&mapping), ["b", "c"]);
        assert!(mapping.contains_key("b"));
        assert!(!mapping.contains_key("a"));
        assert!(mapping["a"].is_null());
        *mapping.get_mut("b").unwrap() = Value::from("y");
        assert_eq!(mapping.get("b").and_then(Value::as_str), Some("y"));
        assert!(mapping.get_mut("missing").is_none());
        for (_, value) in mapping.iter_mut() {
            *value = Value::from(7_i64);
        }
        let values: Vec<_> = mapping.values().cloned().collect();
        assert_eq!(values, [Value::from(7_i64), Value::from(7_i64)]);
        let mut entries = mapping.iter();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries.next_back().map(|(key, _)| key.clone()),
            Some(Value::from("c"))
        );
        assert_eq!((&mapping).into_iter().count(), 2);
        let collected: Mapping = mapping
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        assert_eq!(collected, mapping);
    }

    /// A sequence keeps its values in order, and edits through `iter_mut`
    /// and a mutable view land in the value that holds it.
    #[test]
    fn sequence_edits_land_in_place() {
        let mut sequence: Sequence = [Value::from("a"), Value::from(1_u64)].into_iter().collect();
        sequence.push(false);
        assert_eq!(sequence.len(), 3);
        for value in sequence.iter_mut() {
            *value = Value::from(value.as_str().unwrap_or("other"));
        }
        let mut value = Value::from(sequence);
        value.as_sequence_mut().unwrap().push("d");
        let strings: Vec<_> = value
            .as_sequence()
            .unwrap()
            .iter()
            .rev()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(strings, ["d", "other", "other", "a"]);
        assert_eq!((value.as_sequence().unwrap()).into_iter().len(), 4);
        assert_eq!(Value::from(vec![Value::from(1_u64)])[0], 1);
        assert!(Sequence::new().is_empty());
        let mut mapping = Value::from(Mapping::new());
        mapping.as_mapping_mut().unwrap().insert("k", "v");
        assert_eq!(mapping["k"], "v");
        assert!(Value::from("s").as_sequence_mut().is_none());
        assert!(Value::from("s").as_mapping_mut().is_none());
    }

    /// Values compare with plain Rust values of the same kind only.
    #[test]
    fn values_compare_with_plain_values() {
        assert_eq!(Value::from("x"), "x");
        assert_eq!(Value::from("x"), *"x");
        assert_eq!(Value::from("x"), "x".to_string());
        assert_ne!(Value::from("1"), 1);
        assert_eq!(Value::from(true), true);
        assert_ne!(Value::from(true), "true");
        assert_eq!(Value::from(-2_i64), -2_i64);
        assert_eq!(Value::from(-2_i64), -2);
        assert_ne!(Value::from(3_i64), 4);
        assert_eq!(Value::from(3_u64), 3_u64);
        assert_eq!(Value::from(0.25), Value::from(0.25));
        assert_eq!(Value::from("s".to_string()), Value::from("s"));
    }

    /// The types read and write as the parser's do: a flattened mapping, a
    /// typed decode, a round trip, and `{:?}` output (which snapshots hold).
    #[test]
    fn serde_and_debug_match_the_parser() {
        #[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
        struct Options {
            pattern: String,
            limits: Vec<u32>,
        }
        #[derive(serde::Deserialize)]
        struct Spec {
            kind: String,
            #[serde(flatten)]
            extra: Mapping,
        }
        let text = "kind: k\npattern: x\nlimits: [1, 2]\n";
        let spec: Spec = from_str(text).unwrap();
        assert_eq!(spec.kind, "k");
        let parser: ayaml::Mapping =
            ayaml::compat::from_str_with_limits("pattern: x\nlimits: [1, 2]\n", &limits()).unwrap();
        assert_eq!(format!("{:?}", spec.extra), format!("{parser:?}"));
        assert_eq!(
            format!("{:?}", spec.extra["limits"]),
            format!("{:?}", parser["limits"])
        );
        assert_eq!(
            format!("{:?}", spec.extra["limits"].as_sequence().unwrap()),
            format!("{:?}", parser["limits"].as_sequence().unwrap())
        );
        let options: Options = from_value(Value::from(spec.extra.clone())).unwrap();
        assert_eq!(options.limits, [1, 2]);
        assert_eq!(to_value(&options).unwrap(), Value::from(spec.extra.clone()));
        let written = to_string(&spec.extra).unwrap();
        assert_eq!(from_str::<Mapping>(&written).unwrap(), spec.extra);
        let sequence: Sequence = from_str("[a, b]").unwrap();
        let items: Vec<_> = sequence.iter().map(|item| item.as_str().unwrap()).collect();
        assert_eq!(items, ["a", "b"]);
        assert_eq!(
            from_str::<Sequence>(&to_string(&sequence).unwrap()).unwrap(),
            sequence
        );
        assert_eq!(serde_json::to_value(&spec.extra).unwrap()["limits"][1], 2);
    }

    /// An error gives the parser's message, without and with its location,
    /// and a Serde error carries its message alone.
    #[test]
    fn errors_give_message_and_location() {
        let error = from_str::<Value>("a: [1, 2\n").unwrap_err();
        let location = error.location().unwrap();
        assert_eq!((location.line(), location.column()), (2, 1), "{error}");
        // The message leaves out the location that the error prints.
        assert!(error.to_string().contains("at line 2 column 1"), "{error}");
        assert!(
            !error.message().contains("at line 2 column 1"),
            "{}",
            error.message()
        );
        assert!(
            error.message().starts_with("did not find expected"),
            "{}",
            error.message()
        );
        let inline = from_str::<Value>("a: b: c\n")
            .unwrap_err()
            .location()
            .unwrap();
        assert_eq!((inline.line(), inline.column()), (1, 5));
        let typed = from_str::<u32>("text").unwrap_err();
        assert!(
            typed.message().contains("invalid type"),
            "{}",
            typed.message()
        );
        let custom = <Error as serde::de::Error>::custom("bad option");
        assert_eq!(custom.to_string(), "bad option");
        assert_eq!(custom.message(), "bad option");
        assert!(custom.location().is_none());
        assert!(std::error::Error::source(&custom).is_none());
        let chained = Error::from_parser(
            ayaml::Error::new(ayaml::ErrorKind::Serde, "outer")
                .with_source(std::io::Error::other("cause")),
        );
        let source = std::error::Error::source(&chained).map(ToString::to_string);
        assert_eq!(source.as_deref(), Some("cause"));
        let custom = <Error as serde::ser::Error>::custom("cannot write");
        assert_eq!(custom.to_string(), "cannot write");
        assert_eq!(format!("{custom:?}"), format!("{:?}", custom.into_parser()));
    }

    /// The caps that stay still refuse: depth past 128, and input past the
    /// 32 MiB cap.
    #[test]
    fn the_depth_and_input_caps_still_refuse() {
        let deep = format!("{}{}", "[".repeat(129), "]".repeat(129));
        assert!(from_str::<ayaml::Value>(&deep).is_err());
        let large = format!("a: \"{}\"\n", "x".repeat(crate::MAX_STRUCTURED_BYTES));
        assert!(from_str::<ayaml::Value>(&large).is_err());
    }
}
