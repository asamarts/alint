# Design doc: <feature or rule kind>

Status: Draft. (Draft | Implemented in <commit> | Superseded by <doc>.)
Decisions: <ADR-NNNN, if this introduces or changes an architectural decision; else "none">
Demand evidence: <link to case-study / launch-evidence rows, for rule kinds>

<!--
This is the canonical design-doc template. It codifies the seven-section convention the
project has followed since v0.7. Copy it into docs/design/vX.Y/<name>.md before writing
code. At merge time, flip Status to "Implemented in <commit>". The doc is the spec; it is
not generated from code and it is not rewritten after the version ships (it becomes the
archival record under that version's directory).

If the work changes an architectural decision (a new dispatch class, a new trust gate, a
new dependency of consequence), also write an ADR under docs/adr/ and link it above.
-->

## 1. Problem

The user pain, with concrete real-repo evidence. Why this is worth building. What breaks
or stays unsolved without it.

## 2. Surface area

The engine, DSL, and schema changes required. Sketch the exact YAML the user writes,
including every option key and its default.

## 3. Semantics

What the engine does on each evaluation path: inputs, the matching and extraction steps,
what counts as a violation, fix behavior, dispatch class (per-file / cross-file / single-shot).

## 4. False-positive surface

What could fire wrongly, and the planned mitigations. This section is mandatory; a rule kind
with no analysis of its false positives is not ready.

## 5. Implementation notes

Module location, new dependencies (and their cargo-deny status), complexity estimate, and any
invariant from `../constitution.md` the implementation must uphold.

## 6. Tests

The coverage plan: the firing scenario, the silent scenario, unit gaps, and any bench-compare
thresholds. Every registered kind needs both a firing and a silent e2e scenario (constitution 8).

## 7. Open questions

Decisions to resolve before implementation. Resolve them inline with an editorial note when the
work lands, so the doc records both the question and its answer.
