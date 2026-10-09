//! YAML -> `serde_json::Value` conversion for [`super::Format::Yaml`]: merge keys
//! applied, custom tags dropped (see [`yaml_to_value`]).

use serde_json::Value;

/// Parse YAML into the family's `serde_json::Value` tree, with two YAML-specific
/// adaptations a plain `serde_yaml_ng -> serde_json::Value` deserialize lacks:
///
/// - **Merge keys** (YAML 1.1 `<<: *base` / `<<: [*a, *b]`) are APPLIED: the
///   merged mapping's keys join the enclosing mapping, explicit keys win, and in
///   a sequence of merges earlier mappings take precedence (yaml.org/type/merge).
///   Left literal, `<<` hid every merged key from a query -- a `yaml_path_equals`
///   false positive and a `yaml_path_absent` bypass. A `<<` whose value is not a
///   mapping (or a sequence of them) stays an ordinary key.
/// - **Custom tags** (AWS `CloudFormation` `!Ref` / `!GetAtt` / `!Sub`, GitLab CI
///   `!reference`, ...) are DROPPED and the tagged value kept, so `!Ref Bucket`
///   queries as `"Bucket"` and `!GetAtt [B, Arn]` as `["B", "Arn"]`. (`serde_json`
///   has no tag representation; previously any tag made the whole file a parse
///   error.) Core tags (`!!str`, `!!int`, ...) keep their usual meaning.
///
/// Everything else is exactly the plain deserialize -- scalars go through
/// `serde_json::Value`'s own visitor, keys are read as strings the same way, and a
/// duplicate key keeps its last value -- which a test pins on untagged documents.
pub(super) fn yaml_to_value(text: &str) -> std::result::Result<Value, String> {
    use serde::de::DeserializeSeed as _;
    YamlJson
        .deserialize(serde_yaml_ng::Deserializer::from_str(text))
        .map_err(|e| e.to_string())
}

/// The [`yaml_to_value`] seed / visitor: builds a `serde_json::Value`, applying
/// merge keys and stripping custom tags (which `serde_yaml_ng` surfaces as an
/// enum: variant = tag, newtype payload = the tagged node).
#[derive(Clone, Copy)]
struct YamlJson;

impl<'de> serde::de::DeserializeSeed<'de> for YamlJson {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

/// A mapping key, read exactly as `serde_json::Value` reads one (`deserialize_str`).
struct YamlKey;

impl<'de> serde::de::DeserializeSeed<'de> for YamlKey {
    type Value = String;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        d.deserialize_str(self)
    }
}

impl serde::de::Visitor<'_> for YamlKey {
    type Value = String;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a string key")
    }
    fn visit_str<E>(self, v: &str) -> Result<String, E> {
        Ok(v.to_owned())
    }
    fn visit_string<E>(self, v: String) -> Result<String, E> {
        Ok(v)
    }
}

/// Scalars are delegated to `serde_json::Value`'s own visitor, so number / null /
/// non-finite-float handling is identical to the plain deserialize.
macro_rules! yaml_json_scalar {
    ($($method:ident($ty:ty) => $de:ident;)*) => {$(
        fn $method<E: serde::de::Error>(self, v: $ty) -> Result<Value, E> {
            serde::Deserialize::deserialize(serde::de::value::$de::<E>::new(v))
        }
    )*};
}

impl<'de> serde::de::Visitor<'de> for YamlJson {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("any YAML value")
    }
    yaml_json_scalar! {
        visit_bool(bool) => BoolDeserializer;
        visit_i64(i64) => I64Deserializer;
        visit_u64(u64) => U64Deserializer;
        visit_i128(i128) => I128Deserializer;
        visit_u128(u128) => U128Deserializer;
        visit_f64(f64) => F64Deserializer;
    }
    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Value, E> {
        Ok(Value::String(v.to_owned()))
    }
    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Value, E> {
        Ok(Value::String(v))
    }
    fn visit_none<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_some<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_newtype_struct<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = seq.next_element_seed(self)? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        let mut out = serde_json::Map::new();
        while let Some(key) = map.next_key_seed(YamlKey)? {
            let value = map.next_value_seed(self)?;
            out.insert(key, value);
        }
        // Apply a merge key. The merged values were built by this same visitor,
        // so their own merges are already resolved (nested merges compose).
        let mergeable = match out.get("<<") {
            Some(Value::Object(_)) => true,
            Some(Value::Array(items)) => items.iter().all(Value::is_object),
            _ => false,
        };
        if mergeable && let Some(merge) = out.remove("<<") {
            let sources = match merge {
                Value::Array(items) => items,
                single => vec![single],
            };
            for source in sources {
                if let Value::Object(entries) = source {
                    for (k, v) in entries {
                        out.entry(k).or_insert(v);
                    }
                }
            }
        }
        Ok(Value::Object(out))
    }
    fn visit_enum<A: serde::de::EnumAccess<'de>>(self, data: A) -> Result<Value, A::Error> {
        use serde::de::VariantAccess as _;
        // A custom tag: drop it (the variant name) and keep the tagged node.
        let (_tag, variant) = data.variant::<serde::de::IgnoredAny>()?;
        variant.newtype_variant_seed(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::structured_format::Format;
    use serde_json::json;

    #[test]
    fn yaml_merge_keys_are_applied_not_kept_as_a_literal_key() {
        // Regression: `<<: *base` stayed a literal "<<" key, so
        // `yaml_path_equals $.derived.x` was a false positive and
        // `yaml_path_absent $.derived.secret` a bypass (the merged key hid
        // under "<<"). YAML 1.1 merge keys are applied; explicit keys win.
        let doc = "base: &b {x: 1, y: 2, secret: s}\n\
                   derived:\n  <<: *b\n  y: 3\n";
        let v = Format::Yaml.parse(doc).unwrap();
        assert_eq!(v["derived"]["x"], json!(1));
        assert_eq!(v["derived"]["y"], json!(3), "an explicit key overrides");
        assert_eq!(v["derived"]["secret"], json!("s"));
        assert!(v["derived"].get("<<").is_none(), "no literal merge key");
        // A sequence of merges: earlier mappings take precedence.
        let doc = "a: &a {k: 1}\nb: &b {k: 2, j: 2}\nc:\n  <<: [*a, *b]\n";
        let v = Format::Yaml.parse(doc).unwrap();
        assert_eq!(v["c"], json!({"k": 1, "j": 2}));
        // Nested merges resolve through every level.
        let doc = "base: &b {x: 1}\nmid: &m {<<: *b, y: 2}\ntop: {<<: *m, z: 3}\n";
        let v = Format::Yaml.parse(doc).unwrap();
        assert_eq!(v["top"], json!({"x": 1, "y": 2, "z": 3}));
        // A `<<` whose value is not a mapping is kept as an ordinary key.
        let v = Format::Yaml.parse("a:\n  <<: plain\n").unwrap();
        assert_eq!(v["a"]["<<"], json!("plain"));
    }

    #[test]
    fn yaml_custom_tags_are_stripped_not_a_parse_error() {
        // Regression: a custom tag (CloudFormation `!Ref`/`!GetAtt`, GitLab CI
        // `!reference`) made the whole file a parse error. The tag is dropped
        // and the tagged value kept.
        let doc = "Resources:\n  B:\n    Properties:\n      Name: !Ref BucketName\n      \
                   Arn: !GetAtt [B, Arn]\n      Sub: !Sub\n        - x-${A}\n        - {A: 1}\n\
                   job:\n  script: !reference [.setup, script]\n";
        let v = Format::Yaml.parse(doc).unwrap();
        let p = &v["Resources"]["B"]["Properties"];
        assert_eq!(p["Name"], json!("BucketName"));
        assert_eq!(p["Arn"], json!(["B", "Arn"]));
        assert_eq!(p["Sub"], json!(["x-${A}", {"A": 1}]));
        assert_eq!(v["job"]["script"], json!([".setup", "script"]));
        // Core tags keep their usual meaning.
        let v = Format::Yaml
            .parse("a: !!str 123\nb: !!int \"7\"\n")
            .unwrap();
        assert_eq!(v["a"], json!("123"));
    }

    #[test]
    fn yaml_conversion_matches_the_plain_serde_path_on_untagged_documents() {
        // The tag/merge-aware conversion must not change anything else: on
        // documents without tags or merge keys it equals a plain
        // `serde_yaml_ng -> serde_json::Value` deserialize, value for value.
        let docs = [
            "a: 1\nb: -2\nc: 1.5\nd: 18446744073709551615\ne: .inf\nf: .nan\ng: ~\nh: true\n",
            "a: 'q'\nb: \"x\\ty\"\nc: yes\nd: 0x1F\ne: 2002-12-14\nf: 1e3\n",
            "list: [1, two, {three: 3}]\nnested: {a: {b: [null, false]}}\n",
            "1: int-key\ntrue: bool-key\n3.5: float-key\n",
            "a: 1\na: 2\n",
            "- x\n- y: [1, 2]\n",
            "anchor: &a [1, 2]\nref: *a\n",
            "plain\n",
            "s: |\n  multi\n  line\nf: >-\n  folded\n  text\n",
        ];
        for doc in docs {
            let plain =
                serde_yaml_ng::from_str::<serde_json::Value>(doc).map_err(|e| e.to_string());
            assert_eq!(Format::Yaml.parse(doc), plain, "diverged on {doc:?}");
        }
    }
}
