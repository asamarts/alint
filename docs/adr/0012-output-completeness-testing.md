---
status: accepted
date: 2026-08-06
decision-makers: asamarts
---

# 0012. Output-completeness as a tested contract

## Status

Accepted (2026-08-08). Commitments 1 and 2 shipped: the positive-invariant gates
driven by `all_kinds.yaml` (`explain_surfaces_configured_rule_detail`,
`explain_covers_every_registered_kind`) and the `list` / `export-agents-md`
remediations (the human `kind` + `[fix]` marker, the scope fallback, and
kind-specific options). Commitment 3's structural-prevention choice is decided in
ADR-0013 - retain the `RuleSpec` projection on `RuleEntry` and render from it; the
`Rule::message()` accessor is rejected there. Originally prompted by the `alint
explain` gap fixed in the Tier 1 explain change and a 2026-08-06 audit of every
subcommand's output against its data model.

## Context

`alint explain <rule>` shipped rendering only `id`, `level`, and `policy_url`,
silently dropping the rule's `kind`, `paths`, and author `message:` — all present
in the loaded config. Users saw most rules "explain" to two bare lines. The defect
sat in released binaries and **escaped both unit and e2e tests.** Understanding
*why the tests missed it* matters more than the one fix.

Root cause of the escape (audit, 2026-08-06):

1. **The one `explain` snapshot froze the thin output as correct.**
   `crates/alint/tests/cli/explain.stdout` was a byte-exact capture of the
   three-line output, and its fixture rule (`explain.in`) deliberately set **no
   `message:`**, so the omission was baked in as the expected value. Worse,
   `TRYCMD=overwrite` regenerates the snapshot from whatever the binary currently
   prints — so a snapshot can only ever ratify current behaviour, never judge it.
2. **The only machine-format coverage asserted nothing about content.** The
   output-contract gate `cli_format_contract.rs::format_json_is_never_a_silent_
   human_fallthrough` (G1a) asserted only that stdout **parses as JSON** — zero
   fields. Thin JSON passes it perfectly.
3. **Format tests checked exit codes only** (`cli_consistency.rs`), never output.
4. **No unit test** covered `cmd_explain` / `explain_json` / `cmd_list` /
   `list_json`.

The missing structural property: **no test asserted a positive completeness
invariant** — "for a rule that sets `message:`/`kind:`/`paths:`, the command's
output actually CONTAINS them." Every existing check was either exact-equality
against a baseline that already omitted the data, or a parse/exit-code check the
thin output satisfied.

This is not unique to `explain`. The same lossy `RuleSpec` -> `RuleEntry`
projection (only `kind` was retained at load) means:

- **`list`** — the human output hides `kind`, `categories`, and `fixable` that
  `list --format json` emits, and both formats drop `message`/`paths`.
- **`export-agents-md`** — falls back to `"<kind> rule"` (e.g. "file_exists rule")
  when a rule sets no `message:`, dropping the actionable `paths:`.
- **`explain`/`list` JSON parity** — `explain --format json` omitted the rule kind
  and categories that `list --format json` carries (fixed for `explain` in the
  Tier 1 change).

## Decision

We will treat **output completeness as an explicit, tested contract**, and close
the class of defect at the type level where practical. Three commitments:

1. **Positive-invariant gates, driven by `all_kinds.yaml`.** Beyond snapshots
   (which guard against regression), add completeness assertions that, for a rule
   configured with a `message:`/`kind:`/`paths:`, the rendered `explain` (and
   `list`) output *contains* those values. The Tier 1 change seeds this with
   `explain_surfaces_configured_rule_detail`; generalise it to iterate the
   `crates/alint-dsl/tests/fixtures/all_kinds.yaml` fixture (which already gives
   most kinds a `message:`; complete it so every kind does) so a newly registered
   kind cannot render blank.
2. **Audit and remediate the shared defect.** The `list` human gap and the
   `export-agents-md` fallback (above) are tracked as follow-ups to this ADR and
   fixed or explicitly deferred-with-rationale, using the same mechanism.
3. **Prevent recurrence structurally.** Two complementary moves, to be sequenced
   in the follow-up:
   - **A `Rule::message()` accessor.** 60 of ~71 rule structs already store
     `message: Option<String>`, and 70 kinds use the `rule_common_impl!` macro
     that already stamps `id`/`level`/`policy_url` from same-named fields. Adding
     `fn message(&self) -> Option<&str> { None }` to the `Rule` trait and emitting
     it from the macro makes "surface the message" a near-mechanical, type-level
     obligation wired from a field the rule already holds; the ~11 kinds without
     the field keep the `None` default.
   - **Retain the `RuleSpec` projection in `LoadedConfig`.** Heterogeneous data
     (paths, kind-specific options) is a poor fit for 71 bespoke trait methods.
     Keeping the specs (or a display projection) alongside `entries` — as
     `export_agents_md::collect_directives` already renders straight from
     `config.rules` — lets `explain`/`list` show everything with zero per-kind
     code. The Tier 1 change took the narrower step of retaining `paths`/`message`
     on `RuleEntry`; this ADR decides whether to generalise to the full projection
     (and, if so, retire the ad-hoc `RuleEntry` display fields in its favour).

## Consequences

Easier:

- The whole class of "a command silently drops data it has" becomes catchable, not
  just the one instance we tripped over.
- New rule kinds and new commands inherit the completeness gate for free (the
  `all_kinds.yaml` driver already enumerates every kind).

Harder, and accepted:

- Completeness invariants are more code than a snapshot, and the `all_kinds.yaml`
  driver is one more thing to keep green. We accept it: a snapshot proves output
  is *stable*, never that it is *adequate*, and adequacy is exactly what failed.
- The `Rule::message()` and projection changes touch many kinds and the load path.
  Mechanical, but not free; sequenced behind the gate so the gate proves the fix.

## Considered Options

- **Snapshot-only (status quo).** Rejected: snapshots ratify current behaviour and
  `TRYCMD=overwrite` re-bakes it; this is precisely what froze the bug.
- **Positive completeness invariants + structural prevention (chosen).** Catches
  the class, and the type-level accessor makes the common case a compile-time
  obligation.
- **Trait accessors only.** Clean for `message`, but a poor fit for heterogeneous
  `paths`/options — would need 71 bespoke methods.
- **Spec-projection only.** Renders everything with no per-kind code, but gives no
  compile-time guarantee a *new* kind is surfaced; pairs best with the chosen
  option's gate.

## More Information

- The triggering fix and its seed gate: the Tier 1 `explain` change
  (`explain_surfaces_configured_rule_detail` in
  `crates/alint/tests/cli_format_contract.rs`).
- Related: ADR-0011 (per-kind explanations — the *other* explain gap) and ADR-0009
  (the `list` / `rules` surfaces these findings touch).
- Anchors: the lossy projection at load (`crates/alint/src/main.rs`, the
  `RuleEntry::with_kind` site), the `Rule` trait + `rule_common_impl!`
  (`crates/alint-core/src/rule.rs`), and the `all_kinds.yaml` fixture.
