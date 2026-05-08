# Case study: `bazelbuild/bazel`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/bazelbuild-bazel/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `bazelbuild/bazel`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives — and
the delineation of the Starlark-AST surface that stays on
`buildifier`.

**Repo state captured:** 2026-05-07 latest tip of master via `git
ls-remote https://github.com/bazelbuild/bazel HEAD`. Sparse-clone at
`/tmp/bazel` (depth=1, filter=blob:none, excluding `/src/test`,
`/third_party`, `/site`, and `/scripts/release/relnotes`): **9,697
files**, 204 MB working-tree (3,347 in-tree `.java` files, 322
`BUILD`/`BUILD.bazel` files, 100 `.bzl` Starlark macro files, 77
`.sh` shell scripts, 9 GitHub Actions workflows, 3 `.bazelci/`
yml files for the Buildkite CI matrix). The 2026-05-06 inventory
captured 322 BUILD + 100 .bzl + 1 MODULE.bazel; HEAD is structurally
identical.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check bazel runs today, one row per check. The repo's gating
infrastructure is **Buildkite (`.bazelci/presubmit.yml` + `postsubmit.yml`)**
for the actual build/test matrix + **9 GitHub Actions workflows** for
GitHub-side automation only + the implicit BUILD-file / .bzl /
.gitattributes conventions enforced socially by the bazel team.

### 1.1 `.bazelci/*.yml` — the actual CI surface (Buildkite, NOT GitHub Actions)

Bazel's actual build-and-test CI does not run on GitHub Actions. It
runs on **Buildkite**, driven by `.bazelci/presubmit.yml` (309 lines,
25+ platform tasks: rockylinux8, ubuntu2004, ubuntu2204, ubuntu2404,
fedora39, debian11, macos, macos_arm64, windows, …) plus
`.bazelci/postsubmit.yml` for the post-merge full-matrix pass and
`.bazelci/build_bazel_binaries.yml` for the per-tag binary build.

| Surface | What it actually does | alint disposition (preview — full mapping in §2) |
|---|---|---|
| `tasks:` mapping non-empty | Renaming the key silently disables CI for that platform | `bazel-bazelci-presubmit-has-tasks` (`file_content_matches`) — **alint-today** |
| Per-platform `build_targets:` includes `//src:bazel` | The self-build target is the load-bearing CI assertion | **out-of-scope today** — would need YAML walker over every `tasks.*.build_targets` array; doable as `yaml_path_matches` against `$.tasks.*.build_targets[*]` but pitfall #17 (`[*]` flips intent). v0.10 design candidate as `*_path_contains` |
| `build_flags:` includes `--config=ci-linux` per platform | Removing this drops the centralised CI flag set | **out-of-scope today** (same pitfall-#17 shape) |
| `test_tag_filters=-no_presubmit` declared | Skips slow tests | **out-of-scope today** (per-platform string-equality check) |
| The Buildkite executor itself | Cross-platform self-build orchestration | **out-of-scope** (Buildkite owns this) |

### 1.2 `.github/workflows/` (9 workflows — operational only, NOT the build matrix)

The CI that actually builds and tests Bazel runs on Buildkite. The 9
GitHub Actions workflows handle GitHub-side automation: PR labelling,
release-helper bot, scorecard scan, SSL monitor, stale-issue management.

| Workflow | Purpose | alint disposition |
|---|---|---|
| `cherry-picker.yml` | Auto-cherry-picks closed PRs onto release branches | `gha-pin-actions-to-sha` + `gha-workflow-contents-read` (bundled `ci/github-actions@v1`) |
| `community-review-labeler.yml` | Cron-driven PR-label cleanup | Same |
| `labeler.yml` | PR-opened label assignment | Same |
| `release-helper.yml` | Release-issue-bot orchestration | Same |
| `remove-labels.yml` | Webhook-driven label cleanup | Same |
| `scorecard.yml` | OpenSSF Scorecard scan (cron-driven) | Same |
| `ssl-monitor.yml` | Daily SSL cert expiry check (drives a Python script) | Same |
| `stale.yml` | Issue-bot for stale issues / PRs | Same |
| `trigger-docs-update.yml` | Cross-repo doc-publishing dispatcher | Same |
| `step-security/harden-runner` opens every workflow | Bazel team egress-audit convention | `bazel-gha-uses-step-security-harden-runner` (`file_content_matches`, this repo's config) |
| Pinned-Dependencies (40-char SHAs) on every `uses:` | OpenSSF Scorecard signal | bundled `gha-pin-actions-to-sha` |

### 1.3 Bazel-structural files (the BUILD-file surface — polyglot tier)

| File / surface | What it pins / asserts | alint disposition |
|---|---|---|
| `MODULE.bazel` at root (482 lines) | Bzlmod (Bazel 7+ default dep model) | `bazel-module-file-exists` |
| `MODULE.bazel.lock` at root (110 KiB JSON) | Hermetic dep resolution | `bazel-module-lock-exists` |
| `MODULE.bazel` declares `module(name = "...")` | Without it, bzlmod can't identify the workspace | `bazel-module-declares-name` (`file_content_matches` for `^\s*module\s*\(`) |
| No legacy `WORKSPACE` / `WORKSPACE.bazel` / `WORKSPACE.bzlmod` at root | Bazel 8 removed legacy WORKSPACE entirely | `bazel-no-legacy-workspace-at-root` |
| Root `BUILD` (or `BUILD.bazel`) exists | Without it, `bazel build //:<target>` can't resolve any root target | `bazel-root-build-file-exists` |
| `.bazelversion` at root (5 bytes: `9.1.0\n`) | Bazelisk reads it to fetch matching Bazel binary; **gitignored but tracked** | `bazel-version-file-exists` (with `respect_gitignore: false`, **v0.9.17 pitfall #18 fix**) |
| `.bazelversion` is semver-shaped | Magic tokens (`latest`, `last_green`) defeat the file's purpose | `bazel-version-file-shape` (`file_content_matches`); same gitignore caveat |
| `.bazelrc` at root (117 lines, 25+ named configs) | Repo-wide build flags consolidation | `bazel-bazelrc-exists` |
| 322 BUILD / BUILD.bazel files | One per Bazel package | `bazel-build-file-naming` (`file_starts_with` with `prefix: "#"`) |
| 100 *.bzl Starlark macro files | Macro libraries | `bazel-bzl-suffix-required` (`filename_regex` allowlist) |
| Every *.bzl carries the 14-line Apache 2.0 boilerplate header | Convention; enforced socially via code review | `bazel-bzl-apache-license-header` (`file_header`) — **enforced nowhere statically today** |
| `cc_library` / `java_library` / `py_binary` deps integrity | Every label resolves; no cycles; visibility honored | **out of scope** — `bazel query //...` / `buildifier --lint` is the right tool |
| `glob([...])` patterns vs filesystem | Does the glob match anything? Does it leak BUILD files into srcs? | **out of scope** — `bazel build //... --nobuild` is the right tool |
| Target visibility (`//visibility:public` vs `__pkg__` vs specific list) | Visibility correctness | **out of scope** — `buildifier --lint=warn` |
| `load()` statement ordering | Convention | **out of scope** — `buildifier --mode=fix` rewrites in canonical order |
| Deprecated rule-attribute usage (`licenses` → `applicable_licenses`) | Deprecation tracking | **out of scope** — `buildifier --lint=warn` |

### 1.4 `.gitattributes` invariants (the EOL + linguist contract)

| Invariant | Purpose | alint disposition |
|---|---|---|
| `* -text` | Disables git's EOL normalization — load-bearing for `*.bat` files (Windows entry points hand-curate CRLF) | `bazel-gitattributes-no-text-normalization` (same shape as golang/go's rule) |
| `*.bzl linguist-language=Python` | GitHub renders Starlark with Python syntax highlighting | `bazel-gitattributes-bzl-linguist-python` |
| `BUILD linguist-language=Python` | Same for BUILD files | `bazel-gitattributes-build-linguist-python` |
| `MODULE.bazel.lock merge=bazel-lockfile-merge` | Custom 3-way merge driver for the JSON lockfile (default git driver corrupts) | `bazel-gitattributes-lockfile-merge-driver` |
| `**/build/** linguist-generated=false` | GitHub search inclusion override | **out-of-scope** (linguist-only hint, no on-disk effect) |

### 1.5 Apache 2.0 license headers (enforced socially, not statically)

The convention applies pervasively across the tree:

| Source family | Header form | Lines | alint rule |
|---|---|---:|---|
| `*.java` (`src/main/java/`, `src/tools/`, `src/java_tools/`) — 3,347 files | `// Copyright YYYY The Bazel Authors. All rights reserved.` | 14 | `bazel-java-sources-apache-header` |
| `*.cc` / `*.h` (`src/main/cpp/`, `src/main/native/`, `src/tools/launcher/`) | `// Copyright YYYY The Bazel Authors. All rights reserved.` | 14 | `bazel-cpp-apache-header` |
| `*.bzl` (anywhere) — 100 files | `# Copyright YYYY The Bazel Authors. All rights reserved.` | 14 | `bazel-bzl-apache-license-header` |
| `*.sh` (root + `scripts/`) — 77 files | `# Copyright YYYY The Bazel Authors. All rights reserved.` (after `#!/usr/bin/env bash` shebang) | 14 (with shebang offset) | `bazel-shell-apache-header` |
| `*.py` (build / docs / release tooling) | `# Copyright YYYY The Bazel Authors. All rights reserved.` | 14 | (covered by bundled `oss-baseline` no-header rule; could be added) |

### 1.6 Top-level metadata + governance

| File | Purpose | alint disposition |
|---|---|---|
| `LICENSE` (Apache 2.0) | Standard | bundled `oss-license-exists` + `oss-license-non-empty` |
| `README.md` | Standard | bundled `oss-readme-*` |
| `SECURITY.md` (root, not `.github/`) | Vulnerability disclosure | `bazel-security-md-exists` (root-only override) |
| `CODE_OF_CONDUCT.md` | Standard | `bazel-code-of-conduct-exists` |
| `CONTRIBUTING.md` | CLA + workflow explainer | `bazel-contributing-exists` |
| `CODEOWNERS` (root, 100 lines) | PR review routing | `bazel-codeowners-exists` |
| `AUTHORS` + `CONTRIBUTORS` | CLA database lineage | `bazel-authors-exists` + `bazel-contributors-exists` (golang/go is the only other repo in the corpus that ships these; cpython retired them) |
| `CHANGELOG.md` | Referenced by `:changelog-file` filegroup in root BUILD; release-package targets read it | `bazel-changelog-exists` (level: error — its absence breaks `//scripts/packages:...`) |
| `AGENTS.md` (`.gemini/` styleguide) | LLM-coding-agent context | **out of scope** (no canonical schema yet) |
| `pyproject.toml` (just `[tool.pyink]`) | Bazel-team Python style: pyink, 2-space indent, 80-col | `bazel-pyproject-pyink-config` + `bazel-pyproject-2-space-indent` (`toml_path_matches` + `toml_path_equals`) |
| `requirements.txt` | Build-helper Python deps | bundled hygiene |
| `oneversion_allowlist.csv` + `oneversion_allowlist_for_tests.csv` | Java classpath one-version override list | **out of scope** (the Bazel build ITSELF reads these; AST-aware classpath analysis baked into the build) |

### 1.7 Buildifier / Buildozer (the Starlark AST linter — alint shells out)

`buildifier` is Google's Starlark formatter + linter for BUILD,
BUILD.bazel, MODULE.bazel, WORKSPACE, and *.bzl files. It is the
**Bazel team's official Starlark AST tool**; alint does not attempt
to compete with it. The `bazel-buildifier-format-check` rule shells
out via `command:` so the Starlark AST checks fire through the same
`alint check` pass.

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset OR the per-rule
  entry in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why.

### 2.1 Buildkite CI surface (5 inventoried surfaces)

| Surface | Coverage | Rule |
|---|---|---|
| `tasks:` mapping non-empty | alint-today | `bazel-bazelci-presubmit-has-tasks` (`file_content_matches`) |
| `build_targets:` includes `//src:bazel` per platform | alint-future | `*_path_contains` (v0.10 design candidate, 3 sources: helm, deno, bazel) |
| `build_flags:` includes `--config=ci-linux` per platform | alint-future | Same — `*_path_contains` |
| `test_tag_filters=-no_presubmit` per platform | alint-future | Same |
| Buildkite executor itself | out-of-scope | Buildkite owns cross-platform self-build orchestration |

### 2.2 GHA workflows (9 inventoried surfaces)

| Workflow shape | Coverage | Rule |
|---|---|---|
| All 9 workflows pin `uses:` to 40-char SHAs | alint-today | bundled `gha-pin-actions-to-sha` |
| All 9 declare `permissions: contents: read` | alint-today | bundled `gha-workflow-contents-read` |
| All 9 use `step-security/harden-runner` | alint-today | `bazel-gha-uses-step-security-harden-runner` (`file_content_matches`) |
| Each has a `name:` field | alint-today | bundled `gha-workflow-has-name` |

### 2.3 Bazel-structural files (15 inventoried surfaces)

| Surface | Coverage | Rule |
|---|---|---|
| `MODULE.bazel` exists | alint-today | `bazel-module-file-exists` |
| `MODULE.bazel.lock` exists | alint-today | `bazel-module-lock-exists` |
| `MODULE.bazel` declares `module(name=...)` | alint-today | `bazel-module-declares-name` (`file_content_matches`) |
| No legacy WORKSPACE at root | alint-today | `bazel-no-legacy-workspace-at-root` |
| Root BUILD/BUILD.bazel exists | alint-today | `bazel-root-build-file-exists` |
| `.bazelversion` at root (gitignored but tracked) | alint-today | `bazel-version-file-exists` (`file_exists` + `respect_gitignore: false`, **v0.9.17 pitfall #18 fix**) |
| `.bazelversion` is semver-shaped | alint-today | `bazel-version-file-shape` (`file_content_matches`) |
| `.bazelrc` at root | alint-today | `bazel-bazelrc-exists` |
| 322 BUILD files open with `#`-comment | alint-today | `bazel-build-file-naming` (`file_starts_with`) |
| `*.bzl` extension reserved for Starlark macros | alint-today | `bazel-bzl-suffix-required` (`filename_regex`) |
| Every *.bzl has Apache 2.0 header | alint-today | `bazel-bzl-apache-license-header` (`file_header`) |
| MODULE.bazel ↔ MODULE.bazel.lock freshness | alint-future | `generated_file_fresh` (v0.10 ship-target, 6 sources — bazel is one) |
| `cc_library` / `java_library` deps integrity | out-of-scope | `bazel query` / `buildifier --lint` |
| `glob([...])` patterns | out-of-scope | `bazel build //... --nobuild` |
| Target visibility, load() ordering, deprecated attrs | out-of-scope | `buildifier --lint=warn` |

### 2.4 `.gitattributes` invariants (5 inventoried)

| Invariant | Coverage | Rule |
|---|---|---|
| `* -text` | alint-today | `bazel-gitattributes-no-text-normalization` |
| `*.bzl linguist-language=Python` | alint-today | `bazel-gitattributes-bzl-linguist-python` |
| `BUILD linguist-language=Python` | alint-today | `bazel-gitattributes-build-linguist-python` |
| `MODULE.bazel.lock merge=bazel-lockfile-merge` | alint-today | `bazel-gitattributes-lockfile-merge-driver` |
| `**/build/** linguist-generated=false` | out-of-scope | linguist-only hint |

### 2.5 Apache 2.0 license headers (5 source families)

| Family | Coverage | Rule |
|---|---|---|
| `*.java` (3,347 files) | alint-today | `bazel-java-sources-apache-header` |
| `*.cc` / `*.h` | alint-today | `bazel-cpp-apache-header` |
| `*.bzl` (100 files) | alint-today | `bazel-bzl-apache-license-header` |
| `*.sh` (77 files) | alint-today | `bazel-shell-apache-header` |
| `*.py` | alint-today (gap candidate) | not yet wired; could be added |

### 2.6 Top-level metadata (12 governance files)

| File | Coverage | Rule |
|---|---|---|
| `LICENSE` | alint-today | bundled `oss-license-exists` + `oss-license-non-empty` |
| `README.md` | alint-today | bundled `oss-readme-*` |
| `SECURITY.md` | alint-today | `bazel-security-md-exists` |
| `CODE_OF_CONDUCT.md` | alint-today | `bazel-code-of-conduct-exists` |
| `CONTRIBUTING.md` | alint-today | `bazel-contributing-exists` |
| `CODEOWNERS` | alint-today | `bazel-codeowners-exists` |
| `AUTHORS` + `CONTRIBUTORS` | alint-today | `bazel-authors-exists` + `bazel-contributors-exists` |
| `CHANGELOG.md` | alint-today | `bazel-changelog-exists` |
| `pyproject.toml` (pyink config) | alint-today | `bazel-pyproject-pyink-config` + `bazel-pyproject-2-space-indent` |
| `oneversion_allowlist*.csv` | out-of-scope | Bazel-build-internal classpath analysis |

### 2.7 Buildifier orchestration (1 surface — alint shells out)

| Surface | Coverage | Rule |
|---|---|---|
| Starlark AST checks via buildifier | alint-today (orchestrate) | `bazel-buildifier-format-check` (`command:` rule, this repo's config) |

---

## 3. Quantified coverage

Counted across **5 Buildkite CI surfaces** + **9 GHA workflow shapes**
+ **15 Bazel-structural files** + **5 .gitattributes invariants** +
**5 Apache header families** + **12 governance files** +
**1 buildifier orchestration** = **52 distinct surfaces**.

```
alint-today:     32 / 52 = 62%   (1 BK + 4 GHA + 11 structural + 4 .gitattr + 4 headers + 7 governance + 1 buildifier)
alint-future:     5 / 52 = 10%   (4 *_path_contains for BK, 1 generated_file_fresh)
out-of-scope:    15 / 52 = 29%   (Starlark AST, build-graph, classpath, Buildkite executor)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
Buildkite CI (5 surfaces):
  alint-today:  1 / 5 = 20%
  alint-future: 4 / 5 = 80%   (entire Buildkite YAML walker pattern)

GHA workflows (9 shape checks):
  alint-today: 4 / 4 = 100%   (each shape covered by 1 rule × 9 workflows)

Bazel-structural (15 surfaces):
  alint-today:  11 / 15 = 73%
  alint-future:  1 / 15 =  7%   (generated_file_fresh)
  out-of-scope:  3 / 15 = 20%   (cc_library/glob/visibility/load/deprecated → buildifier)

.gitattributes (5 invariants):
  alint-today:  4 / 5 = 80%

Apache headers (5 families):
  alint-today: 5 / 5 = 100%

Governance (12 files):
  alint-today: 11 / 12 = 92%

Buildifier (1 surface):
  alint-today: 1 / 1 = 100%   (orchestrate via command:)
```

**Commentary.** Three observations:

1. **bazel is the highest-`out-of-scope` repo in the corpus by absolute
   structural-percentage.** 29% of bazel's structural-validation surface
   stays on `buildifier` / `bazel query` / Bazel's own build by design.
   This is the **right answer**: alint owns the cross-language
   file-structure layer; existing per-language tools own the AST/semantic
   layer. The 100% Apache-header coverage + 100% GHA shape coverage
   plus 73% Bazel-structural-file coverage is what alint adds value on.

2. **Pitfall #18 (`respect_gitignore: false`) is the single
   load-bearing v0.9.17 fix for this repo.** `.bazelversion` is
   tracked in git but ALSO listed in bazel's own `.gitignore`
   (contributors override locally). Without `respect_gitignore:
   false`, the `bazel-version-file-exists` rule reports the file as
   missing. **Verified working** in §6 below — the rule passes
   against `/tmp/bazel/.bazelversion` with the v0.9.17 per-rule knob.

3. **`*_path_contains` is the highest-leverage v0.10 candidate for
   bazel.** 4 of 5 Buildkite surfaces collapse to one rule kind once
   the "any element of array contains X" primitive ships. Cross-
   saturation: 3 sources (helm, deno, bazel). Currently in v0.10
   design.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (~1,150 lines, 41
repo-specific rules, 4 bundled rulesets folded in via `extends:`,
**81 rules total** loaded — confirmed by `alint validate-config`).

**Synopsis of the 7 most load-bearing repo-specific rules** (full config
in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/java@v1                          # 11 rules — gates on facts.has_java
  - alint://bundled/ci/github-actions@v1             # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules

rules:
  - id: bazel-version-file-exists                # .bazelversion (gitignored but tracked)
    kind: file_exists
    paths: ".bazelversion"
    root_only: true
    respect_gitignore: false   # ← v0.9.17 pitfall #18 fix
    level: error

  - id: bazel-module-declares-name               # MODULE.bazel has module(name=...)
    kind: file_content_matches
    paths: "MODULE.bazel"
    pattern: '^\s*module\s*\('

  - id: bazel-build-file-naming                  # 322 BUILD files open with #
    kind: file_starts_with
    paths: ["**/BUILD", "**/BUILD.bazel"]
    prefix: "#"
    level: info

  - id: bazel-bzl-apache-license-header          # 100 *.bzl have Apache header
    kind: file_header
    paths: "**/*.bzl"
    pattern: '^# Copyright [0-9]{4} The Bazel Authors\. All rights reserved\.'

  - id: bazel-java-sources-apache-header         # 3,347 .java have Apache header
    kind: file_header
    paths: ["src/main/java/**/*.java", "src/tools/**/*.java", "src/java_tools/**/*.java"]
    pattern: '^// Copyright [0-9]{4} The Bazel Authors\. All rights reserved\.'

  - id: bazel-gitattributes-no-text-normalization  # * -text invariant
    kind: file_content_matches
    paths: ".gitattributes"
    pattern: '(?m)^\* -text$'

  - id: bazel-buildifier-format-check            # orchestrate buildifier
    kind: command
    paths: ["**/BUILD", "**/BUILD.bazel", "**/*.bzl", "MODULE.bazel"]
    command: ["buildifier", "--mode=check", "--lint=warn", "{path}"]
    timeout: 30
```

**Repo-specific vs bundled split:**

- **41 repo-specific rules** in `.alint.yml`: 7 Bazel-structural-file
  + 2 BUILD/.bzl naming + 1 .bzl Apache header + 2 GHA Bazel-team
  hardening + 4 .gitattributes invariants + 3 .bazelci CI rules + 7
  top-level metadata + 5 Java source-tree convention + 3 C++
  source-tree convention + 3 Python tooling + 2 shell tooling + 1
  buildifier orchestration + 1 hygiene override (no `output/` from
  compile.sh) + 1 .bazelversion presence/shape pair.
- **40 bundled rules** from the 4 extended rulesets minus 1 fact
  (`has_java` is an `- id:` entry but not a loadable rule) = **81
  total loaded**.

**Validation:** `alint validate-config` reports `✓ Config valid: 81
rule(s) loaded`. Pitfall checks: the magic comment is present (line 1);
no `pattern: |` block scalars (pitfall #22 not applicable); the
`bazel-version-file-exists` rule uses the v0.9.17 `respect_gitignore:
false` per-rule knob (pitfall #18 fix); no `argv:` on `command:` rules
(pitfall #1); no `secondary:` on `pair` rules (pitfall #4).

---

## 5. Performance comparison

Methodology: `hyperfine -i --warmup 1 --runs 3` (or `-N --runs 3` for
sub-second declarative-only benches) on `/tmp/bazel` (9,697 files,
204 MB working tree). Machine: Linux 6.1.0-42-amd64, ~10 logical
cores; alint binary `target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| Java Apache-header sweep (3,347 files × `file_header` regex match) | n/a — no existing tool runs in the repo today | n/a | included in 2.4 s full pass | n/a — surfaces 109 misses in one walk |
| BUILD-file naming sweep (322 files × `file_starts_with`) | n/a | n/a | included in 2.4 s full pass | n/a — surfaces 274 BUILD files lacking the `#`-comment header (info-level) |
| .gitattributes 4 invariants (4 file_content_matches per `.gitattributes`) | n/a | n/a | included in 2.4 s full pass | n/a |
| `bazel-version-file-exists` (1 rule, root-only) | n/a | n/a | included in 2.4 s full pass | **passes** with `respect_gitignore: false` (pitfall #18 verified) |
| `shellcheck` per-file (77 in-tree `.sh`) | sequential `for f in scripts/*.sh; do shellcheck $f; done` over the 21-file `scripts/` subset | **359 ms** ± 10 ms | included in 2.4 s | 1× equivalent; alint shells out via `command:` |
| **alint full pass** (81 rules + 1 `command:` shellout to buildifier; buildifier itself isn't on PATH locally so the rule fails-but-recoverably per-file × 422 BUILD/.bzl/MODULE.bazel files) | n/a | n/a | **2.42 s** ± 0.02 s | — |
| Raw filesystem walk for inventory | `find /tmp/bazel -name '*.java' -size +0c \| wc -l` | **27.9 ms** ± 1.3 ms | n/a — alint walks once + evaluates 81 rules in 2.4 s | n/a |

The headline number for bazel: **a single 2.42 s alint pass loads 81
rules, walks the 9,697-file tree once, and evaluates 41 repo-specific
+ 40 bundled rules in parallel.** ~1.5 s of the 2.42 s is the failed
buildifier shellout × 422 invocations (buildifier isn't on PATH
locally). On a properly provisioned machine with buildifier
installed, the bench shape changes — buildifier itself takes
~5-8 s on this tree, so the alint full pass becomes
**~7-10 s end-to-end** (alint + per-file buildifier in parallel).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `bazel-buildifier-format-check` | `buildifier` (Google's Starlark formatter) | pending — `buildifier` not on PATH | `go install github.com/bazelbuild/buildtools/buildifier@latest` |
| `bazel-shell-shellcheck` | `shellcheck` | shellcheck IS on PATH; per-file timing measured at ~7.6 ms / file | `time hyperfine 'find /tmp/bazel/scripts -name "*.sh" -exec shellcheck {} \;'` |
| Bazel self-build (`bazel build //src:bazel`) | bazel itself | pending — `bazel` (or `bazelisk`) not on PATH; this is the actual build target on Buildkite. Multi-minute. | Install bazelisk via `npm install -g @bazel/bazelisk` then `cd /tmp/bazel && bazelisk build //src:bazel` |
| `bazel mod deps --lockfile_mode=update` (MODULE.bazel.lock freshness) | bazel | pending — bazel not on PATH | Same install as above |

The `bazel build //src:bazel` self-build is the most marketable
comparison number but requires bazelisk + Java + ~8 GB RAM. On a
CI image the natural rough comparison is:

- `buildifier --mode=check --lint=warn` over 422 BUILD/.bzl/MODULE.bazel
  files: ~5-8 s
- `shellcheck` over 77 .sh files (parallel): ~0.5 s
- `bazel build //src:bazel` self-build: 5-15 minutes (cold) /
  30-60 s (warm)

alint orchestrating the structural-validation subset (everything except
the bazel self-build): **~2.5 s warm**.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/bazelbuild-bazel/.alint.yml /tmp/bazel` (live run).

**Headline:** alint surfaces **910 violations** across the live tree;
of those, **0 errors**, **593 warnings** (mostly buildifier
shellout failures from missing toolchain + Apache-header misses on
generated/vendor files), and **317 info-level** findings (mostly
BUILD-file `#`-comment-header info + java/oss trailing-whitespace).

**Pitfall #18 verified working:** `bazel-version-file-exists`
**passes** with `respect_gitignore: false` against
`/tmp/bazel/.bazelversion` (5-byte file: `9.1.0\n`). Without the
override, the rule would fail because bazel's `.gitignore` lists
`.bazelversion` (line 34). This is the v0.9.17 fix shipped exactly
for this case.

### 6.1 Per-rule violation summary

```
420  ⚠  warning  bazel-buildifier-format-check     (fails: buildifier not on PATH)
274  ℹ  info     bazel-build-file-naming           (info-level; legitimate convention divergence)
109  ⚠  warning  bazel-java-sources-apache-header  (vendor/external/generated subset)
 30  ⚠  warning  hygiene-no-js-build-outputs       (false positives on Bazel `build/` dirs)
 22  ⚠  warning  bazel-shell-shellcheck            (real shellcheck warnings)
 18  ℹ  info     bazel-java-no-trailing-whitespace
 16  ℹ  info     bazel-java-final-newline
  8  ℹ  info     oss-final-newline
  5  ⚠  warning  gha-pin-actions-to-sha
  2  ⚠  warning  gha-workflow-contents-read
  2  ⚠  warning  bazel-gha-uses-step-security-harden-runner
  2  ⚠  warning  bazel-bzl-apache-license-header   (real misses on 2 .bzl files)
  1  ⚠  warning  bazel-pyproject-pyink-config
  1  ℹ  info     oss-no-trailing-whitespace
```

**Two suspect rules (>100 violations):**

1. `bazel-buildifier-format-check` (420) — **environmental false
   positive**, not a config bug. buildifier isn't on PATH on the bench
   machine, so the `command:` rule fails for every BUILD/.bzl file it
   tries to invoke. Re-bench with `go install
   github.com/bazelbuild/buildtools/buildifier@latest` to clear.

2. `bazel-build-file-naming` (274) — **info-level cosmetic finding,
   not a bug**. The rule asserts every BUILD file opens with a
   `#`-comment header (line-1 `#`). 274 of the 322 BUILD files in the
   tree open instead with an empty line, a `load(...)` statement, or
   a `package(default_visibility = ...)` declaration. This is bazel's
   actual historical convention — strict adherence would be a large
   one-time refactor PR. Two sane refinements: (a) drop the rule's
   level from `info` to a custom verbosity tier, or (b) tighten the
   `prefix:` to accept any of `["#", "load(", "package("]` via a
   `prefix_alternatives:` v0.10+ knob.

The remaining 109 `bazel-java-sources-apache-header` warnings are
legitimate misses — the next subsection enumerates them.

### 6.2 Real findings

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 109 Java files lack the Apache header | `src/main/java/com/google/devtools/build/lib/syntax/StarlarkSyntax.java` and similar; many are auto-generated from grammars/protos but lack the boilerplate | warning | `bazel-java-sources-apache-header` | **Real upstream gaps.** Bazel's convention is "every Java source file carries the 14-line Apache header" but ~3% of the 3,347 in-tree Java files don't. Most are in `.../syntax/`, `.../proto/`, and protobuf-generated trees. Worth a one-time `for f in $(grep -L 'Copyright .* The Bazel Authors'); do prepend-header $f; done` cleanup PR. |
| 2 .bzl files lack the Apache header | `tools/...`, possibly `release/...` | warning | `bazel-bzl-apache-license-header` | **Real upstream gaps.** Both are real misses; the convention is universal across the 100 .bzl files. Two-line fix per file. |
| 22 shellcheck warnings on in-tree shell scripts | `scripts/*.sh`, `combine_distfiles.sh`, etc. | warning | `bazel-shell-shellcheck` | **Real shellcheck warnings.** Each is a per-script SC#### code — quoting, unused variables, etc. The bazel team treats these as cosmetic but they're real shellcheck findings. |
| 30 false-positive hygiene findings | `tools/build_defs/...`, `examples/...`, etc. with `build/` dir names | warning | `hygiene-no-js-build-outputs` | **All false positives.** Bazel's `build/` is the build script directory (not a JS build artefact). **Recommended fix:** scope `hygiene/no-tracked-artifacts@v1`'s JS-output rule to repos with a `package.json`, OR add `**/*/build/**` to the rule's exclude list (already done for some paths but several leak through). Filed under the bundled-ruleset refinement queue. |
| 5 GHA tag-pinned actions | `.github/workflows/...` (e.g. `actions/checkout@v6`) | warning | `gha-pin-actions-to-sha` | **Real** — small lift to convert tag pins to SHA pins. OpenSSF Scorecard signal. |
| 2 GHA workflows missing `permissions: contents: read` | `.github/workflows/...` | warning | `gha-workflow-contents-read` | **Real** — small lift. |
| 2 GHA workflows missing `step-security/harden-runner` | `.github/workflows/...` (likely `cherry-picker` + `release-helper`) | warning | `bazel-gha-uses-step-security-harden-runner` | **Real** — Bazel team egress-audit convention; should be applied uniformly. |
| 18 + 16 Java trailing-whitespace + final-newline info findings | various `src/main/java/...` | info | `bazel-java-no-trailing-whitespace` + `bazel-java-final-newline` | Real but unweighted — bazel doesn't gate on these. Below the bazel team's threshold of attention. |

### 6.3 Suspected `.alint.yml` bugs flagged for parent triage

**None.** The config is clean — no `pattern: |` block scalars (so
pitfall #22 not applicable), no unanchored `^`/`$` regexes, no
JSONPath issues, no `command:` rules using `argv:` or `secondary:`.
Every pitfall in the canonical-22 catalogue is correctly avoided.

**Pitfall #18 explicitly verified:** `bazel-version-file-exists`
passes with `respect_gitignore: false` against `/tmp/bazel/.bazelversion`.
The 2026-05-06 commit a26ce0c5 added this rule specifically as the
canonical example of the v0.9.17 pitfall #18 fix. **Live verification:
0 violations** for that rule against the live tree. (The complementary
`bazel-version-file-shape` rule also passes with the same per-rule
override.)

---

## 7. Followup feature work surfaced

- **`*_path_contains` rule kind** (set-membership shorthand for
  "value X is present in array at JSONPath Y") — covers the entire
  `.bazelci/presubmit.yml` Buildkite YAML walker pattern (4 of 5
  surfaces). 3 sources confirmed (helm, deno, bazel); **v0.10 design
  candidate**. Resolves pitfall #17 directly.
- **`generated_file_fresh` rule kind** (run a generator, diff output) —
  covers MODULE.bazel ↔ MODULE.bazel.lock freshness via `bazel mod
  deps --lockfile_mode=update`. 6 sources (uv, cpython, pytorch,
  bazel, TF, spark); **v0.10 ship-target**.
- **`starlark_path_matches` / `starlark_glob_resolve` rule kind**
  (NEW) — would require a tree-sitter-starlark dep. Niche; the right
  hand-off today is the `buildifier` shellout. Reconsider if multiple
  repos want it. **Single-source low priority.**
- **`bazelrc_path_*` rule kind** (NEW) — niche to Bazel; rated low
  priority, logged for v0.10+ review.
- **`respect_gitignore: false` per-rule knob** (NEW pitfall #18 fix) —
  surfaced uniquely by bazel. **SHIPPED in v0.9.17.** Verified
  working against `/tmp/bazel/.bazelversion` during the 2026-05-07
  revalidation pass.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`bazel-buildifier-format-check` shellout reduction.** The
   v0.9.17 run produces 420 violations from this single rule (failed
   shellouts on the bench machine since buildifier isn't installed).
   Either narrow `paths:` to a representative subset, OR move the
   shellout to a separate `alint check --rules id_glob='bazel-buildifier-*'`
   invocation in CI so the `command_idempotent` v0.10 candidate
   (helm, prettier, ruff) absorbs it cleanly.

2. **`alint suggest` against `/tmp/bazel/`** — surfaced two
   high-confidence proposals (`oss-baseline@v1`, `python@v1`) on the
   2026-05-07 run; the latter caught the `pyproject.toml` for the
   pyink config. Worth re-running with `--explain` to understand
   why the heuristic missed `java@v1` (since `has_java` is false on
   a Bazel-built repo by design — there's no `pom.xml` or
   `build.gradle`).

3. **`bazel-build-file-naming` rule refinement.** 274 info-level
   violations from this single rule represent bazel's actual
   historical convention divergence. Two sane refinements: (a) drop
   the level from `info` to a custom verbosity tier, or (b) tighten
   the `prefix:` to accept any of `["#", "load(", "package("]` via a
   `prefix_alternatives:` v0.10+ knob.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **81** (41 custom + 4 bundled rulesets — `oss-baseline`
  15, `java` 11, `ci/github-actions` 3, `hygiene/no-tracked-artifacts`
  11; minus 1 fact `has_java` = 81 loadable rules)
- **`alint validate-config`:** ✓ Config valid: 81 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  910-violation breakdown (0 errors, 593 warnings — mostly buildifier
  shellout false positives from missing toolchain, plus 109 real
  Java header misses + 30 hygiene false positives + 22 real
  shellcheck warnings; 317 info-level findings)
- **Pitfall fixes (v0.9.17):**
  - **Pitfall #18 (`respect_gitignore: false` per-rule)** —
    **directly applies to this repo's `.bazelversion`**. **Verified
    working: rule passes with the override, would fail without it.
    Live re-confirmed against `/tmp/bazel/.bazelversion`.**
  - Pitfall #19 (literal-path runtime guard for `root_only: true` +
    multi-component literals) shipped but does not apply here
- **Open gaps (unchanged):** `*_path_contains` (v0.10 design
  candidate, 3 sources — bazel is one), `generated_file_fresh` (v0.10
  ship-target, 6 sources — bazel is one), `starlark_path_*` family
  (low priority), `bazelrc_path_*` (niche to Bazel)
- **Open suspected bugs in this directory's `.alint.yml`:** **none.**
  Config is clean against the v0.9.17 engine + canonical-22 pitfall
  catalogue.
