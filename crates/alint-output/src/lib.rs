//! Output formatters. Each format converts an [`alint_core::Report`] into
//! bytes suitable for stdout or a file.

mod agent;
mod diff;
mod github;
mod gitlab;
mod human;
mod json;
mod junit;
mod markdown;
mod sanitize;
mod sarif;
pub mod style;

use std::io::Write;
use std::str::FromStr;

use alint_core::{FixReport, Report};

/// A repo-relative path rendered with `/` separators, for the CI-consumed
/// machine formats (GitHub annotations, GitLab Code Quality, `JUnit`): a
/// Windows `\` separator would otherwise break their repo-file mapping. Only
/// the platform separator is rewritten -- on Unix `\` is a legal file-name
/// character and is kept.
pub(crate) fn slash_path(path: &std::path::Path) -> String {
    normalize_separators(&path.to_string_lossy(), std::path::MAIN_SEPARATOR)
}

fn normalize_separators(s: &str, separator: char) -> String {
    if separator == '/' {
        s.to_string()
    } else {
        s.replace(separator, "/")
    }
}

#[cfg(test)]
mod path_tests {
    use super::*;

    #[test]
    fn windows_separators_become_slashes() {
        assert_eq!(normalize_separators("src\\a\\b.rs", '\\'), "src/a/b.rs");
        // On a `/` platform a backslash is part of the file name.
        assert_eq!(normalize_separators("odd\\name.rs", '/'), "odd\\name.rs");
    }

    #[cfg(windows)]
    #[test]
    fn slash_path_normalizes_on_windows() {
        assert_eq!(
            slash_path(std::path::Path::new("src\\a\\b.rs")),
            "src/a/b.rs"
        );
    }
}

pub use agent::write_agent;
pub use diff::write_fix_diff;
pub use github::write_github;
pub use gitlab::write_gitlab;
pub use human::{wrap_message, write_fix_human, write_human};
pub use json::{write_fix_json, write_fix_json_with_mode, write_json, write_json_with_baseline};
pub use junit::write_junit;
pub use markdown::{write_fix_markdown, write_markdown};
pub use sanitize::sanitize_terminal;
pub use sarif::{
    DEFAULT_CONFIG_URI, write_sarif, write_sarif_for_config, write_sarif_with_baseline,
    write_sarif_with_fingerprints,
};
pub use style::{ColorChoice, GlyphSet, HumanOptions};

/// Per-result baseline output, threaded into the SARIF and JSON emitters so
/// they render baselined findings "marked, not removed" (SARIF) or counted
/// (JSON). Built by the CLI from [`alint_core::baseline::apply`]; the
/// [`per_result`](Self::per_result) vector is parallel to the live report's
/// `results`. Only the SARIF and JSON formatters consult it; the rest ignore
/// the baseline entirely and emit only the live (new) findings.
#[derive(Debug, Clone, Default)]
pub struct BaselineMarks {
    /// One entry per `Report.results`, in the same index/order.
    pub per_result: Vec<ResultMarks>,
    /// Total suppressed occurrences across all rules (for the JSON envelope).
    pub suppressed_total: u64,
}

/// The baseline marks for one rule's result.
#[derive(Debug, Clone, Default)]
pub struct ResultMarks {
    /// The fingerprint of each LIVE violation, parallel to the result's
    /// `violations` (so SARIF can stamp `partialFingerprints` on new findings).
    pub live_fingerprints: Vec<String>,
    /// The baselined (suppressed) findings of this rule, each with its
    /// fingerprint — re-emitted by SARIF as dismissed (`suppressions`).
    pub suppressed: Vec<SuppressedFinding>,
}

/// A baselined finding carried to the formatters: the violation plus its
/// matched baseline fingerprint.
#[derive(Debug, Clone)]
pub struct SuppressedFinding {
    pub violation: alint_core::Violation,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
    Sarif,
    Github,
    Markdown,
    Junit,
    Gitlab,
    Agent,
}

impl FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "human" | "pretty" | "text" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            "sarif" => Ok(Self::Sarif),
            "github" | "github-actions" => Ok(Self::Github),
            "markdown" | "md" => Ok(Self::Markdown),
            "junit" | "junit-xml" => Ok(Self::Junit),
            "gitlab" | "gitlab-codequality" | "code-quality" => Ok(Self::Gitlab),
            "agent" | "agentic" | "ai" => Ok(Self::Agent),
            other => Err(format!("unknown output format: {other}")),
        }
    }
}

impl Format {
    /// Write a check-report. Convenience wrapper that uses default
    /// [`HumanOptions`] (Unicode glyphs, no hyperlinks). Callers
    /// that care about glyph fallback or hyperlink support — i.e.
    /// the CLI — should use [`Format::write_with_options`].
    pub fn write(self, report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
        self.write_with_options(report, w, HumanOptions::default())
    }

    /// Like [`Format::write`], but with explicit rendering options.
    /// Only the `Human` format inspects `opts`; the others ignore it.
    pub fn write_with_options(
        self,
        report: &Report,
        w: &mut dyn Write,
        opts: HumanOptions,
    ) -> std::io::Result<()> {
        match self {
            Self::Human => write_human(report, w, opts),
            Self::Json => write_json(report, w),
            Self::Sarif => write_sarif(report, w),
            Self::Github => write_github(report, w),
            Self::Markdown => write_markdown(report, w),
            Self::Junit => write_junit(report, w),
            Self::Gitlab => write_gitlab(report, None, w),
            Self::Agent => write_agent(report, w),
        }
    }

    /// Write a fix-report. `Human`, `Json`, and `Markdown` have
    /// dedicated renderers; SARIF, GitHub annotations, `JUnit`,
    /// and `GitLab` Code Quality describe findings, not
    /// remediations, so they fall back to the human formatter
    /// for fix reports.
    pub fn write_fix(self, report: &FixReport, w: &mut dyn Write) -> std::io::Result<()> {
        self.write_fix_with_options(report, w, HumanOptions::default())
    }

    /// Like [`Format::write_fix`], but with explicit rendering options.
    pub fn write_fix_with_options(
        self,
        report: &FixReport,
        w: &mut dyn Write,
        opts: HumanOptions,
    ) -> std::io::Result<()> {
        self.write_fix_report(report, w, opts, false)
    }

    /// Like [`Format::write_fix_with_options`], for a run that may be a dry
    /// run: the JSON report carries `dry_run` so a preview is
    /// distinguishable from a real run. (The human and markdown renderers
    /// already word dry-run outcomes as "would ...".)
    pub fn write_fix_report(
        self,
        report: &FixReport,
        w: &mut dyn Write,
        opts: HumanOptions,
        dry_run: bool,
    ) -> std::io::Result<()> {
        match self {
            Self::Human
            | Self::Sarif
            | Self::Github
            | Self::Junit
            | Self::Gitlab
            // Agent format is check-side only; an agent confirming a
            // fix landed should re-run `alint check --format=agent`
            // against the now-modified tree. The fix-report itself
            // falls back to the human formatter so logs from
            // `alint fix --format=agent` still read sensibly.
            | Self::Agent => write_fix_human(report, w, opts),
            Self::Json => write_fix_json_with_mode(report, dry_run, w),
            Self::Markdown => write_fix_markdown(report, w),
        }
    }
}
