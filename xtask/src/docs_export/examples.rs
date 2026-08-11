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

use alint_testkit::{DocsCase, DocsExample, Scenario, TreeNode, TreeSpec, materialize};
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
/// The `alint` binary is built only when at least one documented scenario
/// exists, so an all-legacy tree pays nothing.
pub(crate) fn render_documented(
    workspace: &Path,
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

    let bin = crate::build_release_binary()?;
    for (path, scenario, docs) in &documented {
        let rendered = render_one(&bin, scenario, docs, path)
            .with_context(|| format!("rendering documented example {}", path.display()))?;
        out.entry(docs.kind.clone()).or_default().push(rendered);
    }
    for examples in out.values_mut() {
        examples.sort_by_key(|e| (matches!(e.case, DocsCase::Pass), e.order));
    }
    Ok(out)
}

/// Materialise one documented scenario, run `alint check` against it under the
/// pinned-invocation contract, and render its markdown subsection.
fn render_one(
    bin: &Path,
    scenario: &Scenario,
    docs: &DocsExample,
    path: &Path,
) -> Result<RenderedExample> {
    let tmp = tempfile::Builder::new()
        .prefix("alint-docs-example-")
        .tempdir()
        .context("creating tempdir for a documented example")?;
    let repo = tmp.path().join("repo");
    std::fs::create_dir_all(&repo)?;
    materialize(&scenario.given.tree, &repo).context("materialising documented example tree")?;

    // Config OUTSIDE the walked tree, reached via `-c`, so `.alint.yml` is
    // never itself a rule-matchable file (ADR-0014).
    let config_path = tmp.path().join("config.alint.yml");
    std::fs::write(&config_path, &scenario.given.config)?;

    // Pinned-invocation contract: run from inside the tree with a `.` target
    // (no absolute path leaks), config via `-c`, ASCII glyphs (the generated
    // pages are ASCII-gated) and no color, under a sanitised environment.
    let output = Command::new(bin)
        .current_dir(&repo)
        .arg("check")
        .arg("-c")
        .arg(&config_path)
        .arg("--ascii")
        .arg("--color=never")
        .arg(".")
        .env("LC_ALL", "C")
        .env("TERM", "xterm")
        .env_remove("CLICOLOR_FORCE")
        .env_remove("NO_COLOR")
        .env_remove("ALINT_FORCE_HYPERLINKS")
        .env_remove("ALINT_LOG")
        .output()
        .with_context(|| format!("spawning `{} check`", bin.display()))?;

    // A `fail` example must actually fire (non-zero); a `pass` example must be
    // clean (zero). The scenario's own `expect:` (asserted by the scenarios.rs
    // harness) guarantees a fail is a real violation rather than a rule error;
    // the exit-code cross-check here stops a mislabeled case from shipping.
    let code = output.status.code();
    match docs.case {
        DocsCase::Fail if code == Some(0) => bail!(
            "documented `fail` example produced no findings (exit 0); a fail \
             example must make the rule fire"
        ),
        DocsCase::Pass if code != Some(0) => bail!(
            "documented `pass` example is not clean (exit {code:?}); a pass \
             example must report no findings"
        ),
        _ => {}
    }

    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("non-UTF8 `alint check` output for {}", path.display()))?;

    Ok(RenderedExample {
        case: docs.case,
        order: docs.order,
        markdown: render_markdown(docs, &scenario.given.tree, &scenario.given.config, &stdout),
    })
}

/// The markdown subsection for one example: heading, example repo, config, and
/// the captured `alint check` output.
fn render_markdown(docs: &DocsExample, tree: &TreeSpec, config: &str, output: &str) -> String {
    let lead = match docs.case {
        DocsCase::Fail => "The rule fires on this repository:",
        DocsCase::Pass => "This repository is compliant:",
    };
    let mut md = String::new();
    let _ = writeln!(&mut md, "### {}", docs.title);
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "{lead}");
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "```text");
    md.push_str(&render_tree(tree));
    let _ = writeln!(&mut md, "```");
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "With this `.alint.yml`:");
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "```yaml");
    md.push_str(config.trim_end_matches('\n'));
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "```");
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "`alint check` reports:");
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "```text");
    md.push_str(output.trim_end_matches('\n'));
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "```");
    let _ = writeln!(&mut md);
    md
}

/// Render a tree spec as a sorted, ASCII, relative-path listing. Directories
/// get a trailing `/`. ASCII because the generated pages are ASCII-gated.
fn render_tree(tree: &TreeSpec) -> String {
    let mut lines: Vec<String> = tree
        .iter()
        .map(|(path, node)| match node {
            TreeNode::File(_) => path,
            TreeNode::Dir(_) => format!("{path}/"),
        })
        .collect();
    lines.sort();
    let mut s = lines.join("\n");
    s.push('\n');
    s
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
