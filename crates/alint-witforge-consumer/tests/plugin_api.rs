//! The hand-written `alint_core::plugin_api` is the contract alint runs in
//! v0.21, and the WIT is the contract's source of truth. Each conversion below
//! destructures its input without `..` and builds its output with a full struct
//! literal or an exhaustive match, and every enum converts both ways, so a
//! field, case, or required trait method on one side without a counterpart on
//! the other stops this file compiling.
//!
//! The intended differences appear where they occur: the engine's `usize`
//! against the wire's `u32`, borrowed inputs, `Resource` against
//! `RepoResource`, a record field as a tuple, and named fields on the limit
//! errors. `PluginInfo` has no hand-written counterpart yet; the handshake that
//! carries it arrives with a plugin host.

use alint_core::plugin_api as api;
use alint_witforge_consumer::glue::v0::types as glue;
use alint_witforge_consumer::plain::v0_1::types as wit;
use alint_witforge_consumer::plain::v0_1::{AdapterApi, AnalyzerApi};

/// A count, position, or limit crossing to the wire, which carries `u32`.
fn wire(value: usize) -> u32 {
    u32::try_from(value).expect("the value fits the contract's u32")
}

fn host(value: u32) -> usize {
    usize::try_from(value).expect("a u32 fits usize")
}

fn resource_to_wit(value: &api::Resource) -> wit::RepoResource {
    let api::Resource {
        path,
        indexed,
        index,
    } = value;
    wit::RepoResource {
        path: path.clone(),
        indexed: *indexed,
        index: *index,
    }
}

fn resource_from_wit(value: wit::RepoResource) -> api::Resource {
    let wit::RepoResource {
        path,
        indexed,
        index,
    } = value;
    api::Resource {
        path,
        indexed,
        index,
    }
}

fn node_to_wit(value: &api::Node) -> wit::Node {
    match value {
        api::Node::Null => wit::Node::Null,
        api::Node::Bool(value) => wit::Node::Bool(*value),
        api::Node::Number(value) => wit::Node::Number(value.clone()),
        api::Node::String(value) => wit::Node::String(value.clone()),
        api::Node::Bytes(value) => wit::Node::Bytes(value.clone()),
        api::Node::Path(value) => wit::Node::Path(value.clone()),
        api::Node::List(children) => wit::Node::List(children.clone()),
        api::Node::Record(fields) => wit::Node::Record(
            fields
                .iter()
                .map(|(name, child)| wit::RecordField {
                    name: name.clone(),
                    child: *child,
                })
                .collect(),
        ),
    }
}

fn node_from_wit(value: wit::Node) -> api::Node {
    match value {
        wit::Node::Null => api::Node::Null,
        wit::Node::Bool(value) => api::Node::Bool(value),
        wit::Node::Number(value) => api::Node::Number(value),
        wit::Node::String(value) => api::Node::String(value),
        wit::Node::Bytes(value) => api::Node::Bytes(value),
        wit::Node::Path(value) => api::Node::Path(value),
        wit::Node::List(children) => api::Node::List(children),
        wit::Node::Record(fields) => api::Node::Record(
            fields
                .into_iter()
                .map(|wit::RecordField { name, child }| (name, child))
                .collect(),
        ),
    }
}

fn flat_to_wit(value: &api::FlatValue) -> wit::FlatValue {
    wit::FlatValue {
        nodes: value.nodes().iter().map(node_to_wit).collect(),
    }
}

fn flat_from_wit(value: wit::FlatValue) -> api::FlatValue {
    let wit::FlatValue { nodes } = value;
    api::FlatValue::new(
        nodes.into_iter().map(node_from_wit).collect(),
        api::FlatValueLimits::default(),
    )
    .expect("a valid flat value")
}

fn segment_to_wit(value: &api::PointerSegment) -> wit::PointerSegment {
    match value {
        api::PointerSegment::Key(key) => wit::PointerSegment::Key(key.clone()),
        api::PointerSegment::Index(index) => wit::PointerSegment::Index(*index),
    }
}

fn segment_from_wit(value: wit::PointerSegment) -> api::PointerSegment {
    match value {
        wit::PointerSegment::Key(key) => api::PointerSegment::Key(key),
        wit::PointerSegment::Index(index) => api::PointerSegment::Index(index),
    }
}

fn state_to_wit(value: &api::RowState) -> wit::RowState {
    match value {
        api::RowState::Value(value) => wit::RowState::Value(flat_to_wit(value)),
        api::RowState::Key => wit::RowState::Key,
        api::RowState::Missing => wit::RowState::Missing,
        api::RowState::Error(message) => wit::RowState::Error(message.clone()),
    }
}

fn state_from_wit(value: wit::RowState) -> api::RowState {
    match value {
        wit::RowState::Value(value) => api::RowState::Value(flat_from_wit(value)),
        wit::RowState::Key => api::RowState::Key,
        wit::RowState::Missing => api::RowState::Missing,
        wit::RowState::Error(message) => api::RowState::Error(message),
    }
}

fn row_to_wit(value: &api::Row) -> wit::Row {
    let api::Row {
        resource,
        pointer,
        label,
        ordinal,
        start,
        end,
        state,
        original,
    } = value;
    wit::Row {
        resource: resource_to_wit(resource),
        pointer: pointer
            .as_ref()
            .map(|segments| segments.iter().map(segment_to_wit).collect()),
        label: label.clone(),
        ordinal: *ordinal,
        start: *start,
        end: *end,
        state: state_to_wit(state),
        original: original.as_ref().map(flat_to_wit),
    }
}

fn row_from_wit(value: wit::Row) -> api::Row {
    let wit::Row {
        resource,
        pointer,
        label,
        ordinal,
        start,
        end,
        state,
        original,
    } = value;
    api::Row {
        resource: resource_from_wit(resource),
        pointer: pointer.map(|segments| segments.into_iter().map(segment_from_wit).collect()),
        label,
        ordinal,
        start,
        end,
        state: state_from_wit(state),
        original: original.map(flat_from_wit),
    }
}

fn related_to_wit(value: &api::RelatedLocation) -> wit::RelatedLocation {
    let api::RelatedLocation {
        path,
        label,
        line,
        column,
    } = value;
    wit::RelatedLocation {
        path: path.clone(),
        label: label.clone(),
        line: line.map(wire),
        column: column.map(wire),
    }
}

fn related_from_wit(value: wit::RelatedLocation) -> api::RelatedLocation {
    let wit::RelatedLocation {
        path,
        label,
        line,
        column,
    } = value;
    api::RelatedLocation {
        path,
        label,
        line: line.map(host),
        column: column.map(host),
    }
}

fn finding_to_wit(value: &api::Finding) -> wit::Finding {
    let api::Finding {
        path,
        message,
        line,
        column,
        related,
        baseline_key,
        sensitive,
        analysis_failure,
        failure_kind,
        not_fixable,
    } = value;
    wit::Finding {
        path: path.clone(),
        message: message.clone(),
        line: line.map(wire),
        column: column.map(wire),
        related: related.iter().map(related_to_wit).collect(),
        baseline_key: baseline_key.clone(),
        sensitive: *sensitive,
        analysis_failure: analysis_failure.clone(),
        failure_kind: failure_kind.clone(),
        not_fixable: *not_fixable,
    }
}

fn finding_from_wit(value: wit::Finding) -> api::Finding {
    let wit::Finding {
        path,
        message,
        line,
        column,
        related,
        baseline_key,
        sensitive,
        analysis_failure,
        failure_kind,
        not_fixable,
    } = value;
    api::Finding {
        path,
        message,
        line: line.map(host),
        column: column.map(host),
        related: related.into_iter().map(related_from_wit).collect(),
        baseline_key,
        sensitive,
        analysis_failure,
        failure_kind,
        not_fixable,
    }
}

fn output_to_wit(value: &api::AdapterOutput) -> wit::AdapterOutput {
    let api::AdapterOutput {
        rows,
        non_strings,
        notes,
    } = value;
    wit::AdapterOutput {
        rows: rows.iter().map(row_to_wit).collect(),
        non_strings: non_strings.clone(),
        notes: notes.clone(),
    }
}

fn output_from_wit(value: wit::AdapterOutput) -> api::AdapterOutput {
    let wit::AdapterOutput {
        rows,
        non_strings,
        notes,
    } = value;
    api::AdapterOutput {
        rows: rows.into_iter().map(row_from_wit).collect(),
        non_strings,
        notes,
    }
}

fn limit_to_wit(limit: usize, reached: usize) -> wit::LimitError {
    wit::LimitError {
        limit: wire(limit),
        reached: wire(reached),
    }
}

fn adapter_error_to_wit(value: &api::AdapterError) -> wit::AdapterError {
    match value {
        api::AdapterError::Parse(message) => wit::AdapterError::Parse(message.clone()),
        api::AdapterError::Limit { limit, reached } => {
            wit::AdapterError::Limit(limit_to_wit(*limit, *reached))
        }
    }
}

fn adapter_error_from_wit(value: wit::AdapterError) -> api::AdapterError {
    match value {
        wit::AdapterError::Parse(message) => api::AdapterError::Parse(message),
        wit::AdapterError::Limit(wit::LimitError { limit, reached }) => api::AdapterError::Limit {
            limit: host(limit),
            reached: host(reached),
        },
    }
}

fn analyzer_error_to_wit(value: &api::AnalyzerError) -> wit::AnalyzerError {
    match value {
        api::AnalyzerError::InvalidInput(message) => {
            wit::AnalyzerError::InvalidInput(message.clone())
        }
        api::AnalyzerError::Limit { limit, reached } => {
            wit::AnalyzerError::Limit(limit_to_wit(*limit, *reached))
        }
    }
}

fn analyzer_error_from_wit(value: wit::AnalyzerError) -> api::AnalyzerError {
    match value {
        wit::AnalyzerError::InvalidInput(message) => api::AnalyzerError::InvalidInput(message),
        wit::AnalyzerError::Limit(wit::LimitError { limit, reached }) => {
            api::AnalyzerError::Limit {
                limit: host(limit),
                reached: host(reached),
            }
        }
    }
}

fn capabilities_to_wit(value: api::Capabilities) -> wit::Capabilities {
    let api::Capabilities {
        spawn_user_command,
        host_keyring,
    } = value;
    wit::Capabilities {
        spawn_user_command,
        host_keyring,
    }
}

fn capabilities_from_wit(value: &wit::Capabilities) -> api::Capabilities {
    let wit::Capabilities {
        spawn_user_command,
        host_keyring,
    } = value;
    api::Capabilities {
        spawn_user_command: *spawn_user_command,
        host_keyring: *host_keyring,
    }
}

fn adapter_input_to_wit(value: &api::AdapterInput<'_>) -> wit::AdapterInput {
    let api::AdapterInput {
        resource,
        bytes,
        max_rows,
    } = value;
    wit::AdapterInput {
        resource: resource_to_wit(resource),
        bytes: bytes.to_vec(),
        max_rows: wire(*max_rows),
    }
}

fn analyzer_input_to_wit(value: &api::AnalyzerInput<'_>) -> wit::AnalyzerInput {
    let api::AnalyzerInput {
        rule_id,
        rows,
        max_findings,
    } = value;
    wit::AnalyzerInput {
        rule_id: (*rule_id).to_string(),
        rows: rows.iter().map(row_to_wit).collect(),
        max_findings: wire(*max_findings),
    }
}

/// A `plugin_api` adapter behind the generated trait, as a process or WASM host
/// would serve it.
struct AdapterAsWit<T>(T);

impl<T: api::Adapter> AdapterApi for AdapterAsWit<T> {
    fn id(&self) -> String {
        self.0.id().to_string()
    }

    fn redacts_by_default(&self) -> bool {
        self.0.redacts_by_default()
    }

    fn extract(&self, input: wit::AdapterInput) -> Result<wit::AdapterOutput, wit::AdapterError> {
        let wit::AdapterInput {
            resource,
            bytes,
            max_rows,
        } = input;
        let resource = resource_from_wit(resource);
        self.0
            .extract(api::AdapterInput {
                resource: &resource,
                bytes: &bytes,
                max_rows: host(max_rows),
            })
            .map(|output| output_to_wit(&output))
            .map_err(|error| adapter_error_to_wit(&error))
    }
}

/// A generated-trait adapter behind `plugin_api`, as the engine would call a
/// plugin.
struct WitAsAdapter<T> {
    inner: T,
    id: String,
}

impl<T: AdapterApi> WitAsAdapter<T> {
    fn new(inner: T) -> Self {
        let id = inner.id();
        Self { inner, id }
    }
}

impl<T: AdapterApi + Send + Sync> api::Adapter for WitAsAdapter<T> {
    fn id(&self) -> &str {
        &self.id
    }

    fn redacts_by_default(&self) -> bool {
        self.inner.redacts_by_default()
    }

    fn extract(
        &self,
        input: api::AdapterInput<'_>,
    ) -> Result<api::AdapterOutput, api::AdapterError> {
        self.inner
            .extract(adapter_input_to_wit(&input))
            .map(output_from_wit)
            .map_err(adapter_error_from_wit)
    }
}

/// A `plugin_api` analyzer behind the generated trait.
struct AnalyzerAsWit<T>(T);

impl<T: api::Analyzer> AnalyzerApi for AnalyzerAsWit<T> {
    fn id(&self) -> String {
        self.0.id().to_string()
    }

    fn analyze(&self, input: wit::AnalyzerInput) -> Result<Vec<wit::Finding>, wit::AnalyzerError> {
        let wit::AnalyzerInput {
            rule_id,
            rows,
            max_findings,
        } = input;
        let rows: Vec<api::Row> = rows.into_iter().map(row_from_wit).collect();
        self.0
            .analyze(api::AnalyzerInput {
                rule_id: &rule_id,
                rows: &rows,
                max_findings: host(max_findings),
            })
            .map(|findings| findings.iter().map(finding_to_wit).collect())
            .map_err(|error| analyzer_error_to_wit(&error))
    }
}

/// A generated-trait analyzer behind `plugin_api`.
struct WitAsAnalyzer<T> {
    inner: T,
    id: String,
}

impl<T: AnalyzerApi> WitAsAnalyzer<T> {
    fn new(inner: T) -> Self {
        let id = inner.id();
        Self { inner, id }
    }
}

impl<T: AnalyzerApi + Send + Sync> api::Analyzer for WitAsAnalyzer<T> {
    fn id(&self) -> &str {
        &self.id
    }

    fn analyze(
        &self,
        input: api::AnalyzerInput<'_>,
    ) -> Result<Vec<api::Finding>, api::AnalyzerError> {
        self.inner
            .analyze(analyzer_input_to_wit(&input))
            .map(|findings| findings.into_iter().map(finding_from_wit).collect())
            .map_err(analyzer_error_from_wit)
    }
}

/// Every node kind, in a value `FlatValue::new` accepts.
fn sample_value() -> api::FlatValue {
    api::FlatValue::new(
        vec![
            api::Node::Record(vec![("items".to_string(), 1), ("meta".to_string(), 2)]),
            api::Node::List(vec![3, 4, 5, 6]),
            api::Node::Record(vec![("path".to_string(), 7), ("raw".to_string(), 8)]),
            api::Node::Null,
            api::Node::Bool(true),
            api::Node::Number("1e3".to_string()),
            api::Node::String("text".to_string()),
            api::Node::Path("src/lib.rs".to_string()),
            api::Node::Bytes(vec![0, 255]),
        ],
        api::FlatValueLimits::default(),
    )
    .expect("a valid flat value")
}

fn sample_row(path: &str, ordinal: u32, state: api::RowState) -> api::Row {
    api::Row {
        resource: api::Resource {
            path: path.to_string(),
            indexed: true,
            index: Some(7),
        },
        pointer: Some(vec![
            api::PointerSegment::Key("items".to_string()),
            api::PointerSegment::Index(2),
        ]),
        label: Some("items[2]".to_string()),
        ordinal,
        start: Some(10),
        end: Some(42),
        state,
        original: Some(sample_value()),
    }
}

fn sample_finding() -> api::Finding {
    api::Finding {
        path: Some("locales/de.json".to_string()),
        message: "key `nav.home` is missing".to_string(),
        line: Some(3),
        column: Some(5),
        related: vec![api::RelatedLocation {
            path: "locales/en.json".to_string(),
            label: "defined here".to_string(),
            line: Some(4),
            column: None,
        }],
        baseline_key: Some("nav.home".to_string()),
        sensitive: true,
        analysis_failure: Some("unreadable".to_string()),
        failure_kind: Some("resource-read".to_string()),
        not_fixable: true,
    }
}

#[test]
fn every_contract_type_converts_without_loss() {
    let states = [
        api::RowState::Value(sample_value()),
        api::RowState::Key,
        api::RowState::Missing,
        api::RowState::Error("bad".to_string()),
    ];
    for (ordinal, state) in (0..).zip(states) {
        let row = sample_row("a.json", ordinal, state);
        assert_eq!(row_from_wit(row_to_wit(&row)), row);
    }
    let bare = api::Row {
        pointer: None,
        label: None,
        start: None,
        end: None,
        original: None,
        ..sample_row("b.json", 0, api::RowState::Missing)
    };
    assert_eq!(row_from_wit(row_to_wit(&bare)), bare);

    let finding = sample_finding();
    assert_eq!(finding_from_wit(finding_to_wit(&finding)), finding);
    let output = api::AdapterOutput {
        rows: vec![sample_row("a.json", 0, api::RowState::Key)],
        non_strings: vec!["number".to_string()],
        notes: vec!["note".to_string()],
    };
    assert_eq!(output_from_wit(output_to_wit(&output)), output);
    for error in [
        api::AdapterError::Parse("bad".to_string()),
        api::AdapterError::Limit {
            limit: 1,
            reached: 2,
        },
    ] {
        assert_eq!(adapter_error_from_wit(adapter_error_to_wit(&error)), error);
    }
    for error in [
        api::AnalyzerError::InvalidInput("bad".to_string()),
        api::AnalyzerError::Limit {
            limit: 3,
            reached: 4,
        },
    ] {
        assert_eq!(
            analyzer_error_from_wit(analyzer_error_to_wit(&error)),
            error
        );
    }
    let capabilities = api::Capabilities {
        spawn_user_command: true,
        host_keyring: false,
    };
    assert_eq!(
        capabilities_from_wit(&capabilities_to_wit(capabilities)),
        capabilities
    );
}

#[test]
fn a_plugin_api_row_survives_the_protobuf_wire() {
    let row = sample_row("a.json", 1, api::RowState::Value(sample_value()));
    let wire_row = glue::row::to_proto(&row_to_wit(&row));
    let decoded = glue::row::from_proto(wire_row).expect("the row decodes");
    assert_eq!(row_from_wit(decoded), row);
}

/// Returns one row naming its input, or a limit error when no rows may be
/// returned.
struct ApiEcho;

impl api::Adapter for ApiEcho {
    fn id(&self) -> &'static str {
        "api-echo"
    }

    fn redacts_by_default(&self) -> bool {
        true
    }

    fn extract(
        &self,
        input: api::AdapterInput<'_>,
    ) -> Result<api::AdapterOutput, api::AdapterError> {
        if input.max_rows == 0 {
            return Err(api::AdapterError::Limit {
                limit: 0,
                reached: 1,
            });
        }
        let text = String::from_utf8_lossy(input.bytes).into_owned();
        Ok(api::AdapterOutput {
            rows: vec![api::Row {
                resource: input.resource.clone(),
                ..sample_row("unused", 0, api::RowState::Error(text))
            }],
            non_strings: Vec::new(),
            notes: vec![format!("at most {} rows", input.max_rows)],
        })
    }
}

/// The same behaviour written against the generated trait.
struct WitEcho;

impl AdapterApi for WitEcho {
    fn id(&self) -> String {
        "wit-echo".to_string()
    }

    fn redacts_by_default(&self) -> bool {
        false
    }

    fn extract(&self, input: wit::AdapterInput) -> Result<wit::AdapterOutput, wit::AdapterError> {
        if input.max_rows == 0 {
            return Err(wit::AdapterError::Limit(wit::LimitError {
                limit: 0,
                reached: 1,
            }));
        }
        let text = String::from_utf8_lossy(&input.bytes).into_owned();
        let mut row = row_to_wit(&sample_row("unused", 0, api::RowState::Error(text)));
        row.resource = input.resource;
        Ok(wit::AdapterOutput {
            rows: vec![row],
            non_strings: Vec::new(),
            notes: vec![format!("at most {} rows", input.max_rows)],
        })
    }
}

#[test]
fn adapters_run_behind_either_trait() {
    let resource = api::Resource {
        path: "config/app.json".to_string(),
        indexed: false,
        index: None,
    };
    let input = || api::AdapterInput {
        resource: &resource,
        bytes: b"{}",
        max_rows: 5,
    };

    let served = AdapterAsWit(ApiEcho);
    assert_eq!(served.id(), "api-echo");
    assert!(served.redacts_by_default());
    let direct = api::Adapter::extract(&ApiEcho, input()).expect("extracts");
    let through_wit = served
        .extract(adapter_input_to_wit(&input()))
        .expect("extracts");
    assert_eq!(output_from_wit(through_wit), direct);
    assert_eq!(
        served.extract(adapter_input_to_wit(&api::AdapterInput {
            max_rows: 0,
            ..input()
        })),
        Err(wit::AdapterError::Limit(wit::LimitError {
            limit: 0,
            reached: 1
        }))
    );

    let plugin: Box<dyn api::Adapter> = Box::new(WitAsAdapter::new(WitEcho));
    assert_eq!(plugin.id(), "wit-echo");
    assert!(!plugin.redacts_by_default());
    let output = plugin.extract(input()).expect("extracts");
    assert_eq!(output.rows[0].resource, resource);
    assert_eq!(output.rows[0].state, api::RowState::Error("{}".to_string()));
    assert_eq!(output.notes, ["at most 5 rows"]);
    assert_eq!(
        plugin
            .extract(api::AdapterInput {
                max_rows: 0,
                ..input()
            })
            .unwrap_err(),
        api::AdapterError::Limit {
            limit: 0,
            reached: 1
        }
    );
}

/// One finding per row, or a limit error past `max_findings`.
struct ApiPerRow;

impl api::Analyzer for ApiPerRow {
    fn id(&self) -> &'static str {
        "api-per-row"
    }

    fn analyze(
        &self,
        input: api::AnalyzerInput<'_>,
    ) -> Result<Vec<api::Finding>, api::AnalyzerError> {
        if input.rows.len() > input.max_findings {
            return Err(api::AnalyzerError::Limit {
                limit: input.max_findings,
                reached: input.rows.len(),
            });
        }
        Ok(input
            .rows
            .iter()
            .map(|row| api::Finding {
                path: Some(row.resource.path.clone()),
                message: format!("{}: row {}", input.rule_id, row.ordinal),
                ..sample_finding()
            })
            .collect())
    }
}

/// The same behaviour written against the generated trait.
struct WitPerRow;

impl AnalyzerApi for WitPerRow {
    fn id(&self) -> String {
        "wit-per-row".to_string()
    }

    fn analyze(&self, input: wit::AnalyzerInput) -> Result<Vec<wit::Finding>, wit::AnalyzerError> {
        if input.rows.len() > host(input.max_findings) {
            return Err(wit::AnalyzerError::Limit(wit::LimitError {
                limit: input.max_findings,
                reached: wire(input.rows.len()),
            }));
        }
        Ok(input
            .rows
            .iter()
            .map(|row| {
                let mut finding = finding_to_wit(&sample_finding());
                finding.path = Some(row.resource.path.clone());
                finding.message = format!("{}: row {}", input.rule_id, row.ordinal);
                finding
            })
            .collect())
    }
}

#[test]
fn analyzers_run_behind_either_trait() {
    let rows = [
        sample_row("a.json", 0, api::RowState::Key),
        sample_row("b.json", 1, api::RowState::Missing),
    ];
    let input = |max_findings| api::AnalyzerInput {
        rule_id: "parity",
        rows: &rows,
        max_findings,
    };

    let served = AnalyzerAsWit(ApiPerRow);
    assert_eq!(served.id(), "api-per-row");
    let direct = api::Analyzer::analyze(&ApiPerRow, input(10)).expect("analyzes");
    let through_wit = served
        .analyze(analyzer_input_to_wit(&input(10)))
        .expect("analyzes");
    assert_eq!(
        through_wit
            .into_iter()
            .map(finding_from_wit)
            .collect::<Vec<_>>(),
        direct
    );
    assert_eq!(
        served.analyze(analyzer_input_to_wit(&input(1))),
        Err(wit::AnalyzerError::Limit(wit::LimitError {
            limit: 1,
            reached: 2
        }))
    );

    let plugin: Box<dyn api::Analyzer> = Box::new(WitAsAnalyzer::new(WitPerRow));
    assert_eq!(plugin.id(), "wit-per-row");
    let findings = plugin.analyze(input(10)).expect("analyzes");
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>(),
        ["parity: row 0", "parity: row 1"]
    );
    assert_eq!(findings[1].path.as_deref(), Some("b.json"));
    assert_eq!(
        plugin.analyze(input(1)).unwrap_err(),
        api::AnalyzerError::Limit {
            limit: 1,
            reached: 2
        }
    );
}
