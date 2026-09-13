//! Proptest strategies for generating [`Scenario`]-shaped inputs.
//!
//! The strategies here are intentionally small and focused — just
//! enough variety to shake invariants (panic-freedom, dry-run
//! purity, fix idempotence, fix→check convergence) without blowing
//! up the state space. Tune via the [`ScenarioTreeParams`] knobs.
//!
//! Three factories are provided:
//!
//! - [`any_scenario_tree`] — tree + config drawn from a broad
//!   catalogue. Covers non-fixable rule kinds too; use for
//!   invariants that only require alint to not panic.
//! - [`fixable_scenario_tree`] — multi-rule; restricts the rule
//!   catalogue to four whole-file-ish fixers (`file_create`,
//!   `file_remove`, `file_rename`, `file_append`) whose combination
//!   stays well-behaved for dry-run purity. Use for invariants that
//!   exercise `alint fix` with several rules at once.
//! - [`single_fixable_scenario_tree`] — EXACTLY ONE rule, drawn from
//!   the full fixer catalogue (adds the content-hygiene, strip, and
//!   located `replace` fixers). Use for the idempotence / convergence invariants:
//!   Phase-0 guarantees a per-fixer fixed point, not multi-rule
//!   single-pass convergence, so these are asserted one rule at a time.

use std::collections::BTreeMap;

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::string::string_regex;

use crate::scenario::{Given, Scenario, Step};
use crate::treespec::{TreeNode, TreeSpec};

#[derive(Debug, Clone)]
pub struct ScenarioTreeParams {
    /// Max number of files in the tree. The generator draws 0..=max.
    pub max_files: usize,
    /// Max depth of directories. 0 = files at the root only.
    pub max_depth: usize,
    /// Max number of rules per scenario. Drawn 0..=max.
    pub max_rules: usize,
}

impl Default for ScenarioTreeParams {
    fn default() -> Self {
        Self {
            max_files: 8,
            max_depth: 2,
            max_rules: 3,
        }
    }
}

/// Any combination of tree + config. Rules span both fixable and
/// non-fixable kinds; use for invariants like panic-freedom.
pub fn any_scenario_tree() -> impl Strategy<Value = Scenario> {
    any_scenario_tree_with(ScenarioTreeParams::default())
}

// Params is copied into the closures that build each sub-strategy;
// taking by value keeps `impl Trait` from capturing a caller lifetime.
#[allow(clippy::needless_pass_by_value)]
pub fn any_scenario_tree_with(params: ScenarioTreeParams) -> impl Strategy<Value = Scenario> {
    let tree_strategy = any_tree(params.max_files, params.max_depth);
    let rules_strategy = any_rules(params.max_rules);
    (tree_strategy, rules_strategy).prop_map(|(tree, rules_yaml)| {
        let config = compose_config(&rules_yaml);
        Scenario {
            name: "property-any".into(),
            tags: vec!["proptest".into()],
            given: Given {
                tree,
                config,
                git: None,
            },
            when: vec![],
            expect: vec![],
            expect_tree: None,
            expect_tree_mode: crate::scenario::ExpectTreeMode::default(),
            docs: None,
        }
    })
}

/// Restricted variant: every emitted rule has a declared fixer, so
/// `fix` reports can be asserted against without `Unfixable` noise.
pub fn fixable_scenario_tree() -> impl Strategy<Value = Scenario> {
    fixable_scenario_tree_with(ScenarioTreeParams::default())
}

#[allow(clippy::needless_pass_by_value)]
pub fn fixable_scenario_tree_with(params: ScenarioTreeParams) -> impl Strategy<Value = Scenario> {
    let tree_strategy = any_tree(params.max_files, params.max_depth);
    let rules_strategy = fixable_rules(params.max_rules);
    (tree_strategy, rules_strategy).prop_map(|(tree, rules_yaml)| {
        let config = compose_config(&rules_yaml);
        Scenario {
            name: "property-fixable".into(),
            tags: vec!["proptest".into()],
            given: Given {
                tree,
                config,
                git: None,
            },
            when: vec![],
            expect: vec![],
            expect_tree: None,
            expect_tree_mode: crate::scenario::ExpectTreeMode::default(),
            docs: None,
        }
    })
}

/// A scenario with EXACTLY ONE fixable rule, drawn from the full
/// catalogue (including the content-hygiene, strip, and located `replace` fixers the multi-rule
/// [`fixable_scenario_tree`] omits). Single-rule by design: with one rule there
/// is no cross-rule, single-pass ordering interaction, so the fix→check
/// convergence law ("a fully-applied fix leaves the check clean") holds per
/// fixer and can be asserted without false positives from Phase-0's known
/// order-dependence limitation. This is the strategy that makes the convergence
/// invariant non-vacuous and actually exercises every fixer.
pub fn single_fixable_scenario_tree() -> impl Strategy<Value = Scenario> {
    let params = ScenarioTreeParams::default();
    let tree_strategy = any_tree(params.max_files, params.max_depth);
    (tree_strategy, one_fixable_rule_yaml()).prop_map(|(mut tree, rule_yaml)| {
        // Plant guaranteed triggers so the drawn fixer is ACTUALLY exercised.
        // Drawing the rule (~1/12) and the file content independently otherwise
        // triggers a given fixer only a fraction of the time -- the invariants
        // missed an injected non-idempotence bug ~40% of the time at 48 cases.
        // With a matching trigger present, every case exercises its fixer, so
        // the property is a reliable standalone gate at the default case count.
        plant_fixable_triggers(&mut tree.root);
        let config = compose_config(std::slice::from_ref(&rule_yaml));
        Scenario {
            name: "property-single-fixable".into(),
            tags: vec!["proptest".into()],
            given: Given {
                tree,
                config,
                git: None,
            },
            when: vec![],
            expect: vec![],
            expect_tree: None,
            expect_tree_mode: crate::scenario::ExpectTreeMode::default(),
            docs: None,
        }
    })
}

/// Attach `steps` to a scenario produced by one of the strategies.
/// The `expect:` list is left empty — invariant tests inspect the
/// [`crate::ScenarioRun`] directly rather than scripted assertions.
pub fn with_steps(mut s: Scenario, steps: Vec<Step>) -> Scenario {
    s.when = steps;
    s
}

// ─── tree generation ─────────────────────────────────────────────

fn any_tree(max_files: usize, max_depth: usize) -> impl Strategy<Value = TreeSpec> {
    vec(any_file_entry(max_depth), 0..=max_files).prop_map(|entries| {
        let mut root: BTreeMap<String, TreeNode> = BTreeMap::new();
        for (segments, content) in entries {
            insert_file(&mut root, &segments, content);
        }
        TreeSpec { root }
    })
}

/// (path-components, file-content) — components has length >= 1.
fn any_file_entry(max_depth: usize) -> impl Strategy<Value = (Vec<String>, String)> {
    // 1 to max_depth+1 components; last is a filename, rest are dirs.
    let depth = 1_usize..=(max_depth + 1);
    (
        depth.prop_flat_map(|d| {
            let leaf = filename_component();
            let dirs = vec(dirname_component(), d.saturating_sub(1));
            (dirs, leaf).prop_map(|(mut ds, leaf)| {
                ds.push(leaf);
                ds
            })
        }),
        content_blob(),
    )
}

fn filename_component() -> impl Strategy<Value = String> {
    prop_oneof![
        // snake_case stems with a common extension
        (
            string_regex(r"[a-z][a-z0-9_]{0,6}").unwrap(),
            select(&[".rs", ".md", ".toml", ".txt", ".json"][..]),
        )
            .prop_map(|(stem, ext)| format!("{stem}{ext}")),
        // PascalCase stems (will fail snake-case checks)
        (
            string_regex(r"[A-Z][a-zA-Z0-9]{0,6}").unwrap(),
            select(&[".rs", ".tsx"][..]),
        )
            .prop_map(|(stem, ext)| format!("{stem}{ext}")),
        // Well-known filenames
        select(&["README.md", "Cargo.toml", "LICENSE", "package.json"][..])
            .prop_map(str::to_string),
        // Backup-ish names (exercises file_absent)
        (
            string_regex(r"[a-z]{1,4}").unwrap(),
            select(&[".bak", ".swp"][..]),
        )
            .prop_map(|(stem, ext)| format!("{stem}{ext}")),
    ]
}

fn dirname_component() -> impl Strategy<Value = String> {
    select(&["src", "tests", "docs", "scripts", "a", "b", "pkg"][..]).prop_map(str::to_string)
}

fn content_blob() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just("hello\n".to_string()),
        Just("// Copyright 2026\n".to_string()),
        // Fixtures that actually *trigger* the content-hygiene / strip fixers.
        // Without these (and the `\t`/`\r` in the regex arm below) the generated
        // corpus never exercised trim / normalize / collapse / append-newline /
        // strip-bom / strip-bidi / strip-zero-width -- the fixers where the
        // round-3 non-convergence bugs lived -- so the fix invariants were
        // vacuous for them.
        Just("trailing   \n".to_string()), // no_trailing_whitespace
        Just("tab\tmid\tend \n".to_string()), // tabs + trailing space
        Just("crlf\r\nline\r\n".to_string()), // line_endings (lf target)
        Just("lf\nonly\n".to_string()),    // line_endings (crlf target)
        Just("dbl\r\r\ncr\r\n".to_string()), // doubled CR (round-3 F1)
        Just("a\n\n\n\n\nb\n".to_string()), // max_consecutive_blank_lines
        Just("no final newline".to_string()), // final_newline
        Just("\u{FEFF}bom\n".to_string()), // no_bom
        Just("\u{FEFF}\u{FEFF}stacked bom\n".to_string()), // stacked BOM (round-3 F3)
        Just("bidi\u{202E}rtl\u{202C}\n".to_string()), // no_bidi_controls
        Just("zero\u{200B}width\u{200D}\n".to_string()), // no_zero_width_chars
        // NUL-bearing "binary" (U+0000 is valid UTF-8, so it fits a String): the
        // content detectors must SKIP it and the fixers must not touch it, so the
        // invariants also cover the binary path.
        Just("data\u{0}here  \r\n\n\n\n".to_string()),
        // Widen the random arm to include tab and CR (was `[a-zA-Z0-9 \n]`).
        string_regex(r"[a-zA-Z0-9 \t\r\n]{0,40}").unwrap(),
    ]
}

/// Plant files under `_trig/` that trigger every fixer in the single-rule
/// catalogue, so whichever rule is drawn has a matching violation to fix. The
/// random dir names never include `_trig`, so these don't collide. Only ONE rule
/// is active per single-rule scenario, so the several flaws in the kitchen-sink
/// file never interact.
fn plant_fixable_triggers(root: &mut BTreeMap<String, TreeNode>) {
    let dir = "_trig".to_string();
    // Kitchen-sink `.txt`: a leading BOM, a bidi override, a zero-width char,
    // trailing whitespace, a CRLF, a >max blank run, and no final newline. Its
    // `.txt` name matches every content/strip rule's glob
    // (`**/*.{md,rs,txt,toml,json,tsx}`) plus `file_header`'s, so it triggers
    // trim / normalize / collapse / final_newline / strip_bom / strip_bidi /
    // strip_zero_width / file_prepend.
    insert_file(
        root,
        &[dir.clone(), "sink.txt".to_string()],
        "\u{FEFF}a\u{202E}\u{200B}  \r\n\n\n\n\nb".to_string(),
    );
    // PascalCase files with no header pattern: trigger filename_case (rename) and
    // file_content_matches (append, whose globs are `**/*.rs` / `**/*.md`).
    insert_file(
        root,
        &[dir.clone(), "Pascal.rs".to_string()],
        "fn x() {}\n".to_string(),
    );
    insert_file(
        root,
        &[dir.clone(), "Pascal.md".to_string()],
        "# doc\n".to_string(),
    );
    // A file containing the forbidden `DEBUGME` token triggers
    // file_content_forbidden + the located `replace` fix (rewrites DEBUGME ->
    // LOGGED). `.txt` keeps it out of filename_case's `.rs`/`.md` scope, and the
    // content is otherwise clean (final newline, no trailing ws / BOM / bidi), so
    // no other single-rule draw touches it. The token appears nowhere else.
    insert_file(
        root,
        &[dir.clone(), "replaceme.txt".to_string()],
        "DEBUGME token\n".to_string(),
    );
    // A backup file triggers file_absent (remove).
    insert_file(root, &[dir, "junk.bak".to_string()], "junk\n".to_string());
    // file_create is triggered by the ABSENCE of REQUIRED.md / CONFIG.toml /
    // NOTES.txt, which `_trig/` does not contain -- no plant needed.
}

fn insert_file(root: &mut BTreeMap<String, TreeNode>, segments: &[String], content: String) {
    match segments {
        [] => {}
        [leaf] => {
            // If a directory already occupies this name, skip; prop
            // tests mustn't panic on collision.
            root.entry(leaf.clone()).or_insert(TreeNode::File(content));
        }
        [head, rest @ ..] => {
            let entry = root
                .entry(head.clone())
                .or_insert_with(|| TreeNode::Dir(BTreeMap::new()));
            if let TreeNode::Dir(children) = entry {
                insert_file(children, rest, content);
            }
        }
    }
}

// ─── rule generation ─────────────────────────────────────────────

fn any_rules(max_rules: usize) -> impl Strategy<Value = Vec<String>> {
    vec(any_rule_yaml(), 0..=max_rules).prop_map(dedupe_ids)
}

fn fixable_rules(max_rules: usize) -> impl Strategy<Value = Vec<String>> {
    vec(fixable_rule_yaml(), 0..=max_rules).prop_map(dedupe_ids)
}

/// The per-kind `rule_id` generators draw a prefix + 1-4 lowercase
/// letters from a small space (`fe-a`, `fa-p`, …), so two rules of
/// the same kind can land on the same id even at small `max_rules`.
/// alint-dsl rejects duplicate ids at parse time, which makes the
/// proptest harness panic on `parse(...).unwrap()`.
///
/// Prepend each rule's id with an index marker so every emitted
/// scenario is collision-free regardless of how unlucky the
/// underlying letter-draw was. We splice into the leading
/// `  - id: ` prefix once per entry, leaving every other field
/// untouched.
fn dedupe_ids(rules: Vec<String>) -> Vec<String> {
    rules
        .into_iter()
        .enumerate()
        .map(|(i, r)| r.replacen("  - id: ", &format!("  - id: i{i}-"), 1))
        .collect()
}

fn any_rule_yaml() -> impl Strategy<Value = String> {
    prop_oneof![
        rule_file_exists(false),
        rule_file_absent(false),
        rule_filename_case(false),
        rule_file_content_matches(false),
        rule_file_content_forbidden(),
    ]
}

fn fixable_rule_yaml() -> impl Strategy<Value = String> {
    prop_oneof![
        rule_file_exists(true),
        rule_file_absent(true),
        rule_filename_case(true),
        rule_file_content_matches(true),
    ]
}

fn rule_id(prefix: &'static str) -> impl Strategy<Value = String> {
    string_regex(r"[a-z]{1,4}")
        .unwrap()
        .prop_map(move |s| format!("{prefix}-{s}"))
}

fn rule_file_exists(fix: bool) -> impl Strategy<Value = String> {
    let target = select(&["REQUIRED.md", "CONFIG.toml", "NOTES.txt"][..]);
    (rule_id("fe"), target).prop_map(move |(id, target)| {
        let mut yaml = format!(
            "  - id: {id}\n    kind: file_exists\n    paths: {target}\n    level: warning\n"
        );
        if fix {
            yaml.push_str("    fix:\n      file_create:\n        content: \"placeholder\\n\"\n");
        }
        yaml
    })
}

fn rule_file_absent(fix: bool) -> impl Strategy<Value = String> {
    let glob = select(&["**/*.bak", "**/*.swp", "**/forbidden.*"][..]);
    (rule_id("fa"), glob).prop_map(move |(id, glob)| {
        let mut yaml = format!(
            "  - id: {id}\n    kind: file_absent\n    paths: \"{glob}\"\n    level: warning\n"
        );
        if fix {
            yaml.push_str("    fix:\n      file_remove: {}\n");
        }
        yaml
    })
}

fn rule_filename_case(fix: bool) -> impl Strategy<Value = String> {
    let glob = select(&["**/*.rs", "**/*.md"][..]);
    let case = select(&["snake", "kebab", "lower"][..]);
    (rule_id("fc"), glob, case).prop_map(move |(id, glob, case)| {
        let mut yaml = format!(
            "  - id: {id}\n    kind: filename_case\n    paths: \"{glob}\"\n    case: {case}\n    level: warning\n"
        );
        if fix {
            yaml.push_str("    fix:\n      file_rename: {}\n");
        }
        yaml
    })
}

fn rule_file_content_matches(fix: bool) -> impl Strategy<Value = String> {
    let glob = select(&["**/*.md", "**/*.rs", "README.md"][..]);
    let pattern = select(&["SPDX", "Copyright", "TODO"][..]);
    (rule_id("fcm"), glob, pattern).prop_map(move |(id, glob, pattern)| {
        // The appended text MUST contain the checked pattern, or the fix does
        // not resolve the "content must match" violation and the rule re-flags
        // forever (non-convergent / non-idempotent). Each pattern here is a
        // literal, so a line containing it satisfies the regex.
        let fix_block = if fix {
            format!(
                "    fix:\n      file_append:\n        content: \"\\n{pattern} (added by alint)\\n\"\n"
            )
        } else {
            String::new()
        };
        format!(
            "  - id: {id}\n    kind: file_content_matches\n    paths: \"{glob}\"\n    pattern: \"{pattern}\"\n    level: warning\n{fix_block}"
        )
    })
}

fn rule_file_content_forbidden() -> impl Strategy<Value = String> {
    let glob = select(&["**/*.rs", "src/**/*.rs"][..]);
    let pattern = select(&[r"dbg!\s*\(", r"TODO", r"XXX"][..]);
    (rule_id("fcf"), glob, pattern).prop_map(|(id, glob, pattern)| {
        format!(
            "  - id: {id}\n    kind: file_content_forbidden\n    paths: \"{glob}\"\n    pattern: '{pattern}'\n    level: warning\n"
        )
    })
}

/// A `file_content_forbidden` rule fixed via the located `replace` op (Phase 1):
/// rewrites the forbidden `DEBUGME` token (planted only in `_trig/replaceme.txt`)
/// to `LOGGED`. Unsafe by default, so it is *suggested* under a bare `Fix` and
/// *applied* under `FixUnsafe` -- exercising the located path + the tier gate.
fn rule_file_content_forbidden_replace() -> impl Strategy<Value = String> {
    rule_id("repl").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: file_content_forbidden\n    paths: \"**/*.txt\"\n    pattern: 'DEBUGME'\n    level: error\n    fix:\n      replace:\n        replacement: \"LOGGED\"\n"
        )
    })
}

// ─── single-rule fixable catalogue (all fix ops) ──────────────
//
// These generators each emit ONE fixable rule covering a fix op that the
// multi-rule `fixable_rule_yaml` deliberately omits. They are drawn one at a
// time by `single_fixable_scenario_tree`: with exactly one rule there is no
// cross-rule, single-pass ordering interaction (two content fixers racing on
// the same file via the compose buffer -- a known Phase-0 limitation, tracked
// for Phase 1), so "a fully-applied fix leaves the check clean" is a sound
// convergence law to assert per fixer. All use `level: error` so the property
// is also exercised at error level. The content-hygiene fixers only trigger on
// the `content_blob` fixtures added for them.
//
// The glob is `**/*.{md,rs,txt,toml,json,tsx}` -- every extension the tree
// generator emits EXCEPT `.yml` -- rather than `**/*`, because the scenario
// runner writes the config to `.alint.yml` *inside* the linted tree. A `**/*`
// rule would match the config and a structural fixer (e.g. `file_prepend`)
// would rewrite it into invalid YAML, breaking the between-steps re-walk. Real
// corpus scenarios avoid this the same way (scoped globs).

fn rule_no_trailing_whitespace() -> impl Strategy<Value = String> {
    rule_id("ntw").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: no_trailing_whitespace\n    paths: \"**/*.{{md,rs,txt,toml,json,tsx}}\"\n    level: error\n    fix:\n      file_trim_trailing_whitespace: {{}}\n"
        )
    })
}

fn rule_line_endings() -> impl Strategy<Value = String> {
    (rule_id("le"), select(&["lf", "crlf"][..])).prop_map(|(id, target)| {
        format!(
            "  - id: {id}\n    kind: line_endings\n    paths: \"**/*.{{md,rs,txt,toml,json,tsx}}\"\n    target: {target}\n    level: error\n    fix:\n      file_normalize_line_endings: {{}}\n"
        )
    })
}

fn rule_final_newline() -> impl Strategy<Value = String> {
    rule_id("fnl").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: final_newline\n    paths: \"**/*.{{md,rs,txt,toml,json,tsx}}\"\n    level: error\n    fix:\n      file_append_final_newline: {{}}\n"
        )
    })
}

fn rule_max_blank_lines() -> impl Strategy<Value = String> {
    (rule_id("mbl"), select(&[0u32, 1, 2][..])).prop_map(|(id, max)| {
        format!(
            "  - id: {id}\n    kind: max_consecutive_blank_lines\n    paths: \"**/*.{{md,rs,txt,toml,json,tsx}}\"\n    max: {max}\n    level: error\n    fix:\n      file_collapse_blank_lines: {{}}\n"
        )
    })
}

fn rule_no_bom() -> impl Strategy<Value = String> {
    rule_id("nb").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: no_bom\n    paths: \"**/*.{{md,rs,txt,toml,json,tsx}}\"\n    level: error\n    fix:\n      file_strip_bom: {{}}\n"
        )
    })
}

fn rule_no_bidi_controls() -> impl Strategy<Value = String> {
    rule_id("nbd").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: no_bidi_controls\n    paths: \"**/*.{{md,rs,txt,toml,json,tsx}}\"\n    level: error\n    fix:\n      file_strip_bidi: {{}}\n"
        )
    })
}

fn rule_no_zero_width_chars() -> impl Strategy<Value = String> {
    rule_id("nzw").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: no_zero_width_chars\n    paths: \"**/*.{{md,rs,txt,toml,json,tsx}}\"\n    level: error\n    fix:\n      file_strip_zero_width: {{}}\n"
        )
    })
}

fn rule_file_header_prepend() -> impl Strategy<Value = String> {
    // `file_header` + `file_prepend`: prepending the required header lands the
    // pattern inside the first `lines`, so the check passes on the next read.
    rule_id("fh").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: file_header\n    paths: \"**/*.{{md,rs,txt,toml,json,tsx}}\"\n    pattern: \"(?s)Copyright\"\n    lines: 3\n    level: error\n    fix:\n      file_prepend:\n        content: \"// Copyright 2026\\n\"\n"
        )
    })
}

/// The full fixable catalogue: every fix op, one rule at a time.
/// The four whole-file-ish ops reuse the multi-rule generators (at
/// `level: warning`); the eight content/header ops come from the single-rule
/// generators above. Drives `single_fixable_scenario_tree`.
fn one_fixable_rule_yaml() -> impl Strategy<Value = String> {
    prop_oneof![
        // file_create, file_remove, file_rename, file_append.
        rule_file_exists(true),
        rule_file_absent(true),
        rule_filename_case(true),
        rule_file_content_matches(true),
        // file_prepend, and the seven content-hygiene / strip ops.
        rule_file_header_prepend(),
        rule_no_trailing_whitespace(),
        rule_line_endings(),
        rule_final_newline(),
        rule_max_blank_lines(),
        rule_no_bom(),
        rule_no_bidi_controls(),
        rule_no_zero_width_chars(),
        // the located `replace` op (Phase 1).
        rule_file_content_forbidden_replace(),
    ]
}

fn compose_config(rules: &[String]) -> String {
    let mut out = String::from("version: 1\nrules:\n");
    if rules.is_empty() {
        out.push_str("  []\n");
    } else {
        for r in rules {
            out.push_str(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    #[test]
    fn any_scenario_tree_produces_well_formed_scenarios() {
        let mut runner = TestRunner::default();
        for _ in 0..50 {
            let tree = any_scenario_tree().new_tree(&mut runner).unwrap().current();
            // Config must be valid YAML that alint-dsl accepts
            // (empty rules is OK).
            let parsed = alint_dsl::parse(&tree.given.config);
            assert!(
                parsed.is_ok(),
                "generated invalid config: {}\n---\n{}",
                parsed.unwrap_err(),
                tree.given.config,
            );
        }
    }

    #[test]
    fn fixable_scenario_tree_every_rule_has_a_fix_block() {
        let mut runner = TestRunner::default();
        for _ in 0..50 {
            let scenario = fixable_scenario_tree()
                .new_tree(&mut runner)
                .unwrap()
                .current();
            let cfg = alint_dsl::parse(&scenario.given.config).unwrap();
            for r in &cfg.rules {
                // file_content_forbidden is specifically excluded
                // from the fixable catalogue because it has no fix.
                assert_ne!(
                    r.kind, "file_content_forbidden",
                    "fixable strategy emitted a non-fixable kind"
                );
                assert!(
                    r.fix.is_some(),
                    "rule {:?} (kind {}) from fixable strategy has no `fix:` block",
                    r.id,
                    r.kind,
                );
            }
        }
    }

    #[test]
    fn single_fixable_scenario_tree_emits_exactly_one_fixable_rule() {
        let mut runner = TestRunner::default();
        for _ in 0..50 {
            let scenario = single_fixable_scenario_tree()
                .new_tree(&mut runner)
                .unwrap()
                .current();
            let cfg = alint_dsl::parse(&scenario.given.config).unwrap();
            assert_eq!(
                cfg.rules.len(),
                1,
                "single-fixable strategy must emit exactly one rule, got {}",
                cfg.rules.len()
            );
            assert!(
                cfg.rules[0].fix.is_some(),
                "single-fixable rule {:?} (kind {}) has no `fix:` block",
                cfg.rules[0].id,
                cfg.rules[0].kind,
            );
        }
    }

    #[test]
    fn single_fixable_scenario_tree_covers_all_fix_ops() {
        // The whole point of the single-rule strategy is that it exercises
        // EVERY fixer (the multi-rule catalogue covers only 4 of 12). Draw
        // enough scenarios that each of the 12 uniform arms is overwhelmingly
        // likely to appear (P(miss) ~ 12 * (11/12)^1500 ~ 1e-53), and assert we
        // saw the whole `FixSpec::ALL_OP_NAMES` set.
        use std::collections::BTreeSet;
        let mut runner = TestRunner::default();
        let mut seen: BTreeSet<&'static str> = BTreeSet::new();
        for _ in 0..1500 {
            let scenario = single_fixable_scenario_tree()
                .new_tree(&mut runner)
                .unwrap()
                .current();
            let cfg = alint_dsl::parse(&scenario.given.config).unwrap();
            if let Some(fix) = &cfg.rules[0].fix {
                seen.insert(fix.op_name());
            }
        }
        let expected: BTreeSet<&'static str> =
            alint_core::FixSpec::ALL_OP_NAMES.iter().copied().collect();
        assert_eq!(
            seen,
            expected,
            "single-fixable strategy did not cover every fix op; missing: {:?}",
            expected.difference(&seen).collect::<Vec<_>>()
        );
    }
}
