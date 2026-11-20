//! What alint's typed YAML decoding adds to ayaml's: every typed decode of
//! [`crate::yaml`] goes through [`Checked`], a deserializer over ayaml's that
//! reads what ayaml reads, but for the first point below, and says more when
//! it refuses:
//! - an enum's variant without data takes only its name, a string, and the
//!   error names the field, the allowed values and what was found, as in
//!   `level: expected one of `error`, `warning`, `info`, `off`, found a
//!   mapping`. ayaml read serde's form of a variant with data there too, a
//!   one-entry mapping (`level: {error: ~}`) or a tag naming the variant,
//!   which the parser v0.22 used refused, and so does alint;
//! - a value of the wrong type where a string or a struct goes, decoded from a
//!   value tree, is named, as in `invalid type: integer `1`, expected a
//!   string`, where ayaml says only `non-string YAML value`;
//! - a YAML tag a typed field cannot read is named, as in `` `version` has the
//!   YAML tag `!!timestamp` ``, where ayaml's error speaks of an enum.
//!
//! [`indentation_hint`] adds a hint to the parse error the common indentation
//! mistakes give.

use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, Error as _, MapAccess, SeqAccess, Unexpected,
    VariantAccess, Visitor,
};

/// Where a decode reads from. ayaml reads a value differently from text, and
/// [`Checked`] keeps each reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// The text of a document. A string target reads any scalar's text there
    /// (`pattern: 1` is `"1"`), as the parser v0.22 used did, and an empty
    /// value is an empty struct, which `deserialize_any` cannot tell from `~`:
    /// strings and structs keep ayaml's own reading.
    Text,
    /// A value tree, which `from_value` decodes. A string target takes only a
    /// string there, and a struct only a mapping, so [`Checked`] reads the
    /// value itself to name what it found.
    Value,
}

/// The seed a decode from text starts with: `T` read through [`Checked`].
pub(crate) struct TextSeed<T>(PhantomData<T>);

impl<T> TextSeed<T> {
    pub(crate) fn new() -> Self {
        Self(PhantomData)
    }
}

impl<'de, T: de::Deserialize<'de>> DeserializeSeed<'de> for TextSeed<T> {
    type Value = T;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<T, D::Error> {
        T::deserialize(Checked::new(deserializer, Mode::Text))
    }
}

/// Decode `value` into `T` through [`Checked`], with [`finish`] applied to
/// the error.
pub(crate) fn from_value<T: de::DeserializeOwned>(value: ayaml::Value) -> Result<T, ayaml::Error> {
    T::deserialize(Checked::new(value, Mode::Value)).map_err(finish)
}

/// What a tag error says after the tag (see [`TagRefused`]).
const TAG_REFUSED: &str = "; YAML tags are not supported here";

/// What a tag error says before the tag.
const HAS_THE_TAG: &str = "has the YAML tag `";

/// `error` as alint reports it: a tag error, which [`Checked`] words as `has
/// the YAML tag ...` and ayaml then puts the path in front of, as `version:
/// has ...`, names the path as alint's own tag errors do, `` `version` has
/// ...``. Any other error is ayaml's.
pub(crate) fn finish(error: ayaml::Error) -> ayaml::Error {
    let Some(head) = error.message().strip_suffix(TAG_REFUSED) else {
        return error;
    };
    let (subject, tag) = if let Some(tag) = head.strip_prefix(HAS_THE_TAG) {
        ("the value".to_owned(), tag)
    } else {
        // A tag holds no space, so the last `: has the YAML tag` is the one
        // after the path, whatever a quoted key in the path holds.
        let Some(at) = head.rfind(&format!(": {HAS_THE_TAG}")) else {
            return error;
        };
        (
            format!("`{}`", &head[..at]),
            &head[at + 2 + HAS_THE_TAG.len()..],
        )
    };
    let Some(tag) = tag.strip_suffix('`') else {
        return error;
    };
    if tag.is_empty() || tag.contains(['`', ' ']) {
        return error;
    }
    let mut rewritten = ayaml::Error::new(
        error.kind(),
        format!("{subject} {HAS_THE_TAG}{tag}`{TAG_REFUSED}"),
    );
    if let Some(span) = error.span() {
        rewritten = rewritten.with_span(span);
    }
    if let Some(location) = error.location() {
        rewritten = rewritten.with_location(location);
    }
    rewritten
}

/// A deserializer over ayaml's `D` that reads what `D` reads, but for a
/// variant without data (see [`Checked::deserialize_enum`]), and words some
/// of its refusals better (see the module's documentation). Everything
/// it hands a visitor is wrapped too, so it reads every value of a document.
/// Errors keep ayaml's path in front, which ayaml's own collections add as an
/// error passes out through them.
struct Checked<D> {
    de: D,
    mode: Mode,
}

impl<D> Checked<D> {
    fn new(de: D, mode: Mode) -> Self {
        Self { de, mode }
    }
}

/// [`Checked`]'s methods that hand the wrapped visitor to ayaml's.
macro_rules! wrapped {
    ($($method:ident)*) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
            self.de.$method(Wrap::new(visitor, self.mode))
        }
    )*};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Checked<D> {
    type Error = D::Error;

    wrapped! {
        deserialize_any deserialize_bool deserialize_i8 deserialize_i16 deserialize_i32
        deserialize_i64 deserialize_i128 deserialize_u8 deserialize_u16 deserialize_u32
        deserialize_u64 deserialize_u128 deserialize_f32 deserialize_f64 deserialize_char
        deserialize_bytes deserialize_byte_buf deserialize_option deserialize_unit
        deserialize_seq deserialize_map deserialize_identifier
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        match self.mode {
            Mode::Text => self.de.deserialize_str(Wrap::new(visitor, self.mode)),
            Mode::Value => self.de.deserialize_any(StringShape(visitor)),
        }
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        match self.mode {
            Mode::Text => self.de.deserialize_string(Wrap::new(visitor, self.mode)),
            Mode::Value => self.de.deserialize_any(StringShape(visitor)),
        }
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.de
            .deserialize_unit_struct(name, Wrap::new(visitor, self.mode))
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.de
            .deserialize_newtype_struct(name, Wrap::new(visitor, self.mode))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.de
            .deserialize_tuple(len, Wrap::new(visitor, self.mode))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.de
            .deserialize_tuple_struct(name, len, Wrap::new(visitor, self.mode))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        match self.mode {
            Mode::Text => self
                .de
                .deserialize_struct(name, fields, Wrap::new(visitor, self.mode)),
            Mode::Value => self.de.deserialize_any(StructShape {
                visitor,
                name,
                fields,
            }),
        }
    }

    /// An enum: a string names a variant without data, and a mapping of one
    /// entry or a tag names a variant with data and holds the data, as in
    /// serde's externally tagged form. ayaml reads a mapping or a tag as a
    /// variant's name and data for a variant without data too, so it read
    /// `level: {error: ~}` as `level: error`, which the parser v0.22 used
    /// refused, as alint does: anything but a variant's name where a variant
    /// without data goes is refused, with the variants listed. (alint's own
    /// enums with data are `untagged`, which reads its value with
    /// `deserialize_any`, not here.)
    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.de.deserialize_any(EnumShape {
            visitor,
            variants,
            mode: self.mode,
        })
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.de.deserialize_ignored_any(visitor)
    }

    fn is_human_readable(&self) -> bool {
        self.de.is_human_readable()
    }
}

/// A visitor that hands each value it is given on through [`Checked`], and
/// names a tag its visitor refuses (see [`visit_tagged`]).
struct Wrap<V> {
    visitor: V,
    mode: Mode,
}

impl<V> Wrap<V> {
    fn new(visitor: V, mode: Mode) -> Self {
        Self { visitor, mode }
    }
}

/// [`Wrap`]'s methods that pass a scalar on as it is.
macro_rules! scalars {
    ($($method:ident: $ty:ty),* $(,)?) => {$(
        fn $method<E: de::Error>(self, value: $ty) -> Result<V::Value, E> {
            self.visitor.$method(value)
        }
    )*};
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Wrap<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(formatter)
    }

    scalars! {
        visit_bool: bool, visit_i8: i8, visit_i16: i16, visit_i32: i32, visit_i64: i64,
        visit_i128: i128, visit_u8: u8, visit_u16: u16, visit_u32: u32, visit_u64: u64,
        visit_u128: u128, visit_f32: f32, visit_f64: f64, visit_char: char, visit_str: &str,
        visit_borrowed_str: &'de str, visit_string: String, visit_bytes: &[u8],
        visit_borrowed_bytes: &'de [u8], visit_byte_buf: Vec<u8>,
    }

    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        self.visitor.visit_none()
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.visitor.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        self.visitor
            .visit_some(Checked::new(deserializer, self.mode))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<V::Value, D::Error> {
        self.visitor
            .visit_newtype_struct(Checked::new(deserializer, self.mode))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, access: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_seq(WrapAccess {
            access,
            mode: self.mode,
        })
    }

    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_map(WrapAccess {
            access,
            mode: self.mode,
        })
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        visit_tagged(self.visitor, data, self.mode)
    }
}

/// A tagged value, which ayaml hands a visitor as an enum whose variant is
/// the tag, given to `visitor`. A visitor that keeps tags, as a YAML value's
/// does, reads it; one that refuses it without reading the tag, as a number's
/// or an untagged enum's does, refuses the tag, so the error names it.
fn visit_tagged<'de, V: Visitor<'de>, A: EnumAccess<'de>>(
    visitor: V,
    data: A,
    mode: Mode,
) -> Result<V::Value, A::Error> {
    let (tag, variant) = data.variant::<String>()?;
    let engaged = Cell::new(false);
    let replay = Replay {
        tag: &tag,
        variant,
        engaged: &engaged,
        mode,
    };
    visitor.visit_enum(replay).map_err(|error| {
        if engaged.get() {
            error
        } else {
            A::Error::custom(TagRefused(&tag))
        }
    })
}

/// A tagged value's enum access, given again after its tag was read, which
/// records whether the visitor read the tag.
struct Replay<'a, A> {
    tag: &'a str,
    variant: A,
    engaged: &'a Cell<bool>,
    mode: Mode,
}

impl<'de, A: VariantAccess<'de>> EnumAccess<'de> for Replay<'_, A> {
    type Error = A::Error;
    type Variant = WrapAccess<A>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, WrapAccess<A>), A::Error> {
        self.engaged.set(true);
        let tag = seed.deserialize(de::value::StrDeserializer::<A::Error>::new(self.tag))?;
        Ok((
            tag,
            WrapAccess {
                access: self.variant,
                mode: self.mode,
            },
        ))
    }
}

/// A sequence's, a mapping's or a tagged value's access, whose values go
/// through [`Checked`].
struct WrapAccess<A> {
    access: A,
    mode: Mode,
}

impl<A> WrapAccess<A> {
    fn seed<S>(&self, seed: S) -> WrapSeed<S> {
        WrapSeed {
            seed,
            mode: self.mode,
        }
    }
}

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for WrapAccess<A> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        let seed = self.seed(seed);
        self.access.next_element_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.access.size_hint()
    }
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for WrapAccess<A> {
    type Error = A::Error;

    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        let seed = self.seed(seed);
        self.access.next_key_seed(seed)
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        let seed = self.seed(seed);
        self.access.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.access.size_hint()
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for WrapAccess<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.access.unit_variant()
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        let seed = self.seed(seed);
        self.access.newtype_variant_seed(seed)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.access
            .tuple_variant(len, Wrap::new(visitor, self.mode))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.access
            .struct_variant(fields, Wrap::new(visitor, self.mode))
    }
}

/// A seed whose value is read through [`Checked`].
struct WrapSeed<S> {
    seed: S,
    mode: Mode,
}

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for WrapSeed<S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<S::Value, D::Error> {
        self.seed.deserialize(Checked::new(deserializer, self.mode))
    }
}

/// The tag error's message, after the path ayaml puts in front: `has the YAML
/// tag `!!binary`; YAML tags are not supported here`, which [`finish`] turns
/// into `` `path` has the YAML tag ... ``.
struct TagRefused<'a>(&'a str);

impl fmt::Display for TagRefused<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{HAS_THE_TAG}{}`{TAG_REFUSED}", tag_text(self.0))
    }
}

/// The error for a value with the tag `tag` where alint reads no tag, as
/// [`TagRefused`] words it.
pub(crate) fn tag_refused(tag: &ayaml::Tag) -> String {
    format!("{HAS_THE_TAG}{tag}`{TAG_REFUSED}")
}

/// A tag as a document writes it, from its name in serde's data model: a
/// local tag's name, as `celsius` for `!celsius`, and any other tag's full
/// name in angle brackets. A tag in the YAML namespace reads in its short
/// form (`!!binary`), and any other global tag by its full name, as
/// [`crate::yaml::Value::tag`] gives them.
fn tag_text(name: &str) -> String {
    let tag = match name
        .strip_prefix('<')
        .and_then(|rest| rest.strip_suffix('>'))
    {
        Some(full) => ayaml::Tag::new("", full),
        None if name.starts_with('!') => ayaml::Tag::new("", name),
        None => ayaml::Tag::local(name),
    };
    tag.to_string()
}

/// Whether the tag named `name` in serde's data model is in the YAML
/// namespace, as `!!binary` is. ayaml looks past any other tag where a struct,
/// a number or a boolean goes, and refuses one of these.
fn yaml_namespace(name: &str) -> bool {
    name.starts_with("<tag:yaml.org,2002:")
}

/// What a value found where an enum's variant name goes is, for the error.
enum Found<'a> {
    Null,
    Bool(bool),
    Integer(&'a dyn fmt::Display),
    Float(f64),
    Sequence,
    Mapping,
    Tag(&'a str),
}

impl fmt::Display for Found<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => formatter.write_str("null"),
            Self::Bool(value) => write!(formatter, "boolean `{value}`"),
            Self::Integer(value) => write!(formatter, "integer `{value}`"),
            Self::Float(value) => write!(formatter, "float `{value:?}`"),
            Self::Sequence => formatter.write_str("a sequence"),
            Self::Mapping => formatter.write_str("a mapping"),
            Self::Tag(name) => write!(formatter, "a value with the YAML tag `{}`", tag_text(name)),
        }
    }
}

/// An enum's variants as an expectation, as serde lists them for an unknown
/// variant: ``expected `a` ``, ``expected `a` or `b` ``, or ``expected one
/// of `a`, `b`, `c` ``.
struct OneOf(&'static [&'static str]);

impl fmt::Display for OneOf {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            [] => formatter.write_str("expected no value"),
            [only] => write!(formatter, "expected `{only}`"),
            [first, second] => write!(formatter, "expected `{first}` or `{second}`"),
            [first, rest @ ..] => {
                write!(formatter, "expected one of `{first}`")?;
                for variant in rest {
                    write!(formatter, ", `{variant}`")?;
                }
                Ok(())
            }
        }
    }
}

/// The error for `found` where one of `variants` goes.
fn refusal(variants: &'static [&'static str], found: &Found<'_>) -> String {
    format!("{}, found {found}", OneOf(variants))
}

/// The visitor of an enum (see [`Checked::deserialize_enum`]).
struct EnumShape<V> {
    visitor: V,
    variants: &'static [&'static str],
    mode: Mode,
}

impl<V> EnumShape<V> {
    fn refuse<E: de::Error>(&self, found: &Found<'_>) -> E {
        E::custom(refusal(self.variants, found))
    }

    /// A variant named `name`, which came with `data`, the value of a
    /// one-entry mapping or of a tag: a variant without data refuses it, as
    /// does a name that is not a variant's, with the error that `found` gets.
    fn visit_data<'de, A: VariantAccess<'de>>(
        self,
        name: &str,
        data: A,
        found: &Found<'_>,
    ) -> Result<V::Value, A::Error>
    where
        V: Visitor<'de>,
    {
        let refusal = refusal(self.variants, found);
        let named = Cell::new(false);
        let variant = DataVariant {
            name,
            named: &named,
            data,
            refusal: &refusal,
        };
        self.visitor.visit_enum(variant).map_err(|error| {
            if named.get() {
                error
            } else {
                A::Error::custom(&refusal)
            }
        })
    }
}

impl<'de, V: Visitor<'de>> Visitor<'de> for EnumShape<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&OneOf(self.variants), formatter)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<V::Value, E> {
        self.visitor
            .visit_enum(de::value::StrDeserializer::new(value))
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<V::Value, E> {
        self.visitor
            .visit_enum(de::value::BorrowedStrDeserializer::new(value))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<V::Value, E> {
        self.visitor
            .visit_enum(de::value::StringDeserializer::new(value))
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        Err(self.refuse(&Found::Null))
    }

    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        Err(self.refuse(&Found::Null))
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<V::Value, E> {
        Err(self.refuse(&Found::Bool(value)))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<V::Value, E> {
        Err(self.refuse(&Found::Integer(&value)))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<V::Value, E> {
        Err(self.refuse(&Found::Integer(&value)))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<V::Value, E> {
        Err(self.refuse(&Found::Integer(&value)))
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<V::Value, E> {
        Err(self.refuse(&Found::Integer(&value)))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<V::Value, E> {
        Err(self.refuse(&Found::Float(value)))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, _access: A) -> Result<V::Value, A::Error> {
        Err(self.refuse(&Found::Sequence))
    }

    /// A mapping of one entry, its key a variant's name and its value the
    /// variant's data.
    fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<V::Value, A::Error> {
        let Ok(Some(name)) = access.next_key::<String>() else {
            return Err(self.refuse(&Found::Mapping));
        };
        let mode = self.mode;
        self.visit_data(&name, MapData { access, mode }, &Found::Mapping)
    }

    /// A tag that names a variant, its value the variant's data.
    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        let (tag, access) = data.variant::<String>()?;
        let mode = self.mode;
        self.visit_data(&tag, WrapAccess { access, mode }, &Found::Tag(&tag))
    }
}

/// A variant's name and the data it came with (see [`EnumShape::visit_data`]),
/// which records whether the name was a variant's.
struct DataVariant<'a, A> {
    name: &'a str,
    named: &'a Cell<bool>,
    data: A,
    refusal: &'a str,
}

impl<'de, A: VariantAccess<'de>> EnumAccess<'de> for DataVariant<'_, A> {
    type Error = A::Error;
    type Variant = Self;

    fn variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<(S::Value, Self), A::Error> {
        let variant = seed.deserialize(de::value::StrDeserializer::<A::Error>::new(self.name))?;
        self.named.set(true);
        Ok((variant, self))
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for DataVariant<'_, A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        Err(A::Error::custom(self.refusal))
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        self.data.newtype_variant_seed(seed)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.data.tuple_variant(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.data.struct_variant(fields, visitor)
    }
}

/// The rest of a one-entry mapping whose key names a variant: its value, the
/// variant's data, read through [`Checked`].
struct MapData<A> {
    access: A,
    mode: Mode,
}

impl<'de, A: MapAccess<'de>> MapData<A> {
    fn value<S: DeserializeSeed<'de>>(mut self, seed: S) -> Result<S::Value, A::Error> {
        let mode = self.mode;
        let value = self.access.next_value_seed(WrapSeed { seed, mode })?;
        if self.access.next_key::<de::IgnoredAny>()?.is_some() {
            return Err(A::Error::custom(
                "a variant with data is a mapping of one entry, the variant's name and its data",
            ));
        }
        Ok(value)
    }
}

impl<'de, A: MapAccess<'de>> VariantAccess<'de> for MapData<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.value(PhantomData::<()>)
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        self.value(seed)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.value(VariantData::Tuple(len, visitor))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.value(VariantData::Struct(fields, visitor))
    }
}

/// The data of a tuple or a struct variant, read from a mapping's value.
enum VariantData<V> {
    Tuple(usize, V),
    Struct(&'static [&'static str], V),
}

impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for VariantData<V> {
    type Value = V::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        match self {
            Self::Tuple(len, visitor) => deserializer.deserialize_tuple(len, visitor),
            Self::Struct(fields, visitor) => deserializer.deserialize_struct("", fields, visitor),
        }
    }
}

/// An integer wider than 64 bits as serde's `Unexpected` names one.
fn wide_integer(value: &dyn fmt::Display) -> String {
    format!("integer `{value}`")
}

/// The visitor of a string decoded from a value tree, where ayaml takes a
/// string, or a tagged string, and refuses anything else as a `non-string
/// YAML value`: this names what it found instead.
struct StringShape<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for StringShape<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(formatter)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<V::Value, E> {
        self.0.visit_str(value)
    }

    fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<V::Value, E> {
        self.0.visit_borrowed_str(value)
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<V::Value, E> {
        self.0.visit_string(value)
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Unit, &self.0))
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Bool(value), &self.0))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Signed(value), &self.0))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<V::Value, E> {
        Err(E::invalid_type(
            Unexpected::Other(&wide_integer(&value)),
            &self.0,
        ))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Unsigned(value), &self.0))
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<V::Value, E> {
        Err(E::invalid_type(
            Unexpected::Other(&wide_integer(&value)),
            &self.0,
        ))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Float(value), &self.0))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, _access: A) -> Result<V::Value, A::Error> {
        Err(A::Error::invalid_type(Unexpected::Seq, &self.0))
    }

    fn visit_map<A: MapAccess<'de>>(self, _access: A) -> Result<V::Value, A::Error> {
        Err(A::Error::invalid_type(Unexpected::Map, &self.0))
    }

    /// A tagged string is its string, whatever the tag, as ayaml reads it.
    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        let (_, variant) = data.variant::<de::IgnoredAny>()?;
        variant.newtype_variant_seed(StringSeed(self.0))
    }
}

/// The value of a tag where a string goes, read as a string.
struct StringSeed<V>(V);

impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for StringSeed<V> {
    type Value = V::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        Checked::new(deserializer, Mode::Value).deserialize_string(self.0)
    }
}

/// The visitor of a struct decoded from a value tree, where ayaml takes a
/// mapping, past any tag outside the YAML namespace, and refuses anything
/// else, a scalar as a `non-mapping YAML value`: this names what it found
/// instead.
struct StructShape<V> {
    visitor: V,
    name: &'static str,
    fields: &'static [&'static str],
}

impl<'de, V: Visitor<'de>> Visitor<'de> for StructShape<V> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.visitor.expecting(formatter)
    }

    fn visit_map<A: MapAccess<'de>>(self, access: A) -> Result<V::Value, A::Error> {
        self.visitor.visit_map(WrapAccess {
            access,
            mode: Mode::Value,
        })
    }

    fn visit_seq<A: SeqAccess<'de>>(self, _access: A) -> Result<V::Value, A::Error> {
        Err(A::Error::invalid_type(Unexpected::Seq, &self.visitor))
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Unit, &self.visitor))
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Bool(value), &self.visitor))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Signed(value), &self.visitor))
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<V::Value, E> {
        Err(E::invalid_type(
            Unexpected::Other(&wide_integer(&value)),
            &self.visitor,
        ))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Unsigned(value), &self.visitor))
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<V::Value, E> {
        Err(E::invalid_type(
            Unexpected::Other(&wide_integer(&value)),
            &self.visitor,
        ))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Float(value), &self.visitor))
    }

    /// The string is not quoted: it can be the whole document, quoted by
    /// mistake.
    fn visit_str<E: de::Error>(self, _value: &str) -> Result<V::Value, E> {
        Err(E::invalid_type(Unexpected::Other("string"), &self.visitor))
    }

    /// A tag outside the YAML namespace is looked past, as ayaml does, and
    /// one in it refused.
    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        let (tag, variant) = data.variant::<String>()?;
        if yaml_namespace(&tag) {
            return Err(A::Error::custom(TagRefused(&tag)));
        }
        variant.newtype_variant_seed(StructSeed(self))
    }
}

/// The value of a tag where a struct goes, read as the struct.
struct StructSeed<V>(StructShape<V>);

impl<'de, V: Visitor<'de>> DeserializeSeed<'de> for StructSeed<V> {
    type Value = V::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<V::Value, D::Error> {
        let StructShape {
            visitor,
            name,
            fields,
        } = self.0;
        Checked::new(deserializer, Mode::Value).deserialize_struct(name, fields, visitor)
    }
}

/// The hint for an error that is likely an indentation mistake.
pub(crate) const INDENTATION_HINT: &str = "(check this line's indentation against the lines above)";

/// Whether `error`, from decoding `input`, gets [`INDENTATION_HINT`]. A key
/// indented one space less or more than its siblings, `-id: b` without a
/// space, and a top-level key that is indented all end the document's root
/// collection, so ayaml reports what follows as content after the document,
/// which says nothing of indentation. ayaml gives the error no kind of its own
/// (it is a parser error), so the message tells it. The hint goes on that
/// error when the document's root is a block collection, the error is where
/// its line's content starts, and that line:
/// - starts with `-` and no white space after it;
/// - or starts at another column than the content of the nearest line above
///   it that holds any (not blank or a comment): the column after its
///   indentation and any `- ` entries it starts with, where the line's
///   siblings start;
/// - or is the first line with content and is indented.
///
/// A second document, after `---`, is another error, and trailing content
/// after a flow collection has no block collection for its root.
pub(crate) fn indentation_hint(input: &str, error: &ayaml::Error) -> bool {
    if error.kind() != ayaml::ErrorKind::Parser
        || error.message() != "unexpected content after document"
    {
        return false;
    }
    let Some(span) = error.span() else {
        return false;
    };
    let at = span.start();
    let (Some(before), Some(rest)) = (input.get(..at), input.get(at..)) else {
        return false;
    };
    let line_start = before.rfind(['\n', '\r']).map_or(0, |index| index + 1);
    let indent = &before[line_start..];
    if !indent.bytes().all(|byte| byte == b' ') || !root_is_a_block_collection(before) {
        return false;
    }
    let dash_without_space = rest.strip_prefix('-').is_some_and(|after| {
        after
            .chars()
            .next()
            .is_some_and(|next| !matches!(next, ' ' | '\t' | '\n' | '\r'))
    });
    if dash_without_space {
        return true;
    }
    match before[..line_start]
        .split(['\n', '\r'])
        .rev()
        .find(|line| holds_content(line))
    {
        Some(above) => content_column(above) != indent.len(),
        None => !indent.is_empty(),
    }
}

/// Whether the first node of `prefix`, the input up to the error, is a block
/// collection, as ayaml reads it. The prefix holds the first document, which
/// ended before the error.
fn root_is_a_block_collection(prefix: &str) -> bool {
    let Ok(events) = crate::yaml::stream_events(prefix) else {
        return false;
    };
    events.iter().find_map(|event| match event {
        ayaml::Event::StreamStart { .. }
        | ayaml::Event::DocumentStart { .. }
        | ayaml::Event::DocumentEnd { .. } => None,
        ayaml::Event::SequenceStart { style, .. } | ayaml::Event::MappingStart { style, .. } => {
            Some(*style == ayaml::CollectionStyle::Block)
        }
        _ => Some(false),
    }) == Some(true)
}

/// Whether `line` holds content: it is not blank, a comment, a directive or
/// a document marker.
fn holds_content(line: &str) -> bool {
    let text = line.trim_start_matches([' ', '\t']);
    !(text.is_empty()
        || text.starts_with('#')
        || line.starts_with('%')
        || ["---", "..."].iter().any(|marker| {
            line.strip_prefix(marker)
                .is_some_and(|after| after.is_empty() || after.starts_with([' ', '\t']))
        }))
}

/// The column, counted from zero, where the content of `line` starts: past
/// its indentation and any block sequence entries (`- `) it starts with.
fn content_column(line: &str) -> usize {
    let mut column = line.len() - line.trim_start_matches(' ').len();
    let mut rest = &line[column..];
    while let Some(after) = rest.strip_prefix('-')
        && after.starts_with([' ', '\t'])
    {
        let content = after.trim_start_matches([' ', '\t']);
        column += rest.len() - content.len();
        rest = content;
    }
    column
}

#[cfg(test)]
#[path = "yaml_decode_tests.rs"]
mod tests;
