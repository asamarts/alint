//! A version 2 document decodes as a v1 one does, plus what only v2 has.
//!
//! v2 is a superset of v1 (design section 3 item 2), so a v2 source's v1 keys
//! must read exactly as a v1 source's: from the text when nothing is
//! interpolated, where a string field takes any scalar's text and an error has
//! a line and column, and from the interpolated value otherwise. [`Document`]
//! therefore decodes [`RawConfig`] through the same deserializer, and only
//! takes the v2-only top-level keys aside, undecoded, as it goes. `RawConfig`
//! itself never learns them, so a v1 source's errors list exactly the keys
//! they always did.

use std::cell::RefCell;

use serde::de::{self, DeserializeSeed, Deserializer, IntoDeserializer as _, MapAccess, Visitor};
use serde::{Deserialize, forward_to_deserialize_any};

use crate::RawConfig;

/// The top-level keys only a version 2 source has.
pub(crate) const V2_ONLY_KEYS: &[&str] = &["selectors", "relations"];

/// The keys of an `extends:` entry's mapping form in a version 2 source.
pub(crate) const EXTENDS_ENTRY_KEYS: &[&str] = &["url", "only", "except", "as"];

/// A version 2 document: its v1 part, and the v2-only top-level keys with
/// their values as written (after interpolation), in document order.
pub(crate) struct Document {
    pub(crate) config: RawConfig,
    pub(crate) v2_keys: Vec<(String, ayaml::Value)>,
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let taken = RefCell::new(Vec::new());
        let config = RawConfig::deserialize(TakeV2Keys {
            inner: deserializer,
            taken: &taken,
        })?;
        Ok(Self {
            config,
            v2_keys: taken.into_inner(),
        })
    }
}

/// A deserializer that hands `RawConfig` every top-level entry but those in
/// [`V2_ONLY_KEYS`], which it keeps in `taken`.
struct TakeV2Keys<'t, D> {
    inner: D,
    taken: &'t RefCell<Vec<(String, ayaml::Value)>>,
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for TakeV2Keys<'_, D> {
    type Error = D::Error;

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_struct(
            name,
            fields,
            TakeVisitor {
                inner: visitor,
                taken: self.taken,
            },
        )
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.inner.deserialize_any(visitor)
    }

    fn is_human_readable(&self) -> bool {
        self.inner.is_human_readable()
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map enum identifier ignored_any
    }
}

struct TakeVisitor<'t, V> {
    inner: V,
    taken: &'t RefCell<Vec<(String, ayaml::Value)>>,
}

impl<'de, V: Visitor<'de>> Visitor<'de> for TakeVisitor<'_, V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.inner.expecting(formatter)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_map(TakeMap {
            inner: map,
            taken: self.taken,
        })
    }

    fn visit_seq<A: de::SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
        self.inner.visit_seq(seq)
    }
}

struct TakeMap<'t, A> {
    inner: A,
    taken: &'t RefCell<Vec<(String, ayaml::Value)>>,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for TakeMap<'_, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        // The dispatch checked every top-level key of a v2 document before
        // this decode, so each one reads as a string.
        while let Some(key) = self.inner.next_key::<String>()? {
            if V2_ONLY_KEYS.contains(&key.as_str()) {
                let value: ayaml::Value = self.inner.next_value()?;
                self.taken.borrow_mut().push((key, value));
                continue;
            }
            return seed.deserialize(key.into_deserializer()).map(Some);
        }
        Ok(None)
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<S::Value, Self::Error> {
        self.inner.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

/// An `extends:` entry as the loader holds it: the entry, and the alias a
/// version 2 source gives it (`as:`). A v1 source never reaches the decode
/// with an alias (the dispatch refuses it first), and the rest of an entry
/// decodes, and fails, exactly as [`alint_core::ExtendsEntry`] does.
#[derive(Debug, Clone)]
pub(crate) struct ExtendsEntry {
    pub(crate) entry: alint_core::ExtendsEntry,
    pub(crate) alias: Option<String>,
}

impl<'de> Deserialize<'de> for ExtendsEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // `alint_core::ExtendsEntry` reads a value first too, so taking `as`
        // out of it leaves everything else as that decode would see it.
        let mut value = ayaml::Value::deserialize(deserializer)?;
        let alias = match &mut value {
            ayaml::Value::Mapping(entry) => match entry.remove("as") {
                Some(ayaml::Value::String(alias)) => Some(alias),
                Some(_) => {
                    return Err(de::Error::custom("`as` must be an alias name, a string"));
                }
                None => None,
            },
            _ => None,
        };
        let entry = alint_core::ExtendsEntry::deserialize(value).map_err(de::Error::custom)?;
        Ok(Self { entry, alias })
    }
}

/// The top-level keys of a v1 source, as `RawConfig` reads them, in its order.
pub(crate) fn v1_keys() -> &'static [&'static str] {
    static KEYS: std::sync::OnceLock<&'static [&'static str]> = std::sync::OnceLock::new();
    KEYS.get_or_init(|| {
        let mut fields: &'static [&'static str] = &[];
        // Serde hands a struct's field names to `deserialize_struct`; the
        // capture refuses the decode right after reading them.
        let _ = RawConfig::deserialize(FieldNames(&mut fields));
        fields
    })
}

/// A deserializer that records the field names a struct asks for.
struct FieldNames<'f>(&'f mut &'static [&'static str]);

impl<'de> Deserializer<'de> for FieldNames<'_> {
    type Error = de::value::Error;

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Self::Error> {
        *self.0 = fields;
        Err(de::Error::custom("field names read"))
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Self::Error> {
        Err(de::Error::custom("not a struct"))
    }

    forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple
        tuple_struct map enum identifier ignored_any
    }
}
