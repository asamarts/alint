//! `xtask` rule-family overview page generator.
//!
//! Each `/docs/rules/<family>/` index page is a table of the family's rule
//! kinds (name linked to its page + a one-line description), generated from
//! `docs/rules.md` (the SSOT, via each kind's first sentence) so it can't drift
//! from the rule pages. No per-family diagram: the table already lists every
//! kind, so the `LikeC4` family view would just duplicate it. Descriptions are
//! ASCII-dashed and pipe-escaped, and `check_ascii` enforces, across all family
//! pages at once, that none carries an em dash.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::docs_export::{RuleEntry, escape_yaml_string};

/// Render a family overview page: frontmatter, a lede, then the rule table.
pub(crate) fn render(
    family_title: &str,
    family_order: u32,
    family_slug: &str,
    rules: &[RuleEntry],
) -> String {
    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: '{}'", escape_yaml_string(family_title));
    let _ = writeln!(
        &mut page,
        "description: 'Rule reference: the {} family.'",
        family_title.to_lowercase()
    );
    let _ = writeln!(&mut page, "sidebar:");
    let _ = writeln!(&mut page, "  order: {family_order}");
    let _ = writeln!(&mut page, "  label: '{}'", escape_yaml_string(family_title));
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    let _ = writeln!(
        &mut page,
        "Rule kinds in the **{family_title}** family. Each rule below links to its own page with options, an example, and any auto-fix support."
    );
    let _ = writeln!(&mut page);
    let _ = writeln!(&mut page, "| Rule | Description |");
    let _ = writeln!(&mut page, "| --- | --- |");
    for r in rules {
        let _ = writeln!(
            &mut page,
            "| [`{kind}`](/docs/rules/{family_slug}/{kind}/) | {summary} |",
            kind = r.kind,
            summary = escape_table_cell(&ascii_dashes(&r.summary)),
        );
    }
    page
}

/// Replace em/en dashes with a comma. The project voice avoids them (they read
/// as an "AI tell"), so the generated overview pages must stay ASCII-clean.
fn ascii_dashes(s: &str) -> String {
    s.replace(" — ", ", ")
        .replace(" – ", ", ")
        .replace(['—', '–'], ",")
}

/// Escape a value for a Markdown table cell: a literal `|` would split the row.
/// GFM renders `\|` as a literal pipe, even inside a code span.
fn escape_table_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Enforce that every generated rule-family overview page
/// (`rules/<family>/index.md`) is free of em/en dashes, consistently across all
/// families. `render` ASCII-izes its descriptions, so this guards against a
/// regression (a future field forwarding raw `rules.md` prose, or the stripping
/// being dropped) on every family page at once. Scoped to the family `index.md`
/// files; the per-kind rule pages keep their full prose.
pub(crate) fn check_ascii(target_dir: &Path) -> Result<()> {
    let rules_dir = target_dir.join("rules");
    let mut offenders: Vec<String> = Vec::new();
    for entry in
        fs::read_dir(&rules_dir).with_context(|| format!("read_dir {}", rules_dir.display()))?
    {
        let dir = entry?.path();
        if !dir.is_dir() {
            continue;
        }
        let index = dir.join("index.md");
        if index.exists() {
            let text = fs::read_to_string(&index)?;
            if text.contains('—') || text.contains('–') {
                offenders.push(
                    dir.file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    if !offenders.is_empty() {
        offenders.sort();
        bail!(
            "rule-family overview pages contain em/en dashes (must be ASCII): {offenders:?}. \
             family_index::render strips them; check for a new field forwarding raw rules.md prose."
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The family overview page is a table (not a list), carries no `LikeC4`
    /// diagram, is free of em/en dashes, and escapes literal pipes in summaries.
    #[test]
    fn render_is_a_dash_free_table() {
        let rules = vec![
            RuleEntry {
                kind: "file_exists".into(),
                summary: "Every glob match in `paths` must exist.".into(),
            },
            RuleEntry {
                kind: "no_merge_conflict_markers".into(),
                summary: "Flag `||||||| ` markers — left over from a merge.".into(),
            },
        ];
        let md = render("Existence", 1, "existence", &rules);
        assert!(md.contains("| Rule | Description |"));
        assert!(md.contains("| --- | --- |"));
        assert!(md.contains("| [`file_exists`](/docs/rules/existence/file_exists/) |"));
        assert!(!md.contains("<likec4-view"), "diagram must be gone");
        assert!(!md.contains('—') && !md.contains('–'), "no em/en dashes");
        assert!(!md.contains("- [`"), "must be a table, not a list");
        assert!(md.contains("\\|"), "summary pipes are escaped");
        assert!(
            !md.contains("||||||| "),
            "a raw pipe run in a cell would break the table"
        );
    }

    #[test]
    fn ascii_dashes_and_pipe_escaping() {
        assert_eq!(
            ascii_dashes("not bytes — code points"),
            "not bytes, code points"
        );
        assert_eq!(ascii_dashes("a — b — c"), "a, b, c");
        assert!(!ascii_dashes("x — y").contains('—'));
        assert_eq!(escape_table_cell("a|b|c"), "a\\|b\\|c");
    }
}
