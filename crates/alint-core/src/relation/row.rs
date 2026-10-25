//! Rows, their absence states and cardinality, and relations in canonical
//! order (general-linter sections 6.2 and 6.3).

use std::cmp::Ordering;
use std::collections::BTreeSet;

use super::pointer::Pointer;
use super::resource::Resource;
use super::value::Value;
use crate::extract::ByteSpan;

/// What an extractor found at a row's location. An explicit `null`
/// (`Value(Value::Null)`), a structural key with no value, a missing location,
/// and an extraction error are four different outcomes, and no stage may
/// coerce one into another.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RowState {
    Value(Value),
    /// The row names a structural key and has no value. Key-only adapters
    /// (`key_parity`'s entry rows) use this; it is deliberately not
    /// `Value(Value::Null)`.
    Key,
    /// The location does not exist in the resource.
    Missing,
    /// The location exists, but its value could not be extracted, for example
    /// a non-literal `${VAR}` or a type the extractor does not handle.
    Error(Box<str>),
}

/// One extracted value with its provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub resource: Resource,
    /// Where in the resource, when the adapter knows: structured rows have a
    /// pointer, rows from today's string extraction do not.
    pub pointer: Option<Pointer>,
    /// An extractor-chosen name (a capture group, a relation field).
    pub label: Option<Box<str>>,
    /// The order the adapter emitted this row in, within its resource.
    pub ordinal: u32,
    /// Byte range of the value when the adapter knows it.
    pub span: Option<ByteSpan>,
    pub state: RowState,
    /// The value as the source wrote it, when a transform (a `normalize:`
    /// list) changed it; messages quote both.
    pub original: Option<Value>,
}

impl Row {
    #[must_use]
    pub fn new(
        resource: Resource,
        pointer: Option<Pointer>,
        ordinal: u32,
        state: RowState,
    ) -> Self {
        Self {
            resource,
            pointer,
            label: None,
            ordinal,
            span: None,
            state,
            original: None,
        }
    }

    #[must_use]
    pub fn with_original(mut self, original: Value) -> Self {
        self.original = Some(original);
        self
    }

    #[must_use]
    pub fn with_label(mut self, label: &str) -> Self {
        self.label = Some(label.into());
        self
    }

    #[must_use]
    pub fn with_span(mut self, span: ByteSpan) -> Self {
        self.span = Some(span);
        self
    }

    #[must_use]
    pub fn value(&self) -> Option<&Value> {
        match &self.state {
            RowState::Value(value) => Some(value),
            RowState::Key | RowState::Missing | RowState::Error(_) => None,
        }
    }

    /// Whether this row names a structural key rather than a value.
    #[must_use]
    pub fn is_key(&self) -> bool {
        matches!(self.state, RowState::Key)
    }

    /// The canonical order: path (segment by segment, the index order),
    /// structural pointer (none first), label (none first), emission order,
    /// then the byte offset where known (rows without one after those with
    /// one), the state, and the resource's origin, so the order is total and
    /// never depends on how rows were collected.
    #[must_use]
    pub fn canonical_cmp(&self, other: &Self) -> Ordering {
        let offset = |row: &Self| row.span.map_or(usize::MAX, |span| span.start);
        self.resource
            .path()
            .cmp(other.resource.path())
            .then_with(|| self.pointer.cmp(&other.pointer))
            .then_with(|| self.label.cmp(&other.label))
            .then_with(|| self.ordinal.cmp(&other.ordinal))
            .then_with(|| offset(self).cmp(&offset(other)))
            .then_with(|| self.state.cmp(&other.state))
            .then_with(|| self.resource.origin().cmp(&other.resource.origin()))
            .then_with(|| self.original.cmp(&other.original))
    }
}

/// How many values an extractor yields for one resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cardinality {
    One,
    ZeroOrOne,
    OneOrMore,
    Many,
}

/// An extractor yielded a number of values its cardinality does not allow.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("expected {expected:?} values, found {found}")]
pub struct CardinalityError {
    pub expected: Cardinality,
    pub found: usize,
}

impl Cardinality {
    #[must_use]
    pub fn admits(self, count: usize) -> bool {
        match self {
            Self::One => count == 1,
            Self::ZeroOrOne => count <= 1,
            Self::OneOrMore => count >= 1,
            Self::Many => true,
        }
    }

    pub fn check(self, count: usize) -> Result<(), CardinalityError> {
        if self.admits(count) {
            Ok(())
        } else {
            Err(CardinalityError {
                expected: self,
                found: count,
            })
        }
    }
}

/// A finite multiset of rows in canonical order. Duplicates stay: they are
/// often the violation being sought. Converting to a set is explicit
/// ([`Relation::distinct_values`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Relation {
    rows: Vec<Row>,
}

impl Relation {
    #[must_use]
    pub fn new(mut rows: Vec<Row>) -> Self {
        rows.sort_by(Row::canonical_cmp);
        Self { rows }
    }

    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The distinct values present, ignoring missing and error rows. The
    /// caller checks the result against `LimitKind::DistinctValues`.
    #[must_use]
    pub fn distinct_values(&self) -> BTreeSet<&Value> {
        self.rows.iter().filter_map(Row::value).collect()
    }

    /// The first `n` rows in canonical order and how many were cut, so a "top
    /// N, and M more" report is deterministic.
    #[must_use]
    pub fn first(&self, n: usize) -> (&[Row], usize) {
        let shown = n.min(self.rows.len());
        (&self.rows[..shown], self.rows.len() - shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::value::tests::value_strategy;
    use proptest::prelude::*;

    fn row(path: &str, key: &str, state: RowState) -> Row {
        Row::new(
            Resource::for_test(0, path),
            Some(Pointer::root().key(key)),
            0,
            state,
        )
    }

    #[test]
    fn missing_null_key_and_error_are_distinct() {
        let null = row("a.json", "k", RowState::Value(Value::Null));
        let key = row("a.json", "k", RowState::Key);
        let missing = row("a.json", "k", RowState::Missing);
        let error = row("a.json", "k", RowState::Error("non-literal value".into()));
        assert_ne!(null, key);
        assert_ne!(null, missing);
        assert_ne!(missing, error);
        assert_ne!(null, error);
        assert_ne!(key, missing);
        assert_ne!(key, error);
        assert_eq!(null.value(), Some(&Value::Null));
        assert_eq!(key.value(), None);
        assert!(key.is_key());
        assert_eq!(missing.value(), None);
        assert_eq!(error.value(), None);
        let relation = Relation::new(vec![error, missing, null]);
        assert_eq!(relation.len(), 3, "absence states are rows, not omissions");
        assert_eq!(
            relation.distinct_values().into_iter().collect::<Vec<_>>(),
            [&Value::Null]
        );
    }

    #[test]
    fn canonical_order_is_path_pointer_label_ordinal_offset() {
        let first_path = row("a-c.json", "z", RowState::Missing);
        let subdir = row("a/b.json", "z", RowState::Missing);
        let no_pointer = Row::new(Resource::for_test(0, "b.json"), None, 0, RowState::Missing);
        let pointer_a_late = row("b.json", "a", RowState::Missing).with_span(ByteSpan::new(9, 10));
        let pointer_a_early = row("b.json", "a", RowState::Missing).with_span(ByteSpan::new(2, 3));
        let pointer_a_second = {
            let mut row = row("b.json", "a", RowState::Missing);
            row.ordinal = 1;
            row
        };
        let labelled = row("b.json", "a", RowState::Missing).with_label("cap");
        let pointer_x = row("b.json", "x", RowState::Missing);
        let relation = Relation::new(vec![
            pointer_x.clone(),
            labelled.clone(),
            pointer_a_second.clone(),
            pointer_a_late.clone(),
            no_pointer.clone(),
            pointer_a_early.clone(),
            subdir.clone(),
            first_path.clone(),
        ]);
        assert_eq!(
            relation.rows(),
            [
                // Segment order: `a/b.json` before `a-c.json` before `b.json`.
                subdir,
                first_path,
                no_pointer,
                // Same path, pointer and label: emission order, then offset.
                pointer_a_early,
                pointer_a_late,
                pointer_a_second,
                labelled,
                pointer_x,
            ]
        );
    }

    #[test]
    fn the_original_value_rides_along() {
        let normalized = row(
            "a.json",
            "v",
            RowState::Value(Value::String("1.2.0".into())),
        )
        .with_original(Value::String("v1.2.0".into()));
        assert_eq!(normalized.value(), Some(&Value::String("1.2.0".into())));
        assert_eq!(normalized.original, Some(Value::String("v1.2.0".into())));
    }

    #[test]
    fn duplicates_stay_and_truncation_cuts_after_the_sort() {
        let rows: Vec<Row> = (0..5)
            .map(|i| row("a.json", "k", RowState::Value(Value::Bool(i % 2 == 0))))
            .collect();
        let relation = Relation::new(rows);
        assert_eq!(relation.len(), 5);
        assert_eq!(relation.distinct_values().len(), 2);
        let (shown, cut) = relation.first(2);
        assert_eq!((shown.len(), cut), (2, 3));
        assert_eq!(relation.first(10).1, 0);
    }

    #[test]
    fn cardinality_admits_exactly_its_counts() {
        let table = [
            (Cardinality::One, [false, true, false]),
            (Cardinality::ZeroOrOne, [true, true, false]),
            (Cardinality::OneOrMore, [false, true, true]),
            (Cardinality::Many, [true, true, true]),
        ];
        for (cardinality, admits) in table {
            for (count, want) in [0, 1, 2].into_iter().zip(admits) {
                assert_eq!(cardinality.admits(count), want, "{cardinality:?} {count}");
                assert_eq!(cardinality.check(count).is_ok(), want);
            }
        }
        assert_eq!(
            Cardinality::One.check(3),
            Err(CardinalityError {
                expected: Cardinality::One,
                found: 3
            })
        );
    }

    fn row_strategy() -> impl Strategy<Value = Row> {
        let state = prop_oneof![
            value_strategy().prop_map(RowState::Value),
            Just(RowState::Key),
            Just(RowState::Missing),
            "[a-z]{0,3}".prop_map(|m| RowState::Error(m.into())),
        ];
        (
            0u32..3,
            "[a-c]{1,2}(/[a-c]{1,2})?",
            proptest::collection::vec("[a-c]{0,2}", 0..3),
            proptest::option::of("[a-b]{1,2}"),
            proptest::option::of(0usize..20),
            state,
        )
            .prop_map(|(id, path, keys, label, start, state)| {
                let pointer =
                    (!keys.is_empty()).then(|| keys.iter().fold(Pointer::root(), |p, k| p.key(k)));
                let mut row = Row::new(Resource::for_test(id, &path), pointer, id, state);
                row.label = label.map(Into::into);
                row.span = start.map(|s| ByteSpan::new(s, s + 1));
                row
            })
    }

    proptest! {
        #[test]
        fn canonical_order_does_not_depend_on_input_order(
            rows in proptest::collection::vec(row_strategy(), 0..12),
            rotate in 0usize..12,
        ) {
            let mut shuffled = rows.clone();
            shuffled.reverse();
            if !shuffled.is_empty() {
                let by = rotate % shuffled.len();
                shuffled.rotate_left(by);
            }
            prop_assert_eq!(Relation::new(rows), Relation::new(shuffled));
        }
    }
}
