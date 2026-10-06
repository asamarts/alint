# `registry_paths_resolve` — every path a manifest enumerates resolves on disk

Status: **Implemented** — lands with the rule in v0.10 (this
commit; first rule kind of the case-study coverage push). Was a
design draft (2026-05-17). Top of the v0.10 cut by demand (13
sources, ROADMAP-canonical). Open questions resolved on
implementation: explicit `extract` only (Q1); `orphans` shipped
opt-in (Q2); regex literal-subset for Nix non-literals,
antiquoted entries skipped (Q3); additive, docs cross-link (Q4);
violation anchored at the registry-file path, byte-span SARIF
regions deferred (Q5).

Demand evidence: [`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
(the P2a row counts it the "highest-leverage gap"; that line's
"8 sources" is the older P2a sub-count) and the per-repo tracker
in [`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`registry_paths_resolve` row: arrow, spark, dotnet, flutter,
kubernetes, nixpkgs, node, protobuf, cpython, pytorch,
rust-lang/rust, tensorflow, next.js). Canonical scope:
[`../ROADMAP.md`](../ROADMAP.md#v010--case-study-coverage-push).

## Problem

A *registry file* enumerates path-like entries that are each
supposed to map to a real on-disk artefact. When an entry and the
tree drift apart, the failure is silent until a downstream tool
(cargo, npm, the build) trips on it — usually in CI, sometimes
only at publish. The same shape recurs across every demand source:

- **`Cargo.toml` `[workspace] members` / `exclude`** (rust-lang/rust,
  clap, tokio): each entry (often a glob like `"crates/*"`) must
  resolve to a directory that itself contains a `Cargo.toml`. The
  real pain is the reverse: a new crate dir added without a
  `members` entry builds locally (path dep) but is excluded from
  the workspace lint/test/publish set.
- **`package.json` `workspaces` / `files` / `bin`** (next.js,
  pnpm, nodejs/node): `workspaces` globs must each match ≥1 dir
  with a `package.json`; `files`/`bin` entries are publish
  correctness — a missing `bin` target ships a broken package.
- **`pnpm-workspace.yaml` `packages:`**, **flutter `pubspec.yaml`
  workspace + `bin/internal/*` engine-version pointers**.
- **nixpkgs `pkgs/top-level/all-packages.nix` `callPackage ./path`
  and the `pkgs/by-name` shard convention** (NixOS, ×3 registry
  files): every literal path argument must exist; every dir under
  `by-name/<shard>/` must be referenced.
- **dotnet/runtime solution filters (`*.slnf`) and `*.proj`
  `<ProjectReference Include="…"/>`**; **kubernetes `go.mod`
  `replace … => ./staging/src/…`**; **Bazel/CMake source lists in
  protobuf, pytorch, tensorflow** (`build_variables.bzl`,
  `srcs = [...]`); **arrow/spark component + module manifests**;
  **cpython built-module/source registries**.

Today alint can express *one* corner of this with `pair` or
`for_each_dir` + a hand-written `when:`, but not "parse this
manifest, extract its path list, assert each resolves" — and not
the reverse-completeness check at all. `markdown_paths_resolve`
(v0.7) proves the resolve-paths-from-a-file shape is in scope and
wanted; `registry_paths_resolve` is its manifest-driven
generalisation: structured extraction instead of backtick
scraping, plus orphan detection.

This is a *precise* rule, not heuristic (ROADMAP cross-cutting
decision for v0.10): literal path resolution against the file
index, no guessing.

## Surface area

New cross-file rule kind `registry_paths_resolve` in
`alint-rules`. One new optional top-level shape; `version: 1`
unchanged; every v0.9.x config still parses.

```yaml
- id: cargo-workspace-members-resolve
  kind: registry_paths_resolve
  source: Cargo.toml                   # the manifest/registry file (path or glob), relative to lint root
  extract:                             # how to pull the path list out
    toml: "$.workspace.members[*]"     # one of: toml/json/yaml (structured-query),
                                       # lines (line list), regex (capture group 1)
  base: registry_dir                   # resolve entries relative to: registry_dir
                                       # (default) | lint_root | "<path>"
  entries_are_globs: true              # entries may be globs; each must match ≥1
  expect: dir                          # each resolved path is: any (default) | file | dir
  must_contain: Cargo.toml             # optional: an expect:dir must itself contain this
  exclude_query: "$.workspace.exclude[*]"  # optional: entries to subtract before checking
  orphans:                             # optional reverse-completeness check
    space: "crates/*"                  # on-disk artefacts in this glob …
  level: error                         # applies to forward and orphan findings
```

`extract` is a one-of. The three structured forms reuse the
existing JSON/YAML/TOML path engine (so the
[dashed-key bracket-notation rule](../../development/) and
multi-doc semantics already apply). `lines` handles plain lists
(one path per non-blank, non-comment line; `comment:` prefix
configurable). `regex` handles quasi-structured registries (Nix
`callPackage\s+(\./\S+)`, Bazel `"\([^"]+\.cc\)"`): capture group
1 is the path.

`source:` itself may be a glob (`**/Cargo.toml`) — the rule then
runs once per matching manifest, each resolving against its own
dir. That makes one rule express "every nested workspace manifest
in this monorepo is internally consistent", which is the
monorepo-tier value proposition.

### Option naming (v0.10 pre-release harmonization)

The authoritative-input option is **`source:`** — the
manifest/registry file that enumerates the path list. It was
`registry:` through the v0.10 development cycle and was renamed
to `source:` before the v0.10 release as part of the
cross-file-family option-naming harmonization (aggressive
decision: every cross-file rule's authoritative input is
`source:`). All v0.10 work is on `[Unreleased]`, so this is not
a breaking change to any shipped release. Sibling shapes:
[`cross_file_value_equals`](./cross_file_value_equals.md) (the
reference `source:` + `targets:` form) and
[`pair_hash`](./pair_hash.md) (`source:` + `target:`). The
*concept* is still a registry/manifest of paths; only the option
key is `source:`.

## Semantics

Per matched `registry` file:

1. **Parse + extract** the entry list via the chosen `extract`
   mode. Non-literal entries — genuine interpolation /
   concatenation: `${var}`, `$(cmd)`, `{{ tmpl }}`, `"a" + b`
   (precise heuristic, v0.10 post-audit P2: bare `$` / `` ` `` /
   `(.` are all legal in real filenames and no longer count as
   non-literal — over-skipping them silently dropped real
   literal paths, a false negative) — are **skipped, not
   failed**: an *intentional silent skip*, the rationale
   documented here. `alint check` has no informational-finding /
   `--explain` channel, so *visibly* surfacing the skip list is
   a tracked **v0.11** item; v0.10 skips silently by design.
2. **Subtract** `exclude_query` entries if present.
3. **Resolve** each entry against `base` (default: the registry
   file's own directory; matches Cargo/npm semantics and alint's
   nested-manifest model).
4. If `entries_are_globs`, expand each via the existing glob
   engine; an entry matching **zero** paths is a violation
   (reported against the registry file, with the entry).
5. **Existence + kind**: each resolved path must exist and satisfy
   `expect`. If `must_contain` is set, an `expect: dir` must
   contain that sub-path (the "dir is a real crate/package, not
   just an empty folder" check).
6. **Orphans** (if configured): walk `orphans.space`; any on-disk
   artefact matching it but not covered by a (post-glob-expansion)
   entry is reported at the rule's top-level `level`. This is
   the "new crate not added to the workspace" / "by-name shard not
   wired" detector — the highest-value half of the rule.

Violations are per-entry (and per-orphan), each with the registry
file + the offending entry/path, so `--format sarif|github`
annotates the manifest line region (best-effort: byte offset of
the entry string when the structured parser exposes spans, else
the registry file head).

One existing config runs unchanged on upgrade; the rule only adds
new shapes.

## False-positive surface

The mitigations are the design's load-bearing part — a
resolve-paths rule that cries wolf gets disabled.

- **Non-literal entries.** `callPackage (./. + "/pkgs/${name}")`,
  `members = [env!("X")]`-style, Bazel `glob([...])`. → step 1
  skips, never fails (intentionally silent in v0.10; visibly
  surfacing the skip list is a tracked v0.11 item — `check` has
  no notes/`--explain` channel).
- **`exclude` / negation.** Cargo `exclude`, npm workspace `"!…"`
  negations, Bazel exclude globs → `exclude_query` + glob negation
  honored before the existence check.
- **Conditional / platform-gated entries.** cfg-gated members,
  OS-specific `bin`. v0.10 scope: treat as literal (check
  existence regardless of cfg — a gated path should still exist on
  disk). Documented limitation; revisit only if a 2nd source
  needs cfg evaluation.
- **Case-insensitive / symlinked filesystems.** Resolution goes
  through `alint-core`'s `FileIndex`, which already encodes the
  walker's case + symlink policy; the rule inherits it (no
  bespoke `Path::exists`, which would diverge on macOS/Windows
  runners).
- **Generated registries.** A registry produced by codegen
  (Bazel-generated `BUILD`) may legitimately reference
  yet-to-be-generated outputs. `expect:`/`must_contain:` let the
  user scope to source artefacts; omitting `orphans` disables the
  reverse check. Codegen-running stays alint's non-goal (see
  `generated_file_fresh`, the sibling v0.10 primitive).
- **Comments in line registries.** `lines` mode strips a
  configurable `comment:` prefix (default `#`) and blank lines.
- **Orphan over-firing.** `orphans.space` is an explicit
  opt-in glob, not a tree-wide scan; entries that are globs are
  fully expanded before the orphan diff so a `crates/*` member
  doesn't flag every crate as an orphan.

## Implementation notes

- Module: `crates/alint-rules/src/registry_paths_resolve.rs`.
  Registered like the other cross-file kinds; alias considered
  (`paths_resolve`) — deferred to Open questions.
- Cross-file dispatch: it reads one file and resolves against the
  index, so it slots into the v0.9.3 `PerFileRule` / cross-file
  shape, not the per-file walker. Existence checks use v0.9.5's
  lazy `FileIndex` path-index → **O(1) per entry**, same fast-path
  class as `file_exists` / `for_each_dir` post the v0.9.5
  regression fix. No per-entry `stat`.
- Extraction reuses the structured-query parsers already in
  `alint-rules` (JSON/YAML/TOML) and the `glob` crate (already a
  workspace dep). `lines`/`regex` are thin; `regex` reuses the
  shared compiled-regex cache.
- Orphan scan: one bounded walk of `orphans.space` — and when the
  rule is part of a full run the engine already walked the tree,
  so the index is warm; the orphan diff is a set-difference over
  the in-memory index, not a second I/O pass.
- `include_str!`-style data: none; nothing leaves the crate
  (keeps `cargo publish` clean — see the include_str memory).

## Tests

- Pass/fail fixtures per `extract` mode: `toml` (Cargo workspace),
  `json` (package.json workspaces + files + bin), `yaml`
  (pnpm-workspace), `lines` (a plain path manifest), `regex`
  (nixpkgs `callPackage`, a Bazel `srcs` list).
- Glob entries: `crates/*` with present + absent expansions; the
  zero-match-is-a-violation case.
- `expect` matrix: file vs dir vs any; `must_contain` present /
  absent dir.
- `exclude_query` subtraction; non-literal-entry **skip** (assert
  it does NOT fail); the narrowed `is_non_literal` boundary
  (bare `$`/`` ` ``/`(.` are literal; `${`/`$(`/`{{`/`+ ` are
  not). Visible surfacing deferred to v0.11.
- Orphan detection: a dir in `space` missing from the registry →
  fires at configured severity; a glob-covered dir → does not.
- False-positive guards each get a regression test (symlinked
  entry, commented line, interpolated Nix path).
- Coverage audits: add to `coverage_audit_pass_fail` and
  `coverage_audit_cross_file_dispatch`; extend
  `coverage_audit_readme_claims` rule-kind count (60 → 61
  behaviours; the canonical "70" headline tracks behaviours +
  aliases — update `xtask/src/docs_export.rs` derivation and the
  `all_kinds.yaml` fixture in lockstep, per the rule-count memory).
- **Bench-compare threshold:** add `registry_paths_resolve` to a
  synthetic 5,000-package workspace fixture in the bench harness.
  The phase commits to: full-run S3-class wall at 1M **does not
  regress** vs the pre-phase baseline beyond noise (CV-gated,
  per `RELEASING.md` bench-record review) — the rule is O(N)
  existence over the warm path-index, the same complexity class
  the v0.9.5 fix established; a regression means the index
  fast-path was bypassed.

## Open questions

Resolve inline when implementation lands.

1. **Extractor selection ergonomics.** Explicit `extract: {toml:
   …}` is unambiguous but verbose for the common cases. Worth an
   auto-detect (Cargo.toml → `workspace.members`; package.json →
   `workspaces`)? Leaning: ship explicit-only in v0.10, add
   sugar in v0.11 once real configs show the common shapes.
2. **Does orphan detection ship in v0.10?** It's the
   highest-value half but also the highest false-positive
   surface. Option: ship existence-only in v0.10, orphans behind
   the same kind in v0.11. Counter: 6+ sources want specifically
   the reverse check (new-crate-not-wired). Leaning: ship it,
   opt-in (`orphans:` absent ⇒ off), conservative defaults.
3. **Nix non-literal paths.** `by-name` is literal and the big
   win; `all-packages.nix` has many antiquoted paths. Is
   regex-extract of just the literal `callPackage ./…` subset
   enough demand value, or does NixOS need a real Nix-aware
   extractor (out of v0.10 scope)? Validate against actual
   nixpkgs before committing the `regex` recipe to docs.
4. **Relationship to `for_each_dir` / `pair` /
   `markdown_paths_resolve`.** Should those gain a `see also`
   note, or is `registry_paths_resolve` strictly additive? It is
   additive (different trigger: a manifest, not the tree walk),
   but the docs should cross-link so users pick the right one.
5. **Span-accurate annotations.** TOML/JSON parsers used: do they
   expose byte spans for array elements for precise SARIF
   regions, or is registry-file-head the v0.10 ceiling? Affects
   the `--format sarif|github` UX claim.
