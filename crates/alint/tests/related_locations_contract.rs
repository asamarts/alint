//! Related-location completeness for `constraint` findings, an output
//! completeness contract in the sense of ADR-0012: every related location the
//! engine produces for a finding reaches each format that has a place for it,
//! in the order the finding holds them, with the same path, range and label,
//! and with the count of the ones it left out.
//!
//! One fixture produces a finding of each family: `equal` in its three forms
//! (within the relation, against a source row, against another field of the
//! row), `unique` with and without `determines:`, and `set` with a missing,
//! an extra and a shared value. Two of the findings carry more than 20 related
//! rows, one with ranges and one without, and two have no path (a group only
//! `groups:` names). The engine's findings, run in process, are the truth;
//! JSON and SARIF come from `alint check`, and the language server's from
//! `alint lsp` over its protocol.
//!
//! How each format shows the related locations a finding left out:
//! - JSON: `related_locations_omitted`, the count, absent when zero;
//! - SARIF: one more `relatedLocations` entry, after the others, at the
//!   config file with no region, whose message is `and N more related
//!   locations`;
//! - the language server: one more `relatedInformation` entry, after the
//!   others, at the diagnostic's own location, with the same message.
//!
//! A finding without a path sits, in SARIF, at the config file; the language
//! server anchors it to the config file as well, and the test asserts that.
//! The `github`, `gitlab`, `junit` and `markdown` formats carry one location
//! per finding, or none, so they are not covered here, and neither is
//! `agent`, which carries no related locations.
//!
//! The language server's part runs on Unix only, as the other tests of its
//! protocol do: they build `file://` URIs by hand.

use std::path::{Component, Path};

use serde_json::Value;

const CONFIG: &str = r#"version: 2
selectors:
  manifests:
    paths: "packages/*/package.json"
relations:
  versions:
    from: manifests
    read: { json: "$.version" }
    fields: { version: value }
  node-pins:
    union:
      - from: { paths: .nvmrc }
        read: { whole_file: {} }
      - from: manifests
        read: { json: "$.engines.node" }
    fields: { source: path, node: value }
    transform: { node: [trim] }
  aliases:
    from: { paths: exports.json }
    entries: { format: json, at: "$.aliases" }
    fields: { name: entry.key, target: entry.value }
  names:
    from: manifests
    read: { json: "$.name" }
    fields: { name: value }
  dependency-ranges:
    from: manifests
    entries: { format: json, at: "$.dependencies" }
    fields: { dependency: entry.key, range: entry.value }
  messages:
    from: { paths: "locales/*.json" }
    entries: { format: json, at: "$" }
    fields: { locale: stem, key: entry.key }
  banned:
    from: { paths: banned.json }
    entries: { format: json, at: "$" }
    fields: { name: entry.key }
rules:
  - id: one-version
    kind: constraint
    input: versions
    assert: { equal: { field: version } }
    level: error
  - id: node-matches-nvmrc
    kind: constraint
    input: node-pins
    assert: { equal: { field: node, to: { row: { source: .nvmrc } } } }
    level: error
  - id: alias-names-its-target
    kind: constraint
    input: aliases
    assert: { equal: { field: target, to: { field: name } } }
    level: error
  - id: unique-names
    kind: constraint
    input: names
    assert: { unique: { key: [name] } }
    level: error
  - id: one-range-per-dependency
    kind: constraint
    input: dependency-ranges
    assert: { unique: { key: [dependency], determines: [range] } }
    level: error
  - id: locales-match-english
    kind: constraint
    input: messages
    assert: { set: { field: key, group_by: locale, equal_to: { group: en }, groups: [de] } }
    level: error
  - id: no-banned-dependencies
    kind: constraint
    input: dependency-ranges
    assert: { set: { field: dependency, disjoint_from: { relation: banned, field: name } } }
    level: error
"#;

/// A related location: its path's components, its range (1-based lines and
/// columns, end exclusive) when it has one, and its label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Related {
    path: Vec<String>,
    range: Option<[u64; 4]>,
    label: String,
}

/// A finding as a format shows it: its rule, its message, its path's
/// components (none for a finding without a path), its related locations in
/// order, and how many more it left out.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Finding {
    rule: String,
    message: String,
    path: Option<Vec<String>>,
    related: Vec<Related>,
    omitted: u64,
}

fn components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str().unwrap().to_string(),
            other => panic!("{other:?} in a repository-relative path"),
        })
        .collect()
}

fn uri_components(uri: &str) -> Vec<String> {
    uri.split('/').map(ToString::to_string).collect()
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = relative
        .split('/')
        .fold(root.to_path_buf(), |path, part| path.join(part));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// The fixture: 26 manifests, of which `p25` has another version, `p03`
/// another Node version, `p05` and `p06` one name, `p07` another `react`
/// range and `p09` a banned dependency; an alias that names another target;
/// and locales where `fr` lacks a key, `es` has one more, and `de` (named only
/// in `groups:`) has no file.
fn fixture() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write(root, ".alint.yml", CONFIG);
    write(root, ".nvmrc", "20.11.1\n");
    write(
        root,
        "exports.json",
        "{\n  \"aliases\": {\n    \"button\": \"button\",\n    \"card\": \"panel\"\n  }\n}\n",
    );
    write(
        root,
        "banned.json",
        "{\n  \"left-pad\": \"unmaintained\"\n}\n",
    );
    write(
        root,
        "locales/en.json",
        "{\n  \"home\": \"Home\",\n  \"about\": \"About\"\n}\n",
    );
    write(root, "locales/fr.json", "{\n  \"home\": \"Accueil\"\n}\n");
    write(
        root,
        "locales/es.json",
        "{\n  \"home\": \"Inicio\",\n  \"about\": \"Acerca\",\n  \"extra\": \"Extra\"\n}\n",
    );
    for package in 0..26 {
        let name = format!("p{package:02}");
        let version = if package == 25 { "2.0.0" } else { "1.0.0" };
        let node = if package == 3 { "18.19.0" } else { "20.11.1" };
        let declared = if matches!(package, 5 | 6) {
            "dup"
        } else {
            &name
        };
        let react = if package == 7 { "^17.0.2" } else { "^18.2.0" };
        let banned = if package == 9 {
            ",\n    \"left-pad\": \"^1.3.0\""
        } else {
            ""
        };
        write(
            root,
            &format!("packages/{name}/package.json"),
            &format!(
                "{{\n  \"name\": \"{declared}\",\n  \"version\": \"{version}\",\n  \
                 \"engines\": {{\"node\": \"{node}\"}},\n  \"dependencies\": {{\n    \
                 \"react\": \"{react}\"{banned}\n  }}\n}}\n"
            ),
        );
    }
    tmp
}

/// The engine's findings, in process: the truth each format must show.
fn engine_findings(root: &Path) -> Vec<Finding> {
    let registry = alint_rules::builtin_registry();
    let options = alint_dsl::LoadOptions::default()
        .with_kind_catalog(Some(registry.catalog()))
        .with_global_git_excludes(false);
    let config = alint_dsl::load_with(&root.join(".alint.yml"), &options).unwrap();
    let entries = config
        .rules
        .iter()
        .map(|spec| alint_core::RuleEntry::new(registry.build(spec).unwrap()))
        .collect();
    let engine = alint_core::Engine::from_entries(entries, registry);
    let index = alint_core::walk(
        root,
        &alint_core::WalkOptions::default().with_global_git_excludes(false),
    )
    .unwrap();
    let report = engine.run(root, &index).unwrap();
    let mut findings = Vec::new();
    for result in &report.results {
        assert!(result.analysis_failures.is_empty(), "{result:?}");
        for violation in &result.violations {
            findings.push(Finding {
                rule: result.rule_id.to_string(),
                message: violation.message.to_string(),
                path: violation.path.as_deref().map(components),
                related: violation
                    .related_locations
                    .iter()
                    .map(|location| Related {
                        path: components(&location.path),
                        range: location.range.as_ref().map(|range| {
                            [
                                range.start_line(),
                                range.start_column(),
                                range.end_line(),
                                range.end_column(),
                            ]
                            .map(|n| u64::try_from(n).unwrap())
                        }),
                        label: location.label.clone(),
                    })
                    .collect(),
                omitted: u64::try_from(violation.related_locations_omitted).unwrap(),
            });
        }
    }
    findings
}

fn check(root: &Path, format: &str) -> Value {
    let output = crate::hermetic::command(env!("CARGO_BIN_EXE_alint"))
        .args(["check", "--format", format])
        .current_dir(root)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn range_of(value: &Value, keys: [&str; 4]) -> Option<[u64; 4]> {
    value.get(keys[0])?;
    Some(keys.map(|key| value[key].as_u64().unwrap()))
}

/// The findings of `alint check --format json`.
fn json_findings(report: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    for result in report["results"].as_array().unwrap() {
        for violation in result["violations"].as_array().unwrap() {
            let related = violation
                .get("related_locations")
                .map(|locations| {
                    let locations = locations.as_array().unwrap();
                    // Absent, never empty.
                    assert!(!locations.is_empty(), "{violation}");
                    locations
                        .iter()
                        .map(|location| Related {
                            path: components(Path::new(location["path"].as_str().unwrap())),
                            range: range_of(location, ["line", "column", "end_line", "end_column"]),
                            label: location["label"].as_str().unwrap().to_string(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let omitted = violation
                .get("related_locations_omitted")
                .map(|count| count.as_u64().unwrap())
                .inspect(|&count| assert!(count > 0, "{violation}"))
                .unwrap_or(0);
            findings.push(Finding {
                rule: result["id"].as_str().unwrap().to_string(),
                message: violation["message"].as_str().unwrap().to_string(),
                path: violation
                    .get("path")
                    .map(|path| components(Path::new(path.as_str().unwrap()))),
                related,
                omitted,
            });
        }
    }
    findings
}

/// The `and N more related locations` message SARIF and the language server
/// give the entry that counts what a finding left out.
fn omitted_count(message: &str) -> Option<u64> {
    message
        .strip_prefix("and ")?
        .strip_suffix(" more related locations")?
        .parse()
        .ok()
}

/// The findings of `alint check --format sarif`. A result at the config file
/// is a finding without a path.
fn sarif_findings(report: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();
    for result in report["runs"][0]["results"].as_array().unwrap() {
        let uri = result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
            .as_str()
            .unwrap();
        let mut related = Vec::new();
        let mut omitted = 0;
        for location in result["relatedLocations"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let physical = &location["physicalLocation"];
            let path = uri_components(physical["artifactLocation"]["uri"].as_str().unwrap());
            let label = location["message"]["text"].as_str().unwrap().to_string();
            let range = physical.get("region").map(|region| {
                range_of(region, ["startLine", "startColumn", "endLine", "endColumn"]).unwrap()
            });
            if let Some(count) = omitted_count(&label) {
                // The last entry, at the config file, with no region.
                assert_eq!(omitted, 0, "{result}");
                assert_eq!(path, [".alint.yml"], "{result}");
                assert_eq!(range, None, "{result}");
                omitted = count;
                continue;
            }
            assert_eq!(omitted, 0, "a related location after the count: {result}");
            related.push(Related { path, range, label });
        }
        findings.push(Finding {
            rule: result["ruleId"].as_str().unwrap().to_string(),
            message: result["message"]["text"].as_str().unwrap().to_string(),
            path: (uri != ".alint.yml").then(|| uri_components(uri)),
            related,
            omitted,
        });
    }
    findings
}

/// The fixture's findings, once each format has them, in one order.
fn sorted(mut findings: Vec<Finding>) -> Vec<Finding> {
    findings.sort();
    findings
}

/// The fixture produces what the gate needs: every rule's findings, related
/// locations with and without ranges, more than 20 of them twice, and
/// findings without a path.
#[test]
fn the_fixture_produces_each_family_of_finding() {
    let tmp = fixture();
    let findings = engine_findings(tmp.path());
    let rules: std::collections::BTreeSet<&str> = findings
        .iter()
        .map(|finding| finding.rule.as_str())
        .collect();
    assert_eq!(rules.len(), 7, "{rules:?}");
    let related = findings.iter().flat_map(|finding| &finding.related);
    assert!(related.clone().any(|location| location.range.is_some()));
    assert!(related.clone().any(|location| location.range.is_none()));
    let over = findings.iter().filter(|finding| finding.omitted > 0);
    assert_eq!(over.clone().count(), 2, "{findings:#?}");
    assert!(over.clone().all(|finding| finding.related.len() == 20));
    assert_eq!(
        findings
            .iter()
            .filter(|finding| finding.path.is_none() && !finding.related.is_empty())
            .count(),
        2,
        "{findings:#?}"
    );
    // The `set` finding for a value only `es` holds has no related location.
    assert!(findings.iter().any(|finding| finding.related.is_empty()));
}

#[test]
fn json_carries_every_related_location_of_each_finding() {
    let tmp = fixture();
    let expected = sorted(engine_findings(tmp.path()));
    let json = sorted(json_findings(&check(tmp.path(), "json")));
    assert_eq!(json, expected);
}

#[test]
fn sarif_carries_every_related_location_of_each_finding() {
    let tmp = fixture();
    let expected = sorted(engine_findings(tmp.path()));
    let sarif = sorted(sarif_findings(&check(tmp.path(), "sarif")));
    assert_eq!(sarif, expected);
}

#[cfg(unix)]
mod language_server {
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::time::Instant;

    use serde_json::{Value, json};

    use super::{Finding, Related, engine_findings, fixture, omitted_count, sorted};
    use crate::lsp_common::{RECV_TIMEOUT, recv_response, spawn_server};

    fn uri(path: &Path) -> String {
        format!("file://{}", path.to_str().unwrap())
    }

    /// The diagnostics the server publishes for each of `files` and the
    /// config file, by URI, once it has published for every one.
    fn diagnostics(root: &Path, files: &[Vec<String>]) -> BTreeMap<String, Vec<Value>> {
        let mut server = spawn_server(root);
        server.send(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "rootUri": uri(root),
                "capabilities": {
                    "textDocument": { "publishDiagnostics": { "relatedInformation": true } }
                }
            }
        }));
        recv_response(&server.rx, 1);
        server.send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
        let mut wanted = vec![uri(&root.join(".alint.yml"))];
        for file in files {
            let path = file
                .iter()
                .fold(root.to_path_buf(), |path, part| path.join(part));
            let file_uri = uri(&path);
            server.send(&json!({
                "jsonrpc": "2.0", "method": "textDocument/didOpen",
                "params": { "textDocument": {
                    "uri": file_uri, "languageId": "json", "version": 1,
                    "text": std::fs::read_to_string(&path).unwrap()
                }}
            }));
            wanted.push(file_uri);
        }
        let mut published = BTreeMap::new();
        let deadline = Instant::now() + RECV_TIMEOUT;
        while wanted.iter().any(|uri| !published.contains_key(uri)) {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("timed out waiting for diagnostics");
            let message = server.rx.recv_timeout(remaining).unwrap();
            if message["method"] == "textDocument/publishDiagnostics" {
                published.insert(
                    message["params"]["uri"].as_str().unwrap().to_string(),
                    message["params"]["diagnostics"]
                        .as_array()
                        .cloned()
                        .unwrap(),
                );
            }
        }
        server.shutdown();
        published
    }

    /// A range as 1-based lines and columns. The fixture is ASCII, so a
    /// UTF-16 column is a character's.
    fn range(value: &Value) -> [u64; 4] {
        let at = |end: &str, key: &str| value[end][key].as_u64().unwrap() + 1;
        [
            at("start", "line"),
            at("start", "character"),
            at("end", "line"),
            at("end", "character"),
        ]
    }

    /// Where the server puts a related location without a range: the first
    /// character of the file.
    const NO_RANGE: [u64; 4] = [1, 1, 1, 2];

    #[test]
    fn the_language_server_carries_every_related_location_of_each_finding() {
        let tmp = fixture();
        let root = tmp.path();
        let mut expected = engine_findings(root);
        let mut files: Vec<Vec<String>> = expected
            .iter()
            .filter_map(|finding| finding.path.clone())
            .collect();
        files.sort();
        files.dedup();
        let published = diagnostics(root, &files);

        let root_uri = format!("{}/", uri(root));
        let config_uri = uri(&root.join(".alint.yml"));
        let mut findings = Vec::new();
        for (file_uri, diagnostics) in &published {
            for diagnostic in diagnostics {
                let mut related = Vec::new();
                let mut omitted = 0;
                for information in diagnostic["relatedInformation"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                {
                    let location = &information["location"];
                    let label = information["message"].as_str().unwrap().to_string();
                    if let Some(count) = omitted_count(&label) {
                        // The last entry, at the diagnostic's own location.
                        assert_eq!(omitted, 0, "{diagnostic}");
                        assert_eq!(location["uri"], file_uri.as_str(), "{diagnostic}");
                        assert_eq!(location["range"], diagnostic["range"], "{diagnostic}");
                        omitted = count;
                        continue;
                    }
                    assert_eq!(
                        omitted, 0,
                        "a related location after the count: {diagnostic}"
                    );
                    let path = location["uri"]
                        .as_str()
                        .unwrap()
                        .strip_prefix(&root_uri)
                        .unwrap();
                    related.push(Related {
                        path: super::uri_components(path),
                        range: Some(range(&location["range"])),
                        label,
                    });
                }
                // A finding without a path is anchored to the config file.
                let path = (*file_uri != config_uri)
                    .then(|| super::uri_components(file_uri.strip_prefix(&root_uri).unwrap()));
                findings.push(Finding {
                    rule: diagnostic["code"].as_str().unwrap().to_string(),
                    message: diagnostic["message"].as_str().unwrap().to_string(),
                    path,
                    related,
                    omitted,
                });
            }
        }
        // The server gives every related location a range.
        for finding in &mut expected {
            for location in &mut finding.related {
                location.range.get_or_insert(NO_RANGE);
            }
        }
        assert_eq!(sorted(findings), sorted(expected));
    }
}
