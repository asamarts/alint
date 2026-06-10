---
status: accepted
date: 2026-06-10
decision-makers: asamarts
---

# 1. Adopt spec-driven development

## Status

Accepted (2026-06-10).

## Context

alint already has a strong, mostly-implicit spec-driven culture: a design-doc-first
workflow, a JSON Schema for the config DSL, a canonical `all_kinds.yaml`, declarative
bundled rulesets, and a real multi-layer drift-control system (the `coverage_audit_*`
tests, `check-version-pins.sh`, `xtask docs-export`). Roughly 70 percent of a
spec-driven model is therefore already in place.

Three weaknesses remain, ranked by risk:

1. The DSL JSON Schema (`schemas/v1/config.json`) is over 2000 lines of hand-written
   JSON kept in sync with the Rust types by human discipline plus a byte-copy into the
   crate. No test checks that its per-kind option keys match the actual serde structs,
   so a field rename propagates only by hand. This is the largest untracked drift class.
2. The alint.org marketing surface (version strings, counts, feature claims) lives in a
   separate repo and is not audited from here, so it can advertise a stale reality.
3. Per-kind reference prose, several "second list" hazards, and architecture intent are
   maintained by hand with no machine check, and there is no architecture decision log,
   despite alint shipping a ruleset that lints ADRs in other repos.

A full analysis is in `docs/design/spec-driven-development.md`.

## Decision

We will adopt an explicit spec-driven development model, governed by one rule:

> Put machine-checkable contracts on the highest rung (generate everything downstream
> from one source; fail CI on any diff). Keep prose specs (design docs, ADRs) as
> point-in-time scaffolding. Never make English prose a regenerable source of truth.

Concretely, we will execute the five-workstream program in
`docs/design/spec-driven-development.md`:

- WS1 Contracts as source of truth: derive the JSON Schema from the Rust types
  (`schemars`); generate the rule and CLI reference; pin output with `insta`/`trycmd`;
  emit a `facts.json` manifest. The keystone.
- WS2 Architecture Decision Records: MADR 4.0.0 in `docs/adr/`, dogfooded by alint's own
  `docs/adr@v1` ruleset.
- WS3 Architecture diagrams: a hand-modeled Structurizr C4 model (CI-rendered) plus
  code-extracted crate and module graphs.
- WS4 Pragmatic formal methods: Kani (panic/overflow freedom on the pure core), Miri,
  `contracts`, and at most one Stateright model. Heavyweight deductive verifiers
  (Verus/Creusot/Prusti) are explicitly out of scope.
- WS5 Close the alint.org drift loop via the `facts.json` contract.

Wherever a CI gate is needed, we prefer an alint rule (for example `generated_file_fresh`
for regenerate-and-diff freshness) over a bespoke script, so alint enforces its own
spec-driven discipline on itself.

## Consequences

Positive: most drift becomes machine-detectable and auto-fixable; the schema can no
longer disagree with the parser; "alint's own docs and marketing are enforced by alint"
becomes a true, on-brand claim; architectural decisions gain a durable, reviewable record.

Negative and accepted: schemars adoption requires a fidelity spike before the hand-written
schema can be retired; the program adds CI jobs (Kani, Miri) and new dev-dependencies that
must clear `cargo-deny`; ADRs add a small per-decision authoring cost. These are bounded and
the program is phased so each step ships independently and leaves the tree green.

## Considered Options

- Status quo (extend the `coverage_audit_*` audits only): cheap, but leaves the keystone
  schema-drift class and the alint.org surface unaddressed.
- Heavyweight formal verification of the engine (Verus/Creusot): high assurance, but a
  ~5:1 proof-to-code burden that is not maintainable for a small team on application logic.
- The chosen middle path: generate-and-gate for contracts, pragmatic formal methods only
  where they pay off, prose specs kept as scaffolding.

## More Information

- Full proposal and phased rollout: `docs/design/spec-driven-development.md`.
- Constitution of invariants: `docs/design/constitution.md`.
- Related backfilled decisions: ADR-0002 (config language), ADR-0003 (engine dispatch and
  determinism), ADR-0004 (extends trust boundary).
