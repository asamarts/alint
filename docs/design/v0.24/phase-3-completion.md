# Design doc: Phase 3 completion (`version: 2` operators, `#592` to `#598`, B1, C4, strict JSON, and bundled `version: 2`)

Status: Accepted design (2026-11-24, [ADR-0050](../../adr/0050-v024-phase-3-completion-design.md)).
Decisions: [ADR-0050](../../adr/0050-v024-phase-3-completion-design.md) (accepted, D1 to D11), implementing [ADR-0048](../../adr/0048-v024-owner-decisions.md) decisions 3a, 3b, and 3c.
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
     and duplicate keys inside workflow sequences (`steps: [{ name: a, name: b }]` in
     `mattermost-56`), as well as general OpenAPI/YAML duplicate mapping keys (`zulip-28`). Phase
     2's `ayaml 0.2.3` spanned event stream now makes exact document-wide duplicate-key detection
     straightforward without `saphyr`.
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
bounds, baseline identity, and test gates for all 11 Phase 5 decisions (D1 to D11).

## 2. What binds the design

Ground-truth facts verified against `main` (`ccb34ffb4`) after Phases 0 to 4:

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
  - `PlanSource` (`plan.rs:149`), `SourceId` (`v2/model.rs:104`), `Alias` (`v2/model.rs:116`),
    `Objects::record_extended` (`v2/model.rs:45`), and `FIELD_ORIGINS_MARKER`
    (`v2/origins.rs:15-44` indexing into `Config::config_files: Vec<PathBuf>`) currently identify
    v2 sources exclusively by on-disk `PathBuf`. Bundled `version: 2` rulesets have no on-disk
    `PathBuf` and must NOT be inserted into `Config::config_files` (which `check.rs` and `fix.rs`
    treat as on-disk self-edit protection paths), so they require a typed `SourceLocator` enum plus
    a dedicated `v2_sources: Vec<SourceLocator>` origin table.
- **Current v2 DSL and `constraint` surface (`crates/alint-dsl/src/v2/`, `crates/alint-rules/src/constraint/`):**
  - `FIELD_KEYS` (`v2/decode.rs:50`) currently defines `&["source", "sensitive"]`, validated by
    `schema_parity.rs:246-249`. Existing `bound:` syntax is bare `"bound: $.name"` or
    `{ source: "bound: $.name", sensitive: true }` (`decode.rs:1170-1176` explicitly refuses
    `{ bound: ... }`). All new field metadata (`optional`, `on_missing`) and `records:` sources
    extend this single unified `FIELD_KEYS` model.
  - `Rows` has `Part(Part)`, `Union(Vec<Part>)` (sharing one top-level `fields:` map), and
    `Join(Join)` (inner only, one side declared `unique: left | right`).
  - `Reader` has `Entries(Entries)` (`format: json | yaml | toml`, `at: Vec<String>`, `leaves: bool`)
    and `Read { read, optional }` (`json`, `yaml`, `toml`, `regex`, `lines`, `whole_file`).
  - `FieldSource` has `Path(PathToken)`, `EntryKey`, `EntryValue`, `EntryParent`, `EntryRoot`,
    `EntryPath`, `Value`, `Bound(String)`, and `Side(JoinSide, String)`.
  - `Transform` (`v2/model.rs:305`) has 5 fixed zero-argument transforms (`trim`, `lower`,
    `semver-major`, `semver-minor`, `strip-root`); `StringTransform` (`relation/transform.rs:5`) is
    currently `Copy` and shared with `cross_file::normalize`.
  - `Assertion` (`constraint/mod.rs:63`) has `Unique`, `EqualGroups` (single `group_by`),
    `EqualSource`, `EqualField`, and `Set(SetAssertion)` (single `group_by`, target = `Group(Value)`
    or `Relation { relation, field }`, mode = `equal_to | subset_of | disjoint_from`, optional
    `groups:` and `plurals: cldr`).
  - Every `constraint` finding uses a length-prefixed `values::Key` (`<len>:<part>`,
    `Key::value` with `SENSITIVE_PART = "sensitive"` and `KEY_TEXT_BYTES = 128` SHA-256 hashing,
    `Key::row(own, hide_keys)` per ADR-0037 decision 3), and passes through `Run::push`
    (`constraint/families.rs:429-470`) so `LimitKind::FindingsPerRule` applies uniformly.
- **Bundled loader (`crates/alint-dsl/src/loader.rs`, `bundled.rs`, `xtask/src/docs_export/rulesets.rs`):**
  - `decode_bundled` (`loader.rs:1954`) decodes bundled YAML as v1 only and rejects `version: 2`.
  - `v2/decode.rs:293-303` and `xtask/src/gen_schema_v2.rs:873-885` currently refuse `as:` on
    `alint://bundled/...`.
  - `bundled.rs:200-227` has a unit test deserializing every bundled ruleset directly as v1
    `alint_core::Config` (which updates when opt-in `@v2` rulesets are registered).
  - `xtask/src/docs_export/rulesets.rs` currently hardcodes `@v1` across generated bundled-ruleset
    docs pages and must derive `@v<rev>` from each ruleset's canonical id.
  - ADR-0022's version direction rule (`source <= consumer`) must remain unbroken: a `version: 1`
    config extending a `version: 2` bundled ruleset must fail with a user config error (exit 2,
    never `Error::Internal` exit 3).
- **Structured parse cache (`crates/alint-core/src/structured_cache.rs`, `structured_format.rs`, `decode_cache.rs`):**
  - Phase 2 (#548) caches parsed `SharedValue` trees by `(content_hash, Format)` in
    `StructuredCache` / `FileStructuredCache` and decoded relation/extract tables in `DecodeCache`.
    Strict-JSON mode MUST be carried in all three cache keys so a JSONC-tolerant rule cannot poison
    a strict-JSON rule in the same run (or vice versa).
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
       list-order property (orthogonal to set equality; noted honestly when re-scoring
       `backstage-25`).
    Orthogonal multi-field `group_by: [...]` across `equal`, `set`, and `count` (since `unique`
    already takes composite `key: [...]` and `reference` takes composite `fields: [...]`) also
    collapses `ghost-02` from 5 `key_parity` rules into 1 v2 `constraint` rule and enables
    per-manifest grouped `set` (`fluentui-24`, `trpc-07`).

## 3. Accepted design (D1 to D11)

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
    skip_missing: false           # optional boolean, default false
```

**Semantics:**
- `field:` and `fields:` are mutually exclusive on both the source side and `to:`; arity of source
  `fields` MUST equal arity of `to.fields` (checked at config load time).
- Evaluates `to.relation`, builds a hash set of target key tuples (`Vec<Value>` of length `k`),
  bounded by `LimitKind::DistinctValues` (and `LimitKind::BytesPerRun`). Target rows where ANY
  key column is `Null` or `Missing` are skipped when building the target set (matching `set.rs`);
  mixed scalar types within a target key column fail loud (matching `set.rs`).
- For every row in the input relation:
  - If any source key field is `Missing` (for example from an optional `bound:` with
    `on_missing: missing` or optional `record:`): skip the row only if `skip_missing: true`
    (default `false`); by default a `Missing` or `Null` key field in `reference` emits a finding
    naming the unresolved field.
  - If the source key tuple is NOT in the target set, emit one finding through `Run::push`
    anchored at the first source key field's span (with the source row's primary resource path).
  - Type safety: key comparison uses the relation layer's typed `Value` equality (`1` != `"1"`). If
    every target key in column `j` has scalar type `T_target` and a source key in column `j` has a
    different non-null scalar type `T_src`, emit a typed mismatch analysis failure (matching `set`
    and `equal`'s fail-loud type discipline).
- **Finding identity (`baseline_key`):** ADR-0037 length-prefixed `values::Key`, path-free and
  alias-stable:
  `Key::new("reference")` followed by each source key value via `Key::value(v, sensitive[j])`
  (omitting the written target relation name so local `extends:` alias renames never churn
  baselines; repeated identical dangling key tuples are disambiguated by occurrence via
  ADR-0047/ADR-0049).
- **Required v2 plumbing:**
  - `v2/origins.rs` (`nested_reference_fields`): register `("assert", &["reference", "to", "relation"])`.
  - `v2/literal.rs` (`ASSERT_NAME_KEYS`): register `"fields"`.
  - `v2/resolve.rs` (`close_compared_sensitivity`): propagate sensitivity across `fields[j]` and
    `to.fields[j]` so comparing against a sensitive target column marks the source column sensitive.
  - `constraint/mod.rs`: register `reference.to.relation` in rule `inputs` and `TriggerScope` so
    `--changed` runs whenever files in the target relation change.
- **Finding cap:** governed by `LimitKind::FindingsPerRule` (default 10,000; #596) via `Run::push`.
- **Stratum impact:** unlocks single-key (`forem-29`, `jest-28` [note: `selectors:` match files
  only, so `website/versioned_docs/version-*/*` covers non-empty Docusaurus version directories],
  `nextcloud-b02` with D8 prefix strip) AND composite-key (`babel-17`, `trpc-10`, `backstage-26`,
  `rushstack-02`, `yarnpkg-berry-17`, plus `fluentui-24` and `trpc-07` via
  `(manifest, dep) -> (manifest, dep)`).

### 3.2 D2: Unified `count` assertion (folding `cardinality` and `bounded-count`)

**Syntax (under `kind: constraint`, `assert:`):**

```yaml
assert:
  count:
    group_by: manifest            # optional: string or non-empty list of field names; omitted = whole relation
    field: dependency             # optional: required iff `distinct: true`; refused when `distinct: false`
    distinct: false               # optional boolean, default false (count rows vs distinct non-null/present values)
    min: 1                        # optional non-negative integer
    max: 0                        # optional non-negative integer (0 asserts emptiness)
    exact: 1                      # optional non-negative integer (mutually exclusive with min/max)
    groups: [en, fr]              # optional expected group keys (scalars for 1-field, k-arrays for k-field group_by)
```

**Semantics:**
- At least one of `min`, `max`, or `exact` MUST be set; `exact` is mutually exclusive with `min`
  and `max`; when both `min` and `max` are set, `min <= max` is enforced at load time.
- `field:` is required when `distinct: true` and refused at load time when `distinct: false`
  (omitted); `field` must not appear in `group_by`.
- `groups:` requires `group_by:`, accepts scalars when `group_by.len() == 1` and fixed-length
  arrays of length `k` when `group_by.len() == k` (matching D3 `set`), and is refused at load time
  unless `min > 0` or `exact > 0` (mirrors `set` refusing `groups:` on `subset_of`/`disjoint_from`).
- When `distinct: true`: counts distinct non-null, present values of `field` (ignoring `Null` and
  `Missing`, matching `set`), bounded by `LimitKind::DistinctValues`.
- **Emptiness special case (`max: 0` or `exact: 0`, both ungrouped and grouped):**
  - Emits one finding per offending row through `Run::push` (capped by `FindingsPerRule`), anchored
    at that row's file and span, so emptiness checks (`element-web-06`, `babel-12`, `ghost-63`)
    point directly at every offending site rather than one opaque relation-level summary. When
    `distinct: true`, emits per-row findings for rows carrying a non-null, present `field` value.
- **Aggregate count violations (`count < min`, `count > max` where `max > 0`, or `count != exact`
  where `exact > 0`):**
  - Emits one finding per failing group (or one relation-level finding when ungrouped), anchored at
    the group's first row, or `Place::Resource` when a zero-row group comes from `resource_groups`
    or `stub_rows`, or `Place::Nowhere` when a zero-row group has no resource (matching `set.rs`).
    Attaches up to `MAX_RELATED_LOCATIONS = 20` sample row spans as `related_locations` (listing up
    to `LISTED_VALUES = 5` sample values in message text).
- When every field in `group_by:` derives from a path token (or `entry.root` under D9/#598): every
  resource matched by the input relation's selector (and surviving single-root stub) forms a group
  even with 0 rows, so `count: { group_by: path, min: 1 }` fires on empty files.
- **Finding identity (`baseline_key`):** ADR-0037 length-prefixed `values::Key`:
  - For `max: 0` / `exact: 0` per-row findings: `Key::new("count_empty")` +
    `Key::row(self.own(row), self.hide_keys)` (matching `equal_source`/`equal_field`, never leaking
    sensitive non-key fields).
  - For aggregate count violations: `Key::new("count")` + `Key::part(bound_kind)` (`min` | `max` |
    `exact`) + each group key component via `Key::value(v, sensitive[j])` when grouped.

### 3.3 D3: Orthogonal multi-field `group_by`, per-group cross-relation `set`, inline literal `set` targets, and anti-join stance

1. **Orthogonal multi-field `group_by` across `equal`, `set`, and `count`:**
   - Wherever `group_by:` accepts a single field name string (`group_by: locale`), it ALSO accepts
     a non-empty list of distinct field names (`group_by: [locale, namespace]`), normalized
     internally to `Vec<String>`. Rejects `field` appearing inside `group_by` (extending
     `constraint/check.rs`).
   - ADR-0048 decision 3b refinement: `unique` already accepts composite `key: [...]` and
     `reference` accepts composite `fields: [...]`, so orthogonal `group_by: [...]` applies across
     `equal`, `set`, and `count`.
   - Group keys compare and sort lexicographically by tuple `(v_0, v_1, ..., v_{k-1})`. A 1-element
     list `group_by: [f]` serializes identically to scalar `group_by: f` in `values::Key` so
     existing v0.23 baselines never churn.
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
     list of the same arity as the input's `group_by:`) and optional `all_target_groups: true`
     (valid ONLY on `equal_to`, default `false`, refused at load time on `subset_of` and
     `disjoint_from`):
     ```yaml
     assert:
       set:
         group_by: plugin_id
         field: plugin_package
         equal_to:
           relation: workspace-plugins
           field: name
           group_by: plugin_id
           all_target_groups: false   # optional, equal_to only, default false
     ```
   - Semantics: partitions the target relation `R` by `to.group_by` into a map
     `group_key -> Set<Value>`. `LimitKind::DistinctValues` bounds the **total** distinct values
     summed across all target groups (plus `BytesPerRun`), not per-group. Each input group `G`
     compares against `R[G]` (empty set if `G` is absent from `R`; for `equal_to`, target groups
     absent from input also report missing values when `groups:` names them or
     `all_target_groups: true`).
   - `equal_to`, `subset_of`, and `disjoint_from` all work symmetrically on per-group target sets.
   - `plurals: cldr` remains restricted to `{ group: ... }` targets (refused at load time on
     `{ relation, ... }` and `{ values: [...] }`).
   - Sensitivity propagation (`v2/resolve.rs::close_compared_sensitivity`) covers both `field` <->
     `to.field` AND `group_by[j]` <-> `to.group_by[j]`.
   - **Honest note on `backstage-25`:** per-group `set` expresses exact membership equality between
     `backstage.pluginPackages` and sibling packages sharing `pluginId`; checking that the JSON
     array is also lexicographically sorted remains a separate ordering concern (documented honestly
     in the Phase 5 stratum re-score).
3. **Inline literal `set` targets:**
   - `set` targets (`equal_to:`, `subset_of:`, `disjoint_from:`) accept `{ values: [<literal>, ...] }`
     (non-empty list, `minItems: 1`, strings and booleans only -- matching `where: { in: [...] }`,
     no YAML number or `null` literals) in addition to `{ group: ... }` and
     `{ relation: ..., field: ... }`:
     ```yaml
     assert:
       set:
         group_by: file
         field: key
         subset_of: { values: [id, title, description, tags] }
     ```
   - Environment-sourced literals trigger `hide_literals` redaction (including `{{value}}` in D9).
   - **Impact:** cleanly resolves `ghost-63` allowed-field checks without a synthetic relation.
4. **Anti-join stance (deferred -- no `join: { kind: anti }` in v0.24):**
   - Every stratum anti-join need (`fluentui-24`, `trpc-07`, `babel-12`, `wsdk-09`) is already
     covered more simply and declaratively by `reference` (D1), `count: { max: 0 }` over an inner
     `join:` (D2), per-group `set: { subset_of / disjoint_from }` (D3), and
     `where: { not_in: [{ relation, field }] }` (v0.23).
   - Adding a fourth join type (`anti` / `left`) would expand the join planner and `JoinInputs`/
     `JoinOutputs` surface without unlocking any stratum unit beyond D1 + D2 + D3. Anti/left joins
     therefore stay out of scope for v0.24.

### 3.4 D4: Negated equality and ordered comparison (`compare`)

**Syntax (under `kind: constraint`, `assert:`):**

```yaml
assert:
  compare:
    field: value
    op: ne                        # eq | ne | lt | le | gt | ge
    to:
      field: key                  # row-level: compare two fields on the same row
      # OR: value: "latest"       # compare against a literal (string/bool for exact/lexical/semver; finite number for numeric)
      # OR: row: { source: .nvmrc } # compare against a single reference row (like `equal`)
    order: exact                  # optional: exact (default; eq/ne only) | lexical | numeric | semver
```

**Semantics:**
- `order: exact` (default): valid ONLY with `op: eq | ne` (using `lt | le | gt | ge` with
  `order: exact` is rejected at load time). Uses exact typed `Value` equality (`1 != "1"`) across
  scalar values. `to.value` accepts string and boolean literals only (matching `where:` /
  `plan_check.rs`, because YAML does not preserve numeric lexemes).
- `order: lexical`: compares `Value::String` by UTF-8 code-point order (fails loud with a typed
  analysis failure on non-strings); `to.value` must be a string.
- `order: numeric`: compares `Value::Number` parsed as finite `f64` (fails loud with a typed
  analysis failure on strings, booleans, or non-finite numbers -- no implicit string coercion so
  string/number bugs never hide!). `to.value` accepts finite YAML number literals (rejecting
  `NaN`/`inf` at load time).
- `order: semver`: parses both sides (`Value::String`) as semver versions using D5's partial-version
  normalizer and compares by SemVer 2.0.0 precedence (prerelease < release; build metadata ignored,
  so `"22"` == `"v22.0.0+build"` under `eq`). Fails loud on non-strings or unparseable versions.
- **`Null` and `Missing` operands:** across ALL `compare` ops (`eq`, `ne`, `lt`, `le`, `gt`, `ge`),
  a `Null` or `Missing` operand emits a typed analysis failure (preventing silent false passes like
  `null != "prod"` and forcing explicit `where:` filtering for nullable/optional fields).
- Existing `equal:` keeps 100% backwards compatibility (`equal:` also covers group-wide uniformity
  `equal: { field: version, group_by: ... }`, which is a multi-row equivalence check rather than a
  pairwise `compare:`).
- **Finding placement and `baseline_key`:** emitted via `Run::push`, anchored at `field`'s row/span
  (with `to.field` or `to.row` attached as a `related_location` when present); row-keyed ADR-0037
  `values::Key`: `Key::new("compare")` + `Key::part(op)` + `Key::row(self.own(row), self.hide_keys)`
  (mirroring `equal_source` / `equal_field` so editing a failing value does not churn baselines and
  sensitive values never enter baselines).
- **Impact:** cleanly unlocks `element-web-05` (`compare: { field: value, op: ne, to: { field: key } }`),
  `thunderbird-android-55`, and all version-floor / version-ordering `toolchain-pin` units.

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

**Load-time validation:**
- Exactly one of `version` / `version_literal` and exactly one of `range` / `range_literal` must be
  set; setting BOTH `version_literal` and `range_literal` is rejected at load time (no input field
  would be read). Register `"version"` and `"range"` in `v2/literal.rs::ASSERT_NAME_KEYS`.

**Version and range parsing rules (100% offline, deterministic, zero-shell):**
- **Implementation choice:** own a small, pure-Rust, zero-unsafe module `crates/alint-core/src/semver_eval.rs`
  (~250 to 350 LOC, zero new third-party dependencies, `cargo-deny`-clean, MSRV 1.88 compatible).
- **Partial version normalization:** strips optional leading `v`/`V`; parses
  `MAJOR[.MINOR[.PATCH[-PRE][+BUILD]]]` with missing `MINOR`/`PATCH` filled as `0` for concrete
  versions (`22` -> `22.0.0`, `22.13` -> `22.13.0`) while preserving partial semantics in range
  comparators (`^22` -> `>=22.0.0 <23.0.0`, `~22.13` -> `>=22.13.0 <22.14.0`, `22.x` ->
  `>=22.0.0 <23.0.0`).
- **Pinned dialect semantics:**
  - `node` (default): npm/node-semver ranges -- `||` union, whitespace-separated AND comparators
    (strict `node-semver` syntax; commas rejected in `node` dialect so typos don't hide), empty
    range `""` -> `*`, `=` (including `=v1.2.3`), `>`, `>=`, `<`, `<=`, `^`, `~`, hyphen `A - B`,
    `x`/`X`/`*`, leading `v`. Prerelease matching when `include_prerelease: false`: a prerelease
    version matches a `||` clause ONLY if at least one comparator in that clause has the exact same
    `(major, minor, patch)` tuple AND carries a prerelease tag (exact `node-semver` contract).
  - `cargo`: Cargo `VersionReq` comma-separated AND semantics (`^0.0.3` = `>=0.0.3, <0.0.4`,
    `^0.2.3` = `>=0.2.3, <0.3.0`, `^0.0` = `<0.1.0`, `~1` = `>=1.0.0, <2.0.0`,
    `~1.2` = `>=1.2.0, <1.3.0`, bare `1.2.3` = `^1.2.3`).
  - `pep440-basic`: comma-separated Python `requires-python` comparators (`>=`, `<=`, `>`, `<`,
    `==`, `!=`, `~=` compatible release where `~=3.10` = `>=3.10.0, <4.0.0` and
    `~=3.10.2` = `>=3.10.2, <3.11.0`, plus `==3.10.*` / `!=3.10.*` wildcards). Explicitly fails
    loud on unsupported PEP 440 features (epochs `1!`, `.post`, `.dev`, arbitrary equality `===`).
- **Bounds and errors:**
  - Range string capped at `MAX_RANGE_BYTES = 1_024`, at most `64` `||` clauses and `64`
    comparators per clause (preventing crafted range DoS).
  - `Null`, `Missing`, non-string, or unparseable version/range produces an **analysis failure**
    naming the field and resource (never a silent pass!), unless the caller pre-filters non-semver
    specifiers (`workspace:`, `catalog:`, `git+https:`) with `where: { not_matches: ... }`.
- **Finding identity:** emitted via `Run::push`, anchored at the `version` field's span (or `range`
  span when `version_literal` is used, attaching the other field's span as a `related_location`),
  row-keyed ADR-0037 `values::Key`: `Key::new("satisfies")` +
  `Key::row(self.own(row), self.hide_keys)`.
- **Stratum impact:** directly unlocks 21 stratum units (moving `toolchain-pin` from 63% to 83%).

### 3.6 D6: Gap B1 -- General duplicate-key detection (`no_duplicate_keys`, v1 rule kind)

**Why a dedicated v1 rule kind instead of a v2 `entries:` mode?**
1. Duplicate-key detection is a single-file, per-file invariant ("no file matching `*.yml` has
   duplicate keys anywhere, at any depth, including inside arrays/sequences"). Making it a
   first-class per-file v1 rule kind (`DispatchClass::PerFile`, headline canonical kinds
   `104 -> 105`) means:
   - it works in BOTH `version: 1` and `version: 2` configs (constitution invariant / section 12.1:
     single-file policies keep a concise 5-line v1 spelling);
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
       strict: false              # optional boolean (json only; D7)
       level: error
   ```
   - Omitted `format:` auto-detects from file extension; unsupported or unknown extensions (e.g.
     `.xml`, `.txt`) emit a `resource_parse` analysis failure (matching `json_schema_passes`).
   - Explicit `format: xml` is rejected at config build time (XML sibling elements legitimately
     repeat; duplicate XML attributes are already parse errors in `roxmltree`).
   - `strict: true` is rejected at config build time when `format:` is explicitly non-JSON.
3. **Supported formats and exact semantics:**
   - `yaml`: walks `ayaml 0.2.3`'s spanned event stream directly with a stack of seen-key sets (one
     per open mapping; popped on `MappingEnd`, reset at each `DocumentStart` so multi-document
     `---` Kubernetes manifests work cleanly without false cross-document collisions). Uses YAML
     1.2 Core Schema typed scalar keys via `ayaml`'s `ScalarStyle`/`Tag` so `1:` vs `"1":` and
     `true:` vs `"true":` are distinct while `foo:` vs `"foo":` collide. Catches duplicate scalar
     keys at EVERY depth -- including non-leaf mapping keys with disjoint children (`jobs: { a: 1 }`
     / `jobs: { b: 2 }`) AND mappings nested inside sequences (`steps: [{ name: a, name: b }]`).
     Complex (mapping/sequence) keys skip duplicate comparison cleanly without false positives;
     an explicit key overriding a `<<: *base` merge key is NOT a duplicate (two literal `<<:` keys
     in the same mapping ARE flagged); aliased scalar keys (`*k`) are resolved or skipped cleanly
     without false positives. Honors `ayaml`'s 32 MiB byte cap, nesting cap, and alias/parse
     bounds. Reports exact 1-based line and column of the repeated key, with `related_location`
     pointing to the first occurrence.
   - `json`: walks `jsonc-parser` CST (already a direct dependency of `alint-core`), tracking keys
     per `Object` node at every depth (including objects inside arrays), with exact byte-offset ->
     1-based line/column spans for both the duplicate and the first occurrence. Respects `strict: true`
     (D7) when specified.
   - `dotenv`: parses with `adotenv 1.6.0` (`parse_strict_with_options`) and walks `doc.entries()`
     tracking seen keys with exact `entry.key_span()` locations (quotes key names only, never
     values).
   - `properties`: walks `crates/alint-core/src/properties.rs` spanned entries tracking seen keys
     (quotes key names only, never values).
   - `ini`: adds a small spanned entry walker in `crates/alint-core/src/ini.rs`; merges repeated
     `[section]` headers into the same section scope (so `[a]\nk=1\n[a]\nk=2` flags duplicate `k`,
     while disjoint keys in repeated `[a]` pass), reporting line/column and first occurrence.
   - `toml` and `hcl`: TOML duplicate keys/tables and HCL duplicate attributes/object keys are
     already rejected at parse time by the format parsers and surface as non-baselinable
     `AnalysisFailureKind::ResourceParse` (confirmed intentional; note HCL blocks such as
     `resource` or `locals` legitimately repeat and are not flagged).
4. **Trust & sensitivity:** `no_duplicate_keys` quotes ONLY key names (never values) on `dotenv`
   and `.properties`, preserving secret safety, and obeys `sensitive: true` when explicitly set.
5. **Limits & failure semantics:** malformed files emit one non-baselinable `resource_parse`
   analysis failure; per-file duplicate findings cap at `MAX_DUPLICATES_PER_FILE` (default `1_000`,
   emitting one analysis failure on breach), plus engine post-dispatch `findings_per_rule` cap so
   a crafted 100k-duplicate repo cannot flood memory or output.
6. **Inventory & test updates:** adds canonical kind #105 (updating `alint-rules/src/lib.rs`
   `metadata.len()` and `metadata_inventory_matches_the_audited_v019_shape`, `docs/rules.md`,
   `facts.json`, and README count audits).
7. **Stratum impact:** promotes `mattermost-56` and `zulip-28` cleanly to A.

### 3.7 D7: Strict-JSON parse option (`strict: true`)

**Problem:** `Format::Json` tries strict `serde_json` first and falls back to `strip_jsonc` on
failure (`structured_format.rs`), which silently accepts `//` comments, trailing commas, and empty
files in files whose runtime parser is strict RFC 8259 `JSON.parse` (keeping 10 locale units in
bucket C across 8 repos).

**Syntax:**
- On v1 JSON rule kinds (`json_path_equals`, `json_path_matches`, `json_path_absent`,
  `json_schema_passes`, `no_duplicate_keys`, `key_parity` when `format: json`): optional
  `strict: true` (default `false`, preserving 100% backwards compatibility). Reject `strict: true`
  at build time on non-JSON formats (updating `structured_family_is_symmetric` for the JSON-only
  `strict` option; in `json_schema_passes`, `strict: true` governs the target file and is rejected
  when `format:` is explicitly non-JSON).
- On v1 `ExtractSpec` (`cross_file`, `file_graph`, `registry_paths_resolve`):
  `extract: { json: "$.x", strict: true }` (relaxing `ExtractSpec`'s `maxProperties` from `1` to
  `2` specifically for `json` + `strict`, rejecting `strict` with any non-`json` key).
- On v2 `entries:` (`format: json`), `records:` (`format: json`), and `read:` (`{ json: ... }`):
  optional `strict: true` (default `false`, rejecting `strict: true` on non-JSON formats; relaxing
  `v2_read`'s `maxProperties` from `1` to `2` for `json` + `strict` and adding
  `(Position::Read, "strict")` to `BOOLEAN_FIELDS`):
  ```yaml
  entries: { format: json, at: "$", leaves: true, strict: true }
  read: { json: "$.name", strict: true }
  ```
- **Valid-JSON-only rule spelling:** a repo that wants to reject invalid/JSONC files even when no
  specific key is asserted can write `no_duplicate_keys` with `strict: true`, `json_path_absent`
  with `strict: true`, or `json_schema_passes` / `entries:` with `strict: true`.
- **Strict RFC 8259 semantics when `strict: true`:**
  - `strip_jsonc` fallback is DISABLED; any `//`/`/* */` comment or trailing comma produces a
    `resource_parse` analysis failure.
  - Empty / whitespace-only files (which tolerant `Format::parse` short-circuits to `{}` before
    dispatching on `Format`) MUST fail under `strict: true` as `resource_parse`.
  - Leading UTF-8 BOM (`\u{feff}{}`) is rejected under `strict: true` (matching RFC 8259 section
    8.1 and Node.js `JSON.parse`; tolerant `strict: false` keeps stripping BOM as before).
- **Cache & span isolation:** `StructuredCache::CacheKey`, `FileStructuredCache`, and `DecodeCache`
  adapter IDs (`EntryRowsAdapter`, `ExtractAdapter`, `RecordsAdapter`) all include `strict: bool`
  (or `ParseMode::Standard | ParseMode::StrictJson`), guaranteeing a tolerant parse of
  `tsconfig.json` never satisfies a strict-JSON rule in the same run. JSON span helpers
  (`relation/eval/spans.rs`) also honor `strict` while staying within the 3-word
  `size_of::<Option<Span>>()` invariant.
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
           at: "$.spec.template.spec.containers[*]"   # JSONPath (string or non-empty list, reusing entries: decode_paths)
           strict: false           # optional, json only (D7)
         fields:
           file: path
           name: "record: $.name"                     # unified FIELD_KEYS syntax (bare string or map)
           image: "record: $.image"
           pull_policy: { source: "record: $.imagePullPolicy", optional: true }
     ```
   - **Why `records:` + unified `source: "record: <subpath>"`?** `entries:` iterates `(key, value)`
     pairs of a mapping (`entry.key`, `entry.value`, `entry.parent`, `entry.root`, `entry.path`),
     whereas an array of objects emits ONE row per matched element with arbitrary sibling
     sub-fields. Using `"record: <subpath>"` / `{ source: "record: <subpath>", optional?: bool,
     sensitive?: bool }` keeps ONE uniform `FIELD_KEYS` map syntax across `bound:` (#593) and
     `record:` (#592).
   - **Semantics:**
     - `at:` (string or non-empty list of JSONPath queries, reusing `entries:`'s `decode_paths`
       helper and schema definition) selects object elements from each resource. A matched
       non-object element is an analysis failure naming the element index (when `at:` matches
       nothing, emits 0 rows for that resource).
     - Each `record:` field evaluates `<subpath>` (a JSONPath rooted at `$` = the element object,
       e.g. `$.name` or `$.metadata.labels.app`) against the element:
       - Must select at most one scalar (`null`, `bool`, `number`, `string`); multi-match or
         non-scalar fails loud (matching ADR-0034 decision 5).
       - If `<subpath>` matches nothing: emits `Cell { value: None, span: None }` (`Missing`) when
         `optional: true`, else analysis failure.
       - Span and number-lexeme preservation: extracts directly from `ayaml::SpannedTree` (YAML,
         honoring `yaml_budget` / `Limits`), `jsonc-parser` CST (JSON), or `toml_edit` (TOML) so
         every extracted record field keeps its exact source span (within `spans.rs`'s 3-word
         `Option<Span>` budget) and preserves numeric lexemes without `serde_json::Value`'s
         `ARRAY_NUMBER` loss.
     - Bounded by `RowsPerExtractor`, `RowsPerRun`, `CellsPerRun`, and `BytesPerRun`.
   - **Impact:** directly resolves `#592` (`trpc-24`, Kubernetes container name+image policies, and
     the array-of-objects stratum units).
2. **Parameterized string transforms (`transform:` in v2 relations):**
   - Extends `Transform` beyond bare names (`trim`, `lower`, `semver-major`, `semver-minor`,
     `strip-root`) to accept single-key parameterized maps (switching `relation::StringTransform`
     from `Copy` to `Clone` while keeping `cross_file::normalize` clean):
     - `{ split: { on: ":", part: 0, required: false } }` -- splits string by non-empty literal
       separator `on:` (empty `on: ""` rejected at load time) and selects 0-based `part:` (negative
       indices count from the end: `-1` = last segment, e.g. tag in `ghcr.io/org/app:1.2.3`;
       `part: 0` and `part: -1` on separator-less `"nginx"` both return `"nginx"`). When `part` is
       out of range (e.g. `part: 1` on `"nginx"`): emits a `Missing` cell when `required: false`
       (default), or emits an analysis failure when `required: true`.
     - `{ strip_prefix: "$" }` -- strips literal prefix once if present (no-op if absent).
     - `{ strip_suffix: ".singular" }` (or non-empty list `strip_suffix: [".singular", ".plural"]`;
       empty list rejected at load time) -- strips the first matching literal suffix once if present
       (no-op if absent).
     - `{ prefix: "v" }` / `{ suffix: ".json" }` -- prepends/appends a literal string (useful for
       `jest-28` `"version-" + ver` or `forem-29` variant filenames).
   - All transforms are pure, linear-time `O(len)`, deterministic, non-regex (zero ReDoS surface),
     and charge output length against `BytesPerRun`.
   - **Impact:** directly resolves `#592` (`repo:tag`, `nextcloud-b02` `$<name>`, `strapi-21`
     `.singular`/`.plural`, `jest-28` `version-X`, `forem-29`).
3. **Mixed-reader `union:` (per-part `fields:`):**
   - Currently `union:` requires all parts to share one top-level `fields:` map, blocking
     `mattermost-27` (`entries:` `entry.key` unioned with `read:` `$.*.aliases[*]` `value`).
   - Allow either top-level `fields:` (inherited by all parts, 100% backwards compatible) OR
     per-part `fields:` on every part (specifying BOTH top-level `fields:` and per-part `fields:` is
     rejected at load time).
   - All parts MUST bind the exact same set of output field names and matching `sensitive` flags
     (checked at load time), normalized to the first part's field order so `RelationRows` positional
     cell indices align cleanly.
   - Required v2 plumbing: update `v2/decode.rs` (`Position::Part, Some("fields") => Position::Fields`),
     `v2/literal.rs` (check `part.get("fields")`), `v2/environment.rs` (propagate `Family`
     sensitivity per part), `v2/resolve.rs::close_sensitivity` (per part), `v2/plan_check.rs`
     (`strip-root` verifies every part binds `entry.path`), and `xtask/src/gen_schema_v2.rs`
     (move `required: ["fields"]` into `oneOf` branches when `union:` uses per-part `fields:`).

### 3.9 D9: Issues #593, #594, #595, #598, and #570

1. **#593 -- Optional `bound:` queries:**
   - Extend `FIELD_KEYS` (`v2/decode.rs`) from `&["source", "sensitive"]` to
     `&["source", "sensitive", "optional", "on_missing"]` (and add `(Position::Field, "optional")`
     to `BOOLEAN_FIELDS`, keeping `schema_parity.rs` in sync):
     ```yaml
     fields:
       pkg_name: { source: "bound: $.name", optional: true }
       # Optional explicit mode (default: skip):
       alt_name: { source: "bound: $.name", optional: true, on_missing: missing }
     ```
   - Load-time validation: `optional:` and `on_missing:` are accepted ONLY on `bound:` (and in
     Batch 5b `optional:` on `record:`), and `on_missing:` is rejected unless `optional: true`.
   - Semantics when `optional: true` and the `bound:` query matches nothing on a resource:
     - `on_missing: skip` (default): skips the resource cleanly -- emits **no rows** for that
       resource and excludes it from `resource_groups` (matching `read: { ..., optional: true }` so
       manifests without the bound metadata cleanly opt out without failing the relation).
     - `on_missing: missing`: keeps the resource's rows and emits `Cell { value: None, span: None }`
       (`Missing`), aligning directly with `read:`'s `Missing` cell and D1 `reference`'s
       `skip_missing: true`.
     - Multi-match (`values.len() > 1`) and non-scalar bound results remain analysis failures.
     - Default (`optional: false`): unchanged from v0.23 (analysis failure on missing bound).
2. **#594 -- `{{value}}` placeholder in `constraint` custom `message:`:**
   - Supported on `set` (`missing`, `extra`, `shared`) and `reference` findings (accepting optional
     inner whitespace like `{{ value }}`).
   - Implementation: `alint_core::template::render_message` requires a `.` namespace (`row.FIELD`),
     so `constraint/families.rs::Run::render` expands bare `{{value}}` via a small `constraint`-local
     helper alongside `render_message`.
   - Load-time validation (`constraint/check.rs::message`): keeps `{{row.FIELD}}` validation and
     explicitly rejects `{{value}}` on assertions that have no single compared value (`unique`,
     `count`, `equal`, `compare`, `satisfies`) with a clear diagnostic naming the allowed
     placeholders. Arbitrary non-alint `{{...}}` tokens remain untouched (preserving literal i18n
     messages such as `"missing {{name}} placeholder"`).
   - Formatting pipeline:
     1. If the compared field is `sensitive: true`, OR the rule/relation reads the environment
        (`reads_environment`), OR inline literals are hidden (`hide_literals`), render `<redacted>`.
     2. Otherwise format the scalar value via `Scalar::text_shown()` (`SHOWN_CHARS = 64` and
        `values::escaped()` in `constraint/values.rs`, tested against
        `alint_core::invisible::is_deceptive_on_display`), or for composite `reference` join
        formatted tuple elements by `, `.
3. **#595 -- Target-file placement for missing-value `set` findings:**
   - Retain `resources: BTreeSet<&'r RelPath>` on `SetGroup<'r>` alongside `first: Option<&'r RelPath>`
     (populated from both rows and `resource_groups`).
   - When `set` reports "group `G` lacks value `V`", `theirs: Option<&FieldRow>` provides reference
     path `P_ref`, AND `group_by` derives from a `PathToken` (`ResourceGroups`):
     - Compute structural path-component counterparts in `G.resources` matching the non-token path
       components of `P_ref` (for `parent_name`: same `(grandparent_dir, basename)`; for `stem`:
       same `(dir, ext)`; for `dir`: same `basename`; for `ext`: same `(dir, stem)`; for
       `basename`: same `dir`). This eliminates `en/en.json` ambiguity and works automatically under
       case-folding transforms such as `lower` (`locales/EN/common.json` -> `locales/fr/common.json`).
     - If **exactly one** resource in `G.resources` matches (`matches.len() == 1`), anchor at
       `Place::Resource(matched.as_path())` (file-level, no `1:1`).
     - Otherwise (ambiguous match, missing target file, `entry.root` `group_by`, or cross-relation
       target with a different layout) fall back to
       `group.first.map_or(Place::Nowhere, |p| Place::Resource(p.as_path()))`.
   - **Baseline stability:** `baseline_key` for `constraint` findings ALREADY excludes the primary
     path (ADR-0037 decision 3), so improving placement from `group.first` to the structural
     counterpart breaks ZERO existing v2 baselines.
4. **#598 -- Stub locale root naming its group under `group_by: entry.root`:**
   - `ops.rs::take_stub_roots` already extracts empty single-segment root rows (`fr:`, `fr: {}`,
     `fr: []`) under `entries: { leaves: true }` + `strip-root`, validates single-root per file
     (a multi-root stub file such as `fr: {}\nde: {}` remains an analysis failure per ADR-0037
     decision 2), and currently drops them.
   - Preserve surviving single-root stubs on `RelationRows::stub_rows: Vec<FieldRow>` (bounded by
     `RowsPerRun` and `CellsPerRun`):
     - Run non-`entry.path` transforms (e.g. `lower`/`trim` on `entry.root` and path tokens) on
       `stubs`.
     - Filter `stubs` through `where:` clauses that reference only `entry.root` / path-token fields
       (matching `resources_kept` in `ops.rs:607-652`) so a filtered-out `test: {}` stub never forms
       a spurious failing group.
   - In `constraint/set.rs` and `count`: consult `rows.stub_rows` ONLY when `group_by` derives from
     `FieldSource::EntryRoot` (and path tokens for composite `group_by`), seeding zero-row groups
     anchored at `Place::Resource(stub.resource.path())` both with and without `groups:`.
5. **#570 -- Release preflight gate tying `schemas/v2/config.json`'s `$id` to the release tag:**
   - Add `ci/scripts/check-v2-schema-id.sh` (plus hermetic temp-repo regression suite
     `ci/scripts/test-check-v2-schema-id.sh` wired into `ci/scripts/shell-tests.sh`) with two
     explicit modes:
     - **Default / `--dev` mode (invoked in `ci/scripts/preflight.sh`):** verifies internal
       consistency on every commit without demanding a future release tag during development:
       1. `schemas/v2/config.json`'s `$id` matches `xtask/src/gen_schema_v2.rs::RELEASE`;
       2. both documented YAML modelines (`docs/site/configuration/version-2.md` and
          `docs/development/CONFIG-AUTHORING.md`) match that exact `$id` URL.
     - **`--release <tag>` mode (invoked in `.github/workflows/release.yml` `preflight` with
       `--release "$GITHUB_REF_NAME"`):** runs the internal consistency checks PLUS finds the latest
       prior `v*.*.*` tag strictly excluding `<tag>`
       (`git tag -l 'v*.*.*' --sort=-v:refname | grep -Fxv "$TAG" | head -n1`), normalizes `$id`
       out of both `<prev_tag>:schemas/v2/config.json` and working-tree `schemas/v2/config.json`,
       and fails if the schema body changed while `RELEASE != "$TAG"`. If unchanged since the prior
       tag, passes without forcing a spurious `$id` bump.
   - Synchronize `ci/scripts/bump-version.sh` so release version bumps advance
     `gen_schema_v2.rs::RELEASE`, `gen_schema_v2_tests.rs`, `schemas/v2/config.json`, and the two
     docs modelines together, while staying compliant with `ci/scripts/test-release-preflight-parity.sh`.

### 3.10 D10: DEC-8 bundled `version: 2` rulesets (ADR-0048 decision 3c)

**Loader, origin-table, and namespace architecture:**
- Replace raw `PathBuf` v2 source identity in `crates/alint-dsl/src/v2/model.rs` (`SourceId`,
  `Alias::target`, `SourceObjects::extended`) and `crates/alint-core/src/relation/plan.rs`
  (`PlanSource` inside `#[doc(hidden)] pub mod relation`, so zero documented public API break) with:
  ```rust
  #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
  pub enum SourceLocator {
      Local(PathBuf),
      Bundled(String), // canonical bundled id, e.g. "alint://bundled/js/workspace-deps@v2"
  }
  ```
- **Dedicated `v2_sources: Vec<SourceLocator>` origin table:** `FIELD_ORIGINS_MARKER`
  (`v2/origins.rs`) and `resolved.resolve_rule(...)` index into a dedicated
  `v2_sources: Vec<SourceLocator>` table in `LoadState` / `RawConfig` instead of
  `Config::config_files: Vec<PathBuf>`, so `Config::config_files` stays strictly on-disk paths for
  `alint fix` / `alint lsp` self-edit protection while bundled `constraint` rules resolve their v2
  field origins cleanly.
- **Loader, schema, and docs-export updates:**
  - `v2/decode.rs` and `xtask/src/gen_schema_v2.rs`: allow `as: <alias>` on `alint://bundled/...`
    (still refusing `as:` on `https://...`).
  - `loader.rs` (`record_extended`, `load_bundled`, `decode_bundled`): pass consumer version and
    display name into `decode_bundled`, run `v2::decode_objects` / `hoist_inline_inputs` /
    `mark_field_origins` for v2 bundled rulesets, and record `ExtendsEntryKind::Bundled(spec)` in
    `record_extended`.
  - `v2/mod.rs::refuse_v2_edge`: enforce ADR-0022 `source <= consumer` for
    `ConfigSourceClass::Bundled` returning a user config error (`Error::Other`, exit 2 -- never
    `Error::Internal`, exit 3).
  - `crates/alint-dsl/src/bundled.rs` test updates: validate `@v1` rulesets as v1 and `@v2`
    rulesets through the v2 loader.
  - `xtask/src/docs_export/rulesets.rs`: derive `@v<rev>` from each bundled ruleset's canonical id
    instead of hardcoding `@v1` so generated site docs emit working `alint://bundled/...` snippets.
- **Source policy (ADR-0048 decision 3c):**
  - **Allowed `version: 2` sources in v0.24:** top-level config, local `extends:`, `.alint.d`
    fragments, and **bundled rulesets (`alint://bundled/...`)**.
  - **Refused `version: 2` sources in v0.24:** remote (`https://...`) `extends:`, nested configs
    (`nested_configs: true`), and `alint_dsl::parse` inline configs (deferred until v2 stabilizes
    in v0.25).
- **Trust & provenance:** bundled rulesets are first-party compiled-in YAML (trusted, zero network,
  zero env reads). Their rules carry `extended_source = true` (preventing config self-edits) and
  their selectors/relations record `ConfigSourceClass::Bundled` in `PlanSource`, displayed in
  `alint list` and `alint explain`.
- **First-party opt-in bundled `version: 2` ruleset(s) shipped in v0.24:**
  - `js/workspace-deps@v2`: dependency version consistency across JS/TS workspace manifests
    (`package.json` `dependencies` / `devDependencies` / `optionalDependencies`, ignoring
    `workspace:` / `catalog:` / `npm:` protocols and workspace package names), with firing and
    silent `alint test` fixtures.
  - Existing `@v1` bundled rulesets (`docs/adr@v1`, `java@v1`, `rust@v1`, etc.) stay `version: 1`
    and untouched.

### 3.11 D11: Tooling, draft schema `$id`, `facts.json`, and experimental warning

- **`schemas/v2/config.json`:** advance `xtask/src/gen_schema_v2.rs` `RELEASE` to `"v0.24.0"`
  (verified by #570's preflight gate) and regenerate `schemas/v2/config.json` covering every new
  v2 key (`reference`, `count`, `compare`, `satisfies`, `records:`, `strict:`, parameterized
  `transform:`, optional `bound:` + `on_missing:`, per-group `set` + `all_target_groups:`, inline
  `values:`, per-part `union:` `fields:`, bundled `as:`).
- **`schemas/v1/config.json`:** add `no_duplicate_keys` (D6) and `strict:` on JSON surfaces (D7).
- **Headline kind count:** `constraint` stays ONE canonical kind (v2-only, experimental);
  `no_duplicate_keys` adds ONE canonical v1 kind (`104 -> 105` canonical kinds, `11` aliases).
- **Experimental warning text:** update `"v0.24 at the earliest"` -> `"v0.25 at the earliest"`
  (ADR-0048 decision 3a) across `crates/alint/src/config_cmd.rs`, `xtask/src/gen_schema_v2.rs`,
  `xtask/src/gen_schema_v2_tests.rs`, `docs/rules.md`, `docs/site/configuration/version-2.md`, and
  CLI snapshots (`crates/alint/tests/cli/*version-two*`, `policy_test_cli.rs`).
- **`facts.json`:** keeps `compatibility.config_versions: [1]` and
  `compatibility.experimental_config_versions: [2]`.
- **Commands (`validate-config`, `list`, `explain`, `export-agents-md`):** updated additively to
  render `Bundled` v2 sources, `records:` readers, optional `bound:`, and the new `constraint`
  assertion families.

## 4. False-positive surface

Mandatory per `docs/design/TEMPLATE.md` and constitution invariant 12:

- **`reference` (D1):**
  - *Risk:* external/protocol values (`workspace:*`, `catalog:`, `@types/*`) or optional metadata
    not present in the target relation firing as dangling references.
  - *Mitigation:* callers filter with `where: { not_matches / not_in }` before `reference`;
    `skip_missing: true` opts out rows with missing optional `bound:`/`record:` keys; type
    mismatches fail loud as analysis failures rather than producing misleading "dangling reference"
    findings.
- **`count` (D2):**
  - *Risk:* an empty repository or filtered-out relation firing `min: 1` unexpectedly, or `max: 0`
    flooding output on a misconfigured selector.
  - *Mitigation:* `min: 1` only fires on zero-row groups created explicitly via `groups:`,
    path-token `resource_groups`, or `entry.root` `stub_rows` (never on nonexistent arbitrary keys);
    `max: 0` / `exact: 0` per-row findings are bounded by `LimitKind::FindingsPerRule` (10,000
    default; #596).
- **Per-group `set` and multi-field `group_by` (D3):**
  - *Risk:* a target group missing from the input relation silently passing or unexpectedly firing
    when the input selector intentionally excludes draft packages.
  - *Mitigation:* input relation drives active groups by default; requiring target groups absent
    from input requires explicit `groups:` or `all_target_groups: true` (`equal_to` only).
- **`compare` and `satisfies` (D4, D5):**
  - *Risk:* pre-release tags (`22.0.0-rc.1`), partial versions (`22` vs `22.13.0`), `Null`/`Missing`
    operands, or non-semver specifiers (`latest`, `lts/*`, `>=20 || >=22`) mis-comparing silently.
  - *Mitigation:* `satisfies` follows strict `node-semver` prerelease rules
    (`include_prerelease: false` by default), parses partial versions deterministically, supports
    `||` and hyphen/x-ranges natively, and turns `Null`/`Missing` or unparseable versions/ranges
    into explicit analysis failures (forcing explicit `where:` filtering instead of silent false
    passes or false fails).
- **`no_duplicate_keys` (D6):**
  - *Risk:* YAML anchors/merge keys (`<<: *base` with an override key in the mapping), multi-doc
    YAML streams (`---`), typed scalar keys (`1` vs `"1"`), or XML repeated elements firing.
  - *Mitigation:* YAML duplicate detection checks syntactic mapping keys in the same mapping node
    using YAML 1.2 Core Schema typed scalar keys and resets per `---` document (an explicit key
    overriding a merged `<<: *base` key is NOT a duplicate; two literal `<<:` keys in one mapping
    ARE flagged); XML is explicitly refused at config build time; TOML/HCL parse-time errors stay
    non-baselinable `resource_parse`.
- **Strict-JSON mode (D7):**
  - *Risk:* enabling `strict: true` globally breaking `tsconfig.json` / `.vscode/*.json`.
  - *Mitigation:* `strict: false` remains the default everywhere; `strict: true` is strictly
    opt-in per rule/reader, and cache keys across `StructuredCache`, `FileStructuredCache`, and
    `DecodeCache` isolate strict vs tolerant parses so a tolerant rule on `tsconfig.json` never
    interferes with a strict rule on `locales/*.json`.
- **Array-of-objects `records:` and parameterized transforms (D8 / #592):**
  - *Risk:* `split` on a string without the separator (for example an untagged Docker image `nginx`
    split on `:` with `part: 1`) silently producing a bogus empty string matching another untagged
    image.
  - *Mitigation:* out-of-range `split` produces a `Missing` cell under default `required: false`
    (never matching another string) or an analysis failure under `required: true`, while `part: 0`
    and `part: -1` return `"nginx"` on separator-less strings.
- **Optional `bound:` (#593) and stub locale roots (#598):**
  - *Risk:* `{ source: "bound: ...", optional: true }` silently hiding a typo in the JSONPath
    query; or a filtered-out / non-locale empty YAML file (`test: {}`) forming a spurious empty
    group.
  - *Mitigation:* `optional: false` remains the default on `bound:` so typos fail loud unless
    explicitly opted in; stub-root empty groups pass through non-`entry.path` transforms and
    `where:` predicates, reject multi-root stub files per ADR-0037 decision 2, activate ONLY when
    `group_by` explicitly reads `entry.root`, and never invent a fake group for empty files with no
    root mapping key.
- **Target-file `set` placement (#595) and `{{value}}` placeholder (#594):**
  - *Risk:* wrong target-file guess when a group's path token appears twice in the path (e.g.
    `en/en.json`) or differs in case (`EN` vs `fr`), or secret leakage through `{{value}}`.
  - *Mitigation:* target-file matching uses structural path-component equivalence against
    `SetGroup::resources` requiring a unique match (`matches.len() == 1`) and falls back to
    `group.first` on any ambiguity or non-`PathToken` grouping; `{{value}}` unconditionally redacts
    `sensitive: true`, `reads_environment`, and `hide_literals` to `<redacted>` and escapes
    invisible Unicode controls via `values::escaped()` (`SHOWN_CHARS = 64`).

## 5. Implementation notes and phased delivery

Phase 5 executes in 4 ordered implementation batches under accepted ADR-0050:

- **Batch 5a (Core v2 polish + preflight: #593, #594, #595, #598, #570):**
  - Optional `bound:` under `FIELD_KEYS` (#593), `{{value}}` placeholder (#594), structural
    target-file `set` placement (#595), `RelationRows::stub_rows` locale root groups (#598), and
    two-mode `ci/scripts/check-v2-schema-id.sh` + `bump-version.sh` sync (#570).
- **Batch 5b (New operators + #592 readers/transforms: D1, D2, D3, D4, D8):**
  - `reference` (single + composite), unified `count`, multi-field `group_by: [...]`, per-group
    cross-relation `set` (`all_target_groups:`), inline literal `set` targets, `compare`, `records:`
    array-of-objects reader (`source: "record: ..."`), parameterized `transform:` (`split`,
    `strip_prefix`, `strip_suffix`, `prefix`, `suffix`), and mixed-reader `union:` per-part
    `fields:`.
- **Batch 5c (High-ROI stratum capabilities: D5, D6, D7):**
  - `semver_eval.rs` + `satisfies:` / `compare: { order: semver }` (Gap C4), `no_duplicate_keys`
    v1 rule kind across YAML/JSON/dotenv/properties/INI (Gap B1), and `strict: true` JSON parse
    mode with isolated cache keys.
- **Batch 5d (DEC-8 bundled `version: 2`, schema/docs, and stratum re-score: D10, D11):**
  - `SourceLocator::{Local, Bundled}` + `v2_sources` origin table, bundled `version: 2` loader
    support + `as:` + direction-rule gates + `rulesets.rs` `@v<rev>`, first-party
    `js/workspace-deps@v2` bundled ruleset with `alint test` cases, `schemas/v2/config.json`
    `RELEASE = "v0.24.0"`, experimental warning `"v0.25 at the earliest"`, `docs/rules.md` /
    `facts.json` / `explain` / `list`, and the 20-repo corpus stratum re-score recorded in
    `docs/design/general-linter.md` section 12.4.

## 6. Tests

Every batch obeys constitution invariant 8 (firing and silent e2e scenarios), invariant 7 (schema
parity), ADR-0012 (output-completeness across human, JSON, SARIF, and LSP including related
locations), and ADR-0024 decision 6 (micro-bench + `reference-perf-gate` budgets):

- **Unit + property tests:**
  - `semver_eval`: differential vectors covering npm `node-semver`, Cargo `VersionReq`, PEP 440
    `requires-python`, partial versions, prereleases, hyphen/`||`/`x` ranges, malformed-input
    rejections (including PEP 440 epochs/`.post`/`.dev`/`===` and commas in `node`), and 1 KiB /
    64-clause DoS caps.
  - `no_duplicate_keys`: YAML nested non-leaf duplicates with disjoint children and sequence-element
    duplicates (`mattermost-56`), OpenAPI/general YAML duplicates (`zulip-28`), multi-document `---`
    isolation, YAML 1.2 typed scalar keys (`1` vs `"1"`), `<<: *base` merge non-false-positive,
    JSONC comments + nested object duplicates, dotenv/properties/INI (including repeated `[section]`
    merging) duplicates, XML build rejection, omitted-format `.xml` `resource_parse`, and per-file
    finding cap.
  - `strict: true`: strict-vs-tolerant cache isolation across `StructuredCache` and `DecodeCache`
    in a single run, empty-file rejection, UTF-8 BOM rejection, JSONC comment/trailing-comma
    rejection, and build-time rejection on non-JSON formats.
  - `records:`, parameterized `transform:`, optional `bound:` (`skip` + `missing` + exclusion from
    `resource_groups`), `{{value}}` redaction (`sensitive`/`env`/`hide_literals`) + bidi escape +
    `SHOWN_CHARS` truncation + load-time rejection on `unique` (preserving literal `{{name}}`),
    structural target-file `set` placement + case-folding + `en/en.json` + ambiguous fallback,
    `stub_rows` locale root groups (`where:` + `lower` + multi-root rejection), composite
    `reference`, `count` (`min`/`max`/`exact`/`max: 0`/`exact: 0`/`distinct`), multi-field
    `group_by` (and 1-element `baseline_key` parity), per-group `set` + `all_target_groups`, inline
    `values:`, `compare` (`Null`/`Missing` fail-loud), and bundled `version: 2` namespace/alias/
    origin/direction rules (`v1` extending bundled `v2` fails with exit 2; `v2` extending bundled
    `v2` with `as:` succeeds; remote/nested `v2` still fails).
- **E2E + CLI contract tests:** firing and silent scenarios for `no_duplicate_keys` and every new
  `constraint` family, `alint validate-config` / `list` / `explain` / `export-agents-md` snapshots,
  `test-check-v2-schema-id.sh` in `shell-tests.sh` (dev mode + `--release <tag>` prior-tag diff),
  and `alint test` fixtures for bundled `js/workspace-deps@v2`.
- **Stratum re-score:** binary verification of newly promoted A units across the 20-repo corpus
  stratum, updating `docs/design/general-linter.md` section 12.4.

## 7. Accepted decisions summary (D1 to D11)

Recorded and accepted in [ADR-0050](../../adr/0050-v024-phase-3-completion-design.md) (`status: accepted`):

| ID | Decision | Accepted specification |
|---|---|---|
| D1 | `reference` assertion | Single `field:` or composite `fields: [...]` -> `to: { relation, field/fields }`, `values::Key` path-free `baseline_key`, bounded by `DistinctValues` and `FindingsPerRule` (section 3.1) |
| D2 | Unified `count` assertion (ADR-0048 3b) | `count:` with `min`, `max`, `exact`, optional `group_by`, optional `groups:`, optional `distinct: true` + `field:`; `max: 0` / `exact: 0` emits per-row findings at offending rows (section 3.2) |
| D3 | Multi-field `group_by`, per-group `set`, inline `set` targets, anti-join | `group_by:` accepts string or `[field, ...]` across `equal`/`set`/`count`; `set` target supports `group_by:` + `all_target_groups:` (`equal_to` only); non-empty string/bool `{ values: [...] }`; defer `join: { kind: anti }` (section 3.3) |
| D4 | Negated equality and ordering | Unified `compare: { field, op: eq\|ne\|lt\|le\|gt\|ge, to, order: exact\|lexical\|numeric\|semver }` with `Null`/`Missing` fail-loud and row-keyed `values::Key` (section 3.4) |
| D5 | Gap C4 semver range satisfaction | `satisfies:` assertion + owned zero-dep `semver_eval.rs` supporting `node` (default), `cargo`, and `pep440-basic` dialects with partial versions, strict caps, and row-keyed `values::Key` (section 3.5) |
| D6 | Gap B1 duplicate keys | Dedicated per-file v1 rule kind `no_duplicate_keys` over YAML (`ayaml` events, multi-doc, Core Schema typed keys), JSON (`jsonc-parser` CST), dotenv (`doc.entries()`), properties, INI (spanned walker) (section 3.6) |
| D7 | Strict-JSON parse option | Opt-in `strict: true` (default `false`) across v1 and v2 JSON surfaces (incl. `ExtractSpec` / `v2_read`), rejecting comments, trailing commas, empty files, and BOMs, with isolated cache keys (section 3.7) |
| D8 | #592 array-of-objects, transforms, mixed `union:` | Dedicated `records:` reader (`at:` + unified `source: "record: $.sub"`), parameterized non-regex `transform:` (`split`, `strip_prefix`, `strip_suffix`, `prefix`, `suffix`), and per-part `fields:` in `union:` (section 3.8) |
| D9 | #593, #594, #595, #598, #570 | Unified `FIELD_KEYS` `{ source: "bound: ...", optional: true, on_missing?: skip\|missing }` (#593); redacted/escaped `{{value}}` (#594); structural target-file `set` placement (#595); `RelationRows::stub_rows` locale root groups (#598); two-mode `check-v2-schema-id.sh` (#570) (section 3.9) |
| D10 | DEC-8 bundled `version: 2` (ADR-0048 3c) | `SourceLocator::{Local, Bundled}` + `v2_sources` origin table, open `alint://bundled/...` for v2 (with `as:`, remote/nested refused), enforce `source <= consumer` with user errors, ship `js/workspace-deps@v2` (section 3.10) |
| D11 | Schema `$id`, kind count, and v2 soak | Advance `RELEASE = "v0.24.0"` in `gen_schema_v2.rs`, keep `constraint` as 1 kind (`no_duplicate_keys` -> 105 canonical kinds), update experimental warning to `v0.25 at the earliest` (section 3.11) |
