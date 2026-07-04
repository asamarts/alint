# Design: `git_tracked_only` and `respect_gitignore` as kind-specific options

Status: **Draft** (2026-07-04).
Decisions: [ADR-0008](../../adr/0008-git-tracked-only-is-a-kind-option.md).

This is the type-level design + implementation plan for ADR-0008. The ADR records
*what* and *why*; this records *how*, precisely, so the refactor is clean at the
type level.

## 1. Problem

`RuleSpec` (crates/alint-core/src/config.rs) carries `git_tracked_only: bool` and
`respect_gitignore: Option<bool>` as explicit fields, so serde consumes them
before the per-kind remainder lands in `RuleSpec.extra` (the `#[serde(flatten)]`
mapping each kind's `Options` is deserialized from). Consequences (see ADR-0008):
the loader silently accepts `git_tracked_only` on any kind (only the four
existence kinds read it), the schema lists it only on existence branches (so
they diverge), and the existence kinds cannot be schemars-migrated because the
field is not in their `Options`.

## 2. Type-level design

The engine already has the right abstraction: it never reads
`spec.git_tracked_only`; it reads the `Rule::git_tracked_mode()` **trait method**
(default `GitTrackedMode::Off`; existence kinds override). So *where the flag is
stored* is purely a loader/deserialization concern, and we can move it without
touching the engine. The design is: store each flag where its semantics place it.

### 2.1 `git_tracked_only` -> the existence `Options`

Remove `pub git_tracked_only: bool` from `RuleSpec`. Add it to each existence
kind's `Options`:

```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Options {
    /// Restrict matches to files tracked in git's index (`git ls-files`).
    /// Entries present in the walked tree but not tracked are skipped. No
    /// effect outside a git repo. Default `false`.
    #[serde(default)]
    root_only: bool,               // (file_exists: no x-since; siblings: x-since 0.14)
    /// (doc as above)
    #[serde(default)]
    git_tracked_only: bool,
}
```

`build()` reads `opts.git_tracked_only` instead of `spec.git_tracked_only`; the
rule struct field and its `git_tracked_mode()` override are unchanged.

Mechanism that makes this correct with zero new validation code: `git_tracked_only`
now arrives in `RuleSpec.extra`, so `deserialize_options()` feeds it to the kind's
`Options`. An existence `Options` has the field, so it deserializes. Every other
kind's `Options` has `#[serde(deny_unknown_fields)]`, so it **rejects**
`git_tracked_only` with a clean load error - uniformly, no per-kind checks. That
is the fail-loud the `RuleSpec` doc-comment promised but never implemented.

Note on the `x-since` asymmetry: `file_exists.root_only` is released (no
`x-since`); the three siblings' `root_only` is v0.14 (`x-since`). So the four
`Options` structs cannot share one type yet (a shared struct can't carry a
per-kind `x-since`); keep them separate now. Once v0.14 releases and the `x-since`
markers are dropped, unifying the existence `Options` into one shared struct is a
clean follow-up. The `x-since` is expressed type-side as
`#[schemars(extend("x-since" = "0.14"))]` on the three siblings' `root_only` field
(the released `file_exists.root_only` carries no such attribute).

### 2.2 `respect_gitignore` -> `file_exists`'s `Options`

`respect_gitignore` is honored **only by `file_exists`** (the pitfall-#18
literal-path escape hatch); no sibling existence kind and no other kind reads the
per-rule field. So it is treated exactly like `git_tracked_only`, just narrower:
remove `pub respect_gitignore: Option<bool>` from `RuleSpec`, add it to
`file_exists`'s `Options` alone, and have `build()` read `opts.respect_gitignore`.
It is dropped from `rule_common` and its orphaned `$def` is deleted. The
workspace-level walker field `Config.respect_gitignore` (config.rs / walker.rs /
nested.rs) is a *separate* field and is unchanged. Rejecting `respect_gitignore` on
any other kind falls out of the same `deny_unknown_fields` mechanism as 2.1 - no
extra code.

An earlier draft moved `respect_gitignore` to `rule_common` instead (universal
permit, "forward-compatible"). An adversarial review refuted it: that reintroduces
the schema-looser-than-engine fail-quietly - `no_bom: {respect_gitignore: false}`
validated, loaded, and was silently ignored - and it dropped the field's
Options-table row from the `file_exists` rule page (common fields are not rendered
per-rule). See ADR-0008 Considered Options.

### 2.3 schemars migration of the four existence kinds

Add `#[derive(schemars::JsonSchema)]` + field doc-comments + `crate::
options_schema_for!(Options);` to each existence module, and register
`("rule_file_exists", file_exists::options_schema())` (and the three siblings) in
`migrated_option_schemas()` (lib.rs). `compose_branch` then replaces the
hand-authored existence branches with `{kind, paths}` + the derived `{root_only,
git_tracked_only}`. Effects: `x-since` becomes type-derived (2.1); the empty
Options-table descriptions fill from the Rust `///` docs; `file_exists` also gains
a derived `respect_gitignore` (2.2), which restores its Options-table row.

## 3. Change surface

- `crates/alint-core/src/config.rs`: remove **both** `RuleSpec.git_tracked_only`
  and `RuleSpec.respect_gitignore` (+ their doc-comments); keep the nested-strip in
  `PARENT_FIELDS`. Update the config test at ~:991. (The workspace
  `Config.respect_gitignore` walker field stays.)
- `crates/alint-rules/src/{file_exists,file_absent,dir_exists,dir_absent}.rs`:
  add `git_tracked_only` to `Options` (+ `root_only` doc), `#[derive(JsonSchema)]`,
  `options_schema_for!`, read `opts.git_tracked_only`, keep the `x-since` attr on
  the three siblings' `root_only`. In `file_exists` **only**, also add
  `respect_gitignore` to `Options` and read `opts.respect_gitignore`.
- `crates/alint-rules/src/lib.rs`: register the four in `migrated_option_schemas()`.
- `schemas/v1/config.json` (+ in-crate copy): remove `respect_gitignore` from
  `rule_common` and delete the orphaned `per_rule_respect_gitignore` `$def`, then
  regenerate; the existence `x-since` becomes derived and `file_exists` gains a
  derived `respect_gitignore`.
- Tests: see below.

## 4. Tests

- **New (the point of the refactor):** a non-existence kind with `git_tracked_only`
  now fails to load with a clear error. Add a loader test asserting e.g. a
  `file_content_forbidden` rule with `git_tracked_only: true` is rejected.
- **Unchanged behavior:** the existing existence-kind `git_tracked_only` tests
  (build + evaluate) must still pass reading it from `opts`.
- **Schema fidelity:** `gen-schema --check` + the `generated_and_committed_agree_*`
  tests green after regen; the four existence branches now schemars-derived.
- **`respect_gitignore`:** rejected off `file_exists` - a non-existence kind
  (`executable_bit`) and a sibling existence kind (`file_absent`) with
  `respect_gitignore` both fail to load; `file_exists` still honors it.
- Full gate sweep: workspace test, clippy, docs-export --check, dogfood.

## 5. Compatibility

A config that set `git_tracked_only` on a non-existence kind (a silent no-op
before) now errors at load. This is the intended fail-loud for the v0.14 cut;
record it under CHANGELOG **Changed** (a stricter load, catching a real mistake).
No valid config changes behavior: on the existence kinds the flag is read from the
same YAML key, just deserialized via `Options` instead of `RuleSpec`.

## 6. Open questions

- Unify the existence `Options` post-v0.14 (once the `x-since` asymmetry is gone)?
  Only the three siblings (`file_absent`/`dir_exists`/`dir_absent`) can share a
  struct: `file_exists` carries an extra `respect_gitignore` field the others do
  not honor, so it stays distinct. Tracked as a follow-up, not done here.
