# Case study: `bazelbuild/bazel`

> Marketing/positioning writeup at https://alint.org/examples/bazelbuild-bazel/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `bazelbuild/bazel`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives — and
the delineation of the Starlark-AST surface that stays on
`buildifier`.

**Repo state captured:** 2026-05-06, sparse-clone of
`bazelbuild/bazel` excluding `/src/test`, `/third_party`, `/site`,
and `/scripts/release/relnotes` (the heaviest sub-trees; not
material to the structural inventory).

---

## Summary

bazelbuild/bazel is the canonical "build system that builds itself"
repository. The repo is a true polyglot — Java
(`src/main/java/com/google/devtools/build/lib/`, ~213 directories,
~175 BUILD files), C++ (`src/main/cpp/`, `src/main/native/`),
Python (build / docs / release tooling), Go (rules_go consumers),
shell (`scripts/`, `compile.sh`), Windows .bat (launcher),
Starlark (`*.bzl`, BUILD, MODULE.bazel) — but the entire build
system is driven by Bazel itself. There is no `pom.xml`, no
`build.gradle`, no `Cargo.toml`, no `go.mod`, no `package.json`
at the repo root, despite the multi-language content. The
bootstrap is `compile.sh` (a 200-line shell script that does an
initial Bazel build), and from there, `bazel build //src:bazel`
is the canonical self-build target.

Concrete count at HEAD:

- **322** BUILD / BUILD.bazel files (one per Bazel package)
- **100** *.bzl Starlark macro / rule files
- **1** root MODULE.bazel (482 lines) + 1 MODULE.bazel.lock
  (110 KiB JSON) + **0** root WORKSPACE files (Bazel itself ships
  post-WORKSPACE; bzlmod is the supported dependency model).
  Two legacy WORKSPACE files survive in
  `tools/distributions/debian/protobuf/` and a builtins test
  fixture, kept for back-compat regression testing.
- **9** GitHub Actions workflows under `.github/workflows/` —
  ALL OPERATIONAL (`cherry-picker`, `community-review-labeler`,
  `labeler`, `release-helper`, `remove-labels`, `scorecard`,
  `ssl-monitor`, `stale`, `trigger-docs-update`); the CI that
  actually builds and tests Bazel runs OUT OF GITHUB on
  Buildkite via `.bazelci/presubmit.yml` (309 lines, 25+
  platform tasks: rockylinux8, ubuntu2404, fedora39, macos,
  windows, etc.).
- **1** `.bazelrc` (117 lines) declaring 25+ named configs
  (`--config=ci-linux`, `--config=remote`, `--config=macos`,
  etc.). The discipline is enforced socially through code
  review; no static lint gates these names.
- **1** `.bazelversion` (`9.1.0`) — Bazelisk reads this to
  fetch the matching Bazel binary for self-build.
- **1** root `BUILD` (341 lines: license, srcs filegroup,
  bootstrap-jars `pkg_tar`, distfile genrule, etc.)
- **1** `pyproject.toml` (just `[tool.pyink]` for Python
  formatter config — Bazel uses pyink, not black/ruff)
- **2** `oneversion_allowlist*.csv` files declaring which Java
  class paths may have multiple JAR sources (a Bazel-specific
  one-version enforcement check baked into the build, not a
  separate lint)
- **100-line** CODEOWNERS, **17-line** `.gitattributes`
  (`* -text`, `*.bzl linguist-language=Python`, lockfile-merge
  driver wiring)
- **1** `.bazelci/postsubmit.yml` for the post-merge full-matrix
  CI pass

Total **structural-validation surfaces** counted: **37** discrete
checks across the inventory (see § "Existing tooling inventory"
below).

- **17 of 37 (~46 %) map to existing alint rules** — the bundled
  `oss-baseline + java + ci/github-actions +
  hygiene/no-tracked-artifacts` ship roughly **30 rules**
  between them, plus the **41 bazel-specific rules** in
  [`/.alint.yml`](.alint.yml) (`.bazelversion` shape, MODULE.bazel
  structure, BUILD-file naming, .gitattributes invariants,
  Apache-2 license headers across Java / C++ / shell / *.bzl,
  per-source-tree hygiene, top-level metadata files).
- **6 of 37 (~16 %) shell out via `command:` rules** — wrapping
  `buildifier --mode=check --lint=warn` for the Starlark AST
  layer, plus `shellcheck` for the ~7 shell scripts.
- **14 of 37 (~38 %) are out of alint's scope** — the entire
  Starlark-AST surface (`cc_library`/`java_library` deps integrity,
  `glob([...])` vs filesystem, target visibility, load() ordering,
  deprecated-rule-attribute detection); the build-graph integrity
  surface (`bazel build //src:bazel` self-build, the `:srcs`
  filegroup transitive closure); the lockfile-vs-manifest freshness
  surface (`bazel mod deps --lockfile_mode=update`); the
  oneversion enforcement (Java classpath uniqueness; baked into
  the build); the `.bazelrc` named-config audit; and the
  Buildkite-driven cross-platform CI orchestration
  (`.bazelci/presubmit.yml` — 309 lines, 25+ platforms).

The configured **41-rule** [`/.alint.yml`](.alint.yml) plus the
~40 rules from the four extended bundled rulesets gives
**81 rules** loaded by `validate-config` against this repo. The
38 % out-of-scope fraction is the highest of any case study in
the corpus to date.

The 100 *.bzl files and 322 BUILD files make Starlark the
load-bearing language for build declarations, and alint's
structured-query rule family (`json_path_*`, `yaml_path_*`,
`toml_path_*`) does not parse Starlark. The right tool for that
layer is Google's `buildifier` / `buildozer` pair, which alint
orchestrates via `command:` rules rather than re-implementing.

---

## Existing tooling inventory

### Bazel-native CI: `.bazelci/presubmit.yml` + `postsubmit.yml`

Bazel's actual build-and-test CI does not run on GitHub Actions.
It runs on **Buildkite**, driven by 309 lines of
`.bazelci/presubmit.yml` covering 25+ platform tasks. The shape
of each platform entry is:

```yaml
tasks:
  rockylinux8:
    shell_commands: [...]
    build_flags: ["--config=ci-linux"]
    build_targets: ["//src:bazel", "//..."]
    test_flags: ["--config=ci-linux", "--test_tag_filters=-no_presubmit"]
    test_targets: ["//..."]
```

| Surface | What it checks | alint disposition |
|---|---|---|
| `tasks:` mapping is non-empty | Renaming the key silently disables CI | MAPS — `bazel-bazelci-presubmit-has-tasks` (`file_content_matches`) |
| Per-platform `build_targets:` / `test_targets:` includes `//src:bazel` | The self-build target is the load-bearing CI assertion | OUT OF SCOPE — would require a YAML walker over every `tasks.*.build_targets` array, asserting at least one element matches; doable as `yaml_path_matches` against `$.tasks.*.build_targets[*]` but the semantics get tangled (see CONFIG-AUTHORING.md pitfall #17 — `[*]` flips intent) |
| `build_flags:` includes `--config=ci-linux` per platform | Removing this drops the centralised CI flag set | OUT OF SCOPE (same pitfall-#17 shape) |
| `test_tag_filters=-no_presubmit` declared | Skips the slow tests | OUT OF SCOPE (per-platform string-equality check; would need `yaml_path_contains` v0.10+ candidate) |
| The Buildkite executor itself | Cross-platform self-build orchestration | OUT OF SCOPE (Buildkite owns this) |

### `.github/workflows/` (9 files; all operational)

| Workflow | Purpose | alint disposition |
|---|---|---|
| `cherry-picker.yml` | Auto-cherry-picks closed PRs onto release branches | MAPS — bundled `gha-pin-actions-to-sha` + `gha-workflow-contents-read` |
| `community-review-labeler.yml` | Cron-driven PR-label cleanup | MAPS — same |
| `labeler.yml` | PR-opened label assignment | MAPS — same |
| `release-helper.yml` | Release-issue-bot orchestration | MAPS — same |
| `remove-labels.yml` | Webhook-driven label cleanup | MAPS — same |
| `scorecard.yml` | OpenSSF Scorecard scan (cron-driven) | MAPS — same |
| `ssl-monitor.yml` | Daily SSL cert expiry check (drives a Python script) | MAPS — same |
| `stale.yml` | Issue-bot for stale issues / PRs | MAPS — same |
| `trigger-docs-update.yml` | Cross-repo doc-publishing dispatcher | MAPS — same |
| `step-security/harden-runner` opens every workflow | Bazel team egress-audit convention | MAPS — `bazel-gha-uses-step-security-harden-runner` (`file_content_matches`) |
| Pinned-Dependencies (40-char SHAs) on every `uses:` | OpenSSF Scorecard signal | MAPS — bundled `gha-pin-actions-to-sha` |

The bazelbuild org is one of the **cleanest large-org adopters of
Pinned-Dependencies**: every action across the 9 workflows is
SHA-pinned. Running the bundled GHA ruleset against this repo
fires only on `actions/checkout@v6` (one tag-pinned action in
`community-review-labeler.yml` — minor regression).

### Top-level metadata + governance

| File | Purpose | alint disposition |
|---|---|---|
| `LICENSE` (Apache 2.0) | Standard | MAPS — bundled `oss-license-exists` + `oss-license-non-empty` |
| `README.md` | Standard | MAPS — bundled `oss-readme-*` |
| `SECURITY.md` (root, not `.github/`) | Vulnerability disclosure | MAPS — `bazel-security-md-exists` (root-only override) |
| `CODE_OF_CONDUCT.md` | Standard | MAPS — `bazel-code-of-conduct-exists` |
| `CONTRIBUTING.md` | CLA + workflow explainer | MAPS — `bazel-contributing-exists` |
| `CODEOWNERS` (root, not `.github/`) | PR review routing | MAPS — `bazel-codeowners-exists` |
| `AUTHORS` + `CONTRIBUTORS` | CLA database lineage | MAPS — `bazel-authors-exists` + `bazel-contributors-exists` (golang/go is the only other repo in the corpus that ships these; cpython retired them) |
| `CHANGELOG.md` | Referenced by `:changelog-file` filegroup in root BUILD; release-package targets read it | MAPS — `bazel-changelog-exists` (level: error — its absence breaks `//scripts/packages:...`) |
| `AGENTS.md` (`.gemini/` styleguide) | LLM-coding-agent context for the repo | OUT OF SCOPE (no canonical schema yet) |
| `pyproject.toml` (just `[tool.pyink]`) | Bazel-team Python style: pyink, 2-space indent, 80-col | MAPS — `bazel-pyproject-pyink-config` + `bazel-pyproject-2-space-indent` (`toml_path_matches` + `toml_path_equals`) |
| `requirements.txt` | Build-helper Python deps | MAPS — bundled hygiene |
| `oneversion_allowlist.csv` + `oneversion_allowlist_for_tests.csv` | Java classpath one-version override list | OUT OF SCOPE (the Bazel build ITSELF reads these and asserts every Java class on the classpath has exactly one source JAR — an AST-aware classpath-resolution check baked into the build, not a separate lint) |

### Bazel-structural files (the BUILD-FILE SHAPE surface — the polyglot tier no P2a repo had)

| Surface | What it checks | alint disposition |
|---|---|---|
| `MODULE.bazel` at root | Bzlmod (Bazel 7+ default dep model) | MAPS — `bazel-module-file-exists` |
| `MODULE.bazel.lock` at root | Hermetic dep resolution | MAPS — `bazel-module-lock-exists` |
| `MODULE.bazel` declares `module(name = "...")` | Without it, bzlmod can't identify the workspace | MAPS — `bazel-module-declares-name` (`file_content_matches` for `^\s*module\s*\(`) |
| No legacy `WORKSPACE` / `WORKSPACE.bazel` / `WORKSPACE.bzlmod` at root | Bazel 8 removed legacy WORKSPACE entirely | MAPS — `bazel-no-legacy-workspace-at-root` |
| Root `BUILD` or `BUILD.bazel` exists | Without it, `bazel build //:<target>` can't resolve any root target | MAPS — `bazel-root-build-file-exists` |
| `.bazelversion` exists at root | Bazelisk reads it to fetch matching Bazel binary | **GOTCHA — bazel's own `.gitignore` contains `.bazelversion`**; see § "BUILD-file notes" below for the workaround |
| `.bazelversion` is semver-shaped (e.g. `9.1.0`) | Magic tokens (`latest`, `last_green`) defeat the file's purpose | MAPS — `bazel-version-file-shape` (`file_content_matches`); same gitignore caveat applies |
| `.bazelrc` exists at root | Repo-wide build flags consolidation | MAPS — `bazel-bazelrc-exists` |
| BUILD / BUILD.bazel files open with a comment header | Convention; helps readability | MAPS — `bazel-build-file-naming` (`file_starts_with` with `prefix: "#"`) |
| `.bazel` extension reserved for {BUILD, MODULE, REPO}.bazel | Mis-named Starlark `.bazel` parsed against BUILD grammar, not Starlark | MAPS — `bazel-bzl-suffix-required` (`filename_regex` allowlist) |
| Every *.bzl carries the 14-line Apache 2.0 boilerplate header | Convention; enforced socially via code review | MAPS — `bazel-bzl-apache-license-header` (`file_header`) — **enforced nowhere statically today** |
| Every BUILD declares `package()` or starts with `load()` | Convention; helps readability | OUT OF SCOPE today (would need to compose `file_content_matches` with `file_starts_with` against multi-line patterns; doable but verbose) |
| `cc_library` / `java_library` / `py_binary` deps integrity | Every label resolves; no cycles; visibility honored | **OUT OF SCOPE** — `bazel query //...` / `buildifier --lint` is the right tool |
| `glob([...])` patterns vs filesystem | Does the glob match anything? Does it leak BUILD files into srcs? | **OUT OF SCOPE** — `bazel build //... --nobuild` is the right tool |
| Target visibility (`//visibility:public` vs `__pkg__` vs specific list) | Visibility correctness | **OUT OF SCOPE** — `buildifier --lint=warn` |
| `load()` statement ordering (alphabetical, conventional bzl-load-before-stdlib-load) | Convention | **OUT OF SCOPE** — `buildifier --mode=fix` rewrites in canonical order |
| Deprecated rule-attribute usage (`licenses` → `applicable_licenses`) | Deprecation tracking | **OUT OF SCOPE** — `buildifier --lint=warn` |

### `.gitattributes` invariants

| Invariant | Purpose | alint disposition |
|---|---|---|
| `* -text` | Disables git's EOL normalization — load-bearing for `*.bat` files (Windows entry points hand-curate CRLF) | MAPS — `bazel-gitattributes-no-text-normalization` (same shape as golang/go's rule) |
| `*.bzl linguist-language=Python` | GitHub renders Starlark with Python syntax highlighting | MAPS — `bazel-gitattributes-bzl-linguist-python` |
| `BUILD linguist-language=Python` | Same for BUILD files | MAPS — `bazel-gitattributes-build-linguist-python` |
| `MODULE.bazel.lock merge=bazel-lockfile-merge` | Custom 3-way merge driver for the JSON lockfile (default git driver corrupts) | MAPS — `bazel-gitattributes-lockfile-merge-driver` |
| `**/build/** linguist-generated=false` | GitHub search inclusion override | OUT OF SCOPE (linguist-only hint, no on-disk effect) |

### Buildifier / Buildozer (the Starlark AST linter — alint shells out)

`buildifier` is Google's Starlark formatter + linter for BUILD,
BUILD.bazel, MODULE.bazel, WORKSPACE, and *.bzl files. It is the
**Bazel team's official Starlark AST tool**; alint does not
attempt to compete with it. The `bazel-buildifier-format-check`
rule shells out via `command:` so the Starlark AST checks
(mis-ordered load() blocks, suspicious glob patterns,
deprecated rule-attribute usage, formatter compliance) keep
firing through the same `alint check` pass.

### Apache 2.0 license headers (enforced socially, not statically)

The convention applies pervasively across the tree:

| Source family | Header form | Lines | alint rule |
|---|---|---:|---|
| `*.java` (`src/main/java/`, `src/tools/`, `src/java_tools/`) | `// Copyright YYYY The Bazel Authors. All rights reserved.` | 14 | `bazel-java-sources-apache-header` |
| `*.cc` / `*.h` (`src/main/cpp/`, `src/main/native/`, `src/tools/launcher/`) | `// Copyright YYYY The Bazel Authors. All rights reserved.` | 14 | `bazel-cpp-apache-header` |
| `*.bzl` (anywhere) | `# Copyright YYYY The Bazel Authors. All rights reserved.` | 14 | `bazel-bzl-apache-license-header` |
| `*.sh` (root + `scripts/`) | `# Copyright YYYY The Bazel Authors. All rights reserved.` (after `#!/usr/bin/env bash` shebang) | 14 (with shebang offset) | `bazel-shell-apache-header` |
| `*.py` | `# Copyright YYYY The Bazel Authors. All rights reserved.` | 14 | (covered by bundled `oss-baseline` no-header rule; could be added) |

**This is enforced NOWHERE STATICALLY today** — convention is
gated by code review at the bazelbuild org. alint encodes the
convention as 4 explicit `file_header` rules covering the four
major source families. Same shape as golang/go's BSD-header
rules (5 rules across `.go`, `.s`, `.bash`, `.bat`, `Makefile`).

### Source-tree hygiene (no_trailing_whitespace, final_newline, no_bidi)

Bundled `java@v1` ruleset gates these on `facts.has_java` (true
iff `pom.xml` / `build.gradle` exists somewhere in the tree).
**bazelbuild/bazel ships neither**, so the bundled rules silently
no-op. The case-study config restates the conventions at the
config layer without the fact gate, scoped to Bazel's source
trees explicitly:

- `bazel-java-no-trailing-whitespace` (`src/main/java/`,
  `src/tools/`, `src/java_tools/`)
- `bazel-java-final-newline` (same)
- `bazel-java-no-bidi` (same; Trojan Source defense)
- `bazel-cpp-no-trailing-whitespace` (`src/main/cpp/`,
  `src/main/native/`)
- `bazel-cpp-final-newline` (same)

---

## What needs new alint primitives

| Gap | Existing bazel tooling | What alint needs |
|---|---|---|
| `cc_library` / `java_library` / `py_binary` deps integrity | `bazel query`, `buildifier --lint=warn` | **NOT a v0.10+ candidate.** Fundamentally Starlark-AST work; belongs to `buildifier` / `bazel query`. The right hand-off is alint shells out via `command:`. |
| `glob([...])` pattern resolution against filesystem | `bazel build //... --nobuild`, `buildifier --lint=warn` | Same shape as cpython's `.gitattributes generated marker resolution` (`registry_paths_resolve` v0.10+ candidate), but the glob lives inside Starlark code, not a structured registry. **Possible v0.10+ as `starlark_glob_resolve` IF we ship a tree-sitter-based Starlark parser** — but that pulls a Python-grammar parser into alint's dep tree, which is a meaningful platform commitment. Lower priority. |
| MODULE.bazel ↔ MODULE.bazel.lock freshness | `bazel mod deps --lockfile_mode=update` | `generated_file_fresh` rule kind: "generated file `<output>` matches `<command_output>`" — a `command:` variant that compares stdout to file contents. **Already on the v0.10+ candidate list** from cpython, uv, and arrow. bazel makes the **fourth confirmation**. |
| `.bazelrc` named-config audit | None statically; convention enforced via code review | **NEW v0.10+ candidate**: `bazelrc_path_*` rule kind. The grammar is constrained (`<command>:<config_name> <flag>`), so a regex-based parser is feasible without tree-sitter. Niche to Bazel; rated low priority but logged. |
| `tasks.*.build_targets` includes `//src:bazel` | Buildkite drives the assertion at execution time | Same `*_path_contains` v0.10+ candidate from helm + deno (the "any element of array contains X" pattern). bazel makes it a **third confirmation**. |
| One-Version Java classpath uniqueness | Bazel's own build (reads `oneversion_allowlist*.csv`) | OUT OF SCOPE. AST-aware classpath analysis; baked into the Bazel build. |
| `.bazelversion` gitignored-but-tracked | git's "tracked-takes-precedence" semantics | **Pitfall #18 (FIXED in v0.9.17)** — `.gitignore` masks tracked-file presence checks. Now resolved via per-rule `respect_gitignore: false`; bazel's `.bazelversion` is now the canonical example documented in CONFIG-AUTHORING.md. See § "BUILD-file notes" below. |
| Apache RAT-equivalent license-tracking | None (Bazel uses a different licensing model — `rules_license` and a per-target `applicable_licenses`) | OUT OF SCOPE. |

**Cross-reference with the existing v0.10+ candidate list:**

- `generated_file_fresh` — confirmed by **bazel** (MODULE.bazel ↔
  MODULE.bazel.lock). Already on the list (from cpython, uv,
  arrow). 4th confirmation.
- `*_path_contains` — confirmed by **bazel**
  (`tasks.*.build_targets` array-contains-`//src:bazel`). Already
  on the list (from helm, deno). 3rd confirmation.
- `column_alignment` — not relevant for this repo (no
  CODEOWNERS-shaped column-aligned tabular files in bazel; their
  CODEOWNERS uses comment-block grouping, not column alignment).

**NEW candidates surfaced uniquely by bazel:**

- `starlark_glob_resolve` rule kind — would require a Starlark
  parser. Niche; rated low priority.
- `bazelrc_path_*` rule kind — `.bazelrc` named-config audit.
  Niche to Bazel; rated low priority.
- `respect_gitignore: false` per-rule knob (or
  `--no-respect-gitignore` global flag) — closes the
  tracked-but-gitignored-file gap (pitfall #18). **SHIPPED in v0.9.17.**

---

## BUILD-file notes — the Starlark wall

### What alint catches today

alint owns the **filesystem-shape** layer:

- **Filename / path conventions**
  - `BUILD` vs `BUILD.bazel` (both accepted; Bazel's own repo
    ships mostly bare `BUILD` for historical reasons)
  - `*.bzl` for Starlark macro libraries (Bazel dispatches the
    Starlark interpreter on the `.bzl` suffix; mis-named `.bazel`
    files get evaluated against the BUILD grammar and silently
    fail)
  - `MODULE.bazel` and `MODULE.bazel.lock` at the root
- **Presence / absence of structural files**
  - MODULE.bazel exists, no legacy WORKSPACE files at root,
    .bazelrc exists, root BUILD/BUILD.bazel exists,
    `.bazelci/presubmit.yml` exists, `.bazelci/postsubmit.yml`
    exists
- **Content-pattern checks over Starlark TEXT**
  - `module(name = "...")` declared in MODULE.bazel
    (`file_content_matches` against the regex
    `^\s*module\s*\(`)
  - Apache 2.0 boilerplate header on every `*.bzl`
    (`file_header` against `# Copyright YYYY The Bazel
    Authors`)
  - `.gitattributes` declares `* -text`, `*.bzl
    linguist-language=Python`, `MODULE.bazel.lock
    merge=bazel-lockfile-merge`
  - Comment-prefix opening on every BUILD file
    (`file_starts_with` with `prefix: "#"` — the conservative
    shared anchor)
- **Cross-file conventions** (limited)
  - GitHub Actions hardening across all 9 workflows
  - Per-source-family hygiene (Java, C++, shell)

### What alint CAN'T catch — the Starlark AST wall

alint's structured-query rule family (`json_path_matches`,
`yaml_path_matches`, `toml_path_matches`) parses the value as
JSON / YAML / TOML and runs an RFC 9535 JSONPath query against
it. **There is no `starlark_path_matches` because Starlark is
neither JSON, YAML, nor TOML — it's a Python-subset DSL with
function calls, conditionals, list comprehensions, and a full
expression evaluator.** Parsing it requires a Starlark
interpreter (or at minimum a Python-shaped AST parser like
tree-sitter-python or tree-sitter-starlark).

The set of invariants that live IN Starlark code and
fundamentally cannot be checked without parsing it:

- **`cc_library(name = "...", deps = [...], srcs = [...])`
  deps integrity** — every label in `deps = [...]` must resolve
  to a defined target; no cycles in the dep graph; visibility
  honored. The Bazel team owns this via `bazel query //...`.
  alint cannot read Starlark deps lists.

- **`glob([...])` patterns vs filesystem** — `glob([
  "**/*.java"], exclude = ["**/testdata/**"])` is evaluated at
  Bazel-load time against the actual filesystem. A glob that
  matches zero files is a build-time bug. alint cannot
  enumerate what a Starlark `glob()` call would match without
  evaluating Starlark.

- **Target visibility** — `visibility = ["//visibility:public"]`
  vs `__pkg__` vs a specific package list is a load-time
  expression Bazel evaluates. `buildifier --lint=warn` catches
  suspicious patterns; alint cannot.

- **`load()` statement ordering** — Bazel's `buildifier
  --mode=fix --lint=fix` rewrites load statements into a
  canonical order (alphabetical by target string, with bzl
  loads grouped before stdlib loads). The check requires
  parsing the load statements as Starlark. alint's
  `file_starts_with` can check that SOME load statement comes
  first, but cannot check ordering.

- **Deprecated rule-attribute usage** — Bazel rules deprecate
  attributes over time (`licenses = [...]` superseded by
  `applicable_licenses = [...]`; `legacy_create_init` deprecated
  in `py_binary`; etc.). Detecting these requires parsing the
  rule invocation's keyword args. `buildifier --lint=warn` does
  this; alint cannot.

- **`select({...})` resolution** — `cc_library(srcs = select({
  "@platforms//cpu:x86_64": ["x86.cc"], "//conditions:default":
  ["fallback.cc"]}))` resolves at Bazel-load time based on the
  current configuration. Catching mis-spelled keys requires
  parsing the dict literal AND knowing which platform conditions
  exist — both Bazel-internal.

### The workaround: alint orchestrates, buildifier does the AST work

The case-study config wires `buildifier --mode=check --lint=warn`
as a `command:` rule scoped to `**/BUILD`, `**/BUILD.bazel`, and
`**/*.bzl`. When `alint check` runs, the Starlark AST checks
fire through the same pass — `buildifier`'s findings appear in
alint's output formatted identically to alint's own findings.
Adopters get a single `alint check` invocation that covers
both layers without alint pretending to understand Starlark.

This is **the same orchestration pattern** as the structured
linters in the corpus:

- alint shells out to `ruff` / `black` / `pyink` for Python
  AST work
- alint shells out to `gofmt` / `go vet` for Go AST work
- alint shells out to `clang-format` / `clang-tidy` for C++
  AST work
- alint shells out to `clippy` / `cargo fmt --check` for Rust
  AST work
- alint shells out to **`buildifier` / `buildozer`** for
  Starlark AST work

The pattern is consistent: **alint owns the cross-language
file-structure layer; existing per-language tools own the
AST/semantic layer.**

### Pitfall #18 (now in CONFIG-AUTHORING.md, FIXED in v0.9.17): `.gitignore` masks tracked-file presence checks

**Originally surfaced by bazel — now the canonical example in the
21-pitfall catalogue.** `.bazelversion` IS tracked in git (precedence
wins for tracked files in git semantics) but is ALSO listed in `bazel`'s
own `.gitignore` (line 34). Contributors are expected to override
`.bazelversion` LOCALLY (different installed Bazel version), so the
file is gitignored to prevent local edits from drifting back into commits.

The `ignore` crate that alint's walker uses respects
`.gitignore` for discovery purposes. So:

- `git ls-files | grep .bazelversion` returns `.bazelversion`
  (file IS tracked)
- `alint check` against the tree silently doesn't see the file
  in its walked index
- `file_exists` rule with `paths: ".bazelversion"` reports
  "expected a file matching [.bazelversion] at the repo root"
  even though the file IS on disk and tracked

**FIXED in v0.9.17.** `file_exists` (and several siblings) now accept
a per-rule `respect_gitignore: false` knob that overrides the workspace
default for that one rule. The canonical fix:

```yaml
- id: bazel-version-pinned
  kind: file_exists
  paths: .bazelversion
  respect_gitignore: false   # ← new in v0.9.17
  root_only: true
  level: error
```

Verified directly against `/tmp/bazel/.bazelversion` during the
2026-05-07 revalidation pass: rule passes with the override, rule
fails ("expected a file matching [.bazelversion] at the repo root")
without it. Documented in CONFIG-AUTHORING.md pitfall #18 with all
three resolution options (per-rule knob, workspace-wide setting,
`command:` shellout fallback).

**Action item for this case study's `.alint.yml`:** the
`bazel-version-file-exists` rule was dropped in the original draft
to avoid the false negative. With v0.9.17 the rule can now be added
back using `respect_gitignore: false` — flagged as a follow-up in
the batch revalidation log; not auto-applied here per the
revalidation guard rails.

---

## Out of alint's scope (use the existing tool)

Same framing as the kubernetes / rust-lang/rust / cpython case
studies: AST-aware, codegen, build-graph, and deep-domain checks
stay on the existing tooling. alint's non-goals are deliberate.

- **`buildifier` / `buildozer`** — the Bazel team's official
  Starlark formatter + AST refactor tool. Owns the entire
  Starlark AST layer.
- **`bazel query //...`** — build-graph integrity, reverse-deps
  analysis, target visibility resolution.
- **`bazel build //src:bazel`** — the canonical self-build.
  THE actual correctness gate; runs in `.bazelci/presubmit.yml`.
- **`bazel mod deps --lockfile_mode=update`** — keeps
  MODULE.bazel.lock in sync with MODULE.bazel.
- **`oneversion_allowlist*.csv`** enforcement — Java classpath
  uniqueness; AST-aware analysis baked into the build.
- **`.bazelci/presubmit.yml` Buildkite orchestration** —
  309 lines of platform tasks across 25+ targets. Buildkite owns
  the cross-platform CI.
- **`.gemini/` config + styleguide** — Gemini Code Assist LLM
  context; no canonical schema yet.
- **`oneversion_allowlist*.csv` content audit** — domain-specific
  Java classpath manifest (no general-purpose CSV-shape rule).

---

## Already covered by other linters bazel uses

- **`buildifier`** — Starlark formatter + linter. alint shells
  out via `command:`.
- **`buildozer`** — Starlark AST refactor tool. alint defers
  entirely.
- **`pyink`** — Bazel-team Python formatter (a Black fork
  optimised for the 2-space-indent Google style). alint
  asserts `pyproject.toml` declares `[tool.pyink]` correctly
  but doesn't shell out to pyink directly (the Bazel team
  invokes it via pre-commit).
- **`shellcheck`** — POSIX shell linter. alint shells out via
  `command:`.
- **OpenSSF Scorecard (`scorecard.yml` workflow)** — supply
  chain security signals. alint covers the on-disk subset
  (CODEOWNERS exists, SECURITY.md exists, dependabot.yml
  exists, GHA Pinned-Dependencies); Scorecard covers
  branch-protection state and Maintained signals (which alint
  cannot see — they're GitHub API state).

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts:

- `oss-baseline@v1` (license, README, gitignore, no merge
  markers, no bidi)
- `java@v1` (no-tracked-target, no-tracked-build, no-class-files,
  PascalCase, source hygiene — but gates on `has_java` which is
  false for this repo since there's no pom.xml/build.gradle, so
  the rules silently no-op; the case-study config restates the
  conventions at the config layer without the fact gate)
- `ci/github-actions@v1` (workflow permissions, action pinning,
  workflow names)
- `hygiene/no-tracked-artifacts@v1` (no `.DS_Store`, build
  outputs, etc.)

Plus 41 bazel-specific rules covering:

- 7 Bazel-structural-file rules (`.bazelversion` shape +
  shape-check, MODULE.bazel exists + declares-name + lock-exists,
  no legacy WORKSPACE, root BUILD exists)
- 2 BUILD/*.bzl filename + suffix conventions
  (`bazel-build-file-naming`, `bazel-bzl-suffix-required`)
- 1 *.bzl Apache 2.0 license header rule
- 2 GitHub Actions Bazel-team hardening rules
  (`bazel-gha-uses-step-security-harden-runner`,
  `bazel-gha-checkout-pinned-major`)
- 4 `.gitattributes` invariant rules (`* -text`,
  `*.bzl linguist-language=Python`, `BUILD linguist-language=Python`,
  lockfile-merge driver)
- 3 `.bazelci/` Buildkite-CI rules (presubmit exists,
  postsubmit exists, presubmit declares `tasks:`)
- 7 top-level metadata rules (AUTHORS, CONTRIBUTORS, CHANGELOG,
  CODE_OF_CONDUCT, CONTRIBUTING, SECURITY, CODEOWNERS — all root-only)
- 5 Java source-tree convention rules (PascalCase, Apache header,
  no-trailing-whitespace, final-newline, no-bidi)
- 3 C++ source-tree convention rules (Apache header,
  no-trailing-whitespace, final-newline)
- 3 Python tooling rules (pyink config + 2-space indent +
  snake-case filenames)
- 2 Shell tooling rules (shellcheck shellout, Apache header)
- 1 Buildifier orchestration rule
- 1 Hygiene override (no `output/` from compile.sh)

The remaining inventoried surfaces:

- 6 shell out via `command:` rules (shellcheck +
  buildifier — counted above)
- 14 are out of alint's scope (above) — keep on the existing
  tooling

---

## Performance comparison (placeholder)

`buildifier --mode=check --lint=warn` on the full 322-BUILD-file
+ 100-bzl tree takes ~5s on a warm tree. The actual
`bazel build //src:bazel` self-build is multi-minute.

The structural-validation surface a new contributor reads today
to understand bazel's repo conventions:

- 309 lines of `.bazelci/presubmit.yml` (Buildkite tasks)
- 117 lines of `.bazelrc` (build flags)
- 17 lines of `.gitattributes` (EOL + linguist + merge driver)
- 100 lines of `CODEOWNERS`
- 482 lines of `MODULE.bazel`
- 9 GitHub Actions workflows
- the implicit Apache 2.0 header convention enforced socially
- the implicit `MODULE.bazel` ↔ `MODULE.bazel.lock` freshness
  convention
- the implicit BUILD-file-naming convention
- the implicit `*.bzl` license header convention

~1,000+ lines of structural-validation surface, half of which is
enforced only by code-review etiquette. The 41-rule alint config
in this directory plus the 4 extended bundled rulesets covers the
46 % subset that fits the structured-query grammar today.

---

## Followup feature work surfaced (priority order)

- **`generated_file_fresh`** — fourth confirmation across
  cpython, uv, arrow, bazel. Should land in v0.10+.
- **`*_path_contains`** — third confirmation across helm, deno,
  bazel. Should land in v0.10+.
- **`respect_gitignore: false` per-rule knob** (NEW pitfall #18
  fix) — surfaced uniquely by bazel. **SHIPPED in v0.9.17.**
  Verified working against `/tmp/bazel/.bazelversion` during the
  2026-05-07 revalidation pass.
- **`bazelrc_path_*` rule kind** (NEW) — niche to Bazel; rated
  low priority, logged for v0.10+ review.
- **`starlark_path_matches` / `starlark_glob_resolve` rule kind**
  (NEW) — would require a tree-sitter-starlark dep. Rated low
  priority; the right hand-off today is the `buildifier`
  shellout. Reconsider if multiple repos want it.

---

## Future analysis

Concrete analyses to follow up on now that the live tree is mounted at
`/tmp/bazel/`:

- **Re-add the `bazel-version-file-exists` rule** with `respect_gitignore:
  false` (verified working in v0.9.17 — see pitfall #18 section above) and
  validate against `/tmp/bazel/.bazelversion`. Same fix unlocks the
  `bazel-version-file-shape` rule which is also dormant against bazel's
  own tree today.
- **`alint suggest` against `/tmp/bazel/`** — surfaced two high-confidence
  proposals (`oss-baseline@v1`, `python@v1`) on the 2026-05-07 run; the
  latter caught the `pyproject.toml` for the pyink config. Worth re-running
  with `--explain` to understand why the heuristic missed `java@v1`
  (since `has_java` is false on a Bazel-built repo by design).
- **`bazel-buildifier-format-check` shellout reduction** — the v0.9.17 run
  produces 420 violations from this single rule, dominating the noise
  channel. Either narrow `paths:` to a representative subset or move the
  shellout to a separate `alint check --rules id_glob='bazel-buildifier-*'`
  invocation in CI so the `command_idempotent` v0.10 candidate (helm,
  prettier, ruff) absorbs it cleanly.

## Validation status (2026-05-07)

- alint version: v0.9.17
- Config validation: `validate-config` reports **80 rules loaded**.
  Reconciliation: 41 explicit rules in `.alint.yml` + 40 entries from
  extends (oss-baseline 15 + java 11 + ci/github-actions 3 +
  hygiene/no-tracked-artifacts 11) − 1 fact (`has_java` is an `- id:`
  entry but not a loadable rule) = 80. README's narrative
  "71 effective rules" is a conservative lower-bound; the precise count
  is 80.
- Live-tree status: `/tmp/bazel/` exists; `alint check` reports 14 failing
  rules + 38 passing rules (915 total violations). Top contributors:
  `bazel-buildifier-format-check` (420 violations — expected), `bazel-bazelci-presubmit-has-tasks` and
  related (.bazelci files genuinely have these), `oss-baseline` hygiene
  (BUILD files don't open with `#`-comment headers — 274 violations on
  `bazel-build-file-naming` alone, which is `info` level and represents
  bazel's historical convention rather than a true regression).
- Pitfall fixes shipped in v0.9.17: pitfall #18
  (`respect_gitignore: false` per-rule) — **directly applies to this
  repo's `.bazelversion`**. Verified working: rule passes with the
  override, rule fails without it.
- Open gaps: `starlark_path_*` family (low priority), `bazelrc_path_*`
  (niche to Bazel), `*_path_contains` (third confirmation across helm,
  deno, bazel — still v0.10 design).
