//! The value-bearing wrapper over today's [`Extract`].

use crate::extract::{Extract, ExtractFailure, extract_values_bounded_typed, is_non_literal};
use crate::relation::adapter::{
    Adapter, AdapterError, AdapterNote, AdapterOutput, check_row_count,
};
use crate::relation::limits::{LimitExceeded, LimitKind, Limits};
use crate::relation::resource::Resource;
use crate::relation::row::{Row, RowState};
use crate::relation::value::Value;

/// Wraps today's string-only extraction as a relation adapter.
///
/// It reproduces `cross_file`'s decoding contract: lossy UTF-8, capture group 1
/// for `regex:`, trimmed non-comment lines for `lines:`, and the non-literal
/// skip for every extractor but `whole_file`. Non-string matches stay typed in
/// [`AdapterOutput::non_strings`] so the consuming rule can render the existing
/// one-finding summary, and skipped computed values stay in
/// [`AdapterOutput::notes`] so the rule can redact and report them.
///
/// The adapter id includes the extractor's specification, so two different
/// queries over the same file never share a decode cache entry.
///
/// The wrapper is generic. `cross_file` additionally filters target regex
/// captures by syntax (Dockerfile `FROM`, `__version__`, and similar); the
/// shadow port (R4) applies that filter around the adapter, not inside it.
#[derive(Debug, Clone)]
pub struct ExtractAdapter {
    extract: Extract,
    id: String,
}

impl ExtractAdapter {
    #[must_use]
    pub fn new(extract: Extract) -> Self {
        let id = spec_id(&extract);
        Self { extract, id }
    }

    #[must_use]
    pub fn extractor(&self) -> &Extract {
        &self.extract
    }
}

impl Adapter for ExtractAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn redacts_by_default(&self) -> bool {
        self.extract.redacts_by_default()
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        // `cross_file` reads every file through `String::from_utf8_lossy`;
        // the wrapper must stay byte-compatible with it.
        let text = String::from_utf8_lossy(bytes);
        let max = limits.get(LimitKind::RowsPerExtractor);
        let values =
            extract_values_bounded_typed(&self.extract, &text, max).map_err(
                |error| match error {
                    ExtractFailure::Parse(message) => AdapterError::Parse(message),
                    ExtractFailure::Limit { limit, reached } => {
                        AdapterError::Limit(LimitExceeded {
                            kind: LimitKind::RowsPerExtractor,
                            limit,
                            reached,
                        })
                    }
                },
            )?;

        let mut rows = Vec::new();
        let mut notes = Vec::new();
        let skip_non_literal = !matches!(self.extract, Extract::WholeFile);
        for value in values.strings {
            if skip_non_literal && is_non_literal(&value) {
                notes.push(AdapterNote::NonLiteral { value });
                continue;
            }
            check_row_count(limits, rows.len() + 1)?;
            rows.push(Row::new(
                resource.clone(),
                None,
                u32::try_from(rows.len()).expect("the row limit is far below u32::MAX"),
                RowState::Value(Value::String(value.into())),
            ));
        }
        Ok(AdapterOutput {
            rows,
            non_strings: values.non_strings,
            notes,
        })
    }
}

fn spec_id(extract: &Extract) -> String {
    match extract {
        Extract::Structured(extract) => {
            format!("extract:structured:{:?}:{}", extract.format, extract.query)
        }
        Extract::Lines(options) => format!("extract:lines:{}", options.comment),
        Extract::Regex(pattern) => format!("extract:regex:{pattern}"),
        Extract::WholeFile => "extract:whole_file".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{LinesOpts, NonStringValueKind};
    use crate::structured_format::Format;

    fn resource(path: &str) -> Resource {
        Resource::for_test(0, path)
    }

    fn run(extract: Extract, text: &str) -> Result<AdapterOutput, AdapterError> {
        ExtractAdapter::new(extract).extract(
            &resource("a.json"),
            text.as_bytes(),
            &Limits::default(),
        )
    }

    #[test]
    fn structured_strings_become_rows_and_non_strings_stay_typed() {
        let extract = Extract::structured(Format::Json, "$.items[*]").unwrap();
        let output = run(extract, r#"{"items":["a",1,null]}"#).unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].value(), Some(&Value::String("a".into())));
        assert_eq!(
            output.non_strings,
            [NonStringValueKind::Number, NonStringValueKind::Null]
        );
        assert_eq!(output.notes.len(), 0);
    }

    #[test]
    fn non_literal_values_are_notes_not_rows() {
        let extract = Extract::Regex(r"(?m)^v=(.*)$".to_string());
        let output = run(extract, "v=${VERSION}\nv=1.2.3\n").unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].value(), Some(&Value::String("1.2.3".into())));
        assert_eq!(
            output.notes,
            [AdapterNote::NonLiteral {
                value: "${VERSION}".into()
            }]
        );
    }

    #[test]
    fn whole_file_does_not_skip_non_literal_content() {
        let output = run(Extract::WholeFile, "${VERSION}\n").unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(
            output.rows[0].value(),
            Some(&Value::String("${VERSION}\n".into()))
        );
        assert_eq!(output.notes.len(), 0);
    }

    #[test]
    fn lines_and_regex_keep_cross_files_value_shape() {
        let lines = Extract::Lines(LinesOpts::default());
        let output = run(lines, "# comment\n a \n\n").unwrap();
        assert_eq!(output.rows.len(), 1);
        assert_eq!(output.rows[0].value(), Some(&Value::String("a".into())));
    }

    #[test]
    fn parse_failures_and_limits_are_typed() {
        let extract = Extract::structured(Format::Json, "$.a").unwrap();
        assert!(matches!(run(extract, "{"), Err(AdapterError::Parse(_))));
        let limits = Limits::default()
            .with(LimitKind::RowsPerExtractor, 1)
            .unwrap();
        let extract = Extract::Regex(r"(?m)^(.*)$".to_string());
        let err = ExtractAdapter::new(extract)
            .extract(&resource("a.txt"), b"a\nb\n", &limits)
            .unwrap_err();
        assert!(matches!(
            err,
            AdapterError::Limit(LimitExceeded {
                kind: LimitKind::RowsPerExtractor,
                limit: 1,
                reached: 2
            })
        ));
    }

    #[test]
    fn different_specifications_have_different_ids() {
        let a = ExtractAdapter::new(Extract::structured(Format::Json, "$.a").unwrap());
        let b = ExtractAdapter::new(Extract::structured(Format::Json, "$.b").unwrap());
        let c = ExtractAdapter::new(Extract::Regex("a".into()));
        assert_ne!(a.id(), b.id());
        assert_ne!(b.id(), c.id());
    }

    #[test]
    fn sensitive_extracts_ask_the_consumer_to_redact() {
        let extract = Extract::structured(Format::Dotenv, "$.SECRET").unwrap();
        assert!(ExtractAdapter::new(extract).redacts_by_default());
        let extract = Extract::structured(Format::Json, "$.a").unwrap();
        assert!(!ExtractAdapter::new(extract).redacts_by_default());
    }

    #[test]
    fn lossy_decoding_matches_cross_file() {
        let extract = Extract::WholeFile;
        let output = ExtractAdapter::new(extract)
            .extract(&resource("a.txt"), &[0xff, b'a'], &Limits::default())
            .unwrap();
        assert_eq!(
            output.rows[0].value(),
            Some(&Value::String("\u{fffd}a".into()))
        );
    }
}
