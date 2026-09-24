//! The generated `/docs/cli/<subcommand>/` pages: dropping the global options
//! clap repeats in every subcommand's `--help` (the CLI landing page documents
//! them once), and merging the hand-written prose from
//! `docs/site/cli/<subcommand>.md` around the captured reference.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};

use super::{
    CLI_REFERENCE_SUBCMDS, cli_view, escape_yaml_string, help_section_body, meta_desc_clean,
    parse_help_definition_list,
};

/// The long flag of a clap option term: `-c, --config <CONFIG>` -> `--config`.
fn option_long_flag(term: &str) -> Option<&str> {
    term.split(|c: char| c == ',' || c.is_whitespace())
        .find(|token| token.starts_with("--"))
}

/// The top-level `alint --help` options as parsed `(term, description)`
/// entries, keyed by long flag. clap repeats these global options, verbatim,
/// in every subcommand's `--help`.
pub(super) fn global_options(top_help: &str) -> HashMap<String, (String, String)> {
    parse_help_definition_list(&help_section_body(top_help, "Options:"))
        .into_iter()
        .filter_map(|(term, desc)| {
            let flag = option_long_flag(&term)?.to_string();
            Some((flag, (term, desc)))
        })
        .collect()
}

/// Drop the global options from a subcommand's `--help` Options section; the
/// CLI landing page documents them once, in its Global options table. Left in,
/// the ~17 shared flags made the subcommand pages 70 to 89% identical text, and
/// Google declined to index some of them as near-duplicates (alint.org Search
/// Console, 2026-09).
///
/// An entry is dropped only when its flag AND its help text match the global
/// entry exactly. Several subcommands define their own option under a global's
/// name (`suggest`, `export-agents-md` and `validate-config` each have their
/// own `--format`, with different values and defaults), and those must stay.
/// An Options header left with no entries is dropped too. Lines that don't
/// parse as an option entry are kept verbatim, so a clap layout change
/// degrades to the full dump, never to lost text. Returns the pruned help and
/// how many entries were removed.
pub(super) fn strip_global_options(
    help: &str,
    globals: &HashMap<String, (String, String)>,
) -> (String, usize) {
    let mut out: Vec<&str> = Vec::new();
    let mut removed = 0;
    let mut lines = help.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim_end() != "Options:" {
            out.push(line);
            continue;
        }
        // Group the section into entries: a shallow `-`/`--` header line plus
        // everything indented (or blank) below it, up to the next section.
        let mut entries: Vec<Vec<&str>> = Vec::new();
        while let Some(&next) = lines.peek() {
            if !next.trim().is_empty() && !next.starts_with(char::is_whitespace) {
                break;
            }
            lines.next();
            let trimmed = next.trim_start();
            let indent = next.len() - trimmed.len();
            let is_header = indent <= 6 && trimmed.starts_with('-');
            match entries.last_mut() {
                Some(entry) if !is_header => entry.push(next),
                _ => entries.push(vec![next]),
            }
        }
        let kept: Vec<Vec<&str>> = entries
            .into_iter()
            .filter(|entry| {
                let global = is_global_entry(entry, globals);
                removed += usize::from(global);
                !global
            })
            .collect();
        if kept.iter().flatten().any(|l| !l.trim().is_empty()) {
            out.push(line);
            out.extend(kept.into_iter().flatten());
        }
    }
    // Removing entries can leave runs of blank lines; keep single separators.
    let mut pruned = String::new();
    let mut previous_blank = false;
    for line in out {
        let blank = line.trim().is_empty();
        if blank && previous_blank {
            continue;
        }
        pruned.push_str(if blank { "" } else { line });
        pruned.push('\n');
        previous_blank = blank;
    }
    let pruned = pruned.trim_end().to_string() + "\n";
    (pruned, removed)
}

/// Whether one grouped Options entry is a verbatim copy of a global option.
fn is_global_entry(entry: &[&str], globals: &HashMap<String, (String, String)>) -> bool {
    let non_blank: Vec<&str> = entry
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let parsed = parse_help_definition_list(&non_blank);
    let [(term, desc)] = parsed.as_slice() else {
        return false;
    };
    option_long_flag(term)
        .and_then(|flag| globals.get(flag))
        .is_some_and(|(global_term, global_desc)| global_term == term && global_desc == desc)
}

/// Hand-written prose for a CLI reference page (`docs/site/cli/<sub>.md`): an
/// optional `description:` that replaces the one derived from `--help`, the
/// intro (text before the first `## ` heading), the middle sections (e.g.
/// `## Examples`), and a trailing `## See also` section.
#[derive(Debug, Default, PartialEq)]
pub(super) struct CliProse {
    pub(super) description: Option<String>,
    pub(super) intro: String,
    pub(super) sections: String,
    pub(super) see_also: String,
}

/// Frontmatter keys a prose file may carry. `title` exists so the file is a
/// valid `docs/site` page (the frontmatter audit requires one); the generated
/// page sets its own title. Any other key would be silently dropped, so it is
/// an error instead.
const PROSE_FRONTMATTER_KEYS: &[&str] = &["title", "description"];

pub(super) fn parse_cli_prose(text: &str) -> Result<CliProse> {
    let text = text.replace("\r\n", "\n");
    let (description, body) = match text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
    {
        Some((front, body)) => {
            let yaml: serde_yaml_ng::Mapping =
                serde_yaml_ng::from_str(front).context("parsing CLI prose frontmatter")?;
            for key in yaml.keys() {
                let key = key.as_str().unwrap_or_default();
                if !PROSE_FRONTMATTER_KEYS.contains(&key) {
                    bail!(
                        "CLI prose frontmatter key `{key}` is not supported (allowed: \
                         {PROSE_FRONTMATTER_KEYS:?}); the generated page owns everything else"
                    );
                }
            }
            let description = yaml
                .get("description")
                .and_then(serde_yaml_ng::Value::as_str)
                .map(str::to_string);
            (description, body.to_string())
        }
        None => (None, text.clone()),
    };
    let first_section = heading_offset(&body, |h| h.starts_with("## ")).unwrap_or(body.len());
    let (intro, rest) = body.split_at(first_section);
    let see_also = heading_offset(rest, |h| h == "## See also").unwrap_or(rest.len());
    let (sections, see_also) = rest.split_at(see_also);
    Ok(CliProse {
        description,
        intro: intro.trim().to_string(),
        sections: sections.trim().to_string(),
        see_also: see_also.trim().to_string(),
    })
}

/// Byte offset of the first line (outside fenced code blocks) whose trimmed
/// text satisfies `is_heading`.
fn heading_offset(body: &str, is_heading: impl Fn(&str) -> bool) -> Option<usize> {
    let mut offset = 0;
    let mut in_fence = false;
    for line in body.split_inclusive('\n') {
        let text = line.trim_end();
        if text.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence && is_heading(text) {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

/// Assemble a subcommand's reference page: frontmatter, the prose intro, the
/// flow diagram (for subcommands that have one), the prose sections, the
/// `--help` capture (global options stripped) under a Reference heading, a
/// pointer to the global options, and the prose's See also last. Without prose
/// the page is the diagram and the capture.
pub(super) fn render_cli_page(
    sub: &str,
    derived_description: &str,
    prose: Option<&CliProse>,
    help: &str,
    stripped_globals: bool,
) -> String {
    let description = prose
        .and_then(|p| p.description.as_deref())
        .unwrap_or(derived_description);
    let mut page = String::new();
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page, "title: 'alint {sub}'");
    let _ = writeln!(
        &mut page,
        "description: '{}'",
        escape_yaml_string(&meta_desc_clean(description, 158))
    );
    let _ = writeln!(&mut page, "---");
    let _ = writeln!(&mut page);
    let block = |page: &mut String, text: &str| {
        if !text.is_empty() {
            let _ = writeln!(page, "{text}");
            let _ = writeln!(page);
        }
    };
    if let Some(p) = prose {
        block(&mut page, &p.intro);
    }
    if let Some((view, caption)) = cli_view(sub) {
        let _ = writeln!(&mut page, "{caption}");
        let _ = writeln!(&mut page);
        let _ = writeln!(&mut page, "<likec4-view view-id=\"{view}\"></likec4-view>");
        let _ = writeln!(&mut page);
    }
    if let Some(p) = prose {
        block(&mut page, &p.sections);
        let _ = writeln!(&mut page, "## Reference");
        let _ = writeln!(&mut page);
    }
    let _ = writeln!(&mut page, "```");
    page.push_str(help);
    let _ = writeln!(&mut page, "```");
    if stripped_globals {
        let _ = writeln!(&mut page);
        let _ = writeln!(
            &mut page,
            "The [global options](/docs/cli/#global-options) apply to `alint {sub}` too, where \
             they are relevant."
        );
    }
    if let Some(p) = prose.filter(|p| !p.see_also.is_empty()) {
        let _ = writeln!(&mut page);
        let _ = writeln!(&mut page, "{}", p.see_also);
    }
    page
}

/// Fail on prose files in the bundle's `cli/` directory (copied from
/// `docs/site/cli/`) that no generated page would merge: a name that isn't a
/// documented subcommand would ship as a bare page, and `index.md` would be
/// silently overwritten by the landing page generated from `alint --help`.
pub(super) fn check_cli_prose_files(cli_dir: &Path) -> Result<()> {
    if !cli_dir.is_dir() {
        return Ok(());
    }
    let mut unmatched: Vec<String> = Vec::new();
    for entry in fs::read_dir(cli_dir).with_context(|| format!("reading {}", cli_dir.display()))? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if stem == "index" || !CLI_REFERENCE_SUBCMDS.contains(&stem) {
            unmatched.push(format!("{stem}.md"));
        }
    }
    if !unmatched.is_empty() {
        unmatched.sort();
        bail!(
            "docs/site/cli/ has prose that no generated CLI page merges: {unmatched:?}. Each file \
             must be named after a subcommand in CLI_REFERENCE_SUBCMDS; the CLI landing page \
             (index.md) is generated from `alint --help` and takes no prose."
        );
    }
    Ok(())
}
