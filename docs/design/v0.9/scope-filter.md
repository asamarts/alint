# scope_filter — closest-ancestor manifest scoping for per-file rules

Status: Implemented in v0.9.6 (`crates/alint-core/src/scope_filter.rs`,
engine integration at `crates/alint-core/src/engine.rs`
`run_per_file`). Per-file rule builders thread the filter onto
the built rule via `RuleSpec::parse_scope_filter` (`config.rs`)
so `Rule::scope_filter()` returns it back to the engine.

This is the design for a new per-file-rule scope filter that
makes ecosystem rule kinds (the bundled `rust@v1`, `node@v1`,
`python@v1`, `go@v1`, `java@v1` rulesets) correctly handle
polyglot monorepos with nested-package shapes — Rust under
`crates/`, Node under `packages/`, Python under `apps/`,
each with its own ecosystem manifest. Per-file rules in those
rulesets currently use a tree-global `when: facts.is_rust`
gate, which (a) misses Rust subprojects under non-Rust roots
because the bundled `is_rust` heuristic only matches the root
`Cargo.toml`, and (b) provides no per-file scoping when it
DOES fire — the rule applies tree-wide via its `paths:` glob.

The fix is a new `scope_filter:` rule-schema field whose
single primitive (`has_ancestor: <name>`) makes a rule apply
to a file only when at least one of the file's ancestor
directories (including its own directory) contains a file
matching the listed name. Composed with the existing
tree-level `when:` gate and per-file `paths:` glob, this
expresses "Rust per-file rules apply to files inside Rust
packages" in a single declarative line.

## Problem

Bundled `rust@v1` today:

```yaml
facts:
  - id: is_rust
    any_file_exists: [Cargo.toml]   # root-only literal — see v0.9.5 for why

rules:
  - id: rust-sources-final-newline
    when: facts.is_rust
    kind: final_newline
    paths: "**/*.rs"
    level: warning
```

Two real failure modes against polyglot repos:

**Failure 1 — Rust under non-Rust root.** Repo shape:

```
package.json                       # JS workspace root
packages/native-bindings/Cargo.toml   # Rust subpackage
packages/native-bindings/src/lib.rs
```

`is_rust` is false (no root `Cargo.toml`). Every rust@v1 rule
is gated off. The Rust subpackage gets zero hygiene treatment
even though the user explicitly extended `rust@v1`.

The trivially-broader `any_file_exists: ["Cargo.toml",
"**/Cargo.toml"]` heuristic fixes this case but introduces
Failure 2.

**Failure 2 — broader heuristic + tree-wide rules.** Repo
shape:

```
Cargo.toml                       # Rust workspace
crates/api/Cargo.toml            # Rust member
services/web/package.json        # Node service (no Rust)
services/web/src/index.ts
services/web/scripts/migrate.rs  # ← stray .rs that ISN'T a Rust source
```

With the broader `is_rust`, the rust@v1 rules fire. Every per-
file rule scoped to `**/*.rs` matches `services/web/scripts/
migrate.rs` — a file that has nothing to do with the Rust
ecosystem and shouldn't be governed by Rust hygiene rules.

The right semantics is: **Rust per-file rules apply to files
inside a Rust package** — i.e., files whose closest ancestor
manifest is `Cargo.toml`. That's what `scope_filter:
has_ancestor: Cargo.toml` will mean.

## Surface area

### Schema

A new optional field on every rule:

```yaml
- id: rust-sources-final-newline
  kind: final_newline
  paths: "**/*.rs"
  scope_filter:
    has_ancestor: Cargo.toml             # single literal filename
  level: warning

- id: python-sources-final-newline
  kind: final_newline
  paths: "**/*.py"
  scope_filter:
    has_ancestor: [pyproject.toml, setup.py, requirements.txt]   # any of N
  level: warning
```

**Field shape (YAML):**

```yaml
scope_filter:
  has_ancestor: <string | list-of-strings>
```

**Build-time validation:**

- `has_ancestor` value must be a string OR a non-empty list of strings.
- Each string must be a literal filename (no glob metacharacters: `* ? [ ] { } !`).
- Each string must not contain a path separator (no `foo/bar`; the lookup walks ancestor directories, so a separator-containing string is meaningless).
- The rule must be a per-file rule (`as_per_file().is_some()`). Cross-file rules with `scope_filter:` reject at build time with: *"scope_filter is supported on per-file rules only; cross-file rules express ancestor scoping via `for_each_dir + when_iter:`. Move the rule's logic into a `for_each_dir` block, or remove `scope_filter:`."*

**Default:** `scope_filter:` absent → no filter; the rule's existing `paths:` glob is the sole scope.

### Trait change

```rust
// alint-core/src/rule.rs

pub trait Rule: Send + Sync + std::fmt::Debug {
    // ... existing methods ...

    /// Per-file scope filter — see ScopeFilter docs for
    /// semantics. A file matches the rule iff its
    /// `path_scope` matches AND `scope_filter` (if any)
    /// returns true. Default `None` = no filter; existing
    /// rules don't change behaviour.
    ///
    /// Cross-file rules MUST return `None`; build-time
    /// validation rejects cross-file rules with a
    /// `scope_filter:` field on their spec.
    fn scope_filter(&self) -> Option<&ScopeFilter> {
        None
    }
}
```

The default implementation keeps every existing rule a
zero-cost null-check — engine measures this in Phase 5 to
prove no regression.

### Engine integration

In `Engine::run_per_file`'s applicable-rule filter:

```rust
let applicable: Vec<(usize, &RuleEntry)> = live
    .iter()
    .filter(|(_, entry)| {
        let pf = entry.rule.as_per_file().expect(...);
        if !pf.path_scope().matches(&file_entry.path) {
            return false;
        }
        if let Some(filter) = entry.rule.scope_filter() {
            if !filter.matches(&file_entry.path, per_file_ctx.index) {
                return false;
            }
        }
        true
    })
    .map(|(idx, entry)| (*idx, *entry))
    .collect();
```

The check happens BEFORE the file read, so scope_filter
saves the (potentially expensive) read cost, not just the
rule evaluation cost.

For cross-file rules: their build-path validation already
rejects `scope_filter:`, so the cross-file partition's
evaluation never needs to consult it.

### `ScopeFilter` evaluation

```rust
// alint-core/src/scope_filter.rs

pub struct ScopeFilter {
    has_ancestor: Vec<PathBuf>,
}

impl ScopeFilter {
    pub fn matches(&self, file: &Path, index: &FileIndex) -> bool {
        let mut cur = file.parent();
        loop {
            let dir = cur.unwrap_or(Path::new(""));
            for name in &self.has_ancestor {
                if index.contains_file(&dir.join(name)) {
                    return true;
                }
            }
            match cur {
                Some(p) if p.as_os_str().is_empty() => return false,
                Some(p) => cur = p.parent(),
                None => return false,
            }
        }
    }
}
```

Each `contains_file` is the v0.9.5 path-index hashlookup —
O(1). Per-file check is O(depth × M) where M = manifest
count. Typical: 5 levels × 1 manifest = 5 hashlookups
≈ 150 ns. At 1M files × 5 rules with scope_filter, total
overhead ≈ 750 ms — and that's with no caching.

Caching the per-file ancestor manifests is a v0.10
optimisation if benchmarks show it matters. v0.9.6 ships
the simple form.

### "File's own directory counts as ancestor"

For a file at `crates/api/src/main.rs`, the walk starts at
`crates/api/src/`, then `crates/api/`, then `crates/`,
then root. We look for `Cargo.toml` at each step.

For the file `crates/api/Cargo.toml` itself: the walk starts
at `crates/api/`, finds `crates/api/Cargo.toml` immediately,
and matches. **This is intentional** — the manifest's
directory IS the package, and files in that directory
(including the manifest itself) are part of the package.

For `Cargo.toml` at the repo root: walk starts at the root
(empty path), checks for `Cargo.toml`, matches. Same
intentional semantics.

**Pinned for the impl:** the `Path::parent()` walk includes
the file's parent (which IS the file's directory) as the
first step. Don't second-guess this.

## Semantics

For each (file, rule) pair where the rule has
`scope_filter: { has_ancestor: [...] }`:

1. The rule is considered for the file iff `paths:` matches.
2. If considered, walk `file.parent()` upward through every
   ancestor directory (including root, represented as the
   empty path).
3. At each directory `dir`, check if `index.contains_file(
   dir.join(name))` for any `name` in the `has_ancestor`
   list.
4. First match wins; the rule fires on the file.
5. No match across all ancestors → the rule does not fire
   on this file.

### Composition with other gates

Order of gating, top to bottom:

1. **Tree-level**: `when:` expression (facts + vars). Rule
   skipped entirely if false. v0.9.6 keeps this layer.
2. **Per-file path scope**: `paths:` glob. Standard glob
   match against the file's relative path.
3. **Per-file scope_filter**: `has_ancestor` walk. NEW.
4. **Per-file git filter**: `git_tracked_only` consult of
   `ctx.git_tracked`. Existing.
5. **Rule-specific evaluate body**: the rule actually fires.

Each gate is an AND. A rule fires on a file iff ALL its
gates accept the file.

### Why first-match-wins, not closest-match

`scope_filter: { has_ancestor: <name> }` is a boolean filter.
It returns true / false; the path of the matching manifest
is not exposed. Whether we walked one level or five to find
it doesn't matter for the boolean answer.

A future `closest_ancestor:` could expose the matching path
for templated `paths:` substitution. Out of scope for v0.9.6;
land it iff a real use case appears.

## False-positive surface

What could go wrong with this scoping primitive:

### A vendored Cargo.toml under `vendor/`

Repo shape:

```
Cargo.toml                                    # main Rust workspace
vendor/embedded-rust-tool/Cargo.toml          # vendored copy
vendor/embedded-rust-tool/src/lib.rs
```

With `scope_filter: has_ancestor: Cargo.toml`, every `.rs`
under `vendor/embedded-rust-tool/` matches because of the
nested manifest, AND every `.rs` under any other crate
matches because of the root manifest.

This is the SAME false-positive surface the existing `paths:
"**/*.rs"` already has. scope_filter doesn't make it worse.
Users can override:

```yaml
paths:
  include: ["**/*.rs"]
  exclude: ["vendor/**", "third_party/**"]
```

scope_filter has nothing to add to the exclude story; it's
purely about ecosystem detection.

### Manifests with the same name across ecosystems

`pom.xml` is unambiguous. `package.json` is JS only.
`Cargo.toml` is Rust only. No cross-ecosystem name collisions
in practice. Each bundled ruleset's `has_ancestor` list is
disjoint from the others'.

### Stray manifest at random depth

Repo shape:

```
README.md
docs/examples/legacy-app/package.json   # decorative example
docs/examples/legacy-app/src/index.ts
```

If we extend `node@v1`, the example `index.ts` matches via the
nested `package.json`. Probably not the user's intent.

This is the SAME concern any naive ecosystem-detection has.
Mitigations users can apply:

```yaml
# In the user's own config, post-extending node@v1:
overrides:
  - id: node-sources-no-trailing-whitespace
    paths:
      include: ["**/*.ts", "**/*.js"]
      exclude: ["docs/**"]
```

The bundled rulesets ship with reasonable defaults; users
narrow them with their own `overrides:` block when needed.

### Symlinks pointing across ancestor boundaries

The walker resolves symlinks based on `WalkOptions::follow_links`.
If `crates/api/` is a symlink to `vendor/foo/` (which has its
own `Cargo.toml`), the symlinked-into path's ancestors are
the SYMLINK target's ancestors as recorded by the walker.

Edge case unlikely to surface in practice; document as
"behaves as if the symlinked path were the real path" and
move on.

### root_only rules

`file_exists` with `root_only: true` already constrains the
search to root-component paths. Adding `scope_filter:
has_ancestor: Cargo.toml` to a `root_only` rule is
redundant — root files have only the root as ancestor;
checking for `Cargo.toml` there is the same as
`facts.is_rust` (where `is_rust` is the root-only literal).

Build-time warning: `root_only: true` + `scope_filter:` is
redundant; consider removing one. **Not** a hard error —
might be intentional in edge cases.

## Implementation notes

**Crate location:** `alint-core::scope_filter` (new module),
re-exported as `alint_core::ScopeFilter`. Mirrors the
`alint_core::Scope` shape (separate module, public type,
re-exported at crate root).

**Dependencies:** none beyond what `alint-core` already
imports. Re-uses `FileIndex::contains_file` from v0.9.5 so
no new walker pass / no new lazy field.

**Per-rule wiring:** every rule's `build()` function reads
`spec.scope_filter`, validates against the per-file /
cross-file constraint, stores the parsed `ScopeFilter` on
the rule struct, returns it via the new `Rule::scope_filter`
override. Mechanical change across ~30 per-file rule files.
Cross-file rules' `build()` functions add a single
validation: reject if `spec.scope_filter.is_some()`.

**Default trait method:** `Rule::scope_filter` returns
`None` by default. Cross-file rules and existing per-file
rules that haven't migrated yet keep the default. The engine
short-circuits on `None`, so the per-file dispatch hot path
is unchanged for the default case.

**Schema validation:** `crates/alint-dsl/src/lib.rs`
`schemas/v1/config.json` adds the `scope_filter` field
spec; existing rules without it stay green.

## Tests

### Unit (in `scope_filter.rs`)

```text
- root manifest matches root file
- root manifest matches nested file
- nested manifest matches own dir
- nested manifest matches descendant
- no manifest in any ancestor → false
- two-name list matches if either is found
- empty manifest list → reject at parse (build-time error)
- glob in name → reject at parse
- path separator in name → reject at parse
```

### Integration — engine path

Under `crates/alint-core/src/engine.rs::tests`:

```text
- scope_filter on per-file rule scopes correctly to ancestor-having files
- scope_filter on cross-file rule rejects at build time
- scope_filter null-default matches every file path_scope matches
```

### e2e — new family `crates/alint-e2e/scenarios/check/scope_filter/`

```text
- has_ancestor_root_manifest_scopes_root_files_pass.yml
- has_ancestor_root_manifest_scopes_root_files_fires.yml
- has_ancestor_nested_manifest_scopes_subtree_pass.yml
- has_ancestor_nested_manifest_scopes_subtree_fires.yml
- has_ancestor_no_manifest_silent.yml
- has_ancestor_two_name_list.yml
- has_ancestor_with_paths_glob_intersection.yml   # paths AND scope_filter both required
- has_ancestor_polyglot_monorepo_pass.yml         # ★ headline scenario
- has_ancestor_rejects_glob_at_build_time.yml
- has_ancestor_rejects_separator_at_build_time.yml
- has_ancestor_rejects_on_cross_file_rule.yml
```

The polyglot scenario IS the integration test:

```yaml
# scenarios/check/scope_filter/has_ancestor_polyglot_monorepo_pass.yml
name: scope_filter has_ancestor correctly scopes ecosystem rules in a polyglot monorepo
tags: [check, scope_filter, polyglot, passing]

given:
  tree:
    crates:
      api:
        Cargo.toml: |
          [package]
          name = "api"
          version = "0.1.0"
        src:
          main.rs: "fn main() {}\n"   # well-formed Rust
    services:
      web:
        package.json: '{"name": "@demo/web"}'
        src:
          index.ts: "export default 1\n"   # well-formed TS
      data-tool:
        pyproject.toml: |
          [project]
          name = "data-tool"
        src:
          main.py: "print('hi')\n"   # well-formed Python
  config: |
    version: 1
    rules:
      - id: rust-final-newline
        kind: final_newline
        paths: "**/*.rs"
        scope_filter:
          has_ancestor: Cargo.toml
        level: error
      - id: ts-final-newline
        kind: final_newline
        paths: ["**/*.ts"]
        scope_filter:
          has_ancestor: package.json
        level: error
      - id: py-final-newline
        kind: final_newline
        paths: ["**/*.py"]
        scope_filter:
          has_ancestor: pyproject.toml
        level: error

when: [check]

expect:
  - violations: []
```

Plus a parallel `_fires.yml` that drops a final newline from
ONE language's source and asserts only that language's rule
fires.

### Coverage audit — new

`coverage_audit_scope_filter.rs` (new): every rule that uses
`scope_filter:` must have at least one e2e scenario where
the filter excludes the file (silent path) AND at least one
where it includes (firing path). Lands `#[ignore]` until
Phase 5 fills the bundled-ruleset scenarios; comes off the
ignore list as the migration proceeds.

### Regression — every existing test

The full workspace test suite (1000+ tests) must pass
unchanged through every commit in the implementation chain.
Specifically:

- All 221 e2e scenarios green by default.
- All four existing coverage audits green by default.
- Cross-platform (Linux / macOS / Windows) all green.
- `cargo doc --no-deps --workspace` clean with `RUSTDOCFLAGS=-D warnings`.

## Bundled-ruleset migration

After the engine + tests land, migrate the five ecosystem
rulesets:

| Ruleset | Manifest names |
|---|---|
| `rust@v1` | `Cargo.toml` |
| `node@v1` | `package.json` |
| `python@v1` | `[pyproject.toml, setup.py, requirements.txt]` |
| `go@v1` | `go.mod` |
| `java@v1` | `[pom.xml, build.gradle, build.gradle.kts]` |

Each ruleset's per-file rules add `scope_filter: { has_ancestor:
<list> }`. The `is_*` fact gate stays as-is for the tree-level
short-circuit (avoids running the rule loop entirely on repos
with no Rust anywhere); the new scope_filter handles per-file
scoping when the rule does run.

**Naming change:** rename `is_rust` → `has_rust` (and
analogously `is_node` / `is_python` / `is_go` / `is_java` →
`has_*`). The new name reads better with the broadened
heuristic — `is_rust` implied "this IS a Rust project,"
`has_rust` correctly says "this repo HAS some Rust." Per
project decision: no backwards-compat alias period; rename
is hard.

**Heuristic broadening:** each `has_*` fact's `any_file_exists`
list expands from root-only to `[<manifest>, **/<manifest>]`
to catch nested ecosystems. Combined with scope_filter,
false-positive surface is bounded.

Cross-file rules (`for_each_dir`, etc.) in these rulesets
stay unchanged — they already express their scoping via
`select:` + `when_iter:`.

Rulesets NOT touched:

- `oss-baseline@v1` — rules apply tree-wide by design (LICENSE
  must exist, README must exist; not ecosystem-scoped).
- `monorepo@v1` and submodule variants — already use
  `for_each_dir`.
- `agent-context@v1` / `agent-hygiene@v1` / `compliance/*` /
  `ci/github-actions@v1` / `docs/adr@v1` / `hygiene/*` /
  `tooling/editorconfig@v1` — none ecosystem-specific.

## Benchmarks

Three runs per Phase 5 of the v0.9.6 plan:

1. **Run 1** — pre-implementation baseline. Captures v0.9.5
   floor for cross-version comparison.
2. **Run 2** — after engine change, before bundled-ruleset
   migration. Must be flat (±5 %) on every existing bench;
   the null-default `Rule::scope_filter` should be
   unmeasurable.
3. **Run 3** — after bundled-ruleset migration. S1–S7
   expected flat; new S9 nested-polyglot scenario captures
   the perf shape scope_filter is designed for.

Gating: `xtask bench-compare --threshold 10` against the
v0.9.5 floor at each step. Anything red pauses the chain
for investigation.

### New scenario: S9 — nested polyglot monorepo

`xtask/src/bench/scenarios/s9_nested_polyglot.yml`:

```yaml
extends:
  - alint://bundled/rust@v1
  - alint://bundled/node@v1
  - alint://bundled/python@v1
```

Tree shape: 3 ecosystems × 1000 packages × 100 files = 300k
files distributed under `crates/`, `packages/`, `apps/`. Each
ecosystem's per-file rules apply only to its subtree via
`scope_filter`. Captures the perf shape on the workload the
feature is designed for.

`xtask::Scenario` enum gets `S9`. New helper
`alint_bench::tree::generate_polyglot_monorepo(per_ecosystem,
files_per_package, seed)`.

## Open questions

1. **Symlink semantics.** Should `Path::parent()` walk
   resolve symlinks or use the literal path components? My
   vote: literal components — that's how `paths:` globs
   work today (relative to the walker-recorded path), and
   consistency wins. Document as such.

2. **Empty list of manifests.** `has_ancestor: []` is
   meaningless. Reject at parse time. (Already in the
   build-time validation list above.)

3. **Should `scope_filter:` accept other shapes in v0.9.6?**
   E.g., `has_ancestor_with_content: { name: Cargo.toml,
   matches: '\[workspace\]' }` to scope only to workspace
   roots, not member crates. My vote: NO for v0.9.6 — keep
   the primitive minimal. Composability concerns (matchers
   inside scope_filter inside rule) explode quickly. Re-visit
   in v0.10+ if a real use case appears.

4. **Per-OS path separator.** The `has_ancestor` name "Cargo.toml"
   needs to NOT contain a separator. On Windows, the relative
   path inside FileIndex uses `\` separators. Internally we
   normalise to `/` for glob matching; the same normalisation
   applies here. **Pinned: enforce no separator (forward or
   backward) in the parsed `has_ancestor` strings.**

5. **Should the rule's own `paths:` exclude apply BEFORE
   scope_filter?** Yes — `paths.exclude` is part of `paths:`
   evaluation; scope_filter runs after. Order is paths →
   scope_filter → git_tracked_only.

## Out of scope for v0.9.6

- **`closest_ancestor:`-as-path-template.** Exposing the
  matching ancestor's path for templated `paths:`
  substitution. Useful for "every Rust package must have a
  README.md *next to its Cargo.toml*" rules. Defer.
- **Scoping cross-file rules.** As discussed above; cross-
  file rules use `for_each_dir + when_iter:` instead.
- **Caching per-file ancestor sets.** Profile-driven; only
  add if Phase 5 Run 2 shows scope_filter overhead > 5 %.
- **Globs in `has_ancestor`.** Literal filenames only.

## Summary

A single new schema field, ~300 LoC of engine code, ~30
mechanical rule-build changes, ~10 new e2e scenarios, plus
the bundled-ruleset migration. Composes cleanly with every
existing gate (`when:` / `paths:` / `git_tracked_only`).
Backward-incompatible only at the bundled-ruleset level
(`is_*` → `has_*` rename); no behaviour change for
user-authored rules unless they opt in via `scope_filter:`.

Phase 5 benchmarks gate against v0.9.5 with no perf
regression tolerance > 5 %. The polyglot-monorepo case the
feature is designed for is captured in a new bench scenario
(S9) with its own published per-version snapshot.

## Implementation plan

11 commits, in order:

1. `docs(design): scope-filter design pass for v0.9.6` (this doc)
2. `feat(core): scope_filter primitive + has_ancestor` — engine + scope_filter module + unit tests + per-rule trait method default
3. `bench: capture v0.9.5 floor for v0.9.6 gating` — Run 1 baseline
4. `feat(rules): rust@v1 — has_rust + scope_filter`
5. `feat(rules): node@v1 — has_node + scope_filter`
6. `feat(rules): python@v1 — has_python + scope_filter`
7. `feat(rules): go@v1 — has_go + scope_filter`
8. `feat(rules): java@v1 — has_java + scope_filter`
9. `test(bench): add S9 nested polyglot monorepo`
10. `docs: rule-authoring + bench docs for v0.9.6`
11. `chore(release): bump workspace to 0.9.6`

Each commit is independently reviewable + revertable. The
engine-only commit (#2) is the keystone — if Phase 5 Run 2
shows perf regression on it alone, the bundled migration
doesn't proceed and the engine commit is reworked.

## Post-v0.9.6 follow-ups

The v0.9.6 design landed the primitive but not full
coverage. Three silent-no-op classes surfaced afterwards;
each was fixed in a follow-up patch release.

### v0.9.7 — per-file content rule sweep (25 rules)

The per-file content rules — `no_trailing_whitespace`,
`final_newline`, `line_endings`, `no_bom`,
`file_content_forbidden`, `file_content_matches`,
`file_max_lines`, `file_min_lines`, `line_max_width`,
`indent_style`, `commented_out_code`,
`max_consecutive_blank_lines`, `markdown_paths_resolve`,
`no_bidi_controls`, `no_merge_conflict_markers`,
`no_zero_width_chars`, `file_starts_with`, `file_ends_with`,
`file_header`, `file_footer`, `file_hash`, `file_is_text`,
`file_is_ascii`, `file_shebang`, `structured_path` — each
shipped without parsing `spec.scope_filter`, so the gate
silently dropped on the way through `build()`. v0.9.7 wired
each rule through `parse_scope_filter()` with a uniform
runtime check inside `evaluate()` and an
`impl Rule::scope_filter()` override.

### v0.9.9 — non-PerFileRule rule sweep (17 rules) + bypass guard

Two related gaps closed in v0.9.9:

**Bug #1 — 17 rules bypass the engine's per-file dispatch**
and iterate `ctx.index.files()` directly. They never
consulted `scope_filter`. The fix mirrors v0.9.7's pattern
across `file_max_size`, `file_min_size`, `no_empty_files`,
`executable_bit`, `executable_has_shebang`,
`shebang_has_executable`, `no_symlinks`, `filename_case`,
`filename_regex`, `no_illegal_windows_names`,
`max_files_per_directory`, `max_directory_depth`,
`json_schema_passes`, `command`, `git_blame_age`,
`no_case_conflicts`. One rule (`no_submodules`) is
hardcoded to inspect `.gitmodules` at the repo root and
does not iterate the index — it rejects `scope_filter` at
build time via the new
`reject_scope_filter_with_reason(spec, kind, reason)`
helper. Macro bench scenario S10 covers 5 of these rules
with `scope_filter` narrowing across the polyglot tree.

**Bug #2 — `for_each_dir` literal-path bypass.** The v0.9.8
fast path that dispatches a nested per-file rule directly
via `PerFileRule::evaluate_file` (instead of falling through
to `Rule::evaluate`) skipped the `scope_filter` check. A
nested rule whose spec carried `scope_filter:` would have
the bypass execute regardless of the filter — divergent from
the rule-major fallback. The guard at
`crates/alint-rules/src/for_each_dir.rs::evaluate_for_each`
now consults `nested_rule.scope_filter()` before taking the
bypass. E2e regression at
`crates/alint-e2e/scenarios/check/scope_filter/scope_filter_nested_under_for_each_dir.yml`.

### Why this kept happening + the v0.9.10 plan

The shape of all three bugs is identical: a rule (or dispatch
site) holds a `Scope` and a separate `Option<ScopeFilter>`
field, and forgets to consult the filter. The compiler can't
catch the omission because nothing wires the two together.

v0.9.10 is the structural fix: refactor `Scope` to own the
optional `ScopeFilter`, change `Scope::matches(&Path)` to
`Scope::matches(&Path, &FileIndex)`, and add a
`Scope::from_spec(spec)` constructor. Every call site is
forced to thread the index, and `scope_filter` is honoured
automatically. Tracked under the v0.9.10 milestone.
