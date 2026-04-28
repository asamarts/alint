//! `Proposal` — what a single suggester returns. The
//! dispatcher concatenates proposals across families, filters
//! by confidence + already-covered, then renders.

use std::cmp::Ordering;

/// Confidence rank. Ordered so [`Confidence::High`] sorts above
/// [`Confidence::Low`] in `cmp`. Used by `--confidence` floor
/// filtering and stable sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl PartialOrd for Confidence {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Confidence {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}

impl Confidence {
    fn rank(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
        }
    }

    /// Short human label for the renderer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

/// A single proposal. Two shapes: a bundled-ruleset extension
/// (just an `extends:` URI), or a fully-shaped rule entry. The
/// renderer dispatches on the shape (`BundledRuleset` vs.
/// `Rule`) so YAML / JSON output can surface them cleanly.
#[derive(Debug, Clone)]
pub enum ProposalKind {
    /// "Add this `extends:` line to your config."
    BundledRuleset {
        /// Canonical URI, e.g. `alint://bundled/agent-hygiene@v1`.
        uri: String,
    },
    /// "Add this rule entry under `rules:`."
    Rule {
        /// Rule kind (e.g. `git_blame_age`). Used for JSON
        /// emission and `--explain` headers.
        kind: String,
        /// The full YAML body the user can paste under
        /// `rules:`. Pre-rendered so suggesters control
        /// formatting (anchors, comments).
        yaml: String,
    },
}

/// Per-proposal evidence — one human-readable bullet shown
/// under `--explain`. Suggesters supply zero or more.
#[derive(Debug, Clone)]
pub struct Evidence {
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Proposal {
    pub id: String,
    pub kind: ProposalKind,
    pub confidence: Confidence,
    pub evidence: Vec<Evidence>,
    /// Suggester-specific summary line for the human renderer.
    /// Compact one-liner, distinct from `evidence` (which is
    /// only shown under `--explain`).
    pub summary: String,
}

impl Proposal {
    /// Identifier the dispatcher uses for sorting + already-
    /// covered detection. For `BundledRuleset` proposals this
    /// is the URI; for `Rule` proposals it's the user-chosen
    /// rule id.
    pub fn rule_id(&self) -> &str {
        &self.id
    }

    pub fn is_bundled(&self) -> bool {
        matches!(self.kind, ProposalKind::BundledRuleset { .. })
    }

    pub fn bundled_uri(&self) -> Option<&str> {
        match &self.kind {
            ProposalKind::BundledRuleset { uri } => Some(uri.as_str()),
            ProposalKind::Rule { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_orders_low_below_high() {
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
        assert!(Confidence::High > Confidence::Low);
    }

    #[test]
    fn confidence_floor_filters_with_partial_ord() {
        let xs = [Confidence::Low, Confidence::Medium, Confidence::High];
        let surviving: Vec<_> = xs.iter().filter(|c| **c >= Confidence::Medium).collect();
        assert_eq!(surviving.len(), 2);
    }

    #[test]
    fn proposal_bundled_helpers_round_trip() {
        let p = Proposal {
            id: "alint://bundled/rust@v1".into(),
            kind: ProposalKind::BundledRuleset {
                uri: "alint://bundled/rust@v1".into(),
            },
            confidence: Confidence::High,
            evidence: vec![],
            summary: "Cargo.toml at root".into(),
        };
        assert!(p.is_bundled());
        assert_eq!(p.bundled_uri(), Some("alint://bundled/rust@v1"));
        assert_eq!(p.rule_id(), "alint://bundled/rust@v1");
    }
}
