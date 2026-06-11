# alint: Roadmap

> This roadmap is scope-based; dates are deliberately omitted. Each version is a
> closed cut, work that doesn't fit moves to a later version. See
> [ARCHITECTURE.md](./ARCHITECTURE.md) for the design these phases build out.

> **Source convention**: this file is the canonical roadmap. The generated
> public version at alint.org is produced by `xtask gen-public-roadmap`,
> which elides sections wrapped in paired `alint:internal-*` HTML-comment
> markers. See [`v0.11/roadmap_generator.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.11/roadmap_generator.md)
> for the marker syntax and the v0.9.22 migration plan.

**Latest release: v0.12.0** (2026-06-07). The case-study-driven rule-kind
expansion paired with a security cycle. New kinds: `file_graph`
(file-dependency-graph firewalls + cycle / orphan / dangling checks),
`for_each_match` (a per-line predicate quantifier), a unified `cross_file`
value-relation kind, `pair_changed_together` (a co-change gate), and
`generated_file_fresh` mutating / in-place mode, plus markerless
`ordered_block`, a `php@v1` bundled ruleset, JSONC-tolerant structured parsing,
and more `import_gate` presets. Security: every config-declared path a rule
reads or resolves is now confined to the repo root (the untrusted-`extends:`
threat model), with `allow_out_of_root:` as the explicit top-level opt-in, the
walker pruning symlinks that escape the tree, and a fixed git
argument-injection in the `since:` range mode (write/truncate of an arbitrary
out-of-tree file; affected releases back to v0.9.21).

**Since v0.12, a spec-driven-development program shipped to `main`** (engineering
foundations, not a version cut): the config schema, the per-rule options docs, a
`facts.json` surface-area contract, the crate-dependency graph + C4 model, and a
Kani-verified path-confinement proof are all now generated and regenerate-and-diff
gated, and alint.org renders its headline counts from `facts.json`. See the
[Engineering foundations](#engineering-foundations-spec-driven-development) section
below.

**v0.11.1** (2026-05-31)
was a JetBrains-plugin patch that
clears JetBrains Marketplace moderation: the v0.11.0 plugin read its own
version via platform plugin-lookup APIs (`PluginManagerCore` /
`PluginManager`) the Marketplace rejects as internal even though
`intellij-plugin-verifier` passes them; the version is now stamped into
a build-time classpath resource with zero platform-API surface, gated by
a new `verifyNoMarketplaceDeniedApis` bytecode check. Linter behavior
unchanged. **v0.11.0** (2026-05-28) was the LSP + editor-integration
release: the `alint lsp` language server (the new `alint-lsp` crate)
with in-editor diagnostics, hover-to-explain, and apply-fix quick
actions, wired across the VS Code, JetBrains, and Zed extensions plus
the Neovim / Sublime / Emacs / Helix configs; the v0.9.21-derived DSL
polish (`scope_filter.changed_since:`, the `git_commit_*`
commit-validation family, `{{env.X}}` interpolation); and an
informational-notes channel. v0.10.2 (2026-05-21) was the asciinema-demo
OSC 8 hyperlink fix. v0.10.1 (2026-05-20) extended
`crate::io::read_capped` to `json_schema_passes` +
`for_each_dir`'s literal-path bypass (two pre-existing read
sites missed by the original Phase 3 sweep), plus CI hygiene
(bench-record PR-body template refresh, runner agent bump
2.332.0 → 2.334.0, Docker channel example `:0.9` → `:0.10`).
v0.10.0 (2026-05-20) was the case-study
coverage push (eight new rule kinds, `registry_paths_resolve`,
`cross_file_value_equals`, `ordered_block`, `generated_file_fresh`,
`import_gate`, `command_idempotent`, `xml_path_equals` /
`xml_path_matches`, `pair_hash`, and two new bundled rulesets,
`apache/governance@v1` + `dotnet@v1`; three pre-release
security retirements: spawn-trust-gate generalised, XML
recursion bound, whole-file reads capped at 256 MiB; rule-kind
count 71 → 79, bundled-ruleset count 19 → 21).
v0.9.23 (2026-05-17) shipped GitHub Action pinning plus release-
pipeline hardening (pinned-ref binary, cargo-deny, the bench-gate
re-engineering); v0.9.22 (2026-05-15) was doc-drift cleanup plus
prevention automation; v0.9.21 shipped commit-range mode for
`git_commit_message` (closes #26);
v0.9.12 - v0.9.20 brought the launch-readiness capstone (width-
aware output, bundled-rule message audit, em-dash scrub, install-
snippet pin sweep, polyglot demo). See
[CHANGELOG.md](https://github.com/asamarts/alint/blob/main/CHANGELOG.md) for per-version detail and
[`docs/development/launch-evidence.md`](https://github.com/asamarts/alint/blob/main/docs/development/launch-evidence.md)
for the case-study corpus + rule-kind demand aggregation.

**Launch readiness: clean.** Pre-flight gates wired:
`cargo fmt --check` + clippy + test + docs + version-pin
consistency + dogfood, all bundled into
`ci/scripts/preflight.sh` and gated by an opt-in pre-push hook
(`ci/githooks/pre-push`). The `install-snippets-match-workspace
-version` rule in the dogfood `.alint.yml` flags drift between
Cargo.toml's workspace version and any user-facing install
snippet, so a release with stale `v0.9.X` README/Docker/GHA pins
fails CI before it ships. The project has been launch-ready
since the v0.9.x preflight-gate capstone; public-launch timing
is tracked in the alint.org marketing repo (`marketing/STATE.md`
and `marketing/launch-strategy.md`), deliberately not pinned to
a version here, since a scope-based, dateless roadmap shouldn't
carry a launch-version that keeps going stale.

**v0.9 cut closed (2026-05-02).** A scaling-profile
investigation surfaced a +28-37% 1M S3 regression vs
v0.5.6, `for_each_dir` cross-file dispatch was running
nested rules in O(D × N) shape (5 B glob-match operations
at 1M with 5,000 packages). The fix landed on `main` as
v0.9.5 (lazy path-index on `FileIndex` + literal-path fast
paths in `file_exists`, `structured_path`,
`iter.has_file`); 1M S3 wall: 731.9 s → 11.19 s (65×) full,
6.73 s (108×) changed. v0.9.5 also folded in the
test/coverage/dogfood floor that prevents the same class of
regression from slipping by again:

- **v0.9.5.5**, cross-file dispatch fast paths (the headline)
- **v0.9.5.6**, coverage audits (pass/fail symmetry, bundled-
  ruleset coverage, git-mode symmetry)
- **v0.9.5.7**, 16 coverage scenarios filling the audit punch list
- **v0.9.5.8**, bench-scale S6 (per-file content fan-out) /
  S7 (cross-file relational) / S8 (git-tracked overlay)
- **v0.9.5.9**, `docs/development/rule-authoring.md` workflow doc
- **v0.9.5.reorg**, `docs/benchmarks/{micro,macro,investigations,archive}/`
  layout + per-version results + `xtask publish-benches`

Full v0.9.5 design: [`docs/design/v0.9/coverage-and-dogfood.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/coverage-and-dogfood.md).

**v0.9.6 (2026-05-02), `scope_filter:` primitive.** Closest-
ancestor manifest scoping for per-file rules, plus the
bundled-ecosystem-ruleset migration that motivated it. The
`is_*` ecosystem facts (`is_rust`, `is_node`, …) renamed to
`has_*` and broadened from root-only `[Cargo.toml]` to
`[Cargo.toml, "**/Cargo.toml"]` so polyglot monorepos with
no root manifest correctly fire the ruleset; per-file content
rules add `scope_filter: { has_ancestor: <manifest> }` so
they only fire on files inside their ecosystem's package
subtree. New bench-scale S9 scenario (nested polyglot:
rust + node + python over `crates/` + `packages/` + `apps/`)
captures the dispatch shape this primitive was designed for.
Full design: [`docs/design/v0.9/scope-filter.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/scope-filter.md).

**Forward plan, ordered by user-facing payoff.** The next two
versions split what was originally one v0.10 release into two
focused cuts. Rule-kind coverage ships first because the
case-study aggregation (30 repos under
[`docs/development/launch-evidence.md`](https://github.com/asamarts/alint/blob/main/docs/development/launch-evidence.md))
identified 8 primitives with 5-13 demand sources each. LSP +
editor integration is the second cut because no user is blocked
on it for adoption, they can lint without it.

- **v0.10, Case-study coverage push.** 8 case-study-validated
  rule kinds + 2 bundled rulesets. Closes the gap that today
  requires `command:` shell-out for what should be declarative
  rules. Sized 2-4 weeks. Headline primitives:
  `cross_file_value_equals` (12 sources), `registry_paths_resolve`
  (13 sources), `ordered_block` (8), `generated_file_fresh` (8),
  `import_gate` (5), `command_idempotent` (5), `pair_hash` (3),
  `xml_path_matches` + `xml_path_equals` (2, completes the
  structured-query family). Bundled rulesets:
  `apache/governance@v1` (3 Apache TLPs converge),
  `dotnet@v1` (Microsoft / Azure SDK adopter surface).
  Design candidates land opportunistically as a second demand
  source materialises.

- **v0.11, LSP + developer experience.** LSP server, VS Code
  extension, ScopeFilter generalisation (`has_sibling`,
  `has_descendant`), single-file re-eval hot path, and the
  v0.11+ ship-targets that didn't make v0.10:
  `cross_language_implementation_complete` (5 sources: arrow,
  tensorflow, protobuf, angular, flutter),
  Bazel-licensing-declaration-aware rule kind (tensorflow's
  `licenses(["notice"])` discipline), `walk_error_policy:`
  engine knob (pnpm broken-symlink fixtures). LSP design +
  `tower-lsp` workspace dep already landed in v0.9.7 so the
  crate scaffold is the only structural lift.

- **v0.12, Real-world coverage expansion + gap rule kinds.** A
  100+ repo case study (3-4× the 30-repo re-analysis) plus the
  demand-ranked rule-kind backlog it produced:
  `git_commit_subject_matches`, the diff "must-add" family
  (`changeset_requires_path` / `pair_changed_together`), the
  value-set membership family, a `normalize:` transform on
  `cross_file_value_equals`, richer `import_gate`
  (default-deny + glob-discovered rule files), a resolved-graph
  dependency allowlist, and the ASF compliance-bundle over-fire
  fix.

- **v0.13, WASM plugins.** Bumped one slot to make room for
  v0.12. `wasmtime` host, signed plugin registry, blessed
  examples (mock-ratio checker, near-dup detector,
  debug-statement stripper).

- **v1.0, Stability.** DSL committed, plugin ABI committed,
  `alint-core` public API frozen, docs site committed.

**Single-source design candidates** that surfaced in the
case-study aggregation but haven't hit a second demand source:
`*_path_contains`, `pair_inverse`, `command_per_repo`,
`json_schema_passes` config-shape mode, `*_path_array_iter`,
`json_key_sort_order`, `column_alignment`, `line_spacing`,
`not_executable`, `directory_hash`. Most won't survive
demand-validation; they're tracked under "Emerging gaps" in
[`examples/README.md`](https://github.com/asamarts/alint/blob/main/examples/README.md#emerging-gaps-surfaced-this-audit-not-yet-on-roadmap)
so post-launch users can vote with their adoption.

## Positioning

alint's scope is **the filesystem shape and contents of a
repository**, not the semantics of the code inside it. Sweet
spot: workspace-tier monorepos (Cargo, pnpm, yarn, Lerna) and
OSS-style polyglot monorepos. Honest limits: dependency-graph
problems (`cargo deny`, `bazel mod`, `buildifier`) and
code-content problems (linters, SAST) are explicit non-goals;
hyperscale Bazel monorepos are not the design center:
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

## v0.1: MVP (shipped)

The smallest scope that is usefully adoptable.

- ✅ Walker (honors `.gitignore`), config loader (YAML + JSON Schema validation), globset-based scopes.
- ✅ Rule primitives: `file_exists`, `file_absent`, `dir_exists`, `dir_absent`, `file_content_matches`, `file_content_forbidden`, `file_header`, `filename_case`, `filename_regex`, `file_max_size`, `file_is_text`.
- ✅ Output formats: `human`, `json`.
- ✅ CLI subcommands: `check`, `list`, `explain`.
- ✅ JSON Schema published for editor autocomplete (`schemas/v1/config.json`).
- ✅ Benchmarks published with the release, criterion micro-benches under `crates/alint-bench/` and hyperfine macro-benches via `xtask bench-release`. Methodology at [`docs/benchmarks/METHODOLOGY.md`](https://github.com/asamarts/alint/blob/main/docs/benchmarks/METHODOLOGY.md); per-platform results under `docs/benchmarks/archive/v0.1/`.
- ✅ Static binaries on GitHub Releases, install script, `cargo install alint`, release workflow at `.github/workflows/release.yml`, installer at [`install.sh`](https://github.com/asamarts/alint/blob/main/install.sh).
- ✅ Pre-publish hygiene: binary package renamed `alint-cli` → `alint`; internal crates flagged `publish = false` (only `alint` + `alint-core` publish); crates.io metadata populated on the public crates; `LICENSE-APACHE` + `LICENSE-MIT` + root `README.md` added.
- ✅ Dogfood `.alint.yml` exercising the tool against its own repo.

## v0.2: Cross-file and composition (shipped)

- Cross-file primitives: ✅ `pair`, ✅ `for_each_dir`, ✅ `for_each_file`, ✅ `every_matching_has`, ✅ `dir_contains`, ✅ `dir_only_contains`, ✅ `unique_by`. **(complete)**
- Facts system: ✅ `any_file_exists`, ✅ `all_files_exist`, ✅ `count_files`, ✅ `file_content_matches`, ✅ `git_branch`, ✅ `custom` (security-gated; only allowed in the top-level config, never in `extends:`); ⏳ `detect: linguist`, ⏳ `detect: askalono`, both likely v0.5 alongside bundled rulesets.
- ✅ `when` expression language, bounded grammar with `and`/`or`/`not`, comparison ops (`==` `!=` `<` `<=` `>` `>=`), `in` (list/substring), `matches` (regex), literal types (bool/int/string/list/null), and `facts.X` / `vars.X` identifiers. Parsed at rule-build time; gates rules in Engine + nested rules in `for_each_*`.
- ✅ `extends`: local files (recursive resolution, cycle detection, child-overrides-parent merge) + HTTPS URLs with SHA-256 SRI and caching under the platform user cache dir (`~/.cache/alint/rulesets/` on Linux). Nested remote extends deferred to v0.3, a relative path inside a fetched config has no principled base.
- ✅ `fix` subcommand with `file_create`, `file_remove`, `file_prepend`, `file_append`, `file_rename` (the latter wired to `filename_case`, target name derived from the rule's `case:` setting; extension preserved).
  - Deferred for later (likely v0.5 when bundled rulesets land): `content_from: <path>` for `file_create` / `file_prepend` / `file_append`, so long bodies (LICENSE texts, standard boilerplate) can live alongside the rule rather than inline in YAML.
  - Deferred (likely v0.3): a `rename_to:` template for `filename_regex`, so the pattern's capture groups can drive a substitution target. Not yet designed.
- ✅ Output formats: `sarif`, `github`.
- ✅ Official GitHub Action (`action.yml` at repo root; composite action wrapping `install.sh`).

## v0.3: Hygiene, portable metadata, byte fingerprints (shipped)

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
- ✅ `fix_size_limit` top-level config knob (default 1 MiB; `null` disables), content-editing fixers skip oversize files with a stderr warning rather than rewrite them.
- ✅ Short-name rule aliases (`content_matches`, `content_forbidden`, `header`, `max_size`, `is_text`) for rules without a `dir_*` sibling.

**Deferred to v0.4**: structured-query primitives (`json_path_*`, `yaml_path_*`, `toml_path_*`, `json_schema_passes`), `file_footer`, `file_max_lines`, `file_shebang`, opt-in nested `.alint.yml` discovery for monorepos, `markdown` / `junit` / `gitlab` output formats, `alint facts` subcommand for debugging `when` clauses.

## v0.4: Bundled rulesets + pre-commit (shipped)

Pulled forward from what was v0.5: **bundled rulesets** are the
single biggest adoption lever, turning "write 20 rules" into
"add one `extends:` line." Also lands pre-commit framework
integration so any pre-commit user adopts alint with 4 lines of
YAML.

- ✅ `.pre-commit-hooks.yaml`, exposes `alint` (check) and `alint-fix` (manual-stage) hooks. `language: rust` means zero setup for pre-commit users.
- ✅ Bundled rulesets infra: `alint://bundled/<name>@<rev>` URI scheme resolved offline via `include_str!`. Cycle-safe, leaf-only (bundled rulesets cannot themselves `extends:`). Inherits the same `custom:`-fact guard as HTTPS extends.
- ✅ `alint://bundled/oss-baseline@v1`, 9 rules. Community docs + content hygiene most OSS repos want.
- ✅ `alint://bundled/rust@v1`, 10 rules. Gated `when: facts.has_rust` so it's a safe no-op in polyglot trees.
- ✅ `alint://bundled/node@v1`, 8 rules. Gated `when: facts.has_node`.
- ✅ `alint://bundled/monorepo@v1`, 4 rules. Language-agnostic `for_each_dir` over `{packages,crates,apps,services}/*`.

### v0.4.x point releases (shipped)

Ten point releases shipped after v0.4.0, expanding scope well
past the original cut. Most of what was originally planned
for v0.5 landed here.

- **v0.4.1**, packaging fix.
- **v0.4.2**, pretty `human` formatter overhaul.
- **v0.4.3**, composition: field-level rule override; nested
  `.alint.yml` discovery for monorepos (`nested_configs: true`).
  Four bundled rulesets: `hygiene/no-tracked-artifacts@v1`,
  `hygiene/lockfiles@v1`, `tooling/editorconfig@v1`,
  `docs/adr@v1`.
- **v0.4.4**, `file_min_size` + `file_min_lines` content
  rules; six structured-query rule kinds
  (`{json,yaml,toml}_path_{equals,matches}`). README rewritten
  as a 12-pattern cookbook.
- **v0.4.5**, `alint://bundled/ci/github-actions@v1`;
  `if_present: true` on structured-query rules; selective
  bundled adoption (`only:` / `except:` on `extends:` entries).
- **v0.4.6**, `alint://bundled/python@v1` + `alint://bundled/go@v1`;
  `alint facts` subcommand for debugging `when:` clauses.
- **v0.4.7**, distroless Docker image (`ghcr.io/asamarts/alint`)
  + Homebrew tap (`asamarts/alint`).
- **v0.4.8**, `git_tracked_only: bool`, first git-aware
  rule primitive. Closes the absence-rule false-positive on
  locally built artifacts.
- **v0.4.9**, `alint://bundled/java@v1`. First bundled use of
  `git_tracked_only`.
- **v0.4.10**, `file_max_lines` + `file_footer` + `file_shebang`
  round out the content family. Catalogue at ~55 rule kinds.

## v0.5: Monorepo scale + plugins v1 + remaining distribution

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
- ✅ **Rule templates / parameterized rules**, shipped in
  v0.5.10 (2026-04-27). New top-level `templates:` block
  defines reusable rule bodies; rules instantiate them via
  `extends_template: <id>` and a `vars:` map for the
  `{{vars.<name>}}` substitution. Templates merge through
  the `extends:` chain by id. Leaf-only, a template can't
  itself reference another, mirroring the bundled-rulesets
  restriction.
- ✅ **Selective bundled adoption.** Mapping form on `extends:`
  entries with `only: [...]` (keep listed rules) or
  `except: [...]` (drop listed rules); mutually exclusive;
  unknown ids error at load. Closes the all-or-nothing
  limitation. Shipped 2026-04-23 in v0.4.5.
- ✅ **`.alint.d/*.yml` drop-ins**, shipped in v0.5.10
  (2026-04-27). Auto-discovered next to the top-level
  `.alint.yml` and merged alphabetically; the last drop-in
  wins on field-level conflict (`/etc/*.d/` shape).
  Trust-equivalent to the main config, drop-ins live in
  the same workspace and CAN declare `custom:` facts and
  `kind: command` rules. Non-yaml files in the dir are
  skipped silently. Sub-extended configs don't get their
  own `.alint.d/`; only the top-level config does.

### Monorepo & scale

Identified by the 2026-04 monorepo positioning analysis as
the largest delta between alint's current shape and what
workspace-tier + OSS-polyglot monorepos typically reach for.
Ranked by leverage.

- ✅ **`alint check --changed [--base=<ref>]`.** Incremental
  mode: diff `git diff --name-only <base>...HEAD` (or
  `git ls-files --modified --others --exclude-standard` when
  no base) and only evaluate rules whose path scopes
  intersect the changed-file set. Cross-file rules (`pair`,
  `for_each_dir`, `every_matching_has`, `unique_by`,
  `dir_contains`, `dir_only_contains`) and existence rules
  (`file_exists`, `file_absent`, `dir_exists`, `dir_absent`)
  opt out of the changed-set filter for iteration, their
  verdicts span the whole tree by definition, but existence
  rules still skip when their `paths:` scope doesn't
  intersect the diff, so an unchanged-but-missing LICENSE
  doesn't fire on every PR. Empty diffs short-circuit to an
  empty report. Pairs naturally with `git_tracked_only`.
  Shipped in v0.5.0.
- ✅ **Per-iteration `when_iter:` filter on `for_each_dir` /
  `for_each_file` / `every_matching_has`**, shipped in
  v0.5.2 (2026-04-26). New `iter.*` namespace in the
  existing `when:` grammar exposes the iterated entry's
  `path`, `basename`, `parent_name`, `stem`, `ext`,
  `is_dir`, and `has_file(pattern)`; iterations whose
  verdict is false are skipped before any nested rule is
  built. `iter.has_file("Cargo.toml")` /
  `iter.has_file("**/*.bzl")` / `iter.has_file("BUILD") or
  iter.has_file("BUILD.bazel")` cover the
  Cargo / Bazel-style workspace gates without a
  language-specific parser.
- ✅ **`alint init [--monorepo]` discovery preset**, shipped
  in v0.5.4 (2026-04-26). New `alint init` subcommand
  detects ecosystem (Rust / Node / Python / Go / Java) from
  root manifests and writes a `.alint.yml` extending the
  matching bundled rulesets. With `--monorepo`, also
  detects Cargo `[workspace]`, pnpm-workspace.yaml, and
  `package.json` `workspaces` field, and emits
  `monorepo@v1` + `monorepo/<flavor>-workspace@v1` plus
  `nested_configs: true`. Bazel / Lerna / Nx / Turbo
  detection deferred (the three covered flavours match the
  bundled-overlay set). The runtime-side `--monorepo` flag
  on `alint check` is deferred too, `alint init` is the
  primary adoption shape.
- ✅ **Workspace-aware bundled rulesets**, shipped in
  v0.5.3 (2026-04-26). Three thin overlays on
  `monorepo@v1`: `monorepo/cargo-workspace@v1`,
  `monorepo/pnpm-workspace@v1`,
  `monorepo/yarn-workspace@v1`. Each gated by an
  `is_*_workspace` fact (declared inline in the ruleset)
  and uses `when_iter: 'iter.has_file(...)'` to scope
  per-member checks to actual package directories, no
  false positives on stray `crates/notes/` or
  `packages/drafts/`.
- ✅ **Documented scale ceiling**, shipped in v0.5.6
  (2026-04-26) as `xtask bench-scale`. Publishes
  hyperfine timings across (size × scenario × mode)
  matrix with hardware fingerprint; numbers under
  [`docs/benchmarks/macro/results/linux-x86_64/v0.5.7/`](https://github.com/asamarts/alint/tree/main/docs/benchmarks/macro/results/linux-x86_64/v0.5.7).
  1M-file size opt-in via `--include-1m`.
- ✅ **Competitive comparisons**, shipped in v0.5.7
  (2026-04-26). Same harness now drives ls-lint,
  Repolinter, and `find` + `ripgrep` pipelines alongside
  alint, gated to the scenarios each tool can sanely
  express. Reproducibility via the
  `ghcr.io/asamarts/alint-bench` Docker image (pinned
  versions of every competitor) plus a `--docker` flag
  that re-execs the bench inside the image.

### Other scope

- ✅ Structured-query primitives (v0.4.4, 2026-04-23): `json_path_equals`, `json_path_matches`, `yaml_path_equals`, `yaml_path_matches`, `toml_path_equals`, `toml_path_matches`. JSONPath per RFC 9535; YAML and TOML coerce through serde into the same tree shape. `json_schema_passes` still ⏳.
- ✅ `if_present: true` on structured-query rules (v0.4.5).
- ✅ Additional content primitives (v0.4.10): `file_footer`, `file_max_lines`, `file_shebang`.
- ✅ `alint facts` subcommand (v0.4.6).
- ✅ Homebrew formula via `asamarts/alint` tap (v0.4.7).
- ✅ Distroless Docker image at `ghcr.io/asamarts/alint` (v0.4.7).
- ✅ Git-aware primitive: `git_tracked_only` (v0.4.8).
- ✅ Additional bundled rulesets: `python` (v0.4.6), `go` (v0.4.6), `ci/github-actions` (v0.4.5), `java` (v0.4.9).
- ✅ Output formats: `markdown`, `junit`, `gitlab`, shipped in v0.5.8 (2026-04-26). Brings the format count to seven; SARIF / GitHub / `JUnit` / GitLab fall through to the human formatter on `alint fix` since they describe findings, not remediations.
- ✅ `command` plugin kind (v0.5.1, 2026-04-26). Per-file rule wrapping any CLI on `PATH` (`actionlint` / `shellcheck` / `taplo` / `kubeconform` / etc.); exit `0` = pass, non-zero = violation carrying stdout+stderr. Trust-gated: only the user's own top-level config can declare these (mirror of the `custom:` fact gate). Pairs naturally with `--changed` so external checks become incremental in CI.
- ✅ npm shim (`@alint/alint`), shipped in v0.5.11 (2026-04-27). Closes the install-path gap for JS adopters who don't already have Cargo, Homebrew, or Docker. Wraps a download of the matching pre-built binary at install time; package itself ships zero JS runtime behaviour. Auto-published from `release.yml` alongside crates.io / Docker / Homebrew.
- ✅ Git-aware primitives: `git_no_denied_paths`, `git_commit_message`, both shipped in v0.5.9 (2026-04-27). The first fires on tracked paths matching a glob denylist (secrets / artefacts / "do not commit"); the second validates HEAD's commit message shape via regex / max-subject-length / requires-body. Both no-op silently outside a git repo.
- ✅ `json_schema_passes` primitive, shipped in v0.5.9 (2026-04-27). Validates JSON / YAML / TOML targets against a JSON Schema; reuses the same serde-tree normalisation as `json_path_*`. Schema is loaded + compiled lazily and cached on the rule via `OnceLock`.
- ✅ Remaining bundled rulesets: `compliance/reuse@v1` + `compliance/apache-2@v1` shipped in v0.5.5 (2026-04-26). Both use `file_header` for SPDX / Apache header checks; reuse adds a `dir_exists` on `LICENSES/`; apache-2 adds `file_content_matches` for the LICENSE text + `file_exists` for NOTICE. Both extend without a fact gate, adopting the ruleset signals intent.
- ✅ Additional Scorecard-overlap rules in `oss-baseline@v1`, shipped in v0.5.9 (2026-04-27). Four new rules: SECURITY.md non-empty, Dependency-Update-Tool (Dependabot OR Renovate), CODEOWNERS exists, CODEOWNERS non-empty. Branch-protection state is GitHub-API-only and out of scope; the on-disk piece (CODEOWNERS) is what alint can see. `ci/github-actions@v1` is unchanged, its scope is workflow content, not on-disk artefacts; CODEOWNERS belongs in oss-baseline.

### Generic hygiene rulesets (shipped in v0.4.3)

Identified in a research pass across Turborepo/Nx/Bazel/Cargo/
pnpm docs, OpenSSF Scorecard, Repolinter's archived corpus, and
large orgs' community-health-file conventions. Four rulesets
built on the existing primitive set, no new rule kinds needed.

- ✅ `hygiene/no-tracked-artifacts@v1`, node_modules, target,
  dist, .next, .DS_Store, editor backups, .env variants, 10 MiB
  size gate. Several auto-fixable.
- ✅ `hygiene/lockfiles@v1`, one rule per package manager
  (npm/pnpm/yarn/bun/Cargo/Poetry/uv) forbidding nested lockfiles.
- ✅ `tooling/editorconfig@v1`, `.editorconfig` + `.gitattributes`
  existence with a `text=` normalization directive.
- ✅ `docs/adr@v1`, MADR naming pattern + required `## Status`,
  `## Context`, `## Decision` sections. Gap-free numbering
  deferred (needs `numeric_sequence` primitive).

## v0.6: Agent-era bundled rulesets and output

Two bundled rulesets aimed at the most common AI-coding
leftovers, plus a new output format for agents consuming alint
inside their own self-correction loops. All work composes from
existing rule kinds, no engine changes, no new primitives:
matching the same shape as the language-ecosystem rulesets
(`python@v1`, `go@v1`, `ci/github-actions@v1`, …) shipped in
earlier cuts. The agent-driven-development moment makes
alint's existing niche especially valuable; v0.6 ships
ecosystem-specific bundled rulesets to capitalise on that
without changing the underlying tool.

- ✅ **`alint://bundled/agent-hygiene@v1`**, backup-suffix
  bans (`*.bak`, `*.orig`, `*~`, `*.swp`), versioned-duplicate
  filename guards (`*_v2.ts`, `*_old.py`), scratch-doc bans at
  root (`PLAN.md`, `NOTES.md`, `ANALYSIS.md`, …), `.env`-file
  bans, AI-affirmation regex (`"You're absolutely right"`,
  emoji watermarks), debug-residue bans (`console.log`,
  `debugger`, `breakpoint()`), and model-attributed TODO bans
  (`TODO(claude:)`, `TODO(cursor:)`, …). All composable from
  `file_absent` / `filename_regex` / `file_content_forbidden`.
- ✅ **`alint://bundled/agent-context@v1`**, hygiene rules
  for `AGENTS.md` / `CLAUDE.md` / `.cursorrules`: existence
  recommended, stub guard via `file_min_lines`, bloat guard
  via `file_max_lines` (per Augment Code research, context
  files >300 lines correlate with worse agent performance),
  stale-path heuristic via regex. Subsumes ctxlint's niche
  with no new rule kinds.
- ✅ **`--format=agent` JSON output**, sibling of
  `--format=json` shaped for LLM consumption. Each violation
  carries an `agent_instruction` field templated from the
  rule's `message` + `fix` block: a remediation phrasing
  optimised for an agent to act on, not for a human to read.
  Closes the "agents already consume our JSON, but the SARIF
  shape is awkward in their context" feedback gap.

Out-of-scope for v0.6 (deliberately): new rule kinds, semantic
analysis, secret-entropy scanning, AGENTS.md export. All of
those land in v0.7 or later.

## v0.7: New rule kinds for agentic problems (shipped)

Targeted rule-kind additions that close the gaps Tier-1
exposed. Each got a short design doc before implementation
because heuristic detection has a real false-positive surface
that bundled rulesets don't.

Per-feature design drafts live under
[`docs/design/v0.7/`](https://github.com/asamarts/alint/tree/main/docs/design/v0.7), each settled schema,
semantics, false-positive surface, implementation notes, and
open questions before code started, then was flipped to
`Status: Implemented` on the matching feature commit.

- ✅ **`markdown_paths_resolve` rule kind** (v0.7.1):
  validates that backticked paths in markdown files resolve
  to real files. Targets the AGENTS.md staleness problem more
  precisely than the v0.6 regex heuristic. Required
  `prefixes:` field eliminates the "is this a path or a
  word" question by construction.
- ✅ **`commented_out_code` rule kind** (v0.7.2), heuristic
  detector for blocks of commented-out source code, scored on
  punctuation density rather than identifier-token density
  (English prose has identifier-shaped words too). Severity
  floor `warning`, heuristics have non-zero FP rate.
- ✅ **`git_blame_age` rule kind** (v0.7.3), fire on lines
  matching a regex whose `git blame` author-time exceeds
  `max_age_days`. Closes the gap between `level: warning` on
  every TODO (too noisy) and `level: off` (accepts unbounded
  debt accumulation). Engine plumbing introduces a shared
  `BlameCache` and the `{{ctx.match}}` message placeholder.
- ✅ **`alint suggest` subcommand** (v0.7.4), scans the
  current repo for known antipatterns and proposes rules
  that would catch them. Three suggester families ship:
  bundled-ruleset (high confidence), antipattern (medium:
  agent-hygiene leftovers), stale-TODO (medium, eats the
  v0.7.3 dogfood). Three output formats (human / yaml /
  json) and a strict stdout-vs-stderr split for slow
  operations: `--progress=auto|always|never` controls
  animated bars on stderr, `-q`/`--quiet` silences both.
- ✅ **`alint export-agents-md` subcommand** (v0.7.5):
  renders the active rule set as an `AGENTS.md` directive
  block. Inline mode splices between
  `<!-- alint:start -->` / `<!-- alint:end -->` markers;
  re-runs are byte-identical (no mtime bump on round-trip).
  Closes the "67% of teams maintain duplicate configs
  between AGENTS.md and CI lint" gap by making alint the
  single source of truth.

## v0.8: Comprehensive test + bench foundation (shipped)

Five sub-phases (v0.8.2 → v0.8.5) building the
test/bench/rot-prevention foundation that engine
optimization (now v0.9) needs to land safely. Scope agreed
2026-04-28 after a four-agent coverage audit; phases
merged to `main` 2026-04-28 / 2026-04-29 with full CI
(Linux self-hosted + Coverage 90.57% + Cross-Platform
macOS/Windows + Mutants nightly) green throughout.

### v0.8.2: Rule-kind coverage uplift

Goal: every rule kind has ≥5 unit tests + ≥2 e2e (pass +
fail). Pre-v0.8.2: 34 of 54 rule kinds had 0 unit tests; 4
had 0 e2e (`json_schema_passes`, `git_no_denied_paths`,
`git_commit_message`, `command`); 3 had only pass-variant
e2e (`no_symlinks`, `executable_bit`,
`executable_has_shebang`).

- ✅ ~155 new unit tests across the 34 under-covered rule
  kinds (build / options / evaluate fires / evaluate
  silent / edge cases, the standard quintet).
- ✅ Fail-variant e2e for the 3 pass-only unix-metadata
  rules.
- ✅ E2E for the 4 zero-e2e rules.
- ✅ Integration tests (under `crates/alint-rules/tests/`)
  for the shell-out rules, `git_no_denied_paths`,
  `git_commit_message`, `command` (mirrors the v0.7.3
  `git_blame_age` integration-test pattern).

### v0.8.2: Infrastructure-crate coverage

alint-core's `engine.rs`, `walker.rs`, `registry.rs`,
`report.rs`, `error.rs`, `level.rs`, `scope.rs`,
`config.rs`, `rule.rs` all had 0 unit tests pre-v0.8.2.
The most important crate had the worst coverage of its own
internals.

- ✅ Unit tests for `walker::walk`, `Registry::build`,
  `Report` aggregation, `Scope::matches` edge cases,
  `Engine::run` (changed-mode path-scope intersection,
  fact-eval failure paths), `BlameCache` thread-safety
  under contention. `config.rs` and `rule.rs` covered
  during v0.8 housekeeping (2026-04-29).
- ✅ alint-dsl edges: extends-chain cycle detection,
  diamond inheritance, `extends:` filter validation
  (`only:` / `except:`), nested-config path-prefix
  rewriting, `.alint.d/` merge order determinism, HTTPS
  timeout / size-cap enforcement, SRI algorithm-mismatch
  errors, template-instantiation edge cases.
- ✅ **Cross-formatter snapshot test**:
  `crates/alint-output/tests/cross_formatter.rs`. Same
  fixed Report rendered through all 8 output formatters
  with 13 invariant tests. Catches silent formatter
  divergence; SARIF + agent + JSON-schema validation
  bundled in.

### v0.8.3: CLI surface coverage

trycmd 33 → 56 cases (pre-v0.8.3 had 27 happy-path; only 2
stderr snapshots).

- ✅ Stderr snapshots for every error path: `--changed`
  on non-git, `--base invalid-ref`, malformed YAML,
  unknown rule kind, `--fail-on-warning` exit-code
  verification, `export-agents-md --inline` malformed
  markers.
- ✅ Per-subcommand `--help` snapshot tests for all 9
  subcommands.
- ✅ `--color auto` × `NO_COLOR` × `CLICOLOR_FORCE`
  matrix (5 cases). Surfaced + fixed a real bug:
  CLICOLOR_FORCE wasn't honored under `--color=auto`.
- ✅ `--progress=auto|always|never` × TTY/non-TTY matrix
  via trycmd env vars + `portable-pty` integration test
  for the actual TTY branch.

### v0.8.4: Benchmark uplift

Pre-v0.8.4: 6 of ~50 rule kinds had isolated criterion
benches. 0 output formatters benched. 0 fix-throughput
benches.

- ✅ `single_file_rules.rs`, every per-file rule kind,
  parameterised over file size and tree size.
- ✅ `cross_file_rules.rs`, `pair`, `for_each_dir`,
  `every_matching_has`, `unique_by`, `dir_contains`,
  `dir_only_contains` at varying tree shapes.
- ✅ `structured_query.rs`, JSON / YAML / TOML parse +
  path-query throughput; `json_schema_passes` validation.
- ✅ `output_formats.rs`, 1k / 10k / 100k violation
  Reports rendered through all 8 formatters.
- ✅ `fix_throughput.rs`, every fix-op type on synthetic
  violation lists.
- ✅ `blame_cache.rs`, cold/warm/miss-rate
  characterisation.
- ✅ `dsl_extends.rs`, extends-chain depth + drop-in
  merge cost.
- ✅ Two new hyperfine scenarios: S4 `agent-hygiene`, S5
  `fix-pass`. Walker parallelism baseline captured for
  v0.9's `build_parallel` switch.

### v0.8.5: Regression-guard + rot-prevention

Closes the v0.8 cut. Makes test/bench rot mechanically
impossible to ship.

- ✅ **`xtask bench-compare`**, diffs two
  `target/criterion/` trees; fails when any scenario
  regresses past `--threshold` (default ±10%). PR-time
  gate-ready.
- ✅ **Baseline against v0.7.0** captured at
  `docs/benchmarks/micro/results/linux-x86_64/v0.7.0/` so v0.9 engine
  work has a documented floor.
- ✅ **Fixture-completeness test**:
  `alint-dsl/tests/schema.rs::fixture_covers_every_registered_rule_kind`
  asserts every registered kind appears in
  `all_kinds.yaml` (now 70 kinds, up from 18).
- ✅ **Scenario-coverage audit test**:
  `crates/alint-e2e/tests/coverage_audit.rs`.
- ✅ **Default-option snapshot test**:
  `crates/alint-dsl/tests/default_options_snapshot.rs`
  with elide rules for crate-internal Debug churn.
- ✅ **CLI flag inventory snapshot** per subcommand:
  `crates/alint/tests/cli_flag_inventory.rs` (separate
  from `--help` text snapshots).
- ✅ **JSON report schemas** at
  `schemas/v1/{check-report,fix-report}.json` + cross-
  formatter validation tests.
- ✅ **`cargo llvm-cov` instrumentation**:
  `.github/workflows/coverage.yml` enforces 85% line
  coverage floor (90.57% achieved). `xtask` excluded
  (dev tooling, structurally low coverage). Codecov
  upload opt-in via `CODECOV_TOKEN`.
- ✅ **`cargo mutants` nightly**:
  `.github/workflows/mutants.yml` rotates one crate per
  night.
- ✅ **Cross-platform CI**:
  `.github/workflows/cross-platform.yml` runs
  `cargo test --workspace --locked` on macOS-arm64 +
  Windows-x86_64. Caught two real production bugs not
  surfaced by the Linux lane: a glob mixed-separator bug
  in the DSL nested-config prefix handling and a
  bench-compare key separator bug, both fixed before
  merge.

### Out of scope (deferred to v0.9 engine cut)

- Per-file-rule dispatch flip (engine restructure).
- Parallel walker (`WalkBuilder::build_parallel`).
- Memory-footprint pass (Cow / lazy file content / dhat
  profile).

All three were originally v0.8 sub-themes; they shift to
v0.9 because the v0.8 test/bench foundation is the gate
that lets engine work land without regressing user-visible
behaviour.

## v0.9: Engine optimization

(Was v0.8 sub-themes 2-4 in the pre-2026-04-28 plan;
displaced by the v0.8 test/bench foundation. Per-feature
design drafts live under
[`docs/design/v0.9/`](https://github.com/asamarts/alint/tree/main/docs/design/v0.9), same shape as the v0.7
design pass.)

- ✅ **Parallel walker** (v0.9.1, 2026-04-30), replaces
  `WalkBuilder::build()` with `build_parallel()` driving a
  per-thread `ParallelVisitor` that accumulates `FileEntry`s
  in a thread-local `Vec` and merges via `Drop`. A
  deterministic `sort_unstable_by` post-sort restores the
  byte-identical output snapshot tests + formatters depend
  on. Walker bench: -64% at 10k files, -41% at 1k files,
  +61% at 100 files (1ms thread-spawn overhead, accepted
  trade per design doc).
- ✅ **Memory-footprint pass**, type-level only (v0.9.2,
  2026-04-30). `Arc<Path>` on `FileEntry::path` /
  `Violation::path`, `Arc<str>` on `RuleResult::rule_id` /
  `policy_url`, `Cow<'static, str>` on `Violation::message`.
  Per-violation path / id clones become atomic refcount
  bumps. Byte-slice scanning + bounded prefix/suffix reads
  bundled into v0.9.3 alongside the dispatch flip (rule
  bodies get touched once instead of twice).
- ✅ **Per-file-rule dispatch flip + 8-rule reference
  migration** (v0.9.3, 2026-04-30). New `PerFileRule`
  trait; engine partition; file-major loop reads each
  matched file once and dispatches to every applicable
  rule. 6 line-oriented rules (`no_trailing_whitespace`,
  `final_newline`, `line_endings`,
  `max_consecutive_blank_lines`, `indent_style`,
  `line_max_width`) and 2 bounded-read rules
  (`file_starts_with`, `file_ends_with`) migrated;
  bounded-read helpers (`read_prefix_n` / `read_suffix_n`)
  in `crates/alint-rules/src/io.rs`.
- ✅ **Content-rule mechanical migration** (v0.9.4,
  2026-04-30). 16 more per-file content rules opt into
  the dispatch flip: `file_content_matches`,
  `file_content_forbidden`, `file_header`, `file_footer`,
  `file_shebang`, `file_max_lines`, `file_min_lines`,
  `file_hash`, `file_is_ascii`, `file_is_text`, `no_bom`,
  `no_bidi_controls`, `no_zero_width_chars`,
  `no_merge_conflict_markers`, `markdown_paths_resolve`,
  `structured_path` (json/yaml/toml_path_*).
  `file_max_size` / `file_min_size` stay rule-major
  (metadata-only); `json_schema_passes` stays rule-major
  (repo-level error path doesn't fit per-file dispatch).
- v0.8.5's `bench-compare` gate catches any regression
  from the engine restructure for free.

<!-- alint:internal-start -->
### Reopened sub-phases (2026-05-01)

A scaling-profile investigation surfaced that the v0.9.4 1M
S3 hyperfine number had drifted +28-37% vs v0.5.6, every
`for_each_dir` rule with a `crates/*` select instantiated a
fresh nested rule per matched directory and each ran a
linear scan of `ctx.index.files()`, giving O(D × N) shape
that hit ~5 B glob-match ops at 1M with 5,000 packages.

- ✅ **Cross-file dispatch fast paths** (v0.9.5, released
  2026-05-01). Lazy `OnceLock<HashSet<Arc<Path>>>` path-index
  on `FileIndex` (`contains_file(&Path) -> bool` is the
  canonical O(1) "does this path exist?" query); literal-
  path fast paths in `file_exists::evaluate`,
  `structured_path::evaluate`, and the `iter.has_file`
  `when_iter:` builtin; `pair` switches `find_file().is_some()`
  to `contains_file`. Engine adds `tracing::info!` per-phase
  + per-cross-file-rule wall-time emission via
  `ALINT_LOG=alint_core=info`. **Numbers (1M S3 hyperfine,
  --warmup 1 --runs 3):** full 731.9 s → 11.19 s ± 0.15
  (65×); changed 724.4 s → 6.73 s ± 0.06 (108×). Also ~80×
  faster than the v0.5.6 baseline.
- ✅ **Coverage audits** (v0.9.5.6). Three new tests under
  `crates/alint-e2e/tests/`: pass/fail symmetry per kind;
  bundled-ruleset coverage; git-mode symmetry. Plus a soft-
  warning bench-coverage listing.
- ✅ **Coverage scenarios** (v0.9.5.7). 16 new YAMLs under
  `crates/alint-e2e/scenarios/check/<family>/` filling the
  audit punch list. E2e count 205 → 221.
- ✅ **Bench-scale extension** (v0.9.5.8). S1-S5 unchanged;
  three new perf-shape scenarios, S6 (per-file content
  fan-out), S7 (cross-file relational), S8 (git-tracked
  overlay). New `alint_bench::tree::generate_git_monorepo`
  helper.
- ✅ **Rule-authoring workflow doc** (v0.9.5.9).
  `docs/development/rule-authoring.md` codifies the
  four-step workflow new rules / bundled rulesets / aliases
  follow. Self-dogfooding via `action-selftest.yml` covers
  the in-repo lint enforcement.
- ✅ **`scope_filter:` primitive** (v0.9.6, released
  2026-05-02). Closest-ancestor manifest scoping for per-
  file rules, `Rule::scope_filter() -> Option<&ScopeFilter>`
  trait method, engine integration in `Engine::run_per_file`'s
  applicable-filter, walks `Path::parent()` upward and
  consults the v0.9.5 path-index at each step. The five
  bundled ecosystem rulesets (`rust@v1`, `node@v1`,
  `python@v1`, `go@v1`, `java@v1`) renamed `is_*` → `has_*`,
  broadened heuristics to catch nested manifests, and added
  `scope_filter:` to per-file content rules so they only
  fire on files inside their ecosystem's package subtree.
  New bench-scale S9 scenario (nested polyglot:
  rust + node + python over `crates/` + `packages/` +
  `apps/`) at K100 = 688 ms ± 13 ms. Full design:
  [`docs/design/v0.9/scope-filter.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/scope-filter.md).
- ✅ **`scope_filter:` runtime no-op fix + audit cleanup +
  v0.10 LSP design pass** (v0.9.7, released 2026-05-02).
  v0.9.6 shipped the field, the runtime types, and the
  engine gate but no per-file rule builder threaded the
  parsed filter onto the built rule, `Rule::scope_filter()`
  always returned the trait default `None`, so the gate was a
  silent no-op. v0.9.7 wired each of 25 per-file content
  rules through `parse_scope_filter()`. Same release added
  10 cross-file `reject_scope_filter_on_cross_file` unit
  tests, the release.yml preflight gate (fmt + clippy + test
  + `cargo doc -D warnings`), and the v0.10 LSP design pass
  (`docs/design/v0.10/{lsp_server,single_file_reevaluation,vscode_extension}.md`
  at the time; files moved to `v0.11/` in v0.9.22 when v0.10
  scope flipped to case-study coverage) with `tower-lsp = "0.20"`
  added as a dormant workspace dependency.
- ✅ **Cross-file dispatch fast paths round 2**
  (v0.9.8, released 2026-05-02). `FileIndex::children_of`,
  `file_basenames_of`, `descendants_of` lazy `OnceLock`
  indexes; `dir_only_contains` + `dir_contains` rewrite to
  use `children_of` instead of full-index scan;
  `evaluate_for_each` (shared by `for_each_dir`,
  `for_each_file`, `every_matching_has`) gains a literal-
  path bypass for nested per-file rules. **1M S7 full:
  614 s → 15.3 s (40×); 1M S7 changed: 618 s → 17.9 s
  (34×).** New `coverage_audit_cross_file_dispatch.rs` soft
  audit. Full design:
  [`docs/design/v0.9.8/cross-file-fast-paths-v2.md`](./v0.9.8/cross-file-fast-paths-v2.md).
- ✅ **`scope_filter:` coverage sweep** (v0.9.9, released
  2026-05-03). 17 rules whose `Rule::evaluate` iterates
  `ctx.index.files()` directly (rather than opting into
  the engine's per-file dispatch) had no `scope_filter`
  plumbing at all, same shape as the v0.9.6 → v0.9.7 bug
  for per-file content rules but on a different rule set.
  16 rules now honour the filter (`file_max_size`,
  `file_min_size`, `no_empty_files`, `executable_bit`,
  `executable_has_shebang`, `shebang_has_executable`,
  `no_symlinks`, `filename_case`, `filename_regex`,
  `no_illegal_windows_names`, `max_files_per_directory`,
  `max_directory_depth`, `json_schema_passes`, `command`,
  `git_blame_age`, `no_case_conflicts`); `no_submodules`
  rejects `scope_filter:` at build time via the new
  `reject_scope_filter_with_reason` helper since it's
  hardcoded to `.gitmodules` at the repo root. Same release
  added a `for_each_dir` literal-path bypass guard (the
  v0.9.8 fast path was skipping `nested_rule.scope_filter()`)
  and a new S10 macro bench scenario (scope_filter outside
  the per-file dispatch path).
- ✅ **`Scope` owns `Option<ScopeFilter>` (structural fix)**
  (v0.9.10, released 2026-05-03). The v0.9.6 / v0.9.7 / v0.9.9
  silent-no-op bug class is closed structurally. `Scope`
  bundles its `Option<ScopeFilter>` and `Scope::matches(&Path,
  &FileIndex)` covers both predicates in one call; 41 rules
  cleaned up to drop the standalone `scope_filter` field and
  runtime check; `Rule::scope_filter()` trait method removed;
  new `coverage_audit_scope_owns_filter.rs` audit fails CI
  if the field re-appears. **Breaking**, `alint-core::Scope::matches`
  signature changed; CLI users + bundled rulesets unaffected,
  out-of-tree library consumers see a compile error at every
  call site. Workspace-wide -319 LOC. Same release bundled
  v0.9.11-prep cleanups: alint-bench clippy debt,
  `git_tracked_only` audit test, HISTORY.md regenerated with
  v0.9.7-v0.9.10 columns. Full design:
  [`docs/design/v0.9/scope-owns-scope-filter.md`](./v0.9/scope-owns-scope-filter.md).

Full v0.9.5 design: [`docs/design/v0.9/coverage-and-dogfood.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.9/coverage-and-dogfood.md).
<!-- alint:internal-end -->

## v0.9.11: `git_tracked_only` structural fix + held v0.9 follow-ups

The same recurrence-risk shape that produced the v0.9.6 /
v0.9.7 / v0.9.9 silent-no-op `scope_filter:` bug class
applies to `git_tracked_only` (declared on 4 existence
rules; an audit test landed in v0.9.10 as a pragmatic
backstop, but the structural fix was held). v0.9.11
chooses the engine-side filtered-FileIndex approach
(Option C in the design comparison) over Scope ownership
(Option A), there's no per-rule check to forget at all,
and the `dir-mode vs file-mode` discriminator never
pollutes `Scope`'s path-predicate model.

- **Engine-side filtered FileIndex** for git-tracked rules.
  `Engine::build_filtered_index` mirrors the existing
  `--changed` filtered-index pattern: builds a
  file-tracked subset (`set.contains(path)` filter) and a
  dir-tracked subset (`dir_has_tracked_files` filter) once
  per run when any rule opts in. Existence rules
  (`file_exists`, `file_absent`, `dir_exists`, `dir_absent`)
  receive the pre-filtered index via `pick_ctx`; the
  `if self.git_tracked_only && !ctx.is_git_tracked(...)`
  runtime check disappears from each rule's `evaluate`.
- **`Rule::wants_git_tracked()` engine consultation
  consolidation.** Once the filtered index handles the
  narrowing, `wants_git_tracked()` can derive from
  inspecting rules' specs at engine-construction time
  rather than per-rule trait override.
- **No breaking API change** (in contrast to v0.9.10):
  the change is internal to engine + 4 rules.
- **Acceptance**: S8 macro bench at 100k/full and 1m/full
  within ±5 % of v0.9.10 (slight win expected from
  amortising the HashSet lookup across multiple rules
  opting in); `coverage_audit_git_tracked_only.rs` audit
  retained as backstop.

<!-- alint:internal-start -->
Held v0.9 follow-ups also captured in v0.9.11 (smaller
items, may slip if engine refactor takes longer than
expected):

- **`bench-record.yml` workflow** end-to-end run. Defined
  but never successfully executed, debug the path that
  opens a PR with bench results so post-release backfill
  is mechanical instead of manual.
- **`coverage_audit_cross_file_dispatch.rs` cleanup.**
  Soft warning currently flags rules that still scan
  `entries.iter()` directly; convert to `children_of`
  where the dispatch shape benefits.
- **`when:` ownership**, explicitly NOT included.
  Different semantics (eval-env, not a path predicate);
  no shared silent-no-op shape.
<!-- alint:internal-end -->

## v0.9.12 - v0.9.20 (shipped)

Nine patch releases between the v0.9.11 structural fix and
the launch-ready cut. Headline contents (full per-version
detail in [CHANGELOG.md](https://github.com/asamarts/alint/blob/main/CHANGELOG.md)):

- **v0.9.12** (2026-05-03), backlog cleanup of the explicitly-
  held v0.9 follow-up items.
- **v0.9.13** (2026-05-04), dependency refresh (Dependabot PR
  drainage).
- **v0.9.14** (2026-05-05), CI automation: `bench-record.yml`
  workflow now opens PRs with bench results so post-release
  backfill is mechanical.
- **v0.9.15 / v0.9.16** (2026-05-06), Config DX hardening:
  21-pitfall catalogue with #18 (`respect_gitignore: false`)
  and #19 (`literal_is_nested` runtime guard) fixed in the
  engine; 30 OSS case studies completed; P2b Wave 2
  aggregation; operator-polish (`alint --version` SHA+date,
  `alint validate-config` subcommand, JSON Schema generation,
  did-you-mean parse errors, domain-specific error messages,
  pre-filled crash-report URL on panic). v0.9.16 is the
  tag-only release that never published due to a clippy gate
  failure; **v0.9.17** is the corrective re-publish.
- **v0.9.18** (2026-05-08), pre-launch fix wave: 6 bundled-
  ruleset refinements (A1-A6: hygiene-no-js-build-outputs
  sibling-package.json gate; long-form Apache-2 preamble;
  python@v1 test-fixture excludes; cargo-workspace member
  parsing; LICENSE.TXT/.md recognition; rust-sources-snake-
  case `allow_compiler_naming` knob), 3 case-study config
  fixes (B1-B3: TypeScript pitfall #22, deno defensive `|-`,
  tensorflow header rule scope), B4 cross-cutting revalidation
  pass, `dir_absent` engine extension (now supports
  `scope_filter`).
- **v0.9.19** (2026-05-09), width-aware human output wrapping
  + `--no-docs` flag; verbose-message tightening; demo refresh.
- **v0.9.20** (2026-05-10), width-aware output extended to
  every command; `cmd_list` / `cmd_explain` / `cmd_facts` /
  `cmd_suggest` color parity with `check` / `fix`; new
  `styling_uniform` integration test enforces the contract
  uniformly going forward; bundled-rule message length audit;
  em-dash scrub on first-impression marketing surfaces;
  install-snippet pin sweep (curl + bash now leads everywhere);
  polyglot CLI demo with 5 auto-fixable seeds;
  `install-snippets-match-workspace-version` dogfood rule;
  preflight + pre-push hook bundle.

## v0.10: Case-study coverage push

The 8 rule kinds + 2 bundled rulesets the case-study
aggregation demand-validated. Splits out from the original v0.10
"LSP" framing because rule-kind coverage is what unblocks early
adopters; LSP is developer-experience polish that can ship
separately (now v0.11). Order within the cut by demand × adopter
surface:

| # | Primitive / ruleset | Demand | Notes |
|---:|---|---:|---|
| 1 | `registry_paths_resolve` | 13 | Largest demand surface. Spans rust, clap, cpython, next.js, arrow, pytorch, nodejs/node, NixOS, dotnet, flutter, kubernetes, protobuf, tensorflow. |
| 2 | `cross_file_value_equals` (incl. `value_extractor:`) | 12 | Past saturation. istio surfaces the per-file extractor refinement (pitfall #20); dotnet/runtime SDK band coherence raises the count. |
| 3 | `ordered_block` | 8 | Lines between marker pairs sorted unique under configurable comparator. |
| 4 | `generated_file_fresh` | 8 | Run a generator, diff against on-disk file. Opt-in framing: alint's deliberate non-goal is running codegen. |
| 5 | `import_gate` | 5 | Forbid imports of pattern X in scope Y. k8s prometheus-imports + airflow + go + helm + pytorch. |
| 6 | `command_idempotent` mode | 5 | `--check` mode that fails if working tree would change. `ruff-format` / `prettier --check` / `dprint check` / `deno fmt --check` / `eslint --no-fix` shape. **Promoted from design candidate** in the deep-analysis aggregation. |
| 7 | `xml_path_matches` + `xml_path_equals` | 2 | Completes the structured-query family (JSON / YAML / TOML / XML). spark + dotnet/runtime ~7,100 manifests. |
| 8 | `pair_hash` | 3 | Hash of file A appears at offset Y of file B. golang/go FIPS = highest stakes (CMVP submission references the file format). |
| 9 | `apache/governance@v1` (bundled ruleset) | 3 | LICENSE + NOTICE + KEYS + RAT discipline. Apache TLPs converge: arrow + spark + airflow on 9 of 12 governance artefacts. v0.9.18 A2 is a prerequisite. |
| 10 | `dotnet@v1` (bundled ruleset) | 1 | Single demand source but huge adopter surface (every dotnet/* + every Azure SDK + every microsoft/* .NET project). Depends on `xml_path_*`. |

Plus design candidates landing opportunistically when a 2nd
demand source materialises: `*_path_contains` (3 sources today;
resolves pitfall #17), `pair_inverse` (2 sources), `command_per_repo`
(1), `json_schema_passes` config-shape mode (2), `*_path_array_iter`
(1), `multi_doc_mode:` knob on `yaml_path_*` (1; resolves
pitfall #21).

Per-repo coverage tables + per-primitive demand citations live
in [`examples/README.md`](https://github.com/asamarts/alint/blob/main/examples/README.md#primitive-demand-tracker)
and [`docs/development/launch-evidence.md`](https://github.com/asamarts/alint/blob/main/docs/development/launch-evidence.md).

## v0.11: LSP + DSL polish

**Shipped in v0.11.0** (2026-05-28): the `alint lsp` language server (the new `alint-lsp` crate) wired across the VS Code, JetBrains, Zed, Neovim, Sublime, Emacs, and Helix integrations, the `git_commit_*` commit-validation family, `scope_filter.changed_since:`, and `{{env.X}}` interpolation. The detail below was the cut's plan; `has_sibling` / `has_descendant` were deferred to v0.12.

Originally framed as "LSP + developer experience"; renamed to
capture the DSL-level work that lands alongside LSP. The trigger
was v0.9.21's #26 fix, which added `since:` to `git_commit_message`
and surfaced two natural generalisations: the scope-narrowing
pattern wants to extend to other rule kinds, and the env-var
interpolation in `since:` wants to extend to every string-typed
config field. The per-file dispatch shape from v0.9.3 powers the
per-file-edit re-eval hot path; v0.9.5's `FileIndex::contains_file`
makes single-file path-scope re-tests O(1). Plus the v0.11+ ship-
targets the case-study aggregation surfaced for the smaller-demand
long tail.

Note (post-ship): the originally-planned LSP demonstration of the
interpolation system, hover-on-rule rendering the resolved value of
every `{{env.X}}` site in the user's config, did not land in v0.11.0.
The shipped hover surfaces a violation's rule id, severity, message,
and `policy_url`; the interpolation system shipped as a config-load
feature without the hover demo.

### LSP + editor support

- LSP server (`alint lsp`), shipped as the `alint-lsp` crate built
  on `tower-lsp`. Design pass:
  [`docs/design/v0.11/lsp_server.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.11/lsp_server.md).
- VS Code extension bundling the LSP, shipped alongside the JetBrains
  and Zed extensions. Design:
  [`docs/design/v0.11/vscode_extension.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.11/vscode_extension.md).
- Tier-2 editor configs (Neovim, Sublime, Emacs) plus Helix and Eclipse,
  config-only LSP-client integrations, also shipped in v0.11.0. Neovim is
  upstreamed to nvim-lspconfig; the rest are documented snippets.

### Scope generalisation (v0.9.21 #26 follow-up)

Design pass: [`docs/design/v0.11/scope_filter_changed_since.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.11/scope_filter_changed_since.md).

- **[shipped v0.11.0]** `ScopeFilter.changed_since:` predicate. Restricts a per-file
  rule to files modified in `<since>..HEAD`. Composable with the
  existing `has_ancestor` (AND semantics) and with the rule's
  `paths:` glob. Equivalent to the existing CLI `--changed --base
  <ref>` but expressible per-rule rather than run-wide, so
  mixed-mode configs ("SPDX-header rule on new files only;
  filename-case rule always") become expressible, the first
  example that's not expressible today. Reuses v0.9.5's path-
  index for cheap per-file gating.
- **[still deferred after v0.12.0]** `has_sibling`, `has_descendant` predicates (carried over
  from the original v0.11 plan). v0.9.10's `Scope::from_spec`
  makes additions purely additive (no API churn). **Not built in
  v0.11, and not in v0.12.0 either** (see that version's "Deferred from
  v0.11"). Only `changed_since` shipped here.
- **[shipped v0.11.0]** `git_no_denied_paths` `since:` option. Path-listing analog
  of v0.9.21's `git_commit_message.since:`. Fires on tracked
  paths *added* in `<since>..HEAD`, not on all currently-tracked
  paths. Closes the secrets-introduced-in-PR gap cleanly.
  Conceptually a sibling to `scope_filter.changed_since:`, kept
  as a rule-level option because the rule's semantics already
  enumerate paths from git rather than walking the tree.

### Commit-validation rule family (v0.9.21 #26 follow-up)

Design pass: [`docs/design/v0.11/commit_validation_rules.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.11/commit_validation_rules.md).

Four new rule kinds that share v0.9.21's `git_commit_message`
shape (`since:`, `include_merges:`, env-var interpolation,
per-commit violations with abbreviated SHAs). Designed as a
family so the shared infrastructure cost amortises across all
four. **All four shipped in v0.11.0.**

- **`git_commit_signed_off`**, DCO-style `Signed-off-by:`
  trailer in commit footer. Maps to the kernel / Linux
  Foundation contribution convention; demand surface includes
  every CNCF / LF-hosted project that requires DCO.
- **`git_commit_no_fixup`**, fail on residual `fixup!` /
  `squash!` / `amend!` commits left after rebase. Catches the
  "I forgot to rebase before push" PR shape.
- **`git_commit_author_allowlist`**, author email or name
  matches a pattern. Use cases: enforce committer identity
  against an org domain, exclude bot accounts, gate against
  unverified contributors.
- **`git_commit_gpg_signed`**, `git verify-commit` succeeds
  on every commit in range. Sister to `git_commit_signed_off`
  for projects requiring signature attestation.

All four ship with `since:` from day one. No HEAD-only-then-
retrofit shape; the issue-26 work made the shared infrastructure
cheap to reuse.

### Variable expansion across the DSL (v0.9.21 #26 follow-up)

Design pass: [`docs/design/v0.11/variable_interpolation.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.11/variable_interpolation.md).

- **[shipped v0.11.0]** `{{env.X}}` interpolation at config-load time across every
  string-typed value field: `extends:` URLs, `paths:`, `pattern:`,
  `policy_url:`, `since:`, `changed_since:`, `content:`,
  `content_from:`, and the `vars:` value side. Type-like and
  identifier-like fields (`id:`, `kind:`, `level:`) are skipped
  by design, env-driven rule IDs would break audit trails.
- **[shipped v0.11.0]** `| default(...)` filter for fallbacks. Jinja-conventional;
  future filter additions cost nothing (`| upper`, `| lower`).
  Example: `since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"`.
- **[shipped v0.11.0]** `env.X` in the `when:` expression language as a third
  namespace alongside `vars.X` and `facts.X`. Symmetric with how
  vars/facts surface today.
- **[shipped v0.11.0, deprecation warning]** `${VAR}` in `git_commit_message.since:` is deprecated. Emits a
  load-time warning recommending `{{env.X}}`; remove the legacy
  path in v1.0 (clean break at the version-stability gate).
- **[not shipped]** the planned LSP × DSL hover (render the resolved
  value of every `{{env.X}}` site on hover) did not land; v0.11.0's hover
  surfaces a violation's rule id, severity, message, and `policy_url`.

### Long-tail rule kinds (opportunistic)

Carried over unchanged from the original v0.11 plan; not gating
the release. Anything not picked up opportunistically here rolls
into the v0.12 gap-closing cut (see below), where the 100+ repo
study refreshes the demand ranking. **None were picked up in v0.11; all
carried into the v0.12 cut (per-item status below).**

- **[expressible in v0.12.0 via `cross_file` `set_equals`]** `cross_language_implementation_complete` (5 sources:
  arrow, tensorflow, protobuf, angular, flutter). Densest
  demand: protobuf's 10 in-tree language bindings + 1 spun-out
  (~45 cross-language assertions one rule would express).
  Three distinct topologies (data-format-driven, within-
  language source ↔ golden, platform-driven).
- **[still open]** Bazel-licensing-declaration-aware rule kind (1 source:
  tensorflow's `licenses(["notice"])` BUILD-file discipline).
  Single-source but the source is a 100k+ file tree with high
  alignment cost.
- **[still deferred, see v0.12 "Deferred from v0.11"]** `walk_error_policy:` engine knob (1 source: pnpm's
  `tests/fixtures/has-broken-symlinks/`). `strict` /
  `skip-broken-symlinks` / `permissive` modes.

## v0.12: Real-world coverage expansion (100+ repos) + gap rule kinds

**v0.12.0 shipped** (2026-06-07) a large first cut of this scope. New rule kinds that landed: `file_graph` (file-dependency-graph firewalls plus cycle / orphan / dangling-edge checks, the top demand-ranked kind of the 111-repo study), `for_each_match` (a per-line predicate quantifier), `cross_file` (a unifying value-relation kind that subsumes `cross_file_value_equals` and adds `subset` / `superset` / `set_equals` / `identical` / `resolves`), `git_commit_subject_matches`, `changeset_requires_path`, and `pair_changed_together`. Also shipped: `generated_file_fresh`'s in-place `outputs:` mode, markerless `ordered_block`, the `php@v1` bundled ruleset (21 → 22), JSONC-tolerant structured parsing, the `cross_file` `normalize:` promotion, and `import_gate` presets for Scala / Java / Dart / Nix, plus a security cycle that confines every config-declared path to the repo root and fixes a git argument-injection in `since:` reaching back to v0.9.21. Per-item status is tagged inline below. The 100+ repo study (111 repos done) and the still-open gap kinds continue in the v0.12 line.

The v0.10 case-study coverage push and the post-v0.11 30-repo
re-analysis (see
[`docs/development/case-study-v011-reanalysis-log.md`](https://github.com/asamarts/alint/blob/main/docs/development/case-study-v011-reanalysis-log.md))
proved a method: clone a real repo, catalogue its bespoke
manual/script/CI validation, express as much as alint can, and record
the residual as either a concrete new-kind candidate or a deliberate
non-goal. The 30-repo pass found no v0.10/v0.11 regressions but
surfaced a ranked backlog of additive gaps. v0.12 scales the study
3-4× (100+ repos, broader ecosystem spread) **and** closes the
rule-kind gaps the 30-repo pass already identified, folding any
further gaps the wider study turns up into the same cut.

Design index: [`docs/design/v0.12/`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/README.md).

### The 100+ repo case study

Design pass: [`docs/design/v0.12/case_study_100_repos.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/case_study_100_repos.md).

- **[done in v0.12.0: 111 repos]** the corpus the v0.12.0 rule kinds were derived from. As planned: expand from 30 to 100+ OSS repos. The 30-repo corpus skewed toward
  big-tech monorepos + Rust/Go/JS; the expansion deliberately broadens
  ecosystem coverage: more Python (django, pandas, scikit-learn,
  fastapi, poetry), JVM (spring-boot, gradle, kotlin), Ruby (rails),
  PHP (laravel, symfony), systems (llvm, postgres, redis, sqlite,
  curl), infra/data (terraform, ansible, grafana, prometheus, dbt),
  ML (transformers, jax), web (vue, svelte, solid), and a long tail of
  mid-size single-language libraries, where the bespoke-validation /
  alint fit is often highest.
- Same per-repo methodology, run through the proven subagent
  batch-orchestration pipeline (clone → catalogue → draft config →
  parent validates with `validate-config` / integrates / commits per
  batch).
- Outputs: per-repo example configs, an expanded findings log, and a
  refreshed gap ranking that supersedes the 30-repo synthesis.
- Gate: the study runs first; any new-kind candidates it surfaces
  beyond the list below are triaged into this same release.

### Gap rule kinds (ranked by demand from the 30-repo pass)

Each ships design-doc-first per the project convention; demand counts
name the corpus repos that independently needed the capability.

- **[shipped v0.12.0]** `git_commit_subject_matches` (go, node, nixpkgs), the
  commit-validation family's missing subject-shape rule. Reuses the
  v0.11 `git_commit_*` plumbing (`since:`, per-commit violations with
  abbreviated SHAs); the cheapest, clearest single win. Design:
  [`git_commit_subject_matches.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/git_commit_subject_matches.md).
- **[both shipped v0.12.0]** the diff-aware "must-add" family, `changeset_requires_path` ("the
  diff must add a file matching glob X": prettier changelog_unreleased,
  cpython Misc/NEWS.d, pnpm `.changeset/`) and `pair_changed_together`
  (two files must change in one commit: rust rustdoc_json
  FORMAT_VERSION, turbo/rust release guards). Both build on v0.11's
  `scope_filter.changed_since` machinery. Design:
  [`changeset_requires_path.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/changeset_requires_path.md).
- **[shipped v0.12.0, unified into `cross_file` as `subset` / `superset` / `set_equals`]** the value-set membership family, originally proposed as `registry_value_used` (every TS
  diagnostic / react error code referenced ≥1×), `cross_file_keys_cover`
  (pnpm catalog ⊆ keys), `cross_file_set_equals` (rust features ↔
  unstable-book, tf v1/v2 goldens). `cross_file_value_equals` is 1:1
  and `pair_hash` is digest-only; N-in-1 / set relations are the gap.
  First verify how far `registry_paths_resolve`'s existing
  `orphans` / `must_contain` / `exclude_query` already reaches. Design:
  [`value_set_membership.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/value_set_membership.md).
- **[shipped v0.12.0 on `cross_file`, as `semver-minor` plus composable lists]** `normalize:` value-transform
  (protobuf `4.36-dev` ↔ `4.36.0`, pnpm `pnpm@11.3.0` ↔ `11.3.0`):
  strip-prefix / semver-floor transforms so "same value, two forms"
  stops forcing dual regex pins. Extends the existing trim/lower
  `normalize:`. Design:
  [`cross_file_normalize.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/cross_file_normalize.md).
- **[partly shipped]** richer `import_gate`: a default-deny / table-driven allowlist
  mode (vscode `code-import-patterns`) and glob-discovered
  per-directory rule files (k8s's 66 `.import-restrictions` = 66
  hand-written rules today). The whole-repo layering-firewall this targets
  shipped instead as `file_graph` `forbidden_edges` in v0.12.0; the
  `import_gate` default-deny / glob-discovery enrichment itself is still open.
  Design:
  [`import_gate_enrichment.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/import_gate_enrichment.md).
- **[still open]** dependency-graph allowlist kind (distinct from `import_gate`):
  a `cargo metadata` / lockfile-aware permitted-dependency firewall for
  rust `PERMITTED_DEPENDENCIES` and go's transitive `deps_test.go`
  closure. `import_gate` reads source text, not the resolved graph, so
  this is genuinely separate. v0.12.0's `file_graph` covers the file-reference
  graph but stays path-based (the resolved package graph is its declared
  non-goal), so this lockfile-aware dependency firewall is still unbuilt. Design:
  [`dependency_graph_allowlist.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/dependency_graph_allowlist.md).
- **[partly shipped]** niche kinds (1-2 sources each, one design doc):
  `embedded_checksum` (cpython Argument Clinic self-digests), full-file
  `lines:{}` equality with diff-on-mismatch (tokio README mirror:
  `pair_hash` reports only a digest mismatch), `no_case_collisions`
  (tensorflow Windows dup-casing), `dir_name_equals_field` (turbo crate
  dir ↔ name), `cross_language_implementation_complete` (carried from
  the v0.11 long-tail; arrow/tf/protobuf/angular/flutter parity), and a
  Bazel-licensing-declaration-aware kind (tensorflow `licenses([...])`
  discipline; also a v0.11 long-tail carryover). Of these, v0.12.0 shipped the
  full-file `lines:{}` equality as `cross_file` `identical`, `no_case_collisions`
  as `unique_by` `case_insensitive:`, and cross-language parity as `cross_file`
  `set_equals` over a glob-union source; `embedded_checksum`,
  `dir_name_equals_field`, and the Bazel-licensing kind are still open. Design:
  [`niche_rule_kinds.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/niche_rule_kinds.md).
- **[not shipped; `php@v1` shipped instead]** `nix@v1` ecosystem bundle (nixpkgs).
  v0.12.0 shipped `php@v1` (the higher-demand ecosystem that lacked a bundle),
  taking the bundled count to 22, and gave Nix an `import_gate` preset; a
  dedicated `nix@v1` bundle is still open.

### Deferred from v0.11

Design pass: [`deferred_from_v011.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/deferred_from_v011.md).

v0.11-plan items not picked up before the cut, with no home in the
case-study gap backlog, collected here so they don't slip untracked.
None gated v0.11; all are additive and slip-tolerant. All three remain open after v0.12.0.

- **`has_sibling` / `has_descendant` scope predicates**, the two
  `ScopeFilter` predicates from the v0.11 scope-generalisation plan that
  did not ship alongside `changed_since`. Purely additive via v0.9.10's
  `Scope::from_spec`.
- **`walk_error_policy:` engine knob** (1 source: pnpm broken-symlink
  fixtures), `strict` / `skip-broken-symlinks` / `permissive` walker
  modes. Single-source and touches the walker error path; the 100-repo
  study (far more filesystem shapes) is the right place to settle the
  mode set first.
- **LSP "Add rule to ignore" code action**, explicitly deferred in
  v0.11's `lsp_server.md` (it edits `.alint.yml`, a separate design
  decision). Only relevant if v0.12 reopens LSP work.

### Bundled-ruleset + engine tuning

Design pass: [`asf_bundle_overfire.md`](https://github.com/asamarts/alint/blob/main/docs/design/v0.12/asf_bundle_overfire.md).

- **[shipped v0.12.0]** fix the ASF compliance-bundle over-fire.
  `compliance/apache-2@v1` and `apache/governance@v1` over-fire on
  every large Apache/CNCF repo in the corpus, 5 confirmations
  (airflow, helm, istio, kubernetes, tensorflow). Universal cause:
  branded/abbreviated headers, generated files
  (`.pbtxt`/`.pb.go`/`.gen.go`/`_pb2.py`), `third_party/` vendored
  trees, "The X Authors" attribution, no top-level NOTICE. Ship
  generated-file + third_party excludes and header tolerance in the
  bundles, with a documented per-rule override recipe for the rest.
  Every batch of the 30-repo pass independently re-derived the same
  `paths.exclude` workaround, strong signal the bundle defaults are
  wrong for real ASF repos.
- **[shipped v0.12.0]** `import_gate` presets for scala / java / dart / nix (generic +
  explicit `import_pattern` works but a preset is cleaner; spark,
  flutter, nixpkgs).
- **[shipped v0.12.0]** docs: `generated_file_fresh` is stdout-only. Real codegen
  mutates files in place, so `command_idempotent --check` is the
  broadly-applicable form. Make the distinction explicit in the rule
  reference so users don't reach for the wrong kind (the dominant
  pattern across the corpus was the mutating one). v0.12.0 also added the
  in-place `outputs:` mode to `generated_file_fresh` for that mutating pattern,
  so the kind now covers it directly.

## Engineering foundations: spec-driven development

A drift-elimination program run on `main` after v0.12 — a foundations track, not a
user-facing version cut, so it interleaves with the release cadence rather than
occupying a slot. The premise an anti-drift linter has to honour itself — *alint's
own claims can't drift from the tool* — now holds end to end. Five workstreams
shipped, each artifact regenerate-and-diff gated:

- **Schema from types.** `schemas/v1/config.json` is generated from the Rust
  `Options` structs (schemars) and gated by `gen-schema --check`, so a field rename
  propagates to the schema (and the IDE) automatically.
- **Generated rule reference.** Every rule page's `## Options` table is rendered
  from the type-derived schema (name / type / required / default / constraints), so
  the published options can't drift from the engine.
- **`facts.json` contract.** A committed surface-area manifest — version, the six
  headline counts (rule kinds, families, rulesets, fix ops, output formats,
  subcommands), and catalogue lists — gated by `gen-facts --check`. alint.org renders
  these numbers from it, so the marketing site can't lag a release.
- **Architecture as code.** The crate dependency graph is extracted from
  `cargo metadata` into a committed Mermaid diagram (runtime vs dev/build edges) and
  a hand-modeled Structurizr C4 model is kept honest against the workspace members,
  both gated by `gen-arch --check`.
- **Pragmatic formal methods.** proptest properties as an always-on behaviour spec,
  plus a verified Kani bounded proof of the path-confinement security policy.

Recorded in [`spec-driven-development.md`](https://github.com/asamarts/alint/blob/main/docs/design/spec-driven-development.md),
[ADR-0001](https://github.com/asamarts/alint/blob/main/docs/adr/0001-adopt-spec-driven-development.md),
and the per-workstream design docs. An adversarial multi-reviewer pass after the cut
found and fixed a CI gate bypass, a too-weak Kani proof, and a crate graph that
conflated dev and runtime edges — the value of reviewing your own anti-drift work
adversarially.

## v0.13: WASM plugins

Bumped one slot to make room for the v0.12 real-world-coverage cut;
otherwise unchanged from the previous post-v0.11 scope.

- `wasm` plugin kind with a `wasmtime` host, stable WIT
  interface. Plugins receive their config *post-interpolation*
  per the v0.11 variable-expansion work, the host resolves
  `{{env.X}}` references before passing the config dict to the
  guest, so plugins never see (and can't accidentally exfiltrate)
  the raw env namespace.
- Plugin registry scaffolding with signature verification.
- Bless a few canonical agent-aware semantic plugins
  (mock-ratio checker, file-similarity / near-dup detector,
  debug-statement auto-stripper) as documented examples, not
  bundled, to keep the binary lean.

## v1.0: Stability

- DSL schema committed; semver on `version: 1`.
- Plugin ABI committed.
- `alint-core` public API frozen; breaking changes follow semver-major.
- Documentation site.
- **Remove the deprecated `${VAR}` interpolation path** in
  `git_commit_message.since:` (replaced by `{{env.X}}` in v0.11).
  Clean break at the version-stability gate; the
  deprecation-warning overlap window of one minor release is
  enough for a feature that shipped four days before its
  successor.
