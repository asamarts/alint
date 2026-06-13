//! `xtask gen-public-roadmap` — render the public roadmap from
//! canonical `docs/design/ROADMAP.md`.
//!
//! Per `docs/design/v0.11/roadmap_generator.md`. The generator:
//!
//! - Reads canonical ROADMAP.md.
//! - Strips the leading `# alint — Roadmap` H1 (matching the
//!   prior `copy_one` behaviour in `docs_export.rs` so the
//!   integration point swaps in cleanly).
//! - Elides every block delimited by paired
//!   `<!-- alint:internal-start -->` / `<!-- alint:internal-end -->`
//!   HTML-comment markers, with code-fence awareness so example
//!   markers inside fenced blocks stay literal.
//! - Collapses any run of more than two consecutive blank lines
//!   so a stripped section doesn't leave a visible whitespace
//!   chasm.
//! - Prepends a Starlight frontmatter block (`title: <title>`).
//!
//! The same `generate_public_roadmap` function is called from
//! `docs_export::docs_export()` (replacing the prior
//! `copy_one(ROADMAP_DOC, ...)` invocation) and from the
//! `xtask gen-public-roadmap` subcommand.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

const MARKER_START: &str = "<!-- alint:internal-start -->";
const MARKER_END: &str = "<!-- alint:internal-end -->";

/// Read canonical ROADMAP.md at `input`, transform it (strip H1,
/// elide internal-only blocks, collapse blank-line runs, prepend
/// Starlight frontmatter), write to `output`. Creates `output`'s
/// parent directory if missing.
pub fn generate_public_roadmap(input: &Path, output: &Path, title: &str) -> Result<()> {
    let src = fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;
    let rendered =
        transform(&src, title).with_context(|| format!("transforming {}", input.display()))?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(output, rendered).with_context(|| format!("writing {}", output.display()))?;
    Ok(())
}

/// Pure transformation: the testable inner. Takes canonical-shape
/// markdown plus the title to inject into frontmatter; returns the
/// rendered public-roadmap markdown.
pub(crate) fn transform(content: &str, title: &str) -> Result<String> {
    let stripped_h1 = strip_first_h1(content);
    let elided = elide_internal_blocks(stripped_h1)?;
    let collapsed = collapse_excess_blanks(&elided);
    Ok(format!("---\ntitle: {title}\n---\n\n{collapsed}"))
}

/// Strip the first top-level `# heading` line so the Starlight
/// frontmatter `title` doesn't render next to a duplicate H1.
/// Mirrors the helper in `docs_export.rs` so the byte-equivalence
/// migration guard holds with zero markers in canonical.
fn strip_first_h1(body: &str) -> &str {
    let trimmed = body.trim_start();
    if let Some(rest) = trimmed.strip_prefix("# ") {
        if let Some(idx) = rest.find('\n') {
            return rest[idx + 1..].trim_start_matches('\n');
        }
        return "";
    }
    body
}

/// Walk `content` line-by-line; drop lines inside marker-delimited
/// blocks. Code fences are tracked so example markers inside
/// fenced blocks are treated as literal content, not as block
/// delimiters.
///
/// Failure cases that surface as errors:
/// - Nested `<!-- alint:internal-start -->` before the prior one
///   closed.
/// - Orphan `<!-- alint:internal-end -->` with no open block.
/// - Unclosed `<!-- alint:internal-start -->` at end of input.
///
/// Same-line wrapper (`<!-- alint:internal-start -->...<!-- alint:internal-end -->`
/// on one line) is treated as identity: the marker syntax is
/// stripped from the line, any other content on the line is
/// preserved. Per the design doc: if an author wants intra-line
/// elision, the markers must be split onto separate lines.
fn elide_internal_blocks(content: &str) -> Result<String> {
    let mut out = String::with_capacity(content.len());
    let mut inside_internal = false;
    let mut in_code_fence = false;
    let mut start_line: usize = 0;

    for (i, line) in content.lines().enumerate() {
        let line_num = i + 1;
        let trimmed = line.trim_start();
        let is_fence_toggle = trimmed.starts_with("```");

        // Inside or transitioning through a code fence: marker
        // syntax is literal. Fence-toggle lines themselves are
        // content (the fence delimiter belongs to the code block).
        if is_fence_toggle || in_code_fence {
            if !inside_internal {
                out.push_str(line);
                out.push('\n');
            }
            if is_fence_toggle {
                in_code_fence = !in_code_fence;
            }
            continue;
        }

        // Drop the machine-readable `roadmap-public` markers that feed
        // `xtask gen-roadmap`. They are invisible HTML comments, but
        // stripping them keeps the published /docs/about/roadmap/ source
        // clean. Inside a code fence they stay literal (handled above).
        if trimmed.starts_with("<!-- roadmap-public:") {
            continue;
        }

        let has_start = line.contains(MARKER_START);
        let has_end = line.contains(MARKER_END);

        if has_start && has_end {
            // Same-line wrapper — strip the marker syntax, keep
            // the rest. Marker semantics don't open or close a
            // block here.
            if !inside_internal {
                let cleaned = line.replace(MARKER_START, "").replace(MARKER_END, "");
                out.push_str(&cleaned);
                out.push('\n');
            }
            continue;
        }

        if has_start {
            if inside_internal {
                bail!(
                    "nested {MARKER_START} at line {line_num} \
                     (previous one opened at line {start_line})"
                );
            }
            inside_internal = true;
            start_line = line_num;
            continue;
        }

        if has_end {
            if !inside_internal {
                bail!("orphan {MARKER_END} at line {line_num}");
            }
            inside_internal = false;
            continue;
        }

        if !inside_internal {
            out.push_str(line);
            out.push('\n');
        }
    }

    if inside_internal {
        bail!("unclosed {MARKER_START} (opened at line {start_line})");
    }

    Ok(out)
}

/// Collapse runs of more than two consecutive blank lines down to
/// two. Preserves a trailing newline if the input had one.
fn collapse_excess_blanks(s: &str) -> String {
    let mut out: Vec<&str> = Vec::with_capacity(s.lines().count());
    let mut blank_run = 0usize;
    for line in s.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 2 {
                out.push(line);
            }
        } else {
            blank_run = 0;
            out.push(line);
        }
    }
    let mut joined = out.join("\n");
    if s.ends_with('\n') {
        joined.push('\n');
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_markers_strips_h1_and_adds_frontmatter() {
        let input = "# Title\n\nBody paragraph 1.\n\nBody paragraph 2.\n";
        let expected = "---\ntitle: T\n---\n\nBody paragraph 1.\n\nBody paragraph 2.\n";
        assert_eq!(transform(input, "T").unwrap(), expected);
    }

    #[test]
    fn missing_h1_preserves_all_content() {
        let input = "Body without H1.\n";
        let result = transform(input, "T").unwrap();
        assert!(result.contains("Body without H1."));
        assert!(result.starts_with("---\ntitle: T\n---\n\n"));
    }

    #[test]
    fn internal_block_elided_drops_content_and_markers() {
        let input = "# T\n\
                     \n\
                     Kept 1.\n\
                     \n\
                     <!-- alint:internal-start -->\n\
                     Secret content.\n\
                     <!-- alint:internal-end -->\n\
                     \n\
                     Kept 2.\n";
        let result = transform(input, "T").unwrap();
        assert!(result.contains("Kept 1."));
        assert!(result.contains("Kept 2."));
        assert!(!result.contains("Secret content."));
        assert!(!result.contains("alint:internal-start"));
        assert!(!result.contains("alint:internal-end"));
    }

    #[test]
    fn whole_section_elided() {
        let input = "# T\n\
                     \n\
                     ## Public Section\n\
                     \n\
                     Public.\n\
                     \n\
                     <!-- alint:internal-start -->\n\
                     ## Internal Section\n\
                     \n\
                     Detailed internal-only writeup.\n\
                     <!-- alint:internal-end -->\n\
                     \n\
                     ## Next Public Section\n\
                     \n\
                     Also public.\n";
        let result = transform(input, "T").unwrap();
        assert!(result.contains("## Public Section"));
        assert!(result.contains("## Next Public Section"));
        assert!(!result.contains("Internal Section"));
        assert!(!result.contains("Detailed internal-only writeup"));
    }

    #[test]
    fn nested_markers_rejected() {
        let input = "<!-- alint:internal-start -->\n\
                     <!-- alint:internal-start -->\n\
                     foo\n\
                     <!-- alint:internal-end -->\n\
                     <!-- alint:internal-end -->\n";
        let err = transform(input, "T").unwrap_err().to_string();
        assert!(err.contains("nested"), "want 'nested', got: {err}");
    }

    #[test]
    fn orphan_end_marker_rejected() {
        let input = "foo\n<!-- alint:internal-end -->\nbar\n";
        let err = transform(input, "T").unwrap_err().to_string();
        assert!(err.contains("orphan"), "want 'orphan', got: {err}");
    }

    #[test]
    fn unclosed_start_marker_rejected() {
        let input = "foo\n<!-- alint:internal-start -->\nbar\n";
        let err = transform(input, "T").unwrap_err().to_string();
        assert!(err.contains("unclosed"), "want 'unclosed', got: {err}");
    }

    #[test]
    fn markers_inside_code_fence_treated_as_literal() {
        let input = "# T\n\
                     \n\
                     Usage example:\n\
                     \n\
                     ```markdown\n\
                     <!-- alint:internal-start -->\n\
                     Example internal content.\n\
                     <!-- alint:internal-end -->\n\
                     ```\n\
                     \n\
                     Done.\n";
        let result = transform(input, "T").unwrap();
        // Markers inside the fence appear verbatim in the output.
        assert!(result.contains("<!-- alint:internal-start -->"));
        assert!(result.contains("<!-- alint:internal-end -->"));
        assert!(result.contains("Example internal content."));
        assert!(result.contains("Done."));
    }

    #[test]
    fn excess_blank_lines_collapsed_to_two() {
        let input = "# T\n\
                     \n\
                     A.\n\
                     \n\
                     \n\
                     \n\
                     \n\
                     B.\n";
        let result = transform(input, "T").unwrap();
        // No run of more than three consecutive newlines (which
        // would render as more than two blank lines).
        assert!(!result.contains("\n\n\n\n"));
        assert!(result.contains("A."));
        assert!(result.contains("B."));
    }

    #[test]
    fn same_line_wrapper_strips_marker_syntax_but_keeps_content() {
        let input = "# T\n\
                     \n\
                     foo <!-- alint:internal-start --><!-- alint:internal-end --> bar\n";
        let result = transform(input, "T").unwrap();
        assert!(result.contains("foo "));
        assert!(result.contains(" bar"));
        assert!(!result.contains("alint:internal"));
    }

    #[test]
    fn empty_internal_block_drops_nothing_visible() {
        // A start-marker line immediately followed by an end-
        // marker line. Both lines are dropped; no content is
        // elided beyond those two marker lines themselves.
        let input = "# T\n\
                     \n\
                     before\n\
                     <!-- alint:internal-start -->\n\
                     <!-- alint:internal-end -->\n\
                     after\n";
        let result = transform(input, "T").unwrap();
        assert!(result.contains("before"));
        assert!(result.contains("after"));
        assert!(!result.contains("alint:internal"));
    }

    #[test]
    fn idempotent_on_zero_marker_input() {
        // The generator's output is itself markdown-with-
        // frontmatter; running it back through `transform` would
        // see the frontmatter as content. We don't claim
        // generator(generator(x)) == generator(x). What we DO
        // claim: transform(x) does not depend on anything outside
        // x and title. Two runs on the same input produce the
        // same bytes.
        let input = "# T\n\nbody\n";
        let a = transform(input, "T").unwrap();
        let b = transform(input, "T").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn deterministic_across_runs_with_markers() {
        let input = "# T\n\
                     \n\
                     keep\n\
                     <!-- alint:internal-start -->\n\
                     drop\n\
                     <!-- alint:internal-end -->\n\
                     keep\n";
        let a = transform(input, "T").unwrap();
        let b = transform(input, "T").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn roadmap_public_markers_are_stripped() {
        // The `roadmap-public` markers feed `xtask gen-roadmap`; they must
        // not leak into the published /docs/about/roadmap/ page. The
        // heading they annotate stays.
        let input = "# T\n\
                     \n\
                     ## v0.12: Coverage\n\
                     <!-- roadmap-public: blurb=\"New kinds and a security cut.\" -->\n\
                     \n\
                     Body.\n";
        let result = transform(input, "T").unwrap();
        assert!(result.contains("## v0.12: Coverage"));
        assert!(result.contains("Body."));
        assert!(!result.contains("roadmap-public"));
        assert!(!result.contains("blurb="));
    }
}
