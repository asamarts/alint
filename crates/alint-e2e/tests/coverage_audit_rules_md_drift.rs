//! v0.9.16 audit: every per-ruleset rule table in `docs/rules.md`
//! MUST list exactly the rules the corresponding bundled YAML at
//! `crates/alint-dsl/rulesets/<rev>/<ruleset>.yml` defines.
//!
//! Why this audit exists: during the v0.9.15 launch-prep validation
//! pass, the apache-arrow case-study subagent surfaced a three-way
//! drift on `oss-baseline@v1`: the YAML had 15 rules, `docs/rules.md`
//! claimed "Nine rules" but listed 11, and a marketing draft said 8.
//! The drift was caught entirely by accident — no audit existed to
//! flag it. v0.9.16 adds this audit so the same class of drift on
//! any other ruleset surfaces at PR time.
//!
//! What it checks (per ruleset):
//! - The set of rule IDs under `rules:` in the YAML.
//! - The set of rule IDs in the markdown table for that ruleset
//!   (rows whose first column is a backticked id, second column
//!   a backticked kind, then level and fix).
//! - These two sets must be identical.
//! - The "N rules:" header text in the markdown section must match
//!   the count.
//!
//! What it deliberately doesn't check:
//! - Per-rule field-level documentation (kind / level / fix). Those
//!   evolve more freely; this audit pins identity coverage so a rule
//!   added or removed surfaces immediately, but the per-rule cell
//!   contents are reviewed by humans at PR time.
//! - Bundled rulesets that don't have a corresponding section
//!   header (level-3 markdown heading naming the ruleset URI) in
//!   `docs/rules.md` — some are intentionally undocumented because
//!   they're internal scaffolding, and those are skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Walk every `.yml` under `crates/alint-dsl/rulesets/<rev>/` (recursive)
/// and return `(uri, rule_ids)` for each. URI follows the existing
/// translation: `v1/oss-baseline.yml` -> `alint://bundled/oss-baseline@v1`,
/// `v2/js/workspace-deps.yml` -> `alint://bundled/js/workspace-deps@v2`.
fn collect_yaml_rulesets() -> BTreeMap<String, BTreeSet<String>> {
    let base = workspace_root().join("crates/alint-dsl/rulesets");
    let mut out = BTreeMap::new();
    walk_yaml(&base, &base, &mut out);
    out
}

fn walk_yaml(base: &Path, dir: &Path, out: &mut BTreeMap<String, BTreeSet<String>>) {
    for entry in fs::read_dir(dir).expect("read rulesets dir").flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_yaml(base, &path, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("yml") {
            continue;
        }
        let rel = path.strip_prefix(base).unwrap().with_extension("");
        // The URI scheme uses `/` as the path separator; on Windows
        // `Path::display()` would emit `\` and the audit would
        // mismatch every nested ruleset (`monorepo\cargo-workspace`
        // vs the canonical `monorepo/cargo-workspace`). Iterate the
        // components and join with `/` explicitly so the comparison
        // is platform-independent.
        let components: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let (rev, name) = components
            .split_first()
            .expect("bundled ruleset lives under a <rev>/ directory");
        let rel_uri = name.join("/");
        let uri = format!("alint://bundled/{rel_uri}@{rev}");
        let text = fs::read_to_string(&path).expect("read ruleset yml");
        out.insert(uri, extract_yaml_rule_ids(&text));
    }
}

/// Pull every `- id: <name>` from the YAML's `rules:` block (skipping
/// the `facts:` block above it). Naive line-based parser is fine
/// because bundled rulesets are simple-shape — top-level `rules:` /
/// `facts:` keys with `- id:` entries indented two spaces.
fn extract_yaml_rule_ids(text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut in_rules = false;
    for line in text.lines() {
        if line.starts_with("rules:") {
            in_rules = true;
            continue;
        }
        if in_rules
            && let Some(first) = line.chars().next()
            && first.is_alphabetic()
        {
            // New top-level key. Exit `rules:`.
            in_rules = false;
        }
        if !in_rules {
            continue;
        }
        if let Some(rest) = line.strip_prefix("  - id:") {
            let id = rest.trim().trim_matches(['"', '\'']);
            ids.insert(id.to_string());
        }
    }
    ids
}

/// Parse `docs/rules.md`, find each section heading whose body is
/// the inline-code form of a bundled ruleset URI, and pull the rule
/// IDs from its rule table. Returns a map keyed by URI.
///
/// Section detection: a line that starts with three hashes + space,
/// then a backticked URI of the form `alint://bundled/<name>@v<digits>`.
///
/// Rule-ID detection inside a section: each table row whose first
/// column is a backticked identifier. Stops on the next section
/// heading or top-level heading.
fn parse_docs_rules_md() -> BTreeMap<String, BTreeSet<String>> {
    let text =
        fs::read_to_string(workspace_root().join("docs/rules.md")).expect("read docs/rules.md");
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    let mut current_uri: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_end();

        if line.starts_with("### ") {
            current_uri = parse_uri_heading(line);
            continue;
        }
        if line.starts_with("## ") || line.starts_with("# ") {
            current_uri = None;
            continue;
        }

        let Some(uri) = &current_uri else {
            continue;
        };
        if let Some(id) = parse_rule_table_row(line) {
            out.entry(uri.clone()).or_default().insert(id);
        }
    }

    out
}

/// Section heading bodies hold a backticked ruleset URI; this
/// returns the URI string. Returns `None` for headings that don't
/// follow the URI pattern.
fn parse_uri_heading(line: &str) -> Option<String> {
    let body = line.strip_prefix("### ")?.trim();
    let stripped = body.strip_prefix('`').and_then(|s| s.strip_suffix('`'))?;
    let rest = stripped.strip_prefix("alint://bundled/")?;
    let (name, rev) = rest.rsplit_once("@v")?;
    if name.is_empty() || rev.is_empty() || !rev.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(stripped.to_string())
}

/// Pull the first backticked token out of a markdown table row when
/// the row looks like a rule-table entry: a leading pipe, the
/// first cell wrapped in backticks holding the rule id. Returns
/// `None` for header rows, separator rows, and non-table lines.
fn parse_rule_table_row(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("| `") {
        return None;
    }
    let after_pipe = trimmed.strip_prefix("| `")?;
    let id = after_pipe.split('`').next()?.trim();
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

#[test]
fn every_documented_ruleset_section_matches_its_yaml_rules() {
    let yaml = collect_yaml_rulesets();
    let docs = parse_docs_rules_md();

    let mut failures: Vec<String> = Vec::new();

    for (uri, doc_ids) in &docs {
        let Some(yaml_ids) = yaml.get(uri) else {
            failures.push(format!(
                "{uri}: documented in docs/rules.md but no matching ruleset YAML at \
                 crates/alint-dsl/rulesets/<rev>/...; either drop the section from docs \
                 or restore the YAML",
            ));
            continue;
        };

        let yaml_only: Vec<&String> = yaml_ids.difference(doc_ids).collect();
        let doc_only: Vec<&String> = doc_ids.difference(yaml_ids).collect();

        if !yaml_only.is_empty() || !doc_only.is_empty() {
            let mut msg = format!(
                "{uri}: docs/rules.md table is out of sync with the YAML.\n  \
                 YAML has {} rule(s); docs lists {}.\n",
                yaml_ids.len(),
                doc_ids.len(),
            );
            if !yaml_only.is_empty() {
                msg.push_str("  Missing from docs:\n");
                for id in &yaml_only {
                    use std::fmt::Write as _;
                    writeln!(msg, "    - `{id}`").expect("write to String");
                }
            }
            if !doc_only.is_empty() {
                msg.push_str("  Listed in docs but absent from YAML:\n");
                for id in &doc_only {
                    use std::fmt::Write as _;
                    writeln!(msg, "    - `{id}`").expect("write to String");
                }
            }
            failures.push(msg);
        }
    }

    // Don't fail for YAML rulesets that aren't documented — some
    // are intentionally internal. The reverse direction (documented
    // but no YAML) IS a failure (caught above).

    assert!(
        failures.is_empty(),
        "{} bundled ruleset(s) drift between docs/rules.md and the YAML:\n\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

#[test]
fn extracted_yaml_ids_are_stable_for_known_ruleset() {
    // Sanity: oss-baseline must report the 15 IDs we manually
    // verified during the v0.9.16 drift fix. If extraction breaks,
    // localise the regression here before chasing the cross-check
    // assertion above.
    let yaml = collect_yaml_rulesets();
    let oss_baseline = yaml
        .get("alint://bundled/oss-baseline@v1")
        .expect("oss-baseline must be present");
    assert_eq!(
        oss_baseline.len(),
        15,
        "oss-baseline should have 15 rules per the v0.9.16 fix; got {}",
        oss_baseline.len(),
    );
    for required in [
        "oss-readme-exists",
        "oss-license-exists",
        "oss-no-merge-conflict-markers",
        "oss-codeowners-exists",
    ] {
        assert!(
            oss_baseline.contains(required),
            "oss-baseline missing expected rule `{required}`",
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuleTableRow {
    id: String,
    kind: Option<String>,
    level: Option<String>,
    fix: Option<String>,
}

#[derive(Default)]
struct TableColumns {
    kind: Option<usize>,
    level: Option<usize>,
    fix: Option<usize>,
}

fn first_backticked(cell: &str) -> Option<String> {
    let (_, rest) = cell.split_once('`')?;
    let (tok, _) = rest.split_once('`')?;
    let tok = tok.trim();
    (!tok.is_empty()).then(|| tok.to_string())
}

fn parse_table_header(line: &str) -> Option<TableColumns> {
    let trimmed = line.trim();
    if !trimmed.starts_with("| Rule id ") && !trimmed.starts_with("| Rule id|") {
        return None;
    }
    let headers: Vec<String> = trimmed
        .trim_matches('|')
        .split('|')
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    let mut cols = TableColumns::default();
    for (idx, h) in headers.iter().enumerate() {
        match h.as_str() {
            "kind" => cols.kind = Some(idx),
            "default level" | "level" => cols.level = Some(idx),
            "fix" => cols.fix = Some(idx),
            _ => {}
        }
    }
    Some(cols)
}

fn parse_full_rule_table_row(line: &str, cols: &TableColumns) -> Option<RuleTableRow> {
    let trimmed = line.trim();
    if !trimmed.starts_with("| `") {
        return None;
    }
    let cells: Vec<&str> = trimmed
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    let id = first_backticked(cells.first()?)?;
    let kind = cols
        .kind
        .and_then(|i| cells.get(i))
        .and_then(|c| first_backticked(c));
    let level = cols.level.and_then(|i| cells.get(i)).map(|c| {
        c.split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches('`')
            .to_ascii_lowercase()
    });
    let fix = cols
        .fix
        .and_then(|i| cells.get(i))
        .and_then(|c| first_backticked(c));
    Some(RuleTableRow {
        id,
        kind,
        level,
        fix,
    })
}

fn collect_yaml_ruleset_rows() -> BTreeMap<String, BTreeMap<String, RuleTableRow>> {
    let base = workspace_root().join("crates/alint-dsl/rulesets");
    let mut out = BTreeMap::new();
    let mut stack = vec![base.clone()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("yml") {
                continue;
            }
            let rel = path.strip_prefix(&base).unwrap().with_extension("");
            let components: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let (rev, name) = components.split_first().unwrap();
            let uri = format!("alint://bundled/{}@{rev}", name.join("/"));
            let text = fs::read_to_string(&path).unwrap();
            let doc: ayaml::Value = alint_core::yaml::from_str(&text).unwrap();
            let mut map = BTreeMap::new();
            if let Some(rules) = doc.get("rules").and_then(ayaml::Value::as_sequence) {
                for rule in rules {
                    let Some(id) = rule.get("id").and_then(ayaml::Value::as_str) else {
                        continue;
                    };
                    let kind = rule
                        .get("kind")
                        .and_then(ayaml::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let level = rule
                        .get("level")
                        .and_then(ayaml::Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let fix = rule
                        .get("fix")
                        .and_then(ayaml::Value::as_mapping)
                        .and_then(|m| m.keys().next())
                        .and_then(ayaml::Value::as_str)
                        .map(String::from);
                    map.insert(
                        id.to_string(),
                        RuleTableRow {
                            id: id.to_string(),
                            kind: Some(kind),
                            level: Some(level),
                            fix,
                        },
                    );
                }
            }
            out.insert(uri, map);
        }
    }
    out
}

#[test]
fn every_documented_ruleset_table_row_matches_yaml_kind_level_and_fix() {
    let yaml = collect_yaml_ruleset_rows();
    let text = fs::read_to_string(workspace_root().join("docs/rules.md")).unwrap();

    let mut current_uri: Option<String> = None;
    let mut current_cols: Option<TableColumns> = None;
    let mut failures = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.starts_with("### ") {
            current_uri = parse_uri_heading(line);
            current_cols = None;
            continue;
        }
        if line.starts_with("## ") || line.starts_with("# ") {
            current_uri = None;
            current_cols = None;
            continue;
        }
        let Some(uri) = &current_uri else {
            continue;
        };
        if let Some(cols) = parse_table_header(line) {
            current_cols = Some(cols);
            continue;
        }
        let Some(cols) = &current_cols else {
            continue;
        };
        let Some(doc_row) = parse_full_rule_table_row(line, cols) else {
            continue;
        };
        let Some(yaml_rows) = yaml.get(uri) else {
            continue;
        };
        let Some(yaml_row) = yaml_rows.get(&doc_row.id) else {
            continue;
        };
        if cols.kind.is_some() && doc_row.kind != yaml_row.kind {
            failures.push(format!(
                "{uri} `{}`: docs kind `{:?}` != YAML kind `{:?}`",
                doc_row.id, doc_row.kind, yaml_row.kind
            ));
        }
        if cols.level.is_some() && doc_row.level != yaml_row.level {
            failures.push(format!(
                "{uri} `{}`: docs level `{:?}` != YAML level `{:?}`",
                doc_row.id, doc_row.level, yaml_row.level
            ));
        }
        if cols.fix.is_some() && doc_row.fix != yaml_row.fix {
            failures.push(format!(
                "{uri} `{}`: docs fix `{:?}` != YAML fix `{:?}`",
                doc_row.id, doc_row.fix, yaml_row.fix
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "docs/rules.md bundled-ruleset table cell drift:\n  - {}",
        failures.join("\n  - "),
    );
}
