# alint

[![Crates.io](https://img.shields.io/crates/v/alint.svg)](https://crates.io/crates/alint)
[![CI](https://github.com/asamarts/alint/actions/workflows/ci.yml/badge.svg)](https://github.com/asamarts/alint/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/alint.svg)](#license)

**alint** is a language-agnostic linter for repository structure. You declare the shape your repo should have — required files, filename conventions, content patterns, values inside `package.json` / `Cargo.toml` / GitHub workflows, cross-file relationships — in a single `.alint.yml`, and alint enforces it. It walks the tree honoring `.gitignore`, runs rules in parallel, reports violations in human / JSON / SARIF / GitHub-annotation form, and can auto-fix what it flags. One static Rust binary, any language, any repo.

v0.4 ships **~55 rule kinds** across eleven families and 12 auto-fix ops — see [docs/rules.md](docs/rules.md) for the full catalogue. alint fills the active-maintenance gap left when [Repolinter](https://github.com/todogroup/repolinter) was archived in early 2026, with a superset of its rule catalogue plus first-class cross-file, conditional-rule, and structured-query primitives.

## Core capabilities

- **~55 rule kinds** across eleven families (full reference: [docs/rules.md](docs/rules.md)):
  - *Existence* — `file_exists`, `file_absent`, `dir_exists`, `dir_absent`.
  - *Content* — `file_content_matches`, `file_content_forbidden`, `file_header`, `file_footer`, `file_shebang`, `file_starts_with`, `file_ends_with`, `file_hash`, `file_max_size`, `file_min_size`, `file_max_lines`, `file_min_lines`, `file_is_text`, `file_is_ascii`.
  - *Structured query* — `json_path_equals`, `json_path_matches`, `yaml_path_equals`, `yaml_path_matches`, `toml_path_equals`, `toml_path_matches`. JSONPath (RFC 9535) queries over JSON / YAML / TOML.
  - *Naming* — `filename_case`, `filename_regex`.
  - *Text hygiene* — `no_trailing_whitespace`, `final_newline`, `line_endings`, `line_max_width`, `indent_style`, `max_consecutive_blank_lines`.
  - *Security / Unicode* — `no_merge_conflict_markers`, `no_bidi_controls`, `no_zero_width_chars`.
  - *Encoding* — `no_bom`.
  - *Structure* — `max_directory_depth`, `max_files_per_directory`, `no_empty_files`.
  - *Portable metadata* — `no_case_conflicts`, `no_illegal_windows_names`.
  - *Unix metadata + git* — `no_symlinks`, `executable_bit`, `executable_has_shebang`, `shebang_has_executable`, `no_submodules`.
  - *Cross-file* — `pair`, `for_each_dir`, `for_each_file`, `dir_contains`, `dir_only_contains`, `unique_by`, `every_matching_has`.
- **Auto-fix** — 12 file ops covering content edits (trim whitespace, append newline, normalize line endings, strip BOM / bidi / zero-width, collapse blank lines) and path-level changes (create / remove / rename / prepend / append). Preview with `alint fix --dry-run`. Content-editing ops honour a configurable `fix_size_limit` (default 1 MiB) that skips oversize files rather than rewriting them.
- **Conditional rules** — a bounded `when:` expression language (boolean logic, comparisons, `matches` regex, `in` list membership) gates rules on *facts* evaluated once per run: `any_file_exists`, `all_files_exist`, `count_files`.
- **Composition** — `extends:` pulls in other configs by local path, HTTPS URL (with SRI pinning), or `alint://bundled/<name>@<rev>`. Children override inherited rules field-by-field. Monorepos can opt into `nested_configs: true` to auto-discover `.alint.yml` files in subdirectories and scope their rules to each subtree.
- **Twelve bundled rulesets** — `oss-baseline`, `rust`, `node`, `python`, `go`, `java`, `monorepo`, `hygiene/no-tracked-artifacts`, `hygiene/lockfiles`, `tooling/editorconfig`, `docs/adr`, `ci/github-actions`. Built into the binary — no network round-trip.
- **Four output formats** — `human`, `json` (stable schema), `sarif` (GitHub Code Scanning), `github` (inline PR annotations).
- **JSON Schema** at [`schemas/v1/config.json`](schemas/v1/config.json) for editor autocomplete.
- **Official GitHub Action** — `asamarts/alint@v0.5.0`.

## Non-goals

alint is deliberately **not**:

- a code / AST linter — use [ESLint](https://eslint.org/), [Clippy](https://doc.rust-lang.org/clippy/), [ruff](https://docs.astral.sh/ruff/)
- a SAST scanner — use [Semgrep](https://semgrep.dev/), [CodeQL](https://codeql.github.com/)
- an IaC scanner — use [Checkov](https://www.checkov.io/), [Conftest](https://www.conftest.dev/), [tfsec](https://aquasecurity.github.io/tfsec/)
- a commit-message linter — use [commitlint](https://commitlint.js.org/)
- a secret scanner — use [gitleaks](https://github.com/gitleaks/gitleaks), [trufflehog](https://github.com/trufflesecurity/trufflehog)

Scope is the filesystem shape and contents of a repository, not the semantics of the code inside it.

## Install

### Homebrew (macOS + Linuxbrew)

```bash
brew tap asamarts/alint
brew install alint
```

The [asamarts/homebrew-alint](https://github.com/asamarts/homebrew-alint) tap is auto-updated on every alint release — the formula downloads the matching pre-built binary, verifies its SHA-256, and installs to the Homebrew cellar.

### install.sh (Linux + macOS + Windows tarballs)

```bash
curl -sSL https://raw.githubusercontent.com/asamarts/alint/main/install.sh | bash
```

Detects platform (Linux / macOS, x86_64 / aarch64), downloads the matching tarball, verifies the SHA-256, and installs to `$INSTALL_DIR` (default `~/.local/bin`). Windows users download the Windows tarball from the [Releases page](https://github.com/asamarts/alint/releases).

### Docker

A distroless multi-arch image (`linux/amd64`, `linux/arm64`) is published to ghcr.io on each release:

```bash
# Lint the current directory:
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest

# Pin to an exact version:
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:v0.5.0 check
```

The image runs as the distroless `nonroot` user (UID 65532); host files must be world-readable. To apply fixes and preserve host ownership, pass `-u`:

```bash
docker run --rm -u $(id -u):$(id -g) -v "$PWD:/repo" ghcr.io/asamarts/alint:latest fix
```

Also published: `:<major>.<minor>` (e.g. `:0.5`) and the raw git tag (`:v0.5.0`).

### From crates.io

```bash
cargo install alint
```

### From source

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo build --release -p alint
./target/release/alint --help
```

## Quick start

The fastest on-ramp is a one-line bundled baseline — readable enough to
extend when you're ready:

```yaml
# .alint.yml
# yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/main/schemas/v1/config.json
version: 1
extends:
  - alint://bundled/oss-baseline@v1   # README/LICENSE/SECURITY.md, merge markers, hygiene
```

Then run:

```bash
alint check           # run all rules against the current directory
alint fix --dry-run   # preview the auto-fixes that would be applied
alint fix             # apply every fixable violation in place
alint list            # list effective rules (useful after extends / overrides)
alint explain <id>    # show a rule's full, resolved definition
alint facts           # evaluate facts against the repo — debug `when:` clauses
```

Output formats:

```bash
alint check --format human    # default; colorized; grouped by file
alint check --format json     # stable, versioned JSON schema
alint check --format sarif    # SARIF 2.1.0 (for GitHub Code Scanning)
alint check --format github   # GitHub Actions workflow commands
```

Exit codes: `0` no errors; `1` one or more errors; `2` config error; `3` internal error. Warnings do not fail by default — use `--fail-on-warning` to flip that.

## Cookbook

The patterns below are copy-pasteable. Each one targets a real repo-maintenance problem that has cost somebody time in production.

### 1. One-line baseline from a bundled ruleset

The shortest useful `.alint.yml` — adopt the OSS-hygiene baseline and nothing else. Good for "we just want README / LICENSE / no merge markers" rigour on a fresh repo.

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
```

### 2. Compose several bundled rulesets for a specific stack

A Rust monorepo wants OSS docs + Rust-idiomatic structure + layout checks + no tracked build artefacts:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1                              # Cargo.toml, target/ ban, snake_case
  - alint://bundled/monorepo@v1                          # every crate has README
  - alint://bundled/hygiene/no-tracked-artifacts@v1      # node_modules, target/, .DS_Store…
  - alint://bundled/hygiene/lockfiles@v1                 # Cargo.lock only at root
```

The Rust and Node rulesets are gated by facts (`when: facts.is_rust` / `facts.is_node`) and silently no-op in projects where they don't apply, so layering them is cheap.

### 3. Override a bundled rule without restating its body

Children in an `extends:` chain only need to declare the fields that change. The inherited `kind`, `paths`, `pattern`, etc. carry over:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1

rules:
  # Turn a warning into a blocking error for our repo:
  - id: oss-license-exists
    level: error

  # Silence a rule we've deliberately opted out of:
  - id: oss-code-of-conduct-exists
    level: off
```

Unknown-id overrides are flagged at config load, so typos don't silently pass.

### 3b. Adopt only part of a bundled ruleset

When you want most of a bundled ruleset but not all of it, filter at the `extends:` entry with `only:` or `except:` (mutually exclusive). Unknown rule ids in either list are flagged at load time.

```yaml
version: 1
extends:
  # Most of oss-baseline, minus the CoC nag:
  - url: alint://bundled/oss-baseline@v1
    except: [oss-code-of-conduct-exists]

  # Just the pinning check from the CI ruleset, nothing else:
  - url: alint://bundled/ci/github-actions@v1
    only: [gha-pin-actions-to-sha]
```

### 4. Enforce values inside `package.json` with structured queries

`json_path_equals` applies a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) query and checks the value. Missing fields are treated as violations (conservative default — scope narrowly if a field is truly optional).

```yaml
version: 1
rules:
  - id: require-mit-license
    kind: json_path_equals
    paths: "packages/*/package.json"
    path: "$.license"
    equals: "MIT"
    level: error

  - id: semver-package-version
    kind: json_path_matches
    paths: "packages/*/package.json"
    path: "$.version"
    matches: '^\d+\.\d+\.\d+$'
    level: error
```

### 5. Lock down GitHub Actions workflows

`yaml_path_equals` for workflow-wide permissions; `yaml_path_matches` for action-SHA pinning. Both use the same JSONPath engine — YAML is coerced through serde into a JSON value first, so array and wildcard expressions work the same way. If you want the full set without typing them, `extends: [alint://bundled/ci/github-actions@v1]` ships these rules plus a `name:` presence check.

`if_present: true` on the pinning rule means workflows with only `run:` steps (no `uses:` at all) are silently OK — the rule only fires on actual matches that fail the regex.

```yaml
version: 1
rules:
  # OpenSSF: workflows should declare `permissions.contents: read` explicitly.
  - id: workflow-contents-read
    kind: yaml_path_equals
    paths: ".github/workflows/*.yml"
    path: "$.permissions.contents"
    equals: "read"
    level: error

  # Security practice: pin third-party actions to a full commit SHA,
  # not a mutable @v4-style tag. `$.jobs.*.steps[*].uses` iterates
  # every step across every job. `if_present: true` skips workflows
  # that have no `uses:` at all.
  - id: pin-actions-to-sha
    kind: yaml_path_matches
    paths: ".github/workflows/*.yml"
    path: "$.jobs.*.steps[*].uses"
    matches: '^[a-zA-Z0-9._/-]+@[a-f0-9]{40}$'
    if_present: true
    level: warning
```

### 6. Enforce Cargo manifest shape across a workspace

`toml_path_equals` / `toml_path_matches` round out the structured-query family for Rust and Python (`pyproject.toml`) projects.

```yaml
version: 1
rules:
  - id: rust-edition-2024
    kind: toml_path_equals
    paths: "crates/*/Cargo.toml"
    path: "$.package.edition"
    equals: "2024"
    level: error

  - id: crate-version-follows-semver
    kind: toml_path_matches
    paths: "crates/*/Cargo.toml"
    path: "$.package.version"
    matches: '^\d+\.\d+\.\d+(-[\w.-]+)?$'
    level: error
```

### 7. Monorepo: every package has README + license + non-stub docs

`for_each_dir` iterates every directory matching `select:` and evaluates the nested `require:` block against each, substituting `{path}` with the iterated directory. `file_min_lines` catches the "README is a title plus `TODO`" case without being pedantic about word count.

```yaml
version: 1
rules:
  - id: every-package-is-documented
    kind: for_each_dir
    select: "packages/*"
    level: error
    require:
      - kind: file_exists
        paths: "{path}/README.md"

      - kind: file_min_lines
        paths: "{path}/README.md"
        min_lines: 5
        level: warning

      - kind: file_exists
        paths: ["{path}/LICENSE", "{path}/LICENSE.md"]
        level: warning
```

### 8. Nested `.alint.yml` for subtree-specific rules

Large repos rarely have a single policy. `nested_configs: true` auto-discovers `.alint.yml` files in subdirectories and scopes each nested rule's `paths` / `select` / `primary` to the subtree it lives in. The frontend team can own `packages/frontend/.alint.yml` without waiting on root-config review:

```yaml
# .alint.yml (repo root)
version: 1
nested_configs: true
extends:
  - alint://bundled/oss-baseline@v1
```

```yaml
# packages/frontend/.alint.yml
version: 1
rules:
  - id: components-are-pascal-case
    kind: filename_case
    paths: "components/**/*.{tsx,jsx}"   # auto-scoped to packages/frontend/**
    case: pascal
    level: error
```

MVP guardrails: nested rules must declare at least one scope field; absolute paths and `..`-prefixed globs are rejected; duplicate rule ids across configs surface with a clear message.

### 9. Auto-fix hygiene on commit

Pair a low-severity rule with a fixer and let `alint fix` do the boring part. Ideal for pre-commit or editor-save hooks.

```yaml
version: 1
rules:
  - id: trim-trailing-whitespace
    kind: no_trailing_whitespace
    paths: ["**/*.md", "**/*.rs", "**/*.yml"]
    level: info
    fix:
      file_trim_trailing_whitespace: {}

  - id: final-newline
    kind: final_newline
    paths: ["**/*.md", "**/*.rs", "**/*.yml"]
    level: info
    fix:
      file_append_final_newline: {}

  - id: no-bak-files
    kind: file_absent
    paths: "**/*.{bak,swp,orig}"
    level: warning
    fix:
      file_remove: {}
```

Preview with `alint fix --dry-run`; apply with `alint fix`. Content-editing fixers honour `fix_size_limit` (default 1 MiB) and skip oversize files rather than rewriting them.

### 10. Conditional rules gated on repo facts

Facts are evaluated once per run and referenced in `when:`. Here: only enforce snake_case Rust filenames when the repo actually *is* a Rust project.

```yaml
version: 1

facts:
  - id: is_rust
    any_file_exists: [Cargo.toml]
  - id: is_typescript
    any_file_exists: ["tsconfig.json", "packages/*/tsconfig.json"]

rules:
  - id: rust-snake-case
    when: facts.is_rust
    kind: filename_case
    paths: "src/**/*.rs"
    case: snake
    level: error

  - id: ts-kebab-case
    when: facts.is_typescript and not (facts.is_rust)
    kind: filename_case
    paths: "src/**/*.ts"
    case: kebab
    level: warning
```

### 11. Cross-file relationships

`pair` and `unique_by` cover the "every X has a matching Y" and "no two files share a derived key" cases — the ones that ad-hoc shell pipelines usually get wrong on the edges. Template tokens are `{path}`, `{dir}`, `{basename}`, `{stem}`, `{ext}`, `{parent_name}`.

```yaml
version: 1
rules:
  # Every `*.c` source file has a same-directory `*.h` header:
  - id: every-c-has-a-header
    kind: pair
    primary: "src/**/*.c"
    partner: "{dir}/{stem}.h"
    level: error

  # No two Rust source files share a stem anywhere in the repo — a
  # frequent mod-path surprise in larger workspaces:
  - id: unique-rs-stems
    kind: unique_by
    select: "**/*.rs"
    key: "{stem}"
    level: warning
```

### 12. Ban risky characters / files outright

The security-family rules catch categories that are almost never intentional. Trojan-Source (CVE-2021-42574), zero-width tricks, and stray merge markers all lead to "I didn't write that" incidents.

```yaml
version: 1
rules:
  - id: no-merge-markers
    kind: no_merge_conflict_markers
    paths: ["**/*"]
    level: error

  - id: no-bidi
    kind: no_bidi_controls
    paths: ["**/*"]
    level: error
    fix:
      file_strip_bidi_controls: {}

  - id: no-zero-width
    kind: no_zero_width_chars
    paths: ["**/*"]
    level: error
    fix:
      file_strip_zero_width: {}

  - id: no-committed-env
    kind: file_absent
    paths: [".env", ".env.*.local"]
    level: error
```

### 13. Lint only what changed (pre-commit / PR-fast-path)

`--changed` restricts the check to files in the working-tree
diff (or `<base>...HEAD`'s merge-base diff). Per-file rules
evaluate only against changed files in scope; cross-file
rules (`pair`, `for_each_dir`, `every_matching_has`,
`unique_by`, `dir_contains`, `dir_only_contains`) and
existence rules (`file_exists`, `file_absent`, …) keep
full-tree semantics so an unchanged-but-broken state still
surfaces. Empty diffs short-circuit to an empty report.

```bash
# Pre-commit: lint the working-tree diff
# (`git ls-files --modified --others --exclude-standard`).
alint check --changed

# PR check: lint everything that diverged from main
# (`git diff --name-only --relative main...HEAD`).
alint check --changed --base=main --format=sarif
```

Pairs with the pre-commit hook (the hook can pass
`--changed` via `args:`) and with `git_tracked_only: true`
on absence rules so locally-built artefacts never fire.

## Bundled rulesets

Eight rulesets ship in the binary — zero network round-trip, pinned to the version of alint you're running:

**Ecosystem + project-shape baselines**

- **`oss-baseline@v1`** — README / LICENSE / SECURITY.md / CODE_OF_CONDUCT.md / .gitignore existence; minimum sensible file sizes; merge-marker + bidi-control bans; trailing-whitespace and final-newline hygiene (auto-fixable).
- **`rust@v1`** — Cargo.toml / Cargo.lock / rust-toolchain.toml existence; no committed `target/`; snake_case source filenames; Trojan-Source defenses. Gated with `when: facts.is_rust`.
- **`node@v1`** — package.json + lockfile; no committed `node_modules/`, `dist/`, `.next/`, etc.; Node-version pin via `.nvmrc` or `engines`; JS/TS source hygiene. Gated with `when: facts.is_node`.
- **`python@v1`** — manifest (pyproject.toml / setup.py / setup.cfg) exists; lockfile (uv / poetry / Pipenv / PDM); pyproject.toml declares `project.name` + `project.requires-python` via structured-query; PEP 8 snake_case module filenames; Trojan-Source defenses. Gated with `when: facts.is_python`.
- **`go@v1`** — go.mod + go.sum at root; go.mod declares `module <path>` + `go <version>`; Trojan-Source defenses on `*.go`. Gated with `when: facts.is_go`.
- **`java@v1`** — Maven (`pom.xml`) or Gradle (`build.gradle` / `build.gradle.kts`) manifest; build wrapper (`mvnw` / `gradlew`); no committed `target/` / `build/` (using `git_tracked_only` so locally-built dirs stay silent); no committed `*.class`; PascalCase Java filenames; Trojan-Source defenses. Gated with `when: facts.is_java`.
- **`monorepo@v1`** — every `packages/*`, `crates/*`, `apps/*`, `services/*` directory has a README + ecosystem manifest; unique basenames.

**Namespaced utilities**

- **`hygiene/no-tracked-artifacts@v1`** — build outputs (`node_modules`, `target`, `dist`, `__pycache__`, …), OS junk (`.DS_Store`, `Thumbs.db`), editor backups (`*~`, `*.swp`), secret-shaped files (`.env` and locals), and files over 10 MiB. Several rules auto-fixable via `file_remove`.
- **`hygiene/lockfiles@v1`** — enforce lockfiles (`yarn.lock`, `pnpm-lock.yaml`, `package-lock.json`, `bun.lock`, `Cargo.lock`, `poetry.lock`, `uv.lock`) live only at the workspace root.
- **`tooling/editorconfig@v1`** — root `.editorconfig` + `.gitattributes` with line-ending normalization.
- **`docs/adr@v1`** — MADR-style Architecture Decision Records under `docs/adr/`: `NNNN-kebab-title.md` filename + required `## Status` / `## Context` / `## Decision` sections.
- **`ci/github-actions@v1`** — GitHub Actions hardening guided by OpenSSF Scorecard: workflow-level `permissions.contents: read`, pin third-party actions to full commit SHAs, every workflow declares a `name:`. Scoped to `.github/workflows/*.y{,a}ml`, so it no-ops in repos that don't use GitHub Actions.

All rulesets ship with non-blocking defaults (`info` / `warning` for recommendations, `error` only for unambiguous bugs). Override severity or scope by redeclaring the rule id in your own `.alint.yml`, or disable with `level: off`. Per-ruleset rule lists in [docs/rules.md](docs/rules.md#bundled-rulesets). More rulesets (`java`, `compliance/reuse`, `compliance/apache-2`) are planned for v0.5.

## Use in CI

### GitHub Actions

Inline PR annotations (default):

```yaml
- uses: asamarts/alint@v0.5.0
```

All inputs (all optional):

```yaml
- uses: asamarts/alint@v0.5.0
  with:
    version: v0.5.0        # alint release tag (default: latest)
    path: .                # directory to lint (default: .)
    format: github         # human | json | sarif | github (default)
    config: |              # extra config path(s), one per line
      .alint.yml
    fail-on-warning: false
    args: ""               # extra CLI args appended verbatim
```

Upload findings to GitHub Code Scanning:

```yaml
- uses: asamarts/alint@v0.5.0
  id: alint
  with:
    format: sarif
  continue-on-error: true
- uses: github/codeql-action/upload-sarif@v3
  if: always()
  with:
    sarif_file: ${{ steps.alint.outputs.sarif-file }}
```

### pre-commit

Add to your `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: https://github.com/asamarts/alint
    rev: v0.5.0
    hooks:
      - id: alint
```

The hook runs `alint check` against the repo's `.alint.yml`. For auto-fix, add `id: alint-fix` — it's registered under `stages: [manual]` so it only runs when invoked explicitly (`pre-commit run alint-fix`), since fixers mutate the tree.

## Docs

- [**docs/rules.md**](docs/rules.md) — per-rule user reference, one entry per rule kind with a YAML example and fix-op cross-reference.
- [**ARCHITECTURE.md**](docs/design/ARCHITECTURE.md) — rule model, DSL, execution model, crate layout, plugin model.
- [**ROADMAP.md**](docs/design/ROADMAP.md) — scope per version from v0.1 through v1.0.
- [**CHANGELOG.md**](CHANGELOG.md) — per-version changes, breaking and otherwise.
- [**docs/benchmarks/METHODOLOGY.md**](docs/benchmarks/METHODOLOGY.md) — how benchmarks are measured and published.
- Per-version, per-platform benchmark results under [`docs/benchmarks/<version>/`](docs/benchmarks/).

## Development

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo test --workspace        # 450+ tests; includes end-to-end scenarios
cargo run -- check            # dogfood: alint lints itself
cargo bench -p alint-bench    # criterion micro-benches
```

End-to-end tests live in `crates/alint-e2e/scenarios/` as declarative YAML; adding a new scenario only requires a new file. CLI snapshot tests live in `crates/alint/tests/cli/` under `trycmd`. Property-based invariants are in `crates/alint-e2e/tests/invariants.rs`.

CI is self-hosted with per-job bash scripts under `ci/scripts/` that run locally or in GitHub Actions unchanged. See [ci/env.example](ci/env.example) for runner setup.

## License

Dual-licensed under either of:

- [Apache License 2.0](LICENSE-APACHE) ([SPDX `Apache-2.0`](https://spdx.org/licenses/Apache-2.0.html))
- [MIT License](LICENSE-MIT) ([SPDX `MIT`](https://spdx.org/licenses/MIT.html))

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in alint shall be dual-licensed as above, without any additional terms or conditions.
