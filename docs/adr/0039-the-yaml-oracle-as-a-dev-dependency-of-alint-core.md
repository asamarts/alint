---
status: superseded by ADR-0041
date: 2026-11-15
decision-makers: asamarts
---

# 0039. The YAML oracle as a dev-dependency of `alint-core`

## Status

Superseded by [ADR-0041](0041-a-yaml-parser-facade-with-test-only-reference-parsers.md)
(2026-11-15), which makes `yaml_serde` an optional dependency of `alint-core` for a parser
facade. Accepted 2026-11-15; it amended [ADR-0035](0035-keep-the-early-ayaml-cutover.md)
decision 2 on where `yaml_serde` may be a dependency.

## Context

- **ADR-0035 decision 2** makes `yaml_serde`, the oracle of the differential harness (#482), "a
  dependency of the harness only, never of a published crate".
- **The harness is a test module of `alint-core`,** a published crate, and `yaml_serde` is a
  dev-dependency of `alint-core` (#521). The harness needs `alint-core`'s internals: the limits
  and private entry points of `alint_core::yaml`, the typed decode into `Config` and `RuleSpec`,
  and the seed of the structured-format rules' JSON tree. A separate, unpublished crate could
  reach them only if they were made public.
- **A dev-dependency stays out of users' builds.** Cargo builds it only for the crate's own
  tests, examples and benchmarks, never for a crate that depends on `alint-core`.
  `cargo tree --workspace -e normal,build -i yaml_serde` reports "nothing to print".
- **The documents disagree with the decision as written:** `docs/design/yaml-parser.md` and the
  v0.23 plan say "a dev-dependency of `alint-core` and of no published crate".

## Decision

1. `yaml_serde` may be a dev-dependency of `alint-core`, and of no other crate, for the harness.
2. It is no runtime dependency of any published crate: never a normal or build dependency. The
   workspace pins it exactly (`=0.10.7`), since the oracle is the parser v0.22 used.

## Consequences

- The documents say "a dev-dependency of `alint-core` only, and no runtime dependency of any
  published crate".
- Moving the harness to its own crate would need a public API for the internals it reads; that
  is not planned.
