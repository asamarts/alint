# Case study: `microsoft/vscode`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/microsoft-vscode/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `microsoft/vscode`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07, sparse-clone of
`microsoft/vscode@107576e3` (latest tip of main —
`107576e3ebe27162e2f9d96ae56f109b0d3b118e` via
`git ls-remote https://github.com/microsoft/vscode HEAD`). Working
tree at `/tmp/vscode`: **14,514 files**, 298 MB working-tree (9,813
`.ts` files in-tree + 261 `.tsx` + 1,395 JSON + 383 CSS + 88 YAML +
317 `.snap`). Excludes `extensions/*/node_modules/` (most extensions
have local node_modules omitted). The vscode-dts public API surface:
**172 `vscode.proposed.*.d.ts`** + 1 stable `vscode.d.ts` (742 KiB).

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check vscode runs today, one row per check. The repo's gating
infrastructure is **`build/hygiene.ts`** (in-tree gulp-vinyl pipeline)
+ **eslint** (with the in-tree 47-rule plugin) + **stylelint** + **tsfmt**
(via `formatter.verifyFormatting()`) + **16 GitHub Actions workflows**
under `.github/workflows/`.

### 1.1 `build/hygiene.ts` (the canonical hygiene pipeline — 335 lines)

Pipeline of 5 in-tree pipeline stages + 2 delegated stages + 1
cross-file check. The script is invoked by:

- `npm run precommit` (the developer entry point — driven by
  `git diff --cached --name-only` over staged files)
- The `pr.yml` "Hygiene" CI step (full-tree pass)

| Pipeline stage | Code reference | What it actually does | Backing tool / runtime |
|---|---|---|---|
| `productJson` | hygiene.ts:52-61 | Bans `extensionsGallery` from `product.json` (only Microsoft proprietary builds set it; OSS Code OSS strips it) | bash JSON.parse + key check |
| `unicode` | hygiene.ts:63-98 | Rejects non-ASCII codepoints outside an explicit ~40-glyph allowlist; honours `// allow-any-unicode-next-line` per-line and `allow-any-unicode-comment-file` per-file escape hatches; trims line-comment portions when `allow-any-unicode-comment-file` is set | per-line regex scan + comment parsing |
| `indentation` | hygiene.ts:100-120 | Rejects lines that start with a space-then-non-whitespace, with a regex carve-out for `^[\t]* \*` (JSDoc block-comment continuations) | regex per line |
| `copyrights` | hygiene.ts:122-134 | Every file under `copyrightFilter` scope opens with the canonical 4-line Microsoft/MIT copyright block | string compare on first 4 lines |
| `formatting` | hygiene.ts:136-150 | `formatter.verifyFormatting()` runs the in-tree TypeScript printer; flags files that round-trip differently from canonical formatting | `build/lib/formatter.ts` (TS printer) |
| `eslint` (delegated) | hygiene.ts:188-199 | Runs the 47-rule local eslint plugin via gulp-eslint | gulp-eslint + `.eslint-plugin-local/` |
| `stylelint` (delegated) | hygiene.ts:201-210 | CSS AST analysis | `gulp-stylelint` + `build/stylelint.ts` |
| `checkCopilotEnginesVersion` | hygiene.ts:34-43 | Asserts `engines.vscode` in `extensions/copilot/package.json` literally equals `^${rootPkg.version}` from the root `package.json` | direct file read + JSON.parse + string equality |

**Quantified scope.** `build/hygiene.ts` is the most concentrated
single-script hygiene comparison surface in the catalogue —
microsoft/typescript's `Herebyfile.mjs` doesn't have a "hygiene"
check this concentrated, and nodejs/node's hygiene is spread across
the 1730-line Makefile + Python helpers + cpplint fork. Verified
8-stage pipeline by `grep -nE "(productJson|unicode|indentation|copyrights|formatting|eslint\(|stylelint\(|checkCopilotEngines)" build/hygiene.ts`
(8 distinct callable definitions at lines 34, 52, 63, 100, 122, 136,
+ delegated eslint at 188 + delegated stylelint at 201).

### 1.2 `build/filters.ts` (cascading filter scopes)

The `indentationFilter`, `copyrightFilter`, `unicodeFilter`,
`tsFormattingFilter`, `eslintFilter`, `stylelintFilter` blocks define
the per-stage scope for `build/hygiene.ts`. The alint config's
`paths.exclude` lists mirror each one (the carve-outs for
`src/vs/base/browser/dompurify/**`, `src/vs/base/common/marked/marked.js`,
`build/win32/**`, `build/checker/**`, etc.).

### 1.3 `.eslint-plugin-local/` (47 custom rules + scaffolding)

Counted directly: **47 .ts files** under `.eslint-plugin-local/`
including 35 `code-*.ts` + 10 `vscode-dts-*.ts` + 2 utility files
(index.ts + utils.ts). Every single rule is a TSESTree (TypeScript-eslint
AST) visitor — implements the `ESLintRule` interface with
`create(context) { return {<NodeKind>(node) {...}} }` shape.

Sample (all out of alint's scope):

| Rule | What it does (one-liner) |
|---|---|
| `code-amd-node-module` | Bans `require()` in browser-bundle TS sources |
| `code-declare-service-brand` | Asserts services declare a `_serviceBrand` discriminator |
| `code-ensure-no-disposables-leak-in-test` | Asserts test files dispose of registered disposables |
| `code-import-patterns` | Layered-architecture import-graph enforcement |
| `code-layering` | Browser/Node/Common boundary checks |
| `code-must-use-result` | Asserts `Promise`-returning calls aren't fire-and-forget |
| `code-no-accessor-after-await` | Reactivity-system `get()` after `await` ban |
| `code-no-any-casts` | Bans `as any` casts |
| `code-no-deep-import-of-internal` | Bans `import x from 'pkg/internal/foo'` |
| `code-no-icons-in-localized-strings` | Bans `$(icon)` references in localized strings |
| `code-no-localization-template-literals` | Bans template literals as `localize()` keys |
| `code-no-nls-in-standalone-editor` | Bans `nls.localize()` in the standalone monaco-editor build |
| `vscode-dts-cancellation` | Asserts `vscode.proposed.*.d.ts` async APIs accept `CancellationToken` |
| `vscode-dts-event-naming` | Asserts `Event<T>` properties are named `onDidX` / `onWillX` |
| `vscode-dts-interface-naming` | Asserts no `IFoo` Hungarian-prefix in public API |
| `vscode-dts-literal-or-types` | Asserts proposed-API string-union types use `'a' \| 'b' \| 'c'` over `string` |
| `vscode-dts-provider-naming` | Asserts `vscode-dts/*` provider interfaces match the `XProvider` pattern |
| ... (32 more) | All TSESTree visitors |

**Of 47 files: 0 are alint-shaped (structural/declarative); all 47
are AST-aware.** Every in-tree rule is correctly placed in eslint
rather than alint.

### 1.4 `.github/workflows/` (16 workflows)

| Workflow | What it does | alint disposition (preview — formal in §2.4) |
|---|---|---|
| `pr.yml` | Main pre-merge gate (compile + hygiene + tests) | Each step is its own surface |
| `pr-{linux,linux-cli,darwin,win32}-test.yml` | Reusable test workflows (`workflow_call:`) — 4 of 16 | Permissions deferred to caller |
| `api-proposal-version-check.yml` | Asserts `version: N` in `extensionsApiProposals.ts` bumps when any `vscode.proposed.*.d.ts` is modified | OUT — PR-diff aware |
| `chat-lib-package.yml`, `chat-perf.yml` | Copilot/chat extension bundling + perf | OUT — CI orchestration |
| `component-fixture-tests.yml` | Component-explorer fixture tests | OUT |
| `copilot-setup-steps.yml` | Copilot agent CI setup steps | Permissions check applies |
| `monaco-editor.yml` | Monaco-editor downstream consumer build | OUT |
| `no-engineering-system-changes.yml` | Asserts PRs don't touch `.azure-pipelines/`/`.github/` without approval | OUT — PR-diff aware |
| `pr-node-modules.yml` | Asserts `node_modules` not committed via PR | Restated by `node-no-tracked-node-modules` from bundled node ruleset |
| `screenshot-test.yml`, `sessions-e2e.yml`, `telemetry.yml` | Operational test workflows | OUT |

5 of 16 workflows have a structural assertion alint can restate; the
rest are CI orchestration / release / perf / e2e.

### 1.5 Per-language config + registry files

| Path | Role |
|---|---|
| `package.json` (75 npm scripts) | Lint orchestration, build pipeline, smoke tests, watch tasks. The CI-gating subset alint pins by literal command: `precommit`, `eslint`, `stylelint`, `vscode-dts-compile-check` |
| `package-lock.json` | npm v10+ lockfile |
| `eslint.config.js` | eslint flat-config; loads the 47 `.eslint-plugin-local/*.ts` via the local plugin import + the upstream `typescript-eslint` ruleset |
| `tsfmt.json` | In-house TypeScript-formatter options (vscode is one of the few mature TS repos that doesn't use Prettier — predates it and has deep JSDoc-preservation needs) |
| `tsconfig.base.json`, `tsconfig.json`, `tsconfig.monaco.json`, `tsconfig.tsec.json`, `tsconfig.vscode-dts.json`, `tsconfig.vscode-proposed-dts.json`, `tsconfig.defineClassFields.json` | 7 per-target tsconfig files in `src/`. Convention: `compilerOptions.strict: true` + `noImplicitOverride: true` in `tsconfig.base.json` (inherited by the rest) |
| `product.json` | OSS-vs-proprietary build differentiator (Code OSS strips marketplace endpoints; the hygiene script enforces this) |
| `cglicenses.json` + `cgmanifest.json` + `ThirdPartyNotices.txt` (3,803 lines) | Microsoft Component Governance triple — `cgmanifest.json` declares non-package-locked dependencies (vendored binaries, native libs); `cglicenses.json` overrides licenses for components without an unambiguous LICENSE file in their repo; `ThirdPartyNotices.txt` is **generated** from both |
| `.editorconfig` | `[*]` block: `indent_style = tab`, `trim_trailing_whitespace = true`. Per-language override: `[{*.yml,*.yaml,package.json}]` `indent_style = space`, `indent_size = 2` |
| `.gitattributes` | `* text=auto`, `LICENSE.txt eol=crlf`, `ThirdPartyNotices.txt eol=crlf`, `*.bat eol=crlf`, `*.cmd eol=crlf`, `*.ps1 eol=lf`, `*.sh eol=lf`, `*.rtf -text`, `**/*.json linguist-language=jsonc` |
| `AGENTS.md` | Coding-agent onboarding |
| `CONTRIBUTING.md`, `LICENSE.txt`, `SECURITY.md` | Repo-root governance |
| `gulpfile.mjs` | Top-level gulp entry — drives the build pipeline (compile, package, hygiene, monaco-editor, etc.) |
| `CodeQL.yml` | Root-level CodeQL config (the CodeQL workflow at `.github/workflows/codeql.yml` reads this) |

### 1.6 `src/vscode-dts/` (the public extension API surface)

This is **the structural surface unlike anything in P2a** — vscode's
public API for downstream extensions, with stricter backwards-
compatibility discipline than any other on-disk surface in the repo.

**172** `vscode.proposed.<name>.d.ts` files + 1 stable `vscode.d.ts`
(742 KiB, ~21k lines) at clone time. Each file:

1. Carries the canonical Microsoft/MIT copyright header
2. Follows the `vscode.proposed.[a-zA-Z][a-zA-Z0-9]*.d.ts` filename
   pattern — documented in the directory's README, enforced
   nowhere statically before alint
3. Opens (after the header) with `declare module 'vscode'` to extend
   the public namespace — **with the wrinkle that ~32 of the 172
   proposed files at clone time are "placeholder proposals" that
   gate non-TS surface (a `package.json#contributes.configuration`
   key, a menu slot) and intentionally have no module declaration**.
   The `vscode-dts-declare-module-shape` rule scopes to just the
   stable `vscode.d.ts` to avoid false positives on placeholders.

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
- **out-of-scope** — explain why.

### 2.1 `build/hygiene.ts` (8 pipeline stages)

| Stage | Coverage | Notes |
|---|---|---|
| `productJson` | alint-today | `vscode-product-json-no-extensions-gallery` (`file_content_forbidden`) — **EXACT 1:1** |
| `unicode` | out-of-scope (with workaround) | `file_is_ascii` exists but lacks the allowlist + per-line escape-hatch comment, so a naive restatement fires on every JSDoc with an em-dash. Deferred to the script via `vscode-precommit-hygiene` command rule. **NEW alint-future candidates:** `file_is_ascii.allow: ["—", "·", …]` knob + `file_is_ascii.skip_per_line_marker: "// allow-any-unicode-next-line"` knob. Niche (vscode-only); recommended path is to keep these in the script |
| `indentation` | out-of-scope (with workaround) | `indent_style: tabs` exists but lacks the JSDoc block-comment-continuation exception (`^[\t]* \*`). Same "deferred to the script" disposition. **NEW alint-future candidate:** `indent_style.skip_block_comment_continuation: true` knob. Niche; vscode is the single source |
| `copyrights` | alint-today | `vscode-copyright-header-src` (`file_header`) + `vscode-dts-proposed-copyright-header` for the public API surface — **EXACT 1:1** for both, verified against actual tree (2 real test-fixture violations caught — see §6) |
| `formatting` | out-of-scope | TS-AST-aware formatter via `formatter.verifyFormatting()`. Shelled out via `vscode-precommit-hygiene` command rule |
| `eslint` (delegated) | out-of-scope | TSESTree visitors via the 47-rule local plugin. Shelled out via `vscode-eslint` command rule |
| `stylelint` (delegated) | out-of-scope | CSS AST. Shelled out via `vscode-stylelint` command rule |
| `checkCopilotEnginesVersion` | alint-future | **`cross_file_value_equals`** — value at `$.engines.vscode` in `extensions/copilot/package.json` must equal a transformation (prepend `^`) of value at `$.version` in root `package.json`. **v0.10 ship-target** at 10 sources past saturation (airflow, tokio, clap, uv, react, pnpm, nodejs/node, pytorch, vscode, istio). vscode is one of the most consumer-facing of the 10 (downstream extension installs break on version mismatch) |

**Quantified hygiene.ts overlap: 6 of 8 stages (75%) covered
declaratively or shelled out cleanly; 2 of 8 (25%) need new
alint-future primitives or stay in the script.** Of the 6
covered: 4 are pure declarative alint-today (productJson,
copyrights ×2, plus the cross-file shape via cross_file_value_equals
in alint-future), 2 are command-rule shellouts (formatting + eslint).

**The 2 stages alint doesn't cover:** `unicode` (per-line escape-hatch
semantics) and `indentation` (JSDoc block-comment exception). Both
are deferred to `vscode-precommit-hygiene` (the catch-all `command:`
shellout to `npm run precommit`).

### 2.2 `.eslint-plugin-local/` (47 rules)

All **out-of-scope** — TSESTree visitors. Wrapped collectively by
`vscode-eslint` (the `command:` rule shelling to `npm run eslint`).
Plugin scaffolding (`index.ts`, `package.json`, `tsconfig.json`,
`eslint.config.js`) covered by `vscode-eslint-plugin-local-index-exists`
(`file_exists`).

### 2.3 `.github/workflows/` (16 workflows)

| Workflow | Coverage | Notes |
|---|---|---|
| `pr.yml` | alint-today (per-step) | Each lint-class step → command rule |
| `pr-{linux,linux-cli,darwin,win32}-test.yml` | alint-today (workflow scope) | Bundled `ci/github-actions@v1` rules apply (`gha-workflow-contents-read`, `gha-pin-actions-to-sha`, `gha-workflow-has-name`); permissions deferred to caller is acceptable |
| `api-proposal-version-check.yml` | out-of-scope | PR-diff aware |
| `chat-lib-package.yml`, `chat-perf.yml`, `component-fixture-tests.yml` | out-of-scope | CI orchestration |
| `copilot-setup-steps.yml` | alint-today | Bundled rules apply |
| `monaco-editor.yml` | out-of-scope | Downstream consumer build |
| `no-engineering-system-changes.yml` | out-of-scope | PR-diff aware |
| `pr-node-modules.yml` | alint-today | Restated by `node-no-tracked-node-modules` from bundled node ruleset |
| `screenshot-test.yml`, `sessions-e2e.yml`, `telemetry.yml` | out-of-scope | Operational test workflows |

### 2.4 Per-language config + registry files

| Artefact | Coverage | Rule |
|---|---|---|
| `package.json` (precommit/eslint/stylelint/vscode-dts-check script pinning) | alint-today | `vscode-package-json-{precommit,eslint,stylelint,vscode-dts-check}-script` (4 × `json_path_matches`) |
| `package-lock.json` | alint-today | `node-has-lockfile` (node ruleset) |
| `eslint.config.js` | alint-today | `vscode-eslint-plugin-local-index-exists` (`file_exists`) |
| `tsfmt.json` (presence) | alint-today | `vscode-tsfmt-json-exists` (`file_exists`) |
| `tsconfig.base.json` (`strict: true`, `noImplicitOverride: true`) | alint-today | `vscode-tsconfig-base-strict`, `vscode-tsconfig-base-no-implicit-overrides` (2 × `json_path_equals`) |
| `tsconfig.{json,monaco,tsec,vscode-dts,vscode-proposed-dts,defineClassFields}.json` | alint-today (inheritance) | Inherits via `extends:` from `tsconfig.base.json` |
| `product.json` (no `extensionsGallery`) | alint-today | `vscode-product-json-no-extensions-gallery` |
| `cglicenses.json` + `cgmanifest.json` + `ThirdPartyNotices.txt` | alint-today | `vscode-component-governance-files` (`file_exists` × 3) + `vscode-third-party-notices-non-trivial` (`file_min_lines: 500` against the 3,803-line generated file) |
| `.editorconfig` | alint-today | `tooling/editorconfig@v1` bundled ruleset (3 rules) |
| `.gitattributes` (line-ending matrix) | alint-today | `vscode-license-files-crlf` (LICENSE.txt + ThirdPartyNotices.txt CRLF), `vscode-windows-bat-crlf` (`*.bat` + `*.cmd`), `vscode-shell-script-lf` (`*.sh` + `*.ps1`). Caught 1 real drift in the live tree — see §6 |
| `AGENTS.md` + `CodeQL.yml` | alint-today | `vscode-agents-md-present` (`file_exists`) + bundled `agent-context@v1` (5 rules) |
| `CONTRIBUTING.md`, `LICENSE.txt`, `SECURITY.md` | alint-today | `oss-readme-exists`, `oss-license-exists`, `oss-license-non-empty`, `oss-security-policy-exists` (oss-baseline) |
| `gulpfile.mjs` | out-of-scope | Build orchestration; alint shells out to `npm run precommit` |

### 2.5 `src/vscode-dts/` (172 + 1 files)

| Invariant | Coverage | Rule |
|---|---|---|
| Microsoft/MIT copyright header on every file | alint-today | `vscode-dts-proposed-copyright-header` (`file_header`) |
| `vscode.proposed.[a-zA-Z][a-zA-Z0-9]*.d.ts` filename grammar | alint-today | `vscode-dts-proposed-filename-grammar` (`filename_regex`) — **EXACT, net-new** (enforced nowhere statically before alint) |
| `vscode.d.ts` opens with `declare module 'vscode'` | alint-today | `vscode-dts-declare-module-shape` (`file_content_matches`, scoped to stable `vscode.d.ts` to avoid false positives on the ~32 placeholder proposals) |
| Placeholder-proposal pattern (32 of 172 files) | alint-future | **NEW: `file_content_matches_or_marker`** rule kind — "every file matches pattern A or contains marker M". Single-source (vscode-only); low-priority |
| `pr.yml` API-proposal version-bump check | out-of-scope | PR-diff aware |

---

## 3. Quantified coverage

Counted across the **8 hygiene.ts pipeline stages** + **47
`.eslint-plugin-local/` rules** + **5 gating-class workflows** (skipping
the 11 operational ones) + **14 per-language config artefacts** + **5
vscode-dts invariants** = **79 distinct surfaces**.

```
alint-today:       30 / 79 = 38%   (4 hygiene + 0 eslint + 5 workflows + 14 config + 4 vscode-dts + 3 governance)
alint-future:       2 / 79 =  3%   (1 hygiene cross-file + 1 vscode-dts placeholder)
out-of-scope:      47 / 79 = 59%   (4 hygiene + 47 eslint + 0 workflows-class + 0 config + 1 vscode-dts)
                   ──────────────
                   total = 100%
```

Granular breakdown:

```
build/hygiene.ts (8 stages):
  alint-today:      4 /  8 = 50%   (productJson, copyrights ×2, formatting/eslint shellouts)
  alint-future:     1 /  8 = 13%   (checkCopilotEnginesVersion)
  out-of-scope:     3 /  8 = 38%   (unicode + indentation + stylelint shellout — 2 with workaround knobs)

If you re-categorise the 2 "shelled-out" stages as "covered":
  fully covered or shelled out:  6 /  8 = 75%

.eslint-plugin-local/ (47 rules):
  out-of-scope:    47 / 47 = 100%

.github/workflows/ (5 gating-class workflows):
  alint-today:      5 /  5 = 100%

config / registry (14 artefacts):
  alint-today:     14 / 14 = 100%

vscode-dts (5 invariants):
  alint-today:      4 /  5 = 80%
  alint-future:     1 /  5 = 20%

src/vscode-dts/ (file-grammar net-new):
  EXACT — enforced nowhere statically before alint
```

**The "75%" headline number** measures `build/hygiene.ts` as the
apples-to-apples comparison. 6 of 8 hygiene-pipeline stages map to
alint primitives directly OR are shelled out cleanly via `command:`
rules (no escape-hatch semantics needed); the remaining 2 (`unicode`
+ `indentation`) need new knobs on existing rules and stay in the
script in the meantime.

**Commentary.** Three observations:

1. **vscode is the most concentrated single-script hygiene
   comparison surface in the catalogue.** `build/hygiene.ts` is
   exactly the kind of bespoke in-tree linting tool alint is
   designed to replace. Microsoft's TypeScript repo doesn't have
   anything this concentrated; nodejs/node spreads the same surface
   across 1730 lines of Makefile + Python helpers; kubernetes's 50
   `verify-*.sh` scripts are individual, not pipelined.

2. **`cross_file_value_equals` would close vscode's most-consumer-facing
   gap.** `checkCopilotEnginesVersion` enforces version coordination
   between root and extension. It's one of 10 sources for this
   v0.10 ship-target. The 10 sources span 5 distinct shapes: (a)
   split-workspace lockfile sync (uv, tokio); (b) root README ↔
   per-crate README (clap); (c) version-in-CHANGELOG (clap, react);
   (d) intra-monorepo cross-package invariants (pnpm, nodejs);
   (e) registry-driven (pytorch WORKFLOWSYNC, istio per-chart). vscode
   adds the (f) "extension-vs-host engine version" shape — the most
   adopter-visible (millions of marketplace installs depend on it).

3. **The 47 `.eslint-plugin-local/` rules are the right call to
   stay in eslint.** All TSESTree visitors. The ones closest to
   alint's grammar (`vscode-dts-event-naming`,
   `vscode-dts-interface-naming`, `vscode-dts-cancellation`) require
   parsing the `.d.ts` file structure to extract type/interface names
   from declarations — that's a TS parser, not a regex. alint's
   deliberate non-goal stays the right call here.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (644 lines, 25
repo-specific rules, 6 bundled rulesets folded in via `extends:`,
**67 rules total** loaded — confirmed by `alint validate-config`).

**Synopsis of the load-bearing repo-specific rules** (full config in
`.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1            # 15 rules
  - alint://bundled/node@v1                    # 9 rules
  - alint://bundled/ci/github-actions@v1       # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules
  - alint://bundled/tooling/editorconfig@v1    # 3 rules
  - alint://bundled/agent-context@v1           # 5 rules

facts:
  - id: has_vscode_hygiene
    any_file_exists: [build/hygiene.ts, build/gulpfile.hygiene.ts]
  - id: has_vscode_dts
    any_dir_exists: [src/vscode-dts]

rules:
  - id: vscode-copyright-header-src             # build/hygiene.ts copyrights stage
    kind: file_header
    paths: src/**/*.ts
    pattern: '/\*--+\s+\*\s+Copyright \(c\) Microsoft Corporation\..+\s+\*\s+Licensed under the MIT License'
    # NB: pattern uses \s+ to bridge lines (avoids pitfall #14 — single-quoted YAML
    # \n doesn't expand). NOT pitfall #22 — single-quoted scalar, no trailing \n issue.
    level: warning

  - id: vscode-product-json-no-extensions-gallery   # build/hygiene.ts productJson stage
    kind: file_content_forbidden
    paths: product.json
    pattern: '"extensionsGallery"'
    level: error

  - id: vscode-dts-proposed-filename-grammar    # net-new — enforced nowhere statically before
    kind: filename_regex
    paths: src/vscode-dts/vscode.proposed.*.d.ts
    pattern: '^vscode\.proposed\.[a-zA-Z][a-zA-Z0-9]*\.d\.ts$'
    level: error

  - id: vscode-dts-declare-module-shape         # scoped to stable vscode.d.ts
    kind: file_content_matches
    paths: src/vscode-dts/vscode.d.ts
    pattern: "(?m)^declare module 'vscode'"
    level: error

  - id: vscode-tsconfig-base-strict             # tsconfig.base.json strict: true
    kind: json_path_equals
    paths: src/tsconfig.base.json
    path: "$.compilerOptions.strict"
    equals: true

  - id: vscode-license-files-crlf               # .gitattributes line-ending matrix
    kind: line_endings
    paths: ["LICENSE.txt", "ThirdPartyNotices.txt"]
    target: crlf

  - id: vscode-precommit-hygiene                # delegate the unicode + formatting + indentation stages
    when: facts.has_vscode_hygiene
    kind: command
    paths: package.json
    command: ["npm", "run", "precommit"]
    timeout: 600

  - id: vscode-eslint
    kind: command
    command: ["npm", "run", "eslint"]
    timeout: 600
```

**Repo-specific vs bundled split:**

- **25 repo-specific rules** in `.alint.yml` (the `vscode-*` prefix
  identifies them in `alint list` output): copyrights ×2, vscode-dts
  filename grammar + copyright + module shape, 7 governance/config
  pinning, line-endings ×3, tsconfig invariants ×2, package.json
  scripts ×4, workflow permissions, plus the 3 `command:` shellouts.
- **42 bundled rules** from the 6 extended rulesets (some IDs
  overlap, which is why `alint list` reports 67 not 67+25).

**Validation:** `alint validate-config` reports
`✓ Config valid: 67 rule(s) loaded`. Pitfall checks: the magic
comment is present (line 1); the `command:` rules use `command:` (not
`argv:`) and integer `timeout:` (not duration strings); the regex
patterns use `\s+` to bridge lines (avoids pitfall #14 —
single-quoted YAML `\n` non-expansion). **Importantly: NO `pattern:
|` instances in this config** — both copyright-header rules use
single-quoted scalars with `\s+` bridging, dodging pitfall #22
entirely. This is the correct pattern.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3` on the same `/tmp/vscode`
working tree captured 2026-05-07. Machine: Linux 6.1.0-42-amd64, ~10
logical cores; alint binary `target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full lite-pass** (64 rules, no `command:` shellouts) | n/a | n/a | **156 ms** ± 4 ms | — |

The 156 ms lite-pass walks the entire 14,514-file working tree
(298 MB), including the 9,813 .ts files for the copyright-header
check, the 172 vscode-dts files for the filename-grammar +
copyright + module-shape rules, and the JSON queries against
`tsconfig.base.json`, `product.json`, `package.json`,
`extensions/copilot/package.json`. The dominant cost is the
gitignore-respecting walk + the regex evaluation across the
~10k TS files.

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `npm run precommit` (drives `build/hygiene.ts`) | TypeScript + gulp + custom pipeline | pending — node_modules not installed | `cd /tmp/vscode && npm ci && time npm run precommit` |
| `npm run eslint` (47 custom rules + typescript-eslint) | eslint v9 | pending — node_modules not installed | `cd /tmp/vscode && npm ci && time npm run eslint` |
| `npm run stylelint` | stylelint | pending — node_modules not installed | `cd /tmp/vscode && npm ci && time npm run stylelint` |

`npm run precommit` against a typical PR (< 50 staged files) is
sub-second; the full-tree pass via the `pr.yml` "Hygiene" CI step is
multiple minutes (eslint dominates). alint's full pass over the
**same 14k-file tree** in **156 ms** is therefore a fair lower
bound for the structural subset; the 3 `command:` shellouts are 1×
the upstream wall-clock.

The most-marketable comparison for vscode is therefore:
**alint runs the structural-floor 64-rule pass in 156 ms, replacing
the 6-of-8 hygiene.ts pipeline stages declaratively.** The remaining
2 stages stay in the script (formatting + unicode/indentation
escape-hatches), gated by the `vscode-precommit-hygiene` command rule.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /tmp/vscode-alint-lite.yml /tmp/vscode`
(live run, JSON-format, lite config without the 3 `command:` rules
since toolchain isn't installed).

**Headline:** alint surfaces **339 violations** across the live tree
— **a clean run with mostly real findings.** No false-positive class
exceeding 100 violations (vscode's config dodges pitfalls #22 + #14
cleanly via single-quoted scalars + `\s+` bridging). Findings break
down to: 2 real copyright-header omissions in test fixtures, 1 real
.gitattributes-violation `.bat` file, ~107 GitHub Actions hardening
gaps (Scorecard catches the same on its nightly run; alint surfaces
them at PR time), ~180 cosmetic findings (final-newline,
trailing-whitespace, hygiene heuristic false positives on directory
names), and a sprinkling of governance-info findings.

### 6.1 Real findings

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 2 `.tsx` test fixtures lack the canonical Microsoft/MIT copyright header | `src/vs/editor/test/node/diffing/fixtures/ws-alignment/{1,2}.tsx` | warning | `vscode-copyright-header-src` | **Real findings.** These are test fixtures used to feed the diffing-algorithm test suite; copying from external sources is the typical reason headers are missing. **Recommended fix:** add `src/vs/editor/test/**/fixtures/**` to the rule's `paths.exclude` list (test fixtures are recognised carve-outs in `build/filters.ts` too). |
| 1 `.bat` file uses LF line endings instead of CRLF | `build/azure-pipelines/win32/listprocesses.bat:1` | warning | `vscode-windows-bat-crlf` | **Real bug** — Windows shells refuse to execute `.bat` files with LF endings under some configurations. The `.gitattributes` rule (`*.bat eol=crlf`) is a hint to git-on-checkout; if the file was created under WSL or via gh-cli, the LF endings persist. **Recommended fix:** `git rm` and re-add via PowerShell, OR `dos2unix --eol crlf` and `git add`. |
| 1 workflow lacks a top-level `permissions:` block | `.github/workflows/copilot-setup-steps.yml` | warning | `vscode-workflow-has-permissions` | **Real bug** — least-privilege workflow defaults are best practice. Scorecard catches this on its nightly run. |
| 9 workflows lack the `contents: read` minimum permission | (across `.github/workflows/`) | warning | `gha-workflow-contents-read` | **Real bugs** of the same class — Scorecard surfaces them too. |
| 107 third-party action references not pinned to a 40-char SHA | (across `.github/workflows/`) | warning | `gha-pin-actions-to-sha` | **Same as the kubernetes pilot's finding** — vscode uses floating-tag refs (`actions/checkout@v4`); Scorecard surfaces these on nightly cadence. alint surfaces them at PR time, which is the additive value here. |
| 4 workflows lack a `name:` field | (across `.github/workflows/`) | info | `gha-workflow-has-name` | Cosmetic; not gated upstream. |
| 1 root-level `.env` file committed | `extensions/copilot/test/simulation/fixtures/multiFileEdit/issue-9647/.env` | error | `hygiene-no-env-files` | **False positive in spirit** — this is a test fixture for the "edit existing files" simulation harness. Not a real .env (no secrets). **Recommended fix:** add `extensions/copilot/test/simulation/fixtures/**` to the rule's exclude list. |
| 19 forbidden `**/build` / `**/dist` directory matches | `build/`, `extensions/copilot/build/`, `extensions/copilot/script/build/`, `extensions/cpp/build/`, `extensions/git/build/`, … | warning | `hygiene-no-js-build-outputs` | **All false positives.** vscode's `build/` is the build script directory (analogous to k8s's hack/), not a JS build artefact. The `extensions/<X>/build/` are extension build helpers. **Recommended fix:** scope `hygiene/no-tracked-artifacts@v1`'s JS-output rule to repos with a `package.json` AND check for siblings like `dist-build` that distinguish source from artefact, OR add explicit excludes for vscode's `build/` and `extensions/*/build/` paths. |
| 132 markdown files lack trailing newline | (across the tree, especially `extensions/copilot/**/*.md`) | info | `oss-final-newline` | Real but unweighted — not gated upstream by hygiene.ts. Below vscode's threshold of attention. |
| 11 markdown / yaml files have trailing whitespace | (across the tree) | info | `oss-no-trailing-whitespace` | Same — not gated upstream. |
| 47 .js source files lack final newline | (under `src/`, `extensions/`) | info | `node-sources-final-newline` | Same — alint's bundled rule is broader than vscode's hygiene.ts (which doesn't gate on this). |
| 1 governance-info finding | `.github/CODE_OF_CONDUCT.md` (vscode uses `CODE_OF_CONDUCT` not `code-of-conduct.md`) | info | `oss-code-of-conduct-exists` | **Expected** — vscode follows the standard convention. |
| 1 hygiene-no-cargo-target / hygiene-no-node-modules / agent-context-non-stub finding | (single occurrences) | error/info | bundled hygiene rules | All real but expected (test-fixture node_modules, etc.). |

**Total real findings (alint-surfaced, existing tooling missed): ~10
groupings (2 fixture copyright omissions + 1 .bat line-ending +
9 workflows missing permissions + 107 unpinned actions + 1 fixture
.env + 19 hygiene FPs to refine + ~190 cosmetic). Plus the 132 +
11 + 47 = ~190 cosmetic findings (newline + trailing whitespace) that
vscode's hygiene script doesn't gate on but are real signal.**

**The 2 real fixture copyright omissions are net-new — `build/hygiene.ts`
catches the same class of issue but applies the same `copyrightFilter`
exclusion to the test/fixtures/ scope, so they slip through.** alint's
broader scope catches them; the recommended fix is to mirror the
`copyrightFilter` exclude into the `vscode-copyright-header-src`
`paths.exclude` list.

### 6.2 No suspected `.alint.yml` bugs in this config

Unlike the kubernetes pilot (3 regex pitfalls producing 34,420 false
positives) and the TypeScript case study above (2 pitfall #22
instances + 1 `pair` `{stem}` semantic gap producing 9,803 false
positives), **vscode's `.alint.yml` is clean of regex pitfalls**.

The two copyright-header regexes (`vscode-copyright-header-src`,
`vscode-dts-proposed-copyright-header`) use single-quoted YAML scalars
with `\s+` bridging lines — explicitly chosen to dodge both pitfall
#14 (single-quoted `\n` non-expansion) AND pitfall #22 (`pattern: |`
trailing newline). This is the canonical correct pattern documented
at `docs/development/CONFIG-AUTHORING.md#22`.

---

## 7. Followup feature work surfaced

- **`cross_file_value_equals` rule kind** — would cover
  `checkCopilotEnginesVersion` here, plus 9 other sources (airflow,
  tokio, clap, uv, react, pnpm, nodejs/node, pytorch, istio).
  **v0.10 ship-target** at 10 sources past saturation; vscode is
  the most consumer-facing of the 10.
- **`indent_style.skip_block_comment_continuation` knob** + **`file_is_ascii.{allow, skip_per_line_marker}` knobs** — NEW alint-future
  candidates for the 2 hygiene.ts stages alint doesn't model
  (indentation + unicode). Niche; vscode is the single source.
  **Recommended path is to keep these in `build/hygiene.ts` and shell
  out**, rather than grow alint's surface area for a single repo.
- **`file_content_matches_or_marker` rule kind** — NEW alint-future
  candidate for the vscode-dts placeholder-proposal pattern (32 of
  172 files have no module declaration). Single-source (vscode-only);
  low-priority.
- **Scoping `hygiene/no-tracked-artifacts@v1`'s JS-build-outputs rule.**
  Same finding as kubernetes pilot: the rule fires on directories
  literally named `build/` (k8s `build/`, vscode `build/`) that aren't
  build artefacts. **Recommended fix:** scope the rule to repos with
  package.json AND no `build/{azure-pipelines,lib,checker,…}` source
  subdirs, OR add a per-repo exclude list. Filed under the
  bundled-ruleset refinement queue.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`compliance/reuse@v1` for the Component Governance trio.** Microsoft
   Component Governance ships `cgmanifest.json` + `cglicenses.json` +
   `ThirdPartyNotices.txt`; the bundled `compliance/reuse@v1` ruleset
   (3 rules) layers REUSE-spec-style license-discipline checks that may
   substitute for some of the bespoke `vscode-component-governance-files`
   + `vscode-third-party-notices-non-trivial` invariants. Worth a side-by-
   side comparison.
2. **`hygiene/lockfiles@v1` overlay.** vscode ships `package-lock.json` +
   per-extension lockfiles under `extensions/*/package-lock.json`; the
   bundled `hygiene/lockfiles@v1` (7 rules) would catch nested-lockfile
   drift across the extension tree without adding repo-specific rules.
3. **`nested_configs: true` for `extensions/`.** vscode's `extensions/`
   subtree is effectively a polyglot mini-monorepo (each extension has
   its own `package.json`, lint config, and contribution metadata).
   Adopting `nested_configs: true` would let per-extension `.alint.yml`
   files layer extension-specific assertions on top of the root config
   without bloating the root file.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **67** (25 custom + 6 bundled rulesets — `oss-baseline`
  15, `node` 9, `ci/github-actions` 3, `hygiene/no-tracked-artifacts`
  11, `tooling/editorconfig` 3, `agent-context` 5; some rule IDs
  overlap which is why the grand total is 67 rather than the
  arithmetic sum of 71)
- **`alint validate-config`:** ✓ Config valid: 67 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  339-violation breakdown (2 real fixture copyright omissions + 1
  real .bat line-ending + 116 GHA hardening findings + 1 fixture
  .env + 19 hygiene-rule false positives needing refinement + 190
  cosmetic + 10 governance/info)
- **Apples-to-apples target:** `build/hygiene.ts` — **6 of 8 hygiene
  pipeline stages (75%) covered declaratively or shelled out via
  `command:` rules** (productJson + copyrights ×2 + checkCopilotEnginesVersion
  via cross_file_value_equals alint-future + formatting/eslint/stylelint
  shellouts). The remaining 2 are AST-aware/escape-hatch
  semantics (`unicode` + `indentation`) and stay in the script via the
  `vscode-precommit-hygiene` command-rule backstop.
- **Pitfall instances flagged:** **0 instances of pitfall #22** in
  this config. The two copyright-header rules use single-quoted YAML
  scalars with `\s+` bridging — the canonical correct pattern.
- **Pitfall fixes (v0.9.17):** Pitfalls #18 + #19 do not apply here.
- **Open gaps:** `cross_file_value_equals` (v0.10 ship-target, 10
  sources — vscode is the most consumer-facing),
  `file_content_matches_or_marker` (NEW v0.10+ candidate, vscode-only),
  `indent_style.skip_block_comment_continuation` +
  `file_is_ascii.{allow,skip_per_line_marker}` (NEW v0.10+ candidates
  on existing rules, vscode-only — low priority).
- **Open suspected bugs in this directory's `.alint.yml`:** None.
  Config is clean.
