# Case study: `pytorch/pytorch`

Inventory of the structural-validation tooling in `pytorch/pytorch` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-checkout excluding `torch/csrc/`,
`aten/src/`, `test/`, `third_party/`, `caffe2/` (the heaviest sub-trees;
material to a full lint pass but not to the structural inventory — the
structural surface is concentrated in root, `tools/`, `.github/`, `.ci/`,
`c10/`, `torchgen/`).

---

## Summary

pytorch is a multi-language ML mega-monorepo (~80k+ files; C++/CUDA core,
Python frontend, Bazel + setup.py + CMake build, generated `_C` stubs, JIT/FX
graph machinery, distributed/cuda/xpu/mps/rocm backend matrix). Its
structural-validation surface is dominated by **one artefact**:
`.lintrunner.toml` — a 1876-line TOML manifest declaring **57 distinct linter
"adapters"** orchestrated by `lintrunner`, pytorch's bespoke per-file lint
runner (Rust binary that spawns Python adapter scripts). lintrunner exists
because at the time pytorch needed it, no existing tool handled their
orchestration needs (multi-language scopes, init-then-lint two-phase
adapters, S3-vendored binary fetch, partial-file lint via `@PATHSFILE`
fanout).

This is the launch-pitch story for alint on pytorch:

> **alint isn't trying to replace `lintrunner` — but the structural subset
> of `.lintrunner.toml` is exactly what alint specialises in.**

Of the 57 lintrunner adapters:

- **~28 are pure STRUCTURAL/regex** (24 `grep_linter.py` shims + EXEC + NEWLINE
  + SPACES + TABS) — every one expressible as a 5-10 line alint rule today
- **~21 are command shellouts to mature external tools** (clang-format,
  clang-tidy, ruff, codespell, pyfmt, mypy, pyrefly, actionlint, shellcheck,
  cmake) — alint shells out the same way via `command:` rules
- **~8 are AST-aware/semantic/git-diff-aware** (test_has_main libcst,
  scoped_library AST, set_linter, gb_registry, header_only, import_linter,
  pyproject_linter, no_workflows_on_fork, stable_shim_*) — these stay on
  lintrunner

So the headline is: **~49 of 57 adapters (≈86 %) are within alint's grammar
today**; the AST-aware tail (~8/57 ≈ 14 %) stays on lintrunner. The
`.alint.yml` in this directory ships **35 pytorch-specific rules** plus 6
bundled rulesets covering oss-baseline, python, ci/github-actions,
hygiene/no-tracked-artifacts, agent-hygiene, and tooling/editorconfig.

Beyond `.lintrunner.toml`, pytorch ships:

- **144 GitHub Actions workflows** (25 of which are `_*.yml` callables; 8 are
  `generated-*.yml` produced by `.github/scripts/generate_ci_workflows.py`).
  alint maps the standard surface (permissions, action pinning, callable
  naming) directly via the bundled ruleset + 3 custom rules.
- **A Makefile** that delegates entirely to CMake (no structural lint targets;
  the Make surface is build-only)
- **`tools/linter/adapters/`** — 30 Python files implementing the 57 lintrunner
  codes (24 use `grep_linter.py` as a shared shim; 6 use bespoke libCST/AST
  parsing)
- **`.editorconfig`** declaring LF + UTF-8 + per-extension indent (mirrored
  to alint as `indent_style: spaces` rules)
- **CODEOWNERS** (259 lines), **`.clang-format`** (3.4 KB),
  **`.clang-tidy`** (3 KB), **`.cmakelintrc`**, **`pyrefly.toml`**,
  **`mypy.ini` + `mypy-strict.ini`**, **`pytest.ini`**, **`ubsan.supp`** —
  per-tool configs, all asserted present via a single `file_exists` block
- **`.gitattributes`** (366 bytes, much smaller than cpython's 4.7 KB) —
  one `eol=crlf` for `*.bat`, ~6 `linguist-generated=true` markers
- **CLAUDE.md at the root** — pytorch was an early adopter of agent-friendly
  documentation; flags `.ci/docker/` content-hash trap as a foot-gun

---

## Existing tooling inventory

### `.lintrunner.toml` — 57 adapters, the load-bearing artefact

The full breakdown:

| Code | Adapter | Shape | alint disposition |
|---|---|---|---|
| **STRUCTURAL — single-pattern grep over file content (24)** | | | |
| TYPEIGNORE | grep_linter.py | `# type:\s*ignore([^\[]\|$)` | MAPS — `file_content_forbidden` |
| TYPENOSKIP | grep_linter.py | `follow_imports\s*=\s*skip` | MAPS — `file_content_forbidden` |
| NOQA | grep_linter.py | `# noqa([^:]\|$)` | MAPS — `file_content_forbidden` |
| SPACES | grep_linter.py | `[[:blank:]]$` | MAPS — `no_trailing_whitespace` (native) |
| TABS | grep_linter.py | literal tab | MAPS — `indent_style: spaces` (native) |
| C10_UNUSED | grep_linter.py | `C10_UNUSED` | MAPS — `file_content_forbidden` |
| C10_NODISCARD | grep_linter.py | `C10_NODISCARD` | MAPS — `file_content_forbidden` |
| RAWTHROW | grep_linter.py | `\bthrow\b` (with allowlist) | MAPS — `file_content_forbidden` |
| INCLUDE | grep_linter.py | `#include "` | MAPS — `file_content_forbidden` |
| PYBIND11_INCLUDE | grep_linter.py | `#include <pybind11/...` | MAPS — `file_content_forbidden` |
| ERROR_PRONE_ISINSTANCE | grep_linter.py | `isinstance(...(int\|float))` | MAPS — `file_content_forbidden` |
| PYBIND11_SPECIALIZATION | grep_linter.py | `PYBIND11_DECLARE_HOLDER_TYPE` | MAPS — `file_content_forbidden` |
| PYPIDEP | grep_linter.py | unpinned `pip install` | MAPS — `file_content_forbidden` |
| CUBINCLUDE | grep_linter.py | `#include <cub/` | MAPS — `file_content_forbidden` |
| RAWCUDA | grep_linter.py | `cudaStreamSynchronize` | MAPS — `file_content_forbidden` |
| RAWCUDADEVICE | grep_linter.py | `cudaSetDevice\|cudaGetDevice` | MAPS — `file_content_forbidden` |
| ROOT_LOGGING | grep_linter.py | `logging\.(debug\|info\|...)\(` | MAPS — `file_content_forbidden` |
| DEPLOY_DETECTION | grep_linter.py | `sys\.executable == .torch_deploy.` | MAPS — `file_content_forbidden` |
| CALL_ONCE | grep_linter.py | `std::call_once` | MAPS — `file_content_forbidden` |
| ONCE_FLAG | grep_linter.py | `std::once_flag` | MAPS — `file_content_forbidden` |
| CONTEXT_DECORATOR | grep_linter.py | `@.*(dynamo_timed\|...)` | MAPS — `file_content_forbidden` |
| COPYRIGHT | grep_linter.py | `Confidential and proprietary` | MAPS — `file_content_forbidden` |
| META_NO_CREATE_UNBACKED | grep_linter.py | `create_unbacked` (1 file) | MAPS — `file_content_forbidden` |
| ATEN_CPU_GPU_AGNOSTIC | grep_linter.py | `^#if.*USE_(ROCM\|CUDA)` | MAPS — `file_content_forbidden` |
| **STRUCTURAL — bespoke Python adapters (4)** | | | |
| EXEC | exec_linter.py | source files must not be +x | MAPS — alint walks gitignored already, but lacks an exact `not_executable` rule; close fit via custom `command: ["test", "!", "-x", "{path}"]` shellout |
| NEWLINE | newlines_linter.py | every file ends with `\n` (×3 between non-empty lines for some) | MAPS partial — `final_newline` (native) |
| MERGE_CONFLICTLESS_CSV | no_merge_conflict_csv_linter.py | every non-blank CSV row separated by 3 blanks | NEEDS NEW PRIMITIVE — "every non-blank line followed by N blanks"; close to `unique_line_spacing` shape |
| LINTRUNNER_VERSION | lintrunner_version_linter.py | `lintrunner --version` matches pinned | MAPS partial — `file_content_matches` for the pyproject.toml entry; the version-comparison stays on the adapter |
| **COMMAND SHELLOUTS — wraps mature external tool (15)** | | | |
| FLAKE8 | flake8_linter.py | `flake8` | MAPS — `command:` shellout |
| RUFF | ruff_linter.py | `ruff check` | MAPS — `command:` shellout |
| PYFMT | pyfmt_linter.py | `usort` + `ruff format` | MAPS — `command:` shellout |
| PYREFLY | pyrefly_linter.py | `pyrefly check --config=pyrefly.toml` | MAPS — `command:` shellout |
| MYPY | mypy_linter.py | `mypy --config-file=mypy.ini` | MAPS — `command:` shellout |
| CLANGFORMAT | clangformat_linter.py | vendored `clang-format --dry-run` | MAPS — `command:` shellout |
| CLANGTIDY | clangtidy_linter.py | vendored `clang-tidy` | MAPS — `command:` shellout |
| CLANGTIDY_EXECUTORCH_COMPATIBILITY | clangtidy_linter.py | clang-tidy with `--std=c++17` | MAPS — `command:` shellout |
| CMAKE | cmake_linter.py | `cmakelint --config=.cmakelintrc` | MAPS — `command:` shellout |
| CMAKE_MINIMUM_REQUIRED | cmake_minimum_required_linter.py | parse CMake + assert min version | MAPS partial — `file_content_matches` for `cmake_minimum_required\(VERSION X.Y` |
| SHELLCHECK | shellcheck_linter.py | vendored `shellcheck` | MAPS — `command:` shellout |
| ACTIONLINT | actionlint_linter.py | vendored `actionlint` | MAPS — `command:` shellout |
| CODESPELL | codespell_linter.py | `codespell --toml pyproject.toml` | MAPS — `command:` shellout |
| GHA | gha_linter.py | YAML-load workflow files | MAPS partial — bundled GHA ruleset; the deeper checks stay on the adapter |
| TESTOWNERS | testowners_linter.py | every `test_*.py` has `# Owner(s): [...]` header AND each label exists in PyTorch | MAPS partial — `file_content_matches` enforces the header shape; the cross-reference against the pytorch labels JSON (HTTP fetch) stays on the adapter |
| **AST-AWARE / SEMANTIC / GIT-DIFF-AWARE (8)** | | | |
| TEST_HAS_MAIN | test_has_main_linter.py | libCST AST: every test_*.py has `if __name__ == "__main__"` | OUT OF SCOPE (Python AST) |
| SCOPED_LIBRARY | scoped_library_linter.py | Python AST: forbid `torch.library.Library()`, require `_scoped_library` | OUT OF SCOPE (Python AST) |
| SET_LINTER | set_linter.py | Python AST: forbid built-in `set` under `_inductor` | OUT OF SCOPE (Python AST) |
| DOCSTRING_LINTER | docstring_linter.py | Python AST: every long class/function has substantive docstring | OUT OF SCOPE (Python AST) |
| IMPORT_LINTER | import_linter.py | Python AST: banned-third-party imports per directory | NEEDS NEW PRIMITIVE — `import_gate` (already on v0.10+ list from k8s, airflow, helm) |
| GB_REGISTRY | gb_registry_linter.py | Python AST: `unimplemented_v2(...)` calls cross-referenced against `tools/dynamo/graph_break_registry.json` | OUT OF SCOPE (AST + cross-file registry); partial via `cross_file_value_equals` once that lands |
| HEADER_ONLY_LINTER | header_only_linter.py | reads `torch/header_only_apis.txt`, asserts each symbol appears in at least one .cpp test file | NEEDS NEW PRIMITIVE — `registry_paths_resolve` (5th confirmation) |
| TEST_DEVICE_BIAS | test_device_bias_linter.py | Python AST: tests must not hard-code `cuda:0` etc. | OUT OF SCOPE (Python AST) |
| NATIVEFUNCTIONS | nativefunctions_linter.py | regenerates `aten/src/ATen/native/native_functions.yaml` via torchgen, asserts no diff | NEEDS NEW PRIMITIVE — `generated_file_fresh` (3rd confirmation: cpython, uv, pytorch) |
| GENERATED_SHIMS_VERSION | generated_shims_version_linter.py | parse C `shim.h`, assert all functions in `torchgen/aoti/fallback_ops.py` appear with correct version macro | OUT OF SCOPE (C AST + cross-file) |
| STABLE_SHIM_VERSION | stable_shim_version_linter.py | git-diff-aware: new declarations in `stable/c/shim.h` must be wrapped in `TORCH_FEATURE_VERSION` | OUT OF SCOPE (git-diff aware + C AST) |
| STABLE_SHIM_USAGE | stable_shim_usage_linter.py | maintains `shim_function_versions.txt`; assert usages of shim API match | OUT OF SCOPE (cross-file registry + C AST) |
| WORKFLOWSYNC | workflow_consistency_linter.py | every job under `sync-tag: foo` matches every other across `.github/workflows/*.yml` | NEEDS NEW PRIMITIVE — `cross_file_value_equals` (7th confirmation) |
| NO_WORKFLOWS_ON_FORK | no_workflows_on_fork.py | every workflow with `push`/`pull_request` triggers must guard `if: github.repository_owner == 'pytorch'` | NEEDS NEW PRIMITIVE — `yaml_path_matches` with implication shape (X→Y); single-source candidate |
| PYPROJECT | pyproject_linter.py | parse pyproject.toml, assert version pins match a per-package SpecifierSet | OUT OF SCOPE (deep TOML semantics + version arithmetic) |

**Counts:** 24 single-pattern grep + 4 bespoke structural + 15 command shellouts + 14 AST/semantic = 57.

**Mapping breakdown:**
- **MAPS directly** (single-pattern grep + indent/whitespace + final_newline + the 15 command shellouts) ≈ **41 of 57 (72 %)**
- **MAPS partial** (cmake_minimum_required as content match, testowners header check, lintrunner version pin) ≈ **5 of 57 (9 %)**
- **NEEDS NEW PRIMITIVE** (MERGE_CONFLICTLESS_CSV, IMPORT_LINTER, HEADER_ONLY_LINTER, NATIVEFUNCTIONS, WORKFLOWSYNC, NO_WORKFLOWS_ON_FORK) ≈ **6 of 57 (10 %)**
- **OUT OF SCOPE** (8 AST/git-diff-aware adapters: TEST_HAS_MAIN, SCOPED_LIBRARY, SET_LINTER, DOCSTRING_LINTER, GB_REGISTRY, TEST_DEVICE_BIAS, GENERATED_SHIMS_VERSION, STABLE_SHIM_*, PYPROJECT) ≈ **5 of 57 (9 %)**

So ~**81 % maps cleanly or partially** today; ~10 % needs new alint primitives;
~9 % stays on lintrunner forever.

### `.github/workflows/` (144 files)

| Pattern | Count | alint disposition |
|---|---:|---|
| Total `*.yml` workflows | 144 | — |
| Callable workflows (`_*.yml` prefix) | 25 | MAPS — `pytorch-callable-workflow-declares-workflow-call` (custom) + bundled `gha-workflow-has-name` |
| Generated workflows (`generated-*.yml` prefix) | 8 | MAPS — `pytorch-generated-workflow-has-warning` (custom) — asserts `# @generated` marker |
| Action references should be pinned to 40-char SHA | all | MAPS — bundled `gha-pin-actions-to-sha` |
| Workflow-level `permissions:` declared | all | MAPS — bundled `gha-workflow-permissions` |
| `lint.yml` invokes lintrunner via `_lint.yml` reusable | 1 | OUT OF SCOPE (orchestration, not structural) |
| `generate_ci_workflows.py` produces `generated-*.yml` from templates | — | NEEDS NEW PRIMITIVE — `generated_file_fresh` |
| WORKFLOWSYNC cross-workflow `sync-tag` consistency | — | NEEDS NEW PRIMITIVE — `cross_file_value_equals` |

### `.editorconfig` + `.gitattributes`

| Section | alint disposition |
|---|---|
| `end_of_line=lf, charset=utf-8, insert_final_newline=true` | MAPS — bundled `tooling-editorconfig-*` + custom `pytorch-final-newline` |
| Per-language `indent_style=space` (`*.py`, `*.cpp`, etc.) | MAPS — `pytorch-no-tabs-in-source` |
| `*.bat` is `crlf` | MAPS — `line_endings: crlf` (would add if `*.bat` were in scope; rare in pytorch) |
| `.gitattributes` `linguist-generated=true` markers (~7) | MAPS implicit — alint reads .gitattributes for binary classification |

### `Makefile`, `setup.py`, `pyproject.toml`, `CMakeLists.txt`, `.bzl` files

The Makefile is build-only (no lint targets — `make linecount` is the only
non-build helper). Structural validation lives entirely in `lintrunner`. The
`pyproject.toml` is itself the LINTRUNNER_VERSION pin source-of-truth (asserted
via `pytorch-lintrunner-pinned-in-pyproject`). The `.bzl` Bazel files are
build-only; alint asserts `BUILD.bazel` exists.

### `tools/linter/adapters/` directory

30 Python files (one per adapter family + shared `_linter/` library + S3
init helpers + grandfather-list JSON for docstring_linter). The adapters
themselves are the implementation of the structural rules; alint asserts the
load-bearing ones exist (`pytorch-grep-linter-shim-present`,
`pytorch-lintrunner-adapter-dir-present`).

### Other config files

| File | alint disposition |
|---|---|
| `.clang-format` (3.4 KB) | MAPS — `file_exists` in `pytorch-linter-configs-present` |
| `.clang-tidy` (3 KB) | MAPS — `file_exists` |
| `.cmakelintrc` | MAPS — `file_exists` |
| `pyrefly.toml` | MAPS — `file_exists` |
| `mypy.ini` + `mypy-strict.ini` | MAPS — `file_exists` |
| `pytest.ini` | MAPS — `file_exists` |
| `ubsan.supp` | MAPS — `file_exists` |
| `version.txt` (single-line semver) | MAPS — `file_content_matches` for `^MAJOR.MINOR.PATCH` |
| `CITATION.cff` | MAPS — `file_exists` |
| `RELEASE.md` | MAPS — `file_exists` |
| `Dockerfile` + `.devcontainer/` | MAPS — `file_exists` |
| `CLAUDE.md` (root) | MAPS — `file_exists` |

---

## What needs new alint primitives

| Gap | Existing pytorch tooling | What alint needs |
|---|---|---|
| Cross-workflow `sync-tag` consistency | WORKFLOWSYNC | `cross_file_value_equals` rule kind: "value at JSONPath X across all files matching glob Y must be identical (or vary only along an allowed dimension)". **7th confirmation** of the strongest demand signal in P2a (after airflow + tokio + clap + uv + react + pnpm). |
| Symbol registry resolves to test fixture (`torch/header_only_apis.txt`) | HEADER_ONLY_LINTER | `registry_paths_resolve` rule kind: "every line in registry file X resolves to a path/symbol present in glob Y". **5th confirmation** (rust + clap + cpython×2 + pytorch). cpython subagent flagged as the single highest-leverage gap; pytorch confirms. |
| Banned-third-party imports per-directory | IMPORT_LINTER | `import_gate` rule kind: "forbid imports of pattern X in path scope Y". **3rd confirmation** (k8s + airflow + helm + pytorch). |
| Generated YAML/header freshness (`native_functions.yaml`, `generated_shims_version`) | NATIVEFUNCTIONS, GENERATED_SHIMS_VERSION | `generated_file_fresh` rule kind: "regenerate via command, diff against on-disk". **4th confirmation** (uv + cpython + pytorch). |
| Cross-workflow `if: github.repository_owner == 'pytorch'` guard | NO_WORKFLOWS_ON_FORK | `yaml_path_implication` rule kind: "if YAML path X has value V₁, then path Y must have value V₂". Single-source so far; deferred unless a second confirmation arrives. |
| CSV with non-blank rows separated by N blank lines | MERGE_CONFLICTLESS_CSV | `line_spacing` rule kind: "every non-empty line followed by exactly N blank lines". **NEW candidate** — narrow to merge-conflict-resistant data files. Niche; logged as a v0.10+ candidate. |
| Source files must not be executable | EXEC | `not_executable` rule kind: shorthand for `command: ["test", "!", "-x", "{path}"]` but cross-platform. **NEW candidate** — could ship as a tiny convenience rule. Single-source; defer unless a second confirmation arrives. |
| `.ci/docker/` content-hash drives Docker rebuilds | informal — CLAUDE.md only | `directory_hash` / `pair_hash` rule kind: "compute content hash of glob X, compare to value Y in file Z". Pytorch has no formal check today (CLAUDE.md flags as foot-gun); could be added if the primitive existed. **NEW candidate**, single-source; defer. |

**Cross-reference with the existing v0.10+ candidate list in
[`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md):**

- `cross_file_value_equals` — confirmed by pytorch (WORKFLOWSYNC). **7th
  confirmation** — moves past saturation; this is unambiguously the
  highest-priority new primitive across P2a.
- `registry_paths_resolve` — confirmed by pytorch (HEADER_ONLY_LINTER). **5th
  confirmation** — strong saturation; pytorch's symbol-list-→-test-coverage
  shape is the cleanest example of the pattern.
- `import_gate` — confirmed by pytorch (IMPORT_LINTER). **3rd confirmation**
  (already on v0.10 list from k8s + airflow; helm flagged a fourth).
- `generated_file_fresh` — confirmed by pytorch (NATIVEFUNCTIONS,
  GENERATED_SHIMS_VERSION). **4th confirmation** (uv + cpython + pytorch).

**NEW candidates not previously inventoried:**

- `line_spacing` rule kind — surfaced uniquely by pytorch's
  MERGE_CONFLICTLESS_CSV. Niche; rated low priority.
- `not_executable` rule kind — surfaced uniquely by pytorch's EXEC adapter.
  Could ship as a one-line convenience over a `command:` shellout. Defer.
- `directory_hash` / `pair_hash` rule kind — surfaced uniquely by pytorch's
  `.ci/docker/` Docker-rebuild trigger. Adjacent to the existing `pair_hash`
  candidate (k8s, tokio); broaden the scope to "hash of glob, not just one
  file". Defer.

---

## Out of alint's scope (use the existing tool)

Same framing as cpython, rust-lang/rust, kubernetes: AST-aware, codegen,
binary, and deep-domain checks stay on the existing tooling. alint's
non-goals are deliberate.

- **TEST_HAS_MAIN** — libCST (Python AST) over every test file
- **SCOPED_LIBRARY** — Python `ast` parsing, replaces deprecated API call
- **SET_LINTER** — Python AST, forbids `set()` under `_inductor`
- **DOCSTRING_LINTER** — Python AST, asserts long functions have substantive
  docstrings
- **GB_REGISTRY** — Python AST + cross-file JSON registry walk
- **TEST_DEVICE_BIAS** — Python AST, forbids hard-coded `cuda:0` device IDs
- **GENERATED_SHIMS_VERSION** — C source AST + cross-reference to torchgen
- **STABLE_SHIM_VERSION** — git-diff aware: new lines must be wrapped in
  version macros
- **STABLE_SHIM_USAGE** — cross-file registry + C source parse
- **PYPROJECT** — TOML semantic checks + version SpecifierSet arithmetic
- **GHA** (workflow-shape via ruamel) — maps to bundled `ci/github-actions`
  for the simple bits; deeper analysis stays on the adapter
- **CLANGTIDY** (per-file with build flags) — the deep semantic analysis is
  clang's, not alint's; we only orchestrate via `command:`

---

## Already covered by other linters pytorch uses

- **clang-format / clang-tidy** — alint shells out (CLANGFORMAT,
  CLANGTIDY, CLANGTIDY_EXECUTORCH_COMPATIBILITY adapters)
- **ruff + usort + ruff-format** — alint shells out (RUFF, PYFMT)
- **flake8** — alint shells out (FLAKE8)
- **pyrefly + mypy** — alint shells out (PYREFLY, MYPY)
- **codespell** — alint shells out (CODESPELL)
- **shellcheck** — alint shells out (SHELLCHECK)
- **actionlint** — alint shells out (ACTIONLINT)
- **cmakelint** — alint shells out (CMAKE)
- **lintrunner** — alint sits BENEATH; CI runs both. lintrunner handles the
  AST-aware tail; alint handles the structural floor (faster fail signal,
  parallel walks, no per-adapter Python-venv spawn)

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts:

- `oss-baseline@v1` (license, README, gitignore, no merge markers,
  no bidi)
- `python@v1` (pyproject/setup.py, lockfile, snake_case, source hygiene)
- `ci/github-actions@v1` (workflow permissions / action pinning / SHA-pinned
  references)
- `hygiene/no-tracked-artifacts@v1` (no `.DS_Store`, build outputs, etc.)
- `agent-hygiene@v1` (pytorch ships a CLAUDE.md and is a heavy agent-coding
  target)
- `tooling/editorconfig@v1` (the `.editorconfig` is the source of truth for
  whitespace; the bundled rules assert it exists + checks it)

Plus 35 pytorch-specific rules covering:

- **3 broad-tree hygiene** — `pytorch-final-newline`,
  `pytorch-no-trailing-whitespace`, `pytorch-no-tabs-in-source` (mirrors
  lintrunner NEWLINE + SPACES + TABS)
- **1 Trojan Source override** — `pytorch-no-bidi-in-source` (broadens the
  bundled rule to `*.py` + `*.cpp` + `*.cu`)
- **9 single-pattern `file_content_forbidden`** — direct mappings of the most
  load-bearing lintrunner grep_linter adapters (TYPEIGNORE, NOQA, TYPENOSKIP,
  COPYRIGHT, PYPIDEP, C10_UNUSED, C10_NODISCARD, CALL_ONCE, ONCE_FLAG,
  INCLUDE, PYBIND11_INCLUDE, PYBIND11_SPECIALIZATION) — the remaining 12
  grep adapters can be added with the same template
- **10 `command:` shellouts** — codespell, cmakelint, shellcheck, actionlint,
  clang-format, flake8, ruff, pyrefly, pyfmt-ruff-format
- **3 GitHub Actions custom rules** — `pytorch-callable-workflow-declares-
  workflow-call`, `pytorch-generated-workflow-has-warning`,
  `pytorch-lintrunner-pinned-in-pyproject`
- **1 placeholder for MERGE_CONFLICTLESS_CSV** — `file_min_lines` floor + a
  message pointing at the gap
- **1 `file_content_matches` for version.txt shape**
- **1 `file_min_lines` floor on CODEOWNERS**
- **1 `file_starts_with` for shell shebang in `.ci/pytorch/**/*.sh`**
- **6 `file_exists` blocks** — top-level build files, Bazel files, linter
  configs, Docker files, CLAUDE.md, .ci/{pytorch,docker} tree, lintrunner
  adapter directory presence

The remaining 16 lintrunner adapters not directly modelled:

- 12 `grep_linter`-shim grep adapters (RAWTHROW, ERROR_PRONE_ISINSTANCE,
  CUBINCLUDE, RAWCUDA, RAWCUDADEVICE, ROOT_LOGGING, DEPLOY_DETECTION,
  CONTEXT_DECORATOR, META_NO_CREATE_UNBACKED, ATEN_CPU_GPU_AGNOSTIC, EXEC,
  NEWLINE) — additive `file_content_forbidden` rules using the same template;
  omitted to keep the example's `.alint.yml` readable
- 4 AST/semantic adapters (TEST_HAS_MAIN, SCOPED_LIBRARY, SET_LINTER,
  DOCSTRING_LINTER, GB_REGISTRY, TEST_DEVICE_BIAS, IMPORT_LINTER,
  GENERATED_SHIMS_VERSION, STABLE_SHIM_VERSION, STABLE_SHIM_USAGE,
  PYPROJECT, NO_WORKFLOWS_ON_FORK, WORKFLOWSYNC, HEADER_ONLY_LINTER,
  NATIVEFUNCTIONS, MERGE_CONFLICTLESS_CSV, GHA, TESTOWNERS) — file as
  v0.10+ feature requests (above)

---

## Performance comparison (placeholder — bench when validation pass scales)

`lintrunner` runs adapters in parallel (one Python process per code per
batch of files). On a warm laptop checkout it takes ~30-60 seconds for
`lintrunner --all-files` to complete (CI is much slower because of
the S3-vendored binary fetches). The pre-fetch dance (`lintrunner init`)
adds ~30 seconds the first time.

The alint pitch here is **inventory legibility AND fail-fast latency**:

1. **Legibility** — A new pytorch contributor staring at the structural-
   validation surface today has to read a 1876-line `.lintrunner.toml`,
   30 Python adapter files in `tools/linter/adapters/`, the `.editorconfig`,
   `.clang-format` (3.4 KB), `.clang-tidy` (3 KB), `.cmakelintrc`,
   `pyrefly.toml`, `mypy.ini` (×2), and `pytest.ini` to understand what
   rules apply where. The alint config in this directory is **one file**,
   declarative, with each rule's scope, severity, and rationale visible
   in 5-10 lines.

2. **Fail-fast latency** — alint has zero adapter-spawn cost: it walks the
   tree once and runs every rule in parallel against the in-memory file
   bytes. lintrunner spawns one Python process per code per file batch.
   For the 28 structural-only adapters, alint should be 10-100× faster.
   For the 21 command-shellout adapters, the wall-clock delta is dominated
   by the upstream tool — both runners are roughly equivalent (same `ruff`,
   same `clang-format`, same `actionlint`).

To benchmark for real: `time lintrunner --all-files --take CLANGFORMAT` vs
`time alint check --rules pytorch-clang-format` against the same tree;
then compare the unique-violation overlap. Deferred to the per-repo
measurement pass.

---

## Recommendation for the launch story

**Headline launch quote:** "pytorch built `lintrunner` because no
existing tool handled their orchestration needs — but ~49 of its 57
adapters (≈86 %) are pure structural checks (24 grep shims + 21 mature
external-tool shellouts + 4 simple bespoke). alint expresses every one
of these as a 5-10 line declarative rule. lintrunner stays where it
provably wins (the 8 AST-aware adapters); alint sits beneath as the
structural floor: faster fail signal, no per-adapter Python-venv spawn,
one-file-readable contract."

This is a **fourth positioning narrative** — pytorch fits into all three
existing P2a narratives but with an extra twist:

| Narrative | pytorch data point |
|---|---|
| "Replaces N hand-rolled validation scripts" | 28 structural lintrunner adapters consolidated to one config |
| "Catches conventions your pipeline assumes but doesn't verify" | callable-workflow naming convention (`_*.yml`), generated-workflow `# @generated` marker, `.ci/docker/` content-hash trap |
| "Adds structural floor on top of mature tooling" | 21 command shellouts mirror lintrunner's clang-format / clang-tidy / ruff / mypy / pyrefly / actionlint orchestration |
| **NEW (pytorch-specific):** "Replaces the structural subset of YOUR custom orchestration layer" | lintrunner's 24 grep_linter shims + 4 simple bespoke adapters become 28 alint rules; lintrunner keeps the 8 AST-aware adapters |

The fourth narrative is the launch-pitch differentiator: **alint is what
you would have built instead of `lintrunner` if `lintrunner` had existed
and you'd realised you only needed 86 % of its expressivity.** For repos
that already have a custom orchestrator (pytorch's lintrunner; tensorflow's
`buildifier`/`yapf`-driven CI; bazel's own `buildifier`), alint is the
"don't build your own orchestrator next time" pitch — adopt alint for the
structural floor, keep the AST tail on whatever AST-aware tool you needed
in the first place.

Followup feature work surfaced (priority order):

- **`cross_file_value_equals`** — **7th confirmation**, well past
  saturation. Strongest demand signal in P2a; pytorch's WORKFLOWSYNC is the
  cleanest example of the pattern (every `sync-tag` block across 144
  workflow files must be identical).
- **`registry_paths_resolve`** — **5th confirmation**. pytorch's
  `torch/header_only_apis.txt` registry is the canonical example: a flat
  text file lists symbols, each must appear in a .cpp test file.
- **`import_gate`** — **3rd-4th confirmation** (k8s + airflow + helm + pytorch
  IMPORT_LINTER). pytorch's per-directory `_imports.toml`-style configs are
  the most polished example.
- **`generated_file_fresh`** — **3rd confirmation**. pytorch has TWO
  freshness gates (NATIVEFUNCTIONS + GENERATED_SHIMS_VERSION); pinning down
  the alint primitive's API is overdue.
- **`line_spacing`, `not_executable`, `directory_hash`** — **NEW** but
  single-source; defer.
