# Case-study revalidation — batch 4 (alphabetical: kubernetes-kubernetes through nodejs-node)

Validation pass against alint v0.9.17 (built 2026-05-07).
Engine binary: `/home/kaminsod/projects/alint/target/release/alint`.
Authoritative pitfall catalogue: 21 (CONFIG-AUTHORING.md).
Pitfalls #18 + #19 fixed in engine v0.9.17 (per-rule
`respect_gitignore: false`; literal-path runtime guard for
`root_only: true`).

Bundled-ruleset rule counts (authoritative as of 2026-05-07):
- oss-baseline: 15 | rust: 11 | python: 9 | node: 9 | go: 8 | java: 11
- ci/github-actions: 3 | monorepo: 4 | monorepo/{cargo,pnpm,yarn}-workspace: 4 each
- hygiene/lockfiles: 7 | hygiene/no-tracked-artifacts: 11
- tooling/editorconfig: 3 | compliance/apache-2: 3 | compliance/reuse: 3 | docs/adr: 4
- agent-hygiene: 6 | agent-context: 5

## Per-case-study findings

### kubernetes-kubernetes

- **Validation:** ✓ 49 rule(s) loaded. Stale README claim "12-rule alint
  config" updated to clarify that 12 is the custom-rule count and the
  full config (with 4 bundled rulesets folded in) loads 49.
- **Pitfall numbering:** README does not cite numeric pitfall references
  — no drift to fix.
- **v0.10 candidate status:** `import_gate` (4 sources, k8s + airflow +
  golang/go + pytorch) and `pair_hash` (3 sources, k8s + tokio +
  golang/go FIPS) both promoted to v0.10 ship-target per
  launch-evidence.md; updated README to reflect status + source count.
- **Pitfall #18/#19 sweep:** No tracked-but-gitignored files in this
  config; no `root_only: true` rules. v0.9.17 engine fixes do not
  trigger workaround removal here.
- **`command:` shellouts that v0.9.6+ rule kinds now cover:** the
  `k8s-golangci-lint-config-shape` rule already uses `yaml_path_matches`
  (a v0.9.6+ structured-query primitive); the `k8s-shellcheck`,
  `k8s-spelling`, `k8s-gofmt`, `k8s-golangci-lint`, `k8s-govulncheck`,
  `k8s-owners-fmt` rules are AST-aware tools that legitimately stay as
  `command:` shellouts.
- **New analysis:** Future analysis section added with 3 candidates —
  `json_schema_passes` for the `staging/publishing/import-restrictions.yaml`
  registry (v0.10 design candidate), `hygiene/lockfiles@v1` overlay for
  the `vendor/` tree, and `agent-context@v1` adoption for the existing
  AGENTS.md.
- **Live-tree recheck:** k8s sparse-checkout not in `/tmp/`; deferred.
- **README touched:** lines 20 (rule count clarification), 144-160
  (Future analysis + Validation status appended).

### microsoft-typescript

- **Validation:** ✓ 68 rule(s) loaded. README's narrative count (~22
  custom + 6 bundled rulesets) consistent with the load count.
- **Pitfall numbering:** README cited "12 documented" pitfalls; updated
  to "21 documented" (catalogue at 21 since P2b Wave 2).
- **v0.10 candidate status:** `pair_count` candidate unchanged
  (TypeScript + airflow, 2 sources, design candidate).
- **Pitfall #18/#19 sweep:** No tracked-but-gitignored files; no
  `root_only: true` + multi-component literals. v0.9.17 fixes do not
  apply here.
- **`command:` shellouts that v0.9.6+ rule kinds now cover:** Existing
  `command:` rules (`ts-eslint`, `ts-dprint-check`, `ts-knip`) wrap
  AST-aware tooling — legitimately out of alint's scope. The
  `ts-tsconfig-strict-mode` rule already uses `json_path_equals` (v0.9.6+
  primitive) for the bool field, demonstrating the pitfall #16 canonical
  pattern.
- **New analysis:** Future analysis section added with 3 candidates —
  `agent-context@v1` adoption (AGENTS.md is load-bearing here),
  `pair_count` (≥1 partner files match a registry entry, surfaced by
  `errorCheck.mjs`), and `hygiene/lockfiles@v1` overlay for
  `package-lock.json` discipline.
- **Live-tree recheck:** typescript sparse-checkout not in `/tmp/`;
  deferred.
- **README touched:** line 305 (pitfall count 12→21), lines 312-345
  (Future analysis + Validation status appended).

### microsoft-vscode

- **Validation:** ✓ 67 rule(s) loaded. README claims "67-rule alint
  config" — exact match.
- **Pitfall numbering:** README cited "17 documented" pitfalls (was
  written when catalogue was at 17 after P2a Wave 3); updated to "21
  documented" + explicit note that pitfalls #18 + #19 shipped fixed in
  v0.9.17 with no workaround needed in this config.
- **v0.10 candidate status:** Pre-existing references to
  `cross_file_value_equals` as "v0.10+ candidate at 8 sources" /
  "9th source" updated to "v0.10 ship-target at 10 sources past
  saturation" per launch-evidence.md.
- **Pitfall #18/#19 sweep:** No tracked-but-gitignored files; the
  `root_only: true` rules (`vscode-tsfmt-json-exists`,
  `vscode-component-governance-files`, `vscode-agents-md-present`) all
  use single-segment literals at root — pitfall #19's failure mode is
  not triggered.
- **`command:` shellouts that v0.9.6+ rule kinds now cover:** Existing
  `command:` rules (`vscode-eslint`, `vscode-stylelint`,
  `vscode-precommit-hygiene`) wrap AST-aware / formatter tooling —
  legitimately out of alint's scope. The
  `vscode-tsconfig-base-{strict,no-implicit-overrides}` rules already
  use `json_path_equals` (v0.9.6+ primitive) for bool fields.
- **Apples-to-apples headline (build/hygiene.ts):** README headline
  "75% (6 of 8) declarative" preserved verbatim — strongest "alint
  replaces a hand-rolled script" data point in the case-study catalogue.
  Reaffirmed in the new Validation status footer.
- **New analysis:** Future analysis section added with 3 candidates —
  `compliance/reuse@v1` overlay for the Component Governance trio,
  `hygiene/lockfiles@v1` for the per-extension lockfile fan-out, and
  `nested_configs: true` for the `extensions/` polyglot mini-monorepo.
- **Live-tree recheck:** vscode sparse-checkout not in `/tmp/`;
  deferred.
- **README touched:** line 118 (cross_file_value_equals status update),
  lines 446-457 (cross_file_value_equals demand-count rewrite), line 650
  (followup-work entry status update), line 664 (pitfall count 17→21 +
  v0.9.17 note), lines 700-755 (Future analysis + Validation status
  appended).

### nixos-nixpkgs

- **Validation:** ✓ 79 rule(s) loaded. README claims "79-rule" config
  throughout — exact match maintained.
- **Pitfall numbering:** README cited "17 in CONFIG-AUTHORING.md" once
  (line 614); updated to "21" + explicit v0.9.17 fix note for #18 + #19.
- **v0.10 candidate status:** README's `registry_paths_resolve` "8
  distinct repos" count remains accurate per launch-evidence.md;
  `dir_name_matches_field` at "3 sources" remains accurate. No
  status-promotion changes.
- **Pitfall #18/#19 sweep:** No tracked-but-gitignored files in the
  config; the `root_only: true` rules (`nixpkgs-flake-nix-present`,
  `nixpkgs-default-nix-present`, `nixpkgs-shell-nix-present`,
  `nixpkgs-contributing-md-present`, `nixpkgs-copying-present`,
  `nixpkgs-gitblame-ignore-revs-present`) all use single-segment
  literals at root — pitfall #19's failure mode is not triggered.
- **`command:` shellouts that v0.9.6+ rule kinds now cover:** Existing
  `command:` rules wrap `nix-build` / `treefmt` / `actionlint` / `zizmor`
  — all legitimately out of alint's scope. The
  `nixpkgs-dependabot-includes-actions` rule already uses
  `yaml_path_matches` with bracket-notation for the dashed
  `package-ecosystem` key (canonical pitfall #10 pattern).
- **Headline scale data point:** **39 101 files / 273 ms wall-clock /
  79 rules / 20 678 by-name iterations** — preserved + reinforced in
  the new Validation status footer per the task brief's instruction
  ("make sure that headline number is in the README").
- **New analysis:** Future analysis section added with 3 candidates —
  `nested_configs: true` for the `pkgs/by-name/<2-letter>/<pkg>/`
  subtree fan-out (load-bearing once 5+ rules stack on the same
  iteration), `scope_filter` tightening for the by-name walk (reduces
  walker emit-set constant-factor cost), and `compliance/reuse@v1`
  overlay for the SPDX registry discipline.
- **Live-tree recheck:** README already documents the live-tree result
  at `/tmp/nixpkgs/` from the original P2b Wave 1 pass (273 ms, 34
  violations: 28 GHA hardening warnings, 3 expected `command:`
  PATH-misses, 3 hygiene info, 2 legitimate by-name bundler-cache).
  Not re-run in this batch.
- **README touched:** line 615 (pitfall count 17→21 + v0.9.17 note),
  line 654 (bench data clarification), lines 661-708 (Future analysis +
  Validation status appended).

### nodejs-node

- **Validation:** ✓ 86 rule(s) loaded. README narrative says "40-rule
  alint config" (custom-rule count); the discrepancy (40 vs. 86) is
  legitimate (the 86 includes the 5 bundled rulesets folded in). No
  drift to fix in the rule-count claim — the README already uses "40
  node-specific rules" as the framing.
- **Pitfall numbering:** README cited "16 documented" pitfalls; updated
  to "21 documented" + explicit v0.9.17 fix note for #18 + #19.
- **v0.10 candidate status (LOAD-BEARING UPDATE per task brief):** the
  pre-existing README mentions `cross_file_value_equals` as "v0.10+
  candidate at 6 sources / 7th after node" — updated to "v0.10
  ship-target at 10 sources past saturation". `registry_paths_resolve`
  similarly promoted from "v0.10+ candidate at 4 sources / 5th after
  node" to "v0.10 ship-target at 8 sources". This was the explicit
  callout in the task brief ("pre-existing README probably mentions
  cross_file_value_equals as a v0.10 candidate — it's now 10 sources
  past saturation").
- **Pitfall #18/#19 sweep:** No tracked-but-gitignored files; the
  `root_only: true` rules (`node-governance-files-present`,
  `node-build-files-present`) use single-segment literals only.
  v0.9.17 fixes do not apply here.
- **`command:` shellouts that v0.9.6+ rule kinds now cover:** Existing
  `command:` rules wrap eslint × 4 tiers + cpplint + checkimports +
  clang-format + ruff + yamllint + lint-md + shellcheck — all AST-aware
  / formatter tooling, legitimately out of alint's scope. The
  `node-tsconfig-strict` rule already uses `json_path_equals` (v0.9.6+
  primitive) for the bool field. The `node-lint-md-remark-*-pinned`
  rules use bracket notation for the dashed dependency-name keys
  (canonical pitfall #10 pattern).
- **Live-tree recheck:** sparse-checkout present at `/tmp/nodejs-node`
  + subset at `/tmp/nodejs-node-subset`. Spot-checked via `alint suggest`:
  surfaces `oss-baseline@v1` (high) + `python@v1` (high) +
  `agent-hygiene@v1` (medium). The current config adopts oss-baseline
  but not python (legitimate — python here is build tooling, not the
  primary language) and not agent-hygiene (worth considering).
- **`tools/eslint-rules/` ↔ `eslint.config.mjs` registry pattern (per
  task brief):** flagged in the new Future analysis section as a
  candidate refinement of `cross_file_value_equals` once v0.10 ships
  — the registry-flavoured shape ("every file in dir X appears at some
  path in registry Y") is distinct from the per-key value comparison
  (could be a `cross_file_files_match_registry: true` mode).
- **New analysis:** Future analysis section added with 3 candidates —
  `agent-context@v1` adoption for the existing governance/contributing
  doc network, `hygiene/lockfiles@v1` overlay for `tools/eslint/` +
  `tools/lint-md/` lockfile discipline, and the
  `tools/eslint-rules/*` ↔ `eslint.config.mjs` registry-pattern
  candidate refinement.
- **README touched:** lines 254-255 (cross_file_value_equals +
  registry_paths_resolve status updates), lines 265-272
  (Cross-reference block status updates), lines 466-473 (followup
  work entries status updates), line 482 (pitfall count 16→21 +
  v0.9.17 note), lines 511-562 (Future analysis + Validation status
  appended).

## Cross-cutting patterns observed across this batch

1. **No new pitfalls surfaced** in any of the 5 case studies during
   revalidation. Configs draft cleanly against the 21-pitfall catalogue.
2. **No new rule-kind candidates surfaced.** Every gap mentioned in the
   batch's READMEs reconfirms an existing v0.10 candidate; consistent
   with launch-evidence.md's saturation analysis.
3. **`cross_file_value_equals` and `registry_paths_resolve` are the two
   load-bearing v0.10 ship-targets across the batch** — vscode
   (`checkCopilotEnginesVersion`), nodejs (`tools/eslint-rules/*` ↔
   `eslint.config.mjs`, `tools/dep_updaters/` ↔ `deps/`), nixpkgs
   (`maintainer-list.nix` ↔ `meta.maintainers`, `licenses.nix` ↔
   `meta.license`, `ci/OWNERS` patterns) all surface them. Status
   correctly upgraded across all touched READMEs.
4. **Pitfall-count drift was uniform** — every batch-4 README except
   k8s (which doesn't cite numeric pitfalls) had a stale "12 / 16 / 17
   documented" count from earlier waves; all updated to "21" + explicit
   v0.9.17 fix note for pitfalls #18 + #19.
5. **v0.9.17 engine fixes (#18 + #19) trigger no workaround removal
   in this batch.** None of the 5 configs use the per-rule
   `respect_gitignore: false` pattern (no tracked-but-gitignored files
   in scope), and every `root_only: true` rule uses single-segment
   literals only (no multi-component literal failure mode). The
   v0.9.17 fixes are pure DX upgrades — no config edits were required.
6. **Bundled-ruleset adoption is uneven across the batch** —
   `agent-context@v1` (newly available) is only adopted by typescript
   and vscode; `hygiene/lockfiles@v1` is adopted by none; `compliance/
   reuse@v1` is adopted by none. Future-analysis sections in every
   touched README flag these as candidates worth side-by-side
   comparison.
7. **The nixpkgs scale headline (39 101 / 273 ms / 79 rules / 20 678
   by-name iterations) remains the load-bearing empirical anchor for
   alint's "any size repo" pitch** — preserved + reinforced in the
   nixpkgs Validation status footer.
8. **The vscode build/hygiene.ts apples-to-apples headline (75 %, 6 of
   8 declarative) remains the strongest "alint replaces a hand-rolled
   script" data point** — preserved + reinforced in the vscode
   Validation status footer.

## Blockers

None. All 5 configs revalidate cleanly at v0.9.17. No `.alint.yml`
bugs were surfaced (only README drift was fixed).

## READMEs touched + log entries written

- ✓ `examples/kubernetes-kubernetes/README.md` (rule-count
  clarification + Future analysis + Validation status footer)
- ✓ `examples/microsoft-typescript/README.md` (pitfall count 12→21 +
  Future analysis + Validation status footer)
- ✓ `examples/microsoft-vscode/README.md` (5 status updates +
  pitfall count 17→21 + Future analysis + Validation status footer
  with 75% apples-to-apples preservation)
- ✓ `examples/nixos-nixpkgs/README.md` (pitfall count 17→21 +
  bench-data clarification + Future analysis + Validation status
  footer with 273-ms scale headline preservation)
- ✓ `examples/nodejs-node/README.md` (5 status updates including
  the load-bearing cross_file_value_equals + registry_paths_resolve
  promotions to v0.10 ship-target + pitfall count 16→21 + Future
  analysis + Validation status footer including live-tree
  `alint suggest` evidence)
- ✓ `docs/development/case-study-revalidation-batch-4.md` (this file)
