# Changelog

All notable changes to alint are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.13] — 2026-05-04

Dependency-refresh release. Closes the 10 open Dependabot PRs
plus opportunistic bumps the version specs already accepted.
Net workspace perf neutral-to-positive across S1-S10 — most
scenarios are 4-13 % faster at 100k from upstream improvements
(jsonschema 0.46's API rework + lockfile patches).

### Cargo workspace deps bumped

- **`anstream`** 0.6 → 1.0 — stability commit, no API changes.
- **`trycmd`** 0.15 → 1.x — stability commit.
- **`rand`** 0.9 → 0.10 + **`rand_chacha`** 0.9 → 0.10 (paired
  ecosystem bump). One import in `crates/alint-bench/src/tree.rs`
  switched from `use rand::Rng` to `use rand::{Rng, RngExt, ...}`
  to pick up the new `random_range`/`random` methods (the rand
  0.10 rename of `gen_range`/`gen`).
- **`jsonschema`** 0.29 → 0.46. The 0.36 release privatised
  `ValidationError::instance_path` and exposed it via an
  accessor method; 4 call sites updated
  (`alint-rules/src/json_schema_passes.rs`,
  `alint-output/tests/{cross_formatter,fix_report_schema}.rs`,
  `alint-dsl/tests/schema.rs`).
- **`sha2`** 0.10 → 0.11 — workspace consumers (`alint-dsl/sri`,
  `alint-output/gitlab`, `alint-rules/file_hash`) all use the
  stable `Digest`/`Sha256::new()`/`update`/`finalize` API which
  is unchanged across the major. No code changes needed.
- **Lockfile refresh**: `rustls` 0.23.40, `libc` 0.2.186,
  `wasm-bindgen` 0.2.120, `winnow` 1.0.2, etc. — all in-spec
  patch-level pickups.

### GitHub Actions bumped

| Action | From | To |
|---|---|---|
| `actions/checkout` | v4 | v6 |
| `actions/setup-node` | v4 | v6 |
| `actions/cache` | v4 | v5 |
| `actions/upload-artifact` | v4 | v7 |
| `actions/download-artifact` | v4 | v8 (paired w/ upload) |
| `codecov/codecov-action` | v4 | v6 |
| `webfactory/ssh-agent` | v0.9.0 | v0.10.0 |
| `docker/build-push-action` | v6 | v7 (unpinned only) |
| `docker/login-action` | v3 | v4 (unpinned only) |
| `docker/setup-buildx-action` | v3 | v4 (unpinned only) |
| `docker/setup-qemu-action` | v3 | v4 (unpinned only) |

The SHA-pinned variants of the docker/* actions in `release.yml`
are intentionally left at their pinned SHAs — supply-chain
hardening for the publishing path requires a separate "rotate
SHA + verify" cycle, not a blanket version bump.

### Performance

100k/full deltas vs v0.9.12 baseline:

| Scenario | v0.9.12 | v0.9.13 | Δ |
|---|---:|---:|---:|
| S1 | 163ms | 151ms | **-7 %** |
| S2 | 258ms | 254ms | -2 % |
| S3 | 1301ms | 1130ms | **-13 %** |
| S4 | 156ms | 160ms | +3 % |
| S5 | 885ms | 847ms | -4 % |
| S6 | 1121ms | 1011ms | **-10 %** |
| S7 | 329ms | 316ms | -4 % |
| S8 | 1104ms | 1032ms | **-7 %** |
| S9 | 694ms | 666ms | -4 % |
| S10 | 324ms | 328ms | +1 % |

S3 / S6 / S8 wins are likely jsonschema 0.46's Validator
performance work and lockfile patch updates compounding.
1m/full all within ±5 % (most slightly faster). Full numbers in
[`docs/benchmarks/macro/results/linux-x86_64/v0.9.13/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.13/)
once the bench-record workflow lands the canonical capture.

### Held for v0.9.14+

- **`toml` 0.9 → 1.1** — config-format-stability review needed
  (parser/writer rewrite, `FromStr` semantics changed,
  `serde`/`std` now opt-in default features).
- **`tower-lsp`** dormant dep — the crate appears stalled;
  re-evaluate at v0.10 LSP design time vs maintained
  alternatives (`tower-lsp-server` fork, `lsp-server`,
  `async-lsp`).
- **SHA-pinned `docker/*` actions in `release.yml`** — separate
  rotate + verify cycle.

### Internal

- All 1141 workspace tests pass.
- `cargo clippy --workspace --all-targets --all-features --
  -D warnings` clean.
- `cargo doc --no-deps --workspace` with `RUSTDOCFLAGS=-D warnings`
  clean.

## [0.9.12] — 2026-05-03

Backlog cleanup release closing the explicitly-held v0.9 items
that didn't fit cleanly into earlier cuts. Three structural
audits + one engine refactor + the bench-record CI fix.

### Removed — alint-core API

- **`Rule::wants_git_tracked()`** — deprecated in v0.9.11 with
  a v0.9.12 removal date; now gone. Override
  [`Rule::git_tracked_mode`] instead. The engine consults
  `git_tracked_mode()` (added in v0.9.11) to pick the right
  pre-filtered `FileIndex` for each opted-in rule.

### Added — alint-core API

- **`alint_core::CompiledNestedSpec`** — wraps a
  [`NestedRuleSpec`] with its `when:` source pre-compiled to a
  [`crate::when::WhenExpr`] at rule-build time. Mirrors the
  v0.9.5-era `when_iter:` pattern (parse once at build,
  evaluate per iteration with fresh iter context). The 3
  cross-file iteration rules (`for_each_dir`, `for_each_file`,
  `every_matching_has`) hold `Vec<CompiledNestedSpec>` and
  consume it via the shared `evaluate_for_each` helper.

### Fixed

- **Nested `when:` no longer re-parses per iteration.**
  Pre-v0.9.12 the nested `when:` source string was re-parsed
  inside `evaluate_for_each`'s loop on every (entry,
  nested-rule) pair — at scale (e.g. workspace bundles with
  5000+ packages × 3+ nested rules), thousands of redundant
  parses per cross-file rule eval. The new `CompiledNestedSpec`
  parses each source string exactly once at build time.
  Misconfigured nested-`when:` clauses now surface at config-
  load time instead of mid-evaluation.
- **`bench-record.yml` workflow now completes end-to-end.**
  Pre-v0.9.12 the workflow's `gh pr create` step failed with
  `gh: command not found` on the self-hosted runner — bench
  data was captured + pushed to a `bench-record/<tag>` branch
  but the PR-opening step crashed, leaving stale branches
  (`bench-record/v0.9.{9,10,11}` exist on remote with captured
  but unsurfaced data). v0.9.12 adds an `install gh CLI` step
  before the PR-opening step. Same release adds S10 to the
  workflow's scenarios list (was S1-S9 only, missing the
  v0.9.9 addition).

### Internal

- **`coverage_audit_when_wiring.rs`** (new). Asserts every
  cross-file iteration rule (`for_each_dir`, `for_each_file`,
  `every_matching_has`) wires its `when_iter:` field through
  the shared `parse_when_iter` helper at build time AND
  consults the resulting `Option<WhenExpr>` in its dispatch
  path. Catches the silent-no-op recurrence-risk shape on the
  `when_iter:` axis the same way v0.9.10's audit catches it on
  `scope_filter:`.
- **`coverage_audit_engine_when_dispatch.rs`** (new). Asserts
  every dispatch site in `engine.rs` that calls
  `rule.evaluate(...)` or `run_entry(...)` is preceded by an
  `entry.when` consultation in the surrounding 60 lines, OR
  delegates to `run_entry` (which does the consultation
  centrally). Separate sub-test asserts `run_per_file`'s
  inline `entry.when` check stays in place. Catches a future
  engine extension (e.g., a hypothetical fix-only path or LSP
  single-file re-evaluation path) that adds a new dispatch
  site and forgets the gate.
- **`compile_nested_require` helper** (`alint-rules`'s
  `for_each_dir` module) — single point all 3 cross-file
  iteration rules call to compile their `Vec<NestedRuleSpec>`
  into `Vec<CompiledNestedSpec>` at build time. New iteration
  rules thread their require list through this.

### Held for v0.9.13+ / indefinitely

- **`when:` ownership** remains explicitly out of scope.
  `when:` data (facts/vars/iter) varies at evaluate-time so it
  can't be pre-computed into a struct field; the engine
  already owns the top-level dispatch and the v0.9.12 audits
  + pre-compilation close the remaining gaps.
- **Stale `bench-record/v0.9.{9,10,11}` branches** on remote
  contain captured-but-unsurfaced bench data from before the
  workflow fix. Future bench-record runs (starting v0.9.12)
  produce a fresh PR with S1-S10 + criterion + everything;
  the stale branches can be deleted at the maintainer's
  leisure or kept as historical artefacts of the broken
  state.

## [0.9.11] — 2026-05-03

Structural fix for the `git_tracked_only:` silent-no-op
recurrence-risk shape. v0.9.10 closed the analogous
`scope_filter:` bug class via `Scope` ownership; v0.9.11
closes the `git_tracked_only` shape via engine-side
pre-filtered `FileIndex`es. Rules opted into
`git_tracked_only:` no longer carry a runtime
`if self.git_tracked_only && !ctx.is_git_tracked(...)`
check — the engine hands them a `Context` whose `index`
already iterates only the tracked subset.

### Added — alint-core API

- **`alint_core::GitTrackedMode { Off, FileOnly, DirAware }`**
  enum. Returned by `Rule::git_tracked_mode()` so the
  engine knows which pre-filtered `FileIndex` to hand the
  rule. File-mode rules (`file_exists`, `file_absent`)
  return `FileOnly`; dir-mode rules (`dir_exists`,
  `dir_absent`) return `DirAware`.
- **`Rule::git_tracked_mode(&self) -> GitTrackedMode`**
  trait method (default `Off`). Replaces the boolean
  consultation that `wants_git_tracked()` provided.

### Deprecated

- **`Rule::wants_git_tracked()`** — kept for one minor
  version as a default that delegates to
  `git_tracked_mode() != Off`. Out-of-tree rule plugins
  that override this method continue to work; the engine
  no longer consults it. **Removed in v0.9.12.** Override
  `git_tracked_mode` instead.

### Fixed

- **`git_tracked_only` silent-no-op bug class closed
  structurally.** A new rule that ships
  `git_tracked_only: bool` on its spec but forgets to
  override `git_tracked_mode()` defaults to `Off` —
  same defensive default as today. But once it overrides
  `git_tracked_mode()`, the engine's pre-filtered
  `FileIndex` automatically narrows the rule's iteration
  — no per-evaluate `is_git_tracked(...)` check to
  forget. The audit
  `coverage_audit_git_tracked_only.rs` was updated to
  flag both the missing-mode-override AND the
  re-introduction of any per-rule runtime check.
- **`file_exists` literal-path fast path now applies even
  with `git_tracked_only: true`.** Pre-v0.9.11, the rule
  forced the slow path (entry iteration) when
  `git_tracked_only` was set, because the per-entry
  `is_git_tracked` check ran inside the slow loop. The
  engine's pre-filtered index makes the literal-path
  `contains_file` lookup correct without that check —
  10-30 % S8 speedup at small/medium sizes (see Performance).

### Internal

- **Engine `build_git_tracked_indexes`**: builds at most two
  pre-filtered `FileIndex`es per run (one per `GitTrackedMode`
  in use). Build cost amortises across however many rules opt
  into each mode.
- **`pick_ctx`** extended to route opted-in rules to the
  right pre-filtered `Context`. Existence rules already
  declared `requires_full_index = true`, so the substitution
  is safe — we're swapping the full-index `Context` for a
  tracked-narrowed one.
- **Outside-git-repo silent-no-op preserved** via the
  empty-index fallback: when `git ls-files` returns nothing
  (no repo / non-zero exit), the engine builds empty
  pre-filtered indexes for opted-in modes so rules iterate
  zero entries and fire zero violations.
- 4 existence rules (`file_exists`, `file_absent`,
  `dir_exists`, `dir_absent`) cleaned up: per-evaluate
  `is_git_tracked(...)` / `dir_has_tracked_files(...)`
  checks deleted; `git_tracked_mode()` overrides added.

### Performance

S8 (git overlay) at all sizes is faster:

| Size/mode | v0.9.10 | v0.9.11 | Δ |
|---|---:|---:|---:|
| 1k/full | 24.2ms | 21.9ms | -10 % |
| 1k/changed | 29.9ms | 20.4ms | -32 % |
| 10k/full | 139.7ms | 118.0ms | -15 % |
| 10k/changed | 87.3ms | 74.7ms | -14 % |
| 100k/full | 1068.7ms | 1064.0ms | -0 % |
| 100k/changed | 623.3ms | 579.0ms | -7 % |

The 1k/changed -32 % is mostly the `file_exists`
literal-path fast path applying for the first time with
`git_tracked_only`. Larger sizes show a smaller win
because the slow-path iteration cost is dominant.

### Held for v0.9.12+

- **`Rule::wants_git_tracked()` removal.** Deprecated this
  cycle; remove next.
- **`when:` ownership** remains explicitly out of scope —
  different semantics, no shared silent-no-op shape.

## [0.9.10] — 2026-05-03

Structural fix for the `scope_filter:` silent-no-op bug class.
v0.9.6 / v0.9.7 / v0.9.9 each shipped a different group of rules
that silently dropped `scope_filter:` because rules held a
standalone `Option<ScopeFilter>` field separate from the `Scope`
that owned the path-glob match — the compiler had no way to
catch a rule that forgot to consult both. v0.9.10 collapses the
two so a single `Scope::matches(path, &FileIndex)` covers both
predicates. The bug class can no longer recur.

### Breaking — alint-core API

- **`alint_core::Scope::matches` signature changed** from
  `(&Path)` to `(&Path, &FileIndex)`. Required so the
  `ScopeFilter` ancestor walk can consult the index. CLI users
  + bundled rulesets unaffected; `alint-core`-as-a-library
  consumers (out-of-tree plugin authors) will see a compile
  error at every call site and must thread their `&FileIndex`
  through.
- **`alint_core::Rule::scope_filter()` trait method removed.**
  Any rule that overrode it should drop the override; the
  rule's `Scope` (built via `Scope::from_spec(spec)?`) now
  carries the filter and the engine consults it via the
  per-file dispatch's `path_scope().matches(path, index)`
  call.
- **New `alint_core::Scope::from_spec(spec)` constructor**
  bundles `paths:` + `scope_filter:` parsing into one call.
  Replaces the per-rule `Scope::from_paths_spec(paths)?` +
  `scope_filter: spec.parse_scope_filter()?` two-line
  pattern.
- **New `alint_core::Scope::scope_filter()` accessor**
  exposes the optional filter for dispatch sites that need
  to consult it without going through `matches` (e.g. for
  custom narrowing logic). Most callers should not need it.

### Internal

- **41 rules cleaned up** to drop their now-redundant
  `scope_filter: Option<ScopeFilter>` field, the
  `if let Some(filter) = ...` runtime check inside
  `evaluate()`, and the `impl Rule { fn scope_filter() }`
  override. -502 LOC across the rule files.
- **Engine consultation removed** —
  `crates/alint-core/src/engine.rs::run_per_file` no longer
  separately checks `entry.rule.scope_filter()` after the
  `path_scope().matches()` call; the latter covers it.
- **`for_each_dir` literal-path bypass simplified** — the
  v0.9.9 `nested_rule.scope_filter()` guard is now
  redundant (subsumed by `pf.path_scope().matches(literal,
  ctx.index)`).
- **New audit test** `coverage_audit_scope_owns_filter.rs`
  fails CI if any rule re-introduces a standalone
  `scope_filter: Option<ScopeFilter>` field or re-declares
  the deleted `Rule::scope_filter` trait method. Catches
  recurrence at PR time, not at runtime.

### Performance

- Within ±5 % of v0.9.9 across S1-S10 at all sizes — the per-
  rule iteration shape is unchanged (one `if !scope.matches(p,
  idx) { continue; }` per file), only the field layout is
  flatter. Full numbers in
  [`docs/benchmarks/macro/results/linux-x86_64/v0.9.10/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.10/).

### Held for v0.9.11+

- **`git_tracked_only` ownership** parity with `scope_filter`
  was scoped out — different field semantics, separate work.
  The same recurrence risk applies (a rule could ship with
  `git_tracked_only:` silently dropped) but the field is
  rarer in practice and currently 100 % wired up. Tracked.

## [0.9.9] — 2026-05-03

Patch release for two `scope_filter:` silent-no-op gaps surfaced
by the post-v0.9.8 audit. v0.9.7 wired the filter through the
25 per-file content rules; v0.9.9 closes the residual coverage on
the 17 rules that bypass the engine's per-file dispatch path AND
the `for_each_dir` literal-path bypass introduced in v0.9.8.

### Fixed

- **17 rules now honour `scope_filter:`.** The rules whose
  `Rule::evaluate` iterates `ctx.index.files()` directly
  (rather than opting into the engine's per-file dispatch) had
  no `scope_filter` plumbing at all — the gate silently dropped
  on the way through `build()`, identical shape to the v0.9.6 →
  v0.9.7 bug for per-file content rules. Affects:
  `file_max_size`, `file_min_size`, `no_empty_files`,
  `executable_bit`, `executable_has_shebang`,
  `shebang_has_executable`, `no_symlinks`, `filename_case`,
  `filename_regex`, `no_illegal_windows_names`,
  `max_files_per_directory`, `max_directory_depth`,
  `json_schema_passes`, `command`, `git_blame_age`,
  `no_case_conflicts`. Each rule now stores
  `scope_filter: Option<ScopeFilter>`, exposes it via
  `Rule::scope_filter()`, and short-circuits its
  `ctx.index.files()` loop on out-of-scope files (preserving
  the per-file iteration shape — only the work done per file
  changes). Per-rule unit tests assert narrowing behaviour.
- **`no_submodules` rejects `scope_filter:` at build time.** The
  rule is hardcoded to inspect `.gitmodules` at the repo root —
  it does not iterate the file index, so a `scope_filter` on it
  is meaningless. Build-time rejection via the new
  `reject_scope_filter_with_reason(spec, kind, reason)` helper
  in `alint-core::scope_filter`.
- **`for_each_dir` literal-path bypass now consults `scope_filter`.**
  v0.9.8's fast path (dispatching a nested per-file rule via
  `PerFileRule::evaluate_file` against a single literal,
  bypassing the rule's own `evaluate`) executed regardless of
  whether the literal's ancestor chain satisfied the nested
  rule's `scope_filter:`. Divergent from the rule-major
  fallback, which already honoured the filter (since v0.9.7).
  The bypass guard at
  `crates/alint-rules/src/for_each_dir.rs::evaluate_for_each`
  now consults `nested_rule.scope_filter()` before taking the
  fast path. E2e regression at
  `crates/alint-e2e/scenarios/check/scope_filter/scope_filter_nested_under_for_each_dir.yml`.

### Added

- **`alint_core::reject_scope_filter_with_reason(spec, kind, reason)`**
  — symmetric to `reject_scope_filter_on_cross_file`, but for
  rules whose dispatch shape is hardcoded to a single path. The
  `reason` field surfaces *why* the rule cannot honour the
  filter (improves the error message authors get).
- **Macro bench scenario S10** at
  `xtask/src/bench/scenarios/s10_scope_filter_outside_per_file.yml`
  — 5 rules from outside the PerFileRule dispatch path
  (`file_max_size`, `no_empty_files`, `no_symlinks`,
  `filename_case`, `filename_regex`) each with
  `scope_filter: { has_ancestor: <manifest> }` over the polyglot
  tree. Catches the same bug class S9 catches for per-file
  rules, but on the rule set v0.9.8 silently dropped.
- **17 unit tests** (`scope_filter_narrows`) — one per rule —
  asserting in-scope / out-of-scope narrowing.
- **1 e2e regression** for bug #2 covering the for_each_dir
  literal-path bypass shape.

### Changed

- **`docs/design/v0.9/scope-filter.md`** gains a "Post-v0.9.6
  follow-ups" section documenting the v0.9.7, v0.9.9, and
  v0.9.10 trajectory — captures *why* this kept happening
  (separate `Scope` + `Option<ScopeFilter>` fields without
  compiler-enforced wiring) and the v0.9.10 structural fix
  that prevents recurrence.

### Performance

- 1k/10k/100k S10 numbers within ±5 % of v0.9.8 baseline (the
  per-file iteration shape is unchanged; only the per-file
  cost of an in-scope vs out-of-scope check shifts). The
  point of S10 is regression detection, not a speedup
  headline. Full numbers in
  [`docs/benchmarks/macro/results/linux-x86_64/v0.9.9/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.9/).

### Internal

- All 393 alint-rules tests + 227 e2e scenarios pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  clean.

## [0.9.8] — 2026-05-02

Cross-file dispatch fast paths, round 2. v0.9.5 closed the
`for_each_dir × file_exists` cliff via the lazy `path_set` index;
v0.9.8 closes the residual O(D × N) shapes that v0.9.5 didn't
cover, identified by the comprehensive v0.9.5/.6/.7 macro-bench
backfill (1M S7 was stuck at ~614 s across all three releases).

### Performance

- **1M S7 full: 614 s → 15.3 s (40× speedup).**
- **1M S7 changed: 618 s → 17.9 s (34× speedup).**
- 1M S6 / S8 / S9 within ±5 % of v0.9.7 (no regression on the
  per-file dispatch fast paths).
- See [`docs/benchmarks/HISTORY.md`](docs/benchmarks/HISTORY.md)
  per-scenario tables for the full v0.9.5 → v0.9.8 trajectory
  and the bench captures under
  [`docs/benchmarks/macro/results/linux-x86_64/v0.9.8/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.8/).

### Added

- **`FileIndex::children_of(dir) -> &[usize]`** — direct
  children of `dir`, by index into `entries`. Lazy `OnceLock`
  build mirrors the v0.9.5 `path_set` shape; O(N) build, O(1)
  per-dir lookup.
- **`FileIndex::file_basenames_of(dir) -> impl Iterator<Item = &str>`**
  — direct file children's basenames, borrowed from the
  underlying `Arc<Path>`. Used by `dir_contains` and equivalent.
- **`FileIndex::descendants_of(dir) -> impl Iterator<Item = &FileEntry>`**
  — recursive, depth-first, lazy. Stack-of-iterators state;
  does NOT materialise the full subtree.
- **Debug-only tracing for index builds** — emits `phase=index_build
  kind=<name> elapsed_us=N entries=M` events behind
  `#[cfg(debug_assertions)]`. Release builds compile away both
  the `Instant::now()` timer and the event emission entirely
  (zero overhead for users running release binaries; the events
  are for `xtask bench-scale` profile runs and contributor
  debugging).
- **`coverage_audit_cross_file_dispatch.rs`** — soft-fail audit
  that scans cross-file rule sources for the O(D × N)
  `entries.iter()` pattern v0.9.8 closed. Surfaces gaps for the
  next refactor; doesn't block unrelated work.

### Changed

- **`dir_only_contains`** evaluates via `children_of` instead of
  the per-dir `for file in ctx.index.files()` + `is_direct_child`
  filter. At 1M files / 5K matched dirs, drops the inner loop
  from 1M iterations to ~200 (the dir's actual children).
- **`dir_contains`** evaluates via `children_of` + inline
  basename extraction. Per-(dir, matcher) drops from O(N) to
  O(children).
- **`evaluate_for_each` in `for_each_dir.rs`** (shared by
  `for_each_dir`, `for_each_file`, `every_matching_has`) gained
  a literal-path bypass: when a nested rule's `paths:` template
  resolves to a single literal AND the rule opts into
  `as_per_file()`, dispatch via `evaluate_file` against the
  in-index entry directly instead of running the rule's full
  `evaluate(ctx)` (which would iterate `ctx.index.files()` per
  call, multiplying the 1M scan by the iteration count). Closes
  the residual cliff Phase E iter 1 surfaced (S7's
  `every-lib-has-content` was 484 s post-Phase-C; this drops it
  to milliseconds × N iterations).

### Internal

- New helpers `nested_spec_single_literal` and
  `evaluate_one_per_file_rule` in `crates/alint-rules/src/for_each_dir.rs`.
- `is_direct_child` free function deleted from
  `dir_only_contains.rs` (no longer needed once `children_of`
  is the dispatch shape).
- Memory cost of the new `parent_to_children` index: ~500 KB
  HashMap + ~8 MB usize indices at 1M files / 5K dirs.
  Negligible vs the existing 1 GB `entries` vec.
- All 376 alint-rules tests + 226 e2e scenarios + 183 alint-core
  tests + 11 new walker tests pass.

## [0.9.7] — 2026-05-02

Patch release for the v0.9.6 `scope_filter:` runtime no-op (v0.9.6
shipped the field, the type, and the engine gate but never wired the
parsed filter onto the built rule). Same release also lands the
follow-up audit cleanup (rule-count corrections, design-doc status
flips, alint.org sidebar gaps, release.yml preflight gate) and the
v0.10 LSP design pass with `tower-lsp` added to workspace deps as a
dormant dependency.

### Fixed

- **`scope_filter:` is now honoured at runtime by every per-file rule.**
  v0.9.6 shipped the field on `RuleSpec`, the `ScopeFilter`/`ScopeFilterSpec`
  runtime types, and the engine's per-file dispatch gate
  (`crates/alint-core/src/engine.rs:run_per_file`), but no per-file rule
  builder threaded the parsed filter onto the built rule — `Rule::scope_filter()`
  always returned the trait default `None`, so the gate was a silent no-op
  for every rule (including the bundled ecosystem rulesets that motivated
  the primitive). Each of the 25 per-file rule types now stores a parsed
  `Option<ScopeFilter>`, exposes it via `Rule::scope_filter()`, and gates
  its rule-major fallback path on it as well (so `alint fix` respects the
  filter the same way `alint check` does). New `RuleSpec::parse_scope_filter()`
  helper in `alint-core::config` keeps the per-rule build-site change to
  one line. Integration regression test:
  `crates/alint-rules/tests/scope_filter_integration.rs`.

### Added

- **5 e2e scenarios** under `crates/alint-e2e/scenarios/check/scope_filter/`
  covering positive/negative cases, two-name lists, `extends:` inheritance,
  combined per-file + tree-level gates, and a bundled `rust@v1` dogfood
  against a polyglot tree.
- **10 cross-file `reject_scope_filter_on_cross_file` unit tests**, one per
  cross-file rule that calls the rejection helper (only `pair.rs` had one
  before).
- **`docs/design/v0.10/`** — three-doc design pass for the v0.10 LSP server
  (`lsp_server.md`, `single_file_reevaluation.md`, `vscode_extension.md`)
  mirroring the per-cut design-pass shape used for v0.7 and v0.9.
- **`tower-lsp = "0.20"`** added to `[workspace.dependencies]` ahead of the
  `crates/alint-lsp/` crate landing in v0.10. Dormant — no current member
  depends on it, so `cargo build` doesn't pull it.
- **`development/rule-authoring.md`** now reaches alint.org via
  `xtask docs-export` (previously invisible to the sync script).

### Changed

- **`release.yml` preflight gate**: new top-level job runs fmt + clippy +
  test + `cargo doc -D warnings` on the tagged commit; every publishing
  job (build matrix, release, docker, npm, homebrew, publish-crates)
  `needs:` it. Closes the gap where release.yml could fire while ci.yml
  was failing on the same SHA.
- **`README.md`** rule-count + family-count corrections (54 → 60 across
  thirteen families; was missing `Git hygiene` and `Plugin (tier 1)`),
  ruleset count reconciled (line 28 said 19, line 589 said 17 — both now
  19), version anchor "v0.7 ships" → "v0.9.6 ships".
- **`docs/design/v0.7/{commented_out_code,markdown_paths_resolve}.md`,
  `v0.9/{parallel_walker,scope-filter}.md`** — status headers flipped
  from "Design draft" to "Implemented in v0.7.x / v0.9.x" with crate
  pointers.
- **`docs/site/**` integration pages** — 12 instances of `v0.4.7` updated
  to v0.9.6; `concepts/index.md` "Four formats" → "Eight formats";
  `quickstart.md` ruleset count "eleven" → "nineteen".
- **`action.yml`** format input description lists all 8 formats.
- **`npm/package.json`** version 0.5.10 → 0.9.7 (was four releases stale
  in the checked-in source; release.yml's publish step rewrites this from
  the tag at publish time, but a stale value misled readers).
- **`action-selftest.yml`** pinned-version test 0.3.1 → v0.9.6 (six minors
  stale).
- **`.alint.yml`** dogfood fact `is_rust` → `has_rust` to match the v0.9.6
  bundled-fact rename.

## [0.9.6] — 2026-05-02

Closes the v0.9 cut with the `scope_filter:` primitive — closest-
ancestor manifest scoping for per-file rules, designed to make the
bundled ecosystem rulesets (`rust@v1`, `node@v1`, `python@v1`,
`go@v1`, `java@v1`) correctly handle nested packages in polyglot
monorepos.

### Added

- **`scope_filter: { has_ancestor: <list> }` rule-schema field**
  (per-file rules only). The engine walks `Path::parent()` upward
  from the file and consults the v0.9.5 path-index at each
  directory; first-match-wins on the upward walk gates the rule
  per-file. The file's own directory counts as an ancestor.
  Cross-file rules (`pair`, `for_each_dir`, `file_exists`, etc.)
  reject `scope_filter:` at build time. Design:
  [`docs/design/v0.9/scope-filter.md`](docs/design/v0.9/scope-filter.md).
- **`alint_core::ScopeFilter` + `ScopeFilterSpec` runtime types,
  `Rule::scope_filter()` trait method** (default `None`). Net
  additive — rules that don't override the trait method see no
  behaviour change. New `alint_core::reject_scope_filter_on_cross_file`
  helper + 11 cross-file rule build-time guards.
- **Bench-scale S9 — nested polyglot monorepo** scenario.
  `extends: rust + node + python` over `crates/` + `packages/` +
  `apps/` ecosystem subtrees; new
  `alint_bench::tree::generate_nested_polyglot_monorepo` helper.
  100k S9 = 688 ms ± 13 ms on the published baseline machine.
- **Rule-authoring docs**: `scope_filter:` section in
  [`RULE-AUTHORING.md`](docs/development/RULE-AUTHORING.md) with
  the bundled-ruleset migration recipe.

### Changed

- **Bundled ecosystem rulesets renamed `is_*` → `has_*` for the
  ecosystem facts.** Five rulesets:
  `is_rust` → `has_rust`, `is_node` → `has_node`,
  `is_python` → `has_python`, `is_go` → `has_go`,
  `is_java` → `has_java`. The new name reads better with the
  broadened heuristic (`has_<ecosystem>` is true if any
  ecosystem manifest exists *anywhere* in the tree, not just at
  the root). No backwards-compat alias period — repos that
  override these facts in their own `facts:` blocks need to
  update the identifier; tree-level gates that referenced the
  fact name (`when: facts.is_rust` etc.) need the same update.
- **Bundled ecosystem facts broadened with `**/<manifest>`
  patterns** — `[Cargo.toml]` → `[Cargo.toml, "**/Cargo.toml"]`
  and analogously for the other four. Polyglot monorepos with
  no root manifest now correctly fire the ruleset.
- **Per-file content rules in the five ecosystem rulesets get
  `scope_filter:` added** (e.g.
  `scope_filter: { has_ancestor: Cargo.toml }` on
  `rust-sources-final-newline`). Filename-based rules
  (`filename_case`, etc.) keep their existing `paths:` globs;
  `scope_filter:` is supported on `PerFileRule`-trait rules
  only.

### Performance

`scope_filter:` adds ≤ 150 ns per (file × rule) ancestor-walk
step on the v0.9.5 path-index — sub-millisecond at K100 across
five rules. Same-machine pre/post measurement on the engine
commit (`7b080a0`) shows the null-default `Rule::scope_filter()`
trait method is unmeasurable on rules that don't override it.
See [`docs/benchmarks/investigations/2026-05-scope-filter-baseline-drift/`](docs/benchmarks/investigations/2026-05-scope-filter-baseline-drift/)
for the dispositive A/B and the lesson on machine-state drift in
cross-version bench comparisons.

## [0.9.5] — 2026-05-01

Reopens v0.9 with cross-file dispatch fast paths, the
test/coverage floor that prevents the same class of regression
from slipping by again, and a benchmark-docs reorganisation
that makes the perf story discoverable from a single entry
point. Six sub-phases, all in this release:

| Sub-phase | What it ships |
|---|---|
| .5 | Cross-file dispatch fast paths (lazy path-index on `FileIndex` + literal-path fast paths in `file_exists` / `structured_path` / `iter.has_file`) |
| .6 | Coverage audits (pass/fail symmetry, bundled-ruleset coverage, git-mode symmetry) |
| .7 | 16 new coverage scenarios filling the audit punch list |
| .8 | Bench-scale S6 / S7 / S8 + `generate_git_monorepo` helper |
| .9 | Rule-authoring workflow doc; alint already self-lints via `action-selftest.yml` |
| Reorg | Benchmark documentation reorganisation: `docs/benchmarks/{micro,macro,investigations,archive}/` layout, per-version results, top-level `README.md` / `HISTORY.md` / `RUNNING.md`, new `xtask publish-benches` subcommand |

No new rule kinds, formatters, or schema changes; output bytes
byte-identical to v0.9.4 across all 8 formatters.

Full design:
[`docs/design/v0.9/coverage-and-dogfood.md`](docs/design/v0.9/coverage-and-dogfood.md).
Workflow: [`docs/development/RULE-AUTHORING.md`](docs/development/RULE-AUTHORING.md).
Bench layout: [`docs/benchmarks/README.md`](docs/benchmarks/README.md).

### Performance (.5)

`xtask gen-monorepo --size 1m`, S3 = `oss-baseline + rust +
monorepo + cargo-workspace`, hyperfine `--warmup 1 --runs 3`:

| Cell | v0.9.4 | v0.9.5 | Speedup |
|---|---:|---:|---:|
| `1m S3 full` | 731.856 s | 11.194 s ± 0.154 | **65.4×** |
| `1m S3 changed` | 724.362 s | 6.728 s ± 0.059 | **107.7×** |
| `100k S3 engine total` | 10.7 s | 186 ms | **57×** |
| `10k S3 engine total` | 226 ms | 23 ms | **9.8×** |

Also ~50–80× faster than the published v0.5.6 baseline (the
fastest 1M S3 numbers ever shipped). Full numbers under
[`docs/benchmarks/macro/results/linux-x86_64/v0.9.5/`](docs/benchmarks/macro/results/linux-x86_64/v0.9.5/).

### Engine (.5)

- `alint-core::walker::FileIndex` gains a lazy
  `OnceLock<HashSet<Arc<Path>>>` keyed on every file (non-
  dir) entry. Built on first call to `contains_file` or
  `file_path_set`; concurrent first-call safe via OnceLock.
- New `FileIndex::contains_file(&Path) -> bool` — the
  canonical O(1) "does this exact relative path exist?"
  query. `find_file(&Path)` keeps its signature but does
  the O(1) check first; the linear `&FileEntry` scan only
  runs on a hit. New `FileIndex::from_entries(Vec<FileEntry>)`
  constructor for tests/benches.
- `Engine::run` adds `tracing::info!` per-phase + per-cross-
  file-rule wall-time emission with stable structured fields
  (`phase`, `elapsed_us`, optional `rules` / `files`).
  Drive with `ALINT_LOG=alint_core=info`. Production runs
  pay nothing — events fire only when info is enabled for
  this target.
- Per-file dispatch loop in `Engine::run_per_file` switches
  from `index.files().par_bridge()` to
  `index.entries.par_iter().filter(|e| !e.is_dir)` — the
  native Rayon `ParallelIterator` over the underlying `Vec`
  uses work-stealing slabs instead of par_bridge's Mutex-
  guarded channel. The applicable-rule inner loop carries
  `entry_idx` directly instead of re-resolving via O(L)
  `position` lookup per applicable rule per file.

### Rules (.5)

- `file_exists` — when every `paths:` entry is a literal
  relative path AND the rule is not `git_tracked_only`,
  the build path pre-extracts the literals into
  `Vec<PathBuf>`; evaluate uses `contains_file` per
  literal. Glob/exclude/git-tracked patterns keep the
  existing O(N) scope-match scan.
- `structured_path` (the shared impl behind
  `*_path_{equals,matches}` for json/yaml/toml) — same
  literal-paths fast path. Joins `ctx.root + literal`
  directly for the `std::fs::read` rather than calling
  `find_file`.
- `iter_has_file` (the `iter.has_file("…")` `when_iter:`
  builtin) — when the pattern is a literal filename,
  computes `iter.path.join(pattern)` and consults
  `contains_file`. Glob patterns (`**/*.bzl`) keep the
  scope-match scan.
- `pair::evaluate` swaps `find_file(&p).is_some()` for the
  cheaper `contains_file(&p)`.

### Coverage audits (.6)

Three new tests under `crates/alint-e2e/tests/`:

- **`coverage_audit_pass_fail.rs`** — every canonical rule
  kind has at least one scenario where it fires AND at
  least one where it stays silent. Ships with a small
  `NATIVE_FIRES_ALLOWLIST` for kinds whose firing case
  can't be expressed in YAML today (`executable_bit`,
  `executable_has_shebang`, `no_symlinks`, `git_blame_age`,
  `git_commit_message`); each entry points at the native
  Rust integration test that DOES cover the firing path.
- **`coverage_audit_bundled_rulesets.rs`** — every
  `crates/alint-dsl/rulesets/v1/**/*.yml` is referenced by
  at least one well-formed scenario AND at least one
  ill-formed scenario.
- **`coverage_audit_git_modes.rs`** — the three pure-git
  rule kinds (`git_blame_age`, `git_commit_message`,
  `git_no_denied_paths`) need both an in-repo and an
  outside-git scenario.
- **`coverage_audit_bench_listing.rs`** (soft, always
  passes) — emits an `eprintln!` listing of rule kinds
  absent from any bench scenario.

### Coverage scenarios (.7)

16 new scenarios filling the v0.9.6 audit punch list:

- `structured/{json,toml,yaml}_path_{equals,matches}_silent_when_satisfied.yml` (6)
- `bundled/{agent_context,agent_hygiene,ci_github_actions,docs_adr,hygiene_lockfiles,hygiene_no_tracked_artifacts,tooling_editorconfig}_well_formed_passes.yml` (7)
- `bundled/{agent_context_stub,agent_hygiene_scratch_doc}_flagged.yml` (2)
- `git/git_no_denied_paths_silent_outside_git.yml` (1)

E2e scenario count: 205 → 221. All four `coverage_audit_*`
tests green by default.

### Bench-scale extension (.8)

Three new perf-shape scenarios; S1–S5 unchanged:

- **S6 — Per-file content fan-out**: 13 content rules over
  `**/*.rs`.
- **S7 — Cross-file relational**: `pair`, `unique_by`,
  `for_each_dir`, `for_each_file`, `dir_only_contains`,
  `every_matching_has`.
- **S8 — Git-tracked overlay**: S3 reshape with `.git/` +
  `git_no_denied_paths` + `git_tracked_only`.

New `alint_bench::tree::generate_git_monorepo` helper for
S8 — runs `git init && git add -A && git commit` on a
materialised monorepo tree. `xtask` enum extends to
`Scenario::S1..S8`; default `--scenarios` stays `S1,S2,S3`.

### Workflow doc + dogfood scope (.9)

- New `docs/development/RULE-AUTHORING.md` — the four-step
  process every new rule / bundled ruleset / alias goes
  through; documents the two-layer enforcement (alint
  check . + Rust audits), family conventions, scenario
  shape, the native-test allowlist for testkit gaps, and
  the concrete audit failures contributors will hit.
- The aspirational declarative-coverage rules in
  `.alint.yml` (e.g. "every rule-source file has ≥1 e2e
  scenario") need a rule kind alint doesn't yet have:
  aggregate "does any file in this scope contain pattern
  X?" semantics. Today's `file_content_matches` is
  per-file; the right primitive is deferred to v0.10+.
- alint still self-lints in CI today via the existing
  `.alint.yml` and `action-selftest.yml`; v0.9.9 just
  formalises the workflow contributors should follow.

### Benchmark documentation reorganisation

The `docs/benchmarks/` tree was shaped accidentally over four
release cycles and had four different per-version layouts.
This release lands a clean micro / macro / investigations /
archive split:

- **Top-level** — `README.md` (entry point with current
  numbers), `HISTORY.md` (per-release perf changelog),
  `RUNNING.md` (how-to-run), `METHODOLOGY.md` (focused
  rewrite of the why-behind-the-split).
- **`micro/`** — criterion micro-benches with a per-bench
  catalogue and per-version `results/<arch>/<version>/`
  snapshots.
- **`macro/`** — hyperfine bench-scale with the S1–S8
  catalogue, tool matrix, and per-version
  `results/<arch>/<version>/` snapshots.
- **`investigations/`** — ad-hoc deep-dives (was
  `docs/perf/`); the v0.9.5 cliff investigation lives at
  `2026-05-cross-file-rules/`.
- **`archive/`** — superseded snapshots (v0.1 single-file
  era, v0.9 development-cycle baselines + per-phase
  outputs). Read-only by contract.

`xtask bench-scale --out` default fixes the long-standing
footgun where every run silently overwrote
`docs/benchmarks/v0.5/scale/<arch>/`; new default is
`docs/benchmarks/macro/results/<arch>/v<workspace-version>/`,
read from the workspace `Cargo.toml` so it tracks the
version as it bumps. New `xtask publish-benches` snapshots
`target/criterion/` into the per-version published
location with an optional `--trim` flag dropping criterion's
HTML reports.

### Tooling

- New `xtask gen-monorepo --size {1k|10k|100k|1m} --out PATH`
  materializes a persistent monorepo tree at a fixed path.
  Used by the perf-investigation flow to skip 5+ minutes of
  tree-gen between profile runs. Size labels match
  `bench-scale`'s internal monorepo shape, so trees are
  byte-identical to the published bench corpus.

## [0.9.4] — 2026-04-30

Mechanical follow-up to v0.9.3: migrates 16 of the
remaining ~22 per-file content rules to opt into the
file-major dispatch path. After this cut, every per-file
rule that reads file content benefits from the v0.9.3
read coalescing whenever multiple content rules share a
scope. No new rule kinds, formatters, or subcommands;
every v0.9.3 config runs unchanged. Output bytes from
all 8 formatters are byte-identical to v0.9.3.

### Performance

Each migrated rule grows a `PerFileRule` impl alongside
its existing `Rule` impl; the existing `Rule::evaluate`
body becomes a thin wrapper that walks the index, reads
each file, and delegates to `evaluate_file` (used by
`alint fix` and test harnesses that bypass the engine).

**Migrated (16 rules):**

Content-pattern (5): `file_content_matches`,
`file_content_forbidden`, `file_header`, `file_footer`,
`file_shebang`.

File-property (5): `file_max_lines`, `file_min_lines`,
`file_hash`, `file_is_ascii`, `file_is_text` (declares
`max_bytes_needed: TEXT_INSPECT_LEN`).

Unicode-safety (4): `no_bom` (declares
`max_bytes_needed: 4`; rule-major path uses the bounded
`read_prefix_n` helper), `no_bidi_controls`,
`no_zero_width_chars`, `no_merge_conflict_markers`.

Complex-content (2): `markdown_paths_resolve` (uses
`ctx.index` in `evaluate_file` to look up backticked
paths), `structured_path` (which backs `json_path_*`,
`yaml_path_*`, `toml_path_*`).

### Deliberately not migrated

- `file_max_size` / `file_min_size` — metadata-only
  checks, no `fs::read` to coalesce.
- `json_schema_passes` — emits one repository-level
  violation when the schema fails to compile; the per-
  file dispatch path would multiply that 1 schema-error
  into N file-errors.

### Build / test

- Workspace test suite green (1000+ tests across 7
  crates).
- Pre-v0.9.4 baseline frozen at
  `docs/benchmarks/archive/v0.9-development-baselines/baseline-v0.9.3/criterion/`.
  v0.9.4 numbers at
  `docs/benchmarks/micro/results/linux-x86_64/v0.9.4/criterion/`.

## [0.9.3] — 2026-04-30

Third phase of the v0.9 engine-optimization cut: the
per-file dispatch flip plus the per-rule scanning
conversions deferred from v0.9.2. No new user-visible rule
kinds, formatters, or subcommands; every v0.9.2 config
runs unchanged. Output bytes from all 8 formatters are
byte-identical to v0.9.2.

### Performance

- **Per-file dispatch flip.** New `PerFileRule` trait next
  to `Rule`; rules opt in via `Rule::as_per_file(&self) ->
  Option<&dyn PerFileRule> { None }`. The engine partitions
  entries: per-file rules run under a file-major loop that
  reads each matched file once and dispatches to every
  applicable rule against the same byte buffer. Cross-file
  rules (`pair`, `for_each_dir`, `every_matching_has`,
  `unique_by`, `dir_*`, `file_*` existence rules,
  `git_no_denied_paths`, `git_commit_message`) keep the
  rule-major `par_iter` path. Coalesces N reads of one file
  across N rules sharing it.
- **Line-oriented rules migrated to `PerFileRule` + byte-
  slice scanning** (deferred from v0.9.2):
  `no_trailing_whitespace` (skips the redundant UTF-8
  validation pass via `bytes.split(|&b| b == b'\n')` +
  `last()` byte check), `final_newline` (bounded `last()`
  byte check; `max_bytes_needed: Some(1)`), `line_endings`
  (existing byte-level walker preserved through
  `evaluate_file`), `max_consecutive_blank_lines`,
  `indent_style`, `line_max_width` (the latter three keep
  per-line UTF-8 validation where character counts /
  whitespace classifiers need `&str`).
- **Bounded prefix/suffix reads on byte-pattern rules**
  (deferred from v0.9.2): `file_starts_with` reads only the
  first `prefix.len()` bytes; `file_ends_with` reads only
  the last `suffix.len()` bytes via a new `read_suffix_n`
  helper that seeks from EOF. Both opt into `PerFileRule`
  and declare `max_bytes_needed`.
  `executable_has_shebang` and `shebang_has_executable`
  switch their rule-major paths to bounded 2-byte reads
  (`#!`); they deliberately skip the dispatch-flip path
  because they need `metadata().permissions()` to
  short-circuit on non-`+x` files before any read happens.

### Internal

- New `crates/alint-rules/src/io.rs` helpers:
  `read_prefix_n(path, n)` and `read_suffix_n(path, n)`.
- `Rule::evaluate` for migrated rules becomes a thin
  wrapper that walks the index, reads each file, and
  delegates to `evaluate_file` — used by `alint fix`
  (sequential filesystem mutation rules out coalesced
  reads there) and by test harnesses that bypass the
  engine.
- `Engine::run` aggregates per-file violations back into
  per-rule `RuleResult`s preserving each rule's metadata
  (level / policy_url / is_fixable). Final assembly walks
  `self.entries` in order so output remains deterministic.
- `when:` evaluates once per per-file rule before the file
  loop (not per file) — verdict is independent of the file
  since `iter` is `None` at the engine level.

### Tests

- Four new engine tests exercising the dispatch flip:
  `dispatch_flip_routes_per_file_rule_through_file_major_loop`,
  `dispatch_flip_aggregates_multiple_per_file_rules`
  (verifies aggregation buckets violations per rule across
  N rules sharing one scope),
  `dispatch_flip_passes_when_no_violations` (passing rules
  omitted), `dispatch_flip_preserves_cross_file_rules_unchanged`.

### Migration scope

8 of ~30 per-file content rules migrated to `PerFileRule`
in v0.9.3 (the line-oriented + bounded-read families
deferred from v0.9.2). The remaining ~22 content rules
(`file_content_matches`, `file_content_forbidden`,
`file_header`, `file_footer`, `file_max_lines`,
`file_min_lines`, `file_max_size`, `file_min_size`,
`file_hash`, `file_is_ascii`, `file_is_text`,
`file_shebang`, `json_path_*` / `yaml_path_*` /
`toml_path_*`, `json_schema_passes`, `no_bom`,
`no_bidi_controls`, `no_zero_width_chars`,
`no_merge_conflict_markers`, `markdown_paths_resolve`,
`commented_out_code`) keep the rule-major path. They'll
migrate incrementally in v0.9.4 as a mechanical follow-up;
v0.9.3 ships the engine restructure + 8-rule reference
implementation that proves the shape works.

### Out of scope (deferred)

- Engine-side honouring of the `max_bytes_needed()` hint —
  v0.9.3 reads whole files; a future pass could bound the
  read when every applicable rule's hint sums to less than
  the file size. Hint is captured on the trait so rules
  can declare it now.

### Build / test

- Workspace test suite green (1000+ tests across 7
  crates).
- Pre-v0.9.3 baseline frozen at
  `docs/benchmarks/archive/v0.9-development-baselines/baseline-v0.9.2/criterion/`.
  v0.9.3 numbers at
  `docs/benchmarks/archive/v0.9-development-phases/v0.9.3-dispatch-flip/criterion/`.

## [0.9.2] — 2026-04-30

Second phase of the v0.9 engine-optimization cut: the
type-level memory pass. No new user-visible rule kinds,
formatters, or subcommands; every v0.9.1 config runs
unchanged. Output bytes from all 8 formatters are
byte-identical to v0.9.1.

### Performance

- **`Arc<Path>` on `FileEntry::path` and
  `Violation::path`.** The walker builds one `Arc::from`
  per file; every `Violation` referencing that file now
  shares the same allocation via `Arc::clone` (atomic
  refcount bump) instead of copying the path bytes. At
  100k violations that's ~100k saved path allocations.
  Canonical caller pattern is now
  `.with_path(entry.path.clone())`.
- **`Arc<str>` on `RuleResult::rule_id` and
  `policy_url`.** Set once per rule by the engine, shared
  across N violations of that rule (one alloc per rule
  instead of per violation).
- **`Cow<'static, str>` on `Violation::message`.** Per-
  match templated messages (`format!("line {n}:…")`) live
  as `Cow::Owned(String)` (no change in cost); fixed
  messages can live as `Cow::Borrowed("…")` for a future
  audit pass. Public API preserves `&v.message: &str`
  via `Cow::as_ref`.

### Internal

- `Violation::with_path(impl Into<Arc<Path>>)` accepts
  `PathBuf`, `&Path`, `Box<Path>`, and `Arc<Path>`. Tests
  using a string literal migrate to
  `with_path(Path::new("…"))`.
- `Violation::new(impl Into<Cow<'static, str>>)` accepts
  `&'static str`, `String`, and `Cow<'static, str>`.
  Borrowed `&str` from non-static lifetimes pass via
  `.to_string()`.
- ~70 rule call sites and 8 output formatter structs
  migrated. Output formatters that previously held
  `Option<PathBuf>` / `String` internally now borrow
  `Option<&'a Path>` / `&'a str` from the source where
  possible (json, agent, markdown), or convert at the
  serialise boundary via `.to_string()` / `.as_deref()`
  where the format requires owned.
- `human` formatter: `BTreeMap` keyed on
  `Option<Arc<Path>>` instead of `Option<PathBuf>` — sort
  order unchanged (Arc<Path> sorts via Path's Ord impl).
- `markdown` formatter: bucket map switches to
  `BTreeMap<&'a Path, …>` — eliminates the per-violation
  path clone the bucketing did before.
- `unique_by` and `no_case_conflicts` group on
  `Arc<Path>` internally instead of `PathBuf`.

### Scope deferred to v0.9.3

The original v0.9.2 design scope included per-rule
byte-slice scanning conversions (the 6 line-oriented rules:
`no_trailing_whitespace`, `final_newline`, `line_endings`,
`max_consecutive_blank_lines`, `indent_style`,
`line_max_width`) and bounded prefix/suffix reads (the 4
first-/last-bytes-only rules: `file_starts_with`,
`file_ends_with`, `executable_has_shebang`,
`shebang_has_executable`). Both moved to v0.9.3 because
the per-file dispatch flip hands each rule a pre-loaded
`&[u8]` slice — bundling the conversions there avoids
touching the same rule bodies twice. See
`docs/design/v0.9/memory_pass.md` and
`docs/design/v0.9/dispatch_flip.md` for the rationale.

### Build / test

- Workspace test suite green (1000+ tests across 7
  crates).
- `target-baseline*/` added to `.gitignore` for the
  baseline-bench scratch dirs the v0.9.x flow uses.
- Pre-v0.9.2 baseline frozen at
  `docs/benchmarks/archive/v0.9-development-baselines/baseline-v0.9.1/criterion/` for
  same-day per-phase delta comparisons. v0.9.2 numbers
  at `docs/benchmarks/archive/v0.9-development-phases/v0.9.2-memory-pass/criterion/`.

## [0.9.1] — 2026-04-30

First phase of the v0.9 engine-optimization cut. Parallel
walker. No user-visible rule kinds, formatters, or
subcommands change; every v0.8 config runs unchanged.

### Performance

- **Walker is now parallel.** `crates/alint-core/src/walker.rs`
  switches from `WalkBuilder::build()` (single-threaded
  iterator) to `WalkBuilder::build_parallel()` driving a
  per-thread `ParallelVisitor` that accumulates `FileEntry`s
  in a thread-local `Vec` and merges via `Drop`. A
  deterministic post-sort by relative path
  (`entries.sort_unstable_by(|a, b| a.path.cmp(&b.path))`)
  restores the byte-identical output that snapshot tests +
  formatters depend on. Walker output is the same as v0.8.2
  for any input tree; only the intermediate execution is
  parallel.

  Numbers vs the captured pre-v0.9 baseline at
  `docs/benchmarks/archive/v0.9-development-baselines/baseline-pre/criterion`:

  | bench | before | after | delta |
  |---|---:|---:|---:|
  | `walker/10000` | 52.25 ms | 18.67 ms | **-64.26%** |
  | `walker/1000` | 8.85 ms | 5.25 ms | **-40.62%** |
  | `walker/100` | 1.62 ms | 2.62 ms | +61.18% |

  The walker/100 regression is the small-N thread-spawn-
  overhead trade the design doc anticipated — 1ms of
  overhead at the 100-file size, dwarfed by the 33.6ms saved
  at 10k files. v0.7.0 gate stays green; max delta -4.77% on
  `rule_engine/1000`.

### Added — design + benchmark snapshots

- **`docs/design/v0.9/`** — design pass for the v0.9 cut.
  README + per-sub-theme drafts for the parallel walker
  (this release), memory-footprint pass (v0.9.2), and
  per-file-rule dispatch flip (v0.9.3). Same shape as
  `docs/design/v0.7/` (Problem → Surface → Semantics →
  False-positive surface → Implementation → Tests → Open
  questions).
- **`docs/benchmarks/archive/v0.9-development-baselines/baseline-pre/`** — frozen
  criterion snapshot captured on `bec0cf4` (the v0.9 starting
  point) so subsequent v0.9 phases have a same-day, same-
  hardware "before" reference to measure deltas against.
- **`docs/benchmarks/archive/v0.9-development-phases/v0.9.1-parallel-walker/`** —
  post-merge bench snapshot for this release, with a README
  documenting the deltas vs both the v0.7.0 floor (the gate)
  and the pre-v0.9 baseline (the per-phase delta).

### Tests

- Three new walker unit tests:
  `walk_output_is_deterministic_across_runs` (two runs over
  the same tree produce equal `FileIndex`s — guard against a
  forgotten post-sort), `walk_output_is_alphabetically_sorted`
  (catches a sort that silently breaks), and
  `walk_handles_thousand_files` (concurrency stress: exactly
  1k entries, sorted, no duplicates / drops).

### Internal

- `walk()` body factored into `build_walk_builder` and
  `result_to_entry` private helpers so the visitor closure
  stays small. No public API change — `walk(root, opts) ->
  Result<FileIndex>` is unchanged.

### Out of scope (v0.9.2 / v0.9.3 sub-themes)

- Memory-footprint pass (Cow audit on `Violation` /
  `RuleResult` / `Report`; line-slice scanning on the line-
  oriented rules; dhat workflow). Designed in
  `docs/design/v0.9/memory_pass.md`; ships as v0.9.2.
- Per-file-rule dispatch flip (new `PerFileRule` sub-trait;
  file-major engine loop; coalesce reads when N rules share
  one file). Designed in `docs/design/v0.9/dispatch_flip.md`;
  ships as v0.9.3.

## [0.8.2] — 2026-04-29

Second hotfix on v0.8.0; supersedes v0.8.1. Same code as
v0.8.0 / v0.8.1; manifest + script changes only.

v0.8.1 reverted `publish = false` on the three internal
crates (the v0.8.0 audit-residue bug) but the next publish
attempt failed at `alint-dsl@0.8.1` with "no matching
package named `alint-rules` found". Cargo publish validates
**dev-dependencies** too, and `alint-dsl` carries
`alint-rules` as a `[dev-dependencies]` entry (added by
v0.8.5's fixture-completeness test). With the original
publish order (`alint-core → alint-dsl → alint-rules →
alint-output → alint`), `alint-dsl` packages before
`alint-rules` is on crates.io.

v0.8.1 thus shipped to GitHub Releases, npm, Homebrew,
Docker, and `alint-core@0.8.1` on crates.io — but
`alint-dsl@0.8.1` and downstream (alint-rules, alint-output,
alint) all failed.

### Fixed

- **`ci/scripts/publish-crates.sh`** — re-ordered the
  `CRATES` list so `alint-rules` and `alint-output` publish
  before `alint-dsl`: `alint-core → alint-rules →
  alint-output → alint-dsl → alint`. The new order
  satisfies every dep direction including dev-deps.
- `cargo install alint` against v0.8.2 resolves cleanly.
  v0.8.0 and v0.8.1 remain partially published on
  crates.io; consumers should pin to v0.8.2 or `latest`.

## [0.8.1] — 2026-04-29 (partially published — superseded by v0.8.2)

Reverted `publish = false` on alint-dsl/rules/output to fix
v0.8.0's crates.io publish failure. Pipeline progressed
further (alint-core@0.8.1 published) but failed on
alint-dsl@0.8.1 because of an unrelated dep-order issue —
see v0.8.2 above for the resolution. Use v0.8.2 instead.

### Changed (carried into v0.8.2)

- **`alint-dsl` / `alint-rules` / `alint-output`** —
  reverted `publish = false`. Their manifests now publish to
  crates.io alongside `alint-core` and `alint`. The crates'
  descriptions still read "Internal: Not a stable public
  API" — the published status is a load-bearing accident of
  cargo's resolver, not an invitation to use them. The
  comment block on each manifest documents the constraint
  so a future audit doesn't repeat the trap.

## [0.8.0] — 2026-04-29

> **Note**: this tag shipped to GitHub Releases, npm, Homebrew,
> and Docker, plus `alint-core` on crates.io — but the `alint`
> binary's crates.io publish failed (see v0.8.1 above for the
> root cause + fix). For `cargo install alint` use v0.8.1+; all
> other distribution channels for v0.8.0 are usable.

The v0.8 cut — five sub-phases (v0.8.1 → v0.8.5) building
the comprehensive test/bench/rot-prevention foundation that
v0.9's engine optimization needs to land safely. No new
user-facing rule kinds, formatters, or subcommands; entirely
internal. Schema-compatible: every v0.7 config runs unchanged.

### Added — test infrastructure

- **Cross-platform CI** — `.github/workflows/cross-platform.yml`
  runs `cargo test --workspace --locked` on macOS-arm64 and
  Windows-x86_64 GitHub-hosted runners on every PR/push.
  Caught two real production bugs the Linux lane missed
  (glob mixed-separator + bench-compare key separator,
  both fixed before merge).
- **Coverage workflow** — `.github/workflows/coverage.yml`
  runs `cargo llvm-cov` over the workspace (xtask excluded
  as dev tooling). Enforces 85% line-coverage floor;
  workspace currently at 90.57%. LCOV + HTML uploaded as
  artifacts; opt-in Codecov upload via `CODECOV_TOKEN`.
- **Mutants nightly** — `.github/workflows/mutants.yml`
  rotates one workspace crate per night through cargo-mutants.
  Surfaces unkilled mutants as test-coverage gaps via
  uploaded artifacts.
- **Comprehensive bench suite** —
  `single_file_rules.rs`, `cross_file_rules.rs`,
  `structured_query.rs`, `output_formats.rs`,
  `fix_throughput.rs`, `blame_cache.rs`, `dsl_extends.rs`
  under `crates/alint-bench/benches/`. Plus hyperfine
  scenarios S4 (agent-hygiene) + S5 (fix-pass) and a walker
  parallelism baseline captured for v0.9.

### Added — rot prevention

- **`xtask bench-compare`** — diffs two `target/criterion/`
  trees and gates on regressions past `--threshold`
  (default 10%). PR-time perf-regression gate.
- **JSON report schemas** —
  `schemas/v1/check-report.json` and
  `schemas/v1/fix-report.json` lock the public contract
  for `alint check --format json` and
  `alint fix --format json`. Cross-formatter and
  fix-report tests validate output against the schemas.
- **Fixture-completeness test** in alint-dsl asserts the
  canonical `all_kinds.yaml` fixture exercises every
  registered rule kind (now 70, up from 18).
- **Scenario-coverage audit** in alint-e2e asserts every
  registered rule kind has at least one e2e scenario.
- **Default-option snapshot** in alint-dsl captures every
  rule's resolved Debug output into a checked-in snapshot.
  Catches silent shifts to `#[serde(default = ...)]`
  values (e.g. `commented_out_code::min_lines`).
- **CLI flag inventory snapshot** captures the full
  per-subcommand flag list separately from `--help` text
  to catch flag-name drift independently of help-text edits.
- **Cross-formatter snapshot test** —
  `crates/alint-output/tests/cross_formatter.rs`. Same
  fixed Report rendered through all 8 output formatters
  with 13 invariant tests (rule_id presence, JSON parses,
  schema_version key, SARIF driver completeness, etc.).
- **Pty integration test** —
  `crates/alint/tests/pty_color.rs` (Unix-only) covers
  the `--color=auto` resolution branch trycmd's pipe-only
  spawn can't reach.
- **`.gitattributes`** pins LF for byte-stable test
  artifacts (snapshots, trycmd `.stdout`/`.stderr`/`.toml`,
  YAML fixtures) so Windows checkouts don't introduce
  CRLF drift.

### Added — coverage breadth

- **~155 new rule-kind unit tests** across 34 previously
  under-covered rule kinds (build / options / evaluate
  fires / evaluate silent / edge cases — the standard
  quintet).
- **Fail-variant e2e scenarios** for the 3 pass-only
  unix-metadata rules (`no_symlinks`, `executable_bit`,
  `executable_has_shebang`).
- **E2E scenarios** for the 4 zero-e2e rules
  (`json_schema_passes`, `git_no_denied_paths`,
  `git_commit_message`, `command`).
- **Integration tests** under `crates/alint-rules/tests/`
  for the shell-out rules: `git_no_denied_paths`,
  `git_commit_message`, `command` (mirrors v0.7.3's
  `git_blame_age` integration-test pattern).
- **alint-core internal tests** — `engine.rs`, `walker.rs`,
  `registry.rs`, `report.rs`, `error.rs`, `level.rs`,
  `scope.rs`, `config.rs`, `rule.rs` all gained unit
  tests (had 0 pre-v0.8.2).
- **alint-dsl edges** — extends-chain cycle detection,
  diamond inheritance, `extends:` filter validation,
  nested-config path-prefix rewriting, `.alint.d/` merge
  order determinism, HTTPS timeout / size-cap
  enforcement, SRI algorithm-mismatch errors,
  template-instantiation edge cases.

### Added — CLI surface

- **trycmd 33 → 56 cases** — stderr snapshots for every
  error path, per-subcommand `--help` snapshots,
  `--color × NO_COLOR × CLICOLOR_FORCE` matrix,
  `--progress × TTY/non-TTY` matrix.

### Fixed — production bugs surfaced by the new lanes

- **DSL nested-config glob mixed separators** — on
  Windows, `discover_nested` joined `rel_dir.to_string_lossy()`
  (native separators) with the user's pattern (`/`), producing
  globs like `packages\foo/README.md` that globset couldn't
  match. Nested-config rules silently no-op'd on Windows. Now
  the prefix is normalised to `/` before joining.
- **xtask bench-compare key separators** — comparison keys
  carried native separators, breaking cross-OS tree
  comparison (`target/criterion-main` from a Linux runner
  vs. `target/criterion` from a Windows checkout). Now
  normalised to `/`.
- **CLICOLOR_FORCE on `--color=auto`** — the `auto`
  resolution path didn't honor `CLICOLOR_FORCE` before
  passing the choice to anstream. Fixed via a
  `ColorChoice::resolve()` pre-pass.

### Internal — release safety

- `alint-dsl`, `alint-rules`, `alint-output` now carry
  `publish = false`. Their descriptions have always read
  "Internal: Not a stable public API"; the manifest now
  matches. Historical v0.5.x → v0.7.0 versions remain on
  crates.io for compatibility; new versions are gated off.
- `ci/scripts/publish-crates.sh` reduced to publishing
  only `alint-core` (public library) and `alint`
  (binary). The other three are internal implementation
  detail.

## [0.7.0] — 2026-04-28

Closes the v0.7 cut. Three new rule kinds and two new
subcommands targeting agent-driven development workflows
specifically. Where v0.6 was config-only — bundled rulesets
composed from existing primitives — v0.7 extends the engine
itself: `markdown_paths_resolve` / `commented_out_code` /
`git_blame_age` are new rule kinds with their own heuristic
surfaces, and `alint suggest` / `alint export-agents-md`
add the first new top-level subcommands since `alint init`
in v0.5.4.

Schema-compatible: every v0.6 config runs unchanged. The new
rule kinds parse new YAML shapes that older configs simply
don't use; `version: 1` continues to cover them.

The two subcommands close the cold-start adoption gap for
agent-heavy repos. `alint suggest` scans for known
antipatterns and proposes rules to catch them — its
stale-TODO suggester eats `git_blame_age`'s own dogfood.
`alint export-agents-md` makes alint the single source of
truth for `AGENTS.md` / `CLAUDE.md` / `.cursorrules`
directive blocks: the agent reads what alint enforces, no
duplicate config to maintain.

### Added

- **`alint export-agents-md` subcommand** — generate (or
  maintain a section of) `AGENTS.md` from the active rule
  set, so the agent's pre-prompt directives stay in sync with
  the lint config. Closes the "67% of teams maintain
  duplicate configs between AGENTS.md and CI lint" gap by
  making alint the single source of truth.

  Two output formats:

  - `markdown` (default) — section-per-severity bullet
    list shaped to drop into an `AGENTS.md` /
    `CLAUDE.md` / `.cursorrules` directive block.
  - `json` — stable shape behind `schema_version: 1`,
    parallel to `suggest`'s envelope, suitable for agent
    consumption.

  Three output destinations:

  - **stdout** (default) — pipe / paste by hand.
  - **`--output PATH`** — overwrite-create the named
    file.
  - **`--inline --output PATH`** — splice the generated
    section between `<!-- alint:start -->` and
    `<!-- alint:end -->` markers in the target file. The
    canonical workflow: humans own the prose outside the
    markers; alint owns the directive block between
    them. Re-runs are idempotent — when the existing
    between-markers content already matches what we'd
    generate, the file isn't rewritten.

  Inline mode auto-initialises markers when the target
  file lacks them: the section is appended to the end
  with a stderr warning, and subsequent runs splice in
  place. Multiple-pair / orphan-marker shapes hard-error
  rather than silently overwrite — splicing is destructive
  and ambiguity surfaces explicitly.

  Severity grouping: `Errors (commit will fail)`,
  `Warnings (review before merge)`, optional
  `Info (informational nudges)` (gated by
  `--include-info` — info-level rules are nudges, not
  directives, and clutter the agent's context window
  unless you really want them). Rules without an explicit
  `message:` fall back to a synthesised "<kind> rule"
  line so no directive is silently dropped.

  Stable byte-for-byte output across runs: line endings
  always `\n`, sort by severity desc + rule_id asc within
  each section. Re-running `--inline` produces identical
  bytes; round-trip identity short-circuits the write.

  ```bash
  alint export-agents-md                          # stdout
  alint export-agents-md --output AGENTS.md       # write a file
  alint export-agents-md --inline --output AGENTS.md  # splice in place
  alint export-agents-md --format=json            # stable JSON for agents
  alint export-agents-md --include-info           # include info-level rules
  alint export-agents-md --section-title "Lint policy"  # custom heading
  ```

  Design doc: `docs/design/v0.7/alint_export_agents_md.md`.

- **`alint suggest` subcommand** — scan the repo for known
  antipatterns and propose rules that would catch them. Acts
  as a smart `alint init` for retrofitting alint onto a long-
  running, agent-heavy codebase. Three output formats:

  - **`--format=human`** (default): colourised proposal table
    with optional `--explain` evidence block.
  - **`--format=yaml`**: paste-ready config snippet (`extends:`
    + `rules:`).
  - **`--format=json`**: stable shape behind
    `schema_version: 1` for agent consumption.

  Three suggester families ship in v0.7.4:

  1. **Bundled-ruleset** — high-confidence proposals for
     `oss-baseline@v1` (always) plus per-language
     (`rust@v1` / `node@v1` / `python@v1` / `go@v1` /
     `java@v1`) and workspace-flavour overlays based on the
     same ecosystem detection `alint init` uses.
  2. **Antipattern** — medium-confidence proposal of
     `extends: agent-hygiene@v1` when the repo contains
     backup-suffix files, scratch / planning docs at root
     (`PLAN.md`, `NOTES.md`, …), or `console.log`-style debug
     residue in non-test JS / TS source. `tests/`,
     `__tests__/`, `fixtures/`, and `snapshots/` paths are
     skipped automatically.
  3. **Stale-TODO** — medium-confidence `git_blame_age` rule
     proposal when ≥ 3 `TODO` / `FIXME` / `XXX` / `HACK`
     markers are older than 180 days. Eats our own
     v0.7.3 dogfood.

  `--include-bundled` overrides the already-covered filter
  (which would otherwise skip a `rust@v1` proposal when the
  user's existing `.alint.yml` already extends it).
  `--confidence={low,medium,high}` raises the floor on what
  surfaces. The command always exits 0 unless the scan
  itself fails — `suggest` is exploration, not a CI gate.

- **`--progress={auto,always,never}` + `-q` / `--quiet`
  global flags** — controls stderr-side progress for slow
  commands. Strict stream split: structured stdout
  (`--format=json` / `yaml` / `human`) is byte-for-byte
  clean regardless of progress activity; spinners and
  status lines live exclusively on stderr.

  - `auto` (default): animated bars when stderr is a TTY;
    one-line milestones to plain stderr when captured
    (CI logs).
  - `always`: same as `auto` plus a stderr summary line.
    Bars still require a TTY — non-TTY `always` falls back
    to one-line milestones.
  - `never`: zero stderr noise. `--quiet` is the alias.

  `indicatif` powers the animated bars; the
  `crates/alint/src/progress.rs` module wraps it behind a
  null-handle pattern so suggester code passes
  `&Progress` without branching on visibility.

- **`git_blame_age` rule kind** — fire on lines matching a regex
  whose `git blame` author-time is older than `max_age_days`.
  Closes the gap between `level: warning` on every TODO (too
  noisy) and `level: off` (accepts unbounded debt accumulation).
  Same regex match shape as `file_content_forbidden`, plus a
  per-line age gate. New `{{ctx.match}}` message placeholder
  substitutes capture group 1 (or the full match when no
  capture is present), so messages can be specific about which
  marker was caught. Outside a git repo, on untracked files, or
  when blame fails for any other reason, the rule silently
  no-ops per file — same advisory posture as
  `git_no_denied_paths` / `git_commit_message`. Check-only.

  ```yaml
  - id: stale-todos
    kind: git_blame_age
    paths: "**/*.{rs,ts,tsx,js,jsx,py,go,java}"
    pattern: '\b(TODO|FIXME|XXX|HACK)\b'
    max_age_days: 180
    level: warning
    message: "`{{ctx.match}}` is >180 days old — resolve or remove."
  ```

  Engine plumbing: a new shared `BlameCache` is built once per
  run when any rule reports `wants_git_blame()`, so multiple
  blame-aware rules over overlapping `paths:` re-use the parsed
  output. Cache memoises both successes and failures so a
  large rule fan-out doesn't re-shell-out to git per file.
  Heuristic notes (formatting passes reset blame age unless
  `.git-blame-ignore-revs` is honoured; vendored / imported
  code carries the import commit's timestamp; squash-merged
  PRs collapse to a single date) are documented in
  `docs/rules.md` and `docs/design/v0.7/git_blame_age.md`.

  Pairs naturally with `alint check --changed` so blame only
  runs over modified files in CI.

  Design doc: `docs/design/v0.7/git_blame_age.md`.

- **`commented_out_code` rule kind** — heuristic detector for
  blocks of commented-out source code (as opposed to prose
  comments, license headers, doc comments, or ASCII banners).
  Counts the fraction of non-whitespace characters that are
  structural punctuation strongly biased toward code
  (`( ) { } [ ] ; = < > & | ^`); scores ≥ `threshold` (default
  0.5 after normalisation; midpoint between obvious-prose 0.0
  and obvious-code 1.0) mark the block as code-shaped.
  Supports rust / typescript / javascript / python / go / java
  / c / cpp / ruby / shell. Doc-comment blocks (`///`, `//!`,
  `/** */`) and the file's first `skip_leading_lines` lines
  (default 30 — license headers) are excluded by construction.
  Runs of 5+ identical characters (`============`, `----`,
  `####`) are dropped before scoring so ASCII-art separators
  don't flag as code. Severity floor is `warning`, never
  `error` by default — heuristics have non-zero FP rate.
  Check-only — auto-removing commented-out code is
  destructive. Field-tested against 5 repos (alint, alint.org,
  Aider, Cline, OpenHands) before commit: zero false positives
  across 16 hits.

  ```yaml
  - id: no-commented-code
    kind: commented_out_code
    paths: "src/**/*.{ts,tsx,js,jsx,rs,py}"
    min_lines: 3
    threshold: 0.5
    level: warning
  ```

  Design doc: `docs/design/v0.7/commented_out_code.md`.

- **`markdown_paths_resolve` rule kind** — validates that
  backticked workspace paths in markdown files resolve to
  real files or directories. Targets the AGENTS.md /
  CLAUDE.md / `.cursorrules` staleness problem more
  precisely than v0.6's regex-heuristic
  `agent-context-no-stale-paths` rule. Required `prefixes`
  field declares which path-shapes to validate (eliminating
  the "is this a path or a word" question by construction).
  Skips fenced and 4-space-indented code blocks. Strips
  trailing punctuation, trailing slashes, `:line` /
  `#L<n>` location suffixes before lookup. Glob characters
  in the path resolve via the file index. Check-only.
  Design doc: `docs/design/v0.7/markdown_paths_resolve.md`.

  ```yaml
  - id: agents-md-paths-resolve
    kind: markdown_paths_resolve
    paths: ["AGENTS.md", "CLAUDE.md", ".cursorrules"]
    prefixes: ["src/", "crates/", "docs/"]
    level: warning
  ```

## [0.6.0] — 2026-04-27

Two bundled rulesets and a new output format aimed at the
agent-driven-development era. Schema-compatible: every v0.5.12
config runs unchanged, and the new bundled rulesets compose
from rule kinds that have shipped since v0.1.

The framing: alint's structural / repo-shape niche
(filesystem shape and contents of a repository, not the
semantics of code inside it) fits agent-driven development
naturally. Coding agents leave characteristic structural
debris — backup-suffix files, scratch / planning docs,
debug-print residue, stale model-attributed TODOs — that
existing rule kinds catch cleanly when packaged into the
right bundled ruleset. v0.6 does that packaging, plus a
new output format optimised for agents consuming alint
inside their own self-correction loops. No new rule
kinds, no engine changes, no architectural shift.

### Added

- **`alint://bundled/agent-hygiene@v1`** — six-rule bundled
  ruleset targeting the leftover patterns that show up
  disproportionately in commits authored or co-authored by
  Claude Code, Cursor, Copilot agent, Aider, Codex, and
  similar tools. Composes with the existing `hygiene/*`
  rulesets — extend all three on agent-heavy projects
  without overlap:

  ```yaml
  extends:
    - alint://bundled/hygiene/no-tracked-artifacts@v1
    - alint://bundled/hygiene/lockfiles@v1
    - alint://bundled/agent-hygiene@v1
  ```

  Rules:
  - `agent-no-versioned-duplicates` — bans filenames matching
    `*_old.*` / `*_new.*` / `*_final.*` / `*_FINAL.*` /
    `*_copy.*` / `*_backup.*` / `*.copy.*` (warning). The
    `*_v[0-9]*` / `*-v[0-9]*` patterns were considered and
    deliberately omitted — too many real codebases use those
    for legitimate API versioning, schema migrations, release
    notes, and versioned tests (`gitlab_v1_*.py`,
    `076_add_v1_tables.py`, `release-notes-v1.md`,
    `test_v1_api.py`).
  - `agent-no-scratch-docs-at-root` — bans `PLAN.md` /
    `NOTES.md` / `ANALYSIS.md` / `SUMMARY.md` / `FIX.md` /
    `DECISION.md` / `TODO.md` / `SCRATCH.md` / `DEBUG.md` /
    `TEMP.md` / `WIP.md` at the repo root (warning,
    `root_only: true`).
  - `agent-no-affirmation-prose` — flags AI-style stock
    phrases in source / markdown (`"You're absolutely
    right"`, `"Excellent question"`, `"Happy to help"`,
    etc.) (info).
  - `agent-no-console-log` — bans `console.log` / `.debug` /
    `.trace` in non-test JS / TS source (warning). Excludes
    test directories (`**/*test*/**` — broader than
    `test*/**`, catches `cross-sdk-tests/`, `e2e-tests/`,
    etc.), build / dev tooling configs, `**/scripts/**`,
    `**/website/**` / `**/public/**` / `**/demo/**`,
    `**/vendor/**` and `**/.claude/**` (agent-worktree scratch
    space).
  - `agent-no-debugger-statements` — bans `debugger;` /
    `breakpoint()` in non-test source (error). The regex
    requires `;` immediately after `debugger` so the rule
    doesn't trip on the WORD "debugger" appearing in prose
    comments. Same exclusion list as the console-log rule.
  - `agent-no-model-todos` — bans `TODO(claude:)` /
    `FIXME(cursor:)` / `XXX(gpt:)` and similar
    model-attributed markers (warning). Excludes CHANGELOG /
    ROADMAP / cookbook / test directories — projects that
    document these patterns trip on their own examples
    otherwise.

- **`alint://bundled/agent-context@v1`** — four-rule bundled
  ruleset for the agent-instruction files coding agents read
  on every session: `AGENTS.md` (the cross-tool standard
  backed by agents.md / OpenAI Codex), `CLAUDE.md`,
  `.cursorrules`, `GEMINI.md`, and
  `.github/copilot-instructions.md`. Gated by
  `facts.has_agent_context` so it's a safe no-op in repos
  without any of these files; extend it unconditionally even
  from polyglot configs.

  Rules:
  - `agent-context-recommended` — `file_exists` info-level
    nudge.
  - `agent-context-non-stub` — `file_min_lines: 10`
    (warning).
  - `agent-context-not-bloated` — `file_max_lines: 300`
    (info). Threshold from Augment Code's 2026-03 research
    on AGENTS.md effectiveness.
  - `agent-context-no-stale-paths` — regex-heuristic
    info-level reminder that backticked workspace paths
    drift. The precise check ships in v0.7 as the
    `markdown_paths_resolve` rule kind.

- **`--format=agent` JSON output** — eighth output format,
  alongside `human` / `json` / `sarif` / `github` /
  `markdown` / `junit` / `gitlab`. Shaped for AI coding
  agents that consume alint inside their own self-correction
  loops. Differences vs. `--format=json`: violations are a
  flat list (no per-rule nesting); each violation carries an
  `agent_instruction` field with templated remediation
  phrasing (severity + human message + location + fix
  availability + policy URL); `severity` is the lowercase
  string (`"error"` / `"warning"` / `"info"`). Aliases:
  `--format=agent` / `--format=agentic` / `--format=ai`.
  Stable behind `schema_version: 1`. The fix-report falls
  back to the human formatter (an agent confirming a fix
  landed re-runs `alint check --format=agent`).

### Internal

- Two new tests in `crates/alint-dsl/src/bundled.rs` continue
  to enforce that every shipped ruleset declares its
  canonical `# alint://bundled/<name>@v<rev>` URI tag and
  parses as a valid config; the new rulesets are exercised
  by the existing test loop.
- Five unit tests for the agent formatter cover empty
  reports, path-bound violations, fixable violations
  (suggesting `alint fix --only <id>` in
  `agent_instruction`), cross-file violations
  (repository-level phrasing), and severity-count
  aggregation.
- Help-text snapshot (`crates/alint/tests/cli/help-top-level.stdout`)
  refreshed to mention `agent` in the `--color` flag's
  documented list of plain-bytes formats.

## [0.5.12] — 2026-04-27

Maintenance release. Verifies the npm auto-publish CI wiring
end-to-end after v0.5.11's `publish-npm` job failed (the
`NPM_TOKEN` secret hadn't been provisioned yet, and a detour
through Trusted Publishing was blocked by a broken 2FA
configuration UI on npmjs.com).

No code changes. Every v0.5.11 config runs unchanged.

### Changed

- The npm scope is now `@asamarts/alint` (matches the
  `asamarts/alint` GitHub repo, `asamarts/homebrew-alint`
  tap, and `ghcr.io/asamarts/alint` Docker image). The
  v0.5.11 entry referenced `@alint/alint`, then `@a-lint/alint`
  during the org-name dance; both were placeholders. The
  install snippet now matches what's actually published.

```bash
npm install --save-dev @asamarts/alint
npx alint check
```

## [0.5.11] — 2026-04-27

npm install channel. Closes the v0.5 milestone — every
deferred item from the v0.5 roadmap is now shipped.

### Added

- **`@asamarts/alint` npm package** — fifth install channel
  alongside `cargo install alint`, the Homebrew tap, the
  Docker image, and `install.sh`. The npm package is a thin
  shim that downloads the matching pre-built binary at
  install time, verifies its SHA-256 against the same
  `.sha256` companions the other paths consume, and stages
  it under `bin-platform/` for the npm-exposed
  `bin/alint.js` shim to spawn at runtime.

  ```bash
  # project-local
  npm install --save-dev @asamarts/alint
  npx alint check

  # global
  npm install -g @asamarts/alint
  alint check
  ```

  - The package itself ships zero JS runtime behaviour.
  - Single runtime dep (`tar` for archive extraction).
  - Skip the postinstall network hop with
    `ALINT_SKIP_INSTALL=1` (for CI systems that snapshot
    `node_modules`).
  - Supported platforms: linux x64/arm64 (musl), macOS
    x64/arm64, Windows x64.
  - Auto-published from `release.yml` on tag push,
    alongside crates.io / Docker / Homebrew. The publish
    job stamps `package.json`'s version to match the tag
    immediately before `npm publish --access public`.

### Internal

- New `npm/` directory at the repo root holds
  `package.json`, `install.js` (postinstall), `bin/alint.js`
  (runtime shim), `README.md`, and `.npmignore`.
- `release.yml` gains a `publish-npm` job: `needs: release`
  (the GH Release must be live before any user's
  postinstall can fetch the binary tarballs); reads
  `NPM_TOKEN` secret from repo settings.

## [0.5.10] — 2026-04-27

DSL ergonomics: three composition primitives that close
common monorepo / ops pain points. Schema-compatible: every
v0.5.9 config runs unchanged.

### Added

- **`content_from: <path>` on fix ops** —
  `file_create` / `file_prepend` / `file_append` accept a
  path-relative-to-lint-root as an alternative to inline
  `content:`. The two are mutually exclusive (XOR
  enforced at config-load time). Read at fix-apply time
  via the new `ContentSourceSpec` enum on the fixer
  struct; missing source produces a `Skipped` outcome
  with a clear message rather than aborting the run.
  Use case: LICENSE / NOTICE / CONTRIBUTING /
  SPDX-header boilerplate that's awkward to inline lives
  in `.alint/templates/` and gets referenced by short
  relative path.

- **`.alint.d/*.yml` drop-ins** — auto-discovered next to
  the top-level `.alint.yml` and merged in alphabetical
  order. The last drop-in wins on field-level conflict,
  mirroring the `/etc/*.d/` convention. Stage `00-base.yml`
  for ops defaults, `50-team.yml` for team policies,
  `99-local.yml` for developer-local tweaks. Trust-
  equivalent to the main config (same workspace) — drop-
  ins CAN declare `custom:` facts and `kind: command`
  rules without the trust-gate that protects HTTPS /
  bundled extends. Non-yaml files in the dir are
  silently skipped. Sub-extended configs don't get their
  own `.alint.d/`; only the top-level config does.

- **Rule templates / parameterized rules** — top-level
  `templates:` block defines reusable rule bodies; rules
  instantiate them via `extends_template: <id>` and a
  `vars:` map for the `{{vars.<name>}}` substitution.
  Recursive substitution walks lists and nested mappings,
  so `paths:` / `fix.file_create.content` / etc all get
  vars-expanded. Unknown placeholders preserve literally
  so typos surface. Leaf-only (a template can't itself
  `extends_template:` another, mirroring the bundled
  rulesets restriction). Templates merge through
  `extends:` chains by id.

  ```yaml
  templates:
    - id: dir-has-readme
      kind: file_exists
      paths: ["{{vars.dir}}/README.md"]
      level: warning
      message: "{{vars.dir}} is missing a README"
  rules:
    - extends_template: dir-has-readme
      id: packages-have-readme
      vars: { dir: packages }
    - extends_template: dir-has-readme
      id: services-have-readme
      vars: { dir: services }
  ```

### Internal

- New `ContentSourceSpec` (`Inline(String)` /
  `File(PathBuf)`) and `resolve_content_source` helper in
  `alint-core::config`, exported through the crate root.
  `From<String>` / `From<&str>` impls keep inline-string
  construction terse for tests.
- `RawConfig` gains `templates: Vec<Mapping>`; `merge()`
  merges templates by id; `finalize()` runs the
  `expand_template` pass before each rule deserializes
  into `RuleSpec`.
- New `expand_template` + `substitute_template_vars` /
  `_value` helpers in `alint-dsl` reuse the existing
  `alint_core::template::render_message` engine for the
  `{{namespace.key}}` substitution layer.
- JSON Schema (`schemas/v1/config.json` + the in-crate
  copy) defines top-level `templates: []`, the new
  `rule_template_instance` shape (`oneOf`-branch with
  the kind-driven shape), and the `oneOf` between
  `content` / `content_from` on each affected fix op.
  Drift test passes.
- 12 new unit tests across the three features (3 fixer
  tests for content_from, 3 lib tests for drop-in
  collection + merge, 6 lib tests for template
  expansion).

## [0.5.9] — 2026-04-27

`json_schema_passes` (last unshipped structured-query
primitive), two new git-aware rule kinds, and four
OpenSSF Scorecard-overlap additions to `oss-baseline@v1`.
Schema-compatible: every v0.5.8 config runs unchanged.

### Added

- **`json_schema_passes`** — validate JSON / YAML / TOML
  files against a JSON Schema. Targets coerce through
  serde into the same `serde_json::Value` tree the schema
  sees, so YAML configs (Kubernetes manifests, GitHub
  Actions workflows, Helm `values.schema.json`) and TOML
  manifests (Cargo, pyproject) all work against a JSON
  schema document. Schema is loaded + compiled lazily on
  the first `evaluate()` call and cached on the rule via
  `OnceLock`. Each schema-validation error becomes one
  violation with the failing instance path; a target that
  fails to parse produces a single parse-error violation,
  not a flood. Format is detected from extension; pass
  `format:` to override.

- **`git_no_denied_paths`** — fire when any tracked file
  matches a configured glob denylist. The absence-axis
  companion of `git_tracked_only` (v0.4.8). Catches
  secrets (`*.env`, `id_rsa`, `*.pem`), bulky generated
  artefacts (`dist/**`, `*.log`), and "do not commit"
  sentinels in one rule rather than one `file_absent`
  per pattern. Reports every matching denylist entry per
  offending path. Outside a git repo, silently no-ops.

- **`git_commit_message`** — validate HEAD's commit
  message shape via regex (`pattern:`), max subject
  length (`subject_max_length:`), and body-required
  (`requires_body:`). At least one of the three must be
  set. Subject length counts characters, not bytes.
  Outside a git repo, with no commits, or when `git`
  isn't on PATH, silently no-ops. Pairs naturally with
  `alint check --changed` for per-PR enforcement.

- **`alint-core::git::head_commit_message(root)`** —
  new helper alongside `collect_tracked_paths` /
  `collect_changed_paths`, with the same advisory
  `Option<String>` return shape.

- **Four Scorecard-overlap rules in `oss-baseline@v1`**
  (info-level, no new rule kinds — composes from
  existing `file_exists` + `file_min_size`):
  - `oss-security-policy-non-empty` — 200B floor on
    SECURITY.md (catches the empty stub that satisfies
    Scorecard's existence check while providing no
    reporting guidance).
  - `oss-dependency-update-tool` — `file_exists`
    against every blessed Dependabot / Renovate config
    location.
  - `oss-codeowners-exists` — CODEOWNERS at root,
    `.github/`, or `docs/`.
  - `oss-codeowners-non-empty` — 10B floor on
    CODEOWNERS.

### Internal

- `alint-rules` gains `jsonschema = "0.29"` as a
  regular dep (already a workspace dep used by
  `alint-dsl`'s drift tests).
- JSON Schema (`schemas/v1/config.json` + the in-crate
  copy) defines `rule_json_schema_passes`,
  `rule_git_no_denied_paths`, and
  `rule_git_commit_message`. Drift test passes.
- 21 new unit tests across the three rule files (9 for
  `json_schema_passes`, 5 for `git_no_denied_paths`, 7
  for `git_commit_message`); 1 new e2e fixture
  (`oss_baseline_complete_repo_pass` updated for the
  four new rules); two existing override scenarios
  updated to account for the new bundled rules.

## [0.5.8] — 2026-04-26

Three new output formats. Brings the count to seven and
closes the v0.5 output-format roadmap item. Schema-compatible:
every v0.5.7 config runs unchanged.

### Added

- **`--format markdown`** (alias `md`) — GitHub-Flavored
  Markdown suited to PR comments, mkdocs report pages, and
  Slack via webhook bridges. H1 banner + one-line summary
  (`**N violations across M files** (E errors, W warnings)`)
  + one H2 section per file with bulleted violations + a
  trailing "Cross-file" section for path-less / cross-file
  violations. Output is byte-deterministic so PR-comment
  workflows can diff alint output across runs without
  spurious churn. `alint fix --format markdown` gets a
  dedicated renderer too — lists each rule's items with
  `applied` / `skipped` / `unfixable` status.

- **`--format junit`** (alias `junit-xml`) — the de-facto-
  standard CI test-report XML consumed by Jenkins, Azure
  DevOps, GitHub's `dorny/test-reporter`, and GitLab CI's
  JUnit integration. Common-denominator schema: a single
  `<testsuites>` wrapping a single `<testsuite name="alint">`,
  with one `<testcase>` per (rule, file/path-less-bucket).
  Passing rules contribute self-closed testcases; each
  violation becomes a testcase with a `<failure>` whose
  `type` attribute carries the alint level (`error` /
  `warning` / `info`) so consumers can filter
  level-specifically. XML 1.0-illegal control characters
  are stripped on the way out.

- **`--format gitlab`** (aliases `gitlab-codequality`,
  `code-quality`) — GitLab CI's native Code Quality JSON,
  which is the upstream Code Climate "Issue" specification.
  One issue object per violation:
  `{ description, check_name, fingerprint, severity,
  location: { path, lines: { begin } } }`. Severity
  mapping: `Error → major`, `Warning → minor`,
  `Info → info`. Fingerprint is the SHA-256 hex of
  `rule_id|path|message` — the line number is intentionally
  omitted so a violation that drifts up or down by a few
  lines stays the same issue across runs. Path-less /
  cross-file violations emit `location.path = "."`
  (repository root) so the report still validates against
  the GitLab schema.

### Internal

- `alint-output` gains a `sha2` dep for the GitLab
  fingerprint (already a workspace dep used by
  `alint-rules` + `alint-dsl`).
- 38 new unit tests across the three formatters cover empty
  reports, level-mapping, cross-file edge cases,
  determinism, special-character escaping, and (for
  GitLab) fingerprint stability across line-drift +
  sensitivity to message changes.

## [0.5.7] — 2026-04-26

Competitive bench publication. The v0.5.6 harness becomes a
multi-tool driver: alint, ls-lint, Repolinter, and `find` +
`ripgrep` pipelines all run against the same synthetic
trees on the same hardware, producing wall-time numbers
that are directly comparable. Reproducibility via a new
`ghcr.io/asamarts/alint-bench` Docker image (every
competitor pinned by version) and an `xtask bench-scale
--docker` flag that re-execs the bench inside the image.
Schema-compatible; every v0.5.6 config runs unchanged.

### Added

- **`xtask bench-scale --tools <list>`** — the bench
  harness now runs an arbitrary set of tools across the
  same `(size × scenario × mode)` matrix v0.5.6
  introduced. Default `alint` (preserves the v0.5.6
  publication shape), `all` expands to every known tool,
  comma lists pick a subset (`alint,grep`,
  `alint,ls-lint`). Tools missing on `PATH` are
  log-and-dropped at resolve time so a partial
  installation still produces alint-only rows without
  aborting.
  - **alint** — full matrix (every scenario × mode).
  - **ls-lint** — gated to (S1, full); ls-lint is
    extension/case-class only and has no
    `--changed`-equivalent.
  - **Repolinter** — gated to (S2, full); pinned to
    0.11.2 (last pre-archive release, repo archived
    2026-02-06). The size-bound check (no files >10 MiB)
    is dropped: Repolinter has no built-in primitive
    and emulating via `script` rules would distort
    timings beyond recognition.
  - **find + ripgrep pipeline** (`grep`) — gated to
    (S1, full) and (S2, full); shell-pipeline baseline
    representing the small-team "we just chain `find`
    and `rg`" status quo. S3's cross-file rules have no
    sane shell expression and are out of scope.

- **`bench/Dockerfile` + `ghcr.io/asamarts/alint-bench`
  image** — the canonical competitive-bench environment.
  Built and pushed by `.github/workflows/bench-docker.yml`
  on tag pushes (`:<ver>` + `:latest`), main pushes
  (`:edge`), and manual dispatch. Pinned versions of
  hyperfine, ripgrep, repolinter, ls-lint, Node 20, and
  the Rust toolchain so a given image tag IS the bench
  environment for that release.

- **`xtask bench-scale --docker`** — re-execs the bench
  inside the published image. Bind-mounts the workspace
  at `/work`, uses a named volume for the cargo target
  dir so target/ artefacts persist across runs without
  shadowing the host. Image override via
  `ALINT_BENCH_IMAGE=...`.

- **First competitive numbers** under
  `docs/benchmarks/macro/results/linux-x86_64/v0.5.7/`. Same
  fingerprint as v0.5.6's alint-only publication; rows
  for ls-lint / repolinter / grep added at the
  scenarios + sizes each tool supports. Headline
  ratios (linux-x86_64, 100k files):
  - **S1 (filename hygiene)**: alint vs ls-lint vs
    `find | grep` pipelines.
  - **S2 (existence + content)**: alint vs Repolinter
    vs `find` + `rg` pipelines.
  - **S3 (workspace bundle)**: alint only — no
    competitor models cross-file rules.

  Per-row markdown plus a `results.json` with the full
  matrix; the new `Tool` column makes pivoting trivial.

- **Published 1M-file numbers** under
  `docs/benchmarks/macro/results/linux-x86_64/v0.5.6/1m/results.md`.
  Six rows (`1m × {S1,S2,S3} × {full,changed}`) on the
  same hardware as v0.5.6's 1k/10k/100k publication.
  Headlines: 1m / S1 / full ≈ 3.5s, 1m / S2 / full ≈ 10s,
  1m / S3 / full ≈ 9.5min — the cross-file rules in the
  workspace bundle scale superlinearly with N, exactly as
  the methodology predicted. `--changed` saves ~34% on
  S2's content rules at 1m but barely helps S3 (cross-file
  rules can't be filtered).

- **Auto-reduced sampling at 1m**. The harness caps
  `1m`-row warmup at 1 and measured runs at 3 regardless
  of `--warmup` / `--runs`. A single `1m / S3` invocation
  runs for several minutes; thirteen of them per row would
  push the matrix to many hours. The trade-off is wider
  stddev at 1m — `methodology.md` is updated to flag this
  so readers don't compare 1m's stddev to the smaller-size
  rows like-for-like. Stddev is reported as `0.0` when
  hyperfine emits `null` (single-run rows) instead of
  failing the whole bench.

### Fixed

- **`--include-1m` now actually adds the 1m size** to the
  matrix when `--sizes` is at its default (1k,10k,100k).
  Previously the flag only filtered 1m out unless you
  also retyped the size list — the opposite of what the
  help text implied.

- **Tool-version fingerprint stays one line**. Multi-line
  `--version` banners (notably ripgrep's) were being
  stored verbatim in the fingerprint's `tool_versions`
  map, blowing up the rendered "**Tools:** ..." line in
  committed reports. Capture now keeps just the first
  line of each tool's `--version` output.

## [0.5.6] — 2026-04-26

Scale-ceiling bench publication + a latent walker bug fix
that the bench surfaced. New `xtask bench-scale` subcommand
runs alint across a (size × scenario × mode) matrix with
hardware-fingerprint capture and JSON + Markdown
publication; v0.5.7 layers competitive comparisons
(ls-lint, Repolinter, find/grep) on top of the same
infrastructure. Schema-compatible; every v0.5.5 config
runs unchanged.

### Added

- **`xtask bench-scale`** — scale-ceiling benchmark
  driver. Runs alint across:
  - **Sizes**: `1k` / `10k` / `100k` (default), opt into
    `1m` via `--include-1m`. Synthetic monorepo trees
    generated deterministically from the seed
    (default `0xa11e47`).
  - **Scenarios**: `S1` (filename hygiene, 8 rules) /
    `S2` (existence + content, 8 rules) / `S3` (workspace
    bundle: `oss-baseline` + `rust` + `monorepo` +
    `monorepo/cargo-workspace`).
  - **Modes**: `full` (every file evaluated) and
    `changed` (`alint check --changed` against a
    deterministic 10% diff — measures the v0.5.0
    incremental path).

  Per-row hyperfine measurement (3 warmup + 10 measured
  runs by default); JSON + Markdown output under
  `docs/benchmarks/macro/results/<os>-<arch>/<version>/`. Hardware
  fingerprint (CPU model + cores, RAM, FS type, kernel,
  rustc, alint version + git SHA, hyperfine version)
  embedded in every report so cross-machine comparisons
  stay honest.

- **`alint_bench::tree::generate_monorepo`** — new
  Cargo-workspace-shaped synthetic-tree generator with
  real workspace `[workspace]` + per-package
  `[package].name` Cargo.toml content (so the
  `monorepo/cargo-workspace@v1` ruleset's structured-query
  rules see well-formed manifests). Full determinism for
  byte-identical trees across platforms.

- **`alint_bench::tree::select_subset`** — deterministic
  Fisher-Yates partial shuffle for picking a fraction of
  files to "touch" in `--changed`-mode benches.

- **First published numbers**:
  `docs/benchmarks/macro/results/linux-x86_64/v0.5.7/` with 18 rows
  (3 sizes × 3 scenarios × 2 modes) on AMD Ryzen 9 3900X /
  62 GB / ext4 / Linux 6.1. Companion
  `docs/benchmarks/{README.md,RUNNING.md,macro/README.md} (post-reorg layout)`
  documents the harness + scenario definitions + how to
  reproduce.

- **CLI flags**: `--sizes`, `--scenarios`, `--modes`,
  `--warmup`, `--runs`, `--seed`, `--diff-pct`, `--out`,
  `--include-1m`, `--quick`, `--json-only`. The default
  (`cargo xtask bench-scale` with no args) produces the
  full publication-grade matrix.

### Fixed

- **Walker no longer descends into `.git/`.** `alint
  check` against a tree containing a `.git/` directory
  used to walk into git's internal storage — wasted work
  for every alint rule (none of them target
  `.git/objects/*`) and a TOCTOU hazard during git's
  auto-gc / packfile rewrites. The walker now adds
  `.git` to its exclusion overrides unconditionally.
  No user-visible behaviour change for repos whose
  `.gitignore` already covers `.git/`-shaped paths;
  benchmark and large-monorepo runs become both faster
  and reliable.

### Internal

- New `xtask/src/bench/` module: `mod.rs` (orchestration
  + types), `fingerprint.rs` (hardware capture per OS),
  `scenarios/*.yml` (S1/S2/S3 alint configs embedded via
  `include_str!`).
- `xtask` gains `serde` (with `derive`) and `serde_json`
  dev-deps for hyperfine `--export-json` parsing and the
  results.json schema.
- 11 new unit tests on `alint_bench::tree` covering the
  monorepo shape, file-count exactness, deterministic
  output for the same seed, and `select_subset`'s
  fraction / clamping / determinism semantics.

### Compatibility

- Schema version remains `1`. No rule-config changes.
- Public API additions are non-breaking. Walker
  `.git/`-exclusion is a behaviour fix, not a config
  change.

## [0.5.5] — 2026-04-26

Two license-compliance bundled rulesets — the v0.5 cycle's
expansion beyond the workspace-tier monorepo audience to
OSS maintainers and corporate-policy teams.
Schema-compatible; every v0.5.4 config runs unchanged.

### Added

- **`alint://bundled/compliance/reuse@v1`** — FSFE
  [REUSE Specification](https://reuse.software/)
  compliance. Three rules covering the spec's two
  load-bearing requirements:
  - `reuse-licenses-dir-exists` — top-level `LICENSES/`
    directory present (per
    [§ License files](https://reuse.software/spec/#license-files)).
  - `reuse-source-has-spdx-identifier` — every source
    file carries an `SPDX-License-Identifier:` header in
    its first ~10 lines.
  - `reuse-source-has-copyright-text` — every source
    file carries an `SPDX-FileCopyrightText:` header.

  Source-file rules cover the common code extensions
  (`*.{rs,py,js,jsx,ts,tsx,go,java,kt,c,cc,cpp,h,hpp,
  hh,sh,rb,swift}`) and exclude vendored / build /
  dist directories. Projects that license files via
  `.license` companions or `REUSE.toml` mappings can
  narrow `paths:` on the source rules.

  ```yaml
  extends:
    - alint://bundled/compliance/reuse@v1
  ```

- **`alint://bundled/compliance/apache-2@v1`** —
  compliance for projects distributed under the Apache
  License, Version 2.0. Three rules verifying the
  artefacts the license text itself requires of
  redistributors:
  - `apache-2-license-text-present` — LICENSE (or
    LICENSE.md / LICENSE.txt / COPYING) contains the
    canonical "Apache License, Version 2.0" text.
  - `apache-2-notice-file-exists` — root NOTICE file
    present (per Apache-2.0 §4(d)).
  - `apache-2-source-has-license-header` — every source
    file carries the canonical "Licensed under the
    Apache License, Version 2.0" header in its first
    ~25 lines.

  Substring-matches the canonical license title rather
  than doing full bit-for-bit comparison, so SPDX
  templates, apache.org's template, and GitHub's
  auto-init all parse as compliant. Dual-licensed
  projects (e.g. Apache-2.0 OR MIT) can extend this
  ruleset and use `level: off` on rules they don't want
  firing strictly.

  Bundled catalog: 15 → 17.

### Internal

- New ruleset directory
  `crates/alint-dsl/rulesets/v1/compliance/` with two
  `.yml` files; registered in
  `alint_dsl::bundled::REGISTRY`. Neither ruleset uses a
  fact gate — adopting a compliance ruleset is the
  user's signal that the project intends to be
  compliant with the named scheme.
- 6 new e2e scenarios under
  `crates/alint-e2e/scenarios/check/bundled-compliance/`:
  per ruleset, happy-path + missing-core-artefact +
  missing-source-header.

### Compatibility

- Schema version remains `1`. Pure config — no new rule
  kinds, no new core APIs.
- JSON / SARIF / GitHub outputs byte-equivalent for
  configs that don't extend the new rulesets.

## [0.5.4] — 2026-04-26

`alint init` — the missing one-line adoption story.
Detects the repo's ecosystem (Rust / Node / Python / Go /
Java) and optionally its workspace shape (Cargo / pnpm /
Yarn-or-npm), then writes a `.alint.yml` extending the
right bundled rulesets. Closes the v0.5 monorepo theme on
the adoption side: every primitive shipped in v0.5.0–v0.5.3
now has a one-line on-ramp. Schema-compatible; every v0.5.3
config runs unchanged.

### Added

- **`alint init [PATH]`** — new subcommand. Detects the
  repo's ecosystem from root manifests and writes a
  `.alint.yml` with `extends:` lines for
  `oss-baseline@v1` plus each detected language ruleset.
  Detection is deliberately a presence check (file
  exists, no parsing) so it stays fast and predictable:
  - Rust: `Cargo.toml`
  - Node: `package.json`
  - Python: `pyproject.toml` / `setup.py` / `setup.cfg`
  - Go: `go.mod`
  - Java: `pom.xml` / `build.gradle` / `build.gradle.kts`

  Refuses to overwrite an existing config (any of
  `.alint.yml` / `.alint.yaml` / `alint.yml` /
  `alint.yaml`) — exits non-zero with a clear message
  pointing the user at deletion.

- **`alint init --monorepo`** — adds workspace detection
  on top of the language scan. Recognises:
  - **Cargo workspaces** — root `Cargo.toml` contains a
    `[workspace]` table (line-prefix check, no TOML
    parsing).
  - **pnpm workspaces** — root `pnpm-workspace.yaml` /
    `.yml` exists.
  - **Yarn / npm workspaces** — root `package.json`
    contains `"workspaces"`.

  When a workspace is detected, the generated config also
  extends `monorepo@v1` and the matching
  `monorepo/<flavor>-workspace@v1` overlay, plus sets
  `nested_configs: true` so each subdirectory can layer
  its own `.alint.yml` on top.

  ```yaml
  # `alint init --monorepo` in a Cargo workspace produces:
  version: 1
  nested_configs: true

  extends:
    - alint://bundled/oss-baseline@v1
    - alint://bundled/rust@v1
    - alint://bundled/monorepo@v1
    - alint://bundled/monorepo/cargo-workspace@v1
  ```

  Bazel / Lerna / Nx / Turbo detection deferred — the
  three flavours that have bundled overlays cover the
  workspace-tier sweet spot.

### Internal

- New `crates/alint/src/init.rs` module with `Detection` /
  `Language` / `WorkspaceFlavor` types, a pure `detect()`
  function and a deterministic `render()` emitter. Output
  is hand-formatted YAML (not serialized via serde) so the
  generated file can carry header comments documenting
  what was detected and how to use it.
- 17 new unit tests covering the detector + emitter
  (per-language detection, polyglot repos, workspace
  precedence, header summary).
- 3 new trycmd CLI tests under `tests/cli/init-*.toml`:
  empty-repo init, monorepo-cargo init, refuses-overwrite.
  The trycmd `fs.sandbox = true` mode lets us assert on
  the post-run sandbox state (the generated `.alint.yml`)
  alongside stdout / stderr / exit.
- `help-top-level` snapshot regenerated to include the
  `init` subcommand line.

### Compatibility

- Schema version remains `1`. Pure additive — no rule,
  config, or output changes.
- Public API unchanged (the `init` module is private to
  the binary crate).
- New `tempfile` dev-dependency on `alint` (already a
  workspace dep elsewhere; the binary needs it for the
  init unit tests).

## [0.5.3] — 2026-04-26

Three workspace-aware bundled rulesets layered on top of
`monorepo@v1`. Each is gated by a workspace-flavor fact and
uses the v0.5.2 `when_iter:` filter to scope per-member
checks to actual package directories — `crates/notes/`
(no `Cargo.toml`) or `packages/drafts/` (no `package.json`)
are filtered out without firing false positives.
Schema-compatible; every v0.5.2 config runs unchanged.

### Added

- **`alint://bundled/monorepo/cargo-workspace@v1`** —
  Cargo workspaces. Gated by `facts.is_cargo_workspace`
  (root `Cargo.toml` declares `[workspace]`). Three rules:
  `members = [...]` declared at the workspace root
  (`toml_path_matches`); every `crates/*` directory with
  its own `Cargo.toml` has a README; every member's
  `Cargo.toml` declares `[package].name`.

  ```yaml
  extends:
    - alint://bundled/monorepo@v1
    - alint://bundled/rust@v1
    - alint://bundled/monorepo/cargo-workspace@v1
  ```

- **`alint://bundled/monorepo/pnpm-workspace@v1`** —
  pnpm workspaces. Gated by `facts.is_pnpm_workspace`
  (root `pnpm-workspace.yaml` / `.yml` exists). Three
  rules: `packages: [...]` declared in
  `pnpm-workspace.yaml` (`yaml_path_matches`); every
  `packages/*` with a `package.json` has a README; every
  member's `package.json` declares `name`.

- **`alint://bundled/monorepo/yarn-workspace@v1`** —
  Yarn / npm workspaces (the workspace declaration lives
  in the root `package.json` for both). Gated by
  `facts.is_yarn_workspace` (root `package.json` contains
  `"workspaces"`). Three rules: `workspaces: [...]` is
  non-empty (`json_path_matches` against
  `$.workspaces[*]`); every `{packages,apps}/*` with a
  `package.json` has a README; every member's
  `package.json` declares `name`. Validates the array
  form; the rarer object form
  (`"workspaces": {"packages": [...]}`) is gated by the
  fact but not field-validated here.

  Bundled catalog: 12 → 15 rulesets.

### Internal

- New ruleset directory
  `crates/alint-dsl/rulesets/v1/monorepo/` with three
  `.yml` files; registered in `alint_dsl::bundled::REGISTRY`.
  Each ruleset declares its own `is_*_workspace` fact
  inline rather than promoting them to the core `facts:`
  catalogue — the facts are workspace-specific and not
  meant to be referenced from user configs.
- 7 new e2e scenarios under
  `crates/alint-e2e/scenarios/check/bundled-monorepo/`:
  per-flavor "filters non-member dirs" + "silent outside
  workspace" pair, plus `cargo-workspace`'s "fires on
  missing members" case.

### Compatibility

- Schema version remains `1`. All three rulesets are
  pure config — no new rule kinds, no new core APIs.
- JSON / SARIF / GitHub outputs byte-equivalent for
  configs that don't extend the new rulesets.

## [0.5.2] — 2026-04-26

Per-iteration `when:` filter on iterating rules — closes the
second monorepo-scale gap from the v0.5 roadmap. Combined
with `--changed` (v0.5.0) and `command` plugin (v0.5.1),
this is the third leg of the v0.5 monorepo theme.
Schema-compatible; every v0.5.1 config runs unchanged.

### Added

- **`when_iter:`** field on `for_each_dir`, `for_each_file`,
  and `every_matching_has`. Optional expression evaluated
  against each iterated entry's `iter` context; iterations
  whose verdict is false are skipped before any nested rule
  is built. Closes the Bazel/Cargo/pnpm-workspace gap where
  users previously had to widen `select:` and rely on inner
  rules to short-circuit.

  ```yaml
  - id: workspace-member-has-readme
    kind: for_each_dir
    select: "crates/*"
    when_iter: 'iter.has_file("Cargo.toml")'
    require:
      - kind: file_exists
        paths: "{path}/README.md"
    level: error
  ```

  Without `when_iter:`, `crates/notes/` (no `Cargo.toml`)
  would have fired the missing-README rule. With it, only
  workspace members are evaluated.

- **`iter.*` namespace in `when:` expressions** — exposes
  the iterated entry's metadata to the existing `when:`
  grammar. Same expression compiles in `when_iter:` (outer
  iteration filter) and in any nested rule's `when:`
  (per-iteration nested gate). Outside an iteration
  context, `iter.X` resolves to `null` and
  `iter.has_file(_)` to `false`, matching the
  "missing fact is falsy" convention.

  | Reference | Type | Notes |
  |---|---|---|
  | `iter.path` | string | Relative path of the iterated entry. |
  | `iter.basename` | string | Basename. |
  | `iter.parent_name` | string | Parent dir name. |
  | `iter.stem` | string | Basename minus final extension. |
  | `iter.ext` | string | Final extension without the dot. |
  | `iter.is_dir` | bool | `true` for `for_each_dir`, `false` for `for_each_file`. |
  | `iter.has_file(pattern)` | bool | Glob match relative to the iterated directory. Always `false` on file iteration. |

- **Function-call syntax in the `when:` grammar.** Limited
  to a fixed allow-list of methods on `iter` (currently just
  `has_file`); typos in user configs surface as
  "unknown iter method" parse errors instead of silently
  coercing to `false`. Calls on non-iter namespaces are a
  parse error.

### Internal

- New public types `IterEnv` and `WhenEnv::with_iter()` in
  `alint-core::when`. New `WhenExpr::Call` AST variant.
  `WhenEnv::new()` constructor for callers without
  iteration context.
- Shared parser helper `for_each_dir::parse_when_iter`
  reused by `for_each_file` and `every_matching_has`.
- 9 new unit tests in `alint-core::when` covering the iter
  namespace + function-call grammar + outside-iter
  fallback. 4 new e2e scenarios under
  `crates/alint-e2e/scenarios/check/when_iter/`: marker-file
  filter, basename predicate, recursive-glob predicate,
  composition with `facts.*`.

### Compatibility

- Schema version remains `1`. `when_iter:` is opt-in; rules
  that don't use it behave identically to v0.5.1.
- Public API additions are non-breaking. `WhenEnv` gains an
  `iter: Option<IterEnv>` field; the new `WhenEnv::new()`
  constructor and existing struct-literal syntax both work.
  Out-of-tree code constructing `WhenEnv { facts, vars }`
  (without explicit `iter`) needs to add `iter: None` (or
  switch to `WhenEnv::new(facts, vars)`).
- `evaluate_for_each` (an `alint-rules` crate-private
  helper) gained a `when_iter` parameter — only matters if
  you've forked the crate.

## [0.5.1] — 2026-04-26

Plugin tier 1: `command` rule kind. Wraps any CLI on `PATH`
into alint's report. Continues the v0.5 monorepo-scale theme
— pairs naturally with `--changed` so per-file external
checks (actionlint, shellcheck, kubeconform, …) become
incremental in CI. Schema-compatible; every v0.5.0 config
runs unchanged.

### Added

- **`kind: command`** — per-file rule that spawns argv with
  path-template substitution (`{path}`, `{dir}`, `{stem}`,
  `{ext}`, `{basename}`, `{parent_name}`). Exit `0` is a
  pass; non-zero produces a violation whose message is the
  truncated stdout+stderr. Working dir is the repo root;
  stdin is closed (`/dev/null`). Output is capped at 16 KiB
  per stream to keep reports legible.

  ```yaml
  - id: workflows-clean
    kind: command
    paths: ".github/workflows/*.{yml,yaml}"
    command: ["actionlint", "{path}"]
    level: error
  ```

  Environment threaded into each invocation: `ALINT_PATH`
  (relative to root), `ALINT_ROOT` (absolute), `ALINT_RULE_ID`,
  `ALINT_LEVEL`, plus `ALINT_VAR_<NAME>` per top-level
  `vars:` entry and `ALINT_FACT_<NAME>` per resolved fact.

- **`timeout: <seconds>`** option on `command` rules. Default
  30s. Past the limit, the child process is killed and a
  violation reports the timeout. Bounds runaway tools so a
  hung child never stalls the whole run.

- **`--changed` interaction.** `command` is per-file (no
  `requires_full_index` override), so it inherits the v0.5
  filtered-index iteration: `alint check --changed` spawns
  the wrapped tool only for files in the diff. A
  `shellcheck` rule on a 200-script repo invokes
  `shellcheck` zero times when the diff doesn't touch any
  `.sh`. Largest practical multiplier on CI cost for
  external-linter wrappers.

### Security

- **Trust gate.** `command` rules are only permitted in the
  user's own top-level `.alint.yml`. A `kind: command` rule
  introduced via `extends:` — local file, HTTPS URL, or
  `alint://bundled/<name>@<rev>` — is rejected at load time
  with a clear error pointing at the offending source.
  Adopting a published ruleset must never gain it arbitrary
  process execution. New public function
  `alint_dsl::reject_command_rules_in` mirrors the existing
  `alint_core::facts::reject_custom_facts_in` gate.

### Internal

- New `alint_rules::command` module (~330 LOC including 9
  unit tests). Polling-based wait loop with 10ms granularity
  for the timeout path; output capping via
  `Read::take(OUTPUT_CAP_BYTES)`. JSON Schema gains a
  `rule_command` branch; root + in-crate copies kept
  byte-identical by the existing drift-guard test.

- 3 new e2e integration tests under
  `crates/alint-e2e/tests/command_plugin.rs` (`#[cfg(unix)]`
  — relies on `/bin/sh`): full-engine pass case, full-engine
  fail case (one violation per failing file), and the
  `--changed` interaction (only invoked for files in the
  diff). 2 new unit tests in `alint-dsl` covering the trust
  gate (rejected from `extends:`, allowed in top-level).

### Compatibility

- Schema version remains `1`. JSON / SARIF / GitHub outputs
  byte-equivalent for configs that don't use `command`.
- Public API additions are non-breaking. `Rule` trait
  unchanged; the new `reject_command_rules_in` is a new
  public function in `alint-dsl`.

## [0.5.0] — 2026-04-26

First v0.5 cut. Headline: incremental `alint check --changed`
mode for pre-commit and PR-check paths. Schema-compatible;
every v0.4.10 config runs unchanged. JSON / SARIF / GitHub
outputs byte-equivalent for full-tree runs.

### Added

- **`alint check --changed [--base=<ref>]`** and the same
  flags on `alint fix`. With `--base`, the changed-set is
  derived from `git diff --name-only --relative
  <base>...HEAD` (three-dot — merge-base diff, the right
  shape for PR checks). Without `--base`, it's
  `git ls-files --modified --others --exclude-standard`
  (working-tree diff, the right shape for pre-commit). The
  engine evaluates per-file rules against a [`FileIndex`]
  filtered to the changed-set, so a Java license-header rule
  scoped to `**/*.java` skips entirely when no `.java` file
  is in the diff. Cross-file rules (`pair`, `for_each_dir`,
  `every_matching_has`, `unique_by`, `dir_contains`,
  `dir_only_contains`) and existence rules (`file_exists`,
  `file_absent`, `dir_exists`, `dir_absent`) keep full-tree
  semantics for iteration; existence rules additionally skip
  when their `paths:` scope doesn't intersect the diff so an
  unchanged-but-missing LICENSE doesn't fire on every PR.
  Empty diffs short-circuit to an empty report (the
  no-op-commit case in pre-commit). Outside a git repo or
  when `git` isn't on PATH, `--changed` exits non-zero with
  a clear message rather than silently fall back to a full
  check.

  ```bash
  # Pre-commit: lint the working-tree diff.
  alint check --changed

  # PR check: lint everything that diverged from main.
  alint check --changed --base=main --format=sarif
  ```

- **`Rule::requires_full_index() -> bool`** and
  **`Rule::path_scope() -> Option<&Scope>`** on the public
  `alint-core::Rule` trait. Both default to "no opt-in", so
  out-of-tree rule implementations compile unchanged.
  Internal rules override on the eleven cases that need
  full-tree semantics: the six cross-file kinds plus the
  four existence kinds. Per-file rules need no override —
  the engine hands them the filtered index and their
  existing `Scope::matches` loops do the right thing.

- **`alint_core::git::collect_changed_paths(root, base)`**
  helper, parallel to the existing
  `collect_tracked_paths`. Returns the changed-set as a
  `HashSet<PathBuf>` of paths relative to `root`, or `None`
  outside a git repo / when `git` exits non-zero.

- **`Engine::with_changed_paths(set)`** builder method.
  Threads the changed-set through `Engine::run` and
  `Engine::fix`. Every call costs one walk over the
  index entries to build a filtered subset; absent the
  builder call, the engine behaves exactly as before.

- **`Step::CheckChanged`** in `alint-testkit`'s scenario
  harness. Five new e2e scenarios under
  `crates/alint-e2e/scenarios/check/changed/` cover:
  per-file rule skipped when scope misses the diff,
  per-file rule fires only on changed files, cross-file
  `pair` keeps full-tree semantics, existence rule skips
  when scope doesn't intersect, and the empty-diff
  short-circuit.

### Compatibility

- Schema version remains `1`. Every v0.4 config runs
  unchanged.
- Public API additions are non-breaking: `Rule` trait
  methods have defaults, `Engine::with_changed_paths` is
  additive. Embedders that hand-construct an `Engine` keep
  compiling.
- `alint-testkit::Step` gained a variant
  (`Step::CheckChanged`); embedders that exhaustively
  matched on `Step` need to add an arm for it.

## [0.4.10] — 2026-04-25

Three new content-family rule kinds rounding out the family.
Schema-compatible; every v0.4.9 config runs unchanged. JSON /
SARIF / GitHub outputs byte-equivalent.

### Added

- **`file_max_lines`** (alias `max_lines`). Mirror of
  `file_min_lines`: files in scope must have AT MOST
  `max_lines` lines. Same `wc -l` accounting. Catches the
  everything-module anti-pattern.
- **`file_footer`** (alias `footer`). Mirror of `file_header`
  anchored at the END of the file: the last `lines:` lines
  must match a regex. Use cases: license footers, signed-off-by
  trailers, generated-file sentinels. Fix op: `file_append`.
- **`file_shebang`** (alias `shebang`). First line of each
  file must match a regex. Pairs with `executable_has_shebang`
  (which checks shebang *presence*) — `file_shebang` checks
  shebang *shape*, e.g. `^#!/usr/bin/env bash$` to enforce a
  specific interpreter. Defaults to `^#!` (presence only).

  Brings the rule catalogue to ~55 kinds.

- 6 new e2e scenarios covering pass/fail paths for the three
  new kinds.

## [0.4.9] — 2026-04-25

Java bundled ruleset. Schema-compatible; every v0.4.8 config
runs unchanged. JSON / SARIF / GitHub outputs byte-equivalent.

### Added

- **`alint://bundled/java@v1`** (10 rules). Maven + Gradle
  hygiene, gated `when: facts.is_java`:
  - `java-manifest-exists` — `pom.xml`, `build.gradle`, or
    `build.gradle.kts` at the root (error).
  - `java-build-wrapper-committed` — `mvnw` / `gradlew` checked
    in for reproducible builds (info).
  - `java-no-tracked-target` / `java-no-tracked-build` —
    Maven's `target/` and Gradle's `build/` not committed.
    Both use `git_tracked_only: true` (the v0.4.8 primitive)
    so a developer's locally-built directories stay silent;
    only directories whose contents made it into git's index
    fire (error).
  - `java-no-class-files` — `*.class` files not committed
    (`git_tracked_only: true`, error).
  - `java-sources-pascal-case` — PascalCase filenames for
    `*.java`, with `package-info.java` / `module-info.java`
    excluded (warning).
  - `java-sources-final-newline` /
    `java-sources-no-trailing-whitespace` — text hygiene,
    auto-fixable (info).
  - `java-sources-no-bidi` / `java-sources-no-zero-width` —
    Trojan Source defenses (error).

  Brings the bundled catalogue to 12 rulesets. The
  `git_tracked_only` rules in this ruleset are the first
  bundled use of v0.4.8's git-aware primitive — the
  `silent_on_locally_built_target` e2e scenario proves the
  wiring end-to-end.



First git-aware primitive lands. Schema-compatible; every v0.4.7
config runs unchanged. JSON / SARIF / GitHub outputs gain no new
keys.

### Added

- **`git_tracked_only: bool`** option on `RuleSpec`, currently
  honoured by `file_exists`, `file_absent`, `dir_exists`, and
  `dir_absent`. When `true`, the rule's `paths`-matched entries
  are intersected with `git ls-files`'s output so only files /
  directories actually in git's index participate. Closes the
  approximation gap documented on the
  [walker-and-gitignore concept page](https://alint.org/docs/concepts/walker-and-gitignore/):
  a `dir_absent` rule on `**/target` with `git_tracked_only: true`
  fires only when `target/` was actually committed, never on a
  developer's locally-built `target/` (gitignored or not). Outside
  a git repo, or when `git` isn't on PATH, the tracked-set is
  empty and rules with the flag set become silent no-ops — the
  right default for "don't let X be committed" semantics.

  ```yaml
  - id: target-not-tracked
    kind: dir_absent
    paths: "**/target"
    git_tracked_only: true
    level: error
  ```

  Other rule kinds currently ignore the field; we'll extend
  coverage as concrete use cases come up. The roadmap'd
  `git_no_denied_paths` and `git_commit_message` primitives are
  still pending.

### Changed

- `alint-core::Context` gains a `git_tracked: Option<&HashSet<PathBuf>>`
  field, plus `is_git_tracked` / `dir_has_tracked_files` helpers.
  External embedders constructing a `Context` by hand need to add
  `git_tracked: None`. The engine collects the set at most once
  per `run` / `fix`, only when at least one rule's
  `wants_git_tracked()` is true — zero cost when no rule opts in.
- `alint-testkit`'s `Given` block accepts an optional `git: { init,
  add, commit }` block so e2e scenarios can stand up a real git
  repo in their tempdir before alint runs.



Distribution breadth. Schema-compatible; every v0.4.6 config
runs unchanged. JSON/SARIF/GitHub outputs byte-equivalent. No
Rust code changes — this release ships new install paths only.

### Added

#### Docker image

- **`ghcr.io/asamarts/alint`** — distroless multi-arch
  (`linux/amd64`, `linux/arm64`) image based on
  `gcr.io/distroless/static-debian12:nonroot`. Built by the
  release workflow from the same statically-linked musl
  binaries shipped in the GitHub Release tarballs, so the
  in-image binary matches the tarballed one byte-for-byte.
  Tags published per release: the exact git tag (`:v0.4.7`),
  the bare semver (`:0.4.7`), the `<major>.<minor>` channel
  (`:0.4`), and `:latest`.

  ```bash
  docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest
  ```

  Runs as the distroless `nonroot` user (UID 65532). For
  `alint fix` workflows that need to write with host
  ownership, pass `-u $(id -u):$(id -g)`.

#### Homebrew tap

- **`asamarts/alint`** — dedicated Homebrew tap at
  [asamarts/homebrew-alint](https://github.com/asamarts/homebrew-alint)
  shipping a `Formula/alint.rb` that resolves the right
  pre-built tarball for each platform (macOS arm64 + x86_64,
  Linuxbrew arm64 + x86_64) and verifies its SHA-256.

  ```bash
  brew tap asamarts/alint
  brew install alint
  ```

  The formula is regenerated on every tagged release by a new
  `homebrew` job in `.github/workflows/release.yml` driving
  `ci/scripts/update-homebrew-formula.sh`. The script takes
  SHAs directly from the release's `SHA256SUMS` artifact —
  no re-download, no re-build — and pushes via a per-repo
  ed25519 deploy key scoped to the tap.

### Infrastructure

- New release-workflow jobs: `docker` (builds + pushes the
  multi-arch image to ghcr.io) and `homebrew` (regenerates
  the formula and pushes to the tap). Both run after the
  existing `build` / `release` jobs; failures there skip the
  distribution jobs cleanly.
- New script `ci/scripts/update-homebrew-formula.sh` emits a
  complete `Formula/alint.rb` given `VERSION` + a
  `SHA256SUMS` path. Handles both `sha256sum`-style and
  Windows-mode (`*file`) sum lines, errors clearly on
  missing platforms, validates required env up front.
- New test harness `ci/scripts/test-update-homebrew-formula.sh`
  (14 assertions — happy path, asterisk-prefix handling,
  missing-platform + missing-env error paths, version /
  license / test-block shape). Wired into `ci/scripts/test.sh`
  so it runs on every CI pass.



Ecosystem coverage + debugging ergonomics. Schema-compatible;
every v0.4.5 config runs unchanged. JSON output unchanged for
existing commands; SARIF and GitHub outputs byte-equivalent.

### Added

#### Two new bundled rulesets

- **`alint://bundled/python@v1`** (7 rules). Canonical
  Python-project hygiene:
  - `python-manifest-exists` — pyproject.toml / setup.py /
    setup.cfg at the root (error).
  - `python-has-lockfile` — uv.lock / poetry.lock / Pipfile.lock
    / pdm.lock (warning).
  - `python-pyproject-declares-name` — PEP 621 `$.project.name`
    via `toml_path_matches` (warning).
  - `python-pyproject-declares-requires-python` — a
    `requires-python` floor via `toml_path_matches` on
    `$.project['requires-python']` (info).
  - `python-module-snake-case` — PEP 8 snake_case filenames
    for top-level and `src/**/*.py` (info).
  - `python-sources-final-newline` + `python-sources-no-trailing-whitespace`
    (info, auto-fixable).
  - `python-sources-no-bidi` — Trojan Source defense (error).

  Every rule gated with `when: facts.is_python`, so the ruleset
  silently no-ops on non-Python repos.

- **`alint://bundled/go@v1`** (7 rules). Go-module hygiene:
  - `go-mod-exists` — go.mod at the root (error).
  - `go-sum-exists` — go.sum at the root (warning).
  - `go-mod-declares-module-path` — `module <path>` directive
    (error, `file_content_matches`).
  - `go-mod-declares-go-version` — `go <major>.<minor>` directive
    (warning, `file_content_matches`).
  - `go-sources-no-bidi` / `go-sources-no-zero-width` —
    Trojan Source defenses (error).
  - `go-sources-final-newline` (info, auto-fixable).

  Every rule gated with `when: facts.is_go`.

  Brings the bundled catalog to eleven rulesets.

#### `alint facts` subcommand

- New top-level subcommand that evaluates every `facts:` entry
  in the effective config and prints the resolved value.
  Debugging aid for `when:` clauses — quickly answers "did my
  `facts.is_python` actually match?" without running the full
  check pass. Supports `--format human` (columnar) and
  `--format json` (`{facts: [{id, kind, value}, ...]}`).

### Changed

- `alint-core::FactKind::name()` added — returns the YAML
  discriminator string (`any_file_exists`, `count_files`, etc.).
  Used by the `facts` subcommand's renderers; available to
  external embedders.



Supply-chain hardening ruleset + composition ergonomics.
Schema-compatible; every v0.4.4 config runs unchanged. JSON
output gains no new keys; SARIF and GitHub outputs are
byte-equivalent.

### Added

#### New bundled ruleset

- **`alint://bundled/ci/github-actions@v1`** (3 rules). GitHub
  Actions hardening guided by the two OpenSSF Scorecard checks
  with the strongest supply-chain signal:
  - `gha-workflow-contents-read` — every workflow declares
    `permissions.contents: read` at the workflow level
    (`yaml_path_equals`, warning).
  - `gha-pin-actions-to-sha` — every `uses:` across every job
    is pinned to a 40-char commit SHA, not a mutable tag
    (`yaml_path_matches` with `if_present: true`, warning).
  - `gha-workflow-has-name` — every workflow declares a
    `name:` so the Actions UI shows something friendlier than
    the filename (`yaml_path_matches`, info).

  Scoped to `.github/workflows/*.y{,a}ml`, so it no-ops in
  repos that don't use GitHub Actions. Brings the bundled
  catalogue to nine rulesets.

#### Structured-query `if_present`

- **`if_present: true`** option on every structured-query rule
  kind (`{json,yaml,toml}_path_{equals,matches}`). When
  enabled, a JSONPath query that returns zero matches is
  silently OK — only actual matches that fail the op produce
  violations. Preserves the default "missing = violation"
  semantics (`if_present: false`, the existing behaviour).
  Required for conditional predicates like "every `uses:` is
  SHA-pinned" where a workflow with only `run:` steps
  shouldn't be flagged.

#### Selective bundled adoption

- **`only:` / `except:` on `extends:` entries.** An entry can
  now be a mapping that filters the inherited rule set by id
  before merging:

  ```yaml
  extends:
    - url: alint://bundled/oss-baseline@v1
      except: [oss-code-of-conduct-exists]      # drop one rule

    - url: alint://bundled/ci/github-actions@v1
      only: [gha-pin-actions-to-sha]            # keep one rule
  ```

  Filters resolve against the fully-resolved rule set of the
  entry (i.e. anything it transitively extends). `only:` and
  `except:` are mutually exclusive on a single entry. Listing
  an unknown id is a load-time error so typos don't silently
  drop anything.

  Closes the "all-or-nothing" limitation on bundled-ruleset
  adoption that forced users to extend + then restate overrides
  with `level: off` for every rule they wanted to skip.

### Changed

- `Config.extends` type changed from `Vec<String>` to
  `Vec<ExtendsEntry>`. `ExtendsEntry` is an untagged enum that
  accepts either a bare string (classic form) or a mapping
  `{url, only?, except?}`. YAML ergonomics unchanged for the
  string form; existing configs continue to parse as before.



Rule-catalogue expansion + README rewrite. Schema-compatible;
every v0.4.3 config runs unchanged. JSON output gains no new
keys; SARIF and GitHub outputs are byte-equivalent.

### Added

#### Content-family additions

- **`file_min_size`** — files in scope must be at least
  `min_bytes` bytes. Complements `file_max_size`. Picks up the
  "zero-byte LICENSE" case that passes `file_exists` but carries
  no information.
- **`file_min_lines`** — files in scope must have at least
  `min_lines` lines (`wc -l` semantics: every `\n` terminates a
  line, plus one more when the file has trailing unterminated
  content). Catches the classic "README is a title plus
  `TODO`" stub. Both kinds register short aliases (`min_size`,
  `min_lines`) alongside the prefixed names.

#### Structured-query family (six new rule kinds)

JSONPath (RFC 9535) queries over JSON / YAML / TOML documents,
powered by `serde_json_path`. YAML and TOML files are
deserialized through serde into a `serde_json::Value` so the
same path-expression engine applies to all three. Missing
JSONPath matches are treated as violations (conservative — scope
narrowly or relax the path for optional keys); when a query
returns multiple matches, every match must satisfy the rule.
Unparseable files surface a single per-file violation rather
than being silently skipped.

- **`json_path_equals`** / **`json_path_matches`** — `equals`
  compares by value (string / number / bool / null); `matches`
  runs a regex against the string form of the matched value.
  Canonical use: enforce a `package.json` license, require a
  semver `version`, lock a `private: true` flag.
- **`yaml_path_equals`** / **`yaml_path_matches`** — same
  engine over YAML. Canonical use: lock GitHub Actions
  workflows to `permissions.contents: read`, require every
  `uses:` across every job to be pinned to a 40-char commit
  SHA.
- **`toml_path_equals`** / **`toml_path_matches`** — same
  engine over TOML. Canonical use: require `edition = "2024"`
  across every `Cargo.toml` in a workspace, enforce
  `$.project.version` semver in `pyproject.toml`.

#### `oss-baseline` ruleset extensions

- `oss-license-non-empty` — `file_min_size` at 200 bytes on the
  LICENSE, catching zero-byte placeholders.
- `oss-readme-non-stub` — `file_min_lines` at 3 on the README,
  gentle enough to pass for early-stage repos.

### Changed

- **README rewrite.** Replaced the single monolithic `.alint.yml`
  example with a 12-pattern cookbook covering the real-world use
  cases v0.4 now spans: bundled-ruleset adoption, composition
  overrides, structured queries against `package.json` / GitHub
  workflows / `Cargo.toml`, monorepo per-package rules via
  `for_each_dir`, nested-config subtree scoping, auto-fix
  hygiene, fact-gated conditionals, cross-file `pair` / `unique_by`,
  and the security-family bans. Bumped the family count from
  ten to eleven and the rule-kind count from ~42 to ~50.



Composition ergonomics + monorepo support + four new bundled
rulesets. Schema-compatible; every v0.4.2 config runs unchanged.
JSON output gains no new keys; SARIF and GitHub outputs are
byte-equivalent.

### Added

#### Composition

- **Field-level rule override.** Children in the `extends:`
  chain can specify only the fields that change. A common
  override shrinks from four lines to two:

  ```yaml
  # before — had to restate kind + paths to tweak level
  rules:
    - id: no-bak
      kind: file_absent
      paths: "**/*.bak"
      level: warning
  ```

  ```yaml
  # after — id + changed fields are enough; rest inherits
  rules:
    - id: no-bak
      level: warning
  ```

  The loader keeps rules as raw `serde_yaml_ng::Mapping`s
  through the `extends:` chain and field-merges by id. After
  all extends resolve, each merged mapping is deserialized
  once into a `RuleSpec` — a rule that never receives a
  `kind` anywhere in its chain surfaces as a clean error
  referencing the offending id. Facts still replace wholesale
  by id (their kind is a discriminated union).

- **Nested `.alint.yml` discovery for monorepos.** Opt in with
  `nested_configs: true` on the root config. The loader walks
  the tree (respecting `.gitignore` + `ignore:`), picks up
  every nested `.alint.yml` / `.alint.yaml`, and prefixes each
  nested rule's path-like fields (`paths`, `select`, `primary`)
  with the nested config's relative directory. A rule declared
  in `packages/frontend/.alint.yml` with `paths: "**/*.ts"`
  evaluates as if it read `paths: "packages/frontend/**/*.ts"`
  at the root.

  MVP guardrails: nested configs can only declare `version:`
  and `rules:`; every nested rule must have at least one
  scope field; absolute paths and `..`-prefixed globs are
  rejected; rule-id collisions across configs error with a
  clear message (per-subtree overrides are a follow-up).

#### Bundled rulesets

Four new rulesets pulled from a research pass across
Turborepo/Nx/Bazel/Cargo/pnpm docs, OpenSSF Scorecard, and
Repolinter's archived corpus. Buildable on the existing
primitive set — no new rule kinds required.

- **`alint://bundled/hygiene/no-tracked-artifacts@v1`** — 11
  rules. `dir_absent` on `node_modules`, `target`, `dist`,
  `build`, `out`, `.next`, `.nuxt`, `.svelte-kit`, `.turbo`,
  `coverage`, `__pycache__`, `.venv`, `.mypy_cache`,
  `.pytest_cache`, `.ruff_cache`, `.bundle`, `vendor/bundle`,
  `.go-build`. `file_absent` on `.DS_Store`, `._*`, `Thumbs.db`,
  `desktop.ini`, `*~`, `*.swp`, `*.swo`, `*.bak`, `*.orig`,
  `.env`, `.env.local`, `.env.*.local`, `.env.development` /
  `production` / `staging` (`.env.example` is exempt). 10 MiB
  size gate. Several rules auto-fixable via `file_remove`.

- **`alint://bundled/hygiene/lockfiles@v1`** — 7 rules, one per
  package manager (npm / pnpm / yarn / bun / Cargo / Poetry /
  uv). Each uses an `include/exclude` path pair so the root
  lockfile is exempted while nested copies are flagged as a
  workspace-misconfiguration smell.

- **`alint://bundled/tooling/editorconfig@v1`** — 3 info-level
  rules: root `.editorconfig` + `.gitattributes` exist, and
  `.gitattributes` contains a `text=` normalization directive.

- **`alint://bundled/docs/adr@v1`** — 4 rules. Files under
  `docs/adr/` match `NNNN-kebab-case-title.md`; each ADR has
  `## Status`, `## Context`, `## Decision` sections. Gap-free
  ADR numbering deferred to a future `numeric_sequence`
  primitive.

Bundled catalog now: 8 rulesets (4 ecosystem + 4 namespaced).
Slash-namespaced names (`hygiene/*`, `tooling/*`, `docs/*`)
route through the existing `alint://bundled/<name>@<rev>` URI
scheme — the `@` separator parses cleanly around slashes in
the name.

#### Config

- **`nested_configs: true`** field on the root `Config` to
  opt in to nested-config discovery.

### Changed

- **`extends:` schema description** refreshed to cover SRI
  syntax, `alint://bundled/` URLs, merge semantics, and the
  `level: off` disable idiom. Old description claimed HTTPS
  was "reserved for a future version" (shipped in v0.2.1).

### Tests

Workspace: 422 → 437 tests (+15). Includes 6 new unit tests
on nested-discovery, 3 e2e on field-level override, 2 e2e on
nested discovery, 4 e2e on Phase A bundled rulesets.

## [0.4.2] — 2026-04-22

Pretty-output overhaul of the `human` formatter. Schema-compatible;
every v0.4.1 config still runs, every JSON/SARIF/GitHub output is
byte-equivalent. Only the human-mode rendering and three new global
CLI flags (`--color`, `--ascii`, `--compact`) are new.

### Added

- **Grouped-by-file layout** — violations now render under a
  dim section header per file (`─── src/foo.rs ─────…`), with
  a leading "Repository-level" bucket for path-less findings.
  Each violation shows `<sigil>  <level>  <rule-id>  [fixable]`
  on one line and the message (optionally prefixed with
  `line:col`) on the next. Policy URLs render as `docs: <url>`
  immediately under the relevant violation.
- **Per-severity summary** — `Summary (N violations): ✗ 2 errors
  ⚠ 1 warning  ℹ 5 info` + `X passing · Y failing · Z
  auto-fixable` + a call-to-action line `→ run `alint fix` to
  resolve N fixable violation(s).` when anything's auto-fixable.
  All-passed gets a concise green `✓ All N rule(s) passed.`.
- **`--color <auto|always|never>` global flag.** Defaults to
  `auto` — honors `NO_COLOR`, `CLICOLOR_FORCE`, and TTY status
  via `anstream::AutoStream`. JSON / SARIF / GitHub formats are
  unaffected.
- **`--ascii` global flag** forces ASCII glyphs (e.g. `x`/`!`/`i`
  instead of `✗`/`⚠`/`ℹ`, `---` instead of `───`). Auto-enabled
  when `TERM=dumb`.
- **`--compact` global flag** switches to one-line-per-violation
  output suitable for editors, `grep`, `wc -l`. Format:
  `path:line:col: level: rule-id: message  [fixable]`, with
  `<repo>` as the pseudo-path for path-less findings.
- **OSC 8 hyperlinks on policy URLs** when the terminal supports
  them (detected via `supports-hyperlinks`). Modern terminals
  (iTerm2, Kitty, WezTerm, Alacritty, VSCode, GNOME Terminal,
  Windows Terminal) make the `docs:` URL clickable without any
  visible change. Older terminals see the plain URL they always
  saw.
- **`RuleResult.is_fixable`** exposed on the JSON output as
  `fixable: bool`, letting tooling decide whether to prompt
  users toward `alint fix` without cross-referencing rule
  metadata.

### Changed

- **Terminal-width-aware section separators.** Auto-detected via
  `terminal_size` on TTY, clamped to `[40, 120]` cols so wide
  terminals don't produce unreadably long rules and narrow ones
  still get some visual fill. Falls back to 80 cols off-TTY.
- **Tightened vertical density** — no blank lines between
  violations within a bucket. Visual separation still reads
  clearly because the colored sigil anchors at column 2 while
  continuation lines indent to column 14. A typical 8-violation
  run drops from ~36 to ~27 lines of output.

### Internal

- New `alint-output::style` module centralizing role-based
  `anstyle::Style` constants, `GlyphSet` (Unicode / ASCII), and
  `HumanOptions` (plumbing for glyphs / hyperlinks / width /
  compact). Swapping the palette is a one-file edit.
- `Format::write_with_options` / `Format::write_fix_with_options`
  added; existing `write` / `write_fix` remain as default-opts
  shims so external embedders compile unchanged.

### Dependencies

- `anstyle` + `anstream` (ANSI styling with built-in `NO_COLOR` /
  TTY handling).
- `supports-hyperlinks` (OSC 8 detection).
- `terminal_size` (column-width detection).

## [0.4.1] — 2026-04-21

Packaging fix. v0.4.0 is functionally identical but failed to
publish beyond `alint-core` on crates.io — the bundled-rulesets
`include_str!` paths crossed the crate boundary, so
`cargo publish` for `alint-dsl` couldn't find
`rulesets/v1/*.yml` when packaging the tarball.

### Fixed

- **Move `rulesets/` → `crates/alint-dsl/rulesets/`**. The
  rulesets now live inside the crate that embeds them, so
  `cargo publish` picks them up automatically. Compile-time
  `include_str!` paths in `bundled.rs` change from
  `"../../../rulesets/…"` to `"../rulesets/…"`. No user-visible
  behaviour change — the `alint://bundled/<name>@<rev>` URI
  scheme and all four rulesets work identically.

### Known leftover

- `alint-core@0.4.0` is live on crates.io (it published
  successfully before the packaging error stopped the chain).
  It's functionally identical to `alint-core@0.4.1` and nothing
  transitively depends on it — safe to ignore or yank later.

## [0.4.0] — 2026-04-21

Headline: **bundled rulesets**. The single biggest adoption
lever identified during pre-launch review — reduces onboarding
from "write a ruleset" to "add one `extends:` line." Also lands
pre-commit framework integration so any pre-commit user can
adopt alint with 4 lines of YAML.

### Added

#### Bundled rulesets

- **`alint://bundled/<name>@<rev>` URI scheme** for offline
  resolution of built-in rulesets. Rulesets live under
  `rulesets/<rev>/<name>.yml` and are embedded in the binary via
  `include_str!` at compile time. Cycle-safe, leaf-only — a
  bundled ruleset cannot itself declare `extends:` and cannot
  introduce `custom:` facts, inheriting the same safety guards
  as HTTPS extends.
- **`alint://bundled/oss-baseline@v1`** — 9 rules. Community
  docs (README, LICENSE, SECURITY.md, CODE_OF_CONDUCT.md,
  .gitignore) + merge-marker + bidi-control bans +
  trailing-whitespace / final-newline hygiene (auto-fixable).
- **`alint://bundled/rust@v1`** — 10 rules. Cargo.toml /
  Cargo.lock / rust-toolchain existence, no tracked `target/`,
  snake_case source filenames, Trojan-Source defenses. Every
  rule gated `when: facts.is_rust` so extending it from a
  polyglot repo is a safe no-op outside Rust trees.
- **`alint://bundled/node@v1`** — 8 rules. package.json +
  lockfile (npm / pnpm / yarn / bun), no tracked `node_modules/`
  or common build outputs, Node version pinned via `.nvmrc` /
  `.node-version` / `.tool-versions`, JS/TS source hygiene.
  Gated `when: facts.is_node`.
- **`alint://bundled/monorepo@v1`** — 4 rules. Every directory
  under `{packages,crates,apps,services}/*` has a README +
  ecosystem manifest; unique basenames. Pair with rust@v1 /
  node@v1 for per-package ecosystem checks.

#### Distribution

- **`.pre-commit-hooks.yaml`** at the repo root exposes two
  hooks for [pre-commit](https://pre-commit.com/) users:
  - `alint` — runs `alint check`; non-mutating.
  - `alint-fix` — runs `alint fix`; `stages: [manual]` by
    default so it only runs when explicitly invoked via
    `pre-commit run alint-fix`.

  Both use `language: rust`, so pre-commit builds alint on
  first run — zero install step.

### Changed

- **README quickstart** gains a "Bundled rulesets (one-line
  baseline)" section and a "pre-commit" subsection under "Use
  in CI".
- **`docs/rules.md`** gains a Bundled rulesets section with a
  per-ruleset table (rule id / kind / default level / fix op)
  for every shipped ruleset, plus the override pattern.
- **ROADMAP**: the original v0.4 scope (structured-query
  primitives, git-aware primitives, Homebrew / Docker / npm,
  markdown/junit/gitlab outputs, `command` plugin, nested
  config discovery) rolls forward to v0.5. Bundled rulesets
  moved from v0.5 → v0.4 because they're the largest
  single-step adoption lever.

### Compatibility

- Schema version remains `1`. Every v0.3 config runs unchanged.
- No changes to the `Rule`, `Fixer`, or `Engine` APIs.
  `alint-dsl` gains a new public `bundled` module; the existing
  `load` / `load_with` entry points are unchanged.

## [0.3.2] — 2026-04-21

Patch release fixing a broken `action.yml` that affects every
`asamarts/alint@v0.2.1` / `v0.3.0` / `v0.3.1` consumer. No code
changes to the CLI; no schema changes.

### Fixed

- **`action.yml`** — the `outputs.sarif-file.description` value
  was an unquoted YAML scalar containing `` `format: sarif` ``.
  GitHub's Actions YAML parser recently tightened and now
  rejects the embedded `:` as an ambiguous nested mapping,
  causing `uses: asamarts/alint@<any-released-tag>` to fail at
  job-setup time with `Mapping values are not allowed in this
  context.`. The description is now double-quoted.
- **docs build** — rustdoc's `redundant_explicit_links` became
  warn-by-default in a recent stable; combined with the
  workspace's `RUSTDOCFLAGS="-D warnings"` it was breaking the
  `cargo doc` CI job. Fixed a redundant link target in
  `alint-testkit::runner`. Affects contributors / anyone
  building from source with the current stable; no effect on
  the release binary.

### Changed

- **Action self-test workflow** now pins `uses:` refs to
  `@main` so it exercises the current action code instead of
  an immutable (and, as of this week, unparsable) `@v0.2.1`.
  The explicit-version test still passes a stable release tag
  as the `version:` input — bumped from `v0.2.1` to `v0.3.1`
  so the downloaded CLI understands the dogfood config's v0.3
  rule kinds.

### Upgrade note

Consumers pinning `asamarts/alint@v0.2.1` / `v0.3.0` / `v0.3.1`
should bump to `@v0.3.2`. There are no API / CLI / config
changes — configs that worked under `@v0.3.1` continue to work
verbatim.

## [0.3.1] — 2026-04-21

Documentation-only patch release following v0.3.0. No code
changes; no schema changes; v0.3.0 configs run unchanged.

### Added

- **`docs/rules.md`** — per-rule user reference organised by the
  ten families (Existence, Content, Naming, Text hygiene,
  Security / Unicode, Encoding, Structure, Portable metadata,
  Unix metadata, Git hygiene, Cross-file). Each entry has a
  purpose, a small YAML example, and a pointer to its fix op if
  one exists.

### Changed

- **`docs/design/ARCHITECTURE.md`** — rule-catalogue section
  expanded with new family tables (Text hygiene, Security /
  Unicode sanity, Structure, Portable metadata, Unix metadata,
  Git hygiene) and a new Fix operations subsection listing
  every op with its rule-kind cross-reference.
- **`README.md`** — status line bumped from "v0.2 / 18 rules /
  4 families" to "v0.3 / ~42 rules / 10 families / 12 fix ops";
  GitHub Action references updated from `asamarts/alint@v0.2.1`
  to `@v0.3.0`.
- **`.alint.yml`** (dogfood) — expanded from 17 to 32 rules to
  exercise the v0.3 catalogue against alint's own tree. All
  rules pass.

## [0.3.0] — 2026-04-21

Rule-catalogue expansion. Adds ~25 new rule kinds across seven
phase commits plus one new fix op, covering categories other
repo-linters don't reach: Windows-name reserved words, bidi /
zero-width Unicode scanners, Unix-metadata checks, and
byte-level prefix/suffix. Also introduces the `fix_size_limit`
config knob and short-name aliases for the rules that don't
have a `dir_*` sibling.

### Added

#### Rule kinds (text hygiene — Phase 1)

- **`no_trailing_whitespace`** — flag trailing space/tab on any
  line. Fixable via `file_trim_trailing_whitespace` (preserves
  LF vs CRLF endings).
- **`final_newline`** — file must end with `\n`. Fixable via
  `file_append_final_newline`.
- **`line_endings`** — `target: lf | crlf`; every line must use
  the configured ending. Fixable via
  `file_normalize_line_endings`.
- **`line_max_width`** — cap line length in characters (not
  bytes); optional `tab_width` for tab expansion.

#### Rule kinds (security / Unicode — Phase 2)

- **`no_merge_conflict_markers`** — flag `<<<<<<< `, `=======`,
  `>>>>>>> ` markers at the start of a line.
- **`no_bidi_controls`** — flag Trojan-Source bidi overrides
  (U+202A–202E, U+2066–2069). Fixable via `file_strip_bidi`.
- **`no_zero_width_chars`** — flag body-internal zero-width
  characters (U+200B/C/D plus non-leading U+FEFF). Leading BOM
  is `no_bom`'s concern. Fixable via `file_strip_zero_width`.

#### Rule kinds (encoding + content fingerprint — Phase 3)

- **`file_is_ascii`** — every byte must be < 0x80.
- **`no_bom`** — flag UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE
  byte-order marks. Fixable via `file_strip_bom`.
- **`file_hash`** — assert a SHA-256 digest for specific files
  (rules-as-tripwire for generated artefacts).

#### Rule kinds (structure — Phase 4)

- **`max_directory_depth`** — cap how deep the tree may go.
- **`max_files_per_directory`** — cap per-directory fanout.
- **`no_empty_files`** — flag zero-byte files. Fixable via
  `file_remove`.

#### Rule kinds (portable metadata — Phase 5)

- **`no_case_conflicts`** — flag paths that collide under a
  case-insensitive filesystem (macOS HFS+/APFS, Windows NTFS
  defaults).
- **`no_illegal_windows_names`** — reject CON/PRN/AUX/NUL,
  COM1-9, LPT1-9 (case-insensitive, regardless of extension),
  trailing dots/spaces, and the reserved chars `<>:"|?*`.

#### Rule kinds (Unix metadata + git — Phase 6)

- **`no_symlinks`** — flag tracked paths that are symbolic
  links. Fixable via `file_remove`.
- **`executable_bit`** — `require: true|false`; enforce or
  forbid the `+x` bit. Unix-only; no-op on Windows.
- **`executable_has_shebang`** — `+x` files must begin with
  `#!`. Unix-only.
- **`shebang_has_executable`** — files starting with `#!` must
  have `+x` set. Unix-only.
- **`no_submodules`** — flag `.gitmodules` at the repo root.
  Always targets `.gitmodules` (no `paths` override). Fixable
  via `file_remove`.

#### Rule kinds (hygiene + fingerprint — Phase 7)

- **`indent_style`** — `style: tabs|spaces`, optional `width`
  for spaces; every non-blank line must indent with the
  configured style.
- **`max_consecutive_blank_lines`** — `max: N`; cap runs of
  blank lines. Fixable via new op `file_collapse_blank_lines`.
- **`file_starts_with`** — byte-level prefix check. Works on
  binary files, unlike `file_header` which is UTF-8 text.
- **`file_ends_with`** — byte-level suffix check.

#### Fix ops

- **`file_trim_trailing_whitespace`** — strip trailing space/tab
  on every line (preserves line endings).
- **`file_append_final_newline`** — add `\n` when missing.
- **`file_normalize_line_endings`** — rewrite to the parent
  rule's `lf` / `crlf` target.
- **`file_strip_bidi`** — remove U+202A–202E, U+2066–2069.
- **`file_strip_zero_width`** — remove U+200B/C/D and
  body-internal U+FEFF.
- **`file_strip_bom`** — strip a leading UTF-8/16/32 BOM.
- **`file_collapse_blank_lines`** — collapse blank-line runs to
  the parent rule's `max`.

#### Config + ergonomics

- **`fix_size_limit`** (top-level config field) — maximum bytes
  a content-editing fix will touch. Default 1 MiB; explicit
  `null` disables the cap; path-only fixes (`file_create`,
  `file_remove`, `file_rename`) ignore it. Over-limit files
  report `Skipped` with a stderr warning.
- **Short-name rule aliases** — rules without a `dir_*` sibling
  also resolve under their unprefixed name:
  `content_matches`, `content_forbidden`, `header`, `max_size`,
  `is_text`. `file_exists` / `file_absent` keep the prefix
  because they mirror `dir_exists` / `dir_absent`.

### Changed

- JSON Schema (`schemas/v1/config.json`) gains every new rule
  kind, fix op, and the `fix_size_limit` field. Root and
  in-crate copies stay byte-identical via the drift-guard test.

### Compatibility

- Schema version remains `1`. Every v0.2 config runs unchanged
  under v0.3. The `Rule` and `Fixer` traits gained no new
  required methods; out-of-tree implementations compile
  unmodified. `Engine::with_fix_size_limit` is additive.

## [0.2.1] — 2026-04-20

Patch release. Finishes the v0.2 roadmap item that didn't make it
into v0.2.0: `extends:` composition.

### Added

- **`extends:` for local files.** A config can inherit rules,
  facts, vars, and ignore globs from another YAML file on disk
  (relative or absolute path). Resolution is recursive with cycle
  detection; merge is id-based with child-overrides-parent
  semantics for rules + facts, dict-merge for vars, and
  concatenation for ignore globs.
- **`extends:` for HTTPS URLs with SHA-256 SRI.** Remote entries
  take the form `https://.../foo.yml#sha256-<64 hex chars>`. The
  SRI is non-negotiable: URLs without it are rejected. Responses
  are verified against the declared hash before use and cached
  atomically on disk at `<user-cache-dir>/alint/rulesets/<sri>.yml`.
  `ureq` is the underlying HTTPS client (rustls for TLS — no
  OS-native crypto linking).
- **`alint_dsl::load_with(path, &LoadOptions)`** for embedders and
  tests that need to pin the cache path or override the fetcher.
- **Action self-test workflow** (`.github/workflows/action-selftest.yml`)
  that dogfoods `asamarts/alint@<tag>` across `ubuntu-latest` on
  four configurations (default, `format: sarif` + JSON-parse
  assertion, `format: json`, explicit `version:` input). Catches
  regressions in the release-tarball → install.sh → binary
  distribution chain that in-process tests don't exercise.

### Security

- HTTPS `extends:` requires SRI on every entry; no
  trust-on-first-use. `http://` schemes are rejected outright.
  Cache entries are re-verified against SRI on read, so a
  tampered on-disk cache fails loudly rather than serving bad
  content. Body size is capped at 16 MiB to bound memory against
  a hostile server.

### Known limitations

- Remote configs cannot themselves contain `extends:` (nested
  remote extends deferred — a relative path inside a fetched
  config has no principled base for resolution).
- The config-wide `respect_gitignore` field cannot distinguish
  "unset" from the `true` default during merge; the child's
  value wins unconditionally.

### Added dependencies

- `ureq` (rustls TLS), `sha2`, `directories`. Release-binary size
  impact: ~+1.5–2 MiB, mostly from rustls' embedded root certs.

## [0.2.0] — 2026-04-19

Second release. The theme is composition and remediation: cross-file rules,
conditional gating, auto-fix, and two new output formats. Every v0.1 config
continues to validate and run under v0.2 without changes.

### Added

#### Rule kinds

- **Cross-file primitives (7 kinds)** — `pair`, `for_each_dir`, `for_each_file`,
  `dir_contains`, `dir_only_contains`, `unique_by`, `every_matching_has`. These
  support path-template substitution (`{dir}`, `{stem}`, `{ext}`, `{basename}`,
  `{path}`, `{parent_name}`) and nested rule instantiation for per-iteration
  semantics.

#### Facts + conditional rules

- **Facts system** — repository properties evaluated once per run. Three kinds
  shipped: `any_file_exists`, `all_files_exist`, `count_files`. Referenced
  from `when:` clauses and rule messages.
- **`when:` expression language** — bounded recursive-descent parser with
  boolean logic (`and`, `or`, `not`), comparison operators (`==`, `!=`, `<`,
  `<=`, `>`, `>=`), `in` (list or substring), `matches` (regex), literal
  types (bool/int/string/list/null), and `facts.*` / `vars.*` identifiers.
  Parsed at rule-build time; gates both top-level rules and nested rules
  inside `for_each_*`.

#### `fix` subcommand

- **`alint fix [path]`** — applies mechanical corrections for violations whose
  rule declares a fix strategy. Five ops:
  - `file_create` (paired with `file_exists`) — writes declared content.
  - `file_remove` (paired with `file_absent`) — deletes the violating file.
  - `file_prepend` (paired with `file_header`) — injects content at the top,
    preserving UTF-8 BOM.
  - `file_append` (paired with `file_content_matches`) — appends content.
  - `file_rename` (paired with `filename_case`) — converts the stem to the
    rule's target case, preserving extension and parent directory.
- **`--dry-run`** previews the outcome without touching disk.
- **Safety** — `file_create` refuses to overwrite existing files;
  `file_rename` refuses to overwrite a collision; all ops skip cleanly with
  a diagnostic when preconditions aren't met.

#### Output formats

- **`sarif`** — SARIF 2.1.0 JSON, targeting GitHub Code Scanning's upload
  action. Each violation becomes a `result` with a `physicalLocation`
  anchored on the violating path.
- **`github`** — GitHub Actions workflow-command annotations
  (`::error title=...::`). Renders inline on PR file-changed view.

#### Distribution

- **Official GitHub Action** at `asamarts/alint@v0.2.0` — composite action
  wrapping `install.sh`. Inputs: `version`, `path`, `config` (multi-line),
  `format` (default `github`), `fail-on-warning`, `args`, `working-directory`.
  Output: `sarif-file` for feeding the Code Scanning uploader.

#### Testing infrastructure

- **`alint-testkit` + `alint-e2e`** (internal crates) — scenario-driven
  end-to-end tests. 51 YAML scenarios auto-generated into `#[test]`s via
  `dir-test`, 20 CLI snapshot tests via `trycmd`, and 4 property-based
  invariants via `proptest`. See the [ARCHITECTURE / testing
  sections](docs/design/ARCHITECTURE.md) for the rationale.

### Changed

- **Internal workspace crates published** — `alint-dsl`, `alint-rules`, and
  `alint-output` are now published on crates.io. They carry
  `description: "Internal: ... Not a stable public API."`; only `alint` and
  `alint-core` are semver-stable. This change is required so that
  `cargo install alint` resolves its transitive path-or-crates-io
  dependencies.
- **JSON Schema** (`schemas/v1/config.json`) gains the `fix:` block, every
  new rule kind, and the `facts:` + `when:` fields. Root + in-crate copies
  are kept byte-identical by a drift-guard test.

### Fixed

- **`install.sh`** no longer aborts on SIGPIPE when resolving the latest
  release tag. Previously, `curl | awk '{...; exit}'` under
  `set -o pipefail` caused curl to error out with exit 23 after awk's early
  exit; the fetch is now decoupled from the parse.

### Compatibility

- Config schema version remains `1`. Existing v0.1 configs run unchanged.
- The public API of `alint-core` has not been broken. The `Rule` trait
  gained a `fixer()` method with a `None` default, so out-of-tree rule
  implementations compile without modification.

## [0.1.0] — 2026-04-19

Initial release. MVP.

### Added

- 11 rule primitives — `file_exists`, `file_absent`, `dir_exists`,
  `dir_absent`, `file_content_matches`, `file_content_forbidden`,
  `file_header`, `filename_case`, `filename_regex`, `file_max_size`,
  `file_is_text`.
- Walker that honors `.gitignore`; globset-based scopes with Git-style
  `**` semantics (separator-aware).
- CLI subcommands: `check`, `list`, `explain`.
- Output formats: `human`, `json`.
- JSON Schema at `schemas/v1/config.json` for editor autocomplete.
- Benchmarks (criterion micros + hyperfine macros).
- Static binaries on GitHub Releases for Linux
  (x86_64/aarch64 musl), macOS (x86_64/aarch64), and Windows (x86_64).
- Install script (`install.sh`) with platform detection + SHA-256
  verification.
- Dogfood `.alint.yml` exercising the tool against its own repo.

[Unreleased]: https://github.com/asamarts/alint/compare/v0.9.4...HEAD
[0.9.4]: https://github.com/asamarts/alint/compare/v0.9.3...v0.9.4
[0.9.3]: https://github.com/asamarts/alint/compare/v0.9.2...v0.9.3
[0.9.2]: https://github.com/asamarts/alint/compare/v0.9.1...v0.9.2
[0.9.1]: https://github.com/asamarts/alint/compare/v0.8.2...v0.9.1
[0.8.2]: https://github.com/asamarts/alint/compare/v0.8.1...v0.8.2
[0.8.1]: https://github.com/asamarts/alint/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/asamarts/alint/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/asamarts/alint/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/asamarts/alint/compare/v0.5.12...v0.6.0
[0.5.6]: https://github.com/asamarts/alint/compare/v0.5.5...v0.5.6
[0.5.5]: https://github.com/asamarts/alint/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/asamarts/alint/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/asamarts/alint/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/asamarts/alint/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/asamarts/alint/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/asamarts/alint/compare/v0.4.10...v0.5.0
[0.4.10]: https://github.com/asamarts/alint/compare/v0.4.9...v0.4.10
[0.4.9]: https://github.com/asamarts/alint/compare/v0.4.8...v0.4.9
[0.4.8]: https://github.com/asamarts/alint/compare/v0.4.7...v0.4.8
[0.4.7]: https://github.com/asamarts/alint/compare/v0.4.6...v0.4.7
[0.4.6]: https://github.com/asamarts/alint/compare/v0.4.5...v0.4.6
[0.4.5]: https://github.com/asamarts/alint/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/asamarts/alint/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/asamarts/alint/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/asamarts/alint/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/asamarts/alint/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/asamarts/alint/compare/v0.3.2...v0.4.0
[0.3.2]: https://github.com/asamarts/alint/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/asamarts/alint/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/asamarts/alint/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/asamarts/alint/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/asamarts/alint/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/asamarts/alint/releases/tag/v0.1.0
