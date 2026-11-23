---
status: accepted
date: 2026-11-23
decision-makers: asamarts
---

# 0049. Baseline entries record visible preferred paths for repeated fingerprints

## Status

Accepted (2026-11-23). Amends [ADR-0006](0006-baseline-suppression.md) (baseline entry format and
matching) and [ADR-0047](0047-sarif-tells-apart-a-fingerprint-repeated-within-a-run.md) (stability
of SARIF occurrence fingerprints under a baseline). Resolves #601.

## Context

When several findings share one canonical fingerprint, a baseline entry suppresses up to `count` of
them. Previously, `alint_core::baseline::apply` suppressed the first `min(k, count)` findings in
`(path, line, column, message)` order on every run.

Two rule classes routinely emit findings that share a fingerprint across distinct files:

- `constraint` rules (v0.23), whose `path_free_identity` hashes `(rule_id, "\0constraint", key)` so
  renaming a file keeps a baselined finding suppressed;
- findings whose path alint hides because an environment-derived field chose the file
  ([ADR-0030](0030-v022-owner-decisions.md) decision 1).

When a new `constraint` repeat appears at a path that sorts before an existing baselined finding
(for example, 16 baselined duplicates in `p14..p60` and a new duplicate added at `p13`), `apply`
suppressed `p13` and turned the unchanged `p60` live ("new"). Because SARIF numbers baselined
repeats before live ones (ADR-0047), all 16 baselined SARIF occurrence fingerprints also shifted
by one.

## Decision

1. **Optional `paths` field on `BaselineEntry` (keeping `schema_version: 1`).** Each baseline
   entry gains an optional JSON array `paths` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`),
   listing the sorted visible slash-normalized paths of the findings that entry suppressed.
   - Because `paths` is optional and omitted when empty, existing `schema_version: 1` baseline
     files load unchanged and retain the previous count-order behaviour.
   - `Baseline::from_fingerprints` populates `paths` whenever a non-hidden-path entry spans more
     than one distinct visible path or comes from a `path_free_identity` finding (such as
     `constraint`), and omits it for ordinary path-bound single-file entries so normal baselines
     stay byte-for-byte unchanged.
   - `Baseline::from_fingerprints` also pins the advisory `path` field to the lexicographically
     smallest path in the group (matching `message`), keeping regeneration deterministic regardless
     of parallel rule evaluation order.
   - `Baseline::load` merges duplicate fingerprint lines (for example after a git merge) by summing
     `count` and extending `paths`.
2. **Two-pass matching in `baseline::apply`.** Within each rule (after sorting violations by
   `(canonical_order_path, line, column, message)`):
   - **Pass 1 (preferred paths):** suppress visible-path findings whose slash-normalized path
     matches an available slot in the entry's `paths` multiset, bounded by the entry's `count`.
   - **Pass 2 (remainder):** fill any remaining `count` budget in the existing deterministic order.
     Renaming a baselined `constraint` file therefore still suppresses the renamed finding rather
     than turning it new when no competing finding sits at the old path.
   - Total suppressed occurrences for a fingerprint never exceed `entry.count`, even if `paths` is
     hand-edited to contain more elements than `count`.
3. **Hidden-path entries store and match no `paths` (ADR-0030).** A hidden-path entry (whose
   fingerprint is `sha256(lp(rule_id) || lp("\0hidden-path") || lp("sensitive"))`) always writes an
   empty (omitted) `paths` list, and `apply` never consults `paths` for a finding whose path is
   sensitive (`Violation::path_is_sensitive()`). A baseline file therefore still reveals nothing
   about hidden paths and cannot act as a path oracle.

## Consequences

- Adding a new `constraint` repeat that sorts before already-baselined repeats reports the new
  file as live ("new"), keeps every unchanged baselined file suppressed ("unchanged"), and keeps
  every baselined finding's SARIF occurrence fingerprint unchanged (ADR-0047 numbers baselined
  results before live ones).
- Existing baselines written by v0.23 and earlier continue to work without regeneration, keeping
  count-order matching until regenerated.
- Ordinary path-bound rules (where `path` is already part of the fingerprint) write no `paths`
  field and see zero diff noise.
- Renaming a baselined `constraint` file remains suppressed via Pass 2.
- Hidden-path confidentiality (ADR-0030) is preserved strictly.

## Considered Options

- **Bump `schema_version` to 2.** Would break older baselines on upgrade and force a breaking
  migration for every user even when no repeated path-free findings exist. Unnecessary because
  `paths` is strictly additive and optional.
- **Record `paths` on every baseline entry.** Adds redundant `paths: ["a.rs"]` next to
  `path: "a.rs"` on every line of ordinary path-bound baselines where two files can never share a
  fingerprint.
- **Require exact path matches without Pass 2 fallback.** Would break rename stability for
  `constraint` findings, which is the reason `constraint` leaves the path out of the fingerprint.

## More Information

- Implementation: `crates/alint-core/src/baseline.rs` (`BaselineEntry::paths`,
  `Baseline::from_fingerprints`, `Baseline::load`, `baseline::apply`), `crates/alint/src/check.rs`
  (`cmd_baseline`).
- Design & docs: `docs/design/baseline.md`, `docs/site/concepts/adoption/baseline.md`.
- Issue #601; amends [ADR-0006](0006-baseline-suppression.md) and
  [ADR-0047](0047-sarif-tells-apart-a-fingerprint-repeated-within-a-run.md).
