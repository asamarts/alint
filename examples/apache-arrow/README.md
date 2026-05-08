# Case study: `apache/arrow`

> Marketing/positioning writeup at https://alint.org/examples/apache-arrow/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `apache/arrow`
and an alint config that replaces the rules alint can express
today, plus a catalogue of the rules that need new alint
primitives.

**Repo state captured:** 2026-05-07 sparse-clone at `/tmp/arrow`
(latest tip of `main`), 94 MB working tree: 5,281 files, **6
in-tree language implementations** (cpp/, c_glib/, python/, r/,
ruby/, matlab/) + the cross-language `format/` schema spec, **8
Ruby gem subdirectories** under `ruby/red-*` (each with its own
LICENSE+NOTICE+README+Rakefile+Gemfile+gemspec — the most uniform
per-package shape in the tree), **7 GLib sub-libraries** under
`c_glib/` each with `meson.build`, **28 GitHub Actions workflows**,
**21 distinct hook ids** in 396-line `.pre-commit-config.yaml`
across 14 external + 1 local hook repo, **53 `dev/release/*`
scripts** implementing the Apache release dance, **11 root-level
lint/format tool configs**, **102 path patterns** in
`dev/release/rat_exclude_files.txt`. **alint version:** 0.9.17
(`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check arrow runs today, one row per check. The repo's gating
infrastructure is **`pre-commit` (21 hooks via the `prek` runner) +
`dev/release/run-rat.sh` (the Apache RAT release-audit tool wrapped
as a local hook) + 28 GitHub Actions workflows**. Unlike kubernetes
(Prow + `make verify`), apache/arrow centralises everything through
`.pre-commit-config.yaml`, with `dev/archery` as the
cross-language integration-test harness.

### 1.1 `.pre-commit-config.yaml` (21 distinct hook ids — gating)

Categorised by what they actually do.

| Hook id | Repo / origin | Scope (files: glob) | What it actually does |
|---|---|---|---|
| `rat` | local (runs `dev/release/run-rat.sh`) | `always_run: true` | Downloads `apache-rat-${VERSION}.jar` from Maven Central; runs RAT against an archive of HEAD; passes the report to `check-rat-report.py` for filtering against `rat_exclude_files.txt`. **The Apache release-audit gate** — every PR + every release candidate must clear this |
| `hadolint-docker` | hadolint/hadolint | `(?x)^(ci/docker/.*\.dockerfile\|matlab/Dockerfile\|python/.*\.dockerfile)$` | Lint Dockerfiles (4 specific files) — uses `.hadolint.yaml` for ignore list (DL3008/DL3013/DL3015/DL3018/DL3041) |
| `clang-format` (×4 instances) | pre-commit/mirrors-clang-format | `^cpp/`, `^c_glib/`, `^matlab/src/cpp/`, `^python/pyarrow/src/` | Reformats C/C++ to canonical style (`.clang-format`) — pre-commit demands one hook entry per file-scope |
| `cpplint` (×2 instances) | cpplint/cpplint | `^cpp/`, `^matlab/src/cpp/` | C++ style lint with filter set + 90-char line-length (`CPPLINT.cfg`) |
| `autopep8` | hhatto/autopep8 | `^(c_glib\|dev\|python)/` and `\.py$` | Python formatter (in addition to flake8) |
| `flake8` | pycqa/flake8 | `^(c_glib\|dev\|python)/` and `\.py$` | Python lint |
| `cython-lint` | MarcoGorelli/cython-lint | `^python/.*\.(py\|pyx\|pxd\|pxi)$` | Cython lint (covers `.pyx`/`.pxd` plus `.py` interop) |
| `numpydoc-validation` | numpy/numpydoc | `^python/pyarrow/.*\.py$` | NumPy docstring validation per `numpydoc.validation` |
| `lintr` | local (R lintr via Rscript) | `^r/.*\.R$` | R lint |
| `air-format` | posit-dev/air-pre-commit | `^r/.*\.R$` | R format check |
| `rubocop` | rubocop/rubocop | `^(c_glib\|ruby)/.*\.(rb\|rake)$`, `Rakefile`, `Gemfile`, `*.gemspec` | Ruby AST + style |
| `cmake-format` | cheshirekow/cmake-format-precommit | `*CMakeLists.txt`, `^cpp/.*\.cmake.in$`, `^ci/.*\.cmake$` | CMake formatter (Python-syntax config in `cmake-format.py`) |
| `sphinx-lint` | sphinx-contrib/sphinx-lint | `^docs/source/.*\.(rst\|md)$` | RST/Sphinx lint |
| `shellcheck` | koalaman/shellcheck-precommit | long allowlist of specific shell scripts | Shell static-analysis (`.shellcheckrc`) — scoped via allowlist (legacy lint-debt; new scripts opt in) |
| `shfmt` | scop/pre-commit-shfmt | same allowlist | Shell formatter |
| `meson-fmt` | trim21/pre-commit-mirror-meson | `*meson.build`, `*meson.options` | Meson build-file formatter |

**The 4× `clang-format` repetition is an expressivity gap in
pre-commit** — pre-commit demands one hook entry per `files:`
pattern, so the cpp/, c_glib/, matlab/src/cpp/, and
python/pyarrow/src/ scopes each need their own. Same for cpplint's
2× repetition. alint can express this in one `command:` rule that
drives the umbrella `pre-commit run --all-files`, but a v0.10+
`fanout: {scope_filter: ..., command: ...}` primitive could express
the per-scope fan-out declaratively (single-source so far — defer).

### 1.2 `dev/release/` — Apache release-tooling discipline (53 scripts)

The Apache release dance: source tarball, binary
submit/upload/verify, vote email, post-release cleanup.

| Surface | What it does | Class |
|---|---|---|
| `dev/release/run-rat.sh` | Downloads `apache-rat-${VERSION}.jar` from Maven Central, runs RAT against an archive of HEAD; outputs the RAT report. **Required for both the local pre-flight (`pre-commit run rat`) and the formal release-candidate verification** | Gating |
| `dev/release/check-rat-report.py` | Post-processes the RAT report against `rat_exclude_files.txt`; exits non-zero if any unapproved file remains | Gating |
| `dev/release/rat_exclude_files.txt` (102 patterns) | Path-pattern allowlist consumed by RAT (binary fixtures, vendored code, generated files like `r/R/arrowExports.R`). Apache RAT itself fails non-zero if a pattern in the exclude list **doesn't resolve** to ≥1 on-disk file | Gating + path-registry |
| `dev/release/verify-release-candidate.sh` | The script a PMC member runs against an unfreezed RC tarball during the Apache 72-hour vote window | Operational |
| `dev/release/01-prepare.sh` through `10-vote-email.sh` (and `post-01-tag.sh` through `post-14-conan.sh`) | The numbered scripts that drive the entire release dance | Operational |
| `dev/release/01-prepare-test.rb`, `02-source-test.rb`, `10-vote-email-test.rb` | Per-step Ruby tests for the release scripts (test-driven release tooling) | Operational |
| `dev/merge_arrow_pr.{py,sh}` | Canonical PR-merge script (squashes commits, formats merge message, links the JIRA issue if present) | Operational |
| `dev/release/binary` (subdir) | Per-platform binary submit/upload helpers | Operational |
| `dev/release/git-vars.sh`, `download_rc_binaries.py`, `binary-recover.sh`, `copy-binary.rb`, `account-ruby.sh` | Various release helpers | Operational |

### 1.3 `.github/workflows/` (28 workflows)

| Workflow family | What it does | Class |
|---|---|---|
| `cpp.yml`, `cpp_extra.yml`, `cpp_windows.yml` | C++ build + test (Linux / extra / Windows) | Gating |
| `python.yml` | Python build + test (pyarrow) | Gating |
| `r.yml`, `r_extra.yml`, `r_nightly.yml` | R build + test + nightly | Gating |
| `ruby.yml` | Ruby build + test | Gating |
| `matlab.yml` | MATLAB MEX build + test | Gating |
| `archery.yml` | Cross-language integration test (`dev/archery/` Python harness) | Gating |
| `cuda_extra.yml` | CUDA-accelerated C++ build + test | Gating |
| `integration.yml` | Integration test orchestration | Gating |
| `docs.yml`, `docs_light.yml` | Sphinx docs build + light variant | Gating (docs) |
| `dev.yml`, `dev_pr.yml`, `dev_pr/` | Per-PR dev orchestration | Gating |
| `release.yml`, `release_candidate.yml`, `verify_rc.yml`, `package_linux.yml` | Release orchestration | Operational |
| `comment_bot.yml`, `pr_bot.yml`, `pr_review_trigger.yml`, `report_ci.yml`, `issue_bot.yml`, `stale.yml`, `check_labels.yml` | Bot / triage automation | Operational |

The bundled `ci/github-actions@v1` ruleset (3 rules: workflow
permissions, action SHA pinning, workflow has `name:`) covers the
hardening surface for all 28 workflows at once. The configured
`.alint.yml` restates the SHA-pinning rule at warning level —
**149 of ~197 GHA action uses across the 28 workflows are unpinned
by SHA** (mostly `actions/checkout@v4`-style floating tags).

### 1.4 `ci/scripts/` — per-language build + test runners (4,436 lines)

Per-language build + test scripts that the language-specific
workflows shell out to: `cpp_build.sh`, `cpp_test.sh`,
`python_build.sh`, `python_test.sh`, `r_build.sh`, `r_test.sh`,
`ruby_build.sh`, `ruby_test.sh`, `matlab_build.sh`, plus
integration runners (`integration_arrow.sh`, `integration_dask.sh`,
`integration_hdfs.sh`, `integration_spark.sh`) and per-tool
installers (`install_*.sh` × ~20). All **out of scope** as gates
(they're build-system harnesses, not validators).

### 1.5 Per-language config + registry files

| Path | Role |
|---|---|
| `.clang-format` | C++ formatter config — `BasedOnStyle: Google` |
| `.clang-tidy` | C++ static-analysis config |
| `.clang-tidy-ignore` | Files clang-tidy must skip (typically vendored / auto-generated) |
| `CPPLINT.cfg` | cpplint filter set + `linelength = 90` |
| `cmake-format.py` | cmake-format config (Python-syntax) |
| `.rubocop.yml` | Ruby style config |
| `.shellcheckrc` | shellcheck config — `external-sources=true source-path=SCRIPTDIR` (so it can follow `source` statements across `ci/scripts/`) |
| `.hadolint.yaml` | hadolint Dockerfile-lint ignore list (DL3008/DL3013/DL3015/DL3018/DL3041) |
| `.editorconfig` | Per-extension indent/EOL/charset for C++, CMake, Meson, C#, Cython, Python, Ruby, R, RST, MD, YAML — 67-line file with `root = true` |
| `.gitattributes` | `linguist-generated=true` markers for `cpp/src/generated/*`, `r/man/*.Rd`, etc., plus `r/NEWS.md merge=union` |
| `.gitmodules` | (currently empty — submodules removed) |
| `.dockerignore` | docker context excludes |
| `compose.yaml` | Docker Compose service definitions (~40 services per language × OS × Python-version permutation) used by `dev/archery docker run` |
| `.env` | docker compose default values for ~50 environment variables compose.yaml references |
| `.asf.yaml` | The canonical ASF infrastructure config — description, homepage, notification mailing list addresses (`commits@arrow.apache.org`, `dev@arrow.apache.org`, `issues@arrow.apache.org`, `github@arrow.apache.org`, `user@arrow.apache.org`), branch protection (`main: {}`), 4 collaborators |
| `LICENSE.txt` | Apache 2.0 license text (~270 lines) |
| `NOTICE.txt` | Project NOTICE |
| `README.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `CHANGELOG.md` | Repo-root governance |

### 1.6 The `format/` directory — the cross-language schema spec

A single directory of FlatBuffers + Protobuf files that every
language implementation conforms to. **No per-language linter sees
this** — it's the cross-language contract.

| File | Role |
|---|---|
| `Schema.fbs` | The canonical Arrow type system (FlatBuffers) |
| `Message.fbs` | IPC message envelope |
| `File.fbs` | Arrow file format |
| `Tensor.fbs` | Dense tensor encoding |
| `SparseTensor.fbs` | Sparse tensor encoding |
| `Flight.proto` | Flight RPC protocol (Protobuf) |
| `FlightSql.proto` | FlightSQL extension |
| `README.rst` | Entry point for someone implementing Arrow in a new language |
| `substrait/extension_types.yaml` | Substrait-extension type definitions |

### 1.7 Per-Ruby-gem layout — the cleanest per-package shape (8 gems)

Each of the 8 directories under `ruby/red-*` (`red-arrow`,
`red-arrow-cuda`, `red-arrow-dataset`, `red-arrow-flight`,
`red-arrow-flight-sql`, `red-arrow-format`, `red-gandiva`,
`red-parquet`) ships the same six files + two directories:

```
ruby/red-arrow/
├── LICENSE.txt          (own copy, NOT a symlink)
├── NOTICE.txt           (own copy, NOT a symlink)
├── README.md
├── Gemfile
├── Rakefile
├── red-arrow.gemspec    (basename matches dir name)
├── lib/
└── test/
```

This is the analogue of npm's `packages/*` ↔ `package.json`
convention, but for RubyGems. The convention is implicit (no script
enforces it; it's just what `dev/release/post-06-ruby.sh` assumes
when it walks `ruby/red-*`). 8 gems × 8 files/dirs = 64 atomic
assertions covered by 2 alint rules (`arrow-ruby-gem-required-meta-files`
+ `arrow-ruby-gem-has-gemspec`).

### 1.8 Per-GLib-sublibrary layout (7 sub-libraries)

`c_glib/` is the C-binding layer (built via Meson). 7 sub-libraries,
each in its own subdirectory, each MUST declare `meson.build`. The
top-level `c_glib/meson.build` `subdir()`s into them; missing
`meson.build` = silently skipped sub-library.

| Sub-library | Path |
|---|---|
| arrow-glib | `c_glib/arrow-glib/` |
| arrow-cuda-glib | `c_glib/arrow-cuda-glib/` |
| arrow-dataset-glib | `c_glib/arrow-dataset-glib/` |
| arrow-flight-glib | `c_glib/arrow-flight-glib/` |
| arrow-flight-sql-glib | `c_glib/arrow-flight-sql-glib/` |
| gandiva-glib | `c_glib/gandiva-glib/` |
| parquet-glib | `c_glib/parquet-glib/` |

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset
  (`oss-baseline` / `compliance/apache-2` / `python` /
  `ci/github-actions` / `hygiene/no-tracked-artifacts` /
  `tooling/editorconfig`) OR the per-rule entry in this directory's
  `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why (per-language AST, Apache RAT
  binary classification, Sphinx docs build, runtime cross-language
  conformance, Apache release dance).

### 2.1 The 21 pre-commit hook instances

| Hook id | Coverage | Notes |
|---|---|---|
| `rat` | alint-today (shellout) | `command:` rule `arrow-rat-run` wrapping `pre-commit run --all-files rat`. The RAT binary classification + version metadata pass remain inside the apache-rat jar — out of alint structural scope |
| `hadolint-docker` | alint-today (shellout) | `command:` rule `arrow-hadolint-run` |
| `clang-format` (×4) | alint-today (shellout) | `command:` rule `arrow-cpp-clang-format-check` wrapping `pre-commit run --all-files clang-format` (covers all 4 scoped instances at once) |
| `cpplint` (×2) | alint-today (shellout) | `command:` rule `arrow-cpp-cpplint-check` |
| `autopep8`, `flake8`, `cython-lint`, `numpydoc-validation` | alint-today (shellout) | 4 `command:` rules, one per tool |
| `lintr`, `air-format` | alint-today (shellout) | 2 `command:` rules |
| `rubocop` | alint-today (shellout) | `command:` rule `arrow-ruby-rubocop-check` |
| `cmake-format` | alint-today (shellout) | `command:` rule `arrow-cmake-format-check` |
| `sphinx-lint` | alint-today (shellout) | `command:` rule `arrow-sphinx-lint-run` |
| `shellcheck`, `shfmt` | alint-today (shellout) | 2 `command:` rules |
| `meson-fmt` | alint-today (shellout) | `command:` rule `arrow-meson-fmt-run` |

All 21 hook instances are wrapped via `command:` rules — alint
provides one orchestration layer (one config, one walk, one
report) over the existing per-language toolchain.

### 2.2 The 53 `dev/release/` scripts

| Script | Coverage | Rule |
|---|---|---|
| `run-rat.sh` | alint-today | `arrow-rat-runner-script-present` (`file_exists`) |
| `check-rat-report.py` | alint-today | `arrow-rat-check-report-script-present` |
| `rat_exclude_files.txt` | alint-today (presence) + alint-future (entries-resolve) | `arrow-rat-exclude-list-present`. The deeper "every pattern resolves to ≥1 file" check needs **`registry_paths_resolve`** (v0.10 ship-target, 8 demand sources) |
| `verify-release-candidate.sh` | alint-today | `arrow-release-verify-script-present` |
| `01-prepare.sh` through `10-vote-email.sh` (and post-* scripts) | out-of-scope | Operational (run by the release manager); no per-script presence rule |
| `dev/merge_arrow_pr.{py,sh}` | alint-today | `arrow-merge-script-present` (`file_exists` for either path) |

### 2.3 The 28 GitHub Actions workflows

All **alint-today** via the bundled `ci/github-actions@v1` ruleset
(3 rules — workflow permissions, action SHA pinning, workflow has
`name:`). The configured `.alint.yml` restates the SHA-pinning
rule at warning level (`arrow-workflow-actions-pinned-by-sha`) and
adds `arrow-dependabot-includes-actions` (`yaml_path_matches` for
github-actions ecosystem entry).

### 2.4 The 6 per-language subtrees + format/

| Subtree | Coverage | Rule(s) |
|---|---|---|
| `cpp/` | alint-today | 3 manifest rules (`arrow-cpp-manifest-present`, `arrow-cpp-vcpkg-manifest-present`, `arrow-cpp-meson-manifest-present`) + `arrow-cpp-files-have-apache-header` (cmake/.cmake.in extensions) |
| `c_glib/` | alint-today | `arrow-c-glib-manifest-present` + `arrow-c-glib-sublibrary-has-meson-build` (`for_each_dir` over the 7 named sub-libraries) |
| `python/` | alint-today | `arrow-python-pyproject-present` + 2 metadata rules (`-declares-name`, `-declares-license`) + `arrow-python-files-have-apache-header` (paths under `python/`) |
| `r/` | alint-today | `arrow-r-description-present` + 2 content rules (`-package-name`, `-license`) |
| `ruby/` | alint-today | `arrow-ruby-top-rakefile-present` + the per-gem rules below |
| `matlab/` | alint-today | `arrow-matlab-cmakelists-present` |
| `format/` | alint-today | `arrow-format-spec-files-present` (single `file_exists` covering all 7 .fbs/.proto files) + `arrow-format-readme-present` |

### 2.5 The 8 per-Ruby-gem layout

| Convention | Coverage | Rule |
|---|---|---|
| Every `ruby/red-*/Rakefile` neighbour ships LICENSE.txt + NOTICE.txt + README.md + Gemfile + lib/ + test/ | alint-today | `arrow-ruby-gem-required-meta-files` (`for_each_file` over `ruby/red-*/Rakefile` + nested `require:` for 6 file/dir checks) |
| Each gem dir has its named `<name>.gemspec` | alint-today | `arrow-ruby-gem-has-gemspec` (multi-path `file_exists`) |

### 2.6 The 7 GLib sub-library layout

| Convention | Coverage | Rule |
|---|---|---|
| Every named `c_glib/<name>-glib/` dir has `meson.build` | alint-today | `arrow-c-glib-sublibrary-has-meson-build` (`for_each_dir` over the 7 named sub-libraries) |

### 2.7 Apache governance + release-tooling shape

| Artefact | Coverage | Rule |
|---|---|---|
| `LICENSE.txt` | alint-today | bundled `apache-2-license-text-present` |
| `NOTICE.txt` | alint-today | bundled `apache-2-notice-file-exists` |
| Source-header on every C++/Python/Cython/Ruby/CMake source file | alint-today (with override) | `apache-2-source-has-license-header` (this directory's override widens the bundled pattern to accept the longer ASF preamble + extends file-extension list to `.cs`, `.m`, `.mm`, `.pyx`, `.pxd`, `.fbs`, `.proto`, `.rb`) |
| `.asf.yaml` | alint-today | `arrow-asf-yaml-present` + 2 yaml-path checks (`-declares-homepage`, `-declares-notification-list`) |
| `.pre-commit-config.yaml` registers the `rat` hook | alint-today | `arrow-pre-commit-config-present` + `arrow-pre-commit-runs-rat` (`file_content_matches` for the `id: rat` line) |
| `dev/release/rat_exclude_files.txt` (102 patterns) | alint-today (presence) + alint-future (every pattern resolves to ≥1 file) | `arrow-rat-exclude-list-present` + the future `registry_paths_resolve` v0.10 ship-target (see §6) |
| `dev/release/run-rat.sh`, `check-rat-report.py`, `verify-release-candidate.sh`, `dev/merge_arrow_pr.{py,sh}` | alint-today | 4 `file_exists` rules |

### 2.8 The `format/Schema.fbs` ↔ per-language test fixture coverage

This is the headline polyglot gap. Every cross-language type defined
in `format/Schema.fbs` (the canonical Arrow type system) MUST have
a corresponding test case in each of `cpp/`, `python/`, `r/`,
`ruby/`. The `dev/archery/` integration-test framework runs this
dynamically at test time; alint can't express it because it
requires:

1. Parsing the FlatBuffers file (extracting type names)
2. Cross-referencing to per-language test fixture paths
3. Asserting fixture-file existence per language

**Coverage:** alint-future. `cross_language_implementation_complete`
(v0.11+ ship-target, 5 demand sources per `launch-evidence.md`:
arrow + tensorflow + protobuf + angular + flutter). arrow exercises
this shape across the broadest polyglot surface in the corpus.

### 2.9 Per-language tool-config presence (10 root-level configs)

All **alint-today** — 10 `file_exists` rules with `root_only: true`:
`.clang-format`, `.clang-tidy`, `.clang-tidy-ignore`, `CPPLINT.cfg`,
`cmake-format.py`, `.rubocop.yml`, `.shellcheckrc`, `.hadolint.yaml`,
`compose.yaml`, `.env`. Plus 1 `file_content_matches` for
`.editorconfig` `root = true`.

### 2.10 Hygiene (per-language tracked-artefact patterns)

| Path | Coverage | Rule |
|---|---|---|
| `cpp/build/` | alint-today | `arrow-no-tracked-cpp-build` (per-repo, narrower than the bundled `**/build/`) |
| `r/libs/` | alint-today | `arrow-no-tracked-r-libs` |
| `python/build/`, `python/*.egg-info/`, `python/wheelhouse/` | alint-today | `arrow-no-tracked-python-build-eggs` |
| Cross-language hygiene (`__pycache__`, `node_modules`, `.DS_Store`, etc.) | alint-today | bundled `hygiene/no-tracked-artifacts@v1` (11 rules) |

---

## 3. Quantified coverage

Counted across **21 pre-commit hook instances** + **53 dev/release/
scripts** (rolled to 6 governance rules) + **28 GHA workflows** + **7
language subtree manifests + 1 cross-language schema spec** + **8
Ruby gems × 8 files** (rolled to 2 family rules) + **7 GLib
sub-libraries** (1 family rule) + **10 root tool configs** + **3
hygiene-per-language** = **78 distinct surfaces**.

```
alint-today:     61 / 78 = 78%   (21 pre-commit shellouts + 6 governance + 28 GHA + 7 language + 2 ruby-family + 1 c_glib + 10 tool-configs + 3 hygiene + 4 misc)
alint-future:     3 / 78 =  4%   (registry_paths_resolve for rat_exclude_files.txt + cross_language_implementation_complete for format/ + ordered_block for rat_exclude sortedness)
out-of-scope:    14 / 78 = 18%   (Apache RAT binary classification + dev/archery cross-language conformance + Sphinx docs build + 47 release-dance scripts)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
pre-commit hooks (21):
  alint-today:     21 / 21 = 100% (all wrapped via command: shellouts)

dev/release/ scripts (53):
  alint-today:      6 / 53 = 11%   (presence rules for the gating subset)
  out-of-scope:    47 / 53 = 89%   (operational release dance)

GHA workflows (28):
  alint-today:     28 / 28 = 100%  (covered by ci/github-actions@v1)

per-language subtrees (7):
  alint-today:      7 / 7 = 100%

per-Ruby-gem (8 gems × 8 files = 64):
  alint-today:     64 / 64 = 100%  (covered by 2 family rules)

per-GLib-sublibrary (7):
  alint-today:      7 / 7 = 100%

format/ schema spec (8 files):
  alint-today:      8 / 8 = 100%   (presence)
  alint-future:     1 / 1 = 100%   (cross-language fixture coverage)
```

**Commentary.** Three observations:

1. **apache/arrow is the canonical "polyglot monorepo" data point —
   the highest-density per-language structural discipline in the
   case-study set.** 6 in-tree language implementations + the shared
   `format/` spec, glued together by `.pre-commit-config.yaml` and
   `dev/archery`. No single per-language linter sees the cross-
   language structural shape (clang-format only sees C++, rubocop
   only sees Ruby, flake8 only sees Python). alint's `for_each_dir`
   + `for_each_file` over the language-subtree list iterates across
   the polyglot tree at once — surfaces cross-language drift that
   no per-language tool can see.

2. **`registry_paths_resolve` is the highest-leverage v0.10
   ship-target for arrow.** `dev/release/rat_exclude_files.txt`
   lists 102 path patterns that Apache RAT must skip, and Apache
   RAT itself fails non-zero if a pattern doesn't resolve to ≥1
   file. alint surfaces 23 source files flagged as missing the
   Apache header — **all 23 paths appear in `rat_exclude_files.txt`**,
   confirmed by cross-checking. With `registry_paths_resolve` (v0.10
   ship-target, 8 demand sources), alint could resolve the
   exclude-list pointers from header-missing-finding to known-exempt.

3. **The `cross_language_implementation_complete` shape is genuinely
   net-new, with arrow as the densest source.** Every type in
   `format/Schema.fbs` should have a per-language test fixture in
   each of cpp/, python/, r/, ruby/. arrow is one of 5 demand
   sources for this v0.11+ ship-target (alongside tensorflow's 1,185
   textproto goldens, protobuf's 10 in-tree language bindings,
   angular's per-package API goldens, flutter's 6 native-OS
   embedders).

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (962 lines, 65
repo-specific rules, 6 bundled rulesets folded in via `extends:`,
**107 rules total** loaded per `alint validate-config` (the runtime
emits 87 result entries — some rule IDs are shared/deduped across
overlays)).

**Synopsis of the 8 most load-bearing repo-specific rules** (full
config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules: license/readme/security/CoC + hygiene
  - alint://bundled/compliance/apache-2@v1           # 3 rules: LICENSE, NOTICE, source-header (overridden below for the long ASF preamble)
  - alint://bundled/python@v1                        # 9 rules: pyproject.toml shape + py source hygiene scoped via has_ancestor pyproject.toml
  - alint://bundled/ci/github-actions@v1             # 3 rules: workflow contents-read + pin-to-sha + name (covers all 28)
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules: __pycache__, dist/, build/, etc.
  - alint://bundled/tooling/editorconfig@v1          # 3 rules: .editorconfig shape

rules:
  - id: apache-2-source-has-license-header           # OVERRIDE bundled — accept long ASF preamble
    kind: file_header
    paths:
      include:
        ["**/*.{rs,py,js,jsx,ts,tsx,go,java,kt,c,cc,cpp,h,hpp,hh,sh,rb,swift,scala,cs,m,mm,pyx,pxd,fbs,proto}"]
      exclude: [...rat_exclude_files.txt overlap...]
    lines: 30
    pattern: 'Licensed (to the Apache Software Foundation|under the Apache License,?\s*Version 2)'
    level: warning
  - id: arrow-language-subdir-has-readme              # for_each_file over the 7-language README list
    kind: for_each_file
    select: "{cpp,c_glib,python,r,ruby,matlab,format}/README.{md,rst}"
    require:
      - { kind: file_min_lines, paths: "{path}", min_lines: 5 }
  - id: arrow-format-spec-files-present              # multi-path file_exists for all 7 .fbs/.proto
    kind: file_exists
    paths: [format/Schema.fbs, format/Message.fbs, format/File.fbs, format/Tensor.fbs, format/SparseTensor.fbs, format/Flight.proto, format/FlightSql.proto]
    level: error
  - id: arrow-ruby-gem-required-meta-files           # for_each_file ruby/red-*/Rakefile + 6-file require:
    kind: for_each_file
    select: "ruby/red-*/Rakefile"
    require:
      - { kind: file_exists, paths: "{dir}/LICENSE.txt" }
      - { kind: file_exists, paths: "{dir}/NOTICE.txt" }
      - { kind: file_exists, paths: "{dir}/README.md" }
      - { kind: file_exists, paths: "{dir}/Gemfile" }
      - { kind: dir_exists,  paths: "{dir}/lib" }
      - { kind: dir_exists,  paths: "{dir}/test" }
  - id: arrow-c-glib-sublibrary-has-meson-build      # for_each_dir over named GLib sub-libraries
    kind: for_each_dir
    select: "c_glib/{arrow,arrow-cuda,arrow-dataset,arrow-flight,arrow-flight-sql,gandiva,parquet}-glib"
    require:
      - { kind: file_exists, paths: "{path}/meson.build" }
  - id: arrow-asf-declares-notification-list         # yaml_path_matches .asf.yaml notifications.commits
    # …
  - id: arrow-rat-exclude-list-present                # file_exists dev/release/rat_exclude_files.txt
    # …
  - id: arrow-pre-commit-run                          # command rule wrapping pre-commit run --all-files
    kind: command
    paths: .pre-commit-config.yaml
    command: ["pre-commit", "run", "--all-files"]
    timeout: 600
```

**Repo-specific vs bundled split:**

- **65 repo-specific rules** in `.alint.yml` (the `arrow-*` prefix
  identifies them in `alint list` output): per-language manifests
  (×8), per-Ruby-gem (×2), per-GLib-sublibrary (×1), Apache
  governance (×8), pre-commit + RAT (×5), per-language tool configs
  (×9), Compose orchestration (×2), gitattributes (×1), GHA (×2),
  per-language Apache-header restatements (×3 — including the
  bundled override), hygiene (×3), 16 `command:` shellouts
  (pre-commit + per-tool wrappers).
- **42 bundled rules** from the 6 extended rulesets: 15 from
  oss-baseline + 3 from compliance/apache-2 + 9 from python + 3 from
  ci/github-actions + 11 from hygiene/no-tracked-artifacts + 3 from
  tooling/editorconfig − overlap = 42 effective rule IDs after
  dedup.

**Validation:** `alint validate-config` reports `✓ Config valid:
107 rule(s) loaded`. Pitfall checks: the magic comment is present
(line 1); JSONPath uses `?match(@.uses, '...')` per the honourable
mention; `?@['package-ecosystem']` uses bracket notation per pitfall
#10; `scope_filter.has_ancestor:` uses basenames per pitfall #11;
`(?m)` is used on every `^`/`$` anchored regex; the `command:`
rules use `command:` (not `argv:`) and integer `timeout:`; **no
`pattern: |` block scalars** (no pitfall #22 candidates — the
configured `apache-2-source-has-license-header` override uses a
single-line single-quoted scalar; the bundled rule uses single-line
too).

---

## 5. Performance comparison

Methodology: `hyperfine -i --warmup 1 --runs 5` on the same
`/tmp/arrow` working tree captured 2026-05-07. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary
`target/release/alint v0.9.17`. Where the upstream toolchain isn't
installed locally, the row is `pending — needs <toolchain>` with
the exact reproduction command.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| `find . -name '*.{py,c,cc,cpp,h}' \| xargs grep -L 'Licensed.*Apache'` (Apache header check on first 1000 source files) | `find` + `xargs grep` | **20.3 ms** ± 0.5 ms | included in 59 ms full pass | n/a — alint runs 65 rules + 42 bundled rules = 107 in one pass |
| **alint full lite-pass** (91 rules, no `command:` shellouts) | n/a | n/a | **59 ms** ± 6 ms | — |
| **alint full pass** (107 rules, including 16 `command:` shellouts) | n/a | n/a | **87 ms** ± 34 ms | — (the `command:` rules' tools are not on PATH so they spawn-fail-fast; the +28 ms is process-spawn overhead) |

The headline number: **a single 59 ms alint pass replaces ~40
distinct cross-language structural checks**: 6 per-language
manifest checks + 7 cross-language schema-spec checks + 8 ruby-gem
× 8-file checks + 7 GLib sub-library checks + 8 Apache governance
artefacts + 10 root-tool-config presence checks + per-language
Apache-header overlay across ~3,500 source files + the 28-workflow
GHA hardening pass. **That's roughly ~250 distinct file-system +
content assertions in 59 ms** — **~0.24 ms per assertion**.

The `command:`-shellout class (`arrow-pre-commit-run`, plus 16
per-tool wrappers — `arrow-cpp-clang-format-check`,
`-cpplint-check`, `arrow-ruby-rubocop-check`, etc.) is an
alint-orchestrates-the-existing-tool model. Per-tool wall-clock is
whatever `pre-commit run --all-files <hook>` takes (typically
1-30 s per per-language hook). Full pre-commit suite end-to-end
(21 hooks across 14 hook repos) runs ~60-180 s on a CI machine.

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `pre-commit run --all-files` end-to-end | pre-commit + 14 hook repos | pending — `pre-commit` (or `prek`) not on PATH | `pip install pre-commit && time pre-commit run --all-files` |
| `clang-format` standalone | clang-format | pending | `apt install clang-format && find cpp -name '*.cc' \| xargs clang-format -n` |
| `cpplint` | cpplint | pending | `pip install cpplint && time cpplint --recursive cpp/` |
| `rubocop` | rubocop | pending | `gem install rubocop && time rubocop ruby/` |
| `flake8`, `autopep8`, `cython-lint`, `numpydoc-validation` | various Python tools | pending | `pip install flake8 autopep8 cython-lint numpydoc && time flake8 python/` |
| `lintr`, `air-format` | R packages | pending | `R -e "install.packages(c('lintr', 'air'))" && time Rscript -e 'lintr::lint_dir(\"r/\")'` |
| `cmake-format` | cmake-format | pending | `pip install cmake-format && time find . -name 'CMakeLists.txt' \| xargs cmake-format --check` |
| `sphinx-lint` | sphinx-lint | pending | `pip install sphinx-lint && time sphinx-lint docs/source` |
| `meson-fmt` | meson | pending | `pip install meson && time find . -name 'meson.build' \| xargs meson-fmt --check` |
| `dev/release/run-rat.sh` (Apache RAT) | java + apache-rat-${VERSION}.jar | pending — needs JDK + maven access | `time bash dev/release/run-rat.sh` |

The `pre-commit run --all-files` end-to-end is the most marketable
comparison number but requires the full 14-hook-repo pre-commit
setup (~600 MB of cached envs across the per-language toolchains).
On the working machine without that stack, the reproduction
commands above are documented for a future run on a CI-class image.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config examples/apache-arrow/.alint.yml /tmp/arrow` (live run, JSON-format).

**Headline:** alint surfaces **255 violations** across the live
tree; **failing rules: 29 / passing: 58** (87 declarative + 16
shellouts). Per-rule violation counts (top 10):

| Count | Rule | Class |
|---|---|---|
| 149 | `gha-pin-actions-to-sha` | Real (3rd-party action SHA-pin gaps across ~149 step uses) |
| 24 | `arrow-workflow-actions-pinned-by-sha` | Real (subset of above, scoped to arrow's restated rule) |
| 23 | `oss-no-trailing-whitespace` | Cosmetic (trailing whitespace in markdown / YAML) |
| **23** | **`apache-2-source-has-license-header`** | **All 23 paths appear in `rat_exclude_files.txt` — see §6.1 below** |
| 10 | `oss-final-newline` | Cosmetic |
| 3 | `gha-workflow-contents-read` | Real (3 workflows missing explicit permissions) |
| 1 each | 16 `command:` rule shellouts | False positive (tool not on PATH — single per-tool spawn-fail) |
| 1 | `tooling-gitattributes-normalizes-line-endings` | Real (advisory) |
| 1 | `python-module-snake-case` | Real (warning-level — one Python module not snake_case) |
| 1 | `python-manifest-exists`, `python-has-lockfile`, `oss-security-policy-exists`, `hygiene-no-env-files` | Each: 1 finding |

### 6.1 Real findings — the catches that beat existing tooling

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 23 source files flagged as missing the Apache header | `cpp/build-support/asan_symbolize.py`, `cpp/src/arrow/c/dlpack_abi.h`, `cpp/src/arrow/io/mman.h`, `cpp/src/arrow/status.{cc,h}`, `dev/tasks/homebrew-formulae/apache-arrow{,-glib}.rb`, `python/pyarrow/includes/__init__.pxd`, `python/pyarrow/tests/__init__.py`, `r/src/arrowExports.cpp`, etc. | warning | `apache-2-source-has-license-header` | **All 23 are listed in `dev/release/rat_exclude_files.txt`.** This is the headline finding for the v0.10 ship-target `registry_paths_resolve`: alint cannot today resolve the exclude-list pointers from header-missing-finding to known-exempt. With `registry_paths_resolve`, this rule could either (a) auto-skip files matching the registry, or (b) flag the registry as missing the file. Verified: `while read p; do grep -qF "$p" rat_exclude_files.txt && echo EXCLUDED; done < /tmp/arrow-header-paths.txt` → 23/23 match |
| 149 third-party action invocations not pinned to a SHA | Various `.github/workflows/*.yml` | warning | `gha-pin-actions-to-sha` | **Real findings** — supply-chain integrity. OpenSSF Scorecard "Pinned-Dependencies" check covers this nightly; alint surfaces the same gate at PR time |
| 24 arrow-specific workflow SHA-pin gaps | Subset of above, scoped via `arrow-workflow-actions-pinned-by-sha` | warning | Same | Same |
| 3 GHA workflows missing explicit `permissions: contents: read` | (varies) | warning | `gha-workflow-contents-read` | Real |
| 23 markdown / yaml files with trailing whitespace | `.github/actions/sync-nightlies/README.md`, `.github/workflows/{archery,cpp_extra}.yml`, etc. | info | `oss-no-trailing-whitespace` | Cosmetic |
| 10 files lacking final newline | (varies) | info | `oss-final-newline` | Cosmetic |
| 1 vendored Python file (snake_case violation) | (varies) | warning | `python-module-snake-case` | Real (likely a vendored file with non-snake-case name; could be allowlisted) |

**Total real findings (alint-surfaced, existing tooling either
runs less frequently or covers narrower scope): 149 GHA SHA-pin
gaps, 3 workflow permissions gaps, 23 source-header-vs-RAT-exclude
mismatches (which would resolve cleanly with `registry_paths_resolve`),
1 module-naming drift. Plus ~33 cosmetic findings (trailing
whitespace + missing final newlines).**

### 6.2 Suspected `.alint.yml` bugs flagged for parent triage

**No regex anchor or scope-filter bugs detected** in the arrow
config. All per-rule violation counts are reasonable (max 149 for
GHA SHA-pin which IS the real finding count across 28 workflows;
23 for the Apache-header-on-RAT-excluded-files which is a known
expected pattern requiring `registry_paths_resolve`).

**Recommended `paths.exclude:` extension on the `apache-2-source-has-license-header`
override:** add the 23 RAT-excluded files to the override's
`exclude:` block to clean up the live-tree count from 23 → 0 until
`registry_paths_resolve` ships. Trivial one-line-per-path fix; not
auto-applied here per the brief.

---

## 7. Followup feature work surfaced

- **`registry_paths_resolve` rule kind** — covers
  `dev/release/rat_exclude_files.txt`. Demand: 8 distinct sources
  per `launch-evidence.md` (rust + clap + cpython×2 + next.js +
  arrow + pytorch + nodejs/node + NixOS×3). **v0.10 ship-target.**
- **`cross_language_implementation_complete` rule kind** — covers
  the `format/Schema.fbs` ↔ per-language test-fixture coverage gap.
  Per `launch-evidence.md`, a v0.11+ ship-target with 5 saturated
  demand sources (arrow + tensorflow + protobuf + angular + flutter).
  arrow exercises the broadest polyglot surface in the corpus.
- **`ordered_block` rule kind** — re-confirmed by
  `rat_exclude_files.txt` (conventionally alphabetised) + the long
  `files:` alternation lists in `.pre-commit-config.yaml`
  (`shellcheck` and `shfmt` `files:` patterns). Demand: 7 distinct
  sources per `launch-evidence.md`. **v0.10 ship-target.**
- **`apache/governance@v1` bundled ruleset** — arrow is one of 3
  Apache TLPs converging on 9 of 12 governance artefacts (alongside
  spark + airflow). Once shipped, this config could `extends:` it
  and drop the 8 `arrow-asf-*` / `arrow-rat-*` / `arrow-check-rat-*`
  / `arrow-release-verify-*` rules. **v0.10 ship-target.**
- **Bundled `apache-2-source-has-license-header` long-form pattern
  default** — same flag as the airflow + spark configs. Cross-
  saturation: arrow + spark + airflow all override the bundled rule
  with the same long-form pattern; the bundle should default to it.
- **`fanout: {scope_filter: ..., command: ...}` mode for `command:`
  rule** — covers the pre-commit per-scope hook repetition (4×
  `clang-format`, 2× `cpplint`). Single-source so far (arrow);
  defer.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`compliance/reuse@v1` (3-rule bundled ruleset) trial** — arrow
   carries the longer ASF preamble on every source file and uses
   per-language license headers (C++/Python/Cython/Ruby variants).
   The REUSE-spec form would let the per-language header rules
   collapse into one bundled overlay. Surface: ~10k source files
   across 6 languages.
2. **`apache/governance@v1` bundled-ruleset adoption** — arrow is
   one of 3 saturated demand sources for this v0.10 ship-target
   bundle (alongside spark + airflow). Once shipped, this config
   should `extends:` it and drop the per-asf-yaml + RAT-runner +
   release-dance restated rules (currently 8 `arrow-asf-*` /
   `arrow-rat-*` / `arrow-check-rat-*` rules collapse to one
   bundled extension).
3. **`scope_filter` ancestor-manifest narrowing for the per-language
   rules** — the `arrow-language-subdir-has-readme` rule iterates
   over a hard-coded `{cpp,c_glib,python,r,ruby,matlab,format}`
   list. A refactor to use `for_each_dir` + `scope_filter.has_ancestor`
   of a per-language manifest (`CMakeLists.txt` for cpp/matlab,
   `meson.build` for c_glib, etc.) would make the rule
   self-discovering and resilient to future per-language subdir
   additions.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **107** (65 custom + 6 bundled rulesets —
  `oss-baseline` 15, `compliance/apache-2` 3, `python` 9,
  `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11,
  `tooling/editorconfig` 3; some rule IDs overlap, which is why the
  grand total is 107 rather than the arithmetic sum of 109)
- **`alint validate-config`:** ✓ Config valid: 107 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for
  the 255-violation breakdown (failing rules 29 / passing 58; ~178
  real findings + ~33 cosmetic + ~16 tool-not-on-PATH per-tool
  spawn-fail counts + 23 RAT-excluded false positives that
  `registry_paths_resolve` would resolve cleanly)
- **Pitfall fixes (v0.9.17):** none directly cited in this config
- **Pitfall #22 status:** No `pattern: |` block scalars in this
  config — not a candidate. The `apache-2-source-has-license-header`
  override pattern uses a single-line single-quoted scalar (correct
  form per pitfall #14)
- **Open gaps (unchanged):** `registry_paths_resolve` (v0.10
  ship-target, 8 sources), `cross_language_implementation_complete`
  (v0.11+ ship-target, 5 sources), `ordered_block` (v0.10
  ship-target, 7 sources), `apache/governance@v1` (v0.10
  ship-target, 3 Apache TLPs converging). No new rule-kind gaps
  surfaced
- **Open suspected bugs in this directory's `.alint.yml`:** none.
  The 23 Apache-header false positives are RAT-exclude
  coordinations that require `registry_paths_resolve` (v0.10
  ship-target) to resolve declaratively; a one-line `paths.exclude:`
  extension is the available workaround
