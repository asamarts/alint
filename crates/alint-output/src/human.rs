use std::io::Write;

use alint_core::{FixReport, FixStatus, Level, Report};

use crate::style::{self, HumanOptions};

pub fn write_human(report: &Report, w: &mut dyn Write, opts: HumanOptions) -> std::io::Result<()> {
    // Unused for now — Phase 2 will consume the glyph set when the
    // renderer gets grouped/aligned. Accepted at the signature so
    // the CLI→formatter wiring is in place from Phase 1.
    let _ = opts;

    let mut any = false;
    for result in &report.results {
        if result.passed() {
            continue;
        }
        any = true;
        let sigil = level_sigil(result.level);
        writeln!(w, "{sigil}[{}]:", result.rule_id)?;
        for v in &result.violations {
            if let Some(path) = &v.path {
                writeln!(w, "  - {} — {}", path.display(), v.message)?;
            } else {
                writeln!(w, "  - {}", v.message)?;
            }
        }
        if let Some(url) = &result.policy_url {
            writeln!(w, "  policy: {url}")?;
        }
    }

    let failing = report.failing_rules();
    let passing = report.passing_rules();
    let total = report.total_violations();
    if any {
        // Minimal Phase 1 styling: color the failing/passing counts.
        // Phase 2 will rebuild the whole summary; for now this is
        // just a smoke test that colors survive the AutoStream.
        let err_style = style::ERROR;
        let pass_style = style::SUCCESS;
        writeln!(
            w,
            "\n{err_style}{failing} rule(s) failing{err_style:#}, \
             {pass_style}{passing} passing{pass_style:#}, {total} violation(s).",
        )?;
    } else {
        let ok_style = style::SUCCESS;
        writeln!(w, "{ok_style}All {passing} rule(s) passed.{ok_style:#}")?;
    }
    Ok(())
}

pub fn write_fix_human(
    report: &FixReport,
    w: &mut dyn Write,
    opts: HumanOptions,
) -> std::io::Result<()> {
    let _ = opts;
    for rule in &report.results {
        let sigil = level_sigil(rule.level);
        writeln!(w, "{sigil}[{}]:", rule.rule_id)?;
        for item in &rule.items {
            let path = item
                .violation
                .path
                .as_ref()
                .map(|p| format!("{} — ", p.display()))
                .unwrap_or_default();
            match &item.status {
                FixStatus::Applied(summary) => {
                    writeln!(w, "  ✓ {path}{summary}")?;
                }
                FixStatus::Skipped(reason) => {
                    writeln!(
                        w,
                        "  · {path}{} (skipped: {reason})",
                        item.violation.message
                    )?;
                }
                FixStatus::Unfixable => {
                    writeln!(w, "  · {path}{} (no fixer)", item.violation.message)?;
                }
            }
        }
    }

    let applied = report.applied();
    let skipped = report.skipped();
    let unfixable = report.unfixable();
    writeln!(
        w,
        "\n{applied} applied, {skipped} skipped, {unfixable} unfixable."
    )?;
    Ok(())
}

fn level_sigil(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Info => "info",
        Level::Off => "off",
    }
}
