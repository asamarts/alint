---
status: accepted
date: 2026-07-04
decision-makers: asamarts
---

# 0008. `git_tracked_only` is a kind-specific option; `respect_gitignore` is common

## Status

Accepted. (One of: Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-NNNN.)

## Context

`git_tracked_only` and `respect_gitignore` are the only two fields that live in a
per-kind schema branch while also being `RuleSpec` struct fields (all other
universal fields live in the shared `rule_common` schema def, and all other
kind-specific options live in the kind's `Options` struct, deserialized from
`RuleSpec.extra`). That split produces a real inconsistency:

- **The schema is stricter than the loader.** The generated JSON Schema only
  lists `git_tracked_only` on the four existence branches
  (`file_exists`/`file_absent`/`dir_exists`/`dir_absent`), so an editor rejects
  it elsewhere. But because it is a `RuleSpec` field, the loader *accepts* it on
  any rule and the rule silently ignores it (`git_tracked_mode()` returns the
  default `Off`). The `RuleSpec.git_tracked_only` doc-comment even promises that
  "rule kinds that don't support it surface a clean config error" - that error
  is **not implemented anywhere**. The v0.12 audit noted the divergence and
  filed it as a not-a-bug ("schema stricter than loader is the safe direction"),
  but the loader's silent-ignore is a latent fail-quietly gap.
- **It blocks the type-derived schema.** Because `git_tracked_only` is not in the
  existence kinds' `Options` structs, those kinds cannot be migrated to
  schemars-derived schema (ADR-0001): the derivation would emit only `root_only`
  and drop `git_tracked_only`. So the existence kinds keep a hand-authored schema
  branch with undocumented options and a hand-edited `x-since` (ADR-0007).

The two fields have *opposite* semantics, which points at opposite fixes:

- `git_tracked_only` is a **hard opt-in** honored only by the existence family; it
  changes the rule's index semantics. Setting it on any other kind is a genuine
  mistake that should fail loudly.
- `respect_gitignore` is a **benign, forward-compatible per-rule override** of the
  workspace walker setting. Only `file_exists` honors it today, but by design
  future kinds broaden coverage, and where a kind does not honor it the value
  simply falls through to the workspace default. Setting it on a not-yet-covered
  kind is harmless.

Driver: close the divergence, deliver the fail-loud the doc-comment promises, and
unblock the type-derived schema. Design doc:
[git-tracked-kind-option.md](../design/v0.14/git-tracked-kind-option.md). Related:
ADR-0001 (schema derived from Rust types), ADR-0007 (`x-since`).

## Decision

We will place each field where its semantics belong.

1. **`git_tracked_only` becomes a kind-specific option.** Remove it from
   `RuleSpec`; add it to the four existence `Options` structs (schemars-derived,
   `deny_unknown_fields`); each `build()` reads `opts.git_tracked_only`; the
   engine is unchanged (it reads the `git_tracked_mode()` trait method). Because
   the field now flows through `RuleSpec.extra` into the kind's `Options`, every
   non-existence kind's `deny_unknown_fields` rejects it at load with a clean
   error - uniformly, with no ad-hoc per-kind checks - so the loader is now
   exactly as strict as the schema, and the doc-comment's promise becomes true.
   The four existence kinds are migrated to schemars in the same change, which
   makes `x-since` type-derived and fills their empty Options descriptions.

2. **`respect_gitignore` becomes a common field.** It stays a `RuleSpec` field
   (the engine/walker plumbing is unchanged), but its schema representation moves
   from the `file_exists` branch into `rule_common`, so it is universally
   permitted and forward-compatible. Schema and loader agree: both allow it on
   any rule, and it is honored where supported.

## Consequences

Easier: schema and loader agree for both fields (no divergence, no editor-vs-CLI
surprise); a misplaced `git_tracked_only` now fails loudly at load instead of
silently no-op'ing; the existence kinds gain type-derived schema, documented
options, and type-derived `x-since`; and two ad-hoc concepts collapse into the
uniform `Options`/`rule_common` mechanisms the rest of the catalog already uses.

Harder / risk: this is a **validation tightening** - a config that (incorrectly)
set `git_tracked_only` on a non-existence kind used to load and silently ignore
it, and now errors. Such a config was already a no-op mistake, and this ships in
the v0.14 "fail loudly" cut, so surfacing it is the intended direction; but it is
technically a stricter load, noted for the changelog. `respect_gitignore` moving
to `rule_common` is a slight loosening (now permitted on every kind), which is
correct for a benign forward-compatible knob but does mean the schema no longer
flags it on a kind that does not yet honor it.

## Considered Options

- **Chosen: split by semantics** (`git_tracked_only` kind-specific,
  `respect_gitignore` common).
- **Both into `rule_common`.** Uniform but wrong for `git_tracked_only`: it would
  keep silently ignoring a real misconfiguration instead of failing loud.
- **Schema-only `Options` fields** (declare the fields for schemars but keep
  reading them from `spec.*`). Unblocks the migration without moving ownership,
  but leaves `Options` fields nothing reads (a smell) and does not fix the
  divergence.
- **Keep the hand-edited schema + just fill descriptions.** Safe and in-scope for
  the doc-drift cleanup, but leaves the wart and the aspirational doc-comment.

## More Information

Design, type-level detail, and the implementation/test plan:
[git-tracked-kind-option.md](../design/v0.14/git-tracked-kind-option.md).
Related: ADR-0001, ADR-0007, and the post-v0.12 audit's "schema stricter than
loader" note (`docs/design/v0.12/`). Implementation PR linked on merge.
