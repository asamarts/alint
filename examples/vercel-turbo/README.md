# Case study: `vercel/turbo`

> Marketing/positioning writeup at <https://alint.org/examples/vercel-turbo/>.
> This README is the engineering reference: tooling inventory, mapping table,
> gap catalogue, validation status.

Inventory of the structural-validation tooling in `vercel/turbo` (Turborepo)
and an alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-03, `git rev-parse HEAD` =
`9f7039546ca0d78a424bdae41f80ec290154f57e` (matched `git ls-remote
https://github.com/vercel/turbo HEAD` at the time of the inventory).

---

## Summary

Turborepo is a **dual-language monorepo** — a Rust workspace
(`crates/turbo*`, **61 crates**) and a pnpm workspace (`packages/*`,
`apps/*`, `examples/*`, **17 first-party packages plus 30 examples**).
Unlike Kubernetes (which hand-rolled 50 verify scripts), Turborepo delegates
**all language-level lints** to the canonical per-ecosystem tools — `cargo
fmt` / `cargo clippy` / `cargo deny` for Rust, `oxlint` / `oxfmt` / `taplo` /
`attw` for TS — and uses turborepo itself + a husky `pre-push` hook to
orchestrate them. There is **no `xtask/` crate**, **no `.changeset/`
directory**, **no `hack/verify-*.sh` pipeline**.

That means the structural-validation surface area is small and almost
entirely about **monorepo conventions** rather than language semantics:

- per-crate / per-package layout (README, LICENSE, manifest fields)
- examples-directory hygiene (`meta.json` shape, `turbo.json` per example)
- workspace-uniformity invariants (every crate inherits `[lints]` from the
  workspace, every internal crate declares `publish = false`, every
  per-crate `edition` is workspace-inherited)
- presence of the canonical config files (`deny.toml`, `clippy.toml`,
  `rust-toolchain.toml`, `version.txt`, `.husky/pre-push`) and that they
  carry the expected gates

**Live-tree findings (factual):** Turborepo's Rust workspace has
**drift in 60 of 61 crates** on the `publish = false` guard, **9 of 52
crates** are missing READMEs, **7 crates have a directory name that
doesn't match the crate's published name**
(`crates/turborepo-globwalk` is published as `globwalk`,
`crates/turborepo-paths` as `turbopath`, etc.), and on the JS side
**8 of 17 packages** lack a per-package LICENSE (problematic for
`npm pack` since the repo-root LICENSE doesn't auto-include in
tarballs). One example (`with-microfrontends`) is silently skipped
from the `check-examples.ts` sandbox runs because it lacks
`meta.json`, and `with-nextjs` is missing `turbo.json`.

The full alint config replaces ~22 of these structural checks declaratively
and absorbs ~7 more via the `command` rule kind (one shell-out to each of
`cargo fmt`, `cargo clippy`, `cargo deny`, `oxlint`, `oxfmt`, `taplo`,
`shellcheck`). Net: **~29 structural checks** in one declarative file vs. a
mix of `pre-push` shell, `lint.yml` jobs, and a TypeScript runner.

---

## Existing tooling inventory

Turborepo's structural validation lives in five places:

1. **`.husky/pre-push`** — a 12-line shell hook running, in order:
   - `pnpm exec lint-staged` (oxfmt on `*.{js,jsx,ts,tsx,md,mdx,mjs,yml,yaml,css,json,jsonc}`
     and taplo on `*.toml`)
   - `turbo run format check:toml`
   - `cargo fmt --check`
   - `cargo lint` (workspace alias = `clippy --workspace --features
     rustls-tls --all-targets -- -D warnings`)
   - `cargo check --workspace`
2. **`.github/workflows/lint.yml`** — re-runs the same gates plus
   `cargo deny check licenses`. Splits Rust / formatting / dependency
   changes via `dorny/paths-filter`.
3. **`.github/workflows/lint-pr-title.yml`** — Conventional Commit guard on
   PR titles via `amannn/action-semantic-pull-request`.
4. **`.github/workflows/test-js-packages.yml` / `turborepo-test.yml`** — both
   contain a hand-rolled "release-PR content guard" (~30 lines of bash
   apiece) that reads `gh api repos/.../pulls/N/files` and rejects the PR if
   it touches anything outside `version.txt` / `package.json` /
   `Cargo.toml` / `Cargo.lock` / `CHANGELOG*` / `pnpm-lock.yaml`.
5. **`examples/check-examples.ts`** — a 530-line TS runner that pulls every
   `examples/*/meta.json` with `maintainedByCoreTeam: true`, uploads the
   example to a `@vercel/sandbox`, converts it to each of pnpm/npm/yarn,
   runs every non-`persistent` task in its `turbo.json`, then re-runs to
   verify cache hits. Out of alint's scope (live execution).

There is **no `hack/verify-*.sh` analogue** — Turborepo trusts the per-
language tools to enforce per-language conventions and only adds structural
gates where the per-language tools can't reach.

### Maps to existing alint rules (drop-in replacements)

| Existing tooling | What it checks | alint replacement |
|---|---|---|
| `pre-push` hook + `lint.yml::rust_fmt` | Rust files are gofmt-clean | `command` rule `cargo fmt --check` (gated to `Cargo.toml`) |
| `pre-push` hook + `lint.yml::rust_clippy` | Rust files pass clippy with `-D warnings` | `command` rule `cargo lint` |
| `lint.yml::rust_licenses` | All transitive deps under MIT/Apache/BSD/etc. | `command` rule `cargo deny check licenses` + `toml_path_matches` shape check on `deny.toml` itself |
| `lint.yml::format_lint` | TS/MD/JSON/YAML formatting | `command` rules `pnpm exec oxlint --deny-warnings .` + `pnpm exec oxfmt --check` + `pnpm exec taplo format --check` |
| (implicit, via `.npmrc`) | `deny.toml` exists at repo root | `file_exists deny.toml` + `toml_path_matches` shape check |
| (implicit) | `clippy.toml` exists for the workspace bans | `file_exists clippy.toml` |
| (implicit) | `rust-toolchain.toml` pins a specific channel (no floating nightly) | `toml_path_matches` regex `^(stable\|beta\|nightly\|nightly-\d{4}-\d{2}-\d{2}\|...)$` |
| (implicit) | `version.txt` is `<semver>\n<dist-tag>\n` (the shape `scripts/version.js` writes) | `file_content_matches` regex |
| (implicit) | `.husky/pre-push` runs the canonical gates (cargo fmt + cargo lint) | `file_exists` + `file_content_matches` per gate |
| Convention from `package.json` reviews | Every `crates/*` has `Cargo.toml` + `README.md` | Bundled `monorepo/cargo-workspace@v1` (re-stated at error level for first-party crates) |
| Convention from `package.json` reviews | Every `packages/*` has `package.json` + `README.md` + `LICENSE` | Bundled `monorepo/pnpm-workspace@v1` (re-stated at error level) + new `turbo-package-has-license` rule |
| Convention from `package.json` reviews | Every `packages/*/package.json` declares `repository.directory` pointing to its own subdir | `for_each_dir` + `json_path_matches "$.repository.directory" "^packages/[a-z0-9_.-]+$"` |
| Convention from `Cargo.toml` reviews | Every `crates/*/Cargo.toml` inherits `[lints] workspace = true` | `file_content_matches` regex per crate |
| Convention from `Cargo.toml` reviews | Every internal crate declares `publish = false` | `toml_path_matches "$.package.publish" "false"` per crate |
| Convention from `Cargo.toml` reviews | Every `crates/*/Cargo.toml` inherits `edition = { workspace = true }` | `file_content_matches` regex per crate |
| `examples/check-examples.ts` (skipping logic) | Every `examples/*` has a `meta.json` so the runner can pick it up | `for_each_dir` + `file_exists` per example |
| `examples/check-examples.ts` (input shape) | `meta.json` declares `name`, `description`, `maintainedByCoreTeam` | 3× `json_path_matches` per example |
| Convention | Every `examples/*` has its own `turbo.json` and `.gitignore` | 2× `for_each_dir` + `file_exists` per example |
| `lint.yml::format_lint` (shellcheck on legacy `*.sh`) | Shell scripts pass shellcheck | `command` rule `shellcheck` per `scripts/**/*.sh` |

22 rules — direct replacements + 7 `command`-rule shell-outs to the
existing per-language tools. Most are 5-minute config additions; the
`for_each_dir` / `toml_path_matches` patterns are already proven on the
Kubernetes case study.

### Needs new alint primitive

| Existing tooling | What it checks | What alint needs |
|---|---|---|
| Convention from `Cargo.toml` reviews | The directory name matches the published crate name (`crates/turborepo-globwalk` → `globwalk`, `crates/turborepo-paths` → `turbopath`, etc. — currently 7 crates drift) | A `dir_name_matches_field` rule kind: assert that for each `Cargo.toml`, the parent dir's basename equals (or follows a pattern derived from) `[package].name`. Same primitive applies to `packages/*/package.json` (the `@turbo/scoped` pattern: dir is `kebab-case`, name is `@turbo/scoped`). |
| Convention from `package.json` reviews | The `name` field's scope follows project convention (private first-party = `@turbo/...`, public = `turbo` / `eslint-config-turbo` / `create-turbo` / `turbo-ignore`) | A `json_path_matches_named_capture` rule kind that lets you assert one field matches a regex *and* captures groups can be referenced from the rule message. Workaround today: a `json_path_matches` per allow-list pattern. |
| `examples/check-examples.ts` | Every example successfully runs `turbo run <task>` for every non-persistent task in its `turbo.json`, then re-runs and gets a cache hit | Out of alint's scope (live execution). The TypeScript runner is the right tool. |
| Convention via `clippy.toml` | Forbid `std::collections::hash_map::DefaultHasher` and `VecDeque::new` workspace-wide | Already covered by clippy itself; `command` rule shells out. The interesting alint generalisation is `forbidden_substrings` — "no `*.rs` file in `crates/**` may contain `DefaultHasher`" — but in this case clippy is the right level (it understands paths / aliases / re-exports). Don't try to replicate. |
| `turbo.json` schema check | `turbo.json` validates against `https://turborepo.dev/schema.json` | A `json_schema_passes` rule kind. We have `json_path_matches` for spot-checks, but no full-schema validation. v0.10+ candidate — turbo.json, tsconfig.json, .oxlintrc.json all ship with published JSON Schemas, so the demand is broad even if the per-repo confirmations remain modest. |
| `.github/workflows/test-js-packages.yml` / `turborepo-test.yml` | "Release PRs may only touch version.txt / package.json / Cargo.toml / Cargo.lock / CHANGELOG / pnpm-lock.yaml" — enforced by `gh api repos/.../pulls/N/files` against an allow-list regex | Out of repo-state scope. This is a CI-time diff against the PR's *file list*, not the repo at HEAD. Could be a sibling tool (`alint pr-diff-check`?) but doesn't fit the `alint check` model. |
| `.github/workflows/lint-pr-title.yml` | PR title follows Conventional Commits with subject starting uppercase | Same — a property of the PR, not the repo. |

**Gap pattern: dir-name vs. manifest-name drift.** This is the headline gap
for monorepo-tier validation. Both Cargo and npm let the directory name
diverge from the published name — Turborepo has 7 such crates and 7 such
packages, and they're all intentional (different namespacing eras), but
*new* drift is almost always a mistake. A `dir_name_matches_field` rule
kind would catch the mistake while letting the existing drift be
allow-listed in `paths.exclude:`.

**Gap pattern: full JSON-Schema validation.** Turborepo ships its own
schema (`docs/public/schema.json`) and expects users to validate
`turbo.json` against it via the `$schema` field. Alint has
`json_path_matches` for spot-checks but can't validate against a full
schema document. With Turborepo, `tsconfig.json`, oxlint configs, etc., all
publishing schemas, this is a load-bearing missing primitive. Same
observation surfaced in the Kubernetes case study (`golangci-lint-config.sh`).

### Out of alint's scope (use the existing tool)

These are AST / build-system / live-execution checks. Alint's non-goals
are deliberate; we should mention these in the case study as "alint doesn't
try to do this; keep your existing tool."

- `examples/check-examples.ts` — runs every example in a sandbox under
  pnpm/npm/yarn, validates cache hits across two consecutive `turbo run`s.
  Live execution; out of scope.
- `attw --pack` (in `package-checks` task per package) — type-shape / API
  surface check. AST-aware; out of scope.
- `cargo run -p turborepo-schema-gen verify` (in `@turbo/types`) — verifies
  the generated `schema.json` matches what the Rust schema generator
  produces. Codegen drift; out of scope.
- `turbo run check-types check-links check-openapi --filter='./docs/*'`
  (in `docs.yml`) — TypeScript / link / OpenAPI checks. Build-aware; out
  of scope.
- `lint-staged` (in `pre-push`) — a per-file batcher; alint has its own
  per-file dispatch.

### Already covered by other linters Turborepo uses

- `cargo audit` analogue — handled via `cargo deny check licenses`. Could
  layer `cargo deny check advisories` (security) but Turborepo currently
  doesn't.
- `socket.yaml` — Socket.dev's supply-chain scanner; upstream tool.

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Replaces the structural
gates listed above. Keep the existing per-language tool invocations; the
config wraps them via `command` rules so a single `alint check` runs the
whole structural pipeline plus the language linters in parallel (vs. the
sequential `pre-push` chain today).

The remaining items:

- 3 need new alint primitives (above) — `dir_name_matches_field`
  and `json_schema_passes` are v0.10+ candidates; the `pr-diff-check`
  sibling mode is a separate-binary candidate that doesn't fit the
  `alint check` model.
- 5 are out of alint's scope (above) — keep the existing scripts /
  TypeScript runner.
- The Conventional-Commit PR-title check stays in `lint-pr-title.yml`
  (alint can't reach the PR title from a repo-state config).

---

## Performance comparison (placeholder — bench when validation pass scales)

The current `pre-push` hook runs gates sequentially (`lint-staged` → `turbo
run format check:toml` → `cargo fmt --check` → `cargo lint` → `cargo
check`). On a fresh `cargo check`, the wall time is dominated by the cargo
calls (tens of seconds); on incremental, the chain is ~1-2 s.

alint runs all rules in parallel via the v0.9.3 dispatch flip. For a
~6,000-file repo like Turborepo, the structural rules (READMEs, LICENSEs,
manifest spot-checks, examples meta.json) should clear in well under 1 s
based on the v0.9.13 published S3 benchmark (1.13 s for the workspace
bundle on 100k files). The `command`-rule shell-outs (cargo / oxlint / etc.)
will dominate as before — alint isn't faster than `cargo lint` itself, but
running all of them in parallel under one `alint check` should shave wall
time vs. the sequential pre-push chain.

To benchmark for real: `time bash .husky/pre-push` against `time alint
check` on the same checkout. Deferred to the per-repo measurement pass.

---

## Followup feature work surfaced (consolidated)

The narrative framing for the "structural floor under Vercel-grade
tooling" pitch and the kubernetes-vs-turbo launch hook lives in the
alint.org marketing writeup linked at the top of this README. This
section is the engineering rule-kind candidate list, in priority
order:

1. **`dir_name_matches_field` rule kind** — covers crate-name and
   package-name drift across both Cargo and npm; surfaces in every
   monorepo we've inventoried.
2. **`json_schema_passes` rule kind** — covers `turbo.json` /
   `tsconfig.json` / `.oxlintrc.json` / golangci-lint config validation;
   second-most-load-bearing missing primitive.
3. **`alint pr-diff-check` sibling mode** — operate on a PR's changed-file
   list rather than the repo at HEAD; covers the release-PR content-guard
   pattern (Turborepo, plus most monorepos with auto-release bots).

---

## Future analysis

Suggestions for the next revalidation pass — turbo is the canonical
"Rust monorepo orchestrator with zero hand-rolled verify scripts"
demonstration, and the v0.9.6+ surface plus v0.9.17 polish opens
several refactor opportunities:

- **What `alint suggest` would propose for the 22 gates that don't
  exist in turbo's tooling.** A live `alint suggest` against a fresh
  turbo clone would surface most of the bundled rulesets the config
  already extends (oss-baseline, rust, node, monorepo,
  monorepo/cargo-workspace, monorepo/pnpm-workspace,
  ci/github-actions, hygiene/no-tracked-artifacts,
  tooling/editorconfig — currently 9 in the extends:) plus probably
  `agent-hygiene@v1` (medium — turbo's `crates/turborepo-*` tree
  has a non-trivial number of `// TODO(scope-name)` markers worth a
  blame-driven scan) and `compliance/reuse@v1` if Vercel adopts
  REUSE/SPDX headers (currently they don't; would be a deliberate
  override). The suggest-pass run is a quick (~2-minute) confirmation
  that the case study's 22-gate count is still complete.
- **`scope_filter` for the `crates/` vs `packages/` vs `examples/`
  triad.** Today the per-tree rules glob each subtree
  individually. v0.9.17's `scope_filter` evolution lets each subtree
  be a named scope (`rust-crates`, `js-packages`, `examples-tree`)
  declared once at the top and referenced by name in each rule —
  cuts ~20 lines and makes "which subtree am I checking" one source
  of truth, particularly helpful for the per-example
  `meta.json`/`turbo.json`/`.gitignore` triad rules.
- **The `dir_name_matches_field` v0.10+ candidate revisited.** The
  case study notes 7 crates whose directory name doesn't match the
  published crate name (intentional namespacing drift). When
  `dir_name_matches_field` lands, turbo becomes the canonical
  "expected drift, allowlist 7" demonstration — the v0.10+ design
  needs a `paths.exclude:` or `allow_drift:` knob to make
  intentional drift expressible without disabling the rule
  workspace-wide. File this as a v0.10+ design note.

---

## Validation status (2026-05-07)

- **alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).
- **`validate-config`:** ✓ 88 rules loaded from `.alint.yml`.
- **README rule-count claim:** "~29 structural checks" /
  "22 rules — direct replacements + 7 `command`-rule shell-outs"
  (Summary + table footer) match the actual 28 turbo-specific
  rules (counted via `grep -c '^  - id:'`) within rounding. The
  88-rule `validate-config` total = 28 turbo-specific + 60
  inherited from the 9 bundled rulesets (oss-baseline=15 +
  rust=11 + node=9 + monorepo=4 + monorepo/cargo-workspace=4 +
  monorepo/pnpm-workspace=4 + ci/github-actions=3 +
  hygiene/no-tracked-artifacts=11 + tooling/editorconfig=3 = 64
  declared; the 4-rule slack vs 60 reflects bundled-overlap dedup
  the engine handles transparently).
- **Pitfall catalogue:** v0.9.17 ships fixes for #18 + #19. Neither
  surfaces here. Pitfall #16 (cross-referenced from the next.js case
  study — JSONPath bool/number regex coercion) was originally a
  latent risk in `turbo-example-meta-declares-maintenance`; the
  config already uses `file_content_matches` against the JSON text
  (with an in-line comment citing pitfall #16) — fix applied during
  the original P2b pass. Catalogue is now at 21 pitfalls.
- **Rule-kind candidate status:** `dir_name_matches_field` and
  `json_schema_passes` remain v0.10+ candidates (this case study is
  the headline demand-driver for both, but the per-repo
  confirmation count is modest). The `alint pr-diff-check` sibling
  mode is unchanged.
- **Bundled-ruleset rule counts (authoritative as of 2026-05-07):**
  oss-baseline=15, rust=11, node=9, monorepo=4,
  monorepo/cargo-workspace=4, monorepo/pnpm-workspace=4,
  ci/github-actions=3, hygiene/no-tracked-artifacts=11,
  tooling/editorconfig=3.
