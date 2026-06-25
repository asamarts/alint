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

use std::collections::BTreeSet;
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

/// Whether a trimmed line opens a yaml fence — the bare ```` ```yaml ```` /
/// ```` ```yml ```` OR a decorated fence with an info string
/// (```` ```yaml title="x" ````). Matching only the bare form silently
/// dropped decorated fences from the gate.
fn is_yaml_open_fence(trimmed: &str) -> bool {
    for tag in ["```yaml", "```yml"] {
        if let Some(rest) = trimmed.strip_prefix(tag) {
            if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                return true;
            }
        }
    }
    false
}

/// Extract ```yaml / ```yml fenced blocks with their preceding context.
fn yaml_blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim_start();
        if is_yaml_open_fence(trimmed) {
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
    // ONLY explicit, unambiguous opt-outs — the two conventions the module
    // doc declares. Soft prose words (`future`, `roadmap`, `invalid`, …) used
    // to live here, but they matched any block whose preamble merely mentioned
    // such a word, silently exempting valid examples from the gate. A
    // deliberate counter-example must now be marked, not merely adjacent to a
    // suggestive sentence.
    const MARKERS: &[&str] = &["wrong:", "alint:ignore-example"];
    MARKERS.iter().any(|m| b.preamble.contains(m))
}

/// Top-level config keys (column 0) that mark a block as a (partial) config.
/// MUST track every top-level field the loader accepts — a missing key makes a
/// doc block using only that key invisible to the gate; the
/// `top_keys_track_the_loader` test enforces this against the loader's own
/// `deny_unknown_fields` field list.
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
    "baseline:",
];

/// Normalise a block into a full `.alint.yml` document, or `None` if it
/// isn't a config example (when-expression, output JSON, shell, ...).
fn to_config(body: &str) -> Option<String> {
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

/// Whether the config's `extends:` references a network (http/https) URL,
/// which `alint_dsl::load` would resolve over the wire. ONLY those blocks are
/// skipped — the previous `config.contains("https://")` matched the whole
/// block, so any example merely mentioning an `https://` `policy_url:` (or an
/// SRI-pinned `extends` — the exact shape this gate was added to validate) was
/// silently dropped. A config that doesn't parse here returns `false` so the
/// loader still runs and reports the structural error.
fn has_network_extends(config: &str) -> bool {
    let Ok(doc) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(config) else {
        return false;
    };
    let entry_is_network = |e: &serde_yaml_ng::Value| {
        let url = match e {
            serde_yaml_ng::Value::String(s) => Some(s.as_str()),
            serde_yaml_ng::Value::Mapping(m) => m
                .get(serde_yaml_ng::Value::String("url".into()))
                .and_then(serde_yaml_ng::Value::as_str),
            _ => None,
        };
        url.is_some_and(|u| u.starts_with("http://") || u.starts_with("https://"))
    };
    match doc.get("extends") {
        Some(serde_yaml_ng::Value::Sequence(seq)) => seq.iter().any(entry_is_network),
        Some(other) => entry_is_network(other),
        None => false,
    }
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

    let mut per_file_blocks: Vec<(&str, usize)> = Vec::new();

    for rel in DOC_FILES {
        let path = root.join(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

        let blocks = yaml_blocks(&text);
        per_file_blocks.push((rel, blocks.len()));

        for block in blocks {
            if is_deliberately_invalid(&block) {
                skipped_deliberate += 1;
                continue;
            }
            let Some(config) = to_config(&block.body) else {
                skipped_non_config += 1;
                continue;
            };
            // Skip ONLY blocks whose `extends:` resolves over the network
            // (see `has_network_extends`). Bundled (`alint://`) extends — and
            // any block that merely mentions an `https://` `policy_url:` — load
            // fine and stay in the gate.
            if has_network_extends(&config) {
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
    // A broken extractor (a fence-syntax change, a bad classifier) would
    // silently drop blocks toward zero while a single survivor kept
    // `validated > 0` green. Require every governed doc to still yield blocks,
    // plus a meaningful global floor.
    for (rel, n) in &per_file_blocks {
        assert!(
            *n > 0,
            "no fenced yaml blocks extracted from {rel} — the fence extractor likely broke"
        );
    }
    assert!(
        validated >= 20,
        "only {validated} doc config example(s) validated (far below the usual count) \
         — the extractor or classifier likely regressed"
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

/// `TOP_KEYS` (the config-block classifier) must list every top-level field
/// the loader accepts, or a doc example using only a newly-added key (as
/// `baseline:` was this cycle) is silently classified non-config and escapes
/// the gate. Derive the authoritative set from the loader's own
/// `deny_unknown_fields` error so this can't drift unnoticed.
#[test]
fn top_keys_track_the_loader() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join(".alint.yml");
    std::fs::write(&cfg, "version: 1\n__definitely_not_a_key__: 1\n").unwrap();
    let err = alint_dsl::load(&cfg)
        .expect_err("an unknown top-level field must be rejected")
        .to_string();
    // "... expected one of `version`, `extends`, `ignore`, ..." → the
    // backtick-quoted names after "expected one of" are the valid fields.
    let tail = err
        .split_once("expected one of")
        .map_or(err.as_str(), |(_, rest)| rest);
    let loader_fields: Vec<String> = tail
        .split('`')
        .skip(1)
        .step_by(2)
        .map(|name| format!("{name}:"))
        .collect();
    assert!(
        loader_fields.len() >= 10,
        "could not parse the loader's field list (got {loader_fields:?}) from: {err}"
    );
    let missing: Vec<&String> = loader_fields
        .iter()
        .filter(|f| !TOP_KEYS.contains(&f.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "TOP_KEYS is missing loader field(s) {missing:?} — a doc example using \
         only one of these would be silently skipped by the gate; add them to TOP_KEYS",
    );
}

/// CLASS GUARD: every rule kind must REJECT an unknown option, not silently
/// swallow it. Reuses the same one-valid-example-per-kind doc corpus as
/// `every_doc_config_example_loads`: for each rule that builds cleanly, inject
/// a bogus option into its `extra` and assert the rebuild now FAILS. Catches
/// the whole class — a builder that `.unwrap_or(default)`s its option-parse, or
/// that (being option-less) never validates `extra` at all, lets a typo'd
/// option through as a silent no-op, defeating `validate-config` and the
/// "schema is the contract" guarantee.
#[test]
fn every_rule_kind_rejects_an_unknown_option() {
    // KNOWN exceptions: the existence family accepts an unknown option today
    // because it also accepts `root_only` (used in bundled rulesets + docs) but
    // never implemented it as a validated `Options` struct — it silently ignores
    // it. Strict option validation for them depends on first implementing
    // `root_only` parity with `file_exists` (a feature, tracked separately). They
    // are allow-listed here; a NEW swallower beyond this set still fails the test.
    const KNOWN_OPTION_SWALLOWERS: &[&str] = &["dir_absent", "dir_exists", "file_absent"];

    let root = repo_root();
    let registry = alint_rules::builtin_registry();
    let mut swallowers: BTreeSet<String> = BTreeSet::new();
    let mut probed: BTreeSet<String> = BTreeSet::new();

    for rel in DOC_FILES {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for block in yaml_blocks(&text) {
            if is_deliberately_invalid(&block) {
                continue;
            }
            let Some(config) = to_config(&block.body) else {
                continue;
            };
            if has_network_extends(&config) {
                continue;
            }
            let config = inject_default_levels(&config);
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::write(dir.path().join(".alint.yml"), &config).unwrap();
            let Ok(cfg) = alint_dsl::load(&dir.path().join(".alint.yml")) else {
                continue;
            };
            for spec in &cfg.rules {
                if matches!(spec.level, alint_core::Level::Off) {
                    continue;
                }
                // Only probe rules that build cleanly, so a failure under the
                // probe is attributable to the injected option (not e.g. a
                // missing required field in a terse doc fragment).
                if registry.build(spec).is_err() {
                    continue;
                }
                let mut bogus = spec.clone();
                bogus.extra.insert(
                    serde_yaml_ng::Value::String("__alint_unknown_option_probe__".into()),
                    serde_yaml_ng::Value::Bool(true),
                );
                probed.insert(spec.kind.clone());
                if registry.build(&bogus).is_ok() {
                    swallowers.insert(spec.kind.clone());
                }
            }
        }
    }

    assert!(
        probed.len() >= 30,
        "only probed {} kind(s) for unknown-option rejection — the doc corpus or \
         extractor likely regressed",
        probed.len(),
    );

    let unexpected: Vec<&String> = swallowers
        .iter()
        .filter(|k| !KNOWN_OPTION_SWALLOWERS.contains(&k.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "{} rule kind(s) SILENTLY ACCEPT an unknown option (every other kind rejects \
         it): {unexpected:?}\nEach must validate its options — propagate \
         `deserialize_options()` instead of `.unwrap_or(default)`, or register via \
         `register_optionless` if it takes none.",
        unexpected.len(),
    );
}
