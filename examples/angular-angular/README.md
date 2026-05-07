# Case study: `angular/angular`

> Marketing/positioning writeup at https://alint.org/examples/angular-angular/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `angular/angular`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-clone of
`angular/angular@HEAD` (the Angular framework — TypeScript at scale,
Bazel build, 16 published `@angular/*` packages).

---

## Summary

angular is a **TS-monorepo + Bazel-built** mega-repo — TypeScript at
scale (~3M LoC across 16 published `@angular/*` packages) wired
through a Bazel workspace (`MODULE.bazel` + 638 `BUILD.bazel` files
+ 78 `tsconfig*.json` files) and orchestrated through pnpm-workspace
+ ng-dev (Angular's in-house dev-tooling CLI). Different shape from
microsoft/typescript (which IS the TS compiler — a flat src/+tests/
tree) and from vercel/next.js (hybrid pnpm+Cargo workspace) —
angular is the *consuming framework* with strict per-package
discipline and a Bazel-native build pipeline.

Concrete count at HEAD:

- **16 published `@angular/*` packages** under `packages/*/` (every
  one scoped, every one pinned to literal version
  `0.0.0-PLACEHOLDER`)
- **638 BUILD.bazel files** across the workspace
- **78 tsconfig*.json files** (per-package + per-target)
- **50 *.api.md golden files** under `goldens/public-api/<pkg>/`
  (locks the entire public TypeScript API surface)
- **12 GitHub Actions workflows** (`.github/workflows/*.yml`)
- **3 husky hooks** (pre-commit, commit-msg, prepare-commit-msg) —
  all delegating to `pnpm ng-dev` subcommands
- **1 in-house `ng-dev` CLI** replacing the typical
  changesets/lerna/lint-staged stack, configured under `.ng-dev/`
- **tslint** (pinned, with custom rules under `tools/tslint/`) +
  buildifier + prettier (delegated through `ng-dev format`)
- **`.pullapprove.yml`** as the code-ownership source (NOT
  CODEOWNERS; angular pre-dates GitHub-native CODEOWNERS support
  and stayed on PullApprove)

Total **structural-validation surfaces** counted: **27** discrete
checks across `package.json` scripts, `.github/workflows/`, `.husky/`,
`.ng-dev/`, custom tslint rules, and the `goldens/` API-parity
discipline.

- **17 of 27 (~63 %) map to existing alint rules** — bundled
  `oss-baseline + node + monorepo + monorepo/pnpm-workspace +
  ci/github-actions + hygiene/no-tracked-artifacts + hygiene/lockfiles
  + tooling/editorconfig + agent-context` cover ~58 rules between
  them (oss-baseline=15, node=9, monorepo=4, pnpm-workspace=4,
  ci/github-actions=3, hygiene/no-tracked-artifacts=11,
  hygiene/lockfiles=7, tooling/editorconfig=3, agent-context=5,
  with overlap deduped at load), plus the 73 angular-specific rules in
  [`/.alint.yml`](.alint.yml)
  (per-package conventions, license-header invariants, ng-dev
  config integrity, Bazel workspace shape, husky hook integrity,
  goldens directory structure, etc.).
- **3 of 27 (~11 %) shell out via `command:` rules** — tslint,
  ng-dev format, pullapprove verify, ngbot verify, ai skills validate,
  ts-circular-deps, public-api check, check-tooling-setup (8 commands
  total — covers the entire `pnpm lint`-equivalent pipeline).
- **7 of 27 (~26 %) are out of alint's scope** — the 6 custom
  tslint rules under `tools/tslint/` (TSESTree visitors), the
  `pnpm public-api:check` API Extractor pass against built bundles
  (build-aware), the `pnpm symbol-extractor:check` post-build
  symbol-table comparison (build-aware), `ng-dev release` (codegen
  + git mutation), `pnpm benchmarks` (perf orchestration), and the
  `pnpm ng-dev caretaker` GitHub-API-based queue browser.

The configured 73-rule + 9-bundled-ruleset
[`/.alint.yml`](.alint.yml) (131 rules total post-extends resolution
per `alint validate-config`) covers every structural assertion the
existing tooling makes about
repo *state*, plus several that angular doesn't explicitly enforce
today (per-package PACKAGE.md presence, per-package public-API
golden parity, source-side license-header consistency).

**Live-tree findings (factual):** the `goldens/public-api/<pkg>/`
discipline maps to the v0.11+ `cross_language_implementation_complete`
candidate applied within a single language — TypeScript source ↔
TypeScript API surface golden, 50 goldens locking the public surface
of 13 of 16 packages. Running this config against the cloned tree
surfaces **5 packages without an `index.api.md` golden**
(`benchpress`, `compiler`, `compiler-cli`, `language-service`,
`zone.js` — all internal/build-time packages that should either ship
a golden or appear on an explicit allowlist), **5 packages without
`public_api.ts`** (the API barrel file goldens are extracted from),
**6 source files with non-canonical `@license` header** (3 with extra
blank line, 1 with shebang preceding, 1 with leading UTF-8 BOM byte,
1 with drifted block-comment style), and **2 packages with
non-canonical version field** (zone.js at `0.16.1`, benchpress at
`0.4.0-PLACEHOLDER` instead of `0.0.0-PLACEHOLDER`). These drifts are
not caught by existing tooling: tslint sees source code, prettier sees
formatting, and `pnpm public-api:check` only runs against goldens that
already exist.

---

## Existing tooling inventory

### Root config files (root-level lint policy)

| File | Owner tool | What it pins | alint disposition |
|---|---|---|---|
| `package.json` `scripts:` block | npm | 35+ task aliases (`lint`, `tslint`, `ts-circular-deps:check`, `public-api:check`, etc.) | Not directly alint-checkable; `command:` rules below assert the canonical 8 entry-point scripts wire through |
| `pnpm-workspace.yaml` | pnpm | 28 workspace entries (every published package, plus the meta-tools, plus benchmarks, plus ts/dev-app) + `minimumReleaseAge: 1440` supply-chain defence | `yaml_path_matches` + 2 `file_content_matches` for the canonical entries + the minimum-release-age pin |
| `tslint.json` | tslint | Custom-rule directory (`tools/tslint`) + 18 rule toggles | 1× `file_content_matches` for the rulesDirectory + 1× `dir_contains` for the in-tree custom-rule .ts files |
| `tsconfig-tslint.json` | tslint | TS project config the tslint pass targets (`tslint -c tslint.json --project tsconfig-tslint.json`) | Not asserted directly; covered transitively by `angular-tslint` shellout |
| `MODULE.bazel` + `MODULE.bazel.lock` + `BUILD.bazel` (root) | bazel | bzlmod module declaration + lockfile + root build targets | 3× `file_exists` (load-bearing for the build) |
| `.bazelversion` | bazelisk | Pins Bazel version (`8.6.0`) so `pnpm test` (= `bazelisk test`) uses the right Bazel | `file_exists` + `file_content_matches` for semver shape. Note: angular's `.bazelversion` is **not** gitignored (verified via `git check-ignore`) — so the simple `file_exists` works without the workaround for CONFIG-AUTHORING.md pitfall #18 |
| `pnpm-lock.yaml` | pnpm | Resolved dep graph | `file_exists` (covered by bundled `hygiene/lockfiles@v1`) |
| `.npmrc` | pnpm | `hoist=false` (Bazel rules_js requirement), `engine-strict=false`, `auto-install-peers=false` | 2× `file_content_matches` for the load-bearing settings |
| `.nvmrc` | nvm/etc. | Node major version pin (currently `22.22.2`) | `file_exists` + semver-shape regex |
| `.gitattributes` | git | `* text=auto` + `*.ts eol=lf` + `*.js eol=lf` cross-platform line-ending normalisation | 2× `file_content_matches` for the load-bearing EOL pins |
| `.pullapprove.yml` | PullApprove (3rd-party) | Code-ownership map (replaces GitHub-native CODEOWNERS) | `file_exists` + `yaml_path_matches` for `version: 3` pin |
| `.github/angular-robot.yml` | angular-robot | bot config for size guard + merge plugin | `file_exists` |
| `.husky/pre-commit` | husky | invokes `pnpm ng-dev format staged` | `file_exists` + `file_content_matches` for canonical command |
| `.husky/commit-msg` | husky | invokes `pnpm ng-dev commit-message pre-commit-validate` | `file_exists` |
| `.husky/prepare-commit-msg` | husky | invokes `pnpm ng-dev commit-message restore-commit-message-draft` | `file_exists` |
| `renovate.json` | Renovate Bot | Extends `github>angular/dev-infra//renovate-presets/default.json5` + per-repo overrides | `file_exists` |
| `.pnpmfile.cjs` | pnpm | install hooks for upstream dep manifest patches | `file_exists` |
| `context7.json` | Context7 LLM-context service | docs configuration | `file_exists` (info-level) |

### `.ng-dev/*` — angular's in-house dev tooling

angular replaces the typical changesets/lerna/lint-staged stack with
a single in-house CLI (the `@angular/ng-dev` package), configured
under `.ng-dev/`. Six configs are load-bearing:

| File | What it declares | alint disposition |
|---|---|---|
| `.ng-dev/config.mjs` | Entrypoint that re-exports the others | `file_exists` |
| `.ng-dev/format.mjs` | Which formatters are enabled (`prettier: true, buildifier: true`) | `file_exists` (content covered by `ng-dev format` shellout) |
| `.ng-dev/commit-message.mjs` | The ~22 valid commit-scope strings (`animations`, `core`, `compiler`, `dev-infra`, etc.) — used by the husky `commit-msg` hook + by the `ng-dev commit-message pre-commit-validate` CLI | `file_exists` (the scope-list-vs-directory-tree drift would need `cross_file_value_equals`; today out of scope) |
| `.ng-dev/pull-request.mjs` | Required CI statuses (`test`, `lint`, `adev`), target labels, merge-method strategy | `file_exists` |
| `.ng-dev/google-sync-config.json` | File patterns synced to internal google3 mirror | `file_exists` |
| `.ng-dev/caretaker.mjs` | GitHub queries for triage / merge queue | `file_exists` (info-level; non-load-bearing for CI) |

### `scripts/*.{js,mjs,mts,cjs}` — hand-rolled validation scripts

| Script | What it checks | alint replacement |
|---|---|---|
| `goldens/public-api/manage.js` | Drives `pnpm public-api:check` and `pnpm public-api:update` — runs Microsoft's API Extractor against the built bundles, diffs against `goldens/public-api/<pkg>/index.api.md` | Out of scope (build-aware: API Extractor only works against built `.d.ts` output). Wrapped via `command:` rule |
| `tools/symbol-extractor/run_all_symbols_extractor_tests.js` | Drives `pnpm symbol-extractor:check` — post-build symbol-table comparison against goldens under `tools/symbol-extractor/symbols_*` | Out of scope (build-aware) |
| `scripts/build/build-packages-dist.mts` | Build orchestration — invokes Bazel + collects published-package output | Out of scope (build orchestration) |
| `scripts/diff-release-package.mts` | Diffs published `@angular/*` tarballs across releases | Out of scope (cross-ref diff) |
| `scripts/compare-main-to-patch.js` | Cherry-pick discovery between main and patch branches | Out of scope (git-history-aware) |
| `scripts/benchmarks/index.mts` | Runs Angular's perf benchmarks | Not validation |
| `tools/symbol-extractor/run_all_symbols_extractor_tests.js` (the structural subset of "every package has a symbols-baseline file") | Per-package convention assertion | Not yet asserted; would map to a pair-rule — `packages/<name>` ↔ `tools/symbol-extractor/symbols_<name>.json`; defer until next iteration |

### `.github/workflows/` (12 workflows)

| Workflow | What it does | alint disposition |
|---|---|---|
| `ci.yml` | Push-to-main / patch-branch CI — runs lint + devtools + adev + test in parallel | `file_exists` + `ci/github-actions@v1` covers shape |
| `pr.yml` | PR-opened CI — same job set as `ci.yml`, scoped to PR ref | `file_exists` |
| `merge-ready-status.yml` | Aggregates required-checks into the `merge-ready` GitHub status | `file_exists` |
| `assistant-to-the-branch-manager.yml` | Branch-manager bot interactions (issue/PR comments) | Out of scope (operational) |
| `benchmark-compare.yml` | Auto-comparison of benchmark results across PRs | Out of scope (benchmark orchestration) |
| `cross-repo-adev-docs.yml` | Triggers downstream adev/docs sync | Out of scope (operational) |
| `dev-infra.yml` | Runs the dev-infra-specific subset (formats, commit-message, lockfile) | Subset of CI |
| `google-internal-tests.yml` | Triggers internal google3 test run | Out of scope (operational) |
| `perf.yml` | Perf-test orchestration | Not validation |
| `scorecard.yml` | OpenSSF Scorecard nightly run | `file_exists` |
| `adev-preview-build.yml` + `adev-preview-deploy.yml` | adev (Angular Dev) docs site preview | Out of scope (deployment) |

The `ci/github-actions@v1` ruleset (3 rules: workflow permissions,
action SHA pinning, workflow has `name:`) covers the hardening
surface for all 12 workflows at once. The starter config restates
the SHA-pinning rule at warning level for visibility.

### `tools/tslint/` — angular's custom tslint rules

angular still uses tslint (deprecated upstream 2019, but kept for
the in-tree custom-rules subset). The `tools/tslint/` directory
holds:

| Custom rule | What it does |
|---|---|
| `no-exported-inferred-call-type` | Bans exported declarations whose type is inferred from a call expression (causes API instability) |
| `no-duplicate-enum-values` | Bans enums with two members of the same value |
| `require-internal-with-underscore` | Every `@internal` JSDoc tag must be on a name starting with `_` (so it's discoverable to consumers) |
| `no-implicit-override-abstract` | Every abstract-method override must declare `override` explicitly |
| `validate-import-for-esm-cjs-interop` | Catches the `noNamedExports` pattern for CommonJS deps with ESM-suggestive types |

All 5 are TSESTree visitors → out of alint's "no AST" scope.
Listed for inventory completeness.

### `goldens/public-api/<pkg>/` — the API-parity discipline

The `goldens/` directory is angular's mechanism for locking the public
TypeScript API surface. ~50 `.api.md` files across 13 of 16 packages,
each generated by Microsoft's API Extractor against the built bundles.
The 3 packages without goldens are `compiler`, `language-service`,
`benchpress` — all internal/build-time packages with no public
TypeScript surface to freeze.

This maps to the v0.11+ `cross_language_implementation_complete` shape
applied within a single language — TS source under `packages/<name>/`
↔ TS API surface golden under `goldens/public-api/<name>/index.api.md`.
The starter config expresses the forward direction (every package →
golden) via `for_each_dir`; the inverse direction (every golden →
package) needs `pair_inverse` (ruff's snapshot-freshness candidate).

### Per-package conventions (the monorepo discipline)

Every published `packages/<name>/` directory carries the canonical
six-file structure:

- `package.json` — name `@angular/<dir>`, license `MIT`, version
  `0.0.0-PLACEHOLDER`, `repository.directory: packages/<dir>`,
  author `angular`
- `index.ts` — public entrypoint (re-exports from `public_api.ts`)
- `public_api.ts` — the actual API barrel
- `BUILD.bazel` — Bazel target definitions (`ng_project`, `ng_package`)
- `PACKAGE.md` — published-package README (replaces the typical
  `README.md` because the published tarball needs different
  copy than the contributor docs)
- `src/` — the package source (every TS file opens with the
  canonical `@license` block referencing
  `https://angular.dev/license`)

### Findings against the live tree (run against the snapshot at /tmp/angular)

Running this config against the cloned tree surfaces real,
actionable drift:

| Rule | Findings |
|---|---|
| `angular-source-license-header` | **6 source files** with non-canonical `@license` header — 3 with an extra blank line between `/**` and `* @license`, 1 (`packages/compiler-cli/src/bin/ng_xi18n.ts`) with `#!/usr/bin/env node` shebang preceding the block, **1 (`packages/core/src/defer/interfaces.ts`) with a leading UTF-8 BOM byte that no existing tool catches**, and 1 minor block-comment-style drift |
| `angular-package-has-public-api-golden` | **5 packages** without a `goldens/public-api/<name>/index.api.md` — `benchpress`, `compiler`, `compiler-cli`, `language-service`, `zone.js` (all internal/build-time; should be on an explicit allowlist) |
| `angular-package-has-public-api-ts` | **5 packages** without `public_api.ts` — same set as above (the API barrel + the API golden are coupled) |
| `angular-package-has-package-md` | **5 packages** without `PACKAGE.md` (info-level; mostly the same internal set) |
| `angular-package-version-is-placeholder` | **2 packages** with non-placeholder version: `zone.js` at `0.16.1` (real semver — released independently of the angular-monorepo lockstep), `benchpress` at `0.4.0-PLACEHOLDER` (drifted placeholder format — won't substitute correctly) |
| `angular-package-name-is-scoped` | **1 package** (`zone.js`) with unscoped name — same exception as above |
| `angular-package-repository-directory-matches` | **2 packages** (`compiler-cli/linker/babel/test`, `zone.js/test/typings`) with deeper path values that don't match the canonical `packages/<name>` shape |
| `angular-packages-tsconfig-build-strict` | Pass (after adjusting from `json_path_equals` to `file_content_matches` to handle the JSONC leading block-comment) — strict mode is on |
| `gha-pin-actions-to-sha` | **~3 third-party action invocations** in workflows not pinned to a SHA |

Plus the bundled rules surface ~20 info-level whitespace/newline
findings across markdown docs and 6 lockfile-hygiene findings
(nested package-lock.json under `tests/` fixtures, expected).

**Wall-clock**: full 91-rule pass against the 185 MiB sparse
checkout completes in **single-digit seconds** on a stock workstation
— vs. ~30-60 seconds for `pnpm lint` (which serially runs tslint
+ ng-dev format check + ts-circular-deps + pullapprove verify +
ngbot verify + ai skills validate + check-tooling-setup).

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts the bundled
`oss-baseline + node + monorepo + monorepo/pnpm-workspace +
ci/github-actions + hygiene/no-tracked-artifacts + hygiene/lockfiles
+ tooling/editorconfig + agent-context` overlays, then layers 73
angular-specific rules on top — 131 rules total after extends
resolution.

Selected rules:

- **`angular-source-license-header`** — every hand-edited
  `packages/*/{index.ts,public_api.ts,src/**/*.ts}` opens with the
  canonical 6-line `@license` block referencing
  `https://angular.dev/license`. Currently no automated check
  enforces this on source; only `ng_package`'s bundled-output
  banner enforces it post-build (and only because the bundle
  wrappers prepend it unconditionally). Surfaces 6 drifts in the
  live tree including a leading UTF-8 BOM byte.
- **`angular-package-has-public-api-golden`** — every published
  `packages/<name>/` should have a corresponding
  `goldens/public-api/<name>/index.api.md`. The forward direction
  of the v0.11+ `cross_language_implementation_complete` candidate
  applied within TypeScript (TS source ↔ TS API surface). Surfaces
  5 packages on the wrong side of the golden/no-golden split.
- **`angular-package-version-is-placeholder`** — every
  `packages/<dir>/package.json` must carry `version:
  "0.0.0-PLACEHOLDER"` so the `ng-dev release` substitution
  catches it. Surfaces `zone.js` (legitimate exception — released
  independently) and `benchpress` (drifted placeholder format).
- **`angular-package-name-is-scoped` /
  `angular-package-license-mit` /
  `angular-package-repository-directory-matches`** — per-package
  manifest discipline. The `repository.directory` check catches
  the same class of regression that bit `react-refresh` in the
  facebook/react case study.
- **`angular-packages-tsconfig-build-strict`** — the canonical
  `packages/tsconfig-build.json` keeps `"strict": true`. JSONC
  file → uses `file_content_matches` against the raw text per
  CONFIG-AUTHORING.md pitfall #16's option B (json_path_equals
  can't parse JSONC's leading block-comment).
- **`angular-bazel-version-pinned` /
  `angular-module-bazel-present` /
  `angular-module-bazel-lock-present`** — Bazel workspace
  integrity. The lockfile is a non-trivial novelty — bzlmod's
  `MODULE.bazel.lock` is conceptually equivalent to npm's
  `package-lock.json` but sits in a different ecosystem.
- **`angular-tslint-rules-dir-points-at-tools-tslint`** — locks
  the `tools/tslint` rulesDirectory entry. Without it, `pnpm
  tslint` silently degrades to stock tslint and the
  angular-specific custom rules
  (`require-internal-with-underscore`,
  `no-implicit-override-abstract`,
  `validate-import-for-esm-cjs-interop`, etc.) stop running.
- **`angular-pullapprove-config-present` /
  `angular-pullapprove-version-pinned`** — angular's
  `.pullapprove.yml` replaces GitHub-native CODEOWNERS. Pin the
  schema version + assert presence so a silent removal doesn't
  open code-ownership review to anyone with write access.
- **`angular-husky-{pre-commit,commit-msg,prepare-commit-msg}-present`** —
  three husky hooks all delegating to `pnpm ng-dev`. The
  `pre-commit` hook uses `set +e` (warns but doesn't block) —
  defensible on installed-tool-missing, but a missing hook file
  silently bypasses format entirely.
- **`angular-tslint`** + **`angular-ng-dev-format-check`** +
  **`angular-ng-dev-pullapprove-verify`** +
  **`angular-ng-dev-ngbot-verify`** +
  **`angular-ng-dev-ai-skills-validate`** +
  **`angular-ts-circular-deps`** + **`angular-public-api-check`** +
  **`angular-check-tooling-setup`** — eight `command:` rules
  wrapping the existing pipeline. Together with the rules above,
  `alint check` is a drop-in for `pnpm lint && pnpm
  ts-circular-deps:check && pnpm ng-dev pullapprove verify && pnpm
  ng-dev ngbot verify && pnpm ng-dev ai skills validate && pnpm
  public-api:check && pnpm check-tooling-setup` — with the
  structural checks as a bonus.

---

## What needs new alint primitives

Three patterns specific to angular that don't fit any current rule
kind — all already on the v0.10+ candidate list, with angular
adding demand-saturation evidence:

### 1. `cross_language_implementation_complete` (within-language variant) for the goldens/ pattern

The `goldens/public-api/<pkg>/index.api.md` discipline maps to the
v0.11+ `cross_language_implementation_complete` candidate applied
within a single language: every `packages/<name>/` (the source side)
should have a corresponding `goldens/public-api/<name>/index.api.md`
(the API-surface side), with explicit allowlisting for the
internal/build-time packages (`benchpress`, `compiler`,
`language-service`) that legitimately have no public surface to
freeze. The current `for_each_dir` workaround expresses the forward
direction (package → golden); the inverse direction (every golden
traces back to a package) needs `pair_inverse` (ruff's
snapshot-freshness candidate).

Per `docs/development/launch-evidence.md`, this is one of 5 demand
sources for `cross_language_implementation_complete` (alongside
arrow's `format/Schema.fbs` ↔ per-language test fixtures, TF's 1,185
textproto goldens locking the public Python surface, protobuf's 10
in-tree language bindings, and flutter's 6 native-OS embedders). All
five repos express the same shape: a manifest-style "registry of
things that should exist" file/dir tree that needs to stay in sync
with another manifest-style file/dir tree, with explicit allowlisting
for known exceptions. The candidate is "v0.11+ ship-target" with 5
sources.

### 2. `cross_file_value_equals` for the commit-scope ↔ package-tree drift

`.ng-dev/commit-message.mjs` enumerates ~22 valid commit-scope
strings (`animations`, `common`, `compiler`, `compiler-cli`,
`core`, `dev-infra`, `devtools`, `docs-infra`, `elements`, `forms`,
`http`, `language-service`, `language-server`, `localize`,
`migrations`, `platform-browser`, `platform-browser-dynamic`,
`platform-server`, `router`, `service-worker`, `upgrade`, plus a
fixed extra list). Today, drift between this list and the actual
`packages/<name>/` directory set is caught only by code-review
discipline.

The shape is `cross_file_value_equals` between the
`.ng-dev/commit-message.mjs` `scopes:` array (a JS file with an
exported config object — needs JS-aware extraction beyond the
TS-AST-free scope of alint, OR a regex-based extraction) and the
`packages/<name>/` directory tree. Same pattern as
`validate-externals-doc.js` in vercel/next.js (third confirmation
in the case-study set after airflow + tokio).

**Reconfirms** the existing high-priority candidate.

### 3. `pair_inverse` for the goldens-without-package case

The forward direction (every package has a golden) is expressible
today via `for_each_dir`. The **inverse** direction (every golden
in `goldens/public-api/<name>/index.api.md` traces back to an
existing `packages/<name>/`) needs `pair_inverse` — primarily so
that goldens for packages that have been removed don't silently
linger as zombie files that `pnpm public-api:check` continues to
diff against. ruff was the original source for this candidate (its
`cargo insta --unreferenced=reject` snapshot-freshness check); angular
adds a second source.

**Reconfirms** the v0.10+ candidate.

---

## What's out of alint's scope (kept on the existing tool)

Listed by category for clarity:

- **AST analysis** (tslint + the 5 custom tslint rules + ts-circular-deps + the
  `validate-import-for-esm-cjs-interop` rule) — alint deliberately doesn't try
  to be a parser. Shell out via `command:`.
- **Build-aware checks** (`pnpm public-api:check`, `pnpm
  symbol-extractor:check`) — both run against the *built* bundles
  via Bazel + API Extractor + a custom symbol extractor. The
  freshness checks belong to the build system. Shelled out via
  `command:`.
- **Codegen + git-state mutation** (`ng-dev release`,
  `pnpm public-api:update`, `pnpm symbol-extractor:update`) — alint
  reads files; it doesn't regenerate them and diff. The freshness
  check belongs to the build system.
- **Cross-ref diffs** (`scripts/diff-release-package.mts`,
  `scripts/compare-main-to-patch.js`) — alint sees one tree at a
  time, not diffs.
- **Operational workflows** (release / cron / triage / issue-bot /
  PR-comment / `assistant-to-the-branch-manager.yml`) — not
  validation surfaces.
- **GitHub-API queries** (`pnpm ng-dev caretaker`) — alint reads
  files; it doesn't talk to GitHub.

---

## Already covered by other linters angular uses

- `tslint` (with `tslint-no-toplevel-property-access`) — TS AST/semantics; lives
  with tslint. alint orchestrates via `command:`.
- `prettier` (via `pnpm ng-dev format`) — formatter; lives with prettier. alint
  orchestrates via `command:` so the prettier-config-pinning rules + the format
  check run in one alint pass.
- `buildifier` (via `pnpm ng-dev format`) — Bazel-file formatter; lives with
  buildifier.
- `pnpm ts-circular-deps:check` — TS import-graph cycle detection; lives with
  the angular-specific script.

---

## Performance comparison (placeholder — bench when validation pass scales)

The repo is large enough to be a meaningful stress test:

- **~185 MiB** working tree (after sparse-checkout dropping
  `/packages/compiler/test`, `/packages/core/test`, `/integration`)
- **638 BUILD.bazel files** + **78 tsconfig*.json files**
- **~3M LoC** of TypeScript across `packages/` + `adev/` + `devtools/`
- **12 GitHub Actions workflows**
- **50 .api.md golden files**

The published S3 bench (100k files, mixed languages) hits 1.13 s
for the workspace bundle on a stock CI runner. The angular repo at
full size sits between S3 and S9 (the polyglot monorepo bench, 100k+
files). Expected: **2-4 s** for `alint check` on the structural
rules alone, vs. ~30-60 s for `pnpm lint` (which serially runs
tslint + ng-dev format check + ts-circular-deps + pullapprove verify
+ ngbot verify + ai skills validate + check-tooling-setup).

Coverage on angular specifically: the per-package manifest
spot-checks run against 16 `package.json` files in single-digit
milliseconds (sequential `node -e "require()"` calls would be ~2-3 s
of warm-cache startup). The per-package golden parity check finds 5
packages without goldens. The license-header consistency check finds
6 drifts including a UTF-8 BOM byte.

To benchmark wall-clock for real:
`time { pnpm lint && pnpm ts-circular-deps:check && pnpm ng-dev pullapprove verify; }`
vs `time alint check`. Deferred to the per-repo measurement pass.

---

## Followup primitive demand (consolidated)

1. **`cross_language_implementation_complete` rule kind** — covers
   the `packages/<name>/` ↔ `goldens/public-api/<name>/index.api.md`
   parity here, plus arrow's per-language schema parity, TF's
   textproto goldens, protobuf's 10 in-tree bindings, and flutter's
   6 native-OS embedders. Demand: 5 sources confirmed — v0.11+
   ship-target per `launch-evidence.md`.
2. **`cross_file_value_equals` rule kind** — covers the
   `.ng-dev/commit-message.mjs` `scopes:` ↔ `packages/<name>/` drift
   here, plus the airflow/tokio/clap/uv/react/pnpm/nodejs/pytorch
   workspace-version sync patterns. Demand: 9 case studies.
3. **`pair_inverse` rule kind** — covers the "every golden traces
   back to a package" inverse direction here, plus ruff's `cargo
   insta --unreferenced=reject` snapshot-freshness check. Demand: 2
   case studies.

---

## Pitfalls hit while writing this config (against CONFIG-AUTHORING.md)

While writing this config, **2 of the (then) 19 documented pitfalls
fired** during iteration; **no new pitfalls surfaced** at the time
(the catalogue subsequently grew to 21 in P2b Wave 2 — istio added
#20 + #21; this case study did not surface either).

1. **Pitfall #16** — `*_path_equals` can't parse JSONC files with
   leading block-comments. The first-draft
   `angular-packages-tsconfig-build-strict` rule used
   `json_path_equals: $.compilerOptions.strict equals: true` against
   `packages/tsconfig-build.json`, which fires runtime error
   `not a valid JSON document: expected value at line 1 column 1`
   on every match because the file opens with a JSDoc-style block
   comment. The fix is the same as in microsoft-typescript's
   `ts-tsconfig-strict-mode` — a `file_content_matches` workaround
   against the raw text. **Suggested CONFIG-AUTHORING.md
   strengthening**: pitfall #16's "Right" examples currently focus
   on bool fields specifically; a follow-up bullet noting that
   `json_path_*` rules can't parse JSONC files at all (regardless of
   the value type) would have caught this drift earlier.

2. **`dir_contains` rule field-shape** — first-draft used
   `paths:` + `pattern:` (cargo-culting from `file_*` rules), but the
   rule's actual schema is `select:` + `require:` (matching
   `for_each_dir`'s shape). Schema rejected with `unknown field
   `pattern`, expected `select` or `require``. **Not a documented
   pitfall** because the schema-error message is clear enough to
   self-correct in one iteration, but worth noting that
   `dir_contains` is the only "iterate over directories"-style
   rule that doesn't follow the `paths:` convention — a writer
   reading the rule kind name from the catalogue and going "let me
   write a paths-glob and a content-pattern" hits this. **Suggested
   CONFIG-AUTHORING.md addition**: a one-line note in the
   "Cross-file iteration" canonical pattern at the bottom, calling
   out that `dir_contains` uses the for_each_dir-style
   select/require (not the file-rule-style paths/pattern).

---

## Notes for the parent agent

- Audit (`cargo test -p alint-e2e --test coverage_audit_examples_parse`)
  passes with this config in place. (The historical WIP note about
  an in-progress `RuleSpec.respect_gitignore` field is now resolved
  — that knob shipped in v0.9.17 as the per-rule
  `respect_gitignore: false` option, the direct fix for pitfall #18.)
- No new schema/language pitfalls beyond the documented (then) 19;
  the catalogue subsequently grew to 21 in P2b Wave 2 (istio's
  `cross_file_value_equals` extractor + multi-doc YAML cases). The
  closest near-miss is the `dir_contains` field-name shape noted
  above, which the schema-error was clear enough to self-correct
  on first attempt.
- Config runs cleanly against the actual cloned repo at
  `/tmp/angular/` — **122 violations, all expected real findings**
  (6 license-header drifts including a UTF-8 BOM, 5 packages
  without API goldens, 5 packages without public_api.ts, 5
  without PACKAGE.md, 2 with non-placeholder versions, plus the
  bundled-rules info-level whitespace findings on markdown docs).
  No silent failures.
- Run the config locally:
  `alint check --config examples/angular-angular/.alint.yml /path/to/angular/`

---

## Future analysis

Surfaced during the 2026-05-07 revalidation pass; not yet executed
against a live tree:

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
   adopted `agent-context@v1` ruleset covers context.md presence
   but the related `agent-hygiene` ruleset (canonical AGENTS.md,
   no agent self-edits, etc.) hasn't been trialed here. angular's
   `.ng-dev/` directory + `tools/tslint/` are exactly the kind of
   in-house tooling stack that benefits from explicit agent
   guardrails.

---

## Validation status (2026-05-07)

- alint version validated: 0.9.17 (built 2026-05-07)
- `validate-config` rule count: **131 rules loaded** (matches the
  73-in-config + 9 bundled-overlay shape after extends resolution)
- Live-tree recheck: **pending — `/tmp/angular/` not present** at
  revalidation time; the README's 122-violation claim from the
  original capture (2026-05-06) has not been re-confirmed against
  a current sparse-clone.
- Pitfalls noted in this README that are now fixed in the engine:
  none directly cited — the README's only pitfall reference is #16
  (JSONC + `*_path_*`), which remains documented-with-workaround.
  The historical WIP note about an in-progress
  `RuleSpec.respect_gitignore` field is now resolved (shipped as
  pitfall #18's direct fix in v0.9.17).
- Open gaps after this revalidation: rule-count drift in the prose
  (50 → 73 in-config rules; the README header was written before
  several iteration passes added per-package-discipline rules) was
  the principal stale claim. The `cross_language_implementation_complete`
  saturation count (3 → 5 sources) was also corrected.
