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

/// Enforce that no synced page carries a RAW Unicode character that renders
/// invisibly or reorders surrounding text - a Trojan-Source vector, and doubly
/// embarrassing on the very page documenting `no_bidi_controls`. The docs-export
/// generator tokenises these to a visible `<U+XXXX>` when embedding fixture
/// content, config, file trees, git history, and captured output; this gate is
/// the invariant that keeps ANY render path (present or future) from smuggling
/// one onto alint.org. Scans every `.md` in the bundle (hand-authored +
/// generated), the coverage `family_index::check_ascii` (family `index.md` only)
/// and `check_titles_no_backticks` (frontmatter titles only) never had.
///
/// The forbidden set is `is_dangerous_docs_char` minus two deliberate raw
/// chars: `ESC` (U+001B), which introduces the SGR colour codes in the captured
/// `ansi` blocks, and `\r`, so a CRLF checkout of a hand-authored page does not
/// false-positive.
pub(crate) fn check_no_invisible_controls(target_dir: &Path) -> Result<()> {
    use crate::docs_export::is_dangerous_docs_char;
    let forbidden = |c: char| is_dangerous_docs_char(c) && c != '\u{1b}' && c != '\r';
    let mut offenders: Vec<String> = Vec::new();
    for path in walkdir_plain(target_dir)? {
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        for (lineno, line) in text.lines().enumerate() {
            for (col, c) in line.chars().enumerate() {
                if forbidden(c) {
                    let rel = path.strip_prefix(target_dir).unwrap_or(&path);
                    offenders.push(format!(
                        "{}:{}:{} U+{:04X}",
                        rel.display(),
                        lineno + 1,
                        col + 1,
                        c as u32
                    ));
                }
            }
        }
    }
    if !offenders.is_empty() {
        offenders.sort();
        bail!(
            "synced page(s) carry a raw invisible / bidirectional control character (a \
             Trojan-Source vector) that must be escaped to a visible <U+XXXX> token before \
             publishing: {offenders:?}"
        );
    }
    Ok(())
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

    #[test]
    fn check_no_invisible_controls_flags_raw_bidi_but_allows_ansi_esc() {
        let dir = tempfile::tempdir().unwrap();
        let rules = dir.path().join("rules/foo");
        fs::create_dir_all(&rules).unwrap();

        // A clean page - including an ```ansi block carrying real ESC SGR codes,
        // the one deliberate raw control char - passes.
        fs::write(
            rules.join("clean.md"),
            "---\ntitle: ok\n---\nbody\n\n```ansi\n\u{1b}[31merror\u{1b}[0m at src\n```\n",
        )
        .unwrap();
        assert!(check_no_invisible_controls(dir.path()).is_ok());

        // A raw bidi override ANYWHERE (here in a per-kind page body, the exact
        // surface `check_ascii` never scanned) fails, naming the char + location.
        fs::write(
            rules.join("evil.md"),
            "---\ntitle: ok\n---\nbefore\u{202e}after\n",
        )
        .unwrap();
        let err = check_no_invisible_controls(dir.path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("U+202E"), "{err}");
        assert!(err.contains("evil.md"), "{err}");
    }
}
