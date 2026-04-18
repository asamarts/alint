use std::io::Write;

use alint_core::{Level, Report};

pub fn write_human(report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
    let mut any = false;
    for result in &report.results {
        if result.passed() {
            continue;
        }
        any = true;
        let sigil = match result.level {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
            Level::Off => "off",
        };
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
        writeln!(
            w,
            "\n{failing} rule(s) failing, {passing} passing, {total} violation(s)."
        )?;
    } else {
        writeln!(w, "All {passing} rule(s) passed.")?;
    }
    Ok(())
}
