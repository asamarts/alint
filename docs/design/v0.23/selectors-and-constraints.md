# Design doc: named selectors, relations and the `constraint` kind (Phase 3)

Status: Accepted design (2026-11-14). The owner took each decision in section 15 at the phase 3
gate of the v0.23 plan; phases 6 to 8 implement it.
Decisions: [ADR-0034](../../adr/0034-v2-version-boundary-and-phase-3-design.md), which records the
version boundary (DEC-9) and each outcome in section 15.
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

These are facts about the code at `a04548a74` and the accepted records, not proposals.

**The relation layer** (`alint-core/src/relation/`) is `#[doc(hidden)]` and promises no stability.
v0.21 invariant 6 ("internal stays internal") keeps its types out of every documented API, report
and schema.
- Rows are `Row { resource, pointer, label, ordinal, span, state, original }` in a canonical order,
  with `Value` = null, bool, number (the lexeme is kept: `1.10` is not `1.1`), string, bytes, path,
  list or record (ADR-0024 decision 1).
- Adapters:
  - `EntryRowsAdapter` (JSON, YAML, dotenv, INI, properties) emits key rows **without values**;
  - `TomlRowsAdapter` emits TOML key rows;
  - the Android strings and resx adapters emit XML keys;
  - `ExtractAdapter` emits values without a pointer.
- Limits: rows per extractor (1M by default, 10M hard), rows per run (5M, 50M) and distinct values
  (100k, 10M). The join limits (`JoinInputs`, `JoinOutputs`) are reserved and enforced nowhere.
  Users cannot set any limit.
- Only `key_parity` and `cross_file` use the layer. `cross_file` decodes glob-matched files outside
  `DecodeCache`, so those decodes do not count toward the rows-per-run budget.

**The loader** reads `version:` only after a strict v1 decode (`RawConfig` has
`deny_unknown_fields`). `version: 2` with a `selectors:` key fails as "unknown field `selectors`",
not as an unsupported version, and `version:` may come from `{{env.X}}` (both probed). ADR-0022's
per-source checks run before merge, so the "drops an extended file's `version:`" in section 7.8 is
stale. Rules and templates merge flat by id. Trust markers (provenance, environment, sensitive
fields, field sources) exist on rules and templates only.

**`--changed`.** `FullIndex { path_scope: false }` rules, `cross_file` among them, always run on the
full index. Five full-index kinds (`file_exists`, `file_absent`, `key_parity`,
`max_files_per_directory`, `no_case_conflicts`) declare a path scope and are skipped when the diff
misses it, then judge the whole index. Constitution invariant 2 and ADR-0003 still say a full-index
rule declares no path scope, so the code already departs from them.

**Selection** is `paths:` (a glob, a list with `!` excludes, or `{include, exclude}`) plus
`scope_filter:` (`has_ancestor`, `changed_since`, and ADR-0010's manifest-derived membership).
ADR-0010 deferred a named `path_sets:` block and bound any successor: manifest values decide which
files a rule sees, never what it decides about them. ADR-0008 made `git_tracked_only` a kind option
because a universal option that a kind can ignore fails quietly.

**`when`** has one comparison per term, `or`/`and`/`not`, and `facts`, `vars`, `iter` and `env`.
`==` and `!=` across types return false (`when/eval.rs`), although the module doc says that is an
error, and `vars` are always strings.

**Tooling.** No JSON Schema validates a config at load; the committed schema serves editors and
tests. Its `$id` points at `main`. `validate-config`, `list`, `explain` and `export-agents-md`
handle rules only, and `explain` already prints a `selectors:` line (the kind's selector fields).
`init` writes `version: 1`; `suggest --format yaml` writes no `version:` (probed). `gen-facts`
counts canonical kinds (103) and aliases (11) separately.

## 3. The version boundary (DEC-9)

DEC-9 decided that `version: 2` ships before v1.0, so v1.0 freezes both versions. ADR-0032 made v2
experimental in v0.23. This section fixes what that means; ADR-0034 records it.

1. **The version is read first.** The loader reads the top-level `version:` from the YAML mapping
   before any typed decode and dispatches on it. A v2 source's `version:` must be the literal
   integer `2`. A v1 source keeps today's behavior, including `{{env.X}}`, because rule 10 forbids
   reinterpreting v1.
2. **v2 is a superset of v1 in v0.23.** A v2 source accepts every v1 top-level key with v1's
   meaning, plus `selectors:` and `relations:`, and its rules may use every v1 kind plus
   `constraint`. No v1 key changes meaning.
3. **The new surface is v2-only.** `selectors:`, `relations:` and `kind: constraint` in a v1 source
   are configuration errors that name the version they need.
4. **Experimental.** Loading a v2 source prints a warning, and `validate-config` reports it. Nothing
   about v2 is stable before a release declares it so (v0.24 at the earliest); v2 syntax may change
   between minor releases until then, and the CHANGELOG lists each change.
5. **The direction rule of ADR-0022 holds at every edge.** A v2 source may extend v1 and v2 sources;
   a v1 source never extends a v2 one.
6. **v2 sources are local files in v0.23** (section 10): the top-level config, local `extends:`
   and `.alint.d` fragments. Bundled and remote sources must be `version: 1`.
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
  selector is referenced, an inline one (a `paths:` value) may stand instead, and is anonymous.
- **`path_sets:` becomes this feature.** ADR-0010's manifest-derived membership is already a
  `scope_filter` predicate, so a named selector that uses it is the deferred `path_sets:` block.
  ADR-0010's boundary holds unchanged: a selector decides which resources a relation reads, never a
  verdict, a message or a fix.
- **Referenced by relations only in v0.23.** A v1 kind's `paths:` keeps its v1 form even in a v2
  source. Letting every kind take `paths: { selector: name }` touches every kind's `KindMeta` and
  the `--changed` scope of each, so it waits for demand.
- **No new predicates.** The three policies need none. A selector-level `tracked` predicate, which
  ADR-0008 rejected as a universal option, is not added; ADR-0008 stands.

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
- `entries:`: one row per member of each object `at` selects (a JSONPath subset that must select
  objects), or per leaf below it with `leaves: true`. JSON, YAML and TOML only.
- `read:`: the extract syntax `cross_file` already has (`json:`, `yaml:`, `toml:`, `regex:` and the
  others), one row per extracted value.
- `union:`: the rows of several such parts, each with its own `from:`, in the order written. The
  toolchain-pin policy reads `.nvmrc` with a regex and `package.json` with JSONPath in one relation.
- `join:`: the rows of two relations combined on equal declared fields (section 8).

**Fields.** A field takes one of:
- `path` (the resource's path), or a path token (`stem`, `dir`, `parent_name`, ...) as in nested
  rules;
- `entry.key`, `entry.value`, `entry.parent` (the key of the object `at` selected), `entry.root`
  (the first key of the document) or `entry.path` (the key path, rendered as `Pointer::render`);
- `value` for a `read:` row;
- `bound: <json/yaml/toml path>`, a per-resource value read once (a manifest's `$.name`), which must
  exist exactly once in each resource or the resource is an analysis failure.

**Value types.** Scalars only in v0.23: null, bool, number (lexeme kept) and string. An entry whose
value is an object or an array, or a TOML datetime, is an analysis failure naming the path, never a
silent string (section 7.1 rule 4). YAML stays strict: aliases, merge keys and complex keys are
parse failures, as in every relation today.

**Adapters.** Value-bearing rows are new adapter modes with their own adapter ids, so their decodes
never share a cache entry with the key-only modes. TOML gets its value mode in `TomlRowsAdapter`
(not the `entry` adapter, which rejects TOML; ADR-0033 and the plan say "the `entry` adapter" for
both).

**XML does not join in v0.23.** Android strings and resx stay key-only. The locale policy works on
JSON and YAML trees, and `key_parity` already covers XML catalogs in v1.

**Redaction.** dotenv and properties values are redacted by default (ADR-0019 decision 3), so those
formats emit no value rows. A field may be marked `sensitive: true`; its value never reaches a
message, the JSON report or a baseline.

**Provenance.** Each field keeps its resource, its pointer and its byte span (the key span for a
key, the value span for a value; YAML gives a value span only for a single-line token today). A
finding's location and related locations come from these.

**Limits.** Relation decodes go through `DecodeCache`, so the existing per-extractor, per-run and
distinct-value limits apply; a hit is an analysis failure, never truncation. The join limits are
section 8's.

**`--changed` and caches.** A relation is read once per run whatever number of rules use it (the
existing `ReadCache` and `DecodeCache` keyed by adapter id).

## 6. The `constraint` kind

`constraint` is one kind, full-index, report-only and without fixes (section 8.3 of the program).
It takes one relation as `input:` (a name, or an inline relation) and one assertion.

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

**The families** (ADR-0033 decision 1):
- **`equal`:** within each group (`group_by:`, optional), every row has the same value of a field;
  or every row equals the row from a named source (`to: { field: source, value: .nvmrc }`), as
  `cross_file`'s `equals` names a canonical source.
- **`unique`:** no two rows share `key:`; with `determines:`, rows that share `key:` agree on the
  determined fields (a functional dependency).
- **`set`:** the set of a field's values, per group, is a subset of, equal to, or disjoint from
  another set: another group of the same relation (`group_by: locale` against group `en`) or a field
  of another relation (section 8).

**Transforms.** A fixed list applied to a field before the assertion: `cross_file`'s `normalize`
values (`trim`, `lower`, `semver-major`, `semver-minor`) and `key_parity`'s CLDR plural collapse.
No user-defined functions.

**Findings** (D8). Every finding sits at a row's field (its resource and span), with related
locations at the rows it is compared with (capped at 20, with an omitted count, as today).
- **`unique` with `determines:`:** one finding per distinct determined value of a key whose rows
  disagree, at the first row in canonical order with that value. Its related locations are the
  other rows with that value and one row of each other value. React at `^18.2.0` in 498 manifests
  and at `^17.0.2` in 2 gives two findings.
- **`unique` without `determines:`:** one finding per row that repeats a key, related to the first
  row with that key.
- **`equal`:** without a named source, as `unique` with `determines:` on the compared field within
  each group; with a named source, one finding per row that differs from it, at that row, related to
  the source row.
- **`set`:** one finding per missing or extra value per group. A missing value sits at the group's
  first row and an extra one at the row that holds it, each related to the row it was compared
  with, as `key_parity` reports a missing key at the peer with the reference key related.

**Identity.** New kinds set `baseline_key` from sorted identity fields (DEC-5):
- `unique\0<key values>\0<determined values>` (or `\0<row path>\0<pointer>` without
  `determines:`): a range that appears later is a new finding even when the key's earlier
  disagreement is in the baseline, and fixing a range resolves exactly its finding;
- `equal\0<group>\0<value>`, or `equal\0<group>\0<row path>\0<pointer>` against a named source;
- `set\0<group>\0<value>\0<missing|extra>`.

Related locations never enter a fingerprint.

**Messages.** `{{row.FIELD}}` interpolates a field of the reported row; a `sensitive` field renders
as `<redacted>`. Without `message:`, each family has a generated message that quotes keys and
values, as `key_parity` and `cross_file` do.

## 7. `when` and row filters (Q15)

- **`when` is unchanged in v2.** It stays a rule gate with v1's grammar and evaluator. A typed
  evaluator and an expression syntax wait for a release with demand beyond these policies.
- **Row filters are structural.** A relation may have `where:`, a map from a field to predicates
  that all must hold: `in`, `not_in` (lists of literals), `matches`, `not_matches` (regexes), and
  membership in another relation's field (section 8). The policies need exactly these: jest-23
  exempts `@types/node`, trpc-05 skips `catalog:`, `npm:` and `workspace:` specifiers, and
  yarnpkg-berry-01 exempts dependencies on workspace packages.
- **The cross-type `==` stays as it is in v1.** Changing it would change v1 configurations. The
  doc comment in `when/mod.rs` that calls it an error is corrected instead (follow-up).

## 8. Joins and caps (Q6)

Three operations combine rows. All of them match on equality of declared fields, after each field's
transforms, and compare typed values (a number never equals a string).

- **Grouping** (`group_by`, `key`) hashes the rows of one relation.
- **Semi-joins.** A membership filter (`where: { dependency: { not_in: { relation: workspace-names,
  field: name } } }`) or a `set` assertion against another relation's field builds a lookup set of
  that field and probes it. A filter never emits more rows than it reads.
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
- **One side is unique on `on:`** (D5a): `unique: right`, the default, or `unique: left`. The engine
  checks it while it builds that side's lookup table; a repeated key is an analysis failure that
  names both rows. A join therefore emits at most as many rows as its other side, and its cost is
  linear and known from the plan. Many-to-many joins are not in v0.23; all four stratum units that
  need a join (fluentui-18, trpc-06, backstage-35, nextcloud-b02) join to a unique side.
- **Explicit fields.** `fields:` names every output field as `left.F` or `right.F`; nothing is
  copied implicitly. Each field keeps the provenance of the row it came from, so a finding
  reported at `range` sits in the manifest, with the matched workspace row as a related location.
- **Order.** Output rows follow the canonical order of the side that is not unique.
- **Composition.** `left:` and `right:` name relations of the same source or qualified ones (section
  10); a join may read another join, and the plan check rejects cycles. `where:` and `transform:`
  apply to a join's output fields as to any relation's.

The slice's assertions still need only grouping and semi-joins; the join serves policies beyond the
three in section 12 (nextcloud-b02 needs only a join and equality; the other three units also need
version comparison or a frontmatter reader, which are not in v0.23).

**Caps.** The reserved `JoinInputs` and `JoinOutputs` limits (1M rows by default, 10M hard, in
`relation/limits.rs`) are enforced for the first time. `JoinInputs` caps the rows built into one
lookup table (a join's unique side, or a semi-join's set). `JoinOutputs` caps the rows one join
emits. Grouping is bounded by the existing rows-per-run budget (5M), not by `JoinInputs`, so a
relation of 100 locales with 20,000 keys each (2M rows) still works. A hit is an analysis failure,
never truncation.

**No finding cap** (D5b). A `constraint` rule reports every finding, as every existing kind does,
and each one is baselinable. Findings can outnumber input rows (an empty locale file gives one
finding per reference key), but only as far as the bounded inputs allow.

**Plan check.** Before the walk, the loader resolves every reference and rejects cycles. It checks
that each field a `key`, `group_by`, `on:`, `where:`, `fields:` or `message:` names exists, that a
join declares its unique side, and literal types where they are known. `explain` shows the plan
with its cost class, linear in the rows of each input.

## 9. `--changed` (Q2)

A `constraint` rule's **trigger scope** is the union of the selectors of every relation its plan
reads: its input, both sides of each join, and the relations its membership filters probe. Under
`--changed` the rule runs when a changed path (added, modified or deleted) matches that scope, and
then evaluates the full snapshot, as a full-index rule does today. It reports every finding, not
only those located in changed files: a change to one locale file can make another locale fail.

This amends constitution invariant 2 and ADR-0003, as section 13.3 of the program foresaw: a
full-index rule may declare a path scope that decides **whether** it runs, never **what** it sees.
The five full-index kinds that already declare one (section 2) follow the same rule, so the
amendment records existing practice; the invariant's enforcing test, which checks nine kinds, is
extended to every full-index kind.

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
  trust class when it is declared, as an extended source's rules do today. A rule's capabilities are
  its own plus those of everything it references (a selector with `changed_since` needs Git; a
  relation needs content reads), and the gates judge the rule after resolution. A reference only
  points into the same source or a source it extends, so a source never reaches the objects of a
  source that extends it.
- **Local sources only in v0.23** (D3a). v2 sources may be the top-level config, local `extends:`
  and `.alint.d` fragments. Bundled and remote sources must be `version: 1`, so qualified references
  only cross local, trusted files. Remote v2 sources need their own trust review.
- **No `inputs:`.** `changed_since: {{env.X}}` stays the one route from the environment into
  selection, already refused to untrusted sources.

## 11. Schema, commands, generated docs and counts (section 7.8)

- **Schema.** `schemas/v2/config.json` is a draft that `gen-schema` derives from the v1 schema plus
  the v2 additions, with a versioned `$id` (it names the release, not `main`). The v1 schema keeps
  its `$id` and keeps gaining new v1 kinds until v1.0; freezing it now would block DEC-8's named v1
  kinds.
- **Version dispatch before decoding** (section 3) also gives v2 errors the right message: an
  unknown v2 key is reported against the v2 surface, not as an unknown v1 field.
- **Commands:**
  - `validate-config` validates both versions and reports the version, with the experimental
    warning for v2 (an additive JSON field);
  - `list` lists named selectors and relations after the rules (additive);
  - `explain <rule>` shows a `constraint` rule's resolved plan. The existing `selectors:` line
    (a kind's selector fields) is renamed `selector fields:` in human output only; its JSON key is
    unchanged;
  - `export-agents-md` covers v2 rules and lists named objects by name;
  - `init` and `suggest` keep emitting v1. `suggest` starts writing `version: 1`, which a
    standalone config needs (follow-up, a v1 output fix).
- **Generated docs.** `constraint` is one kind with one rule page and a section per family, marked
  v2-only and experimental. The headline count gains one kind; families are not kinds and do not
  count (constitution invariant 9).
- **Facts.** `facts.json`'s `config_versions` lists 2 as experimental.

## 12. The three conformance policies

Illustrative v2 configurations; phase 8 writes the real ones with `alint test` cases and the section
12.1 measurements.

**Dependency version consistency** (jest-23, trpc-05, yarnpkg-berry-01):

```yaml
version: 2
selectors:
  workspace-manifests: { paths: "packages/*/package.json" }
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
      range: { not_matches: "^(catalog|npm|workspace):" }
      dependency: { not_in: { relation: workspace-names, field: name } }
rules:
  - id: one-range-per-dependency
    kind: constraint
    input: dependency-ranges
    assert: { unique: { key: [dependency], determines: [range] } }
    level: error
```

**Locale key parity** over merged per-locale trees (forem-01; ghost-02's per-file form too):

```yaml
version: 2
selectors:
  locale-files: { paths: "config/locales/**/*.yml" }
relations:
  locale-keys:
    from: locale-files
    entries: { format: yaml, at: "$", leaves: true }
    fields: { locale: entry.root, key: entry.path }
    transform: { key: [strip-root, cldr-plurals] }
    where: { key: { not_matches: "^(date|devise|views)\\." } }
rules:
  - id: locale-key-parity
    kind: constraint
    input: locale-keys
    assert: { set: { group_by: locale, value: key, equal_to_group: en } }
    level: error
```

**Toolchain-pin consistency** (signal-desktop-28):

```yaml
version: 2
relations:
  node-pins:
    union:
      - { from: { paths: ".nvmrc" }, read: { regex: "^v?(\\S+)" } }
      - from: { paths: "package.json" }
        read: { json: ["$.engines.node", "$.devEngines.runtime.version", "$.volta.node"] }
    fields: { source: path, version: value }
rules:
  - id: node-pins-agree
    kind: constraint
    input: node-pins
    assert: { equal: { field: version, to: { field: source, value: .nvmrc } } }
    level: error
```

Section 12.1 requires that a policy with a v1 spelling keep the shorter one: toolchain pins stay a
`cross_file` `equals`, and per-file locale parity stays `key_parity`. The v2 forms are for the cases
v1 cannot express (merged trees, one range per dependency) and for the measurements.

## 13. Corrections to the records

The plan and `general-linter.md` are corrected with this document. Accepted ADRs are not edited,
so this list is the correction of ADR-0033's wording.
- `general-linter.md` section 7.8 said the loader drops an extended source's `version:` unchecked
  (stale since v0.20) and that editors point at `schemas/v1` on `main` (only its `$id`, examples
  and `ARCHITECTURE.md` do). Section 7.8 now points at this document's answers.
- Section 7.4 names operators `functional_dependency` and `sets_equal`; the slice spells them
  `unique` with `determines:` and `set`. Section 7.4 now says so.
- Plan item 3 and ADR-0033 say "the `entry` adapter" emits TOML values; TOML rows come from
  `TomlRowsAdapter`. Plan item 3 now says so.
- Plan section 2 sized the relation layer at "about 3,600 lines" (3,573 without `adapters/`, 4,724
  with them), and called toolchain-pins "41 rules, mostly `cross_file`" (24 rules, 18 of them
  `cross_file`, plus 17 facts).
- Section 12.1 and plan section 5 say the ADR-0012 gates cover SARIF and the language server; they
  cover the JSON and human formats, and SARIF and LSP related locations are unit-tested only.
  Phase 7 adds `constraint` findings to the gates and a SARIF case, and plan section 5 now names
  that work.
- Section 7.3's locale relation emits values; `key_parity` quotes keys only, and the slice's
  locale policy needs keys only. Section 7.3 now says so.

## 14. Out of scope

v0.24 or later: `cardinality`, `reference`, group and bounded count; generic fixes; many-to-many
and outer joins; bundled and remote v2 sources; re-exported and chained qualified names; `inputs:`;
typed templates; a typed `when`; selectors in v1 kinds' `paths:`; XML value rows; a finding cap; and
a stable `version: 2`.

Phase 6 delivers the envelope, selectors, relations, value-bearing entry rows, the draft schema and
the commands; phase 7 the `constraint` kind; phase 8 the three policies with their `alint test`
cases. Each has e2e firing and silent scenarios (invariant 8), a schema entry (invariant 7), the
ADR-0012 gates, and the v0.18.2 performance budget for relation pull requests (ADR-0024 decision 6).

## 15. Decisions

The owner took each decision on 2026-11-13 and 2026-11-14, one at a time, with the options and their
tradeoffs. ADR-0034 records them. Two outcomes differ from the recommendation, and the outcome
column says so.

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
