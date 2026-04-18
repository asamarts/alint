# alint — Roadmap

> This roadmap is scope-based; dates are deliberately omitted. Each version is a
> closed cut — work that doesn't fit moves to a later version. See
> [ARCHITECTURE.md](./ARCHITECTURE.md) for the design these phases build out.

## Current: v0.1 (MVP)

The smallest scope that is usefully adoptable.

- ✅ Walker (honors `.gitignore`), config loader (YAML + JSON Schema validation), globset-based scopes.
- ✅ Rule primitives: `file_exists`, `file_absent`, `dir_exists`, `dir_absent`, `file_content_matches`, `file_content_forbidden`, `file_header`, `filename_case`, `filename_regex`, `file_max_size`, `file_is_text`.
- ✅ Output formats: `human`, `json`.
- ✅ CLI subcommands: `check`, `list`, `explain`.
- ✅ JSON Schema published for editor autocomplete (`schemas/v1/config.json`).
- ✅ Benchmarks published with the release — criterion micro-benches under `crates/alint-bench/` and hyperfine macro-benches via `xtask bench-release`. Methodology at [`docs/benchmarks/METHODOLOGY.md`](../benchmarks/METHODOLOGY.md); per-platform results under `docs/benchmarks/v0.1/`.
- ✅ Static binaries on GitHub Releases, install script, `cargo install alint` — release workflow at `.github/workflows/release.yml`, installer at [`install.sh`](../../install.sh).
- ✅ Pre-publish hygiene: binary package renamed `alint-cli` → `alint`; internal crates flagged `publish = false` (only `alint` + `alint-core` publish); crates.io metadata populated on the public crates; `LICENSE-APACHE` + `LICENSE-MIT` + root `README.md` added.
- ✅ Dogfood `.alint.yml` exercising the tool against its own repo.

## v0.2 — Cross-file and composition

- Cross-file primitives: `pair`, `for_each_dir`, `for_each_file`, `every_matching_has`, `dir_contains`, `dir_only_contains`, `unique_by`.
- Facts system: `any_file_exists`, `all_files_exist`, `detect: linguist`, `detect: askalono`, `count_files`, `custom`.
- `when` expression language.
- `extends` with URL resolution, SHA-256 SRI, caching under `~/.cache/alint/rulesets/`.
- `fix` subcommand with `file_create`, `file_prepend`, `file_append`, `file_remove`, `file_rename`.
- Output formats: `sarif`, `github`.
- Official GitHub Action.

## v0.3 — Structured content

- Structured-query primitives: `json_path_equals`, `json_path_matches`, `yaml_path_*`, `toml_path_*`, `json_schema_passes`.
- Additional content primitives: `file_hash`, `file_shebang`, `file_max_lines`, `file_footer`.
- Opt-in nested `.alint.yml` discovery for monorepos.
- Output formats: `markdown`, `junit`, `gitlab`.
- `alint facts` subcommand (for debugging `when` clauses).

## v0.4 — Plugins v1 and distribution breadth

- `command` plugin kind.
- pre-commit hook (`.pre-commit-hooks.yaml`).
- npm shim (`@alint/alint`), Homebrew formula, Docker image (distroless).
- Git-aware primitives: `git_tracked_only`, `git_no_denied_paths`, `git_commit_message`.

## v0.5 — LSP and bundled rulesets

- LSP server (`alint lsp`): inline diagnostics, hover with rule documentation, code actions for "add to ignore" and "apply fix."
- VS Code extension (bundles the LSP).
- Bundled rulesets: `oss-baseline`, `rust`, `node`, `python`, `java`, `go`, `monorepo`, `compliance/reuse`, `compliance/apache-2`.

## v0.6 — WASM plugins

- `wasm` plugin kind with a `wasmtime` host, stable WIT interface.
- Plugin registry scaffolding with signature verification.

## v1.0 — Stability

- DSL schema committed; semver on `version: 1`.
- Plugin ABI committed.
- `alint-core` public API frozen; breaking changes follow semver-major.
- Documentation site.
