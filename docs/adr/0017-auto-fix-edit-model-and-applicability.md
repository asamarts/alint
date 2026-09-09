---
status: proposed
date: 2026-09-08
decision-makers: asamarts
---

# 0017. Auto-fix edit model and applicability tiers

## Status

Proposed. Companion design doc:
[`docs/design/auto-fix.md`](../design/auto-fix.md), which carries the research survey (the
seven-class fix taxonomy), the per-format structured-write feasibility verdicts, the
architecture, and the phased build plan. This ADR records the two load-bearing decisions
that everything in that plan depends on.

## Context

alint's auto-fix surface is narrow and structurally capped. Only 16 of 94 distinct rule
kinds (about 17%) attach a fixer, and the single largest family, structured query (25 kinds
across JSON / YAML / TOML / XML / dotenv / properties / INI / HCL), is entirely unfixable.
Two properties of the current implementation are the cause, not the symptom:

- **The edit model is whole-file only.** `FixEdit` has four variants (`SetContent`,
  `CreateFile`, `DeleteFile`, `RenameFile`), none of which expresses a located edit at a
  computed byte range or a permission-bit change. Every existing fixer either writes a whole
  file from a template, deletes/renames a whole file, or runs a pure byte-to-byte transform
  over the whole file. A "set the value at this path" or "remove this matched token" fix
  cannot be expressed.
- **The apply loop is single-pass and unaware of fix safety.** `Engine::fix` evaluates each
  rule once and applies its fixer, with no fixpoint, no re-check, no conflict detection
  between edits, and no way to say a fix exists but is not safe to apply unattended.
  Convergence rests entirely on each fixer's own idempotency guard.

The state of the art (ESLint, Ruff) converges on a different substrate: a fix is a
`{range, text}` edit; the engine collects candidates, sorts them, applies the non-overlapping
ones in one pass while skipping conflicts, and re-lints to a fixpoint. Ruff adds a three-tier
`Applicability` (Safe / Unsafe / DisplayOnly), safe-by-default, `--unsafe-fixes` opt-in.
Format-preserving config editing (`toml_edit`, `node-jsonc-parser`) is a span-splice against
a CST or spanned parser, never a parse-then-dump. alint's own unfixed-rule notes already sort
into buckets that map cleanly onto a Safe / Unsafe / Suggestion / Never model, so the model
fits the existing reasoning rather than imposing a new one.

Constraints: determinism of fixes (constitution invariant 1); bounded fixes
(`fix_size_limit`, invariant 4); the spawn/network trust boundary and path confinement
(ADR-0004, invariants 5 and 6); telemetry-free at runtime; backward compatibility for the 12
existing ops.

## Decision

We will make three changes, together, as the foundation of the auto-fix expansion.

**1. A ranged edit primitive and a batched, verifying apply engine.** We will add
`FixEdit::ReplaceRange { path, range, content }` (a byte-span splice; insert is a zero-width
range, delete is empty content) and `FixEdit::SetMode { path, mode }` (chmod, unix-gated),
keeping the existing four variants (`RenameFile` already covers cross-directory moves, so no
`MoveFile` is added). The edit-producing path is generalized to **rule level** via
`collect_edits(&[Violation], file, bytes, root) -> Vec<(FixEdit, Applicability)>`, so a rule
that fans out over N matches (structured-query violations carry no span, and `*_path_absent`
is one file-level violation over N nodes) can re-query with a located API and emit exactly
one edit per node; simple fixers keep the per-violation `apply` / `fix_edit` path.
`Engine::fix` changes from evaluate-then-write to: collect edits (read-only, size-guarded at
this step), filter by applicability threshold, group per file, sort by a total order
`(start, end, rule_index, violation_index)` and apply non-overlapping edits while skipping
conflicts (with Ruff-style isolation groups), verify a re-parse postcondition (reject and
demote to Suggestion, memoized per edit, any edit that would produce an unparseable file), and
write atomically. The **fixpoint loop lands in Phase 1**, not Phase 0, so the Phase 0 engine is
a single behavior-preserving pass; the fixpoint carries an index-invalidation rule (a
path-mutating edit forces a deterministic re-walk) and a loud non-convergence cap. A fix that
spans files is applied as an all-or-nothing multi-file transaction. Idempotence is a tested
contract per fixer. Format-preserving structured edits are span-splices against a per-format
CST or spanned parser, never a parse-then-dump, with a per-format value serializer for quoting,
type fidelity, and separator surgery.

**2. A four-state applicability model, defined by application policy.** Every fixer declares
one of: **Safe** (applied by default), **Unsafe** (applied only with `--unsafe-fixes`),
**Suggestion** (never written; surfaced as a concrete `proposed_edit` payload in the
`agent` / `json` / LSP output), and **Never** (no fix proposed). The tier is the policy; the
properties (idempotent, output re-parses, discards no human content, target unambiguous) are
the inputs an author uses to pick it. Safe `set_value` is restricted to a scalar replacing an
existing scalar; object/array values, key insertion, and nested creation are Suggestions.
`alint fix` applies only Safe fixes by default. The report gains a `Suggested(edit)` outcome and
the agent format gains an `applicability` field, reconciling `has_unfixable_errors` so a
Suggested outcome is not an unresolved error. The 12 existing ops are classified Safe on
introduction so current behavior is unchanged.

**3. A fixer trust boundary distinct from the kind-level spawn gate.** A fixer is a new trust
surface, and the existing `SPAWNING_RULE_KINDS` allowlist gates the rule **kind**, not a
**fixer** attached to a non-spawning kind. We will add a fix-level gate: content-mutating fix
ops (`replace`, `set_value`, `remove_value`, `insert_header`, `sync_from`) and spawning fix ops
(`git_untrack`, a `command`-backed fix, regenerate-from-command) are honored only from the
user's own top-level config, never from an `extends:`-ed ruleset; applicability promotion is
top-level-only (an inherited fixer may be demoted but never promoted), and an inherited
content-mutating fixer defaults to Suggestion.

## Consequences

Easier: the structured-query family and located content replacement become fixable through a
shared substrate rather than per-rule special cases; the LSP gets minimal-diff `TextEdit`s
instead of whole-document replacements; conflicting fixers compose safely; agents get
first-class "proposed but not auto-applied" edits via a new `Suggested` outcome and a
`proposed_edit` payload alongside the existing `fix_command`; every fixer carries a
machine-readable safety declaration.

Harder: `Engine::fix` grows a real scheduler (rule-level collect, total-order sort,
overlap-skip, isolation, postcondition, and from Phase 1 a fixpoint with index invalidation)
instead of a loop, which is more code to keep deterministic and more to test; each structured
format needs its own span resolver and value serializer because the lossy `serde_json::Value`
pipeline cannot be reused for write-back; multi-file fixes need all-or-nothing transactions;
`fix` must become baseline-aware so it does not rewrite grandfathered findings; and a new
fix-level trust gate must be built and maintained because reusing the kind-level spawn gate
would leave a fixer-on-a-non-spawning-kind hole. Reclassifying an existing op (treating
`file_remove` as Unsafe) is a safety-default and semver decision rather than a free change.

## Considered Options

- **Keep whole-file `SetContent` only, compute spliced bytes inside each fixer.** Rejected as
  the primary model: it hides the edit range from the engine, so conflict detection and
  minimal LSP diffs are impossible, though a phase may still emit `SetContent` where a ranged
  edit adds no value.
- **Two-tier autofix/suggestion (ESLint style) instead of four states.** Rejected: it cannot
  distinguish "apply with opt-in" from "never apply", which is exactly the Unsafe vs
  Suggestion line alint's own rule notes already draw.
- **Parse-then-reserialize for structured fixes.** Rejected: it destroys comments, key order,
  and whitespace on every edited config, which is the precise failure that made Repolinter's
  remediation unusable.
- **Keep the per-violation fixer shape for structured edits.** Rejected: a structured-query
  violation carries no span and `*_path_absent` is one file-level violation over N nodes, so a
  per-violation call cannot tell which node it fixes; the rule-level `collect_edits` binding is
  required.
- **Reuse the kind-level `SPAWNING_RULE_KINDS` gate for spawning fixers.** Rejected: it gates
  the rule kind, so a spawning fixer attached to a non-spawning kind (a `git_untrack` fix on
  `file_absent`) would pass untouched; a distinct fix-level gate is required.

## More Information

Design doc: [`docs/design/auto-fix.md`](../design/auto-fix.md). Trust boundary:
[ADR-0004](0004-extends-trust-boundary-and-path-confinement.md). Dispatch and determinism:
[ADR-0003](0003-rule-engine-dispatch-and-determinism.md). Prior art with primary sources is
collected in the design doc's references section (ESLint, Ruff, `toml_edit`,
`node-jsonc-parser`, and the archived Repolinter).
