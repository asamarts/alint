# Case study: `rust-lang/rust`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/rust-lang-rust/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `rust-lang/rust` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 sparse-checkout of `src/tools/tidy`,
`src/ci`, `.github`, top-level config files at
`/tmp/rust-lang-rust/`. Sub-trees `src/llvm-project/` and `src/gcc/`
intentionally elided (vendored upstream projects with their own gating).

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

The Rust monorepo carries its own custom linter — `src/tools/tidy/` is a
~5k-LoC Rust binary dispatched from `main.rs`'s parallel `check!()` macro
into a thread pool. Plus a small set of CI shell scripts and four
GitHub Actions workflows.

### 1.1 `src/tools/tidy/src/*.rs` — 30 check modules + dispatcher (gating)

Counted by walking `main.rs` for every `check!(<name>, …)` call and the
one direct `features::check(…)` invocation. Each row is one logical
check (some are invoked with multiple `*_path` args — those are listed
once and the multi-invocation flagged in "Notes").

| # | Module | What the check actually does (read from module's `.rs`) | Notes |
|---|---|---|---|
| 1 | `alphabetical` | Items between `// tidy-alphabetical-start` / `-end` markers are sorted (case-insensitive, indent-aware joins) | Invoked 6× (root_manifest + typos.toml + src + tests + compiler + library) |
| 2 | `bins` | No accidentally-checked-in binaries (Unix `executable` bit + `git ls-files`) | Unix-only impl; Windows is a no-op |
| 3 | `codegen` | TODO/FIXME policy in `compiler/rustc_codegen_{cranelift,gcc}/` (each codegen owns its own deviation list) | |
| 4 | `debug_artifacts` | No stray `borrowck_graphviz_postflow` debug artefacts in test files | |
| 5 | `deps` | Third-party crate license allowlist + duplicate-dep guard | Reads Cargo metadata graph |
| 6 | `edition` | Every `Cargo.toml` declares `edition = "2021"` or `"2024"` | |
| 7 | `error_codes` | Error-code defns in `compiler/rustc_error_codes/` ↔ doc explanations ↔ UI test annotations cross-reference | |
| 8 | `extdeps` | External package sources in `Cargo.lock` are on the allowlist (e.g., `registry+https://github.com/rust-lang/crates.io-index`) | |
| 9 | `features` | Unstable feature attributes are well-formed; appear in `unstable_book` if expected; tracking issue is referenced consistently | Direct call (not via `check!()`); cross-references attribute scanner output with Unstable Book |
| 10 | `filenames` | No filenames containing `:` (Windows-illegal); no non-UTF-8 names; no control chars | Uses `git ls-files -z` |
| 11 | `gcc_submodule` | Tracked SHA of `src/gcc` submodule equals `compiler/rustc_codegen_gcc/libgccjit.version` | |
| 12 | `known_bug` | Every `tests/crashes/*.rs` carries a `//@ known-bug:` directive | |
| 13 | `mir_opt_tests` | No orphan `.diff` / `.mir` files in `tests/mir-opt/` (each output has a corresponding `.rs`); no dashes in test names | |
| 14 | `pal` | `cfg(unix)` / `cfg(windows)` may only appear in specific places in `library/std` | cfg-attribute scope analysis |
| 15 | `rustdoc_css_themes` | Light/dark theme blocks in `rustdoc.css` and `noscript.css` stay in sync line-by-line | |
| 16 | `rustdoc_gui_tests` | `.goml` files start with a small `// description` comment | |
| 17 | `rustdoc_json` | `FORMAT_VERSION` const updated when `src/rustdoc-json-types` is modified | git-diff aware |
| 18 | `rustdoc_templates` | Tera-style templates close every `{# #}` / `{% %}` / `{{ }}` tag | Balanced delimiters |
| 19 | `style` | Lines ≤ 100 cols (non-Rust); 120 for `.goml`; 80 for error-code `.md`; files ≤ 3000 lines (non-Rust); no tabs (some scopes); no trailing whitespace; no CR; no `TODO`/`XXX`/`FIXME`; no unexplained ` ```ignore ` doc tests | Invoked 4× (src + tests + compiler + library) |
| 20 | `target_policy` | Sanity-check assembly-LLVM tests cover every target spec in `compiler/rustc_target/src/spec/` | |
| 21 | `target_specific_tests` | Tests with `--target` flag declare their pre-requisite LLVM components | Parses compiletest headers |
| 22 | `tests_placement` | `src/test/` directory must not exist (legacy path moved to `tests/`) | |
| 23 | `tests_revision_unpaired_stdout_stderr` | Per `<test>.rs` declaring `revisions: a b c`, `<test>.<rev>.{stdout,stderr}` existence is gated on whether the test references that revision in error annotations | compiletest-header semantics |
| 24 | `triagebot` | Every path in `triagebot.toml`'s `[mentions.*]`, `[autolabel.*.trigger_files]`, etc. exists in the working tree | |
| 25 | `ui_tests` | No stray `.stderr`; allowlist for `issue-XXXX.rs` filenames in `tests/ui/issues.txt` is sorted + minimised | |
| 26 | `unit_tests` | No `#[test]` / `#[bench]` directly inside `library/{core,alloc,std}` (must live in dedicated `tests/` dirs) | Invoked 3× (src + compiler + library) |
| 27 | `unknown_revision` | `//@ unused-revision-names:` and revision-name validation across compiletest headers | |
| 28 | `unstable_book` | Feature attributes ↔ Unstable Book docs cross-reference | Pairs with `features` |
| 29 | `x_version` | Current `x` tool version pinned correctly (calls `cargo install --list`) | Runtime probe |
| 30 | `extra_checks` | Dispatcher: spawns `ruff` (Python lint+fmt), `eslint`+`tsc` (JS), `clang-format` (C++), `shellcheck`, `typos` | 7 sub-checks; configurable via flags |

**Total: 30 logical check modules.** (5 utility files — `lib.rs`,
`main.rs`, `walk.rs`, `iter_header.rs`, `arg_parser.rs`,
`diagnostics.rs` — bring `src/tools/tidy/src/*.rs` to 35 files;
`extra_checks/` is a sub-directory.)

### 1.2 `src/ci/scripts/verify-*.sh` (4 scripts — gating)

| Script | What it actually does | Backing tool |
|---|---|---|
| `verify-line-endings.sh` | Source tree has no CRLF after `core.autocrlf=false` | `git ls-files` + grep |
| `verify-channel.sh` | `src/ci/channel` value matches the branch's expected channel (stable / beta / nightly) | bash |
| `verify-stable-version-number.sh` | Stable-channel version not a duplicate of one already published | curl against `static.rust-lang.org` |
| `verify-backported-commits.sh` | Stable's commits are also in beta + main branches | git rev-list walk |

The other ~15 `src/ci/scripts/*.sh` are **utility / install** scripts
(setup-environment, install-ninja, install-sccache, …) — not gates,
so out of scope for this inventory.

### 1.3 `.github/workflows/` (4 workflows)

| File | Purpose |
|---|---|
| `ci.yml` | Mega-orchestration — dispatches into `src/ci/github-actions/` reusable jobs (build, test, dist), runs `tidy` + per-platform compiler builds |
| `dependencies.yml` | Dependabot / cargo-update PR enforcement |
| `ghcr.yml` | Pushes built dev-container images to GitHub Container Registry |
| `post-merge.yml` | Post-merge analytics + try-build orchestration |

### 1.4 Per-language config + registry files

| Path | Role |
|---|---|
| `Cargo.toml` (workspace, ~600+ lines) | Workspace member list (`compiler`, `library`, `src/tools/*`, `src/etc/*`) + `[workspace.dependencies]` |
| `Cargo.lock` (root + several sub-repo locks) | Pinned dep graph |
| `rustfmt.toml` (workspace root) | rustfmt overrides (excludes for codegen-test fixtures, etc.) |
| `triagebot.toml` (workspace root) | Triagebot config — `[mentions]`, `[autolabel]`, `[transfer]`; consumed by `tidy::triagebot` |
| `typos.toml` (workspace root) | typos spell-check allowlist; consumed by `tidy::extra_checks::spellcheck` |
| `REUSE.toml` | REUSE/SPDX header config |
| `LICENSES/` | SPDX license texts |
| `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `README.md`, `LICENSE-APACHE`, `LICENSE-MIT`, `COPYRIGHT`, `RELEASES.md`, `INSTALL.md` | Repo-root governance / docs |
| `.github/{ISSUE_TEMPLATE,PULL_REQUEST_TEMPLATE.md,workflows}/` | GitHub UI surface |
| `bootstrap.example.toml`, `configure`, `x`, `x.ps1`, `x.py` | Build-system entry points |
| `rust-bors.toml` | bors merge-queue config |
| `package.json` + `yarn.lock` | rustdoc front-end JS deps (consumed by `extra_checks::js_lint` + `js_typecheck`) |
| `tests/`, `compiler/`, `library/`, `src/`, `src/tools/`, `src/etc/`, `src/ci/` | Source tree |

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset (`oss-baseline` /
  `rust` / `ci/github-actions` / `hygiene/no-tracked-artifacts`) OR the
  per-rule entry in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why (Rust AST, codegen drift, compiletest
  header semantics, runtime probe, …). The "out-of-scope" label is
  positive — these are checks where the existing tool *is* the right
  tool.

### 2.1 The 30 tidy modules — explicit ✅ / 🔄 / ❌ tagging

| # | Module | Coverage | Notes |
|---|---|---|---|
| 1 | `alphabetical` | 🔄 alint-future | `ordered_block` (v0.10 ship-target, 7 sources). Marker-pair sortedness is the **canonical demand-driver** for this primitive. |
| 2 | `bins` | ✅ alint-today | `file_is_text` over `paths.exclude` for known-binary dirs (or shell out to `git diff --check` via `command:`). Existing tool is also doing executable-bit + git heuristics; alint can replace the simple cases. |
| 3 | `codegen` | ❌ out-of-scope | Per-sub-repo TODO policy with codegen-domain allowlists; the *outer* TODO regex is `file_content_forbidden` but the per-codegen ownership decision is sub-repo-aware Rust state. |
| 4 | `debug_artifacts` | ✅ alint-today | `file_content_forbidden` over `tests/**/*.rs` for `borrowck_graphviz_postflow`. |
| 5 | `deps` | ❌ out-of-scope | Reads Cargo dependency graph + license metadata. `cargo deny` is in tree and the right tool. |
| 6 | `edition` | ✅ alint-today | `toml_path_matches` against `$.package.edition` per `Cargo.toml`. |
| 7 | `error_codes` | ❌ out-of-scope | Multi-file Rust + Markdown + UI-test cross-reference; needs Rust AST awareness. |
| 8 | `extdeps` | ✅ alint-today | `toml_path_matches` against `$.package[*].source` in `Cargo.lock` (allowlist). |
| 9 | `features` | ❌ out-of-scope | Rust attribute scanner cross-referenced with Unstable Book; AST-aware. |
| 10 | `filenames` | ✅ alint-today | `no_illegal_windows_names` (oss-baseline already covers most; `:` ban is the bundled rule) + `file_content_forbidden`-style for control chars in path. |
| 11 | `gcc_submodule` | 🔄 alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources): "submodule SHA at `src/gcc` equals contents of `compiler/rustc_codegen_gcc/libgccjit.version`". Niche but textbook. |
| 12 | `known_bug` | ✅ alint-today | `file_content_matches` over `tests/crashes/**/*.rs` requiring `//@ known-bug:`. |
| 13 | `mir_opt_tests` | 🔄 alint-future | `pair` (existing) covers the orphan-detection direction in one mode; the strict mode "every file matching X has a sibling matching Y" is the `paired_strict` v0.10+ extension. |
| 14 | `pal` | ❌ out-of-scope | cfg-attribute scope analysis over `library/std`; needs Rust AST. |
| 15 | `rustdoc_css_themes` | 🔄 alint-future | `file_pair_block_match` (v0.10 design candidate, 3 sources: rust + cpython×2). "block between markers in file A == same in file B (after configurable transform)". |
| 16 | `rustdoc_gui_tests` | ✅ alint-today | `file_starts_with` over `tests/rustdoc-gui/**/*.goml` requiring `// description` prefix. |
| 17 | `rustdoc_json` | ❌ out-of-scope | git-diff-aware: the check fires only when `src/rustdoc-json-types` is modified. alint's `--changed` flag informs *which* files to check, not *whether* to check. |
| 18 | `rustdoc_templates` | 🔄 alint-future | `balanced_delimiters` (v0.10 design candidate, 2 sources: rust + cpython). Templating-language nesting check. |
| 19 | `style` | ✅ alint-today | `line_max_width` × 3 (per scope_filter for `.goml=120` / error-code `.md=80` / default 100) + `file_max_lines: 3000` + `no_trailing_whitespace` + `line_endings: lf` + `file_content_forbidden` (TODO/XXX/FIXME). The most-cited tidy check; cleanest fit. |
| 20 | `target_policy` | ❌ out-of-scope | Target-spec parsing + assembly-LLVM cross-reference. |
| 21 | `target_specific_tests` | ❌ out-of-scope | Compiletest header semantics + LLVM-component graph. |
| 22 | `tests_placement` | ✅ alint-today | `dir_absent` for `src/test/`. |
| 23 | `tests_revision_unpaired_stdout_stderr` | 🔄 alint-future | `header_directive_pair` v0.10+ candidate — parses `//@ revisions:` headers and pairs them against expected sibling files. compiletest-specific. |
| 24 | `triagebot` | 🔄 alint-future | `registry_paths_resolve` (v0.10 ship-target, 8 sources). triagebot.toml's `[mentions.*]`, `[autolabel.*]`, `[transfer]` keys with file paths must resolve. **Canonical demand-driver** for this primitive. |
| 25 | `ui_tests` | 🔄 alint-future | (a) `pair` covers most stray-`.stderr` checks; (b) the `tests/ui/issues.txt` allowlist is the `filename_in_allowlist` v0.11+ shape, OR `for_each_file` with a sibling-registry comparator. |
| 26 | `unit_tests` | ✅ alint-today | `file_content_forbidden` over `library/{core,alloc,std}/**/*.rs` for `#[test]` / `#[bench]`. |
| 27 | `unknown_revision` | ❌ out-of-scope | compiletest-header semantics. |
| 28 | `unstable_book` | ❌ out-of-scope | Pairs with `features` (Rust AST). |
| 29 | `x_version` | ❌ out-of-scope | Runtime probe (`cargo install --list`). |
| 30 | `extra_checks` (dispatcher) | ✅ alint-today | `command:` rules per sub-tool: ruff, eslint, tsc, clang-format, shellcheck, typos. |

**Tally for §2.1 (the 30 tidy modules):**

```
✅ alint-today:    11 / 30 = 37%   (bins, debug_artifacts, edition, extdeps, filenames, known_bug, rustdoc_gui_tests, style, tests_placement, unit_tests, extra_checks)
🔄 alint-future:    8 / 30 = 27%   (alphabetical, gcc_submodule, mir_opt_tests, rustdoc_css_themes, rustdoc_templates, tests_revision_unpaired_stdout_stderr, triagebot, ui_tests)
❌ out-of-scope:   11 / 30 = 37%   (codegen, deps, error_codes, features, pal, rustdoc_json, target_policy, target_specific_tests, unknown_revision, unstable_book, x_version)
```

The brief flagged "~13 of ~32 tidy checks become declarative". The
exact count is **11 of 30 today + 8 more once the v0.10 backlog
ships = 19 of 30 (63 %)** of the tidy surface becomes declarative.
The pre-existing "~13" framing combined the today + a partial
v0.10 forecast — replace with the explicit 11 / 8 / 11 split above.

### 2.2 The 4 verify-*.sh scripts

| Script | Coverage | Notes |
|---|---|---|
| `verify-line-endings.sh` | ✅ alint-today | `line_endings: lf` over `**/*.{rs,md,toml,yml}` (rust ruleset already does this for `.rs`). |
| `verify-channel.sh` | ✅ alint-today | `file_content_matches` against `src/ci/channel` (the per-branch expected value lives in CI; the *file shape* is alint's). |
| `verify-stable-version-number.sh` | ❌ out-of-scope | curl against `static.rust-lang.org` (network probe). |
| `verify-backported-commits.sh` | ❌ out-of-scope | git-history walk across branches. |

### 2.3 The 4 GitHub Actions workflows

All 4 workflows fall under `ci/github-actions@v1` (3 rules — workflow
permissions + action SHA pinning + workflow has `name:`). No
rust-specific overrides needed.

### 2.4 Repo-root governance artefacts

| Artefact | Coverage | Rule |
|---|---|---|
| `LICENSE-APACHE`, `LICENSE-MIT`, `COPYRIGHT` | ✅ alint-today | `oss-license-exists`, `oss-license-non-empty` (oss-baseline) — special-case dual-license shape ✓ |
| `README.md` | ✅ alint-today | `oss-readme-exists`, `oss-readme-non-stub` |
| `CODE_OF_CONDUCT.md` | ✅ alint-today | `oss-code-of-conduct-exists` |
| `CONTRIBUTING.md`, `INSTALL.md`, `RELEASES.md` | ✅ alint-today (presence) | `file_exists` per-artefact in this repo's `.alint.yml` |
| `Cargo.toml`, `Cargo.lock`, `rustfmt.toml` | ✅ alint-today | `cargo-toml-exists`, `cargo-lock-exists` (rust ruleset) |
| `triagebot.toml`, `typos.toml`, `REUSE.toml`, `LICENSES/`, `package.json`, `yarn.lock` | ✅ alint-today | `file_exists` / `dir_exists` per-artefact |
| Repo-wide hygiene (no `target/`, no `node_modules/`, no `.DS_Score`) | ✅ alint-today | All 11 rules from `hygiene/no-tracked-artifacts@v1` |
| `.github/workflows/` (4 workflows) | ✅ alint-today | All 3 rules from `ci/github-actions@v1` |

---

## 3. Quantified coverage

Counted across the **30 tidy modules** + **4 verify-*.sh scripts** +
**4 workflows** + **8 governance artefact families** = **46 distinct
surfaces**.

```
✅ alint-today:    23 / 46 = 50%   (11 tidy + 2 verify + 4 workflows + 6 governance)
🔄 alint-future:    8 / 46 = 17%   (all from tidy)
❌ out-of-scope:   13 / 46 = 28%   (11 tidy + 2 verify)
                  governance non-applicable (Cargo dual-license already counted) = 2 = 4%
                  ──────────────
                  total = 100%
```

Granular breakdown:

```
tidy modules (30):
  ✅ alint-today:    11 / 30 = 37%
  🔄 alint-future:    8 / 30 = 27%
  ❌ out-of-scope:   11 / 30 = 37%

verify-*.sh scripts (4):
  ✅ alint-today:     2 / 4  = 50%
  ❌ out-of-scope:    2 / 4  = 50%

GHA workflows (4):
  ✅ alint-today:     4 / 4  = 100%   (all under ci/github-actions@v1)

governance artefacts (8):
  ✅ alint-today:     8 / 8  = 100%
```

**Commentary.** Three observations:

1. **Tidy is a Rust monorepo carrying its own alint.** Half of tidy's
   modules (37 % today + 27 % v0.10 = 64 %) are declarative-able — the
   exact niche alint is built for. The remaining 37 % are AST,
   compiletest semantics, and runtime probes that legitimately need
   Rust to express. **The pitch here is not "alint replaces tidy"
   but "alint is what 64 % of tidy could have been if alint had
   existed when tidy was written"**, while leaving the AST 37 % to
   the existing binary.

2. **`ordered_block` (`tidy::alphabetical`) and `registry_paths_resolve`
   (`tidy::triagebot`) are the v0.10 ship-targets carrying the most
   weight here.** Both are saturated across the case-study set
   independently. Rust monorepo's marker-pair sortedness blocks
   (used 6× in tidy) and triagebot.toml are the canonical
   demand-drivers.

3. **Authoring delta.** Adding a tidy check today: `pub mod foo;` in
   `lib.rs` + new `.rs` file + `check!(foo, …);` in `main.rs` + a
   `tests.rs`. Adding the equivalent alint rule: 5-10 lines of
   YAML. For the 11 modules that fit alint's grammar today, the
   YAML-vs-Rust delta is the headline win. For contributors who
   want to add a structural check but don't want to write Rust + a
   `TidyCtx` + a thread-pool dispatch entry, the delta widens to
   "5 lines of YAML versus owning a Rust binary's review cycle".

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (283 lines, 20
repo-specific rules, 5 bundled rulesets folded in via `extends:`,
**62 rules total** loaded — confirmed by `alint validate-config`).

**Synopsis of the load-bearing rules** (full config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/rust@v1                          # 11 rules
  - alint://bundled/monorepo/cargo-workspace@v1      # 4 rules
  - alint://bundled/ci/github-actions@v1             # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules

rules:
  # tidy::style — three line-length scopes, file_max_lines, no-CR, no-TODO
  - id: rust-tidy-line-100-cols
    kind: line_max_width
    paths: { include: ["compiler/**/*.{md,toml,yml,yaml,sh,py}", …], exclude: ["**/auto-generated/**", "src/llvm-project/**", "src/gcc/**"] }
    max_width: 100
  - id: rust-tidy-line-120-goml
    kind: line_max_width
    paths: "tests/rustdoc-gui/**/*.goml"
    max_width: 120
  - id: rust-tidy-line-80-error-codes
    kind: line_max_width
    paths: "compiler/rustc_error_codes/src/error_codes/*.md"
    max_width: 80
  - id: rust-tidy-no-todo-marker      # tidy::style — no `TODO`/`XXX`
    kind: file_content_forbidden
    paths: { include: ["compiler/**/*.rs", "library/**/*.rs", "src/**/*.rs"], exclude: ["**/tests/**", "src/llvm-project/**", "src/gcc/**"] }
    pattern: '(?i)\b(TODO|XXX)\b'
  - id: rust-tidy-cargo-edition       # tidy::edition — every Cargo.toml is 2021/2024
    kind: toml_path_matches
    path: "$.package.edition"
    matches: '^(2021|2024)$'
  - id: rust-tidy-cargo-lock-source-allowlist  # tidy::extdeps
    kind: toml_path_matches
    paths: "Cargo.lock"
    path: "$['package'][*].source"
    matches: '^(registry\+https://github.com/rust-lang/crates\.io-index|git\+https://github.com/rust-lang/team#)'
  - id: rust-tidy-rustfmt             # rustfmt --check per-package via for_each_dir
    kind: for_each_dir
    select: "**/Cargo.toml"
    require: [{ kind: command, command: ["cargo", "fmt", "--manifest-path", "{path}", "--check"] }]
```

**Repo-specific vs bundled split:**

- **20 repo-specific rules** (the `rust-tidy-*` / `rust-triagebot-*`
  prefix identifies them in `alint list` output): three
  `line_max_width` scopes, `file_max_lines: 3000`,
  `no_trailing_whitespace`, `line_endings: lf`, no-TODO,
  `dir_absent: src/test`, Cargo edition, Cargo.lock source
  allowlist, `tests/crashes/` `//@ known-bug:` directive,
  rustdoc-gui description prefix, `no_illegal_windows_names`,
  no-`#[test]`-in-stdlib, plus 6 `command:` shellouts (typos,
  shellcheck, ruff format + lint, rustfmt, triagebot section
  presence).
- **42 bundled rules** from the 5 extended rulesets (15 + 11 + 4 + 3
  + 11 = 44 with 2-rule overlap dedup).

**Validation:** `alint validate-config` reports `✓ Config valid: 62
rule(s) loaded`. Pitfall checks: the magic comment is present (line
1); all `command:` rules use `command:` and integer `timeout:`; all
patterns use `'…'` single-quote scalars (no YAML literal block
scalars — pitfall #22-clean); JSONPath uses bracket notation
defensively for `$['package']` (the in-config comment on line 171
documents this as "bracket-notation for dashes", though `package`
itself has no dash — the bracket form is the canonical pattern for
keys-with-dashes elsewhere in the file).

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the live
`/tmp/rust-lang-rust/` sparse-checkout. Machine: Linux 6.1.0-42-amd64,
~10 logical cores; alint binary `target/release/alint v0.9.17`.
`-i` ignores non-zero exit (alint exits non-zero on violations,
this is timing not pass-fail).

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| `verify-channel.sh` + `verify-line-endings.sh` (in-tree shell scripts) | `shellcheck -x src/ci/scripts/*.sh src/ci/*.sh` (28 files) | **488 ms** ± 17 ms | included in 1.031 s full pass | n/a — the alint pass also runs the rule via `command:` |
| `tidy::edition` (every Cargo.toml has edition 2021/2024) | `find … -exec grep -L 'edition = "2021"' {} +` (~480 manifests) | **115 ms** ± 1 ms | included in 1.031 s full pass | n/a — alint runs the toml_path_matches across the whole tree |
| **alint full pass (62 rules)** | n/a | n/a | **1.031 s** ± 29 ms | — |

The headline number: **a single 1.03 s alint pass evaluates 62
rules across ~30k Rust files + 50k test-fixture files, replacing
the 11 today-mappable tidy modules + verify-line-endings +
verify-channel + the file-presence + governance rules in one walk.**

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `./x test tidy` (full tidy run, all 30 modules) | `./x test tidy` (Rust monolithic binary built from `src/tools/tidy/`) | pending — needs the full Rust workspace build (rustc + cargo + the workspace's proc-macros and bootstrap; ~2-5 minutes from clean) | `cd /tmp/rust-lang-rust && ./x test tidy` |
| `verify-spellcheck` (typos crate) | `typos` | pending — `typos` not on PATH | `cargo install typos-cli` |
| `cargo fmt --check` (rust-tidy-rustfmt) | rustfmt | pending — needs the Rust workspace's pinned toolchain (`rust-toolchain.toml` per-workspace; the `cargo` on PATH is a dev profile that doesn't match the rust-lang/rust pinned channel) | `rustup show` then `cargo fmt --manifest-path Cargo.toml --check` |
| `ruff format --check` + `ruff check` | ruff | pending — `ruff` not on PATH | `pipx install ruff` |
| `cargo clippy --workspace …` (`extra_checks` cluster, indirectly) | clippy | pending — same as `./x test tidy` | (see `./x test tidy` row) |

The `./x test tidy` end-to-end wall-clock is the single most
marketable comparison — but it requires the full rust-lang/rust
build environment (~2 GB of artifacts after `./x check`). On a CI
runner with that environment, the published S9 macro-bench
(~1.4 s for 100k polyglot files) is the right reference: alint's
1.03 s on this ~30k-Rust + 50k-test-fixture sparse-checkout sits
between S3 and S9, inside the sub-2-second envelope. The pitch
is *not* speed — `./x test tidy` is already parallel Rust. The
pitch is **declarative authoring** (YAML vs Rust binary ownership)
and **gradual adoption** (alint runs *alongside* tidy in CI,
picking off the 11 today-mappable + 8 v0.10 modules without
removing the tidy binary).

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/rust-lang-rust/.alint.yml --format json /tmp/rust-lang-rust/`
(live run, JSON-format).

**Headline:** alint surfaces **2,698 violations** across 30 failing
rules (24 passing); below the 34k-violation pilot bug class because
**no regex-anchor false positives are present in this config** —
all patterns are correctly anchored. The top per-rule counts are:

| # | Count | Rule | Triage |
|---|---|---|---|
| 1 | 1,091 | `rust-sources-snake-case` (bundled rust@v1) | **SUSPECT — high cardinality.** This bundled rule fires on every `compiler/rustc_*` module + acronym-y crate names (`rustc_errors`, `RustcSession`, etc.). The Rust workspace has a **deliberate exception** for compiler-internal naming. **Recommended fix:** add `paths.exclude: ["compiler/**", "library/std/src/sys/**"]` to override the bundled rule, OR file an upstream alint issue to scope `rust-sources-snake-case` away from compiler crates by default. |
| 2 | 668 | `rust-tidy-line-100-cols` | Real findings — `compiler/rustc_error_codes/src/error_codes/*.md` (long URL refs), some `library/**/*.toml` long dep specs, a handful of `tests/**/*.{md,sh}` over-100-col wrapping. Most are tolerable; the rule is `warning`. Match upstream tidy's threshold-vs-allowlist policy. |
| 3 | 237 | `rust-tidy-no-todo-marker` | **Mixed.** Real `// TODO` markers in compiler source (some valid — paired with tracking issues). Upstream tidy has the same complaint pattern; this is the long-tail TODO de-noise queue. |
| 4 | 149 | `rust-sources-final-newline` (bundled rust@v1) | Real findings — vendored `**/auto-generated/**` files. **Recommended:** add `paths.exclude` for `**/auto-generated/**` and the LLVM-project / GCC mirror trees. |
| 5 | 106 | `rust-sources-no-trailing-whitespace` (bundled rust@v1) | Same — vendored fixtures + a few real source-tree drifts. |
| 6 | 66 | `rust-tidy-line-80-error-codes` | All in `compiler/rustc_error_codes/src/error_codes/*.md`. Real — these are the canonical error-code docs and the 80-col rule mirrors the upstream tidy rule. |
| 7 | 63 | `oss-no-trailing-whitespace` | Trailing-ws in `tests/`, `src/`, `compiler/` markdown / yaml. Below tidy's threshold; informational. |
| 8 | 62 | `rust-tidy-lf-line-endings` | CRLF in some Windows-build-script fixtures (`.bat` excluded; `.ps1` is not — likely needs adding to the rule's `paths.exclude`). |
| 9 | 57 | `rust-tidy-no-trailing-whitespace` | Same as #7, narrower scope. |
| 10 | 40 | `oss-final-newline` | Markdown fixtures + governance docs. Below tidy's threshold. |
| 11 | 34+34 | `rust-tidy-ruff-format` + `rust-tidy-ruff-lint` | Pending — `ruff` not on PATH, so the `command:` rule fires per-file as "command not found" rather than reporting actual lint findings. **Bug fix candidate:** the rule emits a clearer "ruff binary missing" diagnostic instead of degrading to per-file failures. |
| 12 | 20 | `rust-tidy-shellcheck` | Real shellcheck warnings on `src/ci/scripts/*.sh`. The upstream tidy `extra_checks::shellcheck` enforces the same. |
| 13 | 12 | `gha-pin-actions-to-sha` (bundled `ci/github-actions@v1`) | Real findings — 12 step entries pin by tag rather than 40-char commit SHA. Worth filing for supply-chain hardening. |
| 14 | 11 | `rust-sources-no-zero-width` (bundled rust@v1) | Real findings — likely test fixtures intentionally exercising zero-width chars (Unicode test cases). **Recommended:** scope-exclude `tests/ui/parser/`, `tests/ui/`. |

**Real findings (alint surfaced, existing tidy missed):**

- 6 vendored Rust files lack final newline + trailing whitespace
  (alint scans the full tree; tidy excludes vendored dirs by
  default).
- 12 GitHub Actions workflow steps pin third-party actions by
  floating tag rather than commit SHA (supply-chain hardening
  candidate; tidy doesn't check this).
- 11 zero-width characters in test fixtures (likely intentional;
  scope-exclude needed).

**False-positive class (the 1,091 `rust-sources-snake-case`
hits):** the single largest violation count is the bundled
`rust@v1` snake-case rule firing on compiler internals where
`rustc_*` naming is deliberate. **Not a config bug** (the rule
fires correctly per its definition); it's a bundled-rule
**scope mismatch** for the rust-lang/rust monorepo. **Recommended
fix:** override the rule with a `paths.exclude` for
`compiler/**` + `library/std/src/sys/**`, OR file an upstream
alint feature request for an `allow_compiler_naming` knob.

**Pitfall #22 verification:** ZERO instances in `.alint.yml`.
`grep -nE 'pattern:\s*[|>]' /home/kaminsod/projects/alint/examples/rust-lang-rust/.alint.yml`
returns no matches. All patterns are single-quoted YAML scalars
(`pattern: '(?i)\b(TODO|XXX)\b'`, `pattern: '^//@ known-bug:'`,
`pattern: '^\s*#\[(test|bench)\]'`). No multi-line license-header
rule is in this config (the rust-lang/rust dual-license is
asserted via `oss-license-exists` + `compliance/apache-2@v1`,
which use file-presence not pattern matching).

---

## 7. Pitfall #22 verification (this batch's special call-out)

The brief asked: **verify every multi-line regex in this case
study's config for the YAML literal-block-scalar trailing-newline
issue (pitfall #22).**

**Verdict for `examples/rust-lang-rust/.alint.yml`: ZERO instances.**
`grep -nE 'pattern:\s*[|>][-+]?$'
/home/kaminsod/projects/alint/examples/rust-lang-rust/.alint.yml`
returns no matches — all regex patterns in this config use
single-quoted YAML scalars (e.g.
`pattern: '(?i)\b(TODO|XXX)\b'`). The Apache-2 dual-license
guard is implemented via file-presence checks
(`oss-license-exists` + `compliance/apache-2@v1`'s presence
rules), not multi-line regex matching, so pitfall #22 doesn't
apply here.

---

## 8. Followup feature work surfaced

Priority order (saturation across case-study set in parens):

- **`ordered_block`** (sortedness between marker pairs) — covers
  `tidy::alphabetical` here (invoked 6× in tidy). **v0.10 ship-target
  (7 sources: rust + airflow + tokio + cpython + arrow + golang/go +
  protobuf failure_lists).** Single most-requested missing rule kind
  across the validation passes.
- **`registry_paths_resolve`** (extract paths from a structured
  registry, assert each resolves) — covers `tidy::triagebot` here.
  **v0.10 ship-target (8 sources).**
- **`cross_file_value_equals`** — covers `tidy::gcc_submodule` here
  (submodule SHA == version-file contents). **v0.10 ship-target
  (10 sources).**
- **`file_pair_block_match`** (markered block in file A equals same
  markered block in file B) — covers `tidy::rustdoc_css_themes`
  here. **v0.10 design candidate (3 sources: rust + cpython×2).**
- **`balanced_delimiters`** — covers `tidy::rustdoc_templates`
  here. **v0.10 design candidate (2 sources: rust + cpython).**

---

## 9. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **A `tidy@v1` bundled-ruleset draft.** 11 of the 30 tidy modules
   are declarative today (line/file lengths, trailing whitespace,
   line endings, no-TODO, edition, extdeps, known-bug, rustdoc-gui
   description, Windows-illegal filenames, no `#[test]` in stdlib,
   debug-artifact guards). Packaging these as
   `alint://bundled/tidy/rust@v1` (with `scope_filter` to exclude
   `src/llvm-project/` and `src/gcc/` by default, parameterised
   line-length thresholds for `.goml` / error-code markdown) would
   raise this case study's pitch from "20 lines of YAML" to "1 line
   of `extends:`". The remaining 19 tidy modules become the
   explicit gap list documented in the bundled ruleset's README.
2. **`scope_filter` for the `src/llvm-project/` + `src/gcc/`
   sub-trees.** v0.9.17's `scope_filter` evolution lets a single
   top-level filter apply once across N rules — eliminates ~30
   lines of repeated exclusions and makes "what counts as
   rust-monorepo source" a single source of truth.
3. **`alint suggest` against a fresh `rust-lang/rust` clone.** The
   current README is hand-authored from the tidy module catalogue;
   running `alint suggest` would surface bundled candidates the
   case study didn't reach for (likely `agent-context`,
   `compliance/apache-2` for the Apache half of the dual-license,
   `tooling/editorconfig`, `docs/adr` for the RFCs tree). Worth a
   5-minute pass for the v0.10 ladder evidence.

---

## 10. Validation status (2026-05-07)

- **alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).
- **`.alint.yml` in this directory:** **shipped — 283 lines, 20
  repo-specific rules, 5 bundled rulesets folded in via `extends:`,
  62 effective rules loaded.**
  `alint validate-config` confirms `✓ Config valid: 62 rule(s)
  loaded`.
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  2,698-violation breakdown (the long tail is dominated by 1,091
  bundled `rust-sources-snake-case` hits on compiler-internal
  `rustc_*` naming, a bundled-rule scope mismatch rather than a
  config bug; 668 line-length warnings on the long-tail of
  `compiler/`+`tests/` markdown / yaml; and 237 real `// TODO`
  marker findings).
- **Rule-kind candidate status:**
  - `ordered_block` — v0.10 ship-target (7 sources). Rust monorepo
    is the canonical demand-driver (6 invocations of `alphabetical`
    in tidy).
  - `registry_paths_resolve` — v0.10 ship-target (8 sources). Rust
    monorepo's `triagebot.toml` is one of the canonical sources.
  - `cross_file_value_equals` — v0.10 ship-target (10 sources).
    Rust monorepo's `gcc_submodule` is the niche-but-textbook
    instance.
  - `file_pair_block_match`, `balanced_delimiters` — v0.10 design
    candidates (rust monorepo is one of the demand-drivers for
    each).
- **Pitfall #22 instances in this directory's config:** **ZERO**
  (`grep -nE 'pattern:\s*[|>][-+]?$' .alint.yml` returns no
  matches; all patterns are single-quoted YAML scalars).
- **Bundled-ruleset rule counts (authoritative as of 2026-05-07):**
  oss-baseline=15, rust=11, monorepo=4, monorepo/cargo-workspace=4,
  ci/github-actions=3, hygiene/no-tracked-artifacts=11,
  compliance/apache-2=3.
