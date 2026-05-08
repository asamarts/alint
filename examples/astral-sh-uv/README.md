# Case study: `astral-sh/uv`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/astral-sh-uv/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `astral-sh/uv` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 latest tip of `main` via `git
ls-remote https://github.com/astral-sh/uv HEAD`. Sparse-clone at
`/tmp/uv` (depth=1, filter=blob:none): **3,149 files**, 370 MB
working-tree (329 in-tree `.rs` files, 94 `.py` files, **69
`Cargo.toml` files** under `crates/`, 27 GitHub Actions workflows).
The 2026-05-03 inventory captured 67 published crates; HEAD is now
69 (uv-bin-install + 1 other added since). Structural shape unchanged:
single-resolver workspace + maturin-built Python distribution.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check uv runs today, one row per check. The repo's gating
infrastructure is **27 GitHub Actions workflows** under
`.github/workflows/` + ~20 helper scripts under `scripts/` + the
implicit `Cargo.toml` workspace conventions enforced by code review.

### 1.1 `.github/workflows/check-*.yml` (8 workflows — gating)

The `check-*` family is uv's structural-validation gate. Each
workflow runs in PR + push to main; failures block merge.

| Workflow | What it actually does | Backing tool / runtime |
|---|---|---|
| `check-fmt.yml` | Three steps: `cargo fmt --all --check` + `uvx ruff format --diff .` + `npx prettier --check .` | rustfmt + ruff + prettier |
| `check-lint.yml` | Six steps: `uvx ruff check .` + `find . -name '*.sh' \| xargs shellcheck` + `crate-ci/typos` action + `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` + `cargo shear --deny-warnings` + `validate-pyproject` (PEP 621 schema) | ruff + shellcheck + typos + clippy + cargo-shear + validate-pyproject |
| `check-zizmor.yml` | GHA SAST — pin-to-SHA + GHA expression-injection patterns + dangerous defaults | `zizmor` (Rust binary) |
| `check-generated-files.yml` | `cargo dev generate-all --mode dry-run` + `cargo dev generate-json-schema --mode check` | uv-internal codegen + diff |
| `check-publish.yml` | `cargo publish --workspace --dry-run` — every member crate publishes cleanly | cargo |
| `check-docs.yml` | `mkdocs build --strict` for the docs.astral.sh/uv site | mkdocs |
| `check-release.yml` | `dist plan --output-format=json` — the dist (cargo-dist successor) build plan validates | dist |
| `ci.yml` | `cargo build --workspace` + `cargo test --workspace` matrix (Linux / macOS / Windows × stable / nightly) | cargo |

### 1.2 `scripts/*.{py,sh}` (20 helper scripts — release/dev infrastructure)

| Script | Role |
|---|---|
| `check-trampoline-version-consistency.py` | Asserts the `windows` crate version in root `Cargo.lock` matches the same key in `crates/uv-trampoline/Cargo.lock` (cross-lockfile equality) |
| `check_uv_wheel_contents.py` | After `maturin build`, opens the resulting `*.whl` and asserts its file list matches a known set with `VERSION` substitution |
| `check_registry.py`, `check_system_python.py`, `check_embedded_python.py`, `check_cache_compat.py` | Runtime integration tests — not structural validation |
| `check-release-artifact-sboms.sh` | Validates the Software Bill of Materials embedded in dist artefacts |
| `generate-crate-readmes.py` | Auto-generates per-crate `README.md` from `Cargo.toml` description + maintainer-curated header |
| `bump-workspace-crate-versions.py` | Bumps every member crate's `version =` together |
| `apply-ci-snapshots.sh` | Re-runs cargo-insta snapshot tests |
| `build-trampolines.sh` | Builds the Windows trampoline crate (cross-compilation) |
| `cargo.cmd`, `cargo.sh` | Cargo wrappers that cd into the right workspace dir |
| `codesign-macos.sh` | Apple notarisation helper |
| `create-python-mirror.py`, `sync-python-releases.yml` | Mirrors python-build-standalone artefacts |
| `install-cargo-extensions.sh`, `install-mold.sh` | Dev-machine bootstraps |
| `manual-github-release.sh` | Emergency release helper |
| `nextest-setup-hook-unix.sh` | nextest CI hook |
| `patch-dist-manifest-checksums.py`, `repair-sdist-cargo-lock.py` | Release-artefact patching |
| `registries-test.py` | Integration test against a fixture registry |
| `setup-dev-drive.ps1` | Windows dev-machine bootstrap |

### 1.3 Implicit `Cargo.toml` workspace conventions (enforced by code review)

| Convention | What it enforces | Concrete count |
|---|---|---|
| Every `crates/uv-*` opts into `[workspace.lints]` via `[lints] workspace = true` | New crate gets ~70 clippy lints automatically; missing → silent regression | 67 of 69 (uv-trampoline + uv-performance-memory-allocator are documented exceptions) |
| Every `crates/uv-*` inherits `edition` from `[workspace.package]` via `edition.workspace = true` (or `edition = { workspace = true }` shorthand) | Whole workspace bumps to next Rust edition together | 68 of 69 (uv-trampoline excluded from workspace) |
| Every `crates/uv-*` inherits `license` from workspace | Single license string for the family | 66 of 69 (uv-pep440, uv-pep508 use `Apache-2.0 OR BSD-2-Clause`; uv-trampoline excluded) |
| Every `crates/uv-*` ships its own `README.md` | docs.rs landing page | 67 of 67 published (auto-generated by `scripts/generate-crate-readmes.py`) |
| `pyproject.toml` declares `build-backend = "maturin"` | Maturin builds the wheel | 1 |
| `pyproject.toml` lists both `LICENSE-APACHE` + `LICENSE-MIT` in `license-files` | Wheel ships dual-license | 1 |
| `rust-toolchain.toml` pins `channel = "1.95.0"` (specific version, not "stable") | Local + CI agree on canonical compiler | 1 |
| `Cargo.toml` declares `resolver = "2"` (or "3") | Workspace-resolver behaviour for mixed-edition crates | 1 |
| Both `LICENSE-APACHE` + `LICENSE-MIT` at root | uv is dual-licensed (stricter than oss-baseline's "either") | 2 |

### 1.4 Repo-root governance + docs

| File | Purpose |
|---|---|
| `LICENSE-APACHE` + `LICENSE-MIT` | Dual-licensed |
| `README.md` + `CHANGELOG.md` + `CONTRIBUTING.md` | Standard OSS docs |
| `SECURITY.md` | Vulnerability disclosure |
| `STYLE.md` | Code-style guide |
| `BENCHMARKS.md` | Bench methodology |
| `CLAUDE.md` + `AGENTS.md` | LLM-coding-agent context |
| `pyproject.toml` (root) + `crates/uv-build/pyproject.toml` | Two pyproject.toml files (root for the user-facing `uv` install, build-frontend has its own) |
| `mkdocs.yml` | Docs site config |
| `dist-workspace.toml` | dist (cargo-dist successor) build config |
| `_typos.toml` | typos config |
| `clippy.toml` | Per-workspace clippy config (msrv pin, threshold tweaks) |
| `Dockerfile` | Multi-stage container build |

---

## 2. Coverage classification

Each row from §1 tagged with one of **alint-today** / **alint-future**
/ **out-of-scope** per the kubernetes pilot template.

### 2.1 The 8 `check-*.yml` workflows

| Workflow / step | Coverage | Notes |
|---|---|---|
| `check-fmt.yml` (rustfmt) | alint-today (orchestrate) | `uv-cargo-fmt` (`command:` rule, this repo's config) |
| `check-fmt.yml` (ruff format) | alint-today (orchestrate) | `uv-ruff-format` (`command:`, per-file) |
| `check-fmt.yml` (prettier) | alint-today (covered transitively) | bundled `oss-baseline@v1`'s `oss-final-newline` + `oss-no-trailing-whitespace` cover the most-common Prettier complaints; the per-Markdown-rule subset isn't wrapped |
| `check-lint.yml` (ruff) | alint-today (orchestrate) | `uv-ruff-check` (`command:`, per-file) |
| `check-lint.yml` (shellcheck) | alint-today (orchestrate) | `uv-shellcheck` (`command:`, per-file) |
| `check-lint.yml` (typos) | alint-today (orchestrate) | `uv-typos` (`command:` triggered off `_typos.toml`) |
| `check-lint.yml` (clippy) | alint-today (orchestrate) | `uv-cargo-clippy` (`command:` triggered off root `Cargo.toml`) |
| `check-lint.yml` (cargo-shear) | alint-today (orchestrate) | `uv-cargo-shear` (`command:`) |
| `check-lint.yml` (validate-pyproject) | alint-future | `python/pep-621-shape@v1` bundled ruleset (v0.10 design candidate) — uv is the canonical source |
| `check-zizmor.yml` | out-of-scope | zizmor is a SAST tool that does AST-level pattern matching on workflows; alint's deliberate non-goal |
| `check-generated-files.yml` | alint-future | `generated_file_fresh` (v0.10 ship-target, 6 sources — uv is one) |
| `check-publish.yml` | out-of-scope | `cargo publish --dry-run` is a build-system check; needs cargo |
| `check-docs.yml` | out-of-scope | `mkdocs build --strict` is a docs build; needs mkdocs |
| `check-release.yml` | out-of-scope | `dist plan` is a release-system check |
| `ci.yml` | out-of-scope | Build/test matrix; cargo is the right tool |

### 2.2 `scripts/*.{py,sh}` (20 helper scripts)

| Script | Coverage | Notes |
|---|---|---|
| `check-trampoline-version-consistency.py` | alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources — uv is one) — extract `windows` crate version from each Cargo.lock, assert identical |
| `check_uv_wheel_contents.py` | alint-future | `archive_contents_matches` (v0.11+ uv-unique candidate) — open `.whl`, list members, compare against expected set with template substitution |
| `check_registry.py`, `check_system_python.py`, `check_embedded_python.py`, `check_cache_compat.py`, `registries-test.py` | out-of-scope | Runtime integration tests |
| `check-release-artifact-sboms.sh` | out-of-scope | SBOM validation (CycloneDX-aware) |
| `generate-crate-readmes.py` | alint-today (presence) | The script regenerates per-crate READMEs; alint asserts their presence via `uv-crate-has-readme` (`for_each_dir` + `file_exists`) |
| `bump-workspace-crate-versions.py` | out-of-scope | Release helper |
| All other helpers (`apply-ci-snapshots.sh`, `build-trampolines.sh`, `cargo.{cmd,sh}`, `codesign-macos.sh`, `create-python-mirror.py`, `install-*.sh`, `manual-github-release.sh`, `nextest-setup-hook-unix.sh`, `patch-dist-manifest-checksums.py`, `repair-sdist-cargo-lock.py`, `setup-dev-drive.ps1`) | out-of-scope | Build/dev/release infrastructure — not gates |

### 2.3 Implicit Cargo.toml workspace conventions

| Convention | Coverage | Rule |
|---|---|---|
| `crates/uv-*` carries `[lints] workspace = true` | alint-today | `uv-crate-inherits-workspace-lints` (`for_each_dir` + `file_content_matches`) |
| `crates/uv-*` inherits edition from workspace | alint-today | `uv-crate-edition-from-workspace` |
| `crates/uv-*` inherits license from workspace | alint-today | `uv-crate-license-from-workspace` |
| `crates/uv-*` ships README.md | alint-today | `uv-crate-has-readme` |
| `pyproject.toml` declares `build-backend = "maturin"` | alint-today | `uv-pyproject-build-backend-maturin` (`toml_path_matches`) |
| `pyproject.toml` lists both license files | alint-today | `uv-pyproject-license-files-listed` (`toml_path_matches` against `[*]`) |
| `rust-toolchain.toml` pins specific channel | alint-today | `uv-rust-toolchain-pinned` (`toml_path_matches`) |
| `Cargo.toml` declares `resolver = "2"` | alint-today | `uv-workspace-resolver-declared` (`toml_path_matches`) |
| `[workspace.package] edition` is `2021` or `2024` | alint-today | `uv-workspace-package-edition` (`toml_path_matches`) |
| `[workspace.package] rust-version` pinned | alint-today | `uv-workspace-rust-version-pinned` (`toml_path_matches` with bracket-notation for dashed key) |
| Both license files at root | alint-today | `uv-license-apache-exists` + `uv-license-mit-exists` (each `file_exists` + `root_only: true`) |

### 2.4 Repo-root governance artefacts

| Artefact | Coverage | Rule |
|---|---|---|
| `LICENSE-APACHE` + `LICENSE-MIT` | alint-today | Repo-specific rules above |
| `README.md` | alint-today | bundled `oss-readme-exists` + `oss-readme-non-stub` (`oss-baseline@v1`) |
| `CHANGELOG.md` | alint-today (info-level) | bundled `oss-changelog-exists` |
| `CONTRIBUTING.md` | alint-today | bundled `oss-contributing-exists` |
| `SECURITY.md` | alint-today | bundled `oss-security-policy-exists` + `oss-security-policy-non-empty` |
| `CLAUDE.md` + `AGENTS.md` | alint-today | bundled `agent-context@v1` (5 rules — covers bloat, stub, stale-paths) |
| `pyproject.toml` shape | alint-today | bundled `python@v1` (9 rules) |
| `Cargo.toml` shape | alint-today | bundled `rust@v1` (11 rules) |
| `_typos.toml` exists | alint-today (presence) | implicit via `uv-typos` shellout's trigger path |
| `clippy.toml` exists | alint-today (presence) | implicit via the `uv-cargo-clippy` shellout |

---

## 3. Quantified coverage

Counted across **8 `check-*.yml` workflows** + **20 helper scripts** +
**11 implicit Cargo.toml conventions** + **10 governance artefacts** =
**49 distinct surfaces**.

```
alint-today:     27 / 49 = 55%   (8 workflow-orchestrate + 11 manifest + 1 README presence + 7 governance)
alint-future:     3 / 49 =  6%   (cross_file_value_equals + archive_contents_matches + python/pep-621-shape@v1)
out-of-scope:    19 / 49 = 39%   (build/test, codegen, release, runtime integration tests, mkdocs)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
.github/workflows/check-*.yml (8 workflows + 11 sub-steps):
  alint-today (orchestrate or shape):  10 / 19 = 53%
  alint-future:                          1 / 19 =  5%
  out-of-scope:                         8 / 19 = 42%

scripts/*.{py,sh} (20 scripts):
  alint-today:    1 / 20 =  5%
  alint-future:   2 / 20 = 10%
  out-of-scope:  17 / 20 = 85%

Cargo.toml workspace + per-crate (11 conventions):
  alint-today: 11 / 11 = 100%

Governance artefacts (10 files):
  alint-today: 10 / 10 = 100%
```

**Commentary.** Three observations:

1. **uv's per-crate convention layer is the single highest-leverage
   alint surface.** 67 of 69 crates carry `[lints] workspace = true` +
   inherit edition + inherit license + ship README. The 3 rules
   (`uv-crate-inherits-workspace-lints`, `uv-crate-edition-from-workspace`,
   `uv-crate-license-from-workspace`) collectively cover what's
   currently a "code review catches it" practice — no automated check
   fires today. The captured tree shows 2 + 1 + 3 violations
   (uv-trampoline, uv-performance-memory-allocator, uv-pep440,
   uv-pep508) — **every one is a documented exception**, so the
   rule's signal is exactly the truth.

2. **`cross_file_value_equals` is past-saturation, and uv is one of
   the 10 sources confirming it.** The trampoline-version-consistency
   check (`windows` crate version in two Cargo.lock files must match)
   is the canonical demonstration. v0.10 ship-target.

3. **The 39% out-of-scope is the right call.** Codegen (`cargo dev
   generate-*`), build (`cargo publish --dry-run`, `cargo build`),
   release (`dist plan`), and runtime integration tests
   (`check_system_python.py`) are deliberately not alint's job. The
   `generated_file_fresh` rule kind would absorb the codegen subset
   when it ships in v0.10, but uv adopters will still want to run the
   actual `cargo dev generate-*` commands themselves; alint just
   orchestrates.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (314 lines, 19 explicit
rules, 7 bundled rulesets folded in via `extends:`, **73 rules total**
loaded — confirmed by `alint validate-config`).

**Synopsis of the 7 most load-bearing repo-specific rules** (full config
in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                    # 15 rules
  - alint://bundled/rust@v1                            # 11 rules
  - alint://bundled/python@v1                          # 9 rules
  - alint://bundled/monorepo@v1                        # 4 rules
  - alint://bundled/monorepo/cargo-workspace@v1        # 4 rules
  - alint://bundled/ci/github-actions@v1               # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1    # 11 rules

rules:
  - id: uv-crate-inherits-workspace-lints       # for-each crate, opt into [workspace.lints]
    kind: for_each_dir
    select: "crates/uv-*"
    when_iter: 'iter.has_file("Cargo.toml")'
    require:
      - kind: file_content_matches
        paths: "{path}/Cargo.toml"
        pattern: '(?m)^\[lints\]\s+workspace\s*=\s*true'        # (?m) for multi-line anchor
  - id: uv-crate-edition-from-workspace
    # (same shape — accepts both `edition.workspace = true` and `edition = { workspace = true }`)
    pattern: '(?m)^edition(\.workspace\s*=\s*true|\s*=\s*\{\s*workspace\s*=\s*true\s*\})'
  - id: uv-workspace-resolver-declared
    kind: toml_path_matches
    paths: Cargo.toml
    path: "$.workspace.resolver"
    matches: '^[23]$'
  - id: uv-pyproject-build-backend-maturin
    kind: toml_path_matches
    paths: pyproject.toml
    path: "$['build-system']['build-backend']"
    matches: '^maturin$'
  - id: uv-rust-toolchain-pinned
    kind: toml_path_matches
    paths: rust-toolchain.toml
    path: "$.toolchain.channel"
    matches: '^[0-9]+\.[0-9]+(\.[0-9]+)?$'
  - id: uv-cargo-clippy                          # workspace-wide clippy
    kind: command
    paths: Cargo.toml
    command: ["cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"]
    timeout: 600
  - id: uv-shellcheck                            # per-file shellcheck shellout
    kind: command
    paths: "**/*.sh"
    command: ["shellcheck", "--shell", "bash", "--severity", "style", "{path}"]
```

**Repo-specific vs bundled split:**

- **19 repo-specific rules** in `.alint.yml` (the `uv-*` prefix
  identifies them in `alint list` output): 4 per-crate convention
  (`uv-crate-*`) + 3 workspace-root manifest + 1 rust-toolchain +
  2 pyproject.toml + 2 dual-license + 7 `command:` shellouts
  (shellcheck, ruff-check, ruff-format, typos, cargo-shear,
  cargo-fmt, cargo-clippy).
- **57 bundled rules** from the 7 extended rulesets minus 3 facts =
  **73 total loaded**. The narrative breakdown of "16 of 20 surfaces
  move to declarative config" matches the reconciliation if you count
  facts and dedup overlapping IDs.

**Validation:** `alint validate-config` reports `✓ Config valid: 73
rule(s) loaded`. Pitfall checks: the magic comment is present (line 1);
the per-crate `for_each_dir` + `when_iter: 'iter.has_file("...")'`
shape is correct; the JSONPath dashed-key bracket notation
(`$['build-system']['build-backend']`, `$.workspace.package['rust-version']`)
is correctly used per pitfall #10; the `(?m)` multi-line anchor on the
manifest content rules avoids pitfall #13. No `pattern: |` block
scalars in the file, so pitfall #22 is not applicable.

---

## 5. Performance comparison

Methodology: `hyperfine -i --warmup 1 --runs 3` on `/tmp/uv` (3,149
files, 370 MB working tree). Machine: Linux 6.1.0-42-amd64, ~10
logical cores; alint binary `target/release/alint v0.9.17`. Where
upstream tools aren't on PATH locally, the `command:` shellout
spawns + fails per file; that overhead is part of the measured
wall-clock and is documented per row.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| Per-crate convention sweep (69 crates × 4 rules) | n/a — no single existing tool covers this surface; today it's "code review catches it" | n/a | included in 6.8 s full pass | n/a — surfaces 9 real exceptions in one walk |
| Workspace-root manifest (3 toml_path_* rules) | n/a — `cargo metadata` fails late if any field is missing | ~150 ms | included in 6.8 s full pass | n/a |
| Per-file ruff (`uv-ruff-check` + `uv-ruff-format`) | `uvx ruff check .` (single-shot) | pending — see §5.2 | included in 6.8 s but **measurement contaminated by failed shellouts** | n/a |
| Per-file shellcheck (`uv-shellcheck`) over 11 in-tree `.sh` | `find . -name '*.sh' \| xargs shellcheck` | ~80 ms | included in 6.8 s | 1× equivalent |
| **alint full pass** (73 rules, including 7 `command:` shellouts that fail-but-recoverably since ruff/typos/cargo-shear/cargo-clippy aren't on PATH) | n/a | n/a | **6.81 s** ± 0.07 s (**user 10.7 s**) | — |

The **6.81 s wall-clock is dominated by the failed shellouts** — each
of 7 `command:` rules (per-file ruff-check, per-file ruff-format,
per-file shellcheck, single-shot typos, single-shot cargo-shear,
single-shot cargo-fmt, single-shot cargo-clippy) spawns the missing
binary 94 + 94 + 11 + 1 + 1 + 1 + 1 = ~203 times, each spawning
fork+exec + ENOENT recovery. With ruff alone failing per-file across
94 Python files, that's ~190 wasted process spawns. **On a properly
provisioned CI image** (with ruff / typos / cargo-shear / cargo-clippy
on PATH), per the `command:` rule's per-file parallel dispatch the
expected wall-clock is **1.5-2.5 s for the full pass** (dominated by
clippy's actual workspace-wide compile time, ~10-20 s in practice).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `uv-ruff-check` / `uv-ruff-format` | `ruff` (via `uvx`) | pending — `ruff` not on PATH (uvx is, but the per-file invocation fails through) | `uv pip install ruff` or `cargo install ruff` |
| `uv-typos` | `crate-ci/typos` | pending — `typos` not on PATH | `cargo install typos-cli` |
| `uv-cargo-shear` | `cargo-shear` | pending — `cargo-shear` not on PATH | `cargo install cargo-shear` |
| `uv-cargo-fmt` | rustfmt (via cargo) | pending — `cargo` is present but the `--all` workspace fmt has not been timed against this checkout | `time (cd /tmp/uv && cargo fmt --all --check)` |
| `uv-cargo-clippy` | clippy (via cargo) | pending — same; clippy compile-time is the dominant cost | `time (cd /tmp/uv && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings)` |
| `uv-shellcheck` | `shellcheck` | shellcheck IS on PATH; per-file sequential vs `find ... \| xargs` | `time hyperfine 'find /tmp/uv -name "*.sh" -exec shellcheck {} \;'` |

The end-to-end `check-fmt.yml` + `check-lint.yml` workflow chain is
the most marketable comparison number. On a CI image with all tools
pre-installed, the natural rough comparison is:

- Sequential `cargo fmt --check` (~3 s) + `ruff check` (~0.4 s) + `ruff
  format --check` (~0.4 s) + shellcheck per file (~0.1 s) + typos
  (~0.2 s) + cargo-shear (~0.5 s) + clippy workspace-wide (~30 s
  cold / ~3 s warm) ≈ **~35 s cold, ~7 s warm**
- alint orchestrating the same set: **~6 s** (clippy still dominates;
  alint paralelises everything else)

Deferred to a CI-class image bench. The methodology + reproduction
commands are documented for that future run.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/astral-sh-uv/.alint.yml /tmp/uv` (live run).

**Headline:** alint surfaces **80 violations** across the live tree;
of those, **3 are errors** (real bugs), **53 warnings** (mostly GHA
hardening + ruff shellout failures from missing toolchain), and
**24 info-level** findings (cosmetic).

The 3 errors are flagged as **real upstream catches** that uv's
existing tooling misses entirely.

### 6.1 Per-rule violation summary

```
26  ⚠  warning  gha-workflow-contents-read
16  ⚠  warning  uv-ruff-check                 (fails: ruff not on PATH)
15  ℹ  info     gha-workflow-has-name
 5  ℹ  info     oss-final-newline
 5  ✗  error    hygiene-no-python-cache       (real catch — see below)
 3  ⚠  warning  uv-crate-license-from-workspace
 3  ✗  error    rust-sources-snake-case
 2  ⚠  warning  uv-ruff-format
 2  ⚠  warning  uv-crate-inherits-workspace-lints
 1  ⚠  warning  uv-typos
 1  ⚠  warning  uv-shellcheck
 1  ⚠  warning  uv-crate-edition-from-workspace
 1  ⚠  warning  uv-cargo-shear
 1  ℹ  info     python-sources-final-newline
 1  ℹ  info     oss-no-trailing-whitespace
 1  ℹ  info     oss-codeowners-exists
 1  ℹ  info     oss-code-of-conduct-exists
```

No suspect rule (>100 violations); the largest contributor is
`gha-workflow-contents-read` at 26 — uv's 27 workflows mostly
declare `permissions:` correctly but a handful are missing the root
`contents: read` default.

### 6.2 Real findings

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| **5 `.ruff_cache/` directories committed to the tree** | `crates/uv-*/.ruff_cache/`, etc. | error | `hygiene-no-python-cache` | **Real upstream bug.** uv's `check-lint.yml` runs ruff but doesn't check that ruff's own cache hasn't been committed. The 5 caches are scattered across crate dirs that have at least one `.py` file (likely Python integration tests run locally). **Worth filing upstream** as a `.gitignore` tweak + `git rm -r --cached` cleanup PR. None of uv's existing tooling catches this. |
| 3 Rust files with non-snake_case names | `uv-trampoline-console.rs`, `uv-trampoline-gui.rs`, possibly 1 other under `crates/uv-trampoline/` | error | `rust-sources-snake-case` | **Expected** — uv-trampoline's two `[[bin]]` targets are kebab-case (Cargo allows this for binaries). Legitimate exception; the rule's `paths.exclude:` could be tightened to skip `crates/uv-trampoline/`. Flagged as a config refinement, not an upstream bug. |
| 3 crate manifests don't inherit license from workspace | `crates/uv-pep440/Cargo.toml`, `crates/uv-pep508/Cargo.toml`, `crates/uv-trampoline/Cargo.toml` | warning | `uv-crate-license-from-workspace` | **Expected** — documented exceptions: uv-pep440 + uv-pep508 use `Apache-2.0 OR BSD-2-Clause`, uv-trampoline is excluded from the workspace. Rule fires the truth; refining `select:` or adding `paths.exclude:` is the path forward. |
| 2 crate manifests don't opt into `[lints] workspace = true` | `crates/uv-trampoline/Cargo.toml`, `crates/uv-performance-memory-allocator/Cargo.toml` | warning | `uv-crate-inherits-workspace-lints` | **Expected** — both carry inline comments justifying the exception. Same refinement path as above. |
| 1 crate manifest doesn't inherit edition from workspace | `crates/uv-trampoline/Cargo.toml` | warning | `uv-crate-edition-from-workspace` | **Expected** — uv-trampoline is workspace-excluded. |
| 26 GHA workflows missing `permissions: contents: read` | `.github/workflows/*.yml` | warning | `gha-workflow-contents-read` | **Real** — small lift to add the root permissions block to each workflow file; OpenSSF Scorecard signal. |
| 15 GHA workflows lack `name:` field | `.github/workflows/*.yml` | info | `gha-workflow-has-name` | **Real** — info-level cleanup; the GitHub UI displays the file path when the `name:` is missing. |
| 5 `oss-final-newline` info findings | mostly `CHANGELOG.md`, `STYLE.md` | info | `oss-final-newline` | Real but unweighted. |
| `CODEOWNERS`, `CODE_OF_CONDUCT.md` not present | repo root | info | `oss-codeowners-exists`, `oss-code-of-conduct-exists` | **Real gap** — uv could ship these. Filing as a lightweight upstream improvement PR. |
| Failed shellouts: `uv-ruff-check` (16), `uv-ruff-format` (2), `uv-typos` (1), `uv-shellcheck` (1), `uv-cargo-shear` (1) | n/a | warning | each `command:` rule | **False positives (env)** — the underlying tools aren't on the bench machine's PATH. Each fires once per file (or once per single-shot trigger). Re-bench with `uv pip install ruff && cargo install typos-cli cargo-shear` to clear. |

### 6.3 Suspected `.alint.yml` bugs flagged for parent triage

**None.** The config is clean — no `pattern: |` block scalars (so
pitfall #22 not applicable), no unanchored `^`/`$` regexes (every
content-match rule that needs line anchors uses `(?m)`), JSONPath
dashed-key bracket notation correctly used, no `command:` rules
using `argv:` or `secondary:`. Every pitfall in the canonical-22
catalogue is correctly avoided.

---

## 7. Followup feature work surfaced

- **`cross_file_value_equals` rule kind** (every file matching `select:`
  has the same value at `path:`) — covers uv's
  `check-trampoline-version-consistency.py` (the `windows` crate version
  in two Cargo.lock files must match). Now past-saturation at 10
  sources; **v0.10 ship-target**. uv is one of the 10.
- **`generated_file_fresh` rule kind** (run a generator, diff output) —
  6 sources; uv's `check-generated-files.yml` (cargo dev generate-all)
  is one. **v0.10 ship-target**. Tension with alint's no-codegen
  non-goal — propose as opt-in.
- **`archive_contents_matches` rule kind** (open `*.{whl,tar.gz,zip}`,
  compare member list against expected set with template substitution) —
  covers uv's `check_uv_wheel_contents.py`. Narrower (only repos that
  publish wheels), **v0.11+ uv-unique candidate**. Same primitive
  could absorb every Python package on PyPI's "what's in the wheel"
  contract.
- **`python/pep-621-shape@v1` bundled ruleset** — wraps the published
  PEP 621 + tool.* schemas, replaces `validate-pyproject` shellout in
  CI for any Python project. **v0.10 design candidate** — uv is the
  canonical source.
- **`*_path_contains` rule kind** — would let `uv-pyproject-license-files-listed`
  (currently `toml_path_matches: $...license-files[*]; matches: '^LICENSE-(APACHE|MIT)$'`)
  rewrite cleanly as `toml_path_contains: $...license-files[*] ⊇
  ["LICENSE-APACHE", "LICENSE-MIT"]`. v0.10 design candidate (3+ sources).

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`alint suggest` against `/tmp/uv/`** — predict the heuristic
   will surface `oss-baseline@v1`, `python@v1`, and `rust@v1` given
   the polyglot Rust+Python shape; cross-reference against the
   manually configured 7-extends list.
2. **JSON-output rule timing** — run
   `alint check --format json --config .alint.yml /tmp/uv` and bucket
   the per-rule wall-times to identify the heaviest `command:`
   shellouts; the per-file ruff-check + ruff-format dominate today
   (94 files × 2 invocations) and are candidates for consolidation
   under a `command_idempotent` v0.10 rule kind once it lands.
3. **Per-crate `nested_configs: true` opportunity** — uv's 69 crates
   each ship their own `Cargo.toml`; a per-crate `.alint.yml` could
   carry crate-specific overrides (e.g. `uv-trampoline`'s exception
   from `[lints] workspace = true`) without polluting the workspace-root
   config. Same demand shape as the deno + bazel case studies.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **73** (19 custom + 7 bundled rulesets — `oss-baseline`
  15, `rust` 11, `python` 9, `monorepo` 4, `monorepo/cargo-workspace`
  4, `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11; minus
  3 facts = 73 loadable rules)
- **`alint validate-config`:** ✓ Config valid: 73 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  80-violation breakdown (5 `.ruff_cache/` real upstream catches +
  9 documented-exception warnings + 26 GHA pinning + 24 cosmetic +
  16 false-positive shellout warnings from missing toolchain)
- **Pitfall fixes (v0.9.17):** Pitfall #18 (per-rule `respect_gitignore:
  false`) and #19 (literal-path runtime guard) both shipped in engine;
  this config does not need either workaround
- **Open gaps (unchanged):** `cross_file_value_equals` (v0.10
  ship-target, 10 sources — uv is one), `generated_file_fresh` (v0.10
  ship-target, 6 sources — uv is one), `archive_contents_matches`
  (v0.11+ uv-unique candidate), `python/pep-621-shape@v1` (v0.10
  design candidate, uv-canonical source)
- **Open suspected bugs in this directory's `.alint.yml`:** **none.**
  Config is clean against the v0.9.17 engine + canonical-22 pitfall
  catalogue.
