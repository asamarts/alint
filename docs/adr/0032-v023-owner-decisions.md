---
status: accepted
date: 2026-11-12
decision-makers: asamarts
---

# 0032. v0.23 owner decisions: a first Phase 3 slice, the YAML groundwork, one API break, and plugins after

## Status

Accepted (2026-11-12). Scopes v0.23, the first release of Phase 3 of
[`general-linter.md`](../design/general-linter.md) (section 11.6), which v0.22.0 unblocked by meeting
the three gates of [ADR-0025](0025-v021-pre-release-decisions.md) decision 3. It moves the sealing
of the remaining public `alint-core` structs, which [ADR-0030](0030-v022-owner-decisions.md)
decision 2 left as a pre-v1.0 roadmap item, into v0.23, and moves the plugin hosts' schedule in
section 11.1 one release later. It does not change the parser criteria of
[ADR-0031](0031-v0221-owner-decisions.md) or the `yaml-parser.md` record.
[ADR-0033](0033-v023-slice-after-the-corpus-stratum.md) (2026-11-13) amends decision 1's
assertion families and relations, and the consequence that v1 configurations keep their findings;
the rest stands.

## Context

- **Phase 3 is large.** Section 11.1 sizes it L: named selectors and relations, a report-only
  `constraint` kind with seven assertion families, three conformance policies, specialized kinds
  and templates for common cases (DEC-8), and a `version: 2` surface across the schema, the
  commands and the generated documentation (section 7.8). Section 12 also asks for an i18n and a
  JavaScript-monorepo stratum in the corpus before Phase 3 is scheduled, because the v0.12 backlog
  has no locale, key-parity or dependency-version findings; that stratum does not exist yet.
- **The YAML parser track.** ayaml 0.1.1 fails the cutover criteria (#481). Two of the four
  alint-side issues do not depend on a new ayaml release: keeping parser types out of the public
  API (#484) and differential parser fuzzing (#482). Re-deriving the `yaml_depth` guard (#483)
  and the cutover itself (#481) do. ayaml 0.1.2 is not released.
- **Public types.** `RuleSpec.extra`, `NestedRuleSpec.extra`, `RuleEntry::extra()`,
  `config_redaction`'s masking functions, `template::render_*`, `Error::Yaml` and three
  `alint-testkit` items name `yaml_serde` types, so any parser change breaks library users (#484).
  `Violation`, `Context`, `RuleSpec` and `WalkOptions` are open struct literals, and Phase 3 is
  likely to add fields to `Violation` (relational evidence, section 8.1) and to `RuleSpec`
  (selector references).
- **Plugin hosts.** Section 11.1 put native Rust plugins and `alint build` at about v0.22 and the
  process host and Rust SDK at about v0.23. None has started; plugin contract 0.1 stays a draft
  until a release loads plugins ([ADR-0027](0027-plugin-contract-0-1-stays-a-draft.md)).

## Decision

1. **v0.23 carries Phase 3's design and a first slice.**
   - The corpus gains the i18n and JavaScript-monorepo stratum, scored with section 12.3's
     buckets, before the design commits to operators.
   - The Phase 3 design document and the version-boundary ADR (DEC-9) answer questions Q1, Q2,
     Q6, Q12 and Q15 and section 7.8. They need owner approval before implementation starts.
   - Then named selectors and relations, the report-only `constraint` kind with the three
     assertion families the conformance policies need (equality, unique/key and cardinality), and
     the three conformance policies as `version: 2` configurations with `alint test` cases.
   - `version: 2` ships as an experimental, opt-in surface with a draft `schemas/v2/config.json`.
     It may change before it is declared stable, and `init` and `suggest` keep emitting v1.
   - The remaining assertion families (reference, set, group, bounded count), the specialized
     kinds and templates, and a stable v2 follow in v0.24 or later.
2. **The YAML groundwork lands regardless; the cutover lands only if ayaml is ready.** #484 and
   #482 are in v0.23. #483 and #481 join only if ayaml 0.1.2 meets the criteria before v0.23's
   feature freeze; otherwise they move to the next minor release. Because #484 removes the parser
   from the public API first, a later cutover is internal.
3. **One API break: alint-owned YAML types, and every remaining struct sealed.**
   - `alint_core::yaml::{Value, Mapping, Error}` are opaque alint-owned types. They expose what
     rules, templates and the loader use, plus typed deserialization, and no public signature of
     `alint-core`, `alint-dsl` or `alint-testkit` names the parser crate. A gate asserts that.
   - `Violation`, `Context`, `RuleSpec`, `NestedRuleSpec` and `WalkOptions` become
     `#[non_exhaustive]`, with constructors and setters as ADR-0030 decision 2 gave `FixContext`
     and `Config`. This happens before Phase 3 adds fields, so those fields are additive.
   - The v1.0 roadmap item for sealing these structs is done by v0.23.
4. **No plugin hosts in v0.23.** v0.23 keeps the witforge consumer current. The host track starts
   in v0.24 with native Rust plugins and `alint build`, on the sealed API and Phase 3's relation
   and constraint model. Section 11.1's Phase 6 row moves one release later.
5. **Also in v0.23:** the v0.21 form of a hidden-path finding's fingerprint is no longer matched
   (ADR-0030 decision 1), and the CHANGELOG says to re-run `alint baseline` before upgrading.

## Consequences

- Demand for the flagship Phase 3 policies is measured on the new stratum before operators are
  designed, and the stop rule of section 11.1 can use that score.
- Users who try `version: 2` in v0.23 may need to change their configuration before v2 is
  stable. v1 configurations keep their findings, fingerprints and exit codes. The e2e suite and
  the corpus replay show that.
- Library users migrate once, in v0.23.0: struct literals become constructor calls, and
  `yaml_serde` types become `alint_core::yaml` types. The ayaml cutover then changes no public
  type, whenever it lands.
- v0.23 is a long release with one owner gate (the design document) partway through. If the
  slice cannot be ready, the plan's status log records it, and the release can ship decisions 2,
  3 and 5 with the stratum and the design.
- Plugin loading moves to v0.24 at the earliest, so plugin contract 0.1 stays a draft through
  v0.23.

## Considered Options

- **Phase 3 size:** design and internal groundwork only, with no public `version: 2` until v0.24
  (smaller, but no user-facing slice to measure the design against); all of Phase 3 in one release
  (longest cycle, and nothing ships until all of it is done).
- **YAML:** all four issues or none, depending on ayaml 0.1.2 (keeps the API change tied to
  ayaml's schedule); the whole YAML track deferred to its own minor release.
- **Public types:** re-exporting the parser's types under `alint_core::yaml` with a stability note
  (cheap, but every parser change still breaks library users); owned types with only `RuleSpec`
  and `NestedRuleSpec` sealed (a second break when Phase 3 extends `Violation`).
- **Plugins:** an ADR for the plugin and `alint build` manifests (question Q17) in v0.23 with the
  hosts in v0.24; native Rust plugins and `alint build` in v0.23 alongside Phase 3 (two tracks
  reshaping the same relation and finding types at once).

## More Information

- [`v0.23/README.md`](../design/v0.23/README.md): the v0.23 plan these decisions scope.
- [`general-linter.md`](../design/general-linter.md): sections 7.8, 11.1, 11.6, 12 and 13.2.
- Issues #481, #482, #483 and #484; [`yaml-parser.md`](../design/yaml-parser.md).
- [ADR-0030](0030-v022-owner-decisions.md): the hidden-path fingerprint and the first sealed
  structs.
