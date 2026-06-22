//! Coverage-audit: the baseline-safety **collision-invariant** (design
//! `docs/design/baseline.md` §6; ADR-0006).
//!
//! Baseline mode fingerprints each violation as
//! `sha(rule_id ‖ path ‖ discriminator)`, where the discriminator is the
//! rule's `baseline_key`, else the offending line's content, else (path-only)
//! empty, else the message. A rule whose violations aren't uniquely+stably
//! identified by that scheme will *silently mis-suppress* a genuinely new
//! finding once a baseline is in effect. This audit runs every rule kind that
//! the firing scenario corpus exercises and asserts two structural invariants
//! on the **actual emitted violations** (so it can't drift out of sync with a
//! hand-maintained per-kind list):
//!
//!   (i)  **No masking collision.** Within one rule's findings, no two
//!        *distinct* findings may share a fingerprint. Two findings are
//!        distinct when they render different messages — UNLESS they are
//!        line-anchored on distinct lines with identical offending content
//!        (the design's intended byte-identical count-collapse, where the
//!        message differs only by line number). A path-only or same-line or
//!        keyed group with ≥2 distinct messages is a masking bug: the rule
//!        needs a finer `baseline_key`.
//!   (ii) **No message reliance.** No violation may fall through to the
//!        message anti-panic branch — every no-path, no-line violation must
//!        carry a `baseline_key` (else a reworded message re-baselines it).
//!
//! Targeted multi-finding fixtures under `fixtures/baseline_multi/` exercise
//! the shapes (several structured-query matches per file, a dependency cycle,
//! a duplicate-key group, several unresolved links on one line) that a
//! single-finding scenario wouldn't, so the gate can't false-green a kind
//! whose corpus fixture happens to emit only one finding.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use alint_core::baseline::fingerprint;
use alint_core::{Engine, RuleEntry, WalkOptions, walk};
use alint_rules::builtin_registry;
use alint_testkit::{Scenario, StepOutcome, run_scenario};

/// Kind-name aliases (same set the other coverage audits use): a scenario
/// using either spelling counts for the canonical kind.
const ALIASES: &[(&str, &str)] = &[
    ("content_matches", "file_content_matches"),
    ("content_forbidden", "file_content_forbidden"),
    ("header", "file_header"),
    ("footer", "file_footer"),
    ("shebang", "file_shebang"),
    ("max_size", "file_max_size"),
    ("min_size", "file_min_size"),
    ("min_lines", "file_min_lines"),
    ("max_lines", "file_max_lines"),
    ("is_text", "file_is_text"),
];

fn canonical(kind: &str) -> String {
    ALIASES
        .iter()
        .find(|(a, _)| *a == kind)
        .map_or(kind, |(_, c)| *c)
        .to_string()
}

/// One emitted violation, reduced to what the invariants need.
struct Finding {
    kind: String,
    fp: String,
    message: String,
    line: Option<usize>,
    /// True when the violation would fall to the message anti-panic branch
    /// (no path, no line, no `baseline_key`).
    message_reliant: bool,
}

/// Parse a scenario's `given.config:` YAML into an id → kind map so a
/// `RuleResult`'s `rule_id` can be resolved back to its kind. Falls back to
/// the `rule_id` itself for rules pulled in via `extends:` (ruleset instances).
fn id_to_kind(config: &str) -> BTreeMap<String, String> {
    fn walk(v: &serde_yaml_ng::Value, out: &mut BTreeMap<String, String>) {
        match v {
            serde_yaml_ng::Value::Mapping(m) => {
                if let (Some(id), Some(kind)) = (
                    m.get("id").and_then(serde_yaml_ng::Value::as_str),
                    m.get("kind").and_then(serde_yaml_ng::Value::as_str),
                ) {
                    out.insert(id.to_string(), kind.to_string());
                }
                for (_, c) in m {
                    walk(c, out);
                }
            }
            serde_yaml_ng::Value::Sequence(s) => s.iter().for_each(|c| walk(c, out)),
            _ => {}
        }
    }
    let mut out = BTreeMap::new();
    let Ok(v) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(config) else {
        return out;
    };
    walk(&v, &mut out);
    out
}

fn yml_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("yml") {
                out.push(p);
            }
        }
    }
    out
}

/// Reduce one `Report` to its non-note findings, resolving each kind and
/// computing the real fingerprint (reading offending files for the
/// line-content discriminator). `root` is the materialised tree.
fn findings_of(
    report: &alint_core::Report,
    id_kind: &BTreeMap<String, String>,
    root: &Path,
    out: &mut Vec<Finding>,
) {
    let mut cache: BTreeMap<PathBuf, Option<Vec<u8>>> = BTreeMap::new();
    for r in &report.results {
        let kind = id_kind
            .get(r.rule_id.as_ref())
            .map_or_else(|| r.rule_id.to_string(), |k| canonical(k));
        for v in &r.violations {
            let bytes = v.path.as_ref().and_then(|p| {
                cache
                    .entry(p.to_path_buf())
                    .or_insert_with(|| std::fs::read(root.join(p)).ok())
                    .clone()
            });
            out.push(Finding {
                kind: kind.clone(),
                fp: fingerprint(r.rule_id.as_ref(), v, bytes.as_deref()),
                message: v.message.to_string(),
                line: v.line,
                message_reliant: v.path.is_none() && v.line.is_none() && v.baseline_key.is_none(),
            });
        }
    }
}

/// Run every scenario under `dir` and collect findings + which kinds fired.
fn collect_from_scenarios(dir: &Path, all: &mut Vec<Vec<Finding>>, fired: &mut BTreeSet<String>) {
    for path in yml_files(dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(scenario) = serde_yaml_ng::from_str::<Scenario>(&text) else {
            continue;
        };
        let id_kind = id_to_kind(&scenario.given.config);
        let Ok(run) = run_scenario(&scenario) else {
            continue;
        };
        for step in &run.steps {
            let StepOutcome::Check(report) = step else {
                continue;
            };
            let mut findings = Vec::new();
            findings_of(report, &id_kind, &run.root, &mut findings);
            for f in &findings {
                fired.insert(f.kind.clone());
            }
            all.push(findings);
        }
    }
}

/// Run the dedicated multi-finding fixtures (`alint.yml` + `tree/`) — the
/// shapes a single-finding corpus scenario can't express.
fn collect_from_multi_fixtures(
    dir: &Path,
    all: &mut Vec<Vec<Finding>>,
    fired: &mut BTreeSet<String>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let scenario = e.path();
        let cfg = scenario.join("alint.yml");
        let tree = scenario.join("tree");
        if !cfg.is_file() || !tree.is_dir() {
            continue;
        }
        let registry = builtin_registry();
        let config =
            alint_dsl::load(&cfg).unwrap_or_else(|err| panic!("load {}: {err}", cfg.display()));
        let id_kind: BTreeMap<String, String> = config
            .rules
            .iter()
            .map(|s| (s.id.clone(), canonical(&s.kind)))
            .collect();
        let mut entries = Vec::new();
        for spec in &config.rules {
            if matches!(spec.level, alint_core::Level::Off) {
                continue;
            }
            let rule = registry
                .build(spec)
                .unwrap_or_else(|err| panic!("build {} in {}: {err}", spec.id, cfg.display()));
            entries.push(RuleEntry::new(rule));
        }
        let engine = Engine::from_entries(entries, registry);
        let index = walk(&tree, &WalkOptions::default()).expect("walk multi fixture");
        let report = engine.run(&tree, &index).expect("run multi fixture");
        let mut findings = Vec::new();
        findings_of(&report, &id_kind, &tree, &mut findings);
        assert!(
            findings.len() >= 2,
            "multi-finding fixture {} produced {} finding(s); it must emit \
             ≥2 on one (rule, path) to exercise the collision-invariant — check its config",
            scenario.display(),
            findings.len(),
        );
        for f in &findings {
            fired.insert(f.kind.clone());
        }
        all.push(findings);
    }
}

#[test]
fn every_kind_emits_baseline_safe_fingerprints() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut runs: Vec<Vec<Finding>> = Vec::new();
    let mut fired: BTreeSet<String> = BTreeSet::new();

    collect_from_scenarios(&manifest.join("scenarios"), &mut runs, &mut fired);
    collect_from_multi_fixtures(
        &manifest.join("fixtures/baseline_multi"),
        &mut runs,
        &mut fired,
    );

    let mut collisions: Vec<String> = Vec::new();
    let mut message_reliant: BTreeSet<String> = BTreeSet::new();

    for findings in &runs {
        // (ii) message-reliance: any no-path/no-line/no-key violation.
        for f in findings {
            if f.message_reliant {
                message_reliant.insert(format!("{} — {:?}", f.kind, truncate(&f.message)));
            }
        }
        // (i) masking collision: group this run's findings by fingerprint.
        let mut by_fp: BTreeMap<&str, Vec<&Finding>> = BTreeMap::new();
        for f in findings {
            by_fp.entry(f.fp.as_str()).or_default().push(f);
        }
        for group in by_fp.values() {
            let msgs: BTreeSet<&str> = group.iter().map(|f| f.message.as_str()).collect();
            if msgs.len() < 2 {
                continue; // identical messages → legitimate count-collapse
            }
            // Allowed: line-anchored findings on DISTINCT lines with identical
            // offending content (same fingerprint ⇒ same content) — the message
            // differs only by line number. Anything else masks a finding.
            let all_lined = group.iter().all(|f| f.line.is_some());
            let lines: BTreeSet<usize> = group.iter().filter_map(|f| f.line).collect();
            let distinct_lines = lines.len() == group.len();
            if all_lined && distinct_lines {
                continue;
            }
            let kind = &group[0].kind;
            let sample: Vec<String> = msgs.iter().take(3).map(|m| truncate(m)).collect();
            collisions.push(format!(
                "  {kind}: {} distinct findings share one fingerprint (needs a finer \
                 baseline_key):\n      - {}",
                msgs.len(),
                sample.join("\n      - "),
            ));
        }
    }

    // Sanity: the corpus must actually exercise a broad set of kinds, else a
    // broken testkit would vacuously pass this gate.
    let canonical_kinds = builtin_registry().known_kinds().count();
    assert!(
        fired.len() >= 60,
        "baseline audit only saw {} kinds fire (of {canonical_kinds}); the scenario \
         corpus looks broken — the invariants weren't meaningfully exercised",
        fired.len(),
    );

    if collisions.is_empty() && message_reliant.is_empty() {
        return;
    }

    let mut report = String::from("\nbaseline-safety invariants violated:\n");
    if !collisions.is_empty() {
        let _ = writeln!(report, "\n(i) masking collisions ({}):", collisions.len());
        collisions.sort();
        collisions.dedup();
        for c in &collisions {
            let _ = writeln!(report, "{c}");
        }
    }
    if !message_reliant.is_empty() {
        let _ = writeln!(
            report,
            "\n(ii) violations relying on the message anti-panic branch \
             (set a baseline_key) ({}):",
            message_reliant.len(),
        );
        for m in &message_reliant {
            let _ = writeln!(report, "  - {m}");
        }
    }
    panic!("{report}");
}

fn truncate(s: &str) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.chars().count() <= 72 {
        one_line
    } else {
        format!("{}…", one_line.chars().take(72).collect::<String>())
    }
}
