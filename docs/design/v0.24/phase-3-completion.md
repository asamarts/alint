# Design doc: Phase 3 completion (`version: 2` operators, `#592` to `#598`, B1, C4, strict JSON, and bundled `version: 2`)

Status: Proposed design (2026-11-24), awaiting owner sign-off at the v0.24 Phase 5 gate ([ADR-0050](../../adr/0050-v024-phase-3-completion-design.md)).
Decisions: [ADR-0050](../../adr/0050-v024-phase-3-completion-design.md) (proposed, D1 to D11), implementing [ADR-0048](../../adr/0048-v024-owner-decisions.md) decisions 3a, 3b, and 3c.
Inputs: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (DEC-8, DEC-9),
[ADR-0022](../../adr/0022-policy-authoring-loop-contracts.md),
[ADR-0034](../../adr/0034-v2-version-boundary-and-phase-3-design.md),
[ADR-0037](../../adr/0037-phase-3-design-refinements.md),
[ADR-0048](../../adr/0048-v024-owner-decisions.md),
[`v0.23/selectors-and-constraints.md`](../v0.23/selectors-and-constraints.md),
[`v0.23/corpus-stratum-log.md`](../v0.23/corpus-stratum-log.md),
[`v0.24/README.md`](README.md),
and GitHub issues #592, #593, #594, #595, #598, and #570.

## 1. Problem and scope

v0.23 shipped the first Phase 3 slice: experimental `version: 2` configs, named `selectors:` and
`relations:`, value-bearing entry rows (`json`, `yaml`, `toml`), inner equality `join:`, and the
`constraint` kind with `unique`, `equal`, and `set`. Phase 8's binary verification on the 20-repo
corpus stratum (`docs/design/v0.23/corpus-stratum-log.md`) proved the core architecture and exposed
the exact gaps keeping high-value policies in buckets B and C:

1. **Missing Phase 3 assertion families:** no `reference` assertion (11 B stratum units), no
   `count` / `bounded-count` assertion (forcing awkward self-disjoint `set` workarounds on
   `element-web-06`, `babel-12`, and `ghost-63`), no multi-field `group_by: [...]` (`ghost-02`
   still needs 5 `key_parity` rules), no per-group cross-relation `set` comparison (`backstage-25`),
   no negated equality (`element-web-05`, `thunderbird-android-55`), and no inline literal `set`
   targets (`ghost-63`).
2. **Six open v0.23 Phase 3 issues (#592, #593, #594, #595, #598, #570):**
   - **#592:** no multi-field extraction from an array of objects into one row (e.g. Kubernetes
     `spec.containers[*]` `{name, image}` or `trpc-24`), no parameterized string split/prefix/suffix
     transform (`repo:tag`, `$<name>`, `.singular`/`.plural`), and no mixed-reader `union:`
     (`mattermost-27`).
   - **#593:** a `bound:` query matching nothing fails the entire relation with no `optional: true`
     escape hatch.
   - **#594:** custom `message:` on `set` (and `reference`) findings has no `{{value}}` placeholder
     for missing values where no input row exists.
   - **#595:** a missing-value `set` finding sits at the group's first file rather than the target
     file matching the reference row under the group's path token.
   - **#598:** a stub locale file (`fr:`, `fr: {}`, `fr: []`) under `strip-root` forms no group
     when `group_by` reads `entry.root`.
   - **#570:** no release preflight gate verifies that `schemas/v2/config.json`'s `$id` and
     `xtask/src/gen_schema_v2.rs`'s `RELEASE` constant advance when the v2 draft schema changes.
3. **High-ROI stratum capabilities identified by Phase 8:**
   - **Gap B1 (`no_duplicate_keys`):** `entries: { format: yaml, at: "$", leaves: true }` missed
     duplicate non-leaf mapping keys with disjoint children (`jobs: { a: 1 }` / `jobs: { b: 2 }`)
     and duplicate keys inside sequences (`steps: [{ name: a, name: b }]`), leaving `mattermost-56`
     and `zulip-28` in bucket B. Phase 2's `ayaml 0.2.3` spanned event stream now makes exact
     document-wide duplicate-key detection straightforward without `saphyr`.
   - **Gap C4 (semver range satisfaction and version comparison):** 21 stratum units (including all
     13 B-adapter `toolchain-pin` units, moving `toolchain-pin` from 63% to 83%) need version
     comparison or range satisfaction (`version satisfies range`, `version >= floor`).
   - **Strict-JSON parse mode:** `Format::Json` is JSONC-tolerant by default (comments and trailing
     commas retry cleanly), which silently accepts invalid RFC 8259 JSON in locale catalogs and
     keeps 10 locale units in bucket C across 8 repos.
4. **DEC-8 bundled `version: 2` rulesets (ADR-0048 decision 3c):** v0.23 restricted `version: 2`
   to local files only. Opening `alint://bundled/...` for `version: 2` lets alint ship first-party
   opt-in v2 policies (such as dependency version consistency) while keeping remote (`https://...`)
   and nested (`nested_configs: true`) `version: 2` sources refused until `version: 2` stabilizes
   in v0.25 at the earliest.

This document locks the exact syntax, semantics, error handling, trust/redaction rules, cost
bounds, baseline identity, and test gates for all 11 Phase 5 decisions (D1 to D11) before
implementation starts.

## 2. What binds the design

Ground-truth facts verified against `main` (`4ae5664c9`) after Phases 0 to 4:

- **Relation IR and limits (`crates/alint-core/src/relation/`):**
  - `Limits` (`limits.rs`) enforces 9 `LimitKind` values with defaults and hard caps:
    `rows_per_extractor` (1M / 10M), `rows_per_run` (5M / 50M), `distinct_values` (100k / 10M),
    `join_inputs` (1M / 10M), `join_outputs` (1M / 10M), `cells_per_run` (5M / 50M),
    `bytes_per_run` (1 GiB / 16 GiB), `key_path_bytes_per_decode` (32 MiB / 16 GiB; #569), and
    `findings_per_rule` (10,000 / 1,000,000; #596). Every breach is a non-baselinable analysis
    failure.
  - `the_join_limits_bound_only_the_evaluators_join` (`limits.rs:375`) is a static source-scan test
    guarding `JoinInputs` and `JoinOutputs`; any new operator that uses them must update that test
    consciously, or use `DistinctValues` / `CellsPerRun` / `BytesPerRun`.
  - `PlanSource` (`plan.rs:149`), `SourceId` (`v2/model.rs:104`), `Alias` (`v2/model.rs:116`), and
    `Objects::record_extended` (`v2/model.rs:45`) currently identify v2 sources exclusively by
    `PathBuf`. Bundled `version: 2` rulesets have no on-disk `PathBuf` and therefore require a typed
    source locator enum.
- **Current v2 DSL and `constraint` surface (`crates/alint-dsl/src/v2/`, `crates/alint-rules/src/constraint/`):**
  - `Rows` has `Part(Part)`, `Union(Vec<Part>)` (sharing one top-level `fields:` map), and
    `Join(Join)` (inner only, one side declared `unique: left | right`).
  - `Reader` has `Entries(Entries)` (`format: json | yaml | toml`, `at: Vec<String>`, `leaves: bool`)
    and `Read { read, optional }` (`json`, `yaml`, `toml`, `regex`, `lines`, `whole_file`).
  - `FieldSource` has `Path(PathToken)`, `EntryKey`, `EntryValue`, `EntryParent`, `EntryRoot`,
    `EntryPath`, `Value`, `Bound(String)`, and `Side(JoinSide, String)`.
  - `Transform` (`v2/model.rs:305`) has 5 fixed zero-argument transforms (`trim`, `lower`,
    `semver-major`, `semver-minor`, `strip-root`).
  - `Assertion` (`constraint/mod.rs:63`) has `Unique`, `EqualGroups` (single `group_by`),
    `EqualSource`, `EqualField`, and `Set(SetAssertion)` (single `group_by`, target = `Group(Value)`
    or `Relation { relation, field }`, mode = `equal_to | subset_of | disjoint_from`, optional
    `groups:` and `plurals: cldr`).
  - Every `constraint` finding uses a path-free `baseline_key` (ADR-0037 decision 3) so moving or
    renaming the file that sorts first never churns baselines.
- **Bundled loader (`crates/alint-dsl/src/loader.rs`, `bundled.rs`):**
  - `decode_bundled` (`loader.rs:1954`) decodes bundled YAML as v1 only and rejects `version: 2`.
  - `v2::may_be_v2` detects v2 sources before typed decode; `bundled.rs` has a unit test asserting
    all bundled rulesets are `version: 1` (which updates when opt-in `@v2` rulesets are registered).
  - ADR-0022's version direction rule (`source <= consumer`) must remain unbroken: a `version: 1`
    config may never extend a `version: 2` bundled ruleset.
- **Structured parse cache (`crates/alint-core/src/structured_cache.rs`, `structured_format.rs`):**
  - Phase 2 (#548) caches parsed `SharedValue` trees by `(content_hash, Format)`. A strict-JSON
    parse option MUST distinguish strict vs JSONC-tolerant keys so a JSONC-tolerant rule cannot
    poison a strict-JSON rule in the same run (or vice versa).
- **Stratum verification of the two `cardinality` / `group` units (ADR-0048 decision 3b):**
  - **`forem-01` (originally bucketed `cardinality`):** already confirmed **A** in v0.23 via
    grouped `set: { group_by: locale, field: key, equal_to: { group: en }, groups: [fr, pt],
    plurals: cldr }`. No separate `cardinality` assertion kind is needed for `forem-01`; folding
    cardinality into unified `count` (`min`, `max`, `exact`, optional `group_by`) cleanly serves
    emptiness and count-bound units (`element-web-06`, `babel-12`, `ghost-63`, plus general count
    policies).
  - **`backstage-25` (originally bucketed `group`):** requires that a non-module plugin package's
    `backstage.pluginPackages` array equals the sorted names of all plugin/library packages sharing
    its `pluginId`. Verifying this unit directly against `docs/design/v0.23/stratum/backstage-backstage.json`
    shows two concrete requirements:
    1. **Per-group cross-relation `set` comparison**: comparing group `G` of input relation `A`
       against matching group `G` of target relation `B` (`equal_to: { relation: ..., field: ...,
       group_by: ... }`), because `pluginPackages` is one relation and sibling packages by
       `pluginId` is another relation.
    2. **Array order**: strict sorted-order enforcement on `backstage.pluginPackages` is a separate
       list-order property (orthogonal to set equality; either checked separately or noted honestly
       when re-scoring `backstage-25`).
    Orthogonal multi-field `group_by: [...]` across all assertion families (`equal`, `set`, `count`)
    also collapses `ghost-02` from 5 `key_parity` rules into 1 v2 `constraint` rule and enables
    per-manifest grouped `set` (`fluentui-24`, `trpc-07`).

## 3. Proposed design (D1 to D11)

All `version: 2` additions below stay **v2-only and experimental** through v0.24 (ADR-0048
decision 3a). The lone new v1 rule kind is `no_duplicate_keys` (D6), and `strict: true` on JSON
parsing (D7) applies across both v1 and v2 JSON surfaces.

### 3.1 D1: `reference` assertion (single-key and composite-key foreign keys)

**Syntax (under `kind: constraint`, `assert:`):**

```yaml
assert:
  reference:
    # Single field or ordered composite key tuple (non-empty list or single field name):
    field: catalog_ref            # shorthand for fields: [catalog_ref]
    # OR:
    fields: [catalog, dependency]
    to:
      relation: yarn-catalogs
      field: key                  # shorthand for fields: [key]
      # OR:
      fields: [catalog, dependency]
    # Optional: restrict reference check to groups, or omit for global foreign-key check.
    # When `fields` already includes the group key (e.g. [manifest, dependency]),
    # composite-key `reference` checks per-manifest foreign keys in O(N + M) without grouping.
```

**Semantics:**
- `field:` and `fields:` are mutually exclusive on both the source side and `to:`; arity of source
  `fields` MUST equal arity of `to.fields` (checked at config load time).
- Evaluates `to.relation`, builds a hash set of target key tuples (`Vec<Value>` of length `k`),
  bounded by `LimitKind::DistinctValues` (and `LimitKind::BytesPerRun`).
- For every row in the input relation:
  - If any source key field is `Missing` (for example from an optional `bound:`): skip the row
    only if `skip_missing: true` (default `false`); by default a `Missing` key field in `reference`
    is an unresolved reference violation naming the missing field.
  - If the source key tuple is NOT in the target set, emit one finding anchored at the first source
    key field's span (with the source row's primary resource path).
  - Type safety: key comparison uses the relation layer's typed `Value` equality (`1` != `"1"`). If
    every target key in column `j` has type `T_target` and a source key in column `j` has a
    different non-null scalar type `T_src`, emit a typed mismatch analysis failure (matching `set`
    and `equal`'s fail-loud type discipline).
- **Finding identity (`baseline_key`):** path-free, deterministic:
  `reference\0<written_target_relation>\0<escaped_key_tuple>` (plus occurrence disambiguation
  via ADR-0047/ADR-0049 when multiple rows share the exact same dangling key tuple; one finding per
  dangling source row so every broken call site is pointed out and actionable).
- **Finding cap:** governed by `LimitKind::FindingsPerRule` (default 10,000; #596).
- **Stratum impact:** unlocks single-key (`forem-29`, `jest-28`, `nextcloud-b02` with D8 prefix
  strip) AND composite-key (`babel-17`, `trpc-10`, `backstage-26`, `rushstack-02`,
  `yarnpkg-berry-17`, plus `fluentui-24` and `trpc-07` via `(manifest, dep) -> (manifest, dep)`).

### 3.2 D2: Unified `count` assertion (folding `cardinality` and `bounded-count`)

**Syntax (under `kind: constraint`, `assert:`):**

```yaml
assert:
  count:
    group_by: manifest            # optional: string or list of field names; omitted = whole relation
    field: dependency             # optional: required only when `distinct: true`
    distinct: false               # optional boolean, default false (count rows vs distinct values)
    min: 1                        # optional non-negative integer
    max: 0                        # optional non-negative integer (0 asserts emptiness)
    exact: 1                      # optional non-negative integer (mutually exclusive with min/max)
    groups: [en, fr]              # optional list of expected group keys (single-field group_by)
```

**Semantics:**
- At least one of `min`, `max`, or `exact` MUST be set; `exact` is mutually exclusive with `min`
  and `max`; when both `min` and `max` are set, `min <= max` is enforced at load time.
- Without `group_by:`: counts all rows of the input relation (or distinct values of `field` when
  `distinct: true`, bounded by `DistinctValues`).
  - Special case `max: 0` (emptiness assertion, replacing the self-disjoint `set` hack for
    `element-web-06`, `babel-12`, and `ghost-63`): emits one finding per offending row (capped by
    `FindingsPerRule`), anchored at that row's file and span, so the user sees every offending row
    rather than one opaque "found 7 rows, expected 0" message at `.alint.yml`!
  - All other violations (`count < min`, `count > max` where `max > 0`, or `count != exact`): emit
    one finding per failing group (or one relation-level finding when ungrouped), anchored at the
    group's first row (or config file when the group has zero rows, e.g. from `groups:` or
    `resource_groups`), with up to 5 sample rows attached as `related_locations`.
- When `group_by:` is a path field (`path`, `dir`, `basename`, `stem`, `ext`, `parent_name`): every
  resource matched by the input relation's selector forms a group (even with 0 rows), matching
  `set`'s `resource_groups` semantics so `count: { group_by: path, min: 1 }` fires on empty files.
- **Finding identity (`baseline_key`):**
  - For `max: 0` per-row findings: `count_empty\0<escaped_group_key>\0<escaped_row_fields>`.
  - For aggregate count violations: `count\0<escaped_group_key>\0<bound_kind>` (`min` | `max` | `exact`).

### 3.3 D3: Orthogonal multi-field `group_by`, per-group cross-relation `set`, inline literal `set` targets, and anti-join stance

1. **Orthogonal multi-field `group_by` across `equal`, `set`, and `count`:**
   - Wherever `group_by:` accepts a single field name string (`group_by: locale`), it ALSO accepts
     a non-empty list of distinct field names (`group_by: [locale, namespace]`), normalized
     internally to `Vec<String>`.
   - Group keys compare and sort lexicographically by tuple `(v_0, v_1, ..., v_{k-1})`.
   - `groups:` on `set` and `count` accepts scalars when `group_by.len() == 1` and fixed-length
     arrays of length `k` when `group_by.len() == k` (checked at load time).
   - `equal_to: { group: ... }` on `set` likewise accepts a scalar (1-field) or `k`-element array
     (`k`-field).
   - `resource_groups` (zero-row file groups) activates when every field in `group_by` derives from
     a path token (or `entry.root` under D9/#598).
   - **Impact:** collapses `ghost-02` (5 `key_parity` rules -> 1 v2 `constraint` rule with
     `group_by: [namespace, locale]`).
2. **Per-group cross-relation `set` comparison (closes `backstage-25` set half, `fluentui-24`, `trpc-07`):**
   - `set` targets `{ relation: R, field: F }` gain an optional `group_by:` field (single name or
     list of the same arity as the input's `group_by:`):
     ```yaml
     assert:
       set:
         group_by: plugin_id
         field: plugin_package
         equal_to:
           relation: workspace-plugins
           field: name
           group_by: plugin_id
     ```
   - Semantics: partitions the target relation `R` by `to.group_by` into a map `group_key -> Set<Value>`
     (bounded by `DistinctValues` and `BytesPerRun`). Each input group `G` compares against `R[G]`
     (empty set if `G` is absent from `R`; for `equal_to`, target groups absent from input also
     report missing values if the input's `groups:` or `all_target_groups: true` opts in, default
     false so input drives which groups exist).
   - `equal_to`, `subset_of`, and `disjoint_from` all work symmetrically on per-group target sets.
   - **Honest note on `backstage-25`:** per-group `set` expresses exact membership equality between
     `backstage.pluginPackages` and sibling packages sharing `pluginId`; checking that the JSON
     array is also lexicographically sorted remains a separate ordering concern (documented honestly
     in the Phase 5 stratum re-score).
3. **Inline literal `set` targets:**
   - `set` targets (`equal_to:`, `subset_of:`, `disjoint_from:`) accept `{ values: [<scalar>, ...] }`
     in addition to `{ group: ... }` and `{ relation: ..., field: ... }`:
     ```yaml
     assert:
       set:
         group_by: file
         field: key
         subset_of: { values: [id, title, description, tags] }
     ```
   - Literals follow the same scalar typing rules as `where: { in: [...] }`, and environment-sourced
     literals trigger the existing `hide_literals` / sensitive-evidence redaction gate.
   - **Impact:** cleanly resolves `ghost-63` allowed-field checks without a synthetic relation.
4. **Anti-join stance (deferred -- no `join: { kind: anti }` in v0.24):**
   - Every stratum anti-join need (`fluentui-24`, `trpc-07`, `babel-12`, `wsdk-09`) is already
     covered more simply and declaratively by `reference` (D1), `count: { max: 0 }` over an inner
     `join:` (D2), per-group `set: { subset_of / disjoint_from }` (D3), and `where: { not_in: [{ relation, field }] }` (v0.23).
   - Adding a fourth join type (`anti` / `left`) would expand the join planner and `JoinInputs`/
     `JoinOutputs` surface without unlocking any stratum unit beyond D1 + D2 + D3. Anti/left joins
     therefore stay out of scope for v0.24.

### 3.4 D4: Negated equality and ordered comparison (`compare` / `not_equal`)

**Design question:** should negated equality (`element-web-05`, `thunderbird-android-55`) be a
boolean flag on `equal:` (`negated: true`), a standalone `not_equal:` family, or unified with D5's
version/scalar comparison under `compare:`?

**Recommended syntax:** add a dedicated **`compare:`** assertion family that covers both negated
equality (`op: ne`) AND ordered comparisons (`op: lt | le | gt | ge | eq | ne`) cleanly in one
orthogonal surface:

```yaml
assert:
  compare:
    field: value
    op: ne                        # eq | ne | lt | le | gt | ge
    to:
      field: key                  # row-level: compare two fields on the same row
      # OR: value: "latest"       # compare against a scalar literal
      # OR: row: { source: .nvmrc } # compare against a single reference row (like `equal`)
    order: exact                  # optional: exact (default; eq/ne only) | lexical | numeric | semver
```

**Semantics:**
- `op: eq | ne` with default `order: exact` uses exact typed `Value` equality (`1 != "1"`) and
  works on all scalar types (`null`, `bool`, `number`, `string`).
- `op: lt | le | gt | ge` requires an ordering:
  - `order: numeric`: compares `Value::Number` (or numeric string when `coerce` is not used -> fail
    loud on non-number so string/number bugs never hide!).
  - `order: lexical`: compares `Value::String` by UTF-8 code-point order.
  - `order: semver`: parses both sides as semver versions (see D5 below) and compares by semver
    precedence (prerelease < release; build metadata ignored per SemVer 2.0.0).
- Existing `equal:` keeps 100% backwards compatibility (`equal:` also covers group-wide uniformity
  `equal: { field: version, group_by: ... }`, which is a multi-row equivalence check rather than a
  pairwise `compare:`).
- **Finding placement and `baseline_key`:** anchored at `field`'s row/span (with `to.field` or
  `to.row` attached as a `related_location` when present); path-free key
  `compare\0<op>\0<order>\0<escaped_lhs>\0<escaped_rhs>`.
- **Impact:** cleanly unlocks `element-web-05` (`compare: { field: value, op: ne, to: { field: key } }`),
  `thunderbird-android-55`, and all version-floor / version-ordering `toolchain-pin` units in one
  small, unified assertion family.

### 3.5 D5: Gap C4 -- Semver range satisfaction (`satisfies`) and version parsing

**Syntax (under `kind: constraint`, `assert:`):**

```yaml
assert:
  satisfies:
    version: pin_version          # field holding a version (e.g. "22.13.0", "v22.13", "3.12")
    # OR: version_literal: "22.13.0"
    range: engine_range           # field holding a range (e.g. ">=20 <23", "^22.0.0", ">=3.10,<3.13")
    # OR: range_literal: ">=20.0.0"
    dialect: node                 # optional: node (default) | cargo | pep440-basic
    include_prerelease: false     # optional boolean, default false
```

**Version and range parsing rules (100% offline, deterministic, zero-shell):**
- **Implementation choice:** own a small, pure-Rust, zero-unsafe module `crates/alint-core/src/semver_eval.rs`
  (~250 to 350 LOC, zero new third-party dependencies, `cargo-deny`-clean, MSRV 1.88 compatible).
  Why owned over the `semver` crate? The `semver` crate implements Cargo's dialect ONLY and rejects
  partial versions (`22`, `22.13`, `v22.13.0`), `||` disjunctions, space-separated AND comparators
  (`>=20 <23`), hyphen ranges (`1.2.3 - 2.3.4`), `x`/`*` wildcards (`22.x`), and `v`-prefixes --
  which are the exact forms appearing across the 21 corpus stratum units!
- **Partial version normalization:** strips optional leading `v`/`V`; parses `MAJOR[.MINOR[.PATCH[-PRE][+BUILD]]]`
  with missing `MINOR`/`PATCH` filled as `0` for concrete versions (for example `22` -> `22.0.0`,
  `22.13` -> `22.13.0`) while preserving partial semantics in range comparators (`^22` -> `>=22.0.0 <23.0.0`,
  `~22.13` -> `>=22.13.0 <22.14.0`, `22.x` -> `>=22.0.0 <23.0.0`).
- **Dialects:**
  - `node` (default): npm/node-semver ranges -- `||` union, space-separated AND (and comma-separated
    AND tolerance when no ambiguity), `=`, `>`, `>=`, `<`, `<=`, `^`, `~`, hyphen `A - B`, `x`/`X`/`*`,
    leading `v`.
  - `cargo`: Cargo `VersionReq` caret/tilde/comma semantics (notably `^0.0.3` = `>=0.0.3 <0.0.4`,
    bare `1.2.3` = `^1.2.3`).
  - `pep440-basic`: comma-separated Python `requires-python` comparators (`>=`, `<=`, `>`, `<`, `==`,
    `!=`, `~=` compatible release, `.*` wildcard), covering Python `pyproject.toml` / `.python-version`
    toolchain pins in the stratum!
- **Bounds and errors:**
  - Range string capped at `MAX_RANGE_BYTES = 1_024`, at most `64` `||` clauses and `64`
    comparators per clause (preventing crafted range DoS).
  - Unparseable version or range string produces an **analysis failure** naming the field and
    resource (never a silent pass!), unless the caller pre-filters non-semver specifiers (`workspace:`,
    `catalog:`, `git+https:`) with `where: { not_matches: ... }` as v2 policies already do.
- **Finding identity:** anchored at the `version` field's span (with the `range` field's span as a
  `related_location`), path-free `baseline_key`:
  `satisfies\0<dialect>\0<escaped_version>\0<escaped_range>`.
- **Stratum impact:** directly unlocks 21 stratum units (moving `toolchain-pin` from 63% to 83%).

### 3.6 D6: Gap B1 -- General duplicate-key detection (`no_duplicate_keys`, v1 rule kind)

**Why a dedicated v1 rule kind instead of a v2 `entries:` mode?**
1. Duplicate-key detection is a single-file, per-file invariant ("no file matching `*.yml` has
   duplicate keys anywhere, at any depth, including inside arrays/sequences"). Making it a first-class
   per-file v1 rule kind (`DispatchClass::PerFile`) means:
   - it works in BOTH `version: 1` and `version: 2` configs (constitution invariant / section 12.1:
     single-file policies should keep a concise 5-line v1 spelling);
   - it runs on the LSP `didChange` single-file hot path (`eval_buffer`) with immediate keystroke
     feedback and exact line/column squiggles;
   - it scales linearly in streaming memory without materializing a relation table for every key in
     the repo.
2. **Syntax (v1 + v2):**
   ```yaml
   rules:
     - id: no-duplicate-yaml-keys
       kind: no_duplicate_keys
       paths: [".github/workflows/*.{yml,yaml}", "api/openapi.yaml"]
       format: yaml               # optional: auto-detected from extension if omitted
       level: error
   ```
3. **Supported formats (`Format` subset):**
   - `yaml`: walks `ayaml 0.2.3`'s spanned event stream directly with a lightweight stack of
     `HashSet<Key>` (one set per open mapping; popped on `MappingEnd`), catching duplicate scalar
     keys at EVERY depth -- including non-leaf mapping keys with disjoint children (`jobs: { a: 1 }`
     / `jobs: { b: 2 }`) AND mappings nested inside sequences (`steps: [{ name: a, name: b }]`).
     Honors `ayaml`'s existing 32 MiB byte cap, nesting cap, and alias/parse bounds. Reports exact
     1-based line and column of the repeated key, with `related_location` pointing to the first
     occurrence!
   - `json`: walks `jsonc-parser` CST (already a direct dependency of `alint-core`!), tracking keys
     per `Object` node at every depth (including objects inside arrays), with exact byte-offset ->
     1-based line/column spans for both the duplicate and the first occurrence. Respects `strict: true`
     (D7) when specified.
   - `dotenv`: uses `adotenv 1.6.0`'s existing duplicate-key diagnostic/entries with exact spans.
   - `properties`: uses `crates/alint-core/src/properties.rs` parser entries with exact spans.
   - `ini`: flags repeated keys within the same `[section]` (or top-level pre-section scope),
     reporting the repeated key's line/column and first occurrence.
   - `toml` and `hcl`: rejected at parse time by the format parsers themselves (emitting a
     `resource_parse` analysis failure, or surfaced cleanly as a parse error; `format: toml | hcl`
     is accepted and delegates to parser duplicate rejection so configs don't error on format).
   - `xml`: rejected at config build time with a clear explanation (XML sibling elements legitimately
     repeat by design; duplicate XML attributes are already parse errors in `roxmltree`).
4. **Trust & sensitivity:** `no_duplicate_keys` quotes ONLY key names (never values) on `dotenv`
   and `.properties`, preserving secret safety, and still obeys `sensitive: true` when explicitly set.
5. **Limits & failure semantics:** malformed files emit one non-baselinable `resource_parse`
   analysis failure; per-file duplicate findings cap at `LimitKind::FindingsPerRule` (or per-file
   cap) to avoid flooding on a crafted 100k-duplicate file.
6. **Stratum impact:** promotes `mattermost-56` and `zulip-28` cleanly to A.

### 3.7 D7: Strict-JSON parse option (`strict: true`)

**Problem:** `Format::Json` tries strict `serde_json` first and falls back to `strip_jsonc` on
failure (`structured_format.rs`), which silently accepts `//` comments and trailing commas in
files whose runtime parser is strict RFC 8259 `JSON.parse` (keeping 10 locale units in bucket C
across 8 repos).

**Syntax:**
- On v1 JSON rule kinds (`json_path_equals`, `json_path_matches`, `json_path_absent`,
  `json_schema_passes`, `no_duplicate_keys`, `key_parity` when `format: json`): optional
  `strict: true` (default `false`, preserving 100% backwards compatibility).
- On v1 extract (`cross_file`, `file_graph`, `registry_paths_resolve`): `extract: { json: "$.x", strict: true }`
  (or `{ format: json, query: "$.x", strict: true }`).
- On v2 `entries:` (`format: json`) and `read: { json: ... }`: optional `strict: true`
  (default `false`):
  ```yaml
  entries: { format: json, at: "$", leaves: true, strict: true }
  read: { json: "$.name", strict: true }
  ```
- **Valid-JSON-only rule spelling:** a repo that wants to reject invalid/JSONC files even when no
  specific key is asserted can write `no_duplicate_keys` with `strict: true` or `json_path_absent`
  with `strict: true`, or a lightweight `strict_json: true` on `json_schema_passes` / `entries:`.
  When `strict: true`:
  - `strip_jsonc` fallback is DISABLED; any `//`/`/* */` comment or trailing comma produces a
    `resource_parse` analysis failure.
  - Empty / whitespace-only files (which v1 `Format::parse` normally short-circuits to `{}`) MUST
    ALSO fail under `strict: true` because an empty file is NOT valid RFC 8259 JSON!
- **Cache isolation:** `StructuredCache` and `DecodeCache` include `strict: bool` in their cache
  key (or use `ParseMode::Standard | ParseMode::StrictJson`), guaranteeing a tolerant parse of
  `tsconfig.json` never satisfies a strict-JSON rule in the same run.
- **Stratum impact:** promotes 10 locale C units to A across 8 repos.

### 3.8 D8: Issue #592 -- Array-of-objects extraction, parameterized transforms, and mixed-reader `union:`

1. **Array-of-objects extraction (`records:` reader in v2 relations):**
   - Adds a third `Reader` variant beside `entries:` and `read:`: **`records:`**:
     ```yaml
     relations:
       k8s-containers:
         from: k8s-manifests
         records:
           format: yaml            # json | yaml | toml
           at: "$.spec.template.spec.containers[*]"   # JSONPath (string or list of strings)
           strict: false           # optional, json only (D7)
         fields:
           file: path
           name: { field: "$.name" }                  # sub-path relative to each matched object
           image: { field: "$.image" }
           pull_policy: { field: "$.imagePullPolicy", optional: true }
     ```
   - **Why `records:` instead of overloading `entries:`?** `entries:` iterates `(key, value)` pairs
     of a mapping (`entry.key`, `entry.value`, `entry.parent`, `entry.root`, `entry.path`), whereas
     an array of objects emits ONE row per matched element with arbitrary sibling sub-fields! A
     dedicated `records:` reader keeps `entries:`'s schema and error messages crisp and eliminates
     ambiguous interactions with `leaves: true` or `entry.root`.
   - **Semantics:**
     - `at:` (string or non-empty list of JSONPath queries) selects object elements from each
       resource. A matched non-object element is an analysis failure naming the element index
       (unless `at:` matched nothing, which emits 0 rows for that resource).
     - Each `fields:` entry using `{ field: "<subpath>", optional?: bool, sensitive?: bool }`
       evaluates `<subpath>` (a JSONPath rooted at `$` = the element object, e.g. `$.name` or
       `$.metadata.labels.app`) against the element:
       - Must select a scalar (`null`, `bool`, `number`, `string`); non-scalar fails loud (matching
         ADR-0034 decision 5).
       - If `<subpath>` matches nothing: emits `Missing` when `optional: true`, else analysis failure.
       - Span resolution: uses `ayaml` `SpannedTree` (YAML), `jsonc-parser` CST (JSON), or
         `toml_edit` (TOML) so every extracted record field keeps its exact source span!
     - Bounded by `RowsPerExtractor`, `RowsPerRun`, `CellsPerRun`, and `BytesPerRun`.
   - **Impact:** directly resolves `#592` (`trpc-24`, `mattermost-07`, `mattermost-32`, Kubernetes
     container name+image policies, and 12 stratum units).
2. **Parameterized string transforms (`transform:` in v2 relations):**
   - Extends `Transform` beyond bare names (`trim`, `lower`, `semver-major`, `semver-minor`,
     `strip-root`) to accept single-key parameterized maps:
     - `{ split: { on: ":", part: 0 } }` -- splits string by non-empty literal separator `on:` and
       selects 0-based `part:` (negative indices count from the end: `-1` = last segment, e.g. tag
       in `ghcr.io/org/app:1.2.3`; out-of-range index returns empty string `""` or fails if
       `required: true`, default `false`).
     - `{ strip_prefix: "$" }` -- strips literal prefix once if present (no-op if absent).
     - `{ strip_suffix: ".singular" }` (or `strip_suffix: [".singular", ".plural"]`) -- strips the
       first matching literal suffix once if present (no-op if absent).
     - `{ prefix: "v" }` / `{ suffix: ".json" }` -- prepends/appends a literal string (useful for
       `jest-28` `"version-" + ver` or `forem-29` variant filenames!).
   - All transforms are pure, linear-time `O(len)`, deterministic, non-regex (zero ReDoS surface),
     and charge output length against `BytesPerRun`.
   - **Impact:** directly resolves `#592` (`repo:tag`, `nextcloud-b02` `$<name>`, `strapi-21`
     `.singular`/`.plural`, `jest-28` `version-X`, `forem-29`).
3. **Mixed-reader `union:` (per-part `fields:`):**
   - Currently `union:` requires all parts to share one top-level `fields:` map, blocking
     `mattermost-27` (`entries:` `entry.key` unioned with `read:` `$.*.aliases[*]` `value`).
   - Allow each part in `union:` to specify its own `fields:` map (with all parts required to bind
     the exact same set of output field names and matching `sensitive` flags, checked at load time).
     When top-level `fields:` is given, parts inherit it as in v0.23 (100% backwards compatible).

### 3.9 D9: Issues #593, #594, #595, #598, and #570

1. **#593 -- Optional `bound:` queries:**
   - Extend `bound:` field source syntax from bare string `bound: "$.name"` to ALSO accept the map
     form:
     ```yaml
     fields:
       pkg_name: { bound: "$.name", optional: true }   # default optional: false (unchanged)
     ```
   - Semantics when `optional: true` and the `bound:` query matches nothing on a resource:
     - Default behavior (`on_missing: skip`, or when `optional: true`): emits **no rows** for that
       resource (matching `read: { ..., optional: true }` so manifests without the bound metadata
       cleanly opt out of the relation without failing it!).
     - Optional `on_missing: null` (emits `Value::Null` for the bound field while keeping the
       resource's rows) if a caller explicitly wants rows kept with a `null` bound value.
     - Default (`optional: false`): unchanged from v0.23 (analysis failure on missing bound).
2. **#594 -- `{{value}}` placeholder in `constraint` custom `message:`:**
   - Supported on `set` and `reference` findings (for the missing/extra value or unresolved key).
   - Formatting pipeline:
     1. If the compared field is `sensitive: true`, OR the rule/relation reads the environment
        (`reads_environment` / `hide_literals`), render `<redacted>` (never leaking secret values).
     2. Otherwise format the scalar value (or tuple joined by `, ` for composite `reference`),
        truncate to the same max length used by generated messages (`MAX_QUOTED_VALUE_LEN`), and
        escape bidi/zero-width/control characters via `alint_core::invisible` (matching #600).
   - Unknown placeholders remain rejected at load time; `{{value}}` on assertions that have no
     single compared value (`unique`, aggregate `count`) is rejected at load time with a clear
     diagnostic naming the allowed placeholders.
3. **#595 -- Target-file placement for missing-value `set` findings:**
   - When `set` reports "group `G` lacks value `V`":
     - If `group_by` derives from a path token (or `entry.root` under #598) and the reference row
       that supplied `V` lives at path `P_ref`:
       - Compute the expected target path `P_target` by replacing the reference group's path
         segment/token with `G` (for example `locales/en/common.json` -> `locales/fr/common.json`).
       - If `P_target` exists among the files belonging to group `G`, anchor the finding at
         `P_target` (line 1, col 1; file-level).
       - Otherwise fall back to group `G`'s first file (existing behavior), or config file if `G`
         has no files.
     - **Baseline stability:** `baseline_key` for `constraint` findings ALREADY excludes the primary
       path (ADR-0037 decision 3), so improving placement from the group's first file to `P_target`
       breaks ZERO existing v2 baselines!
4. **#598 -- Stub locale root naming its group under `group_by: entry.root`:**
   - When `entries: { format: yaml | json | toml, at: "$", leaves: true }` reads a resource whose
     root mapping has a single key `K` whose value is empty (`fr:`, `fr: {}`, `fr: []`) and the
     relation uses `entry.root` as a `group_by` field (with `strip-root` on `entry.path`):
     - Record `(K, resource)` as an **empty root group** on the evaluated relation (anchored at the
       stub file `resource`).
     - In `set` and `count`: `K` forms a group with 0 rows, placed at the stub file!
       - Without `groups:`: the stub locale `fr` is now checked and reports missing keys AT
         `config/locales/fr.yml` instead of silently passing!
       - With `groups: [de, fr, pl]`: `fr`'s missing-key findings now anchor at
         `config/locales/fr.yml` instead of path-less!
     - If a stub file has multiple empty root keys (`fr: {}\nde: {}`), each root key names its own
       empty group at that file.
     - Bounded by `LimitKind::DistinctValues` / `CellsPerRun`.
5. **#570 -- Release preflight gate tying `schemas/v2/config.json`'s `$id` to the release tag:**
   - Add `ci/scripts/check-v2-schema-id.sh` (plus unit regression script
     `ci/scripts/test-check-v2-schema-id.sh` wired into `ci/scripts/shell-tests.sh`), invoked by
     `ci/scripts/preflight.sh` and `.github/workflows/release.yml`:
     - Compares `schemas/v2/config.json` against the latest previous `v*` git tag (`git show <prev_tag>:schemas/v2/config.json`).
     - If the schema content differs (ignoring the `$id` field itself), verifies that:
       1. `xtask/src/gen_schema_v2.rs`'s `RELEASE` constant equals the release tag being cut
          (for example `"v0.24.0"`);
       2. `schemas/v2/config.json`'s `$id` URL contains that exact tag;
       3. the documented YAML modeline in `docs/` matches that exact `$id` URL.
     - If `schemas/v2/config.json` is unchanged since the previous tag, passes without forcing a
       spurious `$id` bump.

### 3.10 D10: DEC-8 bundled `version: 2` rulesets (ADR-0048 decision 3c)

**Loader and namespace architecture:**
- Replace raw `PathBuf` source identity in `crates/alint-dsl/src/v2/model.rs` (`SourceId`,
  `Alias::target`, `SourceObjects::extended`) and `crates/alint-core/src/relation/plan.rs`
  (`PlanSource`) with:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
  pub enum SourceLocator {
      Local(PathBuf),
      Bundled(String), // canonical bundled id, e.g. "alint://bundled/js/workspace-deps@v2"
  }
  ```
- **Source policy (ADR-0048 decision 3c):**
  - **Allowed `version: 2` sources in v0.24:** top-level config, local `extends:`, `.alint.d`
    fragments, and **bundled rulesets (`alint://bundled/...`)**.
  - **Refused `version: 2` sources in v0.24:** remote (`https://...`) `extends:`, nested configs
    (`nested_configs: true`), and `alint_dsl::parse` inline configs (deferred until v2 stabilizes
    in v0.25).
- **Direction rule (ADR-0022):** strictly enforced at every edge!
  - A `version: 2` config may extend `version: 1` OR `version: 2` bundled rulesets (with optional
    `as: <alias>` to reference the bundled ruleset's selectors/relations as `<alias>.<name>`).
  - A `version: 1` config extending a `version: 2` bundled ruleset fails loudly with a clear error
    naming `version: 2`.
- **Trust & provenance:** bundled rulesets are first-party compiled-in YAML (trusted, zero network,
  zero env reads). Their rules still carry `extended_source = true` (preventing config self-edits)
  and their selectors/relations record `ConfigSourceClass::Bundled` in `PlanSource`, displayed in
  `alint list` and `alint explain`.
- **First-party opt-in bundled `version: 2` ruleset(s) shipped in v0.24:**
  - `js/workspace-deps@v2`: dependency version consistency across JS/TS workspace manifests
    (`package.json` `dependencies` / `devDependencies` / `optionalDependencies`, ignoring
    `workspace:` / `catalog:` / `npm:` protocols and workspace package names), parameterized via
    overrideable named selectors/relations or `vars:` where helpful, with firing and silent
    `alint test` fixtures!
  - Existing `@v1` bundled rulesets (`docs/adr@v1`, `java@v1`, `rust@v1`, etc.) stay `version: 1`
    and untouched.

### 3.11 D11: Tooling, draft schema `$id`, `facts.json`, and experimental warning

- **`schemas/v2/config.json`:** advance `xtask/src/gen_schema_v2.rs` `RELEASE` to `"v0.24.0"`
  (verified by #570's preflight gate) and regenerate `schemas/v2/config.json` covering every new
  v2 key (`reference`, `count`, `compare`, `satisfies`, `records:`, `strict:`, parameterized
  `transform:`, optional `bound:`, per-group `set`, inline `values:`, per-part `union:` `fields:`).
- **`schemas/v1/config.json`:** add `no_duplicate_keys` (D6) and `strict:` on JSON surfaces (D7).
- **Headline kind count:** `constraint` stays ONE canonical kind (v2-only, experimental);
  `no_duplicate_keys` adds ONE canonical v1 kind (`104 -> 105` canonical kinds, `11` aliases).
- **Experimental warning text:** update the v2 load warning from "v0.24 at the earliest" to
  "v0.25 at the earliest" (ADR-0048 decision 3a).
- **`facts.json`:** keeps `compatibility.config_versions: [1]` and
  `compatibility.experimental_config_versions: [2]`.
- **Commands (`validate-config`, `list`, `explain`, `export-agents-md`):** updated additively to
  render `Bundled` v2 sources, `records:` readers, and the new `constraint` assertion families.

## 4. False-positive surface

Mandatory per `docs/design/TEMPLATE.md` and constitution invariant 12:

- **`reference` (D1):**
  - *Risk:* external/protocol values (`workspace:*`, `catalog:`, `@types/*`) or optional metadata
    not present in the target relation firing as dangling references.
  - *Mitigation:* callers filter with `where: { not_matches / not_in }` before `reference`; `skip_missing: true`
    opts out rows with missing optional `bound:` keys; type mismatches fail loud as analysis
    failures rather than producing misleading "dangling reference" findings.
- **`count` (D2):**
  - *Risk:* an empty repository or filtered-out relation firing `min: 1` unexpectedly, or `max: 0`
    flooding output on a misconfigured selector.
  - *Mitigation:* `min: 1` only fires on zero-row groups created explicitly via `groups:` or
    path-token `resource_groups` (never on nonexistent arbitrary keys); `max: 0` per-row findings
    are bounded by `LimitKind::FindingsPerRule` (10,000 default; #596).
- **Per-group `set` and multi-field `group_by` (D3):**
  - *Risk:* a target group missing from the input relation silently passing or unexpectedly firing
    when the input selector intentionally excludes draft packages.
  - *Mitigation:* input relation drives active groups by default; requiring target groups absent
    from input requires explicit `groups:` or `all_target_groups: true`.
- **`compare` and `satisfies` (D4, D5):**
  - *Risk:* pre-release tags (`22.0.0-rc.1`), partial versions (`22` vs `22.13.0`), or non-semver
    specifiers (`latest`, `lts/*`, `>=20 || >=22`) mis-comparing silently.
  - *Mitigation:* `satisfies` follows strict `node-semver` prerelease rules (`include_prerelease: false`
    by default), parses partial versions deterministically, supports `||` and hyphen/x-ranges
    natively, and turns unparseable versions/ranges into explicit analysis failures (forcing
    explicit `where:` filtering of non-semver strings instead of silent false passes or false fails).
- **`no_duplicate_keys` (D6):**
  - *Risk:* YAML anchors/merge keys (`<<: *base` with an override key in the mapping) being
    misreported as duplicate keys; or XML repeated elements firing.
  - *Mitigation:* YAML duplicate detection checks syntactic mapping keys in the same mapping node
    (an explicit key overriding a merged `<<: *base` key is NOT a duplicate key in that mapping;
    two literal `<<:` keys in one mapping ARE flagged); XML is explicitly refused at config build
    time; TOML/HCL already fail at parse time.
- **Strict-JSON mode (D7):**
  - *Risk:* enabling `strict: true` globally breaking `tsconfig.json` / `.vscode/*.json`.
  - *Mitigation:* `strict: false` remains the default everywhere; `strict: true` is strictly
    opt-in per rule/reader, and cache keys isolate strict vs tolerant parses so a tolerant rule on
    `tsconfig.json` never interferes with a strict rule on `locales/*.json`.
- **Array-of-objects `records:` and parameterized transforms (D8 / #592):**
  - *Risk:* `split` on a string without the separator (for example an untagged Docker image `nginx`
    split on `:` with `part: 1`) silently producing `""` and matching another untagged image.
  - *Mitigation:* `required: true` option on `split` turns missing-separator parts into `Missing`
    (or fails), while default `required: false` keeps `part: 0` returning the full string `"nginx"`
    (ideal for extracting repo name from `nginx` or `nginx:1.25`).
- **Optional `bound:` (#593) and stub locale roots (#598):**
  - *Risk:* `bound: { ..., optional: true }` silently hiding a typo in the JSONPath query; or a
    non-locale empty YAML file (`foo: {}`) forming a spurious empty group.
  - *Mitigation:* `optional: false` remains the default on `bound:` so typos fail loud unless
    explicitly opted in; stub-root empty groups activate ONLY when `group_by` explicitly reads
    `entry.root` under `entries:` with `leaves: true` and `strip-root`, and empty files with no
    root mapping key do not invent a fake group.
- **Target-file `set` placement (#595) and `{{value}}` placeholder (#594):**
  - *Risk:* wrong target-file guess when a group's path token appears twice in the path (e.g.
    `en/en.json`), or secret leakage through `{{value}}`.
  - *Mitigation:* target-file matching substitutes ONLY the exact matched token span of the
    grouping token and verifies the candidate path actually belongs to group `G`'s resource set
    (falling back to group `G`'s first file on any ambiguity); `{{value}}` unconditionally redacts
    `sensitive: true` and environment-influenced fields to `<redacted>` and escapes invisible
    Unicode controls.

## 5. Implementation notes and phased delivery

To keep reviewable, low-risk PRs with continuous CI verification, Phase 5 executes in 4 ordered
implementation batches once ADR-0050 is approved:

- **Batch 5a (Core v2 polish + preflight: #593, #594, #595, #598, #570):**
  - Optional `bound:` (#593), `{{value}}` placeholder (#594), target-file `set` placement (#595),
    stub locale root groups (#598), and `ci/scripts/check-v2-schema-id.sh` (#570).
- **Batch 5b (New operators + #592 readers/transforms: D1, D2, D3, D4, D8):**
  - `reference` (single + composite), unified `count`, multi-field `group_by: [...]`, per-group
    cross-relation `set`, inline literal `set` targets, `compare`, `records:` array-of-objects
    reader, parameterized `transform:` (`split`, `strip_prefix`, `strip_suffix`, `prefix`,
    `suffix`), and mixed-reader `union:` per-part `fields:`.
- **Batch 5c (High-ROI stratum capabilities: D5, D6, D7):**
  - `semver_eval.rs` + `satisfies:` / `compare: { order: semver }` (Gap C4), `no_duplicate_keys`
    v1 rule kind across YAML/JSON/dotenv/properties/INI (Gap B1), and `strict: true` JSON parse
    mode with isolated cache keys.
- **Batch 5d (DEC-8 bundled `version: 2`, schema/docs, and stratum re-score: D10, D11):**
  - `SourceLocator::{Local, Bundled}`, bundled `version: 2` loader support + direction-rule gates,
    first-party `js/workspace-deps@v2` bundled ruleset with `alint test` cases, `schemas/v2/config.json`
    `RELEASE = "v0.24.0"`, `docs/rules.md` / `facts.json` / `explain` / `list`, and the 20-repo
    corpus stratum re-score recorded in `docs/design/general-linter.md` section 12.4.

## 6. Tests

Every batch obeys constitution invariant 8 (firing and silent e2e scenarios), invariant 7 (schema
parity), ADR-0012 (output-completeness across human, JSON, SARIF, and LSP including related
locations), and ADR-0024 decision 6 (micro-bench + `reference-perf-gate` budgets):

- **Unit + property tests:**
  - `semver_eval`: differential vectors covering npm `node-semver`, Cargo `VersionReq`, PEP 440
    `requires-python`, partial versions, prereleases, hyphen/`||`/`x` ranges, malformed-input
    rejections, and 1 KiB / 64-clause DoS caps.
  - `no_duplicate_keys`: YAML nested non-leaf duplicates with disjoint children (`mattermost-56`),
    YAML sequence-element duplicates (`zulip-28`), YAML `<<: *base` merge non-false-positive,
    JSONC comments + nested object duplicates, dotenv/properties/INI duplicates, XML build rejection,
    and large-file finding cap.
  - `strict: true`: strict-vs-tolerant cache isolation test in a single run, empty-file rejection,
    JSONC comment/trailing-comma rejection.
  - `records:`, parameterized `transform:`, optional `bound:`, `{{value}}` redaction + bidi escape,
    target-file `set` placement + ambiguous-token fallback, stub locale root groups, composite
    `reference`, `count` (`min`/`max`/`exact`/`max: 0`/`distinct`), multi-field `group_by`,
    per-group `set`, inline `values:`, `compare`, and bundled `version: 2` namespace/alias/direction
    rules (`v1` extending bundled `v2` fails; `v2` extending bundled `v2` with `as:` succeeds;
    remote/nested `v2` still fails).
- **E2E + CLI contract tests:** firing and silent scenarios for `no_duplicate_keys` and every new
  `constraint` family, `alint validate-config` / `list` / `explain` / `export-agents-md` snapshots,
  `test-check-v2-schema-id.sh` in `shell-tests.sh`, and `alint test` fixtures for bundled
  `js/workspace-deps@v2`.
- **Stratum re-score:** binary verification of newly promoted A units across the 20-repo corpus
  stratum, updating `docs/design/general-linter.md` section 12.4.

## 7. Open questions / Decisions for owner sign-off (D1 to D11)

Recorded in [ADR-0050](../../adr/0050-v024-phase-3-completion-design.md) (`status: proposed`) with
recommended defaults ready for owner sign-off before Batch 5a begins:

| ID | Decision | Recommended default |
|---|---|---|
| D1 | `reference` assertion | Single `field:` or composite `fields: [...]` -> `to: { relation, field/fields }`, path-free `baseline_key`, bounded by `DistinctValues` and `FindingsPerRule` (section 3.1) |
| D2 | Unified `count` assertion (ADR-0048 3b) | `count:` with `min`, `max`, `exact`, optional `group_by`, optional `distinct: true` + `field:`; `max: 0` emits per-row findings at offending rows (section 3.2) |
| D3 | Multi-field `group_by`, per-group `set`, inline `set` targets, anti-join | `group_by:` accepts string or `[field, ...]` across `equal`/`set`/`count`; `set` target supports `group_by:` for per-group cross-relation comparison (`backstage-25` set half); inline `{ values: [...] }` target; defer `join: { kind: anti }` (section 3.3) |
| D4 | Negated equality and ordering | Unified `compare: { field, op: eq\|ne\|lt\|le\|gt\|ge, to, order: exact\|lexical\|numeric\|semver }` rather than separate `not_equal:` + `order:` families (section 3.4) |
| D5 | Gap C4 semver range satisfaction | `satisfies:` assertion + owned zero-dep `semver_eval.rs` supporting `node` (default), `cargo`, and `pep440-basic` dialects with partial versions and strict caps (section 3.5) |
| D6 | Gap B1 duplicate keys | Dedicated per-file v1 rule kind `no_duplicate_keys` over YAML (`ayaml` events), JSON (`jsonc-parser` CST), dotenv, properties, INI (XML refused; TOML/HCL parse-rejected) (section 3.6) |
| D7 | Strict-JSON parse option | Opt-in `strict: true` (default `false`) across v1 and v2 JSON surfaces, rejecting comments, trailing commas, and empty files, with isolated cache keys (section 3.7) |
| D8 | #592 array-of-objects, transforms, mixed `union:` | Dedicated `records:` reader (`at:` + `{ field: "$.sub" }`), parameterized non-regex `transform:` (`split`, `strip_prefix`, `strip_suffix`, `prefix`, `suffix`), and per-part `fields:` in `union:` (section 3.8) |
| D9 | #593, #594, #595, #598, #570 | Map-form `bound: { bound, optional: true }` (#593); redacted/escaped `{{value}}` (#594); token-substituted target-file `set` placement (#595); stub `entry.root` empty groups (#598); `check-v2-schema-id.sh` preflight gate (#570) (section 3.9) |
| D10 | DEC-8 bundled `version: 2` (ADR-0048 3c) | `SourceLocator::{Local, Bundled}`, open `alint://bundled/...` for v2 (remote/nested stay refused), enforce `source <= consumer`, ship opt-in `js/workspace-deps@v2` (section 3.10) |
| D11 | Schema `$id`, kind count, and v2 soak | Advance `RELEASE = "v0.24.0"` in `gen_schema_v2.rs`, keep `constraint` as 1 kind (`no_duplicate_keys` -> 105 canonical kinds), update experimental warning to `v0.25 at the earliest` (section 3.11) |
