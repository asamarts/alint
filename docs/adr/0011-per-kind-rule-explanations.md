---
status: proposed
date: 2026-08-06
decision-makers: asamarts
---

# 0011. Per-kind rule explanations via a generated summary bridge

## Status

Proposed. (One of: Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-NNNN.)

Proposed as the Tier 2 follow-up to the `alint explain` enrichment (the Tier 1
change that surfaces a rule's configured detail). This ADR covers the remaining
gap: kind-level explanation.

## Context

`alint explain <rule>` and the `alint rules` catalog answer questions about a
rule, but neither can answer the most basic one: **what does this KIND check, and
why.** After the Tier 1 change, `explain` shows a rule's kind, paths, message,
`when:`, and fix (all config-instance data). It still cannot say what
`file_content_forbidden` *is*, because that prose exists only in `docs/rules.md`.

`docs/rules.md` is the single source of truth for per-kind prose, and `xtask`'s
`docs_export::first_sentence` already extracts a one-line summary per kind, but
only to feed the website (family tables, the master index, the SEO meta
description). None of it is compiled into the binary. The in-crate
kind-to-category bridge (`crates/alint-rules/src/categories_gen.rs`) deliberately
carries categories and **not** summaries; that omission is a documented deferral
in `docs/design/rule-categories.md` (the "deferred-summary" note) and ADR-0009's
Data-source section. So the terminal has no offline, config-independent way to
describe a kind, and `alint rules show <kind>` plus `list --search` over
summaries (both deferred in ADR-0009) have no data to read.

## Decision

We will compile a **per-kind one-line summary into the binary via a generated
bridge**, extracted from the `docs/rules.md` SSOT by the existing
`docs_export::first_sentence`, and gated by a `gen-<x> --check` so it can never
drift from the docs. Concretely:

- Extend the generated in-crate artifact (either `categories_gen.rs` or a sibling
  `kind_docs_gen.rs`) to carry each canonical kind's summary string, keyed the
  same way categories are. Aliases resolve to their canonical kind's summary.
- `alint explain` prints the kind's summary under its `kind:` line, plus a deep
  link to `https://alint.org/docs/rules/<family>/<kind>/` for the full prose.
- `alint rules list` gains a description column and `alint rules show <kind>`
  becomes possible; `list --search` can match summaries, closing the ADR-0009
  catalog deferrals with one data source.
- The generator strips the `**Categories:**` line before summarizing (the same
  guard `rule-categories.md` already specifies for the site), and a
  `gen-<x> --check` gate in the docs CI job fails if the committed artifact is
  stale, exactly like `gen-facts --check` and `gen-categories --check`.

## Consequences

Easier:

- `explain` becomes kind-aware: a user learns what a kind does without leaving the
  terminal or reading YAML.
- The `rules` catalog gains descriptions and summary search becomes possible — the
  ADR-0009 catalog deferrals resolve with one data source.
- Summaries stay honest: one SSOT (`docs/rules.md`), one extractor, one gate.

Harder, and accepted:

- **Doc-prose edits leave the docs-only CI fast lane.** Editing a kind's summary
  in `docs/rules.md` now regenerates a committed in-crate artifact, so the change
  is no longer docs-only and must ship in a release to reach CLI users (the site
  reflects it sooner). This is the exact tradeoff ADR-0009 named and deferred; we
  accept it now, scoped to the *first sentence* only (not the whole body) to keep
  the churn surface minimal.
- One more generated artifact and gate to maintain, alongside facts.json and the
  categories bridge.

## Considered Options

- **Generated summary bridge from `docs/rules.md` (chosen).** One SSOT, reuses
  `first_sentence`, drift-gated, and unlocks the catalog deferrals.
- **Leave summaries in docs only; `explain` just deep-links out.** No new
  artifact, but no offline/terminal kind description and no summary search — the
  status quo this ADR exists to end.
- **Hand-write a summary table in Rust.** Immediate, but a second source of truth
  that silently drifts from `docs/rules.md` — the drift class alint itself exists
  to prevent.
- **Compile each kind's full prose body into the binary.** Real kind help in the
  terminal, but tens of KB of doc prose in the binary and a large churn surface;
  the one-line summary is the 90% at a fraction of the cost.

## More Information

- Builds on the Tier 1 `explain` enrichment (config-instance detail) and its
  completeness gate (`explain_surfaces_configured_rule_detail`).
- The deferral this closes: `docs/design/rule-categories.md` (the deferred-summary
  note) and ADR-0009 (rule discovery; the `rules show` / `list --search`
  deferrals and the CI fast-lane tradeoff).
- Reused machinery: `xtask` `docs_export::first_sentence` (the summary extractor)
  and `crates/alint-rules/src/categories_gen.rs` (the generated-bridge pattern).
