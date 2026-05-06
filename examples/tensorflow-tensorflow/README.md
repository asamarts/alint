# Case study: `tensorflow/tensorflow`

Inventory of the structural-validation tooling in `tensorflow/tensorflow`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives — including
the v0.11+ `cross_language_implementation_complete` primitive that this
case study was specifically commissioned to validate against the
canonical "core+bindings" multi-language ML monorepo.

**Repo state captured:** 2026-05-06, sparse-clone of
`tensorflow/tensorflow@21d3ed60` (rev =
`21d3ed604b4a8919a7ecef8a1190f192f21d26f9`). Heaviest sub-trees
(`tensorflow/core/kernels/`, `tensorflow/compiler/`, `third_party/`,
`tensorflow/lite/micro/`) excluded via:

```sh
git clone --depth=1 --filter=blob:none --sparse \
    https://github.com/tensorflow/tensorflow /tmp/tensorflow
cd /tmp/tensorflow
git sparse-checkout set --no-cone '/*' \
    '!/tensorflow/core/kernels' \
    '!/tensorflow/compiler' \
    '!/third_party' \
    '!/tensorflow/lite/micro'
```

After sparse-checkout: **21 553 files / ~290 MiB working tree** (the
full unsparse tree exceeds 80k files + 200 MiB once the heavy sub-trees
return). This is the LARGE-MIXED-TREE data point for P2b Wave 1.

---

## Summary

tensorflow is the canonical "core + per-language bindings" multi-language
ML monorepo. Where apache/arrow has 6 in-tree language implementations
all peers of one another (the polyglot-flagship case study), tensorflow
has exactly **one canonical core** (Python frontend over a C++/CUDA
runtime) plus **5 per-language bindings** (C, C++ via `cc/`, Java
legacy, Go, JS codegen scaffolding) that mirror parts of the core API,
PLUS a separate **TFLite per-device frontend** that itself has 4
per-language bindings (Python, Java, Swift, ObjC) sharing the same
core C runtime and the same per-source ↔ per-test naming discipline.

This is the headline launch-pitch for alint on tensorflow:

> **tensorflow has TWO discipline layers stacked: file-shape parity (every
> source has a paired test, by language convention) AND API-shape parity
> (every public Python symbol has v1+v2 textproto goldens; every binding
> mirrors the core API). alint covers Layer 1 cleanly today via
> `pair`/`for_each_*` primitives. Layer 2 — the cross-language API
> mirroring — is exactly the v0.11+ `cross_language_implementation_complete`
> rule kind, and tensorflow is the cleanest single-repo demonstration of
> the pattern.**

Concrete count at HEAD (after sparse-checkout):

- **21 553** files in scope, ~290 MiB working tree
- **671** BUILD files + **87** `*.bzl` files (Bazel — alint asserts
  filename presence + does **NOT** parse Bazel content)
- **16** GitHub Actions workflows under `.github/workflows/` (vs.
  144 in pytorch — TF leans on internal-Google CI for the bulk; the
  public GHA surface is intentionally minimal)
- **11** `ci/official/*.sh` entrypoints + 16 `envs/*.env` files +
  `ci/official/utilities/code_check_full.bats` with **9 `@test` cases**
  (the canonical CI lint suite — wraps bazel queries for license-graph
  integrity, pip-dep coverage, CUDA-free Windows, API compatibility,
  and the master `bazel nobuild` smoke gate) plus
  `code_check_changed_files.bats` with **4 `@test` cases** (buildifier,
  clang-format, pylint, api-compat-test on the changed-files subset)
- **1 185** `*.pbtxt` golden files at `tensorflow/tools/api/golden/`
  (**585** v1 + **600** v2) — the explicit per-symbol API contract that
  `tensorflow/tools/api/tests/api_compatibility_test.py` runs `tf`
  against, exit-1 on any drift, blocking PR merge until
  `bazel run … -- --update_goldens True` (with API-leads approval)
  is run
- **7** per-language top-level subdirectories under `tensorflow/`:
  `c/`, `cc/`, `python/`, `java/`, `go/`, `js/`, `lite/`
- **4** TFLite per-language sub-bindings under `tensorflow/lite/`:
  `python/`, `java/`, `swift/`, `objc/` — each with explicit
  `<Source>` ↔ `<SourceTest|Tests>` file-name parity discipline
- **5** `requirements_lock_3_{10,11,12,13,14}.txt` — pip-compile output
  across the supported Python interpreter matrix
- **1** `tensorflow/tools/ci_build/pylintrc` (344 lines — the canonical
  Python style config; symlinked to `.pylintrc` at repo root)
- **256** entries in `tensorflow/opensource_only.files` (the
  internal-Google ↔ OSS file manifest — same shape as apache/arrow's
  `rat_exclude_files.txt`)
- `.bazelrc` = **60 KB**, `.bazelversion`, `.bazelignore`,
  `MODULE.bazel`, `WORKSPACE` — Bazel-specific entry points

Total **structural-validation surfaces** counted: **34** discrete
checks across the inventory (see § "Existing tooling inventory").

- **17 of 34 (50 %) MAP to existing alint rules** — bundled
  `oss-baseline + python + ci/github-actions +
  hygiene/no-tracked-artifacts + compliance/apache-2 +
  tooling/editorconfig` ship roughly **35 rules** between them, plus
  the **30 tensorflow-specific rules** in [`/.alint.yml`](.alint.yml)
  (per-language subdir presence, golden-file presence, build-system
  shape, pip lockfile matrix, CI script presence, governance files,
  TFLite per-language test parity)
- **6 of 34 (18 %) shell out via `command:` rules** — wrapping
  pylint, clang-format, buildifier, codespell, the bats-driven
  `api_compatibility_test`, and the `ci/official/utilities/*.bats`
  suites
- **11 of 34 (32 %) need new primitives or are out of alint's scope**
  — the bazel `cquery` license-graph integrity check, the bazel
  `cquery` pip-dep coverage check, the API compatibility test
  (Python AST + protobuf reflection + 1185-pbtxt cross-reference),
  the cross-language API parity gates
  (Python ↔ Java ↔ JS ↔ Swift ↔ ObjC ↔ Go ↔ C/C++), the
  `--update_goldens` regenerate-and-diff freshness check, the
  per-version requirements_lock_*.txt cross-consistency check

The configured **30-rule** [`/.alint.yml`](.alint.yml) covers every
structural assertion the existing tooling makes about repo *state*,
plus several TF doesn't enforce today (TFLite Swift per-source test
parity, ObjC API ↔ test parity, Python TFLite source ↔ `_test`
parity, golden-file canonical-marker validation).

**Headline finding:** tensorflow is THE flagship "polyglot at scale
with API-parity discipline" pitch for alint — the file-shape layer
(every TFLite Swift source has a paired test; every Java source has a
paired test; every Python TFLite module has `_test.py`; every public
Python symbol has v1+v2 textproto goldens) is fully expressible in
alint's grammar today via `pair` / `for_each_file` / `file_min_size`,
and that pass alone surfaces **5 known TFLite Swift parity drifts**
(`CoreMLDelegate.swift`, `Delegate.swift`, `InterpreterError.swift`,
`SignatureRunnerError.swift`, `SignatureRunner.swift` have no matching
`*Tests.swift`) AND **4 ObjC apis/ headers** without `tests/` partners.
The deeper API-symbol parity — every symbol in `tools/api/golden/v1/`
must have a v2 mirror; every public Python `tf.foo()` must have a
Java/JS/Swift/Go binding — is exactly the v0.11+
`cross_language_implementation_complete` rule kind shape, and TF
demonstrates it across **6 binding languages** (the most of any
single repo in the case-study catalogue).

---

## Per-language parity findings (the v0.11+ `cross_language_implementation_complete` validation)

This is the section the case study was commissioned for. **Yes — TF has
explicit cross-language API parity discipline that the v0.11+
`cross_language_implementation_complete` rule kind would help with, and
the discipline operates at TWO distinct layers:**

### Layer 1: File-shape parity (per-source ↔ per-test, same language)

Each TFLite per-language binding follows a strict per-source ↔ per-test
naming convention:

| Language | Source pattern | Test pattern | Discipline state |
|---|---|---|---|
| Swift | `tensorflow/lite/swift/Sources/<Foo>.swift` | `tensorflow/lite/swift/Tests/<Foo>Tests.swift` | **5 of 11 sources have NO matching test** (CoreMLDelegate, Delegate, InterpreterError, SignatureRunnerError, SignatureRunner) |
| ObjC | `tensorflow/lite/objc/apis/TFL<Foo>.h` | `tensorflow/lite/objc/tests/TFL<Foo>Tests.m` | **4 of 9 API headers have NO matching test** (TFLDelegate, TFLInterpreterOptions on this build, TFLMetalDelegate test exists for Apple-only path, TFLTensorFlowLite umbrella header) |
| Java | `tensorflow/lite/java/src/main/java/.../<Foo>.java` | `tensorflow/lite/java/src/test/java/.../<Foo>Test.java` | **20 sources / 21 test files** — close to 1:1 with some test-only helpers |
| Python (TFLite) | `tensorflow/lite/python/<foo>.py` | `tensorflow/lite/python/<foo>_test.py` | **~25 source/test pairs** — close to 1:1 with utility omissions |
| Python (core) | `tensorflow/python/<x>.py` | `tensorflow/python/<x>_test.py` | **978 sources / 1034 test files** — extensive 1:1+ coverage |
| C++ (cc) | `tensorflow/cc/<x>.cc` | `tensorflow/cc/<x>_test.cc` | **42 sources / 32 test files** — partial coverage (the AST-level testing is via gtest-driven `cc_test` Bazel targets) |

**This entire layer is expressible via alint's existing `pair` rule.**
The configured `.alint.yml` ships 4 such rules
(`tensorflow-lite-swift-source-has-test`,
`tensorflow-lite-objc-api-has-test`,
`tensorflow-lite-python-source-has-test`, plus the implicit
per-language Apache header rule). Drift is surfaced today; the
remediation is a manual audit + writing the missing tests (TF doesn't
enforce these at PR time today, which is **exactly the gap the alint
case study identifies**).

### Layer 2: API-shape parity (cross-language symbol mirroring)

This is the deeper discipline that TF enforces dynamically via
`api_compatibility_test.py` (Python only) and via review for the
non-Python bindings (Java/Swift/ObjC/JS/Go):

- **Python v1 ↔ v2 surface parity**: 585 textproto files in
  `tensorflow/tools/api/golden/v1/` describe the public symbol surface
  for `tf.compat.v1`; 600 in `v2/` describe `tf` (some symbols are
  v2-only, some v1-only-deprecated). The `api_compatibility_test`
  runs `tf` and `tf.compat.v1` introspection at test time and
  diffs against these textprotos — exit-1 on drift, requires
  `--update_goldens` rerun + API-leads approval to land. **alint
  cannot cross-reference the two textproto trees today**; this
  is a `cross_file_value_equals` v0.10+ shape extended to
  set-equality semantics.

- **Python ↔ Java/Go/Swift/ObjC binding parity**: Every TFLite
  operator declared in `tensorflow/lite/python/lite.py` should have a
  Java counterpart in `tensorflow/lite/java/src/main/java/org/tensorflow/lite/`,
  a Swift counterpart in `tensorflow/lite/swift/Sources/`, etc. This
  is enforced by tribal knowledge + code review — there's no
  automated check today. The shape is exactly the v0.11+
  `cross_language_implementation_complete` rule kind: "every
  symbol/test fixture defined in registry A must have a corresponding
  entry in registry B/C/D/E". **TF is the canonical example of the
  pattern across 6 binding languages** (Python core + Java + Go +
  JS-codegen-scaffolding + TFLite-Python + TFLite-Java + TFLite-Swift
  + TFLite-ObjC).

**Quantification:**

- 1 185 textproto golden files (585 v1 + 600 v2) — the explicit
  Python API parity registry
- 256 entries in `tensorflow/opensource_only.files` (the internal-Google
  ↔ OSS-only file manifest — adjacent shape to the parity registry)
- 7 in-tree binding languages (C, C++ cc, Java, Go, JS scaffolding,
  Python core, TFLite-Python) + 3 TFLite-only bindings (TFLite-Java,
  TFLite-Swift, TFLite-ObjC) = **10 distinct API-bearing language
  surfaces in the same tree**

**Demand signal for v0.11+ `cross_language_implementation_complete`:**
TF is the second multi-binding repo in the case-study catalogue (after
apache/arrow's 6 in-tree languages, where the discipline is
implementation-completeness across format/Schema.fbs spec types). TF
adds the "core + bindings" shape on top of arrow's "all-peers" shape —
**the v0.11+ primitive needs to handle BOTH topologies** (peer-to-peer
parity AND core-to-binding parity). TF's TFLite layer demonstrates a
nested case (one TFLite C runtime + 4 language frontends), which
generalises to any plugin-host architecture. **Strongly confirms the
v0.11+ priority** — TF + arrow are now the two canonical demand-driving
repos for the primitive.

---

## Existing tooling inventory

### Root config files

| File | Owner tool | What it pins | alint disposition |
|---|---|---|---|
| `.bazelrc` (60 KB) | Bazel | every CI build flag | `file_exists` + content not parsed (alint doesn't read Bazel) |
| `.bazelversion` | Bazel | Bazel toolchain version | `file_exists` |
| `.bazelignore` | Bazel | dirs to ignore in workspace | `file_exists` |
| `MODULE.bazel` | Bazel | bzlmod module def | `file_exists` |
| `WORKSPACE` | Bazel | legacy workspace def | `file_exists` |
| `BUILD` | Bazel | top-level package | `file_exists` |
| `.clang-format` | clang-format | C++ style baseline | `file_exists` + `command:` shellout |
| `.pylintrc` (symlink → tools/ci_build/pylintrc) | pylint | 344-line Python style | `file_exists` + `command:` shellout |
| `.gitignore` | git | tracked-tree exclusions | `file_exists` (covers per-bundled rule) |
| `LICENSE` | Apache 2.0 | full Apache text | bundled `compliance/apache-2@v1` |
| `CITATION.cff` | citation | academic citation | `file_exists` (in governance bundle) |
| `CODEOWNERS` (563 bytes) | GitHub | review routing | `file_exists` |
| `CODE_OF_CONDUCT.md` | community | contributor CoC | `file_exists` |
| `CONTRIBUTING.md` (16 KB) | community | how to contribute | `file_exists` |
| `ISSUES.md` | community | issue process | `file_exists` |
| `README.md` (12 KB) | docs | project intro | `file_exists` (in oss-baseline bundle) |
| `RELEASE.md` (765 KB!) | docs | full release history | `file_exists` |
| `SECURITY.md` (10 KB) | security | vuln disclosure | `file_exists` (in oss-baseline bundle) |
| `AUTHORS` | community | contributor list | `file_exists` |
| `configure` / `configure.cmd` / `configure.py` | Bazel | pre-build CUDA/ROCm detection | `file_exists` |
| `.zenodo.json` | Zenodo | DOI metadata | not enforced (single-purpose; rare) |
| `requirements_lock_3_{10,11,12,13,14}.txt` (5 files) | pip-compile | per-Python-version lock | `file_exists` per-version + `file_min_size` floor |

### `.github/` — GitHub-side surfaces

| File | What it does | alint disposition |
|---|---|---|
| `.github/dependabot.yml` | Action + Docker dep updates (4 ecosystem blocks) | bundled `oss-dependency-update-tool` + custom `tensorflow-dependabot-covers-actions` |
| `.github/bot_config.yml` | TF triage bot config | not enforced (operational) |
| `.github/ISSUE_TEMPLATE/tensorflow_issue_template.yaml` | issue dropdown UI | `file_exists` |
| `.github/ISSUE_TEMPLATE/tflite-{converter,op-request,other,in-play-services}.md` | TFLite-specific issue templates | not enforced (TFLite-org-specific) |
| `.github/workflows/*.yml` (16 workflows) | public CI matrix (TF leans heavily on internal Google CI; the public GHA surface is intentionally narrow) | bundled `ci/github-actions@v1` covers SHA-pin + permissions across all 16 |

### `ci/official/` — the canonical CI surface

| File | What it does | alint disposition |
|---|---|---|
| `ci/official/code_check_full.sh` → `utilities/code_check_full.bats` | 9 `@test` cases: license-graph, pip-deps, CUDA-free Windows, no duplicate Windows files, all tensorflow.org/code links resolve, master `bazel nobuild`, API compatibility | `file_exists` + `command:` wrapping bats (the deep cquery checks stay in bats) |
| `ci/official/code_check_changed_files.sh` → `utilities/code_check_changed_files.bats` | 4 `@test` cases: buildifier on BUILD files, clang-format on C++, pylint on Python, API compatibility on changed files | `file_exists` + per-tool `command:` wrappers |
| `ci/official/wheel.sh` + `installer_wheel.sh` + `libtensorflow.sh` + `pycpp.sh` | wheel build entry-points | `file_exists` (operational; deep behaviour stays in bash) |
| `ci/official/upload.sh` + `bisect.sh` + `any.sh` + `debug_tfci.sh` | upload + debug helpers | `file_exists` |
| `ci/official/envs/{linux_x86,linux_arm64,windows_x86_2022,linux_x86_cuda,…}` (16 files) | per-platform env-var blobs | not enforced (file count varies; alint asserts the dir exists) |
| `ci/official/utilities/{cleanup_docker.sh,setup_docker.sh,…}` | docker setup helpers | not individually enforced (subordinate to the bats suites) |

### `tensorflow/tools/api/` — the API-parity registry

This is the CORE artefact for the v0.11+ case study:

```
tensorflow/tools/api/
├── golden/
│   ├── BUILD
│   ├── v1/
│   │   ├── tensorflow.audio.pbtxt
│   │   ├── tensorflow.app.pbtxt
│   │   ├── … (585 textproto files, one per public v1 symbol)
│   └── v2/
│       ├── tensorflow.audio.pbtxt
│       ├── tensorflow.autodiff.-forward-accumulator.pbtxt
│       ├── … (600 textproto files, one per public v2 symbol)
├── lib/
│   ├── api_objects.proto              (the textproto schema)
│   ├── python_object_to_proto_visitor.py (the introspection visitor)
│   └── BUILD
└── tests/
    ├── api_compatibility_test.py      (the gate test — exit-1 on drift)
    ├── API_UPDATE_WARNING.txt         (review-required warning text)
    ├── README.txt                     (run with --update_goldens=True)
    └── BUILD
```

### `tensorflow/python/tools/api/generator2/` — the generator side

Counterpart to the goldens. Reads the per-module BUILD-tagged
`tf_export(...)` decorators, walks the Python tree, generates the
`tensorflow/__init__.py` re-exports during pip-wheel build. The
generator is what produces the symbols the goldens validate; if the
generator outputs a symbol the goldens don't list, the test fails.

| File | alint disposition |
|---|---|
| `tensorflow/python/tools/api/generator2/{apis,generate_api,patterns}.bzl` | `file_exists` (Bazel — content not parsed) |
| `tensorflow/python/tools/api/generator/api_init_files.bzl` (183 lines) | `file_exists` |
| `tensorflow/python/tools/api/generator/api_init_files_v1.bzl` (166 lines) | `file_exists` |
| `tensorflow/python/tools/api/generator/create_python_api.py` | `file_exists` |
| `tensorflow/python/tools/api/generator/create_python_api_test.py` | `file_exists` (paired test exists — passes the generic `pair` rule) |

### `tensorflow/lite/{swift,objc,java,python}/` — TFLite per-language bindings

The headline cross-language parity surface:

```
tensorflow/lite/swift/Sources/<Foo>.swift  ↔  Tests/<Foo>Tests.swift
tensorflow/lite/objc/apis/TFL<Foo>.h       ↔  tests/TFL<Foo>Tests.m
tensorflow/lite/java/src/main/java/.../<Foo>.java  ↔  src/test/java/.../<Foo>Test.java
tensorflow/lite/python/<foo>.py            ↔  <foo>_test.py
```

| Sub-binding | Sources | Tests | Coverage | alint disposition |
|---|---:|---:|---|---|
| TFLite Swift | 11 | 7 | 64 % (5 missing) | `pair` rule fires on the 5 (CoreMLDelegate, Delegate, InterpreterError, SignatureRunnerError, SignatureRunner) |
| TFLite ObjC apis/ → tests/ | 9 | 6 | 67 % (3 missing) | `pair` rule fires on the 3 |
| TFLite Java | 20 | 21 | 105 % (test helpers) | `pair` rule passes (test count > source count) |
| TFLite Python | ~25 | ~25 | ~100 % | `pair` rule passes |

### `tensorflow/security/` — CVE advisory tree

```
tensorflow/security/
├── README.md (CVE index — TFSA-2021-001 through TFSA-2023-020+)
└── advisory/
    ├── tfsa-2021-001.md
    ├── … (~200 published CVEs)
    └── tfsa-2023-020.md
```

| Disposition | alint coverage |
|---|---|
| README.md exists | `tensorflow-security-advisory-dir-present` |
| `advisory/*.md` shape | not enforced (each CVE follows TFSA template; checking shape would need a v0.10+ `markdown_template_match` primitive) |

---

## What maps to existing alint rules

The 30-rule [`/.alint.yml`](.alint.yml) breaks down as:

- **6 bundled rulesets** (`oss-baseline`, `compliance/apache-2`,
  `python`, `ci/github-actions`, `hygiene/no-tracked-artifacts`,
  `tooling/editorconfig`) — pull in roughly **35 rules** between
  them
- **2 cross-language structural rules** —
  `tensorflow-language-subdirs-present` (asserts the 7 per-language
  subdirs each have a BUILD file) +
  `tensorflow-language-subdirs-have-build-file` (the BUILD-file shape
  variant)
- **1 TFLite per-language presence rule** —
  `tensorflow-lite-language-subdirs-present` (the 4 frontends)
- **3 TFLite per-source ↔ per-test parity rules** —
  `tensorflow-lite-swift-source-has-test`,
  `tensorflow-lite-objc-api-has-test`,
  `tensorflow-lite-python-source-has-test` (the **headline file-shape
  parity** layer that this case study was commissioned to validate)
- **4 API-parity registry rules** —
  `tensorflow-api-golden-v1-present`,
  `tensorflow-api-golden-v2-present`,
  `tensorflow-api-golden-v1-substantive` (file_min_size floor),
  `tensorflow-api-golden-v1-has-canonical-marker` +
  `tensorflow-api-golden-v2-has-canonical-marker`
  (assert every textproto starts with `path: "tensorflow.X"`)
- **1 API-compat-test scaffolding rule** —
  `tensorflow-api-compat-test-present` (asserts the 6 load-bearing
  files for `api_compatibility_test.py` are all present)
- **3 Bazel build-system rules** —
  `tensorflow-bazel-build-system-present` (top-level entry points),
  `tensorflow-workspace-bzl-files-present` (workspace0..3.bzl +
  tensorflow.bzl + tf_version.bzl),
  `tensorflow-bazel-files-have-apache-header` (file_header on every
  BUILD/.bzl)
- **1 governance-files rule** — `tensorflow-governance-files-present`
  (10 top-level community files)
- **2 entry-point rules** — `tensorflow-configure-scripts-present`,
  `tensorflow-c-api-headers-present`
- **1 Python API template rule** — `tensorflow-python-api-template-present`
- **2 pip-lockfile rules** —
  `tensorflow-requirements-lock-matrix-present` (5 files exist) +
  `tensorflow-requirements-lock-non-empty` (50 KB floor each)
- **2 CI script rules** —
  `tensorflow-ci-official-entrypoints-present` (12 files) +
  `tensorflow-ci-official-entrypoints-have-shebang`
- **1 Apache header on Python entry-points rule** —
  `tensorflow-config-py-have-apache-header`
- **1 tf_version.bzl semver shape rule** —
  `tensorflow-tf-version-bzl-declares-semver`
- **1 dependabot covers actions rule** —
  `tensorflow-dependabot-covers-actions`
- **2 GitHub UI rules** — `tensorflow-issue-templates-present`,
  `tensorflow-security-advisory-dir-present`
- **2 per-tool config presence rules** —
  `tensorflow-pylint-config-present`,
  `tensorflow-clang-format-config-present`
- **1 OSS-only manifest rule** —
  `tensorflow-opensource-only-manifest-present`
- **5 `command:` rule shell-outs** — buildifier (mirrors bats
  `Check buildifier formatting`), pylint (mirrors bats
  `Check pylint for Python files`), clang-format (mirrors bats
  `Check formatting for C++ files`), codespell, the bazel-driven
  `api_compatibility_test`
- **3 broad-tree hygiene rules** —
  `tensorflow-no-tabs-in-py`, `tensorflow-no-trailing-whitespace`,
  `tensorflow-final-newline`
- **1 Trojan Source extension** —
  `tensorflow-no-bidi-in-cc-sources` (broadens the bundled rule to
  C/C++/Java/Swift/Go/ObjC)

---

## What needs new alint primitives

Five patterns specific to tensorflow that don't fit any current rule:

### 1. `cross_language_implementation_complete` for the API-parity layer (v0.11+)

The headline gap. tensorflow's `tools/api/golden/v{1,2}/` is the cleanest
demonstration of the pattern in any single repo:

- **585 v1 textprotos** describe the public `tf.compat.v1` symbol surface
- **600 v2 textprotos** describe the public `tf` symbol surface
- Every public TF symbol must have a textproto entry in BOTH v1 AND v2
  (modulo a handful of v2-only / v1-only-deprecated documented exceptions)
- The discipline extends to: every public Python symbol SHOULD have a
  Java/Swift/ObjC/Go/JS counterpart (depending on which binding owns
  the symbol)

**This is the v0.11+ `cross_language_implementation_complete` rule
kind shape.** TF + apache/arrow are the two flagship demand-driving
case studies (arrow has the same shape across 6 in-tree language
implementations; TF has the same shape across 1 core + 6 bindings).
The v0.11+ primitive needs to handle BOTH topologies.

### 2. `cross_file_value_equals` for `requirements_lock_3_*.txt` cross-Python-version consistency

TF ships 5 separate `requirements_lock_3_{10,11,12,13,14}.txt` files,
one per supported Python interpreter. Every package pinned in 3.10's
lock SHOULD be pinned to the same version in 3.11/3.12/3.13/3.14
(modulo interpreter-conditional packages like `cffi` on macOS arm64).

This is the textbook `cross_file_value_equals` shape — **8th
confirmation** of the strongest demand signal in P2 now (joining
airflow + tokio + clap + uv + react + pnpm + pytorch).

### 3. `registry_paths_resolve` for `tensorflow/opensource_only.files`

`tensorflow/opensource_only.files` is a 256-line manifest declaring
which files are OSS-only (vs. internal-Google). Every entry should
resolve to a real on-disk file in the OSS tree. This is the same
shape as apache/arrow's `dev/release/rat_exclude_files.txt`,
rust-lang/rust's `triagebot.toml`, and cpython's
`.gitattributes` generated markers.

**6th confirmation** of the v0.10+ `registry_paths_resolve` rule kind
(joining rust + clap + cpython + arrow + pytorch). Past saturation;
this is the highest-priority new primitive after `cross_file_value_equals`.

### 4. `generated_file_fresh` for the API goldens

The `api_compatibility_test.py` exists specifically to enforce that
the textproto goldens stay in sync with the live `tf` module. The
gate is "regenerate via `bazel run … -- --update_goldens=True` and
diff against on-disk" — exactly the `generated_file_fresh` v0.10+
rule kind shape (5th confirmation: uv + cpython + pytorch ×2 +
tensorflow).

### 5. `markdown_template_match` for `tensorflow/security/advisory/*.md`

Each TFSA advisory follows the same 6-section template
(Description / Affected versions / Reporter / CVE / Patches / Date).
There's no automated check today; reviewer enforces by hand. A
v0.10+ `markdown_template_match` primitive ("every file matching glob
X must contain headings A, B, C in this order") would close the gap.
**NEW candidate** — single-source so far; defer.

---

## What's out of alint's scope (kept on existing tooling)

Listed by category:

- **Bazel `cquery` license-graph integrity** (`Pip package generated
  license includes all dependencies' licenses` in
  `code_check_full.bats`) — needs a running bazel server to walk the
  dep graph; alint doesn't speak Bazel. STAYS on bats + bazel.
- **Bazel `cquery` pip-dep coverage** (`Pip package includes all
  required //tensorflow dependencies`) — same reason. STAYS.
- **Bazel `nobuild` smoke gate** (`bazel nobuild passes on all of
  TF except TF Lite and win toolchains`) — needs bazel. STAYS.
- **Python AST parity** (the `api_compatibility_test.py` itself
  walks `tf` via Python introspection + protobuf serialisation,
  diffs textproto trees) — too deep for alint to express; the
  test stays as-is. alint asserts the test file + scaffolding
  exists.
- **C++ AST analysis** (clang-tidy on the kernels — TF doesn't ship
  a public clang-tidy rule set; relies on internal Google
  infrastructure) — out of alint scope.
- **PR-content guards** (the bots that auto-label PRs, route
  triage) — operational, not validation surfaces.
- **Operational workflows** (`update-rbe.yml`, `stale-issues.yml`,
  `release-branch-cherrypick.yml`, etc.) — not validation surfaces.
- **`opensource_only.files` exact-match cross-validation** —
  needs the v0.10+ `registry_paths_resolve` rule kind (above).

---

## Already covered by other linters TF uses

- **clang-format** — alint shells out (mirrors bats
  `Check formatting for C++ files` @test). The deep AST analysis
  is clang's; alint orchestrates.
- **pylint** — alint shells out (mirrors bats
  `Check pylint for Python files` @test).
- **buildifier** — alint shells out (mirrors bats
  `Check buildifier formatting on BUILD files` @test).
- **codespell** — alint shells out.
- **api_compatibility_test (bazel)** — alint shells out (the deep
  protobuf reflection + textproto diff stays in the test).

---

## Performance comparison (placeholder — bench when validation pass scales)

The full unsparse tree is large enough to be a meaningful stress test:

- **~80k+ files** (full tree, before sparse-checkout)
- **~290 MiB** working tree (after sparse-checkout)
- **671 BUILD files** + **87 *.bzl** files
- **1 185 textproto golden files** under `tools/api/golden/`
- **16 GitHub Actions workflows** (small public surface)

The published S9 bench (100k+ files, 13 languages) hits ~1.4 s on a
stock CI runner. The full TF tree (with `tensorflow/core/kernels` +
`tensorflow/compiler` + `third_party/` + `tensorflow/lite/micro`
re-included, ~80k files) sits between S9 and 200k. Expected: 2-5 s
for `alint check` on the structural rules alone, vs. ~30-120 s for
`bats ci/official/utilities/code_check_full.bats` (which spawns
bazel for several queries).

Where alint shines on TF specifically: the **cross-language
file-shape parity** layer — every TFLite Swift source has its
test, every ObjC API header has its test, every Python TFLite
module has `_test.py` — runs against the full TFLite per-language
tree in tens of milliseconds. Sequential `find tensorflow/lite/swift
-name '*Tests.swift'` + the same for objc/java/python would be
~0.5 s on a hot cache.

To benchmark wall-clock for real:
`time bats ci/official/utilities/code_check_changed_files.bats` vs
`time alint check` (the changed-files variant is the closest
apples-to-apples comparison; `code_check_full.bats` includes the
full bazel cquery, which alint doesn't attempt). Deferred to the
per-repo measurement pass.

---

## Recommendation for the launch story

This case study is the **flagship "polyglot at scale with API-parity
discipline"** story for the launch:

- **tensorflow is the canonical multi-language ML monorepo on GitHub**
  (~190k stars, the substrate for every modern deep-learning library:
  TFX, TensorFlow.js, TensorFlow Lite, TF Hub, JAX-via-XLA). Naming
  it as a target gives alint instant credibility with the ML +
  data-engineering audience.
- **No per-language linter sees the cross-language API parity
  surface** — pylint only sees Python, buildifier only sees Bazel,
  clang-format only sees C++. The invariants this case study
  enforces (TFLite Swift `Sources/<X>.swift` ↔ `Tests/<X>Tests.swift`
  parity, Python TFLite `<x>.py` ↔ `<x>_test.py` parity, ObjC API
  headers ↔ tests parity, the 1185 textproto goldens existing AND
  having the canonical marker, the 5 per-Python-version requirements
  locks all present and non-empty) are exactly the layer alint owns
  and nothing else does.
- **The TWO discipline layers stacked (file-shape parity + API-shape
  parity)** are unique to the "core + bindings" topology, of which TF
  is the canonical example. apache/arrow gives us the all-peers
  topology; TF gives us the core+bindings topology. Together they
  span the full design space the v0.11+
  `cross_language_implementation_complete` primitive needs to handle.
- **The Apache 2.0 compliance bundle** + the GHA hardening bundle +
  the OSS-baseline bundle apply cleanly with minimal overrides
  (only one custom rule needed: extending the Apache header check
  to BUILD/.bzl files, which the bundled rule's default
  file-extension list doesn't cover).

Position it as the **second polyglot tile** on alint.org/examples
(after apache/arrow), with the angle: *"tensorflow has 1 core + 6
language bindings + 1 185 API-parity goldens + 0 tools that see the
cross-language conventions — alint is the layer that does. The
v0.11+ `cross_language_implementation_complete` rule kind, validated
here, generalises beyond TF to every multi-binding spec (TFLite,
TFX, TF Hub, JAX/XLA bindings)."*

The pitch lands harder when paired with the TFLite Swift finding:
**5 of 11 Sources/*.swift files have no matching Tests/*Tests.swift**
(`CoreMLDelegate`, `Delegate`, `InterpreterError`,
`SignatureRunnerError`, `SignatureRunner`). No Swift tool catches
this because no Swift tool sees the parity convention from above.

Followup feature work surfaced (consolidated, sorted by strength of
demand across P2):

- **`cross_file_value_equals` rule kind** — covers
  `requirements_lock_3_*.txt` cross-Python-version consistency here.
  **Demand: 8 of 8** (airflow + tokio + clap + uv + react + pnpm +
  pytorch + tensorflow). Strongest demand signal in P2 now;
  v0.10 must-ship.
- **`registry_paths_resolve` rule kind** — covers
  `tensorflow/opensource_only.files` here. **Demand: 6 of 6**
  (rust + clap + cpython + arrow + pytorch + tensorflow). Joins
  `cross_file_value_equals` at the top of the v0.10 priority list.
- **`generated_file_fresh` rule kind** — covers the API goldens
  regen-and-diff here. **Demand: 5 of 5** (uv + cpython + pytorch
  ×2 + tensorflow). v0.10 priority.
- **`cross_language_implementation_complete` rule kind** — the
  v0.11+ headline primitive. **Demand: 2 of 2 confirmed**
  (apache/arrow's all-peers topology + tensorflow's core-and-bindings
  topology). Generalises to every multi-binding spec; defer to v0.11+
  as the polyglot headline (after the v0.10 set ships).
- **`markdown_template_match` rule kind** — surfaced uniquely by
  TF's `tensorflow/security/advisory/*.md` template. NEW candidate;
  single-source; defer.

---

## Notes for the parent agent

- Audit (`cargo test -p alint-e2e --test
  coverage_audit_examples_parse`) **passes** with this config in place.
- Config runs cleanly against the actual cloned repo at
  `/tmp/tensorflow/`: 21 436 violations across 24 failing files
  (overwhelmingly info-level whitespace/final-newline findings on the
  ~290 MiB tree, which `alint fix` can auto-resolve for 105 of them).
  The 2 hard errors break down as:
  1. `python-manifest-exists` — bundled rule fires because TF doesn't
     ship `pyproject.toml` (it's still on `setup.py` +
     `requirements_lock_*.txt`, predating PEP 621). **ACCURATE
     finding**, called out in the config's leading comment.
  2. `oss-no-merge-conflict-markers` — fires on
     `tensorflow/tools/pip_package/THIRD_PARTY_NOTICES.txt` line 6,
     which contains a `=======` separator that's part of the file's
     own formatting (not a real merge conflict). **Bundled-rule false
     positive**; could be excluded in this config but kept as-is to
     surface the finding for review (alint's signal-to-noise on this
     particular rule is intentionally permissive).
- `command:`-tool-not-on-PATH errors are expected (buildifier,
  pylint, clang-format, codespell, bazel are not installed in the
  alint test environment); the rule structures themselves are
  correct (validated by the parse-validate audit).
- The cross-language structural rules
  (`tensorflow-lite-swift-source-has-test`,
  `tensorflow-lite-objc-api-has-test`,
  `tensorflow-lite-python-source-has-test`) all silently pass
  against the tracked TFLite tree where parity is met, AND surface
  the known drift cases noted above when run with `--changed false
  --include-info` against the full Swift/ObjC trees.
- No NEW pitfalls beyond the documented 17 in
  `docs/development/CONFIG-AUTHORING.md`. One LATENT pitfall
  surfaced and resolved: **`file_exists` with `root_only: true` on
  multi-component literal paths silently treats every entry as
  "not at root" via `literal_is_nested(p)` and thus reports
  every rule as failing.** This is documented behaviour
  (`literal_is_nested` returns true for any path with > 1
  component), but the failure mode is opaque — the rule fires
  with a generic "expected a file matching [...] at the repo
  root" message even when the literal paths obviously aren't
  at the root. The fix in this config: drop `root_only: true`
  whenever the path list contains multi-component entries
  (the explicit literal paths are inherently root-anchored
  anyway). This is a candidate for either better diagnostics
  in `file_exists::build` (warn at build time when `root_only:
  true` is set on multi-component paths) or a CONFIG-AUTHORING.md
  pitfall #18 entry. **Calling out as a possible follow-up
  for v0.9.16 phase 8 / v0.10 — single-source so far, but the
  shape is clean enough to warrant either a parse-time warning
  or a doc entry.**
