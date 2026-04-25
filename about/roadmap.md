---
title: Roadmap
---

> This roadmap is scope-based; dates are deliberately omitted. Each version is a
> closed cut — work that doesn't fit moves to a later version. See
> [ARCHITECTURE.md](./ARCHITECTURE.md) for the design these phases build out.

**Latest release: v0.4.10** (2026-04-25). Headline: `file_max_lines`,
`file_footer`, `file_shebang` round out the content family
(catalogue at ~55 rule kinds). Next planned: v0.5 — monorepo
scale (`--changed` mode, `--monorepo` preset, per-iteration
`when:` on `for_each_dir`), `command` plugin kind, npm shim,
remaining git-aware primitives, compliance rulesets.

## Positioning

alint's scope is **the filesystem shape and contents of a
repository**, not the semantics of the code inside it. Sweet
spot: workspace-tier monorepos (Cargo, pnpm, yarn, Lerna) and
OSS-style polyglot monorepos. Honest limits: dependency-graph
problems (`cargo deny`, `bazel mod`, `buildifier`) and
code-content problems (linters, SAST) are explicit non-goals;
hyperscale Bazel monorepos are not the design center —
some primitives (notably `for_each_dir`) need a per-iteration
`when:` filter to apply there cleanly, addressed in the v0.5
Monorepo & scale subsection below.

The adoption ladder this design points toward:
one-line bundled start → ecosystem overlay (`rust@v1` /
`node@v1` / `python@v1` / `go@v1` / `java@v1`) → CI hardening
(`ci/github-actions@v1`) → field-level overrides → custom
structured-query rules → pre-commit + GHA wiring →
`git_tracked_only` for absence rules → `nested_configs: true`.
v0.5 prioritizes the next rung: tighter monorepo ergonomics
for workspace-tier and OSS-polyglot adopters.

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

### v0.4.x point releases (shipped)

Ten point releases shipped after v0.4.0, expanding scope well
past the original cut. Most of what was originally planned
for v0.5 landed here.

- **v0.4.1** — packaging fix.
- **v0.4.2** — pretty `human` formatter overhaul.
- **v0.4.3** — composition: field-level rule override; nested
  `.alint.yml` discovery for monorepos (`nested_configs: true`).
  Four bundled rulesets: `hygiene/no-tracked-artifacts@v1`,
  `hygiene/lockfiles@v1`, `tooling/editorconfig@v1`,
  `docs/adr@v1`.
- **v0.4.4** — `file_min_size` + `file_min_lines` content
  rules; six structured-query rule kinds
  (`{json,yaml,toml}_path_{equals,matches}`). README rewritten
  as a 12-pattern cookbook.
- **v0.4.5** — `alint://bundled/ci/github-actions@v1`;
  `if_present: true` on structured-query rules; selective
  bundled adoption (`only:` / `except:` on `extends:` entries).
- **v0.4.6** — `alint://bundled/python@v1` + `alint://bundled/go@v1`;
  `alint facts` subcommand for debugging `when:` clauses.
- **v0.4.7** — distroless Docker image (`ghcr.io/asamarts/alint`)
  + Homebrew tap (`asamarts/alint`).
- **v0.4.8** — `git_tracked_only: bool` — first git-aware
  rule primitive. Closes the absence-rule false-positive on
  locally built artifacts.
- **v0.4.9** — `alint://bundled/java@v1`. First bundled use of
  `git_tracked_only`.
- **v0.4.10** — `file_max_lines` + `file_footer` + `file_shebang`
  round out the content family. Catalogue at ~55 rule kinds.

## v0.5 — Monorepo scale + plugins v1 + remaining distribution

The v0.4.x cuts cleared most of the original v0.5 scope (structured-query, ecosystem rulesets, `alint facts`, Docker, Homebrew, first git-aware primitive). What remains, plus new monorepo-scale work surfaced by the 2026-04 monorepo positioning analysis.

### Composition & reuse

A coherent sub-theme on making `.alint.yml` shareable,
overridable, and monorepo-friendly. Ranked by leverage ÷ effort.

- ✅ **Field-level rule override.** Children in the `extends:`
  chain can specify only the fields they change
  (`rules: - {id: X, level: off}`); kind/paths/etc inherit
  from the earliest ancestor that declares them. Shipped
  2026-04-22 (commit `261dda5`).
- ✅ **Refreshed `extends:` schema docs.** Mention SRI syntax,
  `alint://bundled/` URLs, merge semantics, and the `level:
  off` disable idiom. Shipped 2026-04-22 (commit `261dda5`).
- ✅ **Nested `.alint.yml` discovery for monorepos.** Opt-in
  via `nested_configs: true` on the root config. Each nested
  rule's path-like scope fields (`paths`, `select`, `primary`)
  auto-prefix with the config's relative directory. Cross-
  subtree id collisions are rejected for MVP. Shipped
  2026-04-22.
- ⏳ **Rule templates / parameterized rules.** Define a rule
  shape once, instantiate N times with different arguments.
  Example: "every `{{dir}}` has `{{file}}`" instantiated for
  README, LICENSE, package.json. Reuses existing `{{vars.*}}`
  machinery, extended to rule option fields.
- ✅ **Selective bundled adoption.** Mapping form on `extends:`
  entries with `only: [...]` (keep listed rules) or
  `except: [...]` (drop listed rules); mutually exclusive;
  unknown ids error at load. Closes the all-or-nothing
  limitation. Shipped 2026-04-23 in v0.4.5.
- ⏳ **`.alint.d/*.yml` drop-ins.** Auto-discover and merge YAML
  files in a `.alint.d/` directory alphabetically, same merge
  semantics as `extends`. Ops convention for layered team
  configs.

### Monorepo & scale

Identified by the 2026-04 monorepo positioning analysis as
the largest delta between alint's current shape and what
workspace-tier + OSS-polyglot monorepos typically reach for.
Ranked by leverage.

- ⏳ **`alint check --changed [--base=<ref>]`.** Incremental
  mode: diff `git diff --name-only <base>...HEAD` (or
  `git ls-files --modified` when no base) and only evaluate
  rules whose path scopes intersect the changed-file set.
  Cross-file rules (`pair`, `for_each_dir`,
  `every_matching_has`, `unique_by`, `dir_contains`,
  `dir_only_contains`) opt out of the filter — their inputs
  span the whole tree by definition. The leverage move for
  large repos: today, every check runs the full rule set
  over the full tree; this lets pre-commit and PR-check
  paths run in milliseconds on most diffs without changing
  the rule shape. Pairs naturally with `git_tracked_only`.
- ⏳ **Per-iteration `when:` filter on `for_each_dir`.** Today
  `for_each_dir` iterates every directory matching `paths:`;
  the inner rules then short-circuit if a marker file is
  missing. The Bazel-shaped pattern wants the iteration
  itself gated: "iterate only directories whose contents
  satisfy *this fact-style predicate* (e.g., contains a
  `BUILD`, `Cargo.toml`, `package.json`, `go.mod`)." Reuses
  the existing `when:` grammar with per-iteration facts
  (`facts.dir.has_file`, `facts.dir.contains`, etc.).
  Closes the most common gap when applying alint to Bazel-style
  monorepos without adding a Starlark parser.
- ⏳ **`--monorepo` discovery preset.** A new flag on `alint
  init` and `alint check` that auto-detects workspace
  layout — Cargo workspace (`[workspace]` in root
  `Cargo.toml`), pnpm workspace (root `pnpm-workspace.yaml`),
  yarn / npm workspaces (root `package.json` `workspaces`
  field), Lerna (`lerna.json`), Bazel (root `WORKSPACE` /
  `MODULE.bazel`), Nx (`nx.json`), Turborepo (`turbo.json`)
  — and emits / applies a sensible scaffold:
  `nested_configs: true`, the right `for_each_dir` paths
  pulled from the workspace globs, and the relevant
  ecosystem ruleset extends. Removes the boilerplate that
  workspace-tier adopters currently re-derive each time.
- ⏳ **Workspace-aware bundled rulesets.** Three thin overlays
  on top of `monorepo@v1`: `monorepo/cargo-workspace@v1`,
  `monorepo/pnpm-workspace@v1`, `monorepo/yarn-workspace@v1`.
  Each adds one rule: every workspace-member directory has
  the manifest the workspace declared (`Cargo.toml` /
  `package.json`). Gated by the `is_cargo_workspace` /
  `is_pnpm_workspace` / `is_yarn_workspace` facts (new —
  thin wrappers over existing `any_file_exists` +
  `file_content_matches`).
- ⏳ **Documented scale ceiling.** Bench `alint check` on a
  synthetic 100k-file tree (representative of the
  workspace-tier upper bound) and a 1M-file tree
  (Bazel-territory). Publish the numbers under
  `docs/benchmarks/scale/`. Honest baseline — keeps the
  positioning claims falsifiable and tells adopters where
  to stop.

### Other scope

- ✅ Structured-query primitives (v0.4.4, 2026-04-23): `json_path_equals`, `json_path_matches`, `yaml_path_equals`, `yaml_path_matches`, `toml_path_equals`, `toml_path_matches`. JSONPath per RFC 9535; YAML and TOML coerce through serde into the same tree shape. `json_schema_passes` still ⏳.
- ✅ `if_present: true` on structured-query rules (v0.4.5).
- ✅ Additional content primitives (v0.4.10): `file_footer`, `file_max_lines`, `file_shebang`.
- ✅ `alint facts` subcommand (v0.4.6).
- ✅ Homebrew formula via `asamarts/alint` tap (v0.4.7).
- ✅ Distroless Docker image at `ghcr.io/asamarts/alint` (v0.4.7).
- ✅ Git-aware primitive: `git_tracked_only` (v0.4.8).
- ✅ Additional bundled rulesets: `python` (v0.4.6), `go` (v0.4.6), `ci/github-actions` (v0.4.5), `java` (v0.4.9).
- ⏳ Output formats: `markdown`, `junit`, `gitlab`.
- ⏳ `command` plugin kind. (Plugin v1 lever — lets a rule shell out to a checker like `actionlint` / `shellcheck` / `taplo` / `markdownlint` and bridge their findings into alint's report. Path to ecosystem reach without growing the core rule set.)
- ⏳ npm shim (`@alint/alint`). Closes the install-path gap for JS adopters who don't already have Cargo, Homebrew, or Docker. Wraps a download of the matching pre-built binary; package never ships JS.
- ⏳ Git-aware primitives: `git_no_denied_paths`, `git_commit_message`.
- ⏳ `json_schema_passes` primitive.
- ⏳ Remaining bundled rulesets: `compliance/reuse`, `compliance/apache-2`. (Compliance rulesets are higher-leverage now that v0.4 has the structured-query + content primitives needed to express SPDX-Identifier headers, REUSE conformance, and Apache-2 NOTICE/headers.)
- ⏳ Additional Scorecard-overlap rules in `ci/github-actions@v1` and `oss-baseline@v1`. Specifically: SECURITY.md presence + non-empty (already partial), `dependabot.yml` / `renovate.json` presence (Dependency-Update-Tool check), branch protection hints via `.github/CODEOWNERS` shape (Code-Review check).

### Generic hygiene rulesets (shipped in v0.4.3)

Identified in a research pass across Turborepo/Nx/Bazel/Cargo/
pnpm docs, OpenSSF Scorecard, Repolinter's archived corpus, and
large orgs' community-health-file conventions. Four rulesets
built on the existing primitive set — no new rule kinds needed.

- ✅ `hygiene/no-tracked-artifacts@v1` — node_modules, target,
  dist, .next, .DS_Store, editor backups, .env variants, 10 MiB
  size gate. Several auto-fixable.
- ✅ `hygiene/lockfiles@v1` — one rule per package manager
  (npm/pnpm/yarn/bun/Cargo/Poetry/uv) forbidding nested lockfiles.
- ✅ `tooling/editorconfig@v1` — `.editorconfig` + `.gitattributes`
  existence with a `text=` normalization directive.
- ✅ `docs/adr@v1` — MADR naming pattern + required `## Status`,
  `## Context`, `## Decision` sections. Gap-free numbering
  deferred (needs `numeric_sequence` primitive).

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
