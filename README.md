# alint

[![Crates.io](https://img.shields.io/github/v/tag/asamarts/alint?sort=semver&label=crates.io)](https://crates.io/crates/alint)
[![CI](https://github.com/asamarts/alint/actions/workflows/ci.yml/badge.svg)](https://github.com/asamarts/alint/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue)](#license)

**Fast, language-agnostic linter for repository structure, files, and content.** Declare the shape your repo should have (required files, filename conventions, content patterns, values inside `package.json` / `Cargo.toml` / GitHub workflows, cross-file relationships) in a single `.alint.yml`. alint enforces it.

- ⚡ **Fast at scale.** ~1.1 s on a 100K-file workspace bundle, ~12 s at 1M files. [Public benchmarks per release.](docs/benchmarks/HISTORY.md)
- 🤖 **Agent-aware.** First-class `agent` output format with per-violation `agent_instruction` strings; bundled `agent-hygiene` and `agent-context` rulesets for AI-touched repos.
- 🧰 **Powerful + extensible.** 89 rule kinds across 13 families, 22 bundled ecosystem rulesets, 12 auto-fix ops, 8 output formats, structured-query rules with full RFC 9535 JSONPath, cross-file relational rules, conditional `when:` gates over per-run facts, and `extends:` composition with SRI-pinned URLs.
- 📦 **One static Rust binary.** Any language, any repo. No plugin install, no Node/JVM/Python runtime needed.

Working `.alint.yml` configs for 30 OSS repos (single-language workspaces, polyglot monorepos, scale stress-tests) live under [`examples/`](examples/README.md), each with a writeup of what alint catches that the repo's existing tooling misses.

## 60-second quickstart

```sh
# Install (Linux + macOS + Windows tarballs):
curl -sSL https://raw.githubusercontent.com/asamarts/alint/main/install.sh | bash

# Initialise a config in the current repo (uses bundled oss-baseline + auto-detected ecosystem rulesets):
cat > .alint.yml <<'YAML'
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1                  # auto-skips when not a Rust repo
  - alint://bundled/ci/github-actions@v1
YAML

# Check:
alint check

# Auto-fix what's mechanically fixable (preview first):
alint fix --dry-run
alint fix
```

Bundled rulesets are gated by ecosystem facts (`has_rust`, `has_node`, `has_python`, …), so listing one for an ecosystem you don't have is a silent no-op. See [docs/rules.md](docs/rules.md) for the full rule catalogue and [alint.org](https://alint.org) for narrative docs.

## Core capabilities

- **89 rule kinds** across 13 families: existence, content, naming, structured query (RFC 9535 JSONPath over JSON/YAML/TOML/XML), text hygiene, security/unicode, encoding, structure, portable metadata, Unix metadata, git hygiene, cross-file relations, plugin (`command` shellout). Full reference: [`docs/rules.md`](docs/rules.md).
- **22 bundled rulesets**: `oss-baseline` (a strict superset of [Repolinter](https://github.com/todogroup/repolinter)'s default ruleset for users migrating from that tool, archived 2026-02), language sets (`rust`, `node`, `python`, `go`, `java`, `dotnet`, `php`), `ci/github-actions`, monorepo overlays (`cargo-workspace`, `pnpm-workspace`, `yarn-workspace`), hygiene (`no-tracked-artifacts`, `lockfiles`), tooling (`editorconfig`), docs (`adr`), compliance (`reuse`, `apache-2`), `apache/governance` (Apache TLP governance discipline), agent (`hygiene`, `context`). Built into the binary, no network round-trip; ecosystem-gated, so listing one for an absent ecosystem is a silent no-op.
- **Auto-fix**: 12 ops covering content edits (whitespace, newlines, line endings, BOM/bidi/zero-width strip, blank-line collapse) and path ops (create/remove/rename/prepend/append). Preview with `alint fix --dry-run`. Configurable `fix_size_limit` (default 1 MiB) skips oversize files rather than rewriting them.
- **Conditional rules**: a bounded `when:` expression language (boolean logic, comparisons, `matches`, `in`) gates rules on *facts* evaluated once per run (`any_file_exists`, `all_files_exist`, `count_files`).
- **Composition**: `extends:` pulls in other configs by local path, HTTPS URL (SRI-pinned), or `alint://bundled/<name>@<rev>`. Children override field-by-field. Monorepos can opt into `nested_configs: true` for auto-discovered subtree-scoped `.alint.yml` files.
- **10 subcommands**: `check` (default; supports `--changed` for PR-fast-path linting), `fix`, `init` (auto-detect ecosystem + scaffold), `validate-config` (parse-only; for editor LSP / pre-commit / fail-fast CI), `explain <rule>`, `list`, `suggest` (scan for antipatterns and propose rules), `facts` (debug `when:` clauses), `export-agents-md` (sync `AGENTS.md` from active rules), `lsp` (language server over stdio for editor integrations).
- **8 output formats**: `human`, `json` (stable schema), `sarif` (GitHub Code Scanning), `github` (inline PR annotations), `markdown` (PR comments), `junit` (CI test reports), `gitlab` (Code Quality), `agent` (LLM-shaped JSON with `agent_instruction` per violation).
- **JSON Schemas**: config at [`schemas/v1/config.json`](schemas/v1/config.json) for editor autocomplete; report shapes at [`schemas/v1/check-report.json`](schemas/v1/check-report.json) and [`schemas/v1/fix-report.json`](schemas/v1/fix-report.json) for downstream tooling.
- **Telemetry-free.** No network access at runtime, except the user-explicit `extends: https://...` URLs (SRI-pinned). Reproducible builds (`Cargo.lock` committed, pinned toolchain). See [SECURITY.md](SECURITY.md) for the threat model.
- **Official GitHub Action**: [`asamarts/alint@v0.12.0`](https://github.com/asamarts/alint).

## Where alint shines

alint isn't trying to be everything to everyone. The validation pass across 30 OSS repos surfaced five distinct shapes of project where alint earns its keep:

1. **Repos with verify-script sprawl.** *"Replaces the structural subset of N hand-rolled validation scripts."* Best fit: kubernetes (50 verify scripts → 17 declarative rules), apache/airflow (109 pre-commit hooks → ~40% map cleanly), python/cpython (12 validation surfaces consolidated into 1 alint config), microsoft/vscode (`build/hygiene.ts` → ~75% covered declaratively).
2. **Repos that rely on convention without explicit checks.** *"Catches the conventions your pipeline assumes but doesn't verify."* Best fit: tokio (zero hand-rolled scripts; alint catches 15 conventions tokio's pipeline silently assumes), uv (67-crate workspace conventions enforced nowhere in CI today), pnpm (replaces the in-tree `meta-updater` plugin), facebook/react, nodejs/node.
3. **Repos with mature tooling that lacks a structural layer.** *"Adds a structural floor on top of mature tooling."* Best fit: microsoft/typescript (eslint + dprint + knip already tight), astral-sh/ruff (900+ Python lint rules but zero rules for ruff's own internal-crate `publish = false` discipline), prettier, helm, dotnet/runtime (~2,300 XML manifests with structural invariants no existing tool covers).
4. **Repos that built their own lint-orchestration tool.** *"Replaces the structural subset of your custom orchestration layer."* Best fit: pytorch (≈86% of pytorch's 57 [`lintrunner.toml`](https://github.com/suo/lintrunner) adapters are structural; alint sits beneath, lintrunner keeps the AST-aware tail), bazel (alint replaces the structural subset of bazel's hand-rolled CI scripts).
5. **Tightly-curated minimal-tooling projects.** *"Encodes conventions enforced only by code-review discipline."* Best fit: golang/go (zero `.github/workflows/`, zero `Makefile`, zero `.golangci.yml`; the 31-rule alint config encodes the project's structural contract for the first time anywhere).

**Polyglot wins as a sixth shape:** when a single tree spans languages or platforms (apache/arrow's 6 languages, vercel/next.js's TS+Rust, NixOS/nixpkgs at 39k files, flutter's Dart-framework-with-6-native-embedders, protobuf's 11 language bindings), no per-language linter sees the cross-cutting conventions; alint is the layer that does.

If your repo doesn't match one of these shapes, alint is probably still useful (the rule catalogue is broad), but you may want to start by reading the closest case study above to see what a working config looks like in your shape.

## Non-goals

alint is deliberately **not**:

- a code / AST linter (use [ESLint](https://eslint.org/), [Clippy](https://doc.rust-lang.org/clippy/), [ruff](https://docs.astral.sh/ruff/))
- a SAST scanner (use [Semgrep](https://semgrep.dev/), [CodeQL](https://codeql.github.com/))
- an IaC scanner (use [Checkov](https://www.checkov.io/), [Conftest](https://www.conftest.dev/), [tfsec](https://aquasecurity.github.io/tfsec/))
- a commit-message linter (use [commitlint](https://commitlint.js.org/))
- a secret scanner (use [gitleaks](https://github.com/gitleaks/gitleaks), [trufflehog](https://github.com/trufflesecurity/trufflehog))

Scope is the filesystem shape and contents of a repository, not the semantics of the code inside it.

## Install

### install.sh (Linux + macOS + Windows tarballs)

```bash
curl -sSL https://raw.githubusercontent.com/asamarts/alint/main/install.sh | bash
```

Detects platform (Linux / macOS, x86_64 / aarch64), downloads the matching tarball, verifies the SHA-256, and installs to `$INSTALL_DIR` (default `~/.local/bin`). Windows users download the Windows tarball from the [Releases page](https://github.com/asamarts/alint/releases).

### Homebrew (macOS + Linuxbrew)

```bash
brew tap asamarts/alint
brew install alint
```

The [asamarts/homebrew-alint](https://github.com/asamarts/homebrew-alint) tap is auto-updated on every alint release. The formula downloads the matching pre-built binary, verifies its SHA-256, and installs to the Homebrew cellar.

### From crates.io

```bash
cargo install alint
```

### From npm

```bash
# project-local
npm install --save-dev @asamarts/alint
npx alint check

# global (puts `alint` on PATH)
npm install -g @asamarts/alint
alint check
```

The [@asamarts/alint](https://www.npmjs.com/package/@asamarts/alint) package is a thin shim that downloads the matching pre-built binary at install time, verifies its SHA-256 against the same `.sha256` companion `install.sh` and Homebrew use, and stages it under the package's `bin-platform/`. The package itself ships zero JS runtime behaviour. Set `ALINT_SKIP_INSTALL=1` to suppress the postinstall network hop in CI environments that snapshot `node_modules`.

### From source

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo build --release -p alint
./target/release/alint --help
```

### Docker

A distroless multi-arch image (`linux/amd64`, `linux/arm64`) is published to ghcr.io on each release:

```bash
# Lint the current directory:
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest

# Pin to an exact version:
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:v0.12.0 check
```

The image runs as the distroless `nonroot` user (UID 65532); host files must be world-readable. To apply fixes and preserve host ownership, pass `-u`:

```bash
docker run --rm -u $(id -u):$(id -g) -v "$PWD:/repo" ghcr.io/asamarts/alint:latest fix
```

Also published: `:<major>.<minor>` (e.g. `:0.10`) and the raw git tag (`:v0.12.0`).

## Quick start

The fastest on-ramp is `alint init`. It scans your repo for the obvious markers (Cargo.toml, package.json, pnpm-workspace.yaml, …) and writes a `.alint.yml` with the right `extends:` lines:

```bash
alint init             # ecosystem-aware (rust@v1, node@v1, …)
alint init --monorepo  # plus workspace overlays for Cargo / pnpm / Yarn
```

For an existing repo with prior debt, follow up with `alint suggest`. It scans for `*.bak` files, scratch docs at root, `console.log` residue in production source, and TODO markers older than 180 days, then proposes the bundled rulesets and rule entries that would catch them. Output is review-only; `suggest` never edits your config:

```bash
alint suggest                       # human-readable proposal table
alint suggest --format yaml         # paste-ready config snippet
alint suggest --format json         # stable shape for agent consumption
alint suggest --explain             # show file-level evidence per proposal
```

For agent-driven workflows where `AGENTS.md` / `CLAUDE.md` / `.cursorrules` carries the directives the agent reads at session start, `alint export-agents-md` renders the active rule set as a markdown directive block. alint becomes the single source of truth, and the agent reads what alint enforces:

```bash
alint export-agents-md                                # to stdout
alint export-agents-md --inline --output AGENTS.md    # splice between alint markers
```

`--inline` writes only between `<!-- alint:start -->` / `<!-- alint:end -->` markers; everything else in `AGENTS.md` is human-owned prose. Re-runs are idempotent (when the on-disk content already matches, no write happens), and missing markers auto-init with a stderr warning so the second run splices cleanly.

The generated file is editable: start there, override or extend as needed. If you'd rather hand-roll, the minimum viable shape is:

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
alint facts           # evaluate facts against the repo; debug `when:` clauses
alint init [--monorepo]  # scaffold a `.alint.yml` based on detected ecosystem + workspace shape
alint suggest            # scan for known antipatterns and propose rules to catch them
alint export-agents-md   # render the active rule set as an AGENTS.md directive section
```

Output formats:

```bash
alint check --format human    # default; colorized; grouped by file
alint check --format json     # stable, versioned JSON schema
alint check --format sarif    # SARIF 2.1.0 (for GitHub Code Scanning)
alint check --format github   # GitHub Actions workflow commands
alint check --format markdown # GFM, suited to PR comments / mkdocs
alint check --format junit    # JUnit XML, the de-facto CI test report
alint check --format gitlab   # GitLab Code Quality JSON (Code Climate spec)
```

Exit codes: `0` no errors; `1` one or more errors; `2` config error; `3` internal error. Warnings do not fail by default; use `--fail-on-warning` to flip that.

## Cookbook

Copy-pasteable recipes for composing bundled rulesets, structured-query rules over `package.json` / `Cargo.toml` / GitHub workflows, monorepo overlays + nested `.alint.yml`, conditional rules gated on per-run facts, custom `command` shellouts, fast PR-mode linting with `--changed`, auto-fix on commit, cross-file relationships, and per-iteration `when_iter:` filters.

Read at [alint.org/docs/cookbook/](https://alint.org/docs/cookbook/) (source: [`docs/site/cookbook/`](docs/site/cookbook/)).

## Bundled rulesets

Twenty-two rulesets ship in the binary, with zero network round-trip, pinned to the version of alint you're running:

**Ecosystem + project-shape baselines**

- **`oss-baseline@v1`**: README / LICENSE / SECURITY.md / CODE_OF_CONDUCT.md / .gitignore existence; minimum sensible file sizes; merge-marker + bidi-control bans; trailing-whitespace and final-newline hygiene (auto-fixable).
- **`rust@v1`**: Cargo.toml / Cargo.lock / rust-toolchain.toml existence; no committed `target/`; snake_case source filenames; Trojan-Source defenses. Gated with `when: facts.has_rust`.
- **`node@v1`**: package.json + lockfile; no committed `node_modules/`, `dist/`, `.next/`, etc.; Node-version pin via `.nvmrc` or `engines`; JS/TS source hygiene. Gated with `when: facts.has_node`.
- **`python@v1`**: manifest (pyproject.toml / setup.py / setup.cfg) exists; lockfile (uv / poetry / Pipenv / PDM); pyproject.toml declares `project.name` + `project.requires-python` via structured-query; PEP 8 snake_case module filenames; Trojan-Source defenses. Gated with `when: facts.has_python`.
- **`go@v1`**: go.mod + go.sum at root; go.mod declares `module <path>` + `go <version>`; Trojan-Source defenses on `*.go`. Gated with `when: facts.has_go`.
- **`java@v1`**: Maven (`pom.xml`) or Gradle (`build.gradle` / `build.gradle.kts`) manifest; build wrapper (`mvnw` / `gradlew`); no committed `target/` / `build/` (using `git_tracked_only` so locally-built dirs stay silent); no committed `*.class`; PascalCase Java filenames; Trojan-Source defenses. Gated with `when: facts.has_java`.
- **`dotnet@v1`**: root `global.json` exists + pins a concrete SDK version; SDK-style `.csproj`; nullable reference types enabled; Central Package Management actually enabled when `Directory.Packages.props` exists; no committed `bin/` / `obj/`; root `.editorconfig`. Gated with `when: facts.has_dotnet`.
- **`php@v1`**: composer.json `name` is a valid lowercase `vendor/package`; the **Composer-fatals invariants** (every `autoload.psr-4` directory, `autoload.files` entry, and `bin` script resolves on disk); no committed `vendor/`. Gated with `when: facts.has_php`.
- **`monorepo@v1`**: every `packages/*`, `crates/*`, `apps/*`, `services/*` directory has a README + ecosystem manifest; unique basenames.

**Workspace-aware overlays** (use `when_iter:` to scope per-member checks to actual package directories, so non-package dirs under `crates/` / `packages/` don't fire false positives)

- **`monorepo/cargo-workspace@v1`**: Cargo workspaces. Gated by `facts.is_cargo_workspace` (root `Cargo.toml` has `[workspace]`). Verifies `members = [...]` is declared and every workspace member has a README + `[package].name`.
- **`monorepo/pnpm-workspace@v1`**: pnpm workspaces. Gated by `facts.is_pnpm_workspace` (root `pnpm-workspace.yaml` exists). Verifies the `packages:` declaration and per-member README + `name`.
- **`monorepo/yarn-workspace@v1`**: Yarn / npm workspaces. Gated by `facts.is_yarn_workspace` (root `package.json` has `"workspaces"`). Per-member README + `name`, scoped to `{packages,apps}/*`.

**License compliance** (no fact gate; extending signals intent)

- **`compliance/reuse@v1`**: FSFE [REUSE Specification](https://reuse.software/) compliance: top-level `LICENSES/` directory + every source file declares both `SPDX-License-Identifier:` and `SPDX-FileCopyrightText:` in its first ~10 lines.
- **`compliance/apache-2@v1`**: Apache-2.0 compliance: LICENSE contains the Apache 2.0 text, root NOTICE file present, and every source file carries the canonical "Licensed under the Apache License, Version 2.0" header.

**Namespaced utilities**

- **`hygiene/no-tracked-artifacts@v1`**: build outputs (`node_modules`, `target`, `dist`, `__pycache__`, …), OS junk (`.DS_Store`, `Thumbs.db`), editor backups (`*~`, `*.swp`), secret-shaped files (`.env` and locals), and files over 10 MiB. Several rules auto-fixable via `file_remove`.
- **`hygiene/lockfiles@v1`**: enforce lockfiles (`yarn.lock`, `pnpm-lock.yaml`, `package-lock.json`, `bun.lock`, `Cargo.lock`, `poetry.lock`, `uv.lock`) live only at the workspace root.
- **`tooling/editorconfig@v1`**: root `.editorconfig` + `.gitattributes` with line-ending normalization.
- **`docs/adr@v1`**: MADR-style Architecture Decision Records under `docs/adr/`: `NNNN-kebab-title.md` filename + required `## Status` / `## Context` / `## Decision` sections.
- **`ci/github-actions@v1`**: GitHub Actions hardening guided by OpenSSF Scorecard: workflow-level `permissions.contents: read`, pin third-party actions to full commit SHAs, every workflow declares a `name:`. Scoped to `.github/workflows/*.y{,a}ml`, so it no-ops in repos that don't use GitHub Actions.

All rulesets ship with non-blocking defaults (`info` / `warning` for recommendations, `error` only for unambiguous bugs). Override severity or scope by redeclaring the rule id in your own `.alint.yml`, or disable with `level: off`. Per-ruleset rule lists in [docs/rules.md](docs/rules.md#bundled-rulesets).

## Use in CI

### GitHub Actions

Inline PR annotations (default):

```yaml
- uses: asamarts/alint@v0.12.0
```

All inputs (all optional):

```yaml
- uses: asamarts/alint@v0.12.0
  with:
    version: v0.12.0        # alint release tag (default: latest)
    path: .                # directory to lint (default: .)
    format: github         # human | json | sarif | github | markdown | junit | gitlab (default: github)
    config: |              # extra config path(s), one per line
      .alint.yml
    fail-on-warning: false
    args: ""               # extra CLI args appended verbatim
```

Upload findings to GitHub Code Scanning:

```yaml
- uses: asamarts/alint@v0.12.0
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
    rev: v0.12.0
    hooks:
      - id: alint
```

The hook runs `alint check` against the repo's `.alint.yml`. For auto-fix, add `id: alint-fix`. It's registered under `stages: [manual]` so it only runs when invoked explicitly (`pre-commit run alint-fix`), since fixers mutate the tree.

## Docs

- [**docs/rules.md**](docs/rules.md): per-rule user reference, one entry per rule kind with a YAML example and fix-op cross-reference.
- [**ARCHITECTURE.md**](docs/design/ARCHITECTURE.md): rule model, DSL, execution model, crate layout, plugin model.
- [**ROADMAP.md**](docs/design/ROADMAP.md): scope per version from v0.1 through v1.0.
- [**CHANGELOG.md**](CHANGELOG.md): per-version changes, breaking and otherwise.
- [**docs/benchmarks/METHODOLOGY.md**](docs/benchmarks/METHODOLOGY.md): how benchmarks are measured and published.
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
