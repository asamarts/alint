---
status: proposed
date: 2026-11-24
decision-makers: asamarts
---

# 0050. v0.24 Phase 3 completion design decisions (`version: 2` operators, `#592` to `#598`, B1, C4, strict JSON, and bundled `version: 2`)

## Status

Proposed (2026-11-24). Companion design document:
[`v0.24/phase-3-completion.md`](../design/v0.24/phase-3-completion.md). Serves as the owner design
gate required by [ADR-0048](0048-v024-owner-decisions.md) decision 3a and
[`v0.24/README.md`](../design/v0.24/README.md) before Phase 5 implementation PRs begin. Refines
[ADR-0034](0034-v2-version-boundary-and-phase-3-design.md) and
[ADR-0037](0037-phase-3-design-refinements.md) for the v0.24 experimental `version: 2` surface and
operationalizes ADR-0048 decisions 3a, 3b, and 3c.

## Context

- [ADR-0048](0048-v024-owner-decisions.md) decided to complete Phase 3 of
  [`general-linter.md`](../design/general-linter.md) in v0.24 while keeping `version: 2`
  experimental for one soak release (decision 3a), folding `cardinality` into unified `count` and
  `group` into orthogonal multi-field `group_by` after verifying the two exact stratum units
  (decision 3b), and opening bundled (`alint://bundled/...`) `version: 2` sources while keeping
  remote and nested `version: 2` sources refused until v0.25 (decision 3c).
- Phases 0 through 4 of v0.24 are complete (`ayaml 0.2.3` spanned events/trees landed and `saphyr`
  left default release builds in Phase 2; `FindingsPerRule` 10k cap landed in Phase 1; owned
  parallel walker landed in Phase 3; `lsp-server` and `hcl-rs 0.20` landed and passed adversarial
  audit in Phase 4).
- Six open Phase 3 issues remain (#592, #593, #594, #595, #598, #570) alongside the remaining
  Phase 3 assertion families and high-ROI stratum gaps uncovered by v0.23 Phase 8's binary
  verification (`docs/design/v0.23/corpus-stratum-log.md`):
  - **Stratum verification of ADR-0048 decision 3b:** `forem-01` (originally tagged `cardinality`)
    already passed as **A** in v0.23 under grouped `set`; `backstage-25` (originally tagged `group`)
    requires **per-group cross-relation `set` comparison** (`group_by` on both input and target
    relations, plus separate array sort-order verification), while multi-field `group_by: [...]`
    collapses `ghost-02` (5 `key_parity` rules -> 1 v2 rule) and serves per-manifest grouped `set`.
  - **Duplicate keys (Gap B1):** `entries: { format: yaml, leaves: true }` missed duplicate
    non-leaf mapping keys with disjoint children (`mattermost-56`) and duplicate keys inside YAML
    sequences (`zulip-28`).
  - **Version comparison (Gap C4):** 21 stratum units (including 13 B-adapter `toolchain-pin`
    units) require offline semver range satisfaction and version ordering across npm/node-semver,
    Cargo, and Python `requires-python` spellings.
  - **Strict JSON:** JSONC tolerance keeps 10 locale catalog units in bucket C across 8 repos.
  - **Bundled `version: 2` sources:** `SourceId` and `PlanSource` currently key v2 sources only by
    on-disk `PathBuf`, which cannot represent compiled-in `alint://bundled/...` rulesets.

## Decision

Pending owner sign-off, we will implement the Phase 5 design specified in
[`v0.24/phase-3-completion.md`](../design/v0.24/phase-3-completion.md) with the following 11
decisions (D1 to D11):

1. **D1 -- `reference` assertion (single-key and composite-key):** Add `assert.reference` accepting
   `field:` or composite `fields: [...]` against `to: { relation, field | fields }` with matching
   arity, typed key-tuple equality, optional `skip_missing: true` (default `false`), path-free
   `baseline_key`, and bounds under `DistinctValues`, `BytesPerRun`, and `FindingsPerRule`.
2. **D2 -- Unified `count` assertion (ADR-0048 decision 3b):** Add `assert.count` with `min`,
   `max`, `exact` (mutually exclusive with `min`/`max`), optional `group_by`, optional `groups:`,
   and optional `distinct: true` + `field:`. When `max: 0` (emptiness), emit one finding per
   offending row anchored at that row's span (bounded by `FindingsPerRule`) so emptiness checks
   (`element-web-06`, `babel-12`, `ghost-63`) point directly at offending sites.
3. **D3 -- Orthogonal multi-field `group_by`, per-group cross-relation `set`, inline literal `set`
   targets, and anti-join deferral:**
   - Accept single string or non-empty list `group_by: [f1, f2, ...]` across `equal`, `set`, and
     `count` (with matching arity for `groups:` and `equal_to: { group: [...] }`).
   - Add optional `group_by:` on `set` targets `{ relation, field, group_by }` for per-group
     cross-relation comparison (`backstage-25` set half, `fluentui-24`, `trpc-07`).
   - Add inline literal `set` target `{ values: [...] }` (`ghost-63`).
   - Defer `join: { kind: anti | left }` out of v0.24 because D1 + D2 + D3 + `where: { not_in }`
     cover all stratum anti-join units without expanding the join planner.
4. **D4 -- Unified `compare` assertion (negated equality and ordering):** Add `assert.compare` with
   `op: eq | ne | lt | le | gt | ge`, target `{ field } | { value } | { row }`, and
   `order: exact | lexical | numeric | semver` (default `exact` for `eq`/`ne`). Covers both
   negated equality (`element-web-05`, `thunderbird-android-55`) and version/numeric ordering in
   one assertion family while keeping existing `equal:` unchanged.
5. **D5 -- Gap C4 offline semver range satisfaction (`satisfies`):** Add `assert.satisfies`
   (`version` / `version_literal`, `range` / `range_literal`, `dialect: node | cargo | pep440-basic`,
   `include_prerelease: false`) backed by an owned, zero-dependency, pure-Rust `semver_eval.rs`
   module supporting partial versions, `||`, hyphen/`x`-ranges, and bounded complexity (`1_024`
   bytes, `64` clauses, `64` comparators), failing loud on unparseable versions/ranges.
6. **D6 -- Gap B1 general duplicate-key detection (`no_duplicate_keys`, v1 rule kind):** Register
   one new per-file v1 rule kind `no_duplicate_keys` (`DispatchClass::PerFile`, headline canonical
   kinds `104 -> 105`) covering YAML (streaming `ayaml 0.2.3` events across all mapping depths and
   sequence elements, honoring `<<: *base` merges without false positives), JSON (`jsonc-parser`
   CST), dotenv (`adotenv`), `.properties`, and INI (TOML/HCL fail at parse time; XML refused at
   build time). Key-only messages preserve dotenv/properties value redaction.
7. **D7 -- Opt-in strict-JSON parse mode (`strict: true`):** Add optional `strict: true` (default
   `false`) across v1 JSON rules/extracts and v2 JSON `entries:`/`read:`/`records:`, disabling
   `strip_jsonc` fallback and rejecting empty/whitespace-only files as `resource_parse`, with
   strictness carried in `StructuredCache` and `DecodeCache` keys so tolerant and strict rules
   never collide.
8. **D8 -- Issue #592 array-of-objects `records:` reader, parameterized `transform:`, and
   mixed-reader `union:`:**
   - Add a dedicated `records:` reader (`format: json | yaml | toml`, `at: <JSONPath>`, optional
     `strict:`) whose `fields:` read `{ field: "<subpath>", optional?: bool, sensitive?: bool }`
     from each matched object with exact source spans.
   - Add non-regex linear-time parameterized transforms `{ split: { on, part, required? } }`,
     `{ strip_prefix }`, `{ strip_suffix }`, `{ prefix }`, and `{ suffix }`.
   - Allow per-part `fields:` in `union:` (all parts binding the same output field names and
     sensitivity flags), unblocking mixed-reader unions (`mattermost-27`).
9. **D9 -- Issues #593, #594, #595, #598, and #570:**
   - **#593:** allow map-form `bound: { bound: "<query>", optional: true, on_missing?: skip | null }`
     (default `optional: false` unchanged).
   - **#594:** support `{{value}}` in custom `message:` on `set` and `reference` findings,
     unconditionally redacted for `sensitive: true` or env-derived fields and escaped via
     `alint_core::invisible`.
   - **#595:** anchor missing-value `set` findings at the group's matching target file under
     path-token substitution when unambiguous, falling back to the group's first file (path-free
     `baseline_key` keeps baselines stable).
   - **#598:** record empty root keys (`fr:`, `fr: {}`, `fr: []`) as zero-row groups anchored at
     the stub file when `group_by` reads `entry.root` under `entries:` + `leaves: true` + `strip-root`.
   - **#570:** add `ci/scripts/check-v2-schema-id.sh` (plus shell regression test) wired into
     `preflight.sh` and `release.yml` verifying `gen_schema_v2.rs` `RELEASE`, `schemas/v2/config.json`
     `$id`, and docs modelines advance whenever `schemas/v2/config.json` changes from the prior tag.
10. **D10 -- DEC-8 bundled `version: 2` rulesets (ADR-0048 decision 3c):** Replace `PathBuf`-only
    v2 source identity with `SourceLocator::{Local(PathBuf), Bundled(String)}`, allow
    `alint://bundled/...` as a `version: 2` source class while keeping remote and nested `version: 2`
    sources refused until v0.25, strictly enforce ADR-0022's `source <= consumer` direction rule,
    and ship first-party opt-in `js/workspace-deps@v2` with `alint test` fixtures (leaving all `@v1`
    bundled rulesets untouched).
11. **D11 -- Schema `$id`, tooling, and experimental soak:** Advance `gen_schema_v2.rs` `RELEASE`
    to `"v0.24.0"`, keep `constraint` as one canonical v2-only experimental kind, update the v2
    experimental warning to `"v0.25 at the earliest"`, and update `validate-config`, `list`,
    `explain`, `export-agents-md`, `docs/rules.md`, and `facts.json` additively.

## Consequences

- **High corpus stratum lift with a compact surface:** D1 to D10 target the exact units blocking
  the 20-repo stratum (`reference` 11 units, `semver` 21 units, `#592` 12 units, `strict-json` 10
  C->A units, `no_duplicate_keys` `mattermost-56`/`zulip-28`, `count`/`compare`/inline `set`
  promoting `element-web-06`, `babel-12`, `ghost-63`, `element-web-05`, `thunderbird-android-55`)
  while adding only one v1 rule kind (`no_duplicate_keys`) and keeping `constraint` as one v2 kind.
- **Zero new third-party dependencies:** `semver_eval.rs` is ~250 to 350 LOC of pure Rust, and
  `no_duplicate_keys` reuses `ayaml 0.2.3`, `jsonc-parser`, `adotenv`, and `properties.rs` already
  in `alint-core`.
- **Clean soak period before v2 freezes:** `version: 2` remains experimental through v0.24 so the
  new operators, `records:` reader, and bundled `version: 2` rulesets get real-world soak time
  before v0.25 freezes `version: 2` and opens remote/nested v2 sources.
- **Honest limitations retained deliberately:**
  - `backstage-25`'s array sort-order check stays separate from per-group `set` equality.
  - Anti/left joins, regex transforms, and XML value rows stay out of scope for v0.24.
  - `no_duplicate_keys` adds a small maintenance obligation for the `ayaml` event walk and
    `jsonc-parser` CST walk alongside `Format::parse`.
- **Owner gate:** No Phase 5 implementation PR starts until the owner accepts (or amends) D1 to
  D11 here and flips `status` to `accepted`.

## Considered Options

- **D1 (`reference`):** single-field `reference` only vs single + composite `fields: [...]`.
  Composite keys avoid requiring string-concatenation hacks for 5/11 stratum `reference` units
  (`babel-17`, `trpc-10`, `backstage-26`, `rushstack-02`, `yarnpkg-berry-17`) and also cover
  per-manifest `(manifest, dep)` checks (`fluentui-24`, `trpc-07`).
- **D4 (`compare`):** adding `negated: true` to `equal:` plus a separate `order:` family vs one
  unified `compare:` family (`eq | ne | lt | le | gt | ge` with `order:`). Unified `compare:`
  keeps the assertion surface smaller and avoids awkward interactions between `negated: true` and
  group-wide `equal: { group_by: ... }`.
- **D5 (`semver_eval.rs`):** depending on crates.io `semver` / `node-semver` vs owning ~300 LOC.
  `semver` only parses Cargo dialect (missing partial versions, `||`, hyphen, `x`-ranges, and
  PEP 440); third-party `node-semver` crates add supply-chain surface without PEP 440 or Cargo
  dialects.
- **D6 (`no_duplicate_keys`):** v2-only document-wide key stream in `entries:` vs dedicated per-file
  v1 rule kind. A per-file v1 kind runs on the LSP `didChange` hot path, works in `version: 1`
  configs without opting into experimental `version: 2`, and avoids allocating relation tables for
  every key in a repo.
- **D8 (`records:`):** overloading `entries:` with an `array_objects: true` mode vs a dedicated
  `records:` reader. A dedicated reader keeps `entries:`'s key/value fields and `leaves:` semantics
  unambiguous.
- **D10 (bundled `version: 2`):** keeping bundled rulesets `version: 1`-only until v0.25 vs
  opening `alint://bundled/...` now. Opening bundled v2 now fulfills ADR-0048 decision 3c and lets
  users adopt DEC-8 v2 policies without copy-pasting 20-line relation blocks, with zero remote
  trust expansion.

## More Information

- [`v0.24/phase-3-completion.md`](../design/v0.24/phase-3-completion.md): full specification,
  syntax sketches, false-positive analysis, cost bounds, and 4-batch delivery plan.
- [`v0.24/README.md`](../design/v0.24/README.md): v0.24 phased plan.
- [ADR-0034](0034-v2-version-boundary-and-phase-3-design.md),
  [ADR-0037](0037-phase-3-design-refinements.md),
  [ADR-0048](0048-v024-owner-decisions.md).
- Issues #592, #593, #594, #595, #598, #570.
