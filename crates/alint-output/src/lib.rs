//! Output formatters. Each format converts an [`alint_core::Report`] into
//! bytes suitable for stdout or a file.

mod github;
mod human;
mod json;
mod sarif;
pub mod style;

use std::io::Write;
use std::str::FromStr;

use alint_core::{FixReport, Report};

pub use github::write_github;
pub use human::{write_fix_human, write_human};
pub use json::{write_fix_json, write_json};
pub use sarif::write_sarif;
pub use style::{ColorChoice, GlyphSet, HumanOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
    Sarif,
    Github,
}

impl FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "human" | "pretty" | "text" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            "sarif" => Ok(Self::Sarif),
            "github" | "github-actions" => Ok(Self::Github),
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
        }
    }

    /// Write a fix-report. Only `Human` and `Json` are supported — SARIF
    /// and GitHub annotations describe findings, not remediations.
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
            Self::Human | Self::Sarif | Self::Github => write_fix_human(report, w, opts),
            Self::Json => write_fix_json(report, w),
        }
    }
}
