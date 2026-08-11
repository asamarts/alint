//! Render documented example fixtures onto rule pages (ADR-0014).
//!
//! A scenario carrying a `docs:` block is a rule page's worked example. For
//! each one we materialise its `given.tree` into a tempdir, write
//! `given.config` as a sibling **outside** that tree (so the config file is
//! never itself a walked, rule-matchable file), run the real `alint` binary
//! under a pinned-invocation contract, and capture stdout - so the example
//! repo, its config, and "what alint reports" shown on the page are the exact
//! fixture the e2e suite executes, re-verified at generation time.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use alint_testkit::{
    DocsCase, DocsExample, Scenario, Step, TreeNode, TreeSpec, materialize, setup_git,
};
use anyhow::{Context, Result, bail};

/// One rendered documented example: the markdown subsection injected onto the
/// kind's page, plus the keys used to order fail-before-pass.
pub(crate) struct RenderedExample {
    pub(crate) case: DocsCase,
    pub(crate) order: i32,
    pub(crate) markdown: String,
}

/// Scan `crates/alint-e2e/scenarios/**/*.yml`, render every scenario that
/// carries a `docs:` block, and group the results by the documented kind
/// (fail before pass, then by `order`). An empty map means no kind has opted
/// in yet, so nothing is rendered and the legacy hand-written examples stand.
///
/// `capture_output` controls whether the live `alint check` run is captured:
/// the full export passes `true`; the `--rules-only` docs-bundle bridge passes
/// `false` so it renders tree + config only and never cold-builds the release
/// binary (the cost `--rules-only` exists to avoid). Validation gates run in
/// both modes.
pub(crate) fn render_documented(
    workspace: &Path,
    capture_output: bool,
) -> Result<BTreeMap<String, Vec<RenderedExample>>> {
    let dir = workspace.join("crates/alint-e2e/scenarios");
    let mut documented: Vec<(PathBuf, Scenario, DocsExample)> = Vec::new();
    for path in yaml_files(&dir) {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading scenario {}", path.display()))?;
        let scenario = Scenario::from_yaml(&text)
            .with_context(|| format!("parsing scenario {}", path.display()))?;
        if let Some(docs) = scenario.docs.clone() {
            documented.push((path, scenario, docs));
        }
    }

    let mut out: BTreeMap<String, Vec<RenderedExample>> = BTreeMap::new();
    if documented.is_empty() {
        return Ok(out);
    }

    // Only build the binary when we actually capture output.
    let bin = if capture_output {
        Some(crate::build_release_binary()?)
    } else {
        None
    };
    for (path, scenario, docs) in &documented {
        let rendered = render_one(bin.as_deref(), scenario, docs)
            .with_context(|| format!("documented example {}", path.display()))?;
        out.entry(docs.kind.clone()).or_default().push(rendered);
    }
    for examples in out.values_mut() {
        examples.sort_by_key(|e| (matches!(e.case, DocsCase::Pass), e.order));
    }
    Ok(out)
}

/// Validate one documented scenario, optionally capture its live output, and
/// render its markdown subsection.
fn render_one(
    bin: Option<&Path>,
    scenario: &Scenario,
    docs: &DocsExample,
) -> Result<RenderedExample> {
    validate_documented(scenario, docs)?;
    let output = match bin {
        Some(bin) => Some(run_and_capture(bin, scenario, docs)?),
        None => None,
    };
    Ok(RenderedExample {
        case: docs.case,
        order: docs.order,
        markdown: render_markdown(
            docs,
            &scenario.given.tree,
            &scenario.given.config,
            output.as_deref(),
        ),
    })
}

/// Gate a documented scenario before it can back a page: hermetic config, no
/// git (Phase 2), a single `check` step so the `expect:` the `scenarios.rs`
/// harness asserts matches the run the page renders, and exactly one top-level
/// rule whose kind is the documented kind (so the label cannot lie).
fn validate_documented(scenario: &Scenario, docs: &DocsExample) -> Result<()> {
    let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&scenario.given.config)
        .context("parsing the documented example config")?;

    if has_remote_extends(&config) {
        bail!(
            "config uses a remote `extends:` - documented examples must be \
             hermetic (inline rules or `alint://bundled/...` only)"
        );
    }
    if scenario.when.as_slice() != [Step::Check] {
        bail!(
            "documented examples must be a single `check` scenario (when: [check]), \
             so the asserted `expect:` matches the rendered run; found {:?}",
            scenario.when
        );
    }

    let kinds = config_rule_kinds(&config);
    match kinds.as_slice() {
        [k] if *k == docs.kind => Ok(()),
        [k] => bail!(
            "`docs.kind: {}` but the config's rule is `kind: {k}` - they must match",
            docs.kind
        ),
        _ => bail!(
            "a documented example's config must declare exactly one top-level rule \
             (found {})",
            kinds.len()
        ),
    }
}

/// Materialise the scenario, run `alint check` under the pinned-invocation
/// contract, and return its stdout. Rejects an errored run (exit 2) and
/// cross-checks the case against the scenario's asserted `expect:`.
fn run_and_capture(bin: &Path, scenario: &Scenario, docs: &DocsExample) -> Result<String> {
    let tmp = tempfile::Builder::new()
        .prefix("alint-docs-example-")
        .tempdir()
        .context("creating tempdir")?;
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    materialize(&scenario.given.tree, &repo).context("materialising example tree")?;

    // Git-rule examples drive a real repo (init + commits) inside the tree.
    if let Some(git_spec) = &scenario.given.git {
        setup_git(&repo, git_spec).context("git setup for the documented example")?;
    }

    // Config OUTSIDE the walked tree, reached via `-c`, so `.alint.yml` is never
    // itself a rule-matchable file.
    let config_path = tmp.path().join("config.alint.yml");
    std::fs::write(&config_path, &scenario.given.config)?;

    // Pinned-invocation contract: run from inside the tree with a `.` target (no
    // absolute path leaks), config via `-c`, ASCII glyphs (the pages are
    // ASCII-gated) and no color, and a fixed allowlisted environment (env_clear
    // so a stray `{{env.X}}` in a config can't leak host state).
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let output = Command::new(bin)
        .current_dir(&repo)
        .arg("check")
        .arg("-c")
        .arg(&config_path)
        .arg("--ascii")
        .arg("--color=never")
        .arg(".")
        .env_clear()
        .env("PATH", path_var)
        .env("LC_ALL", "C")
        .env("TERM", "xterm")
        .output()
        .with_context(|| format!("spawning `{} check`", bin.display()))?;

    let code = output.status.code();
    if code == Some(2) {
        bail!(
            "`alint check` errored (exit 2) - a documented example must produce a \
             real finding, not an error:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let stdout = String::from_utf8(output.stdout).context("non-UTF8 `alint check` output")?;

    // The `case` must match the scenario's asserted contract (which the
    // scenarios.rs harness verifies against a real run), and a fail must show a
    // finding rather than an empty block.
    match docs.case {
        DocsCase::Fail => {
            if !asserts_violations(scenario) {
                bail!("`docs.case: fail` but the scenario's `expect:` asserts no violations");
            }
            if stdout.trim().is_empty() {
                bail!("`docs.case: fail` produced empty output");
            }
        }
        DocsCase::Pass => {
            if code != Some(0) {
                bail!("`docs.case: pass` is not clean (exit {code:?})");
            }
            if asserts_violations(scenario) {
                bail!("`docs.case: pass` but the scenario's `expect:` asserts violations");
            }
        }
    }
    Ok(stdout)
}

/// Does the scenario's first (only, for a documented example) `expect:` step
/// assert at least one violation?
fn asserts_violations(scenario: &Scenario) -> bool {
    scenario
        .expect
        .first()
        .and_then(|e| e.violations.as_ref())
        .is_some_and(|v| !v.is_empty())
}

/// The `kind:` of each top-level rule in a config's `rules:` list.
fn config_rule_kinds(config: &serde_yaml_ng::Value) -> Vec<String> {
    config
        .get("rules")
        .and_then(|r| r.as_sequence())
        .map(|rules| {
            rules
                .iter()
                .filter_map(|r| r.get("kind").and_then(|k| k.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// A top-level `extends:` that resolves over the network (`http(s)://`).
fn has_remote_extends(config: &serde_yaml_ng::Value) -> bool {
    fn is_remote(s: &str) -> bool {
        s.starts_with("http://") || s.starts_with("https://")
    }
    match config.get("extends") {
        Some(serde_yaml_ng::Value::String(s)) => is_remote(s),
        Some(serde_yaml_ng::Value::Sequence(seq)) => {
            seq.iter().any(|v| v.as_str().is_some_and(is_remote))
        }
        _ => false,
    }
}

/// The markdown subsection for one example: heading, example repo, config, and
/// (when captured) the `alint check` output.
fn render_markdown(
    docs: &DocsExample,
    tree: &TreeSpec,
    config: &str,
    output: Option<&str>,
) -> String {
    let lead = match docs.case {
        DocsCase::Fail => "The rule fires on this repository:",
        DocsCase::Pass => "This repository is compliant:",
    };
    let mut md = String::new();
    let _ = writeln!(&mut md, "### {}", docs.title);
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "{lead}");
    let _ = writeln!(&mut md);
    push_fenced(&mut md, "text", &render_tree(tree));
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "With this `.alint.yml`:");
    let _ = writeln!(&mut md);
    push_fenced(&mut md, "yaml", config);
    if let Some(output) = output {
        let _ = writeln!(&mut md);
        let _ = writeln!(&mut md, "`alint check` reports:");
        let _ = writeln!(&mut md);
        push_fenced(&mut md, "text", output);
    }
    let _ = writeln!(&mut md);
    md
}

/// Append a fenced code block, sizing the fence one backtick longer than the
/// longest backtick run in `content` so embedded fences can't break the block.
fn push_fenced(md: &mut String, lang: &str, content: &str) {
    let fence = fence_for(content);
    let _ = writeln!(md, "{fence}{lang}");
    md.push_str(content.trim_end_matches('\n'));
    let _ = writeln!(md);
    let _ = writeln!(md, "{fence}");
}

/// A fence long enough to wrap `content` - one more than the longest run of
/// backticks inside it, and at least 3.
fn fence_for(content: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for ch in content.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat(longest.max(2) + 1)
}

/// Render a tree spec as a sorted, ASCII, relative-path listing. Directories
/// get a trailing `/`. ASCII because the generated pages are ASCII-gated.
fn render_tree(tree: &TreeSpec) -> String {
    let mut lines: Vec<String> = tree
        .iter()
        .map(|(path, node)| match node {
            TreeNode::File(_) => path,
            TreeNode::Exec(_) => format!("{path}  (executable)"),
            TreeNode::Symlink(link) => format!("{path} -> {}", link.target),
            TreeNode::Dir(_) => format!("{path}/"),
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

/// Every `*.yml` under `dir`, recursively, in a stable order.
fn yaml_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in read.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("yml") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(yaml: &str) -> TreeSpec {
        TreeSpec::from_yaml(yaml).unwrap()
    }

    #[test]
    fn render_tree_sorts_and_marks_dirs() {
        let t = tree("Cargo.toml: \"x\"\nsrc:\n  main.rs: \"y\"\n");
        // Files sort with their paths; the directory gets a trailing slash.
        assert_eq!(render_tree(&t), "Cargo.toml\nsrc/\nsrc/main.rs");
    }

    #[test]
    fn render_tree_empty_is_empty() {
        assert_eq!(render_tree(&tree("{}")), "");
    }

    #[test]
    fn config_rule_kinds_lists_top_level_rules() {
        let cfg = serde_yaml_ng::from_str(
            "version: 1\nrules:\n  - id: a\n    kind: file_exists\n  - id: b\n    kind: dir_absent\n",
        )
        .unwrap();
        assert_eq!(config_rule_kinds(&cfg), vec!["file_exists", "dir_absent"]);
    }

    #[test]
    fn config_rule_kinds_empty_without_rules() {
        let cfg = serde_yaml_ng::from_str("version: 1\nextends: alint://bundled/x\n").unwrap();
        assert!(config_rule_kinds(&cfg).is_empty());
    }

    #[test]
    fn has_remote_extends_detects_http() {
        let remote = serde_yaml_ng::from_str("extends: https://example.com/r.yml\n").unwrap();
        assert!(has_remote_extends(&remote));
        let seq =
            serde_yaml_ng::from_str("extends: [alint://bundled/x, http://h/r.yml]\n").unwrap();
        assert!(has_remote_extends(&seq));
        let local = serde_yaml_ng::from_str("extends: alint://bundled/x\n").unwrap();
        assert!(!has_remote_extends(&local));
    }

    #[test]
    fn fence_grows_past_embedded_backticks() {
        assert_eq!(fence_for("no backticks"), "```");
        assert_eq!(fence_for("a ``` b"), "````");
        assert_eq!(fence_for("```` deep"), "`````");
    }

    #[test]
    fn markdown_omits_output_block_when_not_captured() {
        let docs = DocsExample {
            title: "T".into(),
            case: DocsCase::Pass,
            kind: "file_exists".into(),
            order: 0,
        };
        let md = render_markdown(&docs, &tree("README.md: \"x\""), "version: 1\n", None);
        assert!(md.contains("### T"));
        assert!(md.contains("With this `.alint.yml`:"));
        assert!(!md.contains("alint check` reports"));
    }
}
