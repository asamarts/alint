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
