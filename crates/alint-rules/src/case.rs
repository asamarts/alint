//! Case-convention detectors used by `filename_case` and `path_case`.
//!
//! Each detector checks whether a string already conforms to the named
//! convention. Semantics follow ls-lint's well-established interpretations
//! where they exist (see <https://ls-lint.org/2.3/configuration/rules.html>):
//!
//! - `lowercase` / `uppercase` — every *letter* is lower/upper; non-letters
//!   are permitted (so `hello_world` is lowercase because every letter is).
//! - `flat` — lowercase ASCII letters and digits only, no separators.
//! - `snake`, `kebab`, `screaming-snake` — the obvious ASCII conventions.
//! - `camel` — starts with an ASCII lowercase letter; all remaining
//!   characters are ASCII alphanumeric. Consecutive uppercase letters are
//!   permitted so common acronym styles like `ssrVFor` and `getXMLParser`
//!   match. Use `filename_regex` if you need stricter semantics.
//! - `pascal` — same rule as camel, but the first character is ASCII
//!   uppercase.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseConvention {
    Lower,
    Upper,
    Pascal,
    Camel,
    Snake,
    Kebab,
    ScreamingSnake,
    Flat,
}

impl CaseConvention {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Lower => "lowercase",
            Self::Upper => "uppercase",
            Self::Pascal => "PascalCase",
            Self::Camel => "camelCase",
            Self::Snake => "snake_case",
            Self::Kebab => "kebab-case",
            Self::ScreamingSnake => "SCREAMING_SNAKE_CASE",
            Self::Flat => "flatcase",
        }
    }

    pub fn check(self, s: &str) -> bool {
        if s.is_empty() {
            return false;
        }
        match self {
            Self::Lower => is_lowercase(s),
            Self::Upper => is_uppercase(s),
            Self::Pascal => is_pascal(s),
            Self::Camel => is_camel(s),
            Self::Snake => is_snake(s),
            Self::Kebab => is_kebab(s),
            Self::ScreamingSnake => is_screaming_snake(s),
            Self::Flat => is_flat(s),
        }
    }
}

/// Accept `PascalCase`, `pascal`, `pascal-case`, `pascalcase`, `pascal_case`
/// as equivalent. Any separator character or case variation normalizes to a
/// single canonical form before matching.
impl<'de> Deserialize<'de> for CaseConvention {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw: String = String::deserialize(d)?;
        let canon: String = raw
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        match canon.as_str() {
            "lower" | "lowercase" => Ok(Self::Lower),
            "upper" | "uppercase" => Ok(Self::Upper),
            "pascal" | "pascalcase" | "uppercamel" | "uppercamelcase" => Ok(Self::Pascal),
            "camel" | "camelcase" | "lowercamel" | "lowercamelcase" => Ok(Self::Camel),
            "snake" | "snakecase" => Ok(Self::Snake),
            "kebab" | "kebabcase" | "dash" | "dashcase" => Ok(Self::Kebab),
            "screamingsnake" | "screamingsnakecase" | "upper_snake" | "uppersnakecase" => {
                Ok(Self::ScreamingSnake)
            }
            "flat" | "flatcase" => Ok(Self::Flat),
            other => Err(serde::de::Error::custom(format!(
                "unknown case convention {raw:?} (normalized to {other:?})",
            ))),
        }
    }
}

fn is_lowercase(s: &str) -> bool {
    s.chars().all(|c| !c.is_alphabetic() || c.is_lowercase())
}

fn is_uppercase(s: &str) -> bool {
    s.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
}

fn is_flat(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn is_snake(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_kebab(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_screaming_snake(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn is_camel(s: &str) -> bool {
    check_camel_like(s, /* require_upper_first = */ false)
}

fn is_pascal(s: &str) -> bool {
    check_camel_like(s, /* require_upper_first = */ true)
}

fn check_camel_like(s: &str, require_upper_first: bool) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if require_upper_first {
        if !first.is_ascii_uppercase() {
            return false;
        }
    } else if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pascal_accepts_simple() {
        assert!(is_pascal("Button"));
        assert!(is_pascal("FooBar"));
        assert!(is_pascal("Foo1Bar"));
        assert!(is_pascal("A"));
        assert!(is_pascal("XMLParser")); // consecutive uppercase allowed
    }

    #[test]
    fn pascal_rejects_wrong_shapes() {
        assert!(!is_pascal(""));
        assert!(!is_pascal("foo"));
        assert!(!is_pascal("Foo_Bar"));
        assert!(!is_pascal("Foo-Bar"));
    }

    #[test]
    fn camel_accepts_simple() {
        assert!(is_camel("fooBar"));
        assert!(is_camel("foo"));
        assert!(is_camel("ssrVFor"));
        assert!(is_camel("getXMLParser")); // acronym run allowed
        assert!(is_camel("foo1Bar"));
    }

    #[test]
    fn camel_rejects_wrong_shapes() {
        assert!(!is_camel("FooBar"));
        assert!(!is_camel(""));
        assert!(!is_camel("foo_bar"));
    }

    #[test]
    fn snake_kebab() {
        assert!(is_snake("foo_bar_baz"));
        assert!(!is_snake("fooBar"));
        assert!(is_kebab("foo-bar-baz"));
        assert!(!is_kebab("foo_bar"));
    }

    #[test]
    fn screaming_snake() {
        assert!(is_screaming_snake("FOO_BAR"));
        assert!(is_screaming_snake("HELLO_2_WORLD"));
        assert!(!is_screaming_snake("Foo_Bar"));
    }

    #[test]
    fn flat_vs_lower() {
        assert!(is_flat("helloworld"));
        assert!(!is_flat("hello_world"));
        assert!(is_lowercase("hello_world")); // permissive: letters-only check
    }

    #[test]
    fn alias_deserialization() {
        use serde_yaml_ng::from_str;
        let cases = &[
            ("PascalCase", CaseConvention::Pascal),
            ("pascal", CaseConvention::Pascal),
            ("pascal-case", CaseConvention::Pascal),
            ("UpperCamelCase", CaseConvention::Pascal),
            ("camelCase", CaseConvention::Camel),
            ("camel", CaseConvention::Camel),
            ("kebab-case", CaseConvention::Kebab),
            ("KEBAB", CaseConvention::Kebab),
            ("snake_case", CaseConvention::Snake),
            ("SCREAMING_SNAKE_CASE", CaseConvention::ScreamingSnake),
            ("flatcase", CaseConvention::Flat),
        ];
        for (input, expected) in cases {
            let parsed: CaseConvention = from_str(&format!("\"{input}\"")).unwrap();
            assert_eq!(parsed, *expected, "input = {input}");
        }
    }
}
