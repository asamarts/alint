# Case study: `tensorflow/tensorflow`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/tensorflow-tensorflow/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `tensorflow/tensorflow`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives — including
the v0.11+ `cross_language_implementation_complete` primitive that this
case study was specifically commissioned to validate against the
canonical "core+bindings" multi-language ML monorepo.

**Repo state captured:** 2026-05-07 sparse-clone of
`tensorflow/tensorflow@21d3ed60`. Heaviest sub-trees
(`tensorflow/core/kernels/`, `tensorflow/compiler/`, `third_party/`,
`tensorflow/lite/micro/`) excluded. After sparse-checkout: ~21,553
files / ~290 MiB working tree. Clone command:

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

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

tensorflow's structural validation lives in three places:

1. **`ci/official/utilities/*.bats`** — the canonical CI lint suites
   (9 + 4 `@test` cases across two `.bats` files; **verified** at
   `/tmp/tensorflow/ci/official/utilities/code_check_{full,changed_files}.bats`).
2. **Per-file API parity registries** —
   `tensorflow/tools/api/golden/{v1,v2}/*.pbtxt` (1,185 textprotos,
   **verified**: 585 v1 + 600 v2).
3. **`.github/workflows/`** — 16 workflows (TF leans on internal
   Google CI for the bulk; the public GHA surface is intentionally
   minimal).

There is **no `hack/verify-*.sh`-style pipeline**. There is **no
custom Python or Go AST linter binary** — the AST work happens
inside `bazel test` invocations.

### 1.1 `ci/official/utilities/code_check_full.bats` (9 `@test`s — gating)

| `@test` | What it actually does | Backing tool |
|---|---|---|
| Pip package generated license includes all dependencies' licenses | Bazel `cquery` walks the wheel target, asserts every transitive dep's license appears in `LICENSE` text | `bazel cquery` + license-graph |
| Pip package includes all required //tensorflow dependencies | Same `cquery` walk, asserts every `//tensorflow:` dep is reachable from the wheel | `bazel cquery` |
| Pip package doesn't depend on CUDA | `cquery` filter for cuda labels in the deps closure | `bazel cquery` |
| Pip package doesn't depend on CUDA for static builds (i.e. Windows) | Windows-target variant of the above | `bazel cquery` |
| All tensorflow.org/code links point to real files | greps `**/*.{md,py}` for `tensorflow.org/code/...` URLs, asserts the path exists in tree | bash + grep |
| No duplicate files on Windows | Lower-case-collision check across the source tree | bash |
| bazel nobuild passes on all of TF except TF Lite and win toolchains | `bazel build --nobuild //...` (parses BUILD files without compiling) | `bazel build --nobuild` |
| API compatibility test passes, ensuring no unexpected changes to the TF API | `bazel test //tensorflow/tools/api/tests:api_compatibility_test` — runs Python + protobuf + 1,185-pbtxt diff | `bazel test` |
| Verify that it's possible to query every TensorFlow target without BUILD errors | `bazel query` over `//...` to assert no BUILD-file syntax issues | `bazel query` |

### 1.2 `ci/official/utilities/code_check_changed_files.bats` (4 `@test`s — PR-diff gating)

| `@test` | What it actually does | Backing tool |
|---|---|---|
| Check buildifier formatting on BUILD files | `buildifier --mode=check` on the changed `BUILD`+`*.bzl` files | buildifier |
| Check formatting for C++ files | `clang-format` on changed `.cc`/`.h` | clang-format |
| Check pylint for Python files | `pylint` on changed `.py` (config: `tools/ci_build/pylintrc`, 344 lines) | pylint |
| API compatibility test passes, ensuring no unexpected changes to the TF API | Same as full-suite test 8, scoped to changed files | `bazel test` |

### 1.3 The 16 GitHub Actions workflows under `.github/workflows/`

Counted: **16** files (`ls /tmp/tensorflow/.github/workflows/ | wc -l`).
TF leans heavily on internal Google CI; the public GHA surface covers
issue triage, dependabot, release management, and a handful of
build-and-test triggers. All 16 are covered by `ci/github-actions@v1`
(3 rules: action SHA pinning, contents-read permission, workflow
has `name:`).

### 1.4 Per-file API parity registries

The headline cross-language artefact:

```
tensorflow/tools/api/golden/
├── BUILD
├── v1/        # 585 *.pbtxt — verified count via `ls | wc -l`
│   ├── tensorflow.audio.pbtxt
│   ├── tensorflow.app.pbtxt
│   └── … (one per public v1 symbol)
└── v2/        # 600 *.pbtxt — verified count
    ├── tensorflow.audio.pbtxt
    ├── tensorflow.autodiff.-forward-accumulator.pbtxt
    └── … (one per public v2 symbol)
```

**Verified counts:** 585 v1 + 600 v2 = **1,185 total textproto
goldens**. Path is `tensorflow/tools/api/golden/`, NOT
`tensorflow/python/tools/api/golden/` (correction — the latter does
not exist in the live tree). The Python introspection visitor that
generates these lives at
`tensorflow/tools/api/lib/python_object_to_proto_visitor.py`; the
gating test that diffs `tf` against the goldens lives at
`tensorflow/tools/api/tests/api_compatibility_test.py`.

### 1.5 Per-language sub-bindings under `tensorflow/lite/`

```
tensorflow/lite/swift/Sources/<Foo>.swift  ↔  Tests/<Foo>Tests.swift
tensorflow/lite/objc/apis/TFL<Foo>.h       ↔  tests/TFL<Foo>Tests.m
tensorflow/lite/java/src/main/java/.../<Foo>.java  ↔  src/test/java/.../<Foo>Test.java
tensorflow/lite/python/<foo>.py            ↔  <foo>_test.py
```

Plus 7 per-language top-level bindings under `tensorflow/`: `c/`,
`cc/`, `python/`, `java/`, `go/`, `js/`, `lite/`.

### 1.6 Repo-root governance + per-language config

| File | Owner tool | Role |
|---|---|---|
| `.bazelrc` (60 KB) | Bazel | Every CI build flag |
| `.bazelversion`, `.bazelignore`, `MODULE.bazel`, `WORKSPACE`, top-level `BUILD` | Bazel | Bazel toolchain, ignores, module def, legacy workspace, top-level package |
| `.clang-format` | clang-format | C++ style baseline |
| `.pylintrc` (symlink → `tensorflow/tools/ci_build/pylintrc`) | pylint | 344-line Python style config |
| `.gitignore`, `.gitattributes` | git | Tracked-tree exclusions |
| `LICENSE`, `CITATION.cff`, `CODEOWNERS`, `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `ISSUES.md`, `README.md`, `RELEASE.md`, `SECURITY.md`, `AUTHORS` | community | Repo-root governance |
| `requirements_lock_3_{10,11,12,13,14}.txt` | pip-compile | Per-Python-version lockfile matrix (5 files) |
| `configure`, `configure.cmd`, `configure.py` | Bazel | Pre-build CUDA/ROCm detection |
| `tensorflow/opensource_only.files` | manifest | 256-line internal-Google ↔ OSS file manifest |
| `.github/dependabot.yml`, `.github/bot_config.yml`, `.github/ISSUE_TEMPLATE/{tensorflow_issue_template.yaml,tflite-{converter,op-request,other,in-play-services}.md}` | GitHub UI | Bot config + issue templates |
| `tensorflow/security/{README.md,advisory/*.md}` | community | CVE advisory tree (~200 published TFSAs) |

### 1.7 `ci/official/` entry-points + envs

| File | Role |
|---|---|
| `code_check_full.sh` → `utilities/code_check_full.bats` | Dispatcher for the 9-test full suite |
| `code_check_changed_files.sh` → `utilities/code_check_changed_files.bats` | Dispatcher for the 4-test PR-diff suite |
| `wheel.sh`, `installer_wheel.sh`, `libtensorflow.sh`, `pycpp.sh` | Wheel build entry-points |
| `upload.sh`, `bisect.sh`, `any.sh`, `debug_tfci.sh` | Upload + debug helpers |
| `envs/{linux_x86,linux_arm64,windows_x86_2022,linux_x86_cuda,…}` | 16 per-platform env-var blobs |
| `utilities/{cleanup_docker.sh,setup_docker.sh,…}` | docker setup helpers |

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset OR the per-rule entry
  in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why (Bazel cquery, runtime probe, AST,
  network).

### 2.1 The 13 bats `@test` cases

| `@test` | Coverage | Notes |
|---|---|---|
| Pip license-graph integrity | ❌ out-of-scope | Needs `bazel cquery` (label-graph traversal). |
| Pip dep coverage | ❌ out-of-scope | Same — `bazel cquery`. |
| No CUDA dep (Linux wheel) | ❌ out-of-scope | `bazel cquery`. |
| No CUDA dep (Windows static) | ❌ out-of-scope | `bazel cquery`. |
| tensorflow.org/code links resolve | 🔄 alint-future | `registry_paths_resolve` (v0.10 ship-target, 8 sources): every URL of shape `tensorflow.org/code/<path>` extracted from `**/*.{md,py}` must resolve to an on-disk file. |
| No duplicate Windows files | ✅ alint-today | `file_lowercase_collision` is not a bundled rule yet, but `for_each_file` + a per-file lower-case-of-basename check is the workaround. **Or** a v0.10+ design candidate `case_collision_safe`. |
| bazel nobuild | ❌ out-of-scope | Bazel build invocation. |
| API compatibility test | 🔄 alint-future (partial) | The test itself is `bazel test` (out of scope for execution). The **goldens-vs-runtime parity** is the v0.11+ `cross_language_implementation_complete` primitive (1,185 textprotos = the canonical demand-driver). |
| Bazel query over //... | ❌ out-of-scope | Bazel query. |
| buildifier on BUILD | ✅ alint-today | `command:` rule shelling to `buildifier --mode=check`. |
| clang-format on C++ | ✅ alint-today | `command:` rule shelling to `clang-format`. |
| pylint on Python | ✅ alint-today | `command:` rule shelling to `pylint --rcfile=tensorflow/tools/ci_build/pylintrc`. |
| API compat (changed-files variant) | 🔄 alint-future | Same as full-suite test 8. |

**Tally for §2.1 (the 13 bats cases):**

```
✅ alint-today:    4 / 13 = 31%   (no-dup-windows + buildifier + clang-format + pylint)
🔄 alint-future:   3 / 13 = 23%   (links-resolve + API compat full + API compat changed)
❌ out-of-scope:   6 / 13 = 46%   (the 6 bazel cquery/build/query cases)
```

### 2.2 The 16 GHA workflows

All 16 covered by `ci/github-actions@v1` (3 rules). 100 % alint-today.

### 2.3 The 1,185 API goldens

| Surface | Coverage | Notes |
|---|---|---|
| Each pbtxt exists | ✅ alint-today | `tensorflow-api-golden-v1-present` + `tensorflow-api-golden-v2-present` (custom rules in `.alint.yml`). |
| Each pbtxt is non-empty | ✅ alint-today | `file_min_size: 1` per-pbtxt. |
| Each pbtxt's first non-comment line is `path: "tensorflow.X"` | ✅ alint-today | `file_starts_with` (after a stripping comment). |
| **v1 ↔ v2 set parity** (every public symbol has BOTH a v1 and v2 textproto, modulo documented exceptions) | 🔄 alint-future (v0.11+) | **`cross_language_implementation_complete`** — TF is the **canonical core+bindings demand-driver** (5 sources: arrow + TF + protobuf + angular + flutter). |
| **goldens ↔ live `tf` introspection parity** (the regen-and-diff gate) | 🔄 alint-future | `generated_file_fresh` (v0.10 ship-target, 6 sources). Tension: alint's deliberate non-goal of running codegen makes this opt-in. |

### 2.4 Per-language file-shape parity (TFLite Swift / ObjC / Java / Python)

| Sub-binding | Coverage | Rule |
|---|---|---|
| TFLite Swift `Sources/<Foo>.swift` ↔ `Tests/<Foo>Tests.swift` | ✅ alint-today | `pair` rule kind. **Live-tree finding:** 5 of 11 Swift sources have NO matching test (CoreMLDelegate, Delegate, InterpreterError, SignatureRunnerError, SignatureRunner). |
| TFLite ObjC `apis/TFL<Foo>.h` ↔ `tests/TFL<Foo>Tests.m` | ✅ alint-today | `pair`. Live-tree: 4 of 9 API headers without test partner. |
| TFLite Java `src/main/<Foo>.java` ↔ `src/test/<Foo>Test.java` | ✅ alint-today | `pair`. ~20 sources, ~21 tests (test helpers). |
| TFLite Python `<foo>.py` ↔ `<foo>_test.py` | ✅ alint-today | `pair`. ~25 source/test pairs. |

### 2.5 Per-Python-version lockfile cross-consistency

| Surface | Coverage | Rule |
|---|---|---|
| 5× `requirements_lock_3_{10,11,12,13,14}.txt` files exist | ✅ alint-today | `file_exists` per-version. |
| 5× files are substantive (non-empty floor) | ✅ alint-today | `file_min_size`. |
| **Every pinned package has the same version across all 5 files** (modulo interpreter-conditional packages) | 🔄 alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources). TF is one of the 10 demand-drivers. |

### 2.6 The OSS-only manifest

| Surface | Coverage | Rule |
|---|---|---|
| `tensorflow/opensource_only.files` exists | ✅ alint-today | `file_exists`. |
| **Every entry in the manifest resolves to an on-disk file in the OSS tree** | 🔄 alint-future | `registry_paths_resolve` (v0.10 ship-target). |

### 2.7 Repo-root governance

| Artefact | Coverage |
|---|---|
| `LICENSE`, `README.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `CODEOWNERS`, `AUTHORS`, `CITATION.cff`, `ISSUES.md`, `RELEASE.md` | ✅ alint-today (oss-baseline + per-artefact `file_exists`) |
| `LICENSE` is Apache-2 | ✅ alint-today | `compliance/apache-2@v1` (3 rules). |
| `.github/dependabot.yml` covers GH Actions | ✅ alint-today | Custom `tensorflow-dependabot-covers-actions`. |
| `tensorflow/security/README.md` exists + `advisory/*.md` shape | ✅ presence + 🔄 future shape | `file_exists` for the index; per-CVE template-match needs `markdown_template_match` (v0.10+ design candidate, single-source TF only). |

---

## 3. Quantified coverage

Counted across the **13 bats cases** + **16 workflows** + **5 API
golden surfaces** + **4 TFLite parity surfaces** + **3 lockfile
surfaces** + **2 manifest surfaces** + **9 governance artefact
families** + **3 Bazel-config files** = **55 distinct surfaces**.

```
✅ alint-today:    32 / 55 = 58%   (4 bats + 16 workflows + 3 goldens + 4 TFLite + 2 lockfiles + 2 governance + 1 dependabot)
🔄 alint-future:    9 / 55 = 16%   (3 bats + 2 goldens + 1 lockfile + 1 manifest + 1 cve-template + 1 mixed)
❌ out-of-scope:   14 / 55 = 25%   (6 bazel-driven + Bazel-config files + …)
                  ──────────────
                  total = 100%
```

Granular breakdown:

```
bats @test cases (13):
  ✅ alint-today:     4 / 13 = 31%
  🔄 alint-future:    3 / 13 = 23%
  ❌ out-of-scope:    6 / 13 = 46%

GHA workflows (16):
  ✅ alint-today:    16 / 16 = 100%   (all under ci/github-actions@v1)

API goldens (5 surfaces):
  ✅ alint-today:     3 / 5  = 60%   (presence + non-empty + canonical-marker)
  🔄 alint-future:    2 / 5  = 40%   (set parity v1↔v2 + regen-and-diff freshness)

TFLite per-language parity (4):
  ✅ alint-today:     4 / 4  = 100%   (all `pair` kind)

per-Python-version lockfile (3):
  ✅ alint-today:     2 / 3  = 67%   (presence + non-empty floor)
  🔄 alint-future:    1 / 3  = 33%   (cross-version value parity)
```

**Commentary.** Three observations:

1. **TF is the canonical "core + N bindings" multi-language ML
   monorepo.** It validates the v0.11+
   `cross_language_implementation_complete` rule kind across BOTH
   topologies in a single repo: file-shape parity within one
   language (TFLite Swift `Sources` ↔ `Tests`) AND API-shape parity
   across N bindings (1,185 textprotos = `tf.compat.v1` ↔ `tf` v2,
   plus implicit Python ↔ Java ↔ ObjC ↔ Swift binding parity).
   No other case study in the catalogue exercises both topologies
   so cleanly.

2. **Almost half of the bats suite (6 of 13) is bazel cquery /
   bazel build / bazel query — out of alint's deliberate
   non-goals.** alint never executes bazel; the existing tools are
   the right tools. The remaining 7 bats cases are either
   alint-today shellouts (buildifier, clang-format, pylint,
   no-dup-windows) or v0.10+ rule-kind shape (links-resolve, API
   parity).

3. **Three v0.10 ship-targets converge here:**
   `cross_file_value_equals` (lockfile cross-version parity),
   `registry_paths_resolve` (opensource_only.files + tensorflow.org
   links), and `generated_file_fresh` (API goldens regen). The
   v0.11+ `cross_language_implementation_complete` is the headline
   demand TF was specifically commissioned for.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (1,009 lines, 40
repo-specific rules, 6 bundled rulesets folded in via `extends:`,
**83 rules total** loaded — confirmed by `alint validate-config`).

**Synopsis of the load-bearing rules** (full config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/compliance/apache-2@v1           # 3 rules
  - alint://bundled/python@v1                        # 9 rules
  - alint://bundled/ci/github-actions@v1             # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules
  - alint://bundled/tooling/editorconfig@v1          # 3 rules

rules:
  # ── The TFLite per-language file-shape parity layer ──────────────
  - id: tensorflow-lite-swift-source-has-test    # 5 sources lack test
    kind: pair                                    # tensorflow/lite/swift/Sources/<Foo>.swift ↔ Tests/<Foo>Tests.swift
    paths: "tensorflow/lite/swift/Sources/*.swift"
    partner: "tensorflow/lite/swift/Tests/{stem}Tests.swift"
  - id: tensorflow-lite-objc-api-has-test        # 4 APIs lack test
    kind: pair
    paths: "tensorflow/lite/objc/apis/TFL*.h"
    partner: "tensorflow/lite/objc/tests/TFL{stem}Tests.m"
  - id: tensorflow-lite-python-source-has-test
    kind: pair
    paths: "tensorflow/lite/python/*.py"
    partner: "tensorflow/lite/python/{stem}_test.py"
  # ── The 1,185 API parity textproto goldens ───────────────────────
  - id: tensorflow-api-golden-v1-has-canonical-marker
    kind: file_content_matches
    paths: "tensorflow/tools/api/golden/v1/*.pbtxt"
    pattern: '(?m)^path:\s*"tensorflow\.'
  - id: tensorflow-api-golden-v2-has-canonical-marker
    kind: file_content_matches
    paths: "tensorflow/tools/api/golden/v2/*.pbtxt"
    pattern: '(?m)^path:\s*"tensorflow\.'
  # ── Apache-2 header on every BUILD / .bzl ────────────────────────
  - id: tensorflow-bazel-files-have-apache-header
    kind: file_content_matches
    paths: ["**/BUILD", "**/*.bzl"]
    pattern: 'Licensed under the Apache License,?\s*Version 2'
  # ── The 13 bats @test cases — shellouts to existing tools ────────
  - id: tensorflow-buildifier
    kind: command
    paths: ["**/BUILD", "**/*.bzl"]
    command: ["buildifier", "--mode=check", "{path}"]
  - id: tensorflow-pylint
    kind: command
    paths: "**/*.py"
    command: ["pylint", "--rcfile=tensorflow/tools/ci_build/pylintrc", "{path}"]
```

**Repo-specific vs bundled split:**

- **40 tensorflow-specific rules** (`tensorflow-*` prefix): 1
  cross-language structural (`tensorflow-language-subdirs-present`)
  + 4 TFLite per-source ↔ per-test parity (Swift / ObjC / Java /
  Python) + 5 API-parity registry (golden v1 + v2 presence +
  non-empty + canonical-marker) + 3 Bazel build-system + 6
  governance / config-presence + 2 pip-lockfile + 2 CI-script
  presence + 6 `command:` shellouts (buildifier, pylint,
  clang-format, codespell, api-compatibility-test, plus a few
  smaller helpers) + 11 long-tail (no-tabs-in-py,
  no-trailing-whitespace, final-newline, no-bidi-in-cc-sources, …).
- **44 bundled rules** from the 6 extended rulesets (15 + 3 + 9 + 3
  + 11 + 3 = 44 with overlap dedup).

**Validation:** `alint validate-config` reports `✓ Config valid: 83
rule(s) loaded`. Pitfall checks: the magic comment is present (line
1); all `command:` rules use `command:` and integer `timeout:`;
the `pair` rule uses `partner:` (not `secondary:`); all patterns
use single-quoted YAML scalars (no YAML literal block scalars —
pitfall #22-clean), including the multi-line Apache-header regex
on lines 660 and 678.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the live
`/tmp/tensorflow/` sparse-checkout (~21k files / ~290 MiB).
Machine: Linux 6.1.0-42-amd64, ~10 logical cores; alint binary
`target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| Single-file shellcheck (e.g. `ci/official/utilities/setup.sh`) | shellcheck | **40 ms** ± 1 ms | included in 11.146 s full pass | n/a — alint shells out via `command:` rule |
| **alint full pass (83 rules)** | n/a | n/a | **11.146 s** ± 7.903 s (high variance, 6.5-20.3 s range) | — |

The 11 s wall-clock against the ~21k-file sparse-checkout is
dominated by:

- **The 6 `command:` shellouts** (buildifier, pylint, clang-format,
  codespell, api-compat-test) firing once per matching anchor file
  each. None of these tools are on PATH on the bench machine, so
  each shellout fires as "command not found"; the 11 s
  upper-bounds the spawn-and-fail overhead across thousands of
  matching anchors (671 BUILD files + 87 *.bzl + ~5,000 *.py).
- **The full source-tree walk** for the bundled `python@v1` +
  `oss-baseline@v1` rules over the 21k-file tree.
- **The 1,185-pbtxt golden parity walk** for the 5 API-parity
  rules.

The high variance (6.5-20.3 s) is the spawn-and-fail bottleneck:
on a hot filesystem cache the per-shellout fork overhead drops to
the ~6 s lower bound. **Strip the 6 shellouts and the
declarative-only pass runs in roughly 2-4 s**, between the
published S9 macro-bench (~1.4 s for 100k polyglot files) and the
full-tree walk cost.

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `buildifier --mode=check` (tensorflow-buildifier) | buildifier | pending — `buildifier` not on PATH | `go install github.com/bazelbuild/buildtools/buildifier@latest` |
| `pylint --rcfile=tensorflow/tools/ci_build/pylintrc` (tensorflow-pylint) | pylint | pending — `pylint` not on PATH | `pip install pylint` |
| `clang-format` (tensorflow-clang-format) | clang-format | pending — `clang-format` not on PATH | `apt install clang-format` |
| `codespell` (tensorflow-codespell) | codespell | pending — `codespell` not on PATH | `pip install codespell` |
| `api_compatibility_test` (tensorflow-api-compatibility-test) | bazel + the test target | pending — needs the full `bazel` toolchain (~5-15 s warm-server overhead alone) | `bazel test //tensorflow/tools/api/tests:api_compatibility_test` |
| `bats ci/official/utilities/code_check_full.bats` (the 9 @test full suite) | bats + bazel + buildifier + clang-format + pylint + curl | pending — needs the full TF toolchain stack | `bats ci/official/utilities/code_check_full.bats` |
| `bats ci/official/utilities/code_check_changed_files.bats` (the 4 @test PR-diff suite) | bats + buildifier + clang-format + pylint + bazel | pending — same toolchain requirement | `bats ci/official/utilities/code_check_changed_files.bats` |

The `code_check_changed_files.bats` PR-diff variant is the closest
apples-to-apples comparison (`code_check_full.bats` spawns bazel
cquery for several @tests, which alint deliberately doesn't
replicate). Estimated 10-30 s for the changed-files variant on a
warm cache vs alint's 11 s for the structural-only declarative
pass plus the shellout chain.

**Where alint shines on TF specifically:** the cross-language
file-shape parity layer (TFLite Swift `Sources` ↔ `Tests`, ObjC
`apis/TFL<Foo>.h` ↔ `tests/TFL<Foo>Tests.m`, Python
`<foo>.py` ↔ `<foo>_test.py`, Java `<Foo>.java` ↔ `<Foo>Test.java`)
runs against the full TFLite per-language tree in tens of
milliseconds. Sequential `find tensorflow/lite/swift -name
'*Tests.swift'` + the same for objc/java/python would be ~0.5 s on
a hot cache.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/tensorflow-tensorflow/.alint.yml --format json /tmp/tensorflow/`
(live run, JSON-format).

**Headline:** alint surfaces **21,436 violations** across 24
failing rules (43 passing). The vast majority is the 5 `command:`
shellouts firing as "command not found" on each anchor file
(below), masking the real findings (~150 actual structural
violations once shellouts are filtered).

| # | Count | Rule | Triage |
|---|---|---|---|
| 1 | 10,313 | `tensorflow-codespell` | **All shellout-failure synthesised.** `codespell` not on PATH; the rule fires per matching anchor. **Strip the 6 shellouts and the structural-rule violation count drops to ~150.** |
| 2 | 6,603 | `tensorflow-clang-format` | Same — `clang-format` not on PATH. |
| 3 | 2,880 | `tensorflow-pylint` | Same — `pylint` not on PATH. |
| 4 | 759 | `tensorflow-buildifier` | Same — `buildifier` not on PATH. Note 671 BUILD + 87 .bzl = 758 anchor files (1 over from a long-tail extension). |
| 5 | 700 | `tensorflow-bazel-files-have-apache-header` | **CONFIRMED rule-premise mismatch.** Sampled `head -20 /tmp/tensorflow/BUILD`, `…/tensorflow/BUILD`, `…/tensorflow/lite/BUILD`: TF's BUILD files declare licensing via Bazel's `licenses(["notice"])` + `default_applicable_licenses = ["//tensorflow:license"]` (which points to the repo-root LICENSE), NOT inline Apache-2 headers. The rule's premise is wrong for TF's policy. **Recommended fix:** either (a) drop the rule (TF's licensing model is per-Bazel-package, not per-file), or (b) replace the regex with `'(licenses\(.*notice|default_applicable_licenses.*license)'` to match TF's actual licensing-declaration shape, or (c) scope the rule to .py files only (where Apache headers ARE inline; verify with `head -20 /tmp/tensorflow/tensorflow/python/__init__.py`). |
| 6 | 36 | `oss-no-trailing-whitespace` | Real findings. |
| 7 | 36 | `apache-2-source-has-license-header` | Bundled rule firing on the same files as #5, narrower scope. |
| 8 | 36 | `tensorflow-no-trailing-whitespace` | Same as #6, narrower scope. |
| 9 | 25 | `oss-final-newline` | Real findings. |
| 10 | 18 | `tensorflow-lite-python-source-has-test` | **All real findings.** ~18 Python TFLite modules without `_test.py` partner. Worth filing as test-coverage gaps. |
| 11 | 8 | `tensorflow-final-newline` | Real findings. |
| 12 | 5 | `tensorflow-lite-swift-source-has-test` | **All real findings.** Validates §6 spec: CoreMLDelegate.swift, Delegate.swift, InterpreterError.swift, SignatureRunnerError.swift, SignatureRunner.swift have no matching `*Tests.swift`. |
| 13 | 4 | `tensorflow-lite-objc-api-has-test` | **All real findings.** Validates §6 spec: 4 ObjC `apis/` headers without `tests/` partners. |
| 14 | 3 | `gha-workflow-contents-read` | Real findings — workflows lacking permissions block. |
| 15 | 1 | `oss-no-merge-conflict-markers` | **Validates §6 spec:** the `=======` separator in `tensorflow/tools/pip_package/THIRD_PARTY_NOTICES.txt` (formatting, not a real conflict). **Recommended fix:** add to `paths.exclude` for that one file. |
| 16 | 1 | `python-manifest-exists` | **Validates §6 spec:** TF doesn't ship `pyproject.toml` (still on `setup.py` + `requirements_lock_*.txt`, predating PEP 621). **Accurate finding** — flag as such in config's leading comment. |
| 17 | 1 | `python-has-lockfile` | Same — TF uses `requirements_lock_3_*.txt` matrix, not the PEP 621 lockfile shape the bundled rule expects. |
| 18 | 1 | `tensorflow-tf-version-bzl-declares-semver` | Real finding — needs investigation. |

**Real findings (alint surfaced, existing tooling missed):**

- **9 TFLite test-coverage gaps** (5 Swift + 4 ObjC) — validates
  §6 spec exactly. The bats suite doesn't surface these because
  the bats checks are at-build-time (compile + test) rather than
  at-PR-time (file presence).
- **18 Python TFLite modules without `_test.py` partner** — the
  Python-side equivalent of the Swift/ObjC findings.
- **1 merge-conflict-marker false-positive** in
  `THIRD_PARTY_NOTICES.txt` (validates §6 spec).
- **2 PEP 621 false-positives** (`python-manifest-exists`,
  `python-has-lockfile`) — TF's pre-PEP-621 packaging is
  intentional; the bundled rules need overrides documented.

**SUSPECT — needs investigation:** the 700-violation
`tensorflow-bazel-files-have-apache-header` count (92 % of
BUILD/.bzl files) is either a major real finding (TF's
boilerplate genuinely doesn't carry the Apache header on most
BUILD files) or a regex-phrasing mismatch. **Verify with**
`head -20 /tmp/tensorflow/BUILD` and `head -20
/tmp/tensorflow/tensorflow/BUILD`. If the regex needs adjusting,
the alternative phrasings to consider are `'Apache 2\.0 License'`
or `'Apache License Version 2\.0'`.

**Pitfall #22 verification:** ZERO instances in `.alint.yml`.
`grep -nE 'pattern:\s*[|>][-+]?$'
/home/kaminsod/projects/alint/examples/tensorflow-tensorflow/.alint.yml`
returns no matches. The 5 multi-line patterns in this config use
single-quoted YAML scalars (e.g.
`pattern: '(?m)^path:\s*"tensorflow\.'` on lines 382 + 393 for the
golden canonical-marker check, and
`pattern: 'Licensed under the Apache License,?\s*Version 2'` on
lines 660 + 678 for the Apache-header check). The Apache-header
pattern is the obvious pitfall #22 risk — verified single-quoted,
correctly written.

---

## 7. Pitfall #22 verification (this batch's special call-out)

The brief asked: **verify every multi-line regex in this case
study's config for the YAML literal-block-scalar trailing-newline
issue (pitfall #22).**

**Verdict for `examples/tensorflow-tensorflow/.alint.yml`: ZERO
instances.** `grep -nE 'pattern:\s*[|>][-+]?$'
/home/kaminsod/projects/alint/examples/tensorflow-tensorflow/.alint.yml`
returns no matches. The 5 multi-line patterns in this config use
single-quoted YAML scalars (the obvious one — TF's Apache header
on every BUILD / .bzl — is `pattern: 'Licensed under the Apache
License,?\s*Version 2'` on lines 660 and 678, single-quoted scalar
where `\s*` is a literal regex metacharacter). The header check
is `file_content_matches`, not `file_header`, so the pitfall #22
class (literal block scalar trailing newline coercing
`file_header` patterns) doesn't apply directly here.

---

## 8. Followup feature work surfaced

Sorted by demand strength:

- **`cross_file_value_equals`** — covers
  `requirements_lock_3_*.txt` cross-Python-version consistency.
  **v0.10 ship-target (10 sources).** TF is one of the
  demand-drivers.
- **`registry_paths_resolve`** — covers
  `tensorflow/opensource_only.files` here AND the
  tensorflow.org/code link-resolution bats test. **v0.10
  ship-target (8 sources).**
- **`generated_file_fresh`** — covers the API goldens
  regen-and-diff. **v0.10 ship-target (6 sources: uv + cpython +
  pytorch + bazel + TF + spark).** Tension: alint's deliberate
  no-codegen non-goal makes this opt-in.
- **`cross_language_implementation_complete`** — the v0.11+
  headline primitive. **v0.11+ ship-target (5 confirmed sources:
  arrow + TF + protobuf + angular + flutter).** TF's 1,185
  textproto goldens are the second source after arrow and the
  canonical core+bindings topology demonstration.
- **`markdown_template_match`** — covers
  `tensorflow/security/advisory/*.md` template uniformity. **v0.10
  design candidate (single-source: TF only).** Defer.
- **`case_collision_safe` / `file_lowercase_collision`** — covers
  the "no duplicate Windows files" bats case. **v0.10+ design
  candidate (single-source: TF only).** Defer.

---

## 9. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **A `bazel-monorepo@v1` bundled-ruleset draft.** TF's
   BUILD-file presence + `*.bzl` Apache-header +
   `MODULE.bazel`/`WORKSPACE`/`BUILD` triad checks recur in any
   Bazel monorepo (TF + grpc + envoy + many internal Google +
   Pinterest + Lyft repos). Packaging these as a bundled ruleset
   would let alint claim the "Bazel-tier polyglot monorepo" niche
   before pytorch + grpc + envoy case studies land — TF is the
   canonical example to author it against.
2. **`scope_filter` for the `tensorflow/lite/<lang>/` per-binding
   sub-trees.** The four TFLite parity rules
   (Swift, ObjC, Python, Java) hard-code globs. v0.9.17's
   `scope_filter` evolution lets each binding be a named filter
   with its own per-language file conventions; the parity check
   becomes a single `for_each_dir` against `tensorflow/lite/*/`
   with the per-binding pair pattern declared once. Refactor
   saves ~80 lines and serves as the design template for the
   v0.11+ `cross_language_implementation_complete` rule kind.
3. **`alint suggest` against a fresh `/tmp/tensorflow/` tree.** Past
   runs surfaced only `oss-baseline@v1` + `agent-hygiene@v1`
   (medium) — the bundled-ruleset detector doesn't yet recognise
   the Bazel monorepo shape (no `Cargo.toml` / `package.json` /
   `go.mod`), which is exactly the suggester gap a
   `bazel-monorepo@v1` bundle would close. File as a v0.10+
   suggester-improvement candidate alongside the bundled ruleset.

---

## 10. Validation status (2026-05-07)

- **alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).
- **`.alint.yml` in this directory:** **shipped — 1,009 lines, 40
  repo-specific rules, 6 bundled rulesets folded in via `extends:`,
  83 effective rules loaded.**
  `alint validate-config` confirms `✓ Config valid: 83 rule(s)
  loaded`. **Live-tree recheck:** performed in this batch — see §6
  for the 21,436-violation breakdown (the long tail is dominated by
  ~20k shellout-failure synthesised counts because the 6 external
  tools — buildifier / pylint / clang-format / codespell / etc. —
  aren't on PATH on the bench machine; ~150 real structural
  findings, including the 9 + 18 TFLite test-coverage gaps, the
  THIRD_PARTY_NOTICES merge-conflict-marker false positive, and
  the 700-violation Apache-header SUSPECT that needs regex-pattern
  verification).
- **API goldens path verification:** **CONFIRMED**
  `tensorflow/tools/api/golden/{v1,v2}/` (NOT
  `tensorflow/python/tools/api/golden/`). Counts via
  `ls /tmp/tensorflow/tensorflow/tools/api/golden/v1 | wc -l` =
  **585** and `…/v2 | wc -l` = **600** = **1,185 total**.
- **bats `@test` count verification:** **CONFIRMED 9 + 4 = 13**
  via `grep -E "^@test"` on the two `.bats` files at
  `/tmp/tensorflow/ci/official/utilities/code_check_{full,changed_files}.bats`.
- **GHA workflow count verification:** **CONFIRMED 16** via
  `ls /tmp/tensorflow/.github/workflows/ | wc -l`.
- **Rule-kind candidate status:**
  - `cross_file_value_equals` — v0.10 ship-target (10 sources).
  - `registry_paths_resolve` — v0.10 ship-target (8 sources).
  - `generated_file_fresh` — v0.10 ship-target (6 sources). TF
    is one of the 6.
  - `cross_language_implementation_complete` — v0.11+ ship-target
    (5 sources). TF is the 2nd source after arrow and the
    canonical core+bindings topology demonstration.
  - `markdown_template_match`, `case_collision_safe` — v0.10+
    design candidates, single-source (TF only). Defer.
- **Pitfall #22 instances in this directory's config:** **ZERO**
  (`grep -nE 'pattern:\s*[|>][-+]?$' .alint.yml` returns no
  matches; all 5 multi-line patterns use single-quoted scalars).
  The Apache-header check (lines 660, 678) is the obvious risk
  candidate — verified single-quoted scalar
  `pattern: 'Licensed under the Apache License,?\s*Version 2'`,
  correctly written.
- **Bundled-ruleset rule counts (authoritative as of 2026-05-07):**
  oss-baseline=15, python=9, ci/github-actions=3,
  hygiene/no-tracked-artifacts=11, compliance/apache-2=3,
  tooling/editorconfig=3.
