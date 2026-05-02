# Releasing alint

This file documents the contributor-side release flow. Most steps are
automated by the CI workflows referenced below — the human review
points are explicit.

## Cut a release

1. **Bump the workspace version.**

   ```sh
   # Edit Cargo.toml: workspace.package.version + workspace.dependencies.alint-*
   # Edit npm/package.json: version (release.yml rewrites at publish time
   #   but the source-of-truth should match).
   ```

2. **Update CHANGELOG.md.**

   Move entries from `## [Unreleased]` to a new `## [<x.y.z>] — YYYY-MM-DD`
   section. Add a one-paragraph summary at the top of the new section
   capturing the headline change. Keep the per-section shape (`### Added /
   Changed / Fixed / Removed / Deprecated / Security`) used throughout
   the file.

3. **Verify locally.**

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets
   cargo test --workspace
   RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace
   ./target/release/alint check        # dogfood
   ```

   The `release.yml` `preflight` job re-runs all four; this is the
   pre-push sanity gate.

4. **Commit and tag.**

   ```sh
   git add Cargo.toml npm/package.json CHANGELOG.md
   git commit -m "chore(release): bump workspace to <x.y.z>"
   git tag v<x.y.z>
   git push origin main
   git push origin v<x.y.z>
   ```

## What fires on the tag push

| Workflow | Triggered by | What it does | Time |
|---|---|---|---|
| `ci.yml` | tag + main pushes | fmt + clippy + test + doc + dogfood. Self-hosted Linux. | ~5 min |
| `release.yml` | tag push only | preflight gate → cross-platform build matrix → GitHub Release → ghcr.io Docker → npm → Homebrew tap → crates.io. | ~15-25 min |
| `docs-bundle.yml` | tag + main pushes | `xtask docs-export` → push refreshed bundle to `docs-bundle` branch → Cloudflare deploy hook → alint.org rebuilds. | ~3-5 min |
| `bench-docker.yml` | tag pushes | Build + push `ghcr.io/asamarts/alint-bench:<tag>` (the reproducible competitive-bench environment). | ~5 min |
| **`bench-record.yml`** | tag push only | **Self-hosted full publish-grade `xtask bench-scale` matrix (S1-S9 × {1k, 10k, 100k, 1m} × {full, changed}) at `--warmup 3 --runs 10`. Opens a PR adding the new per-version macro/results dir + criterion micro snapshot.** | **~3.5 hr** |

## Bench-record review (the human gate)

`bench-record.yml` opens a PR titled `docs(bench): <tag> bench-scale results`
when its run completes. Review checklist:

1. **CV check.** Skim the per-cell summary in the PR body for any cell with
   `stddev_ms / mean_ms > 0.10` (CV > 10 %). Re-run those on a quieter
   system (close other workloads on the bench runner; relaunch the
   workflow with `workflow_dispatch` and `--ref` set to the tagged commit)
   before merging. The 1M cells use reduced warmup/runs (`(min(warmup, 1),
   min(runs, 3))` per `xtask/src/bench/mod.rs`) and are inherently noisier
   — review the per-1M-cell stddev separately from the smaller sizes.

2. **Fingerprint check.** Open `results.json` and verify
   `fingerprint.alint_version` matches the tag, `fingerprint.cpu_model`
   matches the canonical baseline (AMD Ryzen 9 3900X), and
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
   (and the workspace member crates: `alint-core`, `alint-dsl`,
   `alint-rules`, `alint-output`).
2. Mark the npm package as `npm deprecate "@asamarts/alint@<x.y.z>"
   "Yanked: <reason> — upgrade to <x.y.z+1>"`.
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
