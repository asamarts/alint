# v0.9.11 — `git_tracked_only` via engine-side filtered FileIndex

Status: Design draft (target: v0.9.11).

## Why

The same recurrence-risk shape that produced the v0.9.6 /
v0.9.7 / v0.9.9 silent-no-op `scope_filter:` bug class
applies to `git_tracked_only`:

| Shape | scope_filter (v0.9.6+) | git_tracked_only (v0.4.8+) |
|---|---|---|
| Per-rule field | `Option<ScopeFilter>` | `bool` |
| Engine wire-up | `Rule::scope_filter()` override | `Rule::wants_git_tracked()` override |
| Runtime check | `if let Some(filter) = ... && !filter.matches(...)` | `if self.git_tracked_only && !ctx.is_git_tracked(...)` |
| Bug class | rule forgets to wire the field through | rule forgets either wire-up |
| Fixed structurally | v0.9.10 (Scope owns the field) | v0.9.11 (this doc) |

v0.9.10's audit test
(`coverage_audit_git_tracked_only.rs`) catches the
silent-drop at PR-time; v0.9.11 closes the gap structurally
so the bug class can no longer be introduced at all.

## Why not Scope ownership (Option A)

Mirroring v0.9.10 — bundling `git_tracked_only` into
`Scope` and reaching it through `Scope::matches(&Path,
&Context)` — was the obvious symmetric followup.
Rejected because:

1. **`Scope`'s mental model is a path predicate.** Adding
   a `dir_mode: bool` discriminator (because `dir_*`
   rules use `dir_has_tracked_files()` not
   `set.contains()`) pollutes the abstraction with a
   "what kind of entry am I checking?" concept that
   doesn't belong there.
2. **Compile-enforcement is incomplete.** The bug class
   is "rule forgets to consult". Scope ownership requires
   the rule to call `scope.matches(path, ctx)` correctly;
   a custom check (`if path.exists() { ... }`) bypassing
   `Scope` re-introduces the bug. Engine-side filtering
   removes the per-rule check entirely — the rule
   physically cannot see out-of-scope files.
3. **Back-to-back `alint-core::Scope::matches` signature
   change** (after v0.9.10 broke it once) would be unkind
   to library consumers.

## What

The engine builds two filtered FileIndexes when any rule
opts into `git_tracked_only`:

```rust
// Engine state, computed once per run when wanted:
struct GitTrackedIndexes {
    /// Files in `git ls-files` output. Built by filtering
    /// `index.entries` to only entries where
    /// `is_dir == false` AND `git_tracked.contains(path)`.
    file_only: FileIndex,
    /// Dirs that contain at least one tracked file
    /// (recursive). Built by filtering `index.entries`
    /// to only entries where `is_dir == true` AND
    /// `dir_has_tracked_files(path, &git_tracked)`.
    /// Files in `index.files()` are also included if
    /// they're tracked (so a `dir_*` rule's nested
    /// `paths:` glob can still match against tracked
    /// files in those dirs).
    dir_aware: FileIndex,
}
```

Per-rule context selection mirrors today's `--changed`
filtering pattern at `engine.rs::pick_ctx`:

```rust
fn pick_git_aware_ctx<'a>(
    rule: &dyn Rule,
    full_ctx: &'a Context<'a>,
    file_tracked_ctx: Option<&'a Context<'a>>,
    dir_tracked_ctx: Option<&'a Context<'a>>,
) -> &'a Context<'a> {
    match rule.git_tracked_mode() {
        GitTrackedMode::Off => full_ctx,
        GitTrackedMode::FileOnly => file_tracked_ctx.unwrap_or(full_ctx),
        GitTrackedMode::DirAware => dir_tracked_ctx.unwrap_or(full_ctx),
    }
}
```

A new `Rule::git_tracked_mode() -> GitTrackedMode`
trait method replaces `wants_git_tracked() -> bool` with
a richer enum that lets the engine pick the right
filtered index. The 4 existence rules override:

```rust
// file_exists, file_absent
fn git_tracked_mode(&self) -> GitTrackedMode {
    if self.git_tracked_only { GitTrackedMode::FileOnly } else { GitTrackedMode::Off }
}

// dir_exists, dir_absent
fn git_tracked_mode(&self) -> GitTrackedMode {
    if self.git_tracked_only { GitTrackedMode::DirAware } else { GitTrackedMode::Off }
}
```

The runtime check inside each rule's `evaluate()` —
`if self.git_tracked_only && !ctx.is_git_tracked(...) { skip }` —
**deletes**. The rule iterates `ctx.index.files()` /
`ctx.index.dirs()` exactly as before; the index is already
pre-filtered to the tracked subset.

`Context::is_git_tracked()` and
`Context::dir_has_tracked_files()` accessors stay (still
useful for hypothetical non-existence rules that might
want to consult tracked-set state without filtering
their iteration), but no rule in the tree calls them
after Phase B.

## Compile-enforcement strength

After v0.9.11, the `git_tracked_only` bug class cannot recur:

- A new rule that ships `git_tracked_only: bool` on its
  spec but forgets `git_tracked_mode()` defaults to
  `GitTrackedMode::Off` and the engine routes it through
  the unfiltered `full_ctx`. `coverage_audit_git_tracked_only.rs`
  catches this at PR-time.
- A new rule that overrides `git_tracked_mode` to return
  `FileOnly` / `DirAware` automatically gets the filtered
  index — there's no per-rule runtime check to forget.
- A new rule that bypasses the engine-provided index and
  reads files directly (e.g., `std::fs::read_dir`) is a
  much rarer anti-pattern; the audit test can be extended
  to flag this.

## Implementation plan

### Phase A — `GitTrackedMode` enum + trait method (alint-core)

1. Add `pub enum GitTrackedMode { Off, FileOnly, DirAware }`
   to `alint-core::rule`.
2. Add `Rule::git_tracked_mode(&self) -> GitTrackedMode`
   trait method (default `Off`).
3. Keep `Rule::wants_git_tracked()` for one minor version
   as a deprecated default that delegates to
   `git_tracked_mode() != Off`. Remove in v0.9.12.
4. Engine: replace `wants_git_tracked()` consultation with
   `git_tracked_mode()` inspection at engine-construction
   time so the HashSet is built when ANY rule's mode is
   non-Off.

### Phase B — Engine `build_git_tracked_indexes` (alint-core)

1. Add `Engine::build_git_tracked_indexes(index, set) ->
   Option<GitTrackedIndexes>` mirroring the existing
   `build_filtered_index` for `--changed`.
2. Build only when at least one rule has a non-Off mode.
3. Each filtered FileIndex is a wholly-owned `FileIndex`
   (not a borrowed view) so it can sit in the engine
   alongside `filtered_index` and be referenced by
   `Context`s built per-rule.
4. Bench microbench: O(N) HashSet lookups at construction;
   should amortise across multiple opted-in rules.

### Phase C — `pick_git_aware_ctx` + per-rule routing (alint-core)

1. Build two extra `Context`s when `git_tracked_indexes`
   is `Some`: one over `file_only`, one over `dir_aware`.
2. At each rule-dispatch site (cross-file partition,
   per-file partition, fix path), call
   `pick_git_aware_ctx(rule, full_ctx, file_ctx, dir_ctx)`
   to pick the right Context for the rule.
3. The existing `pick_ctx` for `--changed` mode composes
   with this: pick git-aware first, then potentially
   filter-down for changed mode.

### Phase D — Rule cleanup (alint-rules)

1. `file_exists`, `file_absent` — drop the
   `if self.git_tracked_only && !ctx.is_git_tracked(...)`
   check from `evaluate()`. Override `git_tracked_mode`.
2. `dir_exists`, `dir_absent` — drop the
   `if self.git_tracked_only && !ctx.dir_has_tracked_files(...)`
   check. Override `git_tracked_mode`.
3. Each rule's `git_tracked_only: bool` field stays on
   the struct (still parsed from YAML, still influences
   the trait method). The structural fix is about *how*
   it's consulted, not whether the field exists.
4. Update each rule's `coverage_audit_git_tracked_only`
   audit-doc inline comment to point at the new design
   rationale.

### Phase E — Bench + e2e validation

1. **Bench**: re-run S8 (git overlay) at all sizes; expect
   ±5 % vs v0.9.10 with potential slight win at scale
   from amortising the HashSet lookup.
2. **E2e**: existing `crates/alint-e2e/scenarios/check/git/`
   scenarios must pass without modification.
3. **Audit**: extend `coverage_audit_git_tracked_only.rs`
   to also assert that NO rule's `evaluate` body still
   contains `is_git_tracked(` or `dir_has_tracked_files(`
   (everything goes through the filtered index now).

### Phase F — Release v0.9.11

CHANGELOG entry calling out:
- New `Rule::git_tracked_mode()` trait method.
- Deprecation of `Rule::wants_git_tracked()` (delegates
  to `git_tracked_mode() != Off`; removed v0.9.12).
- Engine internal-only refactor; **no breaking API
  change** for rule authors.
- 4 existence rules cleaned up.
- Same audit gate from v0.9.10 retained.

## Out of scope for v0.9.11

- **Scope-style ownership** (Option A) — rejected per
  "Why not" above.
- **`when:` ownership.** Different semantics (eval-env,
  not a path predicate); no shared silent-no-op shape.
  Held indefinitely.
- **Generalising `git_tracked_only` to a `git_filter:`
  primitive** with richer predicates (e.g.,
  `git_modified_since: <commit>`, `git_blame_age: <range>`).
  Tracked separately; would compose cleanly with this
  design (extra modes on the enum + extra filtered
  indexes).

## Open questions

1. **Should `Rule::wants_git_tracked()` be deleted in
   v0.9.11 instead of deprecated?** Leaning deprecate
   for one minor version since it's a public trait
   method out-of-tree plugins might override. Same
   trade-off v0.9.10 made for `Scope::matches` (we did
   NOT keep the old signature; rules outside the tree
   broke at compile time). The `wants_git_tracked` case
   is a default-method override (silent break vs compile
   break), so deprecation is gentler.
2. **`dir_aware` index naming.** Mixed-content (dirs that
   have tracked files PLUS the tracked files themselves)
   so `dir_*` rule's nested checks still work. Could be
   cleaner as two truly separate indexes (dirs only +
   files only) at the cost of double the build work.
   Current shape mirrors how `dir_*` rules iterate today.
3. **Performance crossover point.** At 1 rule opting in,
   per-iteration HashSet lookup may beat per-build O(N)
   pass. Worth a microbench before committing — if the
   crossover is at >10 rules, this design is a perf
   regression for the common case (1-2 git-aware rules
   in a typical config).

## Acceptance

- 4 existence rules' `evaluate()` bodies no longer
  contain `is_git_tracked(` or `dir_has_tracked_files(`.
  Verified by extended audit test.
- `Rule::wants_git_tracked()` removed; `git_tracked_mode()`
  is the only consultation point.
- S8 macro bench at all sizes within ±5 % of v0.9.10.
- E2e suite passes without modification.
- New rule that ships `git_tracked_only: bool` cannot
  silently drop the filter (compile-enforced via the
  `git_tracked_mode()` trait method; runtime no-op
  removed).
