//! The YAML entry points every alint crate decodes through, so each document
//! is read under the same parser limits: ayaml's defaults, without its caps on
//! events, collection entries, anchors and alias uses. The input cap (32 MiB),
//! the depth cap (128) and the alias-expansion allowance stay.

/// The parser limits every decode runs under: ayaml's defaults, without its
/// caps on events, collection entries, anchors and alias uses. The parser
/// v0.22 used had none of those caps, and ayaml's default of 1,000,000 events
/// refuses a valid file of a few megabytes, far below the input cap. The
/// input cap (32 MiB, as [`crate::MAX_STRUCTURED_BYTES`]), the depth cap
/// (128) and the alias-expansion budget stay, and together they bound what
/// one parse can cost.
fn limits() -> ayaml::Limits {
    ayaml::Limits::default()
        .with_max_events(usize::MAX)
        .with_max_collection_entries(usize::MAX)
        .with_max_anchors(usize::MAX)
        .with_max_alias_dereferences(usize::MAX)
}

/// Decode one YAML document into `T`, under alint's parser limits.
///
/// # Errors
///
/// When the input is not one valid YAML document, exceeds a limit, or does
/// not decode into `T`.
pub fn from_str<'de, T: serde::Deserialize<'de>>(input: &'de str) -> Result<T, ayaml::Error> {
    ayaml::compat::from_str_with_limits(input, &limits())
}

/// Decode one YAML document through `seed`, under alint's parser limits.
pub(crate) fn from_str_seed<'de, S: serde::de::DeserializeSeed<'de>>(
    input: &'de str,
    seed: S,
) -> Result<S::Value, ayaml::Error> {
    ayaml::compat::from_str_seed_with_limits(input, &limits(), seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;

    /// A valid document with more events, entries, anchors and alias uses
    /// than ayaml's defaults allow (1,000,000, 1,000,000, 100,000 and
    /// 100,000) decodes, as it did in v0.22. Each count sits just past its
    /// default, so dropping any one override fails here.
    #[test]
    fn a_large_valid_document_decodes() {
        // 1,000,001 entries in one flow sequence: past the entries default
        // and, with the sequence's own events, the events default.
        let mut entries = String::with_capacity(2_100_000);
        entries.push('[');
        for i in 0..1_000_001 {
            if i > 0 {
                entries.push(',');
            }
            entries.push('0');
        }
        entries.push(']');
        let decoded: Vec<u8> = from_str(&entries).unwrap();
        assert_eq!(decoded.len(), 1_000_001);

        // 100,001 anchors, and 100,001 uses of one of them.
        let mut anchors = String::with_capacity(4_000_000);
        anchors.push_str("a: &x 0\nanchored: [");
        for i in 0..100_001 {
            if i > 0 {
                anchors.push(',');
            }
            write!(anchors, "&a{i} 0").unwrap();
        }
        anchors.push_str("]\nused: [");
        for i in 0..100_001 {
            if i > 0 {
                anchors.push(',');
            }
            anchors.push_str("*x");
        }
        anchors.push_str("]\n");
        let decoded: std::collections::BTreeMap<String, ayaml::Value> = from_str(&anchors).unwrap();
        assert_eq!(decoded.len(), 3);
    }

    /// The caps that stay still refuse: depth past 128, and input past the
    /// 32 MiB cap.
    #[test]
    fn the_depth_and_input_caps_still_refuse() {
        let deep = format!("{}{}", "[".repeat(129), "]".repeat(129));
        assert!(from_str::<ayaml::Value>(&deep).is_err());
        let large = format!("a: \"{}\"\n", "x".repeat(crate::MAX_STRUCTURED_BYTES));
        assert!(from_str::<ayaml::Value>(&large).is_err());
    }
}
