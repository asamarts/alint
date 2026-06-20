//! Doc-example schema gate (the missing drift gate behind §2 of the
//! external evaluation).
//!
//! Hand-written prose docs restate config syntax in fenced `yaml`
//! blocks, and nothing validated them — so `docs/design/ARCHITECTURE.md`
//! and `docs/rules.md` shipped examples the config loader rejects:
//! `query:`/`pattern:` instead of `path:`/`matches:`, a `sha256:` key
//! that isn't a schema field, map-keyed `rules:`, phantom facts
//! (`detect: linguist`), and rule examples using `max:`/`paths:` keys
//! the rule's own struct denies.
//!
//! This gate extracts every fenced `yaml` block from those docs, normalises
//! it into a loadable `.alint.yml`, and runs it through the SAME path
//! as `coverage_audit_examples_parse` (`alint_dsl::load` +
//! `registry.build`) — the exact place unknown-field / missing-field /
//! wrong-kind errors surface. A doc example that doesn't load fails the
//! build, so the whole §2 class can't silently reappear.
//!
//! Deliberately-invalid teaching snippets are skipped two ways:
//!   * a `**Wrong:**` / `Wrong:` marker in the few lines above the
//!     block (the convention `docs/development/CONFIG-AUTHORING.md`
//!     already uses for its "common mistakes" examples), or
//!   * an explicit `<!-- alint:ignore-example -->` HTML comment
//!     immediately before the fence (for one-off future/design shapes).
//!
//! Blocks that aren't configs (when-expressions, output samples, shell)
//! are skipped by classification. The test prints validated/skipped
//! counts so coverage stays visible.

use std::fmt::Write as _;
use std::path::PathBuf;

/// Docs whose fenced config examples must load. Scoped to the
/// drift-prone hand-written docs the evaluation flagged; extend as new
/// config-bearing prose is added.
const DOC_FILES: &[&str] = &[
    "docs/design/ARCHITECTURE.md",
    "docs/rules.md",
    "docs/development/CONFIG-AUTHORING.md",
    "docs/development/rule-authoring.md",
];

fn repo_root() -> PathBuf {
    // crates/alint-e2e -> repo root
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// One fenced `yaml` block plus enough context to decide intent.
struct Block {
    /// 1-based line number of the opening fence (for error messages).
    start_line: usize,
    body: String,
    /// The handful of source lines immediately above the fence.
    preamble: String,
}

/// Extract ```yaml / ```yml fenced blocks with their preceding context.
fn yaml_blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if trimmed == "```yaml" || trimmed == "```yml" {
            let start_line = i + 1;
            let preamble = lines[i.saturating_sub(6)..i].join("\n").to_lowercase();
            let mut body = String::new();
            i += 1;
            while i < lines.len() && lines[i].trim_start() != "```" {
                body.push_str(lines[i]);
                body.push('\n');
                i += 1;
            }
            blocks.push(Block {
                start_line,
                body,
                preamble,
            });
        }
        i += 1;
    }
    blocks
}

/// Is this block a deliberate teaching counter-example or a
/// future/design shape we shouldn't hold to the current schema?
fn is_deliberately_invalid(b: &Block) -> bool {
    const MARKERS: &[&str] = &[
        "wrong:",
        "alint:ignore-example",
        "design candidate",
        "designed shape",
        "not yet",
        "roadmap",
        "future",
        "won't load",
        "would fail",
        "invalid",
    ];
    MARKERS.iter().any(|m| b.preamble.contains(m))
}

/// Normalise a block into a full `.alint.yml` document, or `None` if it
/// isn't a config example (when-expression, output JSON, shell, ...).
fn to_config(body: &str) -> Option<String> {
    // A top-level config key in column 0 marks a (partial) config doc.
    const TOP_KEYS: &[&str] = &[
        "version:",
        "rules:",
        "extends:",
        "facts:",
        "vars:",
        "templates:",
        "nested_configs:",
        "ignore:",
        "respect_gitignore:",
        "fix_size_limit:",
        "allow_out_of_root:",
    ];
    let has_top_key = body
        .lines()
        .any(|l| TOP_KEYS.iter().any(|k| l.starts_with(k)));

    if has_top_key {
        // Already (part of) a config. Ensure a version line is present.
        if body.lines().any(|l| l.starts_with("version:")) {
            return Some(body.to_string());
        }
        return Some(format!("version: 1\n{body}"));
    }

    // A bare rules sequence (`- id: ... kind: ...`) — the docs/rules.md
    // shape. Wrap it under `rules:`.
    let first = body.lines().find(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
    })?;
    if first.trim_start().starts_with("- ") && body.contains("kind:") {
        let indented: String = body
            .lines()
            .map(|l| {
                if l.trim().is_empty() {
                    String::from("\n")
                } else {
                    format!("  {l}\n")
                }
            })
            .collect();
        return Some(format!("version: 1\nrules:\n{indented}"));
    }
    None
}

/// Inject a synthetic `level: error` into any rule that omits it.
///
/// `level` is required by both the schema and the loader, but the docs
/// deliberately omit it in fragments that illustrate one aspect of a
/// rule. That brevity is a *separate, benign* convention — not the
/// wrong-key / phantom-fact / wrong-kind drift class this gate exists
/// to catch — so we fill a default and let the gate focus on the rest
/// of the rule shape. A block whose YAML is itself malformed (e.g. the
/// map-vs-sequence `rules:` bug) won't parse here; we return it
/// unchanged so the loader still reports the structural error.
fn inject_default_levels(config: &str) -> String {
    let Ok(mut doc) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(config) else {
        return config.to_string();
    };
    if let Some(rules) = doc
        .get_mut("rules")
        .and_then(serde_yaml_ng::Value::as_sequence_mut)
    {
        for rule in rules.iter_mut() {
            if let Some(map) = rule.as_mapping_mut() {
                let key = serde_yaml_ng::Value::String("level".into());
                map.entry(key)
                    .or_insert_with(|| serde_yaml_ng::Value::String("error".into()));
            }
        }
    }
    serde_yaml_ng::to_string(&doc).unwrap_or_else(|_| config.to_string())
}

#[test]
fn every_doc_config_example_loads() {
    let root = repo_root();
    let registry = alint_rules::builtin_registry();
    let mut failures: Vec<String> = Vec::new();
    let mut validated = 0usize;
    let mut skipped_non_config = 0usize;
    let mut skipped_deliberate = 0usize;
    let mut skipped_network = 0usize;

    for rel in DOC_FILES {
        let path = root.join(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

        for block in yaml_blocks(&text) {
            if is_deliberately_invalid(&block) {
                skipped_deliberate += 1;
                continue;
            }
            let Some(config) = to_config(&block.body) else {
                skipped_non_config += 1;
                continue;
            };
            // `extends:` against an HTTPS URL would resolve over the
            // network; we can't validate those offline. Bundled
            // (`alint://`) extends load fine.
            if config.contains("http://") || config.contains("https://") {
                skipped_network += 1;
                continue;
            }

            let config = inject_default_levels(&config);
            let loc = format!("{rel}:{}", block.start_line);
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::write(dir.path().join(".alint.yml"), &config).unwrap();

            match alint_dsl::load(&dir.path().join(".alint.yml")) {
                Ok(cfg) => {
                    for spec in &cfg.rules {
                        if matches!(spec.level, alint_core::Level::Off) {
                            continue;
                        }
                        if let Err(e) = registry.build(spec) {
                            failures
                                .push(format!("{loc}: rule {:?} fails to build — {e}", spec.id));
                        }
                    }
                    validated += 1;
                }
                Err(e) => {
                    failures.push(format!("{loc}: config does not load — {e}"));
                }
            }
        }
    }

    eprintln!(
        "doc-example gate: {validated} validated, \
         {skipped_non_config} non-config, {skipped_deliberate} deliberate-invalid, \
         {skipped_network} network-extends skipped"
    );
    assert!(
        validated > 0,
        "no doc config examples were validated — did the extractor break?"
    );

    if !failures.is_empty() {
        let mut msg = format!(
            "{} doc config example(s) the loader rejects (schema drift).\n\
             Fix the example, or mark a deliberate counter-example with a \
             `**Wrong:**` preamble or `<!-- alint:ignore-example -->`.\n\n",
            failures.len()
        );
        for f in &failures {
            writeln!(msg, "  - {f}").unwrap();
        }
        panic!("{msg}");
    }
}
