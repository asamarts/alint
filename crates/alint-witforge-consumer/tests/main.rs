mod inventory;
mod plugin_api;
mod worlds;

use alint_witforge_consumer::glue::GlueError;
use alint_witforge_consumer::glue::v0::adapter_api::functions::{
    extract, id as adapter_id, redacts_by_default,
};
use alint_witforge_consumer::glue::v0::analyzer_api::functions::{analyze, id as analyzer_id};
use alint_witforge_consumer::glue::v0::types::{
    adapter_error, adapter_input, adapter_output, finding, node, plugin_info, repo_resource, row,
    row_state,
};
use alint_witforge_consumer::plain::v0_1::types::{
    AdapterError, AdapterInput, AdapterOutput, AnalyzerError, AnalyzerInput, Capabilities, Finding,
    FlatValue, LimitError, Node, PluginInfo, PointerSegment, RecordField, RelatedLocation,
    RepoResource, Row, RowState,
};
use alint_witforge_consumer::proto::v0 as p;

fn roundtrip<T>(value: &T) -> T
where
    T: ::prost::Message
        + ::std::default::Default
        + ::serde::Serialize
        + for<'de> ::serde::Deserialize<'de>,
{
    let binary = value.encode_to_vec();
    let decoded = T::decode(binary.as_slice()).unwrap();
    let json = serde_json::to_string(&decoded).unwrap();
    serde_json::from_str(&json).unwrap()
}

#[allow(clippy::too_many_lines)]
#[test]
fn generated_contract_compiles_and_roundtrips() {
    let resource = RepoResource {
        path: "src/lib.rs".to_string(),
        indexed: true,
        index: Some(3),
    };
    let flat = FlatValue {
        nodes: vec![
            Node::Null,
            Node::Bool(true),
            Node::Number("12".to_string()),
            Node::String("text".to_string()),
            Node::Bytes(vec![0, 1, 2]),
            Node::Path("src/lib.rs".to_string()),
            Node::List(vec![0, 1]),
            Node::Record(vec![RecordField {
                name: "field".to_string(),
                child: 2,
            }]),
        ],
    };
    let row = Row {
        resource: resource.clone(),
        pointer: Some(vec![
            PointerSegment::Key("field".to_string()),
            PointerSegment::Index(2),
        ]),
        label: Some("field".to_string()),
        ordinal: 0,
        start: Some(10),
        end: Some(20),
        state: RowState::Value(flat.clone()),
        original: Some(flat.clone()),
    };
    let output = AdapterOutput {
        rows: vec![row.clone()],
        non_strings: vec!["number".to_string()],
        notes: vec!["note".to_string()],
    };
    let input = AdapterInput {
        resource: resource.clone(),
        bytes: vec![b'{', b'}'],
        max_rows: 100,
    };

    let resource_proto = repo_resource::to_proto(&resource);
    assert_eq!(
        repo_resource::from_proto(roundtrip(&resource_proto)).unwrap(),
        resource
    );
    let node_proto = node::to_proto(&flat.nodes[6]);
    assert_eq!(
        node::from_proto(roundtrip(&node_proto)).unwrap(),
        flat.nodes[6]
    );

    let mut missing = resource_proto.clone();
    missing.path = None;
    assert!(matches!(
        repo_resource::from_proto(missing),
        Err(GlueError::MissingField {
            field: "types/repo-resource.path"
        })
    ));
    let optional_resource = p::TypesRepoResource {
        path: Some("optional".to_string()),
        indexed: Some(false),
        index: None,
    };
    assert_eq!(
        repo_resource::from_proto(optional_resource).unwrap().index,
        None
    );
    let mut unknown = node_proto;
    unknown.witforge_value = None;
    assert!(matches!(
        node::from_proto(unknown),
        Err(GlueError::UnknownCase {
            message: "types/node"
        })
    ));

    let finding_proto = finding::to_proto(&Finding {
        path: None,
        message: "adversarial".to_string(),
        line: None,
        column: None,
        related: Vec::new(),
        baseline_key: None,
        sensitive: false,
        analysis_failure: None,
        failure_kind: None,
        not_fixable: false,
    });
    let mut missing_message = finding_proto.clone();
    missing_message.message = None;
    assert!(matches!(
        finding::from_proto(missing_message),
        Err(GlueError::MissingField {
            field: "types/finding.message"
        })
    ));
    let optional_finding = p::TypesFinding {
        message: Some("kept".to_string()),
        ..finding_proto
    };
    assert_eq!(finding::from_proto(optional_finding).unwrap().path, None);

    let row_proto = row::to_proto(&row);
    let mut missing_resource = row_proto.clone();
    missing_resource.resource = None;
    assert!(matches!(
        row::from_proto(missing_resource),
        Err(GlueError::MissingField {
            field: "types/row.resource"
        })
    ));
    let mut optional_row = row_proto;
    optional_row.pointer = None;
    optional_row.label = None;
    optional_row.start = None;
    optional_row.end = None;
    let decoded_row = row::from_proto(optional_row).unwrap();
    assert_eq!(decoded_row.pointer, None);
    assert_eq!(decoded_row.label, None);

    let input_proto = adapter_input::to_proto(&input);
    let mut missing_bytes = input_proto.clone();
    missing_bytes.bytes = None;
    assert!(matches!(
        adapter_input::from_proto(missing_bytes),
        Err(GlueError::MissingField {
            field: "types/adapter-input.bytes"
        })
    ));
    let mut missing_max_rows = input_proto;
    missing_max_rows.max_rows = None;
    assert!(matches!(
        adapter_input::from_proto(missing_max_rows),
        Err(GlueError::MissingField {
            field: "types/adapter-input.max-rows"
        })
    ));
    let output_proto = adapter_output::to_proto(&AdapterOutput {
        rows: Vec::new(),
        non_strings: Vec::new(),
        notes: Vec::new(),
    });
    assert_eq!(
        adapter_output::from_proto(output_proto).unwrap(),
        AdapterOutput {
            rows: Vec::new(),
            non_strings: Vec::new(),
            notes: Vec::new()
        }
    );
    assert!(matches!(
        row_state::from_proto(p::TypesRowState {
            witforge_value: None
        }),
        Err(GlueError::UnknownCase {
            message: "types/row-state"
        })
    ));
    assert!(matches!(
        adapter_error::from_proto(p::TypesAdapterError {
            witforge_value: None
        }),
        Err(GlueError::UnknownCase {
            message: "types/adapter-error"
        })
    ));

    assert_eq!(adapter_id::METHOD, p::AdapterApiMethod::Id);
    assert_eq!(
        redacts_by_default::METHOD,
        p::AdapterApiMethod::RedactsByDefault
    );
    assert_eq!(extract::METHOD, p::AdapterApiMethod::Extract);
    assert_eq!(
        adapter_id::from_request(roundtrip(&adapter_id::to_request())).unwrap(),
        ()
    );
    assert_eq!(
        adapter_id::from_response(roundtrip(&adapter_id::to_response(&"adapter".to_string())))
            .unwrap(),
        "adapter".to_string()
    );
    assert!(
        redacts_by_default::from_response(roundtrip(&redacts_by_default::to_response(&true)))
            .unwrap()
    );
    let request = extract::to_request(&input);
    assert_eq!(extract::from_request(roundtrip(&request)).unwrap(), input);
    let response = extract::to_response(&Ok(output.clone()));
    assert_eq!(
        extract::from_response(roundtrip(&response)).unwrap(),
        Ok(output.clone())
    );
    let error_response = extract::to_response(&Err(AdapterError::Parse("bad".to_string())));
    assert_eq!(
        extract::from_response(roundtrip(&error_response)).unwrap(),
        Err(AdapterError::Parse("bad".to_string()))
    );
    let limit_error = AdapterError::Limit(LimitError {
        limit: 10,
        reached: 11,
    });
    assert_eq!(
        extract::from_response(roundtrip(&extract::to_response(&Err(limit_error.clone()))))
            .unwrap(),
        Err(limit_error)
    );

    let finding = Finding {
        path: Some("src/lib.rs".to_string()),
        message: "message".to_string(),
        line: Some(1),
        column: Some(2),
        related: vec![RelatedLocation {
            path: "src/lib.rs".to_string(),
            label: "related".to_string(),
            line: Some(1),
            column: Some(3),
        }],
        baseline_key: Some("baseline".to_string()),
        sensitive: true,
        analysis_failure: None,
        failure_kind: Some("resource-parse".to_string()),
        not_fixable: true,
    };
    let analyzer_input = AnalyzerInput {
        rule_id: "rule".to_string(),
        rows: vec![row],
        max_findings: 100,
    };

    assert_eq!(analyzer_id::METHOD, p::AnalyzerApiMethod::Id);
    assert_eq!(analyze::METHOD, p::AnalyzerApiMethod::Analyze);
    assert_eq!(
        analyzer_id::from_response(roundtrip(&analyzer_id::to_response(
            &"analyzer".to_string()
        )))
        .unwrap(),
        "analyzer".to_string()
    );
    let analyze_request = analyze::to_request(&analyzer_input);
    assert_eq!(
        analyze::from_request(roundtrip(&analyze_request)).unwrap(),
        analyzer_input
    );
    let findings = vec![finding.clone()];
    assert_eq!(
        analyze::from_response(roundtrip(&analyze::to_response(&Ok(findings.clone())))).unwrap(),
        Ok(findings)
    );
    let analyzer_error = AnalyzerError::Limit(LimitError {
        limit: 10,
        reached: 11,
    });
    assert_eq!(
        analyze::from_response(roundtrip(&analyze::to_response(&Err(
            analyzer_error.clone()
        ))))
        .unwrap(),
        Err(analyzer_error)
    );
    assert_eq!(
        format!("{}", GlueError::InvalidState { message: "x" }),
        "message `x` is invalid"
    );
}

/// `plugin-info` has no `plugin_api` counterpart yet, so only this test
/// exercises its glue, and that of the `capabilities` inside it.
#[test]
fn plugin_info_roundtrips() {
    for (spawn_user_command, host_keyring) in [(true, false), (false, true)] {
        let info = PluginInfo {
            name: "example".to_string(),
            version: "1.2.3".to_string(),
            contract: ">=0.1, <0.2".to_string(),
            capabilities: Capabilities {
                spawn_user_command,
                host_keyring,
            },
        };
        let decoded = plugin_info::from_proto(roundtrip(&plugin_info::to_proto(&info))).unwrap();
        assert_eq!(decoded, info);
    }
}
