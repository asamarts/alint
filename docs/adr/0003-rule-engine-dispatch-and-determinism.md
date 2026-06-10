---
status: accepted
date: 2026-06-10
decision-makers: asamarts
---

# 3. Rule engine dispatch model and determinism guarantee

## Status

Accepted. Backfilled on 2026-06-10; the decision itself predates this record (see ADR-0001).

## Context

alint must evaluate many rules over trees that can reach a million files, efficiently and
reproducibly. Reproducibility is required for snapshot testing, the deterministic perf gate,
and trustworthy CI output. A naive "each rule reads each file it wants" design re-reads hot
files once per matching rule and offers no ordering guarantee.

## Decision

We will use a single `Rule` trait with three dispatch classes, a single shared file walk, and
a hard determinism guarantee.

- Dispatch classes:
  - Rule-major (default): the rule iterates the `FileIndex` itself. Used by existence and
    cross-file rules.
  - Per-file (opt-in via `as_per_file()`): the engine reads each matched file once and
    dispatches it to every applicable per-file rule against the same buffer (read coalescing).
  - Full-index cross-file (`requires_full_index() == true`): the rule sees the whole tree even
    in `--changed` mode and must declare no path scope (`path_scope() == None`). This invariant
    is enforced by the test `v010_cross_file_kinds_require_full_index_and_no_path_scope`.
- A single parallel walk produces one `FileIndex` with lazily-built indices (path set,
  parent-to-children, git-tracked filter) reused by all rules.
- Determinism: the walk result is sorted post-walk; rules that accumulate cross-file results
  sort their output; formatters sort where needed. Output (report, violations, fixes) is
  byte-identical across runs on the same input. No reliance on HashMap iteration or readdir order.

## Consequences

Positive: read coalescing makes the per-file hot path scale; one walk amortizes indexing;
determinism enables snapshots, benches, and the Valgrind perf gate.

Negative and accepted: the cross-file invariant (`requires_full_index` with no path scope) is
load-bearing and easy to violate in a new rule; it is protected by a test and by build-time
rejection of `scope_filter:` on cross-file rules, and it is listed in the constitution.

## More Information

- `docs/design/ARCHITECTURE.md` (rule model, dispatch flip, memory layout).
- `docs/design/constitution.md` (the determinism and cross-file invariants).
- ADR-0001 (the Stateright spike in WS4 targets exactly the cross-file dispatch determinism
  invariant recorded here).
