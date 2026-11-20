//! The occurrence numbers SARIF and GitLab output give the findings of a run
//! that share a fingerprint (ADR-0047): the canonical fingerprint repeats by
//! design (identical content, a hidden path, a `constraint` key), and both
//! formats need each finding to carry its own.

use std::path::Path;

use alint_core::Violation;

/// Where a finding stands among those that share its fingerprint: by the
/// path, line and column it shows, then its rule and message. Only what the
/// finding shows counts, so a hidden path sorts as `<redacted>`, and whether a
/// baseline suppresses the finding does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct OccurrenceKey<'v> {
    path: Option<&'v Path>,
    line: Option<usize>,
    column: Option<usize>,
    rule: &'v str,
    message: &'v str,
}

impl<'v> OccurrenceKey<'v> {
    /// The key of `violation`, a finding of rule `rule`.
    pub(crate) fn of(rule: &'v str, violation: &'v Violation) -> Self {
        Self {
            path: violation.report_path(),
            line: violation.line,
            column: violation.column,
            rule,
            message: &violation.message,
        }
    }
}

/// The occurrence of each of `findings`, a fingerprint and a key each, among
/// those with its fingerprint: 0 for the first in key order, then 1, 2 and
/// on, findings with equal keys in the order given. The numbers do not depend
/// on the order of `findings` otherwise.
pub(crate) fn occurrences<K: Ord>(findings: &[(&str, K)]) -> Vec<u32> {
    let mut order: Vec<usize> = (0..findings.len()).collect();
    order.sort_by(|&a, &b| {
        findings[a]
            .0
            .cmp(findings[b].0)
            .then_with(|| findings[a].1.cmp(&findings[b].1))
            .then(a.cmp(&b))
    });
    let mut numbers = vec![0; findings.len()];
    let mut previous: Option<&str> = None;
    let mut count = 0;
    for at in order {
        let fingerprint = findings[at].0;
        if previous == Some(fingerprint) {
            count += 1;
        } else {
            count = 0;
            previous = Some(fingerprint);
        }
        numbers[at] = count;
    }
    numbers
}

/// The fingerprint of occurrence `occurrence` (N > 0) of `base` in a run: the
/// hex SHA-256 of `base`, `:` and the occurrence as four little-endian bytes.
pub(crate) fn occurrence_fingerprint(base: &str, occurrence: u32) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hasher = Sha256::new();
    hasher.update(base.as_bytes());
    hasher.update(b":");
    hasher.update(occurrence.to_le_bytes());
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each fingerprint's findings are numbered in key order, whatever order
    /// they come in; equal keys keep theirs.
    #[test]
    fn findings_are_numbered_by_key_within_their_fingerprint() {
        let findings = [("x", 3), ("y", 1), ("x", 1), ("x", 2), ("y", 0), ("x", 1)];
        assert_eq!(occurrences(&findings), [3, 1, 0, 2, 0, 1]);
        let mut reversed = findings;
        reversed.reverse();
        assert_eq!(occurrences(&reversed), [0, 0, 2, 1, 1, 3]);
    }
}
