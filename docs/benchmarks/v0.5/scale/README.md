# Scale-ceiling benchmarks (v0.5)

Macro-benchmarks that document where alint scales smoothly and where it doesn't, on synthetic monorepo trees of 1k / 10k / 100k files (and optionally 1M). Numbers are committed per-platform under `<os>-<arch>/`; cross-platform comparisons require like-for-like fingerprints (see [`methodology.md`](methodology.md)).

## What's measured

Three rule-set scenarios × two evaluation modes × four sizes:

| Scenario | Rules | Hot path |
|---|---|---|
| **S1** Filename hygiene | 8 filename rules | walker + globset |
| **S2** Existence + content | 8 layout / content rules | walker + content-IO + regex |
| **S3** Workspace bundle | `oss-baseline + rust + monorepo + monorepo/cargo-workspace` | walker + cross-file rules + iteration |

| Mode | What it measures |
|---|---|
| `full` | Every file evaluated. The status quo. |
| `changed` | `alint check --changed` against a deterministic 10% diff. The v0.5.0 incremental path. |

Per-tree timing comes from `hyperfine` (3 warmup + 10 measured runs by default, `--export-json`). Hardware fingerprint (CPU / RAM / FS / kernel / rustc / hyperfine) lives in each `results.json` header.

## Reproducing locally

A bare `cargo xtask bench-scale` produces a complete report in your platform's directory. The harness builds alint in release mode, generates the synthetic trees deterministically (default seed `0xa11e47`), and writes both `results.json` and per-size Markdown.

```bash
# Full report (default sizes 1k / 10k / 100k, all scenarios, both modes).
cargo xtask bench-scale

# Subset — single size / scenario / mode.
cargo xtask bench-scale --sizes 10k --scenarios S2 --modes full

# Including the 1M-file size (multi-GB working set, several minutes).
cargo xtask bench-scale --include-1m --sizes 1m

# Smoke-test the harness itself (single 1k/S1/full row, fast).
cargo xtask bench-scale --quick

# JSON only — skip the human-readable Markdown.
cargo xtask bench-scale --json-only

# Custom output directory (defaults to docs/benchmarks/v0.5/scale/<os>-<arch>/).
cargo xtask bench-scale --out /tmp/my-bench-run
```

Required tools: `hyperfine` ≥ 1.18 on `PATH` (`cargo install hyperfine` or `apt`/`brew`/`choco install hyperfine`). For `--changed`-mode rows, `git` must also be on `PATH` — the harness initialises a per-tree git repo to make `git ls-files --modified` meaningful.

## Output layout

```
docs/benchmarks/v0.5/scale/
├── README.md                 ← this file
├── methodology.md            ← what the numbers mean + caveats
└── <os>-<arch>/              ← e.g. linux-x86_64
    ├── index.md              ← all rows, summary table
    ├── results.json          ← machine-readable, the canonical record
    ├── 1k/results.md         ← per-size detail
    ├── 10k/results.md
    ├── 100k/results.md
    └── 1m/results.md         ← only present when --include-1m was passed
```

## Currently published

| Platform | Latest |
|---|---|
| `linux-x86_64` | v0.5.6 ([linux-x86_64/index.md](linux-x86_64/index.md)) |
| `macos-arm64` | _(not yet)_ |
| `macos-x86_64` | _(not yet)_ |
| `windows-x86_64` | _(not yet)_ |

Cross-tool competitive comparisons (alint vs. ls-lint, alint vs. Repolinter, alint vs. find/grep pipelines) ship in v0.5.7 — see the `--tools` flag and the Docker-pinned reproduction story coming there.
