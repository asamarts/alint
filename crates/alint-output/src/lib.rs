//! Output formatters. Each format converts an [`alint_core::Report`] into
//! bytes suitable for stdout or a file.

mod github;
mod human;
mod json;
mod sarif;

use std::io::Write;
use std::str::FromStr;

use alint_core::{FixReport, Report};

pub use github::write_github;
pub use human::{write_fix_human, write_human};
pub use json::{write_fix_json, write_json};
pub use sarif::write_sarif;

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
    pub fn write(self, report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
        match self {
            Self::Human => write_human(report, w),
            Self::Json => write_json(report, w),
            Self::Sarif => write_sarif(report, w),
            Self::Github => write_github(report, w),
        }
    }

    /// Write a fix-report. Only `Human` and `Json` are supported — SARIF
    /// and GitHub annotations describe findings, not remediations.
    pub fn write_fix(self, report: &FixReport, w: &mut dyn Write) -> std::io::Result<()> {
        match self {
            Self::Human | Self::Sarif | Self::Github => write_fix_human(report, w),
            Self::Json => write_fix_json(report, w),
        }
    }
}
