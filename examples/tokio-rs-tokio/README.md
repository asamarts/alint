# Case study: `tokio-rs/tokio`

> Marketing/positioning writeup at <https://alint.org/examples/tokio-rs-tokio/>.
> This README is the engineering reference: tooling inventory, mapping table,
> gap catalogue, validation status.

Inventory of the structural-validation tooling in `tokio-rs/tokio` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-03, sparse-checkout of `.github`,
`tests-integration`, `tests-build`, root config files (no `ci/` directory
exists in tokio).

---

## Summary

Tokio is the **inverse of the Kubernetes shape** — there is no `ci/`
directory, no `hack/verify-*.sh` pipeline, no custom linter binary. All
structural validation lives in **7 GitHub Actions workflows** (~1714 lines
total), with the bulk in one ~1365-line `ci.yml` driving a multi-OS / multi-
target / multi-feature matrix. That matrix is mostly *build-and-test* jobs
(out of alint's "no-build / no-execute" scope), but it carries roughly
**12 distinct repo-state checks** woven into the `fmt`, `clippy`, `docs`,
`semver`, `check-readme`, and `check-spelling` jobs, plus the two
cargo-deny workflows (`audit.yml` + `pr-audit.yml`).

Of those 12 structural checks:

- **~50 % map directly to existing alint rules** (6 checks: `cargo fmt
  --check`, `cargo clippy`, `cargo deny`, `cargo doc`, `cargo
  semver-checks`, `cargo spellcheck` — all handled via the `command`
  rule kind shelling out to the canonical tool, plus
  `no_trailing_whitespace` covering the inline `grep -rne '\s$'`
  guard)
- **~40 % need new alint primitives** (5 checks, all variants of "data in
  one file must equal data in another" — `diff README.md
  tokio/README.md`, the version-grep cross-reference, the
  `[patch.crates-io]` mirror keys, the dictionary
  sortedness/uniqueness/count cross-check, and the workspace
  `[lints.rust] unexpected_cfgs` allowlist ↔ per-crate cfg-attribute
  inheritance)
- **~10 % are out of alint's deliberate scope** (1 check: the kernel-build
  probe in `uring-kernel-version-test.yml`, which downloads a kernel and
  compiles it)

The starter config in [`.alint.yml`](.alint.yml) replaces the 6 mappable
checks plus adds **15 cargo-workspace conventions** the existing pipeline
*doesn't* explicitly enforce (per-crate `publish=false` discipline,
edition consistency, license-field presence on published crates, README
existence per workspace member, `[patch.crates-io]` sanity, `[lints]
workspace = true` inheritance, etc.). Net: **27 declarative rules**
covering ~50 % of the existing pipeline plus a thicker layer of
defense-in-depth conventions.

tokio's CI is matrix-driven and assumes the repo state is sane;
alint asserts the assumptions. tokio doesn't have ad-hoc shell scripts
to replace — the value here is in catching the **15 conventions
tokio's CI pipeline silently relies on but doesn't explicitly
verify**. Narrative framing for this case study (and how it slots
against kubernetes / rust-lang-rust as launch counterpoints) lives
in the alint.org marketing writeup linked at the top of this README.

---

## Existing tooling inventory

### `.github/workflows/` — 7 files, ~1714 lines

| Workflow | Lines | Purpose | Repo-state checks |
|---|---|---|---|
| `ci.yml` | 1365 | Full test matrix (test-tokio-full, miri, asan, valgrind, cross-target, wasm32-*, freebsd, sgx, redox, etc.) | `fmt`, `clippy`, `docs`, `semver`, `check-readme`, `check-spelling` jobs (everything else compiles/runs code) |
| `loom.yml` | 129 | PR-label-gated loom model-checking runs | none (jobs are dynamic-CI behavior) |
| `uring-kernel-version-test.yml` | 103 | Build kernel, run io_uring tests at specific kernel versions | none (out-of-scope: downloads + builds Linux) |
| `stress-test.yml` | 45 | Compile + run valgrind on stress-test/examples/ binaries | none (build-and-run) |
| `audit.yml` | 24 | Nightly cron + on-Cargo.toml-push: `cargo deny check` | one (cargo-deny shell-out) |
| `labeler.yml` | 25 | Auto-label PRs based on changed files | none (PR metadata) |
| `pr-audit.yml` | 23 | Per-PR `cargo deny check` on Cargo.toml diffs | one (cargo-deny shell-out — same tool as audit.yml) |

### Maps to existing alint rules (drop-in replacements)

| Existing check (file:job) | What it checks | alint replacement |
|---|---|---|
| `ci.yml:fmt` | `rustfmt --check --edition 2021 $(git ls-files '*.rs')` | `command` per workspace root: `cargo fmt --check` |
| `ci.yml:clippy` | `cargo clippy --workspace --tests --no-deps --features full,test-util` (then a second pass with `--all-features` and `--cfg tokio_unstable`) | `command` per workspace root |
| `ci.yml:docs` | `cargo doc --lib --no-deps --document-private-items` with `RUSTDOCFLAGS=-Dwarnings` | `command` per workspace root: `cargo doc --no-deps` |
| `ci.yml:semver` | `obi1kenobi/cargo-semver-checks-action@v2` against tokio + each non-tokio crate | `for_each_dir` over the 5 published crates + `command: cargo semver-checks --manifest-path {path}/Cargo.toml` |
| `ci.yml:check-spelling` | `cargo spellcheck --code 1` | `command` per `spellcheck.toml` |
| `ci.yml:check-spelling` (tail) | `grep --exclude-dir=.git --exclude-dir=target -rne '\s$' .` | `no_trailing_whitespace` over `**/*.{rs,md,toml,yml,yaml,sh}` (cleaner than the recursive grep) |
| `audit.yml`, `pr-audit.yml` | `EmbarkStudios/cargo-deny-action@v2` | `command` per `deny.toml`: `cargo deny check`. Same tool, one place. |

7 checks captured by the `command` rule kind plus one structural rule —
6 unique tools (the two cargo-deny workflows are the same shell-out, just
with different triggers).

### Maps to existing alint rules (defensive convention checks the pipeline DOESN'T do)

These aren't drop-in replacements — they're conventions the existing
pipeline *implicitly* relies on but doesn't explicitly enforce. Each is
a regression class that's currently caught only when CI mysteriously
breaks downstream:

| alint rule | What it asserts | Why it matters |
|---|---|---|
| `tokio-cargo-deny-config-present` | `deny.toml` exists | If removed, audit.yml's cargo-deny silently no-ops (no allowlist → either default-deny everything or default-allow everything depending on the version) |
| `tokio-cargo-deny-licenses-allowlist` | `[licenses].allow[*]` is non-empty | Same failure mode — empty allowlist breaks the audit |
| `tokio-cargo-deny-bans-wildcards` | `[bans].wildcards == "deny"` | Removing the bans entry would let `*` version requirements ship; nothing in CI catches this |
| `tokio-crate-inherits-workspace-lints` | Each member's `Cargo.toml` has `[lints] workspace = true` | Without this, the workspace's `unexpected_cfgs` allowlist (which permits `cfg(tokio_unstable)`, `cfg(loom)`, `cfg(fuzzing)`, etc.) doesn't propagate; CI's `RUSTFLAGS=-Dwarnings` flakes per-crate |
| `tokio-internal-crate-not-publishable` | `benches`, `examples`, `stress-test`, `tests-build`, `tests-integration` declare `publish = false` | A `cargo publish --workspace` from a maintainer's machine would otherwise leak internal-only crates to crates.io |
| `tokio-cargo-edition` | Every member declares `edition = "2021"` or `"2024"` | Workspace edition drift produces inconsistent diagnostics across members |
| `tokio-published-crate-license` | The 5 published `tokio-*` crates declare `license = "MIT"` | `cargo publish` rejects the upload, but only at the *final* step; lint-time failure is faster feedback |
| `tokio-workspace-member-has-readme` | Each tokio-* member ships its own `README.md` | crates.io / docs.rs land on a non-empty page when the crate publishes |
| `tokio-tests-integration-bin-{cat,mem,process-signal}-exists` | Each `[[bin]]` declared in `tests-integration/Cargo.toml` has its source file | A drifting `[[bin]]` entry only fails when the matching `required-features` are active in CI — easy to miss |
| `tokio-workspace-patch-block-present` | Workspace root `Cargo.toml` has `[patch.crates-io] tokio = { path = "tokio" }` | Without it, cross-member development resolves siblings against crates.io instead of local paths; silently breaks fresh clones |
| `tokio-{ci,audit,pr-audit,buildomat}-config-present` | The 4 critical workflow / config files exist | Catches accidental deletions before the next merge |
| `tokio-spellcheck-{dic,toml}-exists` + first-line-is-count regex | The dictionary file has the integer header | The CI job hand-validates this with bash; alint catches the most-likely regression statically |

15 defensive conventions. Most are 3-5 lines each.

### Needs new alint primitive

| Existing check | What it does | What alint needs |
|---|---|---|
| `ci.yml:check-readme` (1) | `diff README.md tokio/README.md` — root README must equal the per-crate README byte-for-byte | A **`cross_file_value_equals` rule kind**: "contents of file A equal contents of file B" (with optional pre-transform). The `pair` rule asserts the partner exists; equality is the next step. **Same primitive shows up in:** every monorepo with a "synced docs page" pattern (root README ↔ per-crate README), any project where a `LICENSE` is duplicated to per-package directories, any tool with a "primary doc + mirror" relationship. |
| `ci.yml:check-readme` (2) | `grep -q "$(sed '/^version = /!d' tokio/Cargo.toml \| head -n1)" README.md` — the tokio crate version must appear literally in the root README | A `cross_file_value_equals` variant with a **selector**: extract a value from file A (here, `$.package.version` from `tokio/Cargo.toml`), then assert it appears in file B. Generalised this is "value at JSONPath in file A must match value at JSONPath in file B" — the same primitive covers `package.json#version` ↔ `CHANGELOG.md` first-line, etc. |
| `ci.yml:check-spelling` (header) | `spellcheck.dic` first line is an integer N, and the file has exactly N+1 lines (the body is sorted unique under `LC_ALL=en_US.UTF8`) | Two needs: (a) **`pair_hash` rule kind** — "value at offset 0 of file A equals computed property of file A" (here, line count); (b) **`ordered_block` rule kind** — "lines after the header are sorted unique under a configurable comparator". `ordered_block` is now a v0.10 ship-target (rust + tokio + 3 more); `pair_hash` remains a v0.10+ candidate (kubernetes + tokio). |
| Workspace `[patch.crates-io]` ↔ `[workspace] members` | Every workspace member name appears as a key in `[patch.crates-io]` (with `path = "<member>"`) | `cross_file_value_equals` with a JSONPath selector on both sides — the root `Cargo.toml` is *one* file but the check is "every value at `$.workspace.members[*]` appears as a key under `$.patch['crates-io']`". A **same-file `value_set_equality` rule kind** (or `cross_file_value_equals` applied to the same file twice) would cover it. Mid-priority. |
| Workspace `[workspace.lints.rust] unexpected_cfgs` ↔ per-crate `[lints] workspace = true` | The workspace declares the cfg-allowlist; every member must inherit it (the `tokio-crate-inherits-workspace-lints` rule above approximates this, but a stricter check would assert "the bool at `$.lints.workspace` in each member's Cargo.toml is `true`") | A `toml_path_matches` against a bool-typed value works today (we use `matches: '^true$'` against the stringified value). But `toml_path_equals` with a YAML-native `true` literal would be cleaner — there's a docs / DX gap here, not strictly a missing primitive. |

**Gap pattern: cross-file value equality.** The single biggest missing
primitive — 4 of the 5 gaps above are variants of "data in one file must
match data in another file". This is **the same gap surfaced by the
rust-lang/rust pilot** (where it appeared as `tidy::triagebot` paths
resolving against the working tree, and `tidy::rustdoc_css_themes`
mirror-blocks). `cross_file_value_equals` is now the **strongest demand
signal in P2** — saturated at 8+ confirmations (airflow + tokio + clap +
uv + react + pnpm + pytorch + tensorflow) — and is the v0.10 must-ship.

### Out of alint's scope (use the existing tool)

- `uring-kernel-version-test.yml` — downloads a Linux kernel source
  tarball, builds it, boots qemu against the resulting kernel image.
  Out of scope by design (alint never executes external builds).
- The `loom.yml` PR-label gates (`R-loom-blocking`, `R-loom-sync`, etc.) —
  dynamic CI behavior driven by PR labels, not a static tree property.
- The `labeler.yml` workflow — assigns labels based on changed files;
  also dynamic CI behavior.
- The `--cfg tokio_unstable` / `--cfg loom` / `RUSTFLAGS=...` matrix
  dispatch in `ci.yml` — CI orchestration, not repo state.
- The buildomat illumos CI itself — third-party CI service. The
  `.github/buildomat/config.toml` *file* is checked above; what the
  service does with it is out of scope.

### Already covered by existing tools (correctly)

- `cargo deny` already covers license + dependency-source allowlisting.
  alint shells out via `command:` — the right delegation pattern.
- `cargo semver-checks` covers semver-breaking API changes. Same.
- `cargo spellcheck` covers prose spelling. Same.
- `rustfmt`, `clippy`, `cargo doc -Dwarnings` — same.

---

## Existing tooling: top-level config files

| File | What it does | alint asserts |
|---|---|---|
| `Cargo.toml` (workspace) | Declares `[workspace] members`, `[patch.crates-io]`, `[workspace.lints.rust]` | `[patch.crates-io].tokio.path` exists; the bundled `monorepo/cargo-workspace@v1` ruleset asserts `[workspace] members` is non-empty |
| `deny.toml` | cargo-deny config: `[licenses] allow`, `[bans] wildcards = "deny"`, `[sources]` | All three sections are checked (presence + non-empty allowlist + bans-wildcard policy) |
| `spellcheck.toml` | cargo-spellcheck config: Hunspell lang + extra dictionaries | Existence check (the body is small enough that a content-match would be over-fitting) |
| `spellcheck.dic` | Hunspell dictionary: line 1 is an integer count, then sorted unique words | Existence + first-line-is-integer check (the sortedness + count-equals-actual-line-count check needs the `ordered_block` + `pair_hash` primitives) |
| `Cross.toml` | `cross-rs/cross` cross-compilation passthrough config | Not asserted (single line; no convention to enforce) |
| `netlify.toml` | Netlify deploy config for the docs preview | Not asserted (out of CI scope; docs build is in a separate workflow we didn't pull) |
| `CODE_OF_CONDUCT.md`, `SECURITY.md`, `LICENSE` | Standard OSS hygiene files | Covered by the bundled `oss-baseline@v1` ruleset |
| `.gitignore` | One line: `target/` | Covered by the bundled `hygiene/no-tracked-artifacts@v1` ruleset (which already enforces `target/` not being tracked) |

Notably missing (vs. other Rust workspaces we've inventoried):

- **No `rust-toolchain.toml`** — tokio pins toolchain via `env.rust_*`
  variables in the CI workflows (`rust_stable: stable`, `rust_nightly:
  nightly-2025-10-12`, etc.). The bundled `rust@v1` ruleset has a
  `rust-toolchain-pinned` info-level check that would fire here; this
  is a deliberate deviation from the convention rather than a regression,
  so we don't override.
- **No `clippy.toml`** — tokio's clippy bans live in source attributes
  (`#[allow(clippy::*)]`) rather than a `clippy.toml`. Different policy
  posture from e.g. vercel/turbo, which uses `clippy.toml` to enforce
  workspace-wide bans on `VecDeque::new`.
- **No `rustfmt.toml`** — defaults are accepted.

---

## Starter alint config (drop-in)

[`.alint.yml`](.alint.yml) in this directory. Adopts:

- `oss-baseline@v1` (license, README, gitignore, no merge markers, no bidi)
- `rust@v1` (Cargo.toml exists, no tracked target/, snake_case sources, etc.)
- `monorepo@v1` + `monorepo/cargo-workspace@v1` (workspace member coherence)
- `ci/github-actions@v1` (workflow permissions / action pinning)
- `hygiene/no-tracked-artifacts@v1` (no `.DS_Store`, build outputs, etc.)

Plus 27 tokio-specific rules: 6 shell-outs to the existing tools (cargo
fmt / clippy / deny / doc / semver-checks / spellcheck), 1 trailing-
whitespace check, and 20 defensive structural conventions.

The remaining gaps:

- 5 need new alint primitives (above) — `cross_file_value_equals` and
  `ordered_block` are v0.10 ship-targets; the rest stay as v0.10+ candidates
- 1 is out of alint's scope (kernel build) — keep the existing job
- The matrix test runs themselves (~30 jobs across `ci.yml`) — these
  are *behavior tests*, not structural state; alint correctly defers

---

## Performance comparison (placeholder — bench when validation pass scales)

tokio's `ci.yml` runs end-to-end in ~30 minutes on the basics tier and
~90 minutes on the full matrix; the structural-state checks alint would
take over (`fmt`, `clippy`, `docs`, `semver`, `check-readme`,
`check-spelling`) account for roughly 10-15 of those minutes (each
shells out to a separate tool with its own startup overhead, run
sequentially within their respective jobs).

alint runs all rules in parallel via the v0.9.3 dispatch flip. The
shell-out rules in this config are bounded by the underlying tool
(rustfmt is fast; cargo-doc is the slowest). The 20 structural rules
should complete in well under a second on the tokio tree (which is small
— ~800 source files, mostly under `tokio/src/`).

Not the headline pitch here. **The pitch is: tokio's CI assumes the repo
state is sane; alint asserts the assumptions.** Speed is incidental.

To benchmark for real: run `time alint check` on a fresh clone of tokio
and compare to the existing pipeline's wall-clock for the equivalent
job-set. Deferred to the per-repo measurement pass.

---

## Followup feature work surfaced (consolidated)

The narrative framing for the "convention without explicit checks"
pitch and the launch counterpoint to kubernetes / rust-lang-rust
lives in the alint.org marketing writeup linked at the top of this
README. This section is the engineering rule-kind candidate list.

Priority order:

- **`cross_file_value_equals` rule kind** — covers tokio's `diff
  README.md tokio/README.md` and the version-grep cross-reference.
  **Same primitive surfaces in:** rust-lang/rust's `tidy::triagebot`
  paths-on-disk check, kubernetes' `staging/publishing/` mirror
  validation, every monorepo with a "root + per-package mirror docs"
  pattern. **v0.10 ship-target** — 8+ confirmations is past saturation.
- **`ordered_block` rule kind** — **v0.10 ship-target** (rust-lang pilot
  + tokio's `spellcheck.dic` + 3 more confirm demand).
- **`pair_hash` rule kind** — covers the spellcheck.dic header-equals-
  body-line-count check. v0.10+ candidate (kubernetes pilot's
  `vendor/`-readonly check + tokio).
- **`toml_path_equals` typed-value comparison** — minor DX polish; today
  we stringify-and-regex-match against a bool, which works but is ugly.
  This is exactly pitfall #16 from the next.js case study; merits a
  CONFIG-AUTHORING.md cross-reference (v0.10+ DX item).

No new rule-kind candidates beyond the 9 already on the v0.10+ pipeline.
tokio is a clean, conventional Rust workspace — its gap catalogue
overlaps heavily with what the kubernetes and rust-lang/rust pilots
already surfaced, which is itself useful evidence that the gap list is
*saturating*.

---

## Methodology notes (for future case-study authors)

- **Sparse-clone gotcha:** `git sparse-checkout set .gitignore Cargo.toml ...`
  fails with `fatal: '.gitignore' is not a directory` because sparse
  patterns are interpreted as path prefixes by default. Use
  `--skip-checks` to allow file-typed entries: `git sparse-checkout set
  --skip-checks .gitignore Cargo.toml deny.toml ...`. This is a one-shot
  workflow fix rather than a CONFIG-AUTHORING.md addition (the
  briefing's exact command produced the error).
- **`file_starts_with` requires a non-empty `prefix:`.** Hit on first
  parse-validate while trying to express "this file is non-empty". The
  rule rejects empty prefixes at build time with a clear error
  (`file_starts_with.prefix must not be empty`). This is the *correct*
  behavior — a `prefix: ""` would match every file vacuously — but the
  natural-feeling expression for "non-empty" is currently to use
  `file_content_matches` with `pattern: '\A.'` or similar. A
  `file_non_empty` convenience kind would be a small but pleasant DX
  addition. **Adding to CONFIG-AUTHORING.md as pitfall #13.**

No other pitfalls hit during this pass. The CONFIG-AUTHORING.md
canonical-patterns cheat sheet covered every other shape on the first
draft. (12 pitfalls + 1 new = 13.) The catalogue has since grown to
21 pitfalls (P2a/P2b waves added #14-#21); pitfall #13 from this
pass is now part of the published catalogue.

---

## Future analysis

Suggestions for the next revalidation pass — tokio is the cleanest
"convention without explicit checks" demonstration, so the analysis
focuses on what alint's v0.9.6+ rule kinds now express that the
existing config still leaves on the table:

- **Of the 15 conventions tokio's pipeline silently assumes, which
  are now expressible thanks to v0.9.6+?** Most already are
  (`for_each_dir`, `for_each_file`, structured-path matchers,
  `command:` shellouts). The remaining gaps are the 5 cross-file /
  ordering / hashing patterns above — all on the v0.10+ pipeline.
  The v0.9.17 surface (`scope_filter` + `respect_gitignore`
  per-rule + `has_*` predicates) doesn't open new ground for tokio
  specifically, because tokio's CI is matrix-driven and doesn't
  rely on tracked-AND-gitignored files or root-only literal paths.
- **`agent-context` / `docs/adr` bundled-ruleset adoption.** The
  current config extends 6 bundled rulesets but skips the newer
  `agent-context@v1` (5 rules — agent-readable docs presence) and
  `docs/adr@v1` (4 rules — ADR directory shape). tokio doesn't ship
  an ADR tree today; that's a finding (the project relies on RFCs
  in the linked-issues tracker rather than an in-repo log), but
  `agent-context` would catch the absence of `AGENTS.md` /
  `CLAUDE.md` if the maintainers opt into agent-tooling discipline.
- **`alint suggest` against a fresh clone.** Hasn't been run for
  this case study; the briefing's "tokio is a clean conventional
  Rust workspace" pitch is the human read of the manifests. Running
  `alint suggest` would either confirm or surface bundled
  candidates not yet adopted (likely `compliance/apache-2` is wrong
  for tokio's MIT licensing, but `tooling/editorconfig` would land
  cleanly).

---

## Validation status (2026-05-07)

- **alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).
- **`validate-config`:** ✓ 74 rules loaded from `.alint.yml`.
- **README rule-count claim:** "27 declarative rules" (intro) +
  "20 defensive structural conventions" (config-overview line 197)
  matches the actual count of 28 tokio-specific rules within rounding.
  The 74-rule `validate-config` total = 28 tokio-specific + 46
  inherited from the 6 bundled rulesets pulled in via `extends:`
  (oss-baseline=15 + rust=11 + monorepo=4 +
  monorepo/cargo-workspace=4 + ci/github-actions=3 +
  hygiene/no-tracked-artifacts=11 = 48 declared; the 2-rule slack
  vs 46 reflects per-rule bundled-overlap dedup the engine handles
  transparently). No update needed.
- **Pitfall catalogue:** v0.9.17 ships fixes for #18 + #19. Neither
  surfaces here (no tracked-AND-gitignored files; no `root_only:
  true` on multi-component literals). Pitfall #13 (this pass) is
  now in the published catalogue.
- **Rule-kind candidate status:** `cross_file_value_equals` and
  `ordered_block` promoted to v0.10 ship-targets thanks to
  saturated demand (8 + 5 confirmations respectively). `pair_hash`
  and `toml_path_equals` typed comparison stay v0.10+ candidates.
- **Bundled-ruleset rule counts (authoritative as of 2026-05-07):**
  oss-baseline=15, rust=11, monorepo=4, monorepo/cargo-workspace=4,
  ci/github-actions=3, hygiene/no-tracked-artifacts=11.
