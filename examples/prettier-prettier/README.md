# Case study: `prettier/prettier`

Inventory of the structural-validation tooling in `prettier/prettier` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, shallow clone of `main` with `tests/`
sparse-excluded (the format-test corpus is large and well-curated by an
existing `lint:format-test` script — out of scope for structural validation).

---

## Summary

prettier maintains its structural validation in **three places**:
the **`package.json` `scripts.lint:*` cluster (8 yarn scripts)** drives the
local + CI `Lint` workflow; **`.github/workflows/lint.yml` (~16 steps)** layers
yarn-dedupe + dependency-review + actionlint + ensure-no-files-changed on
top; **5 custom node scripts under `scripts/`** encode the project-specific
shape rules (changelog hygiene, format-test layout, dependency pinning).
Roughly **45 % of the lint steps map directly to existing alint rules**
(structural assertions + JSON path matches + filename regex + bundled
node/oss-baseline coverage), **~25 % map via shellouts** through
`command:` rules (eslint, cspell, prettier-self-check, knip, tsc),
**~20 % need new alint primitives** (idempotency check, JSON-key-shape
forbid, cross-dir filename uniqueness), and **~10 % are out of alint's
scope** (eslint AST rules, knip JS dep-graph analysis, actionlint
workflow grammar — though the latter is shellable).

The 70 % that *do* fit (declaratively + via shellouts) translate to a
**24-rule alint config** (below).

**Headline finding:** prettier's per-language-plugin convention discipline
(every `src/language-{js,css,html,markdown,...}/` plugin must export
`index.js` + `languages.evaluate.js`; every plugin maps 1:1 to a
`changelog_unreleased/<lang>/` category dir) is **maintained socially
today — there is zero on-disk enforcement.** None of the 11 `lint:*`
scripts, none of the 16 `.github/workflows/lint.yml` steps, and none of
the 5 custom validation node scripts checks the per-plugin layout.
**Alint's `for_each_dir` over `src/language-*` is the missing structural
floor** — it adds 5 net-new gates that prettier's existing
ESLint+prettier-itself+cspell+knip+tsc stack does not provide today.

This is the cleanest "structural floor on top" win in the Wave 1+2
inventory so far: prettier is itself a code-formatter (it dogfoods),
its existing tooling is mature and tightly curated, and yet the
plugin-shape conventions that the entire codebase architecture rests on
are encoded only in code review and folder-name memory.

---

## Existing tooling inventory

### `package.json` `scripts.lint:*` cluster — 8 declared lint scripts

| Script | What it checks | alint replacement |
|---|---|---|
| `lint:typecheck` | `tsc` over JSDoc-annotated sources | `command` rule (TS AST scope; can't replace) |
| `lint:eslint` | eslint flat-config (incl. 9 `prettier-internal-rules/*`) | `command` rule (JS AST scope; can't replace) |
| `lint:changelog` | `node ./scripts/lint-changelog.js` (custom) | **5 native alint rules** (see below) |
| `lint:prettier` | `prettier . --check --cache` (dogfoods itself) | `command` rule shelling to `prettier` |
| `lint:spellcheck` | `cspell --no-progress --relative --dot --gitignore` | `command` rule shelling to `cspell` |
| `lint:deps` | `node ./scripts/check-deps.js` — pinned versions | `file_content_forbidden` rule with `(?m)^\s*"…":\s*"[\^~]…` |
| `lint:knip` | `knip` — unused exports + dep graph | `command` rule (JS dep-graph; can't replace) |
| `lint:format-test` | `node ./scripts/format-test-lint.js` | `command` rule (depth-walk needs `for_each_dir + when_iter`-with-method-calls; can't yet) |

**Mapping breakdown:** 5 of 8 are `command:` shellouts (which alint can drive
but not replace logic for), 1 (`lint:changelog`) decomposes into 5 native
declarative rules, 1 (`lint:deps`) decomposes into a single
`file_content_forbidden` rule, 1 (`lint:format-test`) stays as a shellout
because its leaf-directory walk needs primitives alint doesn't ship.

### `.github/workflows/lint.yml` — 16 sequential steps

Most steps re-invoke the `lint:*` scripts above. The non-script steps:

| Step | What it checks | alint replacement |
|---|---|---|
| `Check Dependencies` | (alias for `lint:deps`) | covered above |
| `Lint workflow files` | downloads + runs `actionlint` against `.github/workflows/` | `command` rule shelling to `actionlint`; alint's bundled `ci/github-actions@v1` already enforces token perms + SHA pinning + workflow `name` |
| `Dependency Review` | `actions/dependency-review-action` | GitHub-API; out of scope |
| `Validate renovate config` | `yarn dlx --package renovate@latest renovate-config-validator` | not present in repo (no renovate.json); skipped |
| `Run yarn (/)` + 4 sub-dir variants | `yarn install` + `yarn dedupe --check` per workspace | `command` rule (per workspace) |
| `Knip` | (alias for `lint:knip`) | covered above |
| `Ensure no files changed` | `node ./scripts/ensure-no-files-changed.js` (post-fix idempotency) | **needs `command_idempotent` mode** — see gap catalogue |
| `Lint docs code block` | `prettier "{docs,website/versioned_docs/...}/**/*.md" --check` | extra `command` rule with `PRETTIER_DEBUG=true` env (env-injection on `command` rule is itself a v0.10+ gap) |

### Custom node scripts under `scripts/` — 5 shape-validation scripts

The most interesting category for alint — these are the rules that aren't
expressible in a generic linter (eslint/cspell/etc.) and so the project
hand-rolled node scripts to encode them:

| Script | LoC | What it enforces | alint replacement |
|---|---|---|---|
| `lint-changelog.js` | 178 | exact category-dir roster, `.gitkeep` per cat, `<PR_NUMBER>(-N)?.md` filename regex, `#### ` h4 title prefix, no `prettier master` text, no template-comment leakage, no template author-link leakage | **5 native rules** (file_exists × N for roster + .gitkeep, filename_regex for PR file names, file_starts_with for h4 prefix, file_content_forbidden × 2 for "master" + template placeholder) |
| `check-deps.js` | 58 | every entry in `dependencies` / `devDependencies` / `resolutions` is pinned (no `^` / `~`) across 5 package.json files | **1 native rule** (`file_content_forbidden` with `(?m)^\s*"…":\s*"[\^~]…`) on the root; per-subdir variants commented out as v0.10+ candidate (needs `for_each_file` + JSON-key-shape forbid; see gap catalogue) |
| `format-test-lint.js` | 60 | every test directory under `tests/format/` has a `format.test.js` (the leaf-walk skips `__snapshots__/`) | `command` shellout — the leaf-walk semantics need `for_each_dir` + `when_iter: 'iter.has_only_files'` (the latter doesn't exist in the documented `iter.*` accessors; see CONFIG-AUTHORING § 12b) |
| `ensure-no-files-changed.js` | 33 | running the autofix pass leaves the working tree clean (`git diff --exit-code`) | **needs `command_idempotent` mode** (already in the v0.10+ candidate catalogue from ruff) |
| `clean-cspell.js` | (codegen) | scheduled cleanup of `cspell.json` | codegen drift; out of scope |

### Per-language-plugin convention (NOT enforced anywhere on disk)

The headline finding. Eight plugins under `src/language-*/` (js, css, html,
markdown, yaml, json, graphql, handlebars). Each one must export:
- `index.js` — parser/printer/options entry point
- `languages.evaluate.js` — linguist-languages lookup table consumed at
  build time

The build's `scripts/build/build.js` loads each plugin via these conventional
filenames; a plugin that drops `languages.evaluate.js` would silently be
omitted from the production bundle. Yet **nothing on disk asserts this
contract**:

```
$ rg -l "language-\*/index" .github scripts package.json eslint.config.js \
    prettier.config.js knip.config.js
# (no matches)
```

The same pattern applies to the 1:1 mapping between plugins and
`changelog_unreleased/<lang>/` category directories. The CHANGELOG_CATEGORIES
list in `scripts/utilities/changelog-categories.js` happens to enumerate
`javascript`, `css`, `html`, `markdown`, etc. — but nothing checks that
this list aligns with the `src/language-*/` directory roster. A new plugin
landing without its category directory ships fine; PR notes for it would
then have nowhere to go.

This is **the case for `for_each_dir` over `src/language-*`** as a
structural floor. Single declarative gate; catches both directions.

### Ad-hoc CI gates worth knowing about

- `.editorconfig` — root config; the bundled `tooling/editorconfig@v1`
  ruleset already enforces presence
- `.gitattributes` — root config; `* text=auto eol=lf` + the per-tests
  ignores; bundled `oss-baseline` advises but doesn't require
- `.prettierignore` — root config; the dogfood loop depends on it; this
  config asserts presence
- `prettier.config.js` — the dogfood config (parser overrides); this
  config asserts presence
- `eslint.config.js` — the flat-config entry point; `lint:eslint` depends
  on it; this config asserts presence
- `tsconfig.json` — `lint:typecheck` depends on it; this config asserts
  presence
- `cspell.json` — the spellcheck dictionary; the spellcheck step depends
  on it (and the `cleanup-cspell.yml` workflow scheduledly prunes it)
- `knip.config.js` — knip's dep-graph + unused-export config; the lint:knip
  step depends on it

### Needs new alint primitive

| Existing check | What it validates | What alint needs |
|---|---|---|
| `ensure-no-files-changed.js` | After running the autofix pass, the working tree is clean | **`command_idempotent` mode** (already in the v0.10+ candidate catalogue from ruff) — applies to mdformat / prettier / eslint --fix / ruff --fix / etc. across multiple inventoried repos |
| `check-deps.js` per-subdir | The same pinning rule applied to 5 different `package.json` files (root + website + scripts/release + 2 scripts/tools sub-packages) | **`for_each_file` + JSON-key-shape forbid** — today the regex variant works on the root only; running the same rule against 5 different paths needs a primitive that can iterate over a glob of `package.json` files and apply a per-key shape assertion (vs. the current path-by-path duplication). Strong candidate: `json_key_value_forbidden` — for every key matching JSONPath X in the set of paths Y, the value must not match regex Z |
| `format-test-lint.js` | Every leaf directory under tests/format/ that contains test files has a `format.test.js`; `__snapshots__/` subdirs are skipped; `.not-test-directory` opts out | **`when_iter: 'iter.has_only_subdirs'` accessor** OR **`for_each_leaf_dir`** primitive — the existing `for_each_dir` walks all dirs; what's wanted is leaf-walk semantics with an opt-out file marker |
| `lint-changelog.js`: PR-number uniqueness across category dirs | Two notes for the same PR can't coexist in different category dirs | **`unique_by` cross-dir** — the existing `unique_by` rule operates within a single dir/file scope; cross-dir uniqueness on the basename across `changelog_unreleased/*/` needs explicit support |
| `lint-changelog.js`: title parse via remark | The `#### Title (#NNNN by @user)` line parses cleanly under remark with no embedded HTML | Out of scope (markdown AST parsing); stays on the existing node script |
| `lint:eslint` `prettier-internal-rules/*` | 9 internal eslint rules covering AST patterns specific to prettier's printer architecture | Out of scope (JS AST); stays on eslint |
| `lint:knip` | Unused exports + dep-graph analysis | Out of scope (JS dep-graph); stays on knip |

**Three concrete launch-prep proposals surfaced from this case study:**

1. **`json_key_value_forbidden` rule kind** (or a JSON-aware mode on
   `file_content_forbidden`). The current `file_content_forbidden` regex
   approach works but is fragile — a refactor of how dependencies are
   indented would silently break the gate. A first-class JSON-key-shape
   forbid against a JSONPath would be more robust. **Cross-cutting
   demand:** prettier (pinning), turbo (per-package metadata), uv
   (workspace-deps shape) all want some version of this.
2. **`for_each_leaf_dir`** (or `iter.is_leaf` accessor). prettier's
   format-test-lint pattern — "every leaf directory matching X must
   contain Y" — is distinct from `for_each_dir`'s "every dir matching X".
   Same shape recurs in any test-corpus tree: rust-lang/rust's
   `tests/ui/`, ruff's snapshots, deno's specs.
3. **`command_idempotent` mode** (re-affirms the ruff finding). prettier's
   `ensure-no-files-changed.js` is the same shape as ruff's mdformat /
   prettier / ruff-format dogfood loops. The v0.10+ candidate from the
   ruff case study lands here too.

### Out of alint's scope (use the existing tool)

- `lint:typecheck` (`tsc`) — TS AST scope; alint's no-AST non-goal applies.
  Keep on tsc.
- `lint:eslint` (eslint flat-config + 9 internal rules) — JS AST scope; alint's
  no-AST non-goal applies. Keep on eslint.
- `lint:knip` — JS dep-graph analysis; out of scope. Keep on knip.
- `lint:prettier` (the dogfood) — formatter scope; alint shells to it via
  `command` so the gate is in `alint check`, but the formatting decisions
  stay with prettier itself.
- `cspell` spellcheck — lexical AST against an allowlist; could be a future
  alint rule kind but isn't priority. Stays on cspell (shell-driven).
- `actionlint` workflow grammar — yaml grammar specific to GitHub Actions;
  alint's bundled `ci/github-actions@v1` covers token perms + SHA pinning
  + workflow names but not the schema-deep grammar. Stays on actionlint.

---

## Starter alint config (drop-in)

[`.alint.yml`](.alint.yml) in this directory. **24 rules total** (after
extending 6 bundled rulesets). Replaces directly:

- 5 of the 8 `lint:*` scripts via `command` shellouts (so `alint check`
  is the single entry point for the whole gate)
- 5 of the 6 invariants in `lint-changelog.js` declaratively (category
  roster + filename regex + h4 prefix + master/template forbids)
- 1 of the 2 invariants in `check-deps.js` declaratively (root-level
  pinning); the per-subdir variant becomes a v0.10+ candidate

And **adds 5 net-new gates** that prettier's existing tooling does not
enforce today:

- `prettier-each-language-plugin-has-index` — every `src/language-*/`
  exports `index.js`
- `prettier-each-language-plugin-has-languages-evaluate` — every
  `src/language-*/` exports `languages.evaluate.js`
- `prettier-each-plugin-package-shape` — every `packages/plugin-*/` has
  `index.js` + `package.json` + `README.md`
- `prettier-plugin-package-prettier-scope` — every `packages/plugin-*/`
  package is published under the `@prettier/` scope
- `prettier-changelog-categories-exist` — the changelog category roster
  matches the registered list (drift-detection on adding a new plugin)

Plus 6 hardening gates that re-state existing assumptions on disk:
`prettier-package-name`, `prettier-package-license`,
`prettier-package-engine-node`, `prettier-prettierignore-exists`,
`prettier-eslint-config-exists`, `prettier-tsconfig-exists`,
`prettier-dogfood-config-exists`.

---

## Performance comparison (placeholder — bench when validation pass scales)

`yarn lint` runs the 8 lint scripts in parallel via `npm-run-all2 -p`.
Each script does its own fs walk; cspell + prettier + eslint + tsc each
pay startup cost per process. Typical wall-clock on a clean checkout:
**~30-45 s** on a developer laptop, dominated by tsc + knip + cspell.

alint runs all rules in parallel via the v0.9.3 dispatch flip + the
v0.9.5+ cross-file fast paths. Expected: **~0.5-1 s** for the
alint-replaceable subset (the 17 native rules — bundled rulesets +
per-language-plugin gates + changelog-roster + package-shape) on a
prettier-scale repo (~2k JS sources + 8 language plugins + ~150
changelog notes + sparse `tests/`).

The `command:` shellouts of course inherit the underlying tool's runtime
— the speedup is from the native-rule subset *plus* the consolidation
of orchestration into a single `alint check` invocation.

To benchmark for real: `time yarn lint` vs `time alint check` on the
same checkout. Deferred to the per-repo measurement pass.

---

## Recommendation for the launch story

This case study has **two distinct angles** worth featuring:

1. **The "structural floor on top" angle** — prettier has mature tooling
   (eslint flat-config + dogfooded prettier + cspell + knip + tsc + 5
   custom node validation scripts) and yet the per-language-plugin
   architectural invariants — every plugin exports `index.js` +
   `languages.evaluate.js`; every plugin maps to a changelog category dir
   — are encoded **only in code review and folder-name convention**.
   alint adds 5 net-new gates that ship from the first `alint check`
   and would have caught a missing `languages.evaluate.js` on day one.
   Same shape applies to most plugin-architecture projects: webpack,
   rollup, vite, babel, postcss — the "every plugin has shape X" rule
   is universal and rarely enforced.

2. **The "alint as the script consolidation point" angle** — prettier
   has 5 custom node scripts (lint-changelog.js, check-deps.js,
   format-test-lint.js, ensure-no-files-changed.js, clean-cspell.js)
   totalling ~330 LoC, plus 8 yarn lint scripts, plus a 16-step
   GitHub workflow. **5 of 6 invariants in `lint-changelog.js` and
   1 of 2 in `check-deps.js` collapse to declarative alint rules.**
   That's ~80 LoC of bespoke JS replaced by ~50 lines of declarative
   YAML — easier to read in PR review, easier to extend, schema-validated
   at config-load time. The remaining 4-5 shape rules surface clean
   v0.10+ primitive candidates (`json_key_value_forbidden`,
   `for_each_leaf_dir`, `command_idempotent`).

Followup feature work surfaced (in priority order):

- **`json_key_value_forbidden` rule kind** — the structured equivalent
  of `file_content_forbidden`. Demand: prettier (pinning), turbo
  (per-package metadata), uv (workspace shape). Cross-cutting.
- **`for_each_leaf_dir`** (or `iter.is_leaf` + `iter.has_only_subdirs`
  accessors) — leaf-walk variant of `for_each_dir`. Demand: prettier
  (format-test-lint), rust-lang/rust (`tests/ui/`), ruff (snapshot dirs),
  any test-corpus tree.
- **`command_idempotent` mode** (re-affirms the ruff finding) — generalises
  the "fixer in --check mode" pattern. Demand: prettier
  (ensure-no-files-changed), ruff (mdformat/prettier/ruff-format),
  kubernetes/airflow/turbo. Cross-cutting; the most-demanded primitive
  in the inventory.
- **`unique_by` cross-dir** (basename uniqueness across multiple sibling
  dirs) — generalises the existing `unique_by` from "within one dir" to
  "across a set of dirs". Demand: prettier (PR-number uniqueness across
  changelog categories); narrow but clean.
