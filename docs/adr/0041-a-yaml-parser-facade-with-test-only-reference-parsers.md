---
status: accepted
date: 2026-11-15
decision-makers: asamarts
---

# 0041. A YAML parser facade with test-only reference parsers

## Status

Accepted (2026-11-15). Supersedes
[ADR-0039](0039-the-yaml-oracle-as-a-dev-dependency-of-alint-core.md). Amends
[ADR-0035](0035-keep-the-early-ayaml-cutover.md) decision 2 on where `yaml_serde` may be a
dependency, and [ADR-0036](0036-ayaml-issues-and-the-saphyr-exit.md) decision 2 on saphyr after
v0.24. The rest of both stands.

## Context

- **The qualification so far.** The differential harness (#482) compares ayaml with `yaml_serde`
  0.10.7, the parser v0.22 used, at four entry points: the value, the JSON tree of the
  structured-format rules, and the typed decodes into `Config` and `RuleSpec`. ADR-0039 keeps
  `yaml_serde` a dev-dependency of `alint-core` for that harness alone.
- **What the owner wants.** ayaml is new, and alint adopts it while it is still being developed.
  The parser under alint should be switchable, so that alint's own suites can show that ayaml
  reads files as older, proven parsers do. That covers the unit, end-to-end, policy-test and
  corpus-replay suites as well as the harness's generated inputs.
- **What makes it cheap.** Every decode already goes through `alint_core::yaml`
  (ADR-0035 decision 4), and alint's public API names no parser type
  ([ADR-0038](0038-owned-yaml-types-and-the-api-gate.md)). A switch below that module changes no
  API.
- **The reference parsers.**
  - **libyaml**, through `yaml_serde` 0.10.7, is the most proven. It reads YAML 1.1 in places,
    and about half the allowlist's entries are YAML 1.2 readings of ayaml's that libyaml does not
    share.
  - **saphyr** 0.3.3, already a dependency for source spans, reads YAML 1.2 and passes the YAML
    test suite. It has no serde support, so it can check the value a document holds, but not a
    typed decode.

## Decision

1. **`alint_core::yaml` sends every parse of text to a backend.**
   - ayaml is the default, and the only backend in a build without the feature below.
   - libyaml, through `yaml_serde` (`=0.10.7`), decodes values, typed values and the JSON tree
     of the structured-format rules.
   - saphyr (`=0.3.3`) reads value trees only, so it votes in compare mode and is no backend of
     its own.
2. **The reference backends are test-only.** They compile only with `alint-core`'s non-default
   `yaml-reference` feature, which makes `yaml_serde` an optional dependency. Release builds
   never enable it. A CI gate fails if a workspace crate depends on `yaml_serde` with default
   features, on any target, other than as a dev-dependency.
3. **`ALINT_YAML_PARSER` picks the backend at run time** in a build with the feature:
   - `ayaml`, the default;
   - `libyaml`, to run a suite on libyaml alone;
   - `compare`. It decodes with ayaml and returns ayaml's result. It also parses with libyaml
     and saphyr. A difference from libyaml is explained when saphyr reads the input as ayaml
     does, or when an entry of the harness's allowlist explains it. Otherwise the decode panics
     with the input and the three results.

   A build without the feature reads ayaml whatever the variable says.
4. **CI runs the workspace's tests in compare mode,** with the feature on.
5. **The data model stays ayaml's `Value`,** behind ADR-0038's owned types. A parser-independent
   data model is a later decision.

## Consequences

- **`yaml_serde` becomes an optional dependency of a published crate.** A user of `alint-core`
  from crates.io can turn the feature on; no alint release does.
- **saphyr's span uses still leave in v0.24** (ADR-0036). saphyr then stays only as an optional
  reference dependency behind the feature.
- **The allowlist stays.** The harness's report counts the differences saphyr's vote explains,
  apart from those an entry explains, so an entry the vote makes redundant can go on review. The
  first long runs found none: each entry also explains differences saphyr does not share, and
  stays the record of what changed from v0.22 (ADR-0035 decision 5).
- **What compare mode checks:**
  - a typed decode, by whether each parser succeeds, since typed values have no common form;
  - the value tree, through all three parsers.
- **Cost.** Compare mode roughly triples the cost of a parse, so it runs in CI only.
- **Another backend is a local change:** a variant of the backend enum, its conversion into the
  comparison tree, and its row in the vote. An ayaml build from its main branch is one candidate.

## Considered Options

- **Keep the harness as the only comparison** (ADR-0039), with a parser trait inside it. This is
  the cheapest, but alint's own suites never run on another parser, and the parser cannot be
  switched.
- **Ship the reference parsers in release binaries**, as an escape hatch for users. This ships
  libyaml's unsafe code in every binary, adds a second parser to support in bug reports, and
  needs the depth guard proven against libyaml as well.
- **Own the data model too.** alint's own `Value` and serde deserializer would make every parser,
  ayaml included, a backend. This is the most change: about 300 lines in alint-core and alint-dsl,
  and keeping source positions in typed-decode errors. It can follow this decision.
- **libyaml as the only reference.** This is simpler, but every YAML 1.2 difference stays
  hand-explained in the allowlist.

## More Information

- [`yaml-parser.md`](../design/yaml-parser.md), section "Parser facade": the design and its
  phases.
- [ADR-0040](0040-the-v023-advisory-and-the-allowlist-approval.md): the allowlist approval whose
  ayaml bump compare mode qualifies.
