//! Output formatters. Each format converts an [`alint_core::Report`] into
//! bytes suitable for stdout or a file.

mod github;
mod human;
mod json;
mod junit;
mod markdown;
mod sarif;
pub mod style;

use std::io::Write;
use std::str::FromStr;

use alint_core::{FixReport, Report};

pub use github::write_github;
pub use human::{write_fix_human, write_human};
pub use json::{write_fix_json, write_json};
pub use junit::write_junit;
pub use markdown::{write_fix_markdown, write_markdown};
pub use sarif::write_sarif;
pub use style::{ColorChoice, GlyphSet, HumanOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
    Sarif,
    Github,
    Markdown,
    Junit,
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
        }
    }

    /// Write a fix-report. `Human`, `Json`, and `Markdown` have
    /// dedicated renderers; SARIF, GitHub annotations, and
    /// `JUnit` describe findings, not remediations, so they fall
    /// back to the human formatter for fix reports.
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
            Self::Human | Self::Sarif | Self::Github | Self::Junit => {
                write_fix_human(report, w, opts)
            }
            Self::Json => write_fix_json(report, w),
            Self::Markdown => write_fix_markdown(report, w),
        }
    }
}
