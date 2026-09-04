# Animated diagrams -- design moved out of this repo

The animated-diagram engine, DSL, prototype build plan, and supporting research have
moved into a dedicated project, kept internal for now while it is dogfooded across
several repositories before any public release.

alint's docs will consume that project's output as **vendored static SVG** (committed
assets); this public repo takes on no runtime dependency on the internal project. If
and when animated diagrams ship to the live alint docs, that production decision will
be recorded as an ADR here.

This breadcrumb replaces the former `animated-diagrams.md` and
`animated-diagrams-prototype.md` design docs (and a later DSL/engine/packaging
analysis), which now live with the engine.

## Concepts-docs diagrams: inline SVG + CSS (interim)

Decided 2026-09-04 (see [`concepts-section-redesign.md`](concepts-section-redesign.md)):
because the engine is still internal and unbuilt, the Concepts docs use
**hand-authored inline SVG animated with CSS** for their concept diagrams --
animated, reduced-motion-guarded, Starlight-token-themed, and taking no runtime
dependency (CSS animates natively). This revises the "vendored static SVG only"
expectation above for those pages: the diagrams are animated, not static. When the
engine ships, each `<svg>` block can be regenerated from it without changing the
page prose. No ADR: this is a reversible docs technique under ADR-0005's diagram
program.
