# Examples

Real-world `.alint.yml` configurations from the launch-prep validation pass
(see [`docs/launch-prep.md`](../docs/launch-prep.md)). Each subdirectory is one
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

**P2a complete — 20 of 20 single-language + diverse-ecosystem repos.**
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

### Polyglot wins (anticipating P2b)

- [`vercel-next.js/`](vercel-next.js/) — first hybrid pnpm + Cargo dual-workspace win. *"Drift no per-language linter catches because each linter only sees half the tree."*
- [`apache-arrow/`](apache-arrow/) — **flagship polyglot case**: 6 languages in one tree, 21 lint hooks across 14 tool repos, 0 tools that see cross-language conventions. *"alint is the layer that does."* Live findings against the actual arrow clone: 16 source files missing the Apache header (all listed in `dev/release/rat_exclude_files.txt`).

### Other case studies

- [`denoland-deno/`](denoland-deno/) — Rust + JS + TS multi-language; custom validation scripts.
- [`vercel-turbo/`](vercel-turbo/) — Rust monorepo orchestrator; alint adds 22 gates that don't exist.
- [`clap-rs-clap/`](clap-rs-clap/) — Rust workspace; per-member inheritance via `for_each_dir` over family crates.

P2b (20 polyglot monorepos) is queued as ongoing post-launch evidence-driven content marketing.

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

The per-repo workflow ([`docs/launch-prep.md`](../docs/launch-prep.md#per-repo-workflow-2-4-hr-per-repo)) describes the steps.
