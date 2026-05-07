# Examples

Real-world `.alint.yml` configurations from the launch-prep validation pass
(see [`docs/development/launch-evidence.md`](../docs/development/launch-evidence.md)). Each subdirectory is one
case study — a popular OSS repo's existing structural-validation tooling
inventoried, rebuilt as an alint config, and compared.

## Layout

```
examples/
├── README.md                          # this file
├── <owner>-<repo>/
│   ├── README.md                      # case study writeup
│   ├── .alint.yml                     # the alint config that matches their existing tooling
│   ├── existing-tooling.md            # inventory of what they enforce today
│   └── comparison.md                  # alint output vs existing tool output + perf delta
```

## Case studies

**P2a complete — 20 of 20 single-language + diverse-ecosystem repos. P2b Wave 1+2 — 10 of planned 20 polyglot monorepos.**
Organised by the launch-positioning narrative each one anchors:

### "Replaces N hand-rolled validation scripts"

For repos with verify-script sprawl that alint can consolidate.

- [`kubernetes-kubernetes/`](kubernetes-kubernetes/) — 50 verify scripts inventoried; alint replaces 17 declaratively.
- [`apache-airflow/`](apache-airflow/) — 109 pre-commit hooks; ~40 % map to alint.
- [`python-cpython/`](python-cpython/) — 56 surfaces inventoried; one alint config consolidates the 38 % that's declarative orchestration.

### "Catches conventions your pipeline assumes but doesn't verify"

For repos that rely on convention without explicit checks.

- [`tokio-rs-tokio/`](tokio-rs-tokio/) — zero hand-rolled scripts; alint catches 15 conventions tokio's pipeline assumes.
- [`astral-sh-uv/`](astral-sh-uv/) — 67-crate workspace conventions enforced nowhere in CI today.
- [`pnpm-pnpm/`](pnpm-pnpm/) — replaces the `meta-updater` plugin's 13 cross-package field invariants without a per-repo plugin install.
- [`facebook-react/`](facebook-react/) — `codes.json` registry shape + `ReactVersion.js` propagated to 3 per-package fields.
- [`nodejs-node/`](nodejs-node/) — 15-year-old conventions enforced via human review only.

### "Adds structural floor on top of mature tooling"

For repos with mature tooling but missing the structural layer.

- [`microsoft-typescript/`](microsoft-typescript/) — eslint + dprint + knip already tight; alint adds structural floor.
- [`astral-sh-ruff/`](astral-sh-ruff/) — 900+ Python lint rules but zero rules for ruff's own internal-crate `publish = false` discipline.
- [`prettier-prettier/`](prettier-prettier/) — 5 net-new gates on top of eslint + prettier + cspell + knip + tsc.
- [`helm-helm/`](helm-helm/) — Trojan-Source defence + GHA hardening on top of golangci-lint.

### "Replaces the structural subset of your custom orchestration layer"

For repos that built their own lint-orchestration tool.

- [`pytorch-pytorch/`](pytorch-pytorch/) — ≈86 % of pytorch's 57 `lintrunner.toml` adapters are structural; alint sits beneath, lintrunner keeps the AST-aware tail.

### "Encodes conventions enforced only by code-review discipline"

For tightly-curated minimal-tooling repos.

- [`golang-go/`](golang-go/) — zero `.github/workflows/`, zero `Makefile`, zero `.golangci.yml`; the 31-rule alint config encodes Russ Cox & co.'s discipline for the first time anywhere in the project.
- [`rust-lang-rust/`](rust-lang-rust/) — `src/tools/tidy/` is a custom Rust binary doing alint's job; ~13 of ~32 tidy checks become declarative.

### Polyglot wins (P2a wave + P2b Wave 1+2 = 12 polyglot case studies)

P2a polyglot:

- [`vercel-next.js/`](vercel-next.js/) — first hybrid pnpm + Cargo dual-workspace win. *"Drift no per-language linter catches because each linter only sees half the tree."*
- [`apache-arrow/`](apache-arrow/) — **flagship polyglot case**: 6 languages in one tree, 21 lint hooks across 14 tool repos, 0 tools that see cross-language conventions. *"alint is the layer that does."* Live findings against the actual arrow clone: 16 source files missing the Apache header (all listed in `dev/release/rat_exclude_files.txt`).

**P2b Wave 1 — 5 of 5, scale + governance + flagship-visibility:**

- [`nixos-nixpkgs/`](nixos-nixpkgs/) — **scale stress**: 39,101 files + 20,678 `pkgs/by-name/*/*/` package directories. The full 79-rule pass — including the headline `for_each_dir` over the by-name tree — completes in **273 ms wall-clock**. "Any size repo" launch claim is now empirically defensible.
- [`bazelbuild-bazel/`](bazelbuild-bazel/) — surfaces pitfall #18 (the `.bazelversion` tracked-AND-gitignored pattern) which v0.9.16 fixes via the per-rule `respect_gitignore: false` knob.
- [`tensorflow-tensorflow/`](tensorflow-tensorflow/) — 1,185 textproto API goldens under `tensorflow/python/tools/api/golden/{v1,v2}/`. Demand-validates the v0.11+ `cross_language_implementation_complete` candidate at TWO topologies (per-source ↔ per-test within one language; core ↔ N bindings across languages).
- [`apache-spark/`](apache-spark/) — 49 `pom.xml` files. New v0.10 ship-target rule kind: `xml_path_matches` / `xml_path_equals` (completes the structured-query family — JSON/YAML/TOML/XML).
- [`microsoft-vscode/`](microsoft-vscode/) — apples-to-apples vs `build/hygiene.ts`. alint covers ~75% of the 8 distinct hygiene checks (6 of 8) declaratively in one config; verified against the live tree (222 violations, zero false positives). *"alint is what `build/hygiene.ts` would look like as a tool, not a per-repo script."*

**P2b Wave 2 — 5 of 5, platform-driven polyglot density:**

- [`angular-angular/`](angular-angular/) — TypeScript framework with 16 packages. `goldens/public-api/<pkg>/index.api.md` discipline locks the TS API surface of 13 of 16 packages — canonical single-language `cross_language_implementation_complete` instance.
- [`istio-istio/`](istio-istio/) — Single-module Go monorepo with 9 Helm charts, Prow CI, CODEOWNERS not k8s-OWNERS. Per-chart image-hub at *different* JSONPath positions per file — surfaces pitfall #20 (cross-file value-equality with per-file extractor) + a `value_extractor:` design candidate.
- [`dotnet-runtime/`](dotnet-runtime/) — **1,091 .csproj files** (sparse checkout) + 234 solution files + 257 Directory.Build.{props,targets} + 520 .props/.targets ≈ **2,300 distinct XML manifests**. Stress-tests the v0.10 `xml_path_*` candidate at one order of magnitude bigger scale than spark. New bundled ruleset `dotnet@v1` added to v0.10 ship-target.
- [`protocolbuffers-protobuf/`](protocolbuffers-protobuf/) — **densest single-repo source for v0.11+ `cross_language_implementation_complete`**: 10 in-tree language bindings (cpp, java, python, csharp, ruby, php, objc, hpb, upb, rust) + 1 spun-out (dart) with per-binding wire-format failure-allowlist files (`failure_list_<lang>.txt`) and per-binding GHA test workflow. ~45 cross-language assertions one rule would express.
- [`flutter-flutter/`](flutter-flutter/) — **platform-driven polyglot variant**: single Dart framework, native-OS embedders (Android/iOS/macOS/Linux/Windows/Fuchsia/GLFW + ABI) as peer subdirs under `engine/src/flutter/shell/platform/`, each implementing the same surface. **Live tree run catches 5 real Trojan-Source / [CVE-2021-42574](https://nvd.nist.gov/vuln/detail/CVE-2021-42574) errors** in `docs/releases/archive/` via `oss-baseline`'s `no_bidi_controls` — strongest single piece of "alint catches things other tools miss" evidence in the corpus.

### Other case studies

- [`denoland-deno/`](denoland-deno/) — Rust + JS + TS multi-language; custom validation scripts.
- [`vercel-turbo/`](vercel-turbo/) — Rust monorepo orchestrator; alint adds 22 gates that don't exist.
- [`clap-rs-clap/`](clap-rs-clap/) — Rust workspace; per-member inheritance via `for_each_dir` over family crates.

P2b reached 10 of the planned 20 polyglot repos for the v0.9.17 ship; remaining 10 stay queued as ongoing post-launch evidence-driven content marketing.

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
