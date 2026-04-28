# alint — Roadmap

> This roadmap is scope-based; dates are deliberately omitted. Each version is a
> closed cut — work that doesn't fit moves to a later version. See
> [ARCHITECTURE.md](./ARCHITECTURE.md) for the design these phases build out.

**Latest release: v0.7.0** (2026-04-28). Closes the v0.7
cut. Three new rule kinds (`markdown_paths_resolve`,
`commented_out_code`, `git_blame_age`) and two new
subcommands (`alint suggest`, `alint export-agents-md`)
targeting agent-driven development workflows specifically.
Where v0.6 was config-only — bundled rulesets composed
from existing primitives — v0.7 extends the engine itself
with new rule kinds and new CLI surface. Schema-compatible:
every v0.6 config runs unchanged; the new rule kinds parse
new YAML shapes that older configs simply don't use, and
`version: 1` continues to cover them. See
[CHANGELOG.md](../../CHANGELOG.md) for the full feature list.

**Next: v0.8 — Performance & test-floor.** Revival of the
cut that was forward-pointed on 2026-04-27 (v0.5.11 day) and
then dropped the same afternoon when the v0.6 agent-era
re-prioritisation took its slot. With the v0.6 / v0.7 cuts
shipped, the original perf agenda is back on the front
burner: a regression-guard test layer that gates CI on
benchmark drift, a per-file-rule dispatch flip in the
engine (current shape re-walks the index per rule), a
parallel walker via `ignore::WalkBuilder::build_parallel`,
and a memory-footprint audit. Self-justified on its own
merits, and a natural unblocker for v0.9's LSP — which
also wants per-file dispatch (file change → re-evaluate
that file's rules). LSP shifts to v0.9; WASM plugins to
v0.10.

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
- ✅ **Rule templates / parameterized rules** — shipped in
  v0.5.10 (2026-04-27). New top-level `templates:` block
  defines reusable rule bodies; rules instantiate them via
  `extends_template: <id>` and a `vars:` map for the
  `{{vars.<name>}}` substitution. Templates merge through
  the `extends:` chain by id. Leaf-only — a template can't
  itself reference another, mirroring the bundled-rulesets
  restriction.
- ✅ **Selective bundled adoption.** Mapping form on `extends:`
  entries with `only: [...]` (keep listed rules) or
  `except: [...]` (drop listed rules); mutually exclusive;
  unknown ids error at load. Closes the all-or-nothing
  limitation. Shipped 2026-04-23 in v0.4.5.
- ✅ **`.alint.d/*.yml` drop-ins** — shipped in v0.5.10
  (2026-04-27). Auto-discovered next to the top-level
  `.alint.yml` and merged alphabetically; the last drop-in
  wins on field-level conflict (`/etc/*.d/` shape).
  Trust-equivalent to the main config — drop-ins live in
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
  opt out of the changed-set filter for iteration — their
  verdicts span the whole tree by definition — but existence
  rules still skip when their `paths:` scope doesn't
  intersect the diff, so an unchanged-but-missing LICENSE
  doesn't fire on every PR. Empty diffs short-circuit to an
  empty report. Pairs naturally with `git_tracked_only`.
  Shipped in v0.5.0.
- ✅ **Per-iteration `when_iter:` filter on `for_each_dir` /
  `for_each_file` / `every_matching_has`** — shipped in
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
- ✅ **`alint init [--monorepo]` discovery preset** — shipped
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
  on `alint check` is deferred too — `alint init` is the
  primary adoption shape.
- ✅ **Workspace-aware bundled rulesets** — shipped in
  v0.5.3 (2026-04-26). Three thin overlays on
  `monorepo@v1`: `monorepo/cargo-workspace@v1`,
  `monorepo/pnpm-workspace@v1`,
  `monorepo/yarn-workspace@v1`. Each gated by an
  `is_*_workspace` fact (declared inline in the ruleset)
  and uses `when_iter: 'iter.has_file(...)'` to scope
  per-member checks to actual package directories — no
  false positives on stray `crates/notes/` or
  `packages/drafts/`.
- ✅ **Documented scale ceiling** — shipped in v0.5.6
  (2026-04-26) as `xtask bench-scale`. Publishes
  hyperfine timings across (size × scenario × mode)
  matrix with hardware fingerprint; numbers under
  [`docs/benchmarks/v0.5/scale/linux-x86_64/`](../benchmarks/v0.5/scale/linux-x86_64/).
  1M-file size opt-in via `--include-1m`.
- ✅ **Competitive comparisons** — shipped in v0.5.7
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
- ✅ Output formats: `markdown`, `junit`, `gitlab` — shipped in v0.5.8 (2026-04-26). Brings the format count to seven; SARIF / GitHub / `JUnit` / GitLab fall through to the human formatter on `alint fix` since they describe findings, not remediations.
- ✅ `command` plugin kind (v0.5.1, 2026-04-26). Per-file rule wrapping any CLI on `PATH` (`actionlint` / `shellcheck` / `taplo` / `kubeconform` / etc.); exit `0` = pass, non-zero = violation carrying stdout+stderr. Trust-gated: only the user's own top-level config can declare these (mirror of the `custom:` fact gate). Pairs naturally with `--changed` so external checks become incremental in CI.
- ✅ npm shim (`@alint/alint`) — shipped in v0.5.11 (2026-04-27). Closes the install-path gap for JS adopters who don't already have Cargo, Homebrew, or Docker. Wraps a download of the matching pre-built binary at install time; package itself ships zero JS runtime behaviour. Auto-published from `release.yml` alongside crates.io / Docker / Homebrew.
- ✅ Git-aware primitives: `git_no_denied_paths`, `git_commit_message` — both shipped in v0.5.9 (2026-04-27). The first fires on tracked paths matching a glob denylist (secrets / artefacts / "do not commit"); the second validates HEAD's commit message shape via regex / max-subject-length / requires-body. Both no-op silently outside a git repo.
- ✅ `json_schema_passes` primitive — shipped in v0.5.9 (2026-04-27). Validates JSON / YAML / TOML targets against a JSON Schema; reuses the same serde-tree normalisation as `json_path_*`. Schema is loaded + compiled lazily and cached on the rule via `OnceLock`.
- ✅ Remaining bundled rulesets: `compliance/reuse@v1` + `compliance/apache-2@v1` shipped in v0.5.5 (2026-04-26). Both use `file_header` for SPDX / Apache header checks; reuse adds a `dir_exists` on `LICENSES/`; apache-2 adds `file_content_matches` for the LICENSE text + `file_exists` for NOTICE. Both extend without a fact gate — adopting the ruleset signals intent.
- ✅ Additional Scorecard-overlap rules in `oss-baseline@v1` — shipped in v0.5.9 (2026-04-27). Four new rules: SECURITY.md non-empty, Dependency-Update-Tool (Dependabot OR Renovate), CODEOWNERS exists, CODEOWNERS non-empty. Branch-protection state is GitHub-API-only and out of scope; the on-disk piece (CODEOWNERS) is what alint can see. `ci/github-actions@v1` is unchanged — its scope is workflow content, not on-disk artefacts; CODEOWNERS belongs in oss-baseline.

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

## v0.6 — Agent-era bundled rulesets and output

Two bundled rulesets aimed at the most common AI-coding
leftovers, plus a new output format for agents consuming alint
inside their own self-correction loops. All work composes from
existing rule kinds — no engine changes, no new primitives —
matching the same shape as the language-ecosystem rulesets
(`python@v1`, `go@v1`, `ci/github-actions@v1`, …) shipped in
earlier cuts. The agent-driven-development moment makes
alint's existing niche especially valuable; v0.6 ships
ecosystem-specific bundled rulesets to capitalise on that
without changing the underlying tool.

- ⏳ **`alint://bundled/agent-hygiene@v1`** — backup-suffix
  bans (`*.bak`, `*.orig`, `*~`, `*.swp`), versioned-duplicate
  filename guards (`*_v2.ts`, `*_old.py`), scratch-doc bans at
  root (`PLAN.md`, `NOTES.md`, `ANALYSIS.md`, …), `.env`-file
  bans, AI-affirmation regex (`"You're absolutely right"`,
  emoji watermarks), debug-residue bans (`console.log`,
  `debugger`, `breakpoint()`), and model-attributed TODO bans
  (`TODO(claude:)`, `TODO(cursor:)`, …). All composable from
  `file_absent` / `filename_regex` / `file_content_forbidden`.
- ⏳ **`alint://bundled/agent-context@v1`** — hygiene rules
  for `AGENTS.md` / `CLAUDE.md` / `.cursorrules`: existence
  recommended, stub guard via `file_min_lines`, bloat guard
  via `file_max_lines` (per Augment Code research, context
  files >300 lines correlate with worse agent performance),
  stale-path heuristic via regex. Subsumes ctxlint's niche
  with no new rule kinds.
- ⏳ **`--format=agent` JSON output** — sibling of
  `--format=json` shaped for LLM consumption. Each violation
  carries an `agent_instruction` field templated from the
  rule's `message` + `fix` block: a remediation phrasing
  optimised for an agent to act on, not for a human to read.
  Closes the "agents already consume our JSON, but the SARIF
  shape is awkward in their context" feedback gap.

Out-of-scope for v0.6 (deliberately): new rule kinds, semantic
analysis, secret-entropy scanning, AGENTS.md export. All of
those land in v0.7 or later.

## v0.7 — New rule kinds for agentic problems (shipped)

Targeted rule-kind additions that close the gaps Tier-1
exposed. Each got a short design doc before implementation
because heuristic detection has a real false-positive surface
that bundled rulesets don't.

Per-feature design drafts live under
[`docs/design/v0.7/`](./v0.7/) — each settled schema,
semantics, false-positive surface, implementation notes, and
open questions before code started, then was flipped to
`Status: Implemented` on the matching feature commit.

- ✅ **`markdown_paths_resolve` rule kind** (v0.7.1) —
  validates that backticked paths in markdown files resolve
  to real files. Targets the AGENTS.md staleness problem more
  precisely than the v0.6 regex heuristic. Required
  `prefixes:` field eliminates the "is this a path or a
  word" question by construction.
- ✅ **`commented_out_code` rule kind** (v0.7.2) — heuristic
  detector for blocks of commented-out source code, scored on
  punctuation density rather than identifier-token density
  (English prose has identifier-shaped words too). Severity
  floor `warning` — heuristics have non-zero FP rate.
- ✅ **`git_blame_age` rule kind** (v0.7.3) — fire on lines
  matching a regex whose `git blame` author-time exceeds
  `max_age_days`. Closes the gap between `level: warning` on
  every TODO (too noisy) and `level: off` (accepts unbounded
  debt accumulation). Engine plumbing introduces a shared
  `BlameCache` and the `{{ctx.match}}` message placeholder.
- ✅ **`alint suggest` subcommand** (v0.7.4) — scans the
  current repo for known antipatterns and proposes rules
  that would catch them. Three suggester families ship:
  bundled-ruleset (high confidence), antipattern (medium —
  agent-hygiene leftovers), stale-TODO (medium — eats the
  v0.7.3 dogfood). Three output formats (human / yaml /
  json) and a strict stdout-vs-stderr split for slow
  operations: `--progress=auto|always|never` controls
  animated bars on stderr, `-q`/`--quiet` silences both.
- ✅ **`alint export-agents-md` subcommand** (v0.7.5) —
  renders the active rule set as an `AGENTS.md` directive
  block. Inline mode splices between
  `<!-- alint:start -->` / `<!-- alint:end -->` markers;
  re-runs are byte-identical (no mtime bump on round-trip).
  Closes the "67% of teams maintain duplicate configs
  between AGENTS.md and CI lint" gap by making alint the
  single source of truth.

## v0.8 — Performance & test-floor

Revival of the cut that was forward-pointed on the v0.5.11
release (2026-04-27 morning) and then dropped the same
afternoon when v0.6 was re-prioritised for the agent-era
bundled rulesets. With v0.6 / v0.7 shipped, the original
agenda is back on the front burner. Four sub-themes,
ranked by leverage ÷ risk.

### Regression-guard test layer

A CI gate that fails the build when a benchmarked workload
regresses past a threshold. The infrastructure already
exists (criterion micros under `crates/alint-bench/`,
hyperfine macros via `xtask bench-release`, scale matrix
under `docs/benchmarks/v0.5/scale/`); what's missing is
commit-by-commit baseline tracking and the gate itself.

- ⏳ **`xtask bench-regression`** — replays a fixed set
  of bench scenarios against the current commit, compares
  to the baseline stored in-tree (or computed from
  `main`), fails when any scenario regresses past
  `--threshold` (proposed default ±10%).
- ⏳ **CI wiring**: gate `bench-regression` on every PR
  via `ci.yml`; nightly job on `main` refreshes the
  baseline. Probably stages a small set of representative
  scenarios at PR-time and the full matrix nightly to
  keep PR runtime sane.
- ⏳ **Methodology + reproduction doc** so external
  contributors can validate or contest a flagged
  regression. Hardware fingerprinting + warmup-iteration
  conventions documented alongside the gate.
- ⏳ **Backfill the baseline against v0.7.0** — anchors
  "honest now"; everything we improve in v0.8 lands as a
  win against that mark.

### Per-file-rule dispatch flip

Today the engine's outer loop is rules; each rule walks
`ctx.index.files()` and filters by scope. For per-file
rules (the `file_*` family — most of the catalogue), that
re-walks the index N times. Flipping to file-major
dispatch — walk files once, fan rules out per file —
improves cache locality (file content read once, all
matching rules apply) and trims redundant globset work.

- ⏳ **Engine restructure** — dual-loop where per-file
  rules run under a file-major outer loop and cross-file
  rules (`pair`, `for_each_dir`, `every_matching_has`,
  `unique_by`, `dir_contains`, `dir_only_contains`) keep
  the rule-major path. Distinguishable via the existing
  `Rule::requires_full_index()` method.
- ⏳ **File-content read coalescing** — multiple content
  rules over the same file each call `std::fs::read`
  today. Under file-major dispatch the read happens once
  per file and is shared across all matching rules.
- ⏳ **Bench acceptance gate** — numbers must show on the
  `bench-scale` matrix (regression-guard from the previous
  sub-theme catches a regression here for free).

### Parallel walker

`alint-core::walker::walk` is single-threaded. The
`ignore` crate's `WalkBuilder::build_parallel` exists
specifically for trees where syscall bandwidth saturates
ahead of CPU. Switching to it should help on huge trees
(Linux kernel, polyglot monorepos with deep node_modules /
target / venv directories).

- ⏳ Replace `WalkBuilder::build()` with
  `build_parallel()`; collect into the same
  `Vec<FileEntry>` shape via a `Mutex<Vec<_>>` or
  channel.
- ⏳ Sort the collected vec by relative path before
  returning so rule evaluation order stays deterministic
  across runs — output-format snapshot tests depend on
  this.
- ⏳ Document the threading-knob behaviour: rayon-style
  `--jobs N` flag or env var so CI runners with bounded
  CPU don't over-subscribe.

### Memory-footprint pass

Smaller leverage than the dispatch flip but cheap under
the same release.

- ⏳ Audit `Violation` / `RuleResult` / `Report` — many
  fields are owned `String` where `Cow<'_, str>` or
  `&'static str` would do (rule-kind names, severity
  labels, message-template-passthrough cases).
- ⏳ Lazy file-content loading for line-oriented rules
  (`no_trailing_whitespace`, `final_newline`,
  `line_max_width`, `max_consecutive_blank_lines`,
  `indent_style`, …) — `BufReader::lines()` instead of
  `std::fs::read`.
- ⏳ Profile via `dhat` or `valgrind massif` against the
  bench scenarios; publish the report alongside the
  release.

### Out of scope

- **Switching from `regex` to `regex-lite`** — tempting
  for binary size but the catalogue uses features
  (`(?x)`, named captures, lookahead in some rule kinds)
  that `lite` doesn't support.
- **Pre-compiling rule chains via macros** — the
  declarative DSL is the user-facing surface; turning
  rules into static dispatch would lock in the rule
  catalogue.
- **Custom allocators (`mimalloc` / `jemalloc`)** —
  downstream concern. Users can opt in via `RUSTFLAGS`
  if they want; alint stays vanilla.

## v0.9 — LSP

(Was v0.6 in the pre-2026-04-27 roadmap; pushed back three
slots after the agent-era re-prioritisation and the v0.8
perf revival.)

- LSP server (`alint lsp`): inline diagnostics, hover with
  rule documentation, code actions for "add to ignore" and
  "apply fix."
- VS Code extension (bundles the LSP).
- Per-file dispatch shape from v0.8 directly powers the
  per-file-edit re-evaluation hot path.

## v0.10 — WASM plugins

(Was v0.7 in the pre-2026-04-27 roadmap; pushed back three
slots.)

- `wasm` plugin kind with a `wasmtime` host, stable WIT
  interface.
- Plugin registry scaffolding with signature verification.
- Bless a few canonical agent-aware semantic plugins
  (mock-ratio checker, file-similarity / near-dup detector,
  debug-statement auto-stripper) as documented examples — not
  bundled, to keep the binary lean.

## v1.0 — Stability

- DSL schema committed; semver on `version: 1`.
- Plugin ABI committed.
- `alint-core` public API frozen; breaking changes follow semver-major.
- Documentation site.
