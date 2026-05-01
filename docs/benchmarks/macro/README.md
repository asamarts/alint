# Macro benchmarks (hyperfine bench-scale)

End-to-end CLI wall-time over deterministic synthetic monorepos at 1k /
10k / 100k / 1M files. Captures everything the user sees: walker +
engine + rules + formatters, plus per-platform syscall and page-cache
costs that micro-benchmarks deliberately exclude.

## How to run

```sh
xtask bench-scale                           # default: 1k/10k/100k × S1/S2/S3 × full/changed
xtask bench-scale --include-1m              # adds the multi-GB 1M size
xtask bench-scale --tools all               # alint + ls-lint + grep + Repolinter on supported scenarios
xtask bench-scale --scenarios S6,S7,S8      # opt-in to characterization scenarios
```

See [`../RUNNING.md`](../RUNNING.md) for the full flag list and the
publication-grade convention.

## Scenario catalogue

Each scenario is a single config YAML under
`xtask/src/bench/scenarios/`, embedded in the xtask binary so a fresh
clone produces byte-identical configs.

| ID | Rules | Dispatch shape | Why it exists | Catches |
|---|---|---|---|---|
| **S1** | 8 filename-only (`filename_case`, `filename_regex`) | Pure walker + glob match; no content read | Narrowest scope alint shares with `ls-lint` — competitive comparison | Walker / scope-match regressions |
| **S2** | 8 existence + content (`file_exists`, `file_absent`, `file_content_forbidden`, `file_max_size`) | Walker + per-file content scan over narrow scopes | Repolinter-comparable shape | Content-rule regressions on common shapes |
| **S3** | Workspace bundle: `extends: oss-baseline + rust + monorepo + cargo-workspace` (≈34 rules) | Heavy mix — content rules over `**/*.rs`, cross-file `for_each_dir` over `crates/*`, `toml_path_matches` per crate | Realistic monorepo workload | Mixed regressions; the v0.9.5 cliff that triggered the path-index fix lived here |
| **S4** | 5 agent-era hygiene rules (`file_absent`, `file_content_forbidden`) | Filename + content fan-out over agent-shaped trees | Mirrors the v0.6 `agent-hygiene` bundled ruleset | Agent-era rule shapes |
| **S5** | 4 fix-pass content edits (`final_newline`, `no_trailing_whitespace`, `line_endings`, `no_bom`) | `--fix` end-to-end: read, transform, atomic-rename | The only `--fix`-mode bench | Fix-pipeline regressions |
| **S6** | 13 content rules over `**/*.rs` | Per-file dispatch path width — every `.rs` file hit by every rule on a single read | Stresses the read-coalescing path; v0.9.3 dispatch flip's design target | Per-file inner-loop regressions S3 doesn't surface |
| **S7** | 6 cross-file relational kinds (`pair`, `unique_by`, `for_each_dir`, `for_each_file`, `dir_only_contains`, `every_matching_has`) | Various fan-out shapes over the synthetic monorepo | Catches the next O(D × N) cliff after the v0.9.5 path-index fix | Cross-file dispatch shapes the path-index doesn't cover |
| **S8** | S3 reshape + `git_no_denied_paths` + `git_tracked_only` over a real git repo | Same as S3 but with `Engine::collect_git_tracked_if_needed` + `BlameCache` active | v0.7-era `git ls-files` regression had no scale gate; this fixes that | Git-aware dispatch regressions at scale |

## Tool matrix

`bench-scale` can run alint alongside other tools where the comparison
is honest. Each tool declares which `(scenario, mode)` combinations it
supports; unsupported combinations are filtered out automatically.

| Tool | Supports | Notes |
|---|---|---|
| `alint` | every (scenario, mode) | The harness defaults to alint-only. |
| `ls-lint` | S1 / full | Filename hygiene only. Closest single-tool competitor on S1. |
| `grep` | S1 / full, S2 / full | Pure regex pipeline; useful as a "lower bound" reference. Doesn't model rule semantics. |
| `repolinter` | S2 / full | The retired-2026 ancestor. Run via Docker per `bench-docker.yml` workflow. |

`--tools all` expands to every available tool; tools not on PATH are
auto-skipped with a stderr note rather than aborting.

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

S8 uses a parallel `generate_git_monorepo` variant that runs
`git init && git add -A && git commit` after generation so the engine's
git-aware paths actually fire.

## Where results live

```
results/
└── linux-x86_64/
    ├── v0.5.6/1m/                ← only 1m subset captured at v0.5.6
    ├── v0.5.7/                   ← 1k/10k/100k publication
    │   ├── 1k/results.md
    │   ├── 10k/results.md
    │   ├── 100k/results.md
    │   ├── index.md              ← aggregated summary
    │   └── results.json          ← machine-readable for cross-version diffs
    ├── v0.9.4/
    └── v0.9.5/                   ← latest published
```

Each per-version dir is the output of one `xtask bench-scale` run with
the publication-grade flags (`--warmup 3 --runs 10` by default; v0.9.5
used `--warmup 1 --runs 3` because the path-index fix dropped wall time
below where 10 measurements add meaningful signal). The `index.md`
header carries the full hardware fingerprint.

## Adding a new scenario

1. Author `xtask/src/bench/scenarios/s<N>_<topic>.yml` following the
   shape of S6 / S7 / S8 (header comment explaining the dispatch shape
   the scenario stresses).
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
