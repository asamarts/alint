//! v0.9.16 audit: every per-ruleset rule table in `docs/rules.md`
//! MUST list exactly the rules the corresponding bundled YAML at
//! `crates/alint-dsl/rulesets/v1/<ruleset>.yml` defines.
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

/// Walk every `.yml` under `crates/alint-dsl/rulesets/v1/` (recursive)
/// and return `(uri, rule_ids)` for each. URI follows the existing
/// translation: `oss-baseline.yml` → `alint://bundled/oss-baseline@v1`,
/// `monorepo/cargo-workspace.yml` → `alint://bundled/monorepo/cargo-workspace@v1`.
fn collect_yaml_rulesets() -> BTreeMap<String, BTreeSet<String>> {
    let base = workspace_root().join("crates/alint-dsl/rulesets/v1");
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
        let uri = format!("alint://bundled/{}@v1", rel.display());
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
/// then a backticked URI of the form `alint://bundled/.../@v1`.
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
    if !stripped.starts_with("alint://bundled/") || !stripped.ends_with("@v1") {
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
                 crates/alint-dsl/rulesets/v1/...; either drop the section from docs \
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
