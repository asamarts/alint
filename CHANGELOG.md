# Changelog

All notable changes to alint are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] — 2026-04-19

Second release. The theme is composition and remediation: cross-file rules,
conditional gating, auto-fix, and two new output formats. Every v0.1 config
continues to validate and run under v0.2 without changes.

### Added

#### Rule kinds

- **Cross-file primitives (7 kinds)** — `pair`, `for_each_dir`, `for_each_file`,
  `dir_contains`, `dir_only_contains`, `unique_by`, `every_matching_has`. These
  support path-template substitution (`{dir}`, `{stem}`, `{ext}`, `{basename}`,
  `{path}`, `{parent_name}`) and nested rule instantiation for per-iteration
  semantics.

#### Facts + conditional rules

- **Facts system** — repository properties evaluated once per run. Three kinds
  shipped: `any_file_exists`, `all_files_exist`, `count_files`. Referenced
  from `when:` clauses and rule messages.
- **`when:` expression language** — bounded recursive-descent parser with
  boolean logic (`and`, `or`, `not`), comparison operators (`==`, `!=`, `<`,
  `<=`, `>`, `>=`), `in` (list or substring), `matches` (regex), literal
  types (bool/int/string/list/null), and `facts.*` / `vars.*` identifiers.
  Parsed at rule-build time; gates both top-level rules and nested rules
  inside `for_each_*`.

#### `fix` subcommand

- **`alint fix [path]`** — applies mechanical corrections for violations whose
  rule declares a fix strategy. Five ops:
  - `file_create` (paired with `file_exists`) — writes declared content.
  - `file_remove` (paired with `file_absent`) — deletes the violating file.
  - `file_prepend` (paired with `file_header`) — injects content at the top,
    preserving UTF-8 BOM.
  - `file_append` (paired with `file_content_matches`) — appends content.
  - `file_rename` (paired with `filename_case`) — converts the stem to the
    rule's target case, preserving extension and parent directory.
- **`--dry-run`** previews the outcome without touching disk.
- **Safety** — `file_create` refuses to overwrite existing files;
  `file_rename` refuses to overwrite a collision; all ops skip cleanly with
  a diagnostic when preconditions aren't met.

#### Output formats

- **`sarif`** — SARIF 2.1.0 JSON, targeting GitHub Code Scanning's upload
  action. Each violation becomes a `result` with a `physicalLocation`
  anchored on the violating path.
- **`github`** — GitHub Actions workflow-command annotations
  (`::error title=...::`). Renders inline on PR file-changed view.

#### Distribution

- **Official GitHub Action** at `asamarts/alint@v0.2.0` — composite action
  wrapping `install.sh`. Inputs: `version`, `path`, `config` (multi-line),
  `format` (default `github`), `fail-on-warning`, `args`, `working-directory`.
  Output: `sarif-file` for feeding the Code Scanning uploader.

#### Testing infrastructure

- **`alint-testkit` + `alint-e2e`** (internal crates) — scenario-driven
  end-to-end tests. 51 YAML scenarios auto-generated into `#[test]`s via
  `dir-test`, 20 CLI snapshot tests via `trycmd`, and 4 property-based
  invariants via `proptest`. See the [ARCHITECTURE / testing
  sections](docs/design/ARCHITECTURE.md) for the rationale.

### Changed

- **Internal workspace crates published** — `alint-dsl`, `alint-rules`, and
  `alint-output` are now published on crates.io. They carry
  `description: "Internal: ... Not a stable public API."`; only `alint` and
  `alint-core` are semver-stable. This change is required so that
  `cargo install alint` resolves its transitive path-or-crates-io
  dependencies.
- **JSON Schema** (`schemas/v1/config.json`) gains the `fix:` block, every
  new rule kind, and the `facts:` + `when:` fields. Root + in-crate copies
  are kept byte-identical by a drift-guard test.

### Fixed

- **`install.sh`** no longer aborts on SIGPIPE when resolving the latest
  release tag. Previously, `curl | awk '{...; exit}'` under
  `set -o pipefail` caused curl to error out with exit 23 after awk's early
  exit; the fetch is now decoupled from the parse.

### Compatibility

- Config schema version remains `1`. Existing v0.1 configs run unchanged.
- The public API of `alint-core` has not been broken. The `Rule` trait
  gained a `fixer()` method with a `None` default, so out-of-tree rule
  implementations compile without modification.

## [0.1.0] — 2026-04-19

Initial release. MVP.

### Added

- 11 rule primitives — `file_exists`, `file_absent`, `dir_exists`,
  `dir_absent`, `file_content_matches`, `file_content_forbidden`,
  `file_header`, `filename_case`, `filename_regex`, `file_max_size`,
  `file_is_text`.
- Walker that honors `.gitignore`; globset-based scopes with Git-style
  `**` semantics (separator-aware).
- CLI subcommands: `check`, `list`, `explain`.
- Output formats: `human`, `json`.
- JSON Schema at `schemas/v1/config.json` for editor autocomplete.
- Benchmarks (criterion micros + hyperfine macros).
- Static binaries on GitHub Releases for Linux
  (x86_64/aarch64 musl), macOS (x86_64/aarch64), and Windows (x86_64).
- Install script (`install.sh`) with platform detection + SHA-256
  verification.
- Dogfood `.alint.yml` exercising the tool against its own repo.

[Unreleased]: https://github.com/asamarts/alint/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/asamarts/alint/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/asamarts/alint/releases/tag/v0.1.0
