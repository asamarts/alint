# Cross-file value propagation (`sync_from` on `relation: equals`)

Status: **Phase 1 IMPLEMENTED** (2026-09-23; structured-extract targets). Extends
the shipped `sync_from` whole-file mirror (`relation: identical`) to the value
relation. Part of the v0.17 auto-fix arc, Phase 3. Companion:
`docs/design/v0.17/auto-fix-completion-plan.md`. Phase 2 (regex-extract targets)
remains.

## What it is

A `cross_file` `relation: equals` rule declares a source-of-truth value that
every target must match:

```yaml
- id: workspace-versions-coherent
  kind: cross_file
  relation: equals
  source:  { file: Cargo.toml, extract: { toml: "$.workspace.package.version" } }
  targets: { files: "crates/*/Cargo.toml", extract: { toml: "$.package.version" } }
  fix: { sync_from: {} }        # NEW: on `equals`, propagate the value
```

`check` already fires when a target's extracted value differs from the source's.
The fix rewrites each drifting target's node to the source's value, in place, per
format -- the "pick the source of truth, propagate to the N others via structured
edits" case (auto-fix.md 2, "Cross-file value propagation").

## Decisions

1. **Op & host.** No new op name: `sync_from` is already the `cross_file` fix op
   (auto-fix.md op table binds it to `identical` / `equals`). On `identical` it is
   the shipped whole-file mirror; on `equals` it is this value propagation.
2. **Value.** `equals` requires the source to extract EXACTLY one value (the
   rule already errors otherwise). Extraction yields a `String`, so the propagated
   value is `serde_json::Value::String(source_scalar)`.
3. **Per-target mechanism = reuse `StructuredFixer::set`.** Each target's
   `extract` is `Extract::Structured(Format, query)`, which carries the format and
   the `JSONPath` source. For the target file being fixed, the value fixer builds
   `StructuredFixer::set(fmt, JsonPath::parse(query), query, source_value, tier)`
   and delegates to its `collect_edits`. This reuses the located span resolver,
   the TOML/JSON whole-doc-CST `minimal_replace` path, the `serialize_scalar` /
   `set_value_is_statically_applicable` gates, and the `Structured` re-extract
   verifier -- so **all 8 structured formats (toml/json/yaml/xml/ini/hcl/dotenv/
   properties) come together**, with no per-format code here.
4. **Tier.** `Unsafe` by default, **content-injecting** (W2). Same posture as
   `sync_from`-identical: the ruleset's `source:` chooses which value overwrites
   which node, so an untrusted remote demotes it to a suggestion. Safe-promotable
   in the user's own top-level config. (A string propagated onto a typed
   number/bool node makes it a string node -- extracts-equal, so it converges,
   but it IS a type change, another reason Unsafe is correct.)
5. **Multi-file = best-effort per-file, loud reporting (5.2.4).** No new
   transaction machinery. `collect_edits` is per-file: the engine calls it once
   per drifting TARGET file; the value fixer reads the source (a cross-file read,
   like sync_from's) and emits that one target's located edit. Each target's edit
   verifies independently (PutGet). A target whose edit fails to verify demotes to
   a suggestion while the others apply -- exactly the best-effort-with-a-changed-
   files-list model auto-fix.md 5.2.4 deliberately chose over rollback.
6. **Verify.** `StructuredFixer`'s `Structured` verifier (re-parse, re-run the
   target query, `expect: Scalar(source_value)`), reused unchanged.

## Scope (answering "all formats at once, or a slice?")

- **Phase 1 (this increment): `Extract::Structured` targets on `equals`** -- all 8
  structured formats, via the `StructuredFixer` reuse above. This is the bulk of
  the value.
- **Phase 2 (follow-up): `Extract::Regex` targets** -- a located regex rewrite of
  the captured span, a different mechanism (mirrors the `replace` fixer, not
  `StructuredFixer::set`). Deferred so Phase 1 stays a clean reuse.
- **`Extract::WholeFile` on `equals`** is whole-content equality == `identical`
  semantics, already covered by `sync_from`-identical; the builder can steer a
  whole-file source/target to the identical path or reject it (decide in impl).
- **`Extract::Lines`** is a set relation, not a scalar `equals`; N/A.

## The fixer wiring -- a WHOLE-FILE `apply` fixer that reuses the located resolver

**Engine finding (verified in `engine.rs:1633-1656`): a `collect_edits`-based
located fixer must NOT host on `cross_file`.** The engine's located branch
assumes every located fixer hosts on a PER-FILE rule; it confines `--changed`
purely via the filtered index and never consults `writes_outside_changed`. A
located fixer on a `requires_full_index()` rule (cross_file) would, under
`--changed`, get the FULL index and splice edits into out-of-diff files with NO
blast-radius demote -- a silent escape (the code carries a `debug_assert!` and a
comment saying to "wire writes_outside_changed into the located branch before
shipping such a fixer"). That engine change is real and risky (the `--changed`
blast-radius class the arc has audited repeatedly), so we AVOID it.

**Instead the value fixer is a WHOLE-FILE `apply` fixer** (like the identical
`SyncFromFixer`), which reuses the located resolver INTERNALLY to compute the new
bytes and routes through the existing whole-file path -- which already demotes an
out-of-scope target via `writes_outside_changed` (engine.rs:1616). Its `apply`:

1. read the source, extract the source scalar (via the source `Extract`);
2. build `StructuredFixer::set(target_fmt, target_path, target_path_src,
   Value::String(source_scalar), tier)` for THIS target's format/path;
3. read the target bytes (`read_for_fix`, compose-aware, non-regular-guarded);
4. `edits = delegate.collect_edits(&[violation], file, &target_bytes, root)` --
   reuses the per-format locate + serialize (span vs TOML/JSON whole-doc CST) +
   the `Scalar(source)` verifier;
5. `(new_bytes, outcomes) = located_fix::apply_file_edits(&target_bytes,
   <wrap edits as LocatedEdit>, self.applicability())` -- reuses the splice +
   PutGet verify (a target node that cannot be set to the source value is demoted,
   never written);
6. if any edit `Applied` -> `commit_write(new_bytes)` (Applied); else Skip (the
   value couldn't be located/serialized/verified -- a clean decline, so `check`
   and `fix` agree).

So NO engine change, and locate/serialize/verify are all reused. Wiring:

- Widen `cross_file`'s field to `fixer: Option<Box<dyn Fixer>>` (identical ->
  `SyncFromFixer`; equals -> the new value fixer; both `apply`-based).
- `build()` picks by relation: `identical` -> `SyncFromFixer`; `equals` + every
  target extract is `Structured` -> the value fixer; a value relation with a
  `Regex`/`WholeFile`/mixed target extract -> reject at load (Phase 1 scope, with
  a message pointing at the Phase-2 follow-up).
- The value fixer carries the source file + source `Extract`, the target extract
  model (a single glob `Extract`, or per-entry list extracts), and the tier.

## Test seam (the "injectable-writer")

- Fixer units: per format, a drifting target's `collect_edits` yields the right
  `ReplaceRange` + `Scalar(source)` verifier; a source that extracts 0 or >1
  values yields no edit; a target already equal yields no edit (idempotence).
- e2e scenarios: a multi-target `equals` + `sync_from` fixture (2+ crates drifting
  from a root version) converges under `--unsafe-fixes` (`applied: [id]`, re-check
  clean); a bare `fix` suggests; one target that CANNOT converge (e.g. a
  non-scalar node) demotes to a suggestion while the others apply -- the
  best-effort partial-application gate (the "injectable-writer" analogue: a
  fixture that forces one target's edit to fail verify).
- Property net + coverage gates: `sync_from` is already in the net via the
  identical trigger; add an `equals` value-prop scenario to the fix-coverage
  corpus so the op's `equals` arm is exercised + proven convergent.

## Gate cascade (same as any fix-op extension)

No new `FixSpec` variant (still `sync_from`). Touch points: `cross_file` build
routing + the new fixer + the field widening; W2 already covers `sync_from`
(content-injecting); the fix-coverage e2e gains an `equals` scenario; docs
(this doc + auto-fix.md op-table note that `equals` is now live) + CHANGELOG. No
facts.json change (op count stays 20; `sync_from` already counted).

## Risk

Low-medium (de-risked). The whole-file-`apply` approach avoids the engine's
located-branch per-file constraint entirely, so there is NO engine change; the
per-format surface is zero (reused). What is new: the source-value-at-fix-time
plumbing, the `build()` routing, the field widening, and the internal
`collect_edits` + `apply_file_edits` reuse (verify the `LocatedEdit` wrapping and
that a demoted/declined target yields a clean Skip). Type-coercion (a string onto
a typed node) is the one semantic nuance; Unsafe tier + the `serialize_scalar`
gate cover it.
