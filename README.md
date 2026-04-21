# alint

[![Crates.io](https://img.shields.io/crates/v/alint.svg)](https://crates.io/crates/alint)
[![CI](https://github.com/asamarts/alint/actions/workflows/ci.yml/badge.svg)](https://github.com/asamarts/alint/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/alint.svg)](#license)

**alint** (short for *agnostic lint*) is a language-agnostic linter for **repository structure, filenames, and file content rules**, with optional auto-fix.

> Status: v0.3 ships ~42 rule kinds across ten families, auto-fix with 12 ops, and conditional rules via a bounded expression language. See [docs/rules.md](docs/rules.md) for the full catalogue and [docs/design/ROADMAP.md](docs/design/ROADMAP.md) for scope per version.

## What alint does

alint enforces declarative rules over a repository tree. Rules live in a `.alint.yml` at the root; alint walks the tree (honoring `.gitignore`), matches rules against every file and directory, reports violations, and — when you ask — automatically fixes them.

### Core capabilities

- **~42 rule kinds** across ten families (full reference: [docs/rules.md](docs/rules.md)):
  - *Existence* — `file_exists`, `file_absent`, `dir_exists`, `dir_absent`.
  - *Content* — `file_content_matches`, `file_content_forbidden`, `file_header`, `file_starts_with`, `file_ends_with`, `file_hash`, `file_max_size`, `file_is_text`, `file_is_ascii`.
  - *Naming* — `filename_case`, `filename_regex`.
  - *Text hygiene* — `no_trailing_whitespace`, `final_newline`, `line_endings`, `line_max_width`, `indent_style`, `max_consecutive_blank_lines`.
  - *Security / Unicode* — `no_merge_conflict_markers`, `no_bidi_controls`, `no_zero_width_chars`.
  - *Encoding* — `no_bom`.
  - *Structure* — `max_directory_depth`, `max_files_per_directory`, `no_empty_files`.
  - *Portable metadata* — `no_case_conflicts`, `no_illegal_windows_names`.
  - *Unix metadata* — `no_symlinks`, `executable_bit`, `executable_has_shebang`, `shebang_has_executable`.
  - *Git hygiene* — `no_submodules`.
  - *Cross-file* — `pair`, `for_each_dir`, `for_each_file`, `dir_contains`, `dir_only_contains`, `unique_by`, `every_matching_has`.
- **Auto-fix** — 12 file ops covering content edits (trim whitespace, append newline, normalize line endings, strip BOM / bidi / zero-width, collapse blank lines) and path-level changes (create / remove / rename / prepend / append). Preview with `alint fix --dry-run`. Content-editing ops honour a configurable `fix_size_limit` (default 1 MiB) that skips oversize files rather than rewriting them.
- **Conditional rules** — a bounded `when:` expression language (boolean logic, comparisons, `matches` regex, `in` list membership) gates rules on *facts* evaluated once per run: `any_file_exists`, `all_files_exist`, `count_files`.
- **Four output formats** — `human`, `json` (stable schema), `sarif` (GitHub Code Scanning), `github` (inline PR annotations).
- **JSON Schema** at [`schemas/v1/config.json`](schemas/v1/config.json) for editor autocomplete.
- **Official GitHub Action** — `asamarts/alint@v0.3.0`.

### Typical use cases

- "Every package in a monorepo has a `README.md` and a `LICENSE*`" — `dir_contains` across `packages/*`.
- "All Rust source files carry a copyright header; auto-prepend any that don't" — `file_header` + `file_prepend`.
- "No stray `*.bak` or `*.swp` files in committed history; delete any that slip in" — `file_absent` + `file_remove`.
- "Filename case convention enforced per language" — `filename_case` with `when: facts.is_typescript` gating.
- "Every module directory has a `mod.rs`" — `for_each_dir` with nested `file_exists`.
- "No two files share a basename across the tree" — `unique_by` with `key: "{basename}"`.

The full DSL is documented in [docs/design/ARCHITECTURE.md](docs/design/ARCHITECTURE.md).

## Non-goals

alint is deliberately **not**:

- a code / AST linter — use [ESLint](https://eslint.org/), [Clippy](https://doc.rust-lang.org/clippy/), [ruff](https://docs.astral.sh/ruff/)
- a SAST scanner — use [Semgrep](https://semgrep.dev/), [CodeQL](https://codeql.github.com/)
- an IaC scanner — use [Checkov](https://www.checkov.io/), [Conftest](https://www.conftest.dev/), [tfsec](https://aquasecurity.github.io/tfsec/)
- a commit-message linter — use [commitlint](https://commitlint.js.org/)
- a secret scanner — use [gitleaks](https://github.com/gitleaks/gitleaks), [trufflehog](https://github.com/trufflesecurity/trufflehog)

Scope is the filesystem shape and contents of a repository, not the semantics of the code inside it.

## Install

### From a tagged release (recommended)

```bash
curl -sSL https://raw.githubusercontent.com/asamarts/alint/main/install.sh | bash
```

Detects platform (Linux / macOS, x86_64 / aarch64), downloads the matching tarball, verifies the SHA-256, and installs to `$INSTALL_DIR` (default `~/.local/bin`). Windows users download the Windows tarball from the [Releases page](https://github.com/asamarts/alint/releases).

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

Create a `.alint.yml` at the root of your repository:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/main/schemas/v1/config.json
version: 1

facts:
  - id: is_rust
    any_file_exists: [Cargo.toml]

rules:
  - id: readme-exists
    kind: file_exists
    paths: ["README.md", "README"]
    root_only: true
    level: error
    fix:
      file_create:
        content: "# Project\n"

  - id: no-backup-files
    kind: file_absent
    paths: "**/*.{bak,swp}"
    level: warning
    fix:
      file_remove: {}

  - id: components-pascal
    kind: filename_case
    paths: "components/**/*.{tsx,jsx}"
    case: pascal
    level: error
    fix:
      file_rename: {}

  - id: rust-snake
    when: facts.is_rust
    kind: filename_case
    paths: "src/**/*.rs"
    case: snake
    level: error
    fix:
      file_rename: {}

  - id: java-license-header
    kind: file_header
    paths: "**/*.java"
    lines: 20
    pattern: "(?s)Copyright \\(c\\) \\d{4}"
    level: error
    fix:
      file_prepend:
        content: |
          // Copyright (c) 2026 Acme Corp
```

Then run:

```bash
alint check           # run all rules against the current directory
alint fix --dry-run   # preview the auto-fixes that would be applied
alint fix             # apply every fixable violation in place
alint list            # list effective rules
alint explain <id>    # show a rule's definition
```

Output formats:

```bash
alint check --format human    # default; colorized for humans
alint check --format json     # stable, versioned JSON schema
alint check --format sarif    # SARIF 2.1.0 (for GitHub Code Scanning)
alint check --format github   # GitHub Actions workflow commands
```

Exit codes: `0` no errors; `1` one or more errors; `2` config error; `3` internal error. Warnings do not fail by default — use `--fail-on-warning` to flip that.

## Use in CI

### GitHub Actions

Inline PR annotations (default):

```yaml
- uses: asamarts/alint@v0.3.0
```

All inputs (all optional):

```yaml
- uses: asamarts/alint@v0.3.0
  with:
    version: v0.3.0        # alint release tag (default: latest)
    path: .                # directory to lint (default: .)
    format: github         # human | json | sarif | github (default)
    config: |              # extra config path(s), one per line
      .alint.yml
    fail-on-warning: false
    args: ""               # extra CLI args appended verbatim
```

Upload findings to GitHub Code Scanning:

```yaml
- uses: asamarts/alint@v0.3.0
  id: alint
  with:
    format: sarif
  continue-on-error: true
- uses: github/codeql-action/upload-sarif@v3
  if: always()
  with:
    sarif_file: ${{ steps.alint.outputs.sarif-file }}
```

## Docs

- [**ARCHITECTURE.md**](docs/design/ARCHITECTURE.md) — rule model, DSL, execution model, crate layout, plugin model.
- [**ROADMAP.md**](docs/design/ROADMAP.md) — scope per version from v0.1 through v1.0.
- [**CHANGELOG.md**](CHANGELOG.md) — per-version changes, breaking and otherwise.
- [**docs/benchmarks/METHODOLOGY.md**](docs/benchmarks/METHODOLOGY.md) — how benchmarks are measured and published.
- Per-version, per-platform benchmark results under [`docs/benchmarks/<version>/`](docs/benchmarks/).

## Development

```bash
git clone https://github.com/asamarts/alint
cd alint
cargo test --workspace        # 200+ tests; includes end-to-end scenarios
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
