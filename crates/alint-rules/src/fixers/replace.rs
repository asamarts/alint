//! The `replace` located fixer (Phase 1).

use std::path::Path;

use alint_core::{
    Applicability, CollectedEdit, EditVerifier, FixContext, FixEdit, FixOutcome, Fixer, Result,
    Violation,
};
use regex::Regex;

/// Rewrites every span matching `pattern` with `replacement` (regex capture
/// substitution). A *located* fixer: `collect_edits` scans the file's bytes and
/// emits one [`FixEdit::ReplaceRange`] per match, so the engine batches, orders,
/// overlap-skips, verifies, and splices all matches in a single pass.
///
/// Paired with `file_content_forbidden`: a violation there means the banned
/// `pattern` IS present, so replacing each occurrence removes/rewrites it.
/// (`file_content_matches` violates on the pattern's *absence*, so there is
/// nothing for `replace` to act on -- it rejects the op instead.)
///
/// `Unsafe` by default: a regex rewrite is not behaviour-preserving in general,
/// so a bare `alint fix` surfaces it as a suggestion and `--unsafe-fixes` (or a
/// per-rule top-level promotion, when the rewrite is provably a normalization)
/// applies it.
///
/// CONVERGENCE: for a forbidden-pattern host, a replacement that itself still
/// matches the pattern is dropped (with a stderr warning) -- applying it would
/// leave the forbidden pattern present and grow the file across passes. This is
/// single-pass in Phase 1: it does NOT catch a re-match that forms across the
/// splice boundary with surrounding text; the Phase-1 fixpoint (whole-file
/// re-check + a loud non-convergence cap) is the complete guarantee.
#[derive(Debug)]
pub struct ReplaceFixer {
    pattern: Regex,
    replacement: String,
    applicability: Applicability,
}

impl ReplaceFixer {
    /// Construct from the host rule's compiled `pattern`, the configured
    /// `replacement` template, and the resolved tier (the builder passes
    /// `spec.applicability.unwrap_or(Applicability::Unsafe)`).
    pub fn new(pattern: Regex, replacement: String, applicability: Applicability) -> Self {
        Self {
            pattern,
            replacement,
            applicability,
        }
    }
}

impl Fixer for ReplaceFixer {
    fn describe(&self) -> String {
        format!("rewrite matches of /{}/", self.pattern.as_str())
    }

    fn applicability(&self) -> Applicability {
        self.applicability
    }

    fn collects_located_edits(&self) -> bool {
        true
    }

    fn collect_edits(
        &self,
        _violations: &[Violation],
        file: &Path,
        bytes: &[u8],
        _root: &Path,
    ) -> Vec<CollectedEdit> {
        // Regex operates on UTF-8 text; a non-UTF-8 file can't be matched (the
        // host rule doesn't flag one either), so emit no edits.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return Vec::new();
        };
        // One ReplaceRange per (non-overlapping, leftmost) match. `captures_iter`
        // yields disjoint matches, so the batch never self-overlaps; the byte
        // offsets are into the file's current bytes, exactly what ReplaceRange
        // wants. `expand` performs `$1` / `${name}` capture substitution.
        let mut nonconverging = 0usize;
        let edits: Vec<CollectedEdit> = self
            .pattern
            .captures_iter(text)
            .filter_map(|caps| {
                let m = caps.get(0)?;
                let mut content = String::new();
                caps.expand(&self.replacement, &mut content);
                // CONVERGENCE guard (this fixer only serves `file_content_forbidden`,
                // where the pattern is FORBIDDEN): a replacement that itself still
                // matches the pattern is not a valid fix -- applying it leaves the
                // forbidden pattern present and, across passes, GROWS the file
                // unboundedly (`foo`->`foofoo`; or a zero-width pattern like
                // `(?m)^` whose insert point is re-matched). Skip it: better a
                // clean no-op (the violation honestly stands, `check` still fails)
                // than a non-terminating rewrite. NOTE: a re-match forming across
                // the splice BOUNDARY with surrounding text (e.g. `aa`->`a` on
                // `aaa`) is NOT caught here -- the Phase-1 fixpoint's whole-file
                // re-check + loud non-convergence cap is the complete guarantee.
                if self.pattern.is_match(&content) {
                    nonconverging += 1;
                    return None;
                }
                Some(CollectedEdit {
                    edit: FixEdit::ReplaceRange {
                        path: file.to_path_buf(),
                        range: m.start()..m.end(),
                        content: content.into_bytes(),
                    },
                    applicability: self.applicability,
                    // A regex rewrite is not a structured op: correctness is
                    // byte-locality + the golden test, not a PutGet re-query.
                    verify: EditVerifier::None,
                    isolation_group: None,
                })
            })
            .collect();
        if nonconverging > 0 {
            // Surface the skip (the file otherwise silently keeps its violation),
            // matching the `fix_size_limit` over-cap warning's style.
            eprintln!(
                "alint: warning: {}: {nonconverging} `replace` match(es) left unfixed \
                 because the replacement still matches the pattern (a non-convergent \
                 config; the forbidden pattern would remain)",
                file.display()
            );
        }
        edits
    }

    // `apply` / `fix_edit` are the whole-file and LSP paths; a located fixer is
    // routed through `collect_edits` and never reaches `apply`. Provide honest
    // fallbacks (LSP TextEdit mapping for ReplaceRange lands with the located
    // LSP work, not here).
    fn apply(&self, _violation: &Violation, _ctx: &FixContext<'_>) -> Result<FixOutcome> {
        Ok(FixOutcome::Skipped(
            "replace is applied via the located-edit path".to_string(),
        ))
    }

    fn fix_edit(&self, _violation: &Violation, _bytes: &[u8], _root: &Path) -> Option<FixEdit> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(edits: &[CollectedEdit]) -> Vec<(usize, usize, String)> {
        edits
            .iter()
            .map(|e| match &e.edit {
                FixEdit::ReplaceRange { range, content, .. } => (
                    range.start,
                    range.end,
                    String::from_utf8_lossy(content).into_owned(),
                ),
                other => panic!("expected ReplaceRange, got {other:?}"),
            })
            .collect()
    }

    fn fixer(pattern: &str, replacement: &str) -> ReplaceFixer {
        ReplaceFixer::new(
            Regex::new(pattern).unwrap(),
            replacement.to_string(),
            Applicability::Unsafe,
        )
    }

    #[test]
    fn emits_one_range_per_match() {
        let f = fixer("TODO", "DONE");
        let edits = f.collect_edits(
            &[],
            Path::new("a.rs"),
            b"// TODO x\n// TODO y\n",
            Path::new("/r"),
        );
        assert_eq!(
            ranges(&edits),
            vec![(3, 7, "DONE".to_string()), (13, 17, "DONE".to_string())]
        );
    }

    #[test]
    fn expands_capture_references() {
        let f = fixer(r"v(\d+)", "version ${1}");
        let edits = f.collect_edits(&[], Path::new("a"), b"v12 and v3", Path::new("/r"));
        assert_eq!(
            ranges(&edits),
            vec![
                (0, 3, "version 12".to_string()),
                (8, 10, "version 3".to_string())
            ]
        );
    }

    #[test]
    fn no_match_emits_nothing() {
        let f = fixer("ZZZ", "x");
        assert!(
            f.collect_edits(&[], Path::new("a"), b"nothing here", Path::new("/r"))
                .is_empty()
        );
    }

    #[test]
    fn non_utf8_emits_nothing() {
        let f = fixer("x", "y");
        assert!(
            f.collect_edits(&[], Path::new("a"), &[0xff, 0xfe, b'x'], Path::new("/r"))
                .is_empty()
        );
    }

    #[test]
    fn skips_a_self_reintroducing_replacement() {
        // `foo`->`foofoo`: the replacement still contains `foo`, so applying it
        // would leave the forbidden pattern present and grow the file each pass.
        // The convergence guard drops such matches (no edit emitted).
        let f = fixer("foo", "foofoo");
        assert!(
            f.collect_edits(&[], Path::new("a"), b"foo bar foo\n", Path::new("/r"))
                .is_empty(),
            "a replacement that re-matches the pattern must not be emitted"
        );
        // A zero-width pattern whose insert point is re-matched is likewise dropped.
        let zw = fixer("(?m)^", "> ");
        assert!(
            zw.collect_edits(&[], Path::new("a"), b"one\ntwo\n", Path::new("/r"))
                .is_empty(),
            "a zero-width pattern that always re-matches must not be emitted"
        );
        // A resolving replacement is still emitted.
        let ok = fixer("TODO", "DONE");
        assert_eq!(
            ok.collect_edits(&[], Path::new("a"), b"TODO\n", Path::new("/r"))
                .len(),
            1,
            "a resolving replacement is applied"
        );
    }

    #[test]
    fn is_located_and_carries_its_tier() {
        let f = fixer("x", "y");
        assert!(f.collects_located_edits());
        assert_eq!(f.applicability(), Applicability::Unsafe);
        assert_eq!(
            ReplaceFixer::new(Regex::new("x").unwrap(), "y".into(), Applicability::Safe)
                .applicability(),
            Applicability::Safe
        );
        // The emitted edit carries the fixer's tier (Unsafe), not a hardcoded Safe.
        let edits = f.collect_edits(&[], Path::new("a"), b"x", Path::new("/r"));
        assert_eq!(edits[0].applicability, Applicability::Unsafe);
    }
}
