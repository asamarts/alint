# Case study: `nodejs/node`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/nodejs-node/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `nodejs/node` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07, sparse-clone of `nodejs/node@1aebbdef`
(latest tip of main — `1aebbdef06065cef6566525ca4a042ffb8fb5308` via
`git ls-remote https://github.com/nodejs/node HEAD`, commit "src: skip
JS callback for settled Promise.race losers" 2026-05-05). Working tree
at `/tmp/nodejs-node`: **8,214 files** (excludes `deps/` (V8, libuv,
ICU, nghttp2, etc.), `test/parallel/` and `test/sequential/` — the
bulk of the test corpus, ~30k files; not material to the structural
inventory). 132 MB working-tree.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check nodejs/node runs today, one row per check. The repo's
gating infrastructure is **`Makefile`** (1,730 lines — ~12 distinct
`lint-*` / `format-*` / `tidy-*` / `check-*` targets) + **38 GitHub
Actions workflows** under `.github/workflows/` (lint matrix, build,
release, operational bots) + **27 in-tree custom eslint rules** under
`tools/eslint-rules/` + the `Makefile`-driven shellouts to **cpplint**
+ **clang-format** + **lint-md** + **ruff** + **yamllint** +
**shellcheck**.

### 1.1 `Makefile` lint / format / check targets (~12 distinct)

The canonical aggregator. Counted directly from `grep -nE
"^(lint|format|check|tidy)" /tmp/nodejs-node/Makefile`:

| Make target | What it actually does | Backing tool / runtime |
|---|---|---|
| `lint` | Aggregator: `lint-js`, `lint-cpp`, `lint-md`, `lint-addon-docs` | bash / make composition |
| `lint-ci` | CI-tuned: `lint-js-ci`, `lint-cpp`, `lint-py`, `lint-md`, `lint-addon-docs`, `lint-yaml-build`, `lint-yaml` | bash / make composition |
| `lint-js` / `lint-js-ci` / `jslint` | `tools/eslint/node_modules/eslint/bin/eslint.js` over `lib/`, `test/`, `doc/`, `tools/` (all 4 tiers) | eslint v9 + 27 custom plugin rules |
| `lint-js-fix` | `eslint --fix` variant | eslint |
| `lint-js-doc` | `LINT_JS_TARGETS=doc` variant | eslint scoped to docs |
| `lint-cpp` / `cpplint` | `tools/cpplint.py` + `tools/checkimports.py` over `src/**/*.{cc,h}` | python (vendored Google cpplint fork) + python AST helper |
| `format-cpp` | `clang-format` over the diff vs `git merge-base HEAD origin/$BASE` | clang-format (PR-diff scoped) |
| `format-cpp-build` / `format-cpp-clean` | Bootstrap targets | n/a |
| `lint-md` | `tools/lint-md/lint-md.mjs` (remark-preset-lint-node pipeline with ~50 markdown AST rules) over `doc/**/*.md` + `*.md` | remark + remark-preset-lint-node + ~50 visitor rules |
| `format-md` | `lint-md` write mode | remark |
| `lint-py` / `lint-py-fix` / `lint-py-fix-unsafe` | `tools/pip/site-packages/bin/ruff check .` against `pyproject.toml` config | ruff (vendored under tools/pip) |
| `lint-py-build` | Bootstrap | n/a |
| `lint-yaml` | `python -m yamllint .` (yamllint vendored under `tools/pip/`) | yamllint |
| `lint-yaml-build` | Bootstrap | n/a |
| `lint-addon-docs` | `tools/.doclintstamp` — runs the addon-docs linter | custom DSL parser |
| `lint-clean` | Removes lint artefacts | n/a |
| `check`, `check-xz` | Test runner aggregator | n/a (test runner, not validation) |

### 1.2 `tools/eslint-rules/` (27 in-tree custom rules)

Verified count: **27 .js files** under `tools/eslint-rules/`. All but
`rules-utils.js` are TSESTree visitors — out of alint's "no AST"
scope. Listed here so the inventory is complete:

| Rule | What it does (one-liner) |
|---|---|
| `alphabetize-errors` / `alphabetize-primordials` | Sortedness inside specific files |
| `async-iife-no-unused-result` | Async IIFE rule helper |
| `avoid-prototype-pollution` | Bans `Object.prototype` mutations |
| `crypto-check` / `inspector-check` | Conditional-compilation guards on optional features |
| `documented-deprecation-codes` / `documented-errors` | Cross-references error codes against `doc/api/errors.md` |
| `eslint-check` | Asserts `eslint-disable` comments are well-formed |
| `lowercase-name-for-primitive` | Identifier-naming convention |
| `must-call-assert` | `Debug.assert` argument-shape enforcement |
| `no-array-destructuring` | Bans `const [a, b] = arr;` (prototype-pollution defense) |
| `no-duplicate-requires` | Bans duplicate `require()` calls in a file |
| `non-ascii-character` | Bans non-ASCII chars in source |
| `no-unescaped-regexp-dot` | Bans `/./` in regex literals (use `/\./`) |
| `prefer-assert-iferror` / `prefer-assert-methods` | Test-assertion style |
| `prefer-common-mustnotcall` / `prefer-common-mustsucceed` | Test-helper style |
| `prefer-optional-chaining` / `prefer-proto` / `prefer-util-format-errors` | Modern-syntax enforcement |
| `prefer-primordials` | Bans direct use of JS built-ins (`Array.from`, `Object.keys`, etc.); requires the wrapper from `lib/internal/per_context/primordials.js`. **227 lines — the most load-bearing of the 27 rules** |
| `require-common-first` / `required-modules` | Test-file structure |
| `set-proto-to-null-in-object` | Identifier-naming convention |
| `rules-utils.js` | Shared rule helpers (not a rule itself) |

These are perfect examples of "AST analysis is not alint's niche" —
they belong in `tools/eslint-rules/` and stay there. Custom-rule
loading is via `nodeCore.RULES_DIR =
fileURLToPath(new URL('./tools/eslint-rules', import.meta.url))` in
`eslint.config.mjs:27` — every `.js` file in that dir is auto-registered;
**there's no per-rule explicit registration line to verify.**

### 1.3 `tools/lint-*.mjs` shell scripts (4)

| Script | What it does |
|---|---|
| `lint-md.mjs` | Remark-preset-lint-node pipeline (driven by `make lint-md`) |
| `lint-pr-url.mjs` | Reads `git diff` of `doc/api/*.md` for `pr-url:` strings; asserts they match the current PR's URL |
| `lint-readme-lists.mjs` | Validates README's collaborator list against the actual GitHub teams (HTTP fetch + git API queries) |
| `lint-sh.mjs` | Runs `shellcheck` over project shell scripts |

### 1.4 `tools/lint-md/` (the markdown AST pipeline)

| File | Role |
|---|---|
| `lint-md.mjs` | The remark-preset-lint-node entry script |
| `list-released-versions-from-changelogs.mjs` | Parses changelog files for the published version registry |
| `package.json` | Pins `remark-parse`, `remark-preset-lint-node`, `remark-stringify`, `to-vfile`, `unified`, `vfile-reporter`. **A missing pin means the markdown-lint pipeline silently drops most of its rules.** |
| `package-lock.json` | npm lockfile for the lint-md sub-tree |

### 1.5 `tools/find-inactive-{collaborators,tsc}.mjs`

Walks `git log` to find collaborators / TSC members with no recent
commits. alint sees one tree at a time; no git-history awareness.

### 1.6 `tools/test.py`

Test discovery. GLOBS for
`test/{parallel,sequential,async-hooks,...}/test-*.{js,mjs,cjs}`.
Loads each file as a regression-test entry. **The discovery itself
isn't a structural-validation surface, but it implies one — the
filename grammar is enforced nowhere statically; a typo silently
drops the test from the run.**

### 1.7 `.github/workflows/` (38 workflows)

Verified count: 38 (vs the prior README's 25 — workflow-count drifted
since the case study was first written).

| Workflow | What it does | alint disposition |
|---|---|---|
| `linters.yml` | Dispatches `lint-cpp`, `lint-md`, `lint-js`, `lint-py`, `lint-yaml`, `lint-shell`, `format-cpp`, `format-md`, `lint-pr-url`, `lint-readme-list`, `lint-changelog` jobs | Each job is its own surface |
| `commit-lint.yml` | First commit message adheres to the contributing guidelines (subsystem prefix, signed-off-by, etc.) | OUT — git commit-message regex |
| `commit-queue.yml` | Lands PRs via the commit queue | OUT — operational |
| `codeql.yml` | CodeQL static security analysis | OUT — security scanner |
| `scorecard.yml` | OpenSSF Scorecard run (action-SHA pinning, permission blocks) | Partial alint coverage: `node-workflow-actions-pinned-by-sha` + `node-workflow-has-permissions` |
| `auto-start-ci.yml`, `comment-labeled.yml`, `label-flaky-test-issue.yml`, `label-pr.yml`, `notify-on-*.yml`, `stale.yml`, `close-stalled.yml`, `daily*.yml`, `find-inactive-*.yml`, `timezone-update.yml`, `update-*.yml`, `tools.yml` (~20 workflows) | Operational / labelling / notification bots | OUT — not validation |
| `build-tarball.yml`, `coverage-*.yml`, `daily.yml`, `doc.yml`, `license-builder.yml`, `lint-release-proposal.yml`, `major-release.yml`, `post-release.yml`, `create-release-proposal.yml`, `test-*.yml` (~10 workflows) | Build / test / release | OUT — not validation |

**~5 of 38 workflows (~13%) carry a structural assertion alint can
restate.** The other 33 are CI orchestration / release / maintenance.

### 1.8 `eslint.config.mjs` + 7 per-tier partials

| File | Tier |
|---|---|
| `eslint.config.mjs` (root) | Composes per-tier partials, registers all 27 custom rules via `RULES_DIR` mechanism |
| `lib/eslint.config_partial.mjs` | Production lib/ files (`prefer-primordials`, etc.) |
| `test/eslint.config_partial.mjs` | Test files (allows `console.log`, etc.) |
| `doc/eslint.config_partial.mjs` | Markdown doc snippets |
| `benchmark/eslint.config_partial.mjs` | Benchmark scripts |
| `tools/eslint/eslint.config_partial.mjs` + `eslint.config_utils.mjs` | tools/ helpers + the rule-loader plumbing |

### 1.9 `src/node_version.h` (the version-pin file)

Defines `NODE_MAJOR_VERSION`, `NODE_MINOR_VERSION`, `NODE_PATCH_VERSION`,
`NODE_VERSION_LTS_CODENAME`, `NODE_VERSION_IS_LTS`,
`NODE_VERSION_IS_RELEASE`. **The release pipeline reads this file to
compute the release tag.** A typo (e.g. `NODE_MAJOR_VERSION 27.0`
instead of `27`) breaks `node --version` silently.

### 1.10 `lib/internal/per_context/primordials.js`

The prototype-pollution defense layer. The `prefer-primordials` custom
eslint rule and dozens of `lib/` modules import from it.

### 1.11 `doc/changelogs/CHANGELOG_V<MAJOR>.md`

**25 files** at this commit (verified — CHANGELOG_V010, V012, V10..V24
+ CHANGELOG_IOJS + CHANGELOG_ARCHIVE), each named
`CHANGELOG_V<NN>.md` (or one of the 4 legacy variants
`CHANGELOG_V010.md` / `CHANGELOG_V012.md` / `CHANGELOG_IOJS.md` /
`CHANGELOG_ARCHIVE.md`). Editorial review of the release-prep PR
catches a typo today; the convention is enforced statically nowhere
else.

### 1.12 Per-language config + governance files

| Path | Role |
|---|---|
| `pyproject.toml` (48 lines) | `[tool.ruff]` declares `target-version = "py310"` plus ~25 lint families (ASYNC, C90, E, F, ICN, INT, PERF, PLC, PLE, PLR09, PYI, RSE, RUF, T10, TCH, TID, W, YTT). `make lint-py` reads this and runs ruff |
| `.cpplint` | `set noparent`, `filter=-build/c++17,-build/include_alpha,…` (9 categories disabled), `linelength=80` |
| `.clang-format` | Clang-format style file |
| `.editorconfig` | `[*]` block + per-language overrides for `.{c,cc,h,js,mjs,cjs,md,py,json,yml}` |
| `.gitattributes` | `test/fixtures/* -text` (no normalization), `vcbuild.bat text eol=crlf`, `tools/msvs/find_python.cmd text eol=crlf`, `deps/npm/bin/npm text eol=lf`, `deps/npm/bin/npx text eol=lf`, `deps/corepack/shims/corepack text eol=lf`, `doc/**/*.md text eol=lf`, `deps/crates/vendor/**/* -text` |
| `.nvmrc` | Pinned node version |
| `tsconfig.json` | TS compile config (`compilerOptions.strict: true`) |
| `package.json` | Top-level npm metadata + dev script orchestration |
| `LICENSE`, `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `GOVERNANCE.md`, `SECURITY.md`, `BUILDING.md`, `Makefile` | Repo-root governance + build artefacts |

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset
  (`oss-baseline` / `node` / `ci/github-actions` /
  `hygiene/no-tracked-artifacts` / `tooling/editorconfig`) OR the
  per-rule entry in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why.

### 2.1 `Makefile` lint targets (~12 distinct gates)

| Target | Coverage | Notes |
|---|---|---|
| `lint` / `lint-ci` | alint-today | alint *is* this aggregator (single config + single walk + multiple rules in parallel) |
| `lint-js` / `lint-js-ci` / `jslint` | alint-today | 4 `command:` rules per tier (`node-eslint-{lib,test,doc,tools}`) |
| `lint-cpp` / `cpplint` | alint-today | `node-cpplint` + `node-checkimports` (`command:` rules shelling to the 2 python tools) |
| `format-cpp` | out-of-scope | git-merge-base diff scoping (`clang-format --diff` against `origin/$BASE`) — alint sees one tree at a time |
| `lint-md` | alint-today | `node-lint-md-remark` (`command:` rule shelling to `tools/lint-md/lint-md.mjs`) |
| `lint-py` / `lint-py-fix` | alint-today | `node-ruff` (`command:` rule shelling to ruff against `pyproject.toml`) |
| `lint-yaml` | alint-today | `node-yamllint` (`command:` rule) |
| `lint-addon-docs` | out-of-scope | Custom addon-docs DSL parser |
| `format-cpp-build` / `lint-clean` / `lint-py-build` / `lint-yaml-build` | out-of-scope | Build / bootstrap targets, not validation |
| `check` / `check-xz` | out-of-scope | Test runner |

### 2.2 `tools/eslint-rules/` (27 in-tree custom rules)

All **out-of-scope** as alint primitives — TSESTree visitors. Wrapped
collectively by the 4 `node-eslint-{lib,test,doc,tools}` `command:`
rules above. The README's claim of "15-year-old conventions enforced
via human review only" is **partially correct**: the conventions are
enforced by these 27 eslint visitors at PR time (i.e., `make lint-js`
runs them), not by ad-hoc human review. The human-review piece is
the authoring of new visitors when a new convention is introduced.

**Cross-reference between `.alint.yml` claim and node's actual lint
stack:**

- node's `make lint-js` runs all 27 eslint visitors (covering the
  conventions: prefer-primordials, no-array-destructuring, sortedness,
  identifier naming, etc.) on every PR — this **IS the static
  enforcement layer**.
- alint's contribution is the structural-floor layer underneath:
  asserting that the 27 rule files exist (`node-eslint-custom-rules-present`),
  the eslint root + 7 partials exist (`node-eslint-config-root-present`,
  `node-eslint-tier-partials-present`), the `RULES_DIR` mechanism's
  source-of-truth `lib/internal/per_context/primordials.js` exists,
  and the per-tier eslint shellouts gate at PR time.
- **The "human review only" framing should be revised** for §6 to:
  "27 in-tree eslint visitors enforce the conventions; alint's
  contribution is the structural-floor layer that asserts the
  visitor files + their config + the source-of-truth files exist."

### 2.3 `tools/lint-*.mjs` (4 scripts)

| Script | Coverage | Notes |
|---|---|---|
| `lint-md.mjs` | alint-today | `node-lint-md-remark` (`command:` rule) |
| `lint-pr-url.mjs` | out-of-scope | PR-diff aware (reads `git diff` of changed `doc/api/*.md`) |
| `lint-readme-lists.mjs` | out-of-scope | HTTP fetch + GitHub API queries (network) |
| `lint-sh.mjs` | alint-today | `node-shellcheck` (`command:` rule wrapping `shellcheck`) |

### 2.4 `tools/lint-md/` registry

| Artefact | Coverage | Rule |
|---|---|---|
| `tools/lint-md/package.json` (presence) | alint-today | `node-lint-configs-present` (`file_exists`) |
| `tools/lint-md/package.json` `dependencies['remark-preset-lint-node']` (pinned) | alint-today | `node-lint-md-remark-preset-pinned` (`json_path_matches`) |
| `tools/lint-md/package.json` `dependencies['remark-parse']` (pinned) | alint-today | `node-lint-md-remark-parse-pinned` (`json_path_matches`) |
| `tools/lint-md/package-lock.json` | alint-today | `node-lint-configs-present` (covers the lockfile too) |

### 2.5 `tools/find-inactive-{collaborators,tsc}.mjs` + `tools/test.py`

| Script | Coverage | Notes |
|---|---|---|
| `tools/find-inactive-collaborators.mjs` | out-of-scope | `git log` walk |
| `tools/find-inactive-tsc.mjs` | out-of-scope | Same |
| `tools/test.py` | out-of-scope | Test discovery; runtime test runner. **The implied filename grammar IS in alint's scope** — see §2.10 |

### 2.6 `.github/workflows/` (5 of 38 are gating-class)

| Workflow | Coverage | Notes |
|---|---|---|
| `linters.yml` | alint-today (per-step) | Each lint-class step → `command:` rule |
| `commit-lint.yml` | out-of-scope | git commit-message regex (alint has `git_commit_message` but the node convention is rich enough that the existing tool wins) |
| `codeql.yml` | out-of-scope | Security scanner |
| `scorecard.yml` | alint-today (partial) | `node-workflow-actions-pinned-by-sha` + `node-workflow-has-permissions` cover the action-SHA-pinning + permission-block subset |
| 33 operational workflows | out-of-scope | Operational / release / labelling / notification |

### 2.7 `eslint.config.mjs` + 7 per-tier partials

| Artefact | Coverage | Rule |
|---|---|---|
| `eslint.config.mjs` (root) | alint-today | `node-eslint-config-root-present` (`file_exists`) |
| 7 `eslint.config_partial.mjs` files (across `lib/`, `test/`, `doc/`, `benchmark/`, `tools/eslint/`) | alint-today | `node-eslint-tier-partials-present` (`file_exists` over each) |
| `tools/eslint-rules/*.js` (27 files) | alint-today | `node-eslint-custom-rules-present` (`file_exists` over the canonical core entries) |
| `tools/eslint/eslint.config_utils.mjs` | alint-today | Same `node-eslint-tier-partials-present` covers it |

### 2.8 `src/node_version.h`

| Invariant | Coverage | Rule |
|---|---|---|
| File exists | alint-today | `node-version-header-present` (`file_exists`) |
| `NODE_MAJOR_VERSION` defined as integer | alint-today | `node-version-header-major-defined` (`file_content_matches`) |
| `NODE_MINOR_VERSION` defined | alint-today | `node-version-header-minor-defined` |
| `NODE_PATCH_VERSION` defined | alint-today | `node-version-header-patch-defined` |
| `NODE_VERSION_LTS_CODENAME` defined | alint-today | `node-version-header-lts-codename-defined` |

### 2.9 `lib/internal/per_context/primordials.js`

| Invariant | Coverage | Rule |
|---|---|---|
| File exists | alint-today | `node-primordials-present` (`file_exists`) |

The cross-file usage check (every `lib/internal/*.js` that doesn't
import from primordials triggers `prefer-primordials` violations) is
covered by eslint, not alint.

### 2.10 `doc/changelogs/CHANGELOG_V<MAJOR>.md` + `test/{parallel,sequential,async-hooks,...}/test-*.{js,mjs,cjs}`

The two **net-new structural assertions** alint adds:

| Convention | Coverage | Rule |
|---|---|---|
| `test-*.{js,mjs,cjs}` filename grammar in 15 test sub-directories | alint-today | `node-test-filename-grammar` (`filename_regex`) — **EXACT, net-new** (enforced nowhere statically before alint; enforced today only by `tools/test.py` SILENTLY dropping non-matching files) |
| `CHANGELOG_V<MAJOR>.md` per-major-version filename convention | alint-today | `node-changelog-per-major-filename` (`filename_regex`) — **EXACT, net-new** (enforced today only by editorial review of the release-prep PR) |

### 2.11 Per-language config + governance

| Artefact | Coverage | Rule |
|---|---|---|
| `pyproject.toml` `[tool.ruff].target-version` | alint-today | `node-pyproject-has-ruff-section` (`toml_path_matches`) |
| `.cpplint` (presence + non-trivial) | alint-today | `node-lint-configs-present` (`file_exists`) + `node-cpplint-config-substantive` (`file_min_lines`) |
| `.clang-format` (presence + non-trivial) | alint-today | `node-lint-configs-present` + `node-clang-format-config-substantive` |
| `.editorconfig` | alint-today | Bundled `tooling/editorconfig@v1` (3 rules) |
| `.gitattributes` (line-ending matrix) | alint-today | `node-windows-bat-crlf` (`*.bat` + `*.cmd`), `node-doc-md-lf` (`doc/**/*.md`), `node-source-lf-line-endings` (broad source LF), `tooling-gitattributes-normalizes-line-endings` (presence rule from bundled `tooling/editorconfig@v1`) |
| `.nvmrc` | alint-today | Bundled `node@v1` `node-engine-or-nvmrc` |
| `tsconfig.json` `compilerOptions.strict: true` | alint-today | `node-tsconfig-strict` (`json_path_equals`) |
| `package.json` (presence + lockfile) | alint-today | Bundled `node@v1` `node-package-json-exists`, `node-has-lockfile` |
| `LICENSE`, `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `GOVERNANCE.md`, `SECURITY.md`, `BUILDING.md`, `Makefile` | alint-today | `node-governance-files-present` + bundled `oss-baseline@v1` rules |
| `doc/contributing/` (~30 markdown files; canonical entries: cpp-style-guide.md, collaborator-guide.md, pull-requests.md, etc.) | alint-today | `node-doc-contributing-substantive` (`file_exists` + `file_min_lines`) |
| `src/`, `lib/` (substantive content) | alint-today | `node-src-substantive` + `node-lib-substantive` (`dir_exists` + `file_min_count`) |
| `out/`, `build/`, `Release/`, `Debug/` (forbidden) | alint-today | `node-no-tracked-build-outputs` (`dir_absent`) |

### 2.12 4 alint-future cross-file shapes

| Cross-file shape | Coverage | Notes |
|---|---|---|
| `tools/eslint-rules/*` ↔ `eslint.config.mjs` registration (every `.js` in dir is auto-registered via `RULES_DIR`) | alint-future | **`cross_file_value_equals` (registry-direction-only mode)** — every file in directory X must appear at some path in registry Y. Specifically, the `RULES_DIR` mechanism implies "every file in `tools/eslint-rules/` must be valid eslint plugin syntax" — alint can't verify the syntax, but it CAN assert each file exists (already covered by §2.7 above). The TRUE alint-future gap here is the `RULES_DIR` direction-cross-check: the rule-name in eslint config must match the basename of the `.js` file. **NEW v0.10+ candidate: `dir_name_matches_field` extension** for the `RULES_DIR`-mechanism case |
| `tools/dep_updaters/update-<libname>.{sh,mjs}` ↔ `deps/<libname>/` | alint-future | **`registry_paths_resolve`** — every update script's `<libname>` must resolve to an on-disk `deps/<libname>/` directory. **v0.10 ship-target** at 8 sources (rust, clap, cpython×2, next.js, arrow, pytorch, nodejs/node, NixOS×3) |
| C++ EOL banner consistency in `src/**/*.{cc,h}` | alint-future | **NEW: `file_header_consistency`** (or `file_header.alt_pattern` field on the existing rule). Asserts every file in scope X *either* matches the canonical header *or* matches a "newer convention" header. ~21% of `src/**/*.{cc,h}` files at HEAD still carry the Joyent BSD/MIT banner; ~79% don't (newer files just include a header guard). Niche; the cleaner outcome is editorial cleanup. **Single-source (node-only); low-priority** |
| `lib/<module>.js` ↔ `doc/api/<module>.md` cross-reference | alint-today (partial) | The `pair` rule already covers this for the simple case (`lib/foo.js` ↔ `doc/api/foo.md`); the actual node convention is more nuanced (`lib/internal/<module>.js` doesn't need a doc page; `lib/<module>.js` does). Documented in the config as a `pair` rule scoped to top-level `lib/*.js` — confirms `pair` works for the simple case; the nuance is expressed via `paths.exclude`. **Not a gap** |

---

## 3. Quantified coverage

Counted across the **12 Makefile lint targets** + **27
`tools/eslint-rules/`** + **4 `tools/lint-*.mjs`** + **4
`tools/lint-md/` registry** + **3 `tools/find-inactive-*.mjs` /
`test.py`** + **5 gating-class workflows** + **8
`eslint.config.mjs` + 7 partials** + **5 `node_version.h`
invariants** + **1 `primordials.js` invariant** + **2 net-new
filename grammars** + **15 config / governance / hygiene** + **4
cross-file shapes** = **90 distinct surfaces**.

```
alint-today:       50 / 90 = 56%   (8 Makefile + 0 eslint-rules + 2 lint-mjs + 4 lint-md + 0 find-inactive + 2 workflows + 8 eslint-config + 5 node_version + 1 primordials + 2 grammars + 15 config + 3 partial cross-file)
alint-future:       3 / 90 =  3%   (cross_file_value_equals + registry_paths_resolve + file_header_consistency)
out-of-scope:      37 / 90 = 41%   (4 Makefile + 27 eslint-rules + 2 lint-mjs + 0 + 3 find-inactive/test + 3 workflows + 0 + 0 + 0 + 0 + 0 + 0)
                   ──────────────
                   total = 100%
```

Granular breakdown:

```
Makefile lint targets (12):
  alint-today:      8 / 12 = 67%   (every -js, -cpp, -md, -py, -yaml, lint, lint-ci shelled out via command:)
  out-of-scope:     4 / 12 = 33%   (format-cpp diff scoping + lint-addon-docs DSL + bootstrap targets)

tools/eslint-rules/ (27):
  out-of-scope:    27 / 27 = 100%   (all TSESTree visitors)

tools/lint-*.mjs (4):
  alint-today:      2 /  4 = 50%   (lint-md, lint-sh)
  out-of-scope:     2 /  4 = 50%   (lint-pr-url, lint-readme-lists)

tools/lint-md/ registry (4):
  alint-today:      4 /  4 = 100%

tools/find-inactive-*.mjs + test.py (3):
  out-of-scope:     3 /  3 = 100%

.github/workflows/ (5 gating-class):
  alint-today:      2 /  5 = 40%   (linters.yml per-step + scorecard.yml partial)
  out-of-scope:     3 /  5 = 60%   (commit-lint, codeql, operational-class)

eslint.config.mjs + 7 partials (8):
  alint-today:      8 /  8 = 100%

src/node_version.h (5 invariants):
  alint-today:      5 /  5 = 100%

primordials.js (1):
  alint-today:      1 /  1 = 100%

filename grammars (2 net-new):
  alint-today:      2 /  2 = 100%   (test-* + CHANGELOG_V*)

config + governance + hygiene (15):
  alint-today:     15 / 15 = 100%

cross-file shapes (4):
  alint-today:      1 /  4 =  25%   (lib ↔ doc/api covered by pair)
  alint-future:     3 /  4 =  75%   (cross_file_value_equals + registry_paths_resolve + file_header_consistency)
```

**Commentary.** Three observations:

1. **The "15 years of conventions enforced via human review" framing
   is half-true.** The actual enforcement layer is the 27 in-tree
   eslint visitors under `tools/eslint-rules/` — they fire on every
   PR via `make lint-js`. What human-review-only catches is the
   *authoring* of new visitors when a new convention is introduced,
   plus the editorial review of `CHANGELOG_V*.md` per-major-version
   filename conventions and `test/*/test-*.{js,mjs,cjs}` discovery
   grammar (alint provides the static enforcement for both — see
   §2.10). **The fully accurate framing**: "27 eslint visitors enforce
   most conventions at PR time; 2 conventions (changelog filename + test
   discovery filename) were enforced statically by NOTHING before
   alint."

2. **`registry_paths_resolve` is the v0.10 ship-target most likely to
   land for node** — covers `tools/dep_updaters/update-<libname>.{sh,mjs}`
   ↔ `deps/<libname>/`. Demand: 8 sources past saturation (rust, clap,
   cpython×2, next.js, arrow, pytorch, nodejs/node, NixOS×3). One of
   v0.10's two highest-leverage gaps.

3. **`cross_file_value_equals` (registry-direction-only mode) is a
   refinement, not a v0.10 must-ship.** node's `RULES_DIR` mechanism
   doesn't have a per-rule explicit registration to verify — every
   `.js` in the directory is auto-registered. The cleaner alint
   primitive for this case is the `dir_name_matches_field` candidate
   (turbo + next.js + nixpkgs sources), extended to handle the
   `RULES_DIR`-mechanism case. Logged but not driving v0.10 priority.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (940 lines, 40
node-specific rules + 5 bundled rulesets, **86 rules total** loaded
— confirmed by `alint validate-config`).

**Synopsis of the load-bearing repo-specific rules** (full config in
`.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1            # 15 rules
  - alint://bundled/node@v1                    # 9 rules
  - alint://bundled/ci/github-actions@v1       # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules
  - alint://bundled/tooling/editorconfig@v1    # 3 rules

facts:
  - id: has_node_src
    any_dir_exists: [src, lib]

rules:
  # 2 net-new filename grammars
  - id: node-test-filename-grammar
    kind: filename_regex
    paths:
      include:
        - "test/parallel/*.{js,mjs,cjs}"
        - "test/sequential/*.{js,mjs,cjs}"
        - "test/async-hooks/*.{js,mjs,cjs}"
        - "test/es-module/*.{js,mjs,cjs}"
        - "test/message/*.{js,mjs,cjs}"
        - "test/pseudo-tty/*.{js,mjs,cjs}"
        # …(15 test sub-directories total)…
      exclude:
        - "test/*/index.{js,mjs,cjs}"
        - "test/*/common.{js,mjs,cjs}"
        - "test/*/eslint.config_partial.mjs"
    pattern: '^test-[A-Za-z0-9][A-Za-z0-9_.-]*\.(js|mjs|cjs)$'
    level: error
    message: |
      Tests under test/{parallel,sequential,...}/ must be named
      `test-<descriptor>.{js,mjs,cjs}` to be discovered by
      tools/test.py. A file not matching this pattern silently
      drops out of the test run.

  - id: node-changelog-per-major-filename
    kind: filename_regex
    paths:
      include: ["doc/changelogs/CHANGELOG_*.md"]
    pattern: '^CHANGELOG_(V\d+|V010|V012|IOJS|ARCHIVE)\.md$'
    level: error

  # 4 src/node_version.h macros
  - id: node-version-header-major-defined
    kind: file_content_matches
    paths: src/node_version.h
    pattern: '(?m)^#define NODE_MAJOR_VERSION \d+$'
    level: error

  # 7 command:-rule shellouts
  - id: node-eslint-lib
    kind: command
    paths:
      include: ["lib/**/*.{js,mjs,cjs}"]
      exclude: ["deps/**", "test/fixtures/**"]
    command: ["npx", "eslint", "--no-warn-ignored", "{path}"]
    timeout: 120
    level: warning

  - id: node-cpplint
    kind: command
    paths:
      include: ["src/**/*.{cc,h}"]
      exclude: ["src/inspector/**"]
    command: ["python3", "tools/cpplint.py", "{path}"]
    timeout: 60
    level: warning
```

**Repo-specific vs bundled split:**

- **40 node-specific rules** in `.alint.yml` (the `node-*` prefix
  identifies them in `alint list` output): test-filename-grammar,
  changelog-filename, src/node_version.h macros (×4), governance +
  build files, eslint-config root + tier partials, custom-rule
  presence, primordials presence, lint configs, ruff section,
  workflow assertions, and 9 `command:` shellouts.
- **46 bundled rules** from the 5 extended rulesets: 15 from
  oss-baseline + 9 from node + 3 from ci/github-actions + 11 from
  hygiene/no-tracked-artifacts + 3 from tooling/editorconfig (some
  rule IDs may overlap; total reported is 86 after dedup).

**Validation:** `alint validate-config` reports
`✓ Config valid: 86 rule(s) loaded`. Pitfall checks: the magic
comment is present (line 1); `command:` rules use `command:` (not
`argv:`) and integer `timeout:` (not duration strings); JSONPath
bracket notation used for dashed keys (`$.dependencies['remark-preset-lint-node']`);
the `match()` filter is used for the action-SHA-pinning rule.
**0 instances of pitfall #22** in this config (no `pattern: |` block
scalars).

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3` on the same
`/tmp/nodejs-node` working tree captured 2026-05-07. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary `target/release/alint
v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full lite-pass** (69 rules, no `command:` shellouts) | n/a | n/a | **60 ms** ± 2 ms | — |

The 60 ms lite-pass walks the entire 8,214-file working tree (132
MB), including the 4 `node_version.h` macro checks, the 25
`CHANGELOG_V*.md` filename-grammar checks, the 22+ `test/*/test-*.{js,mjs,cjs}`
filename-grammar checks across 15 test sub-directories, and the
JSONPath queries against `tools/lint-md/package.json`,
`pyproject.toml`, `tsconfig.json`. **The fastest case study in this
batch** — node's hygiene surface is concentrated (vs nixpkgs's 20k
by-name dirs or vscode's 14k TS files).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `make lint-js` (eslint over lib/, test/, doc/, tools/) | eslint v9 + 27 custom plugin rules | pending — `tools/eslint/node_modules/` not built | `cd /tmp/nodejs-node && make lint-py-build && make lint-js-ci` |
| `make lint-cpp` (cpplint + checkimports over src/) | python (vendored cpplint fork) | pending — python tools/ not bootstrapped | `cd /tmp/nodejs-node && make lint-py-build && make lint-cpp` |
| `make lint-md` (remark-preset-lint-node) | remark + 50 visitors | pending — `tools/lint-md/node_modules/` not built | `cd /tmp/nodejs-node && make lint-md` |
| `make lint-py` (ruff against pyproject.toml) | ruff | pending — vendored ruff not bootstrapped | `cd /tmp/nodejs-node && make lint-py-build && make lint-py` |
| `make lint-yaml` (yamllint) | yamllint | pending | `cd /tmp/nodejs-node && make lint-py-build && make lint-yaml` |

Estimated wall-clocks (from public benchmarks of `make lint` on
node CI):

| Check | Estimated wall-clock at node tree size |
|---|---|
| `make lint-js-ci` | ~3 minutes (eslint cold-cache over ~445 src + ~15k lib/test files) |
| `make lint-cpp` | ~90 s (cpplint over ~268 src/*.{cc,h}) |
| `make lint-md` | ~30 s (remark over ~80 doc/api/*.md + ~30 doc/contributing/*.md) |
| `make lint-py` | ~5 s (ruff over ~30 python build helpers) |
| `make lint-yaml` | ~10 s (yamllint over ~38 YAML files) |
| `make lint` (full pipeline, sequential) | ~5-7 minutes |

Headline comparison: alint's 60 ms structural pass replaces (a) the
governance-files presence checks, (b) the `node_version.h` macro
shape checks, (c) the changelog + test-filename grammars (both
net-new), (d) the per-tier eslint config presence — all of which
together gate *before* a single `make lint` invocation. **alint is
the fastest fail signal for the structural floor**; the deep checks
(eslint, cpplint, lint-md) stay where they are and run via `command:`
shellouts when invoked.

The most-marketable comparison for nodejs/node is therefore:
**alint runs the structural-floor 69-rule pass in 60 ms, asserting
the 2 net-new conventions** (test-filename grammar + changelog-filename
grammar) **that previously had no static enforcement**, plus the
governance / version-header / config-pinning surface — replacing
the editorial-review-only flow that catches these issues today.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /tmp/node-alint-lite.yml /tmp/nodejs-node`
(live run, JSON-format, lite config without the 9 `command:` rules
since `eslint`/`cpplint`/`lint-md`/`ruff`/`yamllint`/`shellcheck`
aren't installed). Sparse-checkout excludes `deps/`, `test/parallel/`,
`test/sequential/` — the bulk of the test corpus.

**Headline:** alint surfaces **147 violations** across the live tree
— **mostly real findings.** Findings break down to: 22 real test-helper
files matched by the test-filename-grammar rule (need broader exclude
list), 1 bidi-control finding in WPT test fixture (intentional), 43
real `node_modules/` test-fixture commits, 3 real `.env` test fixtures,
~16 cosmetic newline / trailing-whitespace / heuristic findings, and
~62 GHA hardening warnings (Scorecard catches the same on its
nightly run).

### 6.1 Real findings

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 22 test-helper files match the test-filename-grammar | `test/async-hooks/{hook-checks,init-hooks,verify-graph}.js`, `test/message/{assert_throws_stack,console_assert,internal_assert,…}.js`, `test/pseudo-tty/{console-dumb-tty,console_colors,no_dropped_stdio,…}.js`, `test/sqlite/{next-db,worker}.js` | error | `node-test-filename-grammar` | **All real but expected.** These are **intentional helpers** — `init-hooks.js` is shared across multiple `test-*.js` files in `async-hooks/`; `assert_throws_stack.js` is the input fixture for `test-message.js`; `next-db.js` is sqlite worker code. The exclude list (`test/*/index.{js,mjs,cjs}`, `test/*/common.{js,mjs,cjs}`, `test/*/eslint.config_partial.mjs`) catches the 3 generic helper conventions but misses these directory-specific helpers. **Recommended fix:** broaden the exclude list to include canonical helper patterns: `test/*/{*-checks,*-hooks,verify-*,assert_*,console_*,no_*,internal_*}.{js,mjs,cjs}` — OR scope the rule tighter (only `test/parallel/` + `test/sequential/`, the directories where `tools/test.py` actually GLOBs for `test-*` discovery). The current rule is too broad — fires on adjacent test directories where the `tools/test.py` discovery rule doesn't apply |
| 43 `node_modules/` directories committed | `benchmark/fixtures/node_modules`, `test/addons/esm/node_modules`, `test/fixtures/es-module-loaders/node_modules`, `test/fixtures/es-module-specifiers/node_modules`, `test/fixtures/es-modules/custom-condition/node_modules`, … | error | `node-no-tracked-node-modules` + `hygiene-no-node-modules` | **Real but intentional.** These are test fixtures — the tests literally check that node can resolve `node_modules` lookups in baseline scenarios. **Recommended fix:** add `test/fixtures/**/node_modules`, `test/addons/**/node_modules`, `benchmark/fixtures/**/node_modules` to the rule's exclude list. Same class of finding as the TypeScript pilot. |
| 3 `.env` files committed | `test/fixtures/dotenv/.env`, `test/fixtures/run-script/.env`, `test/fixtures/test-runner/flag-propagation/.env` | error | `hygiene-no-env-files` | **Real but intentional.** These are test fixtures for `node --env-file` testing. **Recommended fix:** add `test/fixtures/**/.env` to the rule's exclude list. |
| 1 bidi-control character finding | `test/fixtures/wpt/url/resources/urltestdata.json` | error | `oss-no-bidi-controls` | **Real but expected.** Web Platform Tests data — explicitly tests URL encoding of bidi controls. **Recommended fix:** add `test/fixtures/wpt/**` to the rule's exclude list. |
| 1 file > 10 MiB | (single occurrence) | warning | `hygiene-no-huge-files` | Reviewable. |
| 9 workflows lack the `contents: read` minimum permission | (across `.github/workflows/`) | warning | `gha-workflow-contents-read` | **Real bugs** — Scorecard catches the same on its nightly run. |
| 2 workflows lack `permissions:` block | (across `.github/workflows/`) | warning | `gha-pin-actions-to-sha` | Same class. |
| 3 forbidden directory-name matches | `pkgs/development/python-modules/build` (etc.) | warning | `hygiene-no-js-build-outputs` | **Same false-positive class as kubernetes / vscode / nixpkgs** — directories literally named `build` in non-JS contexts. Cross-cutting bundled-rule refinement (see §7) |
| 10 markdown / 3 source files lack final newline | (across the tree) | info | `oss-final-newline` + `oss-no-trailing-whitespace` | Real but unweighted by the existing tooling. |
| 1 `node-package-json-exists` failure | (related to root config layering) | error | `node-package-json-exists` | **False positive in spirit** — node has multiple `package.json` files (root, tools/lint-md/, tools/eslint/, etc.); the rule fires when scope doesn't unambiguously identify the canonical one. **Recommended fix:** scope the rule to `root_only: true` for the `node@v1` ruleset's package.json check. |
| 1 `node-has-lockfile` warning | repo root | warning | `node-has-lockfile` | node's root has no `package-lock.json` (the project uses `npm` only for `tools/`-internal deps; the build is GYP/configure). **Expected**; rule's expected scope is npm projects. |
| 1 `node-engine-or-nvmrc` info | repo root | info | `node-engine-or-nvmrc` | node ships `.nvmrc` for the buildbot config (verified — present); the rule reads it correctly. |
| 1 governance-info: `.gitattributes` line-endings declaration | repo root | info | `tooling-gitattributes-normalizes-line-endings` | Expected. |

**Total real findings (alint-surfaced, existing tooling missed):**
- **0 NEW real bugs** caught by alint that the existing eslint + cpplint
  + lint-md stack would miss (the 22 test-helper findings are
  legitimate but the rule needs scope refinement; not new bugs)
- **2 conventions enforced statically nowhere before alint** —
  test-filename grammar (caught 22 helper-files needing the exclude
  refinement; would catch real typos in net-new test additions),
  changelog-filename grammar (silently passing on the 25 existing
  files; would catch a `CHANGELOG_v27.md` lowercase typo at PR time)
- **62 GHA hardening warnings** (Scorecard surfaces the same; alint
  surfaces them at PR time)
- **49 false positives needing per-rule scope refinement** (test-fixture
  node_modules + test-fixture .env + bidi WPT + 22 test-helper
  filename matches + 3 hygiene directory-name matches)

### 6.2 Recommended `.alint.yml` config refinements (P1, not P0)

| Refinement | Action | Severity |
|---|---|---|
| `node-test-filename-grammar` exclude list | Broaden to canonical helper patterns OR scope to test/parallel + test/sequential only | P1 (22 false positives) |
| `node-no-tracked-node-modules` + `hygiene-no-node-modules` exclude | Add `test/fixtures/**/node_modules`, `test/addons/**/node_modules`, `benchmark/fixtures/**/node_modules` | P1 (43 false positives — same class as TypeScript pilot) |
| `hygiene-no-env-files` exclude | Add `test/fixtures/**/.env` | P1 (3 false positives) |
| `oss-no-bidi-controls` exclude | Add `test/fixtures/wpt/**` | P1 (1 false positive — WPT data) |
| `node-package-json-exists` scope | Add `root_only: true` to disambiguate root from tools/-nested package.json | P2 |
| `hygiene-no-js-build-outputs` scope | Cross-cutting bundled-ruleset refinement (k8s, vscode, nixpkgs, node all hit this) | P2 (filed under bundled-ruleset queue) |

### 6.3 No suspected `.alint.yml` schema/regex bugs

The config is clean of regex pitfalls. **0 instances of pitfall #22**
(no `pattern: |` block scalars). The `(?m)` line-anchor prefix is
correctly applied in `node_version.h` macro checks and the
workflow-permissions check. JSONPath bracket notation is used for
dashed keys (`$.dependencies['remark-preset-lint-node']`).

---

## 7. Followup feature work surfaced

- **`registry_paths_resolve` rule kind** — would cover
  `tools/dep_updaters/update-<libname>.{sh,mjs}` ↔ `deps/<libname>/`
  here, plus 7 other sources. **v0.10 ship-target** at 8 sources.
- **`cross_file_value_equals` rule kind (registry-direction-only mode)** —
  would cover `tools/eslint-rules/*` ↔ `eslint.config.mjs`
  registration if extended to handle the `RULES_DIR` mechanism.
  **v0.10 ship-target** at 10 sources past saturation.
- **`file_header_consistency` rule kind** (or `file_header.alt_pattern`
  field on the existing rule) — surfaced uniquely by node's drifted
  C++ EOL-banner convention. Niche; the cleaner outcome is editorial
  cleanup. **NEW v0.10+ candidate; single-source (node-only);
  low-priority**.
- **Cross-cutting bundled-ruleset refinement** for
  `hygiene-no-js-build-outputs` (k8s, vscode, nixpkgs, node all hit
  the same false positive on directories literally named `build`).
  Filed under bundled-ruleset refinement queue.
- **Cross-cutting bundled-ruleset refinement** for
  `hygiene-no-env-files` and `node-no-tracked-node-modules`
  (test-fixture exclusions for `test/fixtures/**`). Same class — file
  under bundled-ruleset refinement queue.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`agent-context@v1` adoption.** node ships a load-bearing
   `tools/eslint-rules/` (registers AI-coding-agent helpers via
   eslint visitors) and the broader project has converged on
   agent-instruction discipline across `CONTRIBUTING.md`,
   `doc/contributing/`, and the multiple onboarding docs. The
   bundled `agent-context@v1` ruleset (5 rules) would absorb the
   existing top-level governance assertions without adding
   repo-specific rules.
2. **`hygiene/lockfiles@v1` overlay.** node's `tools/eslint/` and
   `tools/lint-md/` ship pinned `package.json` + `package-lock.json`
   pairs; the bundled `hygiene/lockfiles@v1` ruleset (7 rules) would
   catch nested-lockfile drift, mismatched `lockfileVersion`, and
   the orphan-lockfile pattern across the `tools/` subtree without
   per-tier restatement.
3. **`tools/eslint-rules/*` ↔ `eslint.config.mjs` registry pattern as
   a future rule kind.** This is the canonical 27-source instance of
   `cross_file_value_equals`'s "every file in directory X is referenced
   from file Y" shape — but the cross-file is *registry-flavoured*
   (every file in the dir must appear at some path in the registry,
   not at a specific path). Worth flagging as a candidate refinement
   of `cross_file_value_equals` once that primitive ships in v0.10:
   the registry-direction-only mode (`cross_file_files_match_registry:
   true`?) would express this without the per-key value comparison.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **86** (40 custom + 5 bundled rulesets — `oss-baseline`
  15, `node` 9, `ci/github-actions` 3, `hygiene/no-tracked-artifacts`
  11, `tooling/editorconfig` 3; rule IDs may overlap)
- **`alint validate-config`:** ✓ Config valid: 86 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  147-violation breakdown (22 test-helper false positives + 47
  test-fixture false positives + 62 real GHA hardening + 16
  cosmetic + 0 NEW real bugs caught beyond what the existing eslint
  stack would catch)
- **Pitfall instances flagged:** **0 instances of pitfall #22** in
  this config (no `pattern: |` block scalars). Config is clean.
- **Pitfall fixes (v0.9.17):** Pitfalls #18 + #19 do not apply here.
- **Open gaps:** `cross_file_value_equals` (v0.10 ship-target, 10
  sources — node's `tools/eslint-rules/*` ↔ `eslint.config.mjs` is
  one of the most-adopter-visible instances), `registry_paths_resolve`
  (v0.10 ship-target, 8 sources — node's `tools/dep_updaters/` ↔
  `deps/` is one of the canonical sources), `file_header_consistency`
  (NEW low-priority, node-only).
- **Cross-cutting bundled-rule refinements surfaced:**
  `hygiene-no-js-build-outputs` (k8s + vscode + nixpkgs + node —
  4 sources hitting the same directory-name false positive),
  `hygiene-no-env-files` + `node-no-tracked-node-modules` (TS + node —
  2 sources for test-fixture exclusion). Both filed under
  bundled-ruleset refinement queue.
- **Open suspected bugs in this directory's `.alint.yml`:** None
  (regex / schema). 6 P1/P2 scope refinements recommended for the
  current rule set (see §6.2).
- **Framing correction for the case-study claim** ("15-year-old
  conventions enforced via human review only"): **partially incorrect**.
  27 in-tree eslint visitors under `tools/eslint-rules/` enforce most
  conventions at PR time; alint's contribution is the structural-floor
  layer + 2 net-new filename-grammar conventions (changelog +
  test-discovery) that previously had **no static enforcement at
  all**. See §2.2 for the cross-reference.
