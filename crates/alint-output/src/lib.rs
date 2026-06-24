//! Output formatters. Each format converts an [`alint_core::Report`] into
//! bytes suitable for stdout or a file.

mod agent;
mod github;
mod gitlab;
mod human;
mod json;
mod junit;
mod markdown;
mod sarif;
pub mod style;

use std::io::Write;
use std::str::FromStr;

use alint_core::{FixReport, Report};

pub use agent::write_agent;
pub use github::write_github;
pub use gitlab::write_gitlab;
pub use human::{wrap_message, write_fix_human, write_human};
pub use json::{write_fix_json, write_json, write_json_with_baseline};
pub use junit::write_junit;
pub use markdown::{write_fix_markdown, write_markdown};
pub use sarif::{write_sarif, write_sarif_with_baseline};
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
            Self::Gitlab => write_gitlab(report, w),
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
            Self::Json => write_fix_json(report, w),
            Self::Markdown => write_fix_markdown(report, w),
        }
    }
}
