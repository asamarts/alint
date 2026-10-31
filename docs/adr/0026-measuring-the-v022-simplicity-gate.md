---
status: accepted
date: 2026-10-30
decision-makers: asamarts
---

# 0026. Measuring the v0.22 simplicity gate

## Status

Accepted (2026-10-30). Refines [ADR-0025](0025-v021-pre-release-decisions.md) decision 3: it
replaces gate 2's bound of 190 lines and states how both sides are counted. Gates 1 and 3, and
gate 2's ban on calls into `cross_file::eval`, are unchanged.

## Context

ADR-0025 decision 3 lets Phase 3 start only if v0.22 makes the relation IR the authoritative
implementation of `cross_file` `equals`, deletes the old code, and keeps "the code that
implements `equals` afterwards" within "190 non-comment lines measured at `f4e5fa225`
(`check_equals` 93, plus the source extraction it uses, `source_values` 28 and
`source_values_from` 69)".

Measuring for the W2-8 assessment showed that the 190 does not compare like with like:

- It counts `source_values` and `source_values_from`, which the set relations and `resolves`
  call too, so deleting `equals` would not delete them.
- It leaves out `CrossFileRule::mismatch` (41 lines), which builds every `equals` finding and
  which only `check_equals` and the shadow call.
- It says nothing about the new side: whether the IR's adapters, caches, and contract bridge,
  or the source extraction the replacement reuses, count against the bound.

Counting every `cross_file` line the `equals` evaluation reaches instead gives about 1,000
lines (target iteration, locations, redaction, normalization, and about 445 lines of
syntax-aware target filters), a bound so loose that it would say little about simplicity.

ADR-0025's context also says the shadow shares "one extract adapter" with `key_parity`. It does
not: `key_parity` reads through the entry, TOML, and XML row adapters and the shadow through
`ExtractAdapter`. What the two share is the run's read and decode caches, the `Adapter` trait,
the row, resource, and limit model, and (for `key_parity`) the contract bridge.

## Decision

Gate 2 counts **`equals`-only code**, on both sides by the same rule:

1. **What counts.** Lines in `alint-rules` and `alint-core`, outside tests, that only the
   `cross_file` `equals` evaluation executes: functions every caller of which is on the
   `equals` path (the shadow and tests do not count as callers). A function another relation,
   another kind, or the engine also calls is shared and does not count. A line is counted when
   it is neither blank nor only a `//` comment, from a function's `fn` line to its closing
   brace, as ADR-0025 counted.
2. **The bound is 134 lines**: `check_equals` 93 and `mismatch` 41, measured at `f4e5fa225`
   and unchanged through the W2 work. It replaces 190.
3. **Excluded on both sides:** source extraction (`source_values`, `source_values_from`),
   target iteration (`each_target`, `target_values`), locations and redaction
   (`add_source_locations`, `redact_extracted`), normalization (`spec::apply_normalize`), the
   syntax-aware target filters, and the relation layer's adapters, caches, limits, and
   contract bridge, as long as something other than `equals` uses them.
4. **Moving code counts.** A helper only the new `equals` path uses counts, even if it was
   shared before the port or lives in `alint-core`; code that stays shared does not.
5. Gate 2 still requires that the new code calls nothing in `cross_file::eval`.

## Consequences

- The bound is tighter (134 instead of 190) and measures the code the port can actually
  delete or replace.
- The v0.22 measurement is mechanical: list the functions on the new `equals` path, drop those
  with a caller off that path, and count lines the way ADR-0025 did.
- Shared infrastructure is neither a penalty nor a loophole: reusing it costs nothing, and
  moving `equals`-specific work into it counts once only `equals` uses it.

## Considered Options

- **Keep 190 and define the new side to match** (new `equals` code plus the source extraction
  it owns): rejected; the old side stays mixed, counting shared source code but not `mismatch`.
- **Count everything the `equals` evaluation reaches** (about 1,000 lines today): rejected;
  easy to pass, so it would not show whether the port became simpler.

## More Information

- [ADR-0025](0025-v021-pre-release-decisions.md): the gates this refines.
- [`v0.21/r8-readiness.md`](../design/v0.21/r8-readiness.md): the W2-8 assessment, with the
  measurements.
