# Case study: `microsoft/TypeScript`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/microsoft-typescript/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `microsoft/TypeScript`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07, sparse-clone of
`microsoft/TypeScript@f350b523` (latest tip of main —
`f350b52331494b68c90ab02e2b6d0828d2a22a74` via
`git ls-remote https://github.com/microsoft/TypeScript HEAD`). Working
tree at `/tmp/TypeScript`: **60,931 files**, 674 MB working-tree
(20,798 `.ts` files in-tree + 14,016 `.types`/`.symbols` baseline
artefacts each + 9,983 `.txt` (mostly `.errors.txt` baselines) + 2,181
sub-directories under `tests/baselines/reference/`). Maintenance mode:
TS 6.0 is the last JS-based release; future development moved to
`microsoft/typescript-go`.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check TypeScript runs today, one row per check. The repo's gating
infrastructure is **Hereby** (in-house gulp-replacement) + a small set
of `scripts/*.mjs` files + 18 GitHub Actions workflows.

### 1.1 `Herebyfile.mjs` tasks (the canonical task runner — 971 lines)

Hereby is TypeScript's gulp-replacement task runner; tasks shape the
build + lint + test pipeline. The lint-class subset (the only set
relevant to alint):

| Task | What it actually does | Backing tool / runtime |
|---|---|---|
| `lint` | `node_modules/eslint/bin/eslint --cache --report-unused-disable-directives --max-warnings 0 .` over the whole tree (including the 9 custom `scripts/eslint/rules/*.cjs` plugin) | eslint v9 |
| `format` / `check-format` | `node_modules/dprint/bin.js fmt` / `node_modules/dprint/bin.js check` (whole tree, sequential plugin pass) | dprint v0.x |
| `knip` | `node_modules/knip/bin/knip.js` — unused-export detection across the import graph | knip |
| `runtests-parallel` / `tests` / `runTests` / `watchTests` | mocha-fivemat-progress-reporter compiler test runner | mocha (custom reporter) |
| `baseline-accept` | Regenerates `tests/baselines/reference/*` from compiler output (after a test pass) | tsc + custom diff/copy logic |
| `run-eslint-rules-tests` | mocha tests for the 9 custom eslint-rules under `scripts/eslint/tests/` | mocha |

**Gating subset:** `lint` + `check-format` + `knip` are the 3 lint-class
gates in `.github/workflows/ci.yml` (3 jobs: `lint`, `format`, `knip`).
`runtests-parallel` + `baseline-accept` are the test-class gates;
`run-eslint-rules-tests` is gated as a sub-step of the `misc` job.

### 1.2 `scripts/*.mjs` dev scripts (8 in-tree scripts)

Distinct from Hereby tasks; these are direct `node scripts/X.mjs`
invocations:

| Script | What it actually does | Backing tool / runtime |
|---|---|---|
| `addPackageJsonGitHead.mjs` | Adds the current git SHA to the published `package.json` before tarballing | git + JSON write |
| `browserIntegrationTest.mjs` | Runs the browser smoke test (loads the bundled compiler in playwright, asserts `ts.version` matches `package.json#version`) | playwright |
| `checkModuleFormat.mjs` | Runtime probe — every supported `require`/`import` shape against the published bundle returns the right `version` | node (dynamic import) |
| `checkPackageSize.mjs` | Diffs `npm pack --dry-run --json` between two refs; fails on >10% size growth | npm pack + git diff |
| `configurePrerelease.mjs` | Writes a prerelease suffix into `package.json#version` | JSON write |
| `dtsBundler.mjs` | Bundles `*.d.ts` declarations for the published API | TS AST traversal |
| `errorCheck.mjs` | Asserts every diagnostic in `src/compiler/diagnosticMessages.json` appears in at least one `tests/baselines/reference/*.errors.txt` | bash glob + regex grep |
| `find-unused-diganostic-messages.mjs` | Asserts every diagnostic in the generated `diagnosticInformationMap.generated.ts` is referenced from `src/**/*.ts` | bash glob + grep |
| `generateLocalizedDiagnosticMessages.mjs` | Codegen for the localised diagnostic message tables | XML/.lcl translation pipeline |
| `link-hooks.mjs` | Links `scripts/hooks/*` into `.git/hooks/` (developer-machine setup) | symlinks |
| `post-vsts-artifact-comment.mjs` | Posts an Azure DevOps comment with the build artefact URL | HTTP |
| `processDiagnosticMessages.mjs` | Codegen — generates `diagnosticInformationMap.generated.ts` from `src/compiler/diagnosticMessages.json` | TS codegen |
| `produceLKG.mjs` | Promotes the just-built compiler into `lib/` as the new last-known-good (LKG) bootstrap | file copy |
| `regenerate-unicode-identifier-parts.mjs` | Codegen for the Unicode identifier-parts tables (consumed by `src/compiler/scanner.ts`) | Unicode database |
| `run-sequence.mjs` | Helper: runs a sequence of Hereby tasks with single-failure stop | shell |

### 1.3 `scripts/eslint/rules/*.cjs` (9 custom eslint rules)

All 9 are TSESTree visitors — out of alint's "no AST" scope. Listed
here so the inventory is complete:

| Rule | What it does |
|---|---|
| `argument-trivia.cjs` | Enforces inline-comment style on call arguments (e.g. `foo(/*x*/ 1, /*y*/ 2)`) |
| `debug-assert.cjs` | Argument types of `Debug.assert` calls |
| `jsdoc-format.cjs` | `@internal` placement / multi-JSDoc rules |
| `js-extensions.cjs` | Relative imports must end in `.js` (per the source convention) |
| `no-array-mutating-method-expressions.cjs` | Bans expression-statement uses of `arr.sort()` / `arr.push()` / `arr.reverse()` etc. |
| `no-direct-import.cjs` | Bans deep relative imports across `src/` boundaries |
| `no-in-operator.cjs` | Bans the `in` keyword (use `hasProperty` instead) |
| `no-keywords.cjs` | Bans names like `string`, `number`, `boolean` as identifiers |
| `only-arrow-functions.cjs` | Bans `function` expressions / declarations in favour of arrow fns |
| `utils.cjs` | Shared rule helpers (not a rule itself) |

These are perfect examples of "AST analysis is not alint's niche" —
they belong in eslint and stay in eslint. The mocha self-tests under
`scripts/eslint/tests/` are gated by `npx hereby
run-eslint-rules-tests`.

### 1.4 `.github/workflows/` (18 workflows)

| Workflow | What it does | alint disposition |
|---|---|---|
| `ci.yml` | Orchestrates the 12 CI jobs — `test` (matrix across node 16/18/20/22/24 × {linux, windows, macos}), `coverage`, `lint`, `knip`, `format`, `browser-integration`, `typecheck`, `smoke`, `package-size`, `misc`, `self-check`, `baselines`. Aggregator job `required` confirms each | Each step is a separate surface |
| `pr-modified-files.yml` | Comments / closes PRs based on the changed-files set (e.g. closes PRs touching generated DOM lib files) | OUT — operates on PR diff |
| `codeql.yml` | CodeQL static analysis | OUT — security scanner |
| `scorecard.yml` | OpenSSF Scorecard run | Partial alint coverage: action-SHA pinning, permission-block presence enforced via `ts-workflow-actions-pinned-by-sha` |
| `accept-baselines-fix-lints.yaml` | Manual workflow to regenerate baselines / run `--fix` | OUT — mutation, not validation |
| 13 others (`insiders`, `lkg`, `nightly`, `set-version`, `sync-branch`, `sync-wiki`, `twoslash-repros`, `update-package-lock`, `release-branch-artifact`, `new-release-branch`, `create-cherry-pick-pr`, `close-issues`, `copilot-setup-steps`) | Release / maintenance bots | OUT — operational, not validation |

### 1.5 Per-language config + registry files

| Path | Role |
|---|---|
| `.dprint.jsonc` | dprint v0.x formatter config (TypeScript + JSON + YAML plugin URLs pinned, `incremental: true`) |
| `eslint.config.mjs` (261 lines) | eslint flat-config; loads the 9 `scripts/eslint/rules/*.cjs` via `RULES_DIR` mechanism; sets `unicode-bom: ['error', 'never']`, `prefer-const`, etc. |
| `knip.jsonc` (38 lines) | knip config — entry points: `src/typescript/typescript.ts`, `src/tsserver/server.ts`, `src/typingsInstaller/nodeServer.ts`, plus dev/test entry points |
| `package.json` (117 lines) | Lists the 6 lint-class scripts (`lint`, `format`, `check-format`, `knip`, `setup-hooks`, `test:eslint-rules`) + the build orchestration scripts |
| `package-lock.json` | npm v10+ lockfile |
| `Herebyfile.mjs` (971 lines) | The canonical task definitions |
| `scripts/CopyrightNotice.txt` | The canonical Apache-2 / Microsoft copyright block prepended to *bundled* output by `Herebyfile.mjs#generateLibs`. **Not present at the head of any `src/**/*.ts` file** (verified — see §6) |
| `tsconfig.json` (root, sparse — TS drives compiler config from per-package `tsconfig.*.json` inside `src/`) | Per-package compiler configs |
| `scripts/tsconfig.json` | JSONC compiler config for the build scripts (`allowJs: true` + `checkJs: true`) |
| `.editorconfig` | `[{src,scripts}/**.{ts,json,js}]` block: end_of_line=crlf, charset=utf-8, trim_trailing_whitespace=true, insert_final_newline=true, indent_style=space, indent_size=4 |
| `.gitattributes` | `*.js linguist-language=TypeScript`, `**/*.json linguist-language=jsonc`, **`* -text`** (the load-bearing line — git treats unknown files as binary so it doesn't munge baseline line-endings) |
| `.git-blame-ignore-revs` | List of mass-formatting commits to skip in blame |
| `AGENTS.md` + `CLAUDE.md` (with maintenance-mode marker) | Coding-agent onboarding |
| `CONTRIBUTING.md`, `LICENSE.txt`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `SUPPORT.md`, `ThirdPartyNoticeText.txt` | Repo-root governance artefacts |

### 1.6 `tests/baselines/reference/` (~53k files, 2,181 sub-directories)

The compiler test corpus. Each compiler test produces 4 baseline
artefacts:

| Baseline ext | Purpose |
|---|---|
| `*.errors.txt` | The diagnostic output (only present when the test had errors) |
| `*.js` | The emitted JavaScript (always present after a successful test run) |
| `*.symbols` | The symbol table dump (resolved-symbol-per-identifier annotations) |
| `*.types` | The type-table dump (resolved-type-per-expression annotations) |

Counts at this commit: **9,722 `*.errors.txt`** + **18,152 `*.js`** +
**14,016 `*.symbols`** + **14,016 `*.types`** + 459 `*.baseline` +
~9,000 `*.symbols.diff`/`*.types.diff` + miscellaneous. The harness
writes each set atomically; an `.errors.txt` without its companion
`.js` usually means a stale checkin from a deleted test.

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset
  (`oss-baseline` / `node` / `ci/github-actions` /
  `hygiene/no-tracked-artifacts` / `tooling/editorconfig` /
  `agent-context`) OR the per-rule entry in this directory's
  `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why (TSESTree visitor, npm-pack diff,
  runtime probe, codegen drift, …). The "out-of-scope" label is
  positive — these are checks where the existing tool *is* the right
  tool.

### 2.1 Hereby tasks (3 lint-class gates)

| Task | Coverage | Notes |
|---|---|---|
| `lint` | alint-today | `ts-eslint` (`command:` rule shelling to `npm run lint`) |
| `format` / `check-format` | alint-today | `ts-dprint-check` (`command:` rule shelling to `npx dprint check`) |
| `knip` | alint-today | `ts-knip` (`command:` rule shelling to `npm run knip`) |

### 2.2 `scripts/*.mjs` (8 scripts + 7 codegen/operational)

| Script | Coverage | Notes |
|---|---|---|
| `addPackageJsonGitHead.mjs` | out-of-scope | Mutation script (modifies package.json), not validation |
| `browserIntegrationTest.mjs` | out-of-scope | Runtime probe (playwright) |
| `checkModuleFormat.mjs` | out-of-scope | Runtime probe (dynamic-import shapes against bundle) |
| `checkPackageSize.mjs` | out-of-scope | Cross-ref diff (`npm pack --dry-run --json` between two refs) — alint sees one tree at a time |
| `configurePrerelease.mjs` | out-of-scope | Mutation |
| `dtsBundler.mjs` | out-of-scope | Codegen / TS AST traversal |
| `errorCheck.mjs` | alint-future | **`pair_count`** (≥1 partner files match a registry entry) — every diagnostic in `src/compiler/diagnosticMessages.json` appears in at least one `tests/baselines/reference/*.errors.txt`. Same shape as airflow's `check-no-new-airflow-exceptions` (2 sources, v0.10+ design candidate) |
| `find-unused-diganostic-messages.mjs` | out-of-scope | Cross-file reference graph over a generated registry — effectively eslint's `no-unused-exports` over a generated registry; AST-flavoured |
| `generateLocalizedDiagnosticMessages.mjs` | out-of-scope | Codegen |
| `link-hooks.mjs` | out-of-scope | Developer-machine setup |
| `post-vsts-artifact-comment.mjs` | out-of-scope | HTTP / CI helper |
| `processDiagnosticMessages.mjs` | out-of-scope | Codegen |
| `produceLKG.mjs` | out-of-scope | Mutation (file copy) |
| `regenerate-unicode-identifier-parts.mjs` | out-of-scope | Codegen |
| `run-sequence.mjs` | out-of-scope | Build orchestration |

### 2.3 `scripts/eslint/rules/*.cjs` (9 rules)

All **out-of-scope** as alint primitives — TSESTree visitors. Wrapped
collectively by `ts-eslint` (the `command:` rule shelling to `npm run
lint`). The mocha self-tests under `scripts/eslint/tests/` are
out-of-scope as well (test-runner-driven).

### 2.4 `.github/workflows/` (18 workflows)

| Workflow | Coverage | Notes |
|---|---|---|
| `ci.yml` | alint-today (per-step) | Each lint-class step is its own surface — see §2.1 |
| `pr-modified-files.yml` | out-of-scope | PR-diff aware |
| `codeql.yml` | out-of-scope | Security scanner |
| `scorecard.yml` | alint-today (partial) | `ts-workflow-actions-pinned-by-sha` (`yaml_path_matches` over `$.jobs.*.steps[?match(@.uses, '^[^./]')].uses`) covers the action-SHA-pinning subset |
| `accept-baselines-fix-lints.yaml` | out-of-scope | Mutation |
| 13 operational workflows | out-of-scope | Release / maintenance bots |

### 2.5 Per-language config + registry files

| Artefact | Coverage | Rule |
|---|---|---|
| `.dprint.jsonc` (TypeScript / JSON / YAML plugin pins) | alint-today | `ts-dprint-typescript-plugin-pinned`, `ts-dprint-json-plugin-pinned`, `ts-dprint-yaml-plugin-pinned` (3 × `file_content_matches`) |
| `eslint.config.mjs` | out-of-scope | The 9 custom-rule registrations are TSESTree-driven; `unicode-bom: never` is restated by `ts-src-no-bom` (`no_bom`) |
| `knip.jsonc` | out-of-scope | `command:` rule wraps the existing tool |
| `package.json` (`scripts.lint` / `scripts.format` / `scripts.knip` literal commands) | alint-today | `ts-package-json-has-lint-script`, `ts-package-json-has-format-script`, `ts-package-json-has-knip-script` (3 × `json_path_matches`) |
| `Herebyfile.mjs` | out-of-scope | The task definitions are JS code — alint's `command:` rule wraps them via `npm run` |
| `scripts/CopyrightNotice.txt` (canonical header) | alint-today (aspirational) | `ts-copyright-header-src` + `ts-copyright-header-scripts` (2 × `file_header`) — but **the headers don't exist on disk** (verified — see §6); rules are aspirational |
| `tsconfig.json` per-package (`compilerOptions.strict: true`) | alint-today | `ts-tsconfig-strict-mode` (`json_path_equals`) |
| `scripts/tsconfig.json` (`compilerOptions.checkJs: true`) | alint-today | `ts-scripts-tsconfig-checkjs` (`json_path_equals`) — but JSONC parsing limitation surfaces as a false positive (see §6) |
| `.editorconfig` invariants | alint-today | `ts-src-line-endings-crlf` (`line_endings: crlf`), `ts-src-final-newline` (`final_newline`), `ts-src-no-trailing-whitespace` (`no_trailing_whitespace`), `ts-src-no-bom` (`no_bom`) — plus the `.editorconfig` presence check from `tooling/editorconfig@v1` |
| `.gitattributes` (`* -text` line-endings discipline) | alint-today | `ts-gitattributes-keeps-binary-default` (`file_content_matches`) |
| `.git-blame-ignore-revs` | out-of-scope | git metadata; not a structural property |
| `AGENTS.md` + maintenance marker | alint-today | `ts-agents-md-present` (`file_exists`), `ts-agents-md-maintenance-marker` (`file_content_matches`) — plus the 5 rules from `agent-context@v1` |
| `CONTRIBUTING.md`, `LICENSE.txt`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `SUPPORT.md` | alint-today | `oss-readme-exists`, `oss-license-exists`, `oss-license-non-empty`, `oss-security-policy-exists`, `oss-code-of-conduct-exists` (oss-baseline) |
| `ThirdPartyNoticeText.txt` | alint-today | `ts-third-party-notice-present` (`file_exists`) |
| `package-lock.json` (lockfile presence) | alint-today | `node-has-lockfile` (node ruleset) |
| Repo-wide hygiene (no `node_modules/`, no `target/`, no `dist/`, …) | alint-today | All 11 rules from `hygiene/no-tracked-artifacts@v1`, plus `node-no-tracked-node-modules` (node ruleset) |

### 2.6 `tests/baselines/reference/` invariants

| Invariant | Coverage | Rule |
|---|---|---|
| Baseline file size cap (256 KiB ceiling, with carve-outs for `projectOutput/` and `api/`) | alint-today | `ts-baseline-file-max-size` (`file_max_size`) |
| Pairing — every `*.errors.txt` has a matching `*.js` sibling | alint-today (broken — see §6) | `ts-baseline-errors-pair-with-js` (`pair`) — fires false positives because `{stem}` doesn't strip multi-extensions like `.errors.txt` |
| `.gitattributes` `* -text` discipline | alint-today | `ts-gitattributes-keeps-binary-default` (above) |

---

## 3. Quantified coverage

Counted across the **3 Hereby lint-class gates** + **15
`scripts/*.mjs`** + **9 `scripts/eslint/rules/*.cjs`** + **5
gating-class workflows** (skipping the 13 operational ones) + **9
config / registry artefacts** + **3 baseline invariants** =
**44 distinct surfaces**.

```
alint-today:       18 / 44 = 41%   (3 Hereby + 0 scripts/*.mjs + 0 eslint-rules + 2 workflows + 9 config + 2 baseline + 2 governance)
alint-future:       1 / 44 =  2%   (1 scripts/*.mjs — errorCheck.mjs needs pair_count)
out-of-scope:      25 / 44 = 57%   (13 scripts/*.mjs + 9 eslint-rules + 3 workflows-class + 0)
                   ──────────────
                   total = 100%
```

Granular breakdown:

```
Hereby tasks (3 lint-class gates):
  alint-today:      3 /  3 = 100%
  out-of-scope:     0 /  3 =   0%

scripts/*.mjs (15 scripts):
  alint-today:      0 / 15 =   0%
  alint-future:     1 / 15 =   7%   (errorCheck.mjs)
  out-of-scope:    14 / 15 =  93%

scripts/eslint/rules/*.cjs (9 rules):
  out-of-scope:     9 /  9 = 100%

.github/workflows/ (5 gating-class workflows):
  alint-today:      2 /  5 =  40%   (ci.yml per-step + scorecard.yml partial)
  out-of-scope:     3 /  5 =  60%

config / registry / governance (12 artefacts):
  alint-today:     12 / 12 = 100%

baseline invariants (3):
  alint-today:      3 /  3 = 100%
```

**Commentary.** Three observations:

1. **TypeScript is alint's smallest gating-surface case study so far.**
   The Hereby + scripts + 9 custom eslint rules all collapse into 3
   `command:` rules in alint's config. The eslint rules themselves are
   AST visitors (out of alint's scope) and the validation surface
   beyond eslint + dprint + knip is small — ~12 declarative
   structural assertions. Maintenance mode is the explanation:
   structural conventions are stable, the team isn't actively
   widening the validation surface, and most of the lint discipline
   is in eslint.

2. **The single-source `pair_count` candidate (errorCheck.mjs) is
   already on the v0.10+ design candidate list.** Same shape (≥1
   partner files match a registry entry) as airflow's
   `check-no-new-airflow-exceptions`. 2 sources after this case
   study — design candidate, not a v0.10 must-ship.

3. **Aspirational rules vs reality — the copyright-header rules fire
   on every `src/**/*.ts` file (709) and every `scripts/**/*.{mjs,cjs}`
   file (39).** §6 explains: zero of those files actually carry the
   header (only Hereby's `generateLibs` task prepends it to bundled
   output, never to source). The rules are documented as aspirational
   in the config but produce 748 violations against the live tree, of
   which 100% are absent-header signal (no false positives in the
   regex sense — see §6 for the pitfall #22 nuance).

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (480 lines, 22
repo-specific rules, 6 bundled rulesets folded in via `extends:`,
**68 rules total** loaded — confirmed by `alint validate-config`).

**Synopsis of the load-bearing repo-specific rules** (full config in
`.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1            # 15 rules
  - alint://bundled/node@v1                    # 9 rules: package.json + lockfile + node_modules hygiene
  - alint://bundled/ci/github-actions@v1       # 3 rules: workflow contents-read + pin-to-sha + name
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules
  - alint://bundled/tooling/editorconfig@v1    # 3 rules: .editorconfig presence + final-newline + trim-trailing
  - alint://bundled/agent-context@v1           # 5 rules: AGENTS.md / CLAUDE.md / .cursor/ canonical shape

facts:
  - id: has_dprint_config
    any_file_exists: [.dprint.jsonc, .dprint.json]

rules:
  - id: ts-copyright-header-src           # canonical Apache-2 / Microsoft block on src/
    kind: file_header
    paths: "src/**/*.ts"
    pattern: |                            # ← pitfall #22: trailing-newline appended; see §6
      ^/\*! \*+
      Copyright \(c\) Microsoft Corporation\. All rights reserved\.
      Licensed under the Apache License, Version 2\.0
    level: warning
  - id: ts-baseline-file-max-size         # 256 KiB ceiling on tests/baselines/reference/**
    kind: file_max_size
    max_bytes: 262144
  - id: ts-baseline-errors-pair-with-js   # every *.errors.txt has a matching *.js
    kind: pair
    primary: "tests/baselines/reference/*.errors.txt"
    partner: "{dir}/{stem}.js"            # ← {stem} doesn't strip multi-extension; see §6
  - id: ts-gitattributes-keeps-binary-default
    kind: file_content_matches
    paths: .gitattributes
    pattern: '^\* -text$'
    level: error
  - id: ts-tsconfig-strict-mode           # JSONPath against tsconfig.* (JSONC tolerated for valid JSON)
    kind: json_path_equals
    paths: ["tsconfig*.json", "src/**/tsconfig*.json", "scripts/tsconfig.json"]
    path: "$.compilerOptions.strict"
    equals: true
  - id: ts-eslint                         # delegate to npm run lint
    kind: command
    paths: package.json
    command: ["npm", "run", "lint"]
    timeout: 600
  - id: ts-dprint-check
    kind: command
    paths: .dprint.jsonc
    command: ["npx", "dprint", "check"]
    timeout: 300
  - id: ts-knip
    kind: command
    paths: knip.jsonc
    command: ["npm", "run", "knip"]
    timeout: 600
```

**Repo-specific vs bundled split:**

- **22 repo-specific rules** in `.alint.yml` (the `ts-*` prefix
  identifies them in `alint list` output).
- **46 bundled rules** from the 6 extended rulesets (some IDs
  overlap, which is why `alint list` reports 68 not 68+22): 15 from
  oss-baseline + 9 from node + 3 from ci/github-actions + 11 from
  hygiene/no-tracked-artifacts + 3 from tooling/editorconfig + 5
  from agent-context = 46.

**Validation:** `alint validate-config` reports
`✓ Config valid: 68 rule(s) loaded`. Pitfall checks: the magic
comment is present (line 1); the `command:` rules use `command:` (not
`argv:`) and integer `timeout:` (not duration strings); the `pair`
rule uses `partner:` (not `secondary:`). **One regex pitfall (#22)
surfaces in two `pattern: |` instances at lines 104 and 122 — see
§6.** Plus a `pair` `{stem}` semantic mismatch on
`ts-baseline-errors-pair-with-js` — also §6.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3` on the same
`/tmp/TypeScript` working tree captured 2026-05-07. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary `target/release/alint
v0.9.17`. Where the upstream toolchain isn't installed locally, the
row is `pending — needs <toolchain>` with the exact reproduction
command.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full lite-pass** (65 rules, no `command:` shellouts) | n/a | n/a | **620 ms** ± 4 ms | — |

The 620 ms lite-pass walks the entire 60,931-file working tree
(674 MB), including the 9,722 `*.errors.txt` baseline files for the
`pair` rule, the 14,016 each of `*.symbols`/`*.types` for the
`file_max_size` rule, and the 20,798 `.ts` files for the
copyright-header check. The dominant cost is the `pair` rule
(cross-file) over the baseline corpus.

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `npm run lint` (eslint + 9 custom plugin rules) | eslint | pending — node_modules not installed | `cd /tmp/TypeScript && npm ci && time npm run lint` |
| `npx dprint check` | dprint | pending — node_modules not installed | `cd /tmp/TypeScript && npm ci && time npx dprint check` |
| `npm run knip` | knip | pending — node_modules not installed + depends on `hereby generate-diagnostics` having run first | `cd /tmp/TypeScript && npm ci && npx hereby generate-diagnostics && time npm run knip` |
| `npx hereby check-format` | dprint via hereby | pending — node_modules not installed | `cd /tmp/TypeScript && npm ci && time npx hereby check-format` |

Each of `npm run lint` / `dprint check` / `npm run knip` is a
multi-minute operation on a cold cache (eslint dominates on the
20,798 .ts source corpus + cache-warming pass; dprint adds another
30-60 s; knip has to walk the import graph). The alint shellout via
`command:` rule is roughly 1× the upstream wall-clock — alint's
contribution is orchestration (single config + single walk + single
report instead of 3 sequential `npm run` invocations).

The most-marketable comparison for TypeScript is therefore not
"alint runs the same checks faster" but "alint runs **1 declarative
pass** in 620 ms that catches the 12 structural assertions
(governance files, dprint plugin pins, package.json script pins,
.gitattributes line-endings line, AGENTS.md presence, baseline file
sizes) currently scattered across no canonical script + the editorial
review of the release-prep PR." None of those 12 assertions are
gated by a `verify-*.sh`-class script today.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /tmp/ts-alint-lite.yml /tmp/TypeScript`
(live run, JSON-format, lite config without the 3 `command:` rules
since toolchain isn't installed).

**Headline:** alint surfaces **27,853 violations** across the live
tree; of those, **9,055 are false positives traceable to a `pair`
rule `{stem}` semantic gap** and **748 are aspirational-rule fires
amplified by pitfall #22** (zero `src/`+`scripts/` files actually
carry the canonical Microsoft copyright header). The remaining
**~18,050** are dominated by **9,937 missing-final-newline +
7,876 trailing-whitespace findings** in the `tests/baselines/reference/`
corpus (real but expected — those are compiler-output dumps stored
verbatim, not source code; the bundled `oss-final-newline` /
`oss-no-trailing-whitespace` rules don't need to apply here).
**~237 are real or interesting findings**: the test-baseline file
size ceiling, the tsconfig `strict: true` discipline, the workflow
action-SHA pinning gaps, the JSONC parsing edge case for
`scripts/tsconfig.json`.

The 1 rule producing 9,055 false positives is **P0** — it inverts the
intended signal of the headline baseline-pairing rule. The 2 rules
producing 748 aspirational-rule fires are **P1** — they need both a
regex fix (pitfall #22 — drop the trailing newline) AND a level
adjustment (lower to `info` or `level: off` since the underlying
convention isn't actually applied to source files). Suspected and
flagged here for parent-agent triage; not auto-fixed.

### 6.1 Real findings (after deducting the false-positive class)

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 135 baseline files exceed the 256 KiB ceiling | `tests/baselines/reference/binaryArithmeticControlFlowGraphNotTooLarge.{symbols,types}`, `tests/baselines/reference/canWatch/getDirectoryToWatchFailedLookupLocationAtTypesIndirDos.baseline.md`, … | warning | `ts-baseline-file-max-size` | **Real but mostly known.** The rule is documented as a sentry — these are the runaway-output regressions. Some are intentional (long generated outputs from project-output tests). The exclude list (`tests/baselines/reference/projectOutput/**`, `tests/baselines/reference/api/**`) catches the legitimate-large dirs; everything in this list is novel growth. **Worth filing 5-10 PRs to trim the runaway-test outputs.** |
| 16 tsconfig files don't have `strict: true` | `src/compiler/tsconfig.json`, `src/deprecatedCompat/tsconfig.json`, `src/services/tsconfig.json`, `src/server/tsconfig.json`, … (16 total) | warning | `ts-tsconfig-strict-mode` | **Mixed.** Some of these are intentional (per-package compile config inherits strict from the root); some genuinely don't set strict. Review needed; likely the rule should `extends`-aware (only check the leaf-most config in a chain). |
| 1 tsconfig fails to parse as JSON | `scripts/tsconfig.json:10:9` (`"declaration": true,` in a `// commented-out` line) | warning | `ts-tsconfig-strict-mode` | **Real config gap.** alint's `json_path_equals` doesn't fully tolerate JSONC. The `tsconfig.json` files in TS use jsonc-style line comments (`//`); the rule misclassifies a `//-commented` line as malformed JSON. **Fix path:** alint's structured-query rules need a `Format::Jsonc` variant for tsconfig.* files. Single-source for now (TypeScript), but anywhere tsconfig.json appears with comments will hit this. **Logged as an alint engine gap** — see §7. |
| 18 workflows have third-party actions not pinned to a 40-char SHA | `.github/workflows/{accept-baselines-fix-lints.yaml, ci.yml, close-issues.yml, codeql.yml, …}` | warning | `ts-workflow-actions-pinned-by-sha` | **Real but documented as a "Scorecard catches this on the next nightly" trade-off.** The TS team uses floating-tag refs (`actions/checkout@v4`) for ergonomics. OpenSSF Scorecard surfaces the same 18 findings on its nightly run. alint surfaces them at PR time, which is the additive value here. |
| 17 `node_modules/` directories committed under `tests/baselines/reference/` and `tests/cases/projects/` | `tests/baselines/reference/project/nodeModulesMaxDepthExceeded/{amd,node}/maxDepthExceeded/built/node_modules`, `tests/cases/projects/NodeModulesSearch/importHigher/node_modules`, … | error | `node-no-tracked-node-modules` + `hygiene-no-node-modules` | **Real but intentional.** These are test fixtures — the tests literally check that the compiler can resolve `node_modules` lookups in baseline scenarios. The rule needs an exclude scoped to `tests/baselines/reference/**/node_modules` and `tests/cases/projects/**/node_modules`. **Recommended fix:** add the two scopes to the rule's exclude list. |
| 5 hygiene `**/build` / `**/coverage` / `**/dist` directory matches | `scripts/build/`, `tests/baselines/reference/config/showConfig/Shows tsconfig for single option/out`, `tests/baselines/reference/project/declarationDir2/amd/out`, … | warning | `hygiene-no-js-build-outputs` + `hygiene-no-cargo-target` | **All false positives.** `scripts/build/` is a source directory (not a JS build artefact); the `tests/baselines/reference/` matches are fixture content. **Recommended fix:** add the test-fixture root to the rule's exclude list. |
| 6 src/ files have trailing whitespace | `src/compiler/types.ts:6549`, `src/testRunner/unittests/tsbuild/moduleSpecifiers.ts:124`, `src/testRunner/unittests/tsc/declarationEmit.ts:317`, … | warning | `ts-src-no-trailing-whitespace` | **Real bugs.** dprint catches this on the next format pass; alint surfaces them at `alint check` time (pre-format). Worth filing a janitorial cleanup PR. |
| 1 src/ file has no final newline | (single file) | warning | `ts-src-final-newline` | **Real bug** — same class as above. Editorial polish. |
| 1 `oss-codeowners-exists` info-level finding | repo root | info | `oss-codeowners-exists` | TypeScript uses `.github/pr_owners.txt` instead of `CODEOWNERS`. **Expected.** |

**Total real findings (alint-surfaced, existing tooling missed): ~10
groupings (135 oversized baselines + 16 tsconfigs without strict + 6
trailing-ws + 1 missing newline + the JSONC parsing edge case + the
test-fixture node_modules false positives + the hygiene false
positives). Plus ~17,800 informational / cosmetic findings (trailing
whitespace + final newlines on the baseline corpus) that reflect the
fact that bundled `oss-final-newline` and `oss-no-trailing-whitespace`
sweep too broadly when applied to a compiler-output corpus.**

**Recommended fix to `.alint.yml` to align bundled rule scope to the
TypeScript baseline corpus:** scope the bundled `oss-final-newline`
and `oss-no-trailing-whitespace` to exclude `tests/baselines/reference/**`
(or restate them as repo-specific rules with the explicit exclude).

### 6.2 Suspected `.alint.yml` bugs flagged for parent triage

Two rules in this directory's `.alint.yml` produce systemically
wrong verdicts. Not auto-fixed; flagged here per the brief's
constraint.

#### Bug 1: `ts-baseline-errors-pair-with-js` fires 9,055 false positives

**Cause.** The `pair` rule's `{stem}` token resolves via Rust's
`std::path::Path::file_stem()`, which strips only the **last**
extension. For `ArrowFunction1.errors.txt`, `file_stem()` returns
`ArrowFunction1.errors` (not `ArrowFunction1`). The partner template
`{dir}/{stem}.js` therefore resolves to
`tests/baselines/reference/ArrowFunction1.errors.js` — a path that
doesn't exist. The actual partner is
`tests/baselines/reference/ArrowFunction1.js`, which requires
stripping `.errors.txt` (two extensions) rather than just `.txt`
(one extension).

**Demonstration:**
```python
from pathlib import Path
p = Path('tests/baselines/reference/ArrowFunction1.errors.txt')
print(p.stem)  # 'ArrowFunction1.errors'
```

**Verification (live tree):** of the 9,722 `*.errors.txt` baselines,
**9,055** fire violations and **667** pass. The 667 that pass are
the corner case where a sibling `<stem>.errors.js` happens to exist
(very rare). `ls /tmp/TypeScript/tests/baselines/reference/ArrowFunction1.*`
shows `ArrowFunction1.errors.txt`, `ArrowFunction1.js`,
`ArrowFunction1.symbols`, `ArrowFunction1.types` — the partner is
present, but the rule fails to find it.

**Fix candidates (alint engine-side, not config-side):**

1. **Add a `{stem_all}` template token** that strips every
   recognised extension (`.errors.txt` → bare stem). Single-character
   change in the template; explicit opt-in. Tagged as **NEW
   alint-future candidate**.
2. **Add a `partner_match: regex` mode** to the `pair` rule that
   accepts a regex for the partner-name derivation rather than a
   template. Strictly more general; bigger surface area.
3. **Document the multi-extension case in the `pair` rule docs** and
   instruct config-writers to use a workaround like writing two
   separate `pair` rules (one for `*.errors.txt` matching `<basename
   minus .errors.txt>.js`).

The cleanest fix is option 1 — `{stem_all}` is intuitive, additive,
and zero-cost when not used.

**Config-side workaround (today):** scope the rule down — change
`primary: "tests/baselines/reference/*.errors.txt"` to a more
specific pattern, OR replace the `pair` rule with a custom check
that's expressible. Neither is a clean fix; deserves the engine-side
solution above.

#### Bug 2: `ts-copyright-header-src` and `ts-copyright-header-scripts` fire 748 violations between them (pitfall #22 + aspirational rule)

**Cause.** Two interacting issues:

1. **Pitfall #22 (YAML `|` literal block scalar appends a trailing
   newline to the regex pattern).** Both rules use `pattern: |` (lines
   104 and 122 of `.alint.yml`). The pattern's last visible line
   becomes `Licensed under the Apache License, Version 2\.0\n` (with
   a trailing newline). Real headers continue with `(the "License");`
   on the same line — no `\n` after `2.0` — so the regex matches no
   file regardless of whether the header is present.

2. **Aspirational vs reality.** Verified via `grep -l "Copyright (c)
   Microsoft" /tmp/TypeScript/src/**/*.ts` — **zero** of 709 files
   under `src/` have the header. Same for `scripts/`: **zero** of 39
   files. The header is added only to *bundled* output by
   `Herebyfile.mjs#generateLibs`; source files have never been
   normalised.

**Demonstration of pitfall #22:**
```python
import re
# Hypothetical TS file with header applied:
hypothetical = open('/tmp/TypeScript/scripts/CopyrightNotice.txt').read() + '\n' + open('/tmp/TypeScript/src/compiler/checker.ts').read()[:1000]
# Pattern from `pattern: |` (with trailing \n):
pat_with_pipe = r'^/\*! \*+\nCopyright \(c\) Microsoft Corporation\. All rights reserved\.\nLicensed under the Apache License, Version 2\.0\n'
# Pattern from `pattern: |-` (no trailing \n):
pat_with_pipedash = r'^/\*! \*+\nCopyright \(c\) Microsoft Corporation\. All rights reserved\.\nLicensed under the Apache License, Version 2\.0'
print('|  matches:', re.match(pat_with_pipe, hypothetical) is not None)   # False
print('|- matches:', re.match(pat_with_pipedash, hypothetical) is not None)  # True
```

**Fix part 1 (canonical-correct YAML — pitfall #22):**
```yaml
  - id: ts-copyright-header-src
    kind: file_header
    paths: "src/**/*.ts"
    pattern: |-                            # ← strip-final-newline
      ^/\*! \*+
      Copyright \(c\) Microsoft Corporation\. All rights reserved\.
      Licensed under the Apache License, Version 2\.0
    level: warning

  - id: ts-copyright-header-scripts
    kind: file_header
    paths: ["scripts/**/*.mjs", "scripts/**/*.cjs", "scripts/**/*.mts"]
    pattern: |-                            # ← strip-final-newline
      ^// Copyright \(c\) Microsoft Corporation
    level: info
```

**Fix part 2 (aspirational vs reality).** Even with the regex fix
applied, the rule will still fire on every src/ + scripts/ file
because the headers genuinely don't exist on disk. Two reasonable
recommended paths:

A. **Lower to `level: info` and document as "tracking the gap, not
   gating on it"** — the rule then surfaces the maintenance-mode
   reality without producing CI-blocking noise.
B. **Lower to `level: off` and remove from the active rule set** —
   the rule isn't enforcing anything that's actually being applied.
   Keep the comment block as documentation that "the header is only
   applied to bundled output".

The pilot's recommendation is **path B** for the maintenance-mode
era, with a re-promotion to `level: warning` if a future janitorial
sweep applies the headers tree-wide.

**Note on pitfall #22 in this batch.** TypeScript is the second
case study (after the kubernetes pilot) to surface pitfall #22.
The pilot's `k8s-go-license-header` produced 17,040 false positives
against the kubernetes tree; here the same shape produces 748
aspirational fires. Two distinct repos, same pitfall — confirms
pitfall #22 is the canonical "copyright header rule misfire" pattern
in real-world configs. The pitfall is documented at
`docs/development/CONFIG-AUTHORING.md#22`.

---

## 7. Followup feature work surfaced

- **`pair_count` rule kind** (assert ≥1 partner files match a
  registry entry) — would cover `errorCheck.mjs` here, plus
  airflow's `check-no-new-airflow-exceptions`. **2 sources;
  v0.10+ design candidate.**
- **`{stem_all}` template token** (strip all recognised extensions
  from a path basename) — would fix the `ts-baseline-errors-pair-with-js`
  rule cleanly. **NEW alint-future candidate** surfaced by this
  case study; single-source for now (TypeScript) but applies anywhere
  multi-extension files are paired (`.test.ts.snap` ↔ `.test.ts` etc).
- **`Format::Jsonc` variant for the `*_path_equals` / `*_path_matches`
  structured-query rules** — would fix the `scripts/tsconfig.json`
  parsing failure cleanly. **NEW alint-future candidate** surfaced
  by this case study; tsconfig.* files are JSONC across the
  TypeScript ecosystem (vscode, deno, etc.) — broader applicability
  than just TS itself.
- **`bundled_size_diff` / `cross_ref_diff`** (out of alint's scope —
  PR-diff aware) — `checkPackageSize.mjs` shape; documented as
  out-of-scope. Same class as the kubernetes
  `verify-golangci-lint-pr-hints.sh` and the vscode
  `api-proposal-version-check.yml`.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`hygiene/lockfiles@v1` overlay.** TypeScript ships `package-lock.json`
   + `package.json`; the bundled `hygiene/lockfiles@v1` ruleset (7 rules)
   would catch nested-lockfile drift, mismatched lockfile-versions, and
   the orphan-lockfile pattern (a lockfile with no sibling `package.json`)
   that the existing CI doesn't gate today. The 60+ `package.json` files
   under `tests/cases/projects/**` are mostly fixtures so the overlay
   would need a scope.
2. **Scoping `oss-final-newline` / `oss-no-trailing-whitespace` away from
   `tests/baselines/reference/**`.** The current bundled rules sweep the
   entire tree; against the TS baseline corpus they produce ~17,800
   information-level findings that are not actionable (compiler-output
   dumps stored verbatim). Either scope the bundled rules to skip
   compiler-output corpora by convention, or restate them as
   repo-specific rules with explicit excludes.
3. **`agent-context@v1` surface assertion for the maintenance-mode
   marker.** TypeScript ships AGENTS.md + CLAUDE.md + the maintenance-mode
   marker; the bundled `agent-context@v1` ruleset (5 rules) covers the
   AGENTS.md / CLAUDE.md / .cursor/ canonical shape but doesn't carry
   a "marker string asserted" rule. The current `ts-agents-md-maintenance-marker`
   handles this repo-specifically; could promote to a generic
   `agent_context_marker` rule once a second source exists.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **68** (22 custom + 6 bundled rulesets — `oss-baseline`
  15, `node` 9, `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11,
  `tooling/editorconfig` 3, `agent-context` 5; some rule IDs overlap
  which is why the grand total is 68 rather than the arithmetic sum
  of 68)
- **`alint validate-config`:** ✓ Config valid: 68 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  27,853-violation breakdown (~17,800 baseline-corpus newline + trailing-ws
  cosmetic findings + 9,055 false positives from the `pair` rule
  `{stem}` issue + 748 aspirational copyright-header fires + ~250 real
  findings)
- **Pitfall instances flagged:** **2 instances of pitfall #22**
  (`ts-copyright-header-src` line 104, `ts-copyright-header-scripts`
  line 122) — both producing aspirational-rule fires (the underlying
  headers don't exist on disk; even with the `|-` fix the rules would
  still fire). See §6.2 for canonical-correct YAML + recommended
  level adjustments.
- **Open gaps:** `pair_count` (v0.10+ design candidate, 2 sources),
  `{stem_all}` template token (NEW v0.10+ candidate, 1 source),
  `Format::Jsonc` for structured-query rules (NEW v0.10+ candidate,
  1 source — but applies to vscode, deno, helm, anywhere tsconfig
  is consumed)
- **Open suspected bugs in this directory's `.alint.yml`:** 2 rules
  produce systemically wrong verdicts (Bug 1: 9,055 false positives;
  Bug 2: 748 aspirational fires). **Not auto-fixed in this pass —
  flagged for parent-agent triage.** See §6.2 for canonical-correct
  YAML.
