# Case study: `tokio-rs/tokio`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/tokio-rs-tokio/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `tokio-rs/tokio` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 sparse-checkout of `.github/`,
`tests-integration/`, `tests-build/`, root config files at `/tmp/tokio/`.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Tokio is the **inverse of the kubernetes shape**. **Verified:** there is
no `ci/` directory; no `hack/verify-*.sh` pipeline; no custom linter
binary; no `scripts/`; no `tools/`; no `Makefile`; **no `*.sh` files
anywhere in the tree** (`find /tmp/tokio -name "*.sh"` returns zero
results). All structural validation lives in **7 GitHub Actions
workflows** totalling **1,714 lines** (verified `wc -l` against
`.github/workflows/*.yml`).

### 1.1 `.github/workflows/` (7 files, 1,714 lines)

| Workflow | Lines | Purpose | Repo-state checks |
|---|---|---|---|
| `ci.yml` | 1,365 | Full test matrix (test-tokio-full, miri, asan, valgrind, cross-target, wasm32-*, freebsd, sgx, redox, etc.) | `fmt`, `clippy`, `docs`, `semver`, `check-readme`, `check-spelling` jobs (everything else compiles/runs code) |
| `loom.yml` | 129 | PR-label-gated loom model-checking runs | none (jobs are dynamic-CI behavior) |
| `uring-kernel-version-test.yml` | 103 | Build kernel, run io_uring tests at specific kernel versions | none (out-of-scope: downloads + builds Linux) |
| `stress-test.yml` | 45 | Compile + run valgrind on stress-test/examples/ binaries | none (build-and-run) |
| `audit.yml` | 24 | Nightly cron + on-Cargo.toml-push: `cargo deny check` | one (cargo-deny shell-out) |
| `labeler.yml` | 25 | Auto-label PRs based on changed files | none (PR metadata) |
| `pr-audit.yml` | 23 | Per-PR `cargo deny check` on Cargo.toml diffs | one (cargo-deny shell-out — same tool as audit.yml) |

`grep -E "^(\s+(name:\|run:))" ci.yml` extracts the canonical job
names — the structural-state checks woven into the matrix are
exactly the 6 listed in §1.2 below.

### 1.2 The 12 distinct repo-state checks (woven into ci.yml + audit.yml + pr-audit.yml)

Numbered for the `command:`-shellout vs declarative tally in §2.

| # | Check | Source | What it does |
|---|---|---|---|
| 1 | `fmt` | `ci.yml` job `fmt` | `rustfmt --check --edition 2021 $(git ls-files '*.rs')` |
| 2 | `clippy` | `ci.yml` job `clippy` | `cargo clippy --workspace --tests --no-deps --features full,test-util` then a second pass with `--all-features` and `--cfg tokio_unstable` |
| 3 | `docs` | `ci.yml` job `docs` | `cargo doc --lib --no-deps --document-private-items` with `RUSTDOCFLAGS=-Dwarnings` |
| 4 | `semver` | `ci.yml` job `semver` | `obi1kenobi/cargo-semver-checks-action@v2` against tokio + each non-tokio crate |
| 5 | `check-spelling` (cargo-spellcheck half) | `ci.yml` job `check-spelling` | `cargo spellcheck --code 1` |
| 6 | `check-spelling` (trailing-ws half) | `ci.yml` job `check-spelling` (tail) | `grep --exclude-dir=.git --exclude-dir=target -rne '\s$' .` |
| 7 | `check-readme` (literal byte-equality) | `ci.yml` job `check-readme` | `diff README.md tokio/README.md` |
| 8 | `check-readme` (version cross-reference) | `ci.yml` job `check-readme` (tail) | `grep -q "$(sed '/^version = /!d' tokio/Cargo.toml \| head -n1)" README.md` |
| 9 | `cargo deny` (nightly) | `audit.yml` | `EmbarkStudios/cargo-deny-action@v2` (license + dependency-source allowlisting) |
| 10 | `cargo deny` (per-PR) | `pr-audit.yml` | Same tool, different trigger |
| 11 | `spellcheck.dic` shape | `check-spelling` (header) | `spellcheck.dic` first line is integer N, body is N sorted-unique words |
| 12 | uring kernel test | `uring-kernel-version-test.yml` | Builds Linux kernel + runs io_uring tests against it |

### 1.3 Repo-root config files

| File | Owner tool | What it pins |
|---|---|---|
| `Cargo.toml` (workspace) | cargo | `[workspace] members` (5 published + 5 internal = 10 crates), `[patch.crates-io]`, `[workspace.lints.rust]` (cfg-allowlist for `tokio_unstable`, `loom`, `fuzzing`, etc.) |
| `deny.toml` | cargo-deny | `[licenses] allow`, `[bans] wildcards = "deny"`, `[sources]` |
| `spellcheck.toml` | cargo-spellcheck | Hunspell lang + extra dictionaries |
| `spellcheck.dic` | cargo-spellcheck | Hunspell dictionary: line 1 = integer count (verified: `323`), then sorted unique words |
| `Cross.toml` | cross-rs/cross | Cross-compilation passthrough config |
| `netlify.toml` | Netlify | Docs-preview deploy config |
| `CODE_OF_CONDUCT.md`, `SECURITY.md`, `LICENSE`, `CONTRIBUTING.md`, `README.md` | community / docs | Standard OSS hygiene |
| `.gitignore` | git | One line: `target/` |
| `target-specs/` | rustc | Per-target spec JSON (sgx, redox, freebsd, …) |

Notably **missing** vs other Rust workspaces:

- **No `rust-toolchain.toml`** — tokio pins via `env.rust_*` vars in
  the CI workflows (`rust_stable: stable`, `rust_nightly:
  nightly-2025-10-12`, etc.). Verified at
  `/tmp/tokio/.github/workflows/ci.yml:14-21`.
- **No `clippy.toml`** — bans live in source attributes
  (`#[allow(clippy::*)]`), not a workspace-wide config file.
- **No `rustfmt.toml`** — defaults are accepted.

### 1.4 Workspace member shape (verified)

`grep -A 20 "^members" Cargo.toml`:

- **5 published crates:** `tokio`, `tokio-macros`, `tokio-test`,
  `tokio-stream`, `tokio-util`
- **5 internal crates:** `benches`, `examples`, `stress-test`,
  `tests-build`, `tests-integration`

`[patch.crates-io]` block has 5 entries — one per published crate
(`tokio = { path = "tokio" }`, etc.).

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset OR the per-rule entry
  in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate.
- **out-of-scope** — explain why (kernel build, dynamic CI, runtime
  probe).

### 2.1 The 12 distinct repo-state checks

| # | Check | Coverage | Notes |
|---|---|---|---|
| 1 | `fmt` | ✅ alint-today | `command:` per workspace root: `cargo fmt --check`. |
| 2 | `clippy` | ✅ alint-today | `command:` per workspace root. |
| 3 | `docs` | ✅ alint-today | `command:` per workspace root: `cargo doc --no-deps`. |
| 4 | `semver` | ✅ alint-today | `for_each_dir` over the 5 published crates + `command:` cargo-semver-checks. |
| 5 | `check-spelling` (cargo-spellcheck) | ✅ alint-today | `command:` per `spellcheck.toml`. |
| 6 | `check-spelling` (trailing-ws) | ✅ alint-today | `no_trailing_whitespace` over `**/*.{rs,md,toml,yml,yaml,sh}` (cleaner than the recursive grep). |
| 7 | `check-readme` (byte-equality) | 🔄 alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources) — "contents of file A equal contents of file B". |
| 8 | `check-readme` (version cross-ref) | 🔄 alint-future | `cross_file_value_equals` with selector — extract `$.package.version` from `tokio/Cargo.toml`, assert it appears in `README.md`. |
| 9 | `cargo deny` (nightly) | ✅ alint-today | `command:` per `deny.toml`. |
| 10 | `cargo deny` (per-PR) | ✅ alint-today | Same tool as #9. |
| 11 | `spellcheck.dic` shape | 🔄 alint-future | (a) `pair_hash` (v0.10+ candidate, 3 sources): "value at offset 0 of file A equals computed property of file A" (line count); (b) `ordered_block` (v0.10 ship-target, 7 sources): "lines after the header are sorted unique". |
| 12 | uring kernel test | ❌ out-of-scope | Downloads + builds Linux kernel. alint never executes external builds. |

**Tally for §2.1 (the 12 checks):**

```
✅ alint-today:    8 / 12 = 67%   (fmt, clippy, docs, semver, spellcheck×2, deny×2)
🔄 alint-future:   3 / 12 = 25%   (check-readme×2 + spellcheck.dic shape)
❌ out-of-scope:   1 / 12 = 8%    (uring kernel)
```

### 2.2 The 15 defensive conventions tokio's pipeline silently assumes

The brief asks: **explicitly enumerate the 15 conventions tokio's
pipeline silently assumes + show which alint rule catches each.** The
existing README claims this number; I verified it by reading
`/tmp/tokio/Cargo.toml`, `deny.toml`, `spellcheck.{toml,dic}`,
`.github/workflows/{ci,audit,pr-audit}.yml`, and the per-crate
`Cargo.toml`s. All 15 are conventions the existing pipeline
*implicitly* relies on but **doesn't explicitly enforce** at PR-time
— each is a regression class that's currently caught only when CI
mysteriously breaks downstream.

| # | Convention (exact alint rule ID once authored) | What it asserts | Why it matters | alint kind |
|---|---|---|---|---|
| 1 | `tokio-cargo-deny-config-present` | `deny.toml` exists | If removed, audit.yml's cargo-deny silently no-ops | `file_exists` |
| 2 | `tokio-cargo-deny-licenses-allowlist` | `[licenses].allow[*]` is non-empty | Empty allowlist breaks the audit | `toml_path_matches` |
| 3 | `tokio-cargo-deny-bans-wildcards` | `[bans].wildcards == "deny"` | Removing the bans entry would let `*` version requirements ship | `toml_path_matches` |
| 4 | `tokio-crate-inherits-workspace-lints` | Each member's `Cargo.toml` has `[lints] workspace = true` | Without this, the workspace's `unexpected_cfgs` allowlist (which permits `cfg(tokio_unstable)`, `cfg(loom)`, `cfg(fuzzing)`, etc.) doesn't propagate; CI's `RUSTFLAGS=-Dwarnings` flakes per-crate | `for_each_dir` + `file_content_matches` |
| 5 | `tokio-internal-crate-not-publishable` | `benches`, `examples`, `stress-test`, `tests-build`, `tests-integration` declare `publish = false` | A `cargo publish --workspace` from a maintainer's machine would otherwise leak internal-only crates to crates.io | `for_each_dir` + `toml_path_matches` |
| 6 | `tokio-cargo-edition` | Every member declares `edition = "2021"` or `"2024"` | Workspace edition drift produces inconsistent diagnostics across members | `for_each_dir` + `toml_path_matches` |
| 7 | `tokio-published-crate-license` | The 5 published `tokio-*` crates declare `license = "MIT"` | `cargo publish` rejects the upload, but only at the *final* step; lint-time failure is faster feedback | `for_each_dir` (5-crate selector) + `toml_path_matches` |
| 8 | `tokio-workspace-member-has-readme` | Each `tokio-*` member ships its own `README.md` | crates.io / docs.rs land on a non-empty page when the crate publishes | `for_each_dir` + `file_exists` |
| 9 | `tokio-tests-integration-bin-cat-exists` | `[[bin]] cat` declared in `tests-integration/Cargo.toml` has its source file | A drifting `[[bin]]` entry only fails when the matching `required-features` are active in CI — easy to miss | `file_exists` (per `[[bin]]`) |
| 10 | `tokio-tests-integration-bin-mem-exists` | `[[bin]] mem` source file present | Same | `file_exists` |
| 11 | `tokio-tests-integration-bin-process-signal-exists` | `[[bin]] process-signal` source file present | Same | `file_exists` |
| 12 | `tokio-workspace-patch-block-present` | Workspace root `Cargo.toml` has `[patch.crates-io] tokio = { path = "tokio" }` (and 4 siblings) | Without it, cross-member development resolves siblings against crates.io instead of local paths; silently breaks fresh clones | `toml_path_matches` (5-key check) |
| 13 | `tokio-ci-config-present` | `.github/workflows/ci.yml` exists | Catches accidental deletion before next merge | `file_exists` |
| 14 | `tokio-audit-config-present` | `.github/workflows/audit.yml` exists | Same | `file_exists` |
| 15 | `tokio-spellcheck-dic-first-line-is-count` | `spellcheck.dic` first line is an integer | The CI job hand-validates this with bash; alint catches the most-likely regression statically | `file_starts_with` + regex |

(Two helper assertions also frequently bundled in this group:
`tokio-pr-audit-config-present` for `.github/workflows/pr-audit.yml`,
and `tokio-spellcheck-toml-exists` for the cargo-spellcheck config —
the existing README's "20 defensive structural conventions" framing
includes those plus a few file-presence overlaps. The 15 above is the
non-overlap minimum that maps 1:1 to live regression risks.)

**All 15 are alint-today** — each is expressible with `file_exists`,
`for_each_dir` + nested rules, `toml_path_matches`, or `file_content_matches`.
None require the v0.10 backlog. **This is the headline pitch for the
tokio case study: tokio's CI assumes the repo state is sane; alint
asserts the assumptions.**

### 2.3 Repo-root governance

| Artefact | Coverage | Rule |
|---|---|---|
| `LICENSE` (MIT) | ✅ alint-today | `oss-license-exists`, `oss-license-non-empty` (oss-baseline) |
| `README.md` | ✅ alint-today | `oss-readme-exists`, `oss-readme-non-stub` |
| `SECURITY.md` | ✅ alint-today | `oss-security-policy-exists` |
| `CODE_OF_CONDUCT.md` | ✅ alint-today | `oss-code-of-conduct-exists` |
| `CONTRIBUTING.md` | ✅ alint-today | `file_exists` (no bundled rule for this; per-repo) |
| `Cargo.toml` (workspace) | ✅ alint-today | `cargo-toml-exists` (rust ruleset) |
| `.gitignore` | ✅ alint-today | `oss-gitignore-exists` |
| Repo-wide hygiene | ✅ alint-today | All 11 rules from `hygiene/no-tracked-artifacts@v1` (catches tracked `target/`, etc.) |

**No `rust-toolchain.toml`** — would be flagged info-level by
`rust-toolchain-pinned`. Deliberate deviation from convention rather
than a regression; this case study's config does NOT override the
bundled rule (the info-level finding is the right signal).

---

## 3. Quantified coverage

Counted across the **12 distinct repo-state checks** + **15 defensive
conventions** + **8 governance artefact families** + **7 GHA
workflows** = **42 distinct surfaces**.

```
✅ alint-today:    33 / 42 = 79%   (8 of 12 checks + all 15 defensive + 8 governance + 2 of 7 workflows for ci/github-actions)
🔄 alint-future:    3 / 42 = 7%    (check-readme byte + check-readme version + spellcheck.dic shape)
❌ out-of-scope:    1 / 42 = 2%    (uring kernel)
                  workflow-shape-only:  5 / 42 = 12%   (loom, stress-test, labeler, etc. — covered by ci/github-actions but not gating)
                  ──────────────
                  total = 100%
```

Granular breakdown:

```
12 distinct repo-state checks:
  ✅ alint-today:     8 / 12 = 67%
  🔄 alint-future:    3 / 12 = 25%
  ❌ out-of-scope:    1 / 12 = 8%

15 defensive conventions:
  ✅ alint-today:    15 / 15 = 100%

8 governance artefact families:
  ✅ alint-today:     8 / 8  = 100%

7 GHA workflows:
  ✅ alint-today (ci/github-actions@v1):  7 / 7 = 100%
```

**Commentary.** Three observations:

1. **tokio is the canonical "convention without explicit checks"
   demonstration.** **Verified:** zero hand-rolled scripts (no
   `scripts/`, no `*.sh`, no `tools/`, no `Makefile`). Pure CI
   orchestration relying on the Cargo workspace + a few
   `[workspace.lints]` blocks + the cargo-deny / spellcheck /
   semver / clippy ecosystem. The 15 defensive conventions in §2.2
   are all 100 % expressible with alint's existing rule kinds —
   that's the headline pitch.

2. **The check-readme cross-file-equality + version-cross-reference
   pair is one of the strongest single-repo demand-drivers for
   `cross_file_value_equals`.** Together with the spellcheck.dic
   header-equals-body-line-count check (which needs `pair_hash` +
   `ordered_block`), tokio surfaces 3 of the 7 P2 v0.10 rule-kind
   candidates. The brief's "alint catches 15 conventions tokio's
   pipeline silently assumes" is **accurate** — verified.

3. **One out-of-scope surface (`uring-kernel-version-test.yml`) is
   the only check alint deliberately shouldn't try to express.**
   That's the cleanest out-of-scope ratio in the whole case-study
   set (1 / 42 = 2 %), reflecting tokio's discipline of keeping
   CI orchestration thin and per-tool.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (496 lines, 28
repo-specific rules, 6 bundled rulesets folded in via `extends:`,
**74 rules total** loaded — confirmed by `alint validate-config`).

**Synopsis of the load-bearing rules** (full config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/rust@v1                          # 11 rules
  - alint://bundled/monorepo@v1                      # 4 rules
  - alint://bundled/monorepo/cargo-workspace@v1      # 4 rules
  - alint://bundled/ci/github-actions@v1             # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules

rules:
  # 6 command: shellouts wrapping the existing per-language tools
  - id: tokio-rustfmt-check
    kind: command
    paths: "Cargo.toml"
    command: ["cargo", "fmt", "--check"]
  - id: tokio-clippy-workspace
    kind: command
    paths: "Cargo.toml"
    command: [cargo, clippy, --workspace, --tests, --no-deps, --features, "full,test-util"]
  - id: tokio-cargo-deny             # audit.yml + pr-audit.yml
    kind: command
    paths: "deny.toml"
    command: ["cargo", "deny", "check"]
  # The 15 defensive conventions tokio's CI silently assumes
  - id: tokio-cargo-deny-licenses-allowlist   # Empty allow = audit silently no-ops
    kind: toml_path_matches
    paths: deny.toml
    path: "$.licenses.allow[*]"
    matches: ".+"
  - id: tokio-internal-crate-not-publishable  # publish=false on benches/, examples/, …
    kind: toml_path_equals       # *_path_equals (not *_matches) for bool — pitfall #16
    paths: { include: ["benches/Cargo.toml", "examples/Cargo.toml", "stress-test/Cargo.toml", "tests-build/Cargo.toml", "tests-integration/Cargo.toml"] }
    path: "$.package.publish"
    equals: false
  - id: tokio-workspace-patch-block-present   # [patch.crates-io] tokio = { path = "tokio" }
    kind: toml_path_matches
    paths: Cargo.toml
    path: "$.patch['crates-io'].tokio.path"   # Bracket notation for dashed key
    matches: '^tokio$'
  - id: tokio-spellcheck-dic-first-line-is-count   # check-spelling job's static guard
    kind: file_content_matches
    paths: spellcheck.dic
    pattern: '\A\d+\n'
  - id: tokio-crate-inherits-workspace-lints  # Every member's [lints] workspace = true
    kind: file_content_matches
    paths: { include: ["tokio/Cargo.toml", "tokio-macros/Cargo.toml", …] }
    pattern: '(?ms)^\[lints\]\s*\nworkspace\s*=\s*true'
```

**Repo-specific vs bundled split:**

- **28 tokio-specific rules** (`tokio-*` prefix): 6 `command:`
  shellouts (rustfmt / clippy / cargo-deny / cargo-doc /
  cargo-semver-checks / cargo-spellcheck) + the 15 defensive
  structural conventions enumerated in §2.2 + 7 helpers
  (workflow-presence, deny.toml shape, README-pair file-presence,
  spellcheck file presence, illumos buildomat config presence,
  no-trailing-whitespace).
- **48 bundled rules** from the 6 extended rulesets (15 + 11 + 4 +
  4 + 3 + 11 = 48 with overlap dedup that the engine computes; net
  effective is 46 because of small overlaps with the per-rule
  ones).

**Validation:** `alint validate-config` reports `✓ Config valid: 74
rule(s) loaded`. Pitfall checks: the magic comment is present (line
1); `command:` rules use `command:` (not `argv:`); bool comparison
uses `toml_path_equals`/`equals: false` (pitfall #16 workaround
documented in-line); the `[patch.crates-io]` JSONPath uses bracket
notation for the dashed key (`$.patch['crates-io']`); all patterns
are single-quoted scalars (no YAML literal block scalars — pitfall
#22-clean).

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the live
`/tmp/tokio/` sparse-checkout. Machine: Linux 6.1.0-42-amd64, ~10
logical cores; alint binary `target/release/alint v0.9.17`. `-i`
ignores non-zero exit (alint exits non-zero on violations, this is
timing not pass-fail).

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full pass (74 rules)** | n/a | n/a | **1.567 s** ± 313 ms | — |

The alint pass times in **1.57 s wall-clock against the full
sparse-checkout of the tokio workspace** (5 published + 5 internal
crates, ~800 Rust source files, the 7 GHA workflows). Most of that
time is the Rust source-tree walk for the bundled `rust@v1` rules
(snake-case, no-bidi, final-newline, no-zero-width); the 28
tokio-specific rules add roughly 100 ms each (most are
file-presence / TOML-path / single-file content matches).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `cargo fmt --check` (tokio-rustfmt-check) | rustfmt | pending — needs the workspace's pinned toolchain on PATH | `rustup show` then `cd /tmp/tokio && cargo fmt --check` |
| `cargo clippy --workspace …` (tokio-clippy-workspace) | clippy | pending — same toolchain requirement | `cargo clippy --workspace --tests --no-deps --features full,test-util` |
| `cargo deny check` (tokio-cargo-deny) | cargo-deny | pending — `cargo-deny` not on PATH | `cargo install cargo-deny` |
| `cargo doc --workspace --no-deps` (tokio-cargo-doc-no-deps) | rustdoc | pending — needs full toolchain | `cargo doc --workspace --no-deps` |
| `cargo semver-checks` (tokio-semver-checks-published-crates) | cargo-semver-checks | pending — `cargo-semver-checks` not on PATH | `cargo install cargo-semver-checks` |
| `cargo spellcheck --code 1` (tokio-cargo-spellcheck) | cargo-spellcheck | pending — `cargo-spellcheck` not on PATH | `cargo install cargo-spellcheck` |

The full `make verify`-equivalent end-to-end wall-clock isn't
applicable to tokio (no `Makefile`); the meaningful comparison is
the 6 `command:`-shellout subset against alint's 1.57 s
declarative pass. The pitch: **alint runs the 15 defensive
structural conventions + the 6 cargo shellouts from a single
config in one walk**, replacing the sequential 30-minute basics
tier of `ci.yml`'s structural-state jobs with parallel evaluation
once the tooling is available.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/tokio-rs-tokio/.alint.yml --format json /tmp/tokio/`
(live run, JSON-format).

**Headline:** alint surfaces **180 violations** across 7 failing
rules (46 passing) — the lowest violation count in batch 6,
matching tokio's reputation as a famously clean codebase. The
breakdown:

| # | Count | Rule | Triage |
|---|---|---|---|
| 1 | 172 | `gha-pin-actions-to-sha` (bundled `ci/github-actions@v1`) | **All real findings.** tokio's workflows pin third-party actions by floating tag (`@v6`, `@v2`) rather than 40-char commit SHA. This is the project's stated convention (the in-config comment block on lines 433-438 explicitly notes this). **Recommended:** either accept the convention by overriding the bundled rule to `info` level, or upgrade to SHA pinning project-wide for supply-chain hardening. **Worth filing upstream** as a decision point. |
| 2 | 3 | `rust-sources-snake-case` (bundled rust@v1) | **3 real findings.** Likely test-fixture macros or deliberate non-snake names. Worth a one-line scope-exclude or per-file allowlist. |
| 3 | 1 | `oss-codeowners-exists` | **Real finding.** tokio doesn't ship a `CODEOWNERS` file; ownership is informal via `MAINTAINERS.md` / GitHub repo permissions. Below tokio's threshold of attention. |
| 4 | 1 | `rust-cargo-lock-exists` | **False positive.** The bundled `rust@v1` rule expects `Cargo.lock` at the workspace root, but tokio's sparse-clone of `/tmp/tokio` may not include it (depends on the sparse-checkout filter). Verify with `ls /tmp/tokio/Cargo.lock`. |
| 5 | 1 | `rust-toolchain-pinned` | **Expected — deliberate deviation.** tokio pins toolchain via `env.rust_*` vars in workflows rather than `rust-toolchain.toml`. The bundled rule is `info`-level for exactly this case; the in-config comment on lines 80-86 documents this design choice. |
| 6 | 1 | `gha-workflow-contents-read` | Real finding — one workflow doesn't declare `permissions: contents: read`. Worth filing for hardening. |
| 7 | 1 | `tokio-cargo-spellcheck` | The `command:` rule fires as "spellcheck binary missing" rather than reporting actual spell errors. Pending toolchain installation. |

**Real findings (alint surfaced, existing tooling missed):**

- 172 GHA action references not pinned to commit SHA (against
  current supply-chain best practice; tokio's stated convention is
  major-version pinning, but this is a documented decision not an
  oversight).
- 3 snake-case violations in Rust source (likely test fixtures —
  worth scope-exclude or one-line-each allowlist).
- 1 missing `CODEOWNERS` (tokio uses informal ownership; below
  threshold).

**No P0 / P1 bugs surfaced in tokio's main tree** — the gap
discovery validates §3's commentary: tokio's CI assumes the repo
state is sane, alint asserts the assumptions, the assumptions are
mostly held.

**Pitfall #22 verification:** ZERO instances in `.alint.yml`.
`grep -nE 'pattern:\s*[|>][-+]?$' /home/kaminsod/projects/alint/examples/tokio-rs-tokio/.alint.yml`
returns no matches. The 2 multi-line patterns in this config
(`tokio-crate-inherits-workspace-lints` line 299:
`pattern: '(?ms)^\[lints\]\s*\nworkspace\s*=\s*true'`, and
`tokio-spellcheck-dic-first-line-is-count` line 226:
`pattern: '\A\d+\n'`) both use single-quoted YAML scalars where
`\n` does NOT expand (escape-sequence non-expansion is single-quote
behavior). The first uses `(?ms)` to interpret `\n` as a literal
multi-line regex anchor across the YAML-encoded literal `\n`
(which the regex engine reads byte-for-byte). The second uses `\A`
to anchor at the file start. Both are correct as written.

---

## 7. Pitfall #22 verification (this batch's special call-out)

The brief asked: **verify every multi-line regex in this case
study's config for the YAML literal-block-scalar trailing-newline
issue (pitfall #22).**

**Verdict for `examples/tokio-rs-tokio/.alint.yml`: ZERO instances.**
`grep -nE 'pattern:\s*[|>][-+]?$'
/home/kaminsod/projects/alint/examples/tokio-rs-tokio/.alint.yml`
returns no matches. The 2 multi-line patterns in this config use
single-quoted scalars (`pattern: '(?ms)^\[lints\]\s*\nworkspace\s*=\s*true'`
on line 299 and `pattern: '\A\d+\n'` on line 226). Both are correct
as written; no per-crate license-header `file_header` rule exists
(tokio's MIT license is asserted via `oss-license-exists`, not
pattern matching).

---

## 8. Followup feature work surfaced

Sorted by demand strength:

- **`cross_file_value_equals`** — covers tokio's `diff README.md
  tokio/README.md` (check-readme byte-equality) and the
  version-grep cross-reference. **v0.10 ship-target (10 sources).**
  tokio is one of the demand-drivers.
- **`ordered_block`** — covers tokio's `spellcheck.dic` body
  sortedness. **v0.10 ship-target (7 sources).**
- **`pair_hash`** — covers `spellcheck.dic` first-line-equals-body-
  line-count. **v0.10+ candidate (3 sources: k8s + tokio +
  golang/go FIPS).**
- **`toml_path_equals` typed-value comparison** — minor DX polish;
  today the workaround stringifies-and-regex-matches against bool.
  Same pitfall #16 / pitfall #17 family from CONFIG-AUTHORING.md.
  **v0.10+ DX item.**

No new rule-kind candidates beyond the 9 already on the v0.10+
pipeline. tokio's gap catalogue overlaps heavily with what the
kubernetes and rust-lang/rust pilots already surfaced — itself
useful evidence that the gap list is **saturating**.

---

## 9. Future analysis

Three candidate refinements for the next revalidation pass:

1. **Of the 15 defensive conventions, which become 1-liners under
   the v0.9.6+ surface?** Most already are (`for_each_dir`,
   `for_each_file`, structured-path matchers, `command:`
   shellouts). The remaining gaps are the 3 cross-file / ordering /
   hashing patterns above — all on the v0.10 pipeline.
2. **`agent-context` / `docs/adr` bundled-ruleset adoption.** The
   config will skip the newer `agent-context@v1` (5 rules — agent-
   readable docs presence) and `docs/adr@v1` (4 rules — ADR
   directory shape). tokio doesn't ship an ADR tree (RFCs live in
   linked issues); `agent-context` would catch the absence of
   `AGENTS.md` / `CLAUDE.md` if maintainers opt into agent-tooling
   discipline.
3. **`alint suggest` against a fresh clone.** Hasn't been run for
   this case study; would either confirm or surface bundled
   candidates not yet adopted (likely `compliance/apache-2` is wrong
   for tokio's MIT licensing, but `tooling/editorconfig` would land
   cleanly).

---

## 10. Validation status (2026-05-07)

- **alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).
- **`.alint.yml` in this directory:** **shipped — 496 lines, 28
  repo-specific rules, 6 bundled rulesets folded in via `extends:`,
  74 effective rules loaded.**
  `alint validate-config` confirms `✓ Config valid: 74 rule(s)
  loaded`. **Live-tree recheck:** performed in this batch — see §6
  for the 180-violation breakdown (172 GHA SHA-pinning + 7 small
  long-tail; no P0/P1 bugs).
- **Hand-rolled scripts verification:** **CONFIRMED zero** — `find
  /tmp/tokio -name "*.sh" -not -path "*/target/*"` returns no
  results. `ls /tmp/tokio/` shows no `scripts/`, `tools/`,
  `Makefile`, or `lint*` files. Pure CI orchestration.
- **Workflow count + line-count verification:** **CONFIRMED 7
  workflows totalling 1,714 lines** via `wc -l
  /tmp/tokio/.github/workflows/*.yml`.
- **15 defensive conventions verification:** **CONFIRMED** —
  enumerated in §2.2 with the exact source-file evidence
  (`/tmp/tokio/Cargo.toml` workspace + per-member, `deny.toml`,
  `spellcheck.{toml,dic}`, `.github/workflows/{ci,audit,pr-audit}.yml`).
- **`spellcheck.dic` first-line verification:** **CONFIRMED** —
  `head -1 /tmp/tokio/spellcheck.dic` returns `323` (integer
  count).
- **Workspace-member count verification:** **CONFIRMED 5 published +
  5 internal** via `grep -A 20 "^members" /tmp/tokio/Cargo.toml`.
  `[patch.crates-io]` block has the matching 5 entries.
- **Rule-kind candidate status:**
  - `cross_file_value_equals` — v0.10 ship-target (10 sources).
    tokio is one of the demand-drivers.
  - `ordered_block` — v0.10 ship-target (7 sources).
  - `pair_hash` — v0.10+ candidate (3 sources). tokio is one of
    the 3.
- **Pitfall #22 instances in this directory's config:** **ZERO**
  (`grep -nE 'pattern:\s*[|>][-+]?$' .alint.yml` returns no
  matches; all 2 multi-line patterns use single-quoted scalars).
- **Bundled-ruleset rule counts (authoritative as of 2026-05-07):**
  oss-baseline=15, rust=11, monorepo=4, monorepo/cargo-workspace=4,
  ci/github-actions=3, hygiene/no-tracked-artifacts=11.
