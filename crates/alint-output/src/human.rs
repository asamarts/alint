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
            |p| p.display().to_string(),
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
    if result.is_fixable {
        let fix = style::FIXABLE;
        writeln!(
            w,
            "  {level_style}{sigil}  {level_name}{level_style:#}  {rule_style}{}{rule_style:#}   {fix}fixable{fix:#}",
            result.rule_id,
        )?;
    } else {
        writeln!(
            w,
            "  {level_style}{sigil}  {level_name}{level_style:#}  {rule_style}{}{rule_style:#}",
            result.rule_id,
        )?;
    }

    // Message line. `MSG_INDENT` spaces align under the rule_id
    // (col 2 indent + 1 sigil + 2 spacer + 7 level + 2 spacer = 14).
    let dim = style::DIM;
    match (violation.line, violation.column) {
        (Some(line), Some(col)) => {
            writeln!(
                w,
                "{MSG_INDENT}{dim}{line}:{col}{dim:#}  {}",
                violation.message
            )?;
        }
        (Some(line), None) => {
            writeln!(
                w,
                "{MSG_INDENT}{dim}line {line}{dim:#}  {}",
                violation.message
            )?;
        }
        _ => {
            writeln!(w, "{MSG_INDENT}{}", violation.message)?;
        }
    }

    // Policy URL, if present. Printed once per violation to stay
    // near the relevant message (not once per rule as before —
    // that hid the link below the list). When the terminal
    // supports OSC 8, we wrap the URL as a clickable hyperlink.
    if let Some(url) = &result.policy_url {
        let docs = style::DOCS;
        write!(w, "{MSG_INDENT}{dim}docs:{dim:#} {docs}")?;
        write_hyperlink(w, url, url, opts.hyperlinks)?;
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
/// ```
///
/// Path-less violations use the literal `<repo>` so every line
/// parses uniformly. Missing line / col are rendered as `0`.
/// Levels are color-tagged to aid visual scanning even in
/// compact form; the `AutoStream` still strips SGR escapes when
/// the sink isn't a TTY, so pipe-safe output is automatic.
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
            let path = v
                .path
                .as_ref()
                .map_or_else(|| "<repo>".to_string(), |p| p.display().to_string());
            let line = v.line.unwrap_or(0);
            let col = v.column.unwrap_or(0);
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
            writeln!(
                w,
                "{path}:{line}:{col}: {level_style}{level_name}{level_style:#}: {rule_style}{}{rule_style:#}: {}{fix_tag}",
                result.rule_id, v.message,
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

pub fn write_fix_human(
    report: &FixReport,
    w: &mut dyn Write,
    opts: HumanOptions,
) -> std::io::Result<()> {
    let dim = style::DIM;
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
        writeln!(
            w,
            "{level_style}{level_name}{level_style:#} {rule_style}[{}]{rule_style:#}:",
            rule.rule_id
        )?;
        for item in &rule.items {
            let path = item
                .violation
                .path
                .as_ref()
                .map(|p| format!("{} — ", p.display()))
                .unwrap_or_default();
            match &item.status {
                FixStatus::Applied(summary) => {
                    let s = style::SUCCESS;
                    writeln!(w, "  {s}{} {path}{summary}{s:#}", opts.glyphs.success)?;
                }
                FixStatus::Skipped(reason) => {
                    writeln!(
                        w,
                        "  {dim}{} {path}{} (skipped: {reason}){dim:#}",
                        opts.glyphs.bullet, item.violation.message
                    )?;
                }
                FixStatus::Unfixable => {
                    writeln!(
                        w,
                        "  {dim}{} {path}{} (no fixer){dim:#}",
                        opts.glyphs.bullet, item.violation.message
                    )?;
                }
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
