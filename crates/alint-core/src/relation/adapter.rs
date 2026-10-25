//! The adapter trait: a resource's bytes in, rows and diagnostics out.

use super::limits::{LimitExceeded, LimitKind, Limits};
use super::resource::Resource;
use super::row::Row;
use crate::extract::NonStringValueKind;

/// Why an adapter produced no rows for a resource. Each is an analysis failure
/// for that resource (DEC-2), never an empty result that passes.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AdapterError {
    #[error("{0}")]
    Parse(String),
    #[error(transparent)]
    Limit(#[from] LimitExceeded),
}

/// A non-row outcome an adapter observed while decoding one resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterNote {
    /// A value the string-only `Extract` wrapper skipped because it is
    /// computed, for example `${VAR}` or `{{ name }}`. The value stays here so
    /// the consuming rule can redact it before formatting the existing note
    /// message; the IR never formats a finding.
    NonLiteral { value: String },
}

/// One adapter's result for one resource.
///
/// Rows are values or structural keys. `non_strings` records the typed values a
/// string-only extractor selected (the consumer renders one analysis failure
/// that counts each kind), and `notes` records informational outcomes. Neither
/// list is a row, and a note is deliberately not an analysis failure.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdapterOutput {
    pub rows: Vec<Row>,
    pub non_strings: Vec<NonStringValueKind>,
    pub notes: Vec<AdapterNote>,
}

impl AdapterOutput {
    #[must_use]
    pub fn new(rows: Vec<Row>) -> Self {
        Self {
            rows,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.non_strings.is_empty() && self.notes.is_empty()
    }
}

/// Turns one resource's bytes into rows and diagnostics. Implementations are
/// stateless and shared across threads; R2 caches their output so each
/// (adapter, resource) pair is decoded at most once per run.
pub trait Adapter: Send + Sync + std::fmt::Debug {
    /// A stable identity, used for the decode cache and in provenance.
    ///
    /// This is `&str` rather than `&'static str` because an adapter instance
    /// can carry a specification identity: the `Extract` wrapper includes its
    /// query, pattern, or line-comment in the id, so two different extract
    /// specifications over one file never share a decode cache entry.
    fn id(&self) -> &str;

    /// Whether a parse error from this adapter may quote sensitive source
    /// material. The consuming rule redacts the message when this is true; the
    /// adapter itself keeps the parser's diagnostic so a non-sensitive failure
    /// stays useful.
    fn redacts_by_default(&self) -> bool {
        false
    }

    /// Every row and diagnostic of `resource`. Implementations stop and return
    /// [`AdapterError::Limit`] once they would exceed
    /// [`LimitKind::RowsPerExtractor`] (see [`check_row_count`]).
    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &Limits,
    ) -> Result<AdapterOutput, AdapterError>;
}

/// The per-extractor row limit, for adapters to call as they grow their output.
pub fn check_row_count(limits: &Limits, rows: usize) -> Result<(), AdapterError> {
    Ok(limits.check(LimitKind::RowsPerExtractor, rows)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::{Pointer, RowState, Value};

    /// One row per line, the smallest adapter that exercises the contract.
    #[derive(Debug)]
    struct Lines;

    impl Adapter for Lines {
        fn id(&self) -> &'static str {
            "test-lines"
        }

        fn extract(
            &self,
            resource: &Resource,
            bytes: &[u8],
            limits: &Limits,
        ) -> Result<AdapterOutput, AdapterError> {
            let text =
                std::str::from_utf8(bytes).map_err(|e| AdapterError::Parse(e.to_string()))?;
            let mut rows = Vec::new();
            for (index, line) in text.lines().enumerate() {
                check_row_count(limits, rows.len() + 1)?;
                rows.push(Row::new(
                    resource.clone(),
                    Some(Pointer::root().index(index)),
                    u32::try_from(index).expect("a test file has few lines"),
                    RowState::Value(Value::String(line.into())),
                ));
            }
            Ok(AdapterOutput::new(rows))
        }
    }

    #[test]
    fn an_adapter_reports_parse_failures_and_limits_instead_of_empty_rows() {
        let resource = Resource::for_test(0, "a.txt");
        let limits = Limits::default();
        assert_eq!(
            Lines
                .extract(&resource, b"a\nb", &limits)
                .unwrap()
                .rows
                .len(),
            2
        );
        assert!(matches!(
            Lines.extract(&resource, &[0xff], &limits),
            Err(AdapterError::Parse(_))
        ));
        let tight = Limits::default()
            .with(LimitKind::RowsPerExtractor, 2)
            .unwrap();
        assert_eq!(
            Lines.extract(&resource, b"a\nb\nc", &tight),
            Err(AdapterError::Limit(LimitExceeded {
                kind: LimitKind::RowsPerExtractor,
                limit: 2,
                reached: 3
            }))
        );
    }
}
