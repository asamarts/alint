//! Bridge between the internal relation traits and the std-only plugin
//! contract. The contract types live in `crate::plugin_api`; this module is
//! the only place allowed to know both layouts.
//!
//! The bridge trusts nothing a contract implementation returns about where
//! things are: an adapter's rows must belong to the resource it was given, and
//! an analyzer's findings and related locations must name one of its input
//! resources by its normalized path. Anything else is refused.

use crate::extract::NonStringValueKind;
use crate::plugin_api::{
    Adapter as ContractAdapterTrait, AdapterError as ContractError, AdapterInput as ContractInput,
    AdapterOutput as ContractOutput, Limits as ContractLimits, PointerSegment,
    Resource as ContractResource, Row as ContractRow, RowState as ContractRowState,
};

use super::adapter::{Adapter, AdapterError, AdapterNote, AdapterOutput};
use super::limits::LimitKind;
use super::pointer::{Pointer, Segment};
use super::resource::{Origin, Resource};
use super::row::{Row, RowState};
use super::value::Value;
use crate::plugin_api::{
    Analyzer as ContractAnalyzerTrait, AnalyzerError as ContractAnalyzerError,
    AnalyzerInput as ContractAnalyzerInput, Finding as ContractFinding,
};
use crate::{AnalysisFailureKind, RelatedLocation, SourceRange, Violation};
use std::collections::BTreeSet;
use std::path::Path;

/// Why [`ContractAnalyzer::analyze`] returned no findings.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AnalyzeError {
    /// The analyzer reported an error, or exceeded its finding limit.
    #[error("{0}")]
    Analyzer(ContractAnalyzerError),
    /// The analyzer's findings do not belong to its input.
    #[error("analyzer `{analyzer}` returned {problem}")]
    Rejected { analyzer: String, problem: String },
}

/// Wraps a contract adapter so the engine sees an internal [`Adapter`].
pub struct ContractAdapter<A> {
    inner: A,
}

impl<A: ContractAdapterTrait> std::fmt::Debug for ContractAdapter<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContractAdapter")
            .field("id", &self.inner.id())
            .finish()
    }
}

impl<A> ContractAdapter<A> {
    #[must_use]
    pub fn new(inner: A) -> Self {
        Self { inner }
    }
}

impl<A: ContractAdapterTrait> Adapter for ContractAdapter<A> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn redacts_by_default(&self) -> bool {
        self.inner.redacts_by_default()
    }

    fn extract(
        &self,
        resource: &Resource,
        bytes: &[u8],
        limits: &super::limits::Limits,
    ) -> Result<AdapterOutput, AdapterError> {
        let contract = contract_resource(resource);
        let max_rows = limits.get(LimitKind::RowsPerExtractor);
        let output = self
            .inner
            .extract(ContractInput {
                resource: &contract,
                bytes,
                limits: ContractLimits { max_rows },
            })
            .map_err(from_contract_error)?;
        let output = from_contract_output(output, resource, &contract)?;
        if output.rows.len() > max_rows {
            return Err(AdapterError::Limit(super::limits::LimitExceeded {
                kind: LimitKind::RowsPerExtractor,
                limit: max_rows,
                reached: output.rows.len(),
            }));
        }
        Ok(output)
    }
}

/// Wraps an internal adapter as a contract adapter. This is the reverse bridge
/// used to prove built-in adapters travel through the contract and back
/// without changing their rows.
pub struct InternalContractAdapter<A> {
    inner: A,
}

impl<A: Adapter> std::fmt::Debug for InternalContractAdapter<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternalContractAdapter")
            .field("id", &self.inner.id())
            .finish()
    }
}

impl<A> InternalContractAdapter<A> {
    #[must_use]
    pub fn new(inner: A) -> Self {
        Self { inner }
    }
}

impl<A: Adapter> ContractAdapterTrait for InternalContractAdapter<A> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn redacts_by_default(&self) -> bool {
        self.inner.redacts_by_default()
    }

    fn extract(&self, input: ContractInput<'_>) -> Result<ContractOutput, ContractError> {
        let resource = internal_resource(input.resource).ok_or_else(|| {
            ContractError::Parse(format!(
                "resource path {:?} is not a normalized repository path",
                input.resource.path
            ))
        })?;
        let limits = super::limits::Limits::default()
            .with(LimitKind::RowsPerExtractor, input.limits.max_rows)
            .map_err(|error| ContractError::Limit {
                limit: input.limits.max_rows,
                reached: error.value,
            })?;
        let output = self
            .inner
            .extract(&resource, input.bytes, &limits)
            .map_err(to_contract_error)?;
        Ok(to_contract_output(output))
    }
}

fn contract_resource(resource: &Resource) -> ContractResource {
    ContractResource {
        path: resource.path().as_str().to_string(),
        indexed: matches!(resource.origin(), Origin::Index(_)),
        index: match resource.origin() {
            Origin::Index(id) => Some(u32::try_from(id.index()).unwrap_or(u32::MAX)),
            Origin::Literal => None,
        },
    }
}

fn internal_resource(resource: &ContractResource) -> Option<Resource> {
    let path = super::RelPath::parse(&resource.path)?;
    Some(match (resource.indexed, resource.index) {
        (true, Some(index)) => {
            Resource::from_parts(Origin::Index(super::ResourceId::from_index(index)), path)
        }
        (true, None) => return None,
        (false, _) => Resource::literal(path),
    })
}

/// Wraps a contract analyzer and exposes its findings to the engine's rule
/// layer. Rows are converted to the contract form and findings back; the
/// analyzer never sees an engine type.
pub struct ContractAnalyzer<A> {
    inner: A,
}

impl<A: ContractAnalyzerTrait> std::fmt::Debug for ContractAnalyzer<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContractAnalyzer")
            .field("id", &self.inner.id())
            .finish()
    }
}

impl<A: ContractAnalyzerTrait> ContractAnalyzer<A> {
    #[must_use]
    pub fn new(inner: A) -> Self {
        Self { inner }
    }

    /// Run the analyzer over internal rows and convert its findings back.
    ///
    /// `resources` are the analysis's input resources, including any that
    /// produced no rows (an empty file); a row's resource always counts. A
    /// finding or related location must name one of them by its normalized
    /// path, and an analysis failure must use a contract failure kind;
    /// otherwise the whole result is refused.
    pub fn analyze(
        &self,
        rule_id: &str,
        resources: &[&super::RelPath],
        rows: &[Row],
        limits: &super::limits::Limits,
    ) -> Result<Vec<Violation>, AnalyzeError> {
        let inputs: BTreeSet<&str> = resources
            .iter()
            .map(|path| path.as_str())
            .chain(rows.iter().map(|row| row.resource.path().as_str()))
            .collect();
        let rows: Vec<ContractRow> = rows.iter().cloned().map(to_contract_row).collect();
        let max_rows = limits.get(LimitKind::RowsPerExtractor);
        let findings = self
            .inner
            .analyze(ContractAnalyzerInput {
                rule_id,
                rows: &rows,
                limits: ContractLimits { max_rows },
            })
            .map_err(AnalyzeError::Analyzer)?;
        if findings.len() > max_rows {
            return Err(AnalyzeError::Analyzer(ContractAnalyzerError::Limit {
                limit: max_rows,
                reached: findings.len(),
            }));
        }
        let rejected = |problem: String| AnalyzeError::Rejected {
            analyzer: self.inner.id().to_string(),
            problem,
        };
        let input_path = |path: &str, what: &str| {
            if inputs.contains(path) {
                Ok(())
            } else {
                Err(rejected(format!(
                    "{what} at {path:?}, which is not one of its input resources"
                )))
            }
        };
        for finding in &findings {
            if let Some(path) = &finding.path {
                input_path(path, "a finding")?;
            }
            for related in &finding.related {
                input_path(&related.path, "a related location")?;
            }
            if finding.analysis_failure.is_some() {
                failure_kind(finding.failure_kind.as_deref()).ok_or_else(|| {
                    rejected(format!(
                        "an analysis failure of unknown kind {:?}",
                        finding.failure_kind.as_deref().unwrap_or_default()
                    ))
                })?;
            }
        }
        Ok(findings
            .into_iter()
            .map(|finding| to_violation(finding, rule_id))
            .collect())
    }
}

/// The engine's kind for a contract failure kind, or `None` for a kind the
/// contract does not define. The limit and unavailable kinds have no engine
/// counterpart yet and count as parse failures.
fn failure_kind(kind: Option<&str>) -> Option<AnalysisFailureKind> {
    match kind {
        Some("resource-read" | "resource_read") => Some(AnalysisFailureKind::ResourceRead),
        None
        | Some(
            "resource-parse"
            | "resource_parse"
            | "resource-limit"
            | "resource_limit"
            | "resource-unavailable"
            | "resource_unavailable",
        ) => Some(AnalysisFailureKind::ResourceParse),
        Some(_) => None,
    }
}

/// A finding the bridge has validated: its paths are input resource paths.
fn to_violation(finding: ContractFinding, rule_id: &str) -> Violation {
    let mut violation = Violation::new(finding.message);
    if let Some(path) = finding.path {
        violation.path = Some(std::sync::Arc::from(Path::new(&path)));
    }
    violation.line = finding.line;
    violation.column = finding.column;
    if !finding.related.is_empty() {
        violation.set_related_locations(finding.related.into_iter().map(|related| {
            let mut location = RelatedLocation::new(related.path, related.label);
            if let (Some(line), Some(column)) = (related.line, related.column)
                && let Some(range) = SourceRange::new(line, column, line, column)
            {
                location = location.with_range(range);
            }
            location
        }));
    }
    if let Some(key) = finding.baseline_key {
        violation.baseline_key = Some(key.into());
    }
    violation.not_fixable = finding.not_fixable;
    if let Some(message) = finding.analysis_failure {
        violation = violation.as_analysis_failure(
            failure_kind(finding.failure_kind.as_deref())
                .expect("`analyze` refuses unknown failure kinds"),
        );
        violation.message = message.into();
    }
    if finding.sensitive {
        violation.path_sensitive = true;
        violation.redact_message(rule_id);
    }
    violation
}

fn from_contract_error(error: ContractError) -> AdapterError {
    match error {
        ContractError::Parse(message) => AdapterError::Parse(message),
        ContractError::Limit { limit, reached } => {
            AdapterError::Limit(super::limits::LimitExceeded {
                kind: LimitKind::RowsPerExtractor,
                limit,
                reached,
            })
        }
    }
}

fn to_contract_error(error: AdapterError) -> ContractError {
    match error {
        AdapterError::Parse(message) => ContractError::Parse(message),
        AdapterError::Limit(limit) => ContractError::Limit {
            limit: limit.limit,
            reached: limit.reached,
        },
    }
}

/// The internal form of a contract adapter's output for `resource`, which the
/// adapter received as `contract`. Every row must name that resource exactly
/// (path, origin, and index position).
fn from_contract_output(
    output: ContractOutput,
    resource: &Resource,
    contract: &ContractResource,
) -> Result<AdapterOutput, AdapterError> {
    let mut rows = Vec::with_capacity(output.rows.len());
    for row in output.rows {
        if row.resource != *contract {
            return Err(AdapterError::Parse(format!(
                "the adapter returned a row for resource {:?}, not the resource {:?} it was given",
                row.resource.path, contract.path
            )));
        }
        rows.push(from_contract_row(row, resource)?);
    }
    let mut non_strings = Vec::with_capacity(output.non_strings.len());
    for kind in &output.non_strings {
        non_strings.push(match kind.as_str() {
            "null" => NonStringValueKind::Null,
            "boolean" => NonStringValueKind::Boolean,
            "number" => NonStringValueKind::Number,
            "array" => NonStringValueKind::Array,
            "object" => NonStringValueKind::Object,
            other => {
                return Err(AdapterError::Parse(format!(
                    "unknown non-string value kind {other:?}"
                )));
            }
        });
    }
    let notes = output
        .notes
        .into_iter()
        .map(|value| AdapterNote::NonLiteral { value })
        .collect();
    Ok(AdapterOutput {
        rows,
        non_strings,
        notes,
    })
}

fn from_contract_row(row: ContractRow, resource: &Resource) -> Result<Row, AdapterError> {
    let resource = resource.clone();
    let pointer = match row.pointer {
        Some(segments) => {
            let mut converted = Vec::with_capacity(segments.len());
            for segment in segments {
                converted.push(match segment {
                    PointerSegment::Key(key) => Segment::Key(key.into()),
                    PointerSegment::Index(index) => {
                        Segment::Index(usize::try_from(index).map_err(|_| {
                            AdapterError::Parse("pointer index is too large".into())
                        })?)
                    }
                });
            }
            Some(Pointer::from_segments(converted))
        }
        None => None,
    };
    let state = match row.state {
        ContractRowState::Value(value) => RowState::Value(
            Value::from_contract(&value)
                .map_err(|error| AdapterError::Parse(format!("invalid contract value: {error}")))?,
        ),
        ContractRowState::Key => RowState::Key,
        ContractRowState::Missing => RowState::Missing,
        ContractRowState::Error(message) => RowState::Error(message.into()),
    };
    let original = row
        .original
        .as_ref()
        .map(Value::from_contract)
        .transpose()
        .map_err(|error| AdapterError::Parse(format!("invalid contract value: {error}")))?;
    let mut converted = Row::new(resource, pointer, row.ordinal, state);
    converted.label = row.label.map(Into::into);
    converted.span = match (row.start, row.end) {
        (Some(start), Some(end)) => Some(crate::extract::ByteSpan::new(
            usize::try_from(start)
                .map_err(|_| AdapterError::Parse("span start is too large".into()))?,
            usize::try_from(end)
                .map_err(|_| AdapterError::Parse("span end is too large".into()))?,
        )),
        _ => None,
    };
    converted.original = original;
    Ok(converted)
}

fn to_contract_output(output: AdapterOutput) -> ContractOutput {
    ContractOutput {
        rows: output.rows.into_iter().map(to_contract_row).collect(),
        non_strings: output
            .non_strings
            .into_iter()
            .map(|kind| kind.as_str().to_string())
            .collect(),
        notes: output
            .notes
            .into_iter()
            .map(|note| match note {
                AdapterNote::NonLiteral { value } => value,
            })
            .collect(),
    }
}

fn to_contract_row(row: Row) -> ContractRow {
    let resource = contract_resource(&row.resource);
    let pointer = row.pointer.as_ref().map(|pointer| {
        pointer
            .segments()
            .iter()
            .map(|segment| match segment {
                Segment::Key(key) => PointerSegment::Key(key.to_string()),
                Segment::Index(index) => PointerSegment::Index(*index as u64),
            })
            .collect()
    });
    let state = match row.state {
        RowState::Value(value) => ContractRowState::Value(value.to_contract()),
        RowState::Key => ContractRowState::Key,
        RowState::Missing => ContractRowState::Missing,
        RowState::Error(message) => ContractRowState::Error(message.to_string()),
    };
    ContractRow {
        resource,
        pointer,
        label: row.label.map(|label| label.to_string()),
        ordinal: row.ordinal,
        start: row.span.map(|span| span.start as u64),
        end: row.span.map(|span| span.end as u64),
        state,
        original: row.original.map(|value| value.to_contract()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relation::adapters::EntryRowsAdapter;
    use crate::structured_format::Format;

    struct OneFinding;

    impl crate::plugin_api::Analyzer for OneFinding {
        fn id(&self) -> &'static str {
            "one-finding"
        }

        fn analyze(
            &self,
            input: crate::plugin_api::AnalyzerInput<'_>,
        ) -> Result<Vec<crate::plugin_api::Finding>, crate::plugin_api::AnalyzerError> {
            Ok(vec![crate::plugin_api::Finding {
                path: input.rows.first().map(|row| row.resource.path.clone()),
                message: format!("{} rows", input.rows.len()),
                line: Some(3),
                column: Some(4),
                related: Vec::new(),
                baseline_key: Some("row".into()),
                sensitive: false,
                analysis_failure: None,
                failure_kind: None,
                not_fixable: false,
            }])
        }
    }

    #[test]
    fn contract_analyzer_bridges_rows_and_findings() {
        let rows = vec![Row::new(
            Resource::for_test(0, "a.json"),
            None,
            0,
            RowState::Missing,
        )];
        let findings = ContractAnalyzer::new(OneFinding)
            .analyze(
                "test-rule",
                &[],
                &rows,
                &super::super::limits::Limits::default(),
            )
            .unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].line, Some(3));
        assert_eq!(findings[0].baseline_key.as_deref(), Some("row"));
    }

    /// An analyzer that returns the given findings whatever its input.
    struct Fixed(Vec<crate::plugin_api::Finding>);

    impl crate::plugin_api::Analyzer for Fixed {
        fn id(&self) -> &'static str {
            "fixed"
        }

        fn analyze(
            &self,
            _: crate::plugin_api::AnalyzerInput<'_>,
        ) -> Result<Vec<crate::plugin_api::Finding>, crate::plugin_api::AnalyzerError> {
            Ok(self.0.clone())
        }
    }

    fn finding(path: Option<&str>) -> crate::plugin_api::Finding {
        crate::plugin_api::Finding {
            path: path.map(str::to_string),
            message: "m".into(),
            line: None,
            column: None,
            related: Vec::new(),
            baseline_key: None,
            sensitive: false,
            analysis_failure: None,
            failure_kind: None,
            not_fixable: false,
        }
    }

    fn analyze(findings: Vec<crate::plugin_api::Finding>) -> Result<Vec<Violation>, AnalyzeError> {
        let rows = vec![
            Row::new(Resource::for_test(0, "a.json"), None, 0, RowState::Missing),
            Row::new(
                Resource::literal(crate::relation::RelPath::parse("dir/b.json").unwrap()),
                None,
                0,
                RowState::Missing,
            ),
        ];
        let empty = crate::relation::RelPath::parse("empty.json").unwrap();
        ContractAnalyzer::new(Fixed(findings)).analyze(
            "test-rule",
            &[&empty],
            &rows,
            &super::super::limits::Limits::default(),
        )
    }

    #[test]
    fn findings_must_name_an_input_resource() {
        let mut related = finding(Some("a.json"));
        related.related.push(crate::plugin_api::RelatedLocation {
            path: "dir/b.json".into(),
            label: "peer".into(),
            line: None,
            column: None,
        });
        let accepted = analyze(vec![
            finding(None),
            finding(Some("dir/b.json")),
            finding(Some("empty.json")),
            related,
        ])
        .unwrap();
        assert_eq!(accepted.len(), 4, "a row-less input resource counts");
        assert_eq!(accepted[1].path.as_deref(), Some(Path::new("dir/b.json")));

        for path in [
            "c.json",
            "./a.json",
            "dir/../a.json",
            "/etc/passwd",
            "../a.json",
            "dir\\b.json",
            "",
        ] {
            let error = analyze(vec![finding(Some("a.json")), finding(Some(path))]).unwrap_err();
            assert!(
                matches!(&error, AnalyzeError::Rejected { problem, .. } if problem.starts_with("a finding at")),
                "{path:?}: {error}"
            );
            let mut outside = finding(Some("a.json"));
            outside.related.push(crate::plugin_api::RelatedLocation {
                path: path.into(),
                label: "x".into(),
                line: None,
                column: None,
            });
            let error = analyze(vec![outside]).unwrap_err();
            assert!(
                matches!(&error, AnalyzeError::Rejected { problem, .. } if problem.starts_with("a related location at")),
                "{path:?}: {error}"
            );
        }
    }

    #[test]
    fn analysis_failures_must_use_a_contract_failure_kind() {
        let failure = |kind: Option<&str>| {
            let mut finding = finding(Some("a.json"));
            finding.analysis_failure = Some("broken".into());
            finding.failure_kind = kind.map(str::to_string);
            finding
        };
        for (kind, expected) in [
            (None, AnalysisFailureKind::ResourceParse),
            (Some("resource-read"), AnalysisFailureKind::ResourceRead),
            (Some("resource-parse"), AnalysisFailureKind::ResourceParse),
            (Some("resource-limit"), AnalysisFailureKind::ResourceParse),
            (
                Some("resource-unavailable"),
                AnalysisFailureKind::ResourceParse,
            ),
        ] {
            let violations = analyze(vec![failure(kind)]).unwrap();
            assert_eq!(
                violations[0].disposition,
                crate::FindingDisposition::AnalysisFailure(expected),
                "{kind:?}"
            );
        }
        assert!(matches!(
            analyze(vec![failure(Some("evaluation"))]),
            Err(AnalyzeError::Rejected { .. })
        ));
    }

    /// A contract adapter that returns one row for the given resource,
    /// whatever it was asked to read.
    struct Elsewhere(crate::plugin_api::Resource);

    impl crate::plugin_api::Adapter for Elsewhere {
        fn id(&self) -> &'static str {
            "elsewhere"
        }

        fn extract(
            &self,
            _: crate::plugin_api::AdapterInput<'_>,
        ) -> Result<crate::plugin_api::AdapterOutput, crate::plugin_api::AdapterError> {
            Ok(crate::plugin_api::AdapterOutput {
                rows: vec![crate::plugin_api::Row {
                    resource: self.0.clone(),
                    pointer: None,
                    label: None,
                    ordinal: 0,
                    start: None,
                    end: None,
                    state: crate::plugin_api::RowState::Missing,
                    original: None,
                }],
                non_strings: Vec::new(),
                notes: Vec::new(),
            })
        }
    }

    #[test]
    fn adapter_rows_must_belong_to_the_resource_it_was_given() {
        let resource = Resource::for_test(3, "a.json");
        let limits = super::super::limits::Limits::default();
        let given = contract_resource(&resource);
        let rows = ContractAdapter::new(Elsewhere(given.clone()))
            .extract(&resource, b"", &limits)
            .unwrap();
        assert_eq!(rows.rows[0].resource, resource);
        for other in [
            crate::plugin_api::Resource {
                path: "b.json".into(),
                ..given.clone()
            },
            crate::plugin_api::Resource {
                index: Some(4),
                ..given.clone()
            },
            crate::plugin_api::Resource {
                indexed: false,
                index: None,
                ..given.clone()
            },
        ] {
            let error = ContractAdapter::new(Elsewhere(other.clone()))
                .extract(&resource, b"", &limits)
                .unwrap_err();
            assert!(
                matches!(&error, AdapterError::Parse(message) if message.contains("not the resource")),
                "{other:?}: {error}"
            );
        }
    }

    #[test]
    fn builtin_entry_adapter_round_trips_through_the_contract() {
        let direct = EntryRowsAdapter::new(Format::Json).unwrap();
        let bridged = ContractAdapter::new(InternalContractAdapter::new(
            EntryRowsAdapter::new(Format::Json).unwrap(),
        ));
        let resource = Resource::for_test(0, "a.json");
        let bytes = br#"{"a":[1],"b":{"c":true},"d":null}"#;
        let limits = super::super::limits::Limits::default();
        let expected = direct.extract(&resource, bytes, &limits).unwrap();
        let actual = bridged.extract(&resource, bytes, &limits).unwrap();
        assert_eq!(actual, expected);
    }
}
