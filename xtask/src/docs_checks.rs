//! `xtask` docs-bundle content guards, run in `docs-export --check`.
//!
//! Cross-cutting checks over the generated docs bundle that catch content which
//! renders wrong on alint.org regardless of which generator produced the page.
//! (The per-family em-dash gate lives with its generator in `family_index`.)

use std::fs;
use std::path::Path;

use anyhow::{Result, bail};

use crate::walkdir_plain;

/// Enforce that no synced page carries a Markdown backtick in its frontmatter
/// `title:`. A title is plain-text chrome (sidebar, breadcrumb, prev/next
/// pagination, the browser tab), so a backtick renders literally there instead
/// of as code. Scans every `.md` in the bundle (hand-authored + generated).
pub(crate) fn check_titles_no_backticks(target_dir: &Path) -> Result<()> {
    let mut offenders: Vec<String> = Vec::new();
    for path in walkdir_plain(target_dir)? {
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        if let Some(title) = frontmatter_title(&text) {
            if title.contains('`') {
                let rel = path.strip_prefix(target_dir).unwrap_or(&path);
                offenders.push(format!("{} ({title})", rel.display()));
            }
        }
    }
    if !offenders.is_empty() {
        offenders.sort();
        bail!(
            "synced page title(s) contain a Markdown backtick, which renders literally in the \
             sidebar / breadcrumb / pagination / browser tab: {offenders:?}. Titles are plain \
             text; remove the backticks."
        );
    }
    Ok(())
}

/// The raw `title:` value from a Markdown file's YAML frontmatter (the first
/// `---`-delimited block), trimmed (quotes left intact).
fn frontmatter_title(text: &str) -> Option<String> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    rest[..end]
        .lines()
        .find_map(|l| l.strip_prefix("title:").map(|v| v.trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_title_extracts_value_not_body() {
        let backticked = "---\ntitle: '`content_from:` for fix ops'\ndescription: x\n---\nbody";
        assert!(frontmatter_title(backticked).unwrap().contains('`'));

        // Body backticks are not the title.
        let clean = "---\ntitle: Drop-in configs\n---\nuses `code` in the body";
        let t = frontmatter_title(clean).unwrap();
        assert_eq!(t, "Drop-in configs");
        assert!(!t.contains('`'));

        assert_eq!(frontmatter_title("no frontmatter here"), None);
    }
}
