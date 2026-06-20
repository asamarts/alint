//! Status-drift gate: a ruleset documented as "Planned" must not have
//! shipped.
//!
//! `docs/rules.md` carried a "Planned rulesets (v0.5)" section listing
//! `python` / `go` / `java` / `compliance/*` as not-yet-shipped long
//! after all five shipped — the same status-drift class as the public
//! roadmap mislabelling WASM as the latest release. This gate ties the
//! claim to the surface area: any `alint://bundled/<name>@v1` that
//! appears under a heading containing "planned" must NOT be present in
//! `facts.json`'s `bundled_rulesets` (the shipped catalogue). If it is,
//! it shipped and the "Planned" copy is stale.

use std::collections::BTreeSet;
use std::path::PathBuf;

const DOC_FILES: &[&str] = &["docs/rules.md", "docs/design/ARCHITECTURE.md"];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// Shipped ruleset names, e.g. `go`, `compliance/reuse`.
fn shipped_rulesets() -> BTreeSet<String> {
    let text = std::fs::read_to_string(repo_root().join("facts.json")).expect("read facts.json");
    let facts: serde_json::Value = serde_json::from_str(&text).expect("parse facts.json");
    facts["bundled_rulesets"]
        .as_array()
        .expect("bundled_rulesets array")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Pull the `<name>` out of `alint://bundled/<name>@v1`.
fn ruleset_name(line: &str) -> Option<String> {
    let start = line.find("alint://bundled/")? + "alint://bundled/".len();
    let rest = &line[start..];
    let end = rest.find("@v")?;
    Some(rest[..end].to_string())
}

#[test]
fn no_planned_section_lists_a_shipped_ruleset() {
    let shipped = shipped_rulesets();
    let mut stale: Vec<String> = Vec::new();

    for rel in DOC_FILES {
        let text = std::fs::read_to_string(repo_root().join(rel))
            .unwrap_or_else(|e| panic!("read {rel}: {e}"));

        // Track whether we're inside a heading section whose title
        // mentions "planned"; reset at the next heading of the same or
        // shallower depth (simplest: any new heading line).
        let mut in_planned = false;
        for (i, line) in text.lines().enumerate() {
            if line.starts_with('#') {
                in_planned = line.to_lowercase().contains("planned");
                continue;
            }
            if in_planned
                && let Some(name) = ruleset_name(line)
                && shipped.contains(&name)
            {
                stale.push(format!(
                    "{rel}:{}: `alint://bundled/{name}@v1` is under a \
                     \"Planned\" heading but is in facts.json \
                     (it shipped) — update the status copy.",
                    i + 1
                ));
            }
        }
    }

    assert!(
        stale.is_empty(),
        "stale 'Planned' ruleset claim(s):\n{}",
        stale.join("\n")
    );
}
