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
    // Phase-2 structured ops: one `sv.<ext>` (set_value) + one `rv.<ext>`
    // (remove_value) trigger per format. Distinct single-file globs (`**/sv.<ext>`
    // / `**/rv.<ext>`) so each rule matches ONLY its own trigger; the tree generator
    // emits none of these names, so they are otherwise inert. `sv.*`'s `$.region`
    // is "OLD" (!= the rule's "NEW"), rewritten in place. `rv.*`'s removal target is
    // a UNIQUE key/element (a duplicate / document-root / multi-line value would
    // decline). JSON removal applies via the CST; YAML removal deletes the single-
    // line entry's line. XML/dotenv/INI/properties leaves are strings (so `equals`
    // is "NEW"); the XML removal target is a NON-root element (removing the root is
    // declined).
    let structured: &[(&str, &str)] = &[
        ("sv.tf", "region = \"OLD\"\n"),
        ("rv.tf", "keep = 1\nbanned = \"x\"\n"),
        ("sv.xml", "<region>OLD</region>\n"),
        (
            "rv.xml",
            "<root>\n  <keep>1</keep>\n  <banned>x</banned>\n</root>\n",
        ),
        ("sv.env", "REGION=OLD\n"),
        ("rv.env", "KEEP=1\nBANNED=x\n"),
        ("sv.ini", "[s]\nregion = OLD\n"),
        ("rv.ini", "[s]\nkeep = 1\nbanned = x\n"),
        ("sv.toml", "region = \"OLD\"\n"),
        ("rv.toml", "keep = 1\nbanned = \"x\"\n"),
        ("sv.properties", "region=OLD\n"),
        ("rv.properties", "keep=1\nbanned=x\n"),
        ("sv.json", "{\"region\": \"OLD\"}\n"),
        ("rv.json", "{\"keep\": 1, \"banned\": \"x\"}\n"),
        ("sv.yaml", "region: OLD\n"),
        ("rv.yaml", "keep: 1\nbanned: x\n"),
    ];
    for (name, content) in structured {
        insert_file(
            root,
            &[dir.clone(), (*name).to_string()],
            (*content).to_string(),
        );
    }
    // A shebang script WITHOUT the executable bit triggers shebang_has_executable
    // + the Phase-3 `chmod` fix, which sets +x. `insert_file` materializes it as a
    // plain (non-exec) file, so on Unix the rule fires; on non-Unix the host rule
    // no-ops. `.sh` is outside every content/structured rule's glob, so it is
    // otherwise inert. `**/needsx.sh` matches only this trigger.
    insert_file(
        root,
        &[dir.clone(), "needsx.sh".to_string()],
        "#!/bin/sh\necho hi\n".to_string(),
    );
    // A canonical source + a DRIFTED copy trigger cross_file `identical` + the
    // Phase-3 `sync_from` fix, which overwrites the copy with the source. Both are
    // clean `.txt` (no trailing ws / BOM / bidi / final-newline flaw and no
    // `DEBUGME`), so no other single-rule draw touches them; their 8-char stems
    // exceed the generator's 7-char limit, so they never collide. `**/sync_dst.txt`
    // matches only the copy; the source is referenced by its exact path.
    insert_file(
        root,
        &[dir.clone(), "sync_src.txt".to_string()],
        "canonical line\n".to_string(),
    );
    insert_file(
        root,
        &[dir.clone(), "sync_dst.txt".to_string()],
        "drifted line\n".to_string(),
    );
    // A NESTED file (below the root) triggers a subdir-anchored file_absent +
    // the Phase-3 `relocate` fix, which moves it to the repo root. Clean `.txt`
    // (no hygiene flaw, no `DEBUGME`), so no other single-rule draw disturbs it;
    // its 9-char stem exceeds the generator's 7-char limit, so neither the nested
    // trigger nor the relocated root file collides with a random name -- the root
    // slot is always free, so `relocate` always applies (and then converges).
    insert_file(
        root,
        &[dir.clone(), "reloctrig.txt".to_string()],
        "lockfile\n".to_string(),
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

/// An `hcl_path_equals` rule fixed via the located `set_value` op (Phase 2):
/// rewrites the scalar at `$.region` (planted as "OLD" only in `_trig/sv.tf`) to
/// "NEW". Safe, so a bare `Fix` applies it -- exercising the structured located
/// path (query -> span resolve -> serialize -> splice -> re-parse verify).
fn rule_hcl_path_equals_set_value() -> impl Strategy<Value = String> {
    rule_id("setv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: hcl_path_equals\n    paths: \"**/sv.tf\"\n    path: \"$.region\"\n    equals: \"NEW\"\n    level: error\n    fix:\n      set_value: {{}}\n"
        )
    })
}

/// An `hcl_path_absent` rule fixed via the located `remove_value` op (Phase 2):
/// deletes the `$.banned` node (planted only in `_trig/rv.tf`). Unsafe by
/// default, so it is *suggested* under a bare `Fix` and *applied* under
/// `FixUnsafe` -- exercising the located removal path + the tier gate.
fn rule_hcl_path_absent_remove_value() -> impl Strategy<Value = String> {
    rule_id("remv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: hcl_path_absent\n    paths: \"**/rv.tf\"\n    path: \"$.banned\"\n    level: error\n    fix:\n      remove_value: {{}}\n"
        )
    })
}

/// An `xml_path_equals` rule fixed via the located `set_value` op (Phase 2):
/// rewrites the scalar text at `$.region` (planted as "OLD" only in
/// `_trig/sv.xml`) to "NEW". XML leaves parse as strings, so the target is a
/// STRING literal. Safe, so a bare `Fix` applies it -- exercising the *XML*
/// structured resolver (element text span -> splice -> re-parse verify), a
/// distinct code path from the HCL analog above.
fn rule_xml_path_equals_set_value() -> impl Strategy<Value = String> {
    rule_id("xsetv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: xml_path_equals\n    paths: \"**/sv.xml\"\n    path: \"$.region\"\n    equals: \"NEW\"\n    level: error\n    fix:\n      set_value: {{}}\n"
        )
    })
}

/// An `xml_path_absent` rule fixed via the located `remove_value` op (Phase 2):
/// deletes the nested `$.root.banned` element (planted only in `_trig/rv.xml`).
/// The target is deliberately NON-root (its parent is `<root>`), since removing
/// the document root is declined. Unsafe by default, so it is *suggested* under
/// a bare `Fix` and *applied* under `FixUnsafe` -- exercising the XML removal
/// path (element-line span) + the tier gate.
fn rule_xml_path_absent_remove_value() -> impl Strategy<Value = String> {
    rule_id("xremv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: xml_path_absent\n    paths: \"**/rv.xml\"\n    path: \"$.root.banned\"\n    level: error\n    fix:\n      remove_value: {{}}\n"
        )
    })
}

/// A `dotenv_path_equals` rule fixed via the located `set_value` op (Phase 2):
/// rewrites the scalar at `$.REGION` (planted as "OLD" only in `_trig/sv.env`)
/// to "NEW". dotenv leaves are strings, so the target is a STRING literal. Safe,
/// so a bare `Fix` applies it -- exercising the hand-rolled dotenv resolver
/// (re-scan the raw text -> value span -> splice -> re-parse verify).
fn rule_dotenv_path_equals_set_value() -> impl Strategy<Value = String> {
    rule_id("dsetv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: dotenv_path_equals\n    paths: \"**/sv.env\"\n    path: \"$.REGION\"\n    equals: \"NEW\"\n    level: error\n    fix:\n      set_value: {{}}\n"
        )
    })
}

/// A `dotenv_path_absent` rule fixed via the located `remove_value` op (Phase 2):
/// deletes the unique `$.BANNED` key line (planted only in `_trig/rv.env`).
/// Unsafe by default, so it is *suggested* under a bare `Fix` and *applied*
/// under `FixUnsafe` -- exercising the dotenv whole-line removal + the tier gate.
fn rule_dotenv_path_absent_remove_value() -> impl Strategy<Value = String> {
    rule_id("dremv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: dotenv_path_absent\n    paths: \"**/rv.env\"\n    path: \"$.BANNED\"\n    level: error\n    fix:\n      remove_value: {{}}\n"
        )
    })
}

/// An `ini_path_equals` rule fixed via the located `set_value` op (Phase 2):
/// rewrites the section-scoped scalar at `$['s']['region']` (planted as "OLD"
/// only in `_trig/sv.ini`) to "NEW". Safe, so a bare `Fix` applies it --
/// exercising the hand-rolled INI resolver (2-level section nav -> value span ->
/// splice -> re-parse verify).
fn rule_ini_path_equals_set_value() -> impl Strategy<Value = String> {
    rule_id("isetv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: ini_path_equals\n    paths: \"**/sv.ini\"\n    path: \"$['s']['region']\"\n    equals: \"NEW\"\n    level: error\n    fix:\n      set_value: {{}}\n"
        )
    })
}

/// An `ini_path_absent` rule fixed via the located `remove_value` op (Phase 2):
/// deletes the unique single-line section key `$['s']['banned']` (planted only
/// in `_trig/rv.ini`). Unsafe by default, so it is *suggested* under a bare
/// `Fix` and *applied* under `FixUnsafe` -- exercising the INI whole-line
/// removal + the tier gate.
fn rule_ini_path_absent_remove_value() -> impl Strategy<Value = String> {
    rule_id("iremv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: ini_path_absent\n    paths: \"**/rv.ini\"\n    path: \"$['s']['banned']\"\n    level: error\n    fix:\n      remove_value: {{}}\n"
        )
    })
}

/// A `toml_path_equals` rule fixed via the located `set_value` op (Phase 2):
/// rewrites the scalar at `$.region` (planted as "OLD" only in `_trig/sv.toml`)
/// to "NEW". Safe, so a bare `Fix` applies it -- exercising the `toml_edit`
/// whole-document rewrite path (parse -> decor-preserving set -> reserialize ->
/// re-parse verify), distinct from the span-splice resolvers.
fn rule_toml_path_equals_set_value() -> impl Strategy<Value = String> {
    rule_id("tsetv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: toml_path_equals\n    paths: \"**/sv.toml\"\n    path: \"$.region\"\n    equals: \"NEW\"\n    level: error\n    fix:\n      set_value: {{}}\n"
        )
    })
}

/// A `toml_path_absent` rule fixed via the located `remove_value` op (Phase 2):
/// deletes the `$.banned` key (planted only in `_trig/rv.toml`). Unsafe by
/// default, so it is *suggested* under a bare `Fix` and *applied* under
/// `FixUnsafe` -- exercising the `toml_edit` whole-document removal + the tier gate.
fn rule_toml_path_absent_remove_value() -> impl Strategy<Value = String> {
    rule_id("tremv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: toml_path_absent\n    paths: \"**/rv.toml\"\n    path: \"$.banned\"\n    level: error\n    fix:\n      remove_value: {{}}\n"
        )
    })
}

/// A `properties_path_equals` rule fixed via the located `set_value` op (Phase 2):
/// rewrites the scalar at `$.region` (planted as "OLD" only in
/// `_trig/sv.properties`) to "NEW". Safe, so a bare `Fix` applies it -- exercising
/// the hand-rolled conservative properties resolver.
fn rule_properties_path_equals_set_value() -> impl Strategy<Value = String> {
    rule_id("psetv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: properties_path_equals\n    paths: \"**/sv.properties\"\n    path: \"$.region\"\n    equals: \"NEW\"\n    level: error\n    fix:\n      set_value: {{}}\n"
        )
    })
}

/// A `properties_path_absent` rule fixed via the located `remove_value` op
/// (Phase 2): deletes the `$.banned` key line (planted only in
/// `_trig/rv.properties`). Unsafe by default, so *suggested* under a bare `Fix`
/// and *applied* under `FixUnsafe` -- exercising the properties whole-line removal.
fn rule_properties_path_absent_remove_value() -> impl Strategy<Value = String> {
    rule_id("premv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: properties_path_absent\n    paths: \"**/rv.properties\"\n    path: \"$.banned\"\n    level: error\n    fix:\n      remove_value: {{}}\n"
        )
    })
}

/// A `json_path_equals` rule fixed via the located `set_value` op (Phase 2):
/// rewrites the scalar at `$.region` (planted as "OLD" only in `_trig/sv.json`)
/// to "NEW" via a span-splice over jsonc-parser's AST range. Like TOML, JSON is
/// TYPED, but the target here is a STRING so a bare `Fix` applies it -- exercising
/// the spanned JSON resolver against arbitrary co-resident trees.
fn rule_json_path_equals_set_value() -> impl Strategy<Value = String> {
    rule_id("jsetv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: json_path_equals\n    paths: \"**/sv.json\"\n    path: \"$.region\"\n    equals: \"NEW\"\n    level: error\n    fix:\n      set_value: {{}}\n"
        )
    })
}

/// A `json_path_absent` rule fixed via the located `remove_value` op: deletes the
/// `$.banned` member (planted only in `_trig/rv.json`) via the jsonc-parser editable
/// CST (a WHOLE-DOCUMENT rewrite with correct comma surgery -- unlike JSON `set_value`,
/// which is a span splice). Unsafe by default, so *suggested* under a bare `Fix` and
/// *applied* under `FixUnsafe` -- exercising the CST removal across arbitrary trees.
fn rule_json_path_absent_remove_value() -> impl Strategy<Value = String> {
    rule_id("jremv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: json_path_absent\n    paths: \"**/rv.json\"\n    path: \"$.banned\"\n    level: error\n    fix:\n      remove_value: {{}}\n"
        )
    })
}

/// A `yaml_path_equals` rule fixed via the located `set_value` op (Phase 2):
/// rewrites the scalar at `$.region` (planted as "OLD" only in `_trig/sv.yaml`)
/// to "NEW" via a span-splice over saphyr's `MarkedYaml` node range. YAML is
/// TYPED, but the target here is a STRING so a bare `Fix` applies it -- exercising
/// the spanned YAML resolver (incl. its char->byte offset conversion) against
/// arbitrary co-resident trees.
fn rule_yaml_path_equals_set_value() -> impl Strategy<Value = String> {
    rule_id("ysetv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: yaml_path_equals\n    paths: \"**/sv.yaml\"\n    path: \"$.region\"\n    equals: \"NEW\"\n    level: error\n    fix:\n      set_value: {{}}\n"
        )
    })
}

/// A `yaml_path_absent` rule fixed via the located `remove_value` op: deletes the
/// single-line `$.banned` block-mapping entry (planted only in `_trig/rv.yaml`) by
/// removing its whole physical line (a conservative hand-rolled scan -- saphyr is
/// read-only). Unsafe by default, so *suggested* under a bare `Fix` and *applied*
/// under `FixUnsafe` -- exercising the YAML line removal across arbitrary trees.
fn rule_yaml_path_absent_remove_value() -> impl Strategy<Value = String> {
    rule_id("yremv").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: yaml_path_absent\n    paths: \"**/rv.yaml\"\n    path: \"$.banned\"\n    level: error\n    fix:\n      remove_value: {{}}\n"
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

/// A `shebang_has_executable` rule fixed via the `chmod` op (Phase 3): sets +x on
/// the planted shebang script `_trig/needsx.sh` (materialized WITHOUT the exec
/// bit, so on Unix the rule fires). Safe, so a bare `Fix` applies it. Unix-only:
/// on a non-Unix target the host rule no-ops, so the scenario converges trivially
/// (nothing to fix, nothing to converge). Scoped to `**/needsx.sh` so it matches
/// ONLY its own trigger; `.sh` is outside every other single-rule glob.
fn rule_shebang_chmod() -> impl Strategy<Value = String> {
    rule_id("chmodx").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: shebang_has_executable\n    paths: \"**/needsx.sh\"\n    level: error\n    fix:\n      chmod: {{}}\n"
        )
    })
}

/// A `file_absent` rule fixed via the spawning `git_untrack` op (Phase 3):
/// `git rm --cached` on a tracked-but-forbidden path. Scoped with
/// `git_tracked_only: true`, so on the property net's NON-git trees the host rule
/// is a silent no-op (the tracked set is `None`) -- it produces no violation, and
/// the fix laws early-return (nothing to apply / converge / stage). Its value here
/// is purely the fix-op coverage gate (`single_fixable_scenario_tree_covers_all_fix_ops`);
/// real apply/convergence lives in `scenarios/fix/git_untrack_*` and the fixer
/// units. Scoped to `**/*.trackjunk` so it never collides with a generated name.
fn rule_git_untrack() -> impl Strategy<Value = String> {
    rule_id("gu").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: file_absent\n    paths: \"**/*.trackjunk\"\n    git_tracked_only: true\n    level: error\n    fix:\n      git_untrack: {{}}\n"
        )
    })
}

/// A `dir_exists` rule fixed via `dir_create` (Phase 3): the required literal
/// directory `_reqdir` is absent from every generated tree (the dir generator
/// never emits that name), so `dir_exists` fires and `dir_create` makes it -- a
/// Safe fix that converges (the re-walk then sees the dir) and is idempotent (a
/// second pass finds it already present). No planted trigger needed (the ABSENCE
/// of `_reqdir` is the trigger, like `file_create`'s absent target).
fn rule_dir_create() -> impl Strategy<Value = String> {
    rule_id("dc").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: dir_exists\n    paths: \"_reqdir\"\n    level: error\n    fix:\n      dir_create: {{}}\n"
        )
    })
}

/// A `cross_file` `relation: identical` rule fixed via the content-injecting
/// `sync_from` op (Phase 3): the planted target `_trig/sync_dst.txt` drifts from
/// its canonical source `_trig/sync_src.txt`, so the rule fires and `sync_from`
/// overwrites the target with the source -> byte-identical -> converges (and is
/// idempotent on a second pass). Unsafe by default, so a bare `Fix` *suggests*
/// it; `--unsafe-fixes` applies it (exercising `fix_unsafe_converges`). Both
/// trigger names have 8-char stems, above the generator's 7-char limit, so they
/// never collide with a random file.
fn rule_sync_from() -> impl Strategy<Value = String> {
    rule_id("sf").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: cross_file\n    relation: identical\n    source:\n      file: _trig/sync_src.txt\n    targets:\n      files: \"**/sync_dst.txt\"\n    level: error\n    fix:\n      sync_from: {{}}\n"
        )
    })
}

/// A `file_absent` rule fixed via the `relocate` op (Phase 3): the planted nested
/// file `_trig/reloctrig.txt` matches the SUBDIRECTORY-anchored `**/*/reloctrig.txt`
/// (a nested lockfile), so the rule fires and `relocate` moves it to the repo root
/// `reloctrig.txt` -- which no longer matches the subdir pattern, so it converges
/// (and is idempotent on a second pass: nothing nested remains). Unsafe by default,
/// so a bare `Fix` *suggests* it; `--unsafe-fixes` applies it (exercising
/// `fix_unsafe_converges`). The 9-char stem exceeds the generator's 7-char limit,
/// so the root slot never collides with a random file -- the move always applies.
fn rule_relocate() -> impl Strategy<Value = String> {
    rule_id("rl").prop_map(|id| {
        format!(
            "  - id: {id}\n    kind: file_absent\n    paths: \"**/*/reloctrig.txt\"\n    level: error\n    fix:\n      relocate: {{}}\n"
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
        // the located structured ops (Phase 2): HCL/XML/dotenv/INI/properties/JSON/
        // YAML span resolvers + the TOML whole-document rewriter. (JSON and YAML
        // `remove_value` are deferred, so their generators fuzz the clean-decline
        // path.)
        rule_hcl_path_equals_set_value(),
        rule_hcl_path_absent_remove_value(),
        rule_xml_path_equals_set_value(),
        rule_xml_path_absent_remove_value(),
        rule_dotenv_path_equals_set_value(),
        rule_dotenv_path_absent_remove_value(),
        rule_ini_path_equals_set_value(),
        rule_ini_path_absent_remove_value(),
        rule_toml_path_equals_set_value(),
        rule_toml_path_absent_remove_value(),
        rule_properties_path_equals_set_value(),
        rule_properties_path_absent_remove_value(),
        rule_json_path_equals_set_value(),
        rule_json_path_absent_remove_value(),
        rule_yaml_path_equals_set_value(),
        rule_yaml_path_absent_remove_value(),
        // the metadata `chmod` op (Phase 3): +x on a planted shebang script.
        rule_shebang_chmod(),
        // the spawning `git_untrack` op (Phase 3): a no-op on the non-git property
        // trees (git_tracked_only), so it only exercises the op-coverage gate.
        rule_git_untrack(),
        // the `dir_create` op (Phase 3): creates the absent `_reqdir` -> converges.
        rule_dir_create(),
        // the cross-file `sync_from` op (Phase 3): mirrors a drifted target from its
        // canonical source -> converges (applied under --unsafe-fixes).
        rule_sync_from(),
        // the `relocate` op (Phase 3): moves a nested lockfile to the repo root ->
        // converges (applied under --unsafe-fixes).
        rule_relocate(),
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
        // The whole point of the single-rule strategy is that it exercises EVERY
        // fixer (the multi-rule catalogue covers only 4). Draw enough scenarios that
        // each of the ~17 uniform arms is overwhelmingly likely to appear (P(miss) ~
        // 17 * (16/17)^1500 ~ 1e-38), and assert we saw the whole, dynamically-read
        // `FixSpec::ALL_OP_NAMES` set (so the count stays correct as ops are added).
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
        // `command` (a user-supplied fix command) is EXEMPT from the property net:
        // alint cannot guarantee an arbitrary command's convergence / idempotence,
        // so it is not drawn here (the convergence + idempotence laws would be
        // ill-defined). It is covered instead by `command_ops.rs`/`command.rs` unit
        // tests and by its own fire (`command_runs_the_user_fix_command.yml`) +
        // silent (`command_is_silent_when_check_passes.yml`) e2e scenarios
        // (auto-fix.md 5.6). Every OTHER op must appear.
        let expected: BTreeSet<&'static str> = alint_core::FixSpec::ALL_OP_NAMES
            .iter()
            .copied()
            .filter(|op| *op != "command")
            .collect();
        assert_eq!(
            seen,
            expected,
            "single-fixable strategy did not cover every (non-command) fix op; missing: {:?}",
            expected.difference(&seen).collect::<Vec<_>>()
        );
    }
}
