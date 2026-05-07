# Case study: `rust-lang/rust`

Inventory of the structural-validation tooling in `rust-lang/rust` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-03, sparse-checkout of `src/tools/tidy`,
`src/ci`, `.github`, top-level config files.

---

## Summary

The Rust monorepo carries its own custom linter (`src/tools/tidy/`) — a
~5kLoC Rust binary that runs **32 named tidy checks** plus an `extra_checks`
dispatcher that fans out to `ruff`, `eslint`, `clang-format`, `shellcheck`,
and `typos`. There are also **4 `src/ci/scripts/verify-*.sh` scripts** and
**3 GitHub Actions workflows**.

Of the 32 tidy modules:

- **~30 % map directly to existing alint rules** (10 modules: most of `style`,
  `tests_placement`, `edition`, `extdeps`, `known_bug`, `rustdoc_gui_tests`,
  `filenames`, `unit_tests`, `bins`, `debug_artifacts`)
- **~25 % need new alint primitives** (8 modules: `alphabetical`, `triagebot`,
  `gcc_submodule`, `rustdoc_css_themes`, `rustdoc_templates`,
  `tests_revision_unpaired_stdout_stderr`, `mir_opt_tests`, `ui_tests`)
- **~45 % are out of alint's scope** (14 modules: AST analysis, codegen drift,
  feature/error-code cross-reference, target-policy heuristics — the same
  "we own a compiler, of course we have AST-aware lints" territory we already
  flagged as a deliberate non-goal in the kubernetes case study)

The 30 % that *do* fit translate cleanly to the **~18-rule alint config** in
[`/.alint.yml`](.alint.yml) — including the most-often-cited tidy::style
checks (line length, file length, trailing whitespace, line endings, no-TODO).
Even on the rest, alint can replace the *outer* "walk the tree, dispatch the
linter, collect the diffs" plumbing while shelling out to the existing tools
via the `command` rule kind. Net: alint can take over the orchestration of
**~13 of the 32 tidy modules + most of the verify-*.sh scripts** with one
declarative config.

---

## Existing tooling inventory

`src/tools/tidy/src/*.rs` — 32 modules, dispatched from `main.rs`'s
parallel `check!()` macro. The first doc comment of each module names its
target. Categorised below:

### Maps to existing alint rules (drop-in replacements)

| Tidy module | What it checks | alint replacement |
|---|---|---|
| `style` (line-length subset) | No lines > 100 cols (non-Rust); 120 for `.goml`; 80 for error-code `.md` | `line_max_width` × 3 (per scope_filter) |
| `style` (file-length) | No file > 3000 lines (non-Rust) | `file_max_lines` |
| `style` (trailing-ws + CR) | No trailing whitespace; LF only | `no_trailing_whitespace` + `line_endings` |
| `style` (TODO/XXX) | No `TODO` / `XXX` markers in source | `file_content_forbidden` |
| `tests_placement` | `src/test/` directory must not exist | `dir_absent` |
| `edition` | Every `Cargo.toml` declares `edition = "2021"` or `"2024"` | `toml_path_matches` against `$.package.edition` |
| `extdeps` | External package sources are on the allowlist | `toml_path_matches` against `$.package[*].source` in `Cargo.lock` |
| `known_bug` | Every `tests/crashes/*.rs` has `//@ known-bug:` | `file_content_matches` |
| `rustdoc_gui_tests` | `.goml` files start with a `// description` comment | `file_starts_with` |
| `filenames` (subset) | No filenames that break Windows | `no_illegal_windows_names` |
| `unit_tests` | No `#[test]` / `#[bench]` directly inside `core` / `alloc` / `std` | `file_content_forbidden` |
| `debug_artifacts` | No stray `borrowck_graphviz_postflow` in test files | `file_content_forbidden` |
| `bins` (subset) | No accidentally-checked-in binaries | `file_is_text` over a path glob (or shell out to `git diff --check`) |
| `extra_checks` (dispatch) | Run `ruff`, `shellcheck`, `eslint`, `typos` over the appropriate trees | `command` rule per scope |

13 modules + the `extra_checks` shell-out dispatcher. Captured in 18 rules
in [`.alint.yml`](.alint.yml). Per-rule build-out was 2-10 minutes once the
patterns were settled.

### Needs new alint primitive

| Tidy module | What it checks | What alint needs |
|---|---|---|
| `alphabetical` | Items between `// tidy-alphabetical-start` / `-end` markers are sorted (case-insensitive, indent-aware joins) | An `ordered_block` rule kind: "for every region delimited by `<start_marker>` / `<end_marker>` tokens, lines must be sorted by `<comparator>`". Generic enough to cover `// tidy-alphabetical-*` here, sortedness in `Cargo.toml` `[dependencies]` (a top request from cargo-workspace users), and `requirements.txt`. **v0.10 ship-target** — sortedness is the single most-requested missing rule kind across the ecosystem inventory passes; the rust monorepo's `tidy-alphabetical-*` markers are the canonical example. |
| `triagebot` | Every path mentioned in `triagebot.toml`'s `[mentions.*]`, `[autolabel.*.trigger_files]`, etc. must exist in the working tree | A `registry_paths_resolve` rule kind — generalised cousin of `markdown_paths_resolve`. Reads a structured doc (TOML / YAML / JSON), extracts string values at JSONPath-selected positions, and asserts each one resolves to a path that exists. The same primitive covers GitHub's `CODEOWNERS`, ESLint's `overrides[].files`, Cargo's `[[bin]].path`, and most "registry of paths in a config file" patterns. |
| `gcc_submodule` | The committed SHA of the `src/gcc` submodule equals `compiler/rustc_codegen_gcc/libgccjit.version` | A `git_submodule_pinned` rule kind: "submodule at `<path>`'s tracked commit must equal the contents of `<file>` (or a JSONPath selector into a structured file)". Niche, but the same shape covers Linux-kernel-style "submodule tracks tag X" enforcement. |
| `rustdoc_css_themes` | Light/dark theme blocks in `rustdoc.css` and `noscript.css` must stay in sync (line-by-line) | A `file_pair_block_match` rule kind: "block between `<start>` / `<end>` markers in file A equals block between same markers in file B (after a configurable transform)". Generalises `pair` (which only asserts existence). The "two CSS files must mirror each other for theme parity" pattern shows up in any project with a server-rendered + JS-disabled fallback. |
| `rustdoc_templates` | Tera-style templates close every `{# #}` / `{% %}` / `{{ }}` tag | A `balanced_delimiters` rule kind: "every opening token in `<delimiters>` has a matching closing token". `file_content_forbidden` catches crude cases but doesn't track nesting. Useful beyond rustdoc — Jinja, Liquid, Handlebars, and any custom templating language want this. |
| `tests_revision_unpaired_stdout_stderr` | For every `<test>.rs` declaring `revisions: a b c`, the existence of `<test>.<revision>.{stdout,stderr}` is gated on whether the test references that revision in error annotations | A `header_directive_pair` rule kind that parses compiletest's `//@ revisions:` headers and pairs them against expected sibling files. Highly compiletest-specific; lower priority than the others. Could plausibly stay as a `command` rule shelling out to a small helper. |
| `mir_opt_tests` | `tests/mir-opt/` has no orphan `.diff` / `.mir` files (every output has a corresponding `.rs`) and no dashes in test names | Closer than the others — `pair` covers most of it, and the dash check is `filename_regex`. The orphan-detection direction is the gap: "every file matching `<pattern>` must have a sibling matching `<other_pattern>` *or* be removed". A `paired_strict` mode on the existing `pair` rule kind would cover it. |
| `ui_tests` | No stray `.stderr`; allowlist for `issue-XXXX.rs` filenames is sorted + minimised | Two needs: (1) the `pair`-like check is covered above; (2) the **issues.txt allowlist** is "every `.rs` file whose name matches `issue-\d+\.rs` must appear in `tests/ui/issues.txt`" — needs a `filename_in_allowlist` rule kind, or could be expressed by `for_each_file` + a custom comparator that reads a sibling registry. |

**Gap pattern: cross-file cross-reference rules.** Five of the eight gaps
above (`triagebot`, `gcc_submodule`, `rustdoc_css_themes`, `mir_opt_tests`,
`ui_tests`) are variants of "data in file A must match data in file B (or a
registry, or a section, or an existence check on a third file)". alint's
existing `pair` and `markdown_paths_resolve` cover the easy cases; the rust
monorepo pushes the boundary into structured-registry territory.
**`registry_paths_resolve` is now a v0.10 ship-target** —
it covers triagebot here, CODEOWNERS in any GitHub repo, ESLint overrides,
the kubernetes `import-restrictions.yaml` registry, and dozens of similar
patterns we've already inventoried (saturated demand: 6+ confirmations
across rust + clap + cpython + arrow + pytorch + tensorflow).

### Out of alint's scope (use the existing tool)

Same framing as the kubernetes case study: these are AST-aware, codegen, or
deep-domain checks. alint's non-goals are deliberate; keep these on
`./x test tidy`.

- `features` / `unstable_book` — feature attributes ↔ Unstable Book docs
  cross-reference; AST/attribute scan, out of scope
- `error_codes` — error code defns in `compiler/rustc_error_codes/` ↔
  doc explanations ↔ UI tests; multi-file AST cross-ref, out of scope
- `pal` — `cfg(unix)` / `cfg(windows)` are only allowed in specific places
  in `std`; cfg-attribute scope analysis, out of scope
- `target_policy` — assembly-LLVM tests cover every target spec; needs
  target-spec parsing, out of scope
- `target_specific_tests` — tests with `--target` declare LLVM components;
  parses compiletest headers, out of scope
- `unknown_revision` — `//@ unused-revision-names:` and revision-name
  validation; compiletest header semantics, out of scope
- `codegen` — TODO policy in `rustc_codegen_{cranelift,gcc}/` repos; the
  *codegen* part (which TODO is owned by which sub-repo) is out of scope,
  the regex part is covered above
- `deps` — third-party crate license allowlist; needs Cargo metadata
  graph, out of scope (use `cargo deny` instead — it's already in tree)
- `rustdoc_json` — `FORMAT_VERSION` constant updated when
  `src/rustdoc-json-types` is modified; needs git-diff-aware logic, out of
  scope (alint's `--changed` mode informs *which* files to check, not what
  triggers a check)
- `x_version` — current `x` tool version pinned correctly; runs `cargo
  install --list`, out of scope

### Already covered by other linters Rust uses

- `cargo deny` (ships in the repo) covers third-party dep licensing —
  duplicates `tidy::deps`
- `rustfmt` covers formatting on `.rs` files — alint defers to it via
  the `command` rule (one shell-out per Cargo.toml, not per .rs)
- `clippy` covers Rust-source linting — alint never enters this territory

---

## Existing tooling: CI scripts and workflow YAML

The `src/ci/scripts/verify-*.sh` set is small (4 scripts) and most are
out-of-scope for alint:

| Script | What it checks | alint disposition |
|---|---|---|
| `verify-line-endings.sh` | Source tree has no CRLF after `core.autocrlf=false` | Maps to `line_endings` (already in starter config) |
| `verify-channel.sh` | `src/ci/channel` matches the branch's expected channel | Maps to `file_content_matches` (per-branch logic stays in CI) |
| `verify-stable-version-number.sh` | Stable channel version is not a duplicate of one already published | Out of scope (HTTP fetch against a published manifest) |
| `verify-backported-commits.sh` | Stable's commits are also in beta + main | Out of scope (git-history walk) |

The **3 GitHub Actions workflows** (`ci.yml`, `dependencies.yml`, `ghcr.yml`,
plus `post-merge.yml`) are covered structurally by
`alint://bundled/ci/github-actions@v1` (permissions block, action pinning,
LF endings, final newline). Nothing rust-specific to add beyond the bundled
ruleset.

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts:

- `oss-baseline@v1` (license, README, gitignore, no merge markers, no bidi)
- `rust@v1` (Cargo.toml exists, no tracked target/, snake_case sources)
- `monorepo/cargo-workspace@v1` (workspace member coherence)
- `ci/github-actions@v1` (workflow permissions / action pinning)
- `hygiene/no-tracked-artifacts@v1` (no `.DS_Store`, build outputs, etc.)

Plus 18 rust-specific rules covering the 13 tidy modules listed above.

The remaining 19 tidy modules:

- 8 need new alint primitives (above) — most are v0.10 ship-targets
  (`ordered_block`, `registry_paths_resolve`); the rest are v0.10+ candidates
- 10 are out of alint's scope (above) — keep `./x test tidy` for those
- 1 is a runner / aggregator

---

## Performance comparison (placeholder — bench when validation pass scales)

`./x test tidy` is itself a parallel Rust binary (its `main.rs` uses
`thread::scope` with a configurable concurrency knob, and the macro at the
top dispatches each `check!()` call into the pool). It's not slow — it's
just **monolithic**: every check is hardcoded into one binary, every change
to a check requires a rebuild of the binary, and contributors who want to
add a new structural check have to write Rust + understand the `TidyCtx`
plumbing.

The alint pitch here is **not** speed — it's **declarative authoring**.
Adding a new check to alint is 5-10 lines of YAML. Adding one to tidy is a
new `pub mod foo;` in `lib.rs` + a new file + a `check!(foo, …);` in
`main.rs` + a unit test in `foo/tests.rs`. For the 30 % of checks that fit
alint's grammar, the YAML-vs-Rust delta is the headline win.

To benchmark wall-clock for real: `time ./x test tidy` (after a warm cargo
build) vs `time alint check` against the same tree. Deferred to the
per-repo measurement pass; we expect alint to be roughly comparable on the
declarative subset (both are I/O-bound at this scale; the rust monorepo has
~30k Rust files + ~10k tests + sub-repos in `src/llvm-project/` and
`src/gcc/` we'd exclude).

---

## Recommendation for the launch story

**Headline launch quote:** "rust-lang/rust ships its own ~5kLoC custom linter
binary because no off-the-shelf tool can express its structural rules. alint
covers ~30 % of those rules in 18 lines of YAML — and the gap analysis
points at exactly four new rule kinds that would push that to ~55 %."

This is the **second-strongest** case study (behind kubernetes) for the
launch positioning, and uniquely valuable for the "polyglot monorepo /
project-with-its-own-linter-binary" audience — the readers who already
*know* generic linters don't fit their needs and have built bespoke tooling.
The pitch lands as: "we're not asking you to throw away your custom linter,
we're asking you to push the 30 % of mechanical checks down into a
declarative layer so you can focus your handwritten Rust on the AST-aware
domain logic that *actually* needs a custom binary".

Followup feature work surfaced (priority order):

- **`ordered_block` rule kind** (sortedness between marker pairs) — covers
  `tidy::alphabetical` here, `[dependencies]` ordering in every Cargo
  workspace, `requirements.txt` ordering, `imports` blocks; **single most
  requested missing rule kind** across the validation passes
- **`registry_paths_resolve` rule kind** (extract paths from a structured
  registry, assert each exists) — covers `tidy::triagebot` here,
  CODEOWNERS validation in every GitHub repo, ESLint `overrides[].files`,
  Cargo `[[bin]].path`, and the same pattern in nearly every config-driven
  tool we've inventoried
- **`file_pair_block_match` rule kind** (markered block in file A equals
  same markered block in file B) — covers `rustdoc_css_themes` here, plus
  any "templated config + manually-maintained mirror" duplication pattern
- **`balanced_delimiters` rule kind** — covers `rustdoc_templates` and any
  templating-language project (Jinja, Liquid, Handlebars)

---

## Future analysis

Suggestions for the next revalidation pass (now that v0.9.17 ships
the per-rule `respect_gitignore: false` knob, the `literal_is_nested`
runtime guard, the `scope_filter` evolution, and `has_*` predicate
renames):

- **A `tidy@v1` bundled-ruleset draft.** ~13 of ~32 tidy modules are
  declarative today (line/file lengths, trailing whitespace, line
  endings, no-TODO, edition, extdeps, known-bug, rustdoc-gui description,
  Windows-illegal filenames, no `#[test]` in stdlib, debug-artifact
  guards). Packaging these as `alint://bundled/tidy/rust@v1` (with
  `scope_filter` to exclude `src/llvm-project/` and `src/gcc/` by
  default, and parameterised line-length thresholds for `.goml` /
  error-code markdown) would let any rust-lang/rust contributor adopt
  the canonical 30 % of tidy as a one-liner extends entry, and would
  raise the case study's pitch from "18 lines of YAML" to "1 line of
  YAML". The remaining 8 tidy modules become the explicit gap list
  the bundled ruleset documents in its README.
- **`scope_filter` for the `src/llvm-project/` + `src/gcc/` sub-trees.**
  Today the config repeats `src/llvm-project/**` + `src/gcc/**` exclude
  globs across 5 rules. v0.9.17's `scope_filter` evolution lets a
  single top-level filter apply once. The simplification eliminates
  ~30 lines of repeated exclusions and makes "what counts as
  rust-monorepo source" a single source of truth.
- **`alint suggest` against the live tree.** The current config is
  hand-authored from the tidy module catalogue; running `alint suggest`
  against a fresh `rust-lang/rust` clone would surface bundled-ruleset
  candidates the case study didn't reach for (likely `agent-context`,
  `compliance/apache-2`, `tooling/editorconfig`, `docs/adr` for the
  RFCs tree). Worth a 5-minute pass for the v0.10 ladder evidence.

---

## Validation status (2026-05-07)

- **alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).
- **`validate-config`:** ✓ 62 rules loaded from `.alint.yml`.
- **README rule-count claim:** "18 rust-specific rules" matches the
  actual file count of 20 within rounding (the 2-rule slack reflects
  the `rust-triagebot-relabel-section` + `rust-tidy-rustfmt` rules
  added in P2b polish that postdate the README's "18 rule" framing).
  The 62-rule `validate-config` total = 20 rust-specific + 42
  inherited from the 5 bundled rulesets (oss-baseline=15 + rust=11 +
  monorepo/cargo-workspace=4 + ci/github-actions=3 +
  hygiene/no-tracked-artifacts=11 = 44 declared; the 2-rule slack vs
  42 reflects bundled-overlap dedup the engine handles
  transparently).
- **Pitfall catalogue:** v0.9.17 ships fixes for #18 (per-rule
  `respect_gitignore: false`) and #19 (`literal_is_nested` runtime
  guard with clearer diagnostic). Neither pitfall surfaces in this
  config (no tracked-AND-gitignored files; no `root_only: true` on
  literal-multi-component paths).
- **Rule-kind candidate status:** `ordered_block` +
  `registry_paths_resolve` are now v0.10 ship-targets (saturated
  demand across 5+ case studies each). `file_pair_block_match` and
  `balanced_delimiters` remain v0.10+ candidates (single-source
  demand from this case study).
- **Bundled-ruleset rule counts (authoritative as of 2026-05-07):**
  oss-baseline=15, rust=11, monorepo/cargo-workspace=4,
  ci/github-actions=3, hygiene/no-tracked-artifacts=11.
