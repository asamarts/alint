//! The generated `/docs/bundled-rulesets/<name>/` pages: what each bundled
//! ruleset checks, the reasoning its authors left as comments in the YAML, the
//! facts its `when:` clauses read, and how to customize it.
use std::fmt::Write as _;

use alint_rules::kind_docs::KIND_SUMMARIES;

use super::{
    escape_yaml_string, first_sentence, render_overview_from_comments, ruleset_meta_description,
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
/// Comments nested deeper, inside a list value, stay out of the notes,
/// and so do paragraphs that open with a release tag (`v0.9.18: `).
///
/// Rulesets are written with rules at a two-space indent (`  - id: x`);
/// the catalogue test checks that every rule serde sees is found here.
pub(super) fn rule_sources(yaml_text: &str) -> Vec<RuleSource> {
    fn notes(comment_lines: &[String]) -> String {
        let mut as_yaml = String::new();
        for line in comment_lines {
            let _ = writeln!(&mut as_yaml, "{line}");
        }
        let md = render_overview_from_comments(&as_yaml);
        strip_release_tag(&md)
    }

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
                notes_md: notes(&comments),
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
        if !line.is_empty() && !line.starts_with(' ') {
            // The next top-level key ends the rules block.
            break;
        }
        if let Some(rest) = line.strip_prefix("  - id:") {
            finish(&mut current, &mut out);
            let id = rest.trim().trim_matches(['"', '\'']).to_string();
            let comments = std::mem::take(&mut pending);
            current = Some((id, comments, vec![line[2..].to_string()]));
            continue;
        }
        if let Some(comment) = line.strip_prefix("  #") {
            // A comment at the list indent belongs to the rule below it.
            finish(&mut current, &mut out);
            if comment.trim_start().starts_with("---") {
                pending.clear();
            } else {
                pending.push(format!("#{comment}"));
            }
            continue;
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
                comments.push(format!("#{comment}"));
            } else if line.trim_start().starts_with('#') {
                // A comment inside a value: part of neither.
            } else {
                def.push(line.strip_prefix("  ").unwrap_or(line).to_string());
            }
        }
    }
    finish(&mut current, &mut out);
    out
}

/// Drop the paragraphs of rule notes that open with a release tag
/// (`v0.9.18: ...`, `v0.10 — ...`). Those record how a rule changed,
/// which belongs in the changelog rather than on the reference page.
fn strip_release_tag(md: &str) -> String {
    let is_tagged = |para: &str| {
        let Some(rest) = para.strip_prefix('v') else {
            return false;
        };
        let ver_len = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        let (ver, tail) = rest.split_at(ver_len);
        ver.contains('.')
            && ver.starts_with(|c: char| c.is_ascii_digit())
            && [":", " —", " -", " ("]
                .iter()
                .any(|sep| tail.starts_with(sep))
    };
    md.split("\n\n")
        .filter(|para| !is_tagged(para))
        .collect::<Vec<_>>()
        .join("\n\n")
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
    yaml: &serde_yaml_ng::Value,
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
        let _ = writeln!(&mut out, "_(no rules — this ruleset is a placeholder.)_");
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
fn str_field(rule: &serde_yaml_ng::Value, key: &str) -> Option<String> {
    rule.get(key).and_then(|v| v.as_str()).map(str::to_string)
}

/// The "What it checks" table: one row per rule (its id over its level, and
/// the first sentence of its message, or its kind's summary when it has
/// none; the full message is in the rule's section), then which rules a
/// `when:` fact gates. Two columns, and the ids as plain link text (which
/// wraps at its hyphens, where a code span wouldn't), so the table fits the
/// content width.
fn write_ruleset_summary(out: &mut String, rules: &[serde_yaml_ng::Value]) {
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
            .map(|msg| first_sentence(&msg))
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
        let subject = match (*n == rules.len(), *n) {
            (true, 1) => "The rule runs".to_string(),
            (true, n) => format!("All {n} rules run"),
            (false, 1) => "One rule runs".to_string(),
            (false, n) => format!("{n} rules run"),
        };
        let _ = writeln!(
            out,
            "{subject} only when `{when}` holds, so the ruleset stays quiet in repositories it doesn't apply to."
        );
        let _ = writeln!(out);
    }
}

/// One rule's section: the author's notes, its kind / level / when / fix /
/// policy, its message, and its definition.
fn write_rule_section(
    out: &mut String,
    id: &str,
    rule: &serde_yaml_ng::Value,
    source: Option<&RuleSource>,
    kind_to_family: &std::collections::HashMap<String, String>,
) {
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
            msg.split_whitespace().collect::<Vec<_>>().join(" ")
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
fn write_ruleset_customize(out: &mut String, name: &str, rules: &[serde_yaml_ng::Value]) {
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
