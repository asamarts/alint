---
status: accepted
date: 2026-07-04
decision-makers: asamarts
---

# 0008. `git_tracked_only` and `respect_gitignore` are kind-specific options

## Status

Accepted. (One of: Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-NNNN.)

## Context

`git_tracked_only` and `respect_gitignore` were the only two fields that lived in a
per-kind schema branch while also being `RuleSpec` struct fields. Every other
universal field lives in the shared `rule_common` schema def; every other
kind-specific option lives in the kind's `Options` struct, deserialized from
`RuleSpec.extra`. Housing these two on `RuleSpec` produced a real inconsistency:

- **The schema was stricter than the loader.** The generated JSON Schema listed
  `git_tracked_only` only on the four existence branches
  (`file_exists`/`file_absent`/`dir_exists`/`dir_absent`), so an editor rejected
  it elsewhere. But because it was a `RuleSpec` field, the loader *accepted* it on
  any rule and the rule silently ignored it (`git_tracked_mode()` returns the
  default `Off`). The `RuleSpec.git_tracked_only` doc-comment even promised that
  "rule kinds that don't support it surface a clean config error" - that error was
  **not implemented anywhere**. The v0.12 audit noted the divergence and filed it
  as a not-a-bug ("schema stricter than loader is the safe direction"), but the
  silent-ignore was a latent fail-quietly gap.
- **It blocked the type-derived schema.** Because `git_tracked_only` was not in the
  existence kinds' `Options` structs, those kinds could not be migrated to
  schemars-derived schema (ADR-0001): the derivation would emit only `root_only`
  and drop `git_tracked_only`. So the existence kinds kept a hand-authored schema
  branch with undocumented options and a hand-edited `x-since` (ADR-0007).

Both fields turn out to be **kind-specific behaviors**, honored by a fixed set of
kinds and a genuine mistake elsewhere:

- `git_tracked_only` is a **hard opt-in** honored by the existence family; it
  changes the rule's index semantics. Every one of the four existence kinds reads
  it (via `git_tracked_mode()`); no other kind does.
- `respect_gitignore` is a **per-rule override** of the workspace walker setting,
  honored **only by `file_exists`** for literal-path patterns - the bazel-style
  "tracked AND gitignored" escape hatch (`docs/development/CONFIG-AUTHORING.md`
  pitfall #18). No other kind - not even the sibling existence kinds
  `file_absent`/`dir_exists`/`dir_absent` - reads the per-rule field.

Setting either field on a kind that does not honor it is a mistake that today loads
and is silently ignored. (An earlier draft of this ADR treated `respect_gitignore`
as a "benign, forward-compatible" universal and moved it to `rule_common`; an
adversarial review refuted that - see Considered Options. It is not universal:
`no_bom: {respect_gitignore: false}` validated, loaded, and was silently ignored -
the exact fail-quietly this ADR exists to close.)

Driver: close the divergence, deliver the fail-loud the doc-comment promises, and
unblock the type-derived schema. Design doc:
[git-tracked-kind-option.md](../design/v0.14/git-tracked-kind-option.md). Related:
ADR-0001 (schema derived from Rust types), ADR-0007 (`x-since`).

## Decision

Both fields move off `RuleSpec` into the `Options` struct of exactly the kinds that
honor them - the uniform mechanism the rest of the catalog already uses.

1. **`git_tracked_only` becomes a kind-specific option on the existence family.**
   Remove it from `RuleSpec`; add it to the four existence `Options` structs
   (schemars-derived, `deny_unknown_fields`); each `build()` reads
   `opts.git_tracked_only`; the engine is unchanged (it reads the
   `git_tracked_mode()` trait method). The four existence kinds are migrated to
   schemars in the same change, which makes `x-since` type-derived and fills their
   empty Options descriptions.

2. **`respect_gitignore` becomes a `file_exists`-only option.** Remove it from
   `RuleSpec`; add it to `file_exists`'s `Options` struct alone (the sole kind that
   honors it); `build()` reads `opts.respect_gitignore`. It is dropped from
   `rule_common`. The workspace-level `Config.respect_gitignore` (the walker
   default) is a separate field and is unchanged.

Because both fields now flow through `RuleSpec.extra` into a kind's `Options`,
every kind whose `Options` lacks the field rejects it at load via
`deny_unknown_fields` - uniformly, with no ad-hoc per-kind checks. The loader is now
exactly as strict as the schema for both fields, and the doc-comment's promise
becomes true. Nested rules strip both via `PARENT_FIELDS` (unchanged).

## Consequences

Easier: schema and loader agree for both fields (no divergence, no editor-vs-CLI
surprise); a misplaced `git_tracked_only` or `respect_gitignore` now fails loudly at
load instead of silently no-op'ing; the existence kinds gain type-derived schema,
documented options, and type-derived `x-since`; `respect_gitignore` regains its
user-facing Options-table row on the `file_exists` rule page (moving it to
`rule_common` had silently dropped it, since common fields are not rendered in
per-rule tables); and two ad-hoc concepts collapse into the uniform `Options`
mechanism.

Harder / risk: this is a **validation tightening** - a config that (incorrectly) set
`git_tracked_only` on a non-existence kind, or `respect_gitignore` on any kind but
`file_exists`, used to load and silently ignore the field, and now errors. Such
configs were already no-op mistakes, and this ships in the v0.14 "fail loudly" cut,
so surfacing them is the intended direction; noted for the changelog. If a future
kind gains genuine `respect_gitignore` support, add the field to that kind's
`Options` (and implement the read) - the same pattern, per kind, rather than a
schema-wide permit that over-promises.

## Considered Options

- **Chosen: both kind-specific**, in the `Options` of exactly the kinds that honor
  them (`git_tracked_only` on the four existence kinds; `respect_gitignore` on
  `file_exists`).
- **`respect_gitignore` into `rule_common`** (an earlier draft's choice). Rejected
  on adversarial review: `respect_gitignore` is honored only by `file_exists`, so a
  universal permit reintroduces the exact schema-looser-than-engine fail-quietly
  this ADR closes - `no_bom: {respect_gitignore: false}` validates, loads, and is
  silently ignored - and it dropped the field's documentation from the `file_exists`
  rule page (common fields are not rendered in per-rule Options tables). "Forward
  compatibility" does not justify it: when a kind gains real support, add the option
  to that kind then (YAGNI).
- **Both into `rule_common`.** Uniform but wrong for both: it keeps silently
  ignoring real misconfigurations instead of failing loud.
- **Schema-only `Options` fields** (declare the fields for schemars but keep reading
  them from `spec.*`). Unblocks the migration without moving ownership, but leaves
  `Options` fields nothing reads (a smell) and does not fix the divergence.
- **Keep the hand-edited schema + just fill descriptions.** Safe and in-scope for
  the doc-drift cleanup, but leaves the wart and the aspirational doc-comment.

## More Information

Design, type-level detail, and the implementation/test plan:
[git-tracked-kind-option.md](../design/v0.14/git-tracked-kind-option.md).
Related: ADR-0001, ADR-0007, and the post-v0.12 audit's "schema stricter than
loader" note (`docs/design/v0.12/`). Implementation PR linked on merge.
