//! Row values (ADR-0023 decision V21-1, ADR-0024 decision 1) and their
//! conversion to and from the plugin contract's flat form.

use std::fmt;
use std::sync::Arc;

use super::path::RelPath;
use crate::plugin_api::{FlatValue, Node};

/// A number exactly as its source wrote it. YAML reads `1.10` as the float
/// 1.1, and JSON and TOML have their own grammars, so comparison uses the
/// lexeme unless a policy asks for a numeric transform. The lexeme is shared,
/// as a string value's text is ([`Value`]).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Number(Arc<str>);

impl Number {
    /// A lexeme an adapter's parser already recognised as a number. `None`
    /// for an empty lexeme or one containing whitespace.
    #[must_use]
    pub fn from_lexeme(lexeme: &str) -> Option<Self> {
        (!lexeme.is_empty() && !lexeme.contains(char::is_whitespace)).then(|| Self(lexeme.into()))
    }

    #[must_use]
    pub fn lexeme(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A row value. Domain scalars such as `semver` are added once several kinds
/// consume them; inside alint that is additive, but in the WIT contract a new
/// variant case is a breaking change and needs a new contract minor version.
///
/// A string's text, like a number's lexeme, is shared: a clone copies a
/// pointer, so a relation that copies one value into many fields or through
/// many joins holds its text once.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(Arc<str>),
    Bytes(Vec<u8>),
    Path(RelPath),
    List(Vec<Value>),
    /// Fields in source order.
    Record(Vec<(Box<str>, Value)>),
}

/// A contract value whose scalars are not valid engine values.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContractValueError {
    #[error("node {node}: {lexeme:?} is not a number lexeme")]
    Number { node: usize, lexeme: String },
    #[error("node {node}: {path:?} is not a normalized repository path")]
    Path { node: usize, path: String },
}

impl Value {
    /// A literal a configuration writes, as the relation layer compares it: a
    /// boolean, a string, or a number by the text YAML gives it (which need
    /// not be the text written: YAML reads `1.10` as the float 1.1). `None`
    /// for null, a list, a map or a tagged value, none of which is a literal.
    #[must_use]
    pub fn from_literal(value: &crate::yaml::Value) -> Option<Self> {
        match value.parser() {
            ayaml::Value::Bool(flag) => Some(Self::Bool(*flag)),
            ayaml::Value::Number(number) => {
                let text = number.to_string();
                Some(
                    Number::from_lexeme(&text)
                        .map_or_else(|| Self::String(text.into()), Self::Number),
                )
            }
            ayaml::Value::String(text) => Some(Self::String(text.as_str().into())),
            _ => None,
        }
    }

    /// The bytes the value holds beyond its own size: a string's text, a
    /// number's lexeme, and a list's or record's items. What the relation
    /// evaluator charges to [`LimitKind::BytesPerRun`](super::LimitKind::BytesPerRun)
    /// for a decoded value. Iterative, so value depth cannot exhaust the
    /// stack.
    pub(crate) fn heap_bytes(&self) -> usize {
        let mut bytes = 0usize;
        let mut pending = vec![self];
        while let Some(value) = pending.pop() {
            let held = match value {
                Self::Null | Self::Bool(_) => 0,
                Self::Number(number) => number.lexeme().len(),
                Self::String(text) => text.len(),
                Self::Bytes(bytes) => bytes.len(),
                Self::Path(path) => path.as_str().len(),
                Self::List(items) => {
                    pending.extend(items);
                    items.len() * std::mem::size_of::<Self>()
                }
                Self::Record(fields) => {
                    pending.extend(fields.iter().map(|(_, item)| item));
                    fields
                        .iter()
                        .map(|(name, _)| name.len() + std::mem::size_of::<(Box<str>, Self)>())
                        .sum()
                }
            };
            bytes = bytes.saturating_add(held);
        }
        bytes
    }

    /// The value's contract form, in pre-order. Iterative, so value depth
    /// cannot exhaust the stack.
    #[must_use]
    pub fn to_contract(&self) -> FlatValue {
        let mut nodes = Vec::new();
        // (value, the slot to patch in its parent once its index is known)
        let mut stack: Vec<(&Value, Option<(usize, usize)>)> = vec![(self, None)];
        while let Some((value, slot)) = stack.pop() {
            let index = nodes.len();
            if let Some((parent, position)) = slot {
                let index = u32::try_from(index).expect("flat values stay below 2^32 nodes");
                match &mut nodes[parent] {
                    Node::List(children) => children[position] = index,
                    Node::Record(fields) => fields[position].1 = index,
                    _ => unreachable!("only containers have children"),
                }
            }
            let node = match value {
                Value::Null => Node::Null,
                Value::Bool(b) => Node::Bool(*b),
                Value::Number(n) => Node::Number(n.lexeme().to_string()),
                Value::String(s) => Node::String(s.to_string()),
                Value::Bytes(b) => Node::Bytes(b.clone()),
                Value::Path(p) => Node::Path(p.as_str().to_string()),
                Value::List(items) => {
                    for (position, item) in items.iter().enumerate().rev() {
                        stack.push((item, Some((index, position))));
                    }
                    Node::List(vec![0; items.len()])
                }
                Value::Record(fields) => {
                    for (position, (_, item)) in fields.iter().enumerate().rev() {
                        stack.push((item, Some((index, position))));
                    }
                    Node::Record(
                        fields
                            .iter()
                            .map(|(name, _)| (name.to_string(), 0))
                            .collect(),
                    )
                }
            };
            nodes.push(node);
        }
        FlatValue::from_engine(nodes)
    }

    /// The engine value for a validated contract value, checking that its
    /// number lexemes and paths are what the engine accepts.
    pub fn from_contract(flat: &FlatValue) -> Result<Self, ContractValueError> {
        let nodes = flat.nodes();
        // Children come after parents: build values from the last node back, so
        // every child is finished before its parent takes it.
        let mut built: Vec<Option<Value>> = vec![None; nodes.len()];
        for index in (0..nodes.len()).rev() {
            let mut take = |child: u32| {
                built[child as usize]
                    .take()
                    .expect("a validated flat value builds each child once, before its parent")
            };
            let value = match &nodes[index] {
                Node::Null => Value::Null,
                Node::Bool(b) => Value::Bool(*b),
                Node::Number(lexeme) => {
                    Value::Number(Number::from_lexeme(lexeme).ok_or_else(|| {
                        ContractValueError::Number {
                            node: index,
                            lexeme: lexeme.clone(),
                        }
                    })?)
                }
                Node::String(s) => Value::String(s.as_str().into()),
                Node::Bytes(b) => Value::Bytes(b.clone()),
                Node::Path(path) => match RelPath::parse(path) {
                    Some(parsed) if parsed.as_str() == path => Value::Path(parsed),
                    _ => {
                        return Err(ContractValueError::Path {
                            node: index,
                            path: path.clone(),
                        });
                    }
                },
                Node::List(children) => Value::List(children.iter().map(|c| take(*c)).collect()),
                Node::Record(fields) => Value::Record(
                    fields
                        .iter()
                        .map(|(name, c)| (name.as_str().into(), take(*c)))
                        .collect(),
                ),
            };
            built[index] = Some(value);
        }
        Ok(built[0].take().expect("the root is built"))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::plugin_api::FlatValueLimits;
    use proptest::prelude::*;

    fn record(fields: &[(&str, Value)]) -> Value {
        Value::Record(
            fields
                .iter()
                .map(|(name, value)| ((*name).into(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn numbers_keep_their_lexeme() {
        let a = Number::from_lexeme("1.10").unwrap();
        let b = Number::from_lexeme("1.1").unwrap();
        assert_ne!(a, b);
        assert_eq!(a.lexeme(), "1.10");
        assert!(Number::from_lexeme("").is_none());
        assert!(Number::from_lexeme("1 0").is_none());
    }

    #[test]
    fn contract_form_is_pre_order_with_forward_child_indices() {
        let value = record(&[
            ("a", Value::List(vec![Value::Null, Value::Bool(true)])),
            ("b", Value::String("x".into())),
        ]);
        let flat = value.to_contract();
        assert_eq!(
            flat.nodes(),
            [
                Node::Record(vec![("a".into(), 1), ("b".into(), 4)]),
                Node::List(vec![2, 3]),
                Node::Null,
                Node::Bool(true),
                Node::String("x".into()),
            ]
        );
        assert_eq!(Value::from_contract(&flat), Ok(value));
    }

    #[test]
    fn contract_scalars_must_be_engine_values() {
        let limits = FlatValueLimits::default();
        let bad_number = FlatValue::new(vec![Node::Number("1 0".into())], limits).unwrap();
        assert!(matches!(
            Value::from_contract(&bad_number),
            Err(ContractValueError::Number { node: 0, .. })
        ));
        for path in ["../escape", "/abs", "a//b", "a/./b", ""] {
            let flat = FlatValue::new(vec![Node::Path(path.into())], limits).unwrap();
            assert!(
                matches!(
                    Value::from_contract(&flat),
                    Err(ContractValueError::Path { .. })
                ),
                "{path:?} was accepted"
            );
        }
    }

    #[test]
    fn deep_values_convert_without_recursion() {
        let mut value = Value::Null;
        for _ in 0..10_000 {
            value = Value::List(vec![value]);
        }
        let flat = value.to_contract();
        assert_eq!(flat.nodes().len(), 10_001);
        let limits = FlatValueLimits {
            max_nodes: 20_000,
            max_depth: 20_000,
        };
        let checked = FlatValue::new(flat.nodes().to_vec(), limits).unwrap();
        let back = Value::from_contract(&checked).unwrap();
        // Compare flat forms: derived `==` on 10,000 nested lists recurses.
        assert_eq!(back.to_contract(), flat);
        // Dismantle iteratively: dropping 10,000 nested lists recursively could
        // overflow a test thread's stack.
        for mut v in [value, back] {
            while let Value::List(mut items) = v {
                v = items.pop().unwrap_or(Value::Null);
            }
        }
    }

    pub(crate) fn value_strategy() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::Bool),
            "-?[0-9]{1,3}(\\.[0-9]{1,2})?"
                .prop_map(|s| Value::Number(Number::from_lexeme(&s).unwrap())),
            "[a-z]{0,3}".prop_map(|s| Value::String(s.into())),
            proptest::collection::vec(any::<u8>(), 0..3).prop_map(Value::Bytes),
            "[a-z]{1,3}(/[a-z]{1,3}){0,2}".prop_map(|s| Value::Path(RelPath::parse(&s).unwrap())),
        ];
        leaf.prop_recursive(4, 32, 4, |inner| {
            prop_oneof![
                proptest::collection::vec(inner.clone(), 0..4).prop_map(Value::List),
                proptest::collection::vec(("[a-z]{0,2}", inner), 0..4).prop_map(|fields| {
                    Value::Record(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
                }),
            ]
        })
    }

    proptest! {
        #[test]
        fn nested_to_flat_round_trips(value in value_strategy()) {
            let flat = value.to_contract();
            let checked = FlatValue::new(flat.nodes().to_vec(), FlatValueLimits::default());
            prop_assert_eq!(checked.as_ref(), Ok(&flat));
            prop_assert_eq!(Value::from_contract(&flat), Ok(value));
        }

        #[test]
        fn arbitrary_child_indices_never_panic(
            value in value_strategy(),
            patch in (0usize..16, 0u32..16),
        ) {
            let mut nodes = value.to_contract().nodes().to_vec();
            let (at, to) = patch;
            let target = at % nodes.len().max(1);
            if let Some(node) = nodes.get_mut(target) {
                match node {
                    Node::List(children) if !children.is_empty() => children[0] = to,
                    Node::Record(fields) if !fields.is_empty() => fields[0].1 = to,
                    _ => {}
                }
            }
            if let Ok(flat) = FlatValue::new(nodes, FlatValueLimits::default()) {
                // Whatever survives validation is a well-formed tree: it converts,
                // and its canonical (pre-order) form round-trips.
                let value = Value::from_contract(&flat).unwrap();
                let canonical = value.to_contract();
                prop_assert!(
                    FlatValue::new(canonical.nodes().to_vec(), FlatValueLimits::default()).is_ok()
                );
                prop_assert_eq!(Value::from_contract(&canonical), Ok(value));
            }
        }
    }
}
