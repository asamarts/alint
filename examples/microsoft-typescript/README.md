# Case study: `microsoft/TypeScript`

Inventory of the structural-validation tooling in `microsoft/TypeScript`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-03, sparse-clone of
`microsoft/typescript@HEAD` (the JavaScript-based TypeScript compiler).

---

## Summary

TypeScript is a stable, conventional JS-tooling repo that funnels every
quality gate through **Hereby** (the in-house gulp-replacement task
runner) and a small set of `scripts/*.mjs` files. As of 2026-05 the repo
is in **maintenance mode**: TS 6.0 is the last JS-based release; future
development moved to `microsoft/typescript-go`. That makes the existing
structural-validation surface a *frozen snapshot* — exactly the kind of
target the launch-prep validation pass wants to lint against.

Concrete count: **6 CI quality gates** wired through `Herebyfile.mjs` +
`package.json` (`lint`, `format`/`check-format`, `knip`, `baselines`,
`misc`, `package-size`) plus **8 dev scripts** under `scripts/*.mjs`
that perform structural / runtime checks. Of those 14 surfaces:

- **3 fit alint directly** (Apache-2 header presence, file-size guard,
  baseline pairing) — net new structural enforcement that no script
  currently provides.
- **3 are shelled out via `command`** (eslint, dprint check, knip).
- **8 are out of scope** — TSESTree visitors, npm-pack diffs, runtime
  module-format probes, and the `git diff --staged` baseline accept
  loop. None of these are alint targets.

The headline outcome is *not* "alint replaces N shell scripts" — TS
doesn't have many. The headline is **alint adds structural checks
TypeScript doesn't enforce today** (header consistency, baseline
pairing, dprint plugin pinning, action-SHA pinning at PR time) while
still standing in as the entry point for the existing eslint / dprint
/ knip triple. **For the launch story, this is the "stable, famously
meticulous repo" data point** — alongside the kubernetes one
(massive, script-heavy) and the apache/airflow one (109-hook
pre-commit pipeline).

---

## Existing tooling inventory

### `Herebyfile.mjs` tasks (the canonical task runner)

| Task | What it checks | alint replacement |
|---|---|---|
| `lint` | `eslint --max-warnings 0 .` over the whole tree | Out of scope (AST). Shelled out via `command` rule. |
| `format` / `check-format` | `dprint fmt` / `dprint check` (whole tree) | Out of scope (formatter). Shelled out via `command` rule. |
| `knip` | Unused-export detection across the import graph | Out of scope (module-graph analysis). Shelled out via `command` rule. |
| `runtests-parallel` | Compiler test runner | Out of scope (test runner). |
| `baseline-accept` | Regenerate `tests/baselines/reference/*` from test output | Out of scope (codegen). |

5 tasks in total; 3 lint-class (eslint, dprint, knip), 2 test-class.

### `scripts/*.mjs` dev scripts

| Script | What it checks | alint replacement |
|---|---|---|
| `checkModuleFormat.mjs` | Runtime probe: every supported `require`/`import` shape against the published bundle returns the right `version` | Out of scope (runtime). |
| `checkPackageSize.mjs` | Diffs `npm pack --dry-run --json` between two refs; fails on >10% size growth | Out of scope (cross-ref diff). |
| `errorCheck.mjs` | Every diagnostic in `src/compiler/diagnosticMessages.json` appears in at least one `tests/baselines/reference/*.errors.txt` | Needs a `pair_count` primitive (assert N>=1 partner files match a registry entry). Out of scope today. |
| `find-unused-diganostic-messages.mjs` | Every diagnostic in the generated `diagnosticInformationMap.generated.ts` is referenced from `src/**/*.ts` | Out of scope (cross-file reference graph; effectively ESLint's `no-unused-exports` over a generated registry). |
| `processDiagnosticMessages.mjs` | Codegen for the diagnostic map | Out of scope (codegen). |
| `addPackageJsonGitHead.mjs` | Adds the current git SHA to the published `package.json` | Out of scope (mutation, not validation). |
| `link-hooks.mjs` | Links `scripts/hooks/*` into `.git/hooks/` | Out of scope (developer-machine setup). |
| `regenerate-unicode-identifier-parts.mjs` | Codegen for the Unicode tables | Out of scope (codegen). |

### `.github/workflows/` (18 workflows)

| Workflow | What it does | alint replacement |
|---|---|---|
| `ci.yml` | Orchestrates the 12 CI jobs (test, coverage, lint, knip, format, browser-integration, typecheck, smoke, package-size, misc, self-check, baselines) | Each job is its own surface — see the per-task rows above. The `required` job aggregator is workflow-only. |
| `pr-modified-files.yml` | Comments / closes PRs based on the changed-files set (e.g. closes PRs touching generated DOM lib files) | Out of scope (operates on the PR diff, not repo state). |
| `codeql.yml` | CodeQL static analysis | Out of scope (security scanner). |
| `scorecard.yml` | OpenSSF Scorecard run | Partial alint coverage: action-SHA pinning, permission-block presence enforced via `ts-workflow-actions-pinned-by-sha`. |
| `accept-baselines-fix-lints.yaml` | Manual workflow to regenerate baselines / run `--fix` | Out of scope (mutation). |
| The other 13 (`insiders`, `lkg`, `nightly`, `set-version`, `sync-branch`, `sync-wiki`, `twoslash-repros`, `update-package-lock`, `release-branch-artifact`, `new-release-branch`, `create-cherry-pick-pr`, `close-issues`, `copilot-setup-steps`) | Release / maintenance bots | Out of scope (operational, not validation). |

### `eslint.config.mjs` custom rules (under `scripts/eslint/rules/`)

All 9 are TSESTree visitors — out of alint's "no AST" scope. Listed
here so the inventory is complete:

| Rule | What it does |
|---|---|
| `only-arrow-functions` | Bans `function` expressions / declarations in favour of arrow fns |
| `argument-trivia` | Enforces inline-comment style on call arguments |
| `no-in-operator` | Bans the `in` keyword (use `hasProperty` instead) |
| `debug-assert` | Argument types of `Debug.assert` calls |
| `no-keywords` | Bans names like `string`, `number`, `boolean` as identifiers |
| `jsdoc-format` | `@internal` placement / multi-JSDoc rules |
| `js-extensions` | Relative imports must end in `.js` |
| `no-array-mutating-method-expressions` | Bans expression-statement uses of `arr.sort()` etc. |
| `no-direct-import` | Bans deep relative imports across `src/` boundaries |

These are perfect examples of "AST analysis is not alint's niche" —
they belong in eslint and stay in eslint.

### `package.json` `scripts:` block

| Script | Notes |
|---|---|
| `lint` | `hereby lint` (eslint) — see above |
| `format` | `dprint fmt` |
| `knip` | `hereby knip` |
| `build` / `build:compiler` / `build:tests` / `clean` | Build orchestration, not validation |
| `test` / `test:eslint-rules` | Test runners |
| `setup-hooks` | `node scripts/link-hooks.mjs` |

The 3 lint-class scripts (`lint`, `format`, `knip`) are the alint
shell-out targets; everything else is build / test orchestration.

### `tsconfig.json` conventions

The root `tsconfig.json` doesn't exist as a top-level file in the
sparse checkout (TypeScript drives its compiler config from inside
`src/` per project), but **`scripts/tsconfig.json`** is canonical for
the build scripts. Convention asserted by the alint config:
`compilerOptions.strict: true` everywhere outside `tests/**`, plus
`allowJs + checkJs` on `scripts/tsconfig.json` so the `.mjs` build
scripts get type-checked.

### `tests/baselines/reference/`

~53k baseline files (`.baseline.txt`, `.js`, `.symbols`, `.types`,
`.errors.txt`). The `.gitattributes` at the repo root sets `* -text` to
prevent git from touching line endings on these — a single rule
asserts that line stays in place. The pairing convention
(`.errors.txt` ↔ `.js`) is encoded as a `pair` rule. File-size guard
catches regressions where a test generates a runaway output.

### `CONTRIBUTING.md` + `AGENTS.md`

Both files are load-bearing for the maintenance-mode posture:
`CONTRIBUTING.md` opens with a banner directing all new work to
`microsoft/typescript-go`; `AGENTS.md` is the canonical doc that
coding agents are expected to read first. The alint config asserts
both stay in place and that `AGENTS.md` keeps the maintenance-mode
marker string verbatim.

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts the bundled
`oss-baseline + node + ci/github-actions + hygiene/no-tracked-artifacts +
tooling/editorconfig + agent-context` overlays, then layers ~22
TypeScript-specific rules on top.

The headline rules:

- **`ts-copyright-header-src` / `ts-copyright-header-scripts`** —
  Apache-2 / Microsoft header on every `src/**/*.ts` and
  `scripts/**/*.{mjs,cjs,mts}`. Currently *only* enforced for
  generated bundles (Herebyfile.mjs's `generateLibs` prepends it);
  alint enforces the source-side invariant too.
- **`ts-baseline-file-max-size`** — 256 KiB ceiling on every file
  under `tests/baselines/reference/`, with carve-outs for the
  legitimate large-output dirs. Catches runaway-output regressions.
- **`ts-baseline-errors-pair-with-js`** — every `*.errors.txt`
  baseline has a matching `*.js` sibling. Catches stale baselines
  from deleted tests.
- **`ts-gitattributes-keeps-binary-default`** — locks the `* -text`
  line in `.gitattributes` so git never re-introduces cross-platform
  line-ending churn on baselines.
- **`ts-tsconfig-strict-mode`** + **`ts-scripts-tsconfig-checkjs`** —
  every `tsconfig*.json` outside `tests/` keeps `strict: true`;
  `scripts/tsconfig.json` keeps `checkJs: true`.
- **`ts-dprint-{typescript,json,yaml}-plugin-pinned`** — three rules
  that lock the dprint plugin set so files don't silently stop being
  formatted if a plugin is dropped from `.dprint.jsonc`.
- **`ts-package-json-has-{lint,format,knip}-script`** — assert the
  three `npm run` targets the CI workflow depends on still exist with
  the expected commands. (npm exits 0 on a missing script — silent
  CI pass otherwise.)
- **`ts-workflow-actions-pinned-by-sha`** — every third-party action
  in `.github/workflows/*` is pinned to a 40-char commit SHA.
  Scorecard already checks this on a nightly cadence; alint surfaces
  the same gate at PR time.
- **`ts-agents-md-present`** + **`ts-agents-md-maintenance-marker`** —
  `AGENTS.md` is the canonical "this repo is in maintenance mode" doc.
  Stays in place; keeps the marker string.
- **`ts-eslint`** + **`ts-dprint-check`** + **`ts-knip`** — three
  `command:` rules that wrap the existing tools. Together with the
  rules above, `alint check` is a drop-in for `npm run lint && npx
  dprint check && npm run knip`, with the static checks as a bonus.

---

## What needs new alint primitives

Two patterns specific to TypeScript that don't fit any current rule:

### 1. `pair_count` (assert ≥1 partner files match a registry entry)

`scripts/errorCheck.mjs` checks that every diagnostic in
`src/compiler/diagnosticMessages.json` appears in **at least one**
`tests/baselines/reference/*.errors.txt`. The current `pair` rule
asserts a 1:1 relationship; this needs **N:1** with a presence-only
check ("any baseline file mentions error code X").

Generalised use case: **"every entry in this registry must be used at
least once in this file set"**. Shows up in: i18n string usage,
diagnostic codes, API endpoint routes, feature flags. Worth a v0.10+
design pass — same shape as the existing `unique_by` rule but
inverted (presence-required rather than uniqueness).

### 2. `bundled_size_diff` / `cross_ref_diff` (out of scope, mentioned for completeness)

`scripts/checkPackageSize.mjs` runs `npm pack --dry-run --json`
against two refs and flags >10% growth. Cross-ref diff is structurally
out of alint's "lint the working tree" scope — alint sees one tree at
a time. The closest in-scope analogue would be a `file_max_size`
on the tarball-inputs directory, but the meaningful check ("growth
relative to base") needs git-history awareness.

Same class of rule: `pr-modified-files.yml`'s "files-changed in this
PR" gate. Both belong in CI orchestration, not in alint.

---

## What's out of alint's scope (kept on the existing tool)

The full inventory above flagged 8 of 14 surfaces as out of scope.
Listed by category for clarity:

- **AST analysis** (eslint, knip, the 9 custom eslint rules) — alint
  deliberately doesn't try to be a parser. Shell out via `command:`.
- **Codegen** (`processDiagnosticMessages.mjs`,
  `regenerate-unicode-identifier-parts.mjs`, `generateLibs`) —
  alint doesn't run codegen; the freshness check belongs to the
  build system.
- **Runtime probes** (`checkModuleFormat.mjs`) — alint reads files,
  it doesn't run them.
- **Cross-ref diffs** (`checkPackageSize.mjs`, `pr-modified-files.yml`,
  `baselines` accept loop) — alint sees one tree at a time.
- **Operational workflows** (release / nightly / sync / wiki) — not
  validation surfaces.

---

## Performance comparison (placeholder — bench when validation pass scales)

The repo is large enough to be a meaningful stress test:
- **~7000** TS source files under `src/` (the compiler itself)
- **~53000** baseline files under `tests/baselines/reference/`
- **~140 MiB** of working-tree content (after sparse-checkout of the
  no-baselines subset; full clone is >500 MiB)

The published S3 bench (100k files, mixed languages) hits 1.13 s for
the workspace bundle on a stock CI runner. The TS repo at full size
sits between S3 and S9 (the polyglot monorepo bench, 100k+ files).
Expected: 1-3 s for `alint check` on the full TS tree, vs. 30-60 s
for the eslint cold cache and another 30 s for knip + dprint.

Where alint shines on TS specifically: the **baseline file-size
guard** runs against 53k files in tens of milliseconds (sequential
shell would be ~30 s of `wc -c` calls). The cross-cutting structural
checks pay back the most when the repo size is dominated by a single
homogeneous directory like `tests/baselines/reference/`.

---

## Recommendation for the launch story

This case study is **the "famous, frozen, meticulously curated" data
point** for the launch:

- TypeScript is the most-watched JS-tooling repo on GitHub. Naming
  it as a target gives alint instant credibility with the JS audience.
- The maintenance-mode posture means the structural-validation
  surface is stable — what alint enforces today will still be the
  right check 12 months from now.
- The header / pairing / file-size rules add genuinely new
  enforcement (TS doesn't have these today). Easy "alint caught X
  the existing tooling missed" anecdote.
- The `pair_count` primitive surfaced here is the same shape as
  patterns in airflow (`check-no-new-airflow-exceptions`,
  `check-template-fields-valid`) — a genuine v0.10+ feature
  request, not a one-off.

Position it as the third tile on alint.org/examples (after
kubernetes + airflow), with the angle: "for repos that already have
their lint house in order, alint adds the structural floor under
their existing tools."

Followup feature work surfaced (consolidated):

- **`pair_count` rule kind** (assert ≥1 partner files match a
  registry entry) — would cover `errorCheck.mjs` here, plus the
  airflow `check-no-new-*` family.
- **`json_path_matches` documentation note**: TypeScript's
  `tsconfig.json` files are JSONC (with comments). Worth surfacing
  in the structured-path rule docs that the JSON variant tolerates
  comments — the current docs don't make this explicit and a writer
  reading the rule kind name would assume strict JSON.

No new schema or language pitfalls hit while writing this config —
the 21 documented in `docs/development/CONFIG-AUTHORING.md` cover
everything that came up. The closest near-miss was the JSONPath
match-function form for the workflow-pinning rule (`?match(@.uses,
'^...')`), which is documented as the "honourable mention" at the
end of the pitfalls doc — needed to read it twice to get the
nesting right, but the doc carries the canonical form.

---

## Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`agent-context@v1` adoption.** TypeScript ships a load-bearing
   `AGENTS.md` (the maintenance-mode marker is enforced by
   `ts-agents-md-maintenance-marker`); the bundled `agent-context@v1`
   ruleset (5 rules) would absorb the `ts-agents-md-present` rule and
   assert the broader canonical agent-context shape (CLAUDE.md, .cursor/,
   etc.) without per-repo restatement.
2. **`pair_count` (≥1 partner files match a registry entry).** Surfaced
   here as the canonical example of `errorCheck.mjs` — every diagnostic
   in `src/compiler/diagnosticMessages.json` appears in at least one
   `tests/baselines/reference/*.errors.txt`. Same shape arose in airflow
   (`check-no-new-airflow-exceptions`); 2 sources, design candidate for
   v0.10+ once the `cross_file_value_equals` ship-target lands.
3. **`hygiene/lockfiles@v1` overlay.** TypeScript ships `package-lock.json`
   + `package.json`; the bundled `hygiene/lockfiles@v1` ruleset (7 rules)
   would catch nested-lockfile drift, mismatched lockfile-versions, and
   the orphan-lockfile pattern (a lockfile with no sibling `package.json`)
   that the existing CI doesn't gate today.

---

## Validation status (2026-05-07)

- **alint version:** 0.9.17 (1dbd9b218a0e, built 2026-05-07)
- **Rule count:** 68 (~22 custom + 6 bundled rulesets — `oss-baseline` 15,
  `node` 9, `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11,
  `tooling/editorconfig` 3, `agent-context` 5; rule IDs may overlap)
- **`validate-config`:** ✓ Config valid: 68 rule(s) loaded
- **Live-tree recheck:** not performed in this batch (typescript
  sparse-checkout not present in `/tmp/`)
- **Pitfall fixes (v0.9.17):** Pitfalls #18 + #19 do not apply here (no
  tracked-but-gitignored files, no `root_only: true` + multi-component
  literal entries)
- **Open gaps (unchanged):** `pair_count` (v0.10+ design candidate),
  `bundled_size_diff` / `cross_ref_diff` (out of scope; PR-diff aware).
  No new rule-kind gaps surfaced in this revalidation
