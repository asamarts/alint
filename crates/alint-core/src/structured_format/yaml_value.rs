//! YAML -> `serde_json::Value` conversion for [`super::Format::Yaml`]: merge keys
//! applied, custom tags dropped (see [`yaml_to_value`]).

use serde_json::Value;

/// Parse YAML into the family's `serde_json::Value` tree, with two YAML-specific
/// adaptations a plain YAML `-> serde_json::Value` deserialize lacks:
///
/// - **Merge keys** (YAML 1.1 `<<: *base` / `<<: [*a, *b]`) are APPLIED: the
///   merged mapping's keys join the enclosing mapping, explicit keys win, and in
///   a sequence of merges earlier mappings take precedence (yaml.org/type/merge).
///   Left literal, `<<` hid every merged key from a query -- a `yaml_path_equals`
///   false positive and a `yaml_path_absent` bypass. Only a PLAIN `<<` merges
///   (YAML 1.1 resolves the merge type from the plain scalar alone): a quoted
///   `"<<"` / `'<<'` key, or a `<<` whose value is not a mapping (or a sequence
///   of them), stays an ordinary key.
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
    crate::yaml_backend::json_tree(text).map_err(|e| e.to_string())
}

/// The [`yaml_to_value`] seed / visitor: builds a `serde_json::Value`, applying
/// merge keys and stripping custom tags (which the parser surfaces as an
/// enum: variant = tag, newtype payload = the tagged node). Carries the source
/// text so a key can tell a PLAIN `<<` (a merge key) from a quoted `"<<"` / `'<<'`
/// (an ordinary string key, per YAML 1.1: the merge type is resolved only from
/// the plain scalar).
#[derive(Clone, Copy)]
pub(crate) struct YamlJson<'t> {
    src: &'t str,
}

/// The [`yaml_to_value`] seed for `text`, which every parser of
/// [`crate::yaml_backend`] runs.
pub(crate) fn json_seed(text: &str) -> YamlJson<'_> {
    YamlJson { src: text }
}

impl<'de> serde::de::DeserializeSeed<'de> for YamlJson<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Value, D::Error> {
        d.deserialize_any(self)
    }
}

/// A mapping key, read exactly as `serde_json::Value` reads one (`deserialize_str`),
/// plus whether it is a merge key.
struct YamlKey<'t> {
    src: &'t str,
}

/// A key's text and whether it is a merge key (a PLAIN `<<`).
struct Key {
    text: String,
    merge: bool,
}

impl<'de> serde::de::DeserializeSeed<'de> for YamlKey<'_> {
    type Value = Key;
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Key, D::Error> {
        d.deserialize_str(self)
    }
}

impl<'de> serde::de::Visitor<'de> for YamlKey<'_> {
    type Value = Key;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("a string key")
    }
    /// The parser (ayaml's compat layer, as `yaml_serde` before it) lends a
    /// scalar straight out of the source text whenever its value equals its
    /// source span -- always for a single-line plain scalar, and for a quoted
    /// one minus its quotes. So a borrowed `<<` NOT preceded by a
    /// quote is the plain merge key; anything else (quoted, escaped, block
    /// scalar -- the owned-string paths below) is an ordinary `"<<"` key.
    fn visit_borrowed_str<E>(self, v: &'de str) -> Result<Key, E> {
        let merge = v == "<<" && is_yaml_merge_key(self.src.as_bytes(), v);
        Ok(Key {
            text: v.to_owned(),
            merge,
        })
    }
    fn visit_str<E>(self, v: &str) -> Result<Key, E> {
        Ok(Key {
            text: v.to_owned(),
            merge: false,
        })
    }
    fn visit_string<E>(self, v: String) -> Result<Key, E> {
        Ok(Key {
            text: v,
            merge: false,
        })
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

impl<'de> serde::de::Visitor<'de> for YamlJson<'_> {
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
        // Values of PLAIN (or explicit `!!merge`) `<<` keys whose values are
        // mappings (or sequences of them), applied after explicit keys. Collect
        // every merge key rather than letting a later `<<:` overwrite earlier
        // ones; a quoted `"<<"` key without `!!merge`, or a `<<` whose value is
        // not mergeable, stays an ordinary key.
        let mut merges: Vec<Value> = Vec::new();
        while let Some(key) = map.next_key_seed(YamlKey { src: self.src })? {
            let value = map.next_value_seed(self)?;
            let mergeable = match &value {
                Value::Object(_) => true,
                Value::Array(items) => items.iter().all(Value::is_object),
                _ => false,
            };
            if key.merge && mergeable {
                merges.push(value);
            } else {
                out.insert(key.text, value);
            }
        }
        // Apply merge keys after explicit keys. When multiple `<<:` keys appear
        // in one mapping, later merge keys win over earlier ones (matching YAML
        // duplicate-key precedence) while retaining keys from every source;
        // within a single sequence `<<: [*a, *b]`, earlier items win over later.
        for merge in merges.into_iter().rev() {
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

/// Inspect the YAML node properties (tags `!..` and anchors `&..`, possibly
/// spanning preceding property-only lines) right before a borrowed `<<` key:
/// - Explicit `!!merge` / `!<tag:yaml.org,2002:merge>` -> merge key (even when
///   quoted, e.g. `!!merge "<<"`, matching `duplicate_keys.rs`).
/// - Any other explicit tag (`!!str`, `!`, `!custom`, including `!!str &k <<`
///   and multiline `!!str\n  <<`) -> literal string key, NOT a merge.
/// - No explicit tag -> merge key iff unquoted (plain scalar).
fn is_yaml_merge_key(src: &[u8], borrowed: &str) -> bool {
    let Some(off) = (borrowed.as_ptr() as usize)
        .checked_sub(src.as_ptr() as usize)
        .filter(|&off| off + borrowed.len() <= src.len())
    else {
        return false;
    };
    let quoted = off > 0 && matches!(src[off - 1], b'"' | b'\'');
    let prop_end = if quoted { off - 1 } else { off };
    let mut cursor = prop_end;
    let mut explicit_tag: Option<&[u8]> = None;
    loop {
        // Skip whitespace and YAML `# ...` comments on preceding lines.
        let skipped = skip_yaml_ws_and_comments_backwards(src, cursor);
        if skipped == 0 {
            break;
        }
        // Scan backwards across the preceding non-whitespace/non-flow token.
        let tok_end = skipped;
        let mut tok_start = tok_end;
        while tok_start > 0
            && !matches!(
                src[tok_start - 1],
                b' ' | b'\t' | b'\n' | b'\r' | b'{' | b',' | b'['
            )
        {
            tok_start -= 1;
        }
        let tok = &src[tok_start..tok_end];
        if tok.starts_with(b"!") {
            explicit_tag = Some(tok);
            cursor = tok_start;
        } else if tok.starts_with(b"&") {
            // Anchor property (`&k`): keep scanning left for a preceding tag.
            cursor = tok_start;
        } else {
            break;
        }
    }
    match explicit_tag {
        Some(tag) => tag == b"!!merge" || tag == b"!<tag:yaml.org,2002:merge>",
        None => !quoted,
    }
}

fn skip_yaml_ws_and_comments_backwards(src: &[u8], mut pos: usize) -> usize {
    loop {
        while pos > 0 && matches!(src[pos - 1], b' ' | b'\t' | b'\n' | b'\r') {
            pos -= 1;
        }
        // Check if `pos` sits at the end of a `# ...` comment on its line.
        let line_start = src[..pos]
            .iter()
            .rposition(|&b| b == b'\n' || b == b'\r')
            .map_or(0, |i| i + 1);
        let line = &src[line_start..pos];
        if let Some(hash) = line
            .iter()
            .position(|&b| b == b'#')
            .filter(|&i| i == 0 || matches!(line[i - 1], b' ' | b'\t'))
        {
            pos = line_start + hash;
            continue;
        }
        return pos;
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
    fn yaml_quoted_merge_key_is_an_ordinary_key() {
        // Regression: a QUOTED `"<<"` key was applied as a merge key. YAML 1.1
        // resolves the merge type only from the PLAIN `<<` scalar; quoted (or
        // escaped, or block-scalar) `<<` is just a string key.
        let base = "base: &b {x: 1}\n";
        for key in ["\"<<\"", "'<<'", "\"\\x3c<\"", "\"\\u003C<\""] {
            for doc in [
                format!("{base}d:\n  {key}: *b\n  y: 2\n"),
                format!("{base}d: {{{key}: *b, y: 2}}\n"),
            ] {
                let v = Format::Yaml.parse(&doc).unwrap();
                assert_eq!(v["d"], json!({"<<": {"x": 1}, "y": 2}), "{doc:?}");
            }
        }
        let v = Format::Yaml
            .parse(&format!("{base}d:\n  ? >-\n    <<\n  : *b\n"))
            .unwrap();
        assert_eq!(v["d"], json!({"<<": {"x": 1}}));
        // An alias to a quoted `"<<"` scalar is still that quoted scalar.
        let v = Format::Yaml
            .parse(&format!("{base}q: &q \"<<\"\nd: {{*q : *b}}\n"))
            .unwrap();
        assert_eq!(v["d"], json!({"<<": {"x": 1}}));
        // The PLAIN merge key still merges -- block, flow, anchored, after a
        // quoted literal `"<<"` in the same mapping, and as an alias to a plain
        // `<<` scalar.
        for doc in [
            format!("{base}d:\n  <<: *b\n"),
            format!("{base}d: {{<<: *b}}\n"),
            format!("{base}d: {{ <<: *b}}\n"),
            format!("{base}d:\n  &k <<: *b\n"),
            format!("{base}d:\n  ? <<\n  : *b\n"),
        ] {
            let v = Format::Yaml.parse(&doc).unwrap();
            assert_eq!(v["d"], json!({"x": 1}), "{doc:?}");
        }
        let v = Format::Yaml
            .parse(&format!("{base}d:\n  \"<<\": lit\n  <<: *b\n"))
            .unwrap();
        assert_eq!(v["d"], json!({"<<": "lit", "x": 1}));
        let v = Format::Yaml
            .parse(&format!("{base}m: {{&k <<: *b}}\nd: {{*k : *b}}\n"))
            .unwrap();
        assert_eq!(v["d"], json!({"x": 1}));
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
        // YAML `-> serde_json::Value` deserialize, value for value.
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
            let plain = crate::yaml::from_str::<serde_json::Value>(doc).map_err(|e| e.to_string());
            assert_eq!(Format::Yaml.parse(doc), plain, "diverged on {doc:?}");
        }
    }

    #[test]
    fn yaml_merge_key_respects_tags_anchors_multiline_and_multiple_merges() {
        // Explicit `!!str` (with anchor or across lines) disables merge semantics.
        for doc in [
            "base: &b {x: 1}\nd:\n  !!str &k <<: *b\n",
            "base: &b {x: 1}\nd:\n  !!str\n  <<: *b\n",
            "base: &b {x: 1}\nd:\n  !!str # comment\n  <<: *b\n",
        ] {
            let v = Format::Yaml.parse(doc).unwrap();
            assert_eq!(v["d"], json!({"<<": {"x": 1}}), "{doc:?}");
        }
        // Explicit `!!merge` enables merge even on quoted `"<<"`.
        let v = Format::Yaml
            .parse("base: &b {x: 1}\nd:\n  !!merge \"<<\": *b\n  y: 2\n")
            .unwrap();
        assert_eq!(v["d"], json!({"x": 1, "y": 2}));
        // Multiple `<<:` keys in one mapping merge ALL sources rather than dropping earlier ones.
        let v = Format::Yaml
            .parse("a: &a {x: 1, shared: from_a}\nb: &b {y: 2, shared: from_b}\nc:\n  <<: *a\n  <<: *b\n")
            .unwrap();
        assert_eq!(v["c"], json!({"x": 1, "y": 2, "shared": "from_b"}));
    }
}
