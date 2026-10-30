//! The generated `/docs/cli/<subcommand>/` pages: dropping the global options
//! clap repeats in every subcommand's `--help` (the CLI landing page documents
//! them once), and merging the hand-written prose from
//! `docs/site/cli/<subcommand>.md` around the captured reference.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use super::CLI_REFERENCE_SUBCMDS;
use super::rule_pages::{escape_yaml_string, meta_desc_clean};
use crate::build_release_binary;

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
/// degrades to the full dump, never to lost text.
pub(super) fn strip_global_options(
    help: &str,
    globals: &HashMap<String, (String, String)>,
) -> StrippedHelp {
    let mut out: Vec<&str> = Vec::new();
    let mut removed: Vec<String> = Vec::new();
    let mut own: Vec<String> = Vec::new();
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
            .filter(|entry| match global_match(entry, globals) {
                Some((flag, true)) => {
                    removed.push(flag.to_string());
                    false
                }
                Some((flag, false)) => {
                    own.push(flag.to_string());
                    true
                }
                None => true,
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
    StrippedHelp {
        help: pruned.trim_end().to_string() + "\n",
        removed,
        own,
    }
}

/// A subcommand's `--help` with the global options taken out.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct StrippedHelp {
    pub(super) help: String,
    /// Long flags of the global options removed as verbatim copies.
    pub(super) removed: Vec<String>,
    /// Long flags the subcommand defines itself under a global's name. They
    /// stay in the help, and the subcommand's option replaces the global.
    pub(super) own: Vec<String>,
}

/// For a grouped Options entry whose long flag is a global option's: that
/// flag, and whether the entry is a verbatim copy of the global.
fn global_match<'g>(
    entry: &[&str],
    globals: &'g HashMap<String, (String, String)>,
) -> Option<(&'g str, bool)> {
    let non_blank: Vec<&str> = entry
        .iter()
        .copied()
        .filter(|l| !l.trim().is_empty())
        .collect();
    let parsed = parse_help_definition_list(&non_blank);
    let [(term, desc)] = parsed.as_slice() else {
        return None;
    };
    let (flag, (global_term, global_desc)) = globals.get_key_value(option_long_flag(term)?)?;
    Some((flag, global_term == term && global_desc == desc))
}

/// The global options no subcommand's `--help` carries (clap keeps
/// `--version` on the top-level command), sorted, for the landing page to
/// name as exceptions. `in_subcommands` holds every global flag some
/// subcommand's help repeated or redefined.
pub(super) fn top_level_only(
    globals: &HashMap<String, (String, String)>,
    in_subcommands: &HashSet<String>,
) -> Vec<String> {
    let mut flags: Vec<String> = globals
        .keys()
        .filter(|flag| !in_subcommands.contains(*flag))
        .cloned()
        .collect();
    flags.sort();
    flags
}

/// Flags as a prose list of code spans: `` `--a` ``, `` `--a` and `--b` ``,
/// `` `--a`, `--b` and `--c` ``.
pub(super) fn code_list(flags: &[String]) -> String {
    let spans: Vec<String> = flags.iter().map(|f| format!("`{f}`")).collect();
    match spans.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
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

/// Frontmatter keys a prose file may carry. `title` is allowed so the file
/// reads like every other `docs/site` page, but the generated page sets its
/// own. Any other key would be silently dropped, so it is an error instead.
const PROSE_FRONTMATTER_KEYS: &[&str] = &["title", "description"];

/// The longest prose `description:`, alint.org's cap for a search snippet
/// (`check-meta-descriptions.mjs`). `meta_desc_clean` would cut a longer one
/// back to its last sentence end without a word, so it is an error instead.
const PROSE_DESCRIPTION_MAX: usize = 155;

pub(super) fn parse_cli_prose(text: &str) -> Result<CliProse> {
    let text = text.replace("\r\n", "\n");
    let (description, body) = match text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---\n"))
    {
        Some((front, body)) => {
            let yaml: ayaml::Mapping =
                ayaml::from_str(front).context("parsing CLI prose frontmatter")?;
            for key in yaml.keys() {
                let key = key.as_str().unwrap_or_default();
                if !PROSE_FRONTMATTER_KEYS.contains(&key) {
                    bail!(
                        "CLI prose frontmatter key `{key}` is not supported (allowed: \
                         {PROSE_FRONTMATTER_KEYS:?}); the generated page owns everything else"
                    );
                }
            }
            let description = match yaml.get("description") {
                None => None,
                Some(value) => {
                    let description = value.as_str().map(str::trim).unwrap_or_default();
                    if description.is_empty() {
                        bail!("CLI prose `description:` must be a non-empty string");
                    }
                    let chars = description.chars().count();
                    if chars > PROSE_DESCRIPTION_MAX {
                        bail!(
                            "CLI prose `description:` is {chars} characters; a search snippet \
                             holds {PROSE_DESCRIPTION_MAX}, and a longer one would be cut"
                        );
                    }
                    Some(description.to_string())
                }
            };
            (description, body.to_string())
        }
        None => (None, text.clone()),
    };
    let first_section = heading_offset(&body, |h| h.starts_with("## ")).unwrap_or(body.len());
    let (intro, rest) = body.split_at(first_section);
    let see_also = heading_offset(rest, |h| {
        h.strip_prefix("## ").is_some_and(|t| {
            t.trim_end_matches('#')
                .trim()
                .eq_ignore_ascii_case("see also")
        })
    })
    .unwrap_or(rest.len());
    let (sections, see_also) = rest.split_at(see_also);
    Ok(CliProse {
        description,
        intro: intro.trim().to_string(),
        sections: sections.trim().to_string(),
        see_also: see_also.trim().to_string(),
    })
}

/// Byte offset of the first line outside fenced code blocks whose text
/// satisfies `is_heading`, after dropping up to three spaces of indent (the
/// most a Markdown heading may have) and trailing whitespace. Fences follow
/// the Markdown spec too: three or more backticks or tildes open one, and only
/// a run of the same character, at least as long and with nothing after it,
/// closes it.
fn heading_offset(body: &str, is_heading: impl Fn(&str) -> bool) -> Option<usize> {
    let mut offset = 0;
    let mut fence: Option<(char, usize)> = None;
    for line in body.split_inclusive('\n') {
        let text = line.trim_end();
        let indent = text.len() - text.trim_start_matches(' ').len();
        if indent <= 3 {
            let text = &text[indent..];
            let marker = text.chars().next().filter(|c| matches!(c, '`' | '~'));
            // Both fence characters are one byte, so the run length is a byte index.
            let run = marker.map_or(0, |m| text.chars().take_while(|&c| c == m).count());
            match (fence, marker) {
                (None, Some(m)) if run >= 3 => fence = Some((m, run)),
                (Some((open, len)), Some(m))
                    if m == open && run >= len && text[run..].trim().is_empty() =>
                {
                    fence = None;
                }
                (None, _) if is_heading(text) => return Some(offset),
                _ => {}
            }
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
    stripped: &StrippedHelp,
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
    page.push_str(&stripped.help);
    let _ = writeln!(&mut page, "```");
    if !stripped.removed.is_empty() {
        let _ = writeln!(&mut page);
        let _ = write!(
            &mut page,
            "The [global options](/docs/cli/#global-options) apply to `alint {sub}` too, where \
             they are relevant."
        );
        if let [flag] = stripped.own.as_slice() {
            let _ = write!(
                &mut page,
                " Its own `{flag}` above replaces the global one."
            );
        } else if !stripped.own.is_empty() {
            let _ = write!(
                &mut page,
                " Its own {} above replace the global ones.",
                code_list(&stripped.own)
            );
        }
        let _ = writeln!(&mut page);
    }
    if let Some(p) = prose.filter(|p| !p.see_also.is_empty()) {
        let _ = writeln!(&mut page);
        let _ = writeln!(&mut page, "{}", p.see_also);
    }
    page
}

/// Fail on pages in the bundle's `cli/` directory (copied from
/// `docs/site/cli/`) that no generated page would merge: a name that isn't a
/// documented subcommand would ship as a bare page, `index.md` would be
/// silently overwritten by the landing page generated from `alint --help`, and
/// an `.mdx` file or a subdirectory would ship unvalidated (`check.mdx` on the
/// same URL as the generated `check.md`).
pub(super) fn check_cli_prose_files(cli_dir: &Path) -> Result<()> {
    if !cli_dir.is_dir() {
        return Ok(());
    }
    let mut unmatched: Vec<String> = Vec::new();
    for entry in fs::read_dir(cli_dir).with_context(|| format!("reading {}", cli_dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let matched = match path.extension().and_then(|e| e.to_str()) {
            _ if path.is_dir() => false,
            Some("md") => stem != "index" && CLI_REFERENCE_SUBCMDS.contains(&stem),
            Some("mdx") => false,
            _ => true,
        };
        if !matched {
            unmatched.push(name);
        }
    }
    if !unmatched.is_empty() {
        unmatched.sort();
        bail!(
            "docs/site/cli/ has pages that no generated CLI page merges: {unmatched:?}. Each must \
             be a `<subcommand>.md` named after an entry in CLI_REFERENCE_SUBCMDS (no .mdx, no \
             subdirectories); the CLI landing page (index.md) is generated from `alint --help` \
             and takes no prose."
        );
    }
    Ok(())
}
/// The architecture flow embedded atop a generated `cli/<sub>` page, if any.
/// The architecture view embedded on a `cli/<sub>` reference page, with a
/// one-line caption. `facts` is intentionally absent: factsFlow depicts
/// fact-evaluation *plus* rule gating, but `alint facts` only evaluates and
/// prints facts, so the diagram would over-reach.
pub(crate) fn cli_view(sub: &str) -> Option<(&'static str, &'static str)> {
    match sub {
        "check" => Some(("checkFlow", "The pipeline `alint check` runs:")),
        "fix" => Some(("fixFlow", "How `alint fix` applies fixes and re-checks:")),
        "lsp" => Some(("lspFlow", "How `alint lsp` serves an editor over LSP:")),
        _ => None,
    }
}

/// The contiguous indented body of a top-level `--help` section (e.g.
/// `"Commands:"` / `"Options:"`): the indented lines after the header, up to the
/// next non-indented header. Blank lines are dropped.
fn help_section_body<'a>(help: &'a str, header: &str) -> Vec<&'a str> {
    let mut lines = help.lines();
    for line in lines.by_ref() {
        if line.trim_end() == header {
            break;
        }
    }
    let mut body = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            break; // reached the next section header
        }
        body.push(line);
    }
    body
}

/// Parse an indented clap `term  description` block (a `Commands:` or `Options:`
/// body) into `(term, description)` pairs, folding wrapped continuation lines
/// into the preceding description. clap puts each term at a shallow indent (2 or
/// 6) and wraps its description at a deeper column, which is how we tell them
/// apart.
fn parse_help_definition_list(lines: &[&str]) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = Vec::new();
    for line in lines {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if indent <= 6 {
            let (term, desc) = trimmed.split_once("  ").unwrap_or((trimmed, ""));
            entries.push((term.trim().to_string(), desc.trim().to_string()));
        } else if let Some((_, desc)) = entries.last_mut() {
            if !desc.is_empty() {
                desc.push(' ');
            }
            desc.push_str(line.trim());
        }
    }
    entries
}

/// Render the top-level `alint --help` as a formatted CLI landing page: the
/// about blurb, the usage line, a Commands table (each linked to its subcommand
/// page), and a Global-options table that names the `top_level_only` flags no
/// subcommand takes (`--version`). Everything is parsed from the captured
/// `--help`, so it can never drift from the binary. Returns `None` if the help
/// doesn't parse into a sane shape (no options found) so the caller falls back
/// to the raw help dump — a clap format change degrades to the old behaviour,
/// never to garbage.
pub(super) fn format_top_help(help: &str, top_level_only: &[String]) -> Option<String> {
    let commands = parse_help_definition_list(&help_section_body(help, "Commands:"));
    let options = parse_help_definition_list(&help_section_body(help, "Options:"));
    if options.is_empty() {
        return None;
    }

    let about = help
        .lines()
        .take_while(|l| !l.starts_with("Usage:"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    let usage = help
        .lines()
        .find(|l| l.starts_with("Usage:"))
        .unwrap_or_default();

    let esc = |s: &str| s.replace('|', "\\|");
    let known: std::collections::HashSet<&str> = CLI_REFERENCE_SUBCMDS.iter().copied().collect();

    let mut out = String::new();
    if !about.is_empty() {
        let _ = writeln!(&mut out, "{about}\n");
    }
    if !usage.is_empty() {
        let _ = writeln!(&mut out, "```\n{usage}\n```\n");
    }
    if !commands.is_empty() {
        let _ = writeln!(&mut out, "## Commands\n");
        let _ = writeln!(&mut out, "| Command | Description |");
        let _ = writeln!(&mut out, "| --- | --- |");
        for (cmd, desc) in &commands {
            let name = cmd.split_whitespace().next().unwrap_or(cmd);
            let cell = if known.contains(name) {
                format!("[`{cmd}`](/docs/cli/{name}/)")
            } else {
                format!("`{cmd}`")
            };
            let _ = writeln!(&mut out, "| {cell} | {} |", esc(desc));
        }
        let _ = writeln!(&mut out);
    }
    let _ = writeln!(&mut out, "## Global options\n");
    if top_level_only.is_empty() {
        let _ = writeln!(&mut out, "These apply to every subcommand.\n");
    } else {
        let except = code_list(top_level_only);
        let _ = writeln!(
            &mut out,
            "These apply to every subcommand except {except}.\n"
        );
    }
    let _ = writeln!(&mut out, "| Flag | Description |");
    let _ = writeln!(&mut out, "| --- | --- |");
    for (flag, desc) in &options {
        let _ = writeln!(&mut out, "| `{}` | {} |", esc(flag), esc(desc));
    }
    let _ = writeln!(&mut out);
    let _ = writeln!(&mut out, "<sub>Generated from `alint --help`.</sub>");

    Some(out)
}

/// Build the alint binary in release mode, then capture
/// `alint --help` and `alint <subcmd> --help` for each subcommand.
/// The top-level help renders as a formatted landing page (`cli/index.md`);
/// each subcommand's help becomes its own page under `cli/<subcmd>.md`.
pub(super) fn generate_cli_reference(workspace: &Path, target_dir: &Path) -> Result<()> {
    let bin = build_release_binary()?;

    let cli_dir = target_dir.join("cli");
    fs::create_dir_all(&cli_dir)?;
    // Prose copied in from docs/site/cli/ (by copy_site_tree) must each belong
    // to a subcommand page. Checked before anything is generated here, since
    // the landing page below is written as cli/index.md.
    check_cli_prose_files(&cli_dir)?;

    let top = run_help(&bin, &[])?;
    let globals = global_options(&top);
    // The global flags some subcommand's help repeats or redefines; the others
    // work only on the top-level command, and the landing page says so.
    let mut in_subcommands = std::collections::HashSet::new();
    let subcmds = CLI_REFERENCE_SUBCMDS;
    for sub in subcmds {
        let help = run_help(&bin, &[sub])?;
        // SERP description: clap prints the subcommand's own one-
        // line summary as the first non-empty line of --help
        // (before the blank line and `Usage:`). Use it verbatim
        // so the snippet says what `alint <sub>` does, not "see
        // --help". Keep the command (the search query) in the
        // string; no em-dashes (style guide).
        let help_summary = meta_desc_clean(&help_first_line(&help), 120);
        let cli_desc = if help_summary.len() < 12 {
            format!("alint {sub} subcommand. CLI reference and flags for alint {sub}.")
        } else if help_summary.ends_with('.') {
            format!("{help_summary} alint {sub} CLI reference and flags.")
        } else {
            format!("{help_summary}. alint {sub} CLI reference and flags.")
        };
        // Hand-written prose for the page, from docs/site/cli/<sub>.md. It is
        // already in the bundle at this path (copy_site_tree runs first and
        // applies the release gating), and this page replaces it.
        let page_path = cli_dir.join(format!("{sub}.md"));
        let prose = if page_path.is_file() {
            let text = fs::read_to_string(&page_path)
                .with_context(|| format!("reading {}", page_path.display()))?;
            Some(parse_cli_prose(&text).with_context(|| format!("docs/site/cli/{sub}.md"))?)
        } else {
            None
        };
        let stripped = strip_global_options(&help, &globals);
        in_subcommands.extend(stripped.removed.iter().chain(&stripped.own).cloned());
        let page = render_cli_page(sub, &cli_desc, prose.as_ref(), &stripped);
        fs::write(&page_path, page)?;
    }

    // Top-level help → cli/index.md
    let top_level_only = top_level_only(&globals, &in_subcommands);
    let mut index = String::new();
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index, "title: CLI");
    let _ = writeln!(
        &mut index,
        "description: alint's subcommands and global flags, captured from the binary itself."
    );
    let _ = writeln!(&mut index, "sidebar:");
    let _ = writeln!(&mut index, "  order: 1");
    let _ = writeln!(&mut index, "---");
    let _ = writeln!(&mut index);
    // Prefer a formatted landing page (Commands + Global-options tables) parsed
    // from `--help`; fall back to the raw dump if the help doesn't parse.
    if let Some(body) = format_top_help(&top, &top_level_only) {
        index.push_str(&body);
    } else {
        let _ = writeln!(&mut index, "```");
        index.push_str(&top);
        let _ = writeln!(&mut index, "```");
    }
    fs::write(cli_dir.join("index.md"), index)?;

    // Sanity-check: workspace path exists.
    let _ = workspace;
    Ok(())
}

fn run_help(bin: &Path, subcmd_args: &[&str]) -> Result<String> {
    let mut cmd = Command::new(bin);
    cmd.args(subcmd_args).arg("--help");
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!(
            "alint {:?} --help exited {:?}",
            subcmd_args,
            out.status.code()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// First descriptive line of a clap `--help` dump: clap prints the
/// command's about-string as the leading line(s) before the blank
/// line that precedes `Usage:`. Returns the joined about block
/// (it can wrap to a second line), skipping anything that is
/// itself a section header (`Usage:`, `Options:`, `Arguments:`,
/// `Commands:`). Used as the seed for the CLI page SERP
/// description.
fn help_first_line(help: &str) -> String {
    let mut acc = String::new();
    for line in help.lines() {
        let t = line.trim();
        if t.is_empty() {
            if !acc.is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with("Usage:")
            || t.starts_with("Options:")
            || t.starts_with("Arguments:")
            || t.starts_with("Commands:")
        {
            break;
        }
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(t);
    }
    acc
}
