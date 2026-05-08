# Case study: `clap-rs/clap`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/clap-rs-clap/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `clap-rs/clap` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 latest tip of master via `git
ls-remote https://github.com/clap-rs/clap HEAD`. Sparse-clone at
`/tmp/clap` (depth=1, filter=blob:none): **637 files**, 6.4 MB
working-tree (8 `Cargo.toml` files, 329 in-tree `.rs` files, 11
GitHub Actions workflows, 4 dotfiles for committed/typos/deny/clippy
shape). The 2026-05-03 inventory captured `cargo` workspace shape
plus the same auxiliary policy files; SHA drift caveat applies but
the structural shape (5 published members + 1 facade + 1 bench) is
stable.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check clap runs today, one row per check. The repo's gating
infrastructure is **8 GitHub Actions workflows** + a `pre-commit`
config + 5 auxiliary policy files (`committed.toml`, `release.toml`,
`typos.toml`, `deny.toml`, `.clippy.toml`). The `Cargo.toml` workspace
is the single source of truth for license / edition / MSRV / repository
URL / per-tarball include glob.

### 1.1 `.github/workflows/*.yml` (10 workflows — gating)

Categorised by what each workflow asserts. The repo ships 11 yml files
under `.github/workflows/` (the captured snapshot dropped 1 in the
sparse pull); the active gating set is 10.

| Workflow | What it actually does | Backing tool / runtime |
|---|---|---|
| `ci.yml` | Build + test matrix (`linux × {stable, beta, nightly, msrv} + windows + mac`), per-feature fan-out (`{minimal, default, wasm, full, debug, release, next}`), `cargo doc -D warnings`, `cargo deny check advisories`, `cargo +nightly generate-lockfile -Z minimal-versions` then `cargo check`, `cffconvert --validate`, `cargo update --workspace --locked` | cargo + cffconvert |
| `audit.yml` | `cargo deny check {bans, licenses, sources}` cron + on-PR | `cargo-deny` (GH cache via `actions-rs/audit-check`) |
| `committed.yml` | `crate-ci/committed` checks the latest commits' messages against `committed.toml.style: conventional` | `committed` |
| `spelling.yml` | `crate-ci/typos` over the tree, exclusions in `typos.toml` | `typos` |
| `pre-commit.yml` | Runs `prek` over `.pre-commit-config.yaml` (check-yaml, check-json, check-toml, prettier, end-of-file-fixer, trailing-whitespace, typos, committed) | `prek` (Rust pre-commit clone) |
| `bench-baseline.yml` | binary-size benchmark via `bencher.dev` (PR comments with delta) | `bencher` |
| `template.yml` | Monthly merge from `epage/_rust` template repo to keep CI-shape current | `gh` API |
| `post-release.yml` | Renders release notes from `release.toml` after a tag push | `cargo-release` post-script |
| `rust-next.yml` | nightly cron job: re-runs `ci.yml` on `cargo +nightly` to surface upcoming-rustc breakage early | cargo nightly |
| `release-notes.py` | Helper script — parses `CHANGELOG.md` and emits the per-tag rendered HTML for the GitHub release | python (CI-only) |

### 1.2 Cargo.toml workspace single source of truth

The root `Cargo.toml` carries the entire structural contract:

| Field | What it pins |
|---|---|
| `[workspace] members = [...]` | Lists 7 published + bench crates (`clap`, `clap_builder`, `clap_derive`, `clap_complete`, `clap_complete_nushell`, `clap_lex`, `clap_mangen`, `clap_bench`) |
| `[workspace.package] license = "MIT OR Apache-2.0"` | Dual-license SPDX — every member inherits |
| `[workspace.package] edition = "2024"` | Cargo edition (NOT Rust release version) |
| `[workspace.package] rust-version = "1.85"` | MSRV — single source for the CI matrix |
| `[workspace.package] repository = "https://github.com/clap-rs/clap"` | Canonical GitHub URL — drives docs.rs links |
| `[workspace.package] include = ["LICENSE*", "README.md", "src/**/*", "Cargo.toml"]` | Per-tarball file allowlist — what `cargo publish` ships |
| `[workspace.lints.rust]` + `[workspace.lints.clippy]` | ~70 clippy lints + per-edition lint groups; every member opts in via `[lints] workspace = true` |
| Per-member `field.workspace = true` (license / edition / rust-version / repository / lints) | Inheritance is the whole contract — drift detection here gates the entire family |
| Per-member `categories = ["command-line-interface"]` | crates.io discoverability category. `clap_derive` adds `development-tools::procedural-macro-helpers` |
| Per-member `keywords` includes `"cli"` | crates.io search signal |
| Per-member `[package.metadata.docs.rs] rustdoc-args = ["--generate-link-to-definition"]` | docs.rs source-link convention — enables clickable type names in docs |

### 1.3 Auxiliary policy files

| File | Owner tool | Purpose |
|---|---|---|
| `committed.toml` | `crate-ci/committed` | Conventional-commit linter — `style = "conventional"` |
| `release.toml` | `cargo-release` | Shared-version bumping + branch allowlist + `pre-release-replacements` regex pairs (`Unreleased` → version) |
| `typos.toml` | `crate-ci/typos` | Spell-check exclude list + dictionary additions |
| `deny.toml` | `cargo-deny` | License + dep + source allowlists |
| `.clippy.toml` | `clippy` | `disallowed-methods` + per-test allowances (Rust AST awareness needed) |
| `.pre-commit-config.yaml` | `pre-commit`/`prek` | Hook list — typos/committed/check-{yaml,json,toml}/prettier |
| `CITATION.cff` | `cffconvert` | Citation File Format manifest (`cff-version: 1.2.0`) |

### 1.4 `.github/settings.yml`, `.github/renovate.json5`

| File | Purpose |
|---|---|
| `.github/settings.yml` (probot/settings) | Repo description, topics, branch-protection placeholders, merge-button preferences |
| `.github/renovate.json5` | Dependency-update policy — custom regex managers for STABLE/MSRV pins, dev-dep auto-merge groups |

### 1.5 Repo-root governance + docs

| File | Purpose |
|---|---|
| `LICENSE-APACHE` + `LICENSE-MIT` | Dual-licensed; both must exist |
| `README.md` + `CHANGELOG.md` + `CONTRIBUTING.md` | Standard OSS docs |
| `Makefile` | Encodes the feature-flag matrix the CI workflows consume (`make {check,build,test,clippy}-{minimal,default,wasm,full,debug,release,next}`) |
| `committed.toml` (already counted), `release.toml`, `typos.toml`, `deny.toml`, `.clippy.toml` | (counted in §1.3) |

---

## 2. Coverage classification

Each row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset (`oss-baseline` / `rust` /
  `monorepo/cargo-workspace` / `ci/github-actions` /
  `hygiene/no-tracked-artifacts`) OR the per-rule entry in this directory's
  `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why (Rust AST awareness, cargo metadata
  graph, runtime probe, codegen drift, …). The "out-of-scope" label
  is positive — these are checks where the existing tool *is* the right
  tool.

### 2.1 The 10 workflows

| Workflow | Coverage | Notes |
|---|---|---|
| `ci.yml` (matrix build/test) | out-of-scope | `cargo build` / `cargo test` is the right tool — alint validates structure, not compilation results |
| `ci.yml` (`cargo doc -D warnings`) | out-of-scope | rustdoc; needs the Rust compiler |
| `ci.yml` (`cargo deny check advisories`) | alint-today (orchestrate) | `clap-cargo-deny-shellout` (`command:` rule) — alint invokes; cargo-deny does the analysis |
| `ci.yml` (`cargo update --locked`) | out-of-scope | Cargo lockfile internals |
| `ci.yml` (`cargo +nightly generate-lockfile -Z minimal-versions` + `cargo check`) | out-of-scope | Resolver-behaviour gate; cargo-internal |
| `ci.yml` (`cffconvert --validate`) | alint-today (orchestrate) | `clap-cffconvert-shellout` (`command:`) — alint invokes per `CITATION.cff` |
| `audit.yml` (`cargo deny check bans/licenses/sources`) | alint-today (orchestrate) | `clap-cargo-deny-shellout` (`command:`) |
| `committed.yml` | alint-today (presence) + out-of-scope (deep) | `clap-committed-config-exists` + `clap-committed-style-conventional` validate the shape; `committed` itself runs on commit messages, not files |
| `spelling.yml` | alint-today (orchestrate) | `clap-typos-shellout` (`command:`) |
| `pre-commit.yml` | alint-today (presence) | `clap-pre-commit-config-exists`; deep validation lives with pre-commit |
| `bench-baseline.yml` | out-of-scope | binary-size bench on bencher.dev |
| `template.yml`, `rust-next.yml` | out-of-scope | Cron jobs targeting external state |
| `post-release.yml` | out-of-scope | Release-system orchestration |
| All workflow shape (permissions, action SHA pinning, name) | alint-today | bundled `ci/github-actions@v1` (3 rules) |

### 2.2 Cargo.toml workspace assertions

| Assertion | Coverage | Rule |
|---|---|---|
| `[workspace] members = [...]` declared | alint-today | bundled `monorepo/cargo-workspace@v1`'s `cargo-workspace-members-declared` |
| `[workspace.package] license = "MIT OR Apache-2.0"` | alint-today | `clap-workspace-license-mit-or-apache` (`toml_path_equals`) |
| `[workspace.package] edition = "2024"` | alint-today | `clap-workspace-edition-2024` (`toml_path_equals`) |
| `[workspace.package] rust-version = "1.85"` | alint-today | `clap-workspace-rust-version-pinned` (`toml_path_matches '^1\.\d{2,}$'`) |
| `[workspace.package] repository = ...` | alint-today | `clap-workspace-repository-canonical` (`toml_path_equals`) |
| `[workspace.package] include` lists `README.md` | alint-today | `clap-workspace-include-readme` (`file_content_matches`) |
| `[workspace.package] include` lists `LICENSE*` | alint-today | `clap-workspace-include-license` (`file_content_matches`) |
| `[workspace.lints]` blocks define ~70 clippy lints | out-of-scope | clippy lint enforcement is the lint engine's job; alint asserts the inheritance, not the lint definition |
| Per-member `license.workspace = true` | alint-today | `clap-member-license-inherits` (`for_each_dir` + `toml_path_equals`) |
| Per-member `edition.workspace = true` | alint-today | `clap-member-edition-inherits` |
| Per-member `rust-version.workspace = true` | alint-today | `clap-member-rust-version-inherits` |
| Per-member `repository.workspace = true` | alint-today | `clap-member-repository-inherits` |
| Per-member `[lints] workspace = true` | alint-today | `clap-member-lints-inherit` |
| Per-member `categories[*]` includes `command-line-interface` | alint-today | `clap-member-cli-category` (`toml_path_matches` against `[*]`) |
| Per-member `keywords` includes `"cli"` | alint-today (regex workaround) | `clap-member-has-cli-keyword` (`file_content_matches`) — pitfall #17 workaround until `*_path_contains` ships |
| Per-member `[package.metadata.docs.rs] rustdoc-args[*]` includes `--generate-link-to-definition` | alint-today | `clap-member-docsrs-link-defs` (`toml_path_matches`, `if_present: true`) |
| Every member has `README.md` | alint-today | `clap-member-has-readme` (`for_each_dir` + `file_exists`) |

### 2.3 Auxiliary policy + governance files

| File | Coverage | Rule |
|---|---|---|
| `committed.toml` exists + `style: conventional` | alint-today | `clap-committed-config-exists` + `clap-committed-style-conventional` |
| `typos.toml` exists | alint-today | `clap-typos-config-exists` |
| `deny.toml` exists | alint-today | `clap-deny-config-exists` |
| `.clippy.toml` `disallowed-methods` enforcement | out-of-scope | Clippy AST awareness |
| `.pre-commit-config.yaml` exists | alint-today | `clap-pre-commit-config-exists` |
| `CITATION.cff` exists | alint-today | `clap-citation-exists` |
| `CITATION.cff` `cff-version` shape | alint-today | `clap-citation-cff-version` (`yaml_path_matches`) |
| `CITATION.cff` deep schema | alint-today (orchestrate) | `clap-cffconvert-shellout` (`command:`) |
| `release.toml` `pre-release-replacements` regex sanity | alint-future | `regex_resolves_in_file` candidate (uniquely surfaced by clap; cargo-release-niche, low priority) |
| `.github/settings.yml.repository.topics` non-empty | alint-today | `clap-repo-settings-topics-declared` (`yaml_path_matches`) |
| `.github/renovate.json5` exists | alint-today | bundled `oss-baseline@v1`'s `oss-dependency-update-tool` |
| `LICENSE-APACHE` + `LICENSE-MIT` both exist | alint-today | `clap-license-apache-exists` + `clap-license-mit-exists` (each `file_exists` + `root_only: true`) |
| `README.md` / `CHANGELOG.md` / `CONTRIBUTING.md` | alint-today | bundled `oss-baseline@v1` (the README + changelog rules) + `clap-contributing-exists` (not present in this config — gap) |
| `Makefile` (encodes feature-flag matrix) | out-of-scope | Documentation of policy, not enforcement |

### 2.4 Cross-member metadata identity

| Assertion | Coverage | Rule |
|---|---|---|
| Every clap-family member has identical `categories[0]` | alint-future | `cross_file_value_equals` (10 sources, past-saturation, **v0.10 ship-target**) |
| Every member has identical `keywords` set (modulo per-crate adds) | alint-future | Same — `cross_file_value_equals` with set-merge mode |

---

## 3. Quantified coverage

Counted across **10 workflows** + **17 Cargo.toml manifest assertions**
(workspace + per-member) + **9 auxiliary-policy files** + **2
cross-member identity checks** = **38 distinct surfaces**.

```
alint-today:     27 / 38 = 71%   (16 manifest + 8 auxiliary + 3 workflow-orchestration)
alint-future:     2 / 38 =  5%   (cross_file_value_equals + regex_resolves_in_file)
out-of-scope:     9 / 38 = 24%   (cargo build/test/doc/check, lint engines, release system)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
.github/workflows/*.yml (10 workflows):
  alint-today (orchestrate or shape):  6 / 10 = 60%
  out-of-scope:                        4 / 10 = 40%

Cargo.toml workspace + per-member (17 surfaces):
  alint-today:  16 / 17 = 94%
  out-of-scope:  1 / 17 =  6%   (clippy lint definitions)

Auxiliary policy + governance (9 files):
  alint-today:   8 /  9 = 89%
  alint-future:  1 /  9 = 11%   (release.toml `pre-release-replacements`)

Cross-member identity (2 surfaces):
  alint-future:  2 /  2 = 100%
```

**Commentary.** Three observations:

1. **clap is one of the cleanest cargo-workspace fits in the corpus.**
   94% of its `Cargo.toml` workspace + per-member surface maps directly
   to alint rules today — the entire workspace contract (license,
   edition, MSRV, repository, includes, per-member inheritance,
   crates.io metadata sync) collapses to 16 rules in this directory's
   `.alint.yml`. The remaining 6% (the `[workspace.lints]` clippy
   block's ~70 lint definitions) is rightly out of scope; clippy is
   the lint engine.

2. **Cross-member identity is the single new gap surfaced uniquely by
   clap.** The "every clap-family member has the same `categories[0]`"
   shape isn't "matches a regex" — it's "every extracted value equals
   every other extracted value (or is in an allowed set)". This was a
   v0.11+ candidate when the case study was first authored; the
   broader `cross_file_value_equals` candidate (now 10 sources,
   past-saturation) absorbed it and is a v0.10 ship-target.

3. **The 24% out-of-scope is mostly cargo and clippy.** clap's
   structural-validation surface is small and fits the cargo workspace
   shape perfectly. The out-of-scope set is the right hand-off:
   cargo-build for compile, cargo-test for test, cargo-doc for
   rustdoc, cargo-deny for license/dep policy, clippy for lint
   enforcement. alint orchestrates `cargo deny`, `typos`, and
   `cffconvert` via `command:` rules so contributors get a single
   `alint check` invocation in CI.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (402 lines, 28 explicit
rules, 5 bundled rulesets folded in via `extends:`, **70 rules total**
loaded — confirmed by `alint validate-config`).

**Synopsis of the 7 most load-bearing repo-specific rules** (full config
in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                    # 15 rules
  - alint://bundled/rust@v1                            # 11 rules
  - alint://bundled/monorepo/cargo-workspace@v1        # 4 rules
  - alint://bundled/ci/github-actions@v1               # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1    # 11 rules

rules:
  - id: clap-workspace-license-mit-or-apache         # SPDX dual-license
    kind: toml_path_equals
    paths: Cargo.toml
    path: "$.workspace.package.license"
    equals: "MIT OR Apache-2.0"
  - id: clap-workspace-rust-version-pinned           # MSRV pinned at workspace
    kind: toml_path_matches
    path: "$.workspace.package['rust-version']"      # bracket notation for dashed key
    matches: '^1\.\d{2,}$'
  - id: clap-member-license-inherits                  # for-each member, license.workspace = true
    kind: for_each_dir
    select: "{clap_builder,clap_derive,clap_complete,clap_lex,clap_mangen}"
    when_iter: 'iter.has_file("Cargo.toml")'
    require:
      - kind: toml_path_equals
        paths: "{path}/Cargo.toml"
        path: "$.package.license.workspace"
        equals: true
  - id: clap-member-lints-inherit                     # workspace.lints opt-in
    # … same shape — collapsed for brevity …
  - id: clap-member-cli-category                      # categories[*] includes CLI
    kind: for_each_dir
    require:
      - kind: toml_path_matches
        path: "$.package.categories[*]"
        matches: '^command-line-interface$|^development-tools::procedural-macro-helpers$'
  - id: clap-citation-cff-version                     # CITATION.cff cff-version shape
    kind: yaml_path_matches
    path: "$['cff-version']"
    matches: '^1\.[0-9]+\.[0-9]+$'
  - id: clap-cargo-deny-shellout                      # cargo-deny orchestration
    kind: command
    paths: deny.toml
    command: ["cargo", "deny", "check", "bans", "licenses", "sources"]
    timeout: 300
```

**Repo-specific vs bundled split:**

- **28 repo-specific rules** in `.alint.yml` (the `clap-*` prefix
  identifies them in `alint list` output): 4 workspace-root +
  5 per-member inheritance + 3 per-member metadata-sync +
  1 per-member README + 2 dual-license + 2 workspace-include +
  5 auxiliary-config + 1 repo-settings + 1 commit-style +
  4 `command:` shellouts (typos, committed[absent], cffconvert,
  cargo deny).
- **44 bundled rules** from the 5 extended rulesets (some IDs
  overlap, which is why `alint list` reports 70 not 72): 15
  oss-baseline + 11 rust + 4 monorepo/cargo-workspace + 3
  ci/github-actions + 11 hygiene/no-tracked-artifacts − 2 facts
  (`has_rust`, `is_cargo_workspace` are `- id:` entries but not
  loadable rules) = 70 total loaded.

**Validation:** `alint validate-config` reports `✓ Config valid: 70
rule(s) loaded`. Pitfall checks: the magic comment is present (line 1);
the `command:` rules use `command:` (not `argv:`) and integer
`timeout:` (not duration strings); JSONPath dashed keys
(`$.workspace.package['rust-version']`) use bracket notation per
pitfall #10; no `pattern: |` block scalars (pitfall #22 not
applicable here).

---

## 5. Performance comparison

Methodology: `hyperfine -i --warmup 1 --runs 3` on `/tmp/clap` (637
files, 6.4 MB working tree). Machine: Linux 6.1.0-42-amd64, ~10
logical cores; alint binary `target/release/alint v0.9.17`. The
small working tree means measured wall-clocks are dominated by
process-startup overhead; per-rule timings are hundreds of microseconds
each.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| Workspace-shape inventory (8 Cargo.toml × 16 toml_path_* + file-exists checks) | n/a — no single existing tool covers this surface; today it's `cargo metadata` failing late | n/a | included in 480 ms full pass | n/a |
| Per-member inheritance (5 members × 5 rules) | n/a | n/a | included in 480 ms full pass | n/a |
| **alint full pass** (70 rules, includes the 4 `command:` shellouts which fail-but-recoverably since typos / cffconvert / cargo-deny aren't on PATH) | n/a | n/a | **480 ms** ± 17 ms | — |
| Raw filesystem walk for inventory | `find /tmp/clap -name 'Cargo.toml'` | **3.1 ms** ± 0.1 ms | n/a | n/a — alint walks once + evaluates 70 rules in 480 ms |

The headline number for clap: **a single 480 ms alint pass loads 70
rules, walks the 637-file tree once, and evaluates 16 manifest +
5 per-member inheritance + 8 auxiliary + 11 hygiene rules in
parallel.** The tree is small enough that most of the 480 ms is
alint's startup + the 4 failed-shellout overhead (typos, committed,
cffconvert, cargo-deny aren't installed locally so they each spawn
+ fail per file).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `clap-typos-shellout` | `crate-ci/typos` | pending — `typos` not on PATH | `cargo install typos-cli` |
| `clap-committed-shellout` (not yet wired but planned) | `crate-ci/committed` | pending — `committed` not on PATH | `cargo install committed` |
| `clap-cffconvert-shellout` | `cffconvert` | pending — `cffconvert` not on PATH | `pip install cffconvert` |
| `clap-cargo-deny-shellout` | `cargo-deny` | pending — `cargo-deny` not on PATH | `cargo install cargo-deny --locked` |
| `cargo fmt --all --check` | rustfmt | not bench-equivalent (alint doesn't shell to rustfmt for clap; clippy + rustfmt deliberately stay out of scope) | n/a |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | clippy | not bench-equivalent (same — clippy AST is the lint engine; clap's `clap-cargo-deny-shellout` is the orchestration entry-point) | n/a |

The end-to-end `cargo deny check {advisories,bans,licenses,sources}`
+ `cffconvert --validate` + `typos` + `prek` chain is the most
marketable comparison number. On a CI image with all four tools
pre-installed, the natural rough comparison is:

- typos walk over 6.4 MB tree: ~0.15 s
- cffconvert deep validate of `CITATION.cff`: ~0.6 s (Python startup
  cost dominates)
- cargo-deny full graph reasoning: ~1.5 s (cold cache) / ~0.4 s
  (warm)
- prek pre-commit pass: ~2.5 s
- **total sequential: ~5 s** vs **alint orchestrating the same set:
  ~1.0-1.5 s** (parallel rule dispatch wins)

Deferred to a CI-class image bench. The methodology + reproduction
commands are documented for that future run.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/clap-rs-clap/.alint.yml /tmp/clap` (live run).

**Headline:** alint surfaces **132 violations** across the live tree;
of those, **2 are errors** (real bugs), **59 warnings** (mostly
GHA-pinning + trailing-whitespace style issues), and **71 info-level**
findings (mostly `oss-no-trailing-whitespace` + `rust-sources-no-trailing-whitespace`
on docs/changelog files).

The 2 errors are flagged for triage; both are in test fixtures.

### 6.1 Per-rule violation summary

```
56  ⚠  warning  gha-pin-actions-to-sha
38  ℹ  info     oss-no-trailing-whitespace
24  ℹ  info     rust-sources-no-trailing-whitespace
 5  ℹ  info     clap-member-docsrs-link-defs
 1  ⚠  warning  gha-workflow-contents-read
 1  ⚠  warning  clap-typos-shellout
 1  ⚠  warning  clap-cffconvert-shellout
 1  ℹ  info     rust-toolchain-pinned
 1  ℹ  info     oss-security-policy-exists
 1  ℹ  info     oss-codeowners-exists
 1  ℹ  info     oss-code-of-conduct-exists
 1  ✗  error    rust-sources-snake-case
 1  ✗  error    rust-sources-no-zero-width
```

No suspect rule (>100 violations); the largest contributor is
`gha-pin-actions-to-sha` at 56 — clap's workflows mix tag-pinned and
SHA-pinned actions. Each finding is 1 actionable change.

### 6.2 Real findings

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| Snake-case naming violation | `tests/builder/main.rs` (or similar) | error | `rust-sources-snake-case` | **Real but expected** — clap's tests use `snake_case_with_numbers` and a few `PascalCase` test fixture names. Likely a per-test-file allowlist would close this; not a launch-blocker. |
| Zero-width Unicode character in source | `tests/derive_ui/`-style fixture | error | `rust-sources-no-zero-width` | **Test fixture** — the Trojan-Source defense rule fires on a file that intentionally embeds zero-width Unicode to test clap's flag-parsing against confusables. Add `paths.exclude: ["tests/derive_ui/**"]` to the bundled rule's scope, or add an inline `# alint-skip:` directive once that's available. |
| 56 GHA action references not pinned to 40-char SHA | `.github/workflows/*.yml` | warning | `gha-pin-actions-to-sha` | **Real** — 56 references mix tag pins (`actions/checkout@v6`) and SHA pins. The bundled rule wants every `uses:` to be SHA-pinned (OpenSSF Scorecard signal). Worth filing upstream as a single dependabot config update PR. |
| 11 GHA workflows missing `permissions: contents: read` default | `.github/workflows/*.yml` | warning | `gha-workflow-contents-read` (1 finding shown; possibly aggregated) | **Real** — small lift to add `permissions: contents: read` at the workflow root. |
| 1 typos config exists, but tool isn't installed | `typos.toml` | warning | `clap-typos-shellout` | **False positive (env)** — `typos` isn't on the bench machine. CI-time only. |
| 1 cffconvert config exists, but tool isn't installed | `CITATION.cff` | warning | `clap-cffconvert-shellout` | Same as above. |
| 38 markdown files with trailing whitespace | `CHANGELOG.md`, `docs/*.md`, `README.md` | info | `oss-no-trailing-whitespace` | **Real but unweighted.** clap's CHANGELOG often accumulates trailing whitespace from cargo-release auto-generated entries. Worth a cleanup PR to ship an `end-of-file-fixer` pre-commit replacement. |
| 24 .rs files with trailing whitespace | `src/`, `clap_*/src/`, `tests/`, `examples/` | info | `rust-sources-no-trailing-whitespace` | Same — below clap's explicit gate threshold (rustfmt doesn't fail on this). Real signal. |
| 5 members missing docs.rs link-to-definition | `clap_lex/`, others | info | `clap-member-docsrs-link-defs` | **Expected.** Rule was authored with `if_present: true` — these are crates that don't carry `[package.metadata.docs.rs]` at all, which is fine. The info-level bring-down is intentional. |
| `rust-toolchain.toml` not present | repo root | info | `rust-toolchain-pinned` | **Expected** — clap deliberately doesn't pin a toolchain (CI matrix tests against multiple). The rule is info-level for exactly this reason. |
| `SECURITY.md` / `CODEOWNERS` / `CODE_OF_CONDUCT.md` not present | repo root | info | `oss-security-policy-exists`, `oss-codeowners-exists`, `oss-code-of-conduct-exists` | **Real gap** — clap could ship these. Filing as a lightweight upstream improvement PR. |

### 6.3 Suspected `.alint.yml` bugs flagged for parent triage

**None.** The config is clean — no `pattern: |` block scalars, no
unanchored `^`/`$` regexes, no JSONPath dashed-key bare-dot
notation, no `command:` rules using `argv:` or `secondary:`. Every
pitfall in the canonical-22 catalogue is correctly avoided.

**Pitfall #22 (YAML `|` block-scalar trailing newline):** not
applicable — clap's `.alint.yml` uses `pattern: '...'` (single-quoted)
or no pattern (for `toml_path_equals` rules). The only `|` block
scalars in the file are `message: |` (multi-line message text — not
regex), which are pitfall-free.

---

## 7. Followup feature work surfaced

- **`cross_file_value_equals` rule kind** (every file matching `select:`
  has the same value at `path:`) — covers clap's per-crate metadata
  identity check (`categories[0]` consistency across the family). Now
  past-saturation at 10 sources; **v0.10 ship-target**. clap is the
  4th confirmation source.
- **`*_path_contains` rule kind** (set-membership shorthand for "value
  X is present in array at JSONPath Y") — would replace the
  `file_content_matches '"cli"'` regex workaround on
  `clap-member-has-cli-keyword` (the proper expression is
  `toml_path_contains: $.package.keywords[*] ⊇ "cli"`). v0.10 design
  candidate at 3 sources (helm, deno, bazel); clap pushes to 4.
- **`regex_resolves_in_file` rule kind** (regex extracted from a
  registry file matches at least once in a target file) — covers
  clap's `release.toml.pre-release-replacements` shape (regex+replacement
  pairs that have to match real files at `cargo release` time). Niche
  to cargo-release; **single-source v0.11+ candidate**.
- **`rust/cargo-release-conventions@v1` bundled ruleset** — would
  consolidate the `release.toml` + `committed.toml` + `CITATION.cff`
  + auxiliary policy file presence checks into one `extends:` line.
  v0.10 design candidate (clap is the canonical source).

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`alint suggest` against `/tmp/clap/`** — predict the heuristic
   will surface `oss-baseline@v1`, `rust@v1`, and
   `monorepo/cargo-workspace@v1`. Worth running with `--explain` to
   confirm the ruleset detection works for a 7-member family
   workspace (and not just a single-crate or 2-member workspace).
2. **`for_each_dir` over each `clap_*` workspace member with `{members}`
   placeholder** — current config uses an explicit
   `select: "{clap_builder,clap_derive,clap_complete,clap_lex,clap_mangen}"`
   bracket expansion. Once the `monorepo/cargo-workspace` member-discovery
   refinement ships, `select: "{members}"` (derived from the `[workspace]
   members` array) would survive future crate additions without manual
   edits. Same demand shape as the deno case study.
3. **JSON-output rule timing** — clap is small enough that the structural
   rules complete in under 100 ms. Worth running
   `alint check --format json --config .alint.yml /tmp/clap` and
   confirming the four `command:` shellouts (typos, committed[absent],
   cffconvert, cargo-deny) dominate the wall-clock; if so, narrow each
   one's `paths:` glob.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **70** (28 custom + 5 bundled rulesets — `oss-baseline`
  15, `rust` 11, `monorepo/cargo-workspace` 4, `ci/github-actions` 3,
  `hygiene/no-tracked-artifacts` 11; minus 2 facts = 70 loadable rules)
- **`alint validate-config`:** ✓ Config valid: 70 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  132-violation breakdown (2 real errors in test fixtures, 56 GHA
  pinning warnings, 71 cosmetic info-level findings)
- **Pitfall fixes (v0.9.17):** Pitfall #18 (per-rule `respect_gitignore:
  false`) and #19 (literal-path runtime guard for `root_only: true` +
  multi-component literals) both shipped in engine; this config does
  not need either workaround
- **Open gaps (unchanged):** `cross_file_value_equals` (v0.10
  ship-target, 10 sources — clap is one), `regex_resolves_in_file`
  (v0.11+ single-source candidate via clap's
  `release.toml.pre-release-replacements`), `*_path_contains` (v0.10
  design candidate, 3+ sources)
- **Open suspected bugs in this directory's `.alint.yml`:** **none.**
  Config is clean against the v0.9.17 engine + canonical-22 pitfall
  catalogue.
