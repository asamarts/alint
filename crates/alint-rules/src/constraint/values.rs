//! How the `constraint` kind compares, names and identifies values (design
//! `v0.23/selectors-and-constraints.md` section 6, "Comparison" and
//! "Identity").

use std::fmt::Write as _;
use std::sync::Arc;

use alint_core::relation::Value;
use alint_core::relation::eval::{Cell, Provenance, render_value};
use sha2::{Digest as _, Sha256};

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

    /// The text a baseline key holds: a string's text, a number's lexeme,
    /// `true`/`false`, and nothing for null.
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

    /// The text `{{row.FIELD}}` shows: [`Self::text`], cut as a message cuts
    /// a quoted value, at [`SHOWN_CHARS`] characters with its length named
    /// (`abc... (1048576 bytes)`), and each character a quoted value escapes
    /// (a control, a bidi control or a zero-width character) escaped as it
    /// is there (`\n`, `\u{202e}`), so a value can neither add a line to a
    /// report nor reorder what a reader sees.
    pub(super) fn text_shown(&self) -> String {
        let text = self.text();
        let (kept, cut) = match text.char_indices().nth(SHOWN_CHARS) {
            Some((end, _)) => (&text[..end], true),
            None => (text, false),
        };
        let mut shown = String::with_capacity(kept.len());
        for c in kept.chars() {
            if escaped_in_message(c) {
                shown.extend(c.escape_debug());
            } else {
                shown.push(c);
            }
        }
        if cut {
            write!(shown, "... ({} bytes)", text.len()).expect("writing to a String cannot fail");
        }
        shown
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

/// Whether a message writes `c` escaped, as a string's `Debug` form does,
/// which is how a message quotes a value (`render_value`, and a key path in
/// `alint_core`'s relation messages): a control character (`\n`), a bidi
/// control (`\u{202e}`), a zero-width character and the others that form
/// escapes. Quotes and `\` are not: text shown unquoted keeps them as
/// written.
fn escaped_in_message(c: char) -> bool {
    !matches!(c, '"' | '\'' | '\\') && c.escape_debug().nth(1).is_some()
}

/// A finding's `baseline_key`: parts, each written as its byte length in
/// decimal, a colon and the part (`6:unique`), so no value, NUL or separator
/// inside a part can make two keys the same.
#[derive(Debug, Default, Clone)]
pub(super) struct Key(String);

/// What stands for a value, or a key path, that is kept out of baselines.
const SENSITIVE_PART: &str = "sensitive";

/// The longest text a key holds as it is; a longer value's key holds its
/// SHA-256 digest, so a key stays short however long the value (a `set`
/// copies a value the reference group holds into the key of each group that
/// lacks it).
const KEY_TEXT_BYTES: usize = 128;

/// The characters of a value `{{row.FIELD}}` shows, as `render_value`
/// quotes a value in a message.
const SHOWN_CHARS: usize = 64;

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

    /// A value: its type and its text (its type and its digest, past
    /// [`KEY_TEXT_BYTES`]), or one part for a sensitive field's value, which
    /// never reaches a baseline, not even as a digest: a digest of a short
    /// secret can be found by trying values.
    pub(super) fn value(&mut self, value: &Scalar, sensitive: bool) -> &mut Self {
        let text = value.text();
        if sensitive {
            self.part(SENSITIVE_PART)
        } else if text.len() > KEY_TEXT_BYTES {
            let digest = Sha256::digest(text.as_bytes());
            let mut hex = String::with_capacity(digest.len() * 2);
            for byte in digest {
                write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
            }
            self.part(&format!("{}-sha256", value.type_word()))
                .part(&hex)
        } else {
            self.part(value.type_word()).part(text)
        }
    }

    /// A row, by where the row itself came from (`at`, its entry's, never a
    /// value it copies from elsewhere): the resource path and the entry's key
    /// path, or, for a `read:` row, which has no key path, its part's index
    /// in the relation and its ordinal. `hide_keys` keeps a key path out (a
    /// field that holds keys is sensitive).
    pub(super) fn row(&mut self, at: &Provenance, hide_keys: bool) -> &mut Self {
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

    #[test]
    fn a_long_value_is_a_digest_in_a_key_and_cut_where_a_message_shows_it() {
        let text = "x".repeat(KEY_TEXT_BYTES + 1);
        let mut hex = String::new();
        for byte in Sha256::digest(text.as_bytes()) {
            write!(hex, "{byte:02x}").unwrap();
        }
        let long = Scalar::String(text.into());
        let mut key = Key::new("set");
        key.value(&long, false);
        assert_eq!(key.into_string(), format!("3:set13:string-sha25664:{hex}"));
        // A value at the bound is held as it is, and a sensitive one never
        // as a digest.
        let short = Scalar::String("x".repeat(KEY_TEXT_BYTES).into());
        let mut key = Key::new("set");
        key.value(&short, false).value(&long, true);
        assert_eq!(
            key.into_string(),
            format!("3:set6:string128:{}9:sensitive", "x".repeat(KEY_TEXT_BYTES))
        );
        // `{{row.FIELD}}` cuts as a quoted value is cut.
        assert_eq!(
            long.text_shown(),
            format!(
                "{}... ({} bytes)",
                "x".repeat(SHOWN_CHARS),
                KEY_TEXT_BYTES + 1
            )
        );
        assert_eq!(
            Scalar::String("é".repeat(64).into()).text_shown(),
            "é".repeat(64)
        );
    }

    /// `{{row.FIELD}}` escapes what a quoted value escapes, and only that:
    /// quotes and `\` stay as written.
    #[test]
    fn row_text_escapes_what_a_quoted_value_escapes() {
        let value = Scalar::String("a\n  ~ related: x\u{1b}[31m\u{202e}b\u{200b}\"q\" C:\\".into());
        assert_eq!(
            value.text_shown(),
            "a\\n  ~ related: x\\u{1b}[31m\\u{202e}b\\u{200b}\"q\" C:\\"
        );
        for c in [
            '\n', '\t', '\u{1b}', '\u{202e}', '\u{2066}', '\u{200b}', '\u{feff}',
        ] {
            let quoted = render_value(&Value::String(c.to_string().into()));
            assert_eq!(
                Scalar::String(c.to_string().into()).text_shown(),
                quoted[1..quoted.len() - 1],
                "{c:?} as a quoted value shows it"
            );
        }
        // Cut first, then escaped: the cut counts characters of the value.
        let long = Scalar::String(format!("{}{}", "\n".repeat(SHOWN_CHARS), "x").into());
        assert_eq!(
            long.text_shown(),
            format!(
                "{}... ({} bytes)",
                "\\n".repeat(SHOWN_CHARS),
                SHOWN_CHARS + 1
            )
        );
    }
}
