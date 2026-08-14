# Manifest-derived rule scope — narrowing a per-file rule by a manifest's declared paths

Status: Draft — spec resolved 2026-08-13 (see §7), implementing for v0.15. (Draft | Implemented in <commit> | Superseded by <doc>.)
Decisions: [ADR-0010](../../adr/0010-manifest-derived-rule-scope.md) (manifest-derived rule scope stays on the scope_filter seam)
Demand evidence: the "repo-shape vs manifest" reply drafted in the alint.org marketing tree (`marketing/reddit/drafts/2026-08-reply-repo-shape-vs-manifest.md`); the in-tree `paths.exclude` drift in [`../v0.12/asf_bundle_overfire.md`](../v0.12/asf_bundle_overfire.md).

<!--
SEED DOC. This captures the starting position for a deferred deep-dive, not a finished
spec. Sections 1-6 record where the design is; section 7 leads with the crux that must be
resolved before any code. ADR-0010 fixes the boundary (manifest values scope, never
decide); this doc works out the mechanism inside that boundary.
-->

## 1. Problem

Two distinct operations hide in one recurring request, "make the linter read the manifest":

- **(A) Infer intent from code.** Is this `console.log` dev-guarded? Is this file really an
  entrypoint? A content regex cannot answer this honestly. This is the failure a user hit
  dogfooding their own linter: a `no-console` rule fired 19 times on a Vite app, all 19
  wrong (13 inside `import.meta.env.DEV`, 6 in `scripts/`). alint deliberately does not do
  (A), and this doc does not change that.
- **(B) Derive a path scope from a manifest-declared path set.** "Run `no-console` on
  `src/**` except the entrypoints `package.json` `bin` declares." This is deterministic and
  declarative. alint does not do (B) today, and (B) is what this doc is about.

Today (B) has two workarounds, both unsatisfying:

1. A hand-written `paths.exclude` that lists the entrypoints. It works until someone adds a
   `bin` entry and forgets the exclude; then it silently drifts.
   [`../v0.12/asf_bundle_overfire.md`](../v0.12/asf_bundle_overfire.md) records our own
   bundled rulesets independently re-deriving exactly this workaround.
2. A separate `cross_file` rule asserting the exclude list equals `bin`. This keeps the two
   honest but is a second artifact to maintain, and it validates drift rather than
   preventing it at the point of use.

What stays unsolved without this feature: there is no way to say "this rule's scope comes
from that manifest" in one place, so the manifest that owns the truth and the rule that
depends on it are always two hand-synced artifacts.

## 2. Surface area

The proposed shape generalises `scope_filter` (see [`../v0.9/scope-filter.md`](../v0.9/scope-filter.md)),
which already gates per-file rules by `has_ancestor:` (manifest presence) and
`changed_since:` (git diff). We add a predicate that gates by membership in a
manifest-derived path set.

Before (today):

```yaml
# hand-written exclude; drifts from bin
- id: no-stray-console
  kind: file_content_forbidden
  paths: { include: "src/**/*.{ts,tsx}", exclude: "src/cli.ts" }
  pattern: 'console\.(log|debug|info)\('
  level: error
# ...plus a separate registry_paths_resolve or cross_file rule to keep the exclude honest
```

After (proposed):

```yaml
- id: no-stray-console
  kind: file_content_forbidden
  paths: "src/**/*.{ts,tsx}"
  scope_filter:
    exclude_manifest_paths:            # new sibling of has_ancestor / changed_since
      source: package.json             # the manifest (confined to repo root)
      extract: { json: "$.bin.*" }     # the extract one-of from registry_paths_resolve
      # derive_target: { from: '^dist/(.*)\.js$', to: 'src/$1.ts' }   # optional: map output -> source
  pattern: 'console\.(log|debug|info)\('
  level: error
```

Keys (defaults are in Open questions, not yet fixed):

- `source:` — the manifest path, repo-root-confined (ADR-0004). `allow_out_of_root:`
  interplay TBD.
- `extract:` — the shared one-of: `{ toml | json | yaml: <JSONPath> }`, `{ lines: ... }`, or
  `{ regex: <capture> }`. Non-literal / interpolated entries are dropped, not failed
  (existing extract convention).
- `derive_target:` (optional) — `{ from: <regex on the extracted path>, to: <template> }`,
  reused verbatim from `file_graph` ([`../v0.12/file_dependency_graph.md`](../v0.12/file_dependency_graph.md)),
  to map a declared build-output path back to a source path.
- an `include_manifest_paths:` counterpart (keep only files in the set) is wanted too; see
  Open questions on whether to ship two predicates or one with a mode.

Alternative shape (the main fork, deferred to the deep-dive): a top-level named `path_sets:`
block that any rule references from `paths.include` / `paths.exclude`:

```yaml
path_sets:
  entrypoints: { source: package.json, extract: { json: "$.bin.*" } }
rules:
  - id: no-stray-console
    kind: file_content_forbidden
    paths: { include: "src/**", exclude: { path_set: entrypoints } }
    # ...
```

More reusable (one set, many rules, include or exclude) but a new top-level concept and a
larger schema surface. ADR-0010 fixes the boundary for both; this doc picks between them.

## 3. Semantics

- **Dispatch.** Per-file only. `scope_filter` is already rejected on cross-file rules at
  build time; the manifest-path predicate inherits that.
- **Resolve once.** The path set is extracted, optionally `derive_target`-mapped, resolved
  relative to the manifest's directory, and cached once per run, exactly as `changed_since:`
  resolves its diff once ([`../v0.11/scope_filter_changed_since.md`](../v0.11/scope_filter_changed_since.md)).
  This keeps determinism (ADR-0003): same tree, same set, stable order.
- **Per-file gate.** A candidate file (one the base `paths:` glob already matched) is kept
  iff every `scope_filter` predicate passes. `exclude_manifest_paths` fails a file whose
  resolved path is in the set; `include_manifest_paths` fails a file not in the set.
  Predicates AND-compose with `has_ancestor:` / `changed_since:` (the existing rule: at least
  one predicate present, all must pass).
- **No spawn.** Extraction is pure-parse. Safe inside `extends:`'d rulesets; no new trust
  gate (contrast `custom:` facts and the spawning kinds).
- **`--changed`.** Orthogonal: `--changed` restricts the visited set to the diff; the
  manifest scope intersects with it. Both resolve once.
- **`fix`.** Autofix targets only in-scope files, so the derived scope narrows fixes too.
- **`baseline`.** Downstream: narrowing scope changes which findings exist, and baseline
  fingerprints those findings as usual. No special handling anticipated (confirm in the
  deep-dive).

## 4. False-positive surface

Mandatory section. What fires wrongly, and the mitigations.

- **Source-versus-output mismatch (the crux).** `package.json` `bin` usually names build
  output (`dist/cli.js`), not source (`src/cli.ts`). A naive `exclude_manifest_paths` over
  `$.bin.*` against a `src/**` rule matches nothing, so it silently fails to exempt the
  entrypoint — a false *negative* for the exclusion, which reads as the original false
  positive persisting. Mitigation: `derive_target` maps output back to source; and the doc
  must state plainly that the clean cases are source-declaring manifests (`Cargo.toml`
  `workspace.members`, composer PSR-4 autoload, `tsconfig` references, `go.work`,
  `pnpm-workspace` packages) and that `bin` needs the mapping step.
- **Empty extract.** If the JSONPath matches nothing, the set is empty.
  `exclude_manifest_paths` then excludes nothing (safe, the rule runs full-scope) but
  `include_manifest_paths` includes nothing (the rule silently no-ops — a footgun). Needs an
  `expect_nonempty` guard or a warn.
- **Over-broad `derive_target`.** A loose `from:` regex can map to the wrong sibling.
  Mitigation: the same drop-non-literal discipline as `file_graph`, plus `explain` showing
  the mapped pairs.
- **Manifest absent.** Follow `has_ancestor:` and scope nothing silently, or error? Silent
  matches the sibling predicate; a missing manifest a rule depends on may deserve a warn.
  Open.
- **Legibility.** You can no longer read `paths:` and know the scope. Mitigation: the
  contribution is confined to an explicit `scope_filter` block (never smuggled into
  `paths:`), and `alint explain <rule>` prints the resolved set with provenance.

## 5. Implementation notes

- **Module.** Extend `ScopeFilter` in `crates/alint-core/src/scope_filter.rs`; engine
  integration is already threaded through `run_per_file` (`engine.rs`) and the per-file rule
  builders (`RuleSpec::parse_scope_filter` in `config.rs`).
- **Reuse, do not reinvent.** The extract one-of is already factored for
  `registry_paths_resolve` and `file_graph.from_content`; `derive_target` is already in
  `file_graph`. The new code is a predicate variant plus a resolve-once cache plus `explain`
  output.
- **No new dependencies.** Pure-parse; cargo-deny unaffected.
- **Constitution.** Determinism (resolve once, stable order); no silent caps (log dropped
  non-literal / unmapped entries); per-file dispatch parity. See [`../constitution.md`](../constitution.md).
- **Complexity.** Small-to-moderate: the primitives exist; the work is composition, the
  cache, the schema, and `explain`.

## 6. Tests

- **Firing.** Source-declaring manifest (`Cargo.toml` `workspace.members`) + a per-file
  content rule; a file inside a declared member is excluded (or, for the include variant, is
  the only thing kept).
- **Silent.** Manifest absent → the rule scopes nothing and does not error.
- **Empty set.** Extract yields nothing → the chosen guard behavior (warn or no-op) holds.
- **`derive_target`.** `bin` output paths mapped back to source; exclusion lines up on the
  source tree.
- **`--changed` intersection**; **`fix` targeting** narrowed to in-scope files.
- **Bench-compare.** The resolve-once cost against a plain glob baseline; gate on the
  standard S-scenarios (should be negligible).
- Every registered predicate needs both a firing and a silent e2e scenario (constitution 8).

## 7. Resolved decisions

Resolved 2026-08-13 for the v0.15 cut (ADR-0010 Accepted). The original open questions and
the decision taken on each:

1. **Source-versus-output — the crux.** RESOLVED: land both together. `derive_target` ships
   in v0.15 (reusing `file_graph`'s mapper), so the headline `package.json` `bin` case works
   on arrival — mapping `dist/cli.js` back to `src/cli.ts` — rather than being documented as
   "needs the mapper." Source-declaring manifests (`Cargo.toml` `workspace.members`,
   `tsconfig` references, `go.work`, PSR-4 autoload) work without the mapper; build-output
   manifests opt into it.
2. **Shape.** RESOLVED: a narrow `scope_filter` predicate, not a top-level `path_sets:` block.
   It fits the ScopeFilter generalisation and keeps the schema surface minimal. A set reused
   by many rules is repeated per rule — the accepted cost of not adding a top-level concept;
   `path_sets:` stays a possible future addition if reuse pressure grows.
3. **Include and exclude.** RESOLVED: two predicates, `include_manifest_paths` /
   `exclude_manifest_paths`, not one with a `mode:`. This sits with `has_ancestor` (also a
   predicate, not a mode) and the roadmap's mooted `has_sibling`.
4. **Empty-set default.** RESOLVED: `exclude_manifest_paths` with an empty set excludes
   nothing (the rule runs full-scope — safe), logged at debug. `include_manifest_paths` with
   an empty set would silently no-op the rule (a "silent cap" the constitution forbids), so it
   WARNS by default; `expect_nonempty: false` opts out for a rule that tolerates an empty set.
5. **Manifest-absent default.** RESOLVED: the predicate contributes nothing, consistent with
   `has_ancestor`. `exclude_manifest_paths` → full-scope; `include_manifest_paths` → scopes
   nothing and warns (same footgun as the empty set).
6. **`allow_out_of_root` and confinement.** RESOLVED: `source:` is ALWAYS repo-root-confined
   (an escaping `source` is a build-time error). A manifest a rule scopes by lives in the repo,
   so v1 does not honor `allow_out_of_root:` for the manifest read: the policy is a load-time
   per-rule resolution not reachable from `ScopeFilter::from_spec`, and an out-of-root manifest
   source is not a real use case. Revisit only if one appears.
7. **Relationship to `has_sibling`.** RESOLVED: `has_sibling` is NOT bundled into v0.15 (a
   separate scope generalisation). The manifest-path predicate is named and shaped as a
   `scope_filter` membership predicate so `has_sibling` joins the same family later without a
   rework.
