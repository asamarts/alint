//! Key-only rows from the duplicate-aware structured entry extractor.

use crate::extract::{EntryRowsError, entry_table};
use crate::relation::adapter::{Adapter, AdapterError, AdapterOutput, check_row_count};
use crate::relation::key_paths::{EntryTable, PathBudget};
use crate::relation::limits::{LimitExceeded, LimitKind, Limits, RunBudget};
use crate::relation::resource::Resource;
use crate::relation::row::{Row, RowState};
use crate::structured_format::Format;

/// Emits one [`RowState::Key`] row per source-order object member.
///
/// The adapter preserves duplicate keys as distinct rows and carries the key's
/// byte span eagerly. It is the key-only half of the relation layer: values are
/// not parsed, because `key_parity` never needs them. The `Extract` wrapper is
/// the value-bearing adapter.
///
/// Every object member is emitted, including an object-valued key such as
/// `nav` in `{"nav":{"home":1}}` and objects nested inside arrays. The key
/// model belongs to the consumer (R3): `key_parity` drops rows whose pointer
/// contains an `Index` segment (arrays are values there) and keeps only leaf
/// pointers.
///
/// TOML, generic XML, and HCL are rejected here; R9 adds the TOML and XML
/// dialect adapters.
#[derive(Debug, Clone, Copy)]
pub struct EntryRowsAdapter {
    format: Format,
}

impl EntryRowsAdapter {
    /// An adapter for one of the five formats `extract_entry_rows` supports.
    pub fn new(format: Format) -> Result<Self, AdapterError> {
        match format {
            Format::Json | Format::Yaml | Format::Dotenv | Format::Ini | Format::Properties => {
                Ok(Self { format })
            }
            Format::Toml | Format::Xml | Format::Hcl => Err(AdapterError::Parse(format!(
                "entry-row extraction is not supported for {}",
                format_name(format)
            ))),
        }
    }

    #[must_use]
    pub fn format(self) -> Format {
        self.format
    }
}

impl Adapter for EntryRowsAdapter {
    fn id(&self) -> &'static str {
        match self.format {
            Format::Json => "entry-json",
            Format::Yaml => "entry-yaml",
            Format::Dotenv => "entry-dotenv",
            Format::Ini => "entry-ini",
            Format::Properties => "entry-properties",
            // `new` rejects these, so an instance can never report one.
            Format::Toml | Format::Xml | Format::Hcl => "entry-unsupported",
        }
    }

    fn redacts_by_default(&self) -> bool {
        matches!(self.format, Format::Dotenv | Format::Properties)
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        self.extract_within(resource, bytes, limits, &RunBudget::bytes(limits))
    }

    fn extract_within(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
        budget: &RunBudget,
    ) -> Result<AdapterOutput, AdapterError> {
        // These rows carry byte spans into the original file. Lossy UTF-8
        // replacement would shift every later span, so this adapter is strict;
        // `ExtractAdapter` is the lossy one, matching `cross_file`.
        let text = std::str::from_utf8(bytes).map_err(|_| {
            AdapterError::Parse(format!(
                "the {} document is not valid UTF-8",
                format_name(self.format)
            ))
        })?;
        let entries = entry_table(self.format, text, limits.get(LimitKind::RowsPerExtractor))
            .map_err(entry_error)?;
        Ok(AdapterOutput::new(key_rows(
            &entries, resource, limits, budget,
        )?))
    }
}

/// A key row per member of `entries`, in source order. Each row's key path
/// is built as it is emitted, its bytes taken from `budget`, the run's
/// ([`PathBudget`]): a document deep and wide enough to hold more key-path
/// bytes than the run may fails when it reaches them, not after, and so
/// does the decode that takes the run's key paths past them.
pub(crate) fn key_rows(
    entries: &EntryTable,
    resource: &Resource,
    limits: &Limits,
    budget: &RunBudget,
) -> Result<Vec<Row>, AdapterError> {
    // Check row count and total per-decode key-path bytes BEFORE allocating
    // `rows` or taking from `RunBudget`: a single deep, wide document that
    // exceeds `KeyPathBytesPerDecode` fails immediately without pre-allocating
    // a huge `Vec<Row>` or permanently draining up to 32 MiB per failure from
    // the run-wide `BytesPerRun` budget.
    check_row_count(limits, entries.rows.len())?;
    let decode_limit = limits.get(LimitKind::KeyPathBytesPerDecode);
    let run_limit = limits.get(LimitKind::BytesPerRun);
    let mut total_pointer_bytes = 0usize;
    for entry in &entries.rows {
        total_pointer_bytes =
            total_pointer_bytes.saturating_add(entries.paths.pointer_bytes(entry.path));
        if run_limit < decode_limit && total_pointer_bytes > run_limit {
            return Err(AdapterError::Limit(LimitExceeded {
                kind: LimitKind::BytesPerRun,
                limit: run_limit,
                reached: total_pointer_bytes,
            }));
        }
        if total_pointer_bytes > decode_limit {
            return Err(AdapterError::Limit(LimitExceeded {
                kind: LimitKind::KeyPathBytesPerDecode,
                limit: decode_limit,
                reached: total_pointer_bytes,
            }));
        }
    }
    let budget = PathBudget::for_key_rows(budget, limits);
    let mut rows = Vec::with_capacity(entries.rows.len());
    for (ordinal, entry) in entries.rows.iter().enumerate() {
        rows.push(
            Row::new(
                resource.clone(),
                Some(budget.pointer(&entries.paths, entry.path)?),
                u32::try_from(ordinal).expect("the row limit is far below u32::MAX"),
                RowState::Key,
            )
            .with_span(entry.key_span),
        );
    }
    Ok(rows)
}

fn entry_error(error: EntryRowsError) -> AdapterError {
    match error {
        EntryRowsError::UnsupportedFormat(format) => AdapterError::Parse(format!(
            "entry-row extraction is not supported for {}",
            format_name(format)
        )),
        EntryRowsError::Parse(message) => {
            // The message can quote document content. Keep it; the consuming
            // rule redacts it when `redacts_by_default` is true, and the IR
            // never formats a finding itself.
            AdapterError::Parse(message)
        }
        EntryRowsError::Limit { limit } => AdapterError::Limit(LimitExceeded {
            kind: LimitKind::RowsPerExtractor,
            limit,
            reached: limit.saturating_add(1),
        }),
    }
}

const fn format_name(format: Format) -> &'static str {
    match format {
        Format::Json => "JSON",
        Format::Yaml => "YAML",
        Format::Toml => "TOML",
        Format::Xml => "XML",
        Format::Dotenv => "dotenv",
        Format::Properties => "Java properties",
        Format::Ini => "INI",
        Format::Hcl => "HCL",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::{RowState, Value};
    use crate::structured_format::Format;

    fn adapter(format: Format) -> EntryRowsAdapter {
        EntryRowsAdapter::new(format).unwrap()
    }

    fn resource(path: &str) -> Resource {
        Resource::for_test(0, path)
    }

    #[test]
    fn json_rows_are_keys_with_pointers_and_spans() {
        let text = r#"{"nav":{"home":"Home","about":"About"}}"#;
        let output = adapter(Format::Json)
            .extract(
                &resource("locales/en.json"),
                text.as_bytes(),
                &Limits::default(),
            )
            .unwrap();
        assert_eq!(output.rows.len(), 3);
        let rendered: Vec<String> = output
            .rows
            .iter()
            .map(|row| row.pointer.as_ref().unwrap().render())
            .collect();
        assert_eq!(rendered, ["nav", "nav.home", "nav.about"]);
        assert!(output.rows.iter().all(Row::is_key));
        assert!(output.rows.iter().all(|row| row.value().is_none()));
        let home = &output.rows[1];
        assert_eq!(
            &text[home.span.unwrap().as_range()],
            "\"home\"",
            "the span points at the key token"
        );
        assert_eq!(output.non_strings.len(), 0);
        assert_eq!(output.notes.len(), 0);
    }

    #[test]
    fn yaml_aliases_and_merge_keys_fail_loudly() {
        for text in [
            "base: &base\n  nested: one\ncopy: *base\n",
            "base: {nested: one}\ncopy:\n  <<: {nested: two}\n",
        ] {
            let err = adapter(Format::Yaml)
                .extract(&resource("a.yml"), text.as_bytes(), &Limits::default())
                .unwrap_err();
            assert!(matches!(err, AdapterError::Parse(_)), "{err}");
            assert!(
                err.to_string().contains("aliases") || err.to_string().contains("merge keys"),
                "{err}"
            );
        }
    }

    #[test]
    fn duplicate_keys_stay_distinct_rows() {
        let text = "a: one\na: two\n";
        let output = adapter(Format::Yaml)
            .extract(&resource("a.yml"), text.as_bytes(), &Limits::default())
            .unwrap();
        assert_eq!(output.rows.len(), 2);
        assert_eq!(output.rows[0].pointer, output.rows[1].pointer);
        assert_ne!(output.rows[0].span, output.rows[1].span);
    }

    /// `key_parity` reads a block scalar key, which has no token span, as
    /// a key where its text starts: it is never dropped.
    #[test]
    fn a_yaml_block_scalar_key_is_a_key_row() {
        let text = "deps:\n  ? |\n    block\n  : 1\n  plain: 2\n";
        let output = adapter(Format::Yaml)
            .extract(&resource("a.yml"), text.as_bytes(), &Limits::default())
            .unwrap();
        let pointers: Vec<String> = output
            .rows
            .iter()
            .map(|row| row.pointer.as_ref().unwrap().render())
            .collect();
        assert_eq!(pointers, ["deps", "deps.block\n", "deps.plain"]);
        let block = output.rows[1].span.unwrap();
        assert!(text[block.start..].starts_with('|'), "{block:?}");
    }

    #[test]
    fn array_members_are_emitted_and_left_to_the_key_model() {
        // `extract_entry_rows` reports objects inside arrays; `key_parity`
        // filters their `Index` pointers because arrays are values there.
        let text = "list:\n  - x: one\n";
        let output = adapter(Format::Yaml)
            .extract(&resource("a.yml"), text.as_bytes(), &Limits::default())
            .unwrap();
        let rendered: Vec<String> = output
            .rows
            .iter()
            .map(|row| row.pointer.as_ref().unwrap().render())
            .collect();
        assert_eq!(rendered, ["list", "list[0].x"]);
    }

    #[test]
    fn dotenv_and_properties_are_sensitive_but_emit_keys() {
        for (format, text) in [
            (Format::Dotenv, "A=1\nB=2\n"),
            (Format::Properties, "a=1\nb=2\n"),
        ] {
            let adapter = adapter(format);
            assert!(adapter.redacts_by_default());
            let output = adapter
                .extract(&resource("x"), text.as_bytes(), &Limits::default())
                .unwrap();
            assert_eq!(output.rows.len(), 2);
            assert!(output.rows.iter().all(Row::is_key));
        }
    }

    #[test]
    fn all_five_entry_formats_emit_the_same_key_shape() {
        let cases = [
            (Format::Json, r#"{"a":1}"#, vec!["a"]),
            (Format::Yaml, "a: 1\n", vec!["a"]),
            (Format::Dotenv, "A=1\n", vec!["A"]),
            (Format::Ini, "[s]\na=1\n", vec!["s.a"]),
            (Format::Properties, "a=1\n", vec!["a"]),
        ];
        for (format, text, want) in cases {
            let output = adapter(format)
                .extract(&resource("x"), text.as_bytes(), &Limits::default())
                .unwrap();
            let got: Vec<String> = output
                .rows
                .iter()
                .map(|row| row.pointer.as_ref().unwrap().render())
                .collect();
            assert_eq!(got, want, "{format:?}");
            assert!(output.rows.iter().all(Row::is_key), "{format:?}");
        }
    }

    #[test]
    fn invalid_utf8_and_unsupported_formats_fail_loudly() {
        let err = adapter(Format::Json)
            .extract(&resource("a.json"), &[0xff], &Limits::default())
            .unwrap_err();
        assert!(err.to_string().contains("not valid UTF-8"), "{err}");
        assert!(EntryRowsAdapter::new(Format::Toml).is_err());
        assert!(EntryRowsAdapter::new(Format::Xml).is_err());
        assert!(EntryRowsAdapter::new(Format::Hcl).is_err());
    }

    #[test]
    fn the_row_limit_is_enforced() {
        let limits = Limits::default()
            .with(crate::relation::LimitKind::RowsPerExtractor, 1)
            .unwrap();
        let err = adapter(Format::Json)
            .extract(&resource("a.json"), br#"{"a":1,"b":2}"#, &limits)
            .unwrap_err();
        assert!(matches!(
            err,
            AdapterError::Limit(crate::relation::LimitExceeded {
                kind: crate::relation::LimitKind::RowsPerExtractor,
                limit: 1,
                reached: 2
            })
        ));
    }

    #[test]
    fn a_key_row_is_not_an_explicit_null() {
        let output = adapter(Format::Json)
            .extract(&resource("a.json"), br#"{"a":null}"#, &Limits::default())
            .unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].state, RowState::Key);
        assert_ne!(output.rows[0].state, RowState::Value(Value::Null));
    }
}
