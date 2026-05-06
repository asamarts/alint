# Case study: `nodejs/node`

Inventory of the structural-validation tooling in `nodejs/node` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-clone of `nodejs/node@HEAD`
excluding `deps/` (V8, libuv, ICU, nghttp2, etc.), `test/parallel/` and
`test/sequential/` (the bulk of the test corpus, ~30k files; not
material to the structural inventory).

---

## Summary

`nodejs/node` is the canonical mature C++/JS hybrid mega-repo —
**~15 years of accumulated convention discipline**, with structural
validation scattered across the broadest surface of any P2a-Wave 3 case
study. Concrete count: **44 distinct structural-validation surfaces**
inventoried, including a 1700-line `Makefile` (~12 `lint-*` /
`format-*` / `tidy-*` / `check-*` targets), the Linters workflow
(`linters.yml`) dispatching the lint matrix across 25 GitHub Actions
files, **27 in-tree custom eslint rules** under `tools/eslint-rules/`,
**7 per-tier `eslint.config_partial.mjs` files** composed by the root
`eslint.config.mjs`, a vendored `tools/cpplint.py` (a fork of upstream
Google cpplint, configured via `.cpplint`), `tools/checkimports.py`
(parses `using <ns>::<name>;` for freshness), `tools/lint-md/lint-md.mjs`
(remark-preset-lint-node pipeline with ~50 markdown AST rules),
`tools/lint-{pr-url,readme-lists,sh}.mjs` (PR-time and tree-state lint
helpers), `tools/find-inactive-{collaborators,tsc}.mjs` (cron-driven
inactivity detectors), `tools/test.py` (test discovery via filename
glob), `pyproject.toml` (ruff config + ~25 lint families), `.editorconfig`,
`.gitattributes`, `.clang-format`, **27 per-major-version
`CHANGELOG_V*.md` files** under `doc/changelogs/`, and an unwritten
`test/parallel/test-*.{js,mjs,cjs}` discovery convention enforced
nowhere statically.

Of the 44 distinct structural-validation surfaces inventoried:

- **~43 % map directly to existing alint rules** (~19 surfaces:
  per-tier eslint partials, `.editorconfig` line-ending policy,
  `.gitattributes` Windows-batch CRLF, top-level governance files,
  `src/node_version.h` macros, `lib/internal/per_context/primordials.js`
  presence, the per-major-version changelog grammar, the test-discovery
  filename grammar, action-SHA pinning, workflow permissions blocks,
  `tsconfig.json` strictness, etc.)
- **~9 % need new alint primitives** (~4 surfaces: `tools/eslint-rules/*`
  ↔ `eslint.config.mjs` cross-file registration, `tools/dep_updaters/update-*`
  ↔ `deps/<libname>/` registry resolution, the C++ EOL banner
  consistency check, the lib/ ↔ doc/api/ cross-reference)
- **~48 % are out of alint's scope** (~21 surfaces: 27 `tools/eslint-rules/*`
  TSESTree visitors, `tools/cpplint.py` C++ AST analysis,
  `tools/checkimports.py` C++ `using`-declaration parsing,
  `tools/lint-md/lint-md.mjs` markdown AST, `tools/find-inactive-*`
  git-history walks, `tools/lint-pr-url.mjs` PR-diff queries, `make
  format-cpp` `git merge-base` diff scoping, `tools/test.py` runtime
  test execution, etc.)

The 43 % that *do* fit translate to the **40-rule alint config** in
[`/.alint.yml`](.alint.yml), bundled-rulesets-included. The two single
most alint-shaped surfaces are:

1. **`test/parallel/test-*.{js,mjs,cjs}` discovery filename grammar**
   — `tools/test.py` GLOBS for `test-*.{js,mjs,cjs}` to discover tests;
   a misnamed file (`tst-foo.js`, `text-foo.js`, `Test-Foo.js`) silently
   drops out of the test run. **Enforced nowhere statically today** —
   the only feedback is a missing test failing to fire when an
   intentionally-broken regression test goes silent. alint encodes the
   grammar as a 6-line `filename_regex` rule covering ~15 test
   sub-directories.

2. **`doc/changelogs/CHANGELOG_V<MAJOR>.md` per-major-version
   convention** — 27 changelog files at clone time, each named
   `CHANGELOG_V<NN>.md` (or one of four legacy variants `CHANGELOG_V010.md`
   / `CHANGELOG_V012.md` / `CHANGELOG_IOJS.md` / `CHANGELOG_ARCHIVE.md`).
   Editorial review of the release-prep PR catches typos today; alint
   encodes the grammar as a `filename_regex` rule so a hand-edited
   `CHANGELOG_v27.md` (lowercase) or `CHANGELOG_V27` (no extension)
   can't merge accidentally.

---

## Existing tooling inventory

### `Makefile` lint / format / check targets (~12 distinct)

| Make target | What it does | alint disposition |
|---|---|---|
| `lint` | Aggregator: `lint-js`, `lint-cpp`, `lint-md`, `lint-addon-docs` | MAPS — alint *is* this aggregator |
| `lint-ci` | CI-tuned `lint-js-ci`, `lint-cpp`, `lint-py`, `lint-md`, `lint-addon-docs`, `lint-yaml-build`, `lint-yaml` | MAPS — same aggregator |
| `lint-js` / `lint-js-ci` / `jslint` | `tools/eslint/node_modules/eslint/bin/eslint.js` over `lib/`, `test/`, `doc/`, `tools/` | MAPS — 4 `command` rules per tier |
| `lint-cpp` / `cpplint` | `tools/cpplint.py` + `tools/checkimports.py` over `src/**/*.{cc,h}` | MAPS (shellout) — alint shells out to both |
| `format-cpp` | `clang-format` over the diff vs `git merge-base HEAD origin/$BASE` | OUT OF SCOPE (git-diff aware) |
| `lint-md` | `tools/lint-md/lint-md.mjs` (remark-preset-lint-node) over `doc/**/*.md` + `*.md` | MAPS — `command` shellout |
| `lint-py` / `lint-py-fix` | `tools/pip/site-packages/bin/ruff check .` against `pyproject.toml` config | MAPS — `command` shellout |
| `lint-yaml` | `python -m yamllint .` (yamllint vendored under `tools/pip/`) | MAPS — `command` shellout |
| `lint-addon-docs` | `tools/.doclintstamp` — runs the addon-docs linter | OUT OF SCOPE (rich addon-docs DSL) |
| `lint-clean` / `lint-py-build` / `lint-yaml-build` | Bootstrap targets for the lint pipeline | N/A (not validation) |
| `check`, `check-xz` | Test runner aggregator | OUT OF SCOPE (test runner) |

### `tools/eslint-rules/` (27 in-tree custom rules)

All 27 are TSESTree visitors — out of alint's "no AST" scope. Listed
here so the inventory is complete:

| Rule | What it does |
|---|---|
| `prefer-primordials` | Bans direct use of JS built-ins (`Array.from`, `Object.keys`, etc.); requires the wrapper from `lib/internal/per_context/primordials.js` |
| `no-array-destructuring` | Bans `const [a, b] = arr;` (prototype-pollution defense) |
| `alphabetize-errors` / `alphabetize-primordials` | Sortedness inside specific files |
| `must-call-assert` | `Debug.assert` argument-shape enforcement |
| `prefer-assert-iferror` / `prefer-assert-methods` | Test-assertion style |
| `crypto-check` / `inspector-check` | Conditional-compilation guards on optional features |
| `documented-deprecation-codes` / `documented-errors` | Cross-references error codes against `doc/api/errors.md` |
| `eslint-check` | Asserts `eslint-disable` comments are well-formed |
| `lowercase-name-for-primitive` / `no-keywords` / `set-proto-to-null-in-object` | Identifier-naming conventions |
| `no-duplicate-requires` | Bans duplicate `require()` calls in a file |
| `non-ascii-character` | Bans non-ASCII chars in source |
| `no-unescaped-regexp-dot` | Bans `/./` in regex literals (use `/\./`) |
| `prefer-common-mustnotcall` / `prefer-common-mustsucceed` | Test-helper style |
| `prefer-optional-chaining` / `prefer-proto` / `prefer-util-format-errors` | Modern-syntax enforcement |
| `require-common-first` / `required-modules` | Test-file structure |
| `async-iife-no-unused-result` / `avoid-prototype-pollution` / `rules-utils.js` | Rule helpers / safety |

These are perfect examples of "AST analysis is not alint's niche" —
they belong in `tools/eslint-rules/` and stay there.

### `tools/lint-*.mjs` shell scripts

| Script | What it does | alint disposition |
|---|---|---|
| `lint-md.mjs` | Remark-preset-lint-node pipeline | MAPS — `command` shellout (per-file) |
| `lint-pr-url.mjs` | Reads `git diff` of `doc/api/*.md` for `pr-url:` strings | OUT OF SCOPE (PR-diff aware) |
| `lint-readme-lists.mjs` | Validates README's collaborator list against the actual GitHub teams | OUT OF SCOPE (HTTP fetch + git API) |
| `lint-sh.mjs` | Runs `shellcheck` over project shell scripts | MAPS — `command` shellout |

### `tools/find-inactive-{collaborators,tsc}.mjs`

Walks `git log` to find collaborators / TSC members with no recent
commits. alint sees one tree at a time; no git-history awareness.
**Out of scope; STAYS as the cron-driven workflow** at
`.github/workflows/find-inactive-*.yml`.

### `tools/test.py`

Test discovery. GLOBS for `test/{parallel,sequential,async-hooks,...}/test-*.{js,mjs,cjs}`.
Loads each file as a regression-test entry. **The discovery
itself isn't a structural-validation surface, but it implies one —
the filename grammar is enforced nowhere statically; a typo silently
drops the test from the run.** alint encodes the grammar (see headline
finding 1 above).

### `.gitattributes` (286 bytes)

Terse but load-bearing:

| Section | alint disposition |
|---|---|
| `test/fixtures/* -text` (no line-ending normalization) | MAPS via `paths.exclude` on the line-endings rules |
| `vcbuild.bat text eol=crlf`, `tools/msvs/find_python.cmd text eol=crlf` | MAPS — `node-windows-bat-crlf` rule |
| `deps/npm/bin/npm text eol=lf`, `deps/npm/bin/npx text eol=lf`, `deps/corepack/shims/corepack text eol=lf` | OUT (we exclude all of `deps/`) |
| `doc/**/*.md text eol=lf` | MAPS — `node-doc-md-lf` rule |
| `deps/crates/vendor/**/* -text` | OUT (we exclude all of `deps/`) |

### `.editorconfig`

Maps trivially to the bundled `tooling/editorconfig@v1` ruleset — adds
explicit assertions for the `[*]` block (LF, final newline, trim
trailing) over the source-code subset of the tree (excluding `deps/`,
`test/fixtures/`, `tools/eslint/node_modules/`, etc.).

### `.cpplint`

| Line | Effect | alint disposition |
|---|---|---|
| `set noparent` | No inherited filters | OUT OF SCOPE (cpplint config) |
| `filter=-build/c++17,-build/include_alpha,...` | 9 categories disabled | OUT OF SCOPE (cpplint config) |
| `linelength=80` | 80-char line limit | Could map via `line_max_width` but adds noise; left to cpplint |

### `pyproject.toml` ruff config

`[tool.ruff]` declares `target-version = "py310"` plus ~25 lint families
(ASYNC, C90, E, F, ICN, INT, PERF, PLC, PLE, PLR09, PYI, RSE, RUF, T10,
TCH, TID, W, YTT). `make lint-py` reads this and runs ruff. alint
shells out to ruff; the deep ruff config stays in `pyproject.toml`.

### `.github/workflows/` (25 workflow files)

| Workflow | What it does | alint disposition |
|---|---|---|
| `linters.yml` | Dispatches `lint-cpp`, `lint-md`, `lint-js`, `lint-py`, `lint-yaml`, `lint-shell`, `format-cpp`, `format-md`, `lint-pr-url`, `lint-readme-list`, `lint-changelog` jobs | Each job is its own surface; see per-target rows above |
| `commit-lint.yml` | First commit message adheres to the contributing guidelines (subsystem prefix, signed-off-by, etc.) | OUT OF SCOPE (git commit-message regex; alint has `git_commit_message` but the node convention is rich enough that the existing tool wins) |
| `commit-queue.yml` | Lands PRs via the commit queue | OUT OF SCOPE (operational) |
| `codeql.yml` | CodeQL static security analysis | OUT OF SCOPE (security scanner) |
| `scorecard.yml` | OpenSSF Scorecard run (action-SHA pinning, permission blocks) | Partial alint coverage: see `node-workflow-actions-pinned-by-sha` + `node-workflow-has-permissions` |
| `auto-start-ci.yml`, `comment-labeled.yml`, `label-flaky-test-issue.yml`, `label-pr.yml`, `notify-on-*.yml`, `stale.yml`, `close-stalled.yml`, `daily*.yml`, `find-inactive-*.yml`, `timezone-update.yml`, `update-*.yml`, `tools.yml` | Operational / labelling / notification bots | OUT OF SCOPE |
| `build-tarball.yml`, `coverage-*.yml`, `daily.yml`, `doc.yml`, `license-builder.yml`, `lint-release-proposal.yml`, `major-release.yml`, `post-release.yml`, `create-release-proposal.yml`, `test-*.yml` | Build / test / release | OUT OF SCOPE |

**~5 of 25 workflows (~20 %) carry a structural assertion alint can
restate.** The other 20 are CI orchestration / release / maintenance.

### `eslint.config.mjs` + 7 per-tier partials

| File | Tier | alint disposition |
|---|---|---|
| `eslint.config.mjs` (root) | Composes per-tier partials, registers all 27 custom rules | MAPS — `node-eslint-config-root-present` |
| `lib/eslint.config_partial.mjs` | Production lib/ files (`prefer-primordials`, etc.) | MAPS — file_exists |
| `test/eslint.config_partial.mjs` | Test files (allows `console.log`, etc.) | MAPS — file_exists |
| `doc/eslint.config_partial.mjs` | Markdown doc snippets | MAPS — file_exists |
| `benchmark/eslint.config_partial.mjs` | Benchmark scripts | MAPS — file_exists |
| `tools/eslint/eslint.config_partial.mjs` + `eslint.config_utils.mjs` | tools/ helpers + the rule-loader plumbing | MAPS — file_exists |

### `src/node_version.h`

Defines `NODE_MAJOR_VERSION`, `NODE_MINOR_VERSION`, `NODE_PATCH_VERSION`,
`NODE_VERSION_LTS_CODENAME`, `NODE_VERSION_IS_LTS`, `NODE_VERSION_IS_RELEASE`.
**The release pipeline reads this file to compute the release tag.** alint
asserts the canonical macros are present and integer-typed; a typo (e.g.
`NODE_MAJOR_VERSION 27.0` instead of `27`) breaks `node --version`
silently.

### `tools/lint-md/package.json`

Pins `remark-parse`, `remark-preset-lint-node`, `remark-stringify`,
`to-vfile`, `unified`, `vfile-reporter`. **A missing pin means the
markdown-lint pipeline silently drops most of its rules.** alint
asserts `remark-preset-lint-node` and `remark-parse` stay version-pinned.

### `lib/internal/per_context/primordials.js`

The prototype-pollution defense layer. The `prefer-primordials` custom
eslint rule and dozens of `lib/` modules import from it. alint asserts
the file exists; the eslint rule enforces the use.

### `doc/changelogs/CHANGELOG_V<MAJOR>.md` (27 files)

Per-major-version changelog convention. **Enforced today only by
editorial review of the release-prep PR.** alint encodes the grammar as
a `filename_regex` rule (see headline finding 2 above).

### `doc/contributing/` (~30 markdown files)

The deeper contributor docs (`cpp-style-guide.md`, `collaborator-guide.md`,
`pull-requests.md`, etc.). alint asserts the most-load-bearing entries
exist; a deletion would catastrophically break the contributor onboarding
flow documented in `CONTRIBUTING.md`.

---

## What needs new alint primitives

| Gap | Existing node tooling | What alint needs |
|---|---|---|
| `tools/eslint-rules/*` ↔ `eslint.config.mjs` registration | `eslint.config.mjs` imports each custom rule by path | `cross_file_value_equals` rule kind: "every `.js` file under directory X is referenced from a string literal in file Y". **Already on the v0.10+ candidate list** as the single highest-demand item (6 sources). node confirms a 7th. |
| `tools/dep_updaters/update-<libname>.{sh,mjs}` ↔ `deps/<libname>/` | Ad-hoc per-script convention | `registry_paths_resolve` rule kind: "every path/key in a registry directory resolves to an on-disk artefact in a partner directory". **Already on the v0.10+ candidate list** (4 sources). node confirms a 5th. |
| C++ EOL banner consistency in `src/**/*.{cc,h}` | Historically every file started with the Joyent BSD/MIT banner; ~21 % of files at HEAD still do, ~79 % don't (verified: 56 of 268 src/*.{cc,h} carry the banner). The convention drifted silently — newer files just include a header guard. | **NEW candidate**: `file_header_consistency` — assert every file in scope X *either* matches the canonical header *or* matches a "newer convention" header. Niche; the cleaner outcome here is a one-time editorial sweep that picks one convention and `file_header` enforces it. Logged as a v0.10+ candidate but rated low priority. |
| `lib/<module>.js` ↔ `doc/api/<module>.md` cross-reference | The `node test/doctool/*.js` test suite cross-references | `pair` rule kind already covers this — but the actual node convention is more nuanced (`lib/internal/<module>.js` doesn't need a doc page; `lib/<module>.js` does). Documented in the config as a `pair` rule scoped to top-level `lib/*.js` — confirms `pair` works for the simple case; the nuance can be expressed via `paths.exclude`. **Not a gap; included for completeness.** |
| `make format-cpp` clang-format diff scoping | `git merge-base HEAD origin/$BASE` then `clang-format --diff` | OUT OF SCOPE (git-diff aware). alint's `--changed` mode informs WHICH files to check, not what the check is. STAYS in CI. |
| `tools/lint-pr-url.mjs` PR-time check | Reads `git diff` of `doc/api/*.md` for `pr-url:` strings, asserts they match the current PR's URL | OUT OF SCOPE (PR-diff aware). STAYS as the CI step. |
| `tools/find-inactive-{collaborators,tsc}.mjs` | Walks `git log` for collaborator activity | OUT OF SCOPE (git-history aware). STAYS as the cron workflow. |

**Cross-reference with the existing v0.10+ candidate list in
[`docs/launch-prep.md`](../../docs/launch-prep.md):**

- `cross_file_value_equals` — confirmed by node (`tools/eslint-rules/*`
  ↔ `eslint.config.mjs`). Already the single strongest demand signal in
  P2a (now 7 sources after node).
- `registry_paths_resolve` — confirmed by node (`tools/dep_updaters/`
  ↔ `deps/`). Already the second-strongest demand (5 sources after node).

**NEW candidate not previously inventoried:**

- `file_header_consistency` (or, equivalently, `file_header.alt_pattern`
  field on the existing rule) — asserts a file matches one of N canonical
  header patterns rather than exactly one. Surfaced uniquely by node's
  C++ source tree where the historical Joyent BSD/MIT banner has drifted
  to a "no banner, just an `#include` and `#ifndef` guard" convention
  for new files. Niche — the cleaner outcome is editorial cleanup, not
  a new rule kind. Logged as a v0.10+ candidate, rated low priority.

---

## Out of alint's scope (use the existing tool)

Same framing as the cpython, kubernetes, rust-lang/rust, microsoft/typescript
case studies: AST-aware, codegen, binary, and deep-domain checks stay
on the existing tooling. alint's non-goals are deliberate.

- **`tools/eslint-rules/*`** — 27 TSESTree visitors
- **`tools/cpplint.py`** — C++ AST analysis (vendored Google cpplint fork)
- **`tools/checkimports.py`** — C++ `using <ns>::<name>;` declaration parsing
- **`tools/lint-md/lint-md.mjs`** — markdown AST (remark)
- **`tools/lint-pr-url.mjs`** — PR-diff aware
- **`tools/lint-readme-lists.mjs`** — HTTP fetch + GitHub API queries
- **`tools/find-inactive-{collaborators,tsc}.mjs`** — `git log` walks
- **`tools/test.py`** — runtime test execution
- **`tools/build_addons.py`**, **`tools/install.py`**, **`tools/copyfile.py`** — build / install helpers
- **`tools/dep_updaters/update-*.{sh,mjs}`** — dependency-update orchestration (the PRESENCE check we DO express; the orchestration logic is out of scope)
- **`tools/clang-format`** — formatter (formatter, not linter; alint shells out via a `--dry-run --Werror` invocation as an INFO-level rule)
- **`make format-cpp`** — `git merge-base` diff scoping for clang-format
- **`make lint-addon-docs`** — addon-docs DSL parser
- **`tools/doc/*`** — API docs generator
- **`commit-lint.yml`** — git commit-message regex (alint has `git_commit_message` but the node convention is rich enough that the existing tool wins)
- **`tools/dep_updaters/nghttp.kbx`** — GnuPG keyring (binary, signing keys)
- **`.github/workflows/scorecard.yml`** — OpenSSF Scorecard (security scanner)
- **`.github/workflows/codeql.yml`** — CodeQL static analysis
- **All operational / labelling / notification / stale-bot workflows** — not validation surfaces

---

## Already covered by other linters node uses

- **eslint** (with the root `eslint.config.mjs` + 7 per-tier partials +
  27 custom rules) — alint shells out per tier rather than competing on
  JS rule expressivity.
- **cpplint** (forked under `tools/cpplint.py`) — alint shells out.
- **clang-format** (`tools/clang-format/` pinned binary) — alint shells
  out via a `--dry-run --Werror` invocation.
- **ruff** (configured in `pyproject.toml`) — alint shells out per-file.
- **yamllint** (vendored under `tools/pip/`) — alint shells out per-file.
- **shellcheck** (via `tools/lint-sh.mjs`) — alint shells out per-file.
- **lint-md / remark-preset-lint-node** (vendored under `tools/lint-md/`)
  — alint shells out per-file.
- **CodeQL** (`.github/workflows/codeql.yml`) — never an alint target.
- **OpenSSF Scorecard** (`.github/workflows/scorecard.yml`) — never an
  alint target; alint *does* restate the action-SHA-pinning + permission-block
  invariants Scorecard checks, so they surface at PR time instead of on
  the next nightly run.

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts:

- `oss-baseline@v1` (license, README, gitignore, no merge markers,
  no bidi)
- `node@v1` (package.json + lockfile + node_modules hygiene)
- `ci/github-actions@v1` (workflow permissions / action pinning)
- `hygiene/no-tracked-artifacts@v1` (no `.DS_Store`, build outputs, etc.)
- `tooling/editorconfig@v1` (trim trailing whitespace, insert final
  newline, etc.)

Plus 40 node-specific rules covering:

- 4 broad source-tree hygiene rules (no-trailing-whitespace,
  final-newline, line-endings, no-bidi-controls) over `lib/` + `src/` +
  `tools/` + `doc/` + `*.md`, with `deps/` + `test/fixtures/` excluded
- 3 line-endings rules (Windows batch CRLF, doc/ markdown LF, broad
  source-tree LF) per the `.gitattributes` policy table
- **1 `filename_regex` for `test/{parallel,sequential,async-hooks,...}/`
  test discovery** (the headline finding — enforced nowhere statically
  today)
- **1 `filename_regex` for `doc/changelogs/CHANGELOG_V*.md`**
  (per-major-version convention — enforced nowhere statically today)
- 4 `file_exists` blocks for governance / contributing / build /
  src / lib substantive entries
- 4 `file_content_matches` rules over `src/node_version.h` macros
  (`NODE_MAJOR_VERSION`, `NODE_MINOR_VERSION`, `NODE_PATCH_VERSION`,
  `NODE_VERSION_LTS_CODENAME`)
- 2 `file_exists` for `.github/CODEOWNERS` + `file_min_lines` floor
- 2 `file_exists` for the root + per-tier eslint configs (catches
  silent-coverage-drop on tier deletion)
- 1 `file_exists` for the canonical `tools/eslint-rules/*.js` entries
- 1 `file_exists` for `lib/internal/per_context/primordials.js`
- 1 `file_exists` for the lint-tooling configs (`.clang-format`,
  `.cpplint`, `pyproject.toml`, `tools/lint-md/package.json`,
  `tsconfig.json`, etc.)
- 2 `json_path_matches` rules pinning `tools/lint-md/package.json`'s
  `remark-preset-lint-node` and `remark-parse` deps
- 1 `toml_path_matches` for `pyproject.toml`'s `[tool.ruff].target-version`
- 1 `file_content_matches` for the reusable-workflow naming convention
- **1 `yaml_path_matches` for action-SHA pinning across all workflows**
  (uses RFC 9535 `match()` filter syntax)
- 1 `file_content_matches` for workflow-level permissions blocks
- 9 `command:` rules shelling out to eslint (×4 tiers) + cpplint +
  checkimports + clang-format + ruff + yamllint + lint-md + shellcheck
- 1 `dir_absent` over `out/`, `build/`, `Release/`, `Debug/`
- 1 `json_path_equals` over `tsconfig.json` `compilerOptions.strict: true`
- 2 `file_min_lines` floors on `.clang-format` + `.cpplint`

The remaining 25 inventoried surfaces:

- 4 need new alint primitives (above) — file as v0.10+ feature requests
- 21 are out of alint's scope (above) — keep on the existing tooling

---

## Performance comparison (placeholder — bench when validation pass scales)

`make lint` over the full node tree (excluding deps/) is a 5-7 minute
operation on a stock CI runner — eslint dominates (~3 minutes for
~445 src + ~15k lib/test files), cpplint adds another ~90 s, lint-md
~30 s, ruff ~5 s. The pre-commit pipeline runs each in sequence over
the staged files only.

The alint pitch here is **not** speed — it's **inventory legibility**. A
new contributor staring at node's structural-validation surface today
has to read the 1700-line Makefile, 25 GitHub Actions workflows, 27
custom eslint rules, 7 per-tier eslint partials, the `.gitattributes`
table, the `.editorconfig`, the `tools/lint-*.mjs` helpers, the
vendored `tools/cpplint.py` + `tools/checkimports.py`, the
`tools/lint-md/` remark pipeline, and `pyproject.toml` to understand
what rules apply where. The alint config in this directory is **one
file**, declarative, with each rule's scope, severity, and rationale
visible in 5-10 lines.

For the 43 % of checks that fit alint's grammar today, the pitch is:
**"adopt alint to consolidate the orchestration layer so contributors
can read the structural contract in one file."** The deep tools
(`tools/cpplint.py`, `tools/eslint-rules/*`, `tools/lint-md/lint-md.mjs`,
`tools/checkimports.py`) stay where they are.

To benchmark wall-clock: `time make lint-ci` (after a warm tools/
build) vs `time alint check` against the same tree, then compare the
unique-violation overlap. Deferred to the per-repo measurement pass; we
expect alint to be faster on the orchestration subset (parallel walks,
no per-tool process spawn) and roughly equivalent on the
shell-out-to-eslint subset (the eslint invocation dominates).

---

## Recommendation for the launch story

**Headline launch quote:** "node's structural validation is the
broadest surface alint has linted in P2a — 44 distinct surfaces across
a 1700-line Makefile, 25 workflows, 27 in-tree custom eslint rules, 7
per-tier eslint partials, a vendored Google cpplint fork, a remark-lint
pipeline, ruff, yamllint, lint-md, lint-pr-url, lint-readme-lists,
lint-sh, find-inactive-collaborators, and an unwritten test-discovery
filename grammar. alint consolidates the 43 % that's declarative
orchestration into one 40-rule config — and the two most alint-shaped
surfaces (the `test/parallel/test-*.{js,mjs,cjs}` discovery grammar
and the `doc/changelogs/CHANGELOG_V<MAJOR>.md` per-major-version
convention) are enforced nowhere statically today, only by editorial
review and 'a missing test silently fails to fire'."

This is the **fourth positioning narrative** crystallised in P2a-Wave 3
— it doubles down on the **maturity** angle that complements typescript
(stable & meticulous) and cpython (scattered & hand-rolled):

| Narrative | Strongest data point | Use case |
|---|---|---|
| "Replaces N hand-rolled validation scripts" | kubernetes (50 → 17), airflow (109 hooks → 40 %), cpython (12 surfaces consolidated) | Repos with verify-script sprawl |
| "Catches conventions your pipeline assumes but doesn't verify" | tokio (15 conventions, 0 scripts), uv (67-crate workspace), pnpm (`meta-updater`), react (`ReactVersion.js`), **node (test-discovery grammar + per-major-changelog grammar)** | Repos that rely on convention without explicit checks |
| "Adds a structural floor on top of mature tooling" | typescript (eslint+dprint+knip), prettier (5 net-new gates), **node (eslint + cpplint + clang-format + ruff + yamllint + lint-md all coexist; alint sits BENEATH them as orchestrator)** | Repos with mature tooling but missing structural layer |
| **NEW: "Maturity is the hard test"** | **node (44 surfaces, 15 years, every conceivable convention pinned somewhere — alint expresses 43 % cleanly without overstepping)** | Repos so old that the conventions have *always* worked, but where a new contributor has no single place to read them |

node sits at the **intersection of all four** — it has scattered
hand-rolled validation scripts (`tools/checkimports.py`,
`tools/lint-pr-url.mjs`, `tools/find-inactive-*.mjs`), it has
unverified conventions (test-discovery grammar, per-major-changelog
grammar, the C++ EOL banner), it has mature per-language linting
(eslint + cpplint + ruff + yamllint + lint-md), AND it has accumulated
15 years of "this just works" institutional knowledge that no single
file documents. The alint pitch lands as: **"we sit beneath your
existing linters as the structural-orchestration layer, we collapse the
hand-rolled scripts you've outgrown, we surface the conventions your
tooling assumes but never checks, AND we make 15 years of institutional
discipline legible to a contributor reading the repo for the first time."**

Followup feature work surfaced (priority order):

- **`cross_file_value_equals`** — seventh confirmation. node's
  `tools/eslint-rules/*` ↔ `eslint.config.mjs` registration adds to the
  existing 6-source mandate; this remains v0.10+'s single
  highest-leverage gap.
- **`registry_paths_resolve`** — fifth confirmation. node's
  `tools/dep_updaters/update-<libname>.{sh,mjs}` ↔ `deps/<libname>/`
  cross-reference adds to the rust + clap + cpython×2 mandate.
- **`file_header_consistency` (NEW; low priority)** — surfaced uniquely
  by node's drifted C++ EOL-banner convention. Niche; the cleaner
  outcome is a one-time editorial sweep that picks one convention.
  Logged for v0.10+ review.

---

## No NEW schema/language pitfalls hit

The 16 documented in `docs/development/CONFIG-AUTHORING.md` cover
everything that came up while authoring this config. Specific near-
misses navigated:

- **§13 (regex anchoring)** — every `^` / `$` in this config is `(?m)`
  prefixed (the `node_version.h` macro check, the workflow-permissions
  check, the reusable-workflow workflow_call check) because each file
  is multi-line.
- **§16 (`*_path_matches` against bool fields)** — `tsconfig.json`'s
  `compilerOptions.strict: true` is a bool, so we use `json_path_equals`
  with a YAML-native `equals: true` literal rather than reaching for
  `json_path_matches`.
- **JSONPath bracket notation for dashed keys (§10)** — the
  `package.json` deps `remark-preset-lint-node` and `remark-parse`
  contain dashes, so the JSONPath is `$.dependencies['remark-preset-lint-node']`,
  not `$.dependencies.remark-preset-lint-node`.
- **JSONPath `match()` filter (honourable mention in CONFIG-AUTHORING)**
  — the action-SHA pinning rule uses
  `$.jobs.*.steps[?match(@.uses, '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+@')].uses`
  as documented.

The `coverage_audit_examples_parse.rs` audit passes with this config
in place (run from the repo root).
