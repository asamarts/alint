//! Markdown renderer.
//!
//! Section-per-severity, bullet-per-rule shape sized to drop
//! into an `AGENTS.md` directive block. Severity ordering:
//! Errors first (commit will fail), then Warnings (review
//! before merge), then Info (only with `--include-info`).
//!
//! Stable byte-for-byte output across runs — line endings
//! always `\n`, trailing newline guaranteed, no timestamps in
//! the body so re-running `--inline` produces an identical
//! file.

use alint_core::Level;

use super::Directive;

/// Markers that delimit the alint-managed region inside an
/// `AGENTS.md`. Public so tests can match against the same
/// constant; HTML comments by design so the markers render
/// invisibly in GitHub-rendered markdown.
pub const START_MARKER: &str = "<!-- alint:start -->";
pub const END_MARKER: &str = "<!-- alint:end -->";

pub fn render(directives: &[Directive], section_title: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(1024);
    out.push_str(START_MARKER);
    out.push('\n');
    out.push('\n');
    let _ = writeln!(out, "## {section_title}");
    out.push('\n');
    out.push_str(
        "Generated from `.alint.yml` by `alint export-agents-md`. \
         Re-run after editing the lint config — these directives \
         must stay in sync with what alint blocks at commit time. \
         Manual edits inside this section are overwritten.\n",
    );
    out.push('\n');

    if directives.is_empty() {
        out.push_str("_No rules at the configured severity threshold._\n");
        out.push('\n');
        out.push_str(END_MARKER);
        out.push('\n');
        return out;
    }

    let errors: Vec<&Directive> = directives
        .iter()
        .filter(|d| matches!(d.severity, Level::Error))
        .collect();
    let warnings: Vec<&Directive> = directives
        .iter()
        .filter(|d| matches!(d.severity, Level::Warning))
        .collect();
    let infos: Vec<&Directive> = directives
        .iter()
        .filter(|d| matches!(d.severity, Level::Info))
        .collect();

    if !errors.is_empty() {
        out.push_str("### Errors (commit will fail)\n\n");
        write_bullets(&mut out, &errors);
        out.push('\n');
    }
    if !warnings.is_empty() {
        out.push_str("### Warnings (review before merge)\n\n");
        write_bullets(&mut out, &warnings);
        out.push('\n');
    }
    if !infos.is_empty() {
        out.push_str("### Info (informational nudges)\n\n");
        write_bullets(&mut out, &infos);
        out.push('\n');
    }

    out.push_str(END_MARKER);
    out.push('\n');
    out
}

fn write_bullets(out: &mut String, bullets: &[&Directive]) {
    for d in bullets {
        out.push_str("- **`");
        out.push_str(&d.rule_id);
        out.push_str("`**: ");
        out.push_str(d.directive.trim_end());
        if let Some(url) = &d.policy_url {
            out.push_str(" [policy](");
            out.push_str(url);
            out.push(')');
        }
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(id: &str, sev: Level, msg: &str) -> Directive {
        Directive {
            rule_id: id.into(),
            severity: sev,
            directive: msg.into(),
            policy_url: None,
        }
    }

    fn dp(id: &str, sev: Level, msg: &str, url: &str) -> Directive {
        Directive {
            rule_id: id.into(),
            severity: sev,
            directive: msg.into(),
            policy_url: Some(url.into()),
        }
    }

    #[test]
    fn renders_three_severity_sections() {
        let dirs = vec![
            d("err1", Level::Error, "must not"),
            d("warn1", Level::Warning, "avoid"),
            d("info1", Level::Info, "fyi"),
        ];
        let out = render(&dirs, "Lint rules enforced by alint");
        assert!(out.contains(START_MARKER));
        assert!(out.contains(END_MARKER));
        assert!(out.contains("## Lint rules enforced by alint"));
        assert!(out.contains("### Errors (commit will fail)"));
        assert!(out.contains("### Warnings (review before merge)"));
        assert!(out.contains("### Info (informational nudges)"));
        assert!(out.contains("- **`err1`**: must not"));
        assert!(out.contains("- **`warn1`**: avoid"));
        assert!(out.contains("- **`info1`**: fyi"));
    }

    #[test]
    fn omits_empty_severity_sections() {
        let dirs = vec![d("err", Level::Error, "x")];
        let out = render(&dirs, "Lint rules");
        assert!(out.contains("### Errors"));
        assert!(!out.contains("### Warnings"));
        assert!(!out.contains("### Info"));
    }

    #[test]
    fn empty_set_produces_minimal_section() {
        let out = render(&[], "Lint rules");
        assert!(out.contains(START_MARKER));
        assert!(out.contains(END_MARKER));
        assert!(out.contains("_No rules at the configured severity threshold._"));
    }

    #[test]
    fn policy_url_renders_as_markdown_link() {
        let dirs = vec![dp(
            "x",
            Level::Error,
            "do not",
            "https://example.com/policy",
        )];
        let out = render(&dirs, "Title");
        assert!(out.contains("[policy](https://example.com/policy)"));
    }

    #[test]
    fn output_uses_lf_line_endings_only() {
        let dirs = vec![d("x", Level::Warning, "hi")];
        let out = render(&dirs, "Title");
        assert!(!out.contains("\r\n"), "rendered output must use LF only");
    }

    #[test]
    fn output_ends_with_trailing_newline() {
        let out = render(&[], "Title");
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn round_trip_identical_when_inputs_match() {
        // A re-run with the same directive list must produce
        // byte-identical output. This is the property
        // `--inline` round-trip identity hinges on.
        let dirs = vec![d("a", Level::Error, "1"), d("b", Level::Warning, "2")];
        let first = render(&dirs, "Lint rules");
        let second = render(&dirs, "Lint rules");
        assert_eq!(first, second);
    }
}
