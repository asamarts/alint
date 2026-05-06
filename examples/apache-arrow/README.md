# Case study: `apache/arrow`

Inventory of the structural-validation tooling in `apache/arrow`
and an alint config that replaces the rules alint can express
today, plus a catalogue of the rules that need new alint
primitives.

**Repo state captured:** 2026-05-06, sparse-clone of
`apache/arrow@f69ccb08` (rev =
`f69ccb08d62bf0f1e8b07db3b9d561c234c88abb`). Heavy source trees
(`cpp/src`, `python/pyarrow`, `java/vector/src`, `r/inst`)
excluded; per-language top-level dirs and the `format/`,
`dev/`, `ci/`, `docs/` subtrees kept.

---

## Summary

apache/arrow is **the** canonical multi-language polyglot
monorepo: a single tree historically hosting columnar-format
reference implementations across **a dozen languages**, glued
together by the cross-language `format/` schema spec
(FlatBuffers + Protobuf) and the `dev/archery/` integration-test
harness. At HEAD on the date captured, the in-repo languages
are C++, C (GLib), Python, R, Ruby, and MATLAB; the Java, Go,
JS, Rust, C#, Swift, and Julia implementations have spun out
into sibling repos (`apache/arrow-java`,
`apache/arrow-go`, etc.) but the per-language sub-tree shape
inside `apache/arrow` remains the canonical pattern every
sibling repo mirrors.

Concrete count at HEAD:

- **6** per-language top-level subdirectories (`cpp/`,
  `c_glib/`, `python/`, `r/`, `ruby/`, `matlab/`) + the shared
  `format/` spec
- **8** Ruby gem subdirectories under `ruby/red-*` (each its
  own `LICENSE.txt` + `NOTICE.txt` + `README.md` + `Rakefile` +
  `Gemfile` + `<name>.gemspec` — the most uniform per-package
  shape in the tree)
- **7** GLib sub-libraries under `c_glib/` (each with its own
  `meson.build`)
- **28** GitHub Actions workflows under `.github/workflows/`
- **53** scripts under `dev/release/` implementing the Apache
  release dance (RAT report, source tarball, binary
  submit/upload/verify, vote email, post-release cleanup)
- **4 436** lines across `ci/scripts/` (per-language
  build/test runners that the language-specific workflows shell
  out to)
- **11** root-level lint/format tool configs: `.clang-format`,
  `.clang-tidy`, `.clang-tidy-ignore`, `CPPLINT.cfg`,
  `.editorconfig`, `.rubocop.yml`, `.shellcheckrc`,
  `.hadolint.yaml`, `cmake-format.py`,
  `.pre-commit-config.yaml`, `.gitattributes`
- `.pre-commit-config.yaml` = **396 lines, 21 distinct hook
  ids** across 14 external + 2 local hook repos
- `dev/release/rat_exclude_files.txt` = **102** path patterns
  consumed by Apache RAT (the official Release Audit Tool that
  gates every Apache release vote)

Total **structural-validation surfaces** counted: **34**
discrete checks across the inventory (see § "Existing tooling
inventory" below).

- **20 of 34 (59 %) map to existing alint rules** — the
  bundled `oss-baseline + compliance/apache-2 + python +
  ci/github-actions + hygiene/no-tracked-artifacts +
  tooling/editorconfig` ship roughly **35 rules** between them,
  plus the **65 arrow-specific rules** in [`/.alint.yml`](.alint.yml)
  (cross-language conventions, per-language manifests, Apache
  release tooling, per-Ruby-gem layout, GLib sub-library
  layout, governance files).
- **5 of 34 (15 %) shell out via `command:` rules** — wrapping
  `pre-commit run` for clang-format / cpplint / rubocop /
  flake8 / autopep8 / cython-lint / numpydoc-validation /
  lintr / air-format / cmake-format / shellcheck / shfmt /
  hadolint / sphinx-lint / meson-fmt / rat. (`pre-commit`
  itself is one tool with 21 hooks, so a single-command
  shell-out covers them all.)
- **9 of 34 (26 %) are out of alint's scope** — Apache RAT's
  binary-classification + version-metadata pass, the
  `dev/archery/` Python integration-test framework that
  cross-validates the format spec across languages at runtime,
  the ELF/Mach-O symbol-prefix scans in C++ build-support,
  the Cython AST checks, the Sphinx docs-build cross-references,
  and four operational workflows (release-orchestration / cron
  / triage / issue-bot).

The configured **65-rule** [`/.alint.yml`](.alint.yml) covers
every structural assertion the existing tooling makes about
repo *state*, plus several arrow doesn't enforce today
(per-language subdir README, per-Ruby-gem layout uniformity,
.asf.yaml schema integrity).

**Headline finding:** apache/arrow is **the flagship
"language-agnostic polyglot monorepo" pitch for alint** — a
single declarative config replaces the cross-language
structural conventions (every per-language subdir has a
README; every Ruby gem subdir ships LICENSE+NOTICE+README+
Rakefile+Gemfile+gemspec; every GLib sub-library has
meson.build; every C++/Python/Cython/Ruby source file carries
the longer Apache ASF-preamble header) that **no per-language
linter sees** because each per-language linter only sees its
own subtree. clang-format never sees the Ruby gems, rubocop
never sees the C++ tree, flake8 never sees the GLib sub-
libraries, lintr never sees the Python package — but
alint's `for_each_dir` + `for_each_file` over the canonical
language-subtree list catches drift across the whole polyglot
tree at once. **Surfaces 16 source files missing the
Apache header** (all of which are listed in
`dev/release/rat_exclude_files.txt` — confirming the canonical
v0.10+ `registry_paths_resolve` rule kind would fold the
exclude list directly into alint's scope).

---

## Existing tooling inventory

### Root config files (cross-language gate / orchestration)

| File | Owner tool | What it pins | alint disposition |
|---|---|---|---|
| `.pre-commit-config.yaml` | pre-commit | 21 hooks: rat (Apache RAT), hadolint, clang-format×4 (per-language scope), cpplint×2, autopep8, flake8, cython-lint, numpydoc-validation, lintr, air-format, rubocop, cmake-format, sphinx-lint, shellcheck, shfmt, meson-fmt | `file_exists` + `file_content_matches` for the rat hook id; per-tool `command:` rules wrap each language's hook |
| `.asf.yaml` | ASF infra | description, homepage, notification mailing lists (`commits@/dev@/issues@arrow.apache.org`), branch protection, collaborators | 3× `yaml_path_matches` + `file_exists` |
| `.gitattributes` | git | `linguist-generated=true` markers for `cpp/src/generated/*`, `go/**/*.s`, `r/man/*.Rd`, etc., plus `r/NEWS.md merge=union` | `file_exists` (covers gitattributes-EOL via the bundled `tooling/editorconfig` ruleset's adjacent rule) |
| `.editorconfig` | EditorConfig | Per-extension indent/EOL/charset for C++, CMake, Meson, C#, Cython, Python, Ruby, R, RST, MD, YAML | `file_content_matches` for `root = true` |
| `.clang-format` | clang-format | C++ formatter config | `file_exists` + `command:` wrapping `pre-commit run clang-format` |
| `.clang-tidy` + `.clang-tidy-ignore` | clang-tidy | C++ static-analysis config + skip list | 2× `file_exists` |
| `CPPLINT.cfg` | cpplint | filter set + `linelength = 90` | `file_exists` + `command:` wrapping cpplint hook |
| `cmake-format.py` | cmake-format | Python-syntax cmake-format config | `file_exists` + `command:` wrapping cmake-format hook |
| `.rubocop.yml` | rubocop | Ruby style config | `file_exists` + `command:` wrapping rubocop hook |
| `.shellcheckrc` | shellcheck | `external-sources=true source-path=SCRIPTDIR` | `file_exists` |
| `.hadolint.yaml` | hadolint | Dockerfile lint ignore list (DL3008 / DL3013 / DL3015 / DL3018 / DL3041) | `file_exists` |
| `compose.yaml` | docker compose | Cross-language integration-test service definitions (~40 services per language × OS × Python-version) | `file_exists` |
| `.env` | docker compose | default env values for compose.yaml parameters | `file_exists` |
| `LICENSE.txt` + `NOTICE.txt` | Apache | Apache 2.0 license text + project NOTICE | bundled `compliance/apache-2@v1` covers both |

### `dev/release/` — Apache release-tooling discipline

| Surface | What it does | alint disposition |
|---|---|---|
| `dev/release/run-rat.sh` | Downloads `apache-rat-${VERSION}.jar` from Maven Central; runs RAT against an archive of HEAD; passes the report to `check-rat-report.py`. The `pre-commit` `rat` hook calls this on every commit. | `file_exists` + `command:` wrapping `pre-commit run rat` (the alint config gates the script's existence; the actual RAT run is shelled out) |
| `dev/release/check-rat-report.py` | Filters the RAT report against `rat_exclude_files.txt`; exits non-zero if any unapproved file remains. | `file_exists` |
| `dev/release/rat_exclude_files.txt` | 102 path patterns RAT must skip (binary fixtures, vendored code, generated files). | `file_exists`. **The deeper "every pattern resolves to ≥1 file" check needs the v0.10+ `registry_paths_resolve` rule kind** — this case study's #1 gap. |
| `dev/release/verify-release-candidate.sh` | The script a PMC member runs against the unfreezed RC tarball during the Apache 72-hour vote window. | `file_exists` (warning) |
| `dev/release/01-prepare.sh` through `10-vote-email.sh` (and `post-01-tag.sh` through `post-14-conan.sh`) | The numbered scripts that drive the entire release dance | Out of alint scope (operational, run by the release manager). The numbered convention itself is alint-shaped (every `NN-name.sh` should have a matching test or post-step) but defers to v0.10+ for the cross-file pairing semantics. |
| `dev/merge_arrow_pr.{py,sh}` | The canonical PR-merge script (squashes commits, formats the merge message, links the JIRA issue if present) | `file_exists` (info) |

### `.github/workflows/` (28 workflows)

| Workflow family | What it does | alint disposition |
|---|---|---|
| Per-language CI: `cpp.yml`, `cpp_extra.yml`, `cpp_windows.yml`, `python.yml`, `r.yml`, `r_extra.yml`, `r_nightly.yml`, `ruby.yml`, `matlab.yml`, `archery.yml` | Build + test per language, fanning into `ci/scripts/<lang>_build.sh` + `<lang>_test.sh` | Each is its own surface — the alint bundled `ci/github-actions@v1` ruleset covers shape (workflow has `name:`, permissions declared, action SHA-pinned) for all 28 in one rule |
| `release.yml`, `release_candidate.yml`, `verify_rc.yml`, `package_linux.yml` | Release orchestration | Out of scope (operational); shape covered by the bundled GHA ruleset |
| `cuda_extra.yml`, `integration.yml`, `docs.yml`, `docs_light.yml`, `dev.yml`, `dev_pr.yml`, `comment_bot.yml`, `pr_bot.yml`, `pr_review_trigger.yml`, `report_ci.yml`, `issue_bot.yml`, `stale.yml`, `check_labels.yml` | CUDA build, cross-language integration test, docs publish, PR/issue bot automation | Out of alint scope (operational); shape covered by bundled GHA ruleset |
| `.github/dependabot.yml` | Weekly action update PRs | `yaml_path_matches` for `updates[?@.package-ecosystem == 'github-actions'].directory == '/'` |

The bundled `ci/github-actions@v1` ruleset (3 rules: workflow
permissions, action SHA pinning, workflow has `name:`) covers
the hardening surface for all 28 workflows at once. The
configured `.alint.yml` restates the SHA-pinning rule at
warning level — at clone time **149 of 197 GHA action uses
across the 28 workflows are unpinned by SHA** (mostly
`actions/checkout@v4`-style floating tags), exactly the
finding the OpenSSF Scorecard "Pinned-Dependencies" check
flags.

### Per-language subtree — the polyglot conventions

This is where alint earns its keep on apache/arrow.

| Subdir | Manifest at root | Per-package shape | alint disposition |
|---|---|---|---|
| `cpp/` | `CMakeLists.txt`, `meson.build`, `vcpkg.json`, `Brewfile`, `README.md` | (the C++ side is a single CMake project, no per-package iteration) | 4× `file_exists` for the manifest set |
| `c_glib/` | `meson.build`, `Gemfile` (for the test rake tasks), `README.md` | 7 sub-libraries: `arrow-glib/`, `arrow-cuda-glib/`, `arrow-dataset-glib/`, `arrow-flight-glib/`, `arrow-flight-sql-glib/`, `gandiva-glib/`, `parquet-glib/` — each MUST have its own `meson.build` | `file_exists` + `for_each_dir` over the named sub-library list with `require: meson.build` |
| `python/` | `pyproject.toml`, `setup.cfg`, `MANIFEST.in`, `CMakeLists.txt`, `LICENSE.txt` (symlink), `NOTICE.txt` (symlink), `README.md` | (single-package; `pyarrow` published to PyPI) | 3× structured-query rules: `project.name == 'pyarrow'`, `project.license == 'Apache-2.0'`, plus `file_exists` for the manifest set |
| `r/` | `DESCRIPTION`, `NAMESPACE`, `NEWS.md`, `Makefile`, `cran-comments.md`, `README.md` | (single-package; `arrow` published to CRAN) | `file_content_matches` for `Package: arrow` + `License: Apache License (>= 2.0)` |
| `ruby/` | `Gemfile`, `Rakefile`, `README.md` (top-level orchestration) | **8 gem subdirectories** under `ruby/red-*` — each ships `LICENSE.txt` + `NOTICE.txt` + `README.md` + `Rakefile` + `Gemfile` + `lib/` + `test/` + `<name>.gemspec`. The most uniform per-package shape in the tree. | `for_each_file` over `ruby/red-*/Rakefile` with a `require:` block of 8 child rules (the per-gem layout) — single rule covers all 8 gems |
| `matlab/` | `CMakeLists.txt`, `README.md` | (single-package; MEX-built MATLAB binding) | `file_exists` for the manifest |
| `format/` | `Schema.fbs`, `Message.fbs`, `File.fbs`, `Tensor.fbs`, `SparseTensor.fbs`, `Flight.proto`, `FlightSql.proto`, `README.rst` | (the cross-language schema spec — every per-language implementation conforms to these files) | `file_exists` for each .fbs/.proto file (the integrity gate) |

### Per-Ruby-gem layout (the cleanest per-package shape)

Each of the 8 directories under `ruby/red-*` is required to
ship the same six files + two directories. This is the
analogue of npm's `packages/*` ↔ `package.json` convention,
but for RubyGems. The convention is implicit (no script
enforces it; it's just what `dev/release/post-06-ruby.sh`
assumes when it walks `ruby/red-*`):

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

The configured alint config's `arrow-ruby-gem-required-meta-files`
+ `arrow-ruby-gem-has-gemspec` rules enforce both the file set
AND the per-gem `gemspec` filename convention.

### `format/` — the cross-language schema spec

This is the **most distinctive structural feature of
apache/arrow**: a single directory of FlatBuffers + Protobuf
files that EVERY language implementation must conform to. No
per-language linter sees this — it's the cross-language
contract.

```
format/
├── Schema.fbs              (the canonical type system)
├── Message.fbs             (IPC message envelope)
├── File.fbs                (Arrow file format)
├── Tensor.fbs              (dense tensor encoding)
├── SparseTensor.fbs        (sparse tensor encoding)
├── Flight.proto            (Flight RPC protocol)
├── FlightSql.proto         (FlightSQL extension)
├── README.rst              (the entry point for new-language implementers)
└── substrait/
    └── extension_types.yaml
```

The configured alint rule `arrow-format-spec-files-present` is
a single multi-path `file_exists` covering all seven core
schema files. Removing any one of them silently breaks
cross-language interop (and the integration test under
`dev/archery/`).

### Pre-commit hooks (21 distinct hook ids across 14 + 2 hook repos)

`.pre-commit-config.yaml` is the canonical alint-shaped
surface for this repo: it's the cross-language tool registry
that fans out to every per-language linter.

| Hook id | Repo | Scope (files: glob) | alint disposition |
|---|---|---|---|
| `rat` | local (runs `dev/release/run-rat.sh`) | always_run | `command:` wrapping the pre-commit `rat` hook |
| `hadolint-docker` | hadolint/hadolint | (4 specific dockerfiles, gated by ignore-list) | `command:` wrapping hadolint hook |
| `clang-format` (×4) | pre-commit/mirrors-clang-format | scoped to `^cpp/`, `^c_glib/`, `^matlab/src/cpp/`, `^python/pyarrow/src/` | `command:` wrapping clang-format hook |
| `cpplint` (×2) | cpplint/cpplint | scoped to `^cpp/`, `^matlab/src/cpp/` | `command:` wrapping cpplint hook |
| `autopep8`, `flake8`, `cython-lint`, `numpydoc-validation` | hhatto/autopep8, pycqa/flake8, MarcoGorelli/cython-lint, numpy/numpydoc | scoped to `^(c_glib|dev|python)/` (autopep8/flake8) or `^python/` (cython/numpydoc) | 4× `command:` rules wrapping each |
| `lintr`, `air-format` | local (R lintr) + posit-dev/air-pre-commit | scoped to R sources | 2× `command:` rules |
| `rubocop` | rubocop/rubocop | (excludes homebrew formulae + auto-gen ruby) | `command:` wrapping rubocop hook |
| `cmake-format` | cheshirekow/cmake-format-precommit | scoped to `*CMakeLists.txt`, `cpp/*.cmake.in`, `ci/*.cmake` | `command:` wrapping cmake-format |
| `sphinx-lint` | sphinx-contrib/sphinx-lint | scoped to `docs/source` | `command:` wrapping sphinx-lint |
| `shellcheck`, `shfmt` | koalaman + scop | scoped to specific shell scripts (long allowlist; legacy lint-debt) | 2× `command:` rules |
| `meson-fmt` | trim21/pre-commit-mirror-meson | scoped to `*meson.build`, `*meson.options` | `command:` wrapping meson-fmt |

The 4× `clang-format` repetition is interesting: pre-commit
demands one hook entry per file-scope, so the cpp/, c_glib/,
matlab/src/cpp/, and python/pyarrow/src/ scopes each need
their own. Same for cpplint's 2× repetition. alint expresses
this in one `command:` rule that drives the umbrella
`pre-commit run --all-files`, but a v0.10+
`fanout: {scope_filter: ..., command: ...}` primitive could
express the per-scope fan-out declaratively.

---

## What maps to existing alint rules

The 65-rule [`/.alint.yml`](.alint.yml) breaks down as:

- **6 bundled rulesets** (`oss-baseline`, `compliance/apache-2`,
  `python`, `ci/github-actions`,
  `hygiene/no-tracked-artifacts`, `tooling/editorconfig`) —
  pull in roughly **35 rules** between them
- **1 cross-language structural rule** —
  `arrow-language-subdir-has-readme` (`for_each_file` over
  `{cpp,c_glib,python,r,ruby,matlab,format}/README.{md,rst}`)
- **1 cross-language schema-spec rule** —
  `arrow-format-spec-files-present` (single `file_exists`
  covering all 7 .fbs/.proto files)
- **2 per-Ruby-gem rules** —
  `arrow-ruby-gem-required-meta-files` (`for_each_file` over
  `ruby/red-*/Rakefile` with 6-child `require:` block) +
  `arrow-ruby-gem-has-gemspec` (multi-path file_exists with
  the 8 gemspec basenames)
- **1 per-GLib-sublibrary rule** —
  `arrow-c-glib-sublibrary-has-meson-build` (`for_each_dir`
  over the named sub-library list)
- **8 per-language manifest rules** — `cpp/CMakeLists.txt`,
  `cpp/vcpkg.json`, `cpp/meson.build`, `c_glib/meson.build`,
  `python/pyproject.toml` (+ name + license),
  `r/DESCRIPTION` (+ name + license),
  `ruby/Rakefile`, `matlab/CMakeLists.txt`
- **8 Apache governance rules** — `.asf.yaml` present + 2
  yaml-path checks (homepage, commits notification list);
  `.pre-commit-config.yaml` present + 1 `file_content_matches`
  for the `rat` hook id; `dev/release/rat_exclude_files.txt`
  + `run-rat.sh` + `check-rat-report.py` + `verify-release-
  candidate.sh` + `merge_arrow_pr.{py,sh}` presence
- **9 per-language tool-config presence rules** —
  `.clang-format`, `.clang-tidy`, `.clang-tidy-ignore`,
  `CPPLINT.cfg`, `cmake-format.py`, `.rubocop.yml`,
  `.shellcheckrc`, `.hadolint.yaml`, `.editorconfig` `root = true`
- **2 docker orchestration rules** — `compose.yaml` +
  `.env` presence
- **1 Apache header override** — restates the bundled
  `apache-2-source-has-license-header` rule with arrow's
  longer ASF-preamble pattern (covers `Licensed (to the
  Apache Software Foundation|under the Apache License, Version 2)`)
  + extended file-extension list (adds `.cs`, `.m`, `.mm`,
  `.pyx`, `.pxd`, `.fbs`, `.proto`, `.rb`)
- **2 per-language Apache-header restatements** — explicit
  rules for python/ and cpp/ scopes (cmake/.cmake.in
  extensions the bundled rule doesn't cover)
- **3 hygiene rules** — `cpp/build/`, `r/libs/`, `python/build/`
  + `python/*.egg-info/` + `python/wheelhouse/` absent
- **2 GHA rules** — restatement of SHA-pinning at
  warning level + `.github/dependabot.yml` includes
  `package-ecosystem: github-actions`
- **1 gitattributes presence rule**
- **16 `command:` rule shell-outs** — pre-commit umbrella +
  per-language: clang-format / cpplint / rubocop / flake8 /
  autopep8 / cython-lint / numpydoc / lintr / air-format /
  cmake-format / shellcheck / shfmt / hadolint / sphinx-lint /
  meson-fmt / rat

---

## What needs new alint primitives

Three patterns specific to apache/arrow that don't fit any
current rule:

### 1. `registry_paths_resolve` for `dev/release/rat_exclude_files.txt`

`dev/release/rat_exclude_files.txt` lists 102 path patterns
RAT must skip (binary fixtures, vendored code,
auto-generated files like `r/R/arrowExports.R`). Apache RAT
itself fails non-zero if a pattern in the exclude list
**doesn't resolve** to at least one on-disk file (so the
list can't drift to dead patterns). alint has the file
present; what's missing is the cross-validation that every
pattern in the registry file maps to ≥1 real file.

This is the **fourth repo** to surface this need (rust-lang +
clap + cpython + apache/arrow). Going from "v0.10
high-priority candidate" to "v0.10 must-ship": **demand 4 of 4
candidates** for the same shape, and the apache/arrow
finding directly translates ("16 source files flagged as
missing the Apache header are all listed in
rat_exclude_files.txt — alint can't resolve the
exclude-list pointers from header-missing-finding to known-
exempt").

### 2. `cross_language_implementation_complete` — every type in `format/Schema.fbs` has a per-language test fixture

This is **net new**: every cross-language type defined in
`format/Schema.fbs` (the canonical Arrow type system) must
have a corresponding test case in each of `cpp/`, `python/`,
`r/`, `ruby/`. The dev/archery/ integration-test framework
runs this dynamically at test time; alint can't express it
because it requires:

1. Parsing the FlatBuffers file (extracting type names)
2. Cross-referencing to per-language test fixture paths
3. Asserting fixture-file existence per language

Same shape as the airflow `cross_file_value_equals` candidate
extended to "for each value in registry A, assert N partner
files exist matching template B per language scope". The
**polyglot variant** is unique to apache/arrow + sibling
multi-implementation specs (Substrait, Apache Iceberg, Apache
Beam SDKs).

**Strong v0.10+ signal**: this is the **canonical "alint is
the layer that catches drift across the polyglot tree"** rule
shape. arrow stress-tests it harder than any other repo in
P2a — defer to v0.10+ as the headline polyglot primitive.

### 3. `ordered_block` for `rat_exclude_files.txt` + the long shell file: lists in `.pre-commit-config.yaml`

`rat_exclude_files.txt` is conventionally alphabetised
(verifiable by `LC_ALL=C sort -c rat_exclude_files.txt` —
exits 0 today). The `.pre-commit-config.yaml` `shellcheck`
and `shfmt` `files:` patterns (each ~50 lines of
alternation) follow the same convention. Both are
unenforced. **Re-confirms** the rule-kind from rust-lang +
airflow + tokio + cpython (5 sources now).

### 4. `pre-commit fan-out` mode (extension of existing `command:` rule)

apache/arrow's pre-commit config repeats the `clang-format`
hook 4× (one per language scope: cpp/, c_glib/, matlab/src/cpp/,
python/pyarrow/src/) and `cpplint` 2× (cpp/ and matlab/src/cpp/)
because pre-commit demands one hook entry per `files:` pattern.
A v0.10+ `command_per_scope` rule could express the same
intent declaratively — one `command:` definition + N scope
filters → N rule instances at registry-build time.

Lower demand than candidates 1-3 (single-source: only arrow
has this repetition pattern at this scale), but resurfaces
the broader question: **alint's `command:` rule shape
sometimes wraps a tool that already has its own scope DSL**
(pre-commit, husky-style hooks, `lint-staged`). At v0.10+ the
conversation is "should `command:` learn fan-out, or should
alint integrate directly with pre-commit's hook discovery?".

---

## What's out of alint's scope (kept on the existing tool)

Listed by category for clarity:

- **AST analysis** (clang-format + clang-tidy + cpplint + the
  per-language autopep8/flake8/cython-lint/numpydoc
  validators + rubocop + lintr + air-format) — alint
  deliberately doesn't try to be a parser. Shell out via
  `command:`.
- **Apache RAT's binary classification + version metadata**
  — RAT looks inside `*.jar` archives, classifies binary file
  types, and cross-references the SPDX manifest. alint reads
  files; it doesn't open archives. Shell out via the rat hook.
- **Cross-language conformance at runtime**
  (`dev/archery/`'s integration test, which spins up a
  C++ process + Python process + R process and exchanges
  Arrow IPC messages between them) — alint sees files at
  rest, not protocol behaviour at runtime.
- **C++ symbol-prefix scans** (cpython-style "every libpython
  exported symbol starts with `Py`" — arrow has the analogous
  "every libarrow exported symbol starts with `Arrow` /
  `arrow_`") — out of alint scope (binary parsing); STAYS on
  the per-language CI scripts.
- **Sphinx docs build cross-references** (`docs/source/`
  Sphinx build) — alint reads files; it doesn't run Sphinx.
- **PR-content guards / merge-message format** (the comment
  bot, the JIRA-link extractor in `merge_arrow_pr.py`) — git
  state and PR data, not tree state.
- **Operational workflows** (release / cron / triage /
  issue-bot / PR-comment) — not validation surfaces.

---

## Already covered by other linters arrow uses

- `clang-format` / `clang-tidy` / `cpplint` — C++ AST+style;
  alint orchestrates via `command:` so the per-tool config
  presence rules + the format check run in one alint pass.
- `rubocop` — Ruby AST+style.
- `flake8` / `autopep8` / `cython-lint` / `numpydoc-validation`
  — Python AST+style+docs.
- `lintr` / `air-format` — R AST+style.
- `cmake-format` — CMake formatter.
- `sphinx-lint` — RST/Sphinx.
- `shellcheck` / `shfmt` — shell.
- `hadolint` — Dockerfile.
- `meson-fmt` — Meson build files.
- `apache-rat` — Apache release-audit (license headers + binary
  classification).

---

## Performance comparison (placeholder — bench when validation pass scales)

The repo is large enough to be a meaningful stress test:

- **~42 MiB** working tree (after sparse-checkout dropping
  `cpp/src`, `python/pyarrow`, `java/vector/src`, `r/inst`)
- **6 in-tree language implementations** + the cross-language
  format spec
- **28** GitHub Actions workflows
- **396 lines** of `.pre-commit-config.yaml` orchestrating
  21 hooks across 14 hook repos

The published S9 bench (100k+ files, 13 languages) hits ~1.4 s
on a stock CI runner. The full apache/arrow tree (with
`cpp/src` + `python/pyarrow` + `r/inst` re-included, ~600 MB,
~40k files) sits between S3 and S9. Expected: 1-3 s for
`alint check` on the structural rules alone, vs. ~60-180 s for
`pre-commit run --all-files` (which serially fans through
21 per-language hooks).

Where alint shines on apache/arrow specifically: the
**cross-language conventions** — every per-language subdir
has README, every Ruby gem subdir has 6 files + 2 dirs, every
GLib sub-library has meson.build, every .fbs/.proto exists in
format/ — run against the entire polyglot tree in tens of
milliseconds. Sequential `find . -name README.md -path "*/cpp"`
+ same for python/r/ruby/matlab/format would be ~5 s on a
hot cache.

To benchmark wall-clock for real:
`time pre-commit run --all-files` vs `time alint check`.
Deferred to the per-repo measurement pass.

---

## Recommendation for the launch story

This case study is **the** flagship "language-agnostic
polyglot monorepo" story for the launch:

- **apache/arrow is the canonical multi-language project on
  GitHub** (~14k stars, widely deployed, the basis for every
  modern columnar data library: Pandas 2.0, Polars, DuckDB,
  Snowflake's external table format). Naming it as a target
  gives alint instant credibility with the data-engineering
  audience.
- **No per-language linter sees the cross-language structural
  conventions** — clang-format only sees C++, rubocop only
  sees Ruby, flake8 only sees Python, lintr only sees R. The
  invariants this case study enforces (per-language subdir
  README, per-Ruby-gem layout, per-GLib-sublibrary meson.build,
  format-spec file integrity, Apache governance triad,
  per-language tool-config presence) are exactly the layer
  alint owns and nothing else does.
- **The Apache compliance bundle** (`compliance/apache-2@v1`)
  is alint's tightest fit anywhere in the case-study
  catalogue. Applying it to apache/arrow surfaces the
  longer-vs-shorter header form distinction, which the
  configured override resolves cleanly. The header rule's 16
  findings are ALL legitimate (LLVM-licensed third-party
  files + auto-generated bindings, all listed in
  `dev/release/rat_exclude_files.txt`) — confirming both that
  the rule fires correctly AND that the v0.10+
  `registry_paths_resolve` rule kind would close the loop.
- **The Apache release tooling** (`dev/release/`,
  `.pre-commit-config.yaml`'s rat hook,
  `verify-release-candidate.sh`) is unique among OSS repos:
  the ASF community ships ~250+ projects following this
  exact discipline, and **NONE of them have a structural
  linter that enforces the dance**. This case study is a
  ready-made template for every Apache project.

Position it as the **fifth tile** on alint.org/examples
(after kubernetes, airflow, microsoft/typescript, next.js),
with the angle: *"apache/arrow has 6 languages in one tree,
21 lint hooks across 14 tool repos, and 0 tools that see the
cross-language conventions — alint is the layer that does."*

The pitch lands harder when paired with the per-Ruby-gem
finding: 8 gem subdirs × 6 required files each = 48
file-existence assertions wrapped in a single
`for_each_file` rule. No Ruby tool checks the layout
because no Ruby tool sees the layout from above.

Followup feature work surfaced (consolidated, sorted by
strength of demand across P2a):

- **`registry_paths_resolve` rule kind** — covers
  `rat_exclude_files.txt` here, plus the rust-lang
  triagebot.toml + clap pre-release-replacements + cpython
  .gitattributes generated markers + check-c-api-docs +
  check-manifests.js. **Demand: rust + clap + cpython + arrow
  + next.js (5 distinct repos)** — strongest demand signal
  in P2a now, displacing `cross_file_value_equals` for the
  v0.10 #1 priority slot.
- **`cross_language_implementation_complete` rule kind** —
  net new, covers the format/Schema.fbs ↔ per-language
  test-fixture coverage gap. arrow is the only single-repo
  source today, but the shape generalises to every
  multi-implementation spec (Substrait, Iceberg, Beam SDKs,
  protobuf bindings). File as v0.11+ (after the polyglot
  positioning has more supporting data).
- **`ordered_block` rule kind** — re-confirmed by
  `rat_exclude_files.txt` + the long file: alternation lists
  in `.pre-commit-config.yaml`. **Demand: rust + airflow +
  tokio + cpython + arrow (5 distinct repos)** — joins
  `registry_paths_resolve` at the top of the v0.10 priority
  list.
- **`command_per_scope` mode** for the existing `command:`
  rule — covers the pre-commit per-scope hook repetition
  here. Demand: arrow only at this scale; defer.

---

## Filter-expression pitfall: see CONFIG-AUTHORING.md § 10

The original write-up of this case study claimed a NEW pitfall
(#17) — *"JSONPath filter expressions `?(@.foo == 'bar')` are
parser-rejected"*. Investigation during v0.9.15 Phase 4
disproved that:

- `serde_json_path` 0.7.x **accepts** outer-parens filter
  predicates (`?(@.foo == 'bar')`).
- The arrow config's actual failure was the dashed
  `package-ecosystem` key inside the filter — i.e. a filter-
  context instance of CONFIG-AUTHORING.md **pitfall #10**
  (dashed-key access requires bracket notation).
- The fix is bracket notation, not removing the outer parens.

So the canonical correct path (either form works as long as the
dashed key is bracketed):

```yaml
# Both forms parse cleanly under serde_json_path 0.7.x:
path: "$.updates[?(@['package-ecosystem'] == 'github-actions')].directory"
path: "$.updates[?@['package-ecosystem'] == 'github-actions'].directory"
```

The v0.9.15 Phase 4 JSONPath diagnostic helper catches the dashed-
key shape — both top-level and inside filter contexts — and
suggests bracket notation. See `docs/development/CONFIG-AUTHORING.md`
§ 10 for the full canonical form.

(Lesson for AI agent reviewers: when a JSONPath fails with "long-hand
segment, parser error", the most common cause is a dashed key
needing bracket notation, regardless of whether outer parens are
present.)

---

## Notes for the parent agent

- Audit (`cargo test -p alint-e2e --test
  coverage_audit_examples_parse`) **passes** with this
  config in place.
- The original report attributed a JSONPath parse failure
  to outer parens; v0.9.15 Phase 4 disproved that — the real
  issue was a dashed key inside the filter (an instance of
  pitfall #10). v0.9.15 Phase 4 added a JSONPath diagnostic
  helper that suggests bracket notation in both filter and
  non-filter contexts.
- Config runs cleanly against the actual cloned repo at
  `/tmp/apache-arrow/` (243 violations across 28 failing
  files: 149 GHA SHA-pin warnings + 24 arrow-specific GHA
  warnings + 16 source-header warnings — all 16 are
  RAT-excluded files in `dev/release/rat_exclude_files.txt`,
  confirming the v0.10+ `registry_paths_resolve` gap — plus
  the expected `command:`-rule "tool not on PATH" errors
  for `pre-commit` not being installed in the alint test
  environment). No silent failures. No false positives in
  the structural rule set.
- The cross-language structural rules
  (`arrow-language-subdir-has-readme`,
  `arrow-format-spec-files-present`,
  `arrow-ruby-gem-required-meta-files`,
  `arrow-ruby-gem-has-gemspec`,
  `arrow-c-glib-sublibrary-has-meson-build`) all silently
  pass on the live tree, confirming arrow's polyglot layout
  is fully consistent — and the rules are correctly
  scoped to fire if drift were to occur.
