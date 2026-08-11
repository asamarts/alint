# v0.15 — Planning

Status: **Planning.** No v0.15 scope is committed yet. This directory holds
seed design docs for candidate work: each is a starting position for a
deferred deep-dive, not a finished spec.

## Candidate docs

| Doc | Scope |
|---|---|
| [`manifest-derived-scope.md`](./manifest-derived-scope.md) | Let a per-file rule take its scope from a manifest-declared path set (the "does a repo-shape linter have to read manifests, or keep the globs pure" question), as a generalisation of `scope_filter`. Boundary fixed by [ADR-0010](../../adr/0010-manifest-derived-rule-scope.md); the source-versus-output crux and the predicate-versus-`path_sets` shape stay open. **Draft.** |
| [`documented-example-fixtures.md`](./documented-example-fixtures.md) | Make each rule page's example the exact scenario the e2e suite executes: a `docs:` block marks the fixture, docs-export renders its repo + config + a real `alint check` run, and a gate requires every kind to carry a documented `fail`+`pass` pair - so the docs cannot drift from what actually runs. Decision fixed by [ADR-0014](../../adr/0014-rule-page-examples-are-executed-fixtures.md); the five design forks are resolved, implementation is phased. **Draft.** |
