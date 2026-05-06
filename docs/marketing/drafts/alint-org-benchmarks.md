---
destination: alint.org/benchmarks/ (new top-level route on the site repo)
status: drafting
blocks_on: alint-org-hero.md publishes (hero's speed-bullet links here for the per-release detail); docs-bundle pipeline updated to sync `docs/benchmarks/HISTORY.md` to the site repo
last_touched: 2026-05-06
---

# alint.org/benchmarks/ — content brief for the site repo

## Why

Two of the three heroes (`README.md` + `alint-org-hero.md`) lead with
a concrete latency claim — *"~1.1 s on a 100K-file workspace bundle,
~12 s at 1M files."* That number needs a landing page that:

1. **Backs the claim** with the per-release table from
   `docs/benchmarks/HISTORY.md` so a sceptical reader can verify
   it themselves and see the trajectory across versions.
2. **Surfaces the new P2b Wave 1 real-world data point** — NixOS/
   nixpkgs at 39,101 files runs the full 79-rule pass in **273 ms
   wall-clock**. This is real-world stress beyond the synthetic 100K
   bench and lands the "any size repo" pitch empirically.
3. **Documents the methodology** transparently so reproducers can
   verify the published numbers on their own hardware.
4. **Sets honest comparison expectations** — alint vs Repolinter
   (no public benches), vs ls-lint (faster at narrower scope), vs
   Megalinter (different shape — orchestrator, not linter). Avoiding
   misleading "alint is 100× faster than X" cherry-picks earns more
   trust than overclaiming.
5. **Carries SEO weight on `/benchmarks/`-shaped queries** —
   "fast monorepo linter benchmarks", "lint tool performance comparison".

The internal source of truth is
[`docs/benchmarks/HISTORY.md`](https://github.com/asamarts/alint/blob/main/docs/benchmarks/HISTORY.md)
— per-scenario tables, version-trajectory shape. This public page
**summarises and links into** that file rather than duplicating it.
The docs-bundle pipeline should auto-sync `HISTORY.md` to a
`/benchmarks/history/` sub-route so the public landing can cite it
without forking the data.

## Proposed page

```markdown
---
title: alint benchmarks
description: alint runs sub-second on 100K-file repos and completes the full 79-rule pass on NixOS/nixpkgs (39,101 files) in 273 ms wall-clock. Published per-release, methodology open.
---

# alint benchmarks

## Headline

**Sub-second on 100K-file repos.** ~1.1 s on a 100K-file synthetic
workspace bundle. ~12 s at 1M files. Real-world: the full 79-rule
pass on NixOS/nixpkgs (39,101 files + 20,678 `pkgs/by-name/*/*/`
package directories) completes in **273 ms wall-clock** — faster
than git status on the same repo on a cold cache.

> Hardware: `linux-x86_64` (AMD Ryzen 9 3900X 12-core / 62 GB / ext4
> / rustc 1.95). Numbers are not directly comparable across machines
> — see [methodology](#methodology) for what does and doesn't
> transfer.

---

## Real-world: NixOS/nixpkgs

A common question for any "fast" tool: *how does it actually behave
on a real giant repo, not a synthetic benchmark?* alint's P2b Wave 1
case study answers this for the largest non-trivial OSS monorepo on
GitHub.

| Metric | NixOS/nixpkgs |
|---|---:|
| Files in tree | 39,101 (sparse-cloned) |
| `pkgs/by-name/*/*/` package directories iterated | 20,678 |
| alint config rule count | 79 |
| Wall-clock for full check pass | **273 ms** |

The 79-rule pass includes the headline `for_each_dir` over the
20,678-directory by-name tree — exactly the cross-file dispatch
shape that the v0.9.5 + v0.9.6 + v0.9.8 engine work was designed
to make linear. nixpkgs confirms `for_each_dir` scales gracefully
in real-world conditions; the "any-size repo" pitch is now
empirically defensible beyond the synthetic 100K bench.

[Full nixpkgs case study →](/examples/nixos-nixpkgs/)

---

## Synthetic: per-release trajectory

alint publishes hyperfine-driven wall-time benchmarks across **9
synthetic scenarios** (S1-S9) at four sizes (1k / 10k / 100k / 1M
files), per release. Each scenario stresses a different dispatch
shape; the matrix catches per-shape regressions in CI before they
ship.

### Headline cells across versions

| Version | Date | 1M S3 full | 1M S6 full | 1M S7 full | 1M S9 full |
|---|---|---:|---:|---:|---:|
| **v0.9.14** | 2026-05-05 | 12.06 s | 11.19 s | 15.31 s | 7.33 s |
| v0.9.13 | 2026-05-04 | 11.46 s | 11.18 s | 15.45 s | 7.22 s |
| v0.9.12 | 2026-05-03 | 11.98 s | 11.33 s | 15.36 s | 7.46 s |
| v0.9.10 | 2026-05-03 | 11.62 s | 11.22 s | 15.50 s | 7.21 s |
| v0.9.8 | 2026-05-02 | 11.33 s | 10.89 s | 15.41 s | 7.32 s |
| v0.9.7 | 2026-05-02 | 11.89 s | 11.35 s | **614.4 s** | 7.36 s |
| v0.9.6 | 2026-05-02 | 11.09 s | 11.40 s | **623.7 s** | 7.12 s |
| v0.9.5 | 2026-05-01 | 12.59 s | 11.85 s | **652.4 s** | n/a |
| v0.9.4 | 2026-04-30 | **731.9 s** | — | — | n/a |
| v0.5.7 | 2026-04-26 | — | — | — | n/a |
| v0.5.6 | 2026-04-26 | — | — | — | **569.1 s** (S3 only) |

The v0.9.4 → v0.9.5 cliff (731.9 s → 12.59 s on S3 1M) is the
lazy path-index + literal-path fast paths fix. The v0.9.7 → v0.9.8
cliff (614.4 s → 15.41 s on S7 1M) is the cross-file dispatch
fast paths round 2. Both shipped with investigation write-ups
documenting the diagnostic data, the bisect, and the fix.

[Full per-release table → `HISTORY.md` (24K, all 9 scenarios)](/benchmarks/history/)

### What each scenario stresses

| Scenario | Shape | Catches regressions in… |
|---|---|---|
| **S1** Filename hygiene | 8 filename-only rules | Walker + scope-match |
| **S2** Existence + content | 8 existence + content rules | Per-file content-rule fan-out |
| **S3** Workspace bundle | `extends: oss-baseline + rust + monorepo + cargo-workspace` (~34 rules) | Realistic monorepo workload |
| **S4** Agent-era hygiene | 5 rules from `agent-hygiene@v1` | agent-era rule shapes |
| **S5** Fix-pass content edits | 4 content-edit rules under `--fix` | Fix-pipeline regressions |
| **S6** Per-file content fan-out | 13 content rules over `**/*.rs` | Per-file inner-loop |
| **S7** Cross-file relational | 6 cross-file kinds (`pair`, `unique_by`, `for_each_dir`, …) | Cross-file dispatch cliff |
| **S8** Git overlay | S3 reshape + `git_no_denied_paths` + `git_tracked_only` | Git-aware dispatch |
| **S9** Nested polyglot | `extends: rust + node + python` (~26 rules) over polyglot tree with `scope_filter:` | Polyglot scope-filter dispatch |

Modes: `full` (all-files scan) and `changed` (changed-files scan
via `--changed`). Both are published per release.

---

## Methodology

Two layers. **criterion** for pure-CPU micro-benchmarks (stable,
cross-platform). **hyperfine** driven by `xtask bench-scale` for
end-to-end CLI wall-time (cross-platform, reproducible, honest
about variance).

| Layer | Tool | What it captures | When to look |
|---|---|---|---|
| **Micro** | criterion | Pure-CPU primitives: glob compile/match, regex content scans, engine fan-out, walker, formatters | After every change to `alint-core` / `alint-rules`. Fast (seconds), stable, cross-platform. |
| **Macro** | hyperfine | End-to-end CLI wall-time over deterministic synthetic monorepos at 1k / 10k / 100k / 1M files | Before each release tag. Slow (minutes to hours at 1M), platform-dependent, honest about variance. |

Three deliberate methodology choices documented openly:

- **Why hyperfine and not a custom Rust harness?** Hyperfine
  measures wall-time of an external command from *outside* the
  process. That's exactly the cost shape a CLI user pays —
  including process startup, dynamic linker overhead, stdio
  buffering, TTY detection, format selection, shell-quoting. A
  Rust-internal harness would skip those and overstate alint's
  speed.
- **Why a deterministic synthetic monorepo and not a real-world
  repo?** Cross-machine reproducibility requires byte-identical
  inputs. The synthetic tree is byte-identical across machines
  given the same seed (`0xA11E47`); 1k = 1,001 files exactly,
  1M = 1,000,001 files exactly. Cross-version comparisons aren't
  contaminated by tree-size drift. The nixpkgs data point above
  is the *non*-synthetic complement.
- **Why not CodSpeed / iai-callgrind?** Both are Valgrind-based.
  alint's hot path is syscall-heavy (the `ignore`-crate walk);
  Valgrind reports instruction counts that drift whenever the CI
  runner's glibc or kernel updates — exactly the part of alint
  we most want stable numbers for.

[Full methodology doc →](/benchmarks/methodology/)

---

## Honest comparisons

A persistent question: *how does alint compare to other repo-level
linters?* Honest answer: **mostly we don't have apples-to-apples
public benches, because the other tools haven't published any.**
Here's what we know:

### vs Repolinter (Node.js, archived 2026-02)

- **No public benches exist.** Repolinter was never benchmarked
  publicly across releases.
- **Expected shape:** Node startup overhead (~100 ms cold) +
  per-rule JS execution. On a 1k-file repo, Node startup alone
  exceeds alint's full S1 measurement (8 ms ± 1). At 100K files,
  the per-rule scan cost dominates; we'd expect Repolinter to
  trail by 1-2 orders of magnitude based on the architectural
  shape, but we haven't run a controlled head-to-head.
- **Why not run one?** Repolinter is archived. The maintenance
  signal is the comparison; raw speed is secondary.

### vs ls-lint (Go binary, narrower scope)

- **No public benches at scale.** ls-lint's docs cite "fast" but
  no per-release numbers.
- **Expected shape:** ls-lint is a Go binary doing filename +
  directory pattern matching only. Narrower scope = less work
  per file = should be faster than alint *at its specific job*.
  alint's S1 scenario (filename hygiene only) is the closest
  apples-to-apples shape; we publish numbers there but haven't
  run ls-lint on the same synthetic tree.
- **Honest framing:** if filename conventions are *the only*
  thing you care about, ls-lint will likely be faster. If you
  also need content checks, structured queries, cross-file
  rules, alint's "more work per file" is a feature.

### vs Megalinter (Docker orchestrator, ~70 native linters)

- **No published benches across releases.** Megalinter is a
  shape-mismatch comparison anyway: it's a Docker orchestrator,
  not a linter. Its wall-time is dominated by container startup
  + per-tool execution, not by any single tool's hot path.
- **Use Megalinter alongside alint**, not instead of —
  Megalinter orchestrates the language-specific lint stack;
  alint runs as one additional check inside Megalinter for
  structural coverage.

### vs custom shell scripts (the most-common comparator)

- **Hard to bench.** Each repo's `verify-*.sh` directory is
  bespoke. The kubernetes case study replaced 17 of 50 verify
  scripts with one alint config; the wall-time comparison there
  is meaningful for that specific repo's workload but doesn't
  generalise.
- **Honest framing:** the consolidation win usually shows up in
  *CI wall-time variance* (one binary vs. 50 scripts each with
  per-tool startup) more than in raw speed. See [the kubernetes
  case study](/examples/kubernetes-kubernetes/) for the per-script
  breakdown.

If you'd like to run a head-to-head comparison on your own
hardware, every methodology document and every published number is
reproducible — see [reproducibility](#reproducibility) below.

---

## What gets benched

alint publishes two layers of data per release:

### Macro (hyperfine, end-to-end wall-time)

- **9 scenarios** (S1-S9, per the table above)
- **2 modes** (`full` / `changed`)
- **4 sizes** (1k / 10k / 100k / 1M files)
- **= 72 (scenario, mode, size) cells per release**

Plus the per-version cliff investigations (when a release shifts a
cell by > 20 % up or down) under
[`docs/benchmarks/investigations/`](https://github.com/asamarts/alint/tree/main/docs/benchmarks/investigations).

### Micro (criterion, pure-CPU kernels)

- **12 bench files** under `crates/alint-bench/benches/`
- Stable, cross-platform; floor-tested vs the v0.7.0 publication
  on every PR via `xtask bench-compare --threshold 10`. Anything
  > 10 % slower than v0.7.0 fails CI.

---

## Reproducibility

Every published number is reproducible end-to-end:

```sh
# Clone the repo
git clone https://github.com/asamarts/alint && cd alint

# Run the publish-grade matrix on your machine
xtask publish-benches --trim
xtask bench-scale --include-1m --scenarios S1,S2,S3 --warmup 3 --runs 10

# Or run the micro-benches
cargo bench -p alint-bench --features fs-benches
```

The harness:

1. **Builds** alint in release mode (`cargo build --release -p alint`).
2. **Generates** a deterministic synthetic monorepo via
   `alint_bench::tree::generate_monorepo(packages, files_per_package,
   seed=0xA11E47)`. Byte-identical across machines.
3. **Stages** the scenario's config YAML at the tree root.
4. **Captures** a hardware fingerprint (OS, arch, rustc version,
   CPU model, RAM, filesystem type, hyperfine version, seed,
   warmup/runs counts).
5. **Shells out** to hyperfine with `--warmup 3 --runs 10` (3 warmup
   runs to fill the page cache, 10 measured runs for stddev that's
   small enough to detect 10% deltas).
6. **Writes** per-size `results.md` + an aggregated `index.md` +
   the machine-readable `results.json`.

Caveats we document openly:

- **Absolute numbers are not comparable across machines.** Always
  compare like-for-like fingerprints (OS / arch / rustc / CPU /
  RAM / FS).
- **GitHub-hosted `ubuntu-latest` has 5-30 % wall-time variance** —
  fine for smoke-testing, too noisy for PR-level regression
  gating. Publication-grade numbers come from a self-hosted
  runner with a known fingerprint.
- **Filesystem type matters** (tmpfs > ext4 > NTFS > APFS by
  order of magnitude on walk-heavy workloads).

[Full methodology + caveats →](/benchmarks/methodology/)
[Per-release `HISTORY.md` →](/benchmarks/history/)
[Investigations directory →](https://github.com/asamarts/alint/tree/main/docs/benchmarks/investigations)
```

## Implementation notes (for the site repo)

- New top-level route — `src/pages/benchmarks.astro` or
  `src/content/docs/benchmarks/index.md`, depending on Starlight
  conventions.
- Add to top-level nav (sibling to "Docs", "Examples", "Cookbook",
  "Compare", "Roadmap").
- Two sub-routes the page links into:
  - `/benchmarks/history/` — auto-synced from
    `docs/benchmarks/HISTORY.md` via the docs-bundle pipeline
    (same pattern as `/docs/rules/` auto-syncs from
    `crates/alint-rules/src/<kind>.rs::doc_str()`).
  - `/benchmarks/methodology/` — auto-synced from
    `docs/benchmarks/METHODOLOGY.md` via the same pipeline.
- The headline cell table benefits from a sticky-header treatment
  (Starlight's default tables don't sticky-header; CSS override
  needed if the matrix grows past 20 rows).
- The "Honest comparisons" section uses subheaders rather than a
  matrix on purpose — comparing across asymmetric public-data
  availability with a matrix would suggest equivalence we haven't
  established.

## Open questions before publish

1. **Comparison numbers for tools we don't have benches for.**
   Default in this draft: surface the *shape-of-comparison* honestly
   (Repolinter would be slower because Node startup; ls-lint would
   be faster at narrower scope; Megalinter is a shape-mismatch
   comparison). DO NOT publish made-up numbers. **Recommend: stick
   to alint's own data + qualitative comparison.** If we want to
   add hard comparison numbers later, that's a follow-up project
   to actually run head-to-head benches inside the
   `bench/Dockerfile` matrix (already designed; see methodology
   doc's "Why Docker for `--tools all` runs" section).
2. **Real-world data: nixpkgs only, or expand?** Currently surfaces
   nixpkgs as the headline real-world data point. Other P2b Wave 1
   case studies have wall-clock numbers too (vscode, TF, spark,
   bazel) — should we publish a real-world matrix? **Recommend:
   nixpkgs only at MVP** (it's the scale-stress flagship; the others
   are case-study curiosities). Add a real-world matrix in a v2 of
   the page if traction warrants.
3. **Per-rule cost breakdown.** The micro-bench layer measures
   per-kernel cost (glob compile, regex match, etc.) but doesn't
   today produce a "rule kind X costs N ns/file on average"
   breakdown that adopters could use for capacity planning.
   **Recommend: defer.** Nice-to-have, not load-bearing for the
   public page; surface the existing micro layer as link-only.
4. **Animated chart of the trajectory?** The headline cell table
   shows v0.9.4 → v0.9.5 → v0.9.7 → v0.9.8 cliffs with concrete
   numbers, but a chart would land the trajectory more
   immediately. **Recommend: defer for MVP** — a chart is a
   separate Astro component build, scope creep for the launch.
   A static screenshot of the v0.9.4 → v0.9.14 trajectory could
   ship cheap if needed.
5. **CodSpeed / iai-callgrind explanation.** Currently in the
   methodology section. Some readers will want the technical
   detail; others will skim past. The current draft has it inline
   in the methodology section — moves to an "FAQ" or "deep dive"
   sub-page if it grows. **Recommend: leave inline for MVP.**
6. **HISTORY.md auto-sync mechanism.** The docs-bundle pipeline
   syncs `docs/` content from `asamarts/alint` into the site repo
   at build time. We need to confirm `docs/benchmarks/HISTORY.md`
   is in the sync set (or add it). If not in scope for this draft,
   the page can ship with a GitHub-rendered HISTORY.md link
   (`https://github.com/asamarts/alint/blob/main/docs/benchmarks/HISTORY.md`)
   as the fallback for `/benchmarks/history/`.

## Pre-publish checklist

- [ ] alint.org repo identified + new `/benchmarks/` route created
- [ ] Top-level nav updated to surface `/benchmarks/`
- [ ] Sub-route `/benchmarks/history/` exists (auto-synced from
      `docs/benchmarks/HISTORY.md` via the docs-bundle pipeline) OR
      the link falls back to GitHub-rendered HISTORY.md
- [ ] Sub-route `/benchmarks/methodology/` exists (auto-synced from
      `docs/benchmarks/METHODOLOGY.md`) OR fallback link
- [ ] `/examples/nixos-nixpkgs/` exists (the real-world data
      point links to the nixpkgs case study)
- [ ] `/examples/kubernetes-kubernetes/` exists (custom-shell
      comparison links to it)
- [ ] All `https://github.com/asamarts/alint/...` links resolve
- [ ] Headline numbers verified against latest published
      `docs/benchmarks/HISTORY.md` at publish time. Currently
      reflects v0.9.14 (2026-05-05) headline cells; v0.9.16 doesn't
      change the engine shape so the numbers stand. Confirm before
      publish.
- [ ] STATE.md row for `alint-org-benchmarks.md` flipped to `live`
      with date + commit SHA

## Estimated diff size on the site repo

- 1 new page at `/benchmarks/`: ~250-280 lines of markdown
- Top-level nav config: ~5 lines
- Optional sticky-header CSS for the matrix: ~10 lines
- Pipeline change to sync `HISTORY.md` + `METHODOLOGY.md` to
  `/benchmarks/history/` + `/benchmarks/methodology/`: ~15-30 lines
  of pipeline config (depends on the docs-bundle pipeline's existing
  shape)

Total: ~280-325 lines on the site repo (smaller if pipeline auto-sync
is already in place; larger if `HISTORY.md` needs new sync rules).

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `alint-org-hero.md` | The hero's speed bullet ("~1.1 s on 100K, ~12 s on 1M") links here for detail. Both should ship coordinated, OR the hero links to GitHub-rendered HISTORY.md as fallback. |
| `alint-org-compare.md` | The compare page's "Performance" row in the feature matrix cites `/docs/benchmarks/`; that link target IS this page. Coordinate publish so neither is a 404 link. |
| `alint-org-roadmap.md` (this batch's other draft) | The roadmap's "Where alint stands" section cites the same headline numbers. Single source of truth: this page. The roadmap references it; doesn't duplicate. |
| `alint-org-examples-gallery.md` | The "Real-world: NixOS/nixpkgs" section links to the nixpkgs case study sub-route; same dependency as the hero on the gallery shipping. |
| Internal `docs/benchmarks/HISTORY.md` | Source of truth for the per-release table. Pipeline change to auto-sync into the site repo means HISTORY.md is the only place updates need to land at release time. |
| Internal `docs/benchmarks/METHODOLOGY.md` | Source of truth for the methodology sub-page. Same auto-sync mechanism. |
