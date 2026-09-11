//! The batched located-edit path: tier-filter, total-order sort,
//! overlap-skip, isolation-group exclusion, per-edit verify, and splice.
//!
//! This is pure over one file's bytes (no I/O): the engine hands in the
//! current bytes and a batch of [`CollectedEdit`]s that all target that
//! file, and gets back the new bytes plus a per-edit outcome. The engine
//! writes the result.
//!
//! It is **dormant in Phase 0**: every shipped fix op is whole-file and
//! flows through the `apply()`-based regime, so no [`FixEdit::ReplaceRange`]
//! reaches here in production. It is unit-tested directly and is first
//! wired to a real op by the Phase-1 `replace`. The verify step's
//! `Structured` arm is first exercised in production by the Phase-2
//! `set_value` / `remove_value` ops.

use std::collections::HashSet;
use std::ops::Range;

use serde_json_path::JsonPath;

use crate::rule::{Applicability, CollectedEdit, EditVerifier, ExpectedValue, FixEdit, GroupId};
use crate::structured_format::Format;

/// One located edit plus the provenance the total-order sort needs. The
/// engine assigns `rule_index` in flattened rule-discovery order (so
/// nested-config ordering is deterministic) and `violation_index` per
/// violation within a rule.
#[derive(Debug, Clone)]
pub struct LocatedEdit {
    pub rule_index: usize,
    pub violation_index: usize,
    pub collected: CollectedEdit,
}

/// What the batch did with one located edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatedOutcome {
    /// Spliced into the output.
    Applied,
    /// Not applied but surfaced to the user: tier below the threshold, a
    /// `Suggestion`-tier edit, or an edit whose post-edit verification
    /// failed (the engine declined to write rather than corrupt the file).
    Suggested,
    /// Lost the total-order race to an overlapping edit, or shared an
    /// isolation group with an edit ordered earlier.
    SkippedConflict,
    /// `Never` tier (neither applied nor suggested): collected for
    /// provenance only.
    Dropped,
}

/// The total order the plan pins: `(start, end, rule_index,
/// violation_index)`. Deterministic across runs because `rule_index`
/// spans the flattened root+nested rule list in discovery order.
fn sort_key(e: &LocatedEdit) -> (usize, usize, usize, usize) {
    let (start, end) =
        edit_range(&e.collected.edit).map_or((usize::MAX, usize::MAX), |r| (r.start, r.end));
    (start, end, e.rule_index, e.violation_index)
}

/// The byte range a located edit touches, or `None` for a non-located
/// (whole-file / metadata) edit that should never be in a located batch.
fn edit_range(edit: &FixEdit) -> Option<Range<usize>> {
    match edit {
        FixEdit::ReplaceRange { range, .. } => Some(range.clone()),
        _ => None,
    }
}

/// Apply `batch` — all targeting one file whose current bytes are
/// `original` — at `threshold`, returning the new bytes and each input
/// edit's outcome (paired, in input order).
///
/// Steps, in order: total-order sort; tier-filter (an edit that does not
/// apply at `threshold` becomes `Suggested` or `Dropped` and reserves no
/// range); overlap-skip (an accepted edit reserves its half-open range, a
/// later edit that starts before the last reserved end is a
/// `SkippedConflict`); isolation-group exclusion (an edit sharing a group
/// with an already-accepted edit is a `SkippedConflict` even when
/// byte-disjoint); splice; then verify — each accepted `Structured` edit
/// is checked against the spliced bytes and demoted to `Suggested` on
/// failure, after which the survivors are re-spliced.
#[must_use]
pub fn apply_file_edits(
    original: &[u8],
    batch: Vec<LocatedEdit>,
    threshold: Applicability,
) -> (Vec<u8>, Vec<(LocatedEdit, LocatedOutcome)>) {
    let mut order: Vec<usize> = (0..batch.len()).collect();
    order.sort_by(|&a, &b| sort_key(&batch[a]).cmp(&sort_key(&batch[b])));

    let mut outcome = vec![LocatedOutcome::Dropped; batch.len()];
    let mut accepted: Vec<usize> = Vec::new();
    let mut used_groups: HashSet<GroupId> = HashSet::new();
    let mut reserved_end: Option<usize> = None;

    for &i in &order {
        let ce = &batch[i].collected;
        if !ce.applicability.applies_at(threshold) {
            outcome[i] = if ce.applicability.suggested_at(threshold) {
                LocatedOutcome::Suggested
            } else {
                LocatedOutcome::Dropped
            };
            continue;
        }
        let Some(range) = edit_range(&ce.edit) else {
            // A whole-file edit does not belong in a located batch; drop it
            // defensively rather than mis-splice.
            outcome[i] = LocatedOutcome::Dropped;
            continue;
        };
        if reserved_end.is_some_and(|end| range.start < end) {
            outcome[i] = LocatedOutcome::SkippedConflict;
            continue;
        }
        if let Some(g) = ce.isolation_group {
            if !used_groups.insert(g) {
                outcome[i] = LocatedOutcome::SkippedConflict;
                continue;
            }
        }
        reserved_end = Some(range.end);
        accepted.push(i);
        outcome[i] = LocatedOutcome::Applied;
    }

    // Splice the accepted edits, then verify. A `Structured` edit whose
    // post-edit query does not match its expectation is demoted to
    // `Suggested` rather than written. Demotion changes the spliced bytes,
    // so the still-applied edits are re-verified against the new bytes until
    // the applied set is stable: an edit is applied only if it verifies in
    // the presence of the others that survive alongside it. Each pass
    // demotes at least one edit or stops, so it terminates in at most
    // `accepted.len()` passes. Dormant in Phase 0 (no `Structured` verifier
    // ships until Phase 2); the loop guarantees correctness when they do.
    let mut result = splice(original, &accepted, &batch);
    loop {
        let mut newly_demoted = false;
        for &i in &accepted {
            if outcome[i] != LocatedOutcome::Applied {
                continue;
            }
            if let EditVerifier::Structured {
                format,
                query,
                expect,
            } = &batch[i].collected.verify
            {
                if !verify_structured(&result, *format, query, expect) {
                    outcome[i] = LocatedOutcome::Suggested;
                    newly_demoted = true;
                }
            }
        }
        if !newly_demoted {
            break;
        }
        let survivors: Vec<usize> = accepted
            .iter()
            .copied()
            .filter(|&i| outcome[i] == LocatedOutcome::Applied)
            .collect();
        result = splice(original, &survivors, &batch);
    }

    let paired = batch
        .into_iter()
        .enumerate()
        .map(|(i, e)| (e, outcome[i]))
        .collect();
    (result, paired)
}

/// Splice the `ReplaceRange` edits named by `which` into `original`.
/// `which` is in ascending total order (`(start, end, rule, violation)`)
/// and its ranges are pairwise disjoint by construction (overlap-skip
/// already ran). Applying them back-to-front (highest start first) keeps
/// every earlier offset valid; iterating `which` in reverse also lands
/// zero-width inserts that share a start in total order (the
/// earliest-ordered insert ends up first in the file, because a later
/// splice at the same offset pushes ahead of an earlier one).
fn splice(original: &[u8], which: &[usize], batch: &[LocatedEdit]) -> Vec<u8> {
    let mut out = original.to_vec();
    for &i in which.iter().rev() {
        if let FixEdit::ReplaceRange { range, content, .. } = &batch[i].collected.edit {
            out.splice(range.clone(), content.iter().copied());
        }
    }
    out
}

/// The localized-equivalence (`PutGet`) check: re-parse `bytes` in `format`
/// (syntactic validity) and re-run the `JSONPath` source `query`, asserting
/// the result matches `expect`. Any failure (invalid UTF-8, parse error,
/// bad query, wrong or missing node) returns `false` so the engine demotes
/// the edit rather than writing bytes that don't achieve the rule's goal.
fn verify_structured(bytes: &[u8], format: Format, query: &str, expect: &ExpectedValue) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let Ok(value) = format.parse(text) else {
        return false;
    };
    let Ok(path) = JsonPath::parse(query) else {
        return false;
    };
    let nodes = path.query(&value);
    match expect {
        ExpectedValue::Absent => nodes.is_empty(),
        ExpectedValue::Scalar(want) => nodes.at_most_one().ok().flatten() == Some(want),
    }
}

/// Bounded proof that the located-edit **overlap-skip** accepts a pairwise
/// disjoint set. It models the `reserved_end` greedy in [`apply_file_edits`]:
/// candidates arrive in the pinned total order (leading keys `(start, end)`), a
/// running `reserved_end` holds the end of the last accepted edit, and an edit
/// is accepted iff its `start` is at or after `reserved_end` (which then
/// advances to that edit's `end`). The tier and isolation-group filters only
/// *remove* candidates, and removing a candidate can never create an overlap,
/// so they are abstracted away. The invariant proven -- every pair of accepted
/// half-open ranges is disjoint -- is exactly what lets `splice` apply the
/// accepted edits back-to-front without corrupting an earlier one's offsets.
///
/// The non-obvious step is that `reserved_end` is *overwritten* on each
/// acceptance, not maxed: the proof confirms that acceptance's
/// `start >= reserved_end` guard, over valid sorted ranges, keeps `reserved_end`
/// non-decreasing, so an overwrite can never expose an earlier accepted range to
/// a later overlapping one. This is an independent formulation of the same
/// policy `apply_file_edits` runs; the fixture tests below exercise the real
/// function, and this harness proves the combinatorial core exhaustively.
#[cfg(kani)]
mod kani_proofs {
    #[kani::proof]
    #[kani::unwind(6)]
    fn overlap_skip_accepts_a_pairwise_disjoint_set() {
        const N: usize = 5;
        let starts: [usize; N] = kani::any();
        let ends: [usize; N] = kani::any();

        // Preconditions the real path establishes before overlap-skip runs:
        // every range is valid (`start <= end`), and the batch is in ascending
        // total order, whose leading keys are `(start, end)`.
        for i in 0..N {
            kani::assume(starts[i] <= ends[i]);
        }
        for i in 1..N {
            let ordered =
                starts[i - 1] < starts[i] || (starts[i - 1] == starts[i] && ends[i - 1] <= ends[i]);
            kani::assume(ordered);
        }

        // The `reserved_end` greedy, structurally identical to the accept step
        // of `apply_file_edits`.
        let mut reserved_end: Option<usize> = None;
        let mut accepted = [false; N];
        for i in 0..N {
            if reserved_end.is_some_and(|end| starts[i] < end) {
                continue; // overlap-skip: conflicts with an accepted edit
            }
            reserved_end = Some(ends[i]);
            accepted[i] = true;
        }

        // Invariant: accepted half-open ranges `[start, end)` are pairwise
        // disjoint.
        for a in 0..N {
            for b in (a + 1)..N {
                if accepted[a] && accepted[b] {
                    let disjoint = ends[a] <= starts[b] || ends[b] <= starts[a];
                    assert!(disjoint, "overlap-skip must accept only disjoint ranges");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn replace(range: Range<usize>, content: &str) -> FixEdit {
        FixEdit::ReplaceRange {
            path: PathBuf::from("f"),
            range,
            content: content.as_bytes().to_vec(),
        }
    }

    fn edit(
        rule_index: usize,
        violation_index: usize,
        edit: FixEdit,
        applicability: Applicability,
        verify: EditVerifier,
        isolation_group: Option<GroupId>,
    ) -> LocatedEdit {
        LocatedEdit {
            rule_index,
            violation_index,
            collected: CollectedEdit {
                edit,
                applicability,
                verify,
                isolation_group,
            },
        }
    }

    fn safe(range: Range<usize>, content: &str) -> LocatedEdit {
        edit(
            0,
            0,
            replace(range, content),
            Applicability::Safe,
            EditVerifier::None,
            None,
        )
    }

    #[test]
    fn splice_insert_delete_replace() {
        // insert: empty range, non-empty content.
        let (out, o) = apply_file_edits(b"abcd", vec![safe(2..2, "XY")], Applicability::Safe);
        assert_eq!(out, b"abXYcd");
        assert_eq!(o[0].1, LocatedOutcome::Applied);
        // delete: non-empty range, empty content.
        let (out, _) = apply_file_edits(b"abcd", vec![safe(1..3, "")], Applicability::Safe);
        assert_eq!(out, b"ad");
        // replace: non-empty range, non-empty content.
        let (out, _) = apply_file_edits(b"abcd", vec![safe(1..3, "ZZZ")], Applicability::Safe);
        assert_eq!(out, b"aZZZd");
    }

    #[test]
    fn multiple_disjoint_edits_compose_in_one_pass() {
        // Two disjoint edits splice together; the earlier-offset one must
        // not invalidate the later one's indices.
        let (out, o) = apply_file_edits(
            b"a..b..c",
            vec![safe(1..3, "XX"), safe(4..6, "YY")],
            Applicability::Safe,
        );
        assert_eq!(out, b"aXXbYYc");
        assert!(o.iter().all(|(_, s)| *s == LocatedOutcome::Applied));
    }

    #[test]
    fn tier_filter_gates_unsafe_on_threshold() {
        let mk = || {
            edit(
                0,
                0,
                replace(0..1, "X"),
                Applicability::Unsafe,
                EditVerifier::None,
                None,
            )
        };
        // Below threshold: not applied, surfaced as a suggestion.
        let (out, o) = apply_file_edits(b"abc", vec![mk()], Applicability::Safe);
        assert_eq!(out, b"abc");
        assert_eq!(o[0].1, LocatedOutcome::Suggested);
        // At threshold: applied.
        let (out, o) = apply_file_edits(b"abc", vec![mk()], Applicability::Unsafe);
        assert_eq!(out, b"Xbc");
        assert_eq!(o[0].1, LocatedOutcome::Applied);
    }

    #[test]
    fn suggestion_and_never_tiers() {
        let sugg = edit(
            0,
            0,
            replace(0..1, "X"),
            Applicability::Suggestion,
            EditVerifier::None,
            None,
        );
        let never = edit(
            0,
            0,
            replace(0..1, "X"),
            Applicability::Never,
            EditVerifier::None,
            None,
        );
        let (out, o) = apply_file_edits(b"abc", vec![sugg], Applicability::Unsafe);
        assert_eq!(out, b"abc");
        assert_eq!(o[0].1, LocatedOutcome::Suggested); // never applied, even at Unsafe
        let (out, o) = apply_file_edits(b"abc", vec![never], Applicability::Unsafe);
        assert_eq!(out, b"abc");
        assert_eq!(o[0].1, LocatedOutcome::Dropped);
    }

    #[test]
    fn overlap_skip_earlier_in_total_order_wins() {
        // Two overlapping ranges; the one ordered first by (start, end,
        // rule, violation) wins, the other is a conflict.
        let first = edit(
            0,
            0,
            replace(0..3, "AAAA"),
            Applicability::Safe,
            EditVerifier::None,
            None,
        );
        let second = edit(
            1,
            0,
            replace(2..5, "BBBB"),
            Applicability::Safe,
            EditVerifier::None,
            None,
        );
        // Pass them out of order to prove sort, not input order, decides.
        let (out, o) = apply_file_edits(b"0123456", vec![second, first], Applicability::Safe);
        assert_eq!(out, b"AAAA3456");
        // input[0] was `second` (loses), input[1] was `first` (wins).
        assert_eq!(o[0].1, LocatedOutcome::SkippedConflict);
        assert_eq!(o[1].1, LocatedOutcome::Applied);
    }

    #[test]
    fn isolation_group_excludes_disjoint_edits() {
        // Byte-disjoint edits that share an isolation group must not
        // co-apply: the earlier-ordered one wins, the other is a conflict.
        let a = edit(
            0,
            0,
            replace(0..1, "A"),
            Applicability::Safe,
            EditVerifier::None,
            Some(7),
        );
        let b = edit(
            1,
            0,
            replace(4..5, "B"),
            Applicability::Safe,
            EditVerifier::None,
            Some(7),
        );
        let (out, o) = apply_file_edits(b"012345", vec![a, b], Applicability::Safe);
        assert_eq!(out, b"A12345"); // only the first group member applied
        assert_eq!(o[0].1, LocatedOutcome::Applied);
        assert_eq!(o[1].1, LocatedOutcome::SkippedConflict);
        // Different groups DO co-apply.
        let a = edit(
            0,
            0,
            replace(0..1, "A"),
            Applicability::Safe,
            EditVerifier::None,
            Some(1),
        );
        let b = edit(
            1,
            0,
            replace(4..5, "B"),
            Applicability::Safe,
            EditVerifier::None,
            Some(2),
        );
        let (out, _) = apply_file_edits(b"012345", vec![a, b], Applicability::Safe);
        assert_eq!(out, b"A123B5");
    }

    #[test]
    fn verify_reparse_failure_demotes_to_suggested() {
        // The edit produces bytes that are not valid JSON, so the
        // syntactic re-parse fails and the edit is demoted, not written.
        let verify = EditVerifier::Structured {
            format: Format::Json,
            query: "$.x".to_string(),
            expect: ExpectedValue::Scalar(serde_json::json!(1)),
        };
        // Original valid JSON `{"x": 0}`; edit corrupts it.
        let corrupt = edit(
            0,
            0,
            replace(0..8, "{not json"),
            Applicability::Safe,
            verify,
            None,
        );
        let (out, o) = apply_file_edits(br#"{"x": 0}"#, vec![corrupt], Applicability::Safe);
        assert_eq!(out, br#"{"x": 0}"#); // unchanged: demoted before write
        assert_eq!(o[0].1, LocatedOutcome::Suggested);
    }

    #[test]
    fn verify_wrong_scalar_demotes_distinct_from_reparse() {
        // The edit yields valid JSON, but the query resolves to the wrong
        // scalar, so it is demoted for a DIFFERENT reason than a parse fail.
        let verify = EditVerifier::Structured {
            format: Format::Json,
            query: "$.x".to_string(),
            expect: ExpectedValue::Scalar(serde_json::json!(1)),
        };
        // Replace the value 0 with 2 (valid JSON) while expecting 1.
        let wrong = edit(0, 0, replace(6..7, "2"), Applicability::Safe, verify, None);
        let (out, o) = apply_file_edits(br#"{"x": 0}"#, vec![wrong], Applicability::Safe);
        assert_eq!(out, br#"{"x": 0}"#); // demoted, not written
        assert_eq!(o[0].1, LocatedOutcome::Suggested);

        // The matching value verifies and IS applied.
        let verify = EditVerifier::Structured {
            format: Format::Json,
            query: "$.x".to_string(),
            expect: ExpectedValue::Scalar(serde_json::json!(1)),
        };
        let right = edit(0, 0, replace(6..7, "1"), Applicability::Safe, verify, None);
        let (out, o) = apply_file_edits(br#"{"x": 0}"#, vec![right], Applicability::Safe);
        assert_eq!(out, br#"{"x": 1}"#);
        assert_eq!(o[0].1, LocatedOutcome::Applied);
    }

    #[test]
    fn verify_absent_checks_zero_matches() {
        // `Absent` passes only when the query matches nothing post-edit.
        let verify = EditVerifier::Structured {
            format: Format::Json,
            query: "$.x".to_string(),
            expect: ExpectedValue::Absent,
        };
        // Remove the whole `"x": 0` member, leaving `{}`.
        let remove = edit(0, 0, replace(1..7, ""), Applicability::Safe, verify, None);
        let (out, o) = apply_file_edits(br#"{"x": 0}"#, vec![remove], Applicability::Safe);
        assert_eq!(out, b"{}");
        assert_eq!(o[0].1, LocatedOutcome::Applied);
    }

    #[test]
    fn same_start_inserts_land_in_total_order() {
        // Two zero-width inserts at the same offset from different rules must
        // land in total order (rule 0 before rule 1), never reversed by the
        // back-to-front splice. Pass them out of input order to prove the
        // total-order sort decides, not input order.
        let r0 = edit(
            0,
            0,
            replace(2..2, "A"),
            Applicability::Safe,
            EditVerifier::None,
            None,
        );
        let r1 = edit(
            1,
            0,
            replace(2..2, "B"),
            Applicability::Safe,
            EditVerifier::None,
            None,
        );
        let (out, o) = apply_file_edits(b"xy", vec![r1, r0], Applicability::Safe);
        assert_eq!(out, b"xyAB"); // rule 0's "A" precedes rule 1's "B"
        assert!(o.iter().all(|(_, s)| *s == LocatedOutcome::Applied));
    }

    #[test]
    fn batch_verify_demotes_only_the_failing_edit() {
        // Two disjoint edits on one JSON object, each Structured-verified.
        // One sets a value matching its expectation (applies); the other
        // expects a value it does not produce (demotes). The valid edit must
        // survive, and it is re-verified against the survivors-only splice
        // after the demotion (the cascade loop), staying Applied.
        let ok_verify = EditVerifier::Structured {
            format: Format::Json,
            query: "$.a".to_string(),
            expect: ExpectedValue::Scalar(serde_json::json!(1)),
        };
        let bad_verify = EditVerifier::Structured {
            format: Format::Json,
            query: "$.b".to_string(),
            expect: ExpectedValue::Scalar(serde_json::json!(9)), // expects 9, writes 2
        };
        // {"a":0,"b":0}: a's value at byte 5, b's value at byte 11.
        let set_a = edit(
            0,
            0,
            replace(5..6, "1"),
            Applicability::Safe,
            ok_verify,
            None,
        );
        let set_b = edit(
            1,
            0,
            replace(11..12, "2"),
            Applicability::Safe,
            bad_verify,
            None,
        );
        let (out, o) =
            apply_file_edits(br#"{"a":0,"b":0}"#, vec![set_a, set_b], Applicability::Safe);
        assert_eq!(o[0].1, LocatedOutcome::Applied); // set_a verified
        assert_eq!(o[1].1, LocatedOutcome::Suggested); // set_b demoted
        assert_eq!(out, br#"{"a":1,"b":0}"#); // only a changed
    }
}
