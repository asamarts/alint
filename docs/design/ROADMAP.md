# alint — Roadmap

> This roadmap is scope-based; dates are deliberately omitted. Each version is a
> closed cut — work that doesn't fit moves to a later version. See
> [ARCHITECTURE.md](./ARCHITECTURE.md) for the design these phases build out.

**Latest release: v0.4.0** (crates.io + GitHub Releases, 2026-04-21).
Headline: bundled rulesets (`alint://bundled/<name>@<rev>`) plus
pre-commit integration. Next planned: v0.5 — structured-query
primitives + plugins v1 + distribution breadth.

## v0.1 — MVP (shipped)

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

## v0.2 — Cross-file and composition (shipped)

- Cross-file primitives: ✅ `pair`, ✅ `for_each_dir`, ✅ `for_each_file`, ✅ `every_matching_has`, ✅ `dir_contains`, ✅ `dir_only_contains`, ✅ `unique_by`. **(complete)**
- Facts system: ✅ `any_file_exists`, ✅ `all_files_exist`, ✅ `count_files`, ✅ `file_content_matches`, ✅ `git_branch`, ✅ `custom` (security-gated; only allowed in the top-level config, never in `extends:`); ⏳ `detect: linguist`, ⏳ `detect: askalono` — both likely v0.5 alongside bundled rulesets.
- ✅ `when` expression language — bounded grammar with `and`/`or`/`not`, comparison ops (`==` `!=` `<` `<=` `>` `>=`), `in` (list/substring), `matches` (regex), literal types (bool/int/string/list/null), and `facts.X` / `vars.X` identifiers. Parsed at rule-build time; gates rules in Engine + nested rules in `for_each_*`.
- ✅ `extends`: local files (recursive resolution, cycle detection, child-overrides-parent merge) + HTTPS URLs with SHA-256 SRI and caching under the platform user cache dir (`~/.cache/alint/rulesets/` on Linux). Nested remote extends deferred to v0.3 — a relative path inside a fetched config has no principled base.
- ✅ `fix` subcommand with `file_create`, `file_remove`, `file_prepend`, `file_append`, `file_rename` (the latter wired to `filename_case` — target name derived from the rule's `case:` setting; extension preserved).
  - Deferred for later (likely v0.5 when bundled rulesets land): `content_from: <path>` for `file_create` / `file_prepend` / `file_append`, so long bodies (LICENSE texts, standard boilerplate) can live alongside the rule rather than inline in YAML.
  - Deferred (likely v0.3): a `rename_to:` template for `filename_regex`, so the pattern's capture groups can drive a substitution target. Not yet designed.
- ✅ Output formats: `sarif`, `github`.
- ✅ Official GitHub Action (`action.yml` at repo root; composite action wrapping `install.sh`).

## v0.3 — Hygiene, portable metadata, byte fingerprints (shipped)

The v0.3 cut shifted scope mid-cycle. The originally-planned
"structured content" family (JSON/YAML/TOML path queries) was
rolled over to v0.4; the freed capacity was spent on content and
metadata rules that surfaced during dogfooding as common pain
points in real repos.

- ✅ Text hygiene: `no_trailing_whitespace`, `final_newline`, `line_endings`, `line_max_width`, `indent_style`, `max_consecutive_blank_lines` (+ `file_collapse_blank_lines` fix op).
- ✅ Security / Unicode sanity: `no_merge_conflict_markers`, `no_bidi_controls`, `no_zero_width_chars` (+ `file_strip_bidi` / `file_strip_zero_width` fix ops).
- ✅ Encoding + content fingerprint: `no_bom` (+ `file_strip_bom`), `file_is_ascii`, `file_hash`.
- ✅ Structure: `max_directory_depth`, `max_files_per_directory`, `no_empty_files`.
- ✅ Portable metadata: `no_case_conflicts`, `no_illegal_windows_names`.
- ✅ Unix metadata: `no_symlinks`, `executable_bit`, `executable_has_shebang`, `shebang_has_executable`.
- ✅ Git hygiene: `no_submodules`.
- ✅ Byte-level fingerprint: `file_starts_with`, `file_ends_with`.
- ✅ Auto-fix ops added: `file_trim_trailing_whitespace`, `file_append_final_newline`, `file_normalize_line_endings`, `file_strip_bidi`, `file_strip_zero_width`, `file_strip_bom`, `file_collapse_blank_lines`.
- ✅ `fix_size_limit` top-level config knob (default 1 MiB; `null` disables) — content-editing fixers skip oversize files with a stderr warning rather than rewrite them.
- ✅ Short-name rule aliases (`content_matches`, `content_forbidden`, `header`, `max_size`, `is_text`) for rules without a `dir_*` sibling.

**Deferred to v0.4**: structured-query primitives (`json_path_*`, `yaml_path_*`, `toml_path_*`, `json_schema_passes`), `file_footer`, `file_max_lines`, `file_shebang`, opt-in nested `.alint.yml` discovery for monorepos, `markdown` / `junit` / `gitlab` output formats, `alint facts` subcommand for debugging `when` clauses.

## v0.4 — Bundled rulesets + pre-commit (shipped)

Pulled forward from what was v0.5: **bundled rulesets** are the
single biggest adoption lever, turning "write 20 rules" into
"add one `extends:` line." Also lands pre-commit framework
integration so any pre-commit user adopts alint with 4 lines of
YAML.

- ✅ `.pre-commit-hooks.yaml` — exposes `alint` (check) and `alint-fix` (manual-stage) hooks. `language: rust` means zero setup for pre-commit users.
- ✅ Bundled rulesets infra: `alint://bundled/<name>@<rev>` URI scheme resolved offline via `include_str!`. Cycle-safe, leaf-only (bundled rulesets cannot themselves `extends:`). Inherits the same `custom:`-fact guard as HTTPS extends.
- ✅ `alint://bundled/oss-baseline@v1` — 9 rules. Community docs + content hygiene most OSS repos want.
- ✅ `alint://bundled/rust@v1` — 10 rules. Gated `when: facts.is_rust` so it's a safe no-op in polyglot trees.
- ✅ `alint://bundled/node@v1` — 8 rules. Gated `when: facts.is_node`.
- ✅ `alint://bundled/monorepo@v1` — 4 rules. Language-agnostic `for_each_dir` over `{packages,crates,apps,services}/*`.

## v0.5 — Structured content + plugins v1 + distribution breadth

Rolled forward from the original v0.4 scope, plus a Composition
& reuse subsection for the gaps surfaced in the v0.4.2 audit.

### Composition & reuse

A coherent sub-theme on making `.alint.yml` shareable,
overridable, and monorepo-friendly. Ranked by leverage ÷ effort.

- 🏗 **Field-level rule override.** Let children in the
  `extends:` chain specify only the fields they change
  (`rules: - {id: X, level: off}`); kind/paths/etc inherit
  from the earliest ancestor that declares them. Eliminates
  the current requirement to re-declare the full rule just to
  tweak `level`. *(in progress)*
- 🏗 **Refresh `extends:` schema docs.** Mention SRI syntax,
  `alint://bundled/` URLs, merge semantics, and the `level:
  off` disable idiom. The current schema description is stale
  (says HTTPS is "reserved for a future version"). *(in
  progress)*
- ⏳ **Nested `.alint.yml` discovery for monorepos.** Walk from
  repo root down to each linted file; stack configs per
  directory so `packages/frontend/.alint.yml` can layer on top
  of the root config. Rules scope to the subtree where they're
  declared.
- ⏳ **Rule templates / parameterized rules.** Define a rule
  shape once, instantiate N times with different arguments.
  Example: "every `{{dir}}` has `{{file}}`" instantiated for
  README, LICENSE, package.json. Reuses existing `{{vars.*}}`
  machinery, extended to rule option fields.
- ⏳ **Selective bundled adoption.** Syntax for "extend this
  ruleset but only these rules" (`only: [...]`) or "extend but
  drop these" (`except: [...]`). Fixes the current
  all-or-nothing limitation on bundled rulesets.
- ⏳ **`.alint.d/*.yml` drop-ins.** Auto-discover and merge YAML
  files in a `.alint.d/` directory alphabetically, same merge
  semantics as `extends`. Ops convention for layered team
  configs.

### Other scope

- ⏳ Structured-query primitives: `json_path_equals`, `json_path_matches`, `yaml_path_*`, `toml_path_*`, `json_schema_passes`.
- ⏳ Additional content primitives: `file_footer`, `file_max_lines`, `file_shebang`.
- ⏳ Output formats: `markdown`, `junit`, `gitlab`.
- ⏳ `alint facts` subcommand (for debugging `when` clauses).
- ⏳ `command` plugin kind.
- ⏳ npm shim (`@alint/alint`), Homebrew formula, Docker image (distroless).
- ⏳ Git-aware primitives: `git_tracked_only`, `git_no_denied_paths`, `git_commit_message`.
- ⏳ Additional bundled rulesets: `python`, `java`, `go`, `compliance/reuse`, `compliance/apache-2`.

## v0.6 — LSP

- LSP server (`alint lsp`): inline diagnostics, hover with rule documentation, code actions for "add to ignore" and "apply fix."
- VS Code extension (bundles the LSP).

## v0.7 — WASM plugins

- `wasm` plugin kind with a `wasmtime` host, stable WIT interface.
- Plugin registry scaffolding with signature verification.

## v1.0 — Stability

- DSL schema committed; semver on `version: 1`.
- Plugin ABI committed.
- `alint-core` public API frozen; breaking changes follow semver-major.
- Documentation site.
