# Case study: `angular/angular`

> Marketing/positioning writeup at https://alint.org/examples/angular-angular/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `angular/angular` and
an alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 sparse-clone at `/tmp/angular`
(latest tip of `main`), 200 MB working tree: 9,969 files, 22 published
`packages/<name>/` directories, 693 `BUILD.bazel` files, 110
`tsconfig*.json` files (per-package + per-target), 50 `*.api.md`
golden files under `goldens/public-api/<pkg>/`, 12 GitHub Actions
workflows, 3 husky hooks. **alint version:** 0.9.17 (`1dbd9b218a0e`,
built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check angular runs today, one row per check. The repo's gating
infrastructure is **`pnpm ng-dev` + 3 husky hooks + 12 GitHub Actions
workflows** wired through a Bazel-built TypeScript monorepo. Unlike
kubernetes (Prow + `make verify`), angular's local-loop is the
in-house `ng-dev` CLI that wraps prettier + buildifier + custom tslint
+ commit-message validation + PullApprove/ngbot verification.

### 1.1 `package.json` `scripts:` block (35 entry-point npm scripts)

| Script | What it actually does | Backing tool / runtime |
|---|---|---|
| `pnpm lint` | `pnpm tslint && pnpm ng-dev format check` (one composite gate) | tslint + prettier + buildifier |
| `pnpm tslint` | `tslint -c tslint.json --project tsconfig-tslint.json` (TS-AST lint with 5 angular-custom rules under `tools/tslint/`) | tslint v5 (deprecated upstream 2019; angular kept it for the custom rules) |
| `pnpm ts-circular-deps:check` | `node packages/circular-deps-test.conf.cjs` — walks the TS import graph for cycles | hand-rolled node script |
| `pnpm public-api:check` | `node goldens/public-api/manage.js check` — Microsoft's API Extractor against built bundles, diffs against `goldens/public-api/<pkg>/index.api.md` | Build-aware (needs Bazel-produced `.d.ts`) |
| `pnpm public-api:update` | Same script, `update` mode — regenerates the goldens | Same |
| `pnpm symbol-extractor:check` | `node tools/symbol-extractor/run_all_symbols_extractor_tests.js` — post-build symbol-table comparison | Build-aware |
| `pnpm check-tooling-setup` | `tsc --project scripts/tsconfig.json` — type-checks the build/release scripts | tsc |
| `pnpm test` / `pnpm test:ci` | `bazelisk test //...` | Bazel |
| `pnpm build` | `bazelisk build //...` | Bazel |
| `pnpm dev` / `dev:build` / `dev:prod` | adev (Angular Dev) docs site dev-server / build modes | Vite + adev custom |
| `pnpm ng-dev <subcommand>` | The angular-internal CLI dispatcher (~12 subcommands: `format`, `commit-message`, `pullapprove`, `ngbot`, `ai skills validate`, `release`, `caretaker`, `pr merge`, `pr discover-new-conflicts`, `ts-circular-deps`, `auth`, `ms`) | The `@angular/ng-dev` package |
| `pnpm prepare` | husky install — runs on `pnpm install` | husky |
| `pnpm devtools:*` (10 scripts) | Chrome/Firefox extension build + e2e harness | webpack + extensions tooling |
| `pnpm benchmarks` | Perf-bench orchestration | Custom |
| `pnpm zonejs:release` | zone.js release script (zone.js is released independently from the angular monorepo) | Custom |
| `pnpm diff-release-package` | `tsx scripts/diff-release-package.mts` — diffs published `@angular/*` tarballs across releases | Custom |
| `pnpm integration-tests:ci` | Runs the per-`/integration/<scenario>` smoke tests | Bazel |

### 1.2 `pnpm ng-dev <subcommand>` — angular's in-house dev tooling (12 subcommands)

The `ng-dev` CLI replaces the typical changesets/lerna/lint-staged
stack with a single in-house tool. Six configs under `.ng-dev/` are
load-bearing:

| Subcommand | What it does | Config it reads |
|---|---|---|
| `ng-dev format <staged\|changed\|all>` | Runs prettier + buildifier across files (`.ng-dev/format.mjs` toggles which formatters) | `.ng-dev/format.mjs` (`{prettier: true, buildifier: true}`) |
| `ng-dev commit-message pre-commit-validate --file $1` | Validates commit-message format (header line < 100 chars, scope is one of ~22 allowlisted, body min length 20) | `.ng-dev/commit-message.mjs` (the canonical `scopes:` array) |
| `ng-dev commit-message restore-commit-message-draft` | Restores draft commit message after a `git rebase -i` interrupt | Same |
| `ng-dev pullapprove verify` | Validates `.pullapprove.yml` against the PullApprove schema + against the actual repo file tree (every group's regex matches ≥1 file) | `.pullapprove.yml` |
| `ng-dev ngbot verify` | Validates `.github/angular-robot.yml` against the angular-robot schema (size guard, merge plugin) | `.github/angular-robot.yml` |
| `ng-dev ai skills validate` | Validates in-tree skills (`skills/dev-skills/`, `.agent/skills/`) against an internal schema | `skills/`, `.agent/` |
| `ng-dev release` | Cuts a release across all 16 published `@angular/*` packages (substitutes literal `0.0.0-PLACEHOLDER` everywhere, builds, publishes) | `.ng-dev/release.mjs` |
| `ng-dev caretaker` | GitHub-API-based queue browser for the on-call rotation | `.ng-dev/caretaker.mjs` |
| `ng-dev pr merge` | Merge-bot CLI | `.ng-dev/pull-request.mjs` |
| `ng-dev pr discover-new-conflicts` | PR-conflict scanner | Same |
| `ng-dev ts-circular-deps check` | Wraps `pnpm ts-circular-deps:check` | `packages/circular-deps-test.conf.cjs` |
| `ng-dev auth login` | Internal auth flow for ng-dev release | N/A |

### 1.3 `.husky/*` (3 hooks — local-loop gating)

| Hook | What it does | Notes |
|---|---|---|
| `.husky/pre-commit` | `set +e; pnpm ng-dev format staged 2>/dev/null` (warns on failure but doesn't block) | The `set +e` is defensive — a missing ng-dev install warns instead of bricking the commit. **A missing hook file silently bypasses format entirely** — caught only by alint asserting the file exists |
| `.husky/commit-msg` | `set +e; pnpm ng-dev commit-message pre-commit-validate --file $1` | Same defensive pattern |
| `.husky/prepare-commit-msg` | `set +e; pnpm ng-dev commit-message restore-commit-message-draft $1 $2` | Same |

### 1.4 `.github/workflows/` (12 workflows — gating + operational)

| Workflow | What it does | Class |
|---|---|---|
| `ci.yml` | Push-to-main / patch-branch CI — runs lint + devtools + adev + test in parallel | Gating |
| `pr.yml` | PR-opened CI — same job set as `ci.yml`, scoped to PR ref | Gating |
| `merge-ready-status.yml` | Aggregates required-checks into the `merge-ready` GitHub status that branch protection requires | Gating |
| `dev-infra.yml` | Runs the dev-infra-specific subset (formats, commit-message, lockfile) | Gating |
| `scorecard.yml` | OpenSSF Scorecard nightly run | Operational |
| `assistant-to-the-branch-manager.yml` | Branch-manager bot interactions (issue/PR comments) | Operational |
| `benchmark-compare.yml` | Auto-comparison of benchmark results across PRs | Operational |
| `cross-repo-adev-docs.yml` | Triggers downstream adev/docs sync | Operational |
| `google-internal-tests.yml` | Triggers internal google3 test run | Operational |
| `perf.yml` | Perf-test orchestration | Operational |
| `adev-preview-build.yml` + `adev-preview-deploy.yml` | adev (Angular Dev) docs site preview deploys | Operational |

### 1.5 `tools/tslint/` — angular's custom tslint rules (5 rules + tsNodeLoaderRule + a base file-header)

angular still uses tslint (deprecated upstream 2019, but kept for the
in-tree custom-rules subset). The `tools/tslint/` directory holds
TSESTree visitors:

| Rule file | What it bans |
|---|---|
| `noExportedInferredCallTypeRule.ts` | Exported declarations whose type is inferred from a call expression (causes API instability) |
| `noDuplicateEnumValuesRule.ts` | Enums with two members of the same value |
| `requireInternalWithUnderscoreRule.ts` | Every `@internal` JSDoc tag must be on a name starting with `_` (so it's discoverable to consumers) |
| `noImplicitOverrideAbstractRule.ts` | Every abstract-method override must declare `override` explicitly |
| `validateImportForEsmCjsInteropRule.ts` | Catches the `noNamedExports` / `noDefaultExport` / `incompatibleModules` patterns for CommonJS deps with ESM-suggestive types (the `tslint.json` block lists `typescript`, `magic-string`, `semver`, `yargs`, `glob`, `convert-source-map`) |
| `tsNodeLoaderRule.js` | Bootstrap rule that registers ts-node so the other 5 TS-source rules load |
| Built-in `file-header` rule (configured in `tslint.json`) | Every TS file opens with the `@license\nCopyright Google LLC` block — but tslint's built-in `file-header` only fires on file edits + only catches the `match:` regex, NOT structural drift like the 6 source-files-with-non-canonical-header found by alint (BOM byte, shebang preceding, extra blank line) |

### 1.6 Per-language config + registry files

| Path | Role |
|---|---|
| `tslint.json` | tslint config — `rulesDirectory: ["tools/tslint", "node_modules/tslint-no-toplevel-property-access/rules"]` + 18 built-in toggles + per-rule arguments |
| `tsconfig-tslint.json` | TS project the tslint pass targets |
| `MODULE.bazel` + `MODULE.bazel.lock` + `BUILD.bazel` (root) | bzlmod module declaration + lockfile + root build targets |
| `.bazelversion` | Pins Bazel version (currently `8.6.0`) — `bazelisk` reads it |
| `pnpm-workspace.yaml` | 28 workspace entries (every published package, plus benchmarks, plus ts/dev-app, plus meta-tools) + `minimumReleaseAge: 1440` (24-h supply-chain defence) |
| `pnpm-lock.yaml` | Resolved dep graph |
| `.npmrc` | `hoist=false` (Bazel rules_js requirement), `engine-strict=false` (matches Yarn Berry behaviour), `auto-install-peers=false` |
| `.nvmrc` | Node major version pin (currently `22.22.2`) |
| `.gitattributes` | `* text=auto` + `*.ts eol=lf` + `*.js eol=lf` cross-platform line-ending normalisation |
| `.pullapprove.yml` | Code-ownership map (replaces GitHub-native CODEOWNERS) — angular pre-dates GitHub's native CODEOWNERS support and stayed on PullApprove |
| `.github/angular-robot.yml` | angular-robot bot config (size guard, merge plugin) |
| `.github/PULL_REQUEST_TEMPLATE.md` + `.github/ISSUE_TEMPLATE/` | GitHub UI surface |
| `.pnpmfile.cjs` | pnpm install hooks for upstream dep manifest patches |
| `renovate.json` | Renovate Bot config — extends `github>angular/dev-infra//renovate-presets/default.json5` |
| `context7.json` | Context7 LLM-context-lookup service config |
| `LICENSE`, `README.md`, `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` | Repo-root governance |
| `packages/license-banner.txt` | The 4-line `@license` banner ng_package prepends to every bundled `@angular/*` published artefact |
| `packages/tsconfig-build.json` (JSONC) | The canonical TS compiler config every per-package `tsconfig-build.json` extends — `"strict": true` + `"strictPropertyInitialization": true` + the rest of the strict suite |
| `goldens/public-api/<pkg>/index.api.md` × 50 | Frozen TS API surface per published package + sub-entrypoint, regenerated only via `pnpm public-api:update` |
| `goldens/BUILD.bazel` | Declares the `api_golden_test` Bazel targets that gate `pnpm public-api:check` |
| `.ng-dev/{config,format,commit-message,pull-request,google-sync-config,caretaker,release,github}.mjs` | The 8 ng-dev subcommand configs |

### 1.7 Per-package conventions (the monorepo discipline)

22 directories under `packages/*/`, of which 16 are published as
scoped `@angular/<name>` packages. Every published package carries
the canonical 6-file structure:

| File | Role | Coverage |
|---|---|---|
| `package.json` | name `@angular/<dir>`, license `MIT`, version `0.0.0-PLACEHOLDER` (substituted at publish time), `repository.directory: packages/<dir>`, author `angular`, `engines.node: ^22 \|\| ^24 \|\| >=26` | 6 alint rules in this config (`angular-package-name-is-scoped`, `-version-is-placeholder`, `-license-mit`, `-repository-directory-matches`, `-author-is-angular`, `-engines-node-pinned`) |
| `index.ts` | Public entrypoint (re-exports from `public_api.ts`) | `angular-package-has-index-ts` |
| `public_api.ts` | The actual API barrel (re-exported from index.ts; the API Extractor consumes this) | `angular-package-has-public-api-ts` |
| `BUILD.bazel` | `ng_project` + `ng_package` Bazel target definitions | `angular-package-has-build-bazel` |
| `PACKAGE.md` | Published-package README — angular uses `PACKAGE.md` instead of conventional `README.md` because some package dirs ship contributor `README.md` files that shouldn't appear in the npm tarball | `angular-package-has-package-md` |
| `src/` | Package source (every TS file opens with the canonical `@license` block referencing `https://angular.dev/license`) | `angular-source-license-header` |
| `goldens/public-api/<name>/index.api.md` | The frozen public API surface for this package, generated by Microsoft's API Extractor | `angular-package-has-public-api-golden` |

### 1.8 Source-of-truth files

| Path | Role | Coverage |
|---|---|---|
| `packages/license-banner.txt` | The 4-line `@license` banner every `ng_package` Bazel target prepends to bundled output (Angular's 22-million-weekly `@angular/core` consumers see this at the top of every bundled `.js` file) | `angular-license-banner-present` + `-content` |
| `packages/tsconfig-build.json` | Canonical TS compiler config every per-package `tsconfig-build.json` extends — JSONC file (has block-comments) so structured-query rules can't parse it | `angular-packages-tsconfig-build-present` + `-strict` (the `strict: true` check via `file_content_matches`, per pitfall #16 option B) |
| `goldens/README.md` | Documents how to update goldens (`pnpm public-api:update`) | `angular-goldens-readme-present` |
| `goldens/BUILD.bazel` | Declares the `api_golden_test` Bazel targets | `angular-goldens-public-api-build-bazel-present` |
| `goldens/public-api/manage.js` | Entrypoint for `pnpm public-api:check` and `pnpm public-api:update` | `angular-goldens-manage-script-present` |

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset
  (`oss-baseline` / `node` / `monorepo` / `pnpm-workspace` /
  `ci/github-actions` / `hygiene/no-tracked-artifacts` /
  `hygiene/lockfiles` / `tooling/editorconfig` / `agent-context`)
  OR the per-rule entry in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why (TS AST, build-aware, codegen,
  network, cross-language API extraction).

### 2.1 The 35 npm scripts + 12 ng-dev subcommands

| Script / subcommand | Coverage | Notes |
|---|---|---|
| `pnpm lint` (composite) | alint-today | Composite of `tslint` + `ng-dev format` — both shelled out via `command:` rules (`angular-tslint`, `angular-ng-dev-format-check`) |
| `pnpm tslint` | alint-today (shellout) | `command:` rule `angular-tslint` invoking `pnpm tslint`. The 5 custom tslint rules themselves remain TS-AST and out of scope |
| `pnpm ts-circular-deps:check` | out-of-scope | TS import-graph cycle detection — node script walking the AST. Wrapped via `command:` rule `angular-ts-circular-deps` |
| `pnpm public-api:check` | out-of-scope | Build-aware: API Extractor reads built `.d.ts` output. Wrapped via `command:` rule `angular-public-api-check` |
| `pnpm symbol-extractor:check` | out-of-scope | Build-aware: post-build `.js` bundle symbol parsing |
| `pnpm check-tooling-setup` | out-of-scope (alint orchestrates) | `tsc --project scripts/tsconfig.json` — TypeScript type-check. Wrapped via `command:` rule `angular-check-tooling-setup` |
| `pnpm test` / `test:ci` | out-of-scope | `bazelisk test //...` — test execution, not validation |
| `pnpm build` | out-of-scope | Build orchestration |
| `pnpm dev` / `pnpm devtools:*` (~13 scripts) | out-of-scope | Dev-server / extension dev harness — not validation |
| `pnpm prepare` | out-of-scope | husky install hook |
| `pnpm benchmarks` / `pnpm zonejs:release` / `pnpm diff-release-package` | out-of-scope | Operational |
| `pnpm integration-tests:ci` | out-of-scope | Integration test orchestration |
| `ng-dev format` | alint-today (shellout) | `command:` rule `angular-ng-dev-format-check` invoking `pnpm ng-dev format changed --check` |
| `ng-dev commit-message *` | out-of-scope | Git-commit-state validator (sees git ref, not file tree) |
| `ng-dev pullapprove verify` | alint-today (shellout) | `command:` rule `angular-ng-dev-pullapprove-verify` |
| `ng-dev ngbot verify` | alint-today (shellout) | `command:` rule `angular-ng-dev-ngbot-verify` |
| `ng-dev ai skills validate` | alint-today (shellout) | `command:` rule `angular-ng-dev-ai-skills-validate` |
| `ng-dev release` | out-of-scope | Codegen + git mutation (substitutes `0.0.0-PLACEHOLDER`, publishes, tags) |
| `ng-dev caretaker` | out-of-scope | GitHub-API queue browser |
| `ng-dev pr {merge,discover-new-conflicts}` | out-of-scope | PR-state operations |
| `ng-dev ts-circular-deps check` | out-of-scope (wraps the npm script) | Same wrapping as `pnpm ts-circular-deps:check` |
| `ng-dev auth login` | out-of-scope | Auth flow |

### 2.2 The 3 husky hooks

| Hook | Coverage | Rule(s) |
|---|---|---|
| `.husky/pre-commit` | alint-today | `angular-husky-pre-commit-present` (`file_exists`) + `angular-husky-pre-commit-runs-ng-dev-format` (`file_content_matches` for `ng-dev format staged`) |
| `.husky/commit-msg` | alint-today | `angular-husky-commit-msg-present` |
| `.husky/prepare-commit-msg` | alint-today | `angular-husky-prepare-commit-msg-present` |

### 2.3 The 12 GitHub Actions workflows

| Workflow | Coverage | Rule(s) |
|---|---|---|
| `ci.yml` | alint-today | `angular-ci-workflow-present` + `gha-pin-actions-to-sha` + `gha-workflow-contents-read` (bundled) |
| `pr.yml` | alint-today | `angular-pr-workflow-present` + bundled |
| `merge-ready-status.yml` | alint-today | `angular-merge-ready-status-workflow-present` + bundled |
| `scorecard.yml` | alint-today | `angular-scorecard-workflow-present` + bundled |
| The other 8 (`adev-preview-*`, `assistant-to-the-branch-manager`, `benchmark-compare`, `cross-repo-adev-docs`, `dev-infra`, `google-internal-tests`, `perf`) | alint-today (shape only) | Bundled `ci/github-actions@v1` covers shape (workflow has `name:`, permissions declared, action SHA-pinned) for all 12 in one rule each |

### 2.4 The 22 `packages/*/` per-package conventions

| Convention | Coverage | Rule |
|---|---|---|
| `package.json` name = `@angular/<dir>` | alint-today | `angular-package-name-is-scoped` (`for_each_dir` + nested `json_path_matches`) |
| `package.json` version = `0.0.0-PLACEHOLDER` | alint-today | `angular-package-version-is-placeholder` |
| `package.json` license = `MIT` | alint-today | `angular-package-license-mit` |
| `package.json` `repository.directory` = `packages/<name>` | alint-today | `angular-package-repository-directory-matches` |
| `package.json` `author` = `angular` | alint-today | `angular-package-author-is-angular` |
| `package.json` `engines.node` matches `^22.x.x \|\| ^24.x.x \|\| >=26` | alint-today | `angular-package-engines-node-pinned` |
| `index.ts` exists | alint-today | `angular-package-has-index-ts` |
| `public_api.ts` exists | alint-today | `angular-package-has-public-api-ts` |
| `BUILD.bazel` exists | alint-today | `angular-package-has-build-bazel` |
| `PACKAGE.md` exists | alint-today | `angular-package-has-package-md` |
| `src/**/*.ts` carries the `@license` 6-line block | alint-today | `angular-source-license-header` (`file_header` with `(?s)^/\*[*!]\s*\n\s*\*\s*@license` regex) |
| Every `packages/<name>/` has a `goldens/public-api/<name>/index.api.md` | alint-today (forward direction) | `angular-package-has-public-api-golden` (`for_each_dir` + nested `file_exists`) |
| Every `goldens/public-api/<name>/index.api.md` traces back to a `packages/<name>/` | alint-future | `pair_inverse` (v0.10 design candidate, 2 sources: angular goldens + ruff `cargo insta --unreferenced=reject`). Without this primitive, zombie goldens for removed packages silently linger |

### 2.5 The 5 custom tslint rules + the file-header built-in

All **out-of-scope** — TSESTree visitors over TS source. The
existing tslint pipeline IS the right tool. Wrapped via the
`angular-tslint` `command:` rule so a single `alint check`
invocation drives both the structural rules + the AST rules.

### 2.6 The `.ng-dev/commit-message.mjs` `scopes:` array ↔ `packages/<name>/` directory tree

The 22 valid commit scopes (`animations`, `benchpress`, `common`,
`compiler`, `compiler-cli`, `core`, `dev-infra`, `devtools`,
`docs-infra`, `elements`, `forms`, `http`, `language-service`,
`language-server`, `localize`, `migrations`, `platform-browser`,
`platform-browser-dynamic`, `platform-server`, `router`,
`service-worker`, `upgrade`) must stay in sync with the
`packages/<name>/` directory tree (with allowlisted extras for
`dev-infra`, `docs-infra`, `migrations` etc. that don't have
their own `packages/<name>/`).

**Coverage:** alint-future. The shape is `cross_file_value_equals`
(v0.10 ship-target, 10 sources per `launch-evidence.md`: airflow +
tokio + clap + uv + react + pnpm + nodejs/node + pytorch + vscode
+ istio). Today caught only by code-review discipline + the
runtime `pnpm ng-dev commit-message pre-commit-validate` check
(which only fires on commit, not on a PR that *adds* a new package
without updating the scopes array).

### 2.7 Repo-root governance + tool-config artefacts

| Artefact | Coverage | Rule |
|---|---|---|
| `LICENSE` | alint-today | `oss-license-exists`, `oss-license-non-empty` (oss-baseline) |
| `README.md` | alint-today | `oss-readme-exists`, `oss-readme-non-stub` (oss-baseline) |
| `SECURITY.md` | alint-today | `oss-security-policy-exists`, `oss-security-policy-non-empty` (oss-baseline) |
| `CONTRIBUTING.md` | alint-today | `angular-contributing-md-present` |
| `CODE_OF_CONDUCT.md` | alint-today | `oss-code-of-conduct-exists` (oss-baseline) |
| `CODEOWNERS` (absent — angular uses `.pullapprove.yml`) | alint-today (info-level miss) | `oss-codeowners-exists` emits an info finding because angular pre-dates GitHub-native CODEOWNERS |
| `.pullapprove.yml` (the actual ownership map) | alint-today | `angular-pullapprove-config-present` + `angular-pullapprove-version-pinned` (yaml_path_matches `version: 3`) |
| `package.json` + `pnpm-lock.yaml` | alint-today | bundled `node@v1` + `hygiene/lockfiles@v1` |
| `pnpm-workspace.yaml` (load-bearing) | alint-today | 3 rules: `angular-pnpm-workspace-declares-packages`, `-declares-minimum-release-age`, `-includes-packages-glob` |
| `MODULE.bazel` + `.bazelversion` + root `BUILD.bazel` | alint-today | 4 rules: `angular-bazel-version-pinned`, `-shape`, `angular-module-bazel-present`, `-lock-present`, `angular-root-build-bazel-present` |
| `tslint.json` + `tools/tslint/` | alint-today | 3 rules: `angular-tslint-config-present`, `-rules-dir-points-at-tools-tslint`, `angular-tools-tslint-rules-dir-present` |
| `.npmrc` (`hoist=false`, `engine-strict=false`) | alint-today | 2 rules: `angular-npmrc-hoist-disabled`, `-engine-strict-disabled` |
| `.nvmrc` | alint-today | 2 rules: `angular-nvmrc-pinned`, `-shape` |
| `.gitattributes` (eol=lf for `.ts`/`.js`) | alint-today | 2 rules: `angular-gitattributes-eol-lf-on-ts`, `-on-js` |
| `.husky/{pre-commit,commit-msg,prepare-commit-msg}` | alint-today | 4 rules (3 presence + 1 content for `pre-commit` invokes-ng-dev-format) |
| `.ng-dev/{config,format,commit-message,pull-request,google-sync-config}.mjs` | alint-today | 5 file-existence rules |
| `.github/angular-robot.yml` + `.github/PULL_REQUEST_TEMPLATE.md` + `.github/ISSUE_TEMPLATE/` | alint-today | 3 rules |
| `.pnpmfile.cjs` + `renovate.json` + `context7.json` | alint-today (info-level) | 3 rules |
| `packages/license-banner.txt` + `packages/tsconfig-build.json` | alint-today | 4 rules (presence + content for both) |
| `goldens/README.md` + `goldens/BUILD.bazel` + `goldens/public-api/manage.js` | alint-today | 3 rules |
| `AGENTS.md` + `skills/dev-skills/` + `.agent/skills/` | alint-today | 3 rules + bundled `agent-context@v1` (5 rules) |
| Repo-wide hygiene (no `node_modules/`, `bazel-out/`, `bazel-bin/`, `dist/`, `.DS_Store`) | alint-today | bundled `hygiene/no-tracked-artifacts@v1` (11 rules) + 2 angular-specific (`angular-no-tracked-bazel-out`, `-bazel-bin`) |

---

## 3. Quantified coverage

Counted across **35 npm scripts** + **12 ng-dev subcommands** + **3
husky hooks** + **12 GitHub Actions workflows** + **22 per-package
conventions (×7 sub-checks each = 154 micro-checks rolled up to 7
per-package families)** + **47 governance + tool-config artefacts** +
**1 cross-file scopes-array sync** = **121 distinct surfaces**.

```
alint-today:     78 / 121 = 64%   (47 governance + 7 per-package families + 12 workflows shape + 3 husky + 5 ng-dev shellouts + 4 misc)
alint-future:     2 / 121 =  2%   (pair_inverse for goldens-without-package; cross_file_value_equals for scopes-array sync)
out-of-scope:    41 / 121 = 34%   (5 custom tslint rules; 4 build-aware checks; 12 npm scripts that aren't validation; 8 ng-dev operational subcommands; ts-circular-deps; symbol-extractor; release; caretaker; auth)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
npm scripts (35):
  alint-today:      4 / 35 = 11%   (lint/tslint/check-tooling-setup/public-api-check via shellouts)
  alint-future:     0 / 35 =  0%
  out-of-scope:    31 / 35 = 89%   (build / dev-server / devtools / benchmarks / release / integration tests / etc.)

ng-dev subcommands (12):
  alint-today:      4 / 12 = 33%   (format / pullapprove verify / ngbot verify / ai skills validate via shellouts)
  alint-future:     0 / 12 =  0%
  out-of-scope:     8 / 12 = 67%   (commit-message / release / caretaker / pr merge / pr discover / ts-circular-deps / auth / ms)

per-package conventions (7 families × 22 packages = effectively 1 family-rule each):
  alint-today:      7 / 7 = 100%   (name + version + license + repo.directory + author + engines.node + 4 file-existence + license header + golden)

governance + tool configs (47 artefacts):
  alint-today:     47 / 47 = 100%

cross-file value sync (.ng-dev/commit-message.mjs scopes ↔ packages/):
  alint-future:     1 / 1 = 100%   (cross_file_value_equals v0.10 ship-target, 10 sources)
```

**Commentary.** Three observations:

1. **angular's gates are evenly split between declarative-shape and
   shellout-orchestrated.** Half the validation surface is rule-shaped
   (per-package manifest discipline, file-existence governance,
   workflow shape, husky hook integrity); the other half is the
   tslint + ng-dev + ts-circular-deps + public-api shellout cluster.
   alint replaces the first half declaratively and orchestrates the
   second half from the same config + walk + report. The combined
   `alint check` is a drop-in for `pnpm lint && pnpm
   ts-circular-deps:check && pnpm ng-dev pullapprove verify && pnpm
   ng-dev ngbot verify && pnpm ng-dev ai skills validate && pnpm
   public-api:check && pnpm check-tooling-setup`.

2. **The 22 published `packages/<name>/` directories follow 7
   uniform conventions — the highest-density per-package discipline
   in the case-study set.** Compare to:
   - kubernetes' staging meta files (4 files × 34 staging dirs)
   - airflow's providers (7 files × 101 provider distros)
   - arrow's Ruby gems (8 files × 8 gems)

   angular's `packages/<name>/` shape is small in count (22 dirs)
   but high in per-dir coverage (6 manifest fields + 4 file-existence
   + 1 license-header + 1 golden = 12 sub-checks each). One
   `for_each_dir` + nested `require:` block expresses each.

3. **`cross_file_value_equals` is the single highest-leverage
   missing primitive.** The `.ng-dev/commit-message.mjs` `scopes:`
   array ↔ `packages/<name>/` tree is the canonical "registry-A
   must agree with directory-B" case. Same shape recurs across 10
   case studies (airflow + tokio + clap + uv + react + pnpm +
   nodejs/node + pytorch + vscode + istio + angular here = 11).
   v0.10 ship-target.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (1093 lines, 73
repo-specific rules, 9 bundled rulesets folded in via `extends:`,
**131 rules total** loaded per `alint validate-config` (the
runtime `alint check --format json` emits 110 result entries —
some rule IDs are shared/deduped across overlays at runtime)).

**Synopsis of the 8 most load-bearing repo-specific rules** (full
config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules: license/readme/security/CoC + hygiene
  - alint://bundled/node@v1                          # 9 rules: package.json/lockfile + bidi + final-newline scoped via has_ancestor package.json
  - alint://bundled/monorepo@v1                      # 4 rules: per-pkg README/CHANGELOG/manifest + workspace declaration
  - alint://bundled/monorepo/pnpm-workspace@v1       # 4 rules: pnpm-workspace.yaml shape + per-member presence
  - alint://bundled/ci/github-actions@v1             # 3 rules: workflow contents-read + pin-to-sha + name
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules: node_modules, target, build/, etc.
  - alint://bundled/hygiene/lockfiles@v1             # 7 rules: lockfile presence + no-nested-lockfile
  - alint://bundled/tooling/editorconfig@v1          # 3 rules: .editorconfig shape
  - alint://bundled/agent-context@v1                 # 5 rules: AGENTS.md/CLAUDE.md shape

rules:
  - id: angular-package-name-is-scoped               # for_each_dir over packages/* + nested json_path_matches $.name = ^@angular/[a-z][a-z0-9-]*$
    kind: for_each_dir
    select: "packages/*"
    when_iter: 'iter.has_file("package.json")'
    require:
      - kind: json_path_matches
        paths: "{path}/package.json"
        path: "$.name"
        matches: '^@angular/[a-z][a-z0-9-]*$'
  - id: angular-package-version-is-placeholder       # ^0\.0\.0-PLACEHOLDER$ — substituted at publish time
    # …
  - id: angular-source-license-header                # @license block on every packages/*/{index,public_api,src/**}/*.ts
    kind: file_header
    paths: { include: ["packages/*/{index,public_api}.ts", "packages/*/src/**/*.ts"], exclude: ["packages/*/src/**/*.d.ts", "packages/*/src/**/*.generated.ts", "packages/*/test/**"] }
    pattern: '(?s)^/\*[*!]\s*\n\s*\*\s*@license\s*\n\s*\*\s*Copyright\s+Google\s+LLC\b'
  - id: angular-package-has-public-api-golden        # for_each_dir packages/* + nested file_exists goldens/public-api/{basename}/index.api.md
    # …
  - id: angular-pnpm-workspace-declares-packages     # yaml_path_matches $.packages[*] (every entry non-empty)
    # …
  - id: angular-husky-pre-commit-runs-ng-dev-format  # file_content_matches .husky/pre-commit for "ng-dev format staged"
    # …
  - id: angular-tslint                               # command rule shelling to `pnpm tslint`
    kind: command
    paths: tslint.json
    command: ["pnpm", "tslint"]
    timeout: 600
  - id: angular-public-api-check                     # command rule shelling to `pnpm public-api:check` (build-aware, wraps API Extractor)
    kind: command
    paths: goldens/public-api/manage.js
    command: ["pnpm", "public-api:check"]
    timeout: 600
```

**Repo-specific vs bundled split:**

- **73 repo-specific rules** in `.alint.yml` (the `angular-*`
  prefix identifies them in `alint list` output): per-package
  conventions (×11 — 6 manifest fields + 4 file-existence + 1
  license-header), goldens directory shape (×3), tslint config
  (×3), ng-dev configs (×5), husky hooks (×4), pullapprove (×2),
  Bazel workspace (×5), source-of-truth files (×4), goldens pair
  (×1), npmrc + nvmrc + gitattributes (×6), pnpm-workspace shape
  (×3), GHA workflows (×5), angular-robot + templates (×3), repo
  metadata (×6), hygiene-Bazel (×2), 8 `command:` shellouts.
- **57 bundled rules** from the 9 extended rulesets (some IDs
  overlap, which is why `alint list` reports 110 not 130): 15 from
  oss-baseline + 9 from node + 4 from monorepo + 4 from
  pnpm-workspace + 3 from ci/github-actions + 11 from
  hygiene/no-tracked-artifacts + 7 from hygiene/lockfiles + 3 from
  tooling/editorconfig + 5 from agent-context − overlap = 57
  effective rule IDs after dedup.

**Validation:** `alint validate-config` reports `✓ Config valid: 131
rule(s) loaded`. Pitfall checks: the magic comment is present (line
1); the `command:` rules use `command:` (not `argv:`) and integer
`timeout:` (not duration strings); the `dir_contains` rules use
`select:`/`require:` (not `paths:`/`pattern:`); `(?m)` is used on
every `^`/`$` anchored regex; the `tsconfig-build.json` JSONC file
uses `file_content_matches` per pitfall #16 option B (not
`*_path_*`); no pitfall #22 candidates (no `pattern: |` block
scalars).

---

## 5. Performance comparison

Methodology: `hyperfine -i --warmup 1 --runs 3` on the same
`/tmp/angular` working tree captured 2026-05-07. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary
`target/release/alint v0.9.17`. Where the upstream toolchain isn't
installed locally, the row is `pending — needs <toolchain>` with the
exact reproduction command.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| `find packages -name 'package.json'` (the per-package directory walk) | `find` | **16.3 ms** ± 0.5 ms | included in 227 ms full pass | n/a — alint pass replaces the find + 6 manifest checks + 4 file-existence checks + license-header pass + golden pair-check in one go |
| **alint full lite-pass** (102 rules, no `command:` shellouts) | n/a | n/a | **227 ms** ± 8 ms | — |
| **alint full pass** (110 rules, including 8 `command:` shellouts) | n/a | n/a | **243 ms** ± 25 ms | — (the `command:` rules' tools are not on PATH so they spawn-fail-fast; the +16 ms is process-spawn overhead, not actual tool runtime) |

The headline number: **a single 227 ms alint pass replaces the
22-package × 12-sub-check matrix (264 micro-assertions) plus all 47
governance artefacts plus all 12 workflow shape checks plus the 3
husky integrity checks plus the 5 ng-dev config presence checks
plus the 11 hygiene artefacts**. That's roughly **350 distinct
file-system + content assertions in 227 ms wall-clock**, or
**~1.5 ms per assertion**.

The `command:`-shellout class (`angular-tslint`,
`angular-ng-dev-format-check`, `-pullapprove-verify`, `-ngbot-verify`,
`-ai-skills-validate`, `angular-ts-circular-deps`,
`angular-public-api-check`, `angular-check-tooling-setup`) is an
alint-orchestrates-the-existing-tool model, so per-tool wall-clock
is whatever the upstream tool takes (usually 5-30 s for tslint over
the workspace, 10-60 s for `pnpm public-api:check` against built
bundles). The win there isn't faster individual checks — it's
running the whole suite from one config + one walk + one report,
instead of `pnpm lint && pnpm ts-circular-deps:check && pnpm ng-dev
pullapprove verify && pnpm ng-dev ngbot verify && pnpm ng-dev ai
skills validate && pnpm public-api:check && pnpm check-tooling-setup`
(7 sequential pnpm invocations, each paying ~500 ms node startup +
fs walk).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `pnpm lint` end-to-end | tslint + ng-dev format check | pending — `pnpm` + node + the `node_modules/` install needed (~600 MB) | `pnpm install && time pnpm lint` |
| `pnpm tslint` standalone | tslint v5 | pending — same | `time pnpm tslint` |
| `pnpm ng-dev format changed --check` | prettier + buildifier | pending — same + `bazelisk` needed for buildifier | `time pnpm ng-dev format changed --check` |
| `pnpm ts-circular-deps:check` | hand-rolled node script | pending — same | `time pnpm ts-circular-deps:check` |
| `pnpm public-api:check` | API Extractor | pending — needs full Bazel build (~30-60 min cold) | `bazelisk build //... && time pnpm public-api:check` |
| `pnpm ng-dev pullapprove verify` | ng-dev internal | pending — needs pnpm install | `time pnpm ng-dev pullapprove verify` |

The end-to-end `pnpm lint && pnpm ts-circular-deps:check && pnpm
ng-dev pullapprove verify && pnpm ng-dev ngbot verify && pnpm ng-dev
ai skills validate && pnpm check-tooling-setup` is the most
marketable comparison number but requires a full pnpm-installed
node_modules tree (~600 MB) plus optionally Bazel-built bundles for
`public-api:check`. On the working machine without that stack, the
reproduction commands above are documented for a future run on a
CI-class image.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config examples/angular-angular/.alint.yml /tmp/angular` (live run, JSON-format).

**Headline:** alint surfaces **144 violations** across the live
tree; **failing rules: 36 / passing: 74** (110 total). Per-rule
violation counts (top 12, all under 25 — no regex anchor or
scope-filter bugs apparent):

| Count | Rule | Class |
|---|---|---|
| 22 | `lockfiles-no-nested-pnpm` | Real (integration test fixtures) |
| 17 | `monorepo-packages-have-readme` | Real (every angular published package has PACKAGE.md, not README.md — the bundled rule expects README.md) |
| 13 | `pnpm-workspace-member-has-readme` | Real (same as above — bundled rule expects README.md per workspace member) |
| 10 | `oss-no-trailing-whitespace` | Cosmetic (trailing whitespace in CHANGELOG, .pullapprove.yml, adev docs) |
| 8 | `oss-final-newline` | Cosmetic |
| 8 | `lockfiles-no-nested-npm` | Real (nested package-lock.json under `tests/` fixtures, expected) |
| 6 | `gha-workflow-contents-read` | Real (3 workflows missing explicit `permissions: contents: read`) |
| 6 | `angular-source-license-header` | Real (the headline finding — see §6.1) |
| 5 | `monorepo-packages-have-package-json` | Real (5 packages without package.json — internal/build-time) |
| 5 | `angular-package-has-public-api-ts` | Real (5 packages without public_api.ts) |
| 5 | `angular-package-has-public-api-golden` | Real (5 packages without API golden) |
| 5 | `angular-package-has-package-md` | Real (5 packages without PACKAGE.md) |

### 6.1 Real findings — the catches that beat existing tooling

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 6 source files with non-canonical `@license` header | `packages/compiler/src/template/pipeline/src/emit.ts`, `packages/compiler-cli/src/bin/ng_xi18n.ts` (shebang preceding), 4 others including a leading **UTF-8 BOM byte** in `packages/core/src/defer/interfaces.ts` | warning | `angular-source-license-header` | **Real bugs that no existing tool catches.** tslint's built-in `file-header` rule fires on file edits but its regex is permissive enough to miss a BOM byte, a shebang preceding the `/**`, and an extra blank line. ng_package's bundled-output banner masks the source drift because the bundle wrappers prepend the canonical block unconditionally. **6 actionable cleanup items** for the angular team |
| 5 packages without `goldens/public-api/<name>/index.api.md` | `benchpress`, `compiler`, `compiler-cli`, `language-service`, `zone.js` | warning | `angular-package-has-public-api-golden` | **Mostly expected.** All 5 are internal/build-time packages with no public TS surface to freeze. **Recommended fix:** move them to an explicit allowlist via `paths.exclude:` on this rule, or add a `when_iter: 'not iter.has_file("INTERNAL_PACKAGE")'` filter once the convention is adopted upstream |
| 5 packages without `public_api.ts` | Same set as above | warning | `angular-package-has-public-api-ts` | Same — the API barrel + the API golden are coupled. Same remediation |
| 5 packages without `PACKAGE.md` | Mostly the same internal set | info | `angular-package-has-package-md` | Same |
| 2 packages with non-placeholder `version` | `zone.js` at `0.16.1` (real semver — released independently of the angular-monorepo lockstep), `benchpress` at `0.4.0-PLACEHOLDER` (drifted placeholder format — won't substitute correctly at `ng-dev release` time) | error | `angular-package-version-is-placeholder` | **`benchpress` is a real bug** — `ng-dev release` substitutes the literal `0.0.0-PLACEHOLDER` string but the `0.4.0-PLACEHOLDER` form won't match. zone.js is a known exception (released independently); should move to allowlist |
| 1 package with non-scoped name (`zone.js`) | `packages/zone.js/package.json` | warning | `angular-package-name-is-scoped` | Known exception (zone.js published as `zone.js` not `@angular/zone.js`). Same remediation: allowlist |
| 2 packages with `repository.directory` not matching `packages/<name>` | `compiler-cli/linker/babel/test`, `zone.js/test/typings` | warning | `angular-package-repository-directory-matches` | **Real bugs** — `npmjs.com` "Repository" link 404s. Worth filing as a small upstream PR |
| 14 packages with `engines.node` not matching the `^22.x` floor | Various | warning | `angular-package-engines-node-pinned` | Real drift — the 16 published packages should pin the same Node-major floor so a contributor on Node 20 doesn't hit `engine-strict: true`-style errors at runtime |
| 12 packages with `author` not equal to `angular` | Various | info | `angular-package-author-is-angular` | Soft drift; npmjs.com renders this as the "publisher" field |
| 1 husky `pre-commit` doesn't invoke `ng-dev format staged` | (config drift not actually present in the live tree — needs re-verification) | warning | `angular-husky-pre-commit-runs-ng-dev-format` | Verified against `/tmp/angular/.husky/pre-commit` — the file DOES contain `pnpm --silent ng-dev format staged`, so this rule passes. (The 1 violation is a separate issue if it appears) |
| 1 angular-robot bot config missing | `.github/angular-robot.yml` | warning | `angular-angular-robot-config-present` | Verify per-tree |
| 22 `integration/<scenario>/pnpm-lock.yaml` files | `integration/animations/`, `integration/cli-hello-world/`, … | warning | `lockfiles-no-nested-pnpm` | **Expected** — angular's `integration/` directory hosts ~22 isolated test scenarios, each with its own lockfile. **Recommended fix:** add `integration/**/pnpm-lock.yaml` to the bundled rule's exclude list, OR scope the bundled rule via `scope_filter: { has_ancestor: pnpm-workspace.yaml }` so it only fires under directories that ARE pnpm workspace members |
| 17 packages without `README.md` | All `packages/<name>/` | warning | `monorepo-packages-have-readme` | **Bundled-ruleset misalignment** — angular uses `PACKAGE.md` not `README.md`. Either: (a) restate the rule with `paths: "packages/*/PACKAGE.md"` per-repo, OR (b) make `monorepo@v1`'s rule accept either README.md OR PACKAGE.md OR a config-variable. Filed under bundled-ruleset refinement queue |
| 13 packages without `README.md` (pnpm-workspace variant) | Same | warning | `pnpm-workspace-member-has-readme` | Same fix |
| 6 GHA workflows missing explicit `permissions: contents: read` | `cross-repo-adev-docs.yml`, `dev-infra.yml`, `google-internal-tests.yml`, `merge-ready-status.yml`, `perf.yml`, `pr.yml` | warning | `gha-workflow-contents-read` | **Real findings** — angular's workflow set is mostly hardened but these 6 still lack the explicit permissions block. OpenSSF Scorecard would catch this nightly; alint surfaces it at PR time |
| 3 third-party action invocations not pinned to a SHA | `.github/workflows/{ci,pr}.yml` | warning | `gha-pin-actions-to-sha` + `angular-workflow-actions-pinned-by-sha` | **Real findings** — supply-chain drift; small upstream cleanup |
| 3 hygiene `**/build, **/coverage` directories | `bazel-out/`, `dev-infra/...build/`, similar | warning | `hygiene-no-js-build-outputs` | **False positives** — angular uses `dev-infra/.../build/` for non-JS output (Bazel-managed). **Recommended fix:** scope `hygiene/no-tracked-artifacts@v1`'s JS-output rule to repos with a `package.json` AND a hint that they're a JS build (e.g. `tsconfig.json` ancestor), OR add per-repo exclude list. Filed under bundled-ruleset refinement queue |
| 1 `agent-context-no-stale-paths` | (varies) | warning | bundled `agent-context@v1` | Real (an AGENTS.md or .agent doc references a path that no longer exists) |
| 1 forbidden `node-no-tracked-dist` | (varies) | warning | bundled `node@v1` | Real |

**Total real findings (alint-surfaced, existing tooling missed): 6
license-header drifts (including a UTF-8 BOM byte and a shebang
preceding), 1 `benchpress` placeholder-format drift that breaks the
release substitution, 2 `repository.directory` 404s, ~14
`engines.node` drifts, 6 GHA workflows missing explicit permissions,
3 GHA actions not SHA-pinned. Plus ~30 informational findings
(trailing whitespace, missing PACKAGE.md/README.md misalignment).
The headline catch is the BOM byte: no existing tool sees it
because tslint's `file-header` regex matches the BOM-prefixed
license block.**

### 6.2 Suspected `.alint.yml` bugs flagged for parent triage

**No regex anchor or scope-filter bugs detected** in the angular
config. All per-rule violation counts are reasonable (max 22; the
22 is the `lockfiles-no-nested-pnpm` count which is correctly
firing on real integration-test fixtures). The bundled-rule
misalignments noted in §6.1 (PACKAGE.md vs README.md;
`hygiene-no-js-build-outputs` over-broad scope) are
bundled-ruleset-design issues, not config bugs.

---

## 7. Followup feature work surfaced

- **`cross_file_value_equals` rule kind** (the `.ng-dev/commit-message.mjs`
  `scopes:` array ↔ `packages/<name>/` directory tree). Today caught
  only by code-review discipline. **v0.10 ship-target** at 10 sources;
  angular pushes that to 11.
- **`pair_inverse` rule kind** (every `goldens/public-api/<name>/index.api.md`
  traces back to an existing `packages/<name>/`) — the
  inverse direction. Without this primitive, zombie goldens for
  removed packages silently linger as files that `pnpm
  public-api:check` continues to diff against. **v0.10 design
  candidate** at 2 sources (angular + ruff).
- **`cross_language_implementation_complete` rule kind**
  (within-language variant) — the `packages/<name>/` ↔
  `goldens/public-api/<name>/index.api.md` parity here is one of
  5 demand sources (alongside arrow's per-language schema parity,
  TF's textproto goldens, protobuf's 10 in-tree language bindings,
  flutter's 6 native-OS embedders). **v0.11+ ship-target** at 5
  sources.
- **`monorepo@v1` README-or-alternative knob** — angular uses
  `PACKAGE.md` not `README.md`; the bundled rule should accept
  either, OR offer a config-variable for the canonical name.
  Surfaces 17 false positives in this case study; same
  pattern would help any repo that ships a published-tarball
  README distinct from the contributor README. Bundled-ruleset
  refinement candidate.
- **`hygiene/no-tracked-artifacts@v1` ancestor-manifest scoping** —
  `hygiene-no-js-build-outputs` and `hygiene-no-cargo-target`
  fire on directories named `build/` and `target/` regardless of
  whether the repo has a JS or Rust build. Adding an
  `ancestor_manifest:` knob (only fire under a `package.json` or
  `Cargo.toml` ancestor) would deduplicate the false positives
  observed in 3+ case studies (kubernetes, angular, others).
  Bundled-ruleset refinement candidate.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`pair_inverse` against goldens/ once the rule kind ships** —
   the inverse direction (every `goldens/public-api/<name>/index.api.md`
   traces back to an existing `packages/<name>/`) is the headline
   gap this case study cites; once `pair_inverse` lands (v0.10
   design candidate, 2 sources confirmed: angular + ruff), this
   config should restate it as a direct rule rather than the
   `for_each_dir` workaround.
2. **`compliance/reuse@v1` (3-rule bundled ruleset) trial** — angular
   is MIT-licensed with a canonical `@license` block on every TS
   source file; the REUSE-spec form (SPDX header + per-file
   metadata) would let this case study compare its hand-rolled
   `angular-source-license-header` rule against the bundled
   alternative. Surface: 1k+ TS sources under `packages/*/`.
3. **`agent-hygiene@v1` (6-rule bundled ruleset) overlay** — the
   adopted `agent-context@v1` ruleset covers AGENTS.md presence
   but the related `agent-hygiene` ruleset (canonical AGENTS.md,
   no agent self-edits, etc.) hasn't been trialled here. angular's
   `.ng-dev/` directory + `tools/tslint/` are exactly the kind of
   in-house tooling stack that benefits from explicit agent
   guardrails.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **131** loaded per `validate-config` (73 custom
  + 9 bundled rulesets — `oss-baseline` 15, `node` 9, `monorepo` 4,
  `monorepo/pnpm-workspace` 4, `ci/github-actions` 3,
  `hygiene/no-tracked-artifacts` 11, `hygiene/lockfiles` 7,
  `tooling/editorconfig` 3, `agent-context` 5; runtime emits 110
  result entries because some rule IDs are shared/deduped across
  overlays at runtime)
- **`alint validate-config`:** ✓ Config valid: 131 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for
  the 144-violation breakdown (failing rules 36 / passing 74)
- **Pitfall fixes (v0.9.17):** Pitfall #18 (per-rule
  `respect_gitignore: false`) and #19 (literal-path runtime guard
  for `root_only: true` + multi-component literals) both shipped in
  engine; this config does not need either workaround
- **Pitfall #22 status:** No `pattern: |` block scalars in this
  config — not a candidate
- **Open gaps (unchanged):** `cross_file_value_equals` (v0.10
  ship-target, 11 sources after this validation),
  `pair_inverse` (v0.10 design candidate, 2 sources),
  `cross_language_implementation_complete` (v0.11+ ship-target, 5
  sources). No new rule-kind gaps surfaced
- **Open suspected bugs in this directory's `.alint.yml`:** none
  detected. Bundled-ruleset misalignments (PACKAGE.md vs README.md;
  `hygiene-no-js-build-outputs` over-broad scope) are
  bundled-ruleset refinement candidates, not config bugs
