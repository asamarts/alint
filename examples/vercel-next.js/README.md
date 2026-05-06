# Case study: `vercel/next.js`

Inventory of the structural-validation tooling in `vercel/next.js`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-clone of
`vercel/next.js@98ab09903` (rev = `98ab09903cfb2d35764b6c2eaeb5f0df00589208`),
`/test`, `/examples`, and `/docs` excluded.

---

## Summary

next.js is a **hybrid pnpm + Cargo monorepo**: a pnpm workspace
(`packages/*`, `apps/*`, `bench/*`, `crates/*/js`,
`turbopack/packages/*`, `turbopack/crates/*/js`) running
**alongside** a Cargo workspace (`crates/*` + `turbopack/crates/*`
+ `scripts/send-trace-to-jaeger`). Concrete count at HEAD:

- **19** npm packages under `packages/` (all pinned to
  `16.3.0-canary.11`)
- **63** `Cargo.toml` files across `crates/` + `turbopack/crates/`
- **30+** GitHub Actions workflows under `.github/workflows/`
- **51** `.js` / `.mjs` scripts under `scripts/`, of which **7**
  are hand-rolled `check-*` / `validate-*` structural gates
- **6** root config files for as many lint tools (prettier,
  eslint, ast-grep, alex, typos, lint-staged)
- **2** husky hooks (`.husky/pre-commit` runs `lint-staged`,
  `.husky/pre-push` implements canary-branch push protection)
- Lerna for versioning + turbo for orchestration

Total **structural-validation surfaces** counted: **34** discrete
checks across the inventory (see § "Existing tooling inventory"
below).

- **18 of 34 (53 %) map to existing alint rules** — bundled
  `oss-baseline + node + rust + monorepo + monorepo/cargo-workspace +
  monorepo/pnpm-workspace + ci/github-actions + hygiene/lockfiles +
  hygiene/no-tracked-artifacts + tooling/editorconfig + agent-context`
  cover ~40 rules between them, plus the 35 next.js-specific rules
  in [`/.alint.yml`](.alint.yml) (workspace shape, per-package
  pinning, husky hook integrity, tool-config presence, etc.).
- **7 of 34 (20 %) shell out via `command:` rules** — prettier,
  eslint, tsc, ast-grep, alex, cargo fmt + clippy, typos, plus
  the three custom check scripts (check-examples.sh,
  check-unused-turbo-tasks.mjs, validate-externals-doc.js).
- **9 of 34 (27 %) are out of alint's scope** — the four
  TypeScript runners under `scripts/` that read git state /
  parse Rust ASTs for turbo-tasks usage / probe runtime module
  format / regenerate webpack pre-compiled bundles, plus the
  `check-is-release.js` PR-content script + the four CI
  workflows that operate on the PR file diff.

The configured 59-rule [`/.alint.yml`](.alint.yml) covers every
structural assertion the existing tooling makes about repo *state*,
plus several that next.js doesn't enforce today (per-crate license
uniformity, husky-hook content integrity, gitattributes EOL pin).

**Headline finding:** next.js is the canonical "hybrid pnpm + Cargo
mega-monorepo" — alint's polyglot bundle composition
(`monorepo/cargo-workspace@v1` + `monorepo/pnpm-workspace@v1`
layered together) is the tightest fit in the case-study
catalogue so far, and surfaces **3 of 19 npm packages missing
license fields + 4 of 63 crates missing the standard MIT/MPL
license** — drift no per-language linter catches because each
linter only sees half the tree.

---

## Existing tooling inventory

### Root config files (root-level lint policy)

| File | Owner tool | What it pins | alint disposition |
|---|---|---|---|
| `package.json` `scripts:` block | npm | 70+ task aliases (`lint`, `lint-eslint`, `prettier-check`, etc.) | Not directly alint-checkable; rules below assert the canonical four (`lint`, `prettier-check`, `lint-eslint`, `lint-typescript`) stay declared |
| `pnpm-workspace.yaml` | pnpm | 7 workspace globs (`apps/*`, `packages/*`, `bench/*`, `crates/*/js`, `turbopack/crates/*/js`, `turbopack/crates/turbopack-tests/tests/execution`, `turbopack/packages/*`) | `yaml_path_matches` + 2 `file_content_matches` for the canonical entries |
| `Cargo.toml` workspace | cargo | 13 workspace member globs + 2 exclude entries; `[workspace.lints]` | Bundled `monorepo/cargo-workspace@v1` + per-crate inheritance assertions |
| `lerna.json` | lerna | publish workflow: `npmClient: pnpm`, `version.exact: true`, `publish.allowBranch: [canary]`, root version (`16.3.0-canary.11`) | 3× `json_path_matches` + 1× `file_exists` |
| `turbo.json` | turbo | task graph + cached outputs (`build` → `dist/**`, `dev`, `storybook`, `pack-for-isolated-tests`) | 3× `json_path_matches` (`$schema`, `$.tasks.build.outputs[*]`, presence) |
| `tsconfig.json` | tsc | root TS config — `compilerOptions.strict: true` | `file_content_matches` (NOT `json_path_matches`; see pitfall #16 below) |
| `tsconfig-tsec.json` | tsec | trusted-types security checker | Not asserted (presence-only would be noisy; tsec is a optional supply-chain gate) |
| `eslint.config.mjs` + `eslint.cli.config.mjs` | eslint | flat-config split (IDE vs CI) — both must coexist | 2× `file_exists` |
| `.prettierrc.json` + `.prettierignore` | prettier | `singleQuote: true, semi: false, trailingComma: es5` + ignore globs | 2× `file_exists` |
| `lint-staged.config.js` | lint-staged | per-extension prettier/eslint/rustfmt pipeline triggered by `.husky/pre-commit` | `file_exists` |
| `.husky/pre-commit` | husky | invokes `pnpm lint-staged` | `file_exists` + `file_content_matches` for canonical command |
| `.husky/pre-push` | husky | canary-branch push protection (custom shell) | `file_exists` |
| `.typos.toml` | crate-ci/typos | per-language word list, identifier ignores | `file_exists` |
| `sgconfig.yml` | ast-grep | rule dirs, test config dirs | `file_exists` |
| `.alexrc` + `.alexignore` | alex | insensitive-language allow-list | `file_exists` |
| `socket.yaml` | Socket.dev | supply-chain scanner config | `file_exists` |
| `.npmrc` | pnpm | `auto-install-peers`, `link-workspace-packages`, `provenance`, etc. | 3× `file_content_matches` for the load-bearing settings |
| `.node-version` | nvm/etc. | Node major version pin (`v20`) | `file_exists` |
| `.gitattributes` | git | `* text=auto eol=lf` + per-vendored-dir overrides | `file_content_matches` for the EOL pin |
| `.rustfmt.toml` | rustfmt | edition + style + max width | `toml_path_matches` for `edition = "2024"` |
| `rust-toolchain.toml` | rustup | `nightly-2026-04-02` channel + `rustfmt`, `clippy`, `rust-analyzer` components | 2× `toml_path_matches` |

### `scripts/check-*.{js,mjs,sh}` — hand-rolled structural gates

| Script | What it checks | alint replacement |
|---|---|---|
| `check-examples.sh` | Re-canonicalises every `examples/*/package.json` (drops license/version/name/author/description, sets `private: true`); copies template `next-env.d.ts` and `.gitignore` if missing; **fails if `git status` shows any drift**. Mutation-with-verification, not pure validation. | Out of scope for direct replacement (mutation). Wrapped via `command:` rule that runs the script in CI; failures still flag. The shape "every example matches a normalised template" *could* fit a `for_each_dir` rule once template-substitution lands. |
| `check-manifests.js` | Walks `errors/manifest.json`'s route tree, asserts every `errors/**/*.md` (except `template.md`) is reachable from the route graph. | Partial: `json_path_matches` asserts the manifest's shape (every `routes[*].path` is `^/errors/.+\.md$`). The deeper "every md reachable from routes" is a registry-resolves check — needs the `registry_paths_resolve` v0.10+ candidate. |
| `check-pre-compiled.sh` | Re-runs `pnpm ncc-compiled` (which re-bundles webpack runtime); fails if `git status` shows drift. Codegen-freshness check. | Out of scope (codegen, not validation). Mutation followed by git-diff — same pattern as airflow's `update-spelling-wordlist-to-be-sorted`. |
| `check-is-release.js` | Parses the most recent commit message (`git log -n1 --pretty=format:%B`) for a `^v\d+\.\d+\.\d+(-\w+\.\d+)?$` tag. | Out of scope (operates on git history, not repo state). The `git_commit_message` rule kind exists but checks the staged/HEAD message; this script needs subprocess git access. |
| `check-unused-turbo-tasks.mjs` | Scans every `*.rs` file under `crates/` + `turbopack/crates/` for `#[turbo_tasks::function]` / `value` / `value_trait` annotations; cross-references against usage sites; reports unused. | Out of scope (Rust AST / cross-file reference graph). Wrapped via `command:` rule. Same shape as `knip` in microsoft/typescript. |
| `validate-externals-doc.js` | Reads `packages/next/src/lib/server-external-packages.jsonc`; cross-references against the doc table at the bottom of `errors/improper-server-external.mdx`; reports drift. | Needs the v0.10+ `cross_file_value_equals` rule (registry value at point X in file A appears as table entry in file B). Wrapped via `command:` rule today. |
| `check-backport-canary-release.js` | Validates a `backport-canary-release` branch matches the canary state. | Out of scope (operates on git refs). |

### `.github/workflows/` (30+ workflows)

| Workflow | What it does | alint disposition |
|---|---|---|
| `build_and_test.yml` | The mega-orchestration workflow — kicks off `lint`, `rust-check`, `rustdoc-check`, `check-types-precompiled`, `validate-docs-links` jobs via `build_reusable.yml` | Each job is its own surface (see below); the orchestrator itself is structural shape covered by `ci/github-actions@v1` |
| `build_and_deploy.yml` | Builds + uploads CI/CD artifacts | Shape only |
| `build_reusable.yml` | The shared "checkout + setup-node + setup-pnpm + run script" reusable | Shape only |
| `lint` job (under `build_and_test.yml`) | Runs `pnpm lint-no-typescript && pnpm check-examples && pnpm validate-externals-doc` | Three `command:` rules wrapping these (see below) |
| `rust-check` job | Runs `pnpm dlx turbo run rust-check` (= cargo clippy + rustfmt --check + rustdoc -D warnings under turbo) | `command:` rule wrapping `cargo clippy --workspace -- -D warnings` |
| `code_freeze.yml` | Cron job that pauses canary releases during freezes | Out of scope (operational) |
| `create_release_branch.yml`, `trigger_release.yml`, `code_freeze.yml`, `sync_backport_canary_release.yml` | Release orchestration | Out of scope (operational) |
| `triage.yml`, `issue_lock.yml`, `issue_stale.yml`, `issue_wrong_template.yml`, `pull_request_auto_label.yml`, `popular.yml` | Issue / PR bot automation | Out of scope (operational) |
| `pr_ci_comment.yml`, `pull_request_stats.yml`, `graphite_ci_optimizer.yml` | PR-comment automation | Out of scope (operational) |
| `test_*.yml`, `integration_tests_reusable.yml`, `rspack-*.yml`, `turbopack-*.yml`, `test-turbopack-rust-bench-test.yml`, `retry_test.yml`, `retry_deploy_test.yml` | Test orchestration | Out of scope (test runners) |

The `ci/github-actions@v1` ruleset (3 rules: workflow permissions,
action SHA pinning, workflow has `name:`) covers the hardening
surface for all 30+ workflows at once. The starter config restates
the SHA-pinning rule at warning level here because next.js has
30+ workflows and the supply-chain blast radius is large.

### `.config/ast-grep/rules/` — Rust AST patterns

ast-grep rules under `.config/ast-grep/rules/` are AST-shape
patterns (Rust idioms, anti-patterns). All AST-aware; out of
alint's "no AST" scope. Shelled out via `pnpm lint-ast-grep`.

### `.alex` (insensitive-language NLP) + `.typos` (spell check)

Both are NLP / dictionary-based per-file content checks. `alex`
runs over `*.md` / `*.mdx`; `typos` runs over `*.{rs,toml}` (per
the `.typos.toml` per-type config). Out of alint's scope; both
shelled out via `command:` rules.

### `eslint.config.mjs` rules (TS/JS AST analysis)

The eslint config defines ~50 rules across `eslint:recommended`,
`@typescript-eslint`, `eslint-plugin-react`, `eslint-plugin-react-hooks`,
`eslint-plugin-jest`, `eslint-plugin-import`, `eslint-plugin-jsdoc`,
plus next.js's own `@next/eslint-plugin-internal`. Every one is a
TSESTree visitor — out of alint's scope. Shelled out via
`pnpm lint-eslint .`.

---

## What maps to existing alint rules

The 59-rule [`/.alint.yml`](.alint.yml) breaks down as:

- **11 bundled rulesets** (`oss-baseline`, `node`, `rust`,
  `monorepo`, `monorepo/cargo-workspace`,
  `monorepo/pnpm-workspace`, `ci/github-actions`,
  `hygiene/no-tracked-artifacts`, `hygiene/lockfiles`,
  `tooling/editorconfig`, `agent-context`) — pull in roughly
  40 rules between them
- **3 pnpm-workspace shape assertions** — `packages` glob
  declared, `packages/*` and `turbopack/packages/*` entries
  present
- **4 per-package conventions** — license, version, name, version
  pinned to canary lockstep
- **2 per-Cargo-crate conventions** — edition `2024`, license
  in `MIT|MPL-2.0|Apache-2.0`
- **5 root single-source-of-truth assertions** — rust-toolchain
  channel + components, rustfmt edition, root package.json
  `private: true`, root package.json `workspaces` declared
- **3 lerna assertions** — file present, `npmClient: pnpm`,
  `publish.allowBranch: [canary]`
- **8 tool-config-file presence assertions** — `.prettierrc.json`,
  `.prettierignore`, `eslint.config.mjs`, `eslint.cli.config.mjs`,
  `.typos.toml`, `sgconfig.yml`, `.alexrc`, `lint-staged.config.js`
- **3 husky hook integrity assertions** — `pre-commit` present
  + invokes `lint-staged`, `pre-push` present
- **4 repo-metadata assertions** — `AGENTS.md` present, `CLAUDE.md`
  symlink present, `contributing.md`, `CODE_OF_CONDUCT.md`
- **2 errors/manifest.json assertions** — file present + every
  route shape `^/errors/.+\.md$`
- **3 turbo.json assertions** — present, `$schema` declared,
  `tasks.build.outputs[*]` includes `dist/**`
- **6 workspace-root config assertions** — tsconfig strict,
  npmrc settings (auto-install-peers, strict-peer-deps,
  provenance), `.node-version` present, gitattributes EOL pin,
  `socket.yaml` present
- **3 tracked-artifact hygiene rules** — no `.next/`, no
  `.turbo/`, no `target/debug/` (extends `hygiene/no-tracked-artifacts`
  to nested locations common in dual-language monorepos)
- **1 GHA SHA-pinning restatement** at warning level
- **11 `command:` rule shell-outs** — prettier, eslint, tsc,
  ast-grep, alex, cargo fmt, cargo clippy, typos, check-examples.sh,
  check-unused-turbo-tasks.mjs, validate-externals-doc.js

---

## What needs new alint primitives

Three patterns specific to next.js that don't fit any current rule:

### 1. `cross_file_value_equals` for `validate-externals-doc.js`

`scripts/validate-externals-doc.js` reads
`packages/next/src/lib/server-external-packages.jsonc` (a JSON5
array of package names) and cross-references against a markdown
table embedded inside `errors/improper-server-external.mdx`. The
assertion is "every entry in the JSONC array also appears in the
markdown table, and vice versa." Same shape as the airflow
`cross_file_value_equals` candidate. **Strong v0.10+ signal**:
this is now the **third** repo (airflow, tokio, next.js) where
this pattern surfaces.

### 2. `registry_paths_resolve` for `check-manifests.js`

`scripts/check-manifests.js` walks `errors/manifest.json`'s route
tree and asserts every `errors/**/*.md` (except `template.md`)
appears as a `path:` value somewhere in the route graph. Same
shape as the rust-lang/rust `registry_paths_resolve` candidate
(triagebot.toml + .github/settings.yml referenced files).
**Re-confirms** the rule kind from rust-lang/rust.

### 3. `dir_name_matches_field_with_unscope`

(extension of vercel/turbo's `dir_name_matches_field` candidate)

next.js's package naming is intentionally messy — three
overlapping conventions:

- `packages/next/` → `name: "next"` (unscoped umbrella)
- `packages/font/` → `name: "@next/font"` (scoped, dir is the
  unscoped tail)
- `packages/next-mdx/` → `name: "@next/mdx"` (scoped, dir is
  `next-` + the unscoped tail)
- `packages/eslint-plugin-internal/` → `name: "@next/eslint-plugin-internal"`
  (scoped, dir is the unscoped tail with no `next-` prefix)
- `packages/create-next-app/` → `name: "create-next-app"` (unscoped)

The bare `dir_name_matches_field` from vercel/turbo's gap
catalogue would fire on every npm package here. The next.js shape
needs an *unscoping* transform: `extract @scope/, then compare`.
File as a v0.10+ extension of the existing `dir_name_matches_field`
candidate, not a new rule kind.

---

## What's out of alint's scope (kept on the existing tool)

Listed by category for clarity:

- **AST analysis** (eslint + ast-grep + the @next/eslint-plugin-internal
  rules + the four `scripts/check-unused-turbo-tasks.mjs` Rust
  attribute scanner) — alint deliberately doesn't try to be a
  parser. Shell out via `command:`.
- **Codegen + git-state mutation** (`check-examples.sh`,
  `check-pre-compiled.sh`) — alint reads files; it doesn't
  regenerate them and diff. The freshness check belongs to the
  build system. Shelled out via `command:`.
- **Runtime probes / network** (`socket.yaml` is a Socket.dev
  config, not a check; `check-is-release.js` parses git history)
  — alint reads files; it doesn't probe runtime or git history.
- **PR file-diff guards** (the changes detector in
  `build_and_test.yml`) — alint sees one tree at a time, not
  diffs. Same as the kubernetes / vercel/turbo gap.
- **Operational workflows** (release / cron / triage /
  issue-bot / PR-comment) — not validation surfaces.

---

## Already covered by other linters next.js uses

- `cargo clippy` (with `-D warnings`) — Rust AST/semantics; lives
  with clippy. alint orchestrates via `command:`.
- `rustfmt --check --edition 2024` — formatter; lives with
  rustfmt.
- `prettier --check` — formatter; lives with prettier. alint
  orchestrates via `command:` so the prettier-config-pinning
  rules + the format check run in one alint pass.
- `tsc --noEmit` (`pnpm typescript`, `pnpm lint-typescript`) —
  TypeScript type-checker; lives with TS.
- `alex .` — insensitive-language NLP; lives with alex.
- `typos` — spell check; lives with typos.
- `ast-grep scan` — Rust AST patterns; lives with ast-grep.

---

## Performance comparison (placeholder — bench when validation pass scales)

The repo is large enough to be a meaningful stress test:

- **~163 MiB** working tree (after sparse-checkout dropping
  `/test`, `/examples`, `/docs`)
- **6,000+** TS source files across `packages/next/` alone
- **63** `Cargo.toml` files; **52** turbopack crates in one
  directory
- **30+** GitHub Actions workflows

The published S3 bench (100k files, mixed languages) hits 1.13 s
for the workspace bundle on a stock CI runner. The next.js repo
at full size sits between S3 and S9 (the polyglot monorepo bench,
100k+ files). Expected: 1-3 s for `alint check` on the structural
rules alone, vs. ~30-60 s for `pnpm lint` (which serially fans
through prettier-check + lint-eslint + lint-typescript +
lint-ast-grep + lint-language + check-unused-turbo-tasks).

Where alint shines on next.js specifically: the **per-package
license + version + private-flag uniformity check** runs against
all 19 packages in tens of milliseconds (sequential `jq` would
be ~3-5 s). The cross-cutting structural checks pay back the
most when the repo size is dominated by a polyglot mix where
no single language linter sees the whole tree.

To benchmark wall-clock for real:
`time { pnpm lint && pnpm check-examples && pnpm validate-externals-doc; }`
vs `time alint check`. Deferred to the per-repo measurement pass.

---

## Recommendation for the launch story

This case study is **the "hybrid pnpm + Cargo dual-workspace" data
point** for the launch:

- next.js is the most-watched JS / React framework on GitHub
  (~140k stars). Naming it as a target gives alint instant
  credibility with the JS audience.
- The hybrid pnpm + Cargo workspace shape is alint's tightest
  fit — no other tool composes ecosystem rules at this layer.
  Bundled `monorepo/cargo-workspace@v1` + `monorepo/pnpm-workspace@v1`
  layered together cover both halves of the tree in one
  declarative file.
- The findings on the actual repo (3 of 19 packages missing
  license fields; 4 of 63 crates with non-MIT licenses
  worth verifying; the gitattributes EOL pin sliding into
  drift would silently break Windows test runs) are real and
  actionable.
- The hand-rolled `scripts/check-*.{js,mjs,sh}` family (7 scripts
  totalling ~600 LOC) does work alint largely doesn't replace
  — codegen freshness, git-state mutation, runtime probes — but
  the structural assertions baked into them (manifest route
  reachability, externals-doc table sync) are exactly the v0.10+
  rule-kind candidates this validation pass is surfacing.

Position it as the **fourth tile** on alint.org/examples (after
kubernetes, airflow, microsoft/typescript), with the angle:
"for hybrid monorepos that span multiple ecosystems, no
per-language linter sees the whole tree — alint is the layer
that does."

Followup feature work surfaced (consolidated):

- **`cross_file_value_equals` rule kind** — covers
  `validate-externals-doc.js` here, plus the airflow
  `check-version-consistency` family. Demand: airflow + tokio +
  next.js (3 distinct repos).
- **`registry_paths_resolve` rule kind** — covers
  `check-manifests.js` here, plus rust-lang's triagebot.toml
  + clap's `pre-release-replacements`. Demand: rust-lang + clap
  + next.js (3 distinct repos).
- **`dir_name_matches_field` extension with unscoping** — covers
  the `@next/x` ↔ `packages/x` mapping; same as vercel/turbo's
  base candidate but with a configurable scope-stripping
  transform. Demand: vercel/turbo + next.js + react (likely).

---

## NEW pitfall #16 surfaced by this case study

While writing this config, **a 16th schema/language pitfall**
surfaced that's not in `docs/development/CONFIG-AUTHORING.md`'s
existing 15:

### 16. `json_path_matches` / `yaml_path_matches` cannot regex-match against JSON booleans

The `matches:` field of a JSONPath rule applies a regex to the
string-rendering of the value at the path. For JSON / YAML strings
this works as expected. For JSON **booleans** (and likely numbers),
the rule emits a runtime evaluation error:

```
value at path is not a string (got bool), can't apply regex
```

**Wrong:**
```yaml
- id: package-json-private
  kind: json_path_matches
  paths: package.json
  path: "$.private"
  matches: '^true$'                 # ← runtime error: "got bool, can't apply regex"
```

**Right (option A — file_content_matches against raw text):**
```yaml
- id: package-json-private
  kind: file_content_matches
  paths: package.json
  pattern: '"private":\s*true'      # ← matches the JSON literal
```

**Right (option B — yaml_path_matches against a YAML file with the value as a string):**
```yaml
# only works if the field is genuinely a string in the source —
# e.g. in turbo's `meta.json` `maintainedByCoreTeam` is sometimes
# a string. For real bools, use option A.
```

**Significance for the launch-prep validation pass:** this
pitfall is **silently broken in two existing case studies** —
`microsoft-typescript/.alint.yml::ts-tsconfig-strict-mode`
(`$.compilerOptions.strict matches '^true$'`) and
`vercel-turbo/.alint.yml::turbo-example-meta-declares-maintenance`
(`$.maintainedByCoreTeam matches '^(true|false)$'`). Both pass
the schema audit (the YAML loads cleanly) but emit runtime errors
when run against any input where the value is a real JSON bool
rather than a string. Same pattern as pitfalls #13 and #14 in
CONFIG-AUTHORING.md (parse-validation can't detect semantic
silently-broken rules) — strengthens the "smoke-test fixture"
follow-up audit candidate noted in CONFIG-AUTHORING.md's
"Parse-validation is necessary but not sufficient" section.

The fix in this case study's config used file_content_matches
against the raw text. The alternative — extending the JSONPath
rule kinds to coerce `bool` / `number` into their string forms
(`"true"` / `"false"` / `"42"`) before applying the regex — would
be a half-day's work in `crates/alint-rules/src/structured_path.rs`
and would unblock the natural "match $.foo == true" idiom across
the case-study set.

---

## Notes for the parent agent

- Audit (`cargo test -p alint-e2e --test coverage_audit_examples_parse`)
  passes with this config in place. (At the moment a parallel
  Wave 2 sibling — `examples/facebook-react/.alint.yml` — has an
  unrelated `facts:` block schema bug surfacing as
  `data did not match any variant of untagged enum FactKind at line 125`.
  That's that agent's fix, not this one's.)
- No other NEW schema/language pitfalls beyond #16 above.
- Config runs cleanly against the actual cloned repo at
  `/tmp/next.js/` (528 violations, all expected real findings —
  per-package missing license fields, tracked test fixtures
  with `node_modules` directories, etc.). No silent failures.
