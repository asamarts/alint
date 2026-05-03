# v0.9.10 — Scope owns scope_filter

Status: Design draft (target: v0.9.10).

## Why

Three releases in a row have shipped with at least one rule
class silently dropping `scope_filter:`:

| Release | Bug | Rules affected | Fix |
|---|---|---:|---|
| v0.9.6 | engine never wired the parsed filter onto built rules | 25 per-file content rules | v0.9.7 sweep |
| v0.9.8 | `for_each_dir` literal-path bypass skipped the filter | 1 dispatch site | v0.9.9 guard |
| v0.9.8 | 17 non-PerFileRule rules iterating `ctx.index.files()` ignored the filter | 17 rules | v0.9.9 sweep |

Each fix is correct; none of them prevent the next instance.
The shape is identical: a rule (or dispatch site) holds a
`Scope` + a separate `Option<ScopeFilter>` field, both parsed
from the same `RuleSpec`, and the rule must remember to
consult **both** on every iteration. The compiler does not
catch the omission because nothing wires the two together.

A new rule kind ships in v0.9.X+1, the author copy-pastes
from a sibling rule that doesn't have `scope_filter:`, and
the bug recurs. We have proven this empirically three times.

## What

Make `Scope` own its `scope_filter`. The `Scope::matches`
signature changes from `(&Path)` to `(&Path, &FileIndex)` so
the compiler forces every call site to thread the index —
which is the only thing that lets `scope_filter` evaluate
ancestor predicates.

```rust
// v0.9.10 Scope (sketch)
pub struct Scope {
    include: GlobSet,
    exclude: GlobSet,
    has_include: bool,
    scope_filter: Option<ScopeFilter>,  // moved in
}

impl Scope {
    pub fn from_spec(spec: &RuleSpec) -> Result<Self> {
        let scope = match &spec.paths {
            Some(p) => Self::from_paths_spec(p)?,
            None => Self::match_all(),
        };
        Ok(Scope {
            scope_filter: spec.parse_scope_filter()?,
            ..scope
        })
    }

    // Signature change: takes the index too.
    pub fn matches(&self, path: &Path, index: &FileIndex) -> bool {
        if self.exclude.is_match(path) { return false; }
        if self.has_include && !self.include.is_match(path) { return false; }
        if let Some(filter) = &self.scope_filter
            && !filter.matches(path, index)
        {
            return false;
        }
        true
    }
}
```

Compile-enforced consequences:
- Every rule's `evaluate()` that called `self.scope.matches(&path)`
  is a compile error until the author threads `ctx.index`.
- Once threaded, `scope_filter` is consulted automatically —
  there is no per-rule `scope_filter` field to forget about.
- A new rule kind written from scratch has no way to bypass
  the filter unless the author explicitly *avoids* `Scope`
  (which is then visible in code review).

## Migration

This is a breaking change to public API
(`alint_core::Scope::matches` signature). Acceptable for a
0.y.z release per SemVer, but explicitly named in the release
notes. Migration shape:

| Before | After |
|---|---|
| `let scope = Scope::from_paths_spec(paths)?;` + separate `scope_filter: spec.parse_scope_filter()?,` | `let scope = Scope::from_spec(spec)?;` |
| `if !self.scope.matches(&entry.path) { continue; }` then a separate `if let Some(filter) = &self.scope_filter ...` | `if !self.scope.matches(&entry.path, ctx.index) { continue; }` (single check) |
| `impl Rule { fn scope_filter(&self) -> Option<&ScopeFilter> { self.scope_filter.as_ref() } }` | Override delegates to the scope: `self.scope.scope_filter()` |

The 25 v0.9.7 rules + the 17 v0.9.9 rules + every cross-file
rule that holds a `Scope` are all touched. Each loses the
`scope_filter: Option<ScopeFilter>` field, the
`fn scope_filter()` override (or it delegates), and gains
the `ctx.index` argument on the matches call.

## Implementation plan

1. **Phase I — `Scope` API** (alint-core only). Add the
   field, the constructor, the new signature. Keep the old
   `matches(&Path)` as `#[deprecated]` for one minor version
   so out-of-tree alint plugins migrate cleanly. (Internal
   alint-rules call sites will all migrate in Phase J — the
   deprecation is for downstream consumers.)
2. **Phase J — rule sweep via compiler**. Run `cargo check`
   workspace-wide, fix every compile error: drop the redundant
   `scope_filter` field from each rule struct, replace
   `from_paths_spec` calls with `from_spec`, thread `ctx.index`
   into matches. The compiler is the worklist.
3. **Phase K — `git_tracked_only` parity**. Apply the same
   ownership pattern to `git_tracked_only` (per-rule field
   that's similarly easy to forget on a new rule). Out of
   scope: this is a separate field with separate semantics.
   Decision noted but held for v0.9.11+.
4. **Phase L — tests + audit**. Add a compile-check test that
   asserts no rule directly references `ScopeFilter` (must go
   through `Scope`). Update `coverage_audit_*` files. Re-run
   the v0.9.9 regression suite — must pass without
   modification.
5. **Phase M — release v0.9.10** with prominent breaking-
   change migration note. Rust users of `alint-core` as a
   library see the signature change at compile time; the CLI
   and bundled rulesets are unaffected.
6. **Phase N — validation bench**. Run S1-S10 at all sizes;
   acceptance is ±5 % vs v0.9.9 (no perf change expected).

## Out of scope for v0.9.10

- Generalising the `ScopeFilter` shape beyond `has_ancestor`
  (separate work, tracked under v0.10).
- `git_tracked_only` ownership (see Phase K — held).
- `when:` ownership (different semantics; the eval-env shape
  doesn't share the silent-no-op shape).

## Open questions

1. **Should `Scope::matches` take `&FileIndex` by value or
   reference?** Leaning reference — the index is large
   (`Arc<...>` internally) and per-call clone is wasteful.
2. **Should the deprecated old `matches(&Path)` panic at
   runtime if `scope_filter` is set?** Yes — silently
   succeeding is exactly the bug class we're trying to
   eliminate. The deprecation message should make this
   explicit.
3. **Does `match_all()` belong on `Scope` or `Self::from_spec`
   handle the no-paths case?** Probably both — `match_all()`
   stays for direct construction (tests, etc.); `from_spec`
   internally uses it when `spec.paths` is `None`.

## Acceptance

- Every rule's `evaluate()` call site that touches a path
  threads `ctx.index` through `Scope::matches`.
- No rule struct holds a `scope_filter: Option<ScopeFilter>`
  field. A grep for that pattern in `crates/alint-rules/src/`
  returns zero matches.
- The v0.9.9 regression e2e + the 17 unit tests + the
  `scope_filter_integration.rs` integration test all pass
  without any test modification.
- Macro-bench S10 within ±5 % of v0.9.9.
