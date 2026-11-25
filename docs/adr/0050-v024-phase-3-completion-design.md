---
status: accepted
date: 2026-11-24
decision-makers: asamarts
---

# 0050. v0.24 Phase 3 completion design decisions (`version: 2` operators, `#592` to `#598`, B1, C4, strict JSON, and bundled `version: 2`)

## Status

Accepted (2026-11-24). Companion design document:
[`v0.24/phase-3-completion.md`](../design/v0.24/phase-3-completion.md). Satisfies the owner design
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
    Note on ADR-0048 3b scope: `unique` already takes composite `key: [...]` and `reference` takes
    composite `fields: [...]`, so orthogonal `group_by: [...]` applies to `equal`, `set`, and
    `count`.
  - **Duplicate keys (Gap B1):** `entries: { format: yaml, leaves: true }` missed duplicate
    non-leaf mapping keys with disjoint children and duplicate keys inside workflow sequences
    (`mattermost-56`), as well as general OpenAPI/YAML mapping duplicate keys (`zulip-28`).
  - **Version comparison (Gap C4):** 21 stratum units (including 13 B-adapter `toolchain-pin`
    units) require offline semver range satisfaction and version ordering across npm/node-semver,
    Cargo, and Python `requires-python` spellings.
  - **Strict JSON:** JSONC tolerance keeps 10 locale catalog units in bucket C across 8 repos.
  - **Bundled `version: 2` sources:** `SourceId`, `PlanSource`, and `FIELD_ORIGINS_MARKER`
    currently key v2 sources only by on-disk `PathBuf` (`Config::config_files`), which cannot
    represent compiled-in `alint://bundled/...` rulesets without polluting on-disk self-edit paths.

## Decision

We accept and implement the Phase 5 design specified in
[`v0.24/phase-3-completion.md`](../design/v0.24/phase-3-completion.md) with the following 11
decisions (D1 to D11):

1. **D1 -- `reference` assertion (single-key and composite-key):** Add `assert.reference` accepting
   `field:` or composite `fields: [...]` against `to: { relation, field | fields }` with matching
   arity, typed key-tuple equality, optional `skip_missing: true` (default `false`), ADR-0037
   length-prefixed `values::Key` path-free `baseline_key` (`Key::new("reference")` + source key
   values via `Key::value`, omitting local target alias names), sensitivity propagation across
   compared columns, `TriggerScope` target registration, and bounds under `DistinctValues`,
   `BytesPerRun`, and `FindingsPerRule`.
2. **D2 -- Unified `count` assertion (ADR-0048 decision 3b):** Add `assert.count` with `min`,
   `max`, `exact` (mutually exclusive with `min`/`max`), optional `group_by` (single field or list),
   optional `groups:` (matching `group_by` arity; refused unless `min > 0` or `exact > 0`), and
   optional `distinct: true` + `field:` (`field:` refused when `distinct: false`). When `max: 0` or
   `exact: 0` (emptiness, grouped or ungrouped), emit one finding per offending row anchored at that
   row's span (bounded by `FindingsPerRule`, keyed via `Key::new("count_empty")` + `Key::row`). All
   other count violations emit one aggregate finding per failing group anchored at the group's first
   row or `Place::Nowhere` when empty (up to `MAX_RELATED_LOCATIONS = 20` sample spans, keyed via
   `Key::new("count")` + `Key::part(bound_kind)` + group key via `Key::value`).
3. **D3 -- Orthogonal multi-field `group_by`, per-group cross-relation `set`, inline literal `set`
   targets, and anti-join deferral:**
   - Accept single string or non-empty list `group_by: [f1, f2, ...]` across `equal`, `set`, and
     `count` (refining ADR-0048 3b, since `unique` uses `key: [...]` and `reference` uses composite
     `fields: [...]`), with matching arity for `groups:` and `equal_to: { group: [...] }`, rejecting
     `field` inside `group_by`, and keeping 1-element `group_by: [f]` `baseline_key` output
     identical to `group_by: f`.
   - Add optional `group_by:` on `set` targets `{ relation, field, group_by }` for per-group
     cross-relation comparison (`backstage-25` set half, `fluentui-24`, `trpc-07`), with optional
     `all_target_groups: true` (`equal_to` only, default `false`), bounding total distinct entries
     across all target groups under `DistinctValues`. Keep `plurals: cldr` restricted to `{ group: }`.
   - Add non-empty inline literal `set` target `{ values: [...] }` (strings and booleans only,
     matching `where: { in: [...] }`, redacting env-sourced literals under `hide_literals`) for
     `ghost-63`.
   - Defer `join: { kind: anti | left }` out of v0.24 because D1 + D2 + D3 + `where: { not_in }`
     cover all stratum anti-join units without expanding the join planner.
4. **D4 -- Unified `compare` assertion (negated equality and ordering):** Add `assert.compare` with
   `op: eq | ne | lt | le | gt | ge`, target `{ field } | { value } | { row }`, and
   `order: exact | lexical | numeric | semver` (default `exact`; `lt/le/gt/ge` refused under
   `order: exact`). `order: exact` accepts string/bool `to.value` literals; `order: numeric`
   accepts finite YAML number literals (`f64`, rejecting `NaN`/`inf`) and fails loud on non-numbers;
   `Null` and `Missing` operands emit typed analysis failures (requiring explicit `where:` filters
   when nullable). Uses row-keyed `values::Key` (`Key::new("compare")` + `Key::part(op)` +
   `Key::row`). Covers both negated equality (`element-web-05`, `thunderbird-android-55`) and
   version/numeric ordering while keeping existing `equal:` unchanged.
5. **D5 -- Gap C4 offline semver range satisfaction (`satisfies`):** Add `assert.satisfies`
   (`version` / `version_literal`, `range` / `range_literal`, refusing both literals together;
   `dialect: node | cargo | pep440-basic`, `include_prerelease: false`) backed by an owned,
   zero-dependency, pure-Rust `semver_eval.rs` module supporting partial versions, `||`,
   hyphen/`x`-ranges, and bounded complexity (`1_024` bytes, `64` clauses, `64` comparators),
   failing loud on `Null`/`Missing` or unparseable versions/ranges, and keyed via
   `Key::new("satisfies")` + `Key::row`.
6. **D6 -- Gap B1 general duplicate-key detection (`no_duplicate_keys`, v1 rule kind):** Register
   one new per-file v1 rule kind `no_duplicate_keys` (`DispatchClass::PerFile`, headline canonical
   kinds `104 -> 105`) covering YAML (streaming `ayaml 0.2.3` events across multi-document `---`
   streams, all mapping depths, and sequence elements, using YAML 1.2 Core Schema typed scalar keys
   and honoring `<<: *base` merges without false positives), JSON (`jsonc-parser` CST with optional
   `strict: true`), dotenv (`adotenv` `doc.entries()` + `entry.key_span()`), `.properties`, and INI
   (via a spanned entry walker in `ini.rs` merging repeated `[section]` scopes). TOML and HCL
   attributes/object keys already fail at parse time as non-baselinable `resource_parse` (HCL
   blocks legitimately repeat); XML is refused at build time (and omitted `format:` on `.xml`/unknown
   extensions emits `resource_parse`). Key-only messages preserve dotenv/properties value redaction;
   findings are capped per file (`MAX_DUPLICATES_PER_FILE`) and per rule (`findings_per_rule`).
7. **D7 -- Opt-in strict-JSON parse mode (`strict: true`):** Add optional `strict: true` (default
   `false`) across v1 JSON rules (`json_path_*`, `json_schema_passes`, `no_duplicate_keys`,
   `key_parity` when `format: json`), v1 `ExtractSpec` (`{ json: "$.x", strict: true }`, relaxing
   `maxProperties` to `2` for `json` + `strict` only), and v2 JSON `entries:` / `read:` / `records:`
   (refusing `strict: true` on non-JSON formats). Disables `strip_jsonc` fallback and rejects
   empty/whitespace-only files and UTF-8 BOMs under RFC 8259 as `resource_parse`, with strictness
   carried in `StructuredCache`, `FileStructuredCache`, and `DecodeCache` keys so tolerant and
   strict rules never collide.
8. **D8 -- Issue #592 array-of-objects `records:` reader, parameterized `transform:`, and
   mixed-reader `union:`:**
   - Add a dedicated `records:` reader (`format: json | yaml | toml`, `at:` reusing `entries:`'s
     JSONPath list decoder, optional `strict:` for `json`) whose `fields:` use the unified
     `FIELD_KEYS` map/string form (`"record: $.name"` or `{ source: "record: $.imagePullPolicy",
     optional?: bool, sensitive?: bool }`) with exact CST/spanned-event source spans and lexeme
     preservation (`ayaml::SpannedTree`, `jsonc-parser`, `toml_edit`), staying within `spans.rs`'s
     3-word `Option<Span>` budget.
   - Add non-regex linear-time parameterized transforms (`StringTransform` switched to `Clone`):
     `{ split: { on, part, required? } }` (non-empty `on`, `required: false` default emits `Missing`
     on out-of-range index vs analysis failure when `required: true`), `{ strip_prefix }`,
     `{ strip_suffix }` (non-empty string/list), `{ prefix }`, and `{ suffix }`.
   - Allow per-part `fields:` in `union:` (forbidding BOTH top-level and per-part `fields:`,
     requiring identical output field sets and sensitivity flags across parts, normalized to the
     first part's field order across all v2 decode/resolve/sensitivity/plan-check/schema passes),
     unblocking mixed-reader unions (`mattermost-27`).
9. **D9 -- Issues #593, #594, #595, #598, and #570:**
   - **#593:** extend `FIELD_KEYS` (`source`, `sensitive`, `optional`, `on_missing`) so `bound:`
     fields accept `{ source: "bound: $.name", optional: true, on_missing?: skip | missing,
     sensitive?: bool }` (default `optional: false` unchanged; `on_missing: skip` default drops the
     resource's rows and excludes it from `resource_groups`; `on_missing: missing` emits
     `Cell { value: None, span: None }`, matching D1 `reference`'s `skip_missing`).
   - **#594:** support bare `{{value}}` (and `{{ value }}`) in custom `message:` on `set` and
     `reference` findings via a `constraint`-local helper alongside `template::render_message`,
     explicitly rejecting `{{value}}` on unsupported assertions (`unique`, `count`, `equal`,
     `compare`, `satisfies`) while leaving non-alint `{{...}}` literals untouched, formatting via
     `Scalar::text_shown()` (`SHOWN_CHARS = 64` + `values::escaped()`), and unconditionally
     redacting to `<redacted>` whenever `sensitive || reads_environment || hide_literals`.
   - **#595:** anchor missing-value `set` findings at the group's unique structural path-component
     counterpart in `SetGroup::resources` for `PathToken`-derived `group_by` (handling case
     transforms and repeated tokens like `en/en.json`), using `Place::Resource` (file-level, no
     `1:1`) and `Place::Nowhere` when empty, falling back to `group.first` when ambiguous or for
     `entry.root`/cross-relation targets (path-free `baseline_key` keeps baselines stable).
   - **#598:** preserve surviving single-root stubs from `ops.rs::take_stub_roots` (`fr:`,
     `fr: {}`, `fr: []` under `strip-root`, while multi-root stubs remain analysis failures per
     ADR-0037 decision 2) through non-`entry.path` transforms and `where:` filters onto
     `RelationRows::stub_rows`, consulted by `set` and `count` when `group_by` derives from
     `entry.root`.
   - **#570:** add `ci/scripts/check-v2-schema-id.sh` (plus `ci/scripts/test-check-v2-schema-id.sh`
     in `shell-tests.sh`) with two explicit modes: default dev mode in `ci/scripts/preflight.sh`
     (verifying `schemas/v2/config.json` `$id` == `xtask/src/gen_schema_v2.rs::RELEASE` == both docs
     modelines) and `--release <tag>` mode in `.github/workflows/release.yml` `preflight` (also
     diffing `schemas/v2/config.json` with `$id` normalized against the latest prior `v*.*.*` tag
     strictly excluding `<tag>`, failing if changed while `RELEASE != <tag>`), synchronized with
     `ci/scripts/bump-version.sh` and `test-release-preflight-parity.sh`.
10. **D10 -- DEC-8 bundled `version: 2` rulesets (ADR-0048 decision 3c):** Replace `PathBuf`-only
    v2 source identity with `SourceLocator::{Local(PathBuf), Bundled(String)}` and add a dedicated
    `v2_sources: Vec<SourceLocator>` origin table for `FIELD_ORIGINS_MARKER` (leaving
    `Config::config_files` strictly on-disk paths for self-edit protection). Allow
    `alint://bundled/...` as a `version: 2` source class (including `as:` aliases in `decode.rs` and
    `gen_schema_v2.rs`) while keeping remote and nested `version: 2` sources refused until v0.25,
    enforce ADR-0022's `source <= consumer` direction rule with user config errors (never
    `Error::Internal`), update `xtask/src/docs_export/rulesets.rs` to render `@v<rev>` from each
    ruleset's canonical id instead of hardcoding `@v1`, and ship first-party opt-in
    `js/workspace-deps@v2` with `alint test` fixtures (leaving all `@v1` rulesets untouched).
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
  `no_duplicate_keys` reuses `ayaml 0.2.3`, `jsonc-parser`, `adotenv`, `properties.rs`, and a small
  spanned walker in `ini.rs`.
- **Clean soak period before v2 freezes:** `version: 2` remains experimental through v0.24 so the
  new operators, `records:` reader, and bundled `version: 2` rulesets get real-world soak time
  before v0.25 freezes `version: 2` and opens remote/nested v2 sources.
- **Honest limitations retained deliberately:**
  - `backstage-25`'s array sort-order check stays separate from per-group `set` equality.
  - `jest-28`'s version-directory check covers non-empty directories because `selectors:` match
    files only.
  - Anti/left joins, regex transforms, and XML value rows stay out of scope for v0.24.
  - `no_duplicate_keys` adds a small maintenance obligation for the `ayaml` event walk and
    `jsonc-parser` CST walk alongside `Format::parse`.
- **Execution gate cleared:** With owner approval recorded here (`status: accepted`), Phase 5
  proceeds in four ordered batches (5a -> 5b -> 5c -> 5d).

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
  `records:` reader using unified `FIELD_KEYS` (`source: "record: $.sub"`). A dedicated reader
  keeps `entries:`'s key/value fields and `leaves:` semantics unambiguous while preserving one
  uniform `FIELD_KEYS` map syntax across all field sources.
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
