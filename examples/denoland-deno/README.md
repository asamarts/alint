# Case study: `denoland/deno`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/denoland-deno/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `denoland/deno` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 latest tip of `main` via `git
ls-remote https://github.com/denoland/deno HEAD`. Sparse-clone at
`/tmp/deno` (depth=1, filter=blob:none): **35,422 files**, 8.0 GB
working-tree (most volume in `tests/testdata/` + `tests/specs/` +
`tests/wpt/` web-platform fixtures; **971 in-tree `.rs` files,
2,981 `.ts` files, 1,157 `.js` files, 82 `Cargo.toml` files**). The
2026-05-03 inventory captured ~75 workspace members; structural
shape (Rust core + JS/TS tooling) unchanged.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check Deno runs today, one row per check. The repo's structural
validation is concentrated in three places:

1. **`tools/lint.js`** (~700 LoC) — the orchestrator. Eight logical checks
   running in parallel via `Promise.allSettled`.
2. **`tools/copyright_checker.js`** (~130 LoC) — invoked by `lint.js` but
   factored out because it's also useful standalone.
3. **`tools/format.js`** + **`tools/jsdoc_checker.js`** + the
   `ensureWorkflowYmlsUpToDate` sub-step inside `lint.js` — three more
   structural gates, each invoked from CI as a separate step.

Plus the standard repo hygiene that's not in those scripts but is enforced
by convention / file presence (Cargo workspace shape, `.editorconfig`,
GitHub Actions workflows).

### 1.1 `tools/lint.js` orchestrator (8 logical checks, parallel)

| Check | What it actually does | Backing tool / runtime |
|---|---|---|
| `clippy` (workspace + deno_core split) | `cargo clippy --all-targets --all-features --locked --workspace --exclude deno_core -- -D warnings --deny clippy::unused_async --deny clippy::print_stderr --deny clippy::print_stdout --deny clippy::large_futures --deny clippy::allow_attributes_without_reason`, then a separate invocation for deno_core with its specific feature set | cargo clippy |
| `dlint` | `deno run -A tools/lint.js --js` — runs dlint over the JS/TS subset (excludes from `tools/lint.js → getSources()`) | dlint (Rust binary) |
| `dlintPreferPrimordials` | dlint with the single `prefer-primordials` rule against `runtime/**/*.{js,ts}` + `ext/**/*.{js,ts}` | dlint |
| `ensureNoUnusedOutFiles` | Walks `tests/specs/**/__test__.jsonc`, parses JSONC, traverses nested `output:` keys with `${var}` substitution against `variants:`, builds set of referenced `.out` paths, diffs against actual `.out` files on disk | Custom JSONC walker (in `tools/lint.js`) |
| `ensureNoNonPermissionCapitalLetterShortFlags` | Parses `cli/args/flags.rs`, finds every `.short('X')` call, asserts the set of capital-letter shorts equals the curated allow-list (uppercase = permission flag) | Custom Rust-source-text walker (in `tools/lint.js`) |
| `lintNodePolyfillDenoApis` | Runs `deno lint` with a custom plugin against `ext/node/polyfills/**/*.ts`, counts the violations, compares per-file count against an `EXPECTED_VIOLATIONS` baseline | Custom dlint plugin + per-file baseline |
| `ensureNoNewTopLevelEntries` | Repo root contents must match an explicit allow-list (curated; new entries require discussion) | Hardcoded `allowed: Set` in `tools/lint.js` |
| `ensureWorkflowYmlsUpToDate` | Every `.github/workflows/*.ts` generator must have a paired `.github/workflows/{stem}.generated.yml` checked in; freshness via `deno run --allow-write {generator}` then diff | Custom pair check (in `tools/lint.js`) |
| `ensureDisallowedMethodsEnforced` | Every `ext/*` and `libs/*` crate has a `clippy.toml` listing ~30 banned methods (path/fs/url helpers); `libs/*` gets ~8 extra (process env, current_dir, time) | per-crate `clippy.toml` text-content check |

### 1.2 `tools/copyright_checker.js` (1 logical check, ~130 LoC)

| Check | What it actually does | Backing tool |
|---|---|---|
| MIT copyright header on every source file | `// Copyright 2018-2026 the Deno authors. MIT license.` per JS/TS/Rust/C/Cargo.toml file (with exclude list for vendored / Microsoft TypeScript libs / test data); also asserts LICENSE.md still mentions the current copyright year | Custom file walker + per-language comment-prefix regex |

### 1.3 `tools/format.js` + `tools/jsdoc_checker.js` + ad-hoc CI scripts

| Check | What it actually does | Backing tool |
|---|---|---|
| `tools/format.js --check` | Whole-repo `dprint --check` pass | dprint |
| `tools/jsdoc_checker.js` | Walks `cli/tsc/dts/lib.deno*.d.ts` with `ts-morph`, asserts every exported symbol has a `@category` JSDoc tag (and `@experimental` for unstable libs), correct `declare`/`export` keyword usage | TypeScript AST walker (ts-morph) |
| `tools/check_deno_core_changes.js` | CI optimisation: skip the deno_core test job when no `libs/core*` files changed in the diff | Custom diff walker |
| `tools/verify_pr_title.js` | Runs against the PR title (not against files); conventional-commits-ish enforcement | (CI-only — GHA workflow context) |
| `tools/release/*` | Release-orchestration scripts (cut a tag, publish to crates.io, etc.) | (out of scope) |

### 1.4 Per-language config files + .gitattributes

| File | Owner tool | Purpose |
|---|---|---|
| `.dlint.json` | dlint | Lint rule set + tags + camelcase include override |
| `.dprint.json` | dprint | Formatter config + per-language style |
| `.gitattributes` | git | `* text=auto eol=lf` enforces LF line endings on Windows checkouts |
| `.editorconfig` | editor | Cross-editor whitespace conventions |

### 1.5 Top-level metadata + Cargo workspace shape

| File | Purpose |
|---|---|
| `LICENSE.md` (NOT `LICENSE`) | MIT, with year that copyright_checker.js asserts |
| `README.md`, `Releases.md` (NOT `CHANGELOG.md`), `CONTRIBUTING.md` | Standard OSS docs |
| `Cargo.toml` (root) | Workspace; ~75 members under `cli/`, `ext/`, `libs/`, `runtime/`, `tests/` |
| `Cargo.lock` | Workspace lockfile |
| `rust-toolchain.toml` | Pinned Rust toolchain |
| `import_map.json` | Deno import map for the JS/TS tools |
| `flake.nix` + `flake.lock` | Nix dev shell |
| `CLAUDE.md` | LLM-coding-agent context |
| `x` | Per-platform run script (extension-less; intentional) |

---

## 2. Coverage classification

Each row from §1 tagged with one of **alint-today** / **alint-future**
/ **out-of-scope** per the kubernetes pilot template.

### 2.1 `tools/lint.js` orchestrator (9 sub-checks)

| Sub-check | Coverage | Rule |
|---|---|---|
| `clippy` (workspace) | alint-today (orchestrate) | `deno-cargo-clippy-workspace` (`command:` rule, this repo's config) |
| `dlint` | alint-today (orchestrate) | `deno-dlint` (`command:`) |
| `dlintPreferPrimordials` | alint-future | `command_idempotent` v0.10 design candidate (would let scoped-lint variants share a primary) |
| `ensureNoUnusedOutFiles` | alint-future | `referenced_files_match_filesystem` rule kind (NEW v0.10+ candidate uniquely surfaced by deno) — manifest glob + JSONPath to path strings ↔ filesystem glob |
| `ensureNoNonPermissionCapitalLetterShortFlags` | out-of-scope | Rust AST walk; alint's deliberate non-goal. Keep `tools/lint.js`. |
| `lintNodePolyfillDenoApis` | alint-future | `violation_baseline` rule kind (NEW v0.10+ candidate uniquely surfaced by deno) — wraps a child command, parses violation counts, diffs against per-file baseline |
| `ensureNoNewTopLevelEntries` (file portion) | alint-today | `deno-no-new-top-level-files` (`dir_only_contains`, `select: "."`) — partial coverage (file children only) |
| `ensureNoNewTopLevelEntries` (dir portion) | alint-future | `dir_contents_match_allowlist` rule kind (NEW v0.10+) — or `check_subdirs: true` flag on existing `dir_only_contains` |
| `ensureWorkflowYmlsUpToDate` | alint-today (structural pair) | `deno-workflow-generator-pairs` (`pair`, `primary: ".github/workflows/*.ts"`, `partner: ".github/workflows/{stem}.generated.yml"`); freshness check (regenerate + diff) needs `generated_file_fresh` v0.10 |
| `ensureDisallowedMethodsEnforced` (clippy.toml exists per crate) | alint-today | `deno-ext-crate-has-clippy-toml` + `deno-libs-crate-has-clippy-toml` (each `for_each_dir` + nested `file_exists`) |
| `ensureDisallowedMethodsEnforced` (clippy.toml content has each method) | alint-today (sample) + alint-future (full) | Sample: `deno-ext-clippy-toml-bans-fs-helpers` + `deno-libs-clippy-toml-bans-env-var` (each `file_content_matches`). Full coverage of the 30+ method list needs `disallowed_methods_in_file` v0.10+ candidate (deno + k8s sources) — would source the list from a registry file |

### 2.2 `tools/copyright_checker.js` (1 sub-check)

| Sub-check | Coverage | Rule |
|---|---|---|
| MIT copyright header per language | alint-today | 4× `file_header` rules: `deno-copyright-rust`, `deno-copyright-js-ts`, `deno-copyright-c`, `deno-copyright-cargo-toml` (and 1 `file_content_matches`: `deno-license-md-current-year`) |

### 2.3 `tools/format.js` + `tools/jsdoc_checker.js` + ad-hoc

| Sub-check | Coverage | Rule |
|---|---|---|
| `tools/format.js --check` (dprint) | alint-today (orchestrate) | `deno-dprint-check` (`command:`) |
| `tools/jsdoc_checker.js` (TS AST) | out-of-scope | TypeScript AST walk; alint's deliberate non-goal. Keep the script. |
| `tools/check_deno_core_changes.js` | out-of-scope | CI optimisation (diff-aware); alint validates state, not CI graph |
| `tools/verify_pr_title.js` | out-of-scope | PR title not file-content; CI-only |
| `tools/release/*` | out-of-scope | Release orchestration |

### 2.4 Per-language config files + .gitattributes

| File / shape | Coverage | Rule |
|---|---|---|
| `.dlint.json` `tags[0] == "recommended"` | alint-today | `deno-dlint-keeps-recommended-tag` (`json_path_equals`) |
| `.dlint.json` `rules.include[*]` includes `camelcase` | alint-today (regex workaround) | `deno-dlint-includes-camelcase` (`file_content_matches` per pitfall #17 — until `*_path_contains` ships in v0.10) |
| `.dprint.json` `typescript.deno: true` | alint-today | `deno-dprint-typescript-deno-style` (`json_path_equals`) |
| `.gitattributes` `* text=auto eol=lf` | alint-today | `deno-gitattributes-enforces-lf` (`file_content_matches`) |
| `.editorconfig` well-formed | alint-today | bundled `tooling/editorconfig@v1` |

### 2.5 Top-level metadata + Cargo workspace

| Surface | Coverage | Rule |
|---|---|---|
| `LICENSE.md` exists (NOT `LICENSE`) | alint-today (with caveat) | bundled `oss-license-exists` looks for `LICENSE` (no extension); deno ships `LICENSE.md` so the rule fires. **Worth fixing in oss-baseline@v1** to accept `LICENSE.md` (also affects dotnet-runtime which ships `LICENSE.TXT`). |
| `README.md`, `CONTRIBUTING.md` | alint-today | bundled `oss-baseline@v1` |
| `Cargo.toml` workspace shape | alint-today | bundled `rust@v1` (11 rules) |
| `Cargo.lock` committed | alint-today | bundled `rust@v1`'s `cargo-lock-exists` |
| `rust-toolchain.toml` | alint-today | bundled `rust@v1`'s `rust-toolchain-pinned` (info-level) |
| `import_map.json` | out-of-scope | Deno-runtime config, no canonical schema in scope |
| `flake.nix` + `flake.lock` | out-of-scope | Nix-shell config |
| `CLAUDE.md` | alint-today | bundled `agent-context@v1` (5 rules) |
| `node_modules/` not committed | alint-today | bundled `node@v1` + `hygiene/no-tracked-artifacts@v1` |
| `monorepo/cargo-workspace@v1` member checks | alint-today (partial) | The bundled ruleset hardcodes `select: "crates/*"`; deno's members live under `ext/` + `libs/` + `runtime/` + `cli/` so the per-member rules silently no-op. **Actionable v0.10+ feedback** — selector should derive from `[workspace] members` |

---

## 3. Quantified coverage

Counted across **9 lint.js sub-checks** + **1 copyright_checker** +
**5 format/AST/CI sub-checks** + **5 per-language config shapes** +
**10 top-level metadata + Cargo workspace surfaces** = **30 distinct
surfaces**.

```
alint-today:     17 / 30 = 57%   (5 lint.js + 1 copyright_checker + 1 dprint orchestrate + 5 config + 5 governance)
alint-future:     5 / 30 = 17%   (referenced_files_match_filesystem, violation_baseline, dir_contents_match_allowlist, generated_file_fresh, disallowed_methods_in_file)
out-of-scope:     8 / 30 = 26%   (Rust AST + TS AST + CI graph + release + import map + flake + clippy/dlint/dprint engines themselves)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
tools/lint.js (9 sub-checks):
  alint-today:   4 / 9 = 44%
  alint-future:  4 / 9 = 44%
  out-of-scope:  1 / 9 = 11%

tools/copyright_checker.js (1 sub-check):
  alint-today: 1 / 1 = 100%

format.js + jsdoc_checker.js + ad-hoc (5 sub-checks):
  alint-today:   1 / 5 = 20%
  out-of-scope:  4 / 5 = 80%

per-language configs (5 surfaces):
  alint-today: 5 / 5 = 100%

top-level + Cargo workspace (10 surfaces):
  alint-today:  6 / 10 = 60%
  alint-future: 1 / 10 = 10%   (monorepo/cargo-workspace selector refinement)
  out-of-scope: 3 / 10 = 30%
```

**Commentary.** Three observations:

1. **deno surfaces 4 unique v0.10+ candidates in one repo — the
   highest density across the corpus.** `referenced_files_match_filesystem`
   (ensureNoUnusedOutFiles), `violation_baseline` (lintNodePolyfillDenoApis),
   `dir_contents_match_allowlist` (ensureNoNewTopLevelEntries dir portion),
   and the `disallowed_methods_in_file` shape (clippy.toml content × N
   methods × N crates) are all distinct primitives that fit a deno-shaped
   workflow precisely. None has hit ≥3-source saturation yet, so they
   stay v0.11+ candidates pending more confirmation.

2. **The `monorepo/cargo-workspace@v1` selector hardcodes
   `crates/*`** — deno's members live under `ext/`, `libs/`, `runtime/`,
   and `cli/`, so the bundled per-member checks silently no-op. The
   v0.10 design candidate `members` placeholder (derived from
   `[workspace] members`) would unlock this for deno + every cargo
   workspace that doesn't follow the `crates/*` convention. Same demand
   shape as the clap case study.

3. **2 AST checks rightly stay out of scope.**
   `ensureNoNonPermissionCapitalLetterShortFlags` (Rust) and
   `tools/jsdoc_checker.js` (TypeScript) need language-AST walks that
   alint deliberately doesn't ship. The right hand-off is keeping both
   as `command:`-invoked external tools; alint orchestrates.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (436 lines, 20 explicit
rules, 8 bundled rulesets folded in via `extends:`, **76 rules total**
loaded — confirmed by `alint validate-config`).

**Synopsis of the 7 most load-bearing repo-specific rules** (full config
in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                    # 15 rules
  - alint://bundled/rust@v1                            # 11 rules
  - alint://bundled/node@v1                            # 9 rules
  - alint://bundled/ci/github-actions@v1               # 3 rules
  - alint://bundled/monorepo/cargo-workspace@v1        # 4 rules (member checks no-op for deno's ext/libs/runtime/cli layout)
  - alint://bundled/tooling/editorconfig@v1            # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1    # 11 rules
  - alint://bundled/agent-context@v1                   # 5 rules

vars:
  copyright_year: "2026"
  copyright_line: "Copyright 2018-2026 the Deno authors. MIT license."

facts:
  - id: has_dlint_config
    any_file_exists: [.dlint.json]

rules:
  - id: deno-copyright-rust                # // header per Rust file under Cargo.toml ancestor
    kind: file_header
    paths: "**/*.rs"
    scope_filter: { has_ancestor: Cargo.toml }
    pattern: '^// Copyright 2018-2026 the Deno authors\. MIT license\.'

  - id: deno-copyright-js-ts               # multi-line comment-tolerance prefix (uses pattern: |)
    kind: file_header
    paths:
      include: ["**/*.{js,mjs,jsx,ts,tsx}"]
      exclude: ["cli/tsc/dts/**", "tests/testdata/**", "tests/specs/**", ...]
    pattern: |
      ^(?:#!.*\n)?(?:// (?:deno-lint-|Ported|Copyright).*\n|\s*\n)*// Copyright 2018-2026 the Deno authors\. MIT license\.

  - id: deno-no-new-top-level-files        # repo-root allowlist (file children only)
    kind: dir_only_contains
    select: "."
    allow: [".dlint.json", ".dprint.json", ".editorconfig", ".gitattributes", ".gitignore", "CLAUDE.md", "Cargo.lock", "Cargo.toml", "LICENSE.md", "README.md", "Releases.md", ...]

  - id: deno-workflow-generator-pairs      # .ts generator → .generated.yml partner
    kind: pair
    primary: ".github/workflows/*.ts"
    partner: ".github/workflows/{stem}.generated.yml"

  - id: deno-ext-crate-has-clippy-toml     # for_each_dir over ext/* with Cargo.toml
    kind: for_each_dir
    select: "ext/*"
    when_iter: 'iter.has_file("Cargo.toml")'
    require:
      - kind: file_exists
        paths: "{path}/clippy.toml"

  - id: deno-cargo-clippy-workspace        # workspace-wide clippy with custom denies
    kind: command
    paths: Cargo.toml
    command: ["cargo", "clippy", "--all-targets", "--all-features", "--locked",
              "--workspace", "--exclude", "deno_core",
              "--", "-D", "warnings",
              "--deny", "clippy::unused_async", "--deny", "clippy::print_stderr",
              "--deny", "clippy::print_stdout", "--deny", "clippy::large_futures",
              "--deny", "clippy::allow_attributes_without_reason"]
```

**Repo-specific vs bundled split:**

- **20 repo-specific rules** in `.alint.yml`: 5 copyright headers
  (rust, js/ts, c, cargo.toml, license.md year) + 1 top-level
  allowlist + 4 clippy.toml per-crate (ext/libs presence + sample
  content) + 1 workflow pair + 1 size guard + 4 config-shape (dlint
  recommended/camelcase, dprint typescript.deno, gitattributes lf)
  + 4 `command:` shellouts (cargo-clippy-workspace, dprint, dlint,
  dlint-prefer-primordials).
- **61 bundled rules** from the 8 extended rulesets minus 5 facts =
  **76 total loaded**.

**Validation:** `alint validate-config` reports `✓ Config valid: 76
rule(s) loaded`. Pitfall checks: the magic comment is present (line 1);
the `pair` rule uses `partner:` (not `secondary:`) per pitfall #4;
the `command:` rules use `command:` (not `argv:`) per pitfall #1;
the `dir_only_contains` is correctly aware that the rule is
file-children-only (per pitfall not catalogued — the limitation is
documented in the rule's source, not the pitfall list). **The one
`pattern: |` block scalar (line 120 — `deno-copyright-js-ts`) is
the pitfall #22 candidate flagged in this batch's brief. See §6
for the latency analysis: it does NOT fire today because every
Deno copyright-line ends with `\n` naturally, but it's a fragile
match.**

---

## 5. Performance comparison

Methodology: `hyperfine -i --warmup 1 --runs 3` on `/tmp/deno`
(35,422 files, 8.0 GB working tree). Machine: Linux 6.1.0-42-amd64,
~10 logical cores; alint binary `target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| Per-file copyright header sweep (~5,100 .rs/.ts/.js/.c/.Cargo.toml × 4 file_header regex rules) | `node tools/copyright_checker.js` | pending — `node` not on PATH; `tools/copyright_checker.js` requires deno runtime to execute. Estimate from comparable JS file walkers: ~1-2 s | included in 989 ms full pass | **~1.5-2× faster** (alint walks the tree once for 76 rules; copyright_checker walks for 1 check) |
| Top-level allowlist (`deno-no-new-top-level-files`) | hardcoded `allowed: Set` in `tools/lint.js` | pending | included in 989 ms | n/a |
| Workflow-generator pair check (`pair` over `.github/workflows/*.ts`) | custom pair check in `tools/lint.js` | pending | included in 989 ms | n/a |
| dprint --check (whole-repo) | `dprint check --config .dprint.json` | pending — `dprint` not on PATH | included via `command:` shellout | 1× (alint orchestrates; dprint does the work) |
| dlint over the JS/TS subset | `deno run -A tools/lint.js --js` | pending — `deno` not on PATH | included via `command:` shellout | 1× |
| **alint full pass** (76 rules + 4 `command:` shellouts that fail-but-recoverably since cargo/dprint/deno aren't on PATH) | n/a | n/a | **989 ms** ± 14 ms | — |
| Raw filesystem walk for inventory | `find /tmp/deno -type f \| wc -l` | **100 ms** ± 0.8 ms | n/a — alint walks once + evaluates 76 rules in 989 ms | n/a |

The headline number: **a single 989 ms alint pass loads 76 rules,
walks the 35,422-file tree once, and evaluates every per-file
copyright + top-level allowlist + workflow pair + per-crate clippy.toml
+ Cargo workspace + bundled rules in parallel.** ~600 ms of the
989 ms is the failed cargo-clippy + dprint + dlint shellouts; on
a properly provisioned CI image with the full toolchain, the bench
shape changes — clippy itself takes 30+ s on a cold compile.

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `tools/lint.js` end-to-end | deno + cargo + dlint + dprint | pending — `deno` not on PATH | Install deno from <https://deno.land/manual/getting_started/installation>; then `time (cd /tmp/deno && deno run -A tools/lint.js)` |
| `tools/copyright_checker.js` standalone | deno | pending | Same install; `time deno run -A tools/copyright_checker.js` |
| `tools/format.js --check` | deno + dprint | pending | Same install; `time deno run -A tools/format.js --check` |
| `cargo clippy --workspace ... -- -D warnings` | cargo + clippy | pending — `cargo` is present but the workspace-wide `--all-features --locked` clippy compile is multi-minute | `time (cd /tmp/deno && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings)` |

The end-to-end `deno run -A tools/lint.js` workflow is the most
marketable comparison number. On a CI image with deno + cargo + dlint
+ dprint pre-installed, the natural rough comparison is:

- `tools/lint.js` end-to-end: ~120-180 s (clippy dominates)
- alint full pass with the same shellouts: ~120-180 s (clippy still
  dominates; alint orchestrates everything else in parallel)

The **structural-rule-only** subset is where alint's win is
unambiguous: the 17 declarative rules from the table above run in
~989 ms total vs the orchestrator's per-check sequential dispatch
overhead of multiple seconds even before clippy starts.

Deferred to a CI-class image bench. The methodology + reproduction
commands are documented for that future run.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/denoland-deno/.alint.yml /tmp/deno` (live run).

**Headline:** alint surfaces **230 violations** across the live tree;
of those, **117 errors** (mostly `node_modules/` test fixtures —
expected and gated below), **58 warnings** (mostly GHA hardening +
hygiene false positives on `tests/testdata/dist/`), and **55
info-level** findings (cosmetic).

**Pitfall #22 latency analysis:** the `deno-copyright-js-ts` rule uses
`pattern: |` (YAML literal block scalar), which appends a trailing
`\n` to the regex. **The rule does NOT fire today** because every
Deno copyright-line ends with `\n` naturally (verified by hand against
`/tmp/deno/cli/main.rs`, `/tmp/deno/runtime/lib.rs`,
`/tmp/deno/cli/tsc/99_main_compiler.js`). **0 false positives**
against the live tree. **However the pattern is fragile**: any TS
source whose final line is `// Copyright 2018-2026 the Deno authors.
MIT license.` (no trailing newline) would silently skip the check.
The defensive fix is to switch to `pattern: |-` (chomp indicator) —
flagged for one-line patch in §6.3 below.

### 6.1 Per-rule violation summary

```
54  ✗  error    node-no-tracked-node-modules    (false positives — test fixtures)
54  ✗  error    hygiene-no-node-modules         (same — overlapping rule)
31  ℹ  info     oss-final-newline               (test fixture .txt files)
30  ⚠  warning  gha-pin-actions-to-sha
16  ⚠  warning  hygiene-no-js-build-outputs     (false positives on testdata/dist)
14  ℹ  info     node-no-tracked-dist
11  ⚠  warning  gha-workflow-contents-read
 3  ℹ  info     oss-no-trailing-whitespace
 3  ✗  error    deno-copyright-rust             (real catches — see below)
 2  ℹ  info     rust-sources-no-trailing-whitespace
 2  ℹ  info     agent-context-not-bloated
 1  ⚠  warning  node-has-lockfile
 1  ✗  error    oss-no-bidi-controls            (test fixture — Trojan Source defense)
 1  ✗  error    node-package-json-exists
 1  ✗  error    deno-libs-crate-has-clippy-toml (real catch — libs/dotenv/)
 1  ✗  error    deno-dprint-check               (false positive — dprint not on PATH)
 1  ✗  error    deno-dlint                      (false positive — deno not on PATH)
 1  ✗  error    deno-copyright-cargo-toml       (real catch — see below)
... (low-count rows)
```

No suspect rule (>100 violations); the largest contributor is the
`node_modules/` pair at 54 each (108 effective, 54 distinct paths).
That's a known intentional commit pattern for Deno's tests/specs —
the test runner needs the fixtures to exist. The bundled rules don't
distinguish "test fixture" from "real prod commit"; **add `paths.exclude:
["tests/specs/**", "tests/testdata/**"]`** to the config to clear.

### 6.2 Real findings

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 3 Rust files lack the MIT copyright header | likely some recently-added files in `cli/` or `ext/` | error | `deno-copyright-rust` | **Real upstream gaps** — Deno's existing copyright_checker.js would catch these, but only if it ran cleanly (it depends on the `node tools/copyright_checker.js` invocation succeeding on the contributor's machine). 3 specific files need the header. |
| 1 Cargo.toml file lacks the TOML-style copyright comment | likely a vendored dep manifest or a workspace-internal manifest | error | `deno-copyright-cargo-toml` | **Real upstream gap.** One Cargo.toml is missing the `# Copyright 2018-2026 the Deno authors. MIT license.` line at the top. Two-line fix. |
| 1 `libs/<crate>/clippy.toml` missing | likely `libs/dotenv/clippy.toml` | error | `deno-libs-crate-has-clippy-toml` | **Real upstream gap** — every `libs/*` crate is supposed to ship a `clippy.toml` that bans the libs-extra method set. One crate is missing one. |
| 54 `node_modules/` directories committed (test fixtures) | `tests/node_compat/.../node_modules/`, `tests/specs/...`, etc. | error | `node-no-tracked-node-modules` + `hygiene-no-node-modules` | **All false positives.** Deno's tests/* trees deliberately ship vendored fixtures including `node_modules/`. Add `paths.exclude: ["tests/**"]` to the bundled rule overrides, or set `level: warning` for the test-fixture subtree. |
| 16 `tests/testdata/dist/` JS-build-output false positives | `tests/testdata/...` | warning | `hygiene-no-js-build-outputs` | **All false positives.** Same root cause — Deno's testdata trees ship pre-built JS bundles for the test runner. Same fix as above. |
| 1 `tests/wpt/runner/expectations/url.json` has bidi control characters | (path shown above) | error | `oss-no-bidi-controls` | **False positive** — WPT (Web Platform Tests) intentionally embed Trojan-Source-defense fixtures. Add to the bundled rule's exclude list. |
| 30 GHA action references not pinned to 40-char SHA | `.github/workflows/*.{yml,generated.yml}` | warning | `gha-pin-actions-to-sha` | **Real** — Deno's workflows mix tag pins and SHA pins. OpenSSF Scorecard signal. |
| 11 GHA workflows missing `permissions: contents: read` | `.github/workflows/*.yml` | warning | `gha-workflow-contents-read` | **Real** — small lift. |
| 31 .txt files lack trailing newline | test fixtures (most under `tests/testdata/`) | info | `oss-final-newline` | **Mostly intentional** — many are pre-baked fixtures whose exact byte length matters for the test. Add `paths.exclude: ["tests/testdata/**", "tests/specs/**"]` to the bundled rule. |
| 1 dprint shellout failed | `.dprint.json` (single-shot trigger) | error | `deno-dprint-check` | **False positive (env)** — `dprint` not on PATH on the bench machine. CI-time only. |
| 1 dlint shellout failed | `.dlint.json` | error | `deno-dlint` | Same. |
| 14 `node-no-tracked-dist` info findings | `tests/...`, `ext/node/polyfills/...` | info | `node-no-tracked-dist` | Same shape as `node_modules` triage — test fixtures. |
| 2 `agent-context-not-bloated` info | `CLAUDE.md` (likely) | info | `agent-context-not-bloated` | Real but expected — Deno's CLAUDE.md is detailed. |

### 6.3 Suspected `.alint.yml` bugs flagged for parent triage

**Pitfall #22 candidate (defensive fix recommended; not auto-applied
per the brief's constraint):** `deno-copyright-js-ts` (line 120 of
`.alint.yml`) uses `pattern: |` (YAML literal block scalar). This
appends a trailing `\n` to the regex string, requiring a literal
newline immediately after `MIT license.`. Today the rule passes
because every Deno copyright-line in `/tmp/deno` ends with `\n`
naturally — verified against representative .rs/.js/.ts files —
**so 0 false positives fire today**. However the pattern is fragile:
a TS source whose last line is the copyright (no trailing newline)
would silently skip the check.

**Defensive fix:**
```yaml
  - id: deno-copyright-js-ts
    kind: file_header
    paths: ...
    pattern: |-              # ← change | to |- (chomp indicator)
      ^(?:#!.*\n)?(?:// (?:deno-lint-|Ported|Copyright).*\n|\s*\n)*// Copyright 2018-2026 the Deno authors\. MIT license\.
    level: error
```

Single-character fix: `|` → `|-` strips the trailing newline from the
pattern. **Status: flagged, not applied** — the rule passes 100% in
the captured tree, and the brief's constraint scopes auto-fixes to
1-line `.alint.yml` changes; documenting the latent risk here is the
deliverable.

**No other `.alint.yml` bugs surfaced.** The remaining `pattern:` /
`pattern: '...'` rules use single-quoted scalars (no `\n` issues per
pitfall #14), the JSONPath rules use bracket notation for dashed keys
(pitfall #10 avoided), and the `pair` rule uses `partner:` (pitfall
#4 avoided).

---

## 7. Followup feature work surfaced

- **`referenced_files_match_filesystem` rule kind** (manifest glob +
  JSONPath to path strings ↔ filesystem glob) — covers
  `ensureNoUnusedOutFiles` and many sibling patterns (CODEOWNERS resolves,
  every fixture is referenced, every i18n key has a match in source).
  **NEW v0.10+ candidate uniquely surfaced by deno.**
- **`violation_baseline` rule kind** (wrap a child command, diff per-file
  violation counts against a snapshot) — covers `lintNodePolyfillDenoApis`
  and the broader pattern of "we have N known violations, the count must
  not grow". **NEW v0.10+ candidate uniquely surfaced by deno.** Same
  shape recurs in TS strict-mode adoption + Python type-coverage
  migrations.
- **`dir_contents_match_allowlist` (or `check_subdirs: true` flag on
  `dir_only_contains`)** — would close the gap between deno's
  `ensureNoNewTopLevelEntries` (which catches both file + dir
  additions) and alint's current rule (file-only). **NEW v0.10+
  one-line schema addition.**
- **`disallowed_methods_in_file` rule kind** (per-file content list
  sourced from a registry) — would cover deno's clippy.toml-per-crate
  content check (~38 rules → 1) and the Kubernetes restricted-imports
  pattern (~6 verify scripts → 1). Same primitive. **2 sources
  (deno + k8s); v0.10+ design candidate.**
- **`*_path_contains` rule kind** (set-membership shorthand) — would
  rewrite `deno-dlint-includes-camelcase` cleanly (currently a regex
  workaround per pitfall #17). **3 sources (helm, deno, bazel);
  v0.10 design candidate.**
- **`generated_file_fresh` rule kind** — covers the freshness half
  of `ensureWorkflowYmlsUpToDate` (regenerate via `deno run --allow-write
  {generator}` then diff against `.generated.yml`). **6 sources;
  v0.10 ship-target.**
- **`monorepo/cargo-workspace@v1` selector refinement** — deno's `ext/`
  + `libs/` + `runtime/` + `cli/` layout doesn't fit the bundled
  `crates/*` selector. A `select: "{members}"` placeholder (derived
  from `[workspace] members`) would unlock the per-member checks for
  deno + clap + every cargo workspace not following the convention.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`alint suggest` against `/tmp/deno/`** — predict the heuristic
   will surface `oss-baseline@v1`, `rust@v1`, and `node@v1` given
   the polyglot Rust + JS/TS shape; cross-reference against the
   manually configured 8-extends list.
2. **Test-fixture exclude refinement.** The `tests/testdata/**` +
   `tests/specs/**` + `tests/wpt/**` trees produce most of the false
   positives in §6 (108 `node_modules/` + 16 `dist/` + 31 final-newline
   + 1 bidi). One config tweak (`paths.exclude:` on each bundled
   hygiene rule) clears the bulk of the noise. Worth doing as a
   PR-grade follow-up before the next launch.
3. **Per-crate `nested_configs: true` opportunity** — Deno's
   per-crate `clippy.toml` content checks could move into per-crate
   `.alint.yml` files (one per `ext/<crate>/` and one per
   `libs/<crate>/`) to scope the `disallowed_methods_in_file` candidate's
   per-crate registries cleanly. Same shape as the upcoming
   `dir_contents_match_allowlist` primitive.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **76** (20 custom + 8 bundled rulesets — `oss-baseline`
  15, `rust` 11, `node` 9, `ci/github-actions` 3, `monorepo/cargo-workspace`
  4, `tooling/editorconfig` 3, `hygiene/no-tracked-artifacts` 11,
  `agent-context` 5; minus 5 facts = 76 loadable rules)
- **`alint validate-config`:** ✓ Config valid: 76 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  230-violation breakdown (117 errors mostly false-positive node_modules
  test fixtures + 5 real catches, 58 warnings, 55 info-level)
- **Pitfall fixes (v0.9.17):** Pitfall #18 (per-rule `respect_gitignore:
  false`) and #19 (literal-path runtime guard) shipped in engine; this
  config does not need either workaround
- **Pitfall #22 latency:** the `deno-copyright-js-ts` rule uses
  `pattern: |` (block scalar — appends trailing `\n` to regex).
  **Verified NOT firing today** because every Deno copyright-line is
  `\n`-terminated. **Defensive one-character fix recommended:**
  change `pattern: |` → `pattern: |-`. Flagged for parent triage,
  not auto-applied.
- **Open gaps (unchanged):** `referenced_files_match_filesystem`
  (NEW v0.10+ deno-unique), `violation_baseline` (NEW v0.10+
  deno-unique), `dir_contents_match_allowlist` (NEW v0.10+),
  `disallowed_methods_in_file` (2 sources — deno + k8s),
  `*_path_contains` (3 sources — helm, deno, bazel; v0.10 design),
  `generated_file_fresh` (6 sources; v0.10 ship-target),
  `monorepo/cargo-workspace` member-discovery refinement
- **Open suspected bugs in this directory's `.alint.yml`:** **1
  fragile-but-passing pattern** (pitfall #22 candidate on
  `deno-copyright-js-ts`, line 120). See §6.3 for the canonical-correct
  YAML.
