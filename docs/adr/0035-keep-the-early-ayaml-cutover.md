---
status: accepted
date: 2026-11-14
decision-makers: asamarts
---

# 0035. Keep the early ayaml cutover and qualify it before v0.23.0

## Status

Accepted (2026-11-14). Amends [ADR-0031](0031-v0221-owner-decisions.md) decision 1 and
[ADR-0032](0032-v023-owner-decisions.md) decision 2 on when the ayaml cutover merges. The criteria
of [`v0.22.1/README.md`](../design/v0.22.1/README.md) section 2.2 (Y-C1 to Y-C7) are unchanged.
[ADR-0042](0042-measure-the-yaml-parser-by-memory-and-wall-time.md) (2026-11-16) amends Y-C4: the
deterministic gate passing is not enough, and v0.23.0 also waits for ayaml's peak heap within 1.2x
of libyaml's.

## Context

- **The plan.** ADR-0031 decision 1 has the cutover built and qualified on a branch, merging for
  the first minor release after ayaml 0.1.2 meets the criteria. ADR-0032 decision 2 has #483 and
  #481 join v0.23 only if ayaml 0.1.2 meets them before the feature freeze.
- **What happened.** #510 (2026-11-14) moved `main` from `yaml_serde` 0.10.7 to ayaml 0.1.2
  before the differential fuzzing harness (#482, Y-C6) and the re-derived depth guard (#483,
  Y-C7) existed. `yaml_serde` and `libyaml-rs` left the dependency graph. saphyr and
  saphyr-parser stay, for source spans and structured fixes.
- **An audit of #510 against v0.22.0** found:
  - **The same values** on every probe of scalar resolution, merge keys, duplicate keys, large
    integers and tags.
  - **A regression, fixed with this record.** ayaml caps events (1,000,000), collection entries,
    anchors and alias uses by default, and v0.22 had none of those caps. A valid 9 MB file that
    v0.22.0 reads failed with "events limit exceeded". Every ayaml decode now goes through
    `alint_core::yaml`, which lifts those four caps and keeps the 32 MiB input cap, the depth cap
    of 128 and the alias-expansion budget.
  - **Input that now parses:** a plain scalar containing U+0085, U+2028 or U+2029, which YAML 1.2
    reads as content. libyaml read them as line breaks and rejected the document.
  - **Input that is now rejected:** a block scalar whose header sits at the indentation of its
    parent `-` or `?` (`-\n|2\n  x`). libyaml accepted it.
  - **Messages:** several parse errors are worded differently, including the one for a stream of
    more than one document.
- **The criteria for 0.1.2.**
  - Met: Y-C1 (`pattern: null` is refused, as before) and Y-C2 (the compatibility entry points
    take limits).
  - Unmet: Y-C6 and Y-C7.
  - Not established: Y-C3, which needs error locations and rejected input compared, not only the
    values of the committed corpus.
  - Y-C4: #510's deterministic performance gate passed.
  - Y-C5: ayaml's `serde` and `indexmap` requirements are caret ranges now, and its release
    workflow attests. It is pre-1.0, and alint pins it exactly.

## Decision

1. **ayaml stays on `main`.** #510 is not reverted.
2. **v0.23.0 ships ayaml only once it is qualified.**
   - Phase 5's harness (#482) runs ayaml against `yaml_serde` as an oracle. `yaml_serde` is a
     dependency of the harness only, never of a published crate.
   - #483 re-derives the `yaml_depth` guard from ayaml's tokens, and its parity property passes
     with no relaxed assertion.
   - Every difference the harness finds is fixed, or entered in its allowlist and approved by
     the owner.
3. **If qualification is not done before the release, or finds a difference the owner does not
   approve,** the owner chooses between holding the release and reverting #510 before v0.23.0 is
   tagged.
4. **Every ayaml decode goes through `alint_core::yaml`,** which sets the limits. A dogfood rule
   fails on a direct ayaml decode anywhere else.
5. **The CHANGELOG lists the cutover's observable differences** from v0.22.

## Consequences

- Phase 9 of the v0.23 plan changes from "cut over if qualified" to "qualify the cutover": #483,
  and the harness's verdict over the allowlist. #481 closes with the release that ships the
  qualified parser.
- The release now depends on #482 and #483, or on a revert.
- The corpus replay of every pull request before the release compares against v0.22.1, so a
  parser difference in the corpus shows up early.
- The v0.23 plan, the roadmap's v0.23 section and the
  [`yaml-parser.md`](../design/yaml-parser.md) re-evaluation log record the change.

## Considered Options

- **Keep ayaml and qualify it before the release** (chosen): the dependency graph is pure Rust
  now, and every test run until the release exercises the new parser. The release waits on #482
  and #483.
- **Revert #510** and cut over again as phase 9, after #482 and #483 qualify ayaml: follows
  ADR-0031 and ADR-0032 as written, at the cost of reverting work that passes the full suite and
  applying it again.
- **Keep ayaml without a gate:** the least work, but v0.23.0 would ship a parser change that no
  differential test has qualified, against Y-C3, Y-C6 and Y-C7.

## More Information

- [`v0.23/README.md`](../design/v0.23/README.md): phases 5 and 9, the risks and the status log.
- [`v0.22.1/README.md`](../design/v0.22.1/README.md) section 2.2: the criteria.
- Issues #481, #482, #483 and #484; pull request #510.
