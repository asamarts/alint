# Scale-ceiling benchmarks — methodology

Scale-specific addendum to the project-wide [`docs/benchmarks/METHODOLOGY.md`](../../METHODOLOGY.md). The latter explains why we have two layers (criterion micro + hyperfine macro) and why we publish per-platform; this page documents the v0.5 scale-ceiling harness specifically.

## Synthetic tree shape

`alint_bench::tree::generate_monorepo(packages, files_per_package, seed)` produces a Cargo-workspace-shaped tree:

```
Cargo.toml                        (workspace, members = ["crates/*"])
crates/
  pkg-000000/
    Cargo.toml                    ([package] name = "pkg-000000")
    README.md
    src/
      lib.rs
      mod_0001.rs
      ...
  pkg-000001/
    ...
```

Per-size `(packages, files_per_package)` shape, picked so the total file count hits a round target:

| Size | Packages | Files/pkg | Total |
|---|---:|---:|---:|
| 1k | 50 | 18 | 1,001 |
| 10k | 200 | 48 | 10,001 |
| 100k | 1,000 | 98 | 100,001 |
| 1m | 5,000 | 198 | 1,000,001 |

Each `*.rs` source file gets pseudo-English ASCII content (256–2048 bytes, seeded). `Cargo.toml` and `README.md` carry real workspace / package shape so the bundled `monorepo/cargo-workspace@v1` ruleset's fact gate (`facts.is_cargo_workspace`) and structured-query rules see well-formed manifests.

The generator is fully deterministic: same `(packages, files_per_package, seed)` triple → byte-identical tree across platforms. Default seed `0xa11e47`; override with `--seed`.

## Scenarios

Each scenario is one `.alint.yml` written to the synthetic tree's root before the corresponding hyperfine row runs.

### S1 — Filename hygiene (8 rules)

Eight filename-only rules: `filename_case` for `*.rs` (snake), `*.tsx` (pascal), `*.ts` / `*.yaml` / `*.yml` (kebab), `*.py` (snake); `filename_regex` for `*.md` and `*.json`. Content is never read; perf is dominated by the walker + globset matcher. v0.5.7 will run alint and ls-lint side by side on this scenario — both tools' configs target the same rule set.

### S2 — Existence + content (8 rules)

Layout rules (`file_exists` / `file_absent`) for README, LICENSE, `*.bak`, `*.orig`; content-forbidden rules (`file_content_forbidden`) for TODO markers in Rust, `debugger;` in TypeScript, `print()` in Python; `file_max_size` over `**`. Reads every matching file's contents — perf is dominated by walker + content-IO + regex. v0.5.7 will compare against Repolinter on this scenario.

### S3 — Workspace bundle

`extends:` four bundled rulesets — `oss-baseline@v1`, `rust@v1`, `monorepo@v1`, `monorepo/cargo-workspace@v1`. Sets `nested_configs: true` (matches what `alint init --monorepo` emits). This is the highest-rule-count scenario and includes cross-file rules (`for_each_dir`, `unique_by`, `every_matching_has`) whose costs are non-linear in tree size. Closest match to real-world end-to-end timing for a workspace-tier monorepo. No fair tool-to-tool comparison exists because no other tool ships an equivalent integrated rule set.

## Modes

### `full`

Vanilla `alint check <tree>`. Walks the entire tree, evaluates every rule against every match.

### `changed`

`alint check --changed <tree>`, with the harness pre-arranging a working-tree diff:

1. Generate the tree.
2. `git init -q -b main` + `git config gc.auto 0` (auto-gc disabled — see "Git auto-gc" below) + `git add -A` + `git commit`.
3. Pick a deterministic 10% subset of the tree's files via `alint_bench::tree::select_subset(files, 0.10, seed ^ 0xD1FF)`.
4. Append a marker line to each file in the subset.
5. Hand the tree to hyperfine; each measured run sees the same diff state.

`alint check --changed` (v0.5.0) shells out to `git ls-files --modified --others --exclude-standard` to derive the changed-paths set, then evaluates per-file rules only against that subset. Cross-file + existence rules still see the full tree. This mode measures the v0.5 incremental path's actual savings as a function of (rule scope × diff size).

The `--diff-pct` flag tunes the diff size away from 10%; the default is what we publish.

## Git auto-gc

Git's auto-gc fires implicitly after commits with enough loose objects (default threshold ~7000). On 10k+ trees the initial commit triggers it, which would repack `.git/objects/` mid-bench-run and produce flaky timings. The harness sets `gc.auto = 0` per-repo. As belt-and-suspenders, alint's walker (since v0.5.6) explicitly excludes `.git/` regardless of `.gitignore` content — descending into git's internal storage was wasted work for every alint rule and a TOCTOU hazard during pack rewrites.

## Hyperfine settings

Defaults: `--warmup 3 --min-runs 10 --max-runs 10 --ignore-failure --export-json`. The exit-code ignore is intentional: synthetic trees don't satisfy `oss-baseline@v1`'s required-file rules etc., and the time taken to find those violations is exactly what we want to measure. Override with `--warmup` and `--runs`.

Per-row hyperfine reports mean / median / stddev / min / max in seconds; the harness rescales to milliseconds in the JSON output for legibility.

### 1M-size auto-reduction

At `1m`, the harness caps warmup at 1 and measured runs at 3 (down from the default 3 / 10), regardless of what `--warmup` and `--runs` were passed. A single `S3 / 1m` invocation runs for several minutes; thirteen of them per row would push the full matrix to several hours. The reduction keeps a publication-grade run finishing in roughly an hour on a workstation while still emitting honest mean / min / max numbers — the trade-off is wider stddev. **Read `1m`-row stddev with that in mind: it isn't directly comparable to the smaller-size rows' stddev, where 10 measured samples narrow the band considerably.**

Stddev for 1m rows where `runs == 1` is reported as `0.0` rather than `null` — hyperfine's JSON omits the field when it has no variance to compute, and our schema fills the gap. Min / max / mean are still meaningful; they're the same number on a single-run row.

## What the JSON looks like

```json
{
  "schema_version": 1,
  "fingerprint": {
    "os": "linux", "arch": "x86_64",
    "kernel": "Linux 6.1.0-42-amd64",
    "cpu_model": "AMD Ryzen 9 3900X 12-Core Processor", "cpu_cores": 24,
    "ram_gb": 62, "fs_type": "ext4",
    "rustc": "rustc 1.95.0 ...",
    "alint_version": "0.5.6", "alint_git_sha": "...",
    "hyperfine_version": "1.20.0",
    "timestamp": "unix:..."
  },
  "args": {
    "seed": "0xa11e47", "diff_pct": 10.0, "warmup": 3, "runs": 10,
    "sizes": ["1k","10k","100k"], "scenarios": ["S1","S2","S3"], "modes": ["full","changed"]
  },
  "rows": [
    {
      "size_files": 1000, "size_label": "1k",
      "scenario": "S1", "mode": "full",
      "mean_ms": 8.7, "stddev_ms": 0.2, "median_ms": 8.7,
      "min_ms": 8.3, "max_ms": 9.1, "samples": 10,
      "command": "..."
    },
    ...
  ]
}
```

`schema_version: 1` is the current published schema. Bumps will be additive within v0.5; breaking shape changes go to `2`.

## Reproducibility caveats

Same as the project-wide methodology, plus:

- **Tempfs vs. ext4 vs. APFS** — synthetic-tree generation goes to `$TMPDIR`. On Linux `tmpfs` (in-memory `/tmp`), the walker is several × faster than ext4. The fingerprint records `fs_type`; macOS (APFS) and ext4 are fundamentally not comparable on FS-bound rows.
- **CPU thermal throttling** — long bench runs (especially 100k+ S3) can throttle laptops. Run on plugged-in / desktop hardware for publishable numbers.
- **Background load** — close browsers, shut down test runners. Hyperfine warns about outliers in the live output; outlier-laden rows should be re-run.
- **Filesystem cache state** — first run after a fresh boot is slower; the bench's hyperfine warmup absorbs this.
- **rustc version drift** — release builds aren't bit-reproducible across rustc versions. The fingerprint records `rustc --version`; rerun under the same rustc to compare.
