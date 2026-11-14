//! The generated `/docs/bundled-rulesets/<name>/` pages: what each bundled
//! ruleset checks, the reasoning its authors left as comments in the YAML, the
//! facts its `when:` clauses read, and how to customize it.
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use alint_rules::kind_docs::KIND_SUMMARIES;
use anyhow::{Context, Result};

use crate::walkdir_plain;

use super::docs_paths;
use super::rule_pages::{
    escape_yaml_string, first_sentence, meta_desc_clean, strip_markup, truncate_meta_summary,
};

/// GitHub repo-relative base for source-of-truth links rendered
/// into the bundled-ruleset pages. Pinned to `main` so readers
/// always land on the latest version of each ruleset.
const ALINT_REPO_BLOB_URL: &str = "https://github.com/asamarts/alint/blob/main";

/// One rule of a bundled ruleset as written in its YAML file: the
/// author's comments about it, rendered as markdown, and its
/// definition with those comments removed.
#[derive(Debug, Default, PartialEq)]
pub(super) struct RuleSource {
    pub(super) id: String,
    pub(super) notes_md: String,
    pub(super) definition: String,
}

/// Split the `rules:` block of a bundled ruleset into its rules, keeping
/// what serde drops: the comments. A rule's notes are the comment lines
/// directly above its `- id:` line (a blank line or a `# ---` divider
/// cuts them off) plus the comment lines at the rule's own key indent.
/// Comments nested deeper, inside a list value, explain that value, so
/// they stay in the definition.
///
/// Rulesets are written with rules at a two-space indent (`  - id: x`);
/// the catalogue test checks that every rule serde sees is found here.
pub(super) fn rule_sources(yaml_text: &str) -> Vec<RuleSource> {
    let mut out: Vec<RuleSource> = Vec::new();
    let mut in_rules = false;
    let mut pending: Vec<String> = Vec::new();
    let mut current: Option<(String, Vec<String>, Vec<String>)> = None;
    let finish = |current: &mut Option<(String, Vec<String>, Vec<String>)>,
                  out: &mut Vec<RuleSource>| {
        if let Some((id, comments, mut def)) = current.take() {
            while def.last().is_some_and(|l| l.trim().is_empty()) {
                def.pop();
            }
            out.push(RuleSource {
                id,
                notes_md: notes_markdown(&comments),
                definition: def.join("\n"),
            });
        }
    };

    for line in yaml_text.lines() {
        let line = line.trim_end();
        if !in_rules {
            in_rules = line == "rules:";
            continue;
        }
        if let Some(rest) = line.strip_prefix("  - id:") {
            finish(&mut current, &mut out);
            let id = rest.trim().trim_matches(['"', '\'']).to_string();
            let comments = std::mem::take(&mut pending);
            current = Some((id, comments, vec![line[2..].to_string()]));
            continue;
        }
        // A comment at the list indent (or at the margin) belongs to the
        // rule below it.
        if let Some(comment) = line.strip_prefix("  #").or_else(|| line.strip_prefix('#')) {
            finish(&mut current, &mut out);
            if comment.trim_start().starts_with("---") {
                pending.clear();
            } else {
                pending.push(comment_body(comment));
            }
            continue;
        }
        if !line.is_empty() && !line.starts_with(' ') {
            // The next top-level key ends the rules block.
            break;
        }
        if line.is_empty() {
            pending.clear();
            if let Some((_, _, def)) = current.as_mut() {
                def.push(String::new());
            }
            continue;
        }
        if let Some((_, comments, def)) = current.as_mut() {
            if let Some(comment) = line.strip_prefix("    #") {
                comments.push(comment_body(comment));
            } else {
                def.push(line.strip_prefix("  ").unwrap_or(line).to_string());
            }
        }
    }
    finish(&mut current, &mut out);
    out
}

/// The text of a comment line after its `#`, less the one space that
/// conventionally follows it (further indentation is kept: it marks an
/// aligned block).
fn comment_body(after_hash: &str) -> String {
    after_hash
        .strip_prefix(' ')
        .unwrap_or(after_hash)
        .to_string()
}

/// Render a rule's comment lines as markdown. Comments are hard-wrapped
/// plain text, so each paragraph is rejoined into one line and escaped
/// (a wrapped line starting `+ ` or a bare `**/*.rs` glob would otherwise
/// turn into a list or emphasis). A paragraph whose lines start `- ` or
/// `* ` is a list; an indented or column-aligned paragraph is kept as a
/// text block. A paragraph that opens with a release tag (`v0.9.18: `)
/// reads "Changed in v0.9.18: ".
fn notes_markdown(bodies: &[String]) -> String {
    let mut blocks: Vec<String> = Vec::new();
    for para in bodies.split(|b| b.trim().is_empty()) {
        if para.is_empty() {
            continue;
        }
        let indent = |l: &String| l.len() - l.trim_start().len();
        let aligned = para.iter().any(|l| l.trim().contains("   "));
        if indent(&para[0]) >= 2 || aligned {
            let cut = para.iter().map(indent).min().unwrap_or(0);
            let mut block = String::from("```text\n");
            for line in para {
                let _ = writeln!(&mut block, "{}", &line[cut..]);
            }
            block.push_str("```");
            blocks.push(block);
        } else if para[0].starts_with("- ") || para[0].starts_with("* ") {
            let mut items: Vec<String> = Vec::new();
            for line in para {
                let line = line.trim();
                match line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                    Some(item) => items.push(item.to_string()),
                    None => {
                        if let Some(last) = items.last_mut() {
                            last.push(' ');
                            last.push_str(line);
                        }
                    }
                }
            }
            let list: Vec<String> = items
                .iter()
                .map(|i| format!("- {}", escape_inline(i)))
                .collect();
            blocks.push(list.join("\n"));
        } else {
            let text = para.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
            let text = match release_tag_len(&text) {
                Some(_) => format!("Changed in {text}"),
                None => text,
            };
            blocks.push(escape_block_start(&escape_inline(&text)));
        }
    }
    blocks.join("\n\n")
}

/// The length of a leading release tag (`v0.9.18` in `v0.9.18: ...`,
/// `v0.10 — ...`), if the text opens with one.
fn release_tag_len(text: &str) -> Option<usize> {
    let rest = text.strip_prefix('v')?;
    let ver_len = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    let (ver, tail) = rest.split_at(ver_len);
    let tagged = ver.contains('.')
        && ver.starts_with(|c: char| c.is_ascii_digit())
        && [":", " —", " -", " ("]
            .iter()
            .any(|sep| tail.starts_with(sep));
    tagged.then_some(1 + ver_len)
}

/// Escape plain text for inline markdown: characters that would start
/// emphasis, a link, strikethrough or an escape are escaped, except inside
/// code spans and bare URLs, which render literally anyway, and a word
/// holding a tag (`<year>`) is set as code rather than parsed as HTML.
pub(super) fn escape_inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '`' {
            let ticks = rest.len() - rest.trim_start_matches('`').len();
            let fence = &rest[..ticks];
            if let Some(close) = rest[ticks..].find(fence) {
                let span = ticks + close + ticks;
                out.push_str(&rest[..span]);
                rest = &rest[span..];
                continue;
            }
            for _ in 0..ticks {
                out.push_str("\\`");
            }
            rest = &rest[ticks..];
            continue;
        }
        let at_word_start = out.is_empty() || out.ends_with([' ', '(', '<', '"']);
        if at_word_start && (rest.starts_with("https://") || rest.starts_with("http://")) {
            let url_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push_str(&rest[..url_len]);
            rest = &rest[url_len..];
            continue;
        }
        match c {
            '\\' | '*' | '_' | '[' | ']' | '~' => {
                out.push('\\');
                out.push(c);
            }
            '<' => {
                // A tag or placeholder (`<year>`, `<Nullable>enable</Nullable>`)
                // becomes code, through to the end of its word: escaped, it
                // would be one long unbreakable run of text.
                let word_len = rest.find(char::is_whitespace).unwrap_or(rest.len());
                let word = rest[..word_len].trim_end_matches(['.', ',', ';', ':', ')', '"', '\'']);
                if word.contains('>') && !word.contains('`') {
                    let _ = write!(out, "`{word}`");
                    rest = &rest[word.len()..];
                    continue;
                }
                out.push_str("&lt;");
            }
            _ => out.push(c),
        }
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// Escape what would make a one-line paragraph a heading, quote, list
/// item or thematic break.
fn escape_block_start(line: &str) -> String {
    let ordered = line
        .find(|c: char| !c.is_ascii_digit())
        .is_some_and(|i| i > 0 && line[i..].starts_with(['.', ')']));
    if line.starts_with(['#', '>', '+', '-', '=']) || ordered {
        let at = if ordered {
            line.find(|c: char| !c.is_ascii_digit()).unwrap_or(0)
        } else {
            0
        };
        format!("{}\\{}", &line[..at], &line[at..])
    } else {
        line.to_string()
    }
}

/// The lines of one overview paragraph as markdown. Comments are
/// hard-wrapped, so a wrapped line can happen to start with `+ `, `- `,
/// `1. `, `#` or `>` and would then open a list, heading or quote
/// mid-sentence. A list is real when it opens the paragraph, follows a
/// line ending in `:`, or continues a list; any other such line start is
/// escaped.
pub(super) fn overview_paragraph(lines: &[String]) -> String {
    let marker_len = |line: &str| -> Option<usize> {
        if line.starts_with(['+', '-', '*']) && line[1..].starts_with(' ') {
            return Some(1);
        }
        let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        (digits > 0
            && line[digits..].starts_with(['.', ')'])
            && line[digits + 1..].starts_with(' '))
        .then_some(digits + 1)
    };
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut in_list = false;
    for (i, line) in lines.iter().enumerate() {
        let prev_opens_list = i == 0 || out[i - 1].trim_end().ends_with(':');
        match marker_len(line) {
            Some(_) if in_list || prev_opens_list => {
                in_list = true;
                out.push(line.clone());
            }
            Some(len) => {
                let at = len - 1;
                out.push(format!("{}\\{}", &line[..at], &line[at..]));
            }
            None if i > 0 && line.starts_with(['#', '>']) => out.push(format!("\\{line}")),
            None => out.push(line.clone()),
        }
    }
    out.join("\n")
}

/// The ruleset's top-level `facts:` block, verbatim, if it has one.
fn facts_block(yaml_text: &str) -> Option<String> {
    let mut lines = yaml_text.lines().skip_while(|l| l.trim_end() != "facts:");
    let first = lines.next()?;
    let mut block = vec![first.to_string()];
    for line in lines {
        if !line.is_empty() && !line.starts_with(' ') && !line.starts_with('#') {
            break;
        }
        block.push(line.trim_end().to_string());
    }
    while block
        .last()
        .is_some_and(|l| l.is_empty() || l.starts_with('#'))
    {
        block.pop();
    }
    Some(block.join("\n"))
}

/// A rule's message as one line, fit for a table cell.
fn table_cell(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('|', "\\|")
}

/// Build one searchable, honest ruleset description. The ruleset name leads,
/// so it can never be removed by a cap; a long author-written overview is
/// shortened with an explicit ellipsis rather than presented as a complete but
/// grammatically broken sentence.
pub(super) fn ruleset_meta_description(name: &str, overview_md: &str) -> String {
    const MAX_CHARS: usize = 158;

    let lead = format!("{name}@v1 bundled alint ruleset");
    let opening = strip_markup(&first_overview_sentence(overview_md));
    let opening = meta_desc_clean(&opening, usize::MAX);
    if opening.chars().count() < 25 {
        return format!("{lead}.");
    }

    // Reserve `: ` plus a final period in case the source sentence has none.
    let budget = MAX_CHARS.saturating_sub(lead.chars().count() + 3);
    let summary = truncate_meta_summary(&opening, budget);
    let punctuation = if summary.ends_with('.') || summary.ends_with('!') || summary.ends_with('?')
    {
        ""
    } else {
        "."
    };
    format!("{lead}: {summary}{punctuation}")
}

/// Render the markdown body for a single bundled ruleset. The
/// page has these sections, in order:
///
/// 1. **Overview**: the leading comment block from the YAML,
///    rendered as prose (inline YAML samples become fenced
///    ```yaml``` blocks).
/// 2. **Adopt with**: a copy-pasteable `extends:` snippet,
///    suppressed when the overview already contains an
///    `alint://bundled/...` reference (the layered-overlay case,
///    where the author documents a multi-ruleset recipe).
/// 3. **What it checks**: one table row per rule (id, level, the
///    message), and which rules are gated on a fact.
/// 4. **Rules**: per rule, the author's comments from the YAML as
///    prose, then kind / level / when / fix / policy, the message,
///    and the rule's own definition. Each `kind` links into the
///    rule reference (`/docs/rules/<family>/<kind>/`) when
///    `kind_to_family` knows about it.
/// 5. **Facts**: the `facts:` block the `when:` clauses read.
/// 6. **Customize**: overriding and filtering this ruleset's
///    rules, using its own rule ids.
/// 7. **Source**: a permalink to the YAML in the alint repo.
///
/// Kinds not in `kind_to_family` (e.g. a brand-new kind missing
/// from rules.md) render as plain code; the rules-pages generator
/// emits a warning in that case so the gap surfaces.
pub(super) fn render_ruleset_page(
    name: &str,
    overview_md: &str,
    yaml_text: &str,
    rel_repo_path: &str,
    yaml: &ayaml::Value,
    kind_to_family: &std::collections::HashMap<String, String>,
) -> String {
    let mut out = String::new();
    let _ = writeln!(&mut out, "---");
    let _ = writeln!(&mut out, "title: '{name}@v1'");
    let ruleset_desc = ruleset_meta_description(name, overview_md);
    let _ = writeln!(
        &mut out,
        "description: '{}'",
        escape_yaml_string(&ruleset_desc)
    );
    let _ = writeln!(&mut out, "---");
    let _ = writeln!(&mut out);

    if !overview_md.is_empty() {
        out.push_str(overview_md);
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out);
    }

    let overview_has_adoption = overview_md.contains("alint://bundled/");
    if !overview_has_adoption {
        let _ = writeln!(&mut out, "## Adopt with");
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out, "```yaml");
        let _ = writeln!(&mut out, "extends:");
        let _ = writeln!(&mut out, "  - alint://bundled/{name}@v1");
        let _ = writeln!(&mut out, "```");
        let _ = writeln!(&mut out);
    }

    let rules = yaml
        .get("rules")
        .and_then(|r| r.as_sequence())
        .filter(|r| !r.is_empty());
    let Some(rules) = rules else {
        let _ = writeln!(&mut out, "_(No rules: this ruleset is a placeholder.)_");
        let _ = writeln!(&mut out);
        write_ruleset_source(&mut out, rel_repo_path);
        return out;
    };

    write_ruleset_summary(&mut out, rules);
    let sources: std::collections::HashMap<String, RuleSource> = rule_sources(yaml_text)
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect();
    let _ = writeln!(&mut out, "## Rules");
    let _ = writeln!(&mut out);
    for rule in rules {
        let id = str_field(rule, "id").unwrap_or_else(|| "(no-id)".into());
        write_rule_section(&mut out, &id, rule, sources.get(&id), kind_to_family);
    }

    if let Some(facts) = facts_block(yaml_text) {
        let _ = writeln!(&mut out, "## Facts");
        let _ = writeln!(&mut out);
        let _ = writeln!(
            &mut out,
            "The `when:` clauses above read these facts. Each is resolved once per run; \
             [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository."
        );
        let _ = writeln!(&mut out);
        let _ = writeln!(&mut out, "```yaml");
        let _ = writeln!(&mut out, "{facts}");
        let _ = writeln!(&mut out, "```");
        let _ = writeln!(&mut out);
    }

    write_ruleset_customize(&mut out, name, rules);
    write_ruleset_source(&mut out, rel_repo_path);
    out
}

/// A string field of a rule mapping.
fn str_field(rule: &ayaml::Value, key: &str) -> Option<String> {
    rule.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

/// The "What it checks" table: one row per rule (its id over its level, and
/// the first sentence of its message, or its kind's summary when it has
/// none; the full message is in the rule's section), then which rules a
/// `when:` fact gates. Two columns, and the ids as plain link text (which
/// wraps at its hyphens, where a code span wouldn't), so the table fits the
/// content width.
fn write_ruleset_summary(out: &mut String, rules: &[ayaml::Value]) {
    let _ = writeln!(out, "## What it checks");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{} {}. Each links to its section below, which explains the check and shows its definition.",
        rules.len(),
        if rules.len() == 1 { "rule" } else { "rules" }
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "| Rule | Reports |");
    let _ = writeln!(out, "| --- | --- |");
    let mut gates: Vec<(String, usize)> = Vec::new();
    for rule in rules {
        let id = str_field(rule, "id").unwrap_or_else(|| "(no-id)".into());
        let level = str_field(rule, "level").unwrap_or_default();
        let reports = str_field(rule, "message")
            .map(|msg| escape_inline(&first_sentence(&msg)))
            .or_else(|| {
                let kind = str_field(rule, "kind")?;
                KIND_SUMMARIES
                    .iter()
                    .find(|(k, _)| *k == kind)
                    .map(|(_, summary)| (*summary).to_string())
                    .or(Some(format!("`{kind}`")))
            })
            .unwrap_or_default();
        let _ = writeln!(
            out,
            "| [{id}](#{anchor})<br>`{level}` | {} |",
            table_cell(&reports),
            anchor = id.to_ascii_lowercase()
        );
        if let Some(when) = str_field(rule, "when") {
            match gates.iter_mut().find(|(w, _)| *w == when) {
                Some((_, n)) => *n += 1,
                None => gates.push((when, 1)),
            }
        }
    }
    let _ = writeln!(out);
    for (when, n) in &gates {
        let sentence = match (*n == rules.len(), *n) {
            (true, 1) => format!(
                "The rule runs only when `{when}` holds, so the ruleset stays quiet in \
                 repositories it doesn't apply to."
            ),
            (true, n) => format!(
                "All {n} rules run only when `{when}` holds, so the ruleset stays quiet in \
                 repositories it doesn't apply to."
            ),
            (false, 1) => format!("One rule runs only when `{when}` holds."),
            (false, n) => format!("{n} rules run only when `{when}` holds."),
        };
        let _ = writeln!(out, "{sentence}");
        let _ = writeln!(out);
    }
}

/// One rule's section: the author's notes, its kind / level / when / fix /
/// policy, its message, and its definition.
fn write_rule_section(
    out: &mut String,
    id: &str,
    rule: &ayaml::Value,
    source: Option<&RuleSource>,
    kind_to_family: &std::collections::HashMap<String, String>,
) {
    // alint.org links each bundled rule to this heading by matching
    // `### `<id>`` (src/lib/examples.ts), so the shape is a contract.
    let _ = writeln!(out, "### `{id}`");
    let _ = writeln!(out);
    if let Some(notes) = source.map(|s| s.notes_md.trim()).filter(|n| !n.is_empty()) {
        let _ = writeln!(out, "{notes}");
        let _ = writeln!(out);
    }
    if let Some(kind) = str_field(rule, "kind") {
        let kind_md = match kind_to_family.get(&kind) {
            Some(family) => format!("[`{kind}`](/docs/rules/{family}/{kind}/)"),
            None => format!("`{kind}`"),
        };
        let _ = writeln!(out, "- **kind**: {kind_md}");
    }
    if let Some(level) = str_field(rule, "level") {
        let _ = writeln!(out, "- **level**: `{level}`");
    }
    if let Some(when) = str_field(rule, "when") {
        let _ = writeln!(out, "- **when**: `{when}`");
    }
    if let Some(fix) = rule.get("fix").and_then(|f| f.as_mapping()) {
        let ops: Vec<String> = fix
            .keys()
            .filter_map(|k| k.as_str())
            .map(|k| format!("`{k}`"))
            .collect();
        if !ops.is_empty() {
            let _ = writeln!(
                out,
                "- **fix**: {} (applied by `alint fix`)",
                ops.join(", ")
            );
        }
    }
    if let Some(policy) = str_field(rule, "policy_url") {
        let _ = writeln!(out, "- **policy**: <{policy}>");
    }
    if let Some(msg) = str_field(rule, "message") {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "> {}",
            escape_inline(&msg.split_whitespace().collect::<Vec<_>>().join(" "))
        );
    }
    if let Some(def) = source
        .map(|s| s.definition.as_str())
        .filter(|d| !d.is_empty())
    {
        let _ = writeln!(out);
        let _ = writeln!(out, "```yaml");
        let _ = writeln!(out, "{def}");
        let _ = writeln!(out, "```");
    }
    let _ = writeln!(out);
}

/// The "Customize" section, with this ruleset's own ids: the first rule
/// turned off, and the next rule that isn't already an error promoted to
/// one (left out when every other rule is an error).
fn write_ruleset_customize(out: &mut String, name: &str, rules: &[ayaml::Value]) {
    let first = str_field(&rules[0], "id").unwrap_or_default();
    let promote = rules[1..]
        .iter()
        .find(|r| str_field(r, "level").as_deref() != Some("error"))
        .and_then(|r| str_field(r, "id"));
    let _ = writeln!(out, "## Customize");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Every rule here can be overridden by id from your own `.alint.yml`: \
         change its `level`, or set `level: off` to drop it. An id that doesn't \
         exist is an error at config load, so a typo can't silently pass."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "```yaml");
    let _ = writeln!(out, "extends:");
    let _ = writeln!(out, "  - alint://bundled/{name}@v1");
    let _ = writeln!(out, "rules:");
    let _ = writeln!(out, "  - id: {first}");
    let _ = writeln!(out, "    level: off");
    if let Some(promote) = promote {
        let _ = writeln!(out, "  - id: {promote}");
        let _ = writeln!(out, "    level: error");
    }
    let _ = writeln!(out, "```");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "```yaml");
    let _ = writeln!(out, "extends:");
    let _ = writeln!(out, "  - url: alint://bundled/{name}@v1");
    let _ = writeln!(out, "    except: [{first}]");
    let _ = writeln!(out, "```");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning \
         and how rulesets combine."
    );
    let _ = writeln!(out);
}

fn write_ruleset_source(out: &mut String, rel_repo_path: &str) {
    let _ = writeln!(out, "## Source");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "The full ruleset definition, comments included, is committed at \
         [`{rel_repo_path}`]({ALINT_REPO_BLOB_URL}/{rel_repo_path}) in the alint repo.",
    );
}
/// One markdown page per `crates/alint-dsl/rulesets/v1/**/*.yml`,
/// summarising the ruleset's rules with their level / kind / message
/// / policy URL. Slash-separated names (`hygiene/lockfiles`,
/// `ci/github-actions`) are flattened with a `-` for the bundle
/// filename so Starlight's autogen sidebar produces a flat list.
///
/// Each page now also carries:
/// - An overview parsed from the YAML's leading comment block
///   (the natural-language description the ruleset author wrote
///   above `version: 1`).
/// - A `## Source` section with a link back to the canonical YAML
///   in the alint repo plus the full file embedded as a fenced
///   code block, so readers can see the exact rule definitions
///   without leaving the docs.
pub(super) fn generate_bundled_ruleset_pages(
    workspace: &Path,
    target_dir: &Path,
    kind_to_family: &std::collections::HashMap<String, String>,
) -> Result<()> {
    struct RulesetEntry {
        pretty: String,
        flat_slug: String,
        summary: String,
    }

    let rulesets_root = workspace.join(docs_paths::RULESETS_DIR);
    let bundled_dir = target_dir.join("bundled-rulesets");
    fs::create_dir_all(&bundled_dir)?;

    let mut entries: Vec<RulesetEntry> = Vec::new();

    for entry in walkdir_plain(&rulesets_root)? {
        let md = fs::metadata(&entry)?;
        if !md.is_file() {
            continue;
        }
        let ext = entry.extension().and_then(|s| s.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        let rel = entry.strip_prefix(&rulesets_root).unwrap();
        let pretty_name = rel.with_extension("");
        let pretty_str = pretty_name.to_string_lossy().replace('\\', "/");
        let flat_slug = pretty_str.replace('/', "-");
        let flat_filename = format!("{flat_slug}.md");

        let yaml_text =
            fs::read_to_string(&entry).with_context(|| format!("reading {}", entry.display()))?;
        let yaml: ayaml::Value = ayaml::compat::from_str(&yaml_text)
            .with_context(|| format!("parsing {}", entry.display()))?;

        // Repo-relative path for the source link, always forward-
        // slashed regardless of host OS so the URL is portable.
        let rel_path_str = rel.to_string_lossy().replace('\\', "/");
        let rel_repo_path = format!("{}/{}", docs_paths::RULESETS_DIR, rel_path_str);

        let overview_md = render_overview_from_comments(&yaml_text);
        let summary = first_overview_sentence(&overview_md);

        let page = render_ruleset_page(
            &pretty_str,
            &overview_md,
            &yaml_text,
            &rel_repo_path,
            &yaml,
            kind_to_family,
        );
        let dest = bundled_dir.join(&flat_filename);
        fs::write(&dest, page).with_context(|| format!("writing {}", dest.display()))?;

        entries.push(RulesetEntry {
            pretty: pretty_str,
            flat_slug,
            summary,
        });
    }

    // An index page listing every ruleset — overwrites the hand-
    // written placeholder when the sync script lays the bundle into
    // alint.org. Each entry shows a one-line summary (the first
    // sentence of the ruleset's leading comment block) so the
    // index is scannable without opening every page.
    entries.sort_by(|a, b| a.pretty.cmp(&b.pretty));
    let mut index = String::new();
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index, "title: Bundled Rulesets");
    let _ = writeln!(
        &mut index,
        "description: One-line ecosystem baselines built into the alint binary."
    );
    let _ = writeln!(&mut index, "sidebar:");
    let _ = writeln!(&mut index, "  order: 1");
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index);
    let _ = writeln!(
        &mut index,
        "Adopt with `extends: [alint://bundled/<name>@v1]`. Each ruleset's full rule list lives on its own page below."
    );
    let _ = writeln!(&mut index);
    let _ = writeln!(&mut index, "## Currently shipped");
    let _ = writeln!(&mut index);
    for e in &entries {
        if e.summary.is_empty() {
            let _ = writeln!(
                &mut index,
                "- [`{name}@v1`](/docs/bundled-rulesets/{slug}/)",
                name = e.pretty,
                slug = e.flat_slug
            );
        } else {
            let _ = writeln!(
                &mut index,
                "- [`{name}@v1`](/docs/bundled-rulesets/{slug}/) — {summary}",
                name = e.pretty,
                slug = e.flat_slug,
                summary = e.summary,
            );
        }
    }
    fs::write(bundled_dir.join("index.md"), index)?;

    Ok(())
}

/// Parse the leading comment block of a ruleset YAML into
/// markdown. The first line is expected to be the canonical
/// `# alint://bundled/<name>@<rev>` URI tag and is stripped.
/// Subsequent comment lines are emitted as paragraphs (preserving
/// the author's line breaks so list items render correctly) or,
/// when a paragraph starts with a 4-space indent, as a fenced
/// `yaml` code block — that's the convention authors use for
/// the "here's the `extends:` snippet" mini-blocks.
///
/// Reading stops at the first non-comment, non-blank line. By
/// convention the rule body starts right after the leading
/// comment block, so this naturally captures only the file's
/// top-of-file description.
pub(crate) fn render_overview_from_comments(yaml_text: &str) -> String {
    enum Block {
        Para(Vec<String>),
        Code(Vec<String>),
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut comment_started = false;
    // Treat start-of-input as a paragraph break so the very first
    // non-blank comment line opens a new block.
    let mut paragraph_break = true;

    for raw in yaml_text.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            if !comment_started {
                continue;
            }
            // A literal blank line (no `#`) ends the leading
            // comment block. Authors use blank `#` lines for
            // paragraph breaks INSIDE the block — those are
            // handled below.
            break;
        }
        if !line.starts_with('#') {
            break;
        }
        comment_started = true;

        // Strip the `#` marker and exactly one trailing space.
        let after_hash = &line[1..];
        let body = after_hash.strip_prefix(' ').unwrap_or(after_hash);

        if body.is_empty() {
            paragraph_break = true;
            continue;
        }
        // Skip the canonical `# alint://bundled/<name>@<rev>` URI
        // header — it's metadata, not prose.
        if body.starts_with("alint://bundled/") {
            continue;
        }

        // 4-space indent at the START of a block = code block.
        // Continuation lines inside an existing block keep that
        // block's kind regardless of their own indent (so
        // bulleted lists with hanging-indent continuations
        // stay in a single Para block).
        if paragraph_break {
            if let Some(rest) = body.strip_prefix("    ") {
                blocks.push(Block::Code(vec![rest.to_string()]));
            } else {
                blocks.push(Block::Para(vec![body.to_string()]));
            }
        } else {
            match blocks
                .last_mut()
                .expect("paragraph_break=false implies a current block exists")
            {
                Block::Para(lines) => lines.push(body.to_string()),
                Block::Code(lines) => {
                    // Inside a code block, dedent up to 4 spaces so
                    // the rendered code matches the opening line's
                    // visual indentation.
                    let dedented = body.strip_prefix("    ").unwrap_or(body);
                    lines.push(dedented.to_string());
                }
            }
        }
        paragraph_break = false;
    }

    let mut out = String::new();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        match b {
            Block::Para(lines) => out.push_str(&overview_paragraph(lines)),
            Block::Code(lines) => {
                out.push_str("```yaml\n");
                for l in lines {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push_str("```");
            }
        }
    }
    out
}

/// First-sentence summary of a rendered overview, used to
/// populate the bundled-rulesets index page. Skips fenced code
/// blocks, takes the first paragraph of natural-language prose,
/// and truncates at the first sentence-ending `. ` boundary.
pub(crate) fn first_overview_sentence(overview_md: &str) -> String {
    let mut paragraph = String::new();
    let mut in_code = false;
    for line in overview_md.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(trimmed);
    }
    if let Some(idx) = paragraph.find(". ") {
        paragraph.truncate(idx + 1);
    }
    paragraph.trim().to_string()
}
