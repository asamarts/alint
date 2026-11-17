---
status: accepted
date: 2026-11-16
decision-makers: asamarts
---

# 0042. Measure the YAML parser by memory and wall time, not only instructions

## Status

Accepted (2026-11-16). Amends [ADR-0035](0035-keep-the-early-ayaml-cutover.md) on what qualifies
ayaml for v0.23.0: Y-C4, "#510's deterministic performance gate passed", is not enough, and a
memory condition is added. The rest of ADR-0035 stands.

## Context

- **What the audit measured.** A performance audit of `main` against v0.22.1 on the quiet
  benchmark host found that reading YAML through ayaml 0.1.2 costs more than through
  `yaml_serde` 0.10.7, the libyaml-based parser v0.22 used, on files of 10 KB and up:
  - wall time +9% to +54%;
  - peak RSS 1.3x to 2.9x;
  - instruction counts flat.
- **The cause is in ayaml.** Decoding a document into the JSON tree that the structured-format
  rules query, ayaml's peak heap is about 65 bytes per input byte, and libyaml's 35 to 39. ayaml
  holds the whole document's events and the half-built tree at once, where libyaml streams. The
  upstream issue is asamarts/ayaml#163.
- **Why no gate saw it.** The deterministic gate (`det_check`, `det-perf-gate.sh`) counts
  instructions and system calls under Valgrind. Instructions did not move, and no cell had a YAML
  structured rule. Y-C4 of ADR-0035 counted that gate passing as qualification for performance,
  which was too narrow: it is blind to memory, to allocator and cache costs that show only in wall
  time, and to a YAML path no cell exercised.
- **What one large file costs.** `MAX_STRUCTURED_BYTES` caps a structured parse's input at
  32 MiB, and its documentation claimed this bounded one file's tree to about 1 GB. Measured peak
  RSS for one YAML parse at the cap:
  - a `[1,1,...]` flow sequence: 6.2 GB;
  - `k: 1` lines: 4.5 GB;
  - a 16 MiB flow mapping: 5.0 GB.

  The per-file rules run in parallel (rayon), so several large files multiplied this. On the
  OpenAPI documents of the memory test below, the finished tree keeps about 14 bytes per input
  byte; the rest of a parse's peak is the parser's own.

## Decision

1. **v0.23.0 waits for an ayaml release whose peak heap is within 1.2x of the libyaml-based
   reader's** on documents of 37 KB to 11 MB. The gates below are the measure, and this is a
   qualification criterion beside ADR-0035's.
2. **A memory test compares the two parsers.** alint-core's `yaml_memory` test, a binary of its
   own with a counting global allocator, decodes generated OpenAPI-shaped documents of about
   37 KB and 1.1 MB into the JSON tree, through ayaml and through libyaml, in one process and on
   one thread. It fails when ayaml's peak heap over libyaml's exceeds:
   - **the regression bound,** today's ratio and about 10% (1.95; measured 1.78 at 37 KB and 1.73
     at 1.1 MB with ayaml 0.1.2), in every CI run of the YAML Compare job;
   - **the release bound of 1.2,** with `ALINT_YAML_MEMORY_RELEASE=1`, in the release preflight
     (`ci/scripts/yaml-memory-release.sh`). It fails today, which blocks the release as decision
     1 says.
3. **`det_check` gains a YAML cell,** `yaml_openapi_50x37k`: one `yaml_path_matches` rule over 50
   OpenAPI documents of about 37 KB, in a group of its own, so the instruction gate sees the YAML
   path.
4. **YAML parses share a budget of in-flight input.** A parse of repository YAML over 1 MiB takes
   its input's length from a process-wide budget of 32 MiB before it starts, waiting until enough
   is free, and gives it back when the parse ends: at most one maximal file parses at a time, and a
   file of 1 MiB or less never waits.
   - The budget covers `Format::parse`, the entry rows of `extract`, the structured-fix span
     resolver and `pinned_references`. Configuration files are read one at a time and take none.
   - It is held while the parser runs, not while the parsed value lives. A parse's peak is inside
     the parser, and the code that holds a value may parse another file, which would wait on
     itself if values held budget.
   - A `Mutex` and a `Condvar` count the bytes. Nothing inside a reservation takes another one,
     and a thread that asks while it holds one takes nothing and does not wait, so a rayon worker
     that waits never holds budget another parse needs.
   - The input cap stays at 32 MiB, and its documentation gives the measured figures above.

## Consequences

- The v0.23.0 tag depends on an ayaml release. Until one meets the release bound, the release
  preflight fails, and a release from `main` needs ayaml fixed, or an owner decision to revert the
  cutover (ADR-0035 decision 3) or to change this one.
- A regression in ayaml's memory, or in how alint decodes YAML, fails CI before a release does.
  When an ayaml release improves the ratio, the regression bound is lowered to the new ratio and
  about 10%.
- The budget trades parallelism for memory on large files only: two YAML files of 20 MiB parse one
  after the other. Typical repositories, whose YAML is far below 1 MiB, see no change.
- The parsed trees of files that have finished parsing are not counted, so the bound is on
  parses, not on everything alive. A tree is a fraction of its parse's peak (about 14 bytes per
  input byte on the memory test's documents, against 65 at ayaml's peak).
- alint-core gains a test-only dev-dependency, `cap`, the counting allocator: a crate under
  `unsafe_code = "forbid"` cannot implement one itself.
- A kbench cell for wall time and peak RSS of mid-size YAML would need new code in
  `xtask bench-scale`, which records no RSS today, and a results location that the published
  history does not read. It is left for a separate change, #550.

## Considered Options

- **Release with ayaml as it is,** documenting the cost: fastest, but v0.23.0 would make every
  YAML-heavy repository slower and larger in memory than v0.22 for a parser change users did not
  ask for.
- **Revert the cutover** (ADR-0035 decision 3): returns v0.22's costs, and gives up the pure-Rust
  dependency graph and the qualification work.
- **Wait for ayaml, measured by these gates** (chosen): the release waits, and the gates say when
  it may go.
- **Hold the budget until the parsed value is dropped:** bounds trees as well as parses, but needs
  a guard in every caller's value, and a caller that holds one tree while it parses another file
  would wait on itself.

## More Information

- The audit's findings PERF-1, PERF-3 and ROB-8; upstream asamarts/ayaml#163.
- `crates/alint-core/src/yaml_budget.rs`, `crates/alint-core/tests/alloc/yaml_memory.rs`,
  `crates/alint-bench/benches/det_check.rs`.
- [`v0.23/README.md`](../design/v0.23/README.md) section 5 and the status log.
