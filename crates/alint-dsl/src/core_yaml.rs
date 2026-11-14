//! The loader holds a configuration as the parser's values, while alint-core's
//! API takes its own YAML types (ADR-0038). A value crosses between the two
//! through Serde, which carries every scalar, collection and tag unchanged.

use serde::Deserialize as _;

/// `value` as alint-core's YAML value.
pub(crate) fn to_core(value: ayaml::Value) -> alint_core::yaml::Value {
    alint_core::yaml::Value::deserialize(value).expect("a parser value reads as alint-core's")
}

/// `value` as the parser's YAML value.
pub(crate) fn from_core(value: alint_core::yaml::Value) -> ayaml::Value {
    alint_core::yaml::from_value(value).expect("alint-core's value reads as the parser's")
}

/// Decode `value` into `T`, with alint-core's error.
pub(crate) fn decode<T: serde::de::DeserializeOwned>(
    value: ayaml::Value,
) -> Result<T, alint_core::yaml::Error> {
    alint_core::yaml::from_value(to_core(value))
}

/// [`alint_core::mask_configuration_field`] for a value the loader holds.
pub(crate) fn mask_configuration_field(
    field: &str,
    value: &mut ayaml::Value,
    numbers: alint_core::NumberMask,
) {
    let mut core = to_core(std::mem::replace(value, ayaml::Value::Null));
    alint_core::mask_configuration_field(field, &mut core, numbers);
    *value = from_core(core);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A value comes back from alint-core's type as it went in: every scalar
    /// type, numbers at their ranges' edges, nested collections, non-string
    /// keys, and tags at any depth.
    #[test]
    fn a_value_crosses_both_ways_unchanged() {
        let document = concat!(
            "null: ~\n",
            "bools: [true, false]\n",
            "ints: [0, -1, 9223372036854775807, -9223372036854775808, 18446744073709551615]\n",
            "floats: [0.5, -0.0, .inf, -.inf, 1e300]\n",
            "strings: ['', 'a b', \"\\u00e9\", '123', 'true']\n",
            "nested: {a: [1, {b: [c]}], 7: seven, [k]: list-key}\n",
            "tagged: !custom {x: !inner 1}\n",
            "core_tag: !!str 5\n",
            "local: !e!tag value\n",
        );
        let document = format!("%TAG !e! tag:example.com,2026:\n---\n{document}");
        let value: ayaml::Value = alint_core::yaml::from_str(&document).unwrap();
        assert_eq!(from_core(to_core(value.clone())), value);
        let nan: ayaml::Value = alint_core::yaml::from_str(".nan").unwrap();
        let back = from_core(to_core(nan));
        assert!(back.as_f64().is_some_and(f64::is_nan), "{back:?}");
    }

    /// Masking through the crossing changes the value as alint-core's masking
    /// does, and leaves the rest of the value as it was.
    #[test]
    fn masking_a_parser_value_matches_alint_core() {
        let value: ayaml::Value =
            alint_core::yaml::from_str("{kind: file_exists, paths: x, when: 'false'}").unwrap();
        let mut masked = value.clone();
        mask_configuration_field("rule", &mut masked, alint_core::NumberMask::Up);
        let mut core: alint_core::yaml::Value = to_core(value);
        alint_core::mask_configuration_field("rule", &mut core, alint_core::NumberMask::Up);
        assert_eq!(masked, from_core(core));
        assert_eq!(masked["kind"], "file_exists");
        assert_eq!(masked["when"], "true");
    }
}
