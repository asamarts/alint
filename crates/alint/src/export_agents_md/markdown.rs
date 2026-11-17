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
use crate::inspect::NamedObjects;

/// Markers that delimit the alint-managed region inside an
/// `AGENTS.md`. Public so tests can match against the same
/// constant; HTML comments by design so the markers render
/// invisibly in GitHub-rendered markdown.
pub const START_MARKER: &str = "<!-- alint:start -->";
pub const END_MARKER: &str = "<!-- alint:end -->";

/// The managed section: `directives` by severity, then a version 2 config's
/// named selectors and relations (`objects`; `None` for version 1).
pub fn render(
    directives: &[Directive],
    objects: Option<&NamedObjects<'_>>,
    section_title: &str,
) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(1024);
    out.push_str(START_MARKER);
    out.push('\n');
    out.push('\n');
    let _ = writeln!(out, "## {section_title}");
    out.push('\n');
    out.push_str(
        "Generated from `.alint.yml` by `alint export-agents-md`. \
         Re-run after editing the lint config - these directives \
         must stay in sync with what alint blocks at commit time. \
         Manual edits inside this section are overwritten.\n",
    );
    out.push('\n');

    if directives.is_empty() {
        out.push_str("_No rules at the configured severity threshold._\n");
        out.push('\n');
    } else {
        write_severity_sections(&mut out, directives);
    }
    if let Some(objects) = objects.filter(|objects| !objects.is_empty()) {
        write_named_objects(&mut out, objects);
    }

    out.push_str(END_MARKER);
    out.push('\n');
    out
}

fn write_severity_sections(out: &mut String, directives: &[Directive]) {
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
        write_bullets(out, &errors);
        out.push('\n');
    }
    if !warnings.is_empty() {
        out.push_str("### Warnings (review before merge)\n\n");
        write_bullets(out, &warnings);
        out.push('\n');
    }
    if !infos.is_empty() {
        out.push_str("### Info (informational nudges)\n\n");
        write_bullets(out, &infos);
        out.push('\n');
    }
}

/// A version 2 config's named selectors and relations, as `alint list`
/// shows them: each with the config file that declares it, then one line for
/// each such file the `extends:` aliases (`as:`) reach, with those aliases.
fn write_named_objects(out: &mut String, named: &NamedObjects<'_>) {
    out.push_str("### Named selectors and relations\n\n");
    out.push_str(
        "Declared by this `version: 2` config (experimental), each with the config file \
         that declares it.\n\n",
    );
    for object in &named.objects {
        out.push_str("- ");
        out.push_str(object.what);
        out.push(' ');
        out.push_str(&code_span(object.name));
        out.push_str(": ");
        out.push_str(&code_span(named.display(object.source)));
        out.push('\n');
    }
    out.push('\n');
    let mut reached = named.aliased().peekable();
    if reached.peek().is_none() {
        return;
    }
    out.push_str("The `extends:` aliases that reach those files:\n\n");
    for (source, aliases) in reached {
        let shown: Vec<String> = aliases
            .iter()
            .map(|(alias, consumer)| format!("as {} in {}", code_span(alias), code_span(consumer)))
            .collect();
        out.push_str("- ");
        out.push_str(&code_span(source));
        out.push_str(": ");
        out.push_str(&shown.join(", "));
        out.push('\n');
    }
    out.push('\n');
}

/// `text` as a Markdown code span: delimited by one backtick more than its
/// longest backtick run, and padded with a space when it starts or ends with
/// a backtick, so a file name cannot close the span early (as in
/// `alint-lsp`'s `render::code_span`). A line break shows as a space.
fn code_span(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    let text = text.replace(['\n', '\r'], " ");
    format!("{fence}{pad}{text}{pad}{fence}")
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
    use crate::inspect::{NamedObject, NamedSource};

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
        let out = render(&dirs, None, "Lint rules enforced by alint");
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
        let out = render(&dirs, None, "Lint rules");
        assert!(out.contains("### Errors"));
        assert!(!out.contains("### Warnings"));
        assert!(!out.contains("### Info"));
    }

    #[test]
    fn empty_set_produces_minimal_section() {
        let out = render(&[], None, "Lint rules");
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
        let out = render(&dirs, None, "Title");
        assert!(out.contains("[policy](https://example.com/policy)"));
    }

    #[test]
    fn output_uses_lf_line_endings_only() {
        let dirs = vec![d("x", Level::Warning, "hi")];
        let out = render(&dirs, None, "Title");
        assert!(!out.contains("\r\n"), "rendered output must use LF only");
    }

    #[test]
    fn output_ends_with_trailing_newline() {
        let out = render(&[], None, "Title");
        assert!(out.ends_with('\n'));
    }

    /// The objects `(what, name, source index)` of `sources`, each a display
    /// with its aliases as `(alias, index of the source giving it)`.
    fn named<'p>(
        sources: Vec<(&'p str, Vec<(&'p str, usize)>)>,
        objects: Vec<(&'static str, &'p str, usize)>,
    ) -> NamedObjects<'p> {
        let mut sources: Vec<NamedSource<'p>> = sources
            .into_iter()
            .map(|(display, aliases)| NamedSource {
                display: display.into(),
                aliases,
                declares: false,
            })
            .collect();
        for &(_, _, source) in &objects {
            sources[source].declares = true;
        }
        NamedObjects {
            sources,
            objects: objects
                .into_iter()
                .map(|(what, name, source)| NamedObject { what, name, source })
                .collect(),
        }
    }

    #[test]
    fn named_objects_follow_the_rules_with_their_source_then_its_aliases() {
        let dirs = vec![d("err", Level::Error, "x")];
        let objects = named(
            vec![
                ("deps.yml", vec![("deps", 1), ("d", 2)]),
                (".alint.yml", vec![]),
                ("rules/b.yml", vec![]),
                // Aliased, but declares nothing: no line.
                ("empty.yml", vec![("e", 1)]),
            ],
            vec![
                ("selector", "manifests", 0),
                ("relation", "ranges", 0),
                ("relation", "titles", 1),
            ],
        );
        let out = render(&dirs, Some(&objects), "Lint rules");
        let expected = "### Errors (commit will fail)\n\n- **`err`**: x\n\n\
                        ### Named selectors and relations\n\n\
                        Declared by this `version: 2` config (experimental), each with the \
                        config file that declares it.\n\n\
                        - selector `manifests`: `deps.yml`\n\
                        - relation `ranges`: `deps.yml`\n\
                        - relation `titles`: `.alint.yml`\n\n\
                        The `extends:` aliases that reach those files:\n\n\
                        - `deps.yml`: as `deps` in `.alint.yml`, as `d` in `rules/b.yml`\n\n\
                        <!-- alint:end -->\n";
        assert!(out.ends_with(expected), "{out}");
    }

    #[test]
    fn named_objects_show_without_rules_and_not_when_there_are_none() {
        let objects = named(vec![(".alint.yml", vec![])], vec![("selector", "docs", 0)]);
        let out = render(&[], Some(&objects), "Lint rules");
        assert!(
            out.contains(
                "_No rules at the configured severity threshold._\n\n\
                 ### Named selectors and relations\n"
            ),
            "{out}"
        );
        assert!(
            out.ends_with("- selector `docs`: `.alint.yml`\n\n<!-- alint:end -->\n"),
            "{out}"
        );
        // A version 2 config without named objects, and a version 1 config,
        // render as before.
        assert_eq!(
            render(&[], Some(&named(vec![], vec![])), "Lint rules"),
            render(&[], None, "Lint rules")
        );
    }

    #[test]
    fn a_file_name_cannot_close_its_code_span() {
        assert_eq!(code_span("a.yml"), "`a.yml`");
        assert_eq!(code_span("a`b.yml"), "``a`b.yml``");
        assert_eq!(code_span("`a``.yml"), "``` `a``.yml ```");
        assert_eq!(code_span("a\nb"), "`a b`");
    }

    #[test]
    fn round_trip_identical_when_inputs_match() {
        // A re-run with the same directive list must produce
        // byte-identical output. This is the property
        // `--inline` round-trip identity hinges on.
        let dirs = vec![d("a", Level::Error, "1"), d("b", Level::Warning, "2")];
        let first = render(&dirs, None, "Lint rules");
        let second = render(&dirs, None, "Lint rules");
        assert_eq!(first, second);
    }
}
