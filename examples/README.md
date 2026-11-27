# Examples

Real-world `.alint.yml` configurations from the launch-prep validation pass
(see [`docs/development/launch-evidence.md`](../docs/development/launch-evidence.md)).
Each subdirectory is one case study — a popular OSS repo's existing
structural-validation tooling inventoried, rebuilt as an alint config,
and compared.

> Marketing/positioning writeups for each case study live at
> https://alint.org/examples/. This index is the engineering reference:
> directory layout, factual one-liner per repo, contribution workflow.

## v0.9.20 reconciliation pass (2026-05-10)

The 30 per-example READMEs were last walked against alint v0.9.17
(2026-05-06). Since then:

- **v0.9.17 engine fixes** — pitfall #18 (per-rule
  `respect_gitignore: false` knob; demoed in `bazel`, `flutter`),
  pitfall #19 (`literal_is_nested` runtime guard).
- **v0.9.18 fix wave** (~10k false-positive eliminations across all
  30 trees):
  - **A1** `hygiene-no-js-build-outputs` requires sibling
    `package.json` — closes FP classes in 9 case studies (kubernetes,
    deno, flutter, golang-go, vscode, nixpkgs, node, next.js, turbo).
  - **A2** `apache-2-source-has-license-header` bundles the long-form
    ASF preamble — closes FPs in 4 (airflow, arrow, spark, tensorflow).
  - **A3** `python@v1` default-excludes test-fixture paths — closes
    FPs in 2 (ruff, cpython).
  - **A4** `monorepo/cargo-workspace` selector parses `[workspace]`
    members — applies to 6 (uv, clap, deno, next.js, turbo, dotnet).
  - **A5** `oss-license-exists` recognises `LICENSE.TXT` / `LICENSE.md`
    — closes FPs in 4 (arrow, deno, dotnet, tensorflow).
  - **A6** `rust@v1 rust-sources-snake-case` gains
    `allow_compiler_naming` knob — closes FPs in 2 (clap, rust-lang/rust).
  - **B1 / B2 / B3** pitfall #22 (YAML `|` → `|-`) fixes in
    TypeScript / deno / tensorflow configs.
  - **kubernetes deep-analysis pilot** (commit `c5b6df32`) — 3
    `.alint.yml` regex bugs fixed (pitfalls #13 / #14 / #22),
    eliminating ~34,000 false positives that the v0.9.17 walk had
    flagged as "P0 for parent-agent triage."
  - `dir_absent` engine extension — now supports `scope_filter`.
- **v0.9.19 / v0.9.20** — width-aware human output across every
  command; bundled rule message audit; em-dash scrub; install-snippet
  reorder (curl + bash now leads everywhere).

Each per-example README now tags v0.9.17 capture counts as historical
and names which fix resolved which previously-flagged FP. Counts have
not been re-walked under v0.9.20 (per-tree clones are expensive and
absolute counts drift with upstream tip); category-level findings are
stable.

## Layout

```
examples/
├── README.md                          # this file
├── <owner>-<repo>/
│   ├── README.md                      # case study writeup
│   ├── .alint.yml                     # the alint config that matches their existing tooling
│   ├── existing-tooling.md            # inventory of what they enforce today (where present)
│   └── comparison.md                  # alint output vs existing tool output + perf delta (where present)
```

## Case studies

30 of planned 40 (P2a complete — 20 of 20; P2b Wave 1+2 — 10 of planned 20
polyglot monorepos; remaining 10 polyglot repos queued for post-launch).
Listed alphabetically. Each entry: a factual one-liner with the rule count
from `alint validate-config` against the case study's `.alint.yml`.

- [`angular-angular/`](angular-angular/) (128 rules) — TypeScript framework with 16 packages; `goldens/public-api/<pkg>/index.api.md` discipline locks the TS API surface of 13 of 16 packages.
- [`apache-airflow/`](apache-airflow/) (90 rules) — 109 pre-commit hooks; ~40% map to alint declaratively.
- [`apache-arrow/`](apache-arrow/) (116 rules) — 6 languages in one tree (C++/Java/Python/Rust/Go/JS); 21 lint hooks across 14 tool repos. Live findings: 16 source files missing the Apache header (all listed in `dev/release/rat_exclude_files.txt`).
- [`apache-spark/`](apache-spark/) (110 rules) — 49 `pom.xml` files; exercises `xml_path_matches` / `xml_path_equals` and `apache/governance@v1`.
- [`astral-sh-ruff/`](astral-sh-ruff/) (80 rules) — 900+ Python lint rules but zero rules for ruff's own internal-crate `publish = false` discipline.
- [`astral-sh-uv/`](astral-sh-uv/) (89 rules) — 67-crate workspace conventions enforced nowhere in CI today.
- [`bazelbuild-bazel/`](bazelbuild-bazel/) (85 rules) — surfaces pitfall #18 (`.bazelversion` tracked-AND-gitignored), fixed in v0.9.17 via the per-rule `respect_gitignore: false` knob; the case-study config demonstrates the fix.
- [`clap-rs-clap/`](clap-rs-clap/) (76 rules) — Rust workspace; per-member inheritance via `for_each_dir` over family crates.
- [`denoland-deno/`](denoland-deno/) (81 rules) — Rust + JS + TS multi-language; custom validation scripts in `tools/lint.js`.
- [`dotnet-runtime/`](dotnet-runtime/) (59 rules) — 1,091 `.csproj` files (sparse checkout) + 234 solution files + 257 `Directory.Build.{props,targets}` + 520 `.props/.targets` ≈ 2,300 distinct XML manifests; exercises `xml_path_*` and `dotnet@v1`.
- [`facebook-react/`](facebook-react/) (80 rules) — `codes.json` registry shape + `ReactVersion.js` propagated to 3 per-package fields.
- [`flutter-flutter/`](flutter-flutter/) (57 rules) — Dart framework + native-OS embedders (Android/iOS/macOS/Linux/Windows/Fuchsia/GLFW + ABI) as peer subdirs under `engine/src/flutter/shell/platform/`. Live findings: 5 Trojan-Source / [CVE-2021-42574](https://nvd.nist.gov/vuln/detail/CVE-2021-42574) errors in `docs/releases/archive/` via `oss-baseline`'s `no_bidi_controls`.
- [`golang-go/`](golang-go/) (62 rules) — zero `.github/workflows/`, zero `Makefile`, zero `.golangci.yml`; the alint config encodes the project's structural contract for the first time.
- [`helm-helm/`](helm-helm/) (58 rules) — Trojan-Source defence + GHA hardening on top of golangci-lint.
- [`istio-istio/`](istio-istio/) (64 rules) — Single-module Go monorepo with 9 Helm charts + Prow CI + CODEOWNERS (not k8s-OWNERS). Per-chart image-hub at *different* JSONPath positions per file; multi-doc YAML release-notes file.
- [`kubernetes-kubernetes/`](kubernetes-kubernetes/) (58 rules) — 50 verify scripts inventoried; alint replaces 17 declaratively.
- [`microsoft-typescript/`](microsoft-typescript/) (69 rules) — eslint + dprint + knip already tight; alint adds the structural floor.
- [`microsoft-vscode/`](microsoft-vscode/) (68 rules) — apples-to-apples vs `build/hygiene.ts`. Covers ~75% of the 8 distinct hygiene checks (6 of 8) declaratively in one config; verified against the live tree (222 violations, zero false positives).
- [`nixos-nixpkgs/`](nixos-nixpkgs/) (56 rules) — 39,101 files + 20,678 `pkgs/by-name/*/*/` package directories. Full pass — including `for_each_dir` over the by-name tree — completes in 273 ms wall-clock.
- [`nodejs-node/`](nodejs-node/) (61 rules) — 15-year-old conventions enforced via human review only.
- [`pnpm-pnpm/`](pnpm-pnpm/) (92 rules) — replaces the in-tree `meta-updater` plugin's 13 cross-package field invariants without a per-repo plugin install.
- [`prettier-prettier/`](prettier-prettier/) (65 rules) — 5 net-new gates on top of eslint + prettier + cspell + knip + tsc.
- [`protocolbuffers-protobuf/`](protocolbuffers-protobuf/) (44 rules) — 10 in-tree language bindings (cpp, java, python, csharp, ruby, php, objc, hpb, upb, rust) + 1 spun-out (dart); per-binding `failure_list_<lang>.txt` files; per-binding GHA test workflow.
- [`python-cpython/`](python-cpython/) (78 rules) — 56 surfaces inventoried; one alint config consolidates the 38% that's declarative orchestration.
- [`pytorch-pytorch/`](pytorch-pytorch/) (60 rules) — ≈86% of pytorch's 57 `lintrunner.toml` adapters are structural; alint sits beneath, lintrunner keeps the AST-aware tail.
- [`rust-lang-rust/`](rust-lang-rust/) (58 rules) — `src/tools/tidy/` is a custom Rust binary doing alint's job; ~13 of ~32 tidy checks become declarative.
- [`tensorflow-tensorflow/`](tensorflow-tensorflow/) (55 rules) — 1,185 textproto API goldens under `tensorflow/python/tools/api/golden/{v1,v2}/`.
- [`tokio-rs-tokio/`](tokio-rs-tokio/) (70 rules) — zero hand-rolled scripts; alint catches 15 conventions tokio's pipeline assumes.
- [`vercel-next.js/`](vercel-next.js/) (110 rules) — first hybrid pnpm + Cargo dual-workspace case in the corpus; drift no per-language linter catches because each linter only sees half the tree.
- [`vercel-turbo/`](vercel-turbo/) (85 rules) — Rust monorepo orchestrator; alint adds 22 gates that don't exist.

## Historical primitive demand tracker (v0.9.20 snapshot)

The launch-prep audit surfaced the rule kinds and bundled rulesets below; all eight v0.10 core rule kinds (`cross_file` / `cross_file_value_equals`, `xml_path_*`, `registry_paths_resolve`, `ordered_block`, `generated_file_fresh`, `import_gate`, `pair_hash`, `command_idempotent`), both bundled rulesets (`apache/governance@v1`, `dotnet@v1`), and several emerging capabilities (baselines via `baseline:`, JSONC-tolerant parsing, `no_case_conflicts`, `executable_bit`) have since **shipped** in v0.10–v0.16. See [`docs/design/ROADMAP.md`](../docs/design/ROADMAP.md) for current roadmap status and [`docs/development/case-study-v011-reanalysis-log.md`](../docs/development/case-study-v011-reanalysis-log.md) for the v0.11 config refresh.

## Using these as starting points

Each `<owner>-<repo>/.alint.yml` is a working config. To use one as a starting
point for your own repo:

```sh
curl -fsSL https://raw.githubusercontent.com/asamarts/alint/main/examples/<owner>-<repo>/.alint.yml \
  > .alint.yml
alint check
```

Trim what doesn't apply to your repo, add what's specific. The configs are
deliberately written to be readable + adaptable, not minimal.

## Contributing a case study

If you've adopted alint for a public repo, consider contributing the case
study back — it helps other users with similar repo shapes.

The per-repo workflow ([`docs/development/launch-evidence.md`](../docs/development/launch-evidence.md#per-repo-case-study-contribution-workflow)) describes the steps.
