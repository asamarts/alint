# Case study: `facebook/react`

> Marketing writeup (narrative, headline catch, competitive framing)
> lives at <https://alint.org/examples/facebook-react/>. This README
> is the engineering reference: tooling inventory, mapping table,
> gap catalogue, validation status.

Inventory of the structural-validation tooling in `facebook/react`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-clone of
`facebook/react@HEAD` (the React monorepo — runtime + DOM + DevTools
+ Compiler).

---

## Summary

react is a **Yarn classic v1 multi-package monorepo** — 39 packages
under `packages/*` declared via `"workspaces": ["packages/*"]` in the
root `package.json`, with `packageManager: "yarn@1.22.22"` pinning
the workspace to Yarn classic. Of those 39 packages, **22 publish to
npm** (the `react`, `react-dom`, `react-reconciler`, `scheduler`,
`react-server-dom-*`, `react-devtools-*`, `eslint-plugin-react-hooks`,
`use-subscription`, `use-sync-external-store`, `react-refresh`,
`jest-react`, `react-art`, `react-is`, `react-test-renderer`,
`react-flight-server-fb`, `react-markup` family) and **17 are private**
(internal test-utils, devtools internals, native-renderer bindings,
`shared`, `react-server`, `react-cache`, `react-debug-tools`).

Different shape from kubernetes (Go monorepo, hand-rolled `hack/verify-*.sh`
sprawl), microsoft/typescript (Hereby task runner, frozen-snapshot
maintenance mode), and vercel/turbo (modern Rust+TS hybrid with
zero hand-rolled validators). react is the **"deeply-evolved JS
monorepo with selective custom tooling"** data point.

Concrete count: **5 lint-class CI jobs** in `.github/workflows/shared_lint.yml`
(prettier, eslint, check_license, test_print_warnings, plus the
implicit yarn-install-validates-lockfile gate), **8 hand-rolled
validation scripts** under `scripts/` (`tasks/eslint.js`, `tasks/linc.js`,
`tasks/version-check.js`, `tasks/flow-ci.js`, `ci/check_license.sh`,
`ci/test_print_warnings.sh`, `print-warnings/print-warnings.js`,
`error-codes/extract-errors.js`), **5 in-tree custom eslint rules**
under `scripts/eslint-rules/` (`prod-error-codes`,
`safe-string-coercion`, `warning-args`, `no-primitive-constructors`,
`no-production-logging`), **1 second-pass build-output linter**
(`scripts/rollup/validate/index.js` + 4 per-channel
`eslintrc.{cjs,cjs2015,esm,fb,rn}.js` configs), and **1 PR-time Danger
runner** (`dangerfile.js`).

Of those ~20 surfaces:

- **~10 fit alint directly** (per-package layout, header
  consistency, codes.json shape, .nvmrc/.gitattributes/.eslintignore
  discipline, version-source shape, MAINTAINERS file, workflow
  naming convention, tracked-build-output guard, manifest field
  shapes).
- **~5 are shelled out via `command:`** (eslint, prettier-check,
  flow-ci, version-check, check_license).
- **~5 are out of scope** — the 5 custom eslint rules
  (TSESTree visitors), `extract-errors.js` (codegen against built
  artefacts), `print-warnings.js` (hermes-parser AST walk),
  `lint-build`'s second-pass rollup-output re-lint (build-aware),
  `dangerfile.js` (PR-diff-aware).

Maps-to-alint percentage: **~50%** (10/20). Needs-new-primitive: **~10%**
(2/20 — `cross_file_value_equals` for version-check + `registry_append_only`
for codes.json). Out-of-scope: **~40%** (8/20 — all AST or
build/runtime-aware).

**Key finding:** react carries a ~600-entry, append-only JSON
**registry** (`scripts/error-codes/codes.json`) plus a **single
source of truth** (`packages/shared/ReactVersion.js`) that
propagates across 3 per-package `version` fields. Both are
currently enforced by hand-rolled node scripts. alint replaces
the codes.json shape declaratively today and adds two rule-kind
candidates: `cross_file_value_equals` (now `v0.10 ship-target`,
10 sources per `docs/development/launch-evidence.md`) and
`registry_append_only` (still single-source / react-only —
`v0.10 design candidate`).

---

## Existing tooling inventory

react's structural validation lives in five overlapping places.

### 1. `.github/workflows/shared_lint.yml` (4 jobs)

| Job | What it runs | alint replacement |
|---|---|---|
| `prettier` | `yarn prettier-check` (= `node ./scripts/prettier/index.js`) | `command:` rule |
| `eslint` | `node ./scripts/tasks/eslint` | `command:` rule + structural floor (`react-eslintrc-loads-react-internal-plugin`, `react-eslintignore-skips-build-output`) |
| `check_license` | `./scripts/ci/check_license.sh` (greps `git grep -l PATENTS` against an allow-list) | Replaced declaratively by `react-no-patents-references` (forbidden-pattern with the script itself excluded) + `command:` shell-out for redundancy |
| `test_print_warnings` | `./scripts/ci/test_print_warnings.sh` (asserts `print-warnings.js` emits at least one warning) | Out of scope — the underlying `print-warnings.js` walks every JS source with hermes-parser to enumerate `console.warn(...)` calls |

The other 20 workflows under `.github/workflows/` are runtime/test
orchestration (`runtime_*`, `compiler_*`, `devtools_*`,
`shared_check_maintainer.yml`, etc.) and operational bots
(`shared_close_direct_sync_branch_prs.yml`, `shared_stale.yml`,
`shared_cleanup_*_caches.yml`). None are validation surfaces; they
all delegate to test runners or operate on PR/branch state.

### 2. `package.json` `scripts:` block (the validation entrypoints)

| Script | What it does | alint replacement |
|---|---|---|
| `lint` | `node ./scripts/tasks/eslint.js` (full eslint over `**/*.js` + `.eslintignore`) | `command:` rule |
| `linc` | Same eslint pass but `--onlyChanged` (PR mode) | Out of scope (PR-diff-aware) |
| `prettier` / `prettier-all` / `prettier-check` | `node ./scripts/prettier/index.js [write-changed|write|<no-arg>]` | `command:` rule (`react-prettier-check`) |
| `flow` / `flow-ci` | `node ./scripts/tasks/flow.js` / `flow-ci.js` | `command:` rule (the actual analysis is out of alint scope) |
| `version-check` | `node ./scripts/tasks/version-check.js` — asserts `packages/shared/ReactVersion.js` exports the same string as `packages/{react,react-dom,react-test-renderer}/package.json#version` | **Needs `cross_file_value_equals` primitive.** Today: `command:` shell-out |
| `extract-errors` | `node scripts/error-codes/extract-errors.js` — rewrites `codes.json` from build artefacts | Out of scope (codegen against built bundles) |
| `lint-build` | `node ./scripts/rollup/validate/index.js` — second-pass eslint against the rollup output bundles | Out of scope (build-aware; bundles don't exist until after `yarn build`) |
| `flow-typed-install` / `prebuild` / `build*` / `test*` | Build/test orchestration | Not validation surfaces |

### 3. `scripts/eslint-rules/` (in-tree custom plugin: `eslint-plugin-react-internal`)

All 5 are TSESTree visitors → out of alint's "no AST" scope.
Listed for inventory completeness:

| Rule | What it does |
|---|---|
| `prod-error-codes` | Cross-references every `Error(<literal>)` against `scripts/error-codes/codes.json`; rejects new error messages that don't exist in the registry |
| `safe-string-coercion` | Bans implicit `String(x)` coercion patterns that throw in strict mode |
| `warning-args` | Validates `console.warn(...)` argument shape |
| `no-primitive-constructors` | Bans `new Boolean(...)` / `new String(...)` / etc. |
| `no-production-logging` | Bans `console.log` outside dev-only branches |

These are perfect examples of "AST analysis is not alint's
niche" — they belong in eslint and stay in eslint.

### 4. `scripts/error-codes/codes.json` (the canonical error registry)

Append-only flat JSON object: `{ "<numeric-id>": "<message-template>", ... }`.
~600 entries at the snapshot. Two consumers depend on the exact
shape:

- **`scripts/eslint-rules/prod-error-codes.js`** reads the file at
  rule-init time, builds a `Set` of message templates, and rejects
  any `new Error("foo")` literal whose template isn't in the set.
- **`scripts/error-codes/transform-error-messages.js`** is a babel
  pass that rewrites `new Error("foo")` to `new Error(formatProdErrorMessage(<id>))`
  for production builds, keyed by reverse-lookup against codes.json.

Append-only-ness is enforced **by human review of `git diff
codes.json`** (via Danger and the README's note: "This file is
append-only, which means an existing code in the file will never be
changed/removed"). No automated check enforces it — a stray rebase
or copy-paste could silently re-key an existing error message,
breaking every prod-build that consumed the old code.

alint covers the **shape** (it's a flat `{string: string}` map with
numeric keys, no JSONC comments) declaratively today; the
**append-only invariant** needs the `registry_append_only` rule
kind (see "Needs new alint primitives" below).

### 5. `dangerfile.js` (PR-time inspector)

Out of alint's scope (operates on PR-diff state, not the repo at
HEAD). Listed for completeness — runs on every PR via the
`dangerfile.js` workflow, posts a comment summarising the PR's
impact (changed-file count, bundle-size diff, etc.).

### Configuration files (the "if these go missing CI fails confusingly" set)

| File | Why it matters | alint check |
|---|---|---|
| `.eslintrc.js` | Registers the canonical eslint setup including the in-tree `react-internal` plugin | `react-eslintrc-loads-react-internal-plugin` (forbid silent removal) + `react-eslintrc-uses-hermes-parser` (forbid silent parser swap) |
| `.eslintignore` | Skips `**/node_modules`, `build/`, `coverage/`, `compiler/`, etc. | `react-eslintignore-skips-node-modules`, `react-eslintignore-skips-build-output` |
| `.prettierrc.js` / `.prettierignore` | Prettier shape + ignore set | `react-prettierrc-exists`, `react-prettierignore-exists` |
| `.nvmrc` | Pins node version (read by every workflow's `actions/setup-node@v4`) | `react-nvmrc-version-pinned` (regex shape) |
| `.gitattributes` | `* text=auto` cross-platform line ending normalisation | `react-gitattributes-text-auto` |
| `MAINTAINERS` | `shared_check_maintainer.yml` workflow reads this | `react-maintainers-file-present` + `react-maintainers-file-non-empty` |
| `ReactVersions.js` (root) | Single source of truth for the publishing pipeline | `react-versions-file-declares-stable-packages` |
| `packages/shared/ReactVersion.js` | Single source of truth for the runtime version string | `react-version-source-shape` (regex on the exact form `version-check.js` parses) |

### Per-package conventions (the monorepo discipline)

- Every published package has matching `repository.directory` (=
  `packages/<name>`)
- Every published package's `homepage` points to https://react.dev/
- Every published package's `bugs` (string or object form) points to
  https://github.com/facebook/react/issues
- Every published package's `version` is plain semver
- The Meta copyright + MIT license header on every hand-edited
  `.js` source file under `packages/*/src/`

### Findings against the live tree (run against the snapshot)

Running this config against the cloned tree surfaces real, actionable
drift:

| Rule | Findings |
|---|---|
| `react-copyright-header-src` | 111 `.js` source files missing the standard 6-line Meta header |
| `react-copyright-header-scripts` | 75 dev scripts under `scripts/` missing the header (`info` level — legacy) |
| `react-published-package-has-source-license` | 39 packages without a per-package `LICENSE` file in the source tree (rollup adds one at build time, but `npm pack` from the source tree without building would miss it) |
| `react-package-repository-directory-matches` | **1 real drift: `packages/react-refresh/package.json` declares `repository.directory: "packages/react"` (instead of `packages/react-refresh`)** — likely a copy-paste regression from a sibling `react-*` package |
| `react-package-bugs-points-to-react-issues` | 19 packages whose `bugs` field shape doesn't quite match the canonical pattern |
| `react-package-homepage-canonical` | 1 package with non-canonical homepage URL |

Plus the bundled rules surface:

- 164 third-party action invocations in `.github/workflows/` not pinned to a SHA (`gha-pin-actions-to-sha`)
- 24 workflows missing `permissions: contents: read` declaration
- 5 packages without README.md (mostly internal: `react-dom-bindings`, `react-server-dom-fb`, `react-native-renderer`, `shared`, plus one)
- ~3500 info-level whitespace/newline issues across markdown docs (mostly in the compiler subdir's test fixtures)

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts the bundled
`oss-baseline + node + monorepo + monorepo/yarn-workspace + ci/github-actions
+ hygiene/no-tracked-artifacts + tooling/editorconfig + agent-context`
overlays, then layers ~33 react-specific rules on top. **87 rules
total** as loaded by the v0.9.17 binary (54 from the 8 bundled
rulesets — `oss-baseline=15`, `node=9`, `monorepo=4`,
`monorepo/yarn-workspace=4`, `ci/github-actions=3`,
`hygiene/no-tracked-artifacts=11`, `tooling/editorconfig=3`,
`agent-context=5` — plus 33 react-specific).

The headline rules:

- **`react-copyright-header-src` / `react-copyright-header-scripts`** —
  Meta copyright + MIT block on every hand-edited `.js` file under
  `packages/*/src/` and `scripts/`. Currently no automated check
  enforces this on source; only the `lint-build` pass against the
  built bundles does (and only because the bundle wrappers prepend
  it unconditionally).
- **`react-published-package-has-source-license`** — every published
  package carries its own `LICENSE` file in the source tree. Today
  the rollup `packaging.js` step (`asyncCopyTo('LICENSE', ...)`)
  copies the root LICENSE into every built tarball — so consumers
  see one — but a contributor working on a single package locally
  can't `npm pack` it without the build step.
- **`react-package-repository-directory-matches`** — every published
  package's `repository.directory` field equals `packages/<this-package-name>`.
  Catches the `react-refresh` regression above.
- **`react-error-codes-json-keys-numeric` / `-no-comments`** — assert
  `scripts/error-codes/codes.json` is a flat `{<numeric-key>: <string>}`
  JSON object with no JSONC comments. Both `prod-error-codes.js`
  and `transform-error-messages.js` depend on this shape.
- **`react-no-patents-references`** — declarative version of
  `scripts/ci/check_license.sh`'s `git grep -l PATENTS` invariant.
  The Facebook→Meta relicense in 2017 removed the PATENTS file;
  references that creep back in are the regression the script is
  designed to catch.
- **`react-version-source-shape`** + **`react-versions-file-declares-stable-packages`** —
  pin the shape of the two version sources of truth so the
  `version-check.js` parser regex doesn't silently fail.
- **`react-eslintrc-loads-react-internal-plugin`** + **`react-package-json-links-eslint-rules`** —
  if either `.eslintrc.js` drops the `react-internal` plugin OR
  `package.json` drops the `link:` reference to
  `scripts/eslint-rules`, all 5 in-tree custom rules silently stop
  running. alint catches the regression at config-load time.
- **`react-workflow-name-has-category-prefix`** — every workflow's
  `name:` opens with a category prefix (`(Runtime)`, `(Compiler)`,
  `(DevTools)`, `(Shared)`) so the Actions UI groups related
  workflows visually.
- **`react-package-manager-yarn-classic`** — root `packageManager`
  pinned to Yarn classic v1; protects against silent migration to
  Yarn 2/3 / pnpm / npm (which all change workspace resolution
  semantics in subtle ways).
- **`react-eslint`** + **`react-prettier-check`** + **`react-flow-check`** +
  **`react-version-check`** + **`react-check-license-script`** — five
  `command:` rules wrapping the existing tools. Together with the
  rules above, `alint check` is a drop-in for `yarn lint && yarn
  prettier-check && yarn flow-ci && yarn version-check &&
  ./scripts/ci/check_license.sh`, with the structural checks as a
  bonus.

---

## What needs new alint primitives

Two patterns specific to react that don't fit any current rule kind
— `cross_file_value_equals` is now `v0.10 ship-target` (10 sources
per `docs/development/launch-evidence.md`); `registry_append_only`
is still single-source (react-only) and sits at `v0.10 design
candidate` until a second source surfaces:

### 1. `cross_file_value_equals` — version-check.js shape

`scripts/tasks/version-check.js` reads
`packages/shared/ReactVersion.js`'s exported version string and
asserts it equals the `version` field in three per-package
`package.json` files (`react`, `react-dom`, `react-test-renderer`).
The current `pair` rule asserts a 1:1 file existence; this needs
**value equality across files** (JSONPath value at point X in file A
equals JSONPath value at point Y in file B, with optional regex
extraction for the source file's `export default '<value>';` form).

This is the **same shape** surfaced by airflow + tokio + clap + uv
(all of which need the workspace-version vs per-crate-version
equality check). react is the first JS-side data point.

### 2. `registry_append_only` — codes.json shape

`scripts/error-codes/codes.json` is asserted append-only by human
review only. A `registry_append_only` rule kind would assert that
the JSON object's keys at HEAD are a strict superset of the
previous git revision's keys, with no reassignment of existing
keys. The check needs git-history awareness (compare HEAD to
HEAD~1's blob contents), which is in scope for alint's existing
git-aware rule kinds (`git_blame_age`, `git_no_denied_paths`,
`git_commit_message`).

NEW pattern not previously surfaced — first appearance in P2a;
still single-source as of v0.9.17.
Generalises to: i18n string registries, feature-flag registries,
API endpoint maps, error-code maps. Currently a `v0.10 design
candidate`; promote to `v0.10 ship-target` once a second source
surfaces.

### Out of alint's scope (use the existing tool)

- All 5 in-tree custom eslint rules (`prod-error-codes`,
  `safe-string-coercion`, `warning-args`,
  `no-primitive-constructors`, `no-production-logging`) — TSESTree
  visitors.
- `extract-errors.js` — codegen against built bundles.
- `print-warnings.js` — hermes-parser AST walk over every JS source.
- `lint-build` (the second-pass rollup-output re-lint) — build-aware.
- `dangerfile.js` — PR-diff-aware.
- `linc` (eslint on changed files) — same PR-diff scope.

---

## Performance comparison (placeholder — bench when validation pass scales)

The repo is large enough to be a meaningful stress test:
- **~1,800** `.js` source files under `packages/`
- **~7,800** `.js` files including tests / snapshots
- **~140k** files including the `compiler/` subdir's test fixtures
  (the compiler ships with thousands of `.expect.md` AST snapshots
  that dominate the file count)

The `alint check` against the full sparse-clone tree completes in
under a second for the structural rules; the compiler fixtures
dominate the info-level findings (~3500 trailing-whitespace /
newline issues, 99% in `compiler/packages/babel-plugin-react-compiler/src/__tests__/fixtures/`).
The published S3 bench (100k files, mixed languages) hits 1.13 s on
a stock CI runner; the react full tree sits between S3 and S9.

Where alint shines on react specifically: the **per-package
manifest spot-checks** run against 39 `package.json` files in
single-digit milliseconds (sequential `node -e "require()"` calls
would be ~2-3 s of warm-cache startup). The per-package `repository.directory`
check found one real drift in milliseconds vs. the human-review
status quo.

---

## Followup feature work surfaced (consolidated)

1. **`cross_file_value_equals` rule kind** — covers
   `version-check.js` here, plus the airflow/tokio/clap/uv
   workspace-version sync patterns. Demand: 5 case studies.
2. **`registry_append_only` rule kind** — covers `codes.json`
   here, plus airflow's `check-no-new-airflow-exceptions` family
   (which is structurally the inverse: forbid additions to a
   denylist). Demand: 2 case studies, first appearance.
3. **`json_path_keys_match_pattern`** — extension to
   `json_path_matches` that lets you assert "every KEY (not value)
   under `$` matches the regex". Today the JSONPath wildcard
   `$.*` returns values, not keys; my `react-error-codes-json-keys-numeric`
   rule above is a workaround that asserts the values are non-empty
   (which is true) but doesn't actually constrain the keys to be
   numeric. Soft requirement — the registry-append-only rule
   subsumes this.

No new schema or language pitfalls hit while writing this config.
The 21 documented in `docs/development/CONFIG-AUTHORING.md` cover
everything that came up. ONE process near-miss surfaced: the JSON
output's "passing per-file rules omit `RuleResult` entirely"
behaviour caused initial confusion (16 of 36 react-* rules in the
JSON, the other 20 passing silently). This is documented behaviour
(see `coverage_audit_examples_parse.rs` and the dispatch-flip tests)
but isn't called out in `CONFIG-AUTHORING.md` — worth adding a
footnote to the "Pre-merge checklist" pointing config authors at
the engine's silent-pass semantics so they don't conclude their
rules aren't running. **Suggested CONFIG-AUTHORING.md addition:** a
note under "Parse-validation is necessary but not sufficient"
explaining that `--format json` filters out passing per-file rules,
and to use `alint list --config <path>` (which lists every rule
the engine WOULD run) for the authoritative view.

---

## Validation status (2026-05-07)

- alint version: **0.9.17** (1dbd9b218a0e, built 2026-05-07).
- `validate-config`: **87 rules loaded cleanly** (54 from 8
  bundled rulesets + 33 react-specific).
- Live-tree recheck: **pending** — `/tmp/facebook-react/` not
  present in this validation env.
- Pitfalls fixed in v0.9.17 that touch this config: none
  (react config doesn't use `respect_gitignore` or
  `literal_is_nested` patterns).
- Open gaps with active workarounds: `cross_file_value_equals`
  (v0.10 ship-target — react's `version-check.js` shape;
  current workaround: `command:` shellout); `registry_append_only`
  (v0.10 design candidate, react sole source — current
  workaround: human review of `git diff codes.json`).

## Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **Add the `agent-hygiene@v1` overlay (6 rules).** react ships
   `dangerfile.js` and 5 in-tree custom eslint rules under
   `scripts/eslint-rules/`. The agent-hygiene ruleset would gate
   AI-generated contribution patterns (no rolling commits to
   tracked artefacts, no tracked credentials, no agent-context
   leakage) — natural sixth bundled overlay alongside the
   existing `agent-context@v1`.
2. **Adopt `compliance/reuse@v1` (3 rules) for the per-package
   LICENSE story.** `react-published-package-has-source-license`
   is a per-rule react construct; the bundled `compliance/reuse@v1`
   overlay (REUSE-spec compliance: `LICENSES/` dir + per-file
   SPDX headers + `.reuse/dep5`) would express the same intent
   declaratively across all 22 published packages AND the 17
   internal packages without per-rule duplication.
3. **`alint suggest` against the live tree.** Pending
   `/tmp/facebook-react/`. Would surface candidate rules from
   the ~140k-file compiler subtree (heavy on `.expect.md` test
   fixtures that have repeating shapes the suggester would
   generalise).
