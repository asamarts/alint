# Releasing alint

This file documents the contributor-side release flow. Most steps are
automated by the CI workflows referenced below; the human review
points are explicit.

## Cut a release

1. **Bump the workspace version.**

   ```sh
   bash ci/scripts/bump-version.sh <new-version>   # e.g. 0.9.21
   ```

   The script edits `Cargo.toml [workspace.package].version` + every
   user-facing install snippet (README, SECURITY, docs/site/**) +
   inserts a CHANGELOG stub + refreshes `Cargo.lock` via
   `cargo metadata --offline` (so the workspace internal-crate
   version entries in the lockfile track the bump).

   Deliberately **not** touched:
   - `Cargo.toml [workspace.dependencies].alint-* version`,
     the intra-workspace API-compat floor. Bump by hand only when an
     inter-crate API actually breaks. Preflight asserts each floor
     is `<= workspace.package.version` via
     `ci/scripts/check-workspace-dep-floors.sh`; an over-pinned floor
     would publish-fail, so the check catches it before push.

   `npm/package.json` version was previously left lagging
   (rewritten by `release.yml` at publish time). As of v0.9.22 the
   bump script tracks it alongside the install snippets, so the
   committed value at HEAD matches what users will see post-publish.
   Preflight asserts this via `ci/scripts/check-version-pins.sh`.

2. **Update CHANGELOG.md.**

   Move entries from `## [Unreleased]` to a new `## [<x.y.z>], YYYY-MM-DD`
   section. Add a one-paragraph summary at the top of the new section
   capturing the headline change. Keep the per-section shape (`### Added /
   Changed / Fixed / Removed / Deprecated / Security`) used throughout
   the file.

3. **Verify locally.**

   ```sh
   bash ci/scripts/preflight.sh
   ```

   Runs the full preflight bundle: fmt + clippy + test + doc +
   version-pins + dep-floors + dogfood. The
   `release.yml` `preflight` job runs the same gates remotely; this
   is the pre-push sanity gate.

   Two recurrence guards bundled into `test` / `dep-floors`:
   - README-count claims (rule kinds, families, bundled rulesets,
     fix ops, output formats, subcommands) are asserted against
     the workspace truth by
     `crates/alint-e2e/tests/coverage_audit_readme_claims.rs`.
   - `[workspace.dependencies]` API-compat floors are asserted
     `<= workspace.package.version` by
     `ci/scripts/check-workspace-dep-floors.sh`.

   The pre-push git hook at `ci/githooks/pre-push` runs the same
   script automatically once opted in via
   `git config core.hooksPath ci/githooks`.

4. **Commit and tag.**

   ```sh
   git add Cargo.toml Cargo.lock npm/package.json CHANGELOG.md
   git commit -m "chore(release): bump workspace to <x.y.z>"
   git tag v<x.y.z>
   git push origin main
   git push origin v<x.y.z>
   ```

   **Why `Cargo.lock` is in the stage list.** When
   `[workspace.package].version` bumps, the workspace internal-crate
   entries in `Cargo.lock` (`alint`, `alint-core`, `alint-dsl`,
   `alint-rules`, `alint-output`, `alint-lsp`, `alint-bench`, `alint-e2e`,
   `alint-testkit`) need to track. `bump-version.sh` refreshes the
   lockfile via `cargo metadata --offline` as part of the bump; the
   refresh must be committed alongside `Cargo.toml`, or CI's
   `cargo build --locked` in `release-binary.sh` fails with "cannot
   update the lock file because --locked was passed". This was
   caught on v0.9.22 (`release.yml` run `25890555488` failed at the
   cross-platform build matrix; recovered via tag-move after
   amending the bump commit to include `Cargo.lock`).

## Documentation and site-drift (per release)

Release-aware documentation is enforced mechanically (ADR-0007; see
`docs/design/v0.14/documentation-drift.md`), but a cut still has a short
checklist so the release-gated pieces land and nothing drifts:

1. **Newly-released options and prose.** Any option gated with an `x-since` for
   this version now ships; unwrap it at the source. This is cosmetic (the gates
   strip these pre-release and `--released-version` catches up at the tag), but
   it keeps the source honest.
   - **Schema `x-since`.** For a schemars-migrated kind the keyword is
     type-derived, so remove the `#[schemars(extend("x-since" = "<ver>"))]`
     attribute from the option's Rust struct, then run `gen-schema`. Editing
     `schemas/v1/config.json` directly is silently reverted by `gen-schema` and
     fails `--check`. For a hand-authored branch, drop the keyword from
     `config.json`, then `gen-schema`.
   - **Prose sentinels.** Unwrap every `<!-- alint:since=<ver> -->` block; grep
     `alint:since` across `docs/rules.md` and `docs/site/reference/**` to find all.
   For v0.14: remove `#[schemars(extend("x-since" = "0.14"))]` from
   `crates/alint-rules/src/{file_absent,dir_absent,dir_exists}.rs` (`root_only`),
   and unwrap the six `alint:since=0.14` blocks: five in `docs/rules.md`
   (`root_only` on `file_absent`/`dir_absent`/`dir_exists`, plus
   `no_zero_width_chars` and `no_symlinks`) and one in
   `docs/site/reference/output-formats/index.md`.
2. **Claims whose scope changed.** Narrow any published claim the cut walked
   back. The Kani proof is scoped to the *lexical* path-confinement policy since
   the post-v0.13 H1 fix, so `roadmap.json` / CHANGELOG wording must not imply
   the filesystem layer is machine-checked (E2).
3. **New user-facing features need narrative site docs.** A new subcommand
   auto-documents from clap `--help` and counts flow from `facts.json`, but
   guide/reference prose does not. For v0.14: baseline mode needs a concept
   page, the `baseline:` configuration-reference entry, and an output-formats
   suppression note. Pages under tag-pinned `docs/site/**` (configuration,
   cookbook, getting-started) are safe to pre-write; the output-formats note is
   under the main-overlaid `docs/site/reference/**`, so wrap any unreleased part
   in `<!-- alint:since=X -->`.
4. **Reconcile the trackers.** Bump `alint.org`'s pins (four install sites plus
   prose claims), and reconcile `alint.org` `marketing/STATE.md` to the new
   version and counts so it never drifts weeks behind again (P5.2).
5. **Confirm the gates are green.** `gen-schema` / `gen-facts` / `docs-export`
   `--check` and the `docs/adr@v1` dogfood here; the `check-counts.mjs` count
   gate and `check-version-pins.sh` pin gate on the alint.org side.

## What fires on the tag push

| Workflow | Triggered by | What it does | Time |
|---|---|---|---|
| `ci.yml` | tag + main pushes | fmt + clippy + test + doc + dogfood, plus audit, deny, build, bench-smoke, examples, shell-tests, editors, and the advisory perf-gate. Self-hosted Linux. | ~5 min |
| `release.yml` | tag push only | preflight gate → supply-chain (SBOM + license bundle) → cross-platform build matrix → GitHub Release (cosign-signed `SHA256SUMS` + build-provenance + SBOM attestations) → ghcr.io Docker (attested + cosign-signed by digest) → npm → Homebrew tap → crates.io → VS Code Marketplace + Open VSX → JetBrains Marketplace. | ~15-25 min |
| `docs-bundle.yml` | tag + main pushes | `xtask docs-export` → push refreshed bundle to `docs-bundle` branch → Cloudflare deploy hook → alint.org rebuilds. The sibling `check-pins.yml` workflow in the alint.org repo (PR + push + daily cron) asserts alint.org's three install-pin sites reference the latest tag from this release; fires automatically. | ~3-5 min |
| `bench-docker.yml` | tag pushes | Build + push `ghcr.io/asamarts/alint-bench:<tag>` (the reproducible competitive-bench environment). | ~5 min |
| **`bench-record.yml`** | tag push only | **Self-hosted full publish-grade `xtask bench-scale` matrix (S1-S14 × {1k, 10k, 100k, 1m} × {full, changed}) at `--warmup 3 --runs 10`. Opens a PR adding the new per-version macro/results dir + criterion micro snapshot.** | **~3.5 hr** |

Signing and attestation (the `release`/`docker` cosign + `attest-build-provenance`
/ `attest-sbom` steps) are **keyless** via Sigstore + GitHub OIDC: they add
`id-token: write` + `attestations: write` job permissions (the image path also
uses the `docker` job's existing `packages: write` to store its signature and
attestation in ghcr), no stored secret, so there is nothing here to rotate or that
can expire mid-release. Consumer-side verification commands live in
[SECURITY.md](SECURITY.md#verifying-release-artifacts).

## Adding a new published crate

When a new workspace crate joins the crates.io publish list
(`ci/scripts/publish-crates.sh` `CRATES=()`), **register its crates.io
Trusted Publisher before the release that first publishes it via OIDC**:
crate → Settings → Trusted Publishing → repo `asamarts/alint`, workflow
`release.yml`, no environment. The OIDC token is scoped per crate, so a
crate without a publisher entry fails that release's `publish to
crates.io` job with `403 ... access token is not valid for crate
<name>`, *after* the earlier crates have already published (crates.io
is permanent; see Yanking). The publish script is idempotent, so the
recovery is: add the publisher, then `gh run rerun <id> --failed`
(never re-tag). `alint-lsp` hit this on v0.11.1.

## Recovering a partial release

Most publish jobs run *after* the GitHub Release exists (`needs: release`), but
`docker` and `publish-crates` gate on `build` instead, so a failure once they have
run leaves a split-brain state (crates.io / ghcr published, the Release or a token
channel stale). Recovery is always **rotate or fix, then re-run the failed job,
never re-tag** (crates.io / npm / ghcr are permanent, and a new tag would collide):

- **A supply-chain generation failure** (`MP-M6`). The `supply-chain` job (the
  CycloneDX SBOM + third-party license bundle) is a pre-`build` gate: `build` needs
  it so each tarball and the image can embed `THIRD-PARTY-LICENSES.html`. If it
  fails, `build` and everything downstream are skipped, so nothing publishes
  (fail-closed, not a partial release). Fix the generator and
  `gh run rerun <id> --failed`; ci.yml runs the same script pre-merge (the
  `supply-chain` job), so a release-time failure should be rare.
- **A signing / attestation failure** (`MP-H1`). The `release` job's cosign + attest
  steps run *before* `create GitHub Release` and depend on public-good Sigstore
  (Fulcio + Rekor) plus the GitHub attestations API. A transient Sigstore outage, or
  a cosign / attest-action error, fails the release job before the Release exists,
  while `docker` and `publish-crates` (both `needs: build`) may already have
  published to ghcr / crates.io. Re-run the release job: the cosign + attest steps are
  idempotent and precede Release creation, so a clean re-run re-signs and then creates
  the Release. Never re-tag.
- **Expiring credentials** (`MP-M2`). Four channels carry secrets that can expire: the
  npm PAT (`NPM_TOKEN`), VS Code + Open VSX (`VSCE_PAT` / `OVSX_PAT`), JetBrains (the
  marketplace token + signing cert/key), and the Homebrew tap SSH deploy key. On a
  401/403 from any, rotate the secret (inventory + storage in
  [`release-credentials.md`](docs/development/release-credentials.md)), then
  `gh run rerun <id> --failed`. (npm OIDC trusted publishing is now GA and will retire
  the `NPM_TOKEN` PAT once the package migrates to per-platform sub-packages.)
- **A build-matrix flake blocking crates.io** (`MP-M1`). `publish-crates` deliberately
  `needs: build` (a cross-platform compile gate before the irreversible publish), so a
  flaky windows or aarch64 leg can block it. Re-run once the leg heals with
  `gh run rerun <id> --failed`; the publish script is idempotent, so already-published
  crates are skipped.

## Editor extensions / IDE plugins

The six editor integrations live under `editors/`. Two distribution
paths:

**Token-published on the tag (automated in `release.yml`):**

| Job | Publishes to | Secrets required |
|---|---|---|
| `publish-vscode` | VS Code Marketplace (`vsce`) **+ Open VSX** (`ovsx`) | `VSCE_PAT`, `OVSX_PAT` |
| `publish-jetbrains` | JetBrains Marketplace (`gradle publishPlugin`) | `JETBRAINS_MARKETPLACE_TOKEN`, `JETBRAINS_CERTIFICATE_CHAIN`, `JETBRAINS_PRIVATE_KEY`, `JETBRAINS_PRIVATE_KEY_PASSWORD` |

These stamp the version from the tag (`v0.x.y` → `0.x.y`), so the
committed `package.json` / `pluginVersion` can lag. A token 401 mid-run
is recoverable the same way as npm: rotate + `gh run rerun <id>
--failed`, no new tag (the `.vsix` / plugin `.zip` are idempotent per
version).

**JetBrains Marketplace internal-API rejections** are a *separate* class
of mid-release failure: the Marketplace's validator rejects references
to certain platform-internal classes that `intellij-plugin-verifier`
does NOT flag (e.g. `PluginManagerCore.getPlugin(PluginId)` is rejected
at upload but not annotated `@ApiStatus.Internal` in any released-IDE
bytecode, so `verifyPlugin` reports `Compatible`). The
`verifyNoMarketplaceDeniedApis` gradle task (in
`editors/jetbrains/build.gradle.kts`) scans the built jar's constant
pool for a deny-list of such classes, runs as a `buildPlugin` finalizer
(so every path that produces the zip, including `verifyPlugin`,
`signPlugin`, and `publishPlugin`, picks it up), and points at the
public alternative. When Marketplace
moderation flags a new internal API, add the offending FQN (slashed
JVM form) to the `deniedClasses` list and the gate will catch it
pre-tag. See <https://plugins.jetbrains.com/docs/intellij/api-internal.html>.

See [`docs/development/release-credentials.md`](docs/development/release-credentials.md)
for the full credential inventory, the secret-storage convention, and
the OIDC (keyless) publishing setup for crates.io + npm.

**One-time prerequisites (completed before the first editor release;
retained here as the setup record):**

1. Create the **VS Code Marketplace publisher** `asamarts` (Azure
   DevOps) and an **Open VSX** namespace; generate `VSCE_PAT` / `OVSX_PAT`.
2. Create the **JetBrains Marketplace vendor**; generate the marketplace
   token + a plugin signing certificate/key.
3. Add all of the above as repo **Actions secrets**.
4. Manual `runIde` smoke of the JetBrains plugin (does the LSP server
   attach in a live IDE?) and an install-from-`.vsix` smoke of the VS
   Code extension. The `editors` CI job already build-validates all
   three packaged plugins on every `editors/**` change.

**PR-based registries (manual, NOT automated; do after the release so
the binaries exist on GitHub Releases):**

| Editor | Submit |
|---|---|
| Zed | PR adding `editors/zed/` to [`zed-industries/extensions`](https://github.com/zed-industries/extensions) (registry builds the wasm from source) |
| Neovim | PR upstreaming `editors/nvim/lsp/alint.lua` to [`neovim/nvim-lspconfig`](https://github.com/neovim/nvim-lspconfig) |
| Emacs | MELPA recipe PR for `editors/emacs/alint.el` to [`melpa/melpa`](https://github.com/melpa/melpa) |
| Sublime | (optional) build + submit an `LSP-alint` Package Control helper |

Helix / Eclipse are docs-only (config snippets): nothing to publish.

## Bench-record review (the human gate)

> **Regression detection is the deterministic `perf-gate` CI job**
> (`ci/scripts/det-perf-gate.sh`, per-PR; design:
> [`docs/design/deterministic-perf-gating.md`](docs/design/deterministic-perf-gating.md)).
> It compares Valgrind instruction/cache/branch counts PR-vs-merge-base and is
> **load-immune**, so it catches regressions deterministically where wall-clock
> `bench-scale` cannot. It runs **advisory today** (`DET_PERF_ADVISORY=1` in
> `ci.yml`: it annotates but cannot fail the build, and is excluded from the
> `summary` gate); flip it to enforcing by setting `DET_PERF_ADVISORY=0` and
> adding `perf-gate` to `summary.needs`. The 2026-06 investigation proved the contaminated shared
> runner makes wall-clock regression %s unreliable (v0.11.1 AND v0.12.0 both
> contaminated; see
> [`docs/benchmarks/investigations/2026-06-v0.12-perf-validation/`](docs/benchmarks/investigations/2026-06-v0.12-perf-validation/)).
>
> So **`bench-record` / `bench-scale` is now CHARACTERIZATION** (the published
> absolute throughput + cross-tool numbers), **not the regression gate.** The
> wall-clock `bench-gate` (below) is **only meaningful on a verified-quiet
> runner**: treat a wall-clock "regression" on a busy runner as contamination
> until the deterministic gate (or a quiescent re-run) confirms it. At release
> time also run the 100k deterministic tier:
> `cargo bench -p alint-bench --bench det_check --features det-100k`.
>
> **Caveat: the deterministic gate is I/O-blind.** It confirms *compute/cache*
> regressions; a read / `stat` / spawn-path regression barely moves `Ir` (a syscall is
> ~constant guest instructions regardless of kernel time), so the gate will NOT confirm
> it even though it is real: v0.14.0's S2 read regression passed a flat `Ir` gate and
> only the wall-clock bench caught it
> ([`docs/benchmarks/investigations/2026-07-v0.14-s2-harness-artifact/`](docs/benchmarks/investigations/2026-07-v0.14-s2-harness-artifact/)).
> So a wall-clock regression the deterministic gate does not confirm is *not*
> automatically contamination. If the flagged cells are content-read-heavy (S2 / S6 /
> S12) and the diff touched the read / open / spawn path, disambiguate with a syscall
> count or a same-box quiescent A/B, not the deterministic gate alone.

`bench-record.yml` opens a PR titled `docs(bench): <tag> bench-scale results`
when its run completes. Review checklist:

1. **Run the wall-clock gate (characterization; quiescence-sensitive).**
   `xtask bench-gate` records absolute throughput. It supersedes the old "skim
   the PR body for any cell with `stddev_ms / mean_ms > 0.10`" eyeball: that flat
   per-cell CV rule was never met by any shipped release (chronic
   small/10k measurement-floor noise) and was never enforced in
   code. Evidence + the validated thresholds:
   [`docs/benchmarks/investigations/2026-05-bench-runner-instability/`](docs/benchmarks/investigations/2026-05-bench-runner-instability/).

   ```sh
   tag=v<x.y.z>; prev=v<x.y.z-1>
   git fetch origin "bench-record/$tag"
   git show "origin/bench-record/$tag:docs/benchmarks/macro/results/linux-x86_64/$tag/results.json" > /tmp/new.json
   cargo run -q -p xtask -- bench-gate \
     --results /tmp/new.json \
     --baseline "docs/benchmarks/macro/results/linux-x86_64/$prev/results.json"
   ```

   - **Quality**: per-cell within-run CV `<= 10%`, applied only
     to 100k and 1m cells. 1k/10k are advisory (chronic
     measurement-floor noise; reported, never blocking). A gating
     failure (a 100k+ cell over 10%) means a genuinely bad run:
     re-run via `workflow_dispatch` (`-f ref=v<x.y.z> -f
     label=v<x.y.z>`) on an idle runner.
   - **Regression**: `min_ms` delta vs the previous release's
     `results.json` `<= +15%` on cells of size `>= 10k` (`min_ms`
     is the reproducible cross-version statistic; `1k` excluded).
     A failure here is a real perf regression: open an
     investigation (step 4) before merging.

   Non-zero exit means do not merge. Advisory lines never block.
   The published `HISTORY.md` table still shows `mean +/- stddev`
   (the gate uses `min_ms`; the table keeps the full distribution
   for continuity, see `docs/benchmarks/METHODOLOGY.md`).

2. **Fingerprint check.** Open `results.json` and verify
   `fingerprint.alint_version` matches the tag, `fingerprint.cpu_model`
   matches the canonical baseline (Intel Core i7-6700HQ, host `kbench`;
   the retired 3900X series is at `results/linux-x86_64-ryzen-3900x/`), and
   `fingerprint.os` is `linux`. A bench run on the wrong machine voids
   cross-version comparability.

3. **HISTORY.md update.** The PR body includes a `Per-cell numbers`
   block formatted as `- SX <size> <mode>: <mean> ms ± <stddev>`.
   Paste the relevant cells into `docs/benchmarks/HISTORY.md`:
   - The cross-version trajectory table at the top: a new headline row
     for the released version (S3 columns).
   - Each per-scenario section's `full` and `changed` table: new row at
     the top for the released version.
   The bench-record PR currently does NOT auto-edit HISTORY.md
   (positional markdown table edits are too fragile to autocomment-into);
   the maintainer is the canonical paste channel.

4. **Investigation hand-off.** If a cell drifts > 20 % vs the previous
   release (and the CV is below 10 % so it's a real signal), open
   `docs/benchmarks/investigations/<YYYY-MM-topic>/README.md` capturing
   the diagnostic data (traces, flamegraphs, bisect notes) before
   merging the bench-record PR. The HISTORY.md entry then links to the
   investigation.

5. **Merge.** Once the above are done, merge the PR. The bench numbers
   enter the published corpus.

## Off-cycle bench runs

For characterising a specific commit between releases (e.g. a perf
investigation), trigger `bench-record.yml` via `workflow_dispatch`:

- `ref`: the commit SHA or branch to bench. Defaults to `main`.
- `label`: the output dir label. If blank, derives `v<workspace-version>-rc-<short-sha>`.

This produces an off-corpus bench dir under
`docs/benchmarks/macro/results/linux-x86_64/<label>/` which the
investigation references directly. Off-cycle dirs are NOT added to
HISTORY.md (the cross-version table is release-tag-only).

## Yanking a broken release

`crates.io` only supports yank, not delete. If a published release
contains a critical bug:

1. Yank from crates.io via `cargo yank --version <x.y.z> -p alint`
   (and every published library crate, per `ci/scripts/publish-crates.sh`:
   `alint-core`, `alint-rules`, `alint-output`, `alint-dsl`, `alint-lsp`).
2. Mark the npm package as `npm deprecate "@asamarts/alint@<x.y.z>"
   "Yanked: <reason>; upgrade to <x.y.z+1>"`.
3. Delete the GitHub Release (the asset tarballs stay accessible via
   the tag, but the Release page disappears so install.sh can fail
   loud rather than silently downloading the broken version).
4. Cut the next patch release immediately with the fix; CHANGELOG
   notes the yank explicitly.
5. Update the previous CHANGELOG section's headline with a
   `**(Yanked YYYY-MM-DD: <reason>; upgrade to <x.y.z+1>.)**` prefix.

## Release version policy

Patch (`<x>.<y>.<z+1>`):
- Bug fixes that don't change the documented surface.
- Bench-shape regressions corrected back to baseline.
- Doc/integration updates, dependency bumps, supply-chain hardening.

Minor (`<x>.<y+1>.0`):
- New rule kinds, formatters, CLI flags.
- New bundled rulesets.
- Anything that changes the documented surface but doesn't break
  existing config files.

Major (`<x+1>.0.0`):
- Breaking changes to `.alint.yml` schema (deprecation period
  announced one minor in advance).
- `Rule` trait additions that aren't backwards-compatible (only
  relevant once external plugin authors exist; today the trait is
  effectively crate-private).
- `Engine` API breaking changes for `alint-core` consumers.

Scope:
- The contract covers the **observable product surface** only: the
  `.alint.yml` schema, the CLI (flags, subcommands, exit codes),
  and the machine-readable output formats. The `alint-core` public
  API joins at v1.0 (pre-1.0 the engine API + `Rule` trait are
  effectively crate-private; see the `publish = false` members).
- The GitHub Action wrapper (`action.yml`, incl. its input
  defaults and ref/binary-pin behaviour), `install.sh`, the
  npm / Homebrew / Docker shims, benchmark numbers, and the docs
  site are **integration surface, not the contract**: changes
  there are **patch** even when they warrant an "Action required
  only if ..." edge-case caveat (e.g. the v0.9.23 `version:`-default
  shift: a patch with a caveat, not a minor).
- Tie-breaker when unsure: does an unchanged `.alint.yml`, run
  with the same CLI invocation, still produce the same findings
  and the same machine-readable output? Only a "no" forces
  minor/major; an integration-default shift with an upgrade note
  stays patch.
