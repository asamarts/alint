//! `alint suggest` — scan a repo for known antipatterns and
//! propose rules that would catch them.
//!
//! Suggesters live under `suggest::suggesters::<family>` and
//! consume a [`Scan`] (cached repo-state). Each returns zero or
//! more [`Proposal`]s with a [`Confidence`] tag. The dispatcher
//! filters by the user's confidence floor and the
//! already-covered-by-existing-config set, then renders via
//! [`output::render`].
//!
//! Output strictly to stdout; progress / summary lines strictly
//! to stderr (via the [`crate::progress::Progress`] handle). See
//! `docs/design/v0.7/alint_suggest.md` for the full design.

mod output;
mod proposal;
mod scan;
mod suggesters;

use std::path::Path;
use std::process::ExitCode;
use std::str::FromStr;
use std::time::Instant;

use anyhow::Result;

pub use output::OutputFormat;
pub use proposal::{Confidence, Proposal};
pub use scan::Scan;

use crate::progress::Progress;

/// Options resolved from CLI flags. Threaded into [`run`].
#[derive(Debug)]
pub struct RunOptions {
    pub format: OutputFormat,
    pub confidence: Confidence,
    pub include_bundled: bool,
    pub explain: bool,
    pub quiet: bool,
}

impl FromStr for Confidence {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(format!(
                "invalid confidence {other:?}; expected one of `low`, `medium`, `high`"
            )),
        }
    }
}

impl FromStr for OutputFormat {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "human" => Ok(Self::Human),
            "yaml" => Ok(Self::Yaml),
            "json" => Ok(Self::Json),
            other => Err(format!(
                "invalid format {other:?}; expected one of `human`, `yaml`, `json`"
            )),
        }
    }
}

/// Top-level dispatch. Builds the scan, runs every suggester,
/// filters by confidence + already-covered, renders.
pub fn run(root: &Path, opts: &RunOptions, progress: &Progress) -> Result<ExitCode> {
    let started = Instant::now();
    progress.status("Scanning repository");
    let scan = Scan::collect(root, progress)?;

    let mut proposals: Vec<Proposal> = Vec::new();
    proposals.extend(suggesters::bundled::propose(&scan, progress));
    proposals.extend(suggesters::antipattern::propose(&scan, progress));
    proposals.extend(suggesters::todo_age::propose(&scan, progress));

    proposals.retain(|p| p.confidence >= opts.confidence);
    if !opts.include_bundled {
        proposals.retain(|p| !scan.config_already_covers(p));
    }
    // Stable ordering — confidence desc, then by rule_id ascending.
    proposals.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.rule_id().cmp(b.rule_id()))
    });

    output::render(&proposals, opts, &mut std::io::stdout())?;

    if !opts.quiet {
        let elapsed = started.elapsed();
        let summary = summarise(&proposals, elapsed);
        progress.summary(&summary);
    }
    Ok(ExitCode::SUCCESS)
}

fn summarise(proposals: &[Proposal], elapsed: std::time::Duration) -> String {
    let total = proposals.len();
    if total == 0 {
        return format!(
            "alint: 0 proposals — your config already looks tidy. ({:.1}s)",
            elapsed.as_secs_f64()
        );
    }
    let high = proposals
        .iter()
        .filter(|p| p.confidence == Confidence::High)
        .count();
    let med = proposals
        .iter()
        .filter(|p| p.confidence == Confidence::Medium)
        .count();
    let low = proposals
        .iter()
        .filter(|p| p.confidence == Confidence::Low)
        .count();
    let mut parts = Vec::new();
    if high > 0 {
        parts.push(format!("{high} high"));
    }
    if med > 0 {
        parts.push(format!("{med} medium"));
    }
    if low > 0 {
        parts.push(format!("{low} low"));
    }
    format!(
        "alint: {total} proposal{} ({}) — {:.1}s",
        if total == 1 { "" } else { "s" },
        parts.join(", "),
        elapsed.as_secs_f64(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_parses_three_levels() {
        assert_eq!("low".parse::<Confidence>().unwrap(), Confidence::Low);
        assert_eq!("medium".parse::<Confidence>().unwrap(), Confidence::Medium);
        assert_eq!("high".parse::<Confidence>().unwrap(), Confidence::High);
        assert!("critical".parse::<Confidence>().is_err());
    }

    #[test]
    fn output_format_parses_three_options() {
        assert_eq!(
            "human".parse::<OutputFormat>().unwrap(),
            OutputFormat::Human
        );
        assert_eq!("yaml".parse::<OutputFormat>().unwrap(), OutputFormat::Yaml);
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert!("xml".parse::<OutputFormat>().is_err());
    }
}
