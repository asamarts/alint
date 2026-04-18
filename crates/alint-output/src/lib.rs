//! Output formatters. Each format converts an [`alint_core::Report`] into
//! bytes suitable for stdout or a file.

mod human;
mod json;

use std::io::Write;
use std::str::FromStr;

use alint_core::Report;

pub use human::write_human;
pub use json::write_json;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
}

impl FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "human" | "pretty" | "text" => Ok(Self::Human),
            "json" => Ok(Self::Json),
            other => Err(format!("unknown output format: {other}")),
        }
    }
}

impl Format {
    pub fn write(self, report: &Report, w: &mut dyn Write) -> std::io::Result<()> {
        match self {
            Self::Human => write_human(report, w),
            Self::Json => write_json(report, w),
        }
    }
}
