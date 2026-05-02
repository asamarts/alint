# v0.9 — Design pass

Status: v0.9 cut closed 2026-05-02 with v0.9.6. All four
sub-themes shipped. The per-feature docs in this directory are
kept as the design record — each one's `Status:` header points
to the tag / commit / crate where it landed.

**v0.9 was reopened on 2026-05-01** after the v0.9.4 cut closed.
A scaling-profile investigation surfaced an O(D × N) hot spot
in `for_each_dir` cross-file dispatch that produced a +28-37%
1M S3 regression vs v0.5.6. The fix (lazy path-index on
`FileIndex` + literal-path fast paths) drove a 65× / 108×
speedup at 1M S3 and shipped as v0.9.5. Sub-phases .5.5 –
.5.reorg codify the test/coverage/dogfood floor that lets
future engine work land without that class of regression
slipping by; see [`coverage-and-dogfood.md`](./coverage-and-dogfood.md).
v0.9.6 then closed the cut with the `scope_filter:` per-file
gate plus the bundled-ecosystem-ruleset migration —
[`scope-filter.md`](./scope-filter.md).

## What v0.9 ships

Engine-internal optimizations and the test/coverage floor that
lets them land safely. v0.9 is the first cut since v0.5 that
doesn't add user-visible rule kinds, formatters, or subcommands
— every change is below the rule API.

| File | Sub-theme |
|---|---|
| [`parallel_walker.md`](./parallel_walker.md) ✅ | Replace the sequential `WalkBuilder::build` with `WalkBuilder::build_parallel` + a deterministic post-sort. *(Shipped v0.9.1.)* |
| [`memory_pass.md`](./memory_pass.md) ✅ (partial) | `Arc<Path>` / `Arc<str>` / `Cow<'static, str>` on the Violation / RuleResult hot path. *(Shipped v0.9.2; per-rule byte-slice scanning + bounded prefix/suffix reads moved to v0.9.3 — see the doc for context.)* |
| [`dispatch_flip.md`](./dispatch_flip.md) ✅ | Per-file rules run under a file-major outer loop via a new `PerFileRule` sub-trait; cross-file rules (`requires_full_index() == true`) keep the rule-major path. *(Shipped v0.9.3 with engine restructure + 8-rule reference migration; remaining content rules migrated in v0.9.4.)* |
| [`coverage-and-dogfood.md`](./coverage-and-dogfood.md) ✅ | v0.9.5.5 cross-file dispatch fast paths, v0.9.5.6 coverage audits, v0.9.5.7 coverage scenarios, v0.9.5.8 bench-scale S6/S7/S8, v0.9.5.9 RULE-AUTHORING.md, v0.9.5.reorg bench layout reorganisation. *(Shipped across v0.9.5.5 – v0.9.5.reorg.)* |
| [`scope-filter.md`](./scope-filter.md) ✅ | `scope_filter: { has_ancestor: <manifest> }` per-file gate + bundled-ecosystem-ruleset migration (`is_*` → `has_*` rename + per-language `has_*` rulesets). *(Shipped v0.9.6.)* |

## Cross-cutting decisions

A few questions touch multiple sub-themes and benefit from being
settled once.

### Sub-theme order

Recommended: parallel walker (v0.9.1) → memory pass (v0.9.2) →
dispatch flip (v0.9.3). Each tag-eligible so a regression in one
sub-theme doesn't block the others.

Walker first because it's the cleanest leaf — no rule code
changes, no API surface impact. Memory pass second because the
per-rule conversions are local and additive. Dispatch flip last
because it changes the `Rule` trait and touches every per-file
rule's `evaluate` body; doing it after the memory pass lets the
new `PerFileRule::evaluate_file(..., bytes: &[u8])` interface
match the byte-slice consumption pattern the memory pass already
established.

The one explicit sequencing constraint: memory-pass rule
conversions consume `&[u8]` (e.g. `bytes.split(|&b| b == b'\n')`
or `memchr`), not `BufReader<File>`. This costs nothing in
v0.9.2 (we still own the bytes from the rule's own `fs::read`)
and avoids a v0.9.3 rewrite when the engine starts handing each
rule a pre-loaded slice.

### `bench-compare` gating per phase

Every v0.9.x tag runs:

```sh
xtask bench-compare \
  --before docs/benchmarks/micro/results/linux-x86_64/v0.7.0/criterion \
  --after target/criterion \
  --threshold 10
```

against the v0.7.0 floor. Coverage / Cross-Platform / Mutants
also stay green per phase.

The four v0.7.0-baselined micro-benches are `glob_compile`,
`glob_match`, `regex_content`, `rule_engine`. The v0.8.4 benches
(`single_file_rules`, `cross_file_rules`, `output_formats`,
`fix_throughput`, `dsl_extends`, `structured_query`,
`blame_cache`, `walker`) didn't exist at v0.7.0 — per
`docs/benchmarks/micro/results/linux-x86_64/v0.7.0/README.md`, "their own
first-run numbers serve as their baseline." For these, v0.9.x
compares against the previous v0.9.x phase rather than against
v0.7.0.

When a phase intentionally trades single-rule throughput for
cross-rule throughput (the dispatch flip is the obvious case),
the gate threshold gets bumped on the affected bench in that
phase's PR rather than blanket-disabled. The threshold change
itself is reviewable.

### Behavioural invariants the engine preserves

Three things v0.9.x must not change:

1. **Snapshot-stable output across runs and across hosts.** The
   parallel walker re-introduces non-determinism at the index
   level; the post-sort by relative path eliminates it before
   the index leaves `walk()`. Every formatter that already
   sorts (markdown, human) keeps doing so; no rule that today
   produces deterministic output starts producing
   non-deterministic output.
2. **Public `Rule` trait remains add-only.** The dispatch flip
   adds a default `as_per_file(&self) -> Option<&dyn PerFileRule>
   { None }` method; existing rules that don't override it stay
   on the rule-major path. No breaking change to plugin authors
   (relevant once v0.11 WASM plugins ship).
3. **`Violation` / `RuleResult` / `Report` field shapes don't
   change for downstream consumers.** Cow conversion happens
   inside the type — `Violation::message: Cow<'static, str>`
   replaces `String`, but `Violation::message()` continues to
   return `&str` and `Display` impls are unchanged. JSON / SARIF
   / agent output bytes are byte-identical.

### Threading model

The engine already runs rules in parallel via Rayon
(`engine.rs:159` — `entries.par_iter().filter_map(...)`). v0.9
adds two more parallel layers:

- **Walker** — `WalkBuilder::build_parallel` uses the `ignore`
  crate's own thread pool, sized to `num_cpus` by default.
- **Per-file dispatch (v0.9.3)** — the file-major loop also runs
  on Rayon (`index.files().par_bridge()` or chunked via
  `rayon::scope`).

Rayon's pool is shared across all uses, so we don't fork-bomb
the system; nested parallelism is fine. Document that users
running alint inside CI runners with thread budgets can set
`RAYON_NUM_THREADS` to bound it. The parallel walker's thread
count is `WalkBuilder`-level — it has its own knob — so the
first design doc (`parallel_walker.md`) settles whether we add
a top-level `walker_parallelism` config knob or lean on the
`ignore` crate's defaults.

### Heuristic vs. precise

None of the v0.9 sub-themes have heuristic surfaces. Walker
output is deterministic post-sort; Cow / line-slice conversions
are byte-equivalent; dispatch flip preserves per-rule semantics.
The only correctness question is "do we produce the same
violations as v0.8.2?" — gated by the v0.8.2 e2e + integration
suite running unchanged through every v0.9 phase.

### Schema versioning

No schema changes. Every v0.7 / v0.8 config runs unchanged on
v0.9. `version: 1` covers the entire v0.9 cut.

## Implementation order

1. **`parallel_walker.md`** — leaf. Lands as v0.9.1.
2. **`memory_pass.md`** — additive, rule-local. Lands as v0.9.2.
   May further-split per rule if dhat output flags individual
   conversions worth a standalone commit.
3. **`dispatch_flip.md`** — engine restructure. Lands as v0.9.3.

Each phase carries its own `xtask bench-compare` run and a
short `docs/benchmarks/archive/v0.9-development-phases/<phase>/`
snapshot so v0.10's LSP work (which directly builds on the
per-file dispatch hot path) has a documented floor.

## Out of scope for v0.9

Explicitly held back to keep the cut tight:

- **LSP server** — v0.10. The per-file dispatch shape from
  v0.9.3 directly powers the per-file-edit re-evaluation hot
  path, so v0.10 lands cleaner with v0.9 closed first.
- **WASM plugins** — v0.11.
- **`FileIndex` HashMap index.** Today `FileIndex::find_file` is
  a linear scan. Profiling may show it dominates at 100k trees;
  if so, that's a v0.9.x point release, not a v0.9 sub-theme.
  Adding a `HashMap<PathBuf, &FileEntry>` cache is local to the
  walker output and orthogonal to the three planned changes.
- **`mmap` for whole-file rules.** `memmap2` would skip the
  `fs::read` allocation entirely for `file_hash` / `file_max_size`
  / `file_starts_with`. Worth measuring in v0.9.2 dhat runs but
  not committed to — page-cache + `read` is already fast and
  `mmap` adds platform fragility (Windows, network filesystems).
- **Async I/O.** alint is CPU + page-cache bound, not
  network-bound; `tokio` adds runtime weight without throughput
  gain at our scales.

## How to use these docs

Each design doc has the same shape as the v0.7 design pass:

1. **Problem** — what perf pain this addresses, sourced from
   the v0.8.4 benches and the v0.8.5 baseline numbers.
2. **Surface area** — what changes inside the engine / rule
   layer. ("Schema" in the v0.7 docs; v0.9 is engine-internal so
   the equivalent is the new internal contract.)
3. **Semantics** — what the engine does differently on each
   evaluation pass.
4. **False-positive surface** — what could go wrong (ordering
   non-determinism, latent rule bugs surfaced by parallelism,
   Cow lifetime fights) and the planned mitigations.
5. **Implementation notes** — crate location, dependencies,
   complexity estimate.
6. **Tests** — what to cover, including the bench-compare
   thresholds the phase commits to.
7. **Open questions** — decisions to make before implementation.

When implementation starts, the doc gets a `Status:
Implemented in <commit>` header line and any open questions
get resolved in the doc itself (mirroring the v0.7
`git_blame_age.md` "Resolved open questions" block at the
top), not lost in commit messages.
