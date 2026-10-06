# Macro benchmarks (hyperfine bench-scale)

End-to-end CLI wall-time over deterministic synthetic monorepos at 1k /
10k / 100k / 1M files. Captures everything the user sees: walker +
engine + rules + formatters, plus per-platform syscall and page-cache
costs that micro-benchmarks deliberately exclude.

## How to run

```sh
xtask bench-scale                           # default: 1k/10k/100k, all five scenarios (S1-S4 full+changed, SFIX fix)
xtask bench-scale --include-1m              # adds the multi-GB 1M size
xtask bench-scale --scenarios S1,S4         # a subset by ID (S1, S2, S3, S4, SFIX)
xtask bench-scale --tools all               # alint + ls-lint + shell + Repolinter, each on the scenarios it supports
```

See [`../RUNNING.md`](../RUNNING.md) for the full flag list and the
publication-grade convention.

## Scenario catalogue

Each scenario is a single config YAML under
`xtask/src/bench/scenarios/` (`s1_layout.yml`, `s2_content.yml`,
`s3_relational.yml`, `s4_workspace.yml`, `sfix_all.yml`), embedded in the
xtask binary so a fresh clone produces byte-identical configs. The
`Scenario` enum in `xtask/src/bench/mod.rs` is the source of truth; rule
counts come from `facts.json`'s `bench_scenario_rule_counts`.

> **v0.17 consolidation.** The set was consolidated from the pre-v0.17
> S1-S14 to the five below, spanning four check cost axes plus a dedicated
> auto-fix pass. The IDs are **reused**: pre-v0.17 `S1`-`S14` are a different
> set; their per-scenario overviews and numbers are preserved in
> [`../legacy/HISTORY.md`](../legacy/HISTORY.md), and the raw result dirs in
> `results/legacy/`.

| ID | Rules | Cost axis and dispatch shape | Tool anchor | Absorbs (pre-v0.17) |
|---|---:|---|---|---|
| **S1** | 16 | Layout and path (walk-bound): filename class, existence / absence, path metadata, and a `scope_filter` shape. Walker + `GlobSet` with little or no content read, the cheapest path. | `ls-lint`, `shell` | old S1 / S10 + the layout half of S2 / S4 |
| **S2** | 20 | Per-file content: the 13 content kinds + forbidden-content patterns + `ordered_block` / `import_gate` / `xml_path_*` over `**/*.rs` (plus one `.csproj` overlay). The per-file dispatch fan-out. | `shell`, `repolinter` | old S2 / S5 / S6 / S12 |
| **S3** | 16 | Cross-file, relational and graph: `pair` / `unique_by` / the `for_each_*` family / registry / `cross_file` / `pair_hash`, three `file_graph` build-and-traverse passes, and one single-shot command spawn. The heaviest scenario. | — | old S7 / S11 / S13 / S14 |
| **S4** | 50 | Workspace bundle (realistic; release anchor): `extends` the `oss-baseline` + `rust` + `node` + `python` + `monorepo` + `cargo-workspace` rulesets over a POLYGLOT + GIT tree, `nested_configs` on, with the two git-aware rules inline. | — | old S3 / S8 / S9 |
| **SFIX** | 24 fix ops | Auto-fix (dedicated): `alint fix --unsafe-fixes --dry-run` over all 24 non-spawn fix ops (`FixSpec::ALL_OP_NAMES` minus the spawning ops) on the planted `sfix/` fixture. Runs under `fix` mode only. | — | old S5 (4-op fix pass) + the deterministic `sfix_trim` |

S2 / S3 / SFIX each plant a deterministic fixture overlay
(`Scenario::setup_overlay`); S1 / S4 need none. S4 is the only scenario
that `requires_polyglot_tree` + `requires_git_repo`.

## Tool matrix

`bench-scale` can run alint alongside other tools where the comparison
is honest. Each tool declares which `(scenario, mode)` combinations it
supports; unsupported combinations are filtered out automatically.

| Tool | Supports | Notes |
|---|---|---|
| `alint` | S1-S4 (full + changed); SFIX (fix only) | The harness defaults to alint-only. |
| `ls-lint` | S1 / full | Filename hygiene only. Closest single-tool competitor on S1. |
| `shell` | S1 / full, S2 / full | S1 uses `find` and GNU grep; S2 uses `test`, `find`, and ripgrep. Useful as a lower-bound reference, not a semantic equivalent. |
| `repolinter` | S2 / full | The retired-2026 ancestor. Run via Docker per `bench-docker.yml` workflow. |

`--tools all` expands to every available tool; tools not on PATH are
auto-skipped with a stderr note rather than aborting.

## Reproducible competitive runs (`--docker`)

Comparing alint vs ls-lint vs shell pipelines vs Repolinter on a developer's
laptop is dishonest: each laptop has a different `ls-lint` version,
different `find`, grep, and ripgrep versions, a different Node runtime under
Repolinter, even (depending on rust-toolchain.toml) a different
`alint`. Numbers from such a run aren't comparable to any other
machine's run.

The `--docker` flag fixes this. `xtask bench-scale --docker --tools
all …` runs the entire matrix inside `ghcr.io/asamarts/alint-bench:<tag>`,
a published image that pins:

- `alint` — built from the bind-mounted checkout at run time with the image's
  pinned compiler; its version and Git SHA are recorded in the fingerprint.
- `ls-lint` — pinned `v2.2.3`.
- GNU find and grep from the pinned Debian base image; their exact runtime
  versions are recorded in every result fingerprint.
- `ripgrep` (used by the S2 shell baseline) — pinned `15.1.0`.
- `repolinter` — pinned `0.11.2`.
- Node.js (the Repolinter runtime) — pinned `20.20.2`.
- `hyperfine` — pinned `1.20.0`.
- `rustc` — bench-only `1.88.0`, pinned by the image independently of
  alint's user-facing MSRV and recorded in the result fingerprint.

A given image tag (e.g. `0.17.0`) is therefore the canonical
*"competitive bench environment for v0.17.0."* Bumping any tool's
version requires re-publishing the image and re-running the
competitive numbers — the image tag IS the methodology version.

Before hyperfine times a competitive S1/S2 row, the harness plants a
scenario-specific violation and requires the tool to report its filename or
rule ID with the expected exit status. Only tool-specific finding statuses are
ignored during timing. Runs that include both 1k and 100k also warn when the
100k/1k ratio is below 2×. These checks prevent a missing or malformed config
from becoming a deceptively fast benchmark result.

### Where the image lives

| Asset | Path |
|---|---|
| Build context (just the Dockerfile) | [`bench/Dockerfile`](../../../bench/Dockerfile) at the repo root |
| `.dockerignore` (aggressively scopes the build context to the Dockerfile only) | [`bench/.dockerignore`](../../../bench/.dockerignore) |
| Build/publish workflow | [`.github/workflows/bench-docker.yml`](../../../.github/workflows/bench-docker.yml) |
| Published image | `ghcr.io/asamarts/alint-bench:<tag>` |

### Workflow

The image is built + pushed by `bench-docker.yml` on tag pushes
and on manual workflow-dispatch. Image tags follow the alint
release tags 1:1 (`v0.17.0` → `ghcr.io/asamarts/alint-bench:0.17.0`),
plus a rolling `latest`. The `xtask --docker` flag's bind-mount
shape is documented in the Dockerfile header.

`xtask bench-scale --docker` is the canonical entry point;
direct `docker run …` invocation works too because the image's
entrypoint forwards all args to `xtask bench-scale`.

### When you DON'T need it

The Docker image only matters for `--tools all` runs. alint-only
runs (`xtask bench-scale --tools alint`, the default) read the
freshly-built workspace `alint` binary directly — no Docker
involvement, and no portability concern because alint's own
version is captured in the fingerprint header. The Docker image
exists specifically because `ls-lint` / `grep` / `repolinter`
have NO native version-pinning hook in our harness.

## Tree shape

The synthetic monorepo generator at `crates/alint-bench/src/tree.rs`
produces deterministic Cargo-workspace-shaped trees:

| Size | Packages | Files / package | Total | Use |
|---|---:|---:|---:|---|
| 1k | 50 | 18 | 1,001 | Smoke test; per-PR sanity. |
| 10k | 200 | 48 | 10,001 | Most-PRs default; runs in seconds. |
| 100k | 1,000 | 98 | 100,001 | CI publish; runs in tens of seconds. |
| 1m | 5,000 | 198 | 1,000,001 | Pre-release publication; multi-minute. Opt-in via `--include-1m`. |

`xtask gen-monorepo --size <label> --out <path>` materialises the same
tree at a fixed path for ad-hoc profile work — see
[`../investigations/README.md`](../investigations/README.md).

S4 uses the git-aware polyglot generator
(`generate_git_nested_polyglot_monorepo`), which runs
`git init && git add -A && git commit` after generation so the engine's
git-aware rules (`git_no_denied_paths` / `git_tracked_only`) and
`BlameCache` actually fire.

## Where results live

```
results/
├── linux-x86_64/                ← the consolidated 5-scenario series (kbench)
│   ├── v0.10.0/results.json
│   ├── …
│   └── v0.17.0/results.json     ← latest published
├── legacy/
│   └── linux-x86_64/            ← the pre-v0.17 14-scenario results
└── linux-x86_64-ryzen-3900x/    ← the older 3900X host, pre-kbench
```

Each per-version `results.json` is the output of one `xtask bench-scale`
run with the publication-grade flags (`--warmup 3 --runs 10`, auto-reduced
to `--warmup 1 --runs 3` at the 1M size). Its `fingerprint` header carries
the full hardware and tool-version provenance; the cross-version headline
table is rendered into [`../HISTORY.md`](../HISTORY.md) by
`xtask/scripts/render-history.py`.

## Adding a new scenario

1. Author `xtask/src/bench/scenarios/s<N>_<topic>.yml` following the
   shape of the existing scenario files (header comment explaining the
   dispatch shape the scenario stresses).
2. Extend `xtask::bench::Scenario` with the new variant in `mod.rs`
   (parse / label / description / config_yaml; if it needs a real git
   repo, set `requires_git_repo()` to `true`).
3. Update the `tools.rs` `GrepPipeline::supports` match arm if the new
   scenario can't be approximated by a grep pipeline.
4. Document the scenario in this README's catalogue table.
5. Run `xtask bench-scale --scenarios S<N>` at 1k for smoke-test, then
   at the publication sizes (1k/10k/100k or 1k/10k/100k/1m).

The `coverage_audit_bench_listing.rs` soft warning emits which rule
kinds aren't yet exercised by any scenario — useful as a triage list
when picking what shape to add next.

## Regression gate

`bench-compare` consumes criterion-format directories (so it runs on
the micro side, not the macro side). For macro regressions, the gate
is a manual cross-version comparison: read the headline cells in
[`../HISTORY.md`](../HISTORY.md), run the new release's bench, file
an investigation if any cell drifts > 20 %.
