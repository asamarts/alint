# Examples

Real-world `.alint.yml` configurations from the launch-prep validation pass
(see [`docs/development/launch-evidence.md`](../docs/development/launch-evidence.md)).
Each subdirectory is one case study — a popular OSS repo's existing
structural-validation tooling inventoried, rebuilt as an alint config,
and compared.

> Marketing/positioning writeups for each case study live at
> https://alint.org/examples/. This index is the engineering reference:
> directory layout, factual one-liner per repo, contribution workflow.

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

- [`angular-angular/`](angular-angular/) — TypeScript framework with 16 packages; `goldens/public-api/<pkg>/index.api.md` discipline locks the TS API surface of 13 of 16 packages.
- [`apache-airflow/`](apache-airflow/) — 109 pre-commit hooks; ~40% map to alint declaratively.
- [`apache-arrow/`](apache-arrow/) — 6 languages in one tree (C++/Java/Python/Rust/Go/JS); 21 lint hooks across 14 tool repos. Live findings: 16 source files missing the Apache header (all listed in `dev/release/rat_exclude_files.txt`).
- [`apache-spark/`](apache-spark/) — 49 `pom.xml` files; surfaces the v0.10 ship-target `xml_path_matches` / `xml_path_equals` rule kinds.
- [`astral-sh-ruff/`](astral-sh-ruff/) — 900+ Python lint rules but zero rules for ruff's own internal-crate `publish = false` discipline.
- [`astral-sh-uv/`](astral-sh-uv/) — 67-crate workspace conventions enforced nowhere in CI today.
- [`bazelbuild-bazel/`](bazelbuild-bazel/) — surfaces pitfall #18 (`.bazelversion` tracked-AND-gitignored), fixed in v0.9.17 via the per-rule `respect_gitignore: false` knob; the case-study config demonstrates the fix.
- [`clap-rs-clap/`](clap-rs-clap/) — Rust workspace; per-member inheritance via `for_each_dir` over family crates.
- [`denoland-deno/`](denoland-deno/) — Rust + JS + TS multi-language; custom validation scripts in `tools/lint.js`.
- [`dotnet-runtime/`](dotnet-runtime/) — 1,091 `.csproj` files (sparse checkout) + 234 solution files + 257 `Directory.Build.{props,targets}` + 520 `.props/.targets` ≈ 2,300 distinct XML manifests; demand-validates `xml_path_*` at one OOM bigger scale than spark; `dotnet@v1` bundled-ruleset gap.
- [`facebook-react/`](facebook-react/) — `codes.json` registry shape + `ReactVersion.js` propagated to 3 per-package fields.
- [`flutter-flutter/`](flutter-flutter/) — Dart framework + native-OS embedders (Android/iOS/macOS/Linux/Windows/Fuchsia/GLFW + ABI) as peer subdirs under `engine/src/flutter/shell/platform/`. Live findings: 5 Trojan-Source / [CVE-2021-42574](https://nvd.nist.gov/vuln/detail/CVE-2021-42574) errors in `docs/releases/archive/` via `oss-baseline`'s `no_bidi_controls`.
- [`golang-go/`](golang-go/) — zero `.github/workflows/`, zero `Makefile`, zero `.golangci.yml`; the alint config encodes the project's structural contract for the first time.
- [`helm-helm/`](helm-helm/) — Trojan-Source defence + GHA hardening on top of golangci-lint.
- [`istio-istio/`](istio-istio/) — Single-module Go monorepo with 9 Helm charts + Prow CI + CODEOWNERS (not k8s-OWNERS). Per-chart image-hub at *different* JSONPath positions per file — surfaces pitfall #20 + the `value_extractor:` v0.10 design candidate. Multi-doc YAML release-notes file surfaces pitfall #21.
- [`kubernetes-kubernetes/`](kubernetes-kubernetes/) — 50 verify scripts inventoried; alint replaces 17 declaratively.
- [`microsoft-typescript/`](microsoft-typescript/) — eslint + dprint + knip already tight; alint adds the structural floor.
- [`microsoft-vscode/`](microsoft-vscode/) — apples-to-apples vs `build/hygiene.ts`. Covers ~75% of the 8 distinct hygiene checks (6 of 8) declaratively in one config; verified against the live tree (222 violations, zero false positives).
- [`nixos-nixpkgs/`](nixos-nixpkgs/) — 39,101 files + 20,678 `pkgs/by-name/*/*/` package directories. Full 79-rule pass — including `for_each_dir` over the by-name tree — completes in 273 ms wall-clock.
- [`nodejs-node/`](nodejs-node/) — 15-year-old conventions enforced via human review only.
- [`pnpm-pnpm/`](pnpm-pnpm/) — replaces the in-tree `meta-updater` plugin's 13 cross-package field invariants without a per-repo plugin install.
- [`prettier-prettier/`](prettier-prettier/) — 5 net-new gates on top of eslint + prettier + cspell + knip + tsc.
- [`protocolbuffers-protobuf/`](protocolbuffers-protobuf/) — 10 in-tree language bindings (cpp, java, python, csharp, ruby, php, objc, hpb, upb, rust) + 1 spun-out (dart); per-binding `failure_list_<lang>.txt` files; per-binding GHA test workflow. ~45 cross-language assertions one rule would express.
- [`python-cpython/`](python-cpython/) — 56 surfaces inventoried; one alint config consolidates the 38% that's declarative orchestration.
- [`pytorch-pytorch/`](pytorch-pytorch/) — ≈86% of pytorch's 57 `lintrunner.toml` adapters are structural; alint sits beneath, lintrunner keeps the AST-aware tail.
- [`rust-lang-rust/`](rust-lang-rust/) — `src/tools/tidy/` is a custom Rust binary doing alint's job; ~13 of ~32 tidy checks become declarative.
- [`tensorflow-tensorflow/`](tensorflow-tensorflow/) — 1,185 textproto API goldens under `tensorflow/python/tools/api/golden/{v1,v2}/`; demand-validates `cross_language_implementation_complete` at TWO topologies (per-source ↔ per-test within one language; core ↔ N bindings across languages).
- [`tokio-rs-tokio/`](tokio-rs-tokio/) — zero hand-rolled scripts; alint catches 15 conventions tokio's pipeline assumes.
- [`vercel-next.js/`](vercel-next.js/) — first hybrid pnpm + Cargo dual-workspace case in the corpus; drift no per-language linter catches because each linter only sees half the tree.
- [`vercel-turbo/`](vercel-turbo/) — Rust monorepo orchestrator; alint adds 22 gates that don't exist.

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
