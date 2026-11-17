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

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use serde_json_path::{JsonPath, PathElement};

use crate::rule::{Applicability, CollectedEdit, EditVerifier, ExpectedValue, FixEdit, GroupId};
use crate::structured_fix::{self, Intent, PathSeg};
use crate::structured_format::Format;

/// One located edit plus the provenance the total-order sort needs. The
/// engine assigns `rule_index` in flattened rule-discovery order (so
/// nested-config ordering is deterministic) and `violation_index` per
/// violation within a rule.
#[derive(Debug, Clone)]
pub struct LocatedEdit {
    pub rule_index: usize,
    pub violation_index: usize,
    /// The source finding was sensitive. The engine carries this through the
    /// synthetic value-free fix item so its status detail is still redacted.
    pub sensitive: bool,
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
/// `SkippedConflict`, as is a zero-width insert identical to one already
/// accepted at the same offset); isolation-group exclusion (an edit sharing a group
/// with an already-accepted edit is a `SkippedConflict` even when
/// byte-disjoint); splice; then verify — each accepted `Structured` edit
/// is checked against the spliced bytes and demoted to `Suggested` on
/// failure, after which the survivors are re-spliced. Where the edits of
/// more than one rule change a document checked as a whole (see
/// [`structured_fix::checks_whole_document`]), the survivors must together
/// leave it changed at their targets only, or the later rules are demoted.
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
        // Range validity is a precondition of `splice` (and of the sort's
        // disjointness reasoning): `Vec::splice` panics on an inverted
        // (`start > end`) or out-of-bounds (`end > len`) range. A fixer's ranges
        // can go stale if an earlier fixer shortened the same file (the engine's
        // Phase-1 caveat), so drop a malformed range defensively rather than
        // crash the whole `fix`. This is what makes the overlap-skip Kani
        // proof's `start <= end` assumption an established fact downstream.
        if range.start > range.end || range.end > original.len() {
            outcome[i] = LocatedOutcome::Dropped;
            continue;
        }
        if reserved_end.is_some_and(|end| range.start < end) {
            outcome[i] = LocatedOutcome::SkippedConflict;
            continue;
        }
        // Zero-width inserts never overlap the half-open reservation, so
        // several may land at one offset (distinct content, in total order --
        // intended). An insert IDENTICAL to one already accepted there (same
        // range, same bytes: two rules / violations wanting the same line) would
        // splice the text twice; it is a conflict instead, the first one wins.
        if range.is_empty()
            && accepted.iter().rev().any(|&a| {
                matches!(
                    (&batch[a].collected.edit, &ce.edit),
                    (
                        FixEdit::ReplaceRange { range: ra, content: ca, .. },
                        FixEdit::ReplaceRange { content: cb, .. },
                    ) if *ra == range && ca == cb
                )
            })
        {
            outcome[i] = LocatedOutcome::SkippedConflict;
            continue;
        }
        if let Some(g) = ce.isolation_group
            && !used_groups.insert(g)
        {
            outcome[i] = LocatedOutcome::SkippedConflict;
            continue;
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
    // `accepted.len()` passes.
    //
    // SAFETY vs MAXIMALITY: the output is always safe -- every edit that stays
    // `Applied` verifies against the final bytes, so nothing corrupt is ever
    // written. It is NOT guaranteed maximal: a batch-demote (all failures in one
    // pass) can over-demote when two edits fail together only because a third
    // made the doc invalid, and a `Suggested`-demoted isolation-group leader is
    // never re-promoted nor releases its group, so a group-mate it excluded
    // stays `SkippedConflict`. Both drop good fixes but never write bad ones.
    //
    // As of Phase 2 this loop IS entered by a shipped config: `set_value` /
    // `remove_value` emit `Structured` verifiers (this is the R-VERIFY safety
    // net -- an edit whose re-parse + re-query disagrees with its expectation is
    // demoted, not written). The over-demotion cost stays BENIGN for them: they
    // set `isolation_group: None` (so the group-release gap can't bite), and a
    // `remove_value` batch that can only partially remove (some node's span
    // unresolvable) correctly demotes ALL its edits via the shared whole-file
    // `Absent` re-query -- an all-or-nothing removal, never a partial write. The
    // maximal (culprit-attributing / group-releasing) algorithm is still
    // deferred to a FUTURE op that actually uses isolation groups (a structured
    // op with isolation groups is not yet shipped), when it can be designed +
    // tested against a real grouped op.
    //
    // Each pass parses the spliced bytes once per format and answers a check the
    // batch repeats once (see `Verifier`), so a pass over N edits costs one parse,
    // not N: a `remove_value` over thousands of matches was quadratic.
    let mut result = splice(original, &accepted, &batch);
    loop {
        let newly_demoted = {
            let mut verifier = Verifier::new(&result);
            let mut demoted = false;
            for &i in &accepted {
                if outcome[i] != LocatedOutcome::Applied {
                    continue;
                }
                if let EditVerifier::Structured {
                    format,
                    query,
                    expect,
                } = &batch[i].collected.verify
                    && !verifier.check(*format, query, expect)
                {
                    outcome[i] = LocatedOutcome::Suggested;
                    demoted = true;
                }
            }
            demoted
        };
        // Once each edit verifies on its own, the whole document: a removal
        // that is right alone can change a neighbour beside another rule's
        // (the two last entries of a block mapping, each removed by a rule of
        // its own, leave the mapping empty, which reads as null).
        if !newly_demoted && !demote_collateral(original, &accepted, &batch, &mut outcome) {
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
/// already ran), so one front-to-back pass copies each gap of `original`
/// and then each edit's content: every byte is copied once, where a splice
/// per edit moved the whole tail each time. Zero-width inserts that share a
/// start land in total order (the earliest-ordered insert first in the
/// file), and an insert at a replaced range's start or end lands before or
/// after it, as the sort orders them.
fn splice(original: &[u8], which: &[usize], batch: &[LocatedEdit]) -> Vec<u8> {
    let mut out = Vec::with_capacity(original.len());
    let mut copied = 0;
    for &i in which {
        if let FixEdit::ReplaceRange { range, content, .. } = &batch[i].collected.edit {
            debug_assert!(
                copied <= range.start,
                "accepted ranges are disjoint and sorted"
            );
            out.extend_from_slice(&original[copied.min(range.start)..range.start]);
            out.extend_from_slice(content);
            copied = copied.max(range.end);
        }
    }
    out.extend_from_slice(&original[copied..]);
    out
}

/// Demote the edits [`collateral`] names among the applied ones of
/// `accepted`; whether there was any.
fn demote_collateral(
    original: &[u8],
    accepted: &[usize],
    batch: &[LocatedEdit],
    outcome: &mut [LocatedOutcome],
) -> bool {
    let applied: Vec<usize> = accepted
        .iter()
        .copied()
        .filter(|&i| outcome[i] == LocatedOutcome::Applied)
        .collect();
    let culprits = collateral(original, &applied, batch);
    for &i in &culprits {
        outcome[i] = LocatedOutcome::Suggested;
    }
    !culprits.is_empty()
}

/// The edits among `applied` (in total order) to demote so that each document
/// checked as a whole changes at the applied edits' targets only. Each
/// fixer checks its own edits together (`structured_fix::checked_value_edits`
/// and its siblings), so this looks only where edits of more than one rule
/// meet in one format, and only when every applied edit is one of them: the
/// check could not tell a free-form edit's change from a stray one. The
/// edits' targets are what their queries select in the original, with the
/// change their expectation names: removed for `Absent`, the value for
/// `Scalar`, and for `Matches` what the edited document holds there, which
/// the per-edit verify checked. When the edits together change more, the
/// rules are added one at a time in their order, and a rule whose edits
/// would make the result change more is demoted whole.
fn collateral(original: &[u8], applied: &[usize], batch: &[LocatedEdit]) -> Vec<usize> {
    let structured = |i: usize| match &batch[i].collected.verify {
        EditVerifier::Structured { format, .. } => Some(*format),
        EditVerifier::None => None,
    };
    let Some(format) = applied.first().and_then(|&i| structured(i)) else {
        return Vec::new();
    };
    let one_format = applied.iter().all(|&i| structured(i) == Some(format));
    let rules = applied
        .iter()
        .any(|&i| batch[i].rule_index != batch[applied[0]].rule_index);
    if !structured_fix::checks_whole_document(format) || !one_format || !rules {
        return Vec::new();
    }
    let read = |bytes: &[u8]| {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| format.parse(text).ok())
    };
    let Some(before) = read(original) else {
        return Vec::new();
    };
    let exact = |edits: &[usize]| {
        let bytes = splice(original, edits, batch);
        read(&bytes).is_some_and(|after| {
            // One query's targets once, however many edits share it (a
            // removal batch shares the rule's query).
            let mut seen: HashMap<&str, Vec<&ExpectedValue>> = HashMap::new();
            let mut intents = Vec::new();
            for &i in edits {
                let EditVerifier::Structured { query, expect, .. } = &batch[i].collected.verify
                else {
                    continue;
                };
                let expects = seen.entry(query.as_str()).or_default();
                if expects.contains(&expect) {
                    continue;
                }
                expects.push(expect);
                let Some(found) = targets(&before, &after, query, expect) else {
                    return false;
                };
                intents.extend(found);
            }
            structured_fix::intended(&before, &intents).as_ref() == Some(&after)
        })
    };
    if exact(applied) {
        return Vec::new();
    }
    let mut rules: Vec<usize> = applied.iter().map(|&i| batch[i].rule_index).collect();
    rules.sort_unstable();
    rules.dedup();
    let mut kept = vec![false; batch.len()];
    let mut culprits = Vec::new();
    for rule in rules {
        let candidate: Vec<usize> = applied
            .iter()
            .copied()
            .filter(|&i| kept[i] || batch[i].rule_index == rule)
            .collect();
        if exact(&candidate) {
            for i in candidate {
                kept[i] = true;
            }
        } else {
            culprits.extend(
                applied
                    .iter()
                    .copied()
                    .filter(|&i| batch[i].rule_index == rule),
            );
        }
    }
    culprits
}

/// The targets of an edit's verifier, `query` and `expect`, in `before`, each
/// with its intended change (see [`collateral`]); `None` when the query does
/// not parse or a `Matches` target is gone from `after`.
fn targets(
    before: &serde_json::Value,
    after: &serde_json::Value,
    query: &str,
    expect: &ExpectedValue,
) -> Option<Vec<(Vec<PathSeg>, Intent)>> {
    let path = JsonPath::parse(query).ok()?;
    path.query_located(before)
        .iter()
        .map(|node| {
            let segs: Vec<PathSeg> = node
                .location()
                .iter()
                .map(|element| match element {
                    PathElement::Name(name) => PathSeg::Key((*name).to_string()),
                    PathElement::Index(i) => PathSeg::Index(*i),
                })
                .collect();
            let intent = match expect {
                ExpectedValue::Absent => Intent::Remove,
                ExpectedValue::Scalar(value) => Intent::Set(value.clone()),
                ExpectedValue::Matches(_) => {
                    let mut node = after;
                    for seg in &segs {
                        node = match seg {
                            PathSeg::Key(key) => node.get(key.as_str())?,
                            PathSeg::Index(i) => node.get(*i)?,
                        };
                    }
                    Intent::Set(node.clone())
                }
            };
            Some((segs, intent))
        })
        .collect()
}

std::thread_local! {
    /// How many times [`Verifier`] has parsed spliced bytes on this thread.
    static VERIFY_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times [`apply_file_edits`] has parsed spliced bytes to verify
/// edits on this thread. Test and diagnostic only: the located fixers' tests
/// count with it that a batch is verified with one parse however many edits
/// it holds.
#[doc(hidden)]
#[must_use]
pub fn verify_parses() -> usize {
    VERIFY_PARSES.with(std::cell::Cell::get)
}

/// What a check the batch can repeat asks of its query's nodes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Goal<'b> {
    Absent,
    Matches(&'b str),
}

/// The localized-equivalence (`PutGet`) checks of one spliced result: re-parse
/// the bytes in each edit's format (syntactic validity) and re-run its
/// `JSONPath` source `query`, asserting the result matches `expect`. Any failure
/// (invalid UTF-8, parse error, bad query, wrong or missing node) fails the
/// check, so the engine demotes the edit rather than writing bytes that don't
/// achieve the rule's goal.
///
/// A check depends on nothing but the bytes, the format, the query and the
/// expectation, so the verifier parses the bytes at most once per format and
/// answers a check the batch repeats once: a `remove_value` batch shares one
/// `Absent` query, and a `replace` batch one `matches:` pattern.
struct Verifier<'r, 'b> {
    bytes: &'r [u8],
    /// Each format's reading of the bytes, `None` when they do not parse.
    documents: Vec<(Format, Option<serde_json::Value>)>,
    paths: HashMap<&'b str, Option<JsonPath>>,
    patterns: HashMap<&'b str, Option<regex::Regex>>,
    answers: HashMap<(usize, &'b str, Goal<'b>), bool>,
}

impl<'r, 'b> Verifier<'r, 'b> {
    fn new(bytes: &'r [u8]) -> Self {
        Self {
            bytes,
            documents: Vec::new(),
            paths: HashMap::new(),
            patterns: HashMap::new(),
            answers: HashMap::new(),
        }
    }

    /// Whether the bytes hold what `expect` asks of `query` in `format`.
    fn check(&mut self, format: Format, query: &'b str, expect: &'b ExpectedValue) -> bool {
        let document = self.document(format);
        let goal = match expect {
            ExpectedValue::Absent => Some(Goal::Absent),
            ExpectedValue::Matches(re_src) => Some(Goal::Matches(re_src)),
            ExpectedValue::Scalar(_) => None,
        };
        let Some(goal) = goal else {
            return self.answer(document, query, expect);
        };
        if let Some(&answer) = self.answers.get(&(document, query, goal)) {
            return answer;
        }
        let answer = self.answer(document, query, expect);
        self.answers.insert((document, query, goal), answer);
        answer
    }

    /// The index of `format`'s reading of the bytes, parsing them on first use.
    fn document(&mut self, format: Format) -> usize {
        if let Some(index) = self.documents.iter().position(|(f, _)| *f == format) {
            return index;
        }
        VERIFY_PARSES.with(|parses| parses.set(parses.get() + 1));
        let value = std::str::from_utf8(self.bytes)
            .ok()
            .and_then(|text| format.parse(text).ok());
        self.documents.push((format, value));
        self.documents.len() - 1
    }

    fn answer(&mut self, document: usize, query: &'b str, expect: &'b ExpectedValue) -> bool {
        let Some(value) = &self.documents[document].1 else {
            return false;
        };
        let Some(path) = self
            .paths
            .entry(query)
            .or_insert_with(|| JsonPath::parse(query).ok())
        else {
            return false;
        };
        let nodes = path.query(value);
        match expect {
            ExpectedValue::Absent => nodes.is_empty(),
            ExpectedValue::Scalar(want) => nodes.at_most_one().ok().flatten() == Some(want),
            ExpectedValue::Matches(re_src) => {
                let Some(re) = self
                    .patterns
                    .entry(re_src)
                    .or_insert_with(|| regex::Regex::new(re_src).ok())
                else {
                    return false;
                };
                // EVERY node the query selects must now be a string matching the regex
                // (a wildcard `path:` selects many; `*_path_matches` fires unless ALL of
                // them satisfy `matches:`, so the fix's goal is all-match -- NOT
                // `at_most_one`, which would demote every multi-match replace). Require
                // at least one node: a query that now resolves to nothing did not
                // achieve the rule's goal.
                let mut any = false;
                for v in nodes.iter() {
                    any = true;
                    if !v.as_str().is_some_and(|s| re.is_match(s)) {
                        return false;
                    }
                }
                any
            }
        }
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
            sensitive: false,
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
    fn out_of_bounds_range_is_dropped_not_panicked() {
        // end 10 > len 4: `Vec::splice` would panic. The guard drops it instead.
        let (out, o) = apply_file_edits(b"abcd", vec![safe(2..10, "X")], Applicability::Safe);
        assert_eq!(
            out, b"abcd",
            "an out-of-bounds edit must not change the file"
        );
        assert_eq!(o[0].1, LocatedOutcome::Dropped);
    }

    #[test]
    fn inverted_range_is_dropped_not_panicked() {
        // start 5 > end 2: `Vec::splice` would panic. The guard drops it instead.
        // Built as a struct literal, not `5..2`, so clippy's reversed-empty-range
        // lint doesn't flag the range whose inversion is the point of the test.
        let inverted = std::ops::Range::<usize> { start: 5, end: 2 };
        let (out, o) = apply_file_edits(b"abcd", vec![safe(inverted, "X")], Applicability::Safe);
        assert_eq!(out, b"abcd", "an inverted edit must not change the file");
        assert_eq!(o[0].1, LocatedOutcome::Dropped);
    }

    #[test]
    fn a_valid_edit_applies_when_a_malformed_sibling_is_dropped() {
        // The guard drops only the malformed edit; a valid disjoint one applies.
        let (out, o) = apply_file_edits(
            b"abcd",
            vec![safe(0..1, "X"), safe(2..10, "Y")],
            Applicability::Safe,
        );
        assert_eq!(out, b"Xbcd");
        // sorted by (start,end): the 0..1 edit is first, the 2..10 second.
        assert_eq!(o[0].1, LocatedOutcome::Applied);
        assert_eq!(o[1].1, LocatedOutcome::Dropped);
    }

    #[test]
    fn a_range_ending_exactly_at_len_is_valid() {
        // Boundary: end == len is in-bounds for splice (it truncates the tail).
        let (out, o) = apply_file_edits(b"abcd", vec![safe(2..4, "Z")], Applicability::Safe);
        assert_eq!(out, b"abZ");
        assert_eq!(o[0].1, LocatedOutcome::Applied);
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
    fn verify_absent_failure_demotes() {
        // An `Absent` edit whose post-splice query STILL matches (it removed the
        // wrong bytes, leaving the target present) is demoted, not written. This
        // is the all-or-nothing safety net a partial `remove_value` relies on: if
        // not every matched node can be removed, the shared whole-file `Absent`
        // re-query fails and the batch is demoted rather than partially written.
        let verify = EditVerifier::Structured {
            format: Format::Json,
            query: "$.x".to_string(),
            expect: ExpectedValue::Absent,
        };
        // {"a": 0, "x": 0}: remove the `"a": 0, ` member. Valid JSON results, but
        // `$.x` is still present, so `Absent` fails and the edit demotes.
        let src = br#"{"a": 0, "x": 0}"#;
        let e = edit(0, 0, replace(1..9, ""), Applicability::Safe, verify, None);
        let (out, o) = apply_file_edits(src, vec![e], Applicability::Safe);
        assert_eq!(out, src); // unchanged: demoted before write
        assert_eq!(o[0].1, LocatedOutcome::Suggested);
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
    fn identical_same_offset_inserts_apply_once() {
        // Audit 2026-10 finding 12: the overlap check (`start < reserved_end`)
        // admits every zero-width insert at one offset, so two IDENTICAL inserts
        // (two rules / violations both inserting the same line) spliced the text
        // twice. The duplicate is now a conflict skip; the first applies.
        // Distinct same-offset inserts still both land (test above).
        let r0 = edit(
            0,
            0,
            replace(2..2, "A\n"),
            Applicability::Safe,
            EditVerifier::None,
            None,
        );
        let r1 = edit(
            1,
            0,
            replace(2..2, "A\n"),
            Applicability::Safe,
            EditVerifier::None,
            None,
        );
        let r2 = edit(
            1,
            1,
            replace(2..2, "B\n"),
            Applicability::Safe,
            EditVerifier::None,
            None,
        );
        let (out, o) = apply_file_edits(b"xy", vec![r1, r0, r2], Applicability::Safe);
        assert_eq!(out, b"xyA\nB\n");
        let applied = o
            .iter()
            .filter(|(_, s)| *s == LocatedOutcome::Applied)
            .count();
        assert_eq!(applied, 2, "{o:?}");
        assert_eq!(
            o.iter()
                .filter(|(_, s)| *s == LocatedOutcome::SkippedConflict)
                .count(),
            1
        );
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

    /// ROB-7: each edit's verification re-parsed the whole result, so a batch
    /// of N edits cost N parses. A pass parses the result once per format and
    /// answers a check the batch repeats once.
    #[test]
    fn a_batch_is_verified_with_one_parse() {
        const EDITS: usize = 20_000;
        let src = "- y: 0\n  x: 1\n".repeat(EDITS);
        let removals: Vec<LocatedEdit> = (0..EDITS)
            .map(|i| {
                let verify = EditVerifier::Structured {
                    format: Format::Yaml,
                    query: "$[*].x".to_string(),
                    expect: ExpectedValue::Absent,
                };
                let line = 14 * i + 7;
                edit(
                    0,
                    i,
                    replace(line..line + 7, ""),
                    Applicability::Safe,
                    verify,
                    None,
                )
            })
            .collect();
        let before = verify_parses();
        let (out, o) = apply_file_edits(src.as_bytes(), removals, Applicability::Safe);
        assert_eq!(verify_parses() - before, 1);
        assert!(o.iter().all(|(_, o)| *o == LocatedOutcome::Applied));
        assert_eq!(out, "- y: 0\n".repeat(EDITS).as_bytes());

        // One query per edit, one shared pattern: still one parse.
        let src = "- x: '1'\n".repeat(EDITS);
        let rewrites: Vec<LocatedEdit> = (0..EDITS)
            .map(|i| {
                let verify = EditVerifier::Structured {
                    format: Format::Yaml,
                    query: format!("$[{i}]['x']"),
                    expect: ExpectedValue::Matches("^v".to_string()),
                };
                let at = 9 * i + 5;
                edit(
                    0,
                    i,
                    replace(at..at + 3, "v1"),
                    Applicability::Safe,
                    verify,
                    None,
                )
            })
            .collect();
        let before = verify_parses();
        let (out, o) = apply_file_edits(src.as_bytes(), rewrites, Applicability::Safe);
        assert_eq!(verify_parses() - before, 1);
        assert!(o.iter().all(|(_, o)| *o == LocatedOutcome::Applied));
        assert_eq!(out, "- x: v1\n".repeat(EDITS).as_bytes());
    }

    /// A shared check answered once is still a check of the final bytes: when
    /// one edit is demoted, the next pass parses the survivors' splice afresh
    /// and verifies them again.
    #[test]
    fn a_demotion_reverifies_the_survivors_against_their_own_splice() {
        let absent = || EditVerifier::Structured {
            format: Format::Json,
            query: "$.a".to_string(),
            expect: ExpectedValue::Absent,
        };
        let wrong = EditVerifier::Structured {
            format: Format::Json,
            query: "$.b".to_string(),
            expect: ExpectedValue::Scalar(serde_json::json!(9)),
        };
        // {"a":0,"b":0}: drop `"a":0,` (verifies), set b to 2 (expects 9).
        let drop_a = edit(0, 0, replace(1..7, ""), Applicability::Safe, absent(), None);
        let set_b = edit(1, 0, replace(11..12, "2"), Applicability::Safe, wrong, None);
        let before = verify_parses();
        let (out, o) = apply_file_edits(
            br#"{"a":0,"b":0}"#,
            vec![drop_a, set_b],
            Applicability::Safe,
        );
        assert_eq!(verify_parses() - before, 2, "one parse per pass");
        assert_eq!(o[0].1, LocatedOutcome::Applied);
        assert_eq!(o[1].1, LocatedOutcome::Suggested);
        assert_eq!(out, br#"{"b":0}"#);
    }

    /// YML-4 across rules: each rule's removal is exact alone (its mapping
    /// keeps the other entry), but together the two empty `deps`, which then
    /// reads as null. The rules are taken in order: the first applies, the
    /// second is demoted, and a later pass removes the last entry as `{}`.
    /// Edits that together change their targets only all apply.
    #[test]
    fn edits_of_two_rules_that_together_change_another_node_demote_the_later_rule() {
        let absent = |query: &str| EditVerifier::Structured {
            format: Format::Yaml,
            query: query.to_string(),
            expect: ExpectedValue::Absent,
        };
        let src = "deps:\n  a: 1\n  b: 2\nname: x\n";
        let batch = || {
            vec![
                edit(
                    0,
                    0,
                    replace(6..13, ""),
                    Applicability::Safe,
                    absent("$.deps.a"),
                    None,
                ),
                edit(
                    1,
                    0,
                    replace(13..20, ""),
                    Applicability::Safe,
                    absent("$.deps.b"),
                    None,
                ),
            ]
        };
        let (out, o) = apply_file_edits(src.as_bytes(), batch(), Applicability::Safe);
        assert_eq!(o[0].1, LocatedOutcome::Applied);
        assert_eq!(o[1].1, LocatedOutcome::Suggested);
        assert_eq!(out, b"deps:\n  b: 2\nname: x\n");

        // One rule's edits are its fixer's to check together; the engine
        // leaves them be.
        let mut one_rule = batch();
        one_rule[1].rule_index = 0;
        let (out, o) = apply_file_edits(src.as_bytes(), one_rule, Applicability::Safe);
        assert!(o.iter().all(|(_, o)| *o == LocatedOutcome::Applied));
        assert_eq!(out, b"deps:\nname: x\n");

        // A removal and a set by two rules, each at its own target.
        let set = EditVerifier::Structured {
            format: Format::Yaml,
            query: "$.name".to_string(),
            expect: ExpectedValue::Scalar(serde_json::json!("y")),
        };
        let both = vec![
            edit(
                0,
                0,
                replace(6..13, ""),
                Applicability::Safe,
                absent("$.deps.a"),
                None,
            ),
            edit(1, 0, replace(26..27, "y"), Applicability::Safe, set, None),
        ];
        let (out, o) = apply_file_edits(src.as_bytes(), both, Applicability::Safe);
        assert!(o.iter().all(|(_, o)| *o == LocatedOutcome::Applied));
        assert_eq!(out, b"deps:\n  b: 2\nname: y\n");
    }

    /// ROB-7: the splice moved the whole tail once per edit, so N edits on a
    /// file of M bytes copied about N x M / 2 bytes (here 4 TB, minutes); it
    /// copies each byte once now. A time bound with a wide margin either way.
    #[test]
    fn many_edits_splice_in_one_pass() {
        const EDITS: usize = 200_000;
        let original = vec![b'x'; 200 * EDITS];
        let deletions = (0..EDITS).map(|i| safe(200 * i..200 * i + 1, "")).collect();
        let started = std::time::Instant::now();
        let (out, _) = apply_file_edits(&original, deletions, Applicability::Safe);
        let elapsed = started.elapsed();
        assert_eq!(out.len(), 199 * EDITS);
        assert!(
            elapsed < std::time::Duration::from_secs(10),
            "{EDITS} edits took {elapsed:?}"
        );
    }
}
