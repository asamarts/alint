---
status: accepted
date: 2026-10-14
decision-makers: asamarts
---

# 0019. General-linter program and plugin architecture

## Status

Accepted (2026-10-14).

## Context

[`general-linter.md`](../design/general-linter.md) proposes making alint a general linter for
finite, inspectable project state: explicit internal stages (selection, decoding, extraction,
assertion, reporting, remediation), typed extraction with provenance, findings with related
locations, an internal relation layer, and later a bounded `constraint` surface. The companion
[`rule-coverage-gaps.md`](../design/rule-coverage-gaps.md) lists the detection gaps the program
should unlock.

An adversarial review on 2026-10-14 checked both documents against the v0.18.2 code and the
project's recorded decisions. It found fail-open and leak sites in the shipped engine (rule
errors that become ordinary, level-dependent, baselinable violations; notes dropped by six of
eight output formats; extracted values, possibly secrets, echoed into reports and baseline files;
`scope_filter` silently ignored by several kinds), conflicts with recorded decisions
(constitution invariants 1 and 2, ADRs 0002, 0003, 0004, 0006, 0008, 0010, 0017), three
inconsistent plugin plans, and no milestones. The maintainer then took fourteen program-level
decisions, one at a time.

## Decision

We will run the general-linter program as staged in `general-linter.md` section 11, with these
decisions (the full outcomes are in its section 13.1):

1. **Program.** Phases 0 and 1a ship in v0.19, the authoring loop in v0.20, the relation layer
   (go/no-go) in v0.21, then selectors, report-only constraints, and `version: 2`. The program
   stops early if the Phase 2 pilot or the Phase 3 coverage measure fails.
2. **Analysis failure.** A rule that cannot evaluate repository content emits a finding that
   always counts as error, whatever the rule's level, and is never baselinable. Exit codes stay
   0, 1, 2, and 3; there is no new exit code.
3. **Redaction.** Values from dotenv and properties files are redacted by default in every
   output; other rules can opt in with `sensitive:`; baseline files never store raw values.
4. **JSON report.** The v1 report schema is relaxed to accept additive fields one release before
   new fields appear; a v2 report is reserved for removals and renames.
5. **Baseline identity.** Existing kinds' fingerprints are frozen behind a golden gate; related
   locations never enter a fingerprint; the baseline schema stays at version 1.
6. **Policy tests.** `alint test` uses the existing testkit scenario format, promoted to a
   versioned public schema, with its runner in a published crate.
7. **IR proof.** The relation layer's first user is the new `key_parity` kind; existing kinds are
   ported only under differential shadow evaluation with zero differences for one release.
8. **Relational gaps** ship as named v1 kinds implemented on the relation layer, never as
   standalone evaluators built before it.
9. **DSL version.** `version: 2` ships before v1.0, and v1.0 freezes both versions, the
   `alint-core` API, and plugin contract 1.0 together.
10. **Formats.** The core ships a reasonable default set of data and document format adapters
    and no programming-language syntax; every format, built-in included, sits behind one public
    adapter contract that others can extend.
11. **Time.** Time-dependent rules take a global `--as-of` input, defaulting to the HEAD commit
    time, never the wall clock.
12. **Policy-source files** such as `.editorconfig` get their own ADR, distinct from ADR-0010's
    intent-inferring manifests.
13. **Performance.** The program is held to cumulative budgets against the frozen v0.18.2 tag,
    enforced for relation-layer pull requests.
14. **Plugins** (`general-linter.md` section 9.4):
    - one versioned contract with adapter, analyzer, and fixer roles, served by three hosts
      (native Rust crates compiled in, out-of-process plugins in any language, sandboxed WASM
      components), all feeding the internal trait set that built-in kinds and formats also use;
      fixers only propose typed edits, which the host applies;
    - out-of-process plugins speak length-prefixed protobuf over stdio, with a negotiated
      proto3-JSON encoding, a version and feature handshake, persistent processes, multiplexed
      batched requests, cancellation, and timeouts; not gRPC;
    - native Rust plugins are compiled into a custom binary by `alint build`, never loaded as
      dynamic libraries;
    - WIT is the single source of truth for the contract, compiled to plain Rust types and traits
      and to a protobuf schema (with an append-only field-number lockfile) by a new, separate
      open-source WIT-first contract compiler, with alint as its first consumer;
    - the contract is designed with the Phase 2 relation layer and published as `0.x`; the
      native, process, and WASM hosts follow in that order;
    - process plugins are spawn-gated (top-level config or `trusted_extends:` only) and
      hash-pinned; SRI-pinned WASM plugins may arrive through `extends:` without capabilities;
      an `alint.lock` records hashes for every platform.

## Consequences

- v0.19 changes observable pass/fail behaviour where v1 produced a false pass (rule errors on
  warning-level rules); the changelog lists each such change.
- v1.0 moves later, after Phase 3, because it now freezes `version: 2` and plugin contract 1.0.
- The plugin platform is large (about 33 to 53 engineer-weeks, including the contract compiler;
  a first real plugin after about 12 to 18), and the contract compiler is a second project to
  maintain, exposed to `wit-parser` churn and to how the WIT annotations proposal
  (component-model issue 695) lands.
- WIT's frozen-type rule means every contract change is a new version, and the host carries one
  adapter per supported contract version.
- Native and process plugins are fully trusted native code; only integrity, not sandboxing,
  protects users from them.
- Several existing records are superseded or amended by later ADRs, listed in
  `general-linter.md` section 13.3: constitution invariants 1 and 2, ADRs 0002, 0003, 0004, 0006,
  0008, 0010, and 0017, and the "native Rust plugins are out of scope" statement in
  `ARCHITECTURE.md`, which this ADR replaces.

## Considered Options

Each decision's alternatives and trade-offs are recorded in `general-linter.md` sections 9.4 and
13.1 and in PR #324. Notable rejected options: a new exit code for incomplete analysis; porting
existing kinds to the relation layer before a pilot; standalone evaluators for the relational
gaps; gRPC, JSON-RPC, or MessagePack as the plugin wire protocol; dynamic loading of Rust
plugins; Rust types or protobuf as the contract's source of truth; and plugins strictly after
the constraint phase.

## More Information

- [`general-linter.md`](../design/general-linter.md): the program, section 13 for decisions and
  open questions, section 15 for the review log.
- [`rule-coverage-gaps.md`](../design/rule-coverage-gaps.md): the detection gaps.
- [ROADMAP v0.19](../design/ROADMAP.md): the first scheduled phase.
