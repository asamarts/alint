# `cross_file_value_equals` — a value in one file must equal a value in others

Status: **Implemented** — lands with the rule in v0.10 (this
commit; rule kind #2 of the case-study coverage push). Was a
design draft (2026-05-18). v0.10 demand #2 (12 sources,
ROADMAP-canonical). Open questions resolved on implementation:
`cross_file_field_equals` subsumed — a "field" is a query path,
so it is this kind with a structured `extract`, not a separate
primitive (Q1); shared `crate::extract` helper extracted and
`registry_paths_resolve` refactored onto it (Q2); `normalize` =
`none`/`trim`/`lower`/`semver-major` (Q3); canonical-source-only,
all-equal deferred (Q4); stringify-then-compare scalars (Q5);
violation anchored at the target-file path, byte-span SARIF
deferred (Q6). Sibling of
the shipped [`registry_paths_resolve`](./registry_paths_resolve.md)
(path-existence); this is the value-equality cross-file
primitive on the same infrastructure.

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
(the 11-source row — airflow, tokio, clap, uv, react, pnpm,
nodejs/node, pytorch, vscode, istio, dotnet/runtime — plus the
`value_extractor:` / pitfall-#20 line: istio's per-file-pattern
extractor is a *refinement* of this kind, not a new one) and the
per-repo tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`cross_file_value_equals` row: angular, airflow, helm, istio,
vscode, node, pnpm, pytorch, tensorflow, tokio, next.js, turbo).
Canonical scope:
[`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push)
(#2, "past saturation").

## Problem

A value is authored once and *restated* in many other files,
where it silently drifts. Existing tooling catches it only when
a build or publish trips on the mismatch — often in CI, sometimes
only after a bad release. The shape recurs across every demand
source:

- **Monorepo version lockstep.** `[workspace.package].version`
  in `Cargo.toml` vs every `crates/*/Cargo.toml`
  `[package].version` (tokio, clap, uv); `package.json` `version`
  vs every workspace package and the lockfile / `lerna.json`
  (pnpm, nodejs/node, next.js, turbo, react, vscode).
- **Toolchain pin coherence.** The Rust channel in
  `rust-toolchain.toml` == the CI workflow's toolchain == the
  `Dockerfile` == the README badge; the Python version across
  `pyproject.toml` / `.python-version` / the CI matrix (airflow,
  pytorch).
- **dotnet/runtime SDK band coherence.** `global.json`'s SDK
  version *band* == `Directory.Build.props` / the CI matrix —
  same feature band, not necessarily the same patch.
- **istio (pitfall #20, the `value_extractor:` refinement).**
  The canonical istio version vs every chart's `Chart.yaml`
  `appVersion` and every manifest's image tag: the target is a
  *glob*, and each match needs its *own* extraction query. That
  per-file-pattern extractor is the refinement istio surfaced;
  it is in scope for v0.10 as a shape of this kind, not a
  separate primitive.

Today `pair` checks a partner's *existence*, not its value; the
structured-query kinds (`json_path_equals` etc.) check a value
against a *literal you write in the config*, never against a
value *extracted from another file*. There is no way to say
"this value, wherever it lives, must equal that value, wherever
*it* lives." That is the gap.

This is a *precise* rule, not heuristic (ROADMAP v0.10
cross-cutting decision): literal value extraction + equality, no
guessing.

## Surface area

New cross-file rule kind `cross_file_value_equals` in
`alint-rules`. `version: 1` unchanged; every v0.9.x config still
parses.

```yaml
- id: workspace-versions-coherent
  kind: cross_file_value_equals
  source:                                   # the authoritative value
    file: Cargo.toml                         # literal path
    extract: { toml: "$.workspace.package.version" }
  targets:                                  # one of the two forms:
    # (a) glob + one per-target query — the value_extractor /
    #     pitfall-#20 refinement (istio chart appVersion, etc.):
    files: "crates/*/Cargo.toml"
    extract: { toml: "$.package.version" }
    # (b) OR an explicit list for heterogeneous shapes:
    # - { file: rust-toolchain.toml,            extract: { toml: "$.toolchain.channel" } }
    # - { file: .github/workflows/ci.yml,       extract: { yaml: "$.env.RUST" } }
    # - { file: Dockerfile,                     extract: { regex: "FROM rust:(\\S+)" } }
  normalize: none                           # none (default) | trim | lower | semver-major
  allow_missing_target: false               # absent file/value -> violation (default) | skip
  level: error
  message: "{{ctx.target}} = {{ctx.target_value}} != {{ctx.source}} {{ctx.source_value}}"
```

`extract` is the same one-of shape as `registry_paths_resolve`
(`toml`/`json`/`yaml` RFC 9535 JSONPath, `lines`, `regex` capture
group 1) — and the same struct-of-options encoding, because
serde_yaml cannot decode an externally-tagged enum from a
`{ key: value }` map (see that rule's design for the rationale).

## Semantics

1. **Resolve the source.** Read `source.file`, extract via
   `source.extract`. Exactly one value must result: zero ⇒ a
   violation ("canonical value not found at `<query>` in
   `<file>`"); more than one ⇒ a config-shaped error (the
   authority must be unambiguous).
2. **Enumerate targets.** Form (a): every index path matching
   the `files:` glob, each extracted with the single
   `targets.extract` query. Form (b): each listed `{file,
   extract}`.
3. **Compare.** Each target value is `normalize`-d and the source
   value is `normalize`-d; a target whose value `!=` the source
   value is a per-target violation, anchored on the *target*
   file, message carrying both values.
4. **Non-literal values** (genuine interpolation: `${var}` /
   `$(cmd)` / `{{ tmpl }}` / `"a" + b`) are **skipped, not
   failed** — reuses the shared, v0.10-post-audit-narrowed
   `is_non_literal` (bare `$` / `` ` `` / `(.` are legal in real
   values and no longer over-skip — same false-negative fix as
   `registry_paths_resolve`; a computed pin can't be
   string-compared and isn't drift). The skip is intentionally
   silent in v0.10; visibly surfacing it is a tracked v0.11 item
   (`alint check` has no notes/`--explain` channel).
5. **Missing.** A listed target file absent, a glob matching
   nothing, or a query yielding no value: violation by default;
   `allow_missing_target: true` downgrades to skip (optional
   pins).

`normalize`: `none` (exact string), `trim`, `lower`, or
`semver-major` (compare only the leading `MAJOR` — the
dotnet/runtime SDK-band shape). Deliberately small for v0.10.

One existing config runs unchanged; the rule only adds new
shapes.

## False-positive surface

The load-bearing part — a value-equality rule that cries wolf
gets disabled.

- **Type coercion.** JSON `1`, YAML `1`, TOML `"1"` parse to
  different `serde_json::Value` shapes. Comparison is on the
  *stringified scalar* (the structured engine already coerces
  through `serde_json::Value`); `normalize` operates on that
  string. Documented; `semver-major` covers the
  `"1.2.0"` vs `1.2` band case.
- **Dependency range operators.** `^1.2.0` / `~1.2` / `>=1.2,<2`
  vs an exact `1.2.0` legitimately differ — that is *not* drift,
  it's a version-range, a different concern. `cross_file_value_equals`
  is for *exact* (or major-band) coherence: monorepo lockstep,
  toolchain pins, SDK bands. Dep-range coherence is explicitly
  out of scope (a future `*_range_satisfies` primitive).
- **Multi-value source.** A source query returning an array is
  an error, never a silent "first wins" — the authority must be
  one value.
- **Glob zero-match / missing value.** Governed by
  `allow_missing_target`, opt-in, not a tree-wide surprise.
- **Whitespace / comments.** `trim` normalize; `lines`/`regex`
  extraction already strips per the shared extractor.

## Implementation notes

- Module: `crates/alint-rules/src/cross_file_value_equals.rs`.
  Cross-file (`requires_full_index() == true`,
  `path_scope() == None`), same dispatch class as
  `registry_paths_resolve` / `pair`.
- **Shared extractor (do this first).** `registry_paths_resolve`
  already has a private `structured()` + `is_non_literal()` +
  the `ExtractSpec` one-of. This rule needs the identical
  extraction. Extract a `crate::extract` helper module and
  refactor `registry_paths_resolve` onto it in the *first*
  implementation commit, so the two kinds can't drift. (Open
  question 2.)
- Target enumeration reuses `Scope` + `FileIndex` (the glob
  form); file content is read from disk via `ctx.root.join`
  (same as `registry_paths_resolve` — the index has paths, not
  contents). O(T) target files, one structured-parse each.
- Source/target reads go through `crate::io::read_capped`
  (256 MiB `MAX_ANALYZE_BYTES`, `metadata` stat-gated): an
  over-cap file is one clear "too large to analyze" violation,
  not an OOM (v0.10 post-audit P2, D4). The same helper bounds
  the reads in `registry_paths_resolve` / `pair_hash` /
  `generated_file_fresh`; a too-large target is *not* suppressed
  by `allow_missing_target` (it is present, just unanalysable).
- No `include_str!` data; nothing leaves the crate (keeps
  `cargo publish` clean — see the include_str memory).

## Tests

- Per `extract` mode for source and target (toml/json/yaml/
  lines/regex).
- Form (a) glob targets (the `value_extractor:` / pitfall-#20
  shape) and form (b) explicit heterogeneous list.
- `normalize` matrix: `none` / `trim` / `lower` / `semver-major`
  (the dotnet SDK-band case).
- Missing: default-violation vs `allow_missing_target: true`;
  glob-zero-match.
- Non-literal value **skipped** (asserted not-fail);
  multi-value-source **error**.
- Canonical demand shapes: Cargo workspace version lockstep
  (`crates/*/Cargo.toml` glob); a heterogeneous toolchain-pin
  list; istio-style `Chart.yaml` `appVersion` glob.
- Lockstep with the codebase invariants (the same checklist
  `registry_paths_resolve` followed): `coverage_audit_pass_fail`
  (pass + fail e2e scenarios), `coverage_audit_cross_file_dispatch`,
  schema `$def` + dispatch `$ref` in both mirrored `config.json`,
  `all_kinds.yaml` entry, regenerated default-options snapshot,
  rule-count **71 → 72** across README ×2 / `docs/site/about` /
  `coverage_audit_readme_claims`, `docs/rules.md` section,
  CHANGELOG `[Unreleased]` Added (the second v0.10 item).
- **Bench-compare threshold:** add to the synthetic workspace
  fixture; the rule is O(T) structured-parse over target files —
  full-run S-class wall must not regress vs the pre-phase
  baseline beyond noise (the `xtask bench-gate` gate, per
  `RELEASING.md`).

## Open questions

Resolve inline when implementation lands.

1. **`cross_file_field_equals`.** launch-evidence names a
   variant. A "field" is just a structured-query path, so it is
   the same kind with a structured `extract` — *not* a separate
   primitive and not even an alias. Confirm on implementation;
   document the equivalence so the demand line resolves cleanly.
2. **Shared `crate::extract` helper.** Two consumers now
   (`registry_paths_resolve` + this). Extract + refactor in the
   first impl commit (leaning yes — duplication would drift the
   non-literal / one-of logic), vs duplicate-now-unify-in-v0.11.
   The refactor touches a just-shipped rule, so it ships behind
   that rule's existing test suite as the regression guard.
3. **`normalize` set.** `none`/`trim`/`lower`/`semver-major` for
   v0.10. `semver-minor`, regex-capture-normalize, and a custom
   transform deferred until a 2nd demand source needs them.
4. **"All-equal" mode.** Every file in a set agreeing with each
   other (no canonical authority) is a different shape. v0.10
   ships canonical-source-only; all 12 sources have an
   authoritative file, so all-equal is deferred unless a source
   needs it.
5. **Scalar equality.** Stringify-then-compare (predictable,
   matches `structured_path`'s `Value` handling; `normalize`
   works on the string) vs typed equality (`1 == 1.0`). Leaning
   stringify; lock it in with the type-coercion tests.
6. **Span-accurate annotations.** Byte offset of the target
   value for precise `--format sarif|github` regions, or
   target-file-head as the v0.10 ceiling (same call as
   `registry_paths_resolve` Q5). Default to file-head unless the
   parser exposes element spans cheaply.
