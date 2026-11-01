---
title: Rule authoring
description: 'The checklist for adding a rule kind, bundled ruleset, or rule-kind alias to alint so the coverage audits stay green and CI passes.'
---

This page is the checklist for adding a new rule kind, bundled ruleset, or
rule-kind alias. Follow it and the coverage audits stay green, so CI passes on
the first push.

The path every new rule kind, ruleset, or alias takes:

<likec4-view view-id="addRuleKindFlow"></likec4-view>

## Two-layer enforcement

Two independent layers keep the rule surface honest, and both run in CI.

| Layer | Tool | Catches |
|---|---|---|
| 1. File presence | `alint check .` (the root `.alint.yml`, run by the `action-selftest.yml` workflow on every push) | Missing source files, missing scenario YAMLs, missing bundled-ruleset coverage. Fast feedback while you develop locally. |
| 2. Semantic | `cargo test -p alint-e2e` (the `coverage_audit_*.rs` integration tests) | Pass/fail symmetry, alias-aware kind coverage, bundled-ruleset symmetry, git-mode symmetry, and registry-to-schema-to-docs consistency. |

A PR that adds a rule kind with no e2e scenario fails layer 1 at lint time. A PR
that adds only a passing scenario, with no firing counterpart, fails layer 2 at
test time.

## Adding a new rule kind

1. Implement the rule in `crates/alint-rules/src/<kind>.rs` as a `Rule` or
   `PerFileRule` impl.
2. Register it in `register_builtin()` in `crates/alint-rules/src/lib.rs`.
3. Add a minimal entry for the kind to
   `crates/alint-dsl/tests/fixtures/all_kinds.yaml`. This fixture is the
   canonical enumeration of every kind; the config schema, `facts.json`, the
   LikeC4 model, and the headline counts are all derived from it. The registry
   guard in `crates/alint-dsl/tests/schema.rs` fails and prints the missing kind
   name until the entry exists.
4. Document the kind in `docs/rules.md`, under its family (an `##` heading) as an
   `###` section that contains at least one fenced ` ```yaml ` usage example.
   The reference page at `alint.org/docs/rules/<family>/<kind>/` is sliced from
   this section, and the docs build fails if the example is missing. Directly
   under the `###` heading add a `**Categories:**` line listing the kind's
   categories, PRIMARY FIRST — the primary is the family it sits under. Most kinds
   are single-category (just the family); give a cross-cutting kind its
   secondaries too, e.g. `**Categories:** Content, Security / Unicode sanity`.
   `gen-categories` (step 7) parses this line; its gate enforces primary-first, a
   three-category cap, and the closed `Category` vocabulary. See
   `docs/design/rule-categories.md` and the curated table in
   `docs/design/rule-categories-assignments.md`.
5. Add e2e scenarios:
   - `crates/alint-e2e/scenarios/check/<family>/<kind>_pass.yml`
   - `crates/alint-e2e/scenarios/check/<family>/<kind>_fires.yml`
6. If the rule reads git state (`git_tracked_mode()` returns a non-`Off`
   `GitTrackedMode`, or `wants_git_blame()` returns `true`), add scenarios that
   cover both modes:
   - `<family>/<kind>_in_repo.yml` (with `given.git.init: true`)
   - `<family>/<kind>_no_op_outside_git.yml` (no `given.git` block)
7. Regenerate the derived contracts and run the audits:
   ```text
   cargo run -p xtask -- gen-categories # kind->category bridge (from **Categories:**)
   cargo run -p xtask -- gen-facts      # counts + catalogues + KindMeta inventory
   cargo run -p xtask -- gen-schema     # config schema (every canonical kind)
   cargo run -p xtask -- gen-model      # LikeC4 model fragments
   cargo test -p alint-e2e
   ```
   Every `coverage_audit_*` test should pass. The `gen-* --check` gates in CI and
   the local preflight fail if any committed contract has drifted from the code.

### Options and the config schema

Every canonical kind derives its schema option surface from its Rust option
model. Do not hand-edit generated option properties in
`schemas/v1/config.json`:

1. Add `#[derive(schemars::JsonSchema)]` to the struct, with `///` docs on each
   field and any `#[schemars(range/length/regex)]` constraints. An optionless
   kind uses the shared exact-empty-object schema.
2. Register the schema in `rule_option_schemas()` in
   `crates/alint-rules/src/lib.rs`.
3. For a new kind, add its descriptive base branch: kind discriminator,
   description, and any branch-level cross-field constraint. Generation
   replaces its option properties and applies `KindMeta` common fields. The
   recursive nested-rule schema reuses this generated branch automatically.
4. Run `cargo run -p xtask -- gen-schema` to regenerate the schema (both the root
   copy and the in-crate copy).

CI and preflight run `gen-schema --check`, which rejects omissions, extra kind
branches, KindMeta/schema capability drift, and committed output drift.

The `///` field docs do double duty. `xtask docs-export` reads the type-derived
`$defs/rule_<kind>` branch and renders an `## Options` table (name, type,
required, default, description) onto the rule's page automatically, so you write
each field's doc once. The `committed_schema_every_branch_renders_a_clean_table`
test in `xtask/src/rule_options_table.rs` fails if an option shape cannot be
placed into a clean table cell.

### Regenerating the surface-area contracts

Adding a rule kind moves a surface-area count, so `facts.json` has to be
regenerated with `cargo run -p xtask -- gen-facts`. That file holds the version,
the seven headline counts, catalogue lists, and canonical `KindMeta` inventory
that the README, docs, and alint.org consume. CI and the docs script run
`gen-facts --check` and fail on drift. The same applies when you add a family,
bundled ruleset, fix operation, output format, subcommand, or metadata field.

`cargo run -p xtask -- gen-categories` regenerates the in-crate kind-to-category
bridge (`crates/alint-rules/src/categories_gen.rs`) from the `**Categories:**`
lines in `docs/rules.md`. The CLI (`alint rules`, `alint list --category`) reads
it, and `facts.json` carries the same associations, so run `gen-categories` before
`gen-facts`. `gen-categories --check` gates it; run it after step 4 whenever you
add a kind or edit a `**Categories:**` line.

`cargo run -p xtask -- gen-model` refreshes the LikeC4 model fragment that
carries the rule-kind taxonomy (`docs/design/architecture/model/rule-families.gen.c4`),
which it derives from the `docs/rules.md` heading structure. Run it after step 4.

Run `cargo run -p xtask -- gen-arch` only when you add or remove a crate or an
intra-workspace dependency. It refreshes the crate dependency graph
(`docs/design/architecture/crate-graph.md`) and its model fragment;
`gen-arch --check` gates them.

### Policy-test directory conventions

General rule tests live under `crates/alint-e2e/scenarios/check/<family>/`.
Bundled-ruleset tests live under
`crates/alint-dsl/ruleset-tests/v1/<bundle>/`. Audits walk both trees recursively.

Downstream rulesets use the same public contract under `alint-tests/`. Run `alint test` from the
package root, pass files/directories explicitly for another layout, and use
`alint test --update-snapshots` only after reviewing inline expectation changes. Recursive
discovery requires a top-level `test_version` discriminator, so YAML owned by `given.fixture` or a
local config chain is not mistaken for another test; explicitly named files are always validated.
Command-spawning tests require both `capabilities: { spawn: true }` and the explicit
`--allow-spawn` host grant.

- `content/` content scanning (`file_content_matches`, `file_content_forbidden`,
  `file_header`, `file_footer`, `no_trailing_whitespace`, `final_newline`,
  `line_endings`)
- `cross_file/` relational and fan-out rules (`for_each_dir`, `for_each_file`,
  `pair`, `unique_by`, `every_matching_has`, `dir_contains`)
- `encoding/` Unicode and byte checks (`no_bom`, `no_bidi_controls`,
  `no_zero_width_chars`, `file_is_text`, `file_is_ascii`)
- `existence/` presence and absence (`file_exists`, `file_absent`, `dir_exists`,
  `dir_absent`)
- `git/` git-aware rules (`git_blame_age`, `git_commit_message`,
  `git_no_denied_paths`)
- `metadata/` file-property checks (`file_min_size`, `file_max_size`,
  `file_min_lines`, `file_max_lines`, `file_hash`)
- `naming/` filename shape (`filename_case`, `filename_regex`)
- `scope_filter/` per-file scope-narrowing behaviour
- `security/` Trojan-source defence and denylists
- `structure/` repository layout (`max_directory_depth`,
  `max_files_per_directory`, `no_empty_files`)
- `structured/` JSONPath, JSON-Schema, YAML, and TOML queries
- `unix_metadata/` permission and symlink shapes
- `when_facts/` and `when_iter/` `when:` expression coverage
- `interactions/` multi-rule scenarios

If your rule does not fit an existing family, add a new directory.

### Policy-test shape

Every declarative test uses the versioned public contract:

```yaml
test_version: 1
name: <human-readable description>
tags: [check, <kind>, <family>, passing|failing]

given:
  tree:
    <path>: <content>            # files; nested maps create directories
  config: |
    version: 1
    rules:
      - id: <test-rule-id>
        kind: <kind>
        paths: <glob>
        level: <level>

when: [check]                    # or [fix]

expect:
  - exit: success                # policy_failure for error-level findings
    violations: []               # a passing case
```

Each `when` entry has one `expect` entry. Present channels are exact; omitted channels are
unasserted. A firing check normally uses `exit: policy_failure` and a non-empty `violations`
list. See the [policy-test contract](../design/v0.20/policy-tests.md) and generated
[`schemas/v1/policy-test.json`](../../schemas/v1/policy-test.json) for fixtures, capabilities,
matchers, fixes, final-tree assertions, and snapshots.

For git-aware rules, add `given.git: { init: true, add: [<paths>], commit: true }`.

### Native integration tests

The policy-test tree model supports regular files, executable files, confined file/directory
symlinks, isolated Git histories, backdated commits, environment input, and real fixtures.
`NATIVE_FIRES_ALLOWLIST` is intentionally empty. Use a Rust integration test only for a behavior
that cannot be expressed through this public contract; do not add an allowlist entry to bypass a
missing declarative fixture.

Each crate builds its integration tests as one test binary, `it`, rooted at `tests/main.rs`
(`autotests = false`). A new `tests/<name>.rs` file runs only after you add `mod <name>;` to that
crate's `tests/main.rs`; xtask's `test_layout` test fails until you do. Run one file's tests with
`cargo test -p <crate> --test it -- <name>::`. The filter matches substrings, so it also runs any
module whose name ends in `<name>` (`cli::` runs `baseline_cli::` too); pass `--exact` with a full
test name, or anchor it with nextest: `cargo nextest run -p <crate> -E 'test(/^<name>::/)'`.

## Adding a new bundled ruleset

1. Add `crates/alint-dsl/rulesets/v1/<name>.yml`. Its first three lines must be:
   ```text
   # alint://bundled/<name>@v<rev>
   #
   # <prose description>
   ```
   The `bundled-ruleset-has-uri-header` rule in the root `.alint.yml` enforces
   this shape, so `alint check .` catches a malformed header.
2. Add policy tests under `crates/alint-dsl/ruleset-tests/v1/<name>/`:
   - at least one passing document with `expect.violations: []`
   - at least one firing document with concrete expected violations
3. Document the ruleset in `docs/rules.md`. The rule IDs in its markdown table
   and the "N rules" count in the section header must match the YAML exactly;
   `coverage_audit_rules_md_drift` gates this.
4. Run `cargo test -p alint-e2e --test it -- coverage_audit_bundled_rulesets::`.

### scope_filter for ecosystem rulesets

A bundled ruleset that targets one ecosystem (`rust`, `node`, and so on) should
pair its tree-level `when: facts.has_<ecosystem>` gate with a per-rule
`scope_filter: { has_ancestor: <manifest> }` on its per-file content rules, so a
rule fires only on files inside that ecosystem's package subtree. The two gates
compose. `when:` is a cheap tree-level short-circuit (no facts means no rule
iteration), while `scope_filter:` narrows per-file scope when the rule does run.
That distinction matters in polyglot monorepos, where one language's package
sits next to another's.

```yaml
facts:
  - id: has_rust
    any_file_exists: [Cargo.toml, "**/Cargo.toml"]   # catch nested manifests too

rules:
  - id: rust-sources-no-bidi
    when: facts.has_rust                             # tree gate
    kind: no_bidi_controls
    paths: "**/*.rs"                                 # path glob
    scope_filter:                                    # ancestor walk
      has_ancestor: Cargo.toml                       # per-package manifest
    level: error
```

Constraints:

- **Per-file and rule-major rules.** `scope_filter:` is honoured by `PerFileRule`
  rules, which consult it in the file-major dispatch loop, and — as of v0.15 — by
  rule-major kinds (`filename_case`, `filename_regex`, `file_max_size`, and the
  like): a rule-major rule that applies a scope exposes it via its `Scope`, so the
  engine resolves the manifest / `changed_since` sets before every dispatch path.
  Cross-file rules (`pair`, `for_each_dir`, `file_exists`, and the like) reject
  `scope_filter:` at build time with a pointer to the `for_each_dir` plus
  `when_iter:` pattern.
- **Literal filenames, not globs.** Each `has_ancestor:` entry is a filename such
  as `Cargo.toml` or `package.json`, with no `**/` prefix and no path
  separators. The walk reaches "anywhere up the tree" by traversing parent
  directories upward.
- **A file's own directory counts as an ancestor.** A `pyproject.toml` matched by
  `paths: pyproject.toml` and gated by
  `scope_filter: { has_ancestor: pyproject.toml }` always passes its own ancestor
  walk, so do not add the filter when the rule's `paths:` is already a literal
  manifest filename.
- **`has_ancestor` accepts a single string or a list.** Both
  `has_ancestor: pom.xml` and
  `has_ancestor: [pom.xml, build.gradle, build.gradle.kts]` are valid; the upward
  walk stops at the first match.

The directory mirrors the complete ruleset path. For example,
`rulesets/v1/monorepo/cargo-workspace.yml` is tested under
`ruleset-tests/v1/monorepo/cargo-workspace/`. The parity audit rejects untested bundles,
orphaned test directories, and documents extending a bundle other than their owner.

## Adding a rule-kind alias

An alias registers the same builder under a second name (for example, `max_size`
for `file_max_size`). Aliases need no new scenarios. Add the pair to every alias
table in the coverage audits, so each audit treats both spellings as one
canonical kind:

- the inline `aliased` table in `coverage_audit.rs`
- `ALIASES` in `coverage_audit_pass_fail.rs`
- `ALIASES` in `coverage_audit_bench_listing.rs`
- `ALIASES` in `coverage_audit_baseline_safety.rs`

These tables are kept identical. A missing entry in any one of them fails that
audit.

## Bench-scale coverage (soft)

Bench coverage is not a correctness requirement. The `coverage_audit_bench_listing`
test always passes and only prints a summary of the rule kinds absent from every
`xtask/src/bench/scenarios/*.yml`. Run `cargo test -p alint-e2e -- --nocapture`
to see the listing.

If your rule's dispatch shape is genuinely new (for example, a cross-file
aggregation that the existing S6, S7, and S8 scenarios do not exercise), consider
extending one of those scenarios so `xtask bench-compare` gates regressions of
its performance shape. This is opt-in; most rule additions do not need it.

## Common failures

- `coverage_audit_pass_fail` reports "missing FIRING": add a `<kind>_fires.yml`
  scenario whose `expect.violations:` lists a rule of that kind. If the firing
  case cannot be expressed in YAML, add the kind to `NATIVE_FIRES_ALLOWLIST`.
- `coverage_audit_pass_fail` reports "missing SILENT": add a `<kind>_pass.yml`
  with `expect: - violations: []`.
- `coverage_audit_bundled_rulesets` fails: the new ruleset is not referenced from
  any scenario via `extends:`. Add at least a well-formed scenario.
- `coverage_audit` reports "missing kinds": the audit sees no scenario for the
  kind at all. The more specific failures above usually surface first; this is
  the catch-all backstop.
- The `schema.rs` registry guard reports a missing kind: add the kind's entry to
  `all_kinds.yaml`.
- `alint check .` fires `bundled-ruleset-has-uri-header`: the ruleset's first
  three lines do not match the required header shape. See the rule's `message:`
  in `.alint.yml` for the exact pattern.

## Keeping docs in lockstep

The generated contracts (facts, schema, rules, the architecture model) flow to
alint.org and are gated against drift, so a new rule kind's docs cannot silently
fall out of sync:

<likec4-view view-id="docsAsCodeFlow"></likec4-view>

## Related docs

- `docs/design/spec-driven-development.md`: the spec-driven model and the audit
  matrix.
- `docs/design/facts-json.md`: the facts contract and how the counts are derived.
- `crates/alint-e2e/tests/coverage_audit_*.rs`: the audits themselves, each a
  single integration test with a clear panic message.
