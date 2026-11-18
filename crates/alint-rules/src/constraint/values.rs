//! How the `constraint` kind compares, names and identifies values (design
//! `v0.23/selectors-and-constraints.md` section 6, "Comparison" and
//! "Identity").

use std::fmt::Write as _;
use std::sync::Arc;

use alint_core::relation::Value;
use alint_core::relation::eval::{Cell, render_value};

/// A value as the kind compares it: its type and its text. A number compares
/// by its lexeme (`1.0` is not `1`), and a path is a string. A missing value
/// (a `read:` that matched nothing) is null: it matches nothing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Scalar {
    Null,
    Bool(bool),
    Number(Arc<str>),
    String(Arc<str>),
}

/// The type of a value that can match: a comparison between two types is an
/// analysis failure where the design says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Kind {
    Bool,
    Number,
    String,
}

impl Scalar {
    /// The cell's value as the kind compares it, or the type it holds that no
    /// comparison takes (`a list`, `a record`, `bytes`).
    pub(super) fn of(cell: &Cell) -> Result<Self, &'static str> {
        match &cell.value {
            None | Some(Value::Null) => Ok(Self::Null),
            Some(value) => Self::of_value(value),
        }
    }

    pub(super) fn of_value(value: &Value) -> Result<Self, &'static str> {
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(flag) => Self::Bool(*flag),
            Value::Number(number) => Self::Number(number.lexeme().into()),
            Value::String(text) => Self::String(Arc::clone(text)),
            Value::Path(path) => Self::String(path.as_str().into()),
            Value::Bytes(_) => return Err("bytes"),
            Value::List(_) => return Err("a list"),
            Value::Record(_) => return Err("a record"),
        })
    }

    /// Its type, when it can match.
    pub(super) fn kind(&self) -> Option<Kind> {
        match self {
            Self::Null => None,
            Self::Bool(_) => Some(Kind::Bool),
            Self::Number(_) => Some(Kind::Number),
            Self::String(_) => Some(Kind::String),
        }
    }

    pub(super) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// The type's word in a baseline key.
    fn type_word(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Number(_) => "number",
            Self::String(_) => "string",
        }
    }

    /// The text a baseline key and `{{row.FIELD}}` hold: a string's text, a
    /// number's lexeme, `true`/`false`, and nothing for null.
    pub(super) fn text(&self) -> &str {
        match self {
            Self::Null => "",
            Self::Bool(true) => "true",
            Self::Bool(false) => "false",
            Self::Number(lexeme) => lexeme,
            Self::String(text) => text,
        }
    }

    fn value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(flag) => Value::Bool(*flag),
            Self::Number(lexeme) => alint_core::relation::Number::from_lexeme(lexeme)
                .map_or_else(|| Value::String(Arc::clone(lexeme)), Value::Number),
            Self::String(text) => Value::String(Arc::clone(text)),
        }
    }

    /// The value as a message quotes it: `"^18.2.0"`, `1.0`, `true`, `null`,
    /// or `<redacted>` for a sensitive field's.
    pub(super) fn show(&self, sensitive: bool) -> String {
        if sensitive {
            "<redacted>".to_string()
        } else {
            render_value(&self.value())
        }
    }

    /// The value with its type, for a message about values of several types:
    /// `the number 1`, `the string "1"`, `null`.
    pub(super) fn show_typed(&self, sensitive: bool) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool(_) => format!("the boolean {}", self.show(sensitive)),
            Self::Number(_) => format!("the number {}", self.show(sensitive)),
            Self::String(_) => format!("the string {}", self.show(sensitive)),
        }
    }
}

/// A finding's `baseline_key`: parts, each written as its byte length in
/// decimal, a colon and the part (`6:unique`), so no value, NUL or separator
/// inside a part can make two keys the same.
#[derive(Debug, Default, Clone)]
pub(super) struct Key(String);

/// What stands for a value, or a key path, that is kept out of baselines.
const SENSITIVE_PART: &str = "sensitive";

impl Key {
    pub(super) fn new(family: &str) -> Self {
        let mut key = Self::default();
        key.part(family);
        key
    }

    pub(super) fn part(&mut self, part: &str) -> &mut Self {
        write!(self.0, "{}:{part}", part.len()).expect("writing to a String cannot fail");
        self
    }

    /// A value: its type and its text, or one part for a sensitive field's
    /// value, which never reaches a baseline.
    pub(super) fn value(&mut self, value: &Scalar, sensitive: bool) -> &mut Self {
        if sensitive {
            self.part(SENSITIVE_PART)
        } else {
            self.part(value.type_word()).part(value.text())
        }
    }

    /// A row: the resource path and the key path of `cell`'s value, or, for
    /// a `read:` row, which has no key path, its part's index in the relation
    /// and its ordinal. `hide_keys` keeps a key path out (a field that holds
    /// keys is sensitive).
    pub(super) fn row(&mut self, cell: &Cell, hide_keys: bool) -> &mut Self {
        let at = &cell.at;
        self.part(at.resource.path().as_str());
        match &at.pointer {
            Some(_) if hide_keys => self.part(SENSITIVE_PART),
            Some(pointer) => self.part(&pointer.render()),
            None => self
                .part(&at.part.to_string())
                .part(&at.ordinal.to_string()),
        }
    }

    pub(super) fn into_string(self) -> String {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_are_length_prefixed_in_decimal() {
        let mut key = Key::new("unique");
        key.value(&Scalar::String("a:b".into()), false)
            .value(&Scalar::Number("1.0".into()), false)
            .value(&Scalar::Null, false)
            .value(&Scalar::Bool(true), false)
            .value(&Scalar::String("secret".into()), true);
        assert_eq!(
            key.into_string(),
            "6:unique6:string3:a:b6:number3:1.04:null0:4:bool4:true9:sensitive"
        );
    }

    #[test]
    fn a_number_keeps_its_lexeme_and_a_path_is_a_string() {
        let number = Scalar::of_value(&Value::Number(
            alint_core::relation::Number::from_lexeme("1.0").unwrap(),
        ))
        .unwrap();
        assert_ne!(number, Scalar::Number("1".into()));
        assert_eq!(number.show(false), "1.0");
        assert_eq!(number.show_typed(false), "the number 1.0");
        assert_eq!(
            Scalar::of_value(&Value::String("x".into())).unwrap().kind(),
            Some(Kind::String)
        );
        assert_eq!(Scalar::of_value(&Value::List(Vec::new())), Err("a list"));
        assert_eq!(Scalar::String("x".into()).show(true), "<redacted>");
    }
}
