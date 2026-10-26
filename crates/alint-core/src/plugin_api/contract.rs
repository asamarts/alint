//! Contract-side rows, findings, limits, and the `Adapter`/`Analyzer` traits.
//!
//! These types are deliberately independent of the engine's relation layer:
//! the bridge in `crate::relation::contract` converts between them. That keeps
//! a future process/WASM host from depending on engine internals, and the
//! import-boundary test in this module keeps it enforceable.

use std::fmt;

/// A repository resource named by the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resource {
    /// Normalized `/`-separated repository path.
    pub path: String,
    /// Whether the host selected it from the walked index (rather than a
    /// literal config path). Literal resources may be absent from an index.
    pub indexed: bool,
    /// The index position when `indexed` is true, for round-tripping.
    pub index: Option<u32>,
}

/// What the adapter found at one row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    Value(FlatValue),
    /// A structural key with no value (key-only adapters).
    Key,
    Missing,
    Error(String),
}

/// One step of a row pointer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PointerSegment {
    Key(String),
    Index(u64),
}

/// One row with the provenance the contract exposes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub resource: Resource,
    /// The structural pointer, when the adapter knows it.
    pub pointer: Option<Vec<PointerSegment>>,
    pub label: Option<String>,
    pub ordinal: u32,
    /// Half-open byte span when the adapter knows it.
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub state: RowState,
    /// The value before a transform, in the same flat form.
    pub original: Option<FlatValue>,
}

/// One related location on a finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedLocation {
    pub path: String,
    pub label: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

/// A finding produced by an analyzer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub path: Option<String>,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub related: Vec<RelatedLocation>,
    pub baseline_key: Option<String>,
    pub sensitive: bool,
    pub analysis_failure: Option<String>,
    /// One of `resource-read`, `resource-parse`, `resource-limit`,
    /// `resource-unavailable`; ignored for a non-failure finding.
    pub failure_kind: Option<String>,
    pub not_fixable: bool,
}

/// Resource and adapter limits pushed by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_rows: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_rows: 1_000_000,
        }
    }
}

/// The input to one adapter extraction.
#[derive(Debug)]
pub struct AdapterInput<'a> {
    pub resource: &'a Resource,
    pub bytes: &'a [u8],
    pub limits: Limits,
}

/// What one adapter produced for one resource.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdapterOutput {
    pub rows: Vec<Row>,
    /// Names of typed values a string-only adapter skipped (`number`,
    /// `boolean`, `null`, `array`, `object`).
    pub non_strings: Vec<String>,
    /// Informational notes, already human-readable.
    pub notes: Vec<String>,
}

/// Why an adapter produced no rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdapterError {
    Parse(String),
    Limit { limit: usize, reached: usize },
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(message) => f.write_str(message),
            Self::Limit { limit, reached } => {
                write!(f, "selected {reached} rows; the limit is {limit}")
            }
        }
    }
}

impl std::error::Error for AdapterError {}

/// A host-pushed resource adapter. Stateless and shareable across workers.
pub trait Adapter: Send + Sync {
    fn id(&self) -> &str;
    fn redacts_by_default(&self) -> bool {
        false
    }
    fn extract(&self, input: AdapterInput<'_>) -> Result<AdapterOutput, AdapterError>;
}

/// The input to one analyzer invocation. Rows are host-pushed and already in
/// canonical order; the analyzer must not read the host filesystem.
#[derive(Debug)]
pub struct AnalyzerInput<'a> {
    pub rule_id: &'a str,
    pub rows: &'a [Row],
    pub limits: Limits,
}

/// Why an analyzer could not produce findings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalyzerError {
    InvalidInput(String),
    Limit { limit: usize, reached: usize },
}

impl fmt::Display for AnalyzerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => f.write_str(message),
            Self::Limit { limit, reached } => {
                write!(f, "produced {reached} findings; the limit is {limit}")
            }
        }
    }
}

impl std::error::Error for AnalyzerError {}

/// A pure relation analyzer over host-pushed rows.
pub trait Analyzer: Send + Sync {
    fn id(&self) -> &str;
    fn analyze(&self, input: AnalyzerInput<'_>) -> Result<Vec<Finding>, AnalyzerError>;
}

/// Capabilities a host advertises for a plugin.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub spawn_user_command: bool,
    pub host_keyring: bool,
}

use super::FlatValue;

#[cfg(test)]
mod tests {
    use super::*;

    struct CountAnalyzer;

    impl Analyzer for CountAnalyzer {
        fn id(&self) -> &'static str {
            "count"
        }

        fn analyze(&self, input: AnalyzerInput<'_>) -> Result<Vec<Finding>, AnalyzerError> {
            Ok(input
                .rows
                .iter()
                .enumerate()
                .map(|(index, row)| Finding {
                    path: Some(row.resource.path.clone()),
                    message: format!("row {index}"),
                    line: None,
                    column: None,
                    related: Vec::new(),
                    baseline_key: None,
                    sensitive: false,
                    analysis_failure: None,
                    failure_kind: None,
                    not_fixable: false,
                })
                .collect())
        }
    }

    #[test]
    fn contract_analyzer_sees_only_contract_rows() {
        let rows = [Row {
            resource: Resource {
                path: "a.json".into(),
                indexed: true,
                index: Some(0),
            },
            pointer: None,
            label: None,
            ordinal: 0,
            start: None,
            end: None,
            state: RowState::Missing,
            original: None,
        }];
        let findings = CountAnalyzer
            .analyze(AnalyzerInput {
                rule_id: "test",
                rows: &rows,
                limits: Limits::default(),
            })
            .unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].path.as_deref(), Some("a.json"));
    }
}
