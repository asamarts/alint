//! Human-readable formatter for [`Report`] and [`FixReport`].
//!
//! The check-renderer groups violations by file path (a
//! "Repository-level" bucket leads for path-less violations,
//! everything else is alphabetical by path), emits a terminal-
//! width-aware section header for each bucket, and formats each
//! violation with a colored level sigil, the rule id, an
//! optional `fixable` tag, and the message — prefixed with
//! `line:col` when available.
//!
//! Color, glyph-set, and terminal-width decisions all come from
//! [`HumanOptions`] (see [`crate::style`]). Every styled span is
//! written as `{STYLE}…{STYLE:#}`; the CLI's `AutoStream` decides
//! whether SGR escapes reach the terminal.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use alint_core::{FixReport, FixStatus, Level, Report, RuleResult, Violation};

use crate::sanitize::sanitize_terminal;
use crate::style::{self, GlyphSet, HumanOptions, write_hyperlink};

// ---------------------------------------------------------------
// Check report
// ---------------------------------------------------------------

pub fn write_human(report: &Report, w: &mut dyn Write, opts: HumanOptions) -> std::io::Result<()> {
    // Compact mode short-circuits the grouped layout entirely —
    // its audience is pipes / editors / `wc -l`, not humans
    // scanning output in a terminal.
    if opts.compact {
        return write_human_compact(report, w, &opts);
    }

    // All-clean banner — green check + concise line, no summary
    // block. Nothing else to render.
    if report.failing_rules() == 0 {
        let s = style::SUCCESS;
        let passing = report.passing_rules();
        writeln!(
            w,
            "{s}{} All {passing} rule(s) passed.{s:#}",
            opts.glyphs.success,
        )?;
        return Ok(());
    }

    // Bucket violations by path. `Option<Arc<Path>>` sorts `None`
    // before `Some`, which we want — repository-level gaps lead.
    // Cloning the Arc is an atomic refcount bump, not a path-byte
    // copy.
    let mut by_bucket: BTreeMap<Option<Arc<Path>>, Vec<(&RuleResult, &Violation)>> =
        BTreeMap::new();
    for result in &report.results {
        if result.passed() {
            continue;
        }
        for violation in &result.violations {
            by_bucket
                .entry(violation.path.clone())
                .or_default()
                .push((result, violation));
        }
    }

    let width = opts.effective_width();

    // Layout: one blank line between buckets (separates files
    // from each other) and one before the summary. No blank lines
    // within a bucket — visual separation between violations
    // already comes from the sigil/level anchor at column 2 vs.
    // the indented message continuation. Denser == easier to
    // scan a repo's worth of findings on one screen.
    let mut first_bucket = true;
    for (bucket, items) in &by_bucket {
        if !first_bucket {
            writeln!(w)?;
        }
        first_bucket = false;

        let label = bucket.as_ref().map_or_else(
            || "Repository-level".to_string(),
            // The path is attacker-controlled (a repo file name) — neutralize
            // any terminal escapes before it lands in the header (M8).
            |p| sanitize_terminal(&p.display().to_string()).into_owned(),
        );
        write_section_header(w, &label, width, &opts.glyphs)?;

        for (result, violation) in items {
            write_violation(w, result, violation, &opts)?;
        }
    }

    writeln!(w)?;
    write_summary(w, report, &opts.glyphs)?;
    Ok(())
}

/// Emit a `─── <label> ─────…` section header stretched to
/// `width` columns. Falls back gracefully when the label alone
/// exceeds the width (just emits `─── label`, no trailing fill).
fn write_section_header(
    w: &mut dyn Write,
    label: &str,
    width: usize,
    glyphs: &GlyphSet,
) -> std::io::Result<()> {
    let lead = format!("{r}{r}{r} {label} ", r = glyphs.rule);
    // chars().count() is a display-width approximation that
    // works for ASCII + the single-column Unicode glyphs we ship.
    let used = lead.chars().count();
    let tail_cols = width.saturating_sub(used);
    let tail: String = glyphs.rule.repeat(tail_cols);
    let s = style::DIM;
    writeln!(w, "{s}{lead}{tail}{s:#}")?;
    Ok(())
}

/// Render a single violation block:
///
/// ```text
///   ✗  error    rule-id                           fixable
///               3:12  Merge-conflict markers must not be committed.
///               docs: https://…
/// ```
///
/// Caller is responsible for the blank line before this block.
fn write_violation(
    w: &mut dyn Write,
    result: &RuleResult,
    violation: &Violation,
    opts: &HumanOptions,
) -> std::io::Result<()> {
    let (sigil, level_style, level_name) = level_presentation(result.level, &opts.glyphs);

    let rule_style = style::RULE_ID;
    // First line: indent + sigil + level + rule_id + optional `fixable` tag.
    // The rule id comes from the linted repo's own `.alint.yml`, so a hostile
    // repo can hide terminal escapes in it (YAML `\x1b` decodes to a real ESC
    // byte past the parser's raw-control-char check) — sanitize it like the
    // message + path (M8).
    let safe_rule_id = sanitize_terminal(&result.rule_id);
    if result.is_fixable {
        let fix = style::FIXABLE;
        writeln!(
            w,
            "  {level_style}{sigil}  {level_name}{level_style:#}  {rule_style}{safe_rule_id}{rule_style:#}   {fix}fixable{fix:#}",
        )?;
    } else {
        writeln!(
            w,
            "  {level_style}{sigil}  {level_name}{level_style:#}  {rule_style}{safe_rule_id}{rule_style:#}",
        )?;
    }

    // Message line. `MSG_INDENT` spaces align under the rule_id
    // (col 2 indent + 1 sigil + 2 spacer + 7 level + 2 spacer = 14).
    // Long messages wrap at `effective_width()` with continuation
    // lines re-indented to MSG_INDENT (v0.9.19+). Wrapping is
    // word-aware and falls back gracefully on long unbreakable
    // tokens (URLs, hashed identifiers, etc. emit on their own
    // line and let the terminal handle any overflow).
    let dim = style::DIM;
    let total_width = opts.effective_width();
    // The message can embed a matched value or a `kind: command` rule's
    // subprocess output — neutralize terminal escapes before wrapping, while
    // preserving the intentional `\n` paragraph breaks wrap_message honors (M8).
    let safe_message = sanitize_terminal(&violation.message);
    let lines = wrap_message(&safe_message, MSG_INDENT.len(), total_width);
    let (first_line, rest) = lines
        .split_first()
        .map_or(("", &[][..]), |(f, r)| (f.as_str(), r));
    match (violation.line, violation.column) {
        (Some(line), Some(col)) => {
            writeln!(w, "{MSG_INDENT}{dim}{line}:{col}{dim:#}  {first_line}")?;
        }
        (Some(line), None) => {
            writeln!(w, "{MSG_INDENT}{dim}line {line}{dim:#}  {first_line}")?;
        }
        _ => {
            writeln!(w, "{MSG_INDENT}{first_line}")?;
        }
    }
    for line in rest {
        writeln!(w, "{MSG_INDENT}{line}")?;
    }

    // Policy URL, if present. Printed once per violation to stay
    // near the relevant message (not once per rule as before —
    // that hid the link below the list). When the terminal
    // supports OSC 8, we wrap the URL as a clickable hyperlink.
    // Suppressed entirely when `opts.show_docs` is `false`
    // (`--no-docs`) so narrow terminals + screen recordings stay
    // visually clean.
    if opts.show_docs
        && let Some(url) = &result.policy_url
    {
        // Style swap on `opts.hyperlinks`: when OSC 8 is emitted the
        // terminal handles link styling itself (hover underline +
        // pointer cursor), so emitting our own `\e[4m` on top
        // causes some renderers — notably `asciinema-player` —
        // to extend the underline past the URL to the end of the
        // terminal row. Drop the explicit underline in that path;
        // keep it on the fallback path so non-OSC-8 terminals
        // still get the visual link cue.
        let docs = if opts.hyperlinks {
            style::DOCS_LINKED
        } else {
            style::DOCS
        };
        // The policy_url is also config-controlled; an embedded ESC/BEL would
        // otherwise close the OSC-8 hyperlink sequence early and let the tail be
        // interpreted as terminal control. Sanitize before emitting (target +
        // visible text).
        let safe_url = sanitize_terminal(url);
        write!(w, "{MSG_INDENT}{dim}docs:{dim:#} {docs}")?;
        write_hyperlink(w, &safe_url, &safe_url, opts.hyperlinks)?;
        writeln!(w, "{docs:#}")?;
    }
    Ok(())
}

/// Summary block: per-level counts, overall passing/failing/fixable
/// totals, and a `alint fix` call-to-action when anything's auto-fixable.
fn write_summary(w: &mut dyn Write, report: &Report, glyphs: &GlyphSet) -> std::io::Result<()> {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    let mut fixable_violations = 0usize;

    for r in &report.results {
        if r.passed() {
            continue;
        }
        let count = r.violations.len();
        if r.is_fixable {
            fixable_violations += count;
        }
        match r.level {
            Level::Error => errors += count,
            Level::Warning => warnings += count,
            Level::Info => infos += count,
            Level::Off => {} // filtered at config load; defensive skip
        }
    }

    let total = errors + warnings + infos;
    let failing = report.failing_rules();
    let passing = report.passing_rules();
    let dim = style::DIM;

    let plural = if total == 1 { "" } else { "s" };
    writeln!(w, "{dim}Summary ({total} violation{plural}):{dim:#}")?;

    // First line: per-level breakdown. Skip levels with zero count
    // to keep the line short on typical runs.
    let mut parts: Vec<String> = Vec::new();
    if errors > 0 {
        let s = style::ERROR;
        parts.push(format!(
            "{s}{} {errors} error{e}{s:#}",
            glyphs.error,
            e = if errors == 1 { "" } else { "s" }
        ));
    }
    if warnings > 0 {
        let s = style::WARNING;
        parts.push(format!(
            "{s}{} {warnings} warning{e}{s:#}",
            glyphs.warning,
            e = if warnings == 1 { "" } else { "s" }
        ));
    }
    if infos > 0 {
        let s = style::INFO;
        parts.push(format!("{s}{} {infos} info{s:#}", glyphs.info));
    }
    writeln!(w, "  {}", parts.join("   "))?;

    // Second line: rule-level counts and fixable total.
    let bullet = glyphs.bullet;
    let fixable_tag = if fixable_violations > 0 {
        let fix = style::FIXABLE;
        format!(" {dim}{bullet}{dim:#} {fix}{fixable_violations} auto-fixable{fix:#}")
    } else {
        String::new()
    };
    writeln!(
        w,
        "  {passing} passing {dim}{bullet}{dim:#} {failing} failing{fixable_tag}",
    )?;

    if fixable_violations > 0 {
        writeln!(w)?;
        let fix = style::FIXABLE;
        writeln!(
            w,
            "  {arrow} run {fix}`alint fix`{fix:#} to resolve {fixable_violations} fixable violation{p}.",
            arrow = glyphs.arrow,
            p = if fixable_violations == 1 { "" } else { "s" }
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------
// Compact renderer
// ---------------------------------------------------------------

/// One-line-per-violation rendering, `:`-separated so editor
/// problem-matchers / `grep` / `wc -l` can consume it directly.
///
/// Format:
///
/// ```text
/// <path>:<line>:<col>: <level>: <rule-id>: <message>[  [fixable]]
/// <path>: <level>: <rule-id>: <message>                  (no location)
/// <repo>: <level>: <rule-id>: <message>                  (repo-level)
/// ```
///
/// The `:line:col` suffix is emitted only when the violation actually
/// carries a location, so an editor / grep can still jump to it. A
/// whole-file or repo-level finding has nothing to point at, so the old
/// `:0:0` was noise that also misleadingly implied line 0 / col 0.
/// Path-less violations use the literal `<repo>`. The location prefix is
/// color-tagged (bright magenta + bold) so each finding's start is visually
/// obvious even without the grouped format's per-file separators; levels are
/// color-tagged too. The `AutoStream` strips SGR escapes when the sink
/// isn't a TTY, so pipe-safe `path:line:col` output is automatic.
fn write_human_compact(
    report: &Report,
    w: &mut dyn Write,
    opts: &HumanOptions,
) -> std::io::Result<()> {
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut infos = 0usize;
    let mut fixable = 0usize;

    for result in &report.results {
        if result.passed() {
            continue;
        }
        for v in &result.violations {
            // Path + message are attacker-controlled; neutralize terminal
            // escapes for this single-line format (M8).
            let path = v.path.as_ref().map_or_else(
                || "<repo>".to_string(),
                |p| sanitize_terminal(&p.display().to_string()).into_owned(),
            );
            let message = sanitize_terminal(&v.message);
            // Append `:line:col` only for findings that actually have a
            // location. A repo-level or whole-file finding points at nothing,
            // so `:0:0` was misleading noise. Located findings keep the full
            // `path:line:col` an editor / grep jumps on.
            let loc = match (v.line, v.column) {
                (Some(line), Some(col)) => format!("{path}:{line}:{col}"),
                _ => path,
            };
            let (level_style, level_name) = match result.level {
                Level::Error => {
                    errors += 1;
                    (style::ERROR, "error")
                }
                Level::Warning => {
                    warnings += 1;
                    (style::WARNING, "warning")
                }
                Level::Info => {
                    infos += 1;
                    (style::INFO, "info")
                }
                Level::Off => (style::DIM, "off"), // filtered earlier; defensive
            };
            if result.is_fixable {
                fixable += 1;
            }

            let rule_style = style::RULE_ID;
            let fix_tag = if result.is_fixable {
                let fix = style::FIXABLE;
                format!("  {fix}[fixable]{fix:#}")
            } else {
                String::new()
            };
            let safe_rule_id = sanitize_terminal(&result.rule_id);
            let loc_style = style::LOCATION;
            writeln!(
                w,
                "{loc_style}{loc}{loc_style:#}: {level_style}{level_name}{level_style:#}: {rule_style}{safe_rule_id}{rule_style:#}: {message}{fix_tag}",
            )?;
        }
    }

    // Trailing summary: one line, sentence-cased, no box. Stays
    // at stderr-style density so `alint check --compact | wc -l`
    // still counts only violations + summary (+1).
    if errors == 0 && warnings == 0 && infos == 0 {
        let s = style::SUCCESS;
        writeln!(w, "{s}{} all rules passed.{s:#}", opts.glyphs.success)?;
        return Ok(());
    }

    let mut parts: Vec<String> = Vec::new();
    if errors > 0 {
        let s = style::ERROR;
        parts.push(format!(
            "{s}{errors} error{p}{s:#}",
            p = if errors == 1 { "" } else { "s" }
        ));
    }
    if warnings > 0 {
        let s = style::WARNING;
        parts.push(format!(
            "{s}{warnings} warning{p}{s:#}",
            p = if warnings == 1 { "" } else { "s" }
        ));
    }
    if infos > 0 {
        let s = style::INFO;
        parts.push(format!("{s}{infos} info{s:#}"));
    }
    let mut line = parts.join(", ");
    if fixable > 0 {
        use std::fmt::Write as _;
        let fix = style::FIXABLE;
        write!(line, "; {fix}{fixable} auto-fixable{fix:#}").ok();
    }
    writeln!(w, "{line}.")?;
    Ok(())
}

// ---------------------------------------------------------------
// Fix report
// ---------------------------------------------------------------

/// Continuation indent for `write_fix_human` wrap output.
/// 4 cols sits under the `· `/`✓ ` glyph so wrapped lines align.
const FIX_INDENT: &str = "    ";

pub fn write_fix_human(
    report: &FixReport,
    w: &mut dyn Write,
    opts: HumanOptions,
) -> std::io::Result<()> {
    let dim = style::DIM;
    // v0.9.20: width-aware wrap for fix output. Status-suffix prose
    // ("(no fixer)", "(skipped: <reason>)") stays attached to the
    // message text — wrapped together so it never lands on a line
    // by itself looking orphaned.
    let total_width = opts.effective_width();

    for rule in &report.results {
        // Fix output uses un-padded level names — it's a flat
        // header per rule, no tabular alignment needed.
        let (level_style, level_name) = match rule.level {
            Level::Error => (style::ERROR, "error"),
            Level::Warning => (style::WARNING, "warning"),
            Level::Info => (style::INFO, "info"),
            Level::Off => (style::DIM, "off"),
        };
        let rule_style = style::RULE_ID;
        let safe_rule_id = sanitize_terminal(&rule.rule_id);
        writeln!(
            w,
            "{level_style}{level_name}{level_style:#} {rule_style}[{safe_rule_id}]{rule_style:#}:",
        )?;
        for item in &rule.items {
            // The applied summary already names the file it touched, so the
            // path prefix is added only to the message-based skipped / no-fixer
            // lines below, where the file would otherwise go unnamed.
            let path_prefix = item
                .violation
                .path
                .as_ref()
                .map(|p| format!("{} - ", p.display()))
                .unwrap_or_default();
            let (glyph, line_style_open, line_style_close, content) = match &item.status {
                FixStatus::Applied(summary) => {
                    let s = style::SUCCESS;
                    (
                        opts.glyphs.success,
                        format!("{s}"),
                        format!("{s:#}"),
                        summary.clone(),
                    )
                }
                FixStatus::Skipped(reason) => (
                    opts.glyphs.bullet,
                    format!("{dim}"),
                    format!("{dim:#}"),
                    format!(
                        "{path_prefix}{} (skipped: {reason})",
                        item.violation.message
                    ),
                ),
                FixStatus::Unfixable => (
                    opts.glyphs.bullet,
                    format!("{dim}"),
                    format!("{dim:#}"),
                    format!("{path_prefix}{} (no fixer)", item.violation.message),
                ),
            };
            // `content` embeds the attacker-controlled path + message (alint's
            // own status prose is clean ASCII); styling is applied separately
            // below, so sanitizing the whole string is safe (M8).
            let content = sanitize_terminal(&content);
            let lines = wrap_message(&content, FIX_INDENT.len(), total_width);
            let (first_line, rest) = lines
                .split_first()
                .map_or(("", &[][..]), |(f, r)| (f.as_str(), r));
            writeln!(
                w,
                "  {line_style_open}{glyph} {first_line}{line_style_close}"
            )?;
            for line in rest {
                writeln!(w, "{FIX_INDENT}{line_style_open}{line}{line_style_close}")?;
            }
        }
    }

    let applied = report.applied();
    let skipped = report.skipped();
    let unfixable = report.unfixable();
    let ok = style::SUCCESS;
    writeln!(
        w,
        "\n{ok}{applied} applied{ok:#}, {skipped} skipped, {unfixable} unfixable."
    )?;
    Ok(())
}

// ---------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------

/// Aligns message text under the `rule_id` on the first line.
const MSG_INDENT: &str = "              ";

/// Word-wrap `text` to fit within `total_width` columns, with
/// continuation lines indented by `indent` cols. Returns one
/// String per output line, **content only** (the caller emits the
/// indent itself before each line — keeps the styling/indent
/// concerns in one place per render path).
///
/// Whitespace-aware: breaks on ASCII spaces. Long unbreakable
/// tokens (URLs, hashed identifiers) get their own line and are
/// allowed to overflow rather than being broken mid-token.
/// Embedded newlines in `text` are honoured as paragraph breaks
/// and force a new line (each paragraph is wrapped independently).
///
/// Public since v0.9.20 so other commands' renderers (`alint
/// suggest`, `alint explain`, etc.) can apply consistent wrap
/// semantics to their own message-style output.
pub fn wrap_message(text: &str, indent: usize, total_width: usize) -> Vec<String> {
    let avail = total_width.saturating_sub(indent).max(20);
    let mut out: Vec<String> = Vec::new();
    if text.is_empty() {
        out.push(String::new());
        return out;
    }
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if current.is_empty() {
                current.push_str(word);
            } else if current.len() + 1 + word.len() <= avail {
                current.push(' ');
                current.push_str(word);
            } else {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            }
        }
        out.push(current);
    }
    out
}

/// Pick the sigil, style, and padded level name for a [`Level`].
/// Level names are padded to 7 chars so the `rule_id` column aligns
/// across errors / warnings / infos.
fn level_presentation(
    level: Level,
    glyphs: &GlyphSet,
) -> (&'static str, anstyle::Style, &'static str) {
    match level {
        Level::Error => (glyphs.error, style::ERROR, "error  "),
        Level::Warning => (glyphs.warning, style::WARNING, "warning"),
        Level::Info => (glyphs.info, style::INFO, "info   "),
        // `off` rules never reach the renderer — they're filtered
        // at config load — but map to something sane for test use.
        Level::Off => (glyphs.bullet, style::DIM, "off    "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_message_short_text_emits_one_line() {
        let out = wrap_message("hello world", 14, 80);
        assert_eq!(out, vec!["hello world".to_string()]);
    }

    #[test]
    fn wrap_message_wraps_on_word_boundary_at_avail_width() {
        // avail = 80 - 14 = 66 cols. Choose text that fits 65 chars
        // on the first line and a tail word that pushes past.
        let text = "a b c d e f g h i j k l m n o p q r s t u v w x y z aa bb cc dd ee ff gg";
        let out = wrap_message(text, 14, 80);
        assert!(out.len() >= 2, "expected wrap; got {out:?}");
        for line in &out {
            assert!(line.len() <= 66, "line over avail width: {line:?}");
        }
    }

    #[test]
    fn wrap_message_long_unbreakable_token_emits_on_own_line() {
        // A long URL has no spaces; it should land on its own line
        // and be allowed to overflow.
        let url = "https://example.com/very/long/path/with/many/segments/that/exceeds/the/wrap";
        let text = format!("see {url} for details");
        let out = wrap_message(&text, 14, 60);
        // First line: "see"
        // Then the URL on its own line (overflowing past 46-col avail
        // because no whitespace inside it)
        // Then "for details"
        assert!(
            out.iter().any(|l| l == url),
            "expected URL on its own line; got {out:?}",
        );
    }

    #[test]
    fn wrap_message_honours_explicit_newlines_as_paragraph_breaks() {
        let out = wrap_message("first paragraph\nsecond paragraph", 14, 80);
        assert_eq!(
            out,
            vec![
                "first paragraph".to_string(),
                "second paragraph".to_string(),
            ],
        );
    }

    #[test]
    fn wrap_message_empty_input_emits_one_empty_line() {
        let out = wrap_message("", 14, 80);
        assert_eq!(out, vec![String::new()]);
    }

    #[test]
    fn wrap_message_tiny_width_falls_back_to_min_avail() {
        // Even at width 10 (< indent 14), avail clamps to 20
        // so tokens up to 20 chars fit on one line.
        let out = wrap_message("twenty-char-token-ok", 14, 10);
        assert_eq!(out, vec!["twenty-char-token-ok".to_string()]);
    }

    // M8: the three human render paths must neutralize terminal escapes in
    // attacker-controlled paths/messages. alint emits its own SGR (e.g.
    // `\x1b[2m` dim) but never the clear-screen `\x1b[2J`, so a raw `\x1b[2J`
    // in the output would be an injection that slipped through; its
    // neutralized form `\x1b[2J` (literal text) proves the sanitizer ran.
    const RAW_CLEAR: &str = "\x1b[2J\x1b[H";

    /// Drop `ESC [ ... m` SGR sequences so a test can assert on the plain-text
    /// shape of colored output.
    fn strip_sgr(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' && chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                for e in chars.by_ref() {
                    if e == 'm' {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn evil_report() -> Report {
        use std::path::PathBuf;
        let v = Violation::new(format!("{RAW_CLEAR}forged: all rules passed."))
            .with_path(PathBuf::from(format!("src/{RAW_CLEAR}evil.rs")));
        // The rule_id and policy_url are ALSO config-controlled (a hostile repo's
        // own `.alint.yml`), so they must be sanitized too.
        Report {
            results: vec![RuleResult::new(
                format!("{RAW_CLEAR}evil-rule").into(),
                Level::Error,
                Some(format!("https://example.test/{RAW_CLEAR}x").into()),
                vec![v],
                true,
            )],
        }
    }

    fn assert_neutralized(out: &str) {
        assert!(
            !out.contains("\x1b[2J"),
            "a raw clear-screen escape survived: {out:?}"
        );
        assert!(
            out.contains("\\x1b[2J"),
            "expected the neutralized \\x1b[2J text: {out:?}"
        );
    }

    #[test]
    fn human_format_neutralizes_terminal_escapes() {
        let mut buf = Vec::new();
        write_human(&evil_report(), &mut buf, HumanOptions::default()).unwrap();
        assert_neutralized(&String::from_utf8(buf).unwrap());
    }

    #[test]
    fn compact_format_neutralizes_terminal_escapes() {
        let mut buf = Vec::new();
        let opts = HumanOptions {
            compact: true,
            ..HumanOptions::default()
        };
        write_human(&evil_report(), &mut buf, opts).unwrap();
        assert_neutralized(&String::from_utf8(buf).unwrap());
    }

    #[test]
    fn compact_omits_line_col_when_the_finding_has_no_location() {
        use std::path::PathBuf;
        // Three shapes: located (path + line/col), whole-file (path, no
        // line/col), and repo-level (no path at all).
        let located = Violation::new("located msg")
            .with_path(PathBuf::from("src/lib.rs"))
            .with_location(12, 5);
        let whole_file = Violation::new("whole-file msg").with_path(PathBuf::from("PLAN.md"));
        let repo_level = Violation::new("repo msg");
        let report = Report {
            results: vec![
                RuleResult::new("r-loc".into(), Level::Warning, None, vec![located], false),
                RuleResult::new(
                    "r-file".into(),
                    Level::Warning,
                    None,
                    vec![whole_file],
                    false,
                ),
                RuleResult::new("r-repo".into(), Level::Error, None, vec![repo_level], false),
            ],
        };
        let mut buf = Vec::new();
        let opts = HumanOptions {
            compact: true,
            ..HumanOptions::default()
        };
        write_human(&report, &mut buf, opts).unwrap();
        // Strip SGR so the assertions read against the plain text shape.
        let raw = String::from_utf8(buf).unwrap();
        let out = strip_sgr(&raw);
        assert!(
            out.contains("src/lib.rs:12:5: warning: r-loc: located msg"),
            "located finding keeps path:line:col for editor/grep jump:\n{out}"
        );
        assert!(
            out.contains("PLAN.md: warning: r-file: whole-file msg"),
            "whole-file finding drops the :0:0 noise:\n{out}"
        );
        assert!(
            out.contains("<repo>: error: r-repo: repo msg"),
            "repo-level finding is <repo> with no :0:0:\n{out}"
        );
        assert!(
            !out.contains(":0:0"),
            "no finding should render the misleading :0:0:\n{out}"
        );
    }

    #[test]
    fn fix_format_neutralizes_terminal_escapes() {
        use std::path::PathBuf;
        let v = Violation::new(format!("{RAW_CLEAR}forged"))
            .with_path(PathBuf::from(format!("src/{RAW_CLEAR}evil.rs")));
        let report = FixReport {
            results: vec![alint_core::FixRuleResult {
                rule_id: "demo".into(),
                level: Level::Warning,
                items: vec![alint_core::FixItem {
                    violation: v,
                    status: FixStatus::Unfixable,
                }],
            }],
        };
        let mut buf = Vec::new();
        write_fix_human(&report, &mut buf, HumanOptions::default()).unwrap();
        assert_neutralized(&String::from_utf8(buf).unwrap());
    }
}
