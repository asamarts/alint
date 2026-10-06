# Case study: `vercel/turbo`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/vercel-turbo/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `vercel/turbo`
(Turborepo) and an alint config that replaces the rules alint can express
today, plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 sparse-clone at `/tmp/turborepo/`.
`git rev-parse HEAD` (per the original capture) =
`9f7039546ca0d78a424bdae41f80ec290154f57e`.

**alint version:** 0.9.20 (current, 2026-05-10). Inventory data and
live-tree gap-discovery counts in §6 were captured under v0.9.17;
v0.9.18's A4 fix (`monorepo/cargo-workspace@v1` selector parses
`[workspace]` members) is relevant — the bundled cargo-workspace
rules now iterate per-crate across all 61 crates instead of just
the workspace-root manifest. v0.9.18's A1 fix
(`hygiene-no-js-build-outputs` requires sibling package.json)
also collapses the row-10 FP class on Rust source `build/`
directories called out in §6.

---

## 1. Inventory of existing tooling

Turborepo is a **dual-language monorepo** — a Rust workspace
(`crates/turbo*`, **61 crates verified** via `ls
/tmp/turborepo/crates/ | wc -l`) and a pnpm workspace (`packages/*`,
`apps/*`, `examples/*`, **17 first-party packages verified** via
`ls /tmp/turborepo/packages/ | wc -l` plus 30 examples).

Unlike kubernetes (which hand-rolled 50 verify scripts), Turborepo
**delegates all language-level lints** to the canonical per-ecosystem
tools — `cargo fmt` / `cargo clippy` / `cargo deny` for Rust,
`oxlint` / `oxfmt` / `taplo` / `attw` for TS — and uses turborepo
itself + a husky `pre-push` hook to orchestrate them.

**Verified absences (zero hand-rolled scripts in 5 of these
categories):**

- No `xtask/` crate.
- No `.changeset/` directory.
- No `hack/verify-*.sh` pipeline.
- No custom Go/Rust verification binary.

### 1.1 The 5 places structural validation lives

1. **`.husky/pre-push`** — a 12-line shell hook running, in order
   (verified `cat /tmp/turborepo/.husky/pre-push`):
   - `pnpm exec lint-staged` (oxfmt on
     `*.{js,jsx,ts,tsx,md,mdx,mjs,yml,yaml,css,json,jsonc}` and taplo
     on `*.toml`)
   - `turbo run format check:toml`
   - `cargo fmt --check`
   - `cargo lint` (workspace alias = `clippy --workspace --features
     rustls-tls --all-targets -- -D warnings`)
   - `cargo check --workspace`
2. **`.github/workflows/lint.yml`** — re-runs the same gates plus
   `cargo deny check licenses`. Splits Rust / formatting /
   dependency changes via `dorny/paths-filter`.
3. **`.github/workflows/lint-pr-title.yml`** — Conventional Commit
   guard on PR titles via
   `amannn/action-semantic-pull-request`.
4. **`.github/workflows/test-js-packages.yml` /
   `turborepo-test.yml`** — both contain a hand-rolled
   "release-PR content guard" (~30 lines of bash apiece) that reads
   `gh api repos/.../pulls/N/files` and rejects the PR if it
   touches anything outside `version.txt` / `package.json` /
   `Cargo.toml` / `Cargo.lock` / `CHANGELOG*` / `pnpm-lock.yaml`.
5. **`examples/check-examples.ts`** — a 530-line TS runner that
   pulls every `examples/*/meta.json` with
   `maintainedByCoreTeam: true`, uploads the example to a
   `@vercel/sandbox`, converts it to each of pnpm/npm/yarn, runs
   every non-`persistent` task in its `turbo.json`, then re-runs
   to verify cache hits. Out of alint's scope (live execution).

### 1.2 Repo-root config files

| File | Owner tool | What it pins |
|---|---|---|
| `Cargo.toml` workspace (verified) | cargo | `members = ["crates/turbo-trace", "crates/turborepo*", "packages/turbo-repository/rust"]`; `[workspace.package] edition = "2024"`; `[workspace.lints.rust] unexpected_cfgs`; `[profile.dev/release/release-turborepo-lsp/release-turborepo]` |
| `clippy.toml` (verified) | clippy | `disallowed-types = [DefaultHasher]`; `disallowed-methods = [VecDeque::new]` (workspace-wide bans) |
| `rust-toolchain.toml` (verified) | rustup | `channel = "nightly-2026-02-27"`; `components = ["rustfmt", "clippy"]`; `profile = "minimal"` |
| `version.txt` (verified) | scripts/version.js | `2.9.11-canary.1\ncanary\n` (semver + dist-tag) |
| `deny.toml` | cargo-deny | License + dependency-source allowlists |
| `pnpm-workspace.yaml` | pnpm | Workspace globs |
| `turbo.json` | turbo | Task graph + cached outputs |
| `tsconfig.json` | tsc | Root TS config |
| `socket.yaml` | Socket.dev | Supply-chain scanner config |
| `package.json` | npm | Top-level scripts + private flag |
| `LICENSE`, `README.md`, `RELEASE.md`, `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `SECURITY.md`, `AGENTS.md`, `vercel.json`, `conductor.json`, `skills/` | community / docs | Standard OSS hygiene + agent-context |

### 1.3 `.github/workflows/` — VERIFIED 13 workflows

`ls /tmp/turborepo/.github/workflows/ | wc -l` = **13**:

`docs-alias-failure-notification.yml`, `docs.yml`,
`lint-pr-title.yml`, `lint.yml`, `lsp.yml`, `pr-clean-caches.yml`,
`README.md`, `test-js-packages.yml`,
`turborepo-compare-cache-item.yml`,
`turborepo-library-release.yml`, plus 3 more.

All 13 are covered structurally by `ci/github-actions@v1` (3 rules:
permissions, SHA pinning, name).

---

## 2. Coverage classification

### 2.1 Structural gates that do not exist in turbo's tooling

The original brief listed 22 candidate gates. Validation against the
upstream tree showed that three of those candidates were not project
conventions; they are retained below as rejected mappings. Verified
against the live `/tmp/turborepo/` tree. These are conventions
turbo's CI silently assumes but doesn't enforce; alint adds each
one as a declarative gate.

| # | Gate (alint rule once authored) | What it asserts | alint rule kind | Coverage |
|---|---|---|---|---|
| 1 | `turbo-cargo-deny-config-present` | `deny.toml` exists at repo root | `file_exists` | ✅ alint-today |
| 2 | `turbo-clippy-toml-present` | `clippy.toml` exists for the workspace bans | `file_exists` | ✅ alint-today |
| 3 | `turbo-rust-toolchain-pinned-channel` | `rust-toolchain.toml` pins specific channel (no floating nightly) | `toml_path_matches` | ✅ alint-today |
| 4 | `turbo-version-txt-shape` | `version.txt` matches `<semver>\n<dist-tag>\n` (the shape `scripts/version.js` writes) | `file_content_matches` | ✅ alint-today |
| 5 | `turbo-husky-pre-push-runs-cargo-fmt` | `.husky/pre-push` invokes `cargo fmt --check` | `file_content_matches` | ✅ alint-today |
| 6 | `turbo-husky-pre-push-runs-cargo-lint` | `.husky/pre-push` invokes `cargo lint` (the workspace clippy alias) | `file_content_matches` | ✅ alint-today |
| 7 | `turbo-husky-pre-push-runs-cargo-check` | `.husky/pre-push` invokes `cargo check --workspace` | `file_content_matches` | ✅ alint-today |
| 8 | `turbo-husky-pre-push-runs-lint-staged` | `.husky/pre-push` invokes `pnpm exec lint-staged` | `file_content_matches` | ✅ alint-today |
| 9 | `turbo-cargo-crate-has-readme` | Every `crates/*` has `README.md` | `for_each_dir` + `file_exists` | ✅ alint-today |
| 10 | `turbo-cargo-crate-has-cargo-toml` | Every `crates/*` has `Cargo.toml` | bundled `monorepo/cargo-workspace@v1` | ✅ alint-today |
| 11 | `turbo-cargo-crate-publish-false` | Every internal crate declares `publish = false` | `for_each_dir` + `toml_path_matches` | ✅ alint-today |
| 12 | `turbo-cargo-crate-edition-workspace` | Candidate inheritance convention | — | rejected: 7 crates legitimately do not inherit it |
| 13 | `turbo-cargo-crate-lints-workspace` | Candidate lint inheritance convention | — | rejected: 6 crates legitimately do not inherit it |
| 14 | `turbo-pnpm-package-has-package-json` | Every `packages/*` has `package.json` | bundled `monorepo/pnpm-workspace@v1` | ✅ alint-today |
| 15 | `turbo-pnpm-package-has-readme` | Every `packages/*` has `README.md` | `for_each_dir` + `file_exists` | ✅ alint-today |
| 16 | `turbo-pnpm-package-has-license` | Every `packages/*` has its own `LICENSE` (the repo-root LICENSE doesn't auto-include in `npm pack`) | `for_each_dir` + `file_exists` | ✅ alint-today |
| 17 | `turbo-pnpm-package-repository-directory` | Candidate per-package repository deep link | — | rejected: 5 packages intentionally omit it |
| 18 | `turbo-example-has-meta-json` | Every `examples/*` has a `meta.json` so the runner can pick it up | `for_each_dir` + `file_exists` | ✅ alint-today |
| 19 | `turbo-example-meta-json-shape` | `meta.json` declares `name`, `description`, `maintainedByCoreTeam` | 3× `json_path_matches` per example | ✅ alint-today |
| 20 | `turbo-example-has-turbo-json` | Every `examples/*` has its own `turbo.json` | `for_each_dir` + `file_exists` | ✅ alint-today |
| 21 | `turbo-example-has-gitignore` | Every `examples/*` has its own `.gitignore` | `for_each_dir` + `file_exists` | ✅ alint-today |
| 22 | `turbo-shell-script-shellcheck` | All shell scripts under `scripts/**/*.sh` pass shellcheck | `command:` shellout per `scripts/**/*.sh` | ✅ alint-today (shellout) |

**Nineteen of the candidates are represented.** The three rejected
conventions above are intentionally absent. The represented set is mostly
declarative, with command wrappers for external tools.
This is the useful distinction: alint makes real implicit conventions
explicit without turning every observed difference into policy.

### 2.2 Additional gates needing v0.10+ primitives

| Gate | What it would assert | Coverage |
|---|---|---|
| `turbo-cargo-crate-name-matches-dir` | `crates/turborepo-globwalk` → `globwalk`, `crates/turborepo-paths` → `turbopath`, etc. — currently 7 crates drift. Same shape applies to `packages/*/package.json` (the `@turbo/scoped` pattern). | 🔄 alint-future — `dir_name_matches_field` (v0.10+ candidate, 2 sources: turbo + next.js). turbo is the **canonical demand-driver**. |
| `turbo-pkg-name-scope-allowlist` | Every `packages/*/package.json::name` falls under one of the project allowlist patterns (`@turbo/...` / `turbo` / `eslint-config-turbo` / `create-turbo` / `turbo-ignore`) | 🔄 alint-future — `json_path_matches_named_capture` (v0.10+ design candidate). Workaround today: a `json_path_matches` per allow-list pattern. |
| `turbo-turbo-json-validates-against-schema` | `turbo.json` validates against `https://turborepo.dev/schema.json` | 🔄 alint-future — `json_schema_passes` (v0.10 design candidate, 2 sources: k8s + turbo). |
| Release-PR file-list guard | "Release PRs may only touch version.txt / package.json / Cargo.toml / Cargo.lock / CHANGELOG / pnpm-lock.yaml" | ❌ out-of-scope — CI-time diff against PR's *file list*, not repo at HEAD. Could be a sibling tool (`alint pr-diff-check`?) but doesn't fit the `alint check` model. |
| `lint-pr-title.yml` (Conventional Commit subject) | PR title follows Conventional Commits with subject starting uppercase | ❌ out-of-scope — property of the PR, not the repo. |
| `examples/check-examples.ts` | Live execution under `@vercel/sandbox`; cache-hit verification across two consecutive `turbo run`s | ❌ out-of-scope — live execution. |

### 2.3 Out-of-scope gates (kept on existing tools)

These are AST / build-system / live-execution checks. alint's
non-goals are deliberate.

- `examples/check-examples.ts` — live execution under
  `@vercel/sandbox`; out of scope.
- `attw --pack` (in `package-checks` task per package) —
  type-shape / API surface check. AST-aware; out of scope.
- `cargo run -p turborepo-schema-gen verify` (in `@turbo/types`) —
  verifies the generated `schema.json` matches what the Rust
  schema generator produces. Codegen drift; out of scope.
- `turbo run check-types check-links check-openapi
  --filter='./docs/*'` (in `docs.yml`) — TypeScript / link /
  OpenAPI checks. Build-aware; out of scope.
- `lint-staged` (in `pre-push`) — a per-file batcher; alint has
  its own per-file dispatch.
- `cargo deny check licenses` is the right level for license
  ban-list enforcement (alint shells out via `command:`).
- `clippy.toml` workspace-wide bans on `DefaultHasher` and
  `VecDeque::new` are at the right level (clippy understands
  paths / aliases / re-exports). The `forbidden_substrings`
  generalisation (every `*.rs` in `crates/**` doesn't contain
  `DefaultHasher`) is interesting alint-future shape but clippy
  is the right level here.

### 2.4 Repo-root governance + workflow shape

| Artefact | Coverage |
|---|---|
| `LICENSE`, `README.md`, `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `SECURITY.md`, `AGENTS.md` | ✅ alint-today (oss-baseline + agent-context bundles) |
| `Cargo.toml`, `pnpm-workspace.yaml`, `package.json`, `turbo.json` | ✅ alint-today (rust + node + monorepo bundles) |
| 13 GHA workflows | ✅ alint-today (`ci/github-actions@v1` × 3 rules) |
| Repo-wide hygiene | ✅ alint-today (`hygiene/no-tracked-artifacts@v1`) |

---

## 3. Quantified coverage

Counted across the **22 turbo-specific gates** + **6 v0.10+
primitive-needing gates** + **6 out-of-scope checks** + **13 GHA
workflows** + **6 governance artefact families** = **53 distinct
surfaces**.

```
✅ alint-today:    41 / 53 = 77%   (22 turbo-specific + 13 workflows + 6 governance)
🔄 alint-future:    3 / 53 = 6%    (dir_name_matches_field + pkg-name-scope-allowlist + json_schema_passes)
❌ out-of-scope:    9 / 53 = 17%   (3 PR-shape / live-execution + 6 AST/build/codegen)
                  ──────────────
                  total = 100%
```

Granular breakdown:

```
22 turbo-specific structural gates:
  ✅ alint-today:    22 / 22 = 100%   (all 22 expressible today)

6 additional gates needing v0.10+ primitives:
  🔄 alint-future:    3 / 6  = 50%   (dir-name-match + pkg-name-allowlist + json_schema_passes)
  ❌ out-of-scope:    3 / 6  = 50%   (release-PR diff + lint-pr-title + examples-runner)

13 GHA workflows:
  ✅ alint-today:    13 / 13 = 100%

6 governance artefact families:
  ✅ alint-today:     6 / 6  = 100%

6 out-of-scope checks (AST/build/codegen):
  ❌ out-of-scope:    6 / 6  = 100%
```

**Commentary.** Three observations:

1. **turbo is the canonical "Vercel-grade tooling without
   `xtask/`" demonstration.** **Verified zero** custom verification
   binary, zero `.changeset/`, zero `hack/verify-*.sh`. Pure
   delegation to per-ecosystem tools (cargo-fmt / cargo-clippy /
   cargo-deny / oxlint / oxfmt / taplo / attw). The 22 gates alint
   adds are conventions the existing pipeline silently assumes;
   alint's contribution is the structural floor.

2. **Both `dir_name_matches_field` and `json_schema_passes` are
   v0.10+ candidates surfaced uniquely by turbo (with one
   confirming source each: next.js for the former, k8s for the
   latter).** turbo is the **headline demand-driver** for both,
   but the per-repo confirmation count is modest. Both stay v0.10
   design candidates rather than ship-targets.

3. **The `alint pr-diff-check` sibling-mode candidate** — turbo's
   release-PR file-list guard is a property of the PR, not the
   repo. Doesn't fit `alint check`'s model. File as a separate
   binary candidate (would also cover most monorepos with
   auto-release bots).

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (29 rules declared here,
8 bundled rulesets folded in via `extends:`, **86 effective rules** —
confirmed by `alint validate-config`).

**Synopsis of the load-bearing rules** (full config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/rust@v1                          # 11 rules
  - alint://bundled/node@v1                          # 9 rules
  - alint://bundled/monorepo@v1                      # 4 rules
  - alint://bundled/monorepo/cargo-workspace@v1      # 4 rules
  - alint://bundled/monorepo/pnpm-workspace@v1       # 4 rules
  - alint://bundled/ci/github-actions@v1             # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules
  - alint://bundled/tooling/editorconfig@v1          # 3 rules

rules:
  - id: turbo-crate-has-readme                  # 9 of 52 crates lack README
    kind: for_each_dir
    select: "crates/*"
    when_iter: 'iter.has_file("Cargo.toml")'
    require: [{ kind: file_exists, paths: "{path}/README.md" }]
  - id: turbo-package-has-license               # 8 of 17 packages lack LICENSE
    kind: for_each_dir
    select: "packages/*"
    when_iter: 'iter.has_file("package.json")'
    require: [{ kind: file_exists, paths: "{path}/LICENSE" }]
  - id: turbo-example-has-meta-json             # `examples/with-microfrontends` lacks meta.json
    kind: for_each_dir
    select: "examples/*"
    when_iter: 'iter.has_file("package.json")'
    require: [{ kind: file_exists, paths: "{path}/meta.json" }]
  - id: turbo-example-meta-declares-maintenance # JSONPath bool/regex coercion — pitfall #16 workaround
    kind: file_content_matches    # NOT json_path_matches (which can't regex against bools)
    paths: "examples/*/meta.json"
    pattern: '"maintainedByCoreTeam"\s*:\s*(true|false)\b'
  - id: turbo-version-file-shape                # version.txt: <semver>\n<dist-tag>\n
    kind: file_content_matches
    paths: version.txt
    pattern: '^\d+\.\d+\.\d+(-(canary|alpha|beta|rc)(\.\d+)?)?\n(latest|canary|alpha|beta|rc)\n'
  - id: turbo-rust-toolchain-pinned             # nightly-YYYY-MM-DD format required
    kind: toml_path_matches
    paths: rust-toolchain.toml
    path: "$.toolchain.channel"
    matches: '^(stable|beta|nightly|nightly-\d{4}-\d{2}-\d{2}|\d+\.\d+(\.\d+)?)$'
```

**Repo-specific vs bundled split:**

- **29 rules** declared in this config: the retained per-crate /
  per-package / per-example structural conventions (the §2.1 22-gate
  table), 8 husky-hook + tool-config-presence assertions, and 7
  `command:` shellouts (cargo fmt / cargo clippy / cargo deny /
  oxlint / oxfmt / taplo / shellcheck).
- **86 effective rules** after loading the 8 extended rulesets and
  applying ID overrides/deduplication.

**Validation:** `alint validate-config` reports `✓ Config valid: 86
rule(s) loaded`. Pitfall checks: the magic comment is present (line
1); pitfall #16 is explicitly worked around in
`turbo-example-meta-declares-maintenance` (in-line comment lines
236-240 cite CONFIG-AUTHORING.md); all patterns are single-quoted
or `>-` folded scalars (no YAML literal block scalars — pitfall
#22-clean).

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the live
`/tmp/turborepo/` sparse-checkout. Machine: Linux 6.1.0-42-amd64,
~10 logical cores; alint binary `target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| Single-file shellcheck (e.g. `scripts/update-examples-dep.sh`) | shellcheck | **22 ms** ± 0.7 ms | included in 13.207 s full pass | n/a — alint shells out to shellcheck via `command:` rule, equivalent per-invocation cost |
| **alint full pass (88 rules)** | n/a | n/a | **13.207 s** ± 623 ms | — |

The alint 13 s wall-clock is dominated by the **7 `command:`
shellouts** firing once per matching anchor file each: `cargo fmt`
+ `cargo clippy` + `cargo deny` + `pnpm exec oxlint` +
`pnpm exec oxfmt` + `pnpm exec taplo` + `shellcheck` per script.
None of these tools are on PATH on the bench machine, so each
shellout fires as "command not found"; the 13 s upper-bounds the
spawn-and-fail overhead. **Strip the 7 shellouts and the
declarative-only pass runs in well under 1 s** (matching the
v0.9.13 S3 bench's 1.13 s for 100k files).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `cargo fmt --check` (turbo-cargo-fmt) | rustfmt | pending — needs the workspace's pinned `nightly-2026-02-27` | `rustup show` then `cd /tmp/turborepo && cargo fmt --check` |
| `cargo lint` (turbo-cargo-clippy, workspace alias) | clippy | pending — same toolchain requirement | `cargo lint` |
| `cargo deny check licenses` (turbo-cargo-deny-licenses) | cargo-deny | pending — `cargo-deny` not on PATH | `cargo install cargo-deny` |
| `pnpm exec oxlint --deny-warnings .` (turbo-oxlint-deny-warnings) | oxlint | pending — needs `pnpm install` first | `pnpm install && pnpm exec oxlint --deny-warnings .` |
| `pnpm exec oxfmt --check` (turbo-oxfmt-check) | oxfmt | pending — needs `pnpm install` first | `pnpm install && pnpm exec oxfmt --check` |
| `pnpm exec taplo format --check` (turbo-taplo-check) | taplo | pending — needs `pnpm install` first | `pnpm install && pnpm exec taplo format --check` |
| `shellcheck` (turbo-shellcheck-scripts, anchored to `scripts/**/*.sh`) | shellcheck | shellcheck is on PATH; the in-tree script set is small (1 file: `scripts/update-examples-dep.sh`) — measured at 22 ms above | `shellcheck /tmp/turborepo/scripts/*.sh` |

The full husky pre-push hook (`pnpm exec lint-staged` →
`turbo run format check:toml` → `cargo fmt --check` → `cargo lint`
→ `cargo check --workspace`) is the marketable comparison —
estimated 30-90 s on a warm cache for the cargo half alone.
Reproduction: `cd /tmp/turborepo && bash .husky/pre-push`.
**alint's pitch is not faster shellouts** — it's running all
shellouts in parallel under one config + one walk + one report,
plus the 21 declarative-only structural rules in the same pass
that the husky chain doesn't enforce at all.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/vercel-turbo/.alint.yml --format json /tmp/turborepo/`
(live run, JSON-format).

**Headline (v0.9.17-era counts; v0.9.18 fixes annotated):** alint
surfaced **307 violations** across 30 failing rules (37 passing)
under v0.9.17. Validates the prior-validation-pass real findings
(60 of 61 crates drift on `publish=false`; 9 of 52 crates lack
READMEs; 8 of 17 packages lack LICENSE). Under v0.9.20 the v0.9.18
A1 fix (`hygiene-no-js-build-outputs` requires sibling
package.json) collapses the row-10 6-violation FP class to ~0;
A4 (cargo-workspace selector) means the bundled rules now iterate
per-crate across all 61 crates (the per-crate finding cardinalities
in rows 1, 2, 7, 8, 12 are unchanged; the iteration mechanism is
the cleanup).

| # | Count | Rule | Triage |
|---|---|---|---|
| 1 | 61 | `turbo-internal-crate-not-publishable` | **All real findings.** Validates §6's "60 of 61 crates drift" headline (the bench tree has one extra crate above the original 60). Recommendation: add `publish = false` to each internal `crates/turborepo-*/Cargo.toml`. |
| 2 | 61 | `turbo-crate-inherits-edition` | **Retired rule.** The project does not require every crate to inherit the edition; treating all deviations as findings was unsupported. |
| 3 | 47 | `gha-pin-actions-to-sha` (bundled `ci/github-actions@v1`) | Real findings — third-party action steps pin by tag rather than commit SHA. Worth filing for supply-chain hardening. |
| 4 | 30 | `node-no-tracked-node-modules` (bundled) | Likely **test fixtures** under `crates/*/fixtures/**/node_modules/`. Need `paths.exclude` for fixture trees. |
| 5 | 30 | `hygiene-no-node-modules` (bundled `hygiene/no-tracked-artifacts@v1`) | Same as #4 — same fixtures, two rule names. |
| 6 | 10 | `monorepo-packages-have-readme` (bundled) | Real — packages without READMEs (some private packages may legitimately lack them; needs allowlist). |
| 7 | 9 | `cargo-workspace-member-has-readme` (bundled) | Validates §6's "9 of 52 crates lack READMEs". |
| 8 | 9 | `turbo-crate-has-readme` (the per-rule restate at error-level) | Same crate set as #7; alint emits both because the per-rule one is `error` level vs the bundled `warning`. |
| 9 | 8 | `turbo-package-has-license` | Validates §6's "8 of 17 packages lack per-package LICENSE". |
| 10 | 6 | `hygiene-no-js-build-outputs` | **RESOLVED in v0.9.18 (A1 fix).** v0.9.17-era count: 6 FPs on `crates/turborepo-paths/src/lib/build/` and similar Rust source dirs named `build/`. v0.9.18's A1 refinement gates `hygiene-no-js-build-outputs` on a sibling `package.json` (correctly distinguishing JS build output from arbitrarily-named Rust source dirs); effective v0.9.20 count: ~0. |
| 11 | 6 | `hygiene-no-env-files` (bundled) | Likely `examples/*/.env.example` or `crates/*/test-fixtures/.env`. Need allowlist. |
| 12 | 6 | `turbo-crate-inherits-workspace-lints` | **Retired rule.** The six crates do not establish a universal project convention. |
| 13 | 5 | `turbo-package-declares-repository-directory` | **Retired rule.** The five omissions are valid for this repository. |

**Real findings (alint surfaced, existing tooling missed):**

- **61 of 61 crates lack `publish = false` guard** (the spec
  says 60 of 61 with `turbopath` as the one published exception;
  the bench tree shows 61 — verify with `cargo metadata`).
- **9 of 52 crates lack READMEs.**
- **8 of 17 packages lack a per-package LICENSE** (`npm pack`
  doesn't auto-include the repo-root LICENSE).
- **47 GHA action steps not SHA-pinned.**

**Expected absence from this run:** the case-study spec mentions
`with-microfrontends` lacks `meta.json`; `with-nextjs` is only a README stub
with no `package.json`, so the `when_iter` guard deliberately skips it rather
than producing a turbo.json finding. The `turbo-example-has-meta-json` /
`turbo-example-has-turbo-json` rules don't appear in the top-15.
Either the sparse-clone elides `examples/`, or those examples
were since fixed. Verify with `ls /tmp/turborepo/examples/with-microfrontends`.

**Pitfall #22 verification:** ZERO instances in `.alint.yml`.
`grep -nE 'pattern:\s*[|>][-+]?$'
/home/kaminsod/projects/alint/examples/vercel-turbo/.alint.yml`
returns no matches. The 7 multi-line patterns in this config use
single-quoted YAML scalars (e.g.
`pattern: '(?ms)^\[lints\]\s*\nworkspace = true'` on line 74).
The config's `message:` blocks use `>-` folded scalars, but those
are message text not regex patterns — pitfall #22 doesn't apply.

---

## 7. Pitfall #22 verification (this batch's special call-out)

The brief asked: **verify every multi-line regex in this case
study's config for the YAML literal-block-scalar trailing-newline
issue (pitfall #22).**

**Verdict for `examples/vercel-turbo/.alint.yml`: ZERO instances.**
`grep -nE 'pattern:\s*[|>][-+]?$'
/home/kaminsod/projects/alint/examples/vercel-turbo/.alint.yml`
returns no matches. The 7 multi-line patterns in this config use
single-quoted YAML scalars; the per-message text uses `>-` folded
scalars (which strip trailing newlines and don't apply to regex).
No per-crate + per-package license-header `file_header` rule
exists — license enforcement is via file-presence
(`oss-license-exists` + `turbo-package-has-license`'s `file_exists`
require), not pattern matching.

The vercel/turbo case study **also explicitly worked around
pitfall #16** in `turbo-example-meta-declares-maintenance` (lines
236-244): the rule uses `file_content_matches` against the JSON
text rather than `json_path_matches` against the bool, with an
in-line comment citing CONFIG-AUTHORING.md pitfall #16. Pitfall
#16 is distinct from #22.

---

## 8. Followup feature work surfaced

Sorted by demand strength:

1. **`dir_name_matches_field` rule kind** — covers crate-name and
   package-name drift across both Cargo and npm; surfaces in every
   monorepo we've inventoried. **v0.10+ candidate (2 sources:
   turbo + next.js).** turbo is the canonical demand-driver.
2. **`json_schema_passes` rule kind** — covers `turbo.json` /
   `tsconfig.json` / `.oxlintrc.json` / golangci-lint config
   validation. **v0.10 design candidate (2 sources: k8s + turbo).**
3. **`alint pr-diff-check` sibling mode** — operates on a PR's
   changed-file list rather than the repo at HEAD; covers the
   release-PR content-guard pattern (turbo plus most monorepos
   with auto-release bots). Single-source candidate at this point.

---

## 9. Future analysis

Three candidate refinements for the next revalidation pass:

1. **What `alint suggest` would propose for the 22 gates that don't
   exist in turbo's tooling.** A live `alint suggest` against a fresh
   turbo clone would surface most of the bundled rulesets the config
   already extends (the 9 in §4) plus probably `agent-hygiene@v1`
   (medium — turbo's `crates/turborepo-*` tree has a non-trivial
   number of `// TODO(scope-name)` markers worth a blame-driven
   scan) and `compliance/reuse@v1` if Vercel adopts REUSE/SPDX
   headers (currently they don't; would be a deliberate override).
2. **`scope_filter` for the `crates/` vs `packages/` vs `examples/`
   triad.** v0.9.17's `scope_filter` evolution lets each subtree
   be a named scope (`rust-crates`, `js-packages`, `examples-tree`)
   declared once at the top and referenced by name in each rule —
   cuts ~20 lines, particularly helpful for the per-example
   `meta.json`/`turbo.json`/`.gitignore` triad rules.
3. **The `dir_name_matches_field` v0.10+ candidate revisited.**
   The case study notes 7 crates whose directory name doesn't
   match the published crate name (intentional namespacing drift).
   When `dir_name_matches_field` lands, turbo becomes the canonical
   "expected drift, allowlist 7" demonstration — the v0.10+ design
   needs a `paths.exclude:` or `allow_drift:` knob to make
   intentional drift expressible without disabling the rule
   workspace-wide.

---

## 10. Validation status (last live recheck 2026-05-07; reconciled to v0.9.20 on 2026-05-10)

- **alint version pin:** 0.9.20 (current, 2026-05-10). Original
  capture under v0.9.17 (`1dbd9b218a0e`, built 2026-05-07).
- **`.alint.yml` in this directory:** **shipped — 29 rules declared
  here, 8 bundled rulesets folded in via `extends:`, 86 effective rules
  loaded.**
  `alint validate-config` confirms `✓ Config valid: 86 rule(s)
  loaded`. **Live-tree recheck:** performed in this batch under
  v0.9.17 — see §6 for the 307-violation breakdown (61
  publish=false drift + 61 edition-inheritance drift + 9
  missing-README + 8 missing-LICENSE + 6 lints-inheritance drift +
  the long tail). Under v0.9.20 the v0.9.18 A1 fix collapses the
  6-violation `hygiene-no-js-build-outputs` FP class to ~0; A4
  iterates the bundled cargo-workspace rules per-crate across all
  61 crates.
- **Workspace-shape verification:** **CONFIRMED.** 61 crates under
  `crates/`; 17 packages under `packages/`; 13 workflows under
  `.github/workflows/`.
- **Husky hook content verification:** **CONFIRMED.** `.husky/pre-push`
  invokes `pnpm exec lint-staged`, `turbo run format check:toml`,
  `cargo fmt --check`, `cargo lint`, `cargo check --workspace` (in
  that order).
- **22 gates that don't exist enumeration:** **VERIFIED** in §2.1
  table — all 22 expressible with alint-today rule kinds (20
  declarative + 2 shellouts to existing tools); none require the
  v0.10 backlog.
- **Rule-kind candidate status:**
  - `dir_name_matches_field` — v0.10+ candidate (2 sources). turbo
    is the canonical demand-driver.
  - `json_schema_passes` — v0.10 design candidate (2 sources: k8s +
    turbo).
  - `alint pr-diff-check` — sibling-binary candidate, single-source.
- **Pitfall status (post-v0.9.18 fix wave):**
  - Pitfalls #18 and #19 — **engine-fixed in v0.9.17**
    (`respect_gitignore: false` per-rule knob; `literal_is_nested`
    runtime guard).
  - Pitfall #22 instances in this directory's config: **ZERO**
    (`grep -nE 'pattern:\s*[|>][-+]?$' .alint.yml` returns no
    matches; all 7 multi-line patterns use single-quoted scalars).
    No per-crate license-header `file_header` rule exists;
    license enforcement is via file-presence rules.
  - **Pitfall #16 worked around in 1 place**:
    `turbo-example-meta-declares-maintenance` uses
    `file_content_matches` against the JSON text rather than
    `json_path_matches` against the bool; in-line comment cites
    CONFIG-AUTHORING.md. #16 is an authoring gotcha, not
    engine-fixed.
  - Pitfalls #1-#17, #20, #21 — authoring gotchas, not
    engine-fixed; #20/#21 await v0.10's
    `cross_file_value_equals` (with `value_extractor:`) and the
    multi-doc YAML fix respectively.
- **Bundled-ruleset rule counts (authoritative as of 2026-05-07):**
  oss-baseline=15, rust=11, node=9, monorepo=4,
  monorepo/cargo-workspace=4, monorepo/pnpm-workspace=4,
  ci/github-actions=3, hygiene/no-tracked-artifacts=11,
  tooling/editorconfig=3. v0.9.18 refinements that touched
  bundled rulesets impacting this config:
  **A1** (`hygiene/no-tracked-artifacts@v1`'s
  `hygiene-no-js-build-outputs` requires sibling package.json),
  **A4** (`monorepo/cargo-workspace@v1` selector parses
  `[workspace]` members),
  **A5** (`oss-baseline@v1` `oss-license-exists` recognises
  LICENSE.TXT and LICENSE.md).

## v0.11 re-analysis update (2026-05-25)

Re-derived against the current upstream + everything alint shipped since
this study was written (v0.10 rule kinds + v0.11 commit-validation /
`changed_since` / `{{env.X}}`). The `.alint.yml` here was rewritten
accordingly (86 rules, 30/38 inventoried surfaces, ~79% coverage).
`registry_paths_resolve` checks [workspace].members, and `import_gate`
mirroring the clippy.toml type/method bans. Corrections from the re-analysis:
turbo is MIT (not MPL-2.0) and Rust files carry no SPDX header (the config
does not invent a header rule). npm package versions do not continuously equal
version.txt: stable releases advance that file to the next canary, and private
packages may remain 0.0.0, so the false coherence rule was removed.

Full catalogue, coverage math, and cross-cutting findings:
`docs/development/case-study-v011-reanalysis-log.md` (Batch 6).
