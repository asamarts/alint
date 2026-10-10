//! Hover markdown, rule-reference links, and stderr note blocks.

use std::path::Path;

use crate::Finding;
use crate::diagnostics::severity_label;

/// Markdown hover body for one finding: rule id + severity, the
/// message, the rule kind's description, fix availability, and links to
/// the rule reference and (when declared) the rule's policy.
pub(crate) fn render_finding(f: &Finding) -> String {
    let mut s = format!(
        "**alint** · `{}` ({})\n\n{}",
        f.rule_id,
        severity_label(f.severity),
        f.message
    );
    if let Some(description) = f.description {
        s.push_str("\n\n");
        s.push_str(description);
    }
    s.push_str(if f.fixable {
        "\n\nFix: available (quick-fix, or `alint fix`)"
    } else {
        "\n\nFix: none (this rule has no auto-fix)"
    });
    let mut links: Vec<String> = Vec::new();
    if let Some(url) = &f.docs_url {
        links.push(format!("[Rule reference →]({url})"));
    }
    if let Some(url) = &f.policy_url {
        links.push(format!("[Policy →]({url})"));
    }
    if !links.is_empty() {
        s.push_str("\n\n");
        s.push_str(&links.join(" · "));
    }
    s
}

/// Resolve an alias kind spelling to its canonical kind (identity for a
/// canonical or unknown kind) — the kind that owns the reference page.
fn canonical_kind(kind: &str) -> &str {
    alint_rules::categories::ALIAS_TO_CANONICAL
        .iter()
        .find(|(alias, _)| *alias == kind)
        .map_or(kind, |(_, canonical)| canonical)
}

/// The alint.org rule-reference page for a rule kind — the same URL
/// `alint explain` prints (family = the kind's primary category).
pub(crate) fn rule_docs_url(kind: &str) -> Option<String> {
    let canonical = canonical_kind(kind);
    let family = alint_rules::categories::KIND_CATEGORIES
        .iter()
        .find(|(k, _)| *k == canonical)
        .and_then(|(_, cats)| cats.first())?
        .slug();
    Some(format!(
        "https://alint.org/docs/rules/{family}/{canonical}/"
    ))
}

/// The rule kind's one-sentence description (as `alint explain` shows).
pub(crate) fn kind_description(kind: &str) -> Option<&'static str> {
    let canonical = canonical_kind(kind);
    alint_rules::kind_docs::KIND_DESCRIPTIONS
        .iter()
        .find(|(k, _)| *k == canonical)
        .map(|(_, d)| *d)
        .filter(|d| !d.is_empty())
}

/// The stderr block for a config's informational notes: a one-line
/// count by default, the full list with `--show-notes`. Empty when there
/// are no notes. Control characters are escaped so a note can't inject
/// terminal escapes into a log.
pub(crate) fn render_notes(config: &Path, notes: &[String], show_notes: bool) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let config = escape_controls(&config.display().to_string());
    if show_notes {
        let mut block = format!("alint: {} informational note(s) ({config}):", notes.len());
        for note in notes {
            block.push_str("\n  note: ");
            block.push_str(&escape_controls(note));
        }
        block
    } else {
        format!(
            "alint: {} informational note(s) ({config}); run `alint lsp --show-notes` to list.",
            notes.len()
        )
    }
}

/// Escape control characters (C0/C1, incl. ESC) as `\u{..}`.
fn escape_controls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_control() {
            out.extend(c.escape_default());
        } else {
            out.push(c);
        }
    }
    out
}
