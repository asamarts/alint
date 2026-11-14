---
status: accepted
date: 2026-11-14
decision-makers: asamarts
---

# 0037. Phase 3 refinements: row-level equality, CLDR plurals in `set`, and ADR-0034 corrections

## Status

Accepted (2026-11-14). Amends [ADR-0034](0034-v2-version-boundary-and-phase-3-design.md) decisions
1 and 3 to 9 where listed below; the rest of ADR-0034 stands. The details are in
[`v0.23/selectors-and-constraints.md`](../design/v0.23/selectors-and-constraints.md), whose section
15 records the two new outcomes as D10 and D11.

## Context

- **An audit.** An adversarial audit of the Phase 3 design and ADR-0034 found 31 issues and four
  it could not confirm; #514 applies its fixes. Most correct the design document only. Two needed
  the owner's decision, and some change what an ADR-0034 decision says.
- **The join had no use.** ADR-0034 decision 6 added an equality join, but each assertion family
  compared one field across rows: within a group, against a named source row, or against another
  set. None compared two fields of one row, so no assertion could use the join's output.
  nextcloud-b02, which the design cited as needing only a join and equality, requires that an
  override of a direct dependency be a `$<name>` reference or the dependency's own spec.
- **The CLDR collapse is not a transform.** ADR-0034 decision 7 listed `cldr-plurals` among
  per-value transforms. `key_parity` decides plural groups over both files of a comparison
  (`pair_keys` in `alint-rules/src/key_parity.rs`): a group needs `other` on one side and only
  category-named leaves on every side, and a key that is real on either side is never merged. A
  transform on one value cannot see sibling keys or the other locale, so it would merge a real key
  named `one` or `other`, and phase 8's comparison with `key_parity` would differ.

The owner took decisions 1 and 2 on 2026-11-14, each with the options below.

## Decision

1. **`equal` compares two fields of one row.** A third form,
   `equal: { field: F, to: { field: G } }`, compares F with G in each row, after transforms.
   - It gives one finding per row whose F differs from its G, at F's location, with G's location
     as a related location. A join keeps each field's provenance.
   - Its identity is per row: `baseline_key` parts `equal`, `row`, F's resource path and F's
     pointer, encoded as decision 3 says.
   - With a join, it expresses nextcloud-b02's equality part: an override of a direct dependency
     that is not a `$` reference equals the dependency's spec. That a `$<name>` reference names a
     direct dependency is a `reference` assertion, which waits for v0.24.
2. **CLDR plurals are an option of `set`.** `plurals: cldr` is decided per comparison of the
   reference group with each group, by `key_parity`'s rules and code (`nested_plural_groups`,
   `i18next_plural_groups`):
   - nested CLDR objects and i18next `_<category>` siblings;
   - `other` present on at least one side, and every side's parent holding only category-named
     leaves;
   - for i18next, the base key existing on neither side.

   The per-value transforms are `trim`, `lower`, `semver-major`, `semver-minor` and `strip-root`.
   `strip-root` drops a key path's first segment and fails a resource whose keys do not all sit
   below one top-level key, as `key_parity`'s `strip_root` does. Transforms live on a relation's
   `transform:`, and a relation applies `transform:`, then `where:`. This replaces ADR-0034
   decision 7's transform list.
3. **Corrections that change an ADR-0034 decision:**
   - **Decision 8, identity.** A `constraint` finding's fingerprint leaves the primary path out:
     `sha256(lp(rule_id) ‖ lp("\0constraint") ‖ lp(baseline_key))`, because a finding's location
     (a key's first row, a group's first resource) moves when rows are added, removed or renamed.
     `baseline_key` is built from length-prefixed parts (a decimal length, a colon and the part),
     not joined with NUL, and a value part carries its type. This replaces decision 8's
     `unique\0<key values>\0<determined values>`.
   - **Decision 8, `set` groups.** The groups are the reference group, every group named in
     `groups:` and every value of the `group_by:` field; a path-token `group_by:` makes every
     selected resource a group. A group with no rows reports each reference value as missing, and
     a reference group with no rows is an analysis failure.
   - **Decision 3, invariant 2.** The invariant's test is replaced, not extended:
     `v010_cross_file_kinds_require_full_index_and_no_path_scope` asserts that the eight kinds and
     one alias it lists declare no path scope. `metadata_inventory_matches_the_audited_v019_shape`
     pins the five kinds that declare one, and the build-time rejection of `scope_filter:` on
     cross-file rules stays; `constraint` takes `scope_filter:` only in its selectors.
   - **Decision 3, `--changed`.** The trigger scope also covers the relation a `set` assertion
     compares against. A selector read by a full-index rule may not use `changed_since`. A
     configuration change alone triggers nothing, as in v1.
   - **Decision 6, joins.** `unique:` is required; it has no default.
   - **Decision 1, dispatch.** A `.alint.d` fragment without `version:` is decoded as its root's
     version. A non-literal `version:` is v1-only and must equal 1 after interpolation. Nested
     configs and `alint_dsl::parse` input must be `version: 1` in v0.23.
   - **Decision 5, entry rows.** A value's type is checked only when a field binds `entry.value`,
     so a key-only relation reads files with array values. `read:` takes only `json:`, `yaml:`,
     `toml:`, `regex:`, `lines:` and `whole_file:` in v0.23.
   - **Decision 4, names.** In a v2 source, names, aliases and references (`from:`, `input:`,
     `left:`, `right:`, `as:`, field names, `key:`, `group_by:`, `on:`) stay literal; `{{...}}`
     there is a load error.
   - **Decision 9, registration and facts.** `constraint` is registered with a minimum config
     version of 2. The checks over every registered kind become version-aware, a v2 fixture beside
     `all_kinds.yaml` exercises it, `alint rules list` and `facts.json` mark it v2-only, and a
     nested `constraint` is a load error in v0.23. `facts.json` gains
     `compatibility.experimental_config_versions: [2]`; `config_versions` stays `[1]`.

## Consequences

- **The join has a use in v0.23.** As ADR-0034's consequences said, it serves a policy beyond the
  slice's three, through row-level `equal`.
- **Phase 7 grows** by row-level `equal`, `plurals: cldr`, the version-aware kind checks, and a
  related-location completeness gate over JSON, SARIF and the language server.
- **`constraint` fingerprints are new.** DEC-5 freezes the fingerprints of existing kinds only.
- **Documents.** The design document, constitution invariant 2, general-linter sections 7.2 and
  13.3, the v0.23 plan (phase 7, section 5 and the status log) and ADR-0034's status record the
  change.

## Considered Options

- **Using the join's output:**
  - row-level `equal` (chosen);
  - the join only, carrying fields into grouping, filters and messages, with nextcloud-b02
    deferred to v0.24;
  - dropping the join to v0.24.
- **CLDR plural collapse:**
  - an option of `set`, decided per pair of groups by `key_parity`'s rules (chosen);
  - a simple per-value transform: cheaper, but it cannot see sibling keys or the other locale, so
    it merges real keys named for a category and differs from `key_parity`;
  - dropping plural collapse from v0.23: forem-01's check is plural-aware, so the locale policy
    would report categories a locale does not use.

## More Information

- [`v0.23/selectors-and-constraints.md`](../design/v0.23/selectors-and-constraints.md): sections
  5 to 9, 15 (D10, D11) and 16 (the false-positive surface).
- [ADR-0034](0034-v2-version-boundary-and-phase-3-design.md): the decisions amended here.
- [`constitution.md`](../design/constitution.md): invariant 2.
- [`v0.23/README.md`](../design/v0.23/README.md): the v0.23 plan.
