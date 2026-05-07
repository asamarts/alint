# Case study: `denoland/deno`

> Marketing/positioning writeup at https://alint.org/examples/denoland-deno/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `denoland/deno` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-03, sparse-clone of
`https://github.com/denoland/deno.git` HEAD at the time of the inventory.

---

## Summary

Deno has a Rust core + JavaScript/TypeScript tooling layout: ~75 Cargo
workspace members under `cli/`, `ext/`, `libs/`, `runtime/`, `tests/`, plus a
~3,000 LoC orchestrator at `tools/lint.js` and a sibling `tools/format.js`
that drive every CI gate. The orchestrator runs **9 logical structural checks**
end-to-end (alongside clippy + dprint + dlint, which are lint engines, not
structural checks).

Adding the per-language hygiene the bundled rulesets cover (Cargo workspace
shape, GitHub Actions hardening, OSS baseline, agent-context bloat) the total
**structural-validation surface is ~22 checks** in the repo today.

Of those:

- **~45 % map directly to existing alint rules** (10/22) — copyright headers
  per language, top-level-files allowlist, clippy.toml-per-crate pairing,
  workflow generator pairing, dprint/dlint config shape, file-size guard,
  `.gitattributes` LF enforcement.
- **~30 % need new alint primitives** (6/22) — the AST-aware checks
  (`ensureNoNonPermissionCapitalLetterShortFlags` parses `cli/args/flags.rs`
  for `.short('X')` calls, `tools/jsdoc_checker.js` walks a TypeScript AST),
  the per-file violation-count baseline (`lintNodePolyfillDenoApis`), the
  semantic JSONC traversal (`ensureNoUnusedOutFiles`), and a
  `dir_only_contains` extension that also validates subdirectory children.
- **~25 % are out of alint's scope** (6/22) — the lint engines themselves
  (`clippy`, `dlint`, `dprint`), CI optimisation (`check_deno_core_changes.js`,
  `mtime_cache`), and CI-only checks that don't run against files
  (`verify_pr_title.js`).

The 50 % that *do* fit translate to a 17-rule alint config (below).
This replaces `tools/lint.js`'s nine structural sub-checks plus the bundled
OSS / Rust / Node / GHA / Cargo-workspace baselines with one declarative
config + one `alint check` invocation in CI.

---

## Existing tooling inventory

The Deno repo's structural validation is concentrated in three places:

1. **`tools/lint.js`** (~700 LoC) — the orchestrator. Eight logical checks
   running in parallel via `Promise.allSettled`.
2. **`tools/copyright_checker.js`** (~130 LoC) — invoked by `lint.js` but
   factored out because it's also useful standalone.
3. **`tools/format.js`** + **`tools/jsdoc_checker.js`** + the
   `ensureWorkflowYmlsUpToDate` sub-step inside `lint.js` — three more
   structural gates, each invoked from CI as a separate step.

Plus the standard repo hygiene that's not in those scripts but is enforced
by convention / file presence (Cargo workspace shape, `.editorconfig`,
GitHub Actions workflows).

### Maps to existing alint rules (drop-in replacements)

| Source | What it checks | alint replacement |
|---|---|---|
| `tools/copyright_checker.js` | MIT copyright header on every `.js`/`.mjs`/`.jsx`/`.ts`/`.tsx`/`.rs`/`.c`/`Cargo.toml` source file (per-language comment style, with extensive exclude list) | `file_header` × 4 (one per comment style: `//`, `#`, `/*`, plus one `file_content_matches` for the LICENSE.md year) |
| `tools/lint.js` → `ensureNoNewTopLevelEntries` | Repo root contents must match an explicit allow-list (curated; new entries require discussion) | `dir_only_contains` with `select: "."` — **partial coverage**: catches new top-level *files* only, not new top-level *directories*. See gap analysis below. |
| `tools/lint.js` → `ensureDisallowedMethodsEnforced` (per-crate clippy.toml exists) | Every `ext/*` and `libs/*` crate has a `clippy.toml` | `for_each_dir` × 2 (`ext/*` and `libs/*`) with `when_iter: 'iter.has_file("Cargo.toml")'` and a nested `file_exists` |
| `tools/lint.js` → `ensureDisallowedMethodsEnforced` (per-crate clippy.toml content) | Each crate's `clippy.toml` lists ~30 `std::fs::*` / `std::path::Path::*` / `url::Url::*` methods (libs/* gets ~8 extra) | `file_content_matches` × N (one rule per banned method per scope). **Sample-ed in the config** — full coverage would be ~38 rules, candidate for a v0.10+ `disallowed_methods_in_file` primitive |
| `tools/lint.js` → `ensureWorkflowYmlsUpToDate` | Every `.github/workflows/*.ts` generator has a paired `.generated.yml` checked in | `pair` (primary `.ts`, partner `{stem}.generated.yml`) |
| `.dlint.json` shape | dlint config keeps `recommended` tag + the project-required overrides | `json_path_equals` (assert `tags[0]: recommended`, `include[*]: camelcase`) |
| `.dprint.json` shape | dprint config keeps `typescript.deno: true` | `json_path_equals` |
| `.gitattributes` LF enforcement | `* text=auto eol=lf` is present | `file_content_matches` |
| File size guard (informal — implicit in Deno's commit-review process) | No accidental large blobs in non-testdata trees | `file_max_size` with `paths.exclude` for `tests/testdata/**`, `tests/specs/**`, `Releases.md`, `Cargo.lock`, `cli/tsc/dts/**` |
| `tools/lint.js` → `clippy` (workspace + deno_core split) | Cargo clippy with project's `--deny` set (`unused_async`, `print_stderr`, `print_stdout`, `large_futures`, `allow_attributes_without_reason`) | `command` rule shelling out to `cargo clippy` (single-shot, triggered off the root `Cargo.toml`) |
| `tools/format.js --check` | Whole-repo `dprint --check` pass | `command` rule shelling out to `dprint check` |
| `tools/lint.js` → `dlint` | Whole-JS/TS-tree `dlint` pass | `command` rule shelling out to `tools/lint.js --js` (defers to the existing exclude-list logic in the script) |

Plus the bundled rulesets the config extends:

- `oss-baseline@v1` — README/LICENSE existence, merge-conflict markers, bidi
  controls, etc.
- `rust@v1` — `Cargo.toml` at root, `Cargo.lock` committed,
  `rust-toolchain.toml` pinned, no tracked `target/`, snake_case sources
- `node@v1` — `.nvmrc` recommendation (Deno doesn't use one — fires
  info-level), no tracked `node_modules`
- `ci/github-actions@v1` — workflows have `name:`, `permissions.contents: read`,
  third-party actions pinned to commit SHAs (Deno's workflows mostly comply —
  the SHA-pin regex passes for the actions-checkout / setup-deno / setup-node
  references in the captured snapshot)
- `monorepo/cargo-workspace@v1` — workspace `members = [...]` declared, every
  `crates/*` member has a `README.md` and `[package].name`. Note: Deno's
  workspace members live under `ext/`, `libs/`, `runtime/`, `cli/`, not
  `crates/` — so this rule no-ops on Deno today. **Actionable v0.10+
  feedback:** the bundled cargo-workspace ruleset hard-codes `select: "crates/*"`;
  a `vars`-based override or a smarter selector that derives member dirs from
  the `[workspace] members` array would let Deno benefit from the same
  per-member READMEs check (we already noticed `libs/dotenv/` ships without
  a README in the captured snapshot).
- `tooling/editorconfig@v1` — `.editorconfig` is well-formed
- `agent-context@v1` — Deno ships a `CLAUDE.md`; the bundled rules cover the
  bloat / stub / stale-paths heuristics for free

**13 direct-replacement rules** (10 explicit + 3 from bundled overlays the
config extends). 5-15 minute config-build per rule.

### Needs new alint primitive

| Source | What it checks | What alint needs |
|---|---|---|
| `tools/lint.js` → `ensureNoNonPermissionCapitalLetterShortFlags` | Parse `cli/args/flags.rs`, find every `.short('X')` call, assert the set of capital letters is exactly the curated allow-list (uppercase short flags ⇔ permission flags) | A `rust_ast_query` or `language_token_match` rule kind. AST-aware, narrowly scoped — pattern shows up in any project enforcing a "this kind of construct may only appear in this set of forms" invariant. Out of alint's no-AST non-goals as currently scoped; could be done as a `command` rule shelling out to a custom Rust AST walker |
| `tools/lint.js` → `lintNodePolyfillDenoApis` | Run `deno lint` with a custom plugin against `ext/node/polyfills/**/*.ts`, count the violations, compare per-file count against an `EXPECTED_VIOLATIONS` baseline (drift up = error, drift down = error with message "update the baseline") | A `violation_count_baseline` rule kind — wraps a child command, parses violation counts from output, diffs against a per-file baseline file. **Strongest candidate** for a new v0.10+ primitive: same shape (baselined drift) shows up in many large-codebase migration efforts (e.g. TypeScript strict-mode adoption, Python type-coverage). Could be the same primitive as the `import_gate` registry-file pattern from the Kubernetes case study |
| `tools/jsdoc_checker.js` | Walk `cli/tsc/dts/lib.deno*.d.ts` with `ts-morph`, assert every exported symbol has a `@category` JSDoc tag (and `@experimental` for unstable libs), correct `declare`/`export` keyword usage | Same `language_ast_query` shape as the Rust short-flag check, but for TypeScript. **Out of alint's "no-AST" non-goals as currently scoped.** Realistic path: keep `tools/jsdoc_checker.js` as a `command`-invoked external tool. The pattern (every exported decl carries a category tag) is reusable across any project that ships a typed public API surface |
| `tools/lint.js` → `dlintPreferPrimordials` | Run dlint with the single `prefer-primordials` rule against `runtime/**/*.{js,ts}` + `ext/**/*.{js,ts}` (a different scope than the main dlint pass) | Could be a `command` rule with a narrower `paths:` and a custom `argv`. The gap is more ergonomic than functional — currently the user would write a second `command` rule that duplicates the dprint config exclusions |
| `tools/lint.js` → `ensureNoUnusedOutFiles` | Walk `tests/specs/**/__test__.jsonc`, parse JSONC, traverse nested `output:` keys with `${var}` substitution against `variants:`, build a set of referenced `.out` paths, diff against the actual `.out` files on disk | A `referenced_files_match_filesystem` cross-file rule kind that takes a "manifest file glob + JSONPath to the path-strings + filesystem glob to compare against". Highly reusable: same shape covers "every fixture in tests/data is referenced from at least one test", "every translation key in i18n/en.json appears in source code", "every entry in CODEOWNERS points to a real path". **Second-strongest candidate** for a new primitive — broader applicability than the AST checks |
| `tools/lint.js` → `ensureNoNewTopLevelEntries` (dir portion) | New top-level *directories* (`pkg/`, `vendor/`, etc.) must be in the allow-list. The file portion is captured today; the dir portion is a gap. | `dir_only_contains` currently checks file children only (`crates/alint-rules/src/dir_only_contains.rs:87` — dir children are skipped). A `dir_contents_match_allowlist` (or a `check_subdirs: true` flag on `dir_only_contains`) would close this. Same primitive serves the broader pattern of "this directory's complete contents are listed in N." |

**Gap pattern: AST-aware queries.** Two of the five gaps (`flags.rs` short
flags, `lib.deno*.d.ts` JSDoc) are language-aware AST walks. Alint's
non-goals deliberately avoid the AST tier — the right answer is to keep
`tools/jsdoc_checker.js` and the Rust-flag check as `command`-invoked
external tools, and document the boundary clearly.

**Gap pattern: baselined drift.** The `lintNodePolyfillDenoApis` shape is
specifically interesting. It's not "lint these files"; it's "run a counter,
compare against a snapshot, surface drift in either direction with a
human-readable migration message". Showed up in the Kubernetes case study
too (the `restricted_packages` registry pattern is structurally identical:
read a registry file, walk the codebase, diff). One v0.10+ primitive could
serve both.

### Out of alint's scope (use the existing tool)

These are lint-engine checks or CI-only logic. Alint's non-goals are
deliberate; the existing tooling is the right answer.

- **`tools/lint.js` → `clippy`** — runs cargo clippy against the workspace.
  Clippy *is* the lint engine; alint's `command` rule wraps the invocation
  but doesn't replace clippy itself. (Captured in the config above.)
- **`tools/lint.js` → `dlint` / `dlintPreferPrimordials`** — dlint is the lint
  engine for JS/TS in this repo. Same boundary.
- **`tools/format.js`** — dprint is the formatter. Wrapped by a `command`
  rule but not "replaced".
- **`tools/check_deno_core_changes.js`** — CI optimisation: skip the
  deno_core test job when no `libs/core*` files changed in the diff.
  Out of scope (alint validates state, not CI graph)
- **`.github/mtime_cache/`** — CI caching plugin (mtime-restore for cargo
  builds). Not validation.
- **`tools/verify_pr_title.js`** — runs against the PR title (not against
  files). Conventional-commits-ish enforcement that lives in the
  GitHub Actions workflow context, not the repo state. Out of scope.
- **`tools/release/*`** — release-orchestration scripts (cut a tag,
  publish to crates.io, etc.). Codegen / process automation, not
  validation.
- **`tools/napi/generate_symbols_lists.js`** — codegen for NAPI symbols.
  Out of scope (alint doesn't run codegen).

### Already covered by other tooling Deno uses

- **`Cargo.lock` resolution** — handled by cargo itself (`--locked` in the
  clippy invocation already enforces "Cargo.lock is up-to-date with
  Cargo.toml")
- **TypeScript type-check on .d.ts files** — handled by `tsc` and the deno
  CLI itself; not in scope for alint

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Replaces 11 of `tools/lint.js`'s
+ `tools/copyright_checker.js`'s structural checks, plus 4 more via
`command` shelling out (clippy / dprint / dlint / format-check). Net **15 of
the ~22 structural checks** move to one declarative file.

The remaining 7:

- 5 need new alint primitives (above) — file as v0.10+ feature requests
- 2 are CI-only or codegen (out of alint's scope)

---

## Cross-language pairing observations

Deno's "every `cli/tools/<x>` Rust module pairs with a `tests/specs/<x>` test
directory" was an a-priori candidate for a `pair`/`for_each_dir` use case
spanning languages — this is something the kubernetes case study didn't
have a parallel for. After inventory, the actual mapping is messier than
that: `cli/tools/` contains Rust modules grouped by CLI subcommand
(`bench`, `bundle`, `check`, `clean`, `compile`, etc.), and `tests/specs/`
contains test directories grouped by *behaviour*, not 1:1 by source module.
A literal `pair` rule would fire too noisily.

The cleaner cross-language pairing in this repo is the
**workflow-generator-pairs** rule (every `.github/workflows/*.ts` generator
has a paired `.generated.yml`) — the config captures it explicitly.

The other practical cross-language structural rule is the
**clippy.toml-per-crate** pattern: every `ext/*` and `libs/*` crate ships
a `clippy.toml` with a known list of banned methods. The structural piece
(file exists per crate, contains certain strings) is in alint today; the
content-completeness check ("contains *every entry* of a 30-method list")
collapses to one rule per entry, which is verbose. **`disallowed_methods_in_file`
as a v0.10+ primitive would replace that ~38-rule expansion with a single
rule that sources the list from a registry file.**

---

## Performance comparison (placeholder — bench when validation pass scales)

`tools/lint.js` runs its eight structural sub-checks via
`Promise.allSettled` (parallel). Each sub-check does its own work — file
walks, regex matches, AST parses, child-process spawns. Wall time on the
captured snapshot is dominated by `clippy` + `dlint` + `dprint`, which the
config above also delegates to via `command` rules — so the structural-
check delta is small (those gates run once either way).

Where alint should win is **the structural rules themselves**: the 11
explicit replacements in the config plus the bundled-overlay rules all
run in alint's parallel rule dispatcher (v0.9.3+ flip). On a Deno-scale
repo (~25k files in the captured snapshot, with `tests/testdata/**`
contributing the bulk), the v0.9.6 published S3-100k bench (1.13 s for
the workspace bundle) is the right reference point — expect **~1-2 s for
the alint-replaceable subset**.

To benchmark for real: run `time deno run -A tools/lint.js` against
`time alint check --rules id_glob='deno-*'` on the same checkout. Deferred
to the per-repo measurement pass.

---

## Followup feature work surfaced (combining with Kubernetes findings)

Two cross-cutting themes from this inventory shape the v0.10+ rule-kind
backlog:

1. **The "language-AST query" boundary** — Deno hits it twice (Rust
   short-flag enforcement, TypeScript JSDoc tags). alint's deliberate
   non-goal: AST analysis. The realistic path is to keep
   `tools/jsdoc_checker.js` and the Rust-flag check as
   `command`-invoked external tools.
2. **The baselined-drift primitive (`lintNodePolyfillDenoApis`)** —
   wraps a child command, parses violation counts from output, diffs
   against a per-file baseline. Same shape recurs in the Kubernetes
   restricted-imports pattern and many large-codebase migration efforts
   (TypeScript strict-mode adoption, Python type-coverage). Worth a
   dedicated v0.10+ design pass: a `violation_baseline` rule kind.

Concrete proposals:

- **`disallowed_methods_in_file` rule kind** (per-file content list sourced
  from a registry) — would cover Deno's clippy.toml-per-crate content
  check (~38 rules → 1) and the Kubernetes restricted-imports pattern
  (~6 verify scripts → 1). Same primitive.
- **`violation_baseline` rule kind** (wrap a child command, diff per-file
  violation counts against a snapshot) — covers `lintNodePolyfillDenoApis`
  and the broader pattern of "we have N known violations, the count must
  not grow".
- **`referenced_files_match_filesystem` rule kind** (manifest glob +
  JSONPath to path strings ↔ filesystem glob) — covers
  `ensureNoUnusedOutFiles` and many sibling patterns (CODEOWNERS resolves,
  every fixture is referenced, every i18n key has a match in source).
- **Smarter `monorepo/cargo-workspace` selector** — currently hardcoded to
  `crates/*`; doesn't fit Deno's `ext/*` + `libs/*` + `runtime` + `cli/*`
  layout. A selector that reads `[workspace] members` from the root
  Cargo.toml would let the bundled ruleset's per-member checks
  (README, [package].name) work for any cargo workspace shape.
- **`dir_only_contains` should optionally check subdirectories** — today
  the rule silently skips dir children, so the Deno top-level allowlist
  catches new files but not new subdirs. A `check_subdirs: true` flag
  (or a sibling `dir_contents_match_allowlist`) would close the gap
  with a one-line schema addition.

---

## Future analysis

Concrete analyses to follow up on the live tree (when one becomes
available):

- **`alint suggest` against a fresh `denoland/deno` clone** — predict the
  heuristic will surface `oss-baseline@v1`, `rust@v1`, and `node@v1`
  given the polyglot Rust+JS/TS shape; cross-reference against the
  manually configured 8-extends list.
- **`*_path_contains` v0.10 design progress** — the
  `deno-dlint-includes-camelcase` rule currently uses a
  `file_content_matches` workaround per pitfall #17. Track when the
  v0.10 `*_path_contains` ships; the rule rewrite is mechanical.
- **`nested_configs: true` for the `ext/*` and `libs/*` subtrees** —
  Deno's per-crate `clippy.toml` content checks could move into per-crate
  `.alint.yml` files (one per `ext/<crate>/` and one per `libs/<crate>/`)
  to scope the `disallowed_methods_in_file` candidate's per-crate
  registries cleanly. Same shape as the upcoming `dir_contents_match_allowlist`
  primitive.

## Validation status (2026-05-07)

- alint version: v0.9.17
- Config validation: `validate-config` reports **76 rules loaded**.
  Reconciliation: 20 explicit rules in `.alint.yml` (15 + 5 the README
  understated) + 61 entries from extends (oss-baseline 15 + rust 11 +
  node 9 + ci/github-actions 3 + monorepo/cargo-workspace 4 +
  tooling/editorconfig 3 + hygiene/no-tracked-artifacts 11 +
  agent-context 5) − 5 facts (`has_rust`, `has_node`,
  `has_agent_context`, `is_cargo_workspace` from the bundled
  rulesets, plus `has_dlint_config` declared inline in this config) = 76.
- Live-tree status: pending — `/tmp/deno/` not present at revalidation
  time.
- Pitfall fixes shipped in v0.9.17: pitfall #18 (per-rule
  `respect_gitignore: false`), pitfall #19 (literal_is_nested runtime
  guard) — neither directly affects this config.
- Pitfall #17 (already-documented since P2a Wave 3) is the
  load-bearing one for the `deno-dlint-includes-camelcase` rule; the
  `file_content_matches` regex workaround stays until the v0.10 design
  candidate `*_path_contains` ships (3 sources: helm, deno, bazel).
- Open gaps: `disallowed_methods_in_file` (per-file content list sourced
  from a registry; deno + Kubernetes), `violation_baseline` (deno's
  `lintNodePolyfillDenoApis`), `referenced_files_match_filesystem`
  (deno's `ensureNoUnusedOutFiles`), and the
  `monorepo/cargo-workspace` member-discovery refinement (deno's
  `ext/*` + `libs/*` + `runtime` + `cli/*` layout doesn't fit the
  hardcoded `crates/*` selector).
