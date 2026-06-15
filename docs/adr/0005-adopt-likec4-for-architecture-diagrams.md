---
status: accepted
date: 2026-06-14
decision-makers: asamarts
---

# 5. Adopt LikeC4 for architecture diagrams

## Status

Accepted (2026-06-14).

## Context

WS3 (`docs/design/architecture-as-code.md`) shipped a generated Mermaid crate
graph and a hand-modeled Structurizr C4 model gated on the crate set. The
architecture page still lacks domain/model diagrams (the rule catalogue, the
config DSL) and sequence/flow diagrams (the execution pipeline, config load,
fix, ...); it describes them in prose only. Separately, the Mermaid crate graph
did not render on alint.org (Starlight has no Mermaid renderer), so the
"renders natively on alint.org" claim was false.

We want the new diagrams to be provably generated and consistent (no drift),
visually polished, interactive or animated where it helps, and still renderable
in GitHub markdown. GitHub markdown rendering is effectively Mermaid-only, while
a polished interactive renderer is not Mermaid, so no single artifact meets all
four requirements. The resolution is one model with multiple render targets.

## Decision

We will adopt LikeC4 as the single architecture model.

- One LikeC4 workspace at `docs/design/architecture/model/` holds hand-authored
  intent (`alint.c4`: C4 context/containers/components plus behavioral dynamic
  views) and generated `*.gen.c4` fragments (`xtask gen-model`, starting with the
  rule-kind taxonomy from `docs/rules.md`).
- alint.org renders the views interactively via the standalone `<likec4-view>`
  web component (which also closes the render gap); GitHub gets `likec4 gen
  mermaid` output, assembled by `xtask gen-mermaid` into a `DIAGRAMS.md` gallery
  (the dual-surfaced `ARCHITECTURE.md` links to it rather than embedding Mermaid
  that would double-render on alint.org).
- Drift is controlled in two layers: generated fragments are byte-gated by
  `xtask gen-model --check`; the hand-authored model is gated by `likec4
  validate` (structural integrity of every flow step) plus a crate-set check
  against `cargo metadata`.
- `likec4 validate` runs in the alint repo CI (Node added to `ci/Containerfile`),
  not deferred to alint.org, so a broken model fails at the source of truth.

## Consequences

Positive: behavioral/flow diagrams become model-provable (a flow step cannot
reference an element or relationship that does not exist), not merely
vocabulary-checked; the architecture page gains domain and sequence diagrams; the
render gap closes; the diagrams share one visual language.

Negative and accepted: a new model language (close to the existing Structurizr
DSL); a Node toolchain on the Rust repo's self-hosted runner, which needs a runner
image rebuild to take effect (until then `ci/scripts/likec4.sh` skips with a loud
warning rather than failing CI); LikeC4's dynamic-view sequence variant and its
Mermaid codegen are newer, de-risked by a fidelity spike (LikeC4 1.58.0). The
config DSL is modeled as a domain map, not a field-level ER diagram, because
LikeC4 is not an ER tool and the field detail already lives in the generated
JSON-schema reference.

## Considered Options

- Mermaid-only (hand-authored plus a vocabulary gate): unbeatable GitHub-native
  rendering and zero new dependencies, but behavioral diagrams would be only
  vocabulary-checked, not model-provable, and not interactive.
- Structurizr (extend `workspace.dsl`, export Mermaid, including dynamic-to-
  sequence): one C4 model, but it needs Java/Structurizr in CI and its
  interactive viewer is not cleanly embeddable in a static Astro site.
- D2: the best layout and a browser-free CLI, but it is not a C4 model (no
  dynamic-view provability) and not GitHub-native.
- LikeC4 (chosen): one model, multiple targets; provable, interactive, and
  GitHub-renderable via Mermaid codegen, Node-only.

## More Information

- `docs/design/architecture-diagrams.md` (design, surface area, drift control,
  open items).
- `docs/design/architecture-as-code.md` (the WS3 work this extends), ADR-0001
  (spec-driven development), ADR-0003 (engine dispatch), ADR-0004 (extends trust
  boundary).
- `ci/scripts/likec4.sh`, `xtask/src/gen_model.rs`,
  `docs/design/architecture/model/`.
