# v0.10 — Case-study coverage push

Status: Scope-only README. Per-feature design docs land
opportunistically as each rule kind moves from "demand-validated"
to "implementation imminent" — same shape as the v0.7 design pass
(one design doc per primitive, written before code). When
implementation begins, the doc gets a `Status: Implemented in
<commit>` header.

## What v0.10 ships

The eight rule kinds and two bundled rulesets the case-study
aggregation ([`docs/development/launch-evidence.md`](../../development/launch-evidence.md),
30 OSS repos) demand-validated against the working catalogue.

The original v0.10 framing was "LSP + developer experience"; that
work moved to v0.11 when the case-study aggregation surfaced
enough rule-kind demand to justify a coverage-focused cut first.
LSP design pass artefacts that landed in v0.9.7 originally under
this directory (`lsp_server.md`, `vscode_extension.md`,
`single_file_reevaluation.md`) physically relocated to
[`../v0.11/`](../v0.11/) in v0.9.22 to match the scope flip. The
`tower-lsp` workspace dep is still parked, awaiting the v0.11
`crates/alint-lsp/` crate scaffold.

| # | Primitive / ruleset                                  | Demand sources |
|---:|-----------------------------------------------------|---------------:|
| 1 | `registry_paths_resolve`                             | 13 |
| 2 | `cross_file_value_equals` (incl. `value_extractor:`) | 12 |
| 3 | `ordered_block`                                      | 8 |
| 4 | `generated_file_fresh`                               | 8 |
| 5 | `import_gate`                                        | 5 |
| 6 | `command_idempotent` mode                            | 5 |
| 7 | `xml_path_matches` + `xml_path_equals`               | 2 |
| 8 | `pair_hash`                                          | 3 |
| 9 | `apache/governance@v1` (bundled ruleset)             | 3 |
| 10 | `dotnet@v1` (bundled ruleset)                       | 1 |

Order by demand × adopter surface. Per-repo citations + per-
primitive evidence in [`../../development/launch-evidence.md`](../../development/launch-evidence.md);
canonical scope reference in [`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push).

## How to use this directory

When a rule kind moves from "demand-validated" to "implementation
imminent", add `<kind>.md` here. Follow the v0.7 / v0.9 design-
pass shape:

1. **Problem** — what user pain this addresses, sourced from the
   case studies linked above.
2. **Surface area** — what changes inside the engine / DSL /
   schema.
3. **Semantics** — what the engine does on each evaluation path.
4. **False-positive surface** — what could go wrong and the
   planned mitigations.
5. **Implementation notes** — module location, dependencies,
   complexity estimate.
6. **Tests** — coverage plan including the bench-compare
   thresholds the phase commits to.
7. **Open questions** — decisions to make before implementation.

Resolve open questions in the doc itself when implementation
lands; add a `Status: Implemented in <commit>` header on merge.

## Cross-cutting decisions

- **Heuristic vs. precise.** Every rule kind in v0.10's scope is
  precise (path resolution, value equality, ordered comparison,
  hash equality, regex on declared import scopes, code-generator
  determinism). No heuristic surface in v0.10; heuristics stay in
  `commented_out_code` / `git_blame_age` / `markdown_paths_resolve`
  (v0.7 territory).
- **Schema versioning.** Every v0.9.21 config runs unchanged on
  v0.10. `version: 1` covers the entire v0.10 cut. New rule kinds
  add optional fields to existing top-level shapes.
- **Design candidates landing opportunistically** when a second
  demand source materialises: `*_path_contains`, `pair_inverse`,
  `command_per_repo`, `json_schema_passes` config-shape mode,
  `*_path_array_iter`, `multi_doc_mode:` on `yaml_path_*`.
  Tracked in the canonical ROADMAP under "design candidates".

## Out of scope for v0.10

Held back to keep the cut tight:

- **LSP + editor integration** — moved to v0.11 (the
  `lsp_server.md`, `vscode_extension.md`, and
  `single_file_reevaluation.md` design docs now live in
  [`../v0.11/`](../v0.11/)).
- **WASM plugin tier** — v0.13 (was v0.12 before the v0.12 real-world-coverage cut was inserted).
- **`detect: linguist` and `detect: askalono` facts** — PROPOSAL
  §4.6 items still open; orthogonal to rule-kind coverage.
- **Bazel-licensing-declaration-aware rule kind** — single-source
  demand (tensorflow); held for v0.11+ unless another adopter
  surfaces.
