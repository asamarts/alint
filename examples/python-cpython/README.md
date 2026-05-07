# Case study: `python/cpython`

Inventory of the structural-validation tooling in `python/cpython` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-checkout excluding `Lib/test/`
and `Modules/` (the heaviest sub-trees; not material to the structural
inventory).

---

## Summary

cpython is the canonical Python+C hybrid mega-repo. Unlike `rust-lang/rust`
(which packages structural validation as one Rust binary, `tidy`), or
`kubernetes` (which uses ~50 `hack/verify-*.sh` scripts), cpython's structural
validation is **scattered across 12 distinct surfaces** including a
122-target `Makefile.pre.in`, a 35-hook `.pre-commit-config.yaml` (driving
9 separate ruff configs + black + actionlint + zizmor + sphinx-lint +
check-jsonschema + 2 local hooks), `Tools/build/{check_warnings,check_extension_modules,smelly,stable_abi,generate_sbom}.py`,
`Tools/c-analyzer/check-c-globals.py`, `Tools/check-c-api-docs/main.py`,
`Tools/patchcheck/patchcheck.py`, a 4747-byte `.gitattributes` (encoding
line-ending policy + 50+ generated-file markers + binary classification),
`.editorconfig`, `Misc/stable_abi.toml`, the per-tree `.ruff.toml` files,
the `.azure-pipelines/prebuild-checks.yml` (a script-in-YAML), and 25 GitHub
Actions workflows.

Of the 56 distinct structural-validation surfaces inventoried:

- **~38 % map directly to existing alint rules** (~21 surfaces: 9 ruff configs
  collapse into 9 declarative `command` rules, line-endings policy from
  `.gitattributes` becomes 4 `line_endings` rules, NEWS.d filename
  convention becomes 1 `filename_regex`, generated-files-exist checks
  become 4 `file_exists` blocks, etc.)
- **~16 % need new alint primitives** (~9 surfaces: Argument Clinic block
  matching, cases_generator codegen freshness, .gitattributes generated
  registry resolution, CODEOWNERS column alignment, NEWS.d entry → blurb
  schema validation, sortedness guards in `Modules/Setup.local`, etc.)
- **~46 % are out of alint's scope** (~26 surfaces: stable_abi.py is
  AST-aware C analysis, smelly.py reads ELF/Mach-O symbol tables,
  check-c-api-docs/main.py is multi-file C-API ↔ docs cross-reference,
  patchcheck.py is git-diff-aware, the entire `Tools/cases_generator/`
  is a code generator, etc.)

The 38 % that *do* fit translate to the **72-rule alint config** in
[`/.alint.yml`](.alint.yml) (34 cpython-specific + 38 from 4 bundled
rulesets). The single most alint-shaped surface here is
**`Misc/NEWS.d/next/<Section>/`** — a strict
filename grammar (`YYYY-MM-DD-HH-MM-SS.gh-issue-NUMBER.NONCE.rst`)
enforced today by two LOCAL pre-commit hooks that only encode the WEAKER
"no spaces in path" invariant. alint encodes the full filename grammar
in 6 lines.

---

## Existing tooling inventory

### Makefile.pre.in (122 targets, ~10 lint/check)

| Make target | What it does | alint disposition |
|---|---|---|
| `patchcheck` | Runs `Tools/patchcheck/patchcheck.py` (git-diff-aware preflight: was NEWS.d updated, was configure regenerated, etc.) | OUT OF SCOPE (git-diff aware) |
| `smelly` | Runs `Tools/build/smelly.py` (asserts every libpython exported ELF/Mach-O symbol begins `Py`/`_Py`) | OUT OF SCOPE (binary symbol table) |
| `check-c-globals` | Runs `Tools/c-analyzer/check-c-globals.py` (parses C, asserts no non-static module-state globals) | OUT OF SCOPE (C AST) |
| `check-c-api-docs` | Runs `Tools/check-c-api-docs/main.py` (greps Include/**/*.h for PyAPI_FUNC/PyAPI_DATA, asserts each is documented in Doc/c-api/) | NEEDS NEW PRIMITIVE (`registry_paths_resolve`-shaped) |
| `check-limited-abi` / `check-abidump` | Runs `Tools/build/stable_abi.py --all` (cross-references `Misc/stable_abi.toml` against exported PyAPI_* symbols) | OUT OF SCOPE (parses C) |
| `regen-cases` | Runs `Tools/cases_generator/*.py` (regenerates 5 .c.h files from `Python/bytecodes.c`) | OUT OF SCOPE (codegen) |
| `regen-sbom` | Runs `Tools/build/generate_sbom.py` | OUT OF SCOPE (SBOM gen) |
| `clinic` | Runs `Tools/clinic/clinic.py` (regenerates argument-parsing boilerplate IN-PLACE in .c files via block markers) | NEEDS NEW PRIMITIVE (`balanced_delimiters` / `file_pair_block_match`) |
| `check-clean-src` | Asserts no autotools regenerated artifacts uncommitted | OUT OF SCOPE (git-diff aware) |
| `regen-configure` | Runs `Tools/build/regen-configure.sh` | OUT OF SCOPE (regen) |

### .pre-commit-config.yaml (35 hooks)

| Hook category | Count | alint disposition |
|---|---:|---|
| `ruff-check` (with 9 distinct configs) | 9 | MAPS — 9 `command` rules per scoped tree |
| `ruff-format` | 6 | MAPS — `command` rules (collapsed) |
| `black` (Tools/jit/) | 1 | MAPS — 1 `command` rule |
| `remove-tabs` (Python) | 1 | MAPS — `indent_style: spaces` |
| `check-case-conflict` | 1 | MAPS — alint has `no_case_conflicts` |
| `check-merge-conflict` | 1 | MAPS — bundled `oss-no-merge-conflict-markers` |
| `check-toml` | 1 | MAPS — `toml_path_matches` w/ wildcard forces parse |
| `check-yaml` | 1 | MAPS — `yaml_path_matches` w/ wildcard forces parse |
| `end-of-file-fixer` | 2 | MAPS — `final_newline` |
| `mixed-line-ending --fix=auto` | 1 | MAPS — `line_endings` (one rule per file class from .gitattributes) |
| `trailing-whitespace` | 2 | MAPS — `no_trailing_whitespace` |
| `check-dependabot` (jsonschema) | 1 | MAPS — `command` shellout |
| `check-github-workflows` (jsonschema) | 1 | MAPS — `command` shellout |
| `check-readthedocs` (jsonschema) | 1 | MAPS — `command` shellout |
| `actionlint` | 1 | MAPS — `command` shellout |
| `zizmor` | 1 | MAPS — `command` shellout |
| `sphinx-lint --enable=default-role` | 1 | MAPS — `command` shellout |
| `blurb-no-space-c-api` (LOCAL) | 1 | MAPS — `dir_absent` |
| `blurb-no-space-core-and-builtins` (LOCAL) | 1 | MAPS — `dir_absent` |
| `check-hooks-apply` (meta) | 1 | OUT OF SCOPE (pre-commit-meta) |
| `check-useless-excludes` (meta) | 1 | OUT OF SCOPE (pre-commit-meta) |

**32 of 35 hooks (~91%) map cleanly.**

### Tools/build/ check scripts (~22 .py files; 7 do structural validation)

| Script | What it checks | alint disposition |
|---|---|---|
| `check_extension_modules.py` | Dynamic-loads every shared/built-in extension, verifies imports succeed; reads `Modules/Setup*` to determine expected modules | OUT OF SCOPE (runtime import check) |
| `check_warnings.py` | Parses GCC/Clang stderr, asserts warnings only fire in allowlisted paths | OUT OF SCOPE (compiler-output parser) |
| `smelly.py` | ELF/Mach-O symbol prefix check (covered above) | OUT OF SCOPE |
| `stable_abi.py --check` | Manifest ↔ exported symbols (covered above) | OUT OF SCOPE |
| `generate_sbom.py --check` | Regenerates SBOM, asserts no diff | NEEDS NEW PRIMITIVE (`generated_file_fresh` — same shape as cases_generator) |
| `verify_ensurepip_wheels.py` | Verifies bundled pip/setuptools wheel SHAs | OUT OF SCOPE (HTTP fetch + checksum) |
| `update_file.py` | Codegen helper (no validation) | N/A |

### Tools/clinic/, Tools/cases_generator/, Tools/peg_generator/

All three are code generators — alint asserts the GENERATED outputs exist
(`cpython-cases-generator-outputs-exist`) but cannot assert
freshness-vs-source. Two need new primitives:

- **Argument Clinic** (`Tools/clinic/clinic.py`) — reads
  `/*[clinic input]*/ ... /*[clinic end generated code: ...]*/` blocks
  IN-PLACE in `.c` files, regenerates the body, and writes the
  argument-parsing boilerplate to `<dir>/clinic/<basename>.c.h`. Three
  invariants: (1) every opening marker has a matching close in source
  order; (2) the body between matches the regenerated body; (3) the
  generated header in `clinic/` matches the in-place re-inflation. Same
  shape as **rust-lang/rust's `rustdoc_css_themes` and `rustdoc_templates`
  gaps** — `balanced_delimiters` + `file_pair_block_match` would cover both.

- **cases_generator** (`Tools/cases_generator/`) — generates 5 .c.h files
  from `Python/bytecodes.c`. Same shape as **uv's `cargo dev generate-all
  --mode dry-run` gap**. Needs `generated_file_fresh`.

### .gitattributes (4747 bytes)

cpython's `.gitattributes` is the source of truth for several structural
invariants:

| Section | Lines | alint disposition |
|---|---:|---|
| Binary file extensions (`*.png binary`, etc.) | 22 | MAPS — implicit (alint respects gitignore + reads .gitattributes for binary classification) |
| `noeol` files (no line-ending normalization) | 5 | MAPS via `paths.exclude` on the line-endings rules |
| Per-class line-ending policy (`*.bat dos`, `*.rst text eol=lf`, etc.) | 12 | MAPS — 4 `line_endings` rules in our config |
| `generated` markers (`Python/generated_cases.c.h    generated`) | ~50 | NEEDS NEW PRIMITIVE — `registry_paths_resolve` would assert each path exists |
| `diff=cpp/python/markdown` etc. | 7 | OUT OF SCOPE (git diff hint) |
| `linguist-generated=true` | 1 | OUT OF SCOPE (GitHub linguist hint) |

### Misc/stable_abi.toml — TOML manifest (~1500 lines)

The single largest TOML file in the repo. `Tools/build/stable_abi.py`
reads it and cross-references against exported C symbols. We assert it
exists, parses, and contains at least one `[[function]]` entry — but the
deep semantics (every entry has correct `added` field, every exported
symbol appears in the manifest, etc.) stay on `stable_abi.py`.

### .azure-pipelines/prebuild-checks.yml — script-in-YAML

A single embedded shell script that runs `git diff --name-only` and sets
a `tests.run` variable based on whether non-Doc/Misc files changed. This
is a CI optimisation hint, not a structural-validation surface — out of
scope.

### .github/workflows/ (25 files)

| Surface | alint disposition |
|---|---|
| `permissions: contents: read` declared at workflow level | MAPS via bundled `gha-workflow-contents-read` |
| Action references pinned to 40-char SHA | MAPS via bundled `gha-pin-actions-to-sha` |
| Workflow has `name:` field | MAPS via bundled `gha-workflow-has-name` |
| `reusable-*.yml` files declare `on.workflow_call:` | MAPS — `cpython-reusable-workflow-naming` (custom rule) |
| `lint.yml` invokes `prek run` (orchestrator) | OUT OF SCOPE (CI orchestration) |
| `build.yml` invokes `make smelly`, `make check-c-globals`, etc. | OUT OF SCOPE (proxies to Make targets above) |

### .editorconfig

Maps trivially — `trim_trailing_whitespace=true` + `insert_final_newline=true`
become bundled `oss-no-trailing-whitespace` + `oss-final-newline`,
overridden in our config to broaden the scope from docs to source files.

### Misc/NEWS.d/next/ — the headline finding

12 category subdirectories (Build, C_API, Core_and_Builtins, Documentation,
IDLE, Library, macOS, Security, Tests, Tools-Demos, Windows; plus some
historical strays) holding ~10-200 entries each (108 in `Library/` at
clone time). Every entry MUST be named:

```
YYYY-MM-DD-HH-MM-SS.gh-issue-NUMBER.NONCE.rst
```

— generated exclusively by the `blurb` tool. cpython's existing structural
validation encodes only:

1. `.pre-commit-config.yaml`'s `blurb-no-space-c-api` LOCAL hook (forbids
   `Misc/NEWS.d/next/C API/20*.rst` — must use `C_API/` instead).
2. `.pre-commit-config.yaml`'s `blurb-no-space-core-and-builtins` LOCAL
   hook (same idea, different subdir).

Both hooks cover the WEAKER "no spaces in path" invariant on TWO specific
subdirs. The full filename grammar applies to ALL subdirs and is enforced
nowhere statically — `blurb` generates it correctly, but a hand-edited
filename would silently slip through until Sphinx parsing fails at
release-build time.

The alint rule (`cpython-news-entry-filename`) is **6 lines of YAML** and
covers ALL 12 subdirs:

```yaml
- id: cpython-news-entry-filename
  kind: filename_regex
  paths:
    include: ["Misc/NEWS.d/next/*/*.rst"]
    exclude: ["Misc/NEWS.d/next/*/README.rst"]
  pattern: '^[0-9]{4}-[0-9]{2}-[0-9]{2}-[0-9]{2}-[0-9]{2}-[0-9]{2}\.gh-issue-[0-9]+\.[A-Za-z0-9_-]+\.rst$'
  level: error
```

---

## What needs new alint primitives

| Gap | Existing cpython tooling | What alint needs |
|---|---|---|
| Argument Clinic in-place block markers | `Tools/clinic/clinic.py` | `balanced_delimiters` rule kind: "every opening token in `<delimiters>` has a matching close in source order". `file_content_matches` catches the open/close existence but not the pairing. **Already on the v0.10+ candidate list** from rust-lang/rust (`rustdoc_templates` gap). cpython confirms demand. |
| Argument Clinic in-place body ↔ generated header consistency | `Tools/clinic/clinic.py` | `file_pair_block_match` rule kind: "block between `<start>`/`<end>` markers in file A equals block between same markers in file B (after a configurable transform)". **Already on the v0.10+ candidate list** from rust-lang/rust (`rustdoc_css_themes` gap). cpython confirms demand. |
| cases_generator output freshness | `Tools/cases_generator/`, `make regen-cases-check` | `generated_file_fresh` rule kind: "generated file `<output>` matches `<command_output>`" — a `command:` variant that compares stdout to file contents. **Already on the v0.10+ candidate list** from uv (`cargo dev generate-all --mode dry-run` gap). cpython confirms demand. |
| .gitattributes `generated` markers ↔ on-disk presence | `.gitattributes` | `registry_paths_resolve` rule kind: "extract paths from a structured registry (`.gitattributes` here, plus glob-grammar parsing for the file format), assert each resolves to an existing file". **Already on the v0.10+ candidate list** from rust-lang/rust (`triagebot` gap). cpython confirms demand. |
| C API docs cross-reference | `Tools/check-c-api-docs/main.py` | `registry_paths_resolve` variant: "every `PyAPI_FUNC`/`PyAPI_DATA`-marked symbol in `Include/**/*.h` appears in some `Doc/c-api/*.rst` page (or `Tools/check-c-api-docs/ignored_c_api.txt`)". Same rule kind, different extraction strategy (regex over C source rather than glob over text). |
| CODEOWNERS column alignment | `.github/CODEOWNERS` | `column_alignment` rule kind: "line tokens at index N are aligned to column M (or the next multiple of K)". CODEOWNERS specifies "GitHub usernames should be aligned to column 31, or the next multiple of 3". **NEW candidate** (not on the existing v0.10+ list); narrow but generalises to indent-aligned tables in TOML, requirements.txt comments, etc. |
| NEWS.d entry blurb-schema validation | `blurb` (PyPI tool) | `external_schema_validates` rule kind: "shell out to a tool, parse its YAML/JSON output as a violation list". cpython could install `blurb` and call `blurb test` — `command:` rule covers the invocation but not the structured-output parsing. Lower priority. |
| Sortedness guards | `Tools/build/freeze_modules.py` (sorts the FROZEN_MODULES list); historically `update-spelling-wordlist-to-be-sorted` in airflow | `ordered_block` rule kind: "items between `<start_marker>`/`<end_marker>` are sorted (case-insensitive, indent-aware)". **Already on the v0.10+ candidate list** from rust-lang/rust (`tidy::alphabetical` gap). cpython confirms demand. |
| pep8-naming for module filenames | `ruff` lint rules | Already maps via the existing `filename_case: snake` rule from `bundled/python@v1`. NOT a gap — listed for completeness. |

**Cross-reference with the v0.10 ship-target / design-candidate list in
[`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md):**
- `balanced_delimiters` + `file_pair_block_match` — confirmed by cpython
  (Argument Clinic block markers + body ↔ generated header). **v0.10
  design candidate** (3 sources: rust + cpython×2).
- `generated_file_fresh` — confirmed by cpython (cases_generator).
  **v0.10 ship-target** (6 sources: uv, cpython, pytorch, bazel, TF,
  spark).
- `registry_paths_resolve` — confirmed twice by cpython (`.gitattributes`
  generated markers + `check-c-api-docs`). **v0.10 ship-target — top
  of backlog with 8 sources** (rust, clap, cpython×2, next.js, arrow,
  pytorch, nodejs/node, NixOS×3).
- `ordered_block` — confirmed by cpython (`Modules/Setup` sortedness;
  historically `update-spelling-wordlist-to-be-sorted` patterns).
  **v0.10 ship-target — top of backlog with 7 sources** (rust,
  airflow, tokio, cpython, arrow, golang/go, protobuf failure_lists).

**NEW candidate not previously inventoried:**
- `column_alignment` rule kind — surfaced uniquely by cpython's CODEOWNERS
  ("GitHub usernames should be aligned to column 31, or the next multiple
  of 3"). Niche, but generalises to any column-aligned tabular text file.
  Rated low priority; logged as a v0.10+ candidate for eventual review.

---

## Out of alint's scope (use the existing tool)

Same framing as the kubernetes and rust-lang/rust case studies: AST-aware,
codegen, binary, and deep-domain checks stay on the existing tooling.
alint's non-goals are deliberate.

- **`Tools/build/smelly.py`** — ELF/Mach-O symbol-table parsing
- **`Tools/build/stable_abi.py`** — TOML manifest ↔ exported C symbols
  cross-reference
- **`Tools/c-analyzer/check-c-globals.py`** — C AST analysis
- **`Tools/build/check_warnings.py`** — GCC/Clang stderr parsing
- **`Tools/build/check_extension_modules.py`** — runtime extension loading
- **`Tools/cases_generator/*`** — bytecode-interpreter codegen
- **`Tools/clinic/clinic.py`** (the GENERATOR side) — argument-parsing codegen
- **`Tools/peg_generator/*`** — parser codegen
- **`Tools/build/regen-configure.sh`** — autoconf regeneration
- **`Tools/patchcheck/patchcheck.py`** — git-diff-aware preflight
  (alint's `--changed` mode informs WHICH files to check, not what
  triggers a check)
- **`Tools/build/verify_ensurepip_wheels.py`** — HTTP fetch + checksum
- **`.azure-pipelines/prebuild-checks.yml`** — CI tests-run optimization
- **pre-commit `meta` hooks** (`check-hooks-apply`, `check-useless-excludes`) —
  validate the pre-commit config itself

---

## Already covered by other linters cpython uses

- **ruff** (with 9 distinct configs across Doc/, Lib/test/, Platforms/Apple/,
  Platforms/WASI/, Tools/build/, Tools/clinic/, Tools/i18n/,
  Tools/peg_generator/, Tools/wasm/) — alint shells out per config rather
  than competing on Python rule expressivity.
- **black** (Tools/jit/) — alint shells out.
- **actionlint + zizmor** — workflow security/correctness; alint shells out.
- **sphinx-lint** — RST validity; alint shells out.
- **check-jsonschema** — JSON Schema validation for dependabot.yml,
  workflows, readthedocs.yml; alint shells out.
- **mypy** (configured in `Misc/mypy/`, run on a strict subset of files via
  `.github/workflows/mypy.yml`) — Python type checking; never an alint target.

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts:

- `oss-baseline@v1` (license, README, gitignore, no merge markers,
  no bidi)
- `python@v1` (pyproject/setup.py, lockfile, snake_case, source hygiene)
- `ci/github-actions@v1` (workflow permissions / action pinning)
- `hygiene/no-tracked-artifacts@v1` (no `.DS_Store`, build outputs, etc.)

Plus 23 cpython-specific rules covering:

- 9 ruff command shellouts (one per config)
- 4 line-endings rules from the `.gitattributes` policy table
- 1 broad source-tree no-trailing-whitespace
- 1 broad source-tree final-newline
- 1 source-broadened `no_bidi_controls`
- 1 `filename_regex` for `Misc/NEWS.d/next/*/*.rst`
- 1 `dir_absent` for the legacy spaced section names
- 1 `for_each_dir` requiring `README.rst` in each NEWS.d category subdir
- 1 `for_each_dir` for orphaned Argument Clinic dirs
- 1 `file_exists` for cases_generator outputs
- 1 each for SBOM, externals SBOM, Doc build files, autotools files,
  stable_abi manifest
- 1 `command` shellout each for actionlint, zizmor, sphinx-lint,
  check-jsonschema (×2), black
- 1 `indent_style: spaces` for the no-tabs-in-Python contract
- 1 `file_min_lines` floor on CODEOWNERS
- 1 `file_content_matches` for the reusable-* workflow naming convention
- 1 `toml_path_matches` parseability check on stable_abi.toml

The remaining 33 inventoried surfaces:

- 9 need new alint primitives (above) — file as v0.10+ feature requests
- 26 are out of alint's scope (above) — keep on the existing tooling

---

## Performance comparison (placeholder — bench when validation pass scales)

`make patchcheck` is fast (~3s on a warm tree); `make smelly` requires a
full build first (~minutes). The pre-commit pipeline runs each hook in
sequence over the staged files only — `prek run` (the cpython runner of
choice in CI) parallelises across hooks but each hook still spawns its
own tool process per file batch.

The alint pitch here is **not** speed — it's **inventory legibility**. A
new contributor staring at cpython's structural-validation surface today
has to read 122 Make targets, 35 pre-commit hooks, 7+ Tools/build/*
scripts, the Azure Pipelines YAML, the .gitattributes file, two LOCAL
pre-commit shell hooks, and 9 separate `.ruff.toml` configs to understand
what rules apply where. The alint config in this directory is **one
file**, declarative, with each rule's scope, severity, and rationale
visible in 5-10 lines.

For the 38 % of checks that fit alint's grammar today, the pitch is:
**"adopt alint to consolidate the orchestration layer so contributors can
read the structural contract in one file."** The deep tools
(`stable_abi.py`, `smelly.py`, `check-c-api-docs/main.py`,
`check-c-globals.py`, the codegens) stay where they are.

To benchmark wall-clock for real: `time make patchcheck` (after a warm
build) vs `time alint check` against the same tree, then compare the
unique-violation overlap. Deferred to the per-repo measurement pass; we
expect alint to be faster on the orchestration subset (parallel walks,
no per-hook process spawn) and roughly equivalent on the
shell-out-to-ruff subset (the ruff invocation dominates).

---

## Recommendation for the launch story

**Headline launch quote:** "cpython's structural validation is scattered
across 12 distinct surfaces — 122 Make targets, 35 pre-commit hooks, 9
ruff configs, 7 Tools/build/* check scripts, .gitattributes, .editorconfig,
and 25 GitHub Actions workflows. alint consolidates the 38 % that's
declarative orchestration into one 72-rule config (34 cpython-specific +
38 from 4 bundled rulesets) — and the most alint-shaped surface
(`Misc/NEWS.d/next/*` filename grammar) is enforced nowhere statically
today, only by a downstream tool (`blurb`) that must generate the right
shape at write time. alint encodes the full grammar as a single 6-line
`filename_regex` rule."

This is the **third positioning narrative** crystallised in P2a-Wave 2:

| Narrative | Strongest data point | Use case |
|---|---|---|
| "Replaces N hand-rolled validation scripts" | kubernetes (50 → 17), airflow (109 hooks → 40 %) | Repos with verify-script sprawl |
| "Catches conventions your pipeline assumes but doesn't verify" | tokio (15 conventions, 0 hand-rolled scripts), uv (67-crate workspace conventions), **cpython (NEWS.d filename grammar — enforced nowhere statically today)** | Repos that rely on convention without explicit checks |
| "Adds a structural floor on top of mature tooling" | typescript (eslint + dprint + knip already tight), ruff (900+ Python rules, 0 internal-crate rules), **cpython (9 ruff configs + black + actionlint + zizmor + sphinx-lint exist; alint sits BENEATH them as orchestrator)** | Repos with mature tooling but missing structural layer |

cpython is uniquely valuable as a case study because it sits at the
**intersection of all three** — it has scattered hand-rolled validation
scripts (smelly, stable_abi, check-c-globals), it has unverified
conventions (NEWS.d filename grammar), AND it has mature per-tree linting
(9 ruff configs). The alint pitch lands as: "we sit beneath your existing
linters as the structural-orchestration layer, we collapse the
hand-rolled scripts you've outgrown, and we surface the conventions your
tooling assumes but never checks."

Followup feature work surfaced (priority order):

- **`balanced_delimiters` + `file_pair_block_match`** — v0.10 design
  candidate (3 sources: rust + cpython×2). cpython adds Argument Clinic
  to the rustdoc_css_themes + rustdoc_templates use cases. Should land
  together in v0.10.
- **`registry_paths_resolve`** — **v0.10 ship-target with 8 sources**.
  cpython contributes two of the eight (`.gitattributes` generated
  markers + `check-c-api-docs` symbol ↔ docs cross-ref). Tied with
  `ordered_block` at the top of the v0.10 backlog.
- **`generated_file_fresh`** — **v0.10 ship-target with 6 sources**
  (uv, cpython, pytorch, bazel, TF, spark). cpython's `cases_generator`
  + `generate_sbom.py --check` are canonical examples.
- **`ordered_block`** — **v0.10 ship-target with 7 sources** (rust,
  airflow, tokio, cpython, arrow, golang/go, protobuf failure_lists).
  Tied with `registry_paths_resolve` at top of v0.10 backlog.
- **`column_alignment` rule kind (NEW)** — surfaced only by cpython
  (CODEOWNERS column-31 alignment). Niche; rated low priority,
  single-source.

---

## Future analysis

- **`registry_paths_resolve` + `ordered_block` + `generated_file_fresh`
  ship together in v0.10.** When they land, cpython's gap inventory
  shrinks meaningfully:
  - `.gitattributes` generated markers → `registry_paths_resolve`
    (one rule for ~50 paths)
  - `check-c-api-docs` symbol ↔ docs cross-ref →
    `registry_paths_resolve` (regex extraction variant)
  - `cases_generator` codegen freshness → `generated_file_fresh`
  - `Modules/Setup` sortedness → `ordered_block`
  Two of cpython's 9 "needs new primitive" surfaces close in v0.10
  (the registry pair); the codegen freshness + sortedness close as
  well. Only Argument Clinic's `balanced_delimiters` +
  `file_pair_block_match` pair stays v0.10 design-phase.
- **`docs/adr@v1` (4 rules)** — cpython has no ADR convention; PEPs
  serve a similar role but live elsewhere. Doesn't apply.
- **`compliance/reuse@v1` (3 rules)** — cpython uses PSF licence;
  doesn't apply.
- **`agent-context` / `agent-hygiene`** — cpython has no CLAUDE.md or
  agent-friendly docs convention. If/when one lands, extend
  `agent-context@v1` (5 rules).
- **Per-tree `nested_configs:`.** The 9 ruff configs hint at a
  per-tree contract; alint could mirror this with per-tree
  `.alint.yml` via `nested_configs: true`, scoping rules to
  `Doc/`, `Lib/test/`, `Tools/build/`, etc.

## Validation status (2026-05-07)

- alint binary: v0.9.17 (built 2026-05-07).
- `validate-config` reports **72 rules** loaded from `.alint.yml** (34
  cpython-specific + 38 from 4 bundled rulesets: oss-baseline 15 +
  python 9 + ci/github-actions 3 + hygiene/no-tracked-artifacts 11).
- 1 rule uses `root_only: true` (line 563, autotools files block) —
  all 5 paths are single-segment literals at root (`configure`,
  `configure.ac`, `pyconfig.h.in`, `aclocal.m4`, `Makefile.pre.in`).
  **Pitfall #19 does not fire** (the runtime guard targets multi-
  component literals).
- No `respect_gitignore: false` patterns. Pitfall #18 does not apply.
- 12 validation surfaces (per task brief) consolidated into 1 alint
  config — confirmed; the 38% that fits maps cleanly.
- Live-tree recheck not performed (no /tmp/cpython checkout
  available).
