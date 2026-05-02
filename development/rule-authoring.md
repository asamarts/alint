---
title: Rule authoring
---

Lands in v0.9.9. Codifies the four-step process every new rule
kind, bundled ruleset, or rule-kind alias goes through so the
coverage audits stay green and CI doesn't surprise anyone.

This doc supplements (does not replace)
`docs/design/v0.9/coverage-and-dogfood.md`, which captures
the *why*. This doc captures the *what to do*.

## Two-layer enforcement

| Layer | Tool | Catches |
|---|---|---|
| **1 — File presence** | `alint check .` (the `.alint.yml` at the repo root, run via the `action-selftest.yml` workflow on every push) | Missing source files, missing scenario YAMLs, missing bundled-ruleset coverage. Fast feedback during local development. |
| **2 — Semantic** | `cargo test -p alint-e2e` (`coverage_audit_*.rs` integration tests) | Pass/fail symmetry, alias-aware kind coverage, bundled-ruleset symmetry, git-mode symmetry, registry consistency. |

Both layers run in CI. A PR that lands a new rule without an
e2e scenario fails layer 1 at lint time; a PR that lands only
a passing scenario (no fires/silent counterpart) fails layer 2
at test time.

## Adding a new rule kind

```text
1. Implement under `crates/alint-rules/src/<kind>.rs`.
2. Register in `alint-rules/src/lib.rs::builtin_registry()`.
3. Add e2e scenarios:
     crates/alint-e2e/scenarios/check/<family>/<kind>_pass.yml
     crates/alint-e2e/scenarios/check/<family>/<kind>_fires.yml
4. If `wants_git_tracked()` / `wants_git_blame()` is opt-in or
   mode-dependent, add:
     <family>/<kind>_in_repo.yml         (given.git.init: true)
     <family>/<kind>_no_op_outside_git.yml  (no given.git block)
5. Run `cargo test -p alint-e2e -- --no-fail-fast`. Every
   `coverage_audit_*` should pass.
```

### Family-directory conventions

The family directory is chosen by what the rule *does*:

- `content/` — content scanning rules (file_content_matches /
  no_trailing_whitespace / final_newline / line_endings / …)
- `existence/` — presence/absence (file_exists / file_absent /
  dir_exists / dir_absent)
- `cross_file/` — fan-out / relational (for_each_dir /
  for_each_file / pair / unique_by / every_matching_has /
  dir_only_contains / dir_contains)
- `encoding/` — Unicode + bytes (no_bom / no_bidi_controls /
  no_zero_width_chars / file_is_text / file_is_ascii)
- `metadata/` — file-property (file_min_size / file_max_size /
  file_min_lines / file_max_lines / file_hash)
- `naming/` — filename-shape (filename_case / filename_regex)
- `structure/` — layout (max_directory_depth /
  max_files_per_directory / no_empty_files)
- `structured/` — JSONPath / JSON-Schema / YAML / TOML
- `security/` — Trojan-source defense, denylists
- `git/` — git-aware rules (git_blame_age / git_commit_message
  / git_no_denied_paths / git_tracked_only behaviour)
- `unix_metadata/` — chmod / symlink shapes
- `when_iter/` / `when_facts/` — `when:` expression coverage
- `interactions/` — multi-rule scenarios

If your rule doesn't fit, add a new family. The audits walk
`scenarios/check/**/*.yml` recursively, so directory layout is
purely organisational.

### Scenario shape

Every scenario YAML uses the same shape:

```yaml
name: <human-readable description>
tags: [check, <kind>, <family>, passing|failing]

given:
  tree:
    <path>: <content>            # files
    <path>:                      # directories
      <child>: …
  config: |
    version: 1
    rules:
      - id: <test-rule-id>
        kind: <kind>
        paths: …
        level: …

when: [check]                    # or [fix]

expect:
  - violations: []               # pass
  - violations:                  # fires
      - {rule: <test-rule-id>, level: <level>, path: <where>}
```

For git-aware rules, add `given.git: { init: true, add: […], commit: true }`.

### Native-test allowlist

Some rule kinds can't have firing YAML scenarios because the
testkit doesn't yet materialise the required filesystem
primitive (chmod, symlinks, backdated commits, custom commit
messages). For these, add a Rust integration test under
`crates/alint-rules/tests/` or `crates/alint-e2e/tests/` that
covers the firing path directly, then add the kind to
`coverage_audit_pass_fail.rs::NATIVE_FIRES_ALLOWLIST` with a
pointer to the native test.

The allowlist is meant to shrink, not grow. As the testkit
acquires `mode: 0o755`, `symlink_to: <path>`, custom commit
messages, and `GIT_AUTHOR_DATE` overrides, allowlist entries
move into native YAML coverage.

## Adding a new bundled ruleset

```text
1. Add `crates/alint-dsl/rulesets/v1/<...>.yml`. The first
   three lines must be:
     # alint://bundled/<name>@v<rev>
     #
     # <prose description>
   (enforced by the `bundled-ruleset-has-uri-header` rule in
    .alint.yml — caught by `alint check .`)
2. Add e2e scenarios:
     crates/alint-e2e/scenarios/check/bundled-<name>/
       <name>_well_formed_passes.yml      (every expect.violations: [])
       <name>_*_flagged.yml               (≥1 non-empty violations entry)
3. Run `cargo test -p alint-e2e --test coverage_audit_bundled_rulesets`.
```

### `scope_filter:` for ecosystem rulesets (v0.9.6+)

Bundled rulesets that target one ecosystem (`rust@v1`,
`node@v1`, etc.) should pair their tree-level `when:
facts.has_<ecosystem>` gate with a per-rule
`scope_filter: { has_ancestor: <manifest> }` on per-file
content rules so the rule fires only on files inside that
ecosystem's package subtree. The two gates compose:
`when:` is a cheap tree-level short-circuit (no facts → no
rule iteration); `scope_filter:` narrows per-file scope when
the rule does run, useful in polyglot monorepos where one
language's package sits next to another's.

```yaml
# In a ruleset YAML:
facts:
  - id: has_rust
    any_file_exists: [Cargo.toml, "**/Cargo.toml"]    # broadened: catch nested manifests

rules:
  - id: rust-sources-no-bidi
    when: facts.has_rust                              # tree gate
    kind: no_bidi_controls
    paths: "**/*.rs"                                  # path glob
    scope_filter:                                     # ancestor walk
      has_ancestor: Cargo.toml                        # canonical per-package manifest
    level: error
```

Constraints:

- **Per-file rules only.** `scope_filter:` is supported on
  `PerFileRule`-trait rules (engine consults it in the file-
  major dispatch loop). Cross-file rules (`pair`,
  `for_each_dir`, `file_exists`, etc.) reject `scope_filter:`
  at build time with a pointer to the `for_each_dir +
  when_iter:` pattern. Rule-major rules like `filename_case`
  silently ignore `scope_filter:` today — gate them via the
  rule's `paths:` glob or skip the filter.
- **Literal filenames, not globs.** Each `has_ancestor:` entry
  is a filename like `Cargo.toml` or `package.json`; no `**/`
  prefix, no path separators. The walk handles "anywhere up
  the tree" by traversing `Path::parent()` upward.
- **File's own dir counts as ancestor.** A `pyproject.toml`
  matched by `paths: pyproject.toml` and gated by
  `scope_filter: { has_ancestor: pyproject.toml }` always
  passes its own ancestor walk — don't add the filter when the
  rule's `paths:` is already a literal manifest filename.
- **`has_ancestor` accepts a single string or a list.**
  `has_ancestor: pom.xml` and `has_ancestor: [pom.xml,
  build.gradle, build.gradle.kts]` are both valid; first-match-
  wins on the upward walk.

Design + semantics:
[`docs/design/v0.9/scope-filter.md`](../design/v0.9/scope-filter.md).

The audit treats nested rulesets like
`monorepo/cargo-workspace.yml` as a single unit — its scenarios
live alongside `monorepo`'s under `bundled-monorepo/`. The
audit doesn't require a separate family directory per nested
ruleset; the URI match (`extends: alint://bundled/monorepo/cargo-workspace@v1`)
is what counts.

## Adding a rule-kind alias

Aliases register the same builder under multiple names (e.g.
`max_size` ↔ `file_max_size`). They don't need new scenarios;
add the alias to `coverage_audit.rs::aliased` AND
`coverage_audit_pass_fail.rs::ALIASES` so the audits treat
both spellings as one canonical kind.

## Bench-scale coverage (soft)

Bench coverage isn't a correctness requirement; the
`coverage_audit_bench_listing.rs` test always passes and
just emits an `eprintln!` summary of rule kinds absent from
any `xtask/src/bench/scenarios/*.yml`. Run with
`cargo test -p alint-e2e -- --nocapture` to see the listing.

If your new rule's dispatch shape is novel (e.g. a new
cross-file aggregation that today's S6 / S7 / S8 don't
exercise), consider extending one of those scenarios so
`xtask bench-compare` gates regressions of its perf shape.
This is opt-in — most rule additions don't need it.

## Failure modes you'll hit

- **`coverage_audit_pass_fail` fails with "missing FIRING"** —
  add a `<kind>_fires.yml` scenario whose `expect.violations:`
  lists a rule with that kind. Or, if the firing case can't be
  expressed in YAML, add to `NATIVE_FIRES_ALLOWLIST`.
- **`coverage_audit_pass_fail` fails with "missing SILENT"** —
  add a `<kind>_pass.yml` (or similar) with
  `expect: - violations: []`.
- **`coverage_audit_bundled_rulesets` fails** — the new
  ruleset hasn't been referenced from any scenario via
  `extends:`. Add at least a well-formed scenario.
- **`coverage_audit.rs` fails with "missing kinds"** — the
  audit doesn't see your new kind in any scenario at all. The
  earlier failures usually surface first; this is the
  catch-all backstop.
- **`alint check .` fires `bundled-ruleset-has-uri-header`** —
  the ruleset's first three lines don't match the docs-export
  parser's expected header shape. See the rule's `message:` in
  `.alint.yml` for the exact pattern.

## Related docs

- `docs/design/v0.9/coverage-and-dogfood.md` — design rationale
- `docs/design/ARCHITECTURE.md` — engine + rule layer overview
- `crates/alint-e2e/tests/coverage_audit_*.rs` — the audits
  themselves; each is a single integration test with a clear
  panic message
