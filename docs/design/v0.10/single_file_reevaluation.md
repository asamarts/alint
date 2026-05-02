# Single-file re-evaluation contract

Status: Design draft, written 2026-05-02 after v0.9.6.

## Problem

The LSP server (see [`lsp_server.md`](./lsp_server.md)) needs to
re-evaluate rules against a single file on every keystroke (debounced).
A full `Engine::run` on a 100k-file workspace takes ~186ms (per
`docs/benchmarks/macro/results/linux-x86_64/v0.9.5/`); on every keystroke
that's a UX disaster. We need a `run_for_file` contract that costs
proportional to *one* file's evaluation, not the whole tree's.

Most of the primitives are already in place after v0.9:

- **v0.9.3 dispatch flip** — per-file rules already opt into a
  file-major loop with bytes pre-loaded.
- **v0.9.5 lazy path-index** — `FileIndex::contains_file` is O(1).
- **v0.9.6 scope_filter** (post-release fix) — per-file rules now
  correctly gate on `Rule::scope_filter()`, so a rule scoped to
  `Cargo.toml`-bearing subtrees skips files outside that subtree
  cheaply.

What's missing is the engine-side method.

## Surface area

A new method on `Engine`:

```rust
impl Engine {
    pub fn run_for_file(
        &self,
        root: &Path,
        index: &FileIndex,
        file_path: &Path,
        bytes: &[u8],
    ) -> Result<Vec<RuleResult>>;
}
```

Semantics:

1. Filter `self.entries` to per-file rules whose `path_scope` matches
   `file_path` AND (if `scope_filter` is set) whose `scope_filter`
   matches `file_path`. Skip the rest.
2. For each surviving rule, call `as_per_file().evaluate_file(ctx,
   file_path, bytes)`.
3. Aggregate violations into `RuleResult`s, preserving
   `is_fixable` / `policy_url` / `level` metadata.
4. Cross-file rules: NOT re-evaluated by `run_for_file`. The LSP server
   tracks them separately and re-runs them on save (and only those
   whose scope intersects the changed file).

The `Context` is built the same way as `run` — facts, vars,
git-tracked, blame-cache — but the index is the cached one (no re-walk).

## Semantics

Cost model:

- Per-file rule selection: O(R) where R is the rule count. Each rule's
  `path_scope().matches(file_path)` is O(1) for literal paths,
  O(glob complexity) for globs.
- `scope_filter` check: O(depth × M) per rule where M is the manifest
  list size. Typical ~150ns per rule (5-deep tree, 1 manifest).
- Per-rule evaluation: dependent on rule kind, but bounded by file size
  (not workspace size).

For a 100k-file workspace with 50 loaded rules, single-file
re-evaluation should be ~5ms (vs ~186ms full).

## False-positive surface

- **Cross-file rules silently skipped**: documented behaviour. The LSP
  server is responsible for re-running them on save when their scope
  intersects the changed file.
- **`when:` clauses with `iter`** are per-iteration (only meaningful
  inside `for_each_dir`/`for_each_file`); `when:` at the rule level uses
  the same constant facts/vars `run` does, evaluated once at engine
  build.
- **Facts that depend on file content** (none today, but possible
  future) would need invalidation. Not in v0.10 scope.

## Implementation notes

- Land in `crates/alint-core/src/engine.rs` next to the existing `run`
  and `fix` methods.
- Re-use `run_per_file`'s inner dispatch logic via a shared helper —
  refactor only if the duplication exceeds ~30 lines.
- New micro-bench in `crates/alint-bench/benches/single_file_rules.rs`:
  capture single-file re-evaluation cost vs `Engine::run` ratio at 1k /
  10k / 100k workspaces.

Complexity estimate: ~150 lines, plus a refactor of the existing
`run_per_file` dispatch to extract the shared helper.

## Tests

- Unit: `engine::tests` with a 3-rule fixture (per-file in scope,
  per-file out of scope, cross-file). Assert only the in-scope rule
  fires; cross-file rule absent from result.
- Integration: e2e scenario `crates/alint-e2e/scenarios/check/lsp/`
  that exercises the run_for_file path against a small polyglot tree.
- Bench: `single_file_rules` micro-bench (mentioned above).

## Open questions

1. **Should `run_for_file` accept the file bytes from the LSP server
   (which holds the in-memory edited copy) or re-read from disk?** Take
   the bytes — the LSP server's view is authoritative for unsaved
   edits. Disk reads would surface stale content.
2. **What happens when `file_path` is not in the `index`?** Return an
   error (`Error::file_not_in_index`), distinct from "rules ran but
   produced no violations." The LSP server can interpret this as "the
   file is excluded by `.gitignore` / `ignore:` and shouldn't be
   linted."
3. **Should `--changed` mode use this method internally?** Worth
   measuring — the existing `--changed` filter is at the FileIndex
   level (`build_filtered_index`); re-using `run_for_file` would
   simplify the engine but may regress for large change-sets. Keep
   them separate for v0.10; revisit if profiling shows wins.
