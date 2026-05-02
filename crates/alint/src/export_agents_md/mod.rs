//! `alint export-agents-md` — generate (or maintain a section
//! of) `AGENTS.md` from the active rule set, so the agent's
//! pre-prompt directives stay in sync with the lint config.
//!
//! Two output formats:
//!
//! - **markdown** (default) — section-per-severity bullet
//!   list shaped for an `AGENTS.md` / `CLAUDE.md` /
//!   `.cursorrules` directive block.
//! - **json** — stable shape behind `schema_version: 1`,
//!   parallel to the `suggest` JSON envelope.
//!
//! Two output destinations:
//!
//! - **stdout** — default; pipe / paste into the agent-context
//!   file by hand.
//! - **file (--output)** — overwrite-create the named path.
//! - **inline (--inline)** — splice between
//!   `<!-- alint:start -->` / `<!-- alint:end -->` markers in
//!   an existing file. The canonical workflow.
//!
//! See `docs/design/v0.7/alint_export_agents_md.md` for the
//! full design.

mod markdown;
mod splice;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use alint_core::Level;
use anyhow::{Context, Result, bail};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Markdown,
    Json,
}

impl FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "markdown" => Ok(Self::Markdown),
            "json" => Ok(Self::Json),
            other => Err(format!(
                "invalid format {other:?}; expected one of `markdown`, `json`"
            )),
        }
    }
}

/// A single rule converted into directive form for export.
#[derive(Debug, Clone)]
#[allow(clippy::struct_field_names)] // `directive` is the right English word for the field; collision with the type name is incidental.
pub struct Directive {
    pub rule_id: String,
    pub severity: Level,
    /// The rule's `message:` field, falling back to a synthesised
    /// `<kind> rule` line when the user didn't supply one. Always
    /// non-empty so renderers don't emit blank bullets.
    pub directive: String,
    pub policy_url: Option<String>,
}

/// Resolved CLI options.
#[derive(Debug)]
pub struct RunOptions {
    pub format: OutputFormat,
    pub output: Option<PathBuf>,
    pub inline: bool,
    pub section_title: String,
    pub include_info: bool,
}

pub fn run(config_path: Option<&Path>, opts: &RunOptions) -> Result<ExitCode> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let resolved_config_path = match config_path {
        Some(p) => p.to_path_buf(),
        None => alint_dsl::discover(&cwd).ok_or_else(|| {
            anyhow::anyhow!("no .alint.yml found (searched from {})", cwd.display())
        })?,
    };
    let config = alint_dsl::load(&resolved_config_path)
        .with_context(|| format!("loading {}", resolved_config_path.display()))?;

    let directives = collect_directives(&config, opts.include_info);
    let body = match opts.format {
        OutputFormat::Markdown => markdown::render(&directives, &opts.section_title),
        OutputFormat::Json => render_json(&directives, &opts.section_title)?,
    };

    write_output(&body, opts)?;
    Ok(ExitCode::SUCCESS)
}

/// Convert every config rule into a [`Directive`], filtering by
/// severity. Skips `level: off` always; skips `level: info`
/// unless `include_info` is true. Sort is stable: severity desc
/// then `rule_id` asc.
fn collect_directives(config: &alint_core::Config, include_info: bool) -> Vec<Directive> {
    let mut out: Vec<Directive> = config
        .rules
        .iter()
        .filter(|spec| !matches!(spec.level, Level::Off))
        .filter(|spec| include_info || !matches!(spec.level, Level::Info))
        .map(|spec| Directive {
            rule_id: spec.id.clone(),
            severity: spec.level,
            directive: spec
                .message
                .clone()
                .unwrap_or_else(|| format!("{} rule", spec.kind)),
            policy_url: spec.policy_url.clone(),
        })
        .collect();
    out.sort_by(|a, b| {
        severity_rank(b.severity)
            .cmp(&severity_rank(a.severity))
            .then_with(|| a.rule_id.cmp(&b.rule_id))
    });
    out
}

fn severity_rank(level: Level) -> u8 {
    match level {
        Level::Error => 3,
        Level::Warning => 2,
        Level::Info => 1,
        Level::Off => 0,
    }
}

// ─── JSON renderer ────────────────────────────────────────────

#[derive(Serialize)]
struct JsonReport<'a> {
    schema_version: u32,
    format: &'static str,
    section_title: &'a str,
    generated_at: String,
    directives: Vec<JsonDirective<'a>>,
}

#[derive(Serialize)]
struct JsonDirective<'a> {
    rule_id: &'a str,
    severity: &'static str,
    directive: &'a str,
    policy_url: Option<&'a str>,
}

fn render_json(directives: &[Directive], section_title: &str) -> Result<String> {
    let report = JsonReport {
        schema_version: 1,
        format: "agents-md",
        section_title,
        generated_at: timestamp_now(),
        directives: directives
            .iter()
            .map(|d| JsonDirective {
                rule_id: &d.rule_id,
                severity: severity_label(d.severity),
                directive: &d.directive,
                policy_url: d.policy_url.as_deref(),
            })
            .collect(),
    };
    let mut s = serde_json::to_string_pretty(&report).context("encoding json")?;
    s.push('\n');
    Ok(s)
}

fn severity_label(level: Level) -> &'static str {
    match level {
        Level::Error => "error",
        Level::Warning => "warning",
        Level::Info => "info",
        Level::Off => "off",
    }
}

// ─── output dispatch ──────────────────────────────────────────

fn write_output(body: &str, opts: &RunOptions) -> Result<()> {
    match (&opts.output, opts.inline) {
        (None, false) => {
            // stdout — `cmd_export_agents_md` configured the
            // anstream wrapper.
            std::io::stdout()
                .write_all(body.as_bytes())
                .context("writing to stdout")?;
            std::io::stdout().flush().context("flush stdout")?;
            Ok(())
        }
        (Some(path), false) => {
            fs::write(path, body).with_context(|| format!("writing {}", path.display()))?;
            Ok(())
        }
        (Some(path), true) => splice::splice_inline(path, body),
        (None, true) => bail!("--inline requires --output to point at the target file"),
    }
}

// ─── timestamp (lifted from suggest::output) ─────────────────

fn timestamp_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map_or(0, |d| d.as_secs());
    let (year, month, day, hour, min, sec) = epoch_to_civil(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

#[allow(
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation
)]
fn epoch_to_civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    // Identical to `crate::suggest::output::epoch_to_civil`.
    // Duplication is cheap and avoids cross-module coupling
    // for two fns that compute the same RFC-3339 stamp.
    let days = (secs / 86_400) as i64;
    let time_of_day = secs % 86_400;
    let hour = (time_of_day / 3600) as u32;
    let min = ((time_of_day / 60) % 60) as u32;
    let sec = (time_of_day % 60) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y_final = if m <= 2 { y + 1 } else { y };
    (y_final, m as u32, d as u32, hour, min, sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level_off_rule() -> alint_core::RuleSpec {
        spec("disabled", Level::Off, None)
    }

    fn level_info_rule() -> alint_core::RuleSpec {
        spec("nudge", Level::Info, Some("informational"))
    }

    fn spec(id: &str, level: Level, msg: Option<&str>) -> alint_core::RuleSpec {
        alint_core::RuleSpec {
            id: id.into(),
            kind: "file_exists".into(),
            level,
            paths: None,
            message: msg.map(str::to_string),
            policy_url: None,
            when: None,
            fix: None,
            git_tracked_only: false,
            scope_filter: None,
            extra: serde_yaml_ng::Mapping::new(),
        }
    }

    fn config_with(rules: Vec<alint_core::RuleSpec>) -> alint_core::Config {
        alint_core::Config {
            version: 1,
            rules,
            facts: Vec::new(),
            extends: Vec::new(),
            vars: std::collections::HashMap::new(),
            respect_gitignore: true,
            ignore: Vec::new(),
            fix_size_limit: None,
            nested_configs: false,
        }
    }

    #[test]
    fn off_rules_always_skipped() {
        let cfg = config_with(vec![
            level_off_rule(),
            spec("err", Level::Error, Some("must not")),
        ]);
        let dirs = collect_directives(&cfg, true);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].rule_id, "err");
    }

    #[test]
    fn info_skipped_unless_include_info() {
        let cfg = config_with(vec![
            level_info_rule(),
            spec("warn", Level::Warning, Some("avoid")),
        ]);
        // Default: info hidden.
        let dirs = collect_directives(&cfg, false);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].rule_id, "warn");
        // With --include-info: visible.
        let dirs_with = collect_directives(&cfg, true);
        assert_eq!(dirs_with.len(), 2);
    }

    #[test]
    fn missing_message_falls_back_to_kind() {
        let cfg = config_with(vec![spec("no-msg", Level::Warning, None)]);
        let dirs = collect_directives(&cfg, false);
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0].directive, "file_exists rule");
    }

    #[test]
    fn sorts_severity_desc_then_id_asc() {
        let cfg = config_with(vec![
            spec("z-warn", Level::Warning, Some("z")),
            spec("a-err", Level::Error, Some("a")),
            spec("b-warn", Level::Warning, Some("b")),
            spec("a-info", Level::Info, Some("ai")),
        ]);
        let dirs = collect_directives(&cfg, true);
        let order: Vec<&str> = dirs.iter().map(|d| d.rule_id.as_ref()).collect();
        assert_eq!(order, vec!["a-err", "b-warn", "z-warn", "a-info"]);
    }

    #[test]
    fn epoch_to_civil_round_trips_2026_04_28() {
        // Same fixture as suggest::output exercises — guards
        // against drift between the two duplicated impls.
        assert_eq!(epoch_to_civil(1_777_334_400), (2026, 4, 28, 0, 0, 0));
    }

    #[test]
    fn output_format_parses_two_options() {
        assert_eq!(
            "markdown".parse::<OutputFormat>().unwrap(),
            OutputFormat::Markdown
        );
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert!("yaml".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn json_envelope_is_stable_shape() {
        let directives = vec![Directive {
            rule_id: "no-debugger".into(),
            severity: Level::Error,
            directive: "no debugger statements".into(),
            policy_url: None,
        }];
        let s = render_json(&directives, "Lint rules enforced by alint").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed["schema_version"], 1);
        assert_eq!(parsed["format"], "agents-md");
        assert_eq!(parsed["directives"][0]["rule_id"], "no-debugger");
        assert_eq!(parsed["directives"][0]["severity"], "error");
    }
}
