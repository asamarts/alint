//! The findings every `cross_file` relation builds: a violation, note or
//! analysis failure naming a path, the related location of the source, and
//! the redaction of extracted values a structured extract may hide.

use std::path::Path;

use alint_core::{AnalysisFailureKind, Context, Extract, RelatedLocation, Violation};

use super::CrossFileRule;

impl CrossFileRule {
    pub(super) fn add_source_locations(
        &self,
        ctx: &Context<'_>,
        finding: &mut Violation,
        label: &'static str,
    ) {
        if let Some(scope) = &self.source_glob {
            finding.set_related_locations(
                ctx.index
                    .files()
                    .filter(|entry| scope.matches(&entry.path, ctx.index))
                    .map(|entry| RelatedLocation::new(entry.path.as_ref(), label)),
            );
        } else {
            finding.set_related_locations(std::iter::once(RelatedLocation::new(
                Path::new(&self.source_file),
                label,
            )));
        }
    }

    pub(super) fn violation(path: &Path, reason: &str) -> Violation {
        Violation::new(format!("{}: {reason}", crate::slash(path))).with_path(path.to_path_buf())
    }

    pub(super) fn analysis_failure(
        path: &Path,
        kind: AnalysisFailureKind,
        reason: &str,
    ) -> Violation {
        Self::violation(path, reason).as_analysis_failure(kind)
    }

    /// An informational note (non-violation finding), such as a non-literal
    /// value the rule skipped rather than compared. The engine partitions notes
    /// before baseline and fixpoint identity processing, so they need no key.
    pub(super) fn note(path: &Path, reason: &str) -> Violation {
        Self::violation(path, reason).as_note()
    }

    pub(super) fn redact_extracted(
        &self,
        finding: &mut Violation,
        role: &str,
        path: &Path,
        extract: &Extract,
        category: &str,
        evidence: &[&str],
    ) {
        if !extract.redacts_by_default() {
            return;
        }
        let locator = format!("{role}\0{}\0{extract:?}\0{category}", crate::slash(path));
        finding.redact_evidence(&self.id, &locator, evidence.iter().copied());
    }
}
