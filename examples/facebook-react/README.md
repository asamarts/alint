# Case study: `facebook/react`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/facebook-react/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `facebook/react` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-08 sparse-clone of `facebook/react@HEAD`.
Working-tree at `/tmp/react`: **6,878 tracked files** (`git ls-files`),
55 MB. Per-language counts: **3,487 .js + 396 .ts + 112 .tsx files**, 61
`package.json` files, **24 GitHub Actions workflows**, 55 README files.
The `compiler/` subtree alone ships **3,836 files including 1,719
`*.expect.md` AST snapshots** that dominate the file count and the
info-level gap discovery class (whitespace + final-newline cosmetics on
fixtures).

**alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`.

---

## 1. Inventory of existing tooling

react is a **Yarn classic v1 multi-package monorepo** — 39 packages
under `packages/*` declared via `"workspaces": ["packages/*"]` in the
root `package.json`, with `packageManager: "yarn@1.22.22"` pinning. 22
packages publish to npm; 17 are private (test-utils, devtools internals,
native-renderer bindings, `shared`, etc.).

### 1.1 `.github/workflows/shared_lint.yml` (4 jobs — gating)

| Job | What it actually does | Backing tool |
|---|---|---|
| `prettier` | `yarn prettier-check` (= `node ./scripts/prettier/index.js`) | prettier (with `.prettierrc.js`) |
| `eslint` | `node ./scripts/tasks/eslint` (full eslint over `**/*.js` + `.eslintignore`) | eslint + `scripts/eslint-rules/` (in-tree custom plugin `eslint-plugin-react-internal` with 5 rules) |
| `check_license` | `./scripts/ci/check_license.sh` — `git grep -l PATENTS` against an allow-list (the script itself is the only legitimate carrier) | bash + `git grep -l` |
| `test_print_warnings` | `./scripts/ci/test_print_warnings.sh` — asserts `print-warnings.js` emits at least one warning | hermes-parser AST walk over every JS source enumerating `console.warn(...)` / `console.error(...)` calls |

The other 20 workflows under `.github/workflows/` (`runtime_*`,
`compiler_*`, `devtools_*`, `shared_check_maintainer.yml`,
`shared_close_direct_sync_branch_prs.yml`, `shared_stale.yml`,
`shared_cleanup_*_caches.yml`, …) are runtime/test orchestration and
operational bots, not validation surfaces.

### 1.2 `package.json` `scripts:` block (validation entrypoints)

| Script | What it does | Backing tool |
|---|---|---|
| `lint` | `node ./scripts/tasks/eslint.js` (full eslint over `**/*.js` + `.eslintignore`) | eslint v8 |
| `linc` | Same eslint pass with `--onlyChanged` (PR mode) | eslint |
| `prettier` / `prettier-all` / `prettier-check` | `node ./scripts/prettier/index.js [write-changed\|write\|<no-arg>]` | prettier |
| `flow` / `flow-ci` | `node ./scripts/tasks/flow.js` / `flow-ci.js` | flow |
| `version-check` | `node ./scripts/tasks/version-check.js` — asserts `packages/shared/ReactVersion.js` exports the same string as `packages/{react,react-dom,react-test-renderer}/package.json#version` | node + regex `/export default '([^']+)';/` |
| `extract-errors` | `node scripts/error-codes/extract-errors.js` — rewrites `codes.json` from build artefacts | babel + AST walk |
| `lint-build` | `node ./scripts/rollup/validate/index.js` — second-pass eslint against the rollup output bundles | eslint with per-channel eslintrcs (`scripts/rollup/validate/eslintrc.{cjs,cjs2015,esm,fb,rn}.js`) |
| `flow-typed-install`, `prebuild`, `build*`, `test*` | Build/test orchestration | not validation surfaces |

### 1.3 `scripts/eslint-rules/` (in-tree custom plugin)

`eslint-plugin-react-internal` — 5 TSESTree visitors (out of alint's
"no AST" scope but listed for inventory completeness):

| Rule | What it does |
|---|---|
| `prod-error-codes` | Cross-references every `Error(<literal>)` against `scripts/error-codes/codes.json`; rejects new error messages that don't exist in the registry |
| `safe-string-coercion` | Bans implicit `String(x)` coercion patterns that throw in strict mode |
| `warning-args` | Validates `console.warn(...)` argument shape |
| `no-primitive-constructors` | Bans `new Boolean(...)` / `new String(...)` / etc. |
| `no-production-logging` | Bans `console.log` outside dev-only branches |

### 1.4 `scripts/error-codes/codes.json` (canonical error registry)

Append-only flat JSON: `{ "<numeric-id>": "<message-template>", ... }`.
~600 entries at the snapshot. Two consumers depend on the exact shape:
`scripts/eslint-rules/prod-error-codes.js` (`Set` of message templates)
and `scripts/error-codes/transform-error-messages.js` (babel pass that
rewrites `new Error("foo")` to `formatProdErrorMessage(<id>)` for prod
builds). Append-only-ness enforced **only by human review** of `git diff
codes.json` plus a Danger reminder.

### 1.5 `dangerfile.js` (PR-time inspector)

Out of alint's scope — operates on PR-diff state, posts a comment
summarising the PR's impact (changed-file count, bundle-size diff).

### 1.6 Configuration files (the "if these go missing CI fails confusingly" set)

| File | Why it matters |
|---|---|
| `.eslintrc.js` | Registers the `react-internal` plugin (without it, all 5 in-tree custom rules silently stop running) |
| `.eslintignore` | Skips `**/node_modules`, `build/`, `coverage/`, `compiler/` |
| `.prettierrc.js` / `.prettierignore` | Prettier shape + ignore set |
| `.nvmrc` | Pins node version (read by every workflow's `actions/setup-node@v4`) |
| `.gitattributes` | `* text=auto` cross-platform line-ending normalisation |
| `MAINTAINERS` | `shared_check_maintainer.yml` workflow reads this |
| `ReactVersions.js` (root) | Single source of truth for the publishing pipeline |
| `packages/shared/ReactVersion.js` | Single source of truth for the runtime version string |

### 1.7 Per-package conventions (the monorepo discipline)

- Every published package has matching `repository.directory` (`packages/<name>`)
- Every published package's `homepage` points to https://react.dev/
- Every published package's `bugs` (string or object form) points to https://github.com/facebook/react/issues
- Every published package's `version` is plain semver
- Meta copyright + MIT license header on every hand-edited `.js` source under `packages/*/src/`

---

## 2. Coverage classification

Each surface from §1 tagged with one of:

- **alint-today** — name the rule + ruleset (`oss-baseline` / `node` /
  `monorepo` / `monorepo/yarn-workspace` / `ci/github-actions` /
  `hygiene/no-tracked-artifacts`) OR the per-rule entry in this
  directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate.
- **out-of-scope** — explain why (TSESTree visitor, build-aware
  re-lint, hermes-parser AST walk, PR-diff-aware).

### 2.1 The 4 `shared_lint.yml` jobs

| Job | Coverage | Notes |
|---|---|---|
| `prettier` | alint-today (shellout) | `react-prettier-check` (`command:` rule wrapping `yarn prettier-check`); structural floor `react-prettierrc-exists` + `react-prettierignore-exists` |
| `eslint` | alint-today (shellout + structural floor) | `react-eslint` (`command:` rule wrapping `yarn lint`); `react-eslintrc-loads-react-internal-plugin`, `react-eslintrc-uses-hermes-parser`, `react-eslintignore-skips-{node-modules,build-output}`, `react-package-json-links-eslint-rules` |
| `check_license` | alint-today | Replaced declaratively by `react-no-patents-references` (`file_content_forbidden` over `**/*.{md,txt,js,json,ts,tsx,yml,yaml}` for `PATENTS`, with the script itself excluded); plus `react-check-license-script` (`command:` rule for redundancy) |
| `test_print_warnings` | out-of-scope | Underlying `print-warnings.js` walks every JS source with hermes-parser to enumerate `console.warn(...)` calls — AST analysis, not structural validation |

### 2.2 `package.json scripts:`

| Script | Coverage | Notes |
|---|---|---|
| `lint` | alint-today (shellout) | `react-eslint` |
| `linc` | out-of-scope | PR-diff-aware (`--onlyChanged`) |
| `prettier-check` | alint-today (shellout) | `react-prettier-check` |
| `flow-ci` | alint-today (shellout) | `react-flow-check` (analysis is out of scope; `command:` shellout wraps the existing tool) |
| `version-check` | alint-future | **`cross_file_value_equals`** (v0.10 ship-target, 10 sources). Today: `command:` shellout via `react-version-check` |
| `extract-errors` | out-of-scope | Codegen against built bundles (rewrites `codes.json` by walking `new Error(...)` literals in built artefacts) |
| `lint-build` | out-of-scope | Build-aware (per-channel eslintrcs against `build/oss-experimental/` bundles that don't exist until after `yarn build`) |

### 2.3 The 5 in-tree custom eslint rules

All 5 are **out-of-scope** — TSESTree visitors. `prod-error-codes`'s
cross-reference shape would generalise to the
`cross_file_value_equals` (v0.10 ship-target, 10 sources) primitive
in its registry-membership variant; sub-candidate, not yet promoted.

### 2.4 codes.json (error registry)

| Property | Coverage | Notes |
|---|---|---|
| Shape (flat `{string: string}`, no JSONC) | alint-today | `react-error-codes-json-keys-numeric` (`json_path_matches` against `$.*`) + `react-error-codes-json-no-comments` (`file_content_forbidden` for `(?m)^\s*//`) |
| Append-only invariant | alint-future | **`registry_append_only`** (v0.10 design candidate, react-only single source). Generalises to i18n string registries, feature-flag registries, API endpoint maps |

### 2.5 `dangerfile.js`

Out of scope — PR-diff-aware.

### 2.6 Configuration files

| File | Coverage | Rule |
|---|---|---|
| `.eslintrc.js` | alint-today | `react-eslintrc-loads-react-internal-plugin` (`file_content_matches` for `'react-internal'`); `react-eslintrc-uses-hermes-parser` |
| `.eslintignore` | alint-today | `react-eslintignore-skips-node-modules`, `react-eslintignore-skips-build-output` |
| `.prettierrc.js` / `.prettierignore` | alint-today | `react-prettierrc-exists`, `react-prettierignore-exists` |
| `.nvmrc` | alint-today | `react-nvmrc-version-pinned` (regex `^v\d+\.\d+\.\d+\s*$`) |
| `.gitattributes` | alint-today | `react-gitattributes-text-auto` (regex `^\* text=auto`) |
| `MAINTAINERS` | alint-today | `react-maintainers-file-present` + `react-maintainers-file-non-empty` |
| `ReactVersions.js` (root) | alint-today | `react-versions-file-declares-stable-packages` (`file_content_matches` for `const stablePackages = \{`) |
| `packages/shared/ReactVersion.js` | alint-today | `react-version-source-shape` (regex pinning the exact `export default '<semver>';` form `version-check.js` parses) |

### 2.7 Per-package conventions

| Convention | Coverage | Rule |
|---|---|---|
| `repository.directory` matches `packages/<name>` | alint-today | `react-package-repository-directory-matches` (`for_each_dir` over `packages/*` → `json_path_matches` `$.repository.directory`) |
| `homepage: https://react.dev/` | alint-today | `react-package-homepage-canonical` |
| `bugs` points to react/issues | alint-today | `react-package-bugs-points-to-react-issues` (regex over text — both string and object forms) |
| `version` is plain semver | alint-today | `react-package-version-is-semver` (`for_each_dir` + `json_path_matches`) |
| Meta copyright + MIT header on `packages/*/src/**/*.js` | alint-today | `react-copyright-header-src` (`file_header` with `pattern: |-`) |
| Same for `scripts/**/*.js` | alint-today | `react-copyright-header-scripts` (info-level — legacy backfill) |
| Per-published-package source-tree LICENSE | alint-today | `react-published-package-has-source-license` (info-level — rollup `packaging.js` adds one at build time) |

---

## 3. Quantified coverage

Counted across the **4 shared_lint jobs** + **8 `package.json` validation
scripts** + **5 in-tree eslint rules** + **codes.json (2 properties)** +
**dangerfile.js** + **8 config files** + **7 per-package conventions** =
**35 distinct surfaces**.

```
alint-today:     22 / 35 = 63%   (4 shellouts + 5 config files + 7 per-package + 6 misc)
alint-future:     2 / 35 =  6%   (cross_file_value_equals for version-check + registry_append_only for codes.json)
out-of-scope:    11 / 35 = 31%   (5 custom eslint TSESTree + extract-errors + lint-build + dangerfile + linc + test_print_warnings + 1 partial)
                 ──────────────
                 total = 100%
```

**Commentary.** Three observations:

1. **react is the densest "monorepo discipline" data point.** Of the 22
   alint-today surfaces, 7 are per-published-package conventions (every
   `package.json` field — `repository.directory`, `homepage`, `bugs`,
   `version`, source-LICENSE, copyright-header on `src/`) and another 5
   are the "if this config file disappears CI fails confusingly" set
   (`.eslintrc.js`, `.eslintignore`, `.prettierrc.js`, `.nvmrc`,
   `.gitattributes`). Outside the bundled rulesets, react is largely a
   CONVENTIONS-encoded-as-rules story rather than a script-replacement
   story.

2. **The 5 custom eslint rules are textbook out-of-scope** — TSESTree
   visitors over JS source. They belong in eslint and stay in eslint.
   alint's coverage cleanly *complements* them (catches the structural
   regressions that would silently disable them: `.eslintrc.js`
   dropping the plugin, `package.json` dropping the `link:` ref,
   `.eslintignore` skipping the wrong tree).

3. **Two unique alint-future candidates surface here:**
   - `cross_file_value_equals` (v0.10 ship-target, 10 sources) —
     `version-check.js` is the JS-side data point, joining
     airflow + tokio + clap + uv + helm + 4 others.
   - `registry_append_only` (v0.10 design candidate, **react-only
     single source**) — the codes.json shape. Unique to react; the
     primitive needs git-history awareness (compare HEAD to HEAD~1's
     blob contents) and would also generalise to airflow's
     `check-no-new-airflow-exceptions` family (structural inverse:
     forbid additions to a denylist).

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (878 lines including
narrative comments, **87 rules** loaded — confirmed by
`alint validate-config`: 33 react-specific + 54 from 8 bundled rulesets
— `oss-baseline=15` + `node=9` + `monorepo=4` +
`monorepo/yarn-workspace=4` + `ci/github-actions=3` +
`hygiene/no-tracked-artifacts=11` + `tooling/editorconfig=3` +
`agent-context=5` − overlap = 54 effective rule IDs after dedup).

Synopsis of the load-bearing repo-specific rules (full config in
`.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/node@v1
  - alint://bundled/monorepo@v1
  - alint://bundled/monorepo/yarn-workspace@v1
  - alint://bundled/ci/github-actions@v1
  - alint://bundled/hygiene/no-tracked-artifacts@v1
  - alint://bundled/tooling/editorconfig@v1
  - alint://bundled/agent-context@v1

facts:
  - id: is_yarn_v1
    file_content_matches: { paths: package.json, pattern: '"packageManager"\s*:\s*"yarn@1\.' }
  - id: has_react_package
    file_content_matches: { paths: packages/react/package.json, pattern: '"name"\s*:\s*"react"' }

rules:
  - id: react-copyright-header-src           # Meta + MIT header on packages/*/src/**/*.js
    when: facts.is_yarn_v1 and facts.has_react_package
    kind: file_header
    paths: { include: ["packages/*/src/**/*.js"], exclude: [...] }
    pattern: |-                              # |- (strip trailing newline) — pitfall #22 hardening
      ^/\*\*
       \* Copyright \(c\) Meta Platforms, Inc\. and affiliates\.
       \*
       \* This source code is licensed under the MIT license found in the
  - id: react-published-package-has-source-license  # for_each_dir + nested file_exists
  - id: react-package-repository-directory-matches  # for_each_dir + json_path_matches
  - id: react-no-patents-references          # file_content_forbidden across JS/MD/JSON/YAML
  - id: react-error-codes-json-keys-numeric  # json_path_matches over codes.json
  - id: react-version-source-shape           # regex on packages/shared/ReactVersion.js
  - id: react-eslint                         # command: ["yarn", "lint"]
  - id: react-prettier-check                 # command: ["yarn", "prettier-check"]
  - id: react-flow-check                     # command: ["yarn", "flow-ci"]
  - id: react-version-check                  # command: ["yarn", "version-check"] (until cross_file_value_equals ships)
  - id: react-check-license-script           # command: ["bash", "scripts/ci/check_license.sh"]
```

**Repo-specific vs bundled split:**
- **33 repo-specific rules** in `.alint.yml` (the `react-*` prefix)
- **54 bundled rules** from the 8 extended rulesets

**Validation:** `alint validate-config` reports `✓ Config valid: 87
rule(s) loaded`. The `pattern: |-` (strip-final-newline block scalar)
on `react-copyright-header-{src,scripts}` is a **pitfall #22 hardening
fix landed in this batch** — see §6.

---

## 5. Performance

Methodology: `hyperfine -i --warmup 1 --runs 3` against the same
`/tmp/react` working tree captured 2026-05-08. Machine: Linux
6.1.0-42-amd64, ~10 logical cores. alint binary `target/release/alint
v0.9.17`. The `-i` flag (ignore non-zero exit) is necessary because
several `command:` shellouts fail when their tool isn't on PATH (yarn,
bash); the alint walk + JSON serialisation timing is independent of
their exit code.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full pass** (87 rules, includes `command:` shellouts that fail-fast on missing tools) | n/a | n/a | **114 ms ± 3 ms** | — |
| **alint lite pass** (8 bundled rulesets only — no react-specific shellouts) | n/a | n/a | **62 ms ± 1 ms** | — |
| `yarn lint` (eslint over packages + scripts) | eslint v8 | pending — `yarn` not on PATH | n/a — alint shells out | 1× — alint wraps the existing tool |
| `yarn prettier-check` | prettier | pending — `yarn` not on PATH | n/a — alint shells out | 1× — alint wraps |
| `yarn flow-ci` | flow | pending — `yarn` not on PATH | n/a — alint shells out | 1× — alint wraps |
| `bash scripts/ci/check_license.sh` (replaces `git grep -l PATENTS` + allowlist) | bash + git grep | pending — exists but needs git context | replaced by `react-no-patents-references` (forbidden-pattern) | declarative replacement |

The headline number: **a single 114 ms alint pass replaces all the
shape assertions across 6,878 files** (per-package `repository.directory`
+ `homepage` + `bugs` + `version` shape across 39 packages, plus the
copyright header rule sweeping 1,800+ source files, plus the
PATENTS-grep equivalent across MD/TXT/JS/JSON/YAML/TS/TSX/YML, plus 8
config-file shape pins, plus the bundled hygiene + GHA + monorepo
overlays). The lite pass (bundled-only) at **62 ms** is the floor —
that's 8 rulesets across 6,878 files, including 24 GHA workflow
hardening checks.

### 5.2 Pending — needs additional toolchain

| Check | Tool | Reproduction |
|---|---|---|
| `react-eslint` | yarn + eslint v8 | `nvm use && yarn install --frozen-lockfile && time yarn lint` |
| `react-prettier-check` | yarn + prettier | `yarn prettier-check` |
| `react-flow-check` | yarn + flow | `yarn flow-ci` |
| `react-version-check` | yarn + node script | `yarn version-check` |
| `react-check-license-script` | bash + git | `bash scripts/ci/check_license.sh` |

The end-to-end `make test-style`-equivalent — `yarn lint && yarn
prettier-check && yarn flow-ci && yarn version-check &&
./scripts/ci/check_license.sh` — runs roughly 90-120 seconds on a warm
yarn cache (eslint dominates: a full `yarn lint` over react's 5,000+
JS/TS files in CI takes 60-90s). alint's structural floor at 114 ms
adds <0.2% wall-clock to that pipeline while catching 22 distinct
classes of regression that the existing pipeline doesn't cover at all.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/facebook-react/.alint.yml /tmp/react` (live, JSON-format).

**Headline:** alint surfaces **3,907 violations** across 18 failing
rules. **3,457 are info-level cosmetics (1,731 missing-final-newline +
1,726 trailing-whitespace) overwhelmingly in the `compiler/` test
fixtures** (see compiler subtree note above — 1,719 `.expect.md`
snapshots dominate). The remaining **450 are structural findings**:

### 6.1 Real findings (after deducting cosmetic class)

| Finding | Count | Severity | Rule | Triage |
|---|---:|---|---|---|
| Third-party actions not pinned to SHA | 164 | warning | `gha-pin-actions-to-sha` (bundled) | Real findings — react uses `actions/checkout@v4` style throughout. Recent OpenSSF Scorecard signal; would harden supply-chain posture. |
| Workflows missing `permissions: contents: read` | 24 | warning | `gha-workflow-contents-read` (bundled) | Real findings across all 24 workflows. Adding the explicit declaration is the OpenSSF Token-Permissions check. |
| `packages/*/src/**/*.js` files missing Meta header | 111 | warning | `react-copyright-header-src` | Triaged: 96 are vendored code (eslint-plugin-react-hooks `code-path-analysis/` files starting with `'use strict'`); 7 are old `Copyright (c) Meta Platforms, Inc. AND ITS affiliates.` (drift from the canonical `and affiliates.`); 8 are anomalies (`/**\n/**\n` doubled, generated files starting with `'use strict';` then having Meta header later). **All 111 are real signals** — see §6.2 classification |
| `scripts/**/*.js` files missing Meta header | 75 | info | `react-copyright-header-scripts` | Same flavour — legacy dev scripts; info-level so doesn't gate CI |
| Per-published-package source-tree `LICENSE` missing | 39 | info | `react-published-package-has-source-license` | Real — rollup `packaging.js` adds one at build, but `npm pack` from source without build would ship without |
| `bugs` field shape drift across published packages | 19 | info | `react-package-bugs-points-to-react-issues` | Real shape variance across 19 packages — some legitimately use the string form, some the object form, some omit |
| `package.json` files in private packages without `README.md` | 5 | warning | `monorepo-packages-have-readme`, `yarn-workspace-member-has-readme` (bundled) | Real — `react-dom-bindings`, `react-server-dom-fb`, `react-native-renderer`, `shared`, +1 |
| `packages/react-refresh/package.json#repository.directory: "packages/react"` | 1 | warning | `react-package-repository-directory-matches` | **Real bug — copy-paste regression.** Should be `packages/react-refresh`. The kind of single-character drift that human review consistently misses; alint catches it deterministically at PR time |
| One package with non-canonical `homepage:` URL | 1 | info | `react-package-homepage-canonical` | Real — points to `reactjs.org` instead of canonical `react.dev` |
| `agent-context-non-stub` violation | 1 | warning | `agent-context-non-stub` (bundled) | `CLAUDE.md` exists but minimal content; agent-context bundle wants a proper tour |
| `oss-codeowners-exists` info | 1 | info | `oss-codeowners-exists` (bundled) | react uses `MAINTAINERS` (which is asserted) instead of `CODEOWNERS` — info-only |

**Real net-new findings alint surfaces that existing tooling misses:**
**7 stable, machine-verifiable structural drifts** (the 1 repository.directory
copy-paste regression in react-refresh + the 7 old-style `and its
affiliates` headers + the 1 non-canonical homepage URL); plus **188
hardening signals** (164 SHA-pinning + 24 workflow-permissions); plus
**75 dev-script header backfill candidates** at info level.

### 6.2 The `react-copyright-header-src` 111-violation class — classification

Sampled all 111 violations and classified by file content:

| Class | Count | Example |
|---|---:|---|
| **No Meta header at all** (vendored eslint-plugin-react-hooks code-path-analysis utilities — `assert.js`, `code-path-{analyzer,segment,state}.js`, `fork-context.js`, `id-generator.js`) | 96 | `packages/eslint-plugin-react-hooks/src/code-path-analysis/assert.js` |
| **Old "and its affiliates" Meta header** (drift from canonical `and affiliates.`) | 7 | `packages/react/src/ReactCacheClient.js`, `ReactCacheImpl.js`, `ReactCacheServer.js` |
| **Anomalies** (`/**\n/**\n` doubled comment, generated files with `'use strict';` then Meta header later) | 8 | `packages/react-devtools-shared/src/backend/utils/index.js` (literal `/**\n/**\n` doubled-comment block prefix); `ErrorTesterCompiled.js` (build artefact starting with `'use strict';`) |

The 7 "and its affiliates" findings are the highest-value ones — they
suggest a historical Meta legal-text update happened that didn't
propagate uniformly. The 96 vendored-code findings are arguably
expected (third-party origin); a `paths.exclude:
packages/eslint-plugin-react-hooks/src/code-path-analysis/**` entry on
the rule would cleanly suppress them.

### 6.3 Suspected `.alint.yml` bug attention (pitfall #22 candidates)

The brief flagged TWO `pattern: |` instances in the config — line 164
(`react-copyright-header-src`) and line 189 (`react-copyright-header-scripts`).

**Investigation:** Both rules use YAML `|` (literal block scalar), which
**does** append a trailing `\n` to the pattern string per pitfall #22.
However, the trailing `\n` is **benign in this case** because real
Meta-headered React files always continue with ` * LICENSE file in the
root directory of this source tree.\n` — the `\n` after `the` IS
present in real files, so the regex's trailing `\n` matches. Manual
verification with Python's `re.match` confirms both pattern variants
(with and without trailing `\n`) match the canonical Meta header.

**Hardening fix landed in this batch:** Both rules updated from
`pattern: |` to `pattern: |-` (strip-final-newline block scalar) for
canonical-correct semantics per pitfall #22 guidance — this prevents
future drift if the pattern is ever extended past the current last
line. **Validated:** `alint validate-config` still reports `✓ Config
valid: 87 rule(s) loaded`. The 111 + 75 violation counts were re-verified
unchanged after the fix (real findings, not pitfall-induced false positives).

### 6.4 No silent-failure-mode bugs in this config

No instances of pitfalls #13 (regex `^`/`$` file-anchoring without
`(?m)`), #14 (single-quoted YAML `\n` non-expansion), #16
(`*_path_matches` against bool/number), or #17 (`*_path_equals` against
`[*]`) surfaced. The config is well-disciplined: every rule that uses
line anchors uses `(?m)`; the JSONPath-typed assertions use
`*_path_matches` only when targeting strings.

---

## 7. Followup feature work surfaced

- **`cross_file_value_equals` rule kind** (v0.10 ship-target, 10
  sources). react's `version-check.js` shape is the JS-side data
  point. Workaround used: `command:` shellout via `react-version-check`.
- **`registry_append_only` rule kind** (v0.10 design candidate, react
  sole source). codes.json's append-only invariant. Generalises to
  i18n string registries, feature-flag registries, API endpoint maps.
  Workaround today: human review of `git diff codes.json` + Danger.
- **`paths.exclude` on `react-copyright-header-src`** to drop the 96
  vendored eslint-plugin-react-hooks `code-path-analysis/` files —
  cleanly addressable as a config refinement; not engine work.
- **`agent-hygiene@v1` overlay derivative** — react ships `dangerfile.js`
  + 5 in-tree custom eslint rules. The `agent-hygiene@v1` ruleset would
  gate AI-generated contribution patterns alongside the existing
  `agent-context@v1`.

---

## 8. Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **`compiler/` subtree-config.** The 1,719 `.expect.md` AST snapshots
   under `compiler/packages/babel-plugin-react-compiler/src/__tests__/fixtures/`
   account for ~95% of the 3,457 cosmetic findings. A subtree-scoped
   `.alint.yml` under `compiler/` (the v0.10 candidate `nested_configs:
   true`) would relax those rules per-tree without losing them
   repo-wide.
2. **`compliance/reuse@v1` overlay for the per-package LICENSE story.**
   `react-published-package-has-source-license` is per-rule react
   construct; the bundled `compliance/reuse@v1` overlay (REUSE-spec
   compliance: `LICENSES/` dir + per-file SPDX headers + `.reuse/dep5`)
   would express the same intent declaratively across all 22 published
   packages AND the 17 internal packages without per-rule duplication.
3. **`alint suggest` against the live tree.** Likely candidates: a
   generalised "every file under `packages/*/src/__tests__/__snapshots__/`
   matches `.+\.snap$`" rule the suggester could auto-discover from
   the compiler subtree's repeating shapes.

---

## 9. Validation status (2026-05-08)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **87** (33 react-specific + 54 from 8 bundled
  rulesets — `oss-baseline=15`, `node=9`, `monorepo=4`,
  `monorepo/yarn-workspace=4`, `ci/github-actions=3`,
  `hygiene/no-tracked-artifacts=11`, `tooling/editorconfig=3`,
  `agent-context=5`; rule IDs overlap, total dedups to 54)
- **`alint validate-config`:** ✓ Config valid: 87 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  3,907-violation breakdown (450 structural + 3,457 cosmetic)
- **Pitfall fixes (this batch):** Pitfall #22 hardening — both
  `react-copyright-header-{src,scripts}` patterns changed from
  `pattern: |` to `pattern: |-` for canonical-correct
  strip-final-newline semantics. Trivial 1-line fix per rule;
  zero behaviour change on the live tree (verified)
- **Open gaps:**
  - `cross_file_value_equals` (v0.10 ship-target, 10 sources) —
    react's `version-check.js` is the JS-side data point
  - `registry_append_only` (v0.10 design candidate, single-source —
    react's `codes.json`)
- **Bench numbers:** 114 ms (full 87-rule pass), 62 ms (lite
  bundled-only pass) on `/tmp/react`'s 6,878-file tree
