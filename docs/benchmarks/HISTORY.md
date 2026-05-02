# alint perf history

Per-scenario tables, version-trajectory shape. Headline cells fingerprinted
to `linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4 / rustc 1.95) —
see [`METHODOLOGY.md`](METHODOLOGY.md) for the hardware contract and why
cross-machine comparisons need like-for-like.

## How to read this file

Each scenario gets its own section with:

1. A one-paragraph overview of what dispatch shape the scenario stresses
   and which class of regression it catches.
2. A table per mode (`full` and `changed`) with rows = version (newest
   first), columns = size (1k / 10k / 100k / 1M). Cells are
   `mean ± stddev`, formatted in ms below 1 s and seconds above.
3. `—` means the version was not measured at that size.
   `n/a` means the scenario didn't exist at the tag.

Significant deltas (anything > 20 % across a release) get an investigation
write-up under [`investigations/<YYYY-MM-topic>/`](investigations/) that
captures the diagnostic data (traces, flamegraphs, bisect notes).

## Cross-version headline trajectory (S3, the workspace bundle)

S3 is the most-cited cell for headline narratives — workspace bundle over
a Cargo-shaped monorepo, the realistic mix that surfaces both per-file and
cross-file dispatch costs.

| Version | Date | 1M full | 1M changed | 100k full | 10k full | Headline change |
|---|---|---:|---:|---:|---:|---|
| **v0.9.7** | 2026-05-02 | TBD | TBD | TBD | TBD | `scope_filter:` runtime fix + audit cleanup. |
| v0.9.6 | 2026-05-02 | — | — | 1.14 s ± 0.02 | 125 ms ± 11 | `scope_filter:` primitive + bundled-ruleset migration; new S9 scenario. |
| v0.9.5 | 2026-05-01 | 11.19 s ± 0.15 | 6.73 s ± 0.06 | — | — | Cross-file dispatch fast paths (path-index on FileIndex) — 65× / 108× over v0.9.4. |
| v0.9.4 | 2026-04-30 | 731.9 s ± 5.3 | 724.4 s ± 2.1 | 11.20 s ± 0.13 | 316 ms ± 9 | Content-rule mechanical migration (16 rules to PerFileRule). |
| v0.5.7 | 2026-03 | — | — | 11.39 s ± 0.07 | 356 ms ± 10 | First publish-grade `bench-scale` matrix at 1k/10k/100k. |
| v0.5.6 | 2026-03 | 569.1 s ± 60.9 | 528.1 s ± 2.5 | — | — | Prep run that captured the only pre-v0.9 1M S3 numbers. |

Earlier history (v0.7.x, v0.8.x): no measured perf change beyond v0.5.7;
see [CHANGELOG.md](../../CHANGELOG.md) for the contemporaneous notes.

---

## S1 — Filename hygiene

Eight filename-only rules (`filename_case`, `filename_regex`). Pure walker plus glob match — no content read. Narrowest scope alint shares with `ls-lint`, used as the competitive-comparison anchor. Catches walker and scope-match regressions.

### S1 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 21 ms ± 1 | 160 ms ± 6 | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | 8 ms ± 0 | 20 ms ± 1 | 154 ms ± 14 | 1.53 s ± 0.03 |
| v0.5.7 | 58 ms ± 1 | 194 ms ± 3 | 1.41 s ± 0.01 | — |
| v0.5.6 | — | — | — | — |

### S1 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | 13 ms ± 1 | 46 ms ± 1 | 420 ms ± 16 | 4.18 s ± 0.04 |
| v0.5.7 | 13 ms ± 0 | 73 ms ± 6 | 623 ms ± 9 | — |
| v0.5.6 | — | — | — | — |

## S2 — Existence + content

Eight existence + content rules (`file_exists`, `file_absent`, `file_content_forbidden`, `file_max_size`). Walker plus per-file content scan over narrow scopes. Repolinter-comparable shape. Catches content-rule regressions on common shapes.

### S2 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 32 ms ± 1 | 250 ms ± 11 | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | 10 ms ± 1 | 30 ms ± 1 | 237 ms ± 12 | 2.36 s ± 0.13 |
| v0.5.7 | 486 ms ± 25 | 1.64 s ± 0.04 | 13.76 s ± 0.11 | — |
| v0.5.6 | — | — | — | — |

### S2 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | 14 ms ± 1 | 49 ms ± 1 | 423 ms ± 15 | 4.29 s ± 0.04 |
| v0.5.7 | 15 ms ± 0 | 83 ms ± 15 | 685 ms ± 13 | — |
| v0.5.6 | — | — | — | — |

## S3 — Workspace bundle

`extends: oss-baseline + rust + monorepo + cargo-workspace` (~34 rules). Heavy mix — content rules over `**/*.rs`, cross-file `for_each_dir` over `crates/*`, `toml_path_matches` per crate. Realistic monorepo workload; the v0.9.5 cliff (`investigations/2026-05-cross-file-rules/`) lived here.

### S3 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 125 ms ± 11 | 1.14 s ± 0.02 | — |
| v0.9.5 | — | — | — | 11.19 s ± 0.15 |
| v0.9.4 | 28 ms ± 1 | 316 ms ± 9 | 11.20 s ± 0.13 | 731.9 s ± 5.3 |
| v0.5.7 | 29 ms ± 1 | 356 ms ± 10 | 11.39 s ± 0.07 | — |
| v0.5.6 | — | — | — | 569.1 s ± 60.9 |

### S3 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | — | — | — | 6.73 s ± 0.06 |
| v0.9.4 | 26 ms ± 1 | 276 ms ± 3 | 10.95 s ± 0.26 | 724.4 s ± 2.1 |
| v0.5.7 | 28 ms ± 0 | 324 ms ± 4 | 11.06 s ± 0.07 | — |
| v0.5.6 | — | — | — | 528.1 s ± 2.5 |

## S4 — Agent-era hygiene

Five rules from the v0.6 `agent-hygiene` bundled ruleset (`file_absent`, `file_content_forbidden`). Filename plus content fan-out over agent-shaped trees. Catches agent-era rule shapes.

### S4 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 23 ms ± 1 | 156 ms ± 1 | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

### S4 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

## S5 — Fix-pass content edits

Four content-edit rules under `--fix` (`final_newline`, `no_trailing_whitespace`, `line_endings`, `no_bom`). Read, transform, atomic-rename. The only `--fix`-mode bench. Catches fix-pipeline regressions.

### S5 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 92 ms ± 3 | 917 ms ± 17 | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

### S5 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

## S6 — Per-file content fan-out

Thirteen content rules over `**/*.rs`. Per-file dispatch path width — every `.rs` file hit by every rule on a single read. Stresses the v0.9.3 dispatch-flip read-coalescing path. Catches per-file inner-loop regressions S3 doesn't surface.

### S6 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 119 ms ± 5 | 1.22 s ± 0.02 | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

### S6 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

## S7 — Cross-file relational

Six cross-file relational kinds (`pair`, `unique_by`, `for_each_dir`, `for_each_file`, `dir_only_contains`, `every_matching_has`). Various fan-out shapes over the synthetic monorepo. Catches the next O(D × N) cliff after the v0.9.5 path-index fix.

### S7 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 206 ms ± 4 | 10.79 s ± 1.18 | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

### S7 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

## S8 — Git overlay

S3 reshape plus `git_no_denied_paths` and `git_tracked_only` over a real git repo. Same as S3 but with `Engine::collect_git_tracked_if_needed` and `BlameCache` active. Catches git-aware dispatch regressions at scale.

### S8 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 115 ms ± 4 | 1.07 s ± 0.02 | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

### S8 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | — | — | — | — |
| v0.9.4 | — | — | — | — |
| v0.5.7 | — | — | — | — |
| v0.5.6 | — | — | — | — |

## S9 — Nested polyglot

Three competing ecosystem rulesets: `extends: rust + node + python` (~26 rules) over a polyglot tree (Rust under `crates/`, Node under `packages/`, Python under `apps/`). Per-rule `scope_filter: { has_ancestor: <manifest> }` ancestor walks. The dispatch shape the v0.9.6 `scope_filter:` primitive was designed for — without it, every `**/*.py` rule from python@v1 fires on every `.py` file in the tree. **New in v0.9.6.**

### S9 — full

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | 74 ms ± 1 | 739 ms ± 32 | — |
| v0.9.5 | n/a | n/a | n/a | n/a |
| v0.9.4 | n/a | n/a | n/a | n/a |
| v0.5.7 | n/a | n/a | n/a | n/a |
| v0.5.6 | n/a | n/a | n/a | n/a |

### S9 — changed

| Version | 1k | 10k | 100k | 1M |
|---|---:|---:|---:|---:|
| **v0.9.7** | — | — | — | — |
| v0.9.6 | — | — | — | — |
| v0.9.5 | n/a | n/a | n/a | n/a |
| v0.9.4 | n/a | n/a | n/a | n/a |
| v0.5.7 | n/a | n/a | n/a | n/a |
| v0.5.6 | n/a | n/a | n/a | n/a |

## How to add a row

When a release tag lands, the `bench-record.yml` workflow (introduced in
v0.9.7) auto-runs the publish-grade matrix on the self-hosted Linux runner
and opens a PR adding the new per-version dir under
[`macro/results/linux-x86_64/`](macro/results/linux-x86_64/). The PR also
includes a HISTORY.md row update for every scenario where the new release
was measured. A maintainer reviews the CV (anything > 10 % gets a re-run
on a quieter system) and merges.

To run manually for an off-cycle measurement (e.g. characterising a
specific commit before release):

```sh
xtask bench-scale --include-1m \
    --sizes 1k,10k,100k,1m \
    --scenarios S1,S2,S3,S4,S5,S6,S7,S8,S9 \
    --modes full,changed \
    --tools alint \
    --warmup 3 --runs 10 \
    --json-only \
    --out docs/benchmarks/macro/results/linux-x86_64/<version>
```

See [`RUNNING.md`](RUNNING.md) for the full flag list and the
publication-grade convention.

## Cross-version perf investigations

- v0.9.5 cliff: [`investigations/2026-05-cross-file-rules/`](investigations/2026-05-cross-file-rules/)
  — surfaced the +28-37 % 1M S3 regression vs v0.5.6 and the lazy-path-index fix.
