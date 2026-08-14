---
status: accepted
date: 2026-08-06
decision-makers: asamarts
---

# 0010. Manifest-derived rule scope stays on the scope_filter seam

## Status

Accepted. (One of: Proposed | Accepted | Rejected | Deprecated | Superseded by ADR-NNNN.)

Accepted 2026-08-13 for the v0.15 cut. The companion design doc
(`docs/design/v0.15/manifest-derived-scope.md`) resolved the mechanism details left open
here: a narrow `scope_filter` predicate pair, `include_manifest_paths` /
`exclude_manifest_paths` (not a top-level `path_sets:` block), with `derive_target`
output-to-source mapping shipping in the same release so the motivating `package.json`
`bin` case works on arrival rather than as a fast-follow. The boundary this ADR draws is
unchanged.

## Context

A recurring request is for a content rule to take its file scope from a manifest. The
motivating case, from a user who built their own repo-shape linter: a `no-console` rule
fired 19 times on a Vite app, all 19 wrong (13 inside an `import.meta.env.DEV` guard, 6
in `scripts/`). Their conclusion, which matches alint's own philosophy, was that the
rules worth keeping answer "what is this path allowed to be", not "what is in this file".
Their unresolved boundary: a `cli.ts` in `src/` is an entrypoint, but only `package.json`
`bin` declares that; nothing in the tree does. Exempting it from `no-console` means either
a hand-written exclude that drifts from `bin`, or teaching the content rule to read the
manifest.

This is not only an external ask. `docs/design/v0.12/asf_bundle_overfire.md` records our own
bundled rulesets "independently re-derived the same `paths.exclude` workaround" — a
hand-maintained exclude that silently drifts from the manifest that is the real source of
truth.

Three existing invariants constrain any answer:

1. **Facts are repo-level and evaluated once per run** (ADR-0003). They feed `when:`
   (whole-rule gating) and message vars, never `paths:`. There is no per-file,
   value-driven scoping today.
2. **`paths:` are static globs.** They do not interpolate facts or values. You can read a
   rule and know what it scopes without resolving anything. That legibility is a feature.
3. **Content rules must not infer intent.** A regex that tries to tell a dev-guarded
   `console.log` from a stray one is the exact failure the motivating case hit.

The tension: (1)-(3) are worth keeping, but the drift in the ASF workaround is real, and
"the manifest is the source of truth for what an entrypoint is" is a legitimate thing to
want to express.

The resolution turns on a distinction the boundary case blurs. Deriving a rule's **path
scope** from a manifest-declared path set is deterministic and declarative; it is not the
intent-inference trap. Feeding a manifest **value** into a content rule's **verdict** is
that trap. These are different operations and deserve different answers.

## Decision

We will let a manifest narrow a per-file rule's **scope**, and only its scope, and only
through one mechanism: a path set that is **resolved once per run** and delivered via
`scope_filter` (a generalisation of the existing `has_ancestor:` and `changed_since:`
predicates). Concretely, we will:

- **Add manifest-path predicate(s) to `scope_filter`** that extract a path set from a
  manifest (reusing the `toml`/`json`/`yaml` JSONPath, `lines`, and `regex` extract one-of
  that `registry_paths_resolve` and `file_graph` already share) and gate each candidate
  file by membership in that set. The set is resolved once, cached, and applied per file,
  exactly as `changed_since:` resolves its diff once.
- **Keep the manifest out of the verdict.** A manifest value may decide *which files a rule
  sees*; it may never decide *what the rule concludes about a file's contents*. Content
  rules stay path-scoped.
- **Keep `paths:` static.** Manifest values are never interpolated into `paths:`. The glob
  you read is still the glob that runs; the manifest contribution lives in the explicit
  `scope_filter` block, where it is visible, and `alint explain` will show the resolved set
  and its provenance.
- **Add no new trust surface.** Extraction is pure-parse and never spawns, so the new
  predicate is safe inside an `extends:`'d ruleset (unlike `custom:` facts and the spawning
  kinds). Manifest reads remain confined to the repo root under the existing
  path-confinement gate (ADR-0004).

The one-line invariant: **manifest VALUES may gate WHICH files a rule sees, never WHAT the
rule decides about a file.**

The mechanism details deliberately left to the design doc — the exact predicate names,
whether to ship a narrow `scope_filter` variant or a general named `path_sets:` block, and
how to bridge manifests that declare build output (`package.json` `bin` points at `dist/`,
not `src/`) rather than source paths — do not change this boundary. They change only how
the path set is spelled and mapped.

## Consequences

Easier:

- The drift closes. An exclusion can be pinned to the manifest that owns the truth, so a
  new `bin` entry cannot silently fall out of scope. The ASF workaround gets a real fix
  instead of a hand-maintained list.
- "Exempt the entrypoints the manifest declares" becomes expressible without a content
  rule ever reading the manifest, and without a second `cross_file` rule kept in sync by
  hand.
- The feature composes existing machinery (`scope_filter`, the extract one-of,
  `file_graph`'s `derive_target`) rather than adding an engine concept.

Harder, and accepted:

- **Legibility erodes slightly.** You can no longer read `paths:` alone and know the scope;
  you must also resolve the manifest. We accept this because the contribution is confined to
  an explicit `scope_filter` block (not smuggled into `paths:`) and because `alint explain`
  will surface the resolved set. This is the deliberate price for closing the drift.
- **The motivating example is the weakest case.** `package.json` `bin` usually names build
  output, not source, so the headline "exempt the bin entrypoint from a `src/**` rule" needs
  a `derive_target` mapping step; it does not work by naive membership. The clean cases are
  source-declaring manifests (`Cargo.toml` `workspace.members`, composer PSR-4 autoload,
  `tsconfig` references, `go.work`). The design doc must be honest that the bin case is a
  fast-follow, not the v1 sweet spot.
- **New scope surface to maintain**: another `scope_filter` predicate, its schema, its
  `explain` output, and its firing/silent e2e pair.
- **It does not solve intent.** The 13 dev-guarded consoles in the motivating case still
  fire; this feature scopes by path provenance, not by understanding code. That limit is
  intentional and is the whole reason content rules stay path-scoped.

## Considered Options

- **Manifest-path predicate(s) on `scope_filter`, resolved once (chosen boundary).** Fits
  the existing per-file gating seam and the roadmap's ScopeFilter generalisation; reuses the
  extract one-of; adds no trust surface.
- **A general named `path_sets:` block any rule references in `include`/`exclude`.** Same
  boundary as chosen, more reusable, but a new top-level concept and a larger surface. Not
  rejected — deferred to the design doc as the main shape alternative.
- **Interpolate a manifest value into `paths:`** (for example `exclude: "{{manifest:...}}"`).
  Rejected: it breaks the "read the glob, know the scope" property and smuggles a
  once-per-run resolution into a field that is otherwise static and per-rule-legible.
- **Let a content rule read the manifest to re-decide its verdict per file.** Rejected: this
  is the intent-inference trap the motivating case demonstrates, dressed up as configuration.
  A content rule that reaches into a manifest to change its own conclusion is exactly what
  produces plausible-but-wrong findings.
- **Do nothing; keep `cross_file` validation only.** The status quo: keep the exclude
  path-based and pin it to the manifest with a separate `cross_file` rule. Honest and
  available today, but it leaves two artifacts to maintain and does not close the drift at
  the point of use. Rejected as the end state, retained as the interim answer.

## More Information

- Companion design doc: `docs/design/v0.15/manifest-derived-scope.md` (surface, semantics,
  false-positive analysis, and the open source-versus-output and shape questions).
- The predecessor primitive: `docs/design/v0.9/scope-filter.md` and
  `docs/design/v0.11/scope_filter_changed_since.md` (the once-per-run resolution pattern this
  reuses).
- Reused machinery: `docs/design/v0.10/registry_paths_resolve.md` (the extract one-of) and
  `docs/design/v0.12/file_dependency_graph.md` (`derive_target`, the output-to-source mapper).
- In-tree motivation: `docs/design/v0.12/asf_bundle_overfire.md` (the drifting `paths.exclude`
  workaround).
- Related: ADR-0003 (dispatch and determinism; the once-per-run guarantee) and ADR-0004
  (extends trust boundary and path confinement; why a pure-parse manifest read is safe in an
  inherited ruleset).
