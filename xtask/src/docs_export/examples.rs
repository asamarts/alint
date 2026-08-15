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

use alint_core::RuleRegistry;
use alint_testkit::{
    CommitSpec, DocsCase, DocsExample, GivenGit, Scenario, Step, TreeNode, TreeSpec, materialize,
    setup_git,
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
/// `capture_output` controls whether the live `alint check` run is captured.
/// Both the full export and the `--rules-only` docs-bundle bridge pass `true`,
/// so a doc-only refresh still renders each page's `alint check` output block
/// from a real run (ADR-0014 Phase 5). Passing `false` renders tree + config
/// only, skipping the release build. Validation gates run in either mode.
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
    let registry = alint_rules::builtin_registry();
    for (path, scenario, docs) in &documented {
        let rendered = render_one(bin.as_deref(), &registry, scenario, docs)
            .with_context(|| format!("documented example {}", path.display()))?;
        // Key by the CANONICAL kind: the page/H3 lookup and the double-example /
        // page-target gates all use canonical names, so an alias-spelled
        // `docs.kind` still lands its example on the canonical rule's page.
        out.entry(registry.canonical_kind(&docs.kind).to_string())
            .or_default()
            .push(rendered);
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
    registry: &RuleRegistry,
    scenario: &Scenario,
    docs: &DocsExample,
) -> Result<RenderedExample> {
    validate_documented(scenario, docs, registry)?;
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
            scenario.given.git.as_ref(),
            &scenario.given.config,
            output.as_deref(),
        ),
    })
}

/// Gate a documented scenario before it can back a page: hermetic config, a
/// single `check` step so the `expect:` the `scenarios.rs` harness asserts
/// matches the run the page renders, and exactly one top-level rule whose kind
/// is the documented kind (so the label cannot lie). The kind check is
/// alias-aware: a config `kind: cross_file_value_equals` satisfies a
/// `docs.kind: cross_file` page (both canonicalise to `cross_file`). A git-rule
/// example may carry a `given.git` block - the render drives it.
fn validate_documented(
    scenario: &Scenario,
    docs: &DocsExample,
    registry: &RuleRegistry,
) -> Result<()> {
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
        [k] if registry.canonical_kind(k) == registry.canonical_kind(&docs.kind) => Ok(()),
        [k] => bail!(
            "`docs.kind: {}` but the config's rule is `kind: {k}` - they must name the \
             same rule; `{k}` canonicalises to `{}`, `docs.kind` to `{}`",
            docs.kind,
            registry.canonical_kind(k),
            registry.canonical_kind(&docs.kind),
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
    // absolute path leaks), config via `-c`, ASCII glyphs (the pages keep ASCII
    // box/severity glyphs), `--color=always` so the page can render alint's real
    // ANSI colors (via an `ansi` code block) instead of a flat monochrome dump,
    // and a fixed allowlisted environment (env_clear so a stray `{{env.X}}` in a
    // config can't leak host state). OSC-8 hyperlinks that `always` re-enables
    // are stripped below - `ansi` highlighting wants SGR colors, not link escapes.
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    let output = Command::new(bin)
        .current_dir(&repo)
        .arg("check")
        .arg("-c")
        .arg(&config_path)
        .arg("--ascii")
        .arg("--color=always")
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
    let stdout =
        strip_osc8(&String::from_utf8(output.stdout).context("non-UTF8 `alint check` output")?);

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
    git: Option<&GivenGit>,
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
    push_fenced(
        &mut md,
        "text",
        &escape_docs_control_chars(&render_tree(tree)),
    );
    // The tree shows only names; a content-based rule (file_content_matches,
    // file_header, ...) turns on what's INSIDE the files, so render each file's
    // content too - otherwise the pass/fail premise is invisible.
    push_file_contents(&mut md, tree);
    let _ = writeln!(&mut md);
    let _ = writeln!(&mut md, "With this `.alint.yml`:");
    let _ = writeln!(&mut md);
    push_fenced(&mut md, "yaml", &escape_docs_control_chars(config));
    // Git-rule examples turn on the repo's history (a backdated commit, a bad
    // subject) - surface it, or the worked example's premise is off-screen.
    if let Some(history) = git.and_then(render_git_history) {
        let _ = writeln!(&mut md);
        let _ = writeln!(&mut md, "committed with this history (oldest first):");
        let _ = writeln!(&mut md);
        push_fenced(&mut md, "text", &escape_docs_control_chars(&history));
    }
    if let Some(output) = output {
        let _ = writeln!(&mut md);
        let _ = writeln!(&mut md, "`alint check` reports:");
        let _ = writeln!(&mut md);
        // `ansi` so the site's Shiki highlighter colours alint's real severity
        // markers / rule ids from the captured SGR codes. Escaping preserves ESC
        // (the SGR introducer) while tokenising a bidi/zero-width char a rule
        // message might echo from a hostile filename or commit subject.
        push_fenced(&mut md, "ansi", &escape_ansi_preserving_sgr(output));
    }
    let _ = writeln!(&mut md);
    md
}

/// Render each regular file's content after the tree, so a content-based rule's
/// example shows what's actually inside the files. Files are label + fenced
/// block, language inferred from the extension for highlighting. Empty files are
/// skipped; a file with binary bytes (e.g. the `file_is_text` null-byte fixture)
/// is noted rather than dumped, which never emits control chars into the page.
fn push_file_contents(md: &mut String, tree: &TreeSpec) {
    let mut files: Vec<(String, &str)> = tree
        .iter()
        .filter_map(|(path, node)| match node {
            TreeNode::File(c) => Some((path, c.as_str())),
            TreeNode::Exec(e) => Some((path, e.content.as_str())),
            TreeNode::Dir(_) | TreeNode::Symlink(_) => None,
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, content) in files {
        if content.is_empty() {
            continue;
        }
        let _ = writeln!(md);
        let _ = writeln!(md, "`{path}`:");
        let _ = writeln!(md);
        if looks_binary(content) {
            push_fenced(
                md,
                "text",
                &format!("(binary content, {} bytes)", content.len()),
            );
        } else {
            // Escape invisible / bidi / zero-width chars to a visible `<U+XXXX>`
            // token before fencing: the unicode-safety rules' fixtures carry them
            // deliberately (they ARE what the rule catches), and emitting them raw
            // into a published page ships a live Trojan-Source override / invisible
            // byte. The escaped form still SHOWS the reader where the offending
            // char is. (`\0` files are already noted as binary above.)
            push_fenced(md, lang_for(&path), &escape_docs_control_chars(content));
        }
    }
}

/// A Shiki language id inferred from `path`'s extension, for syntax
/// highlighting. Unknown extensions fall back to `text`.
fn lang_for(path: &str) -> &'static str {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
    {
        "rs" => "rust",
        "md" | "markdown" => "markdown",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "json" => "json",
        "sh" | "bash" => "bash",
        "py" => "python",
        "js" | "mjs" | "cjs" => "js",
        "ts" => "ts",
        "go" => "go",
        "rb" => "ruby",
        "c" | "h" => "c",
        _ => "text",
    }
}

/// Heuristic: content with a NUL byte is binary (the `file_is_text` fixture
/// embeds one, and macOS-junk magic bytes carry them). Kept deliberately narrow
/// so ordinary UTF-8 text - including the non-ASCII `file_is_ascii` fixture - is
/// still rendered verbatim.
fn looks_binary(content: &str) -> bool {
    content.contains('\0')
}

/// True for a char that must never be emitted RAW into a generated docs page:
/// it is invisible or reorders the surrounding text (a Trojan-Source vector).
/// Tab and newline are kept (ordinary layout).
///
/// Delegates to alint's OWN unicode-safety rule predicates so the docs escaper
/// covers exactly what those rules flag - a fixture demonstrating a rule can
/// never ship the very char it warns about. (A hand-maintained parallel list had
/// drifted, letting U+061C / U+200E / U+200F / U+2060 / U+180E through raw.) On
/// top it adds the invisible line/paragraph separators and the terminal control
/// classes (C0 sans tab/newline, DEL, C1).
pub(crate) fn is_dangerous_docs_char(c: char) -> bool {
    alint_rules::no_bidi_controls::is_bidi_control(c)
        // `false` = "not the leading BOM": tokenise a U+FEFF anywhere in the
        // embedded example, even the leading BOM the rule itself exempts - inside
        // a rendered code fence it is still an invisible byte worth showing.
        || alint_rules::no_zero_width_chars::is_flagged_zero_width(c, false)
        || matches!(c, '\u{2028}' | '\u{2029}')
        || (c.is_control() && c != '\t' && c != '\n')
}

/// Render `content` with every dangerous char (an [`is_dangerous_docs_char`] not
/// spared by `keep`) replaced by a visible `<U+XXXX>` token, so a fixture that
/// deliberately carries an invisible / bidi char (the unicode-safety rules) still
/// SHOWS where it is without shipping the live control char into the published
/// page. `keep` spares a char that is legitimately raw in its render context.
fn escape_dangerous_with(content: &str, keep: impl Fn(char) -> bool) -> String {
    let hot = |c: char| is_dangerous_docs_char(c) && !keep(c);
    if !content.chars().any(hot) {
        return content.to_string();
    }
    content
        .chars()
        .map(|c| {
            if hot(c) {
                format!("<U+{:04X}>", c as u32)
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// Escape every dangerous char in ordinary text (file contents, config, file
/// trees, git history) - nothing is spared.
fn escape_docs_control_chars(content: &str) -> String {
    escape_dangerous_with(content, |_| false)
}

/// Escape dangerous chars in captured `alint check` output while preserving `ESC`
/// (U+001B): the output is fenced as an `ansi` block so the site's highlighter
/// can read the real SGR colour codes ESC introduces. A rule whose message echoes
/// a hostile filename / commit subject (a bidi override, a zero-width char) still
/// renders as a visible token, not a live reordering char.
fn escape_ansi_preserving_sgr(output: &str) -> String {
    escape_dangerous_with(output, |c| c == '\u{1B}')
}

/// Strip OSC-8 hyperlink sequences (`ESC ] 8 ; ... ST`) while keeping SGR colour
/// codes. `--color=always` re-enables the terminal hyperlinks that `--color=never`
/// used to drop; an `ansi` code block wants colours, not link escapes.
fn strip_osc8(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // OSC-8 opener: ESC ] 8 ;
        if bytes[i] == 0x1b
            && bytes.get(i + 1) == Some(&b']')
            && bytes.get(i + 2) == Some(&b'8')
            && bytes.get(i + 3) == Some(&b';')
        {
            i += 4;
            // Skip to the string terminator: BEL (0x07) or ST (ESC \).
            while i < bytes.len() {
                if bytes[i] == 0x07 {
                    i += 1;
                    break;
                }
                if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b'\\') {
                    i += 2;
                    break;
                }
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    // Only whole ASCII OSC-8 spans were removed, so the rest is still valid UTF-8.
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// A `git log --oneline`-style summary of the scenario's commits (oldest
/// first), so a git-rule page shows the message, backdate, and staged files
/// that drive the example. `None` when there are no commits.
fn render_git_history(git: &GivenGit) -> Option<String> {
    if git.commits.is_empty() {
        return None;
    }
    let mut lines = Vec::with_capacity(git.commits.len());
    for commit in &git.commits {
        match commit {
            CommitSpec::Subject(subject) => lines.push(subject.clone()),
            CommitSpec::Detailed(d) => {
                let date = d
                    .date
                    .as_deref()
                    .map(|x| format!("{x}  "))
                    .unwrap_or_default();
                let files = if d.add.is_empty() {
                    String::new()
                } else {
                    format!("  (adds {})", d.add.join(", "))
                };
                lines.push(format!("{date}{}{files}", d.message));
            }
        }
    }
    Some(lines.join("\n"))
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
    fn escape_docs_control_chars_neutralizes_dangerous_chars() {
        // A fixture carrying a bidi override / zero-width char / BOM (the very
        // inputs the unicode-safety rules catch) must render as a visible token,
        // never the raw char - emitting it raw ships a Trojan-Source override /
        // invisible byte into the published page. Ordinary text (incl. tab/newline)
        // is untouched.
        let raw = "let c = \"\u{202e}reversed\u{200b}\u{feff}\";";
        let esc = escape_docs_control_chars(raw);
        assert!(
            !esc.chars().any(is_dangerous_docs_char),
            "no raw dangerous char survives: {esc:?}"
        );
        assert!(
            esc.contains("<U+202E>") && esc.contains("<U+200B>") && esc.contains("<U+FEFF>"),
            "shows each char's location: {esc:?}"
        );
        assert_eq!(
            escape_docs_control_chars("plain ascii\n\twith tab"),
            "plain ascii\n\twith tab",
            "ordinary text + tab/newline are untouched"
        );

        // Every codepoint alint's own unicode-safety rules flag - including the
        // bidi MARKS (U+061C/U+200E/U+200F) and the wider zero-width set (U+2060
        // WORD JOINER, U+180E) an earlier hand-rolled list missed - plus the
        // invisible line/paragraph separators and the control classes must escape.
        for c in [
            '\u{061C}', '\u{200E}', '\u{200F}', // bidi marks
            '\u{202A}', '\u{2069}', // bidi embedding / isolate
            '\u{200C}', '\u{200D}', '\u{2060}', '\u{180E}', // zero-width family
            '\u{2028}', '\u{2029}', // line / paragraph separators
            '\u{0007}', '\u{007F}', '\u{0080}', // C0 / DEL / C1
        ] {
            assert!(
                is_dangerous_docs_char(c),
                "U+{:04X} must be treated as dangerous",
                c as u32
            );
            assert_eq!(
                escape_docs_control_chars(&format!("x{c}y")),
                format!("x<U+{:04X}>y", c as u32),
                "U+{:04X} must render as a visible token",
                c as u32
            );
        }

        // Drift guard: the docs escaper must never cover LESS than the rules it
        // documents, or a rule's own example page could ship the raw char - the
        // exact drift that let the bidi marks through. Tie it to the predicates.
        for cp in 0u32..=0x2100 {
            if let Some(c) = char::from_u32(cp) {
                if alint_rules::no_bidi_controls::is_bidi_control(c)
                    || alint_rules::no_zero_width_chars::is_flagged_zero_width(c, false)
                {
                    assert!(
                        is_dangerous_docs_char(c),
                        "escaper drifted below the rules: U+{cp:04X} is rule-flagged but not escaped"
                    );
                }
            }
        }

        // The ```ansi variant tokenises a bidi char a rule message might echo,
        // but preserves ESC so the captured SGR colour codes still highlight.
        let ansi = "\u{1b}[31m\u{202e}oops\u{1b}[0m";
        let out = escape_ansi_preserving_sgr(ansi);
        assert!(out.contains('\u{1b}'), "ESC (SGR) must survive: {out:?}");
        assert!(out.contains("<U+202E>"), "bidi must be tokenised: {out:?}");
        assert!(!out.contains('\u{202e}'), "no raw bidi survives: {out:?}");
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
    fn validate_documented_matches_kinds_alias_aware() {
        let registry = alint_rules::builtin_registry();
        // `docs.kind: cross_file` (canonical) fed by a config that spells the
        // rule as its alias `cross_file_value_equals`. The gate must accept it:
        // both canonicalise to `cross_file`.
        let yaml = |config_kind: &str| {
            format!(
                r#"name: t
docs:
  title: X
  case: fail
  kind: cross_file
given:
  tree:
    a.txt: "x"
  config: |
    version: 1
    rules:
      - id: r
        kind: {config_kind}
when: [check]
expect:
  - violations: []
"#
            )
        };

        let aliased = Scenario::from_yaml(&yaml("cross_file_value_equals")).unwrap();
        let docs = aliased.docs.clone().unwrap();
        assert!(
            validate_documented(&aliased, &docs, &registry).is_ok(),
            "an alias-spelled config must satisfy its canonical page"
        );

        // A genuinely different rule is still rejected.
        let mismatch = Scenario::from_yaml(&yaml("file_header")).unwrap();
        let docs2 = mismatch.docs.clone().unwrap();
        assert!(
            validate_documented(&mismatch, &docs2, &registry).is_err(),
            "a config naming a different rule must still fail the gate"
        );
    }

    #[test]
    fn markdown_omits_output_block_when_not_captured() {
        let docs = DocsExample {
            title: "T".into(),
            case: DocsCase::Pass,
            kind: "file_exists".into(),
            order: 0,
        };
        let md = render_markdown(&docs, &tree("README.md: \"x\""), None, "version: 1\n", None);
        assert!(md.contains("### T"));
        assert!(md.contains("With this `.alint.yml`:"));
        assert!(!md.contains("alint check` reports"));
        // The file's content is shown (not just its name in the tree).
        assert!(
            md.contains("`README.md`:"),
            "file content label missing:\n{md}"
        );
    }

    #[test]
    fn file_contents_render_with_language_and_binary_guard() {
        // Text file: content shown, highlighted by extension. Binary file: noted,
        // never dumped (no control char reaches the page). Empty file: skipped.
        let t = tree(
            "src:\n  main.rs: \"fn main() {}\\n\"\nblob.bin: \"a\\u0000b\"\nempty.txt: \"\"\n",
        );
        let mut md = String::new();
        push_file_contents(&mut md, &t);
        assert!(
            md.contains("`src/main.rs`:") && md.contains("```rust"),
            "rust block missing:\n{md}"
        );
        assert!(md.contains("fn main() {}"));
        assert!(
            md.contains("(binary content, 3 bytes)"),
            "binary note missing:\n{md}"
        );
        assert!(!md.contains('\0'), "a NUL byte leaked into the page");
        assert!(!md.contains("`empty.txt`:"), "empty file should be skipped");
    }

    #[test]
    fn lang_for_maps_common_extensions() {
        assert_eq!(lang_for("a.rs"), "rust");
        assert_eq!(lang_for("dir/README.md"), "markdown");
        assert_eq!(lang_for("Cargo.toml"), "toml");
        assert_eq!(lang_for("scripts/run.sh"), "bash");
        assert_eq!(lang_for("Makefile"), "text");
        assert_eq!(lang_for("noext"), "text");
    }

    #[test]
    fn strip_osc8_removes_hyperlinks_keeps_color() {
        // ESC]8;;https://x ESC\ LINK ESC]8;; ESC\  wrapped in red SGR.
        let input = "\x1b[31m\x1b]8;;https://example.com/rule\x1b\\see docs\x1b]8;;\x1b\\\x1b[0m";
        let out = strip_osc8(input);
        assert!(!out.contains("\x1b]8"), "OSC-8 escape survived: {out:?}");
        assert!(
            !out.contains("https://example.com"),
            "hyperlink URI survived"
        );
        assert!(
            out.contains("\x1b[31m") && out.contains("\x1b[0m"),
            "SGR colour was stripped"
        );
        assert!(out.contains("see docs"), "link text was lost");
    }
}
