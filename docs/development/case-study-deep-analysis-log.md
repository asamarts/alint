# Case-study deep analysis — coverage rollup, perf, gap discovery

Master tracking + cross-cutting findings from the per-case-study deep
analysis pass. Each case study at `examples/<owner>-<repo>/README.md`
carries the 6-section template defined below; this doc aggregates
the per-repo coverage tables + cross-cutting observations + the
performance comparison rollup.

## The 6-section per-repo template

Every public README at `examples/<owner>-<repo>/README.md` covers
these 6 sections in this order. Agents in the deep-analysis pass
re-write each README to this template. Sections may grow / shrink
to fit the repo, but every section is filled (a marker like "perf
bench pending — methodology documented in this section" is
acceptable; an empty section is not).

### 1. Inventory of existing tooling

Every check the repo runs today, one row per check.

| Check | Source | What it does | Approx runtime |
|---|---|---|---|
| `<name>` | `<hook / Makefile / verify-script / GHA / lint config>` | `<one-line description>` | `<measurable / pending>` |

### 2. Coverage classification

Each row from §1 tagged with one of:

- ✅ **alint-today** — name the rule + ruleset that covers it.
  Format: `<rule_kind> in <bundled-ruleset or this repo's config>`.
- 🔄 **alint-future** — name the v0.10 / v0.11+ candidate from
  `launch-evidence.md`. Format:
  `<candidate_name> (<sources count> sources, <ship-target |
  design candidate | single-source>)`.
- ❌ **out-of-scope** — explain why (AST-aware analysis, runtime
  probe, SAST, IaC scan, secret scan, dependency-graph
  resolution, etc.). The "out-of-scope" label is positive, not
  apologetic — it means the existing tool is the right tool for
  that check.

### 3. Quantified coverage table

```
✅ alint-today:    N1 / total = X1%
🔄 alint-future:   N2 / total = X2%
❌ out-of-scope:   N3 / total = X3%
                   ─────────────────
                   total = X1 + X2 + X3 = 100%
```

Plus a 1-paragraph commentary on what the breakdown says about
this repo's shape (e.g., "high alint-future signals demand for
v0.10 ship-targets X and Y").

### 4. The `.alint.yml` synopsis

Link to the working config + a 30-line synopsis showing the
most-load-bearing rules. Explicit note on which rules are
repo-specific vs from bundled rulesets.

### 5. Performance comparison

For each ✅ alint-today check that's measurable:

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|

Methodology: `hyperfine -w 3 -r 5 <existing> <alint>` against the
same captured tree on the same machine. Methodology + reproduction
commands documented per repo. Where the existing toolchain isn't
installed locally, the row is `pending — needs <toolchain>`; the
methodology + commands are still documented so a future run can
fill the data.

### 6. Gap discovery

What alint surfaces in the live tree that the existing tooling
misses. Format:

| Finding | Path | Severity | Why existing tooling misses it |
|---|---|---|---|

Plus a count: `total alint-surfaced violations: <N>; of which:
<N1> already known + tracked / <N2> previously unknown + filed`.

## Per-repo summary table (populated as agents land)

| Repo | ✅ today | 🔄 future | ❌ out | Coverage | Perf | Gaps |
|---|---|---|---|---|---|---|

(One row per case study after deep analysis lands.)

## Cross-cutting findings

(Populated by the parent agent after all batches land.)

### Most-frequently-cited v0.10 candidates

(Pattern: which candidates would unlock the most additional
coverage when shipped — informs ship-priority for v0.10.)

### Performance pattern

(Where measured: the alint-vs-existing wall-clock ratio.
Hypothesis: alint is faster because single static binary vs
Node/Python startup + per-rule subprocess. Verify or refute with
actual numbers.)

### Out-of-scope categories — what alint will never cover

(SAST, IaC scan, AST linting, dependency-graph resolution,
runtime probes, generator drift checks, secret scanning. These
are documented as positive non-goals.)

### Gap discovery roll-up

(Total real bugs/violations alint surfaces across the 30 live
trees that the existing tooling misses. Aggregated count + a
sample of the most interesting ones.)

## Methodology notes

- **Captured commit SHA per repo:** each `examples/<repo>/README.md`
  carries the captured SHA in its top-of-file framing. Deep-analysis
  perf benches run against `/tmp/<repo>/` cloned at that SHA when
  available; latest tip otherwise (with the SHA-drift caveat noted
  per repo).
- **Hyperfine setup:** `hyperfine --warmup 3 --runs 5 '<existing>'
  '<alint>'`. Shorter runs for slow tools (e.g. `--warmup 1
  --runs 3` for `go vet ./...`-class invocations).
- **alint version:** v0.9.17 (released 2026-05-06; see
  CHANGELOG.md). All deep-analysis perf numbers are this version.
- **Toolchains installed locally:** alint binary always available.
  Go, Python, Node, Rust, etc. — installed on demand per case
  study. If a toolchain isn't available, the perf row is marked
  `pending — needs <toolchain>` with reproduction commands so a
  future run can fill the data.
