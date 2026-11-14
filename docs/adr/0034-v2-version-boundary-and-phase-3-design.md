---
status: accepted
date: 2026-11-14
decision-makers: asamarts
---

# 0034. The `version: 2` boundary and the Phase 3 design decisions

## Status

Accepted (2026-11-14). Records DEC-9's version boundary
([ADR-0019](0019-general-linter-program-and-plugin-architecture.md)) for v0.23, decides questions
Q1, Q2, Q6, Q12 and Q15 of general-linter section 13.2, and the parts of
[ADR-0033](0033-v023-slice-after-the-corpus-stratum.md) decision 2 that it left to the design.
Amends constitution invariant 2 and [ADR-0003](0003-rule-engine-dispatch-and-determinism.md)
(decision 3 here). [ADR-0008](0008-git-tracked-only-is-a-kind-option.md) and
[ADR-0010](0010-manifest-derived-rule-scope.md) stand.

## Context

[ADR-0032](0032-v023-owner-decisions.md) made v0.23 carry the Phase 3 design and a first slice, with
`version: 2` experimental and opt-in, and an owner gate between the design and its implementation.
ADR-0033 fixed the slice: named selectors and relations, value-bearing entry rows for JSON, YAML
and TOML, and a report-only `constraint` kind with the equality, unique/key and set families. Its
users are three conformance policies: locale key parity, dependency version consistency and
toolchain-pin consistency.

The design document is
[`v0.23/selectors-and-constraints.md`](../design/v0.23/selectors-and-constraints.md). It records
what binds the design (section 2). Four of those facts shaped these decisions:

- **The loader reads `version:` only after a strict v1 decode.** `version: 2` with a
  `selectors:` key fails as an unknown field, not as an unsupported version, and `version:` may
  come from `{{env.X}}`.
- **The code already departs from constitution invariant 2.** Five full-index kinds (`key_parity`
  among them) declare a path scope that decides under `--changed` whether they run, then judge the
  whole index. The invariant still says a full-index rule declares no path scope.
- **Trust markers exist on rules and templates only.** Named top-level objects are a new
  indirection that they do not cover (section 7.1 rule 9 of the program).
- **Most cross-relation needs in the stratum are membership.** Most of the stratum's `reference`
  units ask only whether a value exists in another relation. Four units compare fields across
  matched rows (fluentui-18, trpc-06, backstage-35 and nextcloud-b02), and all four join to a side
  that is unique on the key.

The owner took each decision below one at a time, with the options, their tradeoffs and a
recommendation, on 2026-11-13 and 2026-11-14.

## Decision

1. **The version boundary (DEC-9).**
   - The loader reads the top-level `version:` before any typed decode and dispatches on it. A v2
     source's `version:` is the literal integer `2`; a v1 source keeps today's behavior.
   - In v0.23, v2 is a superset of v1. It accepts every v1 key and kind with v1's meaning, plus
     `selectors:`, `relations:`, `kind: constraint` and an `extends:` alias (`as:`). Each of those
     is a configuration error in a v1 source.
   - v2 is experimental. Loading it warns, `validate-config` reports it, and nothing about it is
     stable before a release declares it so (v0.24 at the earliest); the CHANGELOG lists each
     change until then.
   - ADR-0022's direction rule holds at every edge, and v1.0 follows the release that declares v2
     stable (DEC-9).
2. **Selectors and relations are top-level and inline (Q1).** A top-level map names reusable
   ones; an inline one may stand wherever one is referenced. A named selector is ADR-0010's
   deferred `path_sets:` block, under ADR-0010's boundary: it decides which resources a relation
   reads, never a verdict. In v0.23 only relations reference selectors; a v1 kind's `paths:` keeps
   its v1 form. No selector predicate is added, so ADR-0008 stands.
3. **`--changed` (Q2).**
   - A `constraint` rule runs when a changed path matches the selectors of any relation its plan
     reads. It then judges the full snapshot and reports every finding, including those located in
     unchanged files.
   - Constitution invariant 2 and ADR-0003 are amended to: a full-index rule may declare a path
     scope that decides whether it runs, never what it sees. This also records what the five
     full-index kinds that declare a scope already do. The invariant's test covers every
     full-index kind.
4. **Names across sources: qualified references (Q12).**
   - Each source has its own namespace, and a bare name resolves in the referencing source.
   - A v2 source may alias an `extends:` entry (`{ url: ./policies/deps.yml, as: deps }`) and
     reference that source's own objects as `deps.ranges`. There is no re-export and no chained
     name.
   - A name or an alias declared twice in one source is a load error. An extended source's rules
     keep resolving to its own objects.
   - Each object passes its declaring source's trust gates. A rule's capabilities include those of
     everything it references, and the gates judge it after resolution.
   - **In v0.23, v2 sources are local files:** the top-level config, local `extends:` and
     `.alint.d` fragments. Bundled and remote sources must be `version: 1`.
5. **Entry rows (ADR-0033 decision 2).**
   - A relation emits rows of named fields. Each field keeps its resource, pointer and span, so a
     finding sits where its value is written.
   - Values are scalars: null, bool, number (lexeme kept) and string. An object or array value,
     or a TOML datetime, is an analysis failure naming the path.
   - Value-bearing modes have their own adapter ids, and TOML values come from the TOML rows
     adapter.
   - dotenv and properties values stay redacted, so those formats emit no value rows, and XML
     stays key-only in v0.23.
6. **Joins (Q6).**
   - Rows combine by grouping, by equality semi-joins (membership filters, and `set` against
     another relation), and by an **equality join**: a `join:` relation over two relations, inner
     only, with explicit `left.`/`right.` output fields.
   - One side of a join must be unique on the join key. The engine checks it, and a repeated key
     is an analysis failure. A join therefore emits at most its other side's rows.
   - The reserved `JoinInputs` and `JoinOutputs` limits are enforced: on lookup tables, and on
     join output. Grouping stays under the rows-per-run budget.
   - A limit hit is an analysis failure. There is no cap on findings.
7. **`when` and row filters (Q15).** `when` is unchanged in v2. Relations filter rows with a
   structural `where:` (`in`, `not_in`, `matches`, `not_matches`, membership in another relation's
   field) and apply a fixed list of transforms (`trim`, `lower`, `semver-major`, `semver-minor`,
   `strip-root`, `cldr-plurals`). There is no expression language in v0.23.
8. **Findings.**
   - `unique` with `determines:` reports one finding per (key, distinct determined value), keyed
     `unique\0<key values>\0<determined values>`.
   - `equal` against a named source reports one finding per differing row.
   - `set` reports one finding per missing or extra value per group.
   - Related locations never enter a fingerprint.
9. **Schema, commands and documentation (section 7.8).**
   - `gen-schema` writes a draft `schemas/v2/config.json` whose `$id` names the release. The v1
     schema keeps its `$id` and gains new v1 kinds until v1.0.
   - `constraint` is one kind (the headline gains one), with one rule page and a section per
     family.
   - `validate-config`, `list`, `explain` and `export-agents-md` cover v2 additively.
   - `init` and `suggest` keep emitting v1.

## Consequences

- **v1 is unchanged.** Every new key is v2-only, and v1 keeps its `{{env.X}}` version and its
  `extends:` entry.
- **Phase 6 grows.** It adds the version dispatch, per-source namespaces with aliases, and the
  join beside the selectors, relations, value-bearing entry rows, schema and commands that the plan
  already listed. The join serves policies beyond the slice's three. If phase 6 cannot carry it
  before the feature freeze, the plan's status log records the move, as ADR-0032's consequences
  allow.
- **The trust review of named objects is local-only.** Remote v2 sources need their own review
  before any release accepts them.
- **Some documents change with this record:**
  - the v0.23 plan (scope item 2, the phase 3 row and the status log);
  - general-linter section 13.2 (Q1, Q2, Q6, Q12 and Q15 decided) and 13.3 (the invariant 2
    amendment recorded);
  - the constitution's invariant 2.

## Considered Options

The design document's section 15 lists each decision with its outcome; the options were:

- **Boundary:**
  - a superset of v1 (chosen);
  - a v2 file that holds only the new surface;
  - a superset that also needs a CLI flag.
- **Placement:**
  - top-level and inline, referenced by relations only (chosen);
  - also referenced from any kind's `paths:`;
  - top-level only.
- **`--changed`:**
  - the plan's selectors decide whether it runs (chosen);
  - report only findings located in changed files;
  - always run.
- **Names:**
  - per-source resolution without cross-source references (recommended);
  - qualified references through an alias (chosen);
  - flat by name, last write wins.
- **Which sources may be v2:**
  - local files only (chosen);
  - local and remote, with gated objects.
- **Rows:**
  - named fields with scalar values (chosen);
  - structured values;
  - raw (pointer, value) rows with fields derived in the rule.
- **Joins:**
  - grouping and semi-joins only (recommended);
  - an equality join as well (chosen).
- **Repeated join keys:**
  - one unique side (chosen);
  - many-to-many under the output cap.
- **Findings beyond a threshold:**
  - no cap (chosen);
  - a cap after baseline suppression;
  - summaries per group.
- **`when` and filters:**
  - structural filters (chosen);
  - row expressions;
  - structural filters plus typed `==` in v2.
- **Finding shape:**
  - per (key, value) (chosen);
  - per key, which would hide a new value behind a baselined key;
  - per row.
- **Tooling:**
  - a draft v2 schema and one kind (chosen);
  - one kind per family;
  - freezing the v1 schema now.

## More Information

- [`v0.23/selectors-and-constraints.md`](../design/v0.23/selectors-and-constraints.md): the design,
  with illustrative v2 configurations of the three policies.
- [`v0.23/README.md`](../design/v0.23/README.md): the v0.23 plan.
- [`general-linter.md`](../design/general-linter.md): sections 7.1 to 7.8, 12.4 and 13.
