# Design doc: named selectors, relations and the `constraint` kind (Phase 3)

Status: Accepted design (2026-11-14), corrected the same day after an audit. The owner took each
decision in section 15 at the phase 3 gate of the v0.23 plan; phases 6 to 8 implement it.
Decisions: [ADR-0034](../../adr/0034-v2-version-boundary-and-phase-3-design.md), which records the
version boundary (DEC-9) and outcomes D1 to D9 of section 15, and
[ADR-0037](../../adr/0037-phase-3-design-refinements.md), which records D10 and D11 and the audit's
corrections that change what an ADR-0034 decision says.
Inputs: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md) (DEC-8, DEC-9),
[ADR-0022](../../adr/0022-policy-authoring-loop-contracts.md),
[ADR-0032](../../adr/0032-v023-owner-decisions.md),
[ADR-0033](../../adr/0033-v023-slice-after-the-corpus-stratum.md), and
[`general-linter.md`](../general-linter.md) sections 7.1 to 7.8, 12.4 and 13.2.

## 1. What this document decides

The v0.23 plan (scope item 2) asks this document to answer:

- **Q1:** where selectors and relations live, and what becomes of ADR-0010's `path_sets:`;
- **Q2:** how selectors on full-index rules interact with `--changed`, which amends constitution
  invariant 2;
- **Q6:** the join vocabulary and its caps;
- **Q12:** namespacing, provenance, no shadowing, and capability monotonicity;
- **Q15:** `when` in v2;
- **section 7.8:** the v2 schema, the commands, the generated documentation and the facts counts;
- **ADR-0033 decision 2:** the entry rows' shape, their limits and provenance, and whether XML
  joins.

It also fixes the version boundary (DEC-9), which ADR-0034 records. Everything it designs is
`version: 2` only, experimental, and report-only. A v1 configuration is unchanged (plan section 3).

The slice is fixed by ADR-0033: named selectors and relations, value-bearing entry rows for JSON,
YAML and TOML, and a `constraint` kind with the equality, unique/key and set families. Its users are
the three conformance policies (plan item 5): locale key parity, dependency version consistency and
toolchain-pin consistency.

## 2. What binds the design

These are facts about the code at `14786ee2f` (#513) and the accepted records, not proposals.
They were first taken at `a04548a74` and re-checked for ADR-0037.

**The relation layer** (`alint-core/src/relation/`) is `#[doc(hidden)]` and promises no stability.
v0.21 invariant 6 ("internal stays internal") keeps its types out of every documented API, report
and schema.
- Rows are `Row { resource, pointer, label, ordinal, span, state, original }` in a canonical order,
  with `Value` = null, bool, number (the lexeme is kept: `1.10` is not `1.1`), string, bytes, path,
  list or record (ADR-0024 decision 1). A row's state is a value, a key, missing or an error.
- Adapters:
  - `EntryRowsAdapter` (JSON, YAML, dotenv, INI, properties) emits key rows **without values**;
  - `TomlRowsAdapter` emits TOML key rows, and treats arrays as values;
  - the Android strings and resx adapters emit XML keys;
  - `ExtractAdapter` emits string values without a pointer.
- Limits: rows per extractor (1M by default, 10M hard), rows per run (5M, 50M) and distinct values
  (100k, 10M; `Relation::distinct_values` enforces it). The join limits (`JoinInputs`,
  `JoinOutputs`) are reserved and enforced nowhere. Users cannot set any limit.
- Only `key_parity` and `cross_file` read rows through the layer. `cross_file` decodes glob-matched
  files outside `DecodeCache`, so those decodes do not count toward the rows-per-run budget.
  `for_each_dir` opens a `relation::IterationScope` for each iteration, so a nested relation rule
  reads through caches of its own (#511).

**The loader** reads `version:` only after a strict v1 decode (`RawConfig` has
`deny_unknown_fields`). `version: 2` with a `selectors:` key fails as "unknown field `selectors`",
not as an unsupported version, and `version:` may come from `{{env.X}}` (both probed). ADR-0022's
per-source checks run before merge, so the "drops an extended file's `version:`" in section 7.8 is
stale. Rules and templates merge flat by id. Trust markers (provenance, environment, sensitive
fields, field sources) exist on rules and templates only.
- Every local source is interpolated, except a rule's or template's `id`, `kind` and `level`
  (`alint-dsl/src/interp.rs`), and a substituted number or boolean is retyped.
- A `.alint.d` fragment may omit `version:` and inherit its root's (ADR-0022 decision 3).
- Nested configs (`ConfigSourceClass::Nested`) and `alint_dsl::parse` input
  (`ConfigSourceClass::Inline`) are source classes of their own.

**`--changed`.** `FullIndex { path_scope: false }` rules, `cross_file` among them, always run on the
full index. Five full-index kinds (`file_exists`, `file_absent`, `key_parity`,
`max_files_per_directory`, `no_case_conflicts`) declare a path scope and are skipped when the diff
misses it, then judge the whole index. `markdown_paths_resolve`, a per-file rule that needs the full
index, declares a scope that never skips it. Constitution invariant 2 and ADR-0003 said a full-index
rule declares no path scope, so the code already departed from them.

**Selection** is `paths:` (a glob, a list with `!` excludes, or `{include, exclude}`) plus
`scope_filter:` (`has_ancestor`, `changed_since`, and ADR-0010's manifest-derived membership).
ADR-0010 deferred a named `path_sets:` block and bound any successor: manifest values decide which
files a rule sees, never what it decides about them. ADR-0008 made `git_tracked_only` a kind option
because a universal option that a kind can ignore fails quietly.

**Baselines.** A fingerprint is `sha256(lp(rule_id) ‖ lp(path) ‖ lp(discriminator))`, where the
discriminator is the finding's `baseline_key` when it sets one (`alint-core/src/baseline.rs`), so
the primary path is hashed whenever the finding has one. Only a hidden-path finding leaves its path
out (ADR-0030 decision 1). `baseline_key` is a string.

**`when`** has one comparison per term, `or`/`and`/`not`, and `facts`, `vars`, `iter` and `env`.
Across types, `==` returns false and `!=` returns true (`when/eval.rs`), although the module doc
in `when/mod.rs` calls a cross-type comparison an error; ordering comparisons and `in` across types
are errors, as it says. `vars` are always strings.

**Tooling.**
- No JSON Schema validates a config at load; the committed schema serves editors and tests. Its
  `$id` points at `main`.
- `validate-config`, `list`, `explain` and `export-agents-md` handle rules only. `explain` and
  `alint rules show` print a `selectors:` line (the kind's selector fields).
- `init` writes `version: 1`. `suggest --format yaml` writes no `version:` (probed), and its
  documented use appends the output to an existing config (`alint suggest --format yaml >>
  .alint.yml`).
- `gen-facts` counts canonical kinds (103) and aliases (11) separately. `facts.json` lists
  `compatibility.config_versions` as integers (`[1]`).
- `coverage_audit_schema_drift` requires every registered kind in `schemas/v1/config.json`, and the
  `all_kinds.yaml` fixture, a v1 config, drives the ADR-0012 gates.
- Of the check formats, human, SARIF and the language server carry related locations, each with a
  unit test. The JSON format has no related-location field (`alint-output/src/json.rs`).

## 3. The version boundary (DEC-9)

DEC-9 decided that `version: 2` ships before v1.0, so v1.0 freezes both versions. ADR-0032 made v2
experimental in v0.23. This section fixes what that means; ADR-0034 records it.

1. **The version is read first.** The loader reads the top-level `version:` from the YAML mapping
   before any typed decode and dispatches on it.
   - A v2 source's `version:` must be the literal integer `2`.
   - A `.alint.d` fragment without `version:` is decoded as its root's version (ADR-0022
     decision 3).
   - A `version:` that is not a literal integer (`'{{env.V}}'`) is read as v1 and must equal 1
     after interpolation. A v1 source keeps today's behavior, including `{{env.X}}`, because
     rule 10 forbids reinterpreting v1.
2. **v2 is a superset of v1 in v0.23.** A v2 source accepts every v1 top-level key with v1's
   meaning, plus `selectors:` and `relations:`, and its rules may use every v1 kind plus
   `constraint`. No v1 key changes meaning.
3. **The new surface is v2-only.** `selectors:`, `relations:` and `kind: constraint` in a v1 source
   are configuration errors that name the version they need. `constraint` is registered with a
   minimum config version of 2 (section 11).
4. **Experimental.** Loading a v2 source prints a warning, and `validate-config` reports it. Nothing
   about v2 is stable before a release declares it so (v0.24 at the earliest); v2 syntax may change
   between minor releases until then, and the CHANGELOG lists each change.
5. **The direction rule of ADR-0022 holds at every edge.** A v2 source may extend v1 and v2 sources;
   a v1 source never extends a v2 one.
6. **v2 sources are local files in v0.23** (section 10): the top-level config, local `extends:`
   and `.alint.d` fragments. Bundled and remote sources must be `version: 1`. So must nested
   configs (`nested_configs:`) and `alint_dsl::parse` input in v0.23.
7. **v1.0** follows the release that declares v2 stable and freezes `version: 1`, `version: 2`,
   the reshaped `alint-core` API and plugin contract 1.0 together (DEC-9).

## 4. Named selectors (Q1)

A selector names a set of resources. It compiles to the same `Scope` a rule's `paths:` compiles to
today, so it has the same grammar and the same predicates:

```yaml
selectors:
  workspace-manifests:
    paths: ["packages/*/package.json", "!packages/docusaurus/package.json"]
    scope_filter: { has_ancestor: package.json }
```

- **Both top-level and inline.** A top-level `selectors:` map names reusable selectors; wherever a
  selector is referenced, an inline one may stand instead, and is anonymous. An inline selector is
  a map (`{ paths: ..., scope_filter: ... }`). A bare string is always a name, never a glob.
- **`path_sets:` becomes this feature.** ADR-0010's manifest-derived membership is already a
  `scope_filter` predicate, so a named selector that uses it is the deferred `path_sets:` block.
  ADR-0010's boundary holds unchanged: a selector decides which resources a relation reads, never a
  verdict, a message or a fix.
- **Referenced by relations only in v0.23.** A v1 kind's `paths:` keeps its v1 form even in a v2
  source. Letting every kind take `paths: { selector: name }` touches every kind's `KindMeta` and
  the `--changed` scope of each, so it waits for demand.
- **`scope_filter:` lives in selectors.** A selector may use `has_ancestor` and the
  manifest-derived predicates. It may not use `changed_since` in v0.23: only full-index
  `constraint` rules read selectors, and what such a rule sees must not depend on the diff
  (section 9).
- **No new predicates.** The three policies need none. A selector-level `tracked` predicate, which
  ADR-0008 rejected as a universal option, is not added; ADR-0008 stands.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `paths` | a glob, a list with `!` excludes, or `{ include, exclude }` | required | as a v1 rule's `paths:` |
| `scope_filter` | `has_ancestor` or a manifest-derived predicate | none | narrows `paths:`; `changed_since` is refused in v0.23 |

## 5. Named relations and entry rows (ADR-0033 decision 2)

A relation names a plan that reads the resources a selector picks and emits rows with **named
fields**. Each field holds a `Value` and remembers where it came from.

```yaml
relations:
  dependency-ranges:
    from: workspace-manifests
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies", "$.optionalDependencies"]
    fields:
      manifest: path
      field: entry.parent
      dependency: entry.key
      range: entry.value
```

**Sources of rows.** A relation reads with one of:
- `entries:`: one row per member of each object `at` selects, or per leaf below it with
  `leaves: true`. A leaf is a key with no key below it. Arrays are values, so an array's elements
  are never keys, as in `key_parity`. `at` is one JSONPath or a list of them, a subset that must
  select objects: a selected array or scalar is an analysis failure naming the path. JSON, YAML and
  TOML only.
- `read:`: the extract syntax `cross_file` already has, limited in v0.23 to `json:`, `yaml:` and
  `toml:` (one JSONPath string each), `regex:` (capture group 1), `lines:` and `whole_file:`, with
  one row per extracted value. Its rows carry no pointer and hold strings, as in `cross_file`: a
  non-literal value (`${VAR}`) is skipped with a note, and non-string matches give one finding per
  resource that names their kinds. dotenv, properties, XML, INI and HCL are not readable in v0.23.
- `union:`: the rows of several parts, each with its own `from:` and its own `entries:` or `read:`,
  in the order written. Every part must bind every field. The toolchain-pin policy reads `.nvmrc`
  with a regex and three `package.json` paths with JSONPath in one relation.
- `join:`: the rows of two relations combined on equal declared fields (section 8).

**Fields.** A field takes one of:
- `path` (the resource's path), or a path token (`dir`, `basename`, `stem`, `ext`, `parent_name`)
  as in nested rules. These are strings;
- `entry.key` (the entry's own key) or `entry.value` (its value);
- `entry.parent`: the key of the object that holds the entry (`dependencies` for a member of
  `$.dependencies`), or null when that object is the document root;
- `entry.root`: the first key of the entry's own key path (`en` for `en.nav.home`);
- `entry.path`: the key path from the document root, rendered as `Pointer::render` writes it
  (`nav.home`, `['a.b']`). The field also keeps the path's segments, which `strip-root` and
  `plurals: cldr` use;
- `value` for a `read:` row;
- `bound: <json/yaml/toml path>`, a per-resource value read once (a manifest's `$.name`), which must
  exist exactly once in each resource or the resource is an analysis failure;
- in a join's output, `left.F` or `right.F` (section 8).

A field is written as its source (`dependency: entry.key`) or as a map
(`token: { source: entry.value, sensitive: true }`).

**Value types.** Scalars only in v0.23: null, bool, number (lexeme kept) and string. The type is
checked only when a field binds `entry.value`: then an object or array value, or a TOML datetime,
is an analysis failure naming the path, never a silent string (section 7.1 rule 4). A relation that
binds only keys therefore reads a Rails locale file whose `date.day_names` is an array. YAML stays
strict: aliases, merge keys and complex keys are parse failures, as in every relation today.

**Absent and missing values.**
- An `at:` path that a resource lacks yields no rows from it: a manifest without
  `devDependencies` contributes none.
- A `read:` that matches nothing in a resource yields one missing row there, unless its part sets
  `optional: true`. A missing row sits at its resource and has no value.
- A missing value never joins, never forms a key or a group, and is in no set, as
  `Relation::distinct_values` skips it today. `equal` reports it as a difference (section 6).

**Adapters.** Value-bearing rows are new adapter modes with their own adapter ids, so their decodes
never share a cache entry with the key-only modes. TOML gets its value mode in `TomlRowsAdapter`
(not the `entry` adapter, which rejects TOML; ADR-0033 and the plan say "the `entry` adapter" for
both).

**XML does not join in v0.23.** Android strings and resx stay key-only. The locale policy works on
JSON and YAML trees, and `key_parity` already covers XML catalogs in v1.

**Redaction.** dotenv and properties values are redacted by default (ADR-0019 decision 3), so those
formats emit no value rows, and `read:` does not take them. A field may be marked
`sensitive: true`; its value never reaches a message, the JSON report or a baseline. A field that
shows a sensitive field's value is sensitive too: a join's copy of it, a join's copy of the field
an `on:` pair joins with it, and a field that an `in:` or `not_in:` list of it filters, with the
fields that show that field's value.

**Provenance.** Each field keeps its resource, its pointer and its byte span (the key span for a
key, the value span for a value). A YAML literal or folded block scalar has no value span today
(`alint-core/src/extract.rs`), so a finding on such a value falls back to its key's span; plain and
quoted scalars, single- or multi-line, have one. A finding's location and related locations come
from these.

**Limits.** Relation decodes go through `DecodeCache`, so the existing per-extractor, per-run and
distinct-value limits apply; a hit is an analysis failure, never truncation. The join limits are
section 8's.

**`--changed` and caches.** A relation is read once per run whatever number of rules use it (the
existing `ReadCache` and `DecodeCache` keyed by adapter id).

**Syntax.** A relation has `fields:` and exactly one of `entries:`, `read:`, `union:` and `join:`.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `from` | a selector name, or an inline selector map | required with `entries:` or `read:` | the resources the relation reads |
| `entries.format` | `json`, `yaml` or `toml` | required | the decoder |
| `entries.at` | a JSONPath, or a list of them | required | the objects whose members become rows, in list order |
| `entries.leaves` | bool | `false` | emit the leaves below each selected object instead of its members |
| `read` | a map with one of `json`, `yaml`, `toml` (a JSONPath), `regex` (a string), `lines` (`{ comment }`), `whole_file` (`{}`) | none | one row per extracted string value |
| `optional` | bool, beside `read:` | `false` | a `read:` that matches nothing yields no row instead of a missing row |
| `union` | a list of parts, each `{ from, entries or read, optional }` | none | the parts' rows in the order written |
| `join.left`, `join.right` | a relation name, bare or qualified | required | the two sides (section 8) |
| `join.on` | a map from a left field to a right field | required | the join key |
| `join.unique` | `left` or `right` | required | the side checked unique on `on:` |
| `fields` | a map from a field name to a source, or to `{ source, sensitive }` | required | the row's named fields; `sensitive` defaults to `false` |
| `transform` | a map from a field name to a list of transform names | none | applied in list order, before `where:` (section 6) |
| `where` | a map from a field name to a predicate map | none | every predicate must hold (section 7) |

## 6. The `constraint` kind

`constraint` is one kind, full-index, report-only and without fixes (section 8.3 of the program).
It takes one relation as `input:` (a name, or an inline relation) and one assertion. It takes no
`paths:` and no `scope_filter:` of its own: its relations' selectors pick what it reads. It is
registered with a minimum config version of 2, and in v0.23 it cannot be nested under a
`for_each_*` kind's `require:` (section 11).

```yaml
rules:
  - id: one-range-per-dependency
    kind: constraint
    input: dependency-ranges
    assert:
      unique: { key: [dependency], determines: [range] }
    level: error
    message: "{{row.dependency}} is required with more than one range"
```

**The families** (ADR-0033 decision 1; row-level `equal` is D10):
- **`equal`,** in one of three forms:
  - within each group (`group_by:`, optional), every row has the same value of `field:`;
  - every row equals one named source row, as `cross_file`'s `equals` names a canonical source:
    `to: { row: { source: .nvmrc } }` names the row whose `source` field is `.nvmrc`. Exactly one
    such row must exist and hold a value; none, several or a missing one is an analysis failure.
    This form takes no `group_by:` in v0.23;
  - within each row, `field:` equals another field of the same row: `to: { field: G }`. It
    compares a join's two sides (section 8).
- **`unique`:** no two rows share `key:`; with `determines:`, rows that share `key:` agree on the
  determined fields (a functional dependency).
- **`set`:** the set of `field:`'s values, per group (`group_by:`, optional), is equal to
  (`equal_to:`), a subset of (`subset_of:`) or disjoint from (`disjoint_from:`) another set: a group
  of the same relation (`{ group: en }`) or a field of another relation
  (`{ relation: workspace-names, field: name }`, section 8).
  - The groups are the reference group, every group named in `groups:`, and every value of the
    `group_by:` field. When `group_by:` is `path` or a path token, every resource the relation's
    selectors pick forms its group, rows or not.
  - A group with no rows reports each reference value as missing: at the resource that declared
    it, or without a path for a group named only in `groups:`. `key_parity` does the same for an
    empty peer file.
  - A reference group with no rows is an analysis failure.
  - `plurals: cldr` collapses CLDR plural groups as `key_parity` does (below; D11).

**Comparison.** Values compare after transforms.
- A value compares by its type and its text. A number compares by its lexeme (`1.0` is not `1`,
  and `0x10` is not `16`). `path` and path-token fields are strings, so `.nvmrc` matches the
  path `.nvmrc`.
- Null matches nothing: a null join key joins no row, a null `key:` is never a repeated key, and
  null is in no set. A null `group_by:` value is an analysis failure.
- A key, join or membership comparison between different types (a number against a string) is an
  analysis failure naming both rows, never a silent non-match. In `equal`, values of different
  types differ, and the finding shows both types.
- `matches`, `not_matches` and the string transforms on a non-string value are analysis failures.

**Transforms.** A relation's `transform:` maps a field to a list of transforms from a fixed set,
applied in order; the relation then applies `where:` (section 7). No user-defined functions.
- `trim`, `lower`, `semver-major`, `semver-minor`: `cross_file`'s `normalize` values, on strings.
- `strip-root`: on an `entry.path` field, drops the path's first segment, as `key_parity`'s
  `strip_root` does. Each resource the relation reads must have at most one top-level key, and
  every row's key path must have at least two segments, so a top-level key that is itself a leaf
  (`en: hello`, `en: {}`) fails. A resource that breaks either rule is an analysis failure naming
  the reason. A resource with no keys emits no rows.

**CLDR plurals.** `plurals: cldr` is an option of `set` against a group, and needs `field:` to be an
`entry.path` field. For each comparison of the reference group with another group, it merges
plural groups over the two groups' key paths, as `key_parity` does for a reference file and a peer
(`nested_plural_groups` and `i18next_plural_groups` in `alint-rules/src/key_parity.rs`):
- **Nested CLDR objects** (Rails YAML): a non-root object with a leaf named for a CLDR category
  (`zero`, `one`, `two`, `few`, `many`, `other`) in either group, `other` among its members in at
  least one, and, in each group that has the object, no member but category-named leaves. Its
  members merge into one value at the object's path.
- **i18next siblings:** `<base>_<category>` keys under one parent, with `<base>_other` in at least
  one group, a non-empty base, and `<base>` itself a key in neither group. They merge into one
  value, `<base>`.
- Nested groups merge first, then i18next ones. A merged value sits at its `other` member, or at
  its first member when the group has no `other`.

A real key named `one` or `other` beside other keys is never merged, because the decision sees both
groups' siblings. A per-value transform cannot see them, so plural collapse is not a transform.

**Findings** (D8). Every finding sits at a row's field (its resource and span) or at a resource,
with related locations at the rows it is compared with (capped at 20, with an omitted count, as
today).
- **`unique` with `determines:`:** one finding per distinct determined value of a key whose rows
  disagree, at the first row in canonical order with that value. Its related locations are the
  other rows with that value and one row of each other value. React at `^18.2.0` in 498 manifests
  and at `^17.0.2` in 2 gives two findings.
- **`unique` without `determines:`:** one finding per row that repeats a key, related to the first
  row with that key.
- **`equal` within groups:** as `unique` with `determines:` on the compared field within each group.
- **`equal` against a named source row:** one finding per row that differs from it, at that row,
  related to the source row. A missing row differs, so a removed `volta.node` fires.
- **`equal` within a row:** one finding per row whose `field:` differs from its `to: { field: }`,
  at `field:`'s location, with the other field's location as a related location. A join keeps each
  field's provenance, so the two can sit in different files.
- **`set`:** one finding per missing, extra or shared value per group.
  - A missing value (`equal_to:`; in the other set, not in the group) sits at the group's first
    resource in canonical order, related to the row that holds it, as `key_parity` reports a
    missing key at the peer file with the reference key related.
  - An extra value (`equal_to:` or `subset_of:`; in the group, not in the other set) sits at the
    row that holds it.
  - A shared value (`disjoint_from:`) sits at the row that holds it, related to a row of the
    other set that holds it.

**Identity.** A `constraint` finding's fingerprint leaves the primary path out:
`sha256(lp(rule_id) ‖ lp("\0constraint") ‖ lp(baseline_key))`, because its location (a key's first
row, or a group's first resource, in canonical order) moves when rows are added, removed or
renamed. DEC-5 freezes only existing kinds. Baseline tests add a row that sorts first and rename the
first resource (general-linter section 12.2).

`baseline_key` is built from parts, each written as its byte length in decimal, a colon and the part
(`6:unique`). A value, a NUL or a separator inside a part therefore cannot make two keys collide.
This is the length prefixing of `hash_components`, written in decimal because `baseline_key` is a
string.
- A value is two parts: its type (`null`, `bool`, `number` or `string`) and its text.
- A group is its `group_by:` value, or the one part `all` without `group_by:`.
- A row is its resource path and its pointer. A `read:` row has no pointer, so it is its part's
  index in the relation and its ordinal instead.

The parts of each finding's key:
- `unique` with `determines:`: `unique`, the key values, the determined values. A range that
  appears later is a new finding even when the key's earlier disagreement is in the baseline, and
  fixing a range resolves exactly its finding;
- `unique` without `determines:`: `unique` and the key values. The rows that repeat a key share
  one key and are told apart by their count, so renaming the first file keeps a baselined finding
  baselined. Phase 7 step 1's review corrected this bullet, which keyed each repeat by its row;
- `equal` within groups: `equal`, the group, the value;
- `equal` against a named source row: `equal`, `source`, the row;
- `equal` within a row: `equal`, `row`, `field:`'s resource path, `field:`'s pointer;
- `set`: `set`, the group, the value, and `missing`, `extra` or `shared`.

Related locations never enter a fingerprint.

**Messages.** `{{row.FIELD}}` interpolates a field of the reported row; a `sensitive` field renders
as `<redacted>`. Without `message:`, each family has a generated message that quotes keys and
values, as `key_parity` and `cross_file` do.

**Syntax.** `constraint` takes the v1 rule keys `id`, `level`, `message`, `when` and the other
metadata keys, plus these. `assert:` holds exactly one of `equal`, `unique` and `set`.

| Key | Type | Default | Meaning |
|---|---|---|---|
| `input` | a relation name, bare or qualified, or an inline relation | required | the rows the assertion reads |
| `equal.field` | a field name | required | the compared field |
| `equal.group_by` | a field name | none: the relation is one group | compare within each group |
| `equal.to` | `{ row: { FIELD: literal } }` or `{ field: G }` | none | a named source row, or another field of the same row |
| `unique.key` | a list of field names | required | the key |
| `unique.determines` | a list of field names | none | the fields the key determines |
| `set.field` | a field name | required | the field whose values form each set |
| `set.group_by` | a field name | none: the relation is one group | the group of each row |
| `set.equal_to`, `set.subset_of`, `set.disjoint_from` | `{ group: literal }` or `{ relation: R, field: F }` | exactly one is required | the other set |
| `set.groups` | a list of literals | `[]` | groups that must exist, rows or not (`equal_to` only) |
| `set.plurals` | `exact` or `cldr` | `exact` | collapse CLDR plural groups (`{ group: }` targets only) |

## 7. `when` and row filters (Q15)

- **`when` is unchanged in v2.** It stays a rule gate with v1's grammar and evaluator. A typed
  evaluator and an expression syntax wait for a release with demand beyond these policies.
- **Row filters are structural.** A relation may have `where:`, a map from a field to predicates
  that all must hold:
  - `in` and `not_in`: a list whose items are literals or `{ relation: R, field: F }` references
    (membership in another relation's field, section 8), so one predicate can exempt both
    (`not_in: ["@types/node", { relation: workspace-names, field: name }]`). A YAML map holds one
    predicate of each name per field, which is why literals and references share a list;
  - `matches` and `not_matches`: a regex, on strings only.
- **Order.** A relation applies `transform:`, then `where:`. The locale policy's prefix filter
  therefore matches after `strip-root` has removed the locale key. Every predicate of `where:`
  tests every row, so their written order does not matter: a row that any predicate is false for
  is dropped, and a row that none is false for, but that one cannot test (section 6: a pattern on
  a non-string, a list of another type), is an analysis failure.
- **What the policies need.** These predicates suffice at the stratum's pins: jest-23 exempts
  `@types/node`, trpc-05 skips `catalog:`, `npm:` and `workspace:` specifiers, and
  yarnpkg-berry-01 exempts dependencies on workspace packages. yarnpkg-berry-01 exempts only
  dev dependencies on them, a condition on two fields that one `where:` map cannot negate; the
  illustrative config exempts them in every field. At berry's pin the two agree, because every
  dependency on a workspace uses `workspace:^` (yarnpkg-berry-02, checked against the clone), which
  the specifier filter drops anyway (section 16).
- **The cross-type `==` stays as it is in v1.** `==` across types returns false and `!=` returns
  true; changing either would change v1 configurations. The doc comment in `when/mod.rs` that calls
  a cross-type comparison an error is corrected for equality (follow-up); for ordering comparisons
  and `in` it is right.

## 8. Joins and caps (Q6)

Three operations combine rows. All of them match on equality of declared fields, after each field's
transforms, under section 6's comparison rules.

- **Grouping** (`group_by`, `key`) hashes the rows of one relation.
- **Semi-joins.** A membership filter (`where: { dependency: { not_in: [{ relation:
  workspace-names, field: name }] } }`) or a `set` assertion against another relation's field
  builds a lookup set of that field and probes it. A filter never emits more rows than it reads.
- **Equality joins** (D5). A relation whose source is `join:` combines the rows of two relations:

```yaml
relations:
  ranges-vs-workspace:
    join:
      left: dependency-ranges
      right: workspace-versions
      on: { dependency: name }
      unique: right
    fields:
      manifest: left.manifest
      dependency: left.dependency
      range: left.range
      version: right.version
```

The join's rules:
- **Inner only.** A row without a match on the other side is dropped. Rows without a match (an
  anti-join) are a `not_in` filter.
- **One side is unique on `on:`** (D5a). `unique:` is required and names that side, `left` or
  `right`. The engine checks it while it builds that side's lookup table; a repeated key is an
  analysis failure that names both rows. A join therefore emits at most as many rows as its other
  side, and its cost is linear and known from the plan. Many-to-many joins are not in v0.23.
  - Eleven stratum records describe a join on a key: fluentui-18, trpc-06, backstage-35 and
    nextcloud-b02, and backstage-20, backstage-26, mattermost-06, -07, -32 and -63, and
    yarnpkg-berry-17.
  - In nine of them one side is keyed by object keys or workspace names, which are unique where
    they are declared. The records of backstage-26 and mattermost-63 do not say whether their key
    is unique; a repeated key there is an analysis failure, never a wrong result.
- **Explicit fields.** `fields:` names every output field as `left.F` or `right.F`; nothing is
  copied implicitly. Each field keeps the provenance of the row it came from, so a finding
  reported at `range` sits in the manifest, with the matched workspace row as a related location.
- **Order.** Output rows follow the canonical order of the side that is not unique.
- **Composition.** `left:` and `right:` name relations of the same source or qualified ones (section
  10); a join may read another join, and the plan check rejects cycles. `where:` and `transform:`
  apply to a join's output fields as to any relation's.

**Using a join** (D10). Row-level `equal` (section 6) compares two fields of a join's output, so the
join serves a policy beyond the slice's three in v0.23. nextcloud-b02 requires that an override of a
direct dependency in the root `package.json` be a `$<name>` reference or the dependency's own spec,
and that each `$<name>` reference name a direct dependency (npm refuses the manifest otherwise).
v0.23 covers its equality part:

```yaml
relations:
  root-overrides:
    from: { paths: package.json }
    entries: { format: json, at: "$.overrides" }
    fields: { name: entry.key, override: entry.value }
    where: { override: { not_matches: "^\\$" } }
  root-dependencies:
    from: { paths: package.json }
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies", "$.optionalDependencies"]
    fields: { name: entry.key, spec: entry.value }
  overridden-dependencies:
    join: { left: root-overrides, right: root-dependencies, on: { name: name }, unique: right }
    fields: { name: left.name, override: left.override, spec: right.spec }
rules:
  - id: override-equals-direct-dependency
    kind: constraint
    input: overridden-dependencies
    assert: { equal: { field: override, to: { field: spec } } }
    level: error
```

- The `where:` skips `$<name>` references. Checking that a reference names a direct dependency is a
  `reference` assertion, which waits for v0.24 with the other families.
- An override written as an object (a nested override) binds `entry.value` to an object, which is
  an analysis failure in v0.23. A name listed in two dependency fields repeats the unique side's
  key, which is an analysis failure too.
- Of the other three units, fluentui-18 needs a `reference` assertion and an expected range derived
  from the target's version (`*`, or `>=X.Y.Z-alpha` when the target is a prerelease), trpc-06
  needs range satisfaction, and backstage-35 a frontmatter reader. None of those is in v0.23.

**Caps.** The reserved `JoinInputs` and `JoinOutputs` limits (1M rows by default, 10M hard, in
`relation/limits.rs`) are enforced for the first time.
- `JoinInputs` caps the rows of each side entering one join, as `relation/limits.rs` documents it.
  `JoinOutputs` caps the rows one join emits.
- A semi-join's lookup set is the probed field's distinct values, so the distinct-value limit (100k
  by default, 10M hard) bounds it, as `Relation::distinct_values` does today.
- Grouping is bounded by the existing rows-per-run budget (5M), not by `JoinInputs`, so a relation
  of 100 locales with 20,000 keys each (2M rows) still works.
- `CellsPerRun` (5M by default, 50M hard) caps the cells the relations of one run build: a row of
  F fields is F cells and each file a part reads is one, taken as each row is built, so a `union:`
  of many parts over one decode, or a join's output, fails at the cap. The run's memo keeps every
  relation it evaluates, so the count is the run's. The locale relation above, with two fields,
  holds 4M.
- `BytesPerRun` (1 GiB by default, 16 GiB hard) caps what those relations hold beyond their cells.
  A cell shares its value's text, so a field, a `union:` part or a join that copies a value adds a
  cell and no bytes; a join shares its side's resource list, and a relation that fails with the
  failures of one it reads shares that list. What is new takes its bytes as it is built: each
  decode once (its rows, values and key paths), each row's key path, each `entry.key`,
  `entry.parent`, `entry.root`, `entry.path` and path token, each text a transform changes (once
  for rows that share it), and each note and failure. A run's relations then hold at most 256
  bytes a cell and `BytesPerRun` bytes, about 2.4 GB at the defaults, whatever the size of a value
  or the number of fields and joins that copy it. The run's decode cache keeps a budget of
  `BytesPerRun` bytes apart from that count: the key paths of every decode it makes take their
  bytes from it as they are built, and `key_parity` the tables it keeps of its files' keys, so the
  run's decodes together, not each alone, hold at most that many bytes of key paths.
- A hit is an analysis failure, never truncation.

**No finding cap** (D5b). A `constraint` rule reports every finding, as every existing kind does,
and each one is baselinable. Findings can outnumber input rows (an empty locale file gives one
finding per reference key), but only as far as the bounded inputs allow.

**Plan check.** Before the walk, the loader resolves every reference and rejects cycles. It checks:
- that each field named by `fields:`, `key:`, `determines:`, `group_by:`, `field:`, `to:`, `on:`,
  `where:`, `transform:` and `message:` exists;
- that a join declares `unique:`, and that every `union:` part binds every field;
- that `strip-root` and `plurals: cldr` apply to an `entry.path` field;
- literal types where they are known.

`explain` shows the plan with its cost class, linear in the rows of each input.

## 9. `--changed` (Q2)

A `constraint` rule's **trigger scope** is the union of the selectors of every relation its plan
reads: its input, both sides of each join, the relations its membership filters probe, and the
relation its `set` assertion compares against. Under `--changed` the rule runs when a changed path
(added, modified or deleted) matches that scope, and then evaluates the full snapshot, as a
full-index rule does today. It reports every finding, not only those located in changed files: a
change to one locale file can make another locale fail. A change to a configuration source alone
does not trigger a rule, as in v1.

A rename is a change to both of its paths: the changed set lists the old path as it lists a deleted
one, so a file renamed out of a selector triggers the rule.

A selector's `scope_filter:` narrows the trigger scope as it narrows what the selector reads, judged
on the tree after the change, so a change to what a predicate consults also triggers the rule: a
file a `has_ancestor` predicate names, in a directory at or below which the selector's `paths:` can
match, or the manifest a manifest-derived predicate reads. A manifest that cannot be read leaves its
predicate out of the trigger scope: a changed path that the selector's `paths:` and its other
predicates keep runs the rule, which reports the failure, and a change outside them (a `README.md`
alone) runs nothing and reports nothing. A selector that does not compile takes every path. An
ignore file the walk reads (a `.gitignore` or `.ignore`, in any directory) decides which files below
it are in the index, so a change to one triggers each selector whose `paths:` can match there.

The changed set is the paths Git reports, so three kinds of change that can alter a verdict trigger
nothing, as they skip a per-file rule or a version 1 full-index kind today; a full run covers them:
- an edit to the target of a symlink a selector reads through: with `locales/de.json` linked to
  `shared/de.json`, Git reports `shared/de.json` alone. Following links back from their targets
  would need the link-to-target map the walk does not keep, and a deleted target leaves a broken
  link the walk prunes, its target no longer known;
- with `respect_gitignore: false`, a file Git ignores, which the walk reads and Git never lists as
  untracked (an untracked file under a gitignored directory);
- a change to `.git/info/exclude`, to the global excludes file, or to an ignore file above the
  linted directory when alint lints a subdirectory of a repository.

A selector read by a full-index rule may not use `changed_since` (section 4), because what the rule
sees would then depend on the diff. The other `scope_filter:` predicates are allowed in those
selectors. A `constraint` rule takes no `scope_filter:` of its own, and the build-time rejection of
a rule-level `scope_filter:` on cross-file rules (`reject_scope_filter_on_cross_file`) stays.

This amends constitution invariant 2 and ADR-0003, as section 13.3 of the program foresaw: a
full-index rule may declare a path scope that decides **whether** it runs, never **what** it sees.
The five full-index kinds that already declare one (section 2) follow the same rule, so the
amendment records existing practice.
- The invariant's enforcing test, `v010_cross_file_kinds_require_full_index_and_no_path_scope`,
  asserts that the eight kinds and one alias it lists (`cross_file_value_equals`) need the full
  index and declare no path scope. It cannot cover a kind that declares one, so it is
  **replaced**, not extended: phase 7 adds a test over every full-index kind that asserts the rule
  receives the full index under `--changed`.
- `metadata_inventory_matches_the_audited_v019_shape` pins the five kinds that declare a scope.

## 10. Names, provenance and trust (Q12)

- **Each source has its own namespace** (D3). A bare name (`from: manifests`, `input: ranges`)
  resolves to an object the referencing source declares.
- **Qualified references.** A v2 source can give an `extends:` entry an alias and use that source's
  objects by qualified name:

  ```yaml
  extends:
    - { url: ./policies/deps.yml, as: deps }
  rules:
    - { id: one-range, kind: constraint, input: deps.ranges, assert: { ... } }
  ```

  A qualified name reaches only the objects the directly extended source declares itself: there is
  no re-export and no `a.b.c` chain. Names and aliases cannot contain `.`. `as:` is accepted in a v2
  source only, so v1's `{ url, only, except }` entry is unchanged.
- **No shadowing.** A name declared twice in one source, or an alias used twice, is a load error.
  An extended source's rules keep resolving to its own objects whatever the consumer declares, and
  a consumer reaches an extended source's object only by its qualified name. A rule that a consumer
  overrides by id resolves each overridden field in the source that wrote it (the existing
  field-source marker).
- **Provenance.** Every named object records its source, and a qualified reference its alias;
  `list` and `explain` show both.
- **Trust and capability monotonicity.** Each object passes the gates of its declaring source's
  trust class when it is declared, as an extended source's rules do today. A rule's capabilities
  are its own plus those of everything it references (a relation reads file content; no selector
  predicate needs Git in v0.23, since `changed_since` is refused there), and the gates judge the
  rule after resolution. A reference only points into the same source or a source it extends, so a
  source never reaches the objects of a source that extends it.
- **Local sources only in v0.23** (D3a). v2 sources may be the top-level config, local `extends:`
  and `.alint.d` fragments. Bundled and remote sources must be `version: 1`, so qualified references
  only cross local, trusted files. Remote v2 sources need their own trust review.
- **No `inputs:`, and names stay literal.** In a v2 source, names, aliases and references
  (`from:`, `input:`, `left:`, `right:`, `as:`, field names, `key:`, `group_by:`, `on:`) stay
  literal; `{{...}}` in any of them is a load error (section 7.1 rule 11 of the program). Other
  values interpolate `{{env.X}}` as in v1, where every local source is interpolated except a rule's
  or template's `id`, `kind` and `level`. `{{env.X}}` in a v1 `paths:` already routes the
  environment into selection, and untrusted sources are refused it.

## 11. Schema, commands, generated docs and counts (section 7.8)

- **Schema.** `schemas/v2/config.json` is a draft that `gen-schema` derives from the v1 schema plus
  the v2 additions, with a versioned `$id` (it names the release, not `main`). The v1 schema keeps
  its `$id` and keeps gaining new v1 kinds until v1.0; freezing it now would block DEC-8's named v1
  kinds.
- **Version dispatch before decoding** (section 3) also gives v2 errors the right message: an
  unknown v2 key is reported against the v2 surface, not as an unknown v1 field.
- **Registration.** `constraint` is registered with a minimum config version of 2, so a v1 source
  that uses it gets an error naming version 2. The checks that cover every registered kind become
  version-aware:
  - `coverage_audit_schema_drift` checks `constraint` against the v2 schema, not
    `schemas/v1/config.json`;
  - a v2 fixture beside `all_kinds.yaml` exercises it in the ADR-0012 gates;
  - `alint rules list` and `facts.json` mark it v2-only and experimental;
  - a `constraint` nested under a `for_each_*` kind's `require:` is a load error in v0.23.
- **Commands:**
  - `validate-config` validates both versions and reports the version, with the experimental
    warning for v2 (an additive JSON field);
  - `list` lists named selectors and relations after the rules (additive);
  - `explain <rule>` shows a `constraint` rule's resolved plan. The existing `selectors:` line
    (a kind's selector fields), which `explain` and `alint rules show` both print, is renamed
    `selector fields:` in human output only; its JSON key is unchanged;
  - `export-agents-md` covers v2 rules and lists named objects by name;
  - `init` and `suggest` keep emitting v1. `suggest --format yaml` keeps writing a snippet without
    `version:`: its documented use appends the snippet to an existing config, where a second
    `version:` would be a duplicate key, and a v1 snippet is valid in a v2 config.
- **Generated docs.** `constraint` is one kind with one rule page and a section per family, marked
  v2-only and experimental. The headline count gains one kind; families are not kinds and do not
  count (constitution invariant 9).
- **Facts.** `facts.json` gains `compatibility.experimental_config_versions: [2]` (additive).
  `config_versions` stays the list of stable versions, `[1]`.

## 12. The three conformance policies

Illustrative v2 configurations; phase 8 writes the real ones with `alint test` cases and the section
12.1 measurements.

**Dependency version consistency** (jest-23, trpc-05, yarnpkg-berry-01). Each unit uses the lines
its comment names; phase 8 writes one configuration per repository.

```yaml
version: 2
selectors:
  workspace-manifests:
    # yarnpkg-berry-01 leaves packages/docusaurus out
    paths: ["packages/*/package.json", "!packages/docusaurus/package.json"]
relations:
  workspace-names:
    from: workspace-manifests
    read: { json: "$.name" }
    fields: { name: value }
  dependency-ranges:
    from: workspace-manifests
    entries:
      format: json
      at: ["$.dependencies", "$.devDependencies", "$.optionalDependencies"]
    fields: { field: entry.parent, dependency: entry.key, range: entry.value }
    where:
      # trpc-05 skips these specifiers
      range: { not_matches: "^(catalog|npm|workspace):" }
      # jest-23 exempts @types/node; yarnpkg-berry-01 exempts workspace packages
      dependency: { not_in: ["@types/node", { relation: workspace-names, field: name }] }
rules:
  - id: one-range-per-dependency
    kind: constraint
    input: dependency-ranges
    assert: { unique: { key: [dependency], determines: [range] } }
    level: error
```

**Locale key parity** over merged per-locale trees (forem-01). ghost-02 compares each namespace
separately, which needs a group of two fields (locale and namespace), not in v0.23, so it keeps its
five `key_parity` rules.

```yaml
version: 2
selectors:
  locale-files: { paths: "config/locales/**/*.yml" }
relations:
  locale-keys:
    from: locale-files
    entries: { format: yaml, at: "$", leaves: true }
    fields: { locale: entry.root, key: entry.path }
    transform: { key: [strip-root] }
    where: { key: { not_matches: "^(date|devise|views)\\." } }
rules:
  - id: locale-key-parity
    kind: constraint
    input: locale-keys
    assert:
      set:
        group_by: locale
        field: key
        equal_to: { group: en }
        groups: [fr, pt]
        plurals: cldr
    level: error
```

**Toolchain-pin consistency** (signal-desktop-28). A part whose path is absent yields a missing row,
which differs from `.nvmrc`'s value, so a removed `volta.node` fires, as the v1 rule's "target query
matched nothing" does.

```yaml
version: 2
relations:
  node-pins:
    union:
      - { from: { paths: .nvmrc }, read: { regex: "^v?(\\S+)" } }
      - { from: { paths: package.json }, read: { json: "$.engines.node" } }
      - { from: { paths: package.json }, read: { json: "$.devEngines.runtime.version" } }
      - { from: { paths: package.json }, read: { json: "$.volta.node" } }
    fields: { source: path, version: value }
rules:
  - id: node-pins-agree
    kind: constraint
    input: node-pins
    assert: { equal: { field: version, to: { row: { source: .nvmrc } } } }
    level: error
```

Section 12.1 requires that a policy with a v1 spelling keep the shorter one: toolchain pins stay a
`cross_file` `equals`, and per-file locale parity stays `key_parity`. The v2 forms are for the cases
v1 cannot express (merged trees, one range per dependency) and for the measurements.

## 13. Corrections to the records

The plan and `general-linter.md` are corrected with this document. Accepted ADRs are not edited,
so this list is the correction of ADR-0033's and ADR-0034's wording. ADR-0037 records the
corrections that change what an ADR-0034 decision says.
- `general-linter.md` section 7.8 said the loader drops an extended source's `version:` unchecked
  (stale since v0.20) and that editors point at `schemas/v1` on `main` (only its `$id`, examples
  and `ARCHITECTURE.md` do). Section 7.8 now points at this document's answers.
- Section 7.4 names operators `functional_dependency` and `sets_equal`; the slice spells them
  `unique` with `determines:` and `set`. Section 7.4 now says so.
- Section 7.2 sketches `inputs:` and a selector `where: { tracked: true }`, and the section 13.3
  row for ADR-0008 said Phase 3 adds a selector-level `tracked` predicate. This design adds neither,
  and its `where:` filters a relation's rows. Section 7.2 now points here, and the row says
  ADR-0008 stands.
- Plan item 3 and ADR-0033 say "the `entry` adapter" emits TOML values; TOML rows come from
  `TomlRowsAdapter`. Plan item 3 now says so.
- Plan section 2 sized the relation layer at "about 3,600 lines" (3,573 without `adapters/`, 4,724
  with them at `a04548a74`; 3,642 and 4,793 at `14786ee2f`), and called toolchain-pins "41 rules,
  mostly `cross_file`" (24 rules, 18 of them `cross_file`, plus 17 facts).
- General-linter section 12.1 requires that a finding reach all its evidence in SARIF and the
  language server; it states a requirement, not today's coverage.
  - ADR-0012's gates cover what `explain`, `list` and `export-agents-md` show (driven by
    `all_kinds.yaml`), and its W4 gate that notes reach every check format, SARIF included
    (`crates/alint/tests/cli_format_contract.rs`).
  - No gate covers related locations. Human, SARIF and language-server related locations have unit
    tests, and the JSON format has no related-location field.
  - Phase 7 adds that field to JSON (additive) and a completeness gate for `constraint` findings
    over JSON, SARIF and the language server. Plan section 5 now says so; junit and the github
    and gitlab formats carry one location or none, so the gate does not cover them.
- Section 7.3's locale relation emits values; `key_parity` quotes keys only, and the slice's
  locale policy needs keys only. Section 7.3 now says so.
- ADR-0034's context says four stratum units compare fields across matched rows. Eleven records
  describe a join on a key (section 8), and fluentui-18 needs a `reference` assertion and a value
  derived from the target's version, not version comparison.
- ADR-0034's consequences list the documents that changed with it. #512 also changed plan sections
  2 and 5, the roadmap's v0.23 section and the design index.
- Constitution invariant 2, as #512 worded it, said its test "checks nine kinds" and is extended to
  every full-index kind. The test lists eight kinds and one alias and asserts that each declares no
  path scope, the opposite of the amended rule, so it is replaced (section 9). The invariant now
  names each enforcement, including the build-time `scope_filter:` rejection #512 left out.

## 14. Out of scope

v0.24 or later: `cardinality`, `reference` (with nextcloud-b02's `$<name>` check), group and bounded
count; generic fixes; many-to-many and outer joins; groups of several fields; comparing a field
with a literal; bundled, remote and nested v2 sources, and a nested `constraint`; re-exported and
chained qualified names; `inputs:`; `changed_since` in selectors; typed templates; a typed `when`;
selectors in v1 kinds' `paths:`; XML value rows; structured values; a finding cap; and a stable
`version: 2`.

Phase 6 delivers the envelope, selectors, relations, value-bearing entry rows, the equality join,
per-source namespaces with `extends:` aliases, the draft schema and the commands; phase 7 the
`constraint` kind, with row-level `equal`, `plurals: cldr` and the related-location gate; phase 8
the three policies with their `alint test` cases. Each has e2e firing and silent scenarios
(invariant 8), a schema entry (invariant 7), the ADR-0012 gates, and the v0.18.2 performance budget
for relation pull requests (ADR-0024 decision 6).

## 15. Decisions

The owner took each decision on 2026-11-13 and 2026-11-14, one at a time, with the options and their
tradeoffs. ADR-0034 records D1 to D9. ADR-0037 records D10 and D11, which the owner took on
2026-11-14 after an audit of this document. Two outcomes of D1 to D9 differ from the
recommendation, and the outcome column says so.

| ID | Decision | Outcome |
|---|---|---|
| D1 | Where selectors and relations live (Q1) | Top-level and inline; `path_sets:` is this feature; only relations reference selectors in v0.23 (section 4) |
| D2 | `--changed` (Q2) | The plan's selectors decide whether the rule runs; it judges the full snapshot and reports every finding; invariant 2 and ADR-0003 are amended (section 9) |
| D3 | Names across sources (Q12) | **Qualified references** through an `extends:` alias (`deps.ranges`), not the recommended per-source resolution without them (section 10) |
| D3a | Which sources may be v2 | Local files only (section 10) |
| D4 | Entry rows (ADR-0033 decision 2) | Named fields with provenance; scalar values; non-scalars and TOML datetimes fail loud; XML key-only (section 5) |
| D5 | Joins (Q6) | **An equality join in v0.23**, beside grouping and semi-joins, not the recommended semi-joins only (section 8) |
| D5a | Repeated join keys | One side must be unique on the join key, checked at evaluation (section 8) |
| D5b | A finding cap | None; every finding is reported (section 8) |
| D6 | `when` and row filters (Q15) | `when` unchanged; structural `where:`; a fixed transform list (section 7) |
| D7 | Schema, commands, docs (7.8) | A draft v2 schema with a release `$id`; the v1 schema evolves until v1.0; `constraint` is one kind with one page (section 11) |
| D8 | Finding shape | One finding per (key, value) for `unique`, per differing row against a source, per missing or extra value for `set` (section 6) |
| D9 | The version boundary (DEC-9) | v2 is a superset of v1, read and dispatched before decoding, experimental until declared stable (section 3) |
| D10 | Using the join's output (ADR-0037) | `equal` gains a row-level form, `to: { field: G }`, one finding per differing row at `field:` with `G` related; nextcloud-b02's equality part is expressible in v0.23 (sections 6 and 8) |
| D11 | CLDR plural collapse (ADR-0037) | `plurals: cldr`, an option of `set`, decided per pair of groups by `key_parity`'s rules; the transforms are `trim`, `lower`, `semver-major`, `semver-minor` and `strip-root`, on a relation's `transform:` before `where:` (section 6) |

## 16. False-positive surface

What could fire wrongly, and what keeps it from doing so. TEMPLATE.md requires this section for a
behavior-bearing kind (constitution invariant 12).

- **Plural groups.** A locale that has fewer plural categories than the reference would report
  each missing category. `plurals: cldr` merges a group only when both sides allow it, so a real key
  named `one` or `other` is never merged. `where:` runs before the merge, unlike `key_parity`'s
  `ignore_keys`, which runs after it: a filter that removes some members of a plural object changes
  the merge. Prefix filters, such as the locale policy's, remove whole subtrees. Phase 8 compares
  the v2 locale policy with `key_parity` on the stratum.
- **Keys placed in different files per locale.** Per-file parity reports them (forem-01's 114
  `key_parity` rules give 8 false positives at its pin); merged trees do not, because a group is a
  locale, not a file.
- **Specifier spellings.** `unique` compares range lexemes after transforms, so `^1.0.0` and
  `^1.0` disagree although they allow the same versions. The policy therefore states "one range
  spelling per dependency". A non-semver specifier the filter does not list (`file:`, `link:`,
  `portal:`, `patch:`, a Git URL) is compared as a range.
- **Exemptions broader than the source rule.** The dependency example exempts workspace packages
  in every field, where yarnpkg-berry-01 exempts dev dependencies only. At berry's pin the two
  agree (section 7). Where they would differ, the example misses findings rather than adding false
  ones.
- **Types.** A number never equals a string in `equal`, so JSON `18` and `"18"` differ; the finding
  shows both types. A key or membership type mismatch is an analysis failure, never a silent drop.
- **Moved findings.** A `constraint` finding's identity leaves the primary path out, so adding or
  renaming the resource that sorts first does not resurface baselined findings (section 6).
- **YAML features.** Anchors, aliases, merge keys and complex keys are parse failures, never
  findings. forem's 171 locale files at its pin use none of them (checked for this document).
- **Join keys.** A repeated key on a join's unique side is an analysis failure, never a silent
  choice of one row.
- **Missing values.** A `read:` that matches nothing yields a missing row that `equal` reports.
  A pin that a repository writes in only some of its files needs `optional: true` on that part,
  or it fires.
