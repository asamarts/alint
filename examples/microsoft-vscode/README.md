# Case study: `microsoft/vscode`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/microsoft-vscode/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> validation status. Same facts, different language.

Inventory of the structural-validation tooling in `microsoft/vscode`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-clone of
`microsoft/vscode@HEAD` excluding `extensions/`, `test/`, and
`build/lib/` (extensions/ alone is hundreds of MB of bundled-extension
source).

---

## Summary

`microsoft/vscode` ships a custom hygiene-check script (`build/hygiene.ts`)
that does structurally what alint is designed to do — making this the
case study with the most direct apples-to-apples comparison surface in
the catalogue.

Concrete count: **34 distinct structural-validation surfaces**
inventoried, including:

- **`build/hygiene.ts`** — the canonical bespoke hygiene pipeline,
  with **5 in-tree pipeline stages** (`productJson`, `unicode`,
  `indentation`, `copyrights`, `formatting`) plus **2 delegated
  stages** (`eslint`, `gulpstylelint`) plus **1 cross-file check**
  (`checkCopilotEnginesVersion`). This is the most direct alint
  comparison target across all P2 studies.
- **`build/filters.ts`** — the cascading-filter machinery
  (`all ⊃ eol ⊇ indentation ⊃ copyright ⊃ typescript`) that scopes
  each hygiene stage. Every `paths.exclude` list in the alint config
  mirrors a corresponding entry here.
- **`.eslint-plugin-local/`** — **45 in-tree custom eslint rules**
  (`code-no-any-casts`, `code-import-patterns`, `code-layering`,
  `vscode-dts-event-naming`, `vscode-dts-provider-naming`, ...). All
  TSESTree visitors; out of alint's "no AST" scope.
- **`src/vscode-dts/`** — the **171 vscode.proposed.\*.d.ts** + 1
  stable `vscode.d.ts` public extension API surface (one of the most
  load-bearing API contracts in OSS — millions of marketplace
  extensions consume it). The naming convention is documented in the
  directory's README but enforced nowhere statically.
- **75 npm scripts** in `package.json` (lint orchestration,
  build pipeline, smoke tests, watch tasks).
- **16 GitHub Actions workflows** (`.github/workflows/`), 4 of which
  are reusable (`workflow_call:`) and the rest are pre-merge gates.
- **`cglicenses.json` + `cgmanifest.json` + `ThirdPartyNotices.txt`**
  — the Microsoft Component Governance triple (3803-line generated
  notices file).
- **7 per-target tsconfig.\*.json files** in `src/` (base, monaco,
  tsec, vscode-dts, vscode-proposed-dts, defineClassFields, root).
- **`tsfmt.json`** — formatting options for the in-house TypeScript
  formatter (vscode is one of the few mature TS repos that doesn't
  use Prettier).
- **`.editorconfig`** + **`.gitattributes`** + **`.nvmrc`** —
  standard editor / git / runtime conventions.
- **`product.json`** — the OSS-vs-proprietary build differentiator
  (Code OSS strips marketplace endpoints; the hygiene script enforces
  this).

Of the 34 distinct structural-validation surfaces inventoried:

- **~50 % map directly to existing alint rules** (~17 surfaces:
  copyright header on src/build, line-endings matrix, .editorconfig
  invariants, governance files, Component Governance trio, vscode-dts
  filename grammar, vscode-dts copyright header, vscode-dts module
  shape, package.json script pinning, tsconfig strictness, AGENTS.md,
  workflow permissions, action-SHA pinning, no-tracked-artifacts,
  product.json no-extensionsGallery, ThirdPartyNotices non-trivial,
  build/eslint.ts presence)
- **~6 % need new alint primitives** (~2 surfaces:
  `checkCopilotEnginesVersion` ↔ `cross_file_value_equals`;
  `indentation`/`unicode` block-comment & per-line-escape-hatch
  semantics ↔ new rule-knobs)
- **~44 % are out of alint's scope** (~15 surfaces: 45 custom eslint
  rules, gulp-stylelint CSS AST, `formatter.verifyFormatting()` TS
  AST, `build/checker/layersChecker.ts`, `checkCyclicDependencies.ts`,
  `pr.yml` API-proposal version-bump diff check, `tsec` security
  TypeScript checker, gulp build pipeline, etc.)

The 50 % that *do* fit translate to the **67-rule alint config** in
[`./.alint.yml`](.alint.yml), bundled-rulesets-included.

---

## Headline coverage

**`build/hygiene.ts` coverage: 6 of 8 hygiene-pipeline stages (75 %)**
covered declaratively by alint primitives. The remaining 2 stages
(`formatting` — TS-AST-aware printer round-trip; `unicode` — per-line
`// allow-any-unicode-next-line` escape-hatch semantics) shell out via
`command:` rules; per-stage details below.

---

## `build/hygiene.ts` analysis (the apples-to-apples comparison)

`build/hygiene.ts` is a 335-line TypeScript script that pipes a
gulp-vinyl stream through 5 in-tree pipeline stages plus 2 delegated
stages plus 1 cross-file check. Direct alint coverage:

| `build/hygiene.ts` stage | Code reference | alint coverage | Status |
|---|---|---|---|
| **`productJson`** — bans `extensionsGallery` from product.json (only Microsoft proprietary builds set it) | hygiene.ts:52-61 | `vscode-product-json-no-extensions-gallery` (`file_content_forbidden`) | **EXACT** — 1:1 mapping |
| **`unicode`** — rejects non-ASCII codepoints outside an explicit ~40-glyph allowlist; honours `// allow-any-unicode-next-line` per-line and `allow-any-unicode-comment-file` per-file escape hatches | hygiene.ts:63-98 | None — see "Needs new primitive" #2 below | **PARTIAL** — `file_is_ascii` exists but lacks the allowlist + per-line escape-hatch comment, so a naive restatement fires on every JSDoc with an em-dash. Deferred to the script |
| **`indentation`** — rejects lines that start with a space-then-non-whitespace, with a regex carve-out for `^[\t]* \*` (JSDoc block-comment continuations) | hygiene.ts:100-120 | None — see "Needs new primitive" #2 below | **PARTIAL** — `indent_style: tabs` exists but lacks the block-comment-continuation exception. Deferred to the script |
| **`copyrights`** — every TS file under the `copyrightFilter` scope opens with the canonical 4-line Microsoft/MIT block | hygiene.ts:122-134 | `vscode-copyright-header-src` (`file_header`) + `vscode-dts-proposed-copyright-header` for the public API surface | **EXACT** — 1:1 mapping (verified against actual tree: 2 real test-fixture violations caught) |
| **`formatting`** — `formatter.verifyFormatting()` runs the TypeScript printer; flags files that round-trip differently | hygiene.ts:136-150 | None — TS-AST-aware formatter | **OUT OF SCOPE** — shelled out via `vscode-precommit-hygiene` command rule |
| `eslint` (delegated) — runs the 45-rule local eslint plugin | hygiene.ts:188-199 | None — TSESTree visitors | **OUT OF SCOPE** — shelled out via `vscode-eslint` command rule |
| `gulpstylelint` (delegated) — runs CSS AST analysis | hygiene.ts:201-210 | None — CSS AST | **OUT OF SCOPE** — shelled out via `vscode-stylelint` command rule |
| **`checkCopilotEnginesVersion`** — asserts `engines.vscode` in `extensions/copilot/package.json` literally equals `^${rootPkg.version}` from the root | hygiene.ts:34-43 | None — see "Needs new primitive" #1 below | **NEEDS NEW PRIMITIVE** — this is the canonical `cross_file_value_equals` shape (now **v0.10 ship-target** at 10 sources past saturation) |

**Quantified overlap: 6 of 8 hygiene-script checks (75 %) covered
directly by alint primitives; 2 of 8 (25 %) need either new
primitives or stay in the script.**

vscode's `build/hygiene.ts` is the most concentrated single-script
hygiene comparison surface in the catalogue —
microsoft/typescript's `Herebyfile.mjs` doesn't have a "hygiene"
check this concentrated, and nodejs/node's hygiene is spread across
the 1700-line Makefile + Python helpers + cpplint fork.

---

## `.eslint-plugin-local/` custom rules analysis

vscode ships **45 custom in-tree eslint rules** under
`.eslint-plugin-local/` (counted by `*.ts` files excluding
`index.ts`, `utils.ts`, and the `tests/` subdirectory). Every single
one is a TSESTree (TypeScript-eslint AST) visitor — implements the
`ESLintRule` interface with `create(context) { return {<NodeKind>(node)
{...}} }` shape.

Sample (all out of alint's scope):

| Rule | What it does (one-liner) |
|---|---|
| `code-amd-node-module` | Bans `require()` in browser-bundle TS sources |
| `code-declare-service-brand` | Asserts services declare a `_serviceBrand` discriminator |
| `code-import-patterns` | Layered-architecture import-graph enforcement |
| `code-layering` | Browser/Node/Common boundary checks |
| `code-must-use-result` | Asserts `Promise`-returning calls aren't fire-and-forget |
| `code-no-accessor-after-await` | Reactivity-system `get()` after `await` ban |
| `code-no-any-casts` | Bans `as any` casts |
| `code-no-deep-import-of-internal` | Bans `import x from 'pkg/internal/foo'` |
| `vscode-dts-cancellation` | Asserts `vscode.proposed.*.d.ts` async APIs accept `CancellationToken` |
| `vscode-dts-event-naming` | Asserts `Event<T>` properties are named `onDidX` / `onWillX` |
| `vscode-dts-interface-naming` | Asserts no `IFoo` Hungarian-prefix in public API |
| `vscode-dts-literal-or-types` | Asserts proposed-API string-union types use `'a' \| 'b' \| 'c'` over `string` |
| `vscode-dts-provider-naming` | Asserts `vscode-dts/*` provider interfaces match the `XProvider` pattern |
| ... (32 more) | All TSESTree visitors |

**Of 45 rules: 0 are alint-shaped (structural/declarative); all 45
are AST-aware.** Every in-tree rule is correctly placed in eslint
rather than alint. The closest near-AST ones
(`vscode-dts-event-naming`, `vscode-dts-interface-naming`) require
parsing the `.d.ts` to extract type/interface names from
declarations; alint would need a TS parser to replicate them.

vscode maintains 45 in-tree eslint rules *alongside* the 335-line
hygiene script — they do different jobs. alint maps onto the
hygiene-script half of that dichotomy.

---

## Existing tooling inventory

### `build/hygiene.ts` (canonical hygiene pipeline)

See "`build/hygiene.ts` analysis" above — 8 distinct checks, 6
covered directly by alint, 1 needs `cross_file_value_equals`, 1
deferred to the script.

### `build/filters.ts` (cascading filter scopes)

The `indentationFilter`, `copyrightFilter`, `unicodeFilter`,
`tsFormattingFilter`, `eslintFilter`, `stylelintFilter` blocks define
the per-stage scope. The alint config's `paths.exclude` lists mirror
each one; the carve-outs for `src/vs/base/browser/dompurify/**`,
`src/vs/base/common/marked/marked.js`, `build/win32/**`,
`build/checker/**`, etc. are copied directly from `filters.ts`.

### `.eslint-plugin-local/` (45 custom rules + plugin scaffolding)

| File | Purpose | alint disposition |
|---|---|---|
| `index.ts` | Plugin entry — registers all 45 rules with eslint | `vscode-eslint-plugin-local-index-exists` (`file_exists`) |
| `package.json` | Local-plugin npm manifest | Same |
| `tsconfig.json` | TS compile config for the plugin | Same |
| `code-*.ts` (32 files) | Microsoft-internal-style rules | OUT OF SCOPE (TSESTree) |
| `vscode-dts-*.ts` (13 files) | Public-API-surface conventions | OUT OF SCOPE (TSESTree) |
| `tests/*.ts` | Plugin's own self-tests | OUT OF SCOPE |
| `utils.ts` | Shared rule helpers | OUT OF SCOPE |

### `.github/workflows/` (16 workflows)

| Workflow | What it does | alint disposition |
|---|---|---|
| `pr.yml` | Main pre-merge gate (compile + hygiene + tests) | Each step is its own surface |
| `pr-{linux,linux-cli,darwin,win32}-test.yml` | Reusable test workflows (`workflow_call:`) | Permissions deferred to caller |
| `api-proposal-version-check.yml` | Asserts `version: N` in `extensionsApiProposals.ts` bumps when any `vscode.proposed.*.d.ts` is modified | OUT OF SCOPE (PR-diff aware) |
| `chat-lib-package.yml`, `chat-perf.yml` | Copilot/chat extension bundling + perf | OUT OF SCOPE (CI orchestration) |
| `component-fixture-tests.yml` | Component-explorer fixture tests | OUT OF SCOPE |
| `copilot-setup-steps.yml` | Copilot agent CI setup steps | Permissions check applies |
| `monaco-editor.yml` | Monaco-editor downstream consumer build | OUT OF SCOPE |
| `no-engineering-system-changes.yml` | Asserts PRs don't touch `.azure-pipelines/`/`.github/` without approval | OUT OF SCOPE (PR-diff aware) |
| `pr-node-modules.yml` | Asserts `node_modules` not committed via PR | Restated by `node-no-tracked-node-modules` from the bundled node ruleset |
| `screenshot-test.yml`, `sessions-e2e.yml`, `telemetry.yml` | Operational test workflows | OUT OF SCOPE |

5 of 16 workflows have a structural assertion alint can restate; the
rest are CI orchestration / release / perf / e2e.

### `package.json` (75 scripts)

The CI-gating subset alint pins by literal command:

| Script | Pinned by |
|---|---|
| `precommit` | `vscode-package-json-precommit-script` |
| `eslint` | `vscode-package-json-eslint-script` |
| `stylelint` | `vscode-package-json-stylelint-script` |
| `vscode-dts-compile-check` | `vscode-package-json-vscode-dts-check` |

The remaining 71 scripts (build orchestration, watch tasks, smoke
tests, copilot setup) aren't lint-gated; the `command:` rules
catch-all backstop is `vscode-precommit-hygiene` + `vscode-eslint` +
`vscode-stylelint`.

### `src/tsconfig*.json` matrix (7 files)

| File | Role | alint coverage |
|---|---|---|
| `tsconfig.base.json` | Root strict-mode + lib settings | `vscode-tsconfig-base-strict`, `vscode-tsconfig-base-no-implicit-overrides` |
| `tsconfig.json` | Main project — extends `.base`, adds tsec plugin | Inherits via `extends` |
| `tsconfig.monaco.json` | Monaco-editor downstream consumer config | Inherits |
| `tsconfig.tsec.json` | Trusted-types security checker config | Inherits |
| `tsconfig.vscode-dts.json` | Stable public API compile-check | Pinned by `vscode-package-json-vscode-dts-check` |
| `tsconfig.vscode-proposed-dts.json` | Proposed API compile-check | Same |
| `tsconfig.defineClassFields.json` | Class-field-init order checker | OUT OF SCOPE |

### `cglicenses.json` + `cgmanifest.json` + `ThirdPartyNotices.txt`

The Microsoft **Component Governance** triple. `cgmanifest.json`
declares non-package-locked dependencies (vendored binaries, native
libs); `cglicenses.json` overrides licenses for components without an
unambiguous LICENSE file in their repo; `ThirdPartyNotices.txt` is
**generated** from both (3803 lines, ~200 KiB at clone time —
contains verbatim license text of every transitive dependency).

| File | alint coverage |
|---|---|
| `cglicenses.json` | `vscode-component-governance-files` (`file_exists`) |
| `cgmanifest.json` | Same |
| `ThirdPartyNotices.txt` | Same + `vscode-third-party-notices-non-trivial` (`file_min_lines: 500`) to catch a corrupted regeneration that left a stub |

The deep license-resolution stays in the CG tooling (which writes
the .txt file from the .json). alint's value here is asserting the
on-disk file is *present* and *non-trivial* — the failure mode it
catches is "a CG run failed mid-write and left a 0-byte stub" or
"someone deleted the file thinking it was generated noise".

### `tsfmt.json`

The in-house TypeScript-formatter options file. vscode is one of the
few mature TS repos that doesn't use Prettier (the team predates
Prettier and has a deep customization need around JSDoc preservation).
If `tsfmt.json` is dropped or mutated, the `formatting` stage in
`build/hygiene.ts` silently passes (tsfmt falls back to its own
defaults). Pinned by `vscode-tsfmt-json-exists`.

### `.editorconfig`

```
[*]
indent_style = tab
trim_trailing_whitespace = true

[{*.yml,*.yaml,package.json}]
indent_style = space
indent_size = 2
```

Mapped to:
- `tooling/editorconfig@v1` bundled ruleset (covers `.editorconfig`
  presence + final-newline + trim-trailing)
- `vscode-indent-style-tabs` (NOT EMITTED — see "Needs new primitive"
  #2 — the rule has no JSDoc block-comment exception)
- per-language indent override is too narrow to be worth restating
  separately — `package.json` is universally 2-space anyway

### `.gitattributes`

```
* text=auto
LICENSE.txt eol=crlf
ThirdPartyNotices.txt eol=crlf
*.bat eol=crlf
*.cmd eol=crlf
*.ps1 eol=lf
*.sh eol=lf
*.rtf -text
**/*.json linguist-language=jsonc
```

Each line maps to a `vscode-*-line-endings` rule:
- `vscode-license-files-crlf` (LICENSE.txt + ThirdPartyNotices.txt)
- `vscode-windows-bat-crlf` (`*.bat` + `*.cmd`)
- `vscode-shell-script-lf` (`*.sh` + `*.ps1`)

The `linguist-language=jsonc` line (which makes GitHub's syntax
highlighter render comments correctly in JSON files vscode commits as
JSONC) is metadata, not a structural assertion.

### `src/vscode-dts/` (the public extension API surface)

This is **the structural surface unlike anything in P2a** — vscode's
public API for downstream extensions, with stricter backwards-
compatibility discipline than any other on-disk surface in the repo.

171 `vscode.proposed.<name>.d.ts` files + 1 stable `vscode.d.ts`
(21k lines) at clone time. Each file:

1. Carries the canonical Microsoft/MIT copyright header
   (`vscode-dts-proposed-copyright-header`)
2. Follows the `vscode.proposed.[a-zA-Z][a-zA-Z0-9]*.d.ts` filename
   pattern (`vscode-dts-proposed-filename-grammar`) — documented in
   the directory's README, enforced nowhere statically
3. Opens (after the header) with `declare module 'vscode'` to extend
   the public namespace — **with the wrinkle that ~32 of the 171
   proposed files at clone time are "placeholder proposals" that
   gate non-TS surface (a `package.json#contributes.configuration`
   key, a menu slot) and intentionally have no module declaration**.
   The `vscode-dts-declare-module-shape` rule scopes to just the
   stable `vscode.d.ts` to avoid false positives on placeholders.

The extension-API surface notes (alint-specific):

- The proposal-naming convention isn't an alint rule kind — it's a
  filename regex check (`filename_regex`). What's *interesting* is
  that this is one of the most-consumed naming conventions in OSS
  (downstream extensions read these files at compile time across the
  marketplace) and is enforced statically by exactly nothing —
  `pr.yml` has no naming-grammar gate, the `tsgo --project
  src/tsconfig.vscode-proposed-dts.json` step doesn't validate
  filenames, and the directory's README is the only documentation.
  alint catching a typo here at PR time is a genuinely net-new check.

- The placeholder-proposal pattern (32 of 171 files) is a NEW rule
  shape: "every file in scope X *either* matches pattern A *or*
  matches a marker M". A v0.10+ candidate
  (`file_content_matches_or_marker`) would let alint enforce the
  module-declaration rule cleanly across both real and placeholder
  proposals. Logged in the gap catalogue below.

- The cross-file `pr.yml` API-proposal version-bump check (when any
  `vscode.proposed.*.d.ts` is modified, the `version: N` pragma in
  `src/vs/platform/extensions/common/extensionsApiProposals.ts` for
  that proposal must increment by 1) is **PR-diff aware** — out of
  alint's scope. STAYS in the workflow.

---

## Maps to alint (~17 surfaces, ~50 %)

The surfaces with direct alint coverage in [`./.alint.yml`](.alint.yml):

| Surface | alint rule(s) | Status |
|---|---|---|
| `build/hygiene.ts` `productJson` (no extensionsGallery) | `vscode-product-json-no-extensions-gallery` | EXACT |
| `build/hygiene.ts` `copyrights` (Microsoft/MIT header on src/build/scripts TS) | `vscode-copyright-header-src` | EXACT |
| `build/hygiene.ts` `copyrights` (Microsoft/MIT header on vscode-dts API surface) | `vscode-dts-proposed-copyright-header` | EXACT |
| `src/vscode-dts/vscode.d.ts` opens with `declare module 'vscode'` | `vscode-dts-declare-module-shape` | EXACT (scoped to stable surface) |
| `vscode.proposed.<name>.d.ts` filename grammar | `vscode-dts-proposed-filename-grammar` | EXACT (net-new — enforced nowhere statically before) |
| `tsconfig.base.json` `strict: true` | `vscode-tsconfig-base-strict` | EXACT |
| `tsconfig.base.json` `noImplicitOverride: true` | `vscode-tsconfig-base-no-implicit-overrides` | EXACT |
| `package.json` precommit/eslint/stylelint script pinning | `vscode-package-json-{precommit,eslint,stylelint}-script` | EXACT |
| `package.json` `vscode-dts-compile-check` script pinning | `vscode-package-json-vscode-dts-check` | EXACT |
| `.gitattributes` LICENSE.txt + ThirdPartyNotices.txt CRLF | `vscode-license-files-crlf` | EXACT |
| `.gitattributes` *.bat / *.cmd CRLF | `vscode-windows-bat-crlf` | EXACT (caught 1 real drift in the live tree) |
| `.gitattributes` *.sh / *.ps1 LF | `vscode-shell-script-lf` | EXACT |
| `tsfmt.json` presence | `vscode-tsfmt-json-exists` | EXACT |
| `.eslint-plugin-local/{index.ts,package.json,tsconfig.json}` + `eslint.config.js` + `.eslint-ignore` | `vscode-eslint-plugin-local-index-exists` | EXACT |
| Component Governance triple (cglicenses + cgmanifest + ThirdPartyNotices) | `vscode-component-governance-files` + `vscode-third-party-notices-non-trivial` | EXACT |
| `AGENTS.md` + agent-context bundled ruleset | `vscode-agents-md-present` + bundled | EXACT |
| Workflow permissions + action-SHA pinning | `vscode-workflow-has-permissions` + bundled `gha-pin-actions-to-sha` | EXACT (caught 105 unpinned-action drifts in the live tree) |

### Bundled rulesets adopted

- `oss-baseline@v1` (license, README, gitignore, no merge markers,
  no bidi, security policy, codeowners, code-of-conduct)
- `node@v1` (package.json + lockfile + node_modules hygiene)
- `ci/github-actions@v1` (workflow permissions + action SHA pinning;
  caught the 105 unpinned-action drifts above)
- `hygiene/no-tracked-artifacts@v1` (no tracked .DS_Store, build
  outputs, etc.)
- `tooling/editorconfig@v1` (final-newline, trim-trailing,
  .editorconfig presence)
- `agent-context@v1` (AGENTS.md hygiene)

(No `cpp@v1` ruleset — vscode's native code lives entirely in
node-gyp dependencies (`@vscode/native-watchdog`, `kerberos`,
`node-pty`, `@parcel/watcher`, etc.) which are out-of-tree. The
checked-in repo is pure TS / JS / JSON / YAML / shell.)

---

## Needs new alint primitive (~2 surfaces, ~6 %)

### 1. `cross_file_value_equals` — `checkCopilotEnginesVersion`

`build/hygiene.ts:34-43` defines:

```ts
export function checkCopilotEnginesVersion(repoRoot: string): string | undefined {
    const rootPkg = JSON.parse(fs.readFileSync(path.join(repoRoot, 'package.json'), 'utf8'));
    const copilotPkg = JSON.parse(fs.readFileSync(path.join(repoRoot, 'extensions/copilot/package.json'), 'utf8'));
    const expected = `^${rootPkg.version}`;
    const actual = copilotPkg?.engines?.vscode;
    if (actual !== expected) {
        return `engines.vscode in 'extensions/copilot/package.json' must be "${expected}" (the version from the root package.json), but found "${actual ?? '<missing>'}"`;
    }
    return undefined;
}
```

This is the **canonical `cross_file_value_equals` shape**: the value
at JSONPath `$.engines.vscode` in file
`extensions/copilot/package.json` must equal a transformation
(prepend `^`) of the value at JSONPath `$.version` in file
`package.json`.

**`cross_file_value_equals` is now a v0.10 ship-target** at 10 sources
past saturation (airflow, tokio, clap, uv, react, pnpm, nodejs/node,
pytorch, vscode, istio per `docs/development/launch-evidence.md`).
vscode's `checkCopilotEnginesVersion` is the most consumer-facing of
the 10 (downstream extension installs break on version mismatch). The
candidate's existing motivating cases: split-workspace lockfile sync
(uv, tokio), root README ↔ per-crate README (clap), version-in-CHANGELOG
(clap, react), pnpm `meta-updater`'s 13 cross-package invariants,
react's `ReactVersion.js` propagated to 3 per-package fields, nodejs
`tools/eslint-rules/*` ↔ `eslint.config.mjs`, pytorch WORKFLOWSYNC,
istio's per-chart `value_extractor:` refinement (pitfall #20).

### 2. `indent_style.skip_block_comment_continuation` + `file_is_ascii.allow` + `file_is_ascii.skip_per_line_marker`

**One surface in spirit, three knobs in implementation** — vscode's
hygiene script's `indentation` and `unicode` streams have semantics
alint's existing `indent_style` and `file_is_ascii` rules don't model:

- **`indent_style: tabs` + JSDoc block-comment carve-out** — the
  hygiene script accepts `^[\t]* \*` (tab(s) followed by a single
  space and asterisk) as valid indentation. alint's `indent_style:
  tabs` flags any mixed tab+space leading whitespace as a violation,
  which fires on every JSDoc continuation line in vscode's
  tab-indented codebase. New knob:
  `indent_style.skip_block_comment_continuation: true`.

- **`file_is_ascii: allow:` + `file_is_ascii: skip_per_line_marker`**
  — the hygiene script's `unicode` stream rejects non-ASCII *unless*
  the codepoint is in an explicit ~40-glyph allowlist (arrows, math
  symbols, em-dash, emoji used in user-facing logs) **OR** the line
  carries a `// allow-any-unicode-next-line` comment **OR** the file
  carries an `allow-any-unicode-comment-file` marker. alint's
  `file_is_ascii` is binary — any non-ASCII codepoint is a
  violation — which fires on any source file with a single em-dash
  in a JSDoc comment (most of `src/`).

  **Two new knobs:**
  - `file_is_ascii.allow: ["—", "·", "•", "→", ...]` — list of
    explicitly-permitted non-ASCII codepoints
  - `file_is_ascii.skip_per_line_marker: "// allow-any-unicode-next-line"`
    — pragma to skip the immediately-following line

These are **NEW rule-kind candidates not on the existing v0.10+
list** (and they're refinements of existing rules, not new kinds).
Niche enough that the alternative — keep them in the script and
shell out via `vscode-precommit-hygiene` — is the recommended path.
Filed as low-priority v0.10+ candidates.

### 3. `file_content_matches_or_marker` — vscode-dts placeholder proposals

Surfaced uniquely by vscode's vscode-dts/ surface: 32 of 171
`vscode.proposed.*.d.ts` files at clone time are "placeholder
proposals" that gate non-TS surface (a
`package.json#contributes.configuration` key, a menu slot) and
intentionally have no `declare module 'vscode'` line.

The clean alint shape would be: "every file matches pattern A *or*
contains marker M", where the marker is a comment like
`// empty placeholder declaration`. Doesn't exist today; same
flavour as the v0.10 single-source candidate
`json_key_value_forbidden`. Logged as low-priority.

The current workaround (in the alint config) is to scope the
`vscode-dts-declare-module-shape` rule to just the stable
`vscode.d.ts` and let the proposal-tier pass.

---

## Out of alint's scope (~15 surfaces, ~44 %)

The full inventory above flagged 15 of 34 surfaces as out of scope.
Listed by category:

- **AST-aware analysis** — 45 `.eslint-plugin-local/*.ts` rules, the
  `formatter.verifyFormatting()` TS-printer round-trip,
  `gulpstylelint` CSS AST analysis, `tsec` trusted-types security
  TypeScript checker, `build/checker/layersChecker.ts`,
  `build/lib/checkCyclicDependencies.ts`. Each is a parser + visitor
  pattern; alint's deliberate non-goal.
- **PR-diff aware** — `api-proposal-version-check.yml` (asserts
  `version:` bumps when `.proposed.d.ts` modified),
  `no-engineering-system-changes.yml` (asserts certain dirs
  untouched), the `git diff --cached` invocation in `hygiene.ts`'s
  `if (import.meta.main)` block. alint sees one tree at a time.
- **Build/release pipeline** — `gulpfile.*.ts` (vscode.linux,
  vscode.win32, vscode.darwin, vscode.web, reh, editor, extensions,
  cli, scan, hygiene, compile), `build/azure-pipelines/`, the
  installer codegen for win32 (`build/win32/code.iss`).
- **Operational / CI orchestration** — `chat-perf.yml`,
  `screenshot-test.yml`, `sessions-e2e.yml`, `telemetry.yml`,
  `monaco-editor.yml`, `component-fixture-tests.yml`.
- **Native binary fetchers** — `build/checksums/*.txt`,
  `build/win32/explorer-dll-fetcher.ts`,
  `build/win32/inno_updater.exe` (that's a .exe checked into the repo
  for installer generation; out of alint's scope but flagged by the
  bundled `hygiene/no-tracked-artifacts@v1` for review).
- **Codegen** — `build/lib/extractTouchBarIcons.ts`,
  `build/builtin/main.js`, the `.eslint-plugin-local/index.ts` rule
  registration. Generated at build time; alint doesn't run codegen.

---

## Already covered by other linters vscode uses

- **eslint** (with `eslint.config.js` + the 45-rule `.eslint-plugin-local/`
  + the upstream typescript-eslint ruleset) — alint shells out to
  `npm run eslint` rather than competing on JS/TS rule expressivity.
- **stylelint** (CSS AST checks driven by `build/stylelint.ts`) —
  alint shells out to `npm run stylelint`.
- **tsfmt** (the in-house TS formatter, used by `formatter.verifyFormatting()`
  in `build/hygiene.ts`) — AST-aware; alint shells out via
  `npm run precommit`.
- **tsec** (trusted-types security TypeScript checker, configured via
  `src/tsconfig.tsec.json`) — alint asserts the tsconfig file exists;
  the deep type-system check stays in tsec.
- **OpenSSF Scorecard** (via `scorecard.yml` in the root `.github/`,
  not in `.github/workflows/`) — alint **does** restate the
  action-SHA-pinning + permission-block invariants Scorecard checks,
  so they surface at PR time instead of on the next nightly run.

---

## Performance comparison (placeholder — bench when validation pass scales)

`npm run precommit` (driving `build/hygiene.ts` over the staged
changeset) is sub-second for a typical PR (< 50 files modified). The
bottleneck is the `eslint` + `stylelint` cold-cache pass when run
across the full tree, which is multiple minutes.

For the 50 % of checks that fit alint's grammar today, the alint
config replaces the relevant `build/hygiene.ts` stages declaratively
in one file, with editor-LSP autocomplete via the JSON Schema, and
with the `validate-config` exit code as a pre-commit fail-fast. The
deep tools (eslint, stylelint, tsfmt, tsec, the AST checkers in
`build/checker/`) stay where they are. `npm run precommit` keeps
running for the AST-aware `formatting` stage and the per-line
`allow-any-unicode-next-line` escape semantics.

To benchmark wall-clock against the live tree:
`time npm run precommit` (after a warm cache) vs `time alint check`
against the same tree. Deferred to the per-repo measurement pass;
expectation is 2-5× faster on the structural subset (Rust + zero
process spawn) and roughly equivalent on the shell-out subset (the
eslint invocation dominates).

---

## Followup feature work surfaced (priority order)

- **`cross_file_value_equals`** — now **v0.10 ship-target** (10 sources
  past saturation). vscode's `checkCopilotEnginesVersion` is the most
  consumer-facing of the 10 cases (downstream extension installs break
  on version mismatch). Reaffirms this as v0.10's single
  highest-leverage gap.
- **`indent_style.skip_block_comment_continuation`** + **`file_is_ascii.{allow,skip_per_line_marker}`**
  — NEW knobs on existing rules (not new kinds). Niche; vscode is the
  single source. Recommended path is to keep these in
  `build/hygiene.ts` and shell out, rather than grow alint's surface.
  Logged as v0.10+ low-priority candidates.
- **`file_content_matches_or_marker`** — NEW rule kind for the
  vscode-dts placeholder-proposal pattern. Single-source
  (vscode-only); logged as low-priority.

---

## No NEW schema/language pitfalls hit

The 21 documented in `docs/development/CONFIG-AUTHORING.md` cover
everything that came up while authoring this config. Pitfalls #18 + #19
were both fixed in engine v0.9.17 (per-rule `respect_gitignore: false`
knob; literal-path runtime guard) — neither workaround is needed in this
config. Specific near-misses navigated:

- **§13 (regex anchoring)** — every `^` / `$` in this config is `(?m)`
  prefixed (the workflow-permissions check) because the workflow
  files are multi-line.
- **§14 (YAML `\n` in regex)** — **HIT TWICE**. The first draft of
  `vscode-copyright-header-src` used a `pattern: |` block scalar with
  a literal `\n` between the two header lines, which compiled into
  a regex matching the two-character sequence `\n` rather than a real
  newline — silently failed against every file in the tree. Fixed by
  collapsing the two lines into a single regex with `\s+` joining
  them. Same fix applied to `vscode-dts-proposed-copyright-header`.
  This is the **most common pitfall in real-world config writing** and
  the parse-validation audit can't catch it (the regex compiles
  fine; only a smoke test against representative input would notice).
- **§16 (`*_path_matches` against bool fields)** —
  `tsconfig.base.json`'s `compilerOptions.strict` and
  `noImplicitOverride` are bools, so we use `json_path_equals` with
  YAML-native `equals: true` literals rather than reaching for
  `json_path_matches`.
- **`level: off` for an intentionally-disabled rule** — used on
  `vscode-indent-style-tabs` originally, then removed entirely in
  favour of a documentation-only comment, since `level: off` rules
  are silently skipped by the parse-validate audit (which is the
  intended behaviour but reads as "rule is unused" to a config
  reviewer).

The `coverage_audit_examples_parse.rs` audit passes with this config
in place (run from the repo root: `cargo test --release -p alint-e2e
--test coverage_audit_examples_parse`).

---

## Future analysis

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
   without bloating the root file. The vscode-dts/ stable surface is
   already fronted by `vscode-dts-declare-module-shape` at the root;
   per-extension rules (e.g. extension-manifest shape, contributes/
   conventions) would benefit most from the nested-config treatment.

---

## Validation status (2026-05-07)

- **alint version:** 0.9.17 (1dbd9b218a0e, built 2026-05-07)
- **Rule count:** 67 (~37 custom + 6 bundled rulesets — `oss-baseline`
  15, `node` 9, `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11,
  `tooling/editorconfig` 3, `agent-context` 5; rule IDs may overlap)
- **`validate-config`:** ✓ Config valid: 67 rule(s) loaded
- **Live-tree recheck:** not performed in this batch (vscode
  sparse-checkout not present in `/tmp/`)
- **Apples-to-apples target:** `build/hygiene.ts` — **6 of 8 hygiene
  pipeline stages (75 %) covered declaratively** by alint primitives;
  the remaining 2 are AST-aware (`formatting`) or stay in the script
  (`unicode` per-line escape-hatch).
- **Pitfall fixes (v0.9.17):** Pitfalls #18 + #19 do not apply here (no
  tracked-but-gitignored files; no `root_only: true` + multi-component
  literal entries beyond the single `AGENTS.md` / `tsfmt.json` cases
  which are at root)
- **Open gaps (status changes):** `cross_file_value_equals` promoted to
  **v0.10 ship-target** (10 sources past saturation; vscode's
  `checkCopilotEnginesVersion` is one of the 10);
  `indent_style.skip_block_comment_continuation` +
  `file_is_ascii.{allow,skip_per_line_marker}` (low-priority knobs on
  existing rules, vscode is the only source);
  `file_content_matches_or_marker` (low-priority, vscode-only)
