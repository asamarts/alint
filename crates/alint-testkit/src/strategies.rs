//! Proptest strategies for generating [`Scenario`]-shaped inputs.
//!
//! The strategies here are intentionally small and focused — just
//! enough variety to shake invariants (panic-freedom, dry-run
//! purity, fix idempotence, fix→check convergence) without blowing
//! up the state space. Tune via the [`ScenarioTreeParams`] knobs.
//!
//! Two factories are provided:
//!
//! - [`any_scenario_tree`] — tree + config drawn from a broad
//!   catalogue. Covers non-fixable rule kinds too; use for
//!   invariants that only require alint to not panic.
//! - [`fixable_scenario_tree`] — restricts the rule catalogue to
//!   kinds whose fix strategy is well-defined for random inputs
//!   (`file_create`, `file_remove`, `file_prepend`, `file_append`,
//!   `file_rename`). Use for invariants that exercise `alint fix`.

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
            given: Given { tree, config },
            when: vec![],
            expect: vec![],
            expect_tree: None,
            expect_tree_mode: crate::scenario::ExpectTreeMode::default(),
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
            given: Given { tree, config },
            when: vec![],
            expect: vec![],
            expect_tree: None,
            expect_tree_mode: crate::scenario::ExpectTreeMode::default(),
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
        string_regex(r"[a-zA-Z0-9 \n]{0,40}").unwrap(),
    ]
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
    vec(any_rule_yaml(), 0..=max_rules)
}

fn fixable_rules(max_rules: usize) -> impl Strategy<Value = Vec<String>> {
    vec(fixable_rule_yaml(), 0..=max_rules)
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
        let mut yaml = format!(
            "  - id: {id}\n    kind: file_content_matches\n    paths: \"{glob}\"\n    pattern: \"{pattern}\"\n    level: warning\n"
        );
        if fix {
            yaml.push_str(
                "    fix:\n      file_append:\n        content: \"\\nSPDX-License-Identifier: Apache-2.0\\n\"\n",
            );
        }
        yaml
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
}
