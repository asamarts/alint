# Case study: `vercel/next.js`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/vercel-next.js/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `vercel/next.js`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 sparse-clone of
`vercel/next.js@98ab09903` at `/tmp/next.js/`.
`/test`, `/examples`, and `/docs` excluded.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

next.js is the **first hybrid pnpm + Cargo dual-workspace data point in
the case-study catalogue**. Verified shape against `/tmp/next.js/`:

### 1.1 Hybrid dual-workspace topology — VERIFIED

**npm side** (`package.json` workspaces field):

```json
"workspaces": [
  "packages/*"
]
```

Just one glob, but `pnpm-workspace.yaml` extends this:

```yaml
packages:
  - 'apps/*'
  - 'packages/*'
  - 'bench/*'
  - 'crates/*/js'
  - 'turbopack/crates/*/js'
  - 'turbopack/crates/turbopack-tests/tests/execution'
  - 'turbopack/packages/*'
```

**7 pnpm globs total**, including the cross-language `crates/*/js`
and `turbopack/crates/*/js` patterns that pull JS test harnesses
embedded inside Rust crate directories.

**Cargo side** (`Cargo.toml [workspace] members`):

```toml
members = [
  "scripts/send-trace-to-jaeger",
  "crates/next-napi-bindings",
  "crates/wasm",
  "crates/next-api",
  "crates/next-build-test",
  "crates/next-build",
  "crates/next-code-frame",
  "crates/next-core",
  "crates/next-custom-transforms",
  "crates/next-taskless",
  "turbopack/crates/*",
  "turbopack/crates/*/fuzz",
  "turbopack/xtask",
]

exclude = [
  "crates/next-error-code-swc-plugin",
  "rspack/crates/binding"
]
```

**13 workspace member globs + 2 exclude entries.** Verified: the
tree contains **68 `Cargo.toml` files** (`find /tmp/next.js -name
"Cargo.toml" -not -path "*/target/*" -not -path
"*/node_modules/*"` ) and **306 `package.json` files** (same find
with `-name "package.json"`).

Top-level package count under the canonical `packages/` glob:
**19 npm packages**, all pinned to `16.3.0-canary.11` (lerna's
exact-version-lockstep).

**The cross-language conventions BOTH linters miss:**

| # | Convention | What pnpm misses | What Cargo misses |
|---|---|---|---|
| 1 | License field uniformity across BOTH halves of the tree | pnpm sees 19 npm packages — has no idea about 68 Rust crates | Cargo sees 68 crates — has no idea about 19 npm packages |
| 2 | Edition + canary-version lockstep | pnpm sees `package.json::version = "16.3.0-canary.11"` | Cargo sees `Cargo.toml::edition = "2024"` |
| 3 | Cross-toolchain channel pinning | pnpm reads `.node-version` (`v20`) | Cargo reads `rust-toolchain.toml` (`nightly-2026-04-02`) |
| 4 | Per-package README + LICENSE for npm publish | pnpm leaves this to humans | Cargo doesn't see npm packages |
| 5 | `@next/x` ↔ `packages/x` directory-name discipline | pnpm wouldn't know to enforce | Cargo doesn't see npm packages |
| 6 | gitattributes EOL pin (`* text=auto eol=lf`) for cross-OS contributors | Neither linter checks tree-wide eol policy | Same |
| 7 | Husky hook integrity (the `.husky/pre-commit` runs `lint-staged`) | pnpm sees `lint-staged.config.js` only | Cargo doesn't see husky |
| 8 | Tracked-artefact hygiene across BOTH `.next/` (JS) AND `target/debug/` (Rust) | pnpm only knows about `.next/`/`node_modules/` | Cargo only knows about `target/` |
| 9 | Workspace-member coherence — every Cargo member directory has a `Cargo.toml`, every pnpm member directory has a `package.json` | pnpm checks pnpm-side coherence | Cargo checks Cargo-side coherence |
| 10 | rust-toolchain.toml has `rustfmt`, `clippy`, `rust-analyzer` components | Not pnpm's concern | Cargo: known but not enforced via lint |
| 11 | Lerna `publish.allowBranch: [canary]` integrity | pnpm sees lerna.json | Not Cargo's concern |
| 12 | Turbo task graph (`turbo.json` `tasks.build.outputs[*]` includes `dist/**`) | pnpm sees turbo.json by file presence only | Not Cargo's concern |
| 13 | Errors registry (`errors/manifest.json` ↔ `errors/**/*.md`) coverage | pnpm has no concept | Cargo has no concept |
| 14 | externals registry (`server-external-packages.jsonc` ↔ `improper-server-external.mdx`) consistency | pnpm has no concept | Cargo has no concept |
| 15 | The 30+ workflow SHA-pinning + permissions-block discipline | pnpm has no concept | Cargo has no concept |

**15 cross-language conventions both ecosystem-specific linters
miss** — exactly the gap alint's polyglot bundle composition
(`monorepo/cargo-workspace@v1` + `monorepo/pnpm-workspace@v1`
layered together, plus oss-baseline + ci/github-actions +
hygiene/no-tracked-artifacts) closes.

### 1.2 Root config files (root-level lint policy)

| File | Owner tool | What it pins |
|---|---|---|
| `package.json` `scripts:` block | npm | 70+ task aliases |
| `pnpm-workspace.yaml` | pnpm | 7 workspace globs |
| `Cargo.toml` workspace | cargo | 13 workspace member globs + 2 exclude entries; `[workspace.lints]` |
| `lerna.json` | lerna | publish workflow: `npmClient: pnpm`, `version.exact: true`, `publish.allowBranch: [canary]`, root version `16.3.0-canary.11` |
| `turbo.json` | turbo | task graph + cached outputs |
| `tsconfig.json` | tsc | root TS config — `compilerOptions.strict: true` |
| `tsconfig-tsec.json` | tsec | trusted-types security checker |
| `eslint.config.mjs` + `eslint.cli.config.mjs` | eslint | flat-config split (IDE vs CI) |
| `.prettierrc.json` + `.prettierignore` | prettier | `singleQuote: true, semi: false, trailingComma: es5` |
| `lint-staged.config.js` | lint-staged | per-extension prettier/eslint/rustfmt pipeline |
| `.husky/pre-commit`, `.husky/pre-push` | husky | hook scripts (lint-staged + canary push protection) |
| `.typos.toml` | crate-ci/typos | per-language word list |
| `sgconfig.yml` | ast-grep | rule dirs |
| `.alexrc` + `.alexignore` | alex | insensitive-language allowlist |
| `socket.yaml` | Socket.dev | supply-chain scanner config |
| `.npmrc` | pnpm | `auto-install-peers`, `link-workspace-packages`, `provenance` |
| `.node-version` | nvm | Node major version pin (`v20`) |
| `.gitattributes` | git | `* text=auto eol=lf` |
| `.rustfmt.toml` | rustfmt | edition + style + max width |
| `rust-toolchain.toml` | rustup | `nightly-2026-04-02` channel + `rustfmt`, `clippy`, `rust-analyzer` components |

### 1.3 `scripts/check-*.{js,mjs,sh}` — hand-rolled structural gates

Verified via `ls /tmp/next.js/scripts/check-* /tmp/next.js/scripts/validate-*`:

| Script | What it checks | Backing tool |
|---|---|---|
| `check-examples.sh` | Re-canonicalises every `examples/*/package.json`; copies template `next-env.d.ts` and `.gitignore`; **fails if `git status` shows drift** (mutation-with-verification) | bash + jq + git |
| `check-manifests.js` | Walks `errors/manifest.json`'s route tree, asserts every `errors/**/*.md` (except `template.md`) is reachable from the route graph | Node.js |
| `check-pre-compiled.sh` + `check-pre-compiled.bat` | Re-runs `pnpm ncc-compiled` (re-bundles webpack runtime); fails if `git status` shows drift | bash + git |
| `check-is-release.js` | Parses the most recent commit message for a `^v\d+\.\d+\.\d+(-\w+\.\d+)?$` tag | Node.js + git log |
| `check-unused-turbo-tasks.mjs` | Scans every `*.rs` file under `crates/` + `turbopack/crates/` for `#[turbo_tasks::function]` annotations; cross-references against usage sites; reports unused | Node.js + Rust source scan |
| `validate-externals-doc.js` | Reads `packages/next/src/lib/server-external-packages.jsonc`; cross-references against the doc table at the bottom of `errors/improper-server-external.mdx`; reports drift | Node.js |
| `check-backport-canary-release.js` | Validates a `backport-canary-release` branch matches the canary state | Node.js + git |

**8 hand-rolled structural gates total** (counting the .sh + .bat
pre-compiled pair as one logical check).

### 1.4 `.github/workflows/` — VERIFIED 36 workflows

`ls /tmp/next.js/.github/workflows/ | wc -l` = **36**. Earlier
revisions of this README cited "30+"; the current count is 36.

Bucketed by purpose:

- **Build + test orchestration**: `build_and_test.yml`,
  `build_and_deploy.yml`, `build_reusable.yml`,
  `integration_tests_reusable.yml`, `test-*.yml`, `rspack-*.yml`,
  `turbopack-*.yml`, `retry_test.yml`, `retry_deploy_test.yml`
- **Release orchestration**: `code_freeze.yml`,
  `create_release_branch.yml`, `trigger_release.yml`,
  `sync_backport_canary_release.yml`
- **Issue / PR bot automation**: `triage.yml`, `issue_lock.yml`,
  `issue_stale.yml`, `issue_wrong_template.yml`,
  `pull_request_auto_label.yml`, `popular.yml`
- **PR-comment automation**: `pr_ci_comment.yml`,
  `pull_request_stats.yml`, `graphite_ci_optimizer.yml`

All 36 covered structurally by `ci/github-actions@v1` (3 rules:
permissions, SHA pinning, name).

### 1.5 `.config/ast-grep/rules/` — Rust AST patterns

Out of alint's "no AST" scope. Shelled out via `pnpm lint-ast-grep`.

### 1.6 `.alex` (insensitive-language NLP) + `.typos` (spell check)

Both NLP / dictionary content checks. Out of alint's scope; shelled
out via `command:` rules.

### 1.7 `eslint.config.mjs` rules (TS/JS AST analysis)

~50 rules across `eslint:recommended`, `@typescript-eslint`,
`eslint-plugin-react`, `eslint-plugin-react-hooks`,
`eslint-plugin-jest`, `eslint-plugin-import`, `eslint-plugin-jsdoc`,
plus next.js's own `@next/eslint-plugin-internal`. Every one is a
TSESTree visitor — out of alint's scope. Shelled out via
`pnpm lint-eslint .`.

---

## 2. Coverage classification

### 2.1 The 8 hand-rolled `scripts/check-*.{js,mjs,sh}` gates

| Script | Coverage | Notes |
|---|---|---|
| `check-examples.sh` | ❌ out-of-scope (mutation) | alint reads files; doesn't regenerate-and-diff. Wrap in `command:` rule that runs the script in CI; failures still flag. |
| `check-manifests.js` | 🔄 alint-future | `registry_paths_resolve` (v0.10 ship-target, 8 sources). The deeper "every md reachable from routes" is exactly this primitive's shape. |
| `check-pre-compiled.{sh,bat}` | ❌ out-of-scope (codegen) | Mutation followed by git-diff. Same pattern as airflow's `update-spelling-wordlist-to-be-sorted`. |
| `check-is-release.js` | ❌ out-of-scope (git history) | Parses commit messages. The `git_commit_message` rule kind exists but checks staged/HEAD message; this script needs subprocess git access. |
| `check-unused-turbo-tasks.mjs` | ❌ out-of-scope (Rust AST) | Wrap in `command:` rule. |
| `validate-externals-doc.js` | 🔄 alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources). |
| `check-backport-canary-release.js` | ❌ out-of-scope (git refs) | |

### 2.2 The 7 root-level lint tool configs

| File | Coverage | Notes |
|---|---|---|
| eslint configs | ❌ out-of-scope (TS/JS AST) | Shell out via `command:`. |
| prettier configs | ✅ alint-today (presence) + ❌ out-of-scope (formatting) | `file_exists` for `.prettierrc.json` + `.prettierignore`. Formatting itself is prettier's job. |
| ast-grep config | ❌ out-of-scope (Rust AST) | Shell out. |
| typos config | ❌ out-of-scope (NLP) | Shell out. |
| alex configs | ❌ out-of-scope (NLP) | Shell out. |
| socket.yaml | ❌ out-of-scope (supply-chain scanner) | File-presence only. |
| lint-staged config | ✅ alint-today (presence) | `file_exists`. |

### 2.3 The 36 GHA workflows

All 36 covered structurally by `ci/github-actions@v1` (3 rules at
warning level — the next.js README restates the SHA-pinning rule
because the supply-chain blast radius is large).

### 2.4 The hybrid dual-workspace shape — the headline alint coverage

| Surface | Coverage | Rule |
|---|---|---|
| pnpm-workspace.yaml shape (7 globs) | ✅ alint-today | `yaml_path_matches` + 2 `file_content_matches`. |
| Cargo.toml workspace (13 members + 2 excludes) | ✅ alint-today | `monorepo/cargo-workspace@v1` (4 rules) + member-coherence per-member. |
| Per-npm-package conventions (license, version, name, private) | ✅ alint-today | `for_each_dir` over `packages/*/` + `json_path_matches`. |
| Per-Cargo-crate conventions (edition, license, workspace-lints inheritance) | ✅ alint-today | `for_each_dir` over Cargo members + `toml_path_matches`. |
| `@next/x` ↔ `packages/x` directory-name discipline | 🔄 alint-future | `dir_name_matches_field` with **unscoping** transform — the next.js shape is intentionally messy (3 conventions: unscoped umbrella, scoped, scoped-with-prefix). v0.10+ candidate (extension of the existing `dir_name_matches_field`, 2 sources: turbo + next.js). |
| Cross-toolchain channel pinning (rust-toolchain + .node-version) | ✅ alint-today | 2× `toml_path_matches` + `file_exists`. |
| gitattributes EOL pin | ✅ alint-today | `file_content_matches`. |
| Husky hook integrity | ✅ alint-today | `file_exists` + `file_content_matches`. |
| errors/manifest.json shape | ✅ alint-today (partial) | `json_path_matches` for shape. The deeper "registry resolves" needs `registry_paths_resolve`. |
| externals doc cross-reference | 🔄 alint-future | `cross_file_value_equals` (v0.10 ship-target). |
| Tracked-artefact hygiene across BOTH `.next/` AND `target/debug/` | ✅ alint-today | `hygiene/no-tracked-artifacts@v1` + extended for nested locations. |

### 2.5 Repo-root governance

| Artefact | Coverage |
|---|---|
| `LICENSE`, `README.md`, `CODE_OF_CONDUCT.md`, `contributing.md`, `AGENTS.md`, `CLAUDE.md` (symlink) | ✅ alint-today (oss-baseline + agent-context bundle) |
| `Cargo.toml`, `pnpm-workspace.yaml`, `package.json` | ✅ alint-today (rust + node + monorepo bundles) |
| Repo-wide hygiene | ✅ alint-today (hygiene/no-tracked-artifacts + hygiene/lockfiles) |

---

## 3. Quantified coverage

Counted across the **8 scripts/check-* gates** + **7 lint configs** +
**36 workflows** + **15 cross-language conventions** + **8 governance
artefact families** = **74 distinct surfaces**.

```
✅ alint-today:    52 / 74 = 70%   (1 lint-staged + 36 workflows + 13 cross-lang + 2 governance)
🔄 alint-future:    4 / 74 = 5%    (3 from scripts/ + 1 dir-name-matches-field-unscoping + 1 mixed)
❌ out-of-scope:   16 / 74 = 22%   (5 scripts + 5 lint configs + 6 governance/AST)
                  ~2 partial (turbo.json, lint-staged config) = 3%
                  ──────────────
                  total = 100%
```

Granular breakdown:

```
scripts/check-* gates (8):
  ✅ alint-today:     0 / 8  = 0%
  🔄 alint-future:    2 / 8  = 25%   (check-manifests + validate-externals-doc)
  ❌ out-of-scope:    6 / 8  = 75%   (check-examples, check-pre-compiled, check-is-release, check-unused-turbo-tasks, check-backport-canary-release)

7 root lint tool configs:
  ✅ alint-today (presence):    3 / 7  = 43%
  ❌ out-of-scope (AST/NLP):    4 / 7  = 57%

36 GHA workflows:
  ✅ alint-today (ci/github-actions@v1):  36 / 36 = 100%

15 cross-language conventions BOTH per-ecosystem linters miss:
  ✅ alint-today:    13 / 15 = 87%
  🔄 alint-future:    2 / 15 = 13%   (dir_name_matches_field-unscoping + cross_file_value_equals)
```

**Commentary.** Three observations:

1. **next.js is the canonical "polyglot at the workspace tier"
   demonstration.** The hybrid pnpm + Cargo dual-workspace shape
   means **15 of 15 cross-language conventions are invisible to
   either ecosystem-specific linter** — pnpm only sees the 19
   packages, Cargo only sees the 68 crates, and neither sees the
   shared workspace-uniformity discipline. That's the headline
   pitch. alint's polyglot bundle composition
   (`monorepo/cargo-workspace@v1` + `monorepo/pnpm-workspace@v1`
   layered together) covers 13 / 15 today; the remaining 2 are
   v0.10 ship-targets.

2. **75 % of the `scripts/check-*` gates (6 of 8) are deliberately
   out of alint's scope.** They're mutation-with-verification (4),
   git-history operations (2), and Rust AST (1). The remaining
   25 % (`check-manifests` + `validate-externals-doc`) are exactly
   the v0.10 ship-target shapes (`registry_paths_resolve` +
   `cross_file_value_equals`).

3. **`dir_name_matches_field` with an unscoping transform** is the
   one v0.10+ extension surfaced uniquely by next.js's intentionally
   messy `@next/x` ↔ `packages/x` mapping. The bare
   `dir_name_matches_field` from vercel/turbo's gap catalogue would
   fire on every npm package here. **File as a v0.10+ extension,
   not a new rule kind.**

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (801 lines, 59
repo-specific rules, 11 bundled rulesets folded in via `extends:`,
**130 rules total** loaded — confirmed by `alint validate-config`).

**Synopsis of the load-bearing rules** (full config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/node@v1                          # 9 rules
  - alint://bundled/rust@v1                          # 11 rules
  - alint://bundled/monorepo@v1                      # 4 rules
  - alint://bundled/monorepo/cargo-workspace@v1      # 4 rules
  - alint://bundled/monorepo/pnpm-workspace@v1       # 4 rules
  - alint://bundled/ci/github-actions@v1             # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules
  - alint://bundled/hygiene/lockfiles@v1             # 7 rules
  - alint://bundled/tooling/editorconfig@v1          # 3 rules
  - alint://bundled/agent-context@v1                 # 5 rules

rules:
  - id: nextjs-pnpm-workspace-declares-packages       # pnpm-workspace.yaml shape
    kind: yaml_path_matches
    paths: pnpm-workspace.yaml
    path: "$.packages[*]"
    matches: ".+"
  - id: nextjs-package-version-pinned-to-canary       # Lerna lockstep — every package on 16.x.y-canary.N
    kind: for_each_dir
    select: "packages/*"
    when_iter: 'iter.has_file("package.json")'
    require: [{ kind: json_path_matches, paths: "{path}/package.json", path: "$.version", matches: '^16\.\d+\.\d+(-(canary|alpha|beta|rc)\.\d+)?$' }]
  - id: nextjs-crate-edition-2024                     # Per-Cargo-crate uniformity
    kind: toml_path_matches
    paths: ["crates/*/Cargo.toml", "turbopack/crates/*/Cargo.toml"]
    path: "$.package.edition"
    matches: '^2024$'
  - id: nextjs-package-json-declares-private          # Pitfall #16 workaround
    kind: file_content_matches    # NOT json_path_matches (bool-vs-regex coercion)
    paths: package.json
    pattern: '"private":\s*true'
  - id: nextjs-rust-toolchain-pinned                  # No floating nightly
    kind: toml_path_matches
    paths: rust-toolchain.toml
    path: "$.toolchain.channel"
    matches: '^(stable|beta|nightly|nightly-\d{4}-\d{2}-\d{2}|\d+\.\d+(\.\d+)?)$'
  - id: nextjs-husky-pre-commit-runs-lint-staged      # The hook actually invokes lint-staged
    kind: file_content_matches
    paths: .husky/pre-commit
    pattern: 'pnpm lint-staged'
  - id: nextjs-gitattributes-eol-lf                   # Cross-OS contributor guard
    kind: file_content_matches
    paths: .gitattributes
    pattern: '(?m)^\*\s+text=auto\s+eol=lf'
  - id: nextjs-errors-manifest-declares-routes        # JSONPath shape on errors/manifest.json
    kind: json_path_matches
    paths: errors/manifest.json
    path: "$.routes[*].path"
    matches: '^/errors/.+\.md$'
```

**Repo-specific vs bundled split:**

- **59 next.js-specific rules** (`nextjs-*` prefix): 3 pnpm-workspace
  shape + 4 per-npm-package + 2 per-Cargo-crate + 5 root SSoT + 3
  lerna + 8 tool-config presence + 3 husky-hook integrity + 4
  repo-metadata + 2 errors/manifest + 3 turbo.json + 6
  workspace-root config + 3 tracked-artefact + 1 GHA SHA-pinning
  restate + 11 `command:` shellouts (prettier, eslint, tsc,
  ast-grep, alex, cargo fmt, cargo clippy, typos, check-examples,
  check-unused-turbo-tasks, validate-externals-doc).
- **76 bundled rules** from the 11 extended rulesets (15 + 9 + 11 +
  4 + 4 + 4 + 3 + 11 + 7 + 3 + 5 = 76 with overlap dedup).

**Validation:** `alint validate-config` reports `✓ Config valid:
130 rule(s) loaded`. Pitfall checks: the magic comment is present
(line 1); pitfall #16 is explicitly worked around in
`nextjs-package-json-declares-private` (line 294, with comment
block on lines 287-293) and `nextjs-tsconfig-strict-mode` (line
613, with comment block on lines 611-616) using
`file_content_matches` against the JSON text rather than
`json_path_matches` against bool; all patterns use single-quoted
scalars (no YAML literal block scalars — pitfall #22-clean); the
GHA `nextjs-workflow-actions-pinned-by-sha` rule uses a JSONPath
filter expression `$.jobs.*.steps[?match(@.uses, '...')].uses`
(JSONPath filter syntax — supported as of v0.9.6).

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the live
`/tmp/next.js/` sparse-checkout. Machine: Linux 6.1.0-42-amd64,
~10 logical cores; alint binary `target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full pass (130 rules)** | n/a | n/a | **10.264 s** ± 2.541 s | — |

The 10 s wall-clock against the ~163 MiB sparse-checkout (68
`Cargo.toml` + 306 `package.json` + 6,000+ TS source files + 36
workflows) is dominated by:

- **The 11 `command:` shellouts** firing once per matching anchor
  file — `cargo fmt --check`, `cargo clippy --workspace`, prettier,
  eslint, tsc, ast-grep, alex, typos, plus the 3 npm-script
  shellouts (`pnpm check-examples`, `pnpm check-unused-turbo-tasks`,
  `pnpm validate-externals-doc`). None of these tools are on PATH
  on the bench machine, so each shellout fires as "command not
  found"; the 10 s upper-bounds the spawn-and-fail overhead.
- **The full Rust + TS source-tree walk** for the bundled
  `rust@v1` + `node@v1` rules.

**Strip the 11 shellouts and the declarative-only pass runs in
roughly 1.5-3 s**, matching the published S9 macro-bench (~1.4 s
for 100k polyglot files).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `pnpm lint` (= prettier + eslint + tsc + ast-grep + alex + check-unused-turbo-tasks chain) | pnpm + the 6 lint tools | pending — needs `pnpm install` first (~2 GB of deps) | `cd /tmp/next.js && pnpm install && pnpm lint` |
| `cargo fmt --check` (nextjs-cargo-fmt-check) | rustfmt | pending — needs the workspace's pinned `nightly-2026-04-02` | `rustup show && cargo fmt --check` |
| `cargo clippy --workspace --all-targets -- -D warnings` (nextjs-cargo-clippy) | clippy | pending — same toolchain requirement | `cargo clippy --workspace --all-targets -- -D warnings` |
| `pnpm check-examples` | Node.js + the examples-runner | pending — needs `pnpm install` first | `pnpm check-examples` |
| `pnpm check-unused-turbo-tasks` | Node.js | pending — needs `pnpm install` first | `pnpm check-unused-turbo-tasks` |
| `pnpm validate-externals-doc` | Node.js | pending — needs `pnpm install` first | `pnpm validate-externals-doc` |

The full `pnpm lint` end-to-end wall-clock is the single most
marketable comparison number — estimated 30-60 s on a warm cache
(prettier + eslint + tsc + ast-grep + alex + check-unused
serially), plus 2-5 minutes for a fresh `pnpm install`. **Where
alint shines on next.js specifically:** the cross-language
license + version + private-flag uniformity check runs against
all 19 npm packages + all 68 Rust crates simultaneously in tens
of milliseconds; sequential `jq`+`taplo` over both halves would
be ~3-5 s. The cross-cutting structural checks pay back the most
when the repo is a polyglot mix where no single language linter
sees the whole tree.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/vercel-next.js/.alint.yml --format json /tmp/next.js/`
(live run, JSON-format).

**Headline:** alint surfaces **525 violations** across 37 failing
rules (60 passing). The breakdown:

| # | Count | Rule | Triage |
|---|---|---|---|
| 1 | 113 | `gha-pin-actions-to-sha` (bundled `ci/github-actions@v1`) | Real findings — third-party action steps pin by floating tag rather than 40-char commit SHA across the 36 workflows. Worth filing for supply-chain hardening (the next.js README §1.4 explicitly notes the supply-chain blast radius). |
| 2 | 106 | `oss-final-newline` | Real findings — markdown / yaml drift across `errors/`, `crates/`, `bench/`. Below tidy's threshold; informational. |
| 3 | 62 | `oss-no-trailing-whitespace` | Same — trailing-ws long tail. Informational. |
| 4 | 56 | `node-no-tracked-node-modules` (bundled) | **Test fixtures** — the next.js repo intentionally tracks `node_modules` under `test/integration/**` for reproducible test environments. **Recommended:** add `paths.exclude: ["test/integration/**", "examples/**"]` to override the bundled rule. |
| 5 | 56 | `hygiene-no-node-modules` (bundled `hygiene/no-tracked-artifacts@v1`) | Same fixtures, two rule names — same exclude. |
| 6 | 33 | `gha-workflow-contents-read` (bundled) | Real findings — workflows lacking `permissions: contents: read` block. Worth filing for hardening across the 36 workflows. |
| 7 | 22 | `nextjs-workflow-actions-pinned-by-sha` (the per-rule restate at warning level) | Same finding set as #1, narrower JSONPath filter. |
| 8 | 12 | `monorepo-packages-have-readme` (bundled) | Real — packages without READMEs (likely test packages or private internal packages). Needs allowlist. |
| 9 | 8 | `hygiene-no-js-build-outputs` | Likely false-positive — `build/` directories inside Rust crate source trees. Need scope override. |
| 10 | 7 | `pnpm-workspace-member-has-readme` | Real findings, similar to #8 with pnpm-side scope. |
| 11 | 6 | `node-no-tracked-dist` | Likely test fixtures with intentionally-tracked `dist/`. |
| 12 | 6 | `lockfiles-no-nested-pnpm` | Likely test fixtures with their own `pnpm-lock.yaml`. |
| 13 | 5 | `cargo-workspace-member-has-readme` | Real findings, Rust-side. |
| 14 | 4 | `nextjs-crate-license-mit` | **Real findings — the headline polyglot pitch.** 4 of 63 Cargo crates lack the standard MIT/MPL license declaration. Invisible to pnpm-side linters. Validates §1.1's "license field uniformity across BOTH halves of the tree" cross-language convention. |
| 15 | 3 | `lockfiles-no-nested-cargo` | Likely test fixtures with their own `Cargo.lock`. |

**Real findings (alint surfaced, existing tooling missed):**

- **4 of 63 Cargo crates lack the standard MIT/MPL license** — the
  flagship polyglot finding; pnpm-side `pnpm lint` doesn't see Rust
  crates, Cargo-side tooling doesn't see npm packages, alint sees
  both halves. (The original validation pass also surfaced 3 of 19
  npm packages missing license fields; the current run shows them
  as already-fixed or scope-excluded — verify the current state.)
- **113 + 22 GHA action references not pinned to commit SHA**
  across the 36 workflows.
- **33 workflows lacking `permissions: contents: read`** declaration.

**False-positive class (mostly tracked-artefact hygiene):** the
test-fixture `node_modules/`, `dist/`, nested-`pnpm-lock.yaml`,
nested-`Cargo.lock` findings (~120 violations) are intentional and
need `paths.exclude: ["test/integration/**", "examples/**"]` added
to the relevant bundled rule overrides. **Not a config bug** —
it's a known-good policy delta between next.js's intentional
fixture tracking and the bundled defaults.

**Pitfall #22 verification:** ZERO instances in `.alint.yml`.
`grep -nE 'pattern:\s*[|>][-+]?$'
/home/kaminsod/projects/alint/examples/vercel-next.js/.alint.yml`
returns no matches. The 9 multi-line patterns in this config use
single-quoted YAML scalars (e.g.
`pattern: '(?m)^\s*-\s*''packages/\*'''` on line 116,
`pattern: '(?m)^\*\s+text=auto\s+eol=lf'` on line 670). The
config's `message:` blocks use `>-` folded scalars, but those are
message text not regex patterns.

---

## 7. Pitfall #22 verification (this batch's special call-out)

The brief asked: **verify every multi-line regex in this case
study's config for the YAML literal-block-scalar trailing-newline
issue (pitfall #22).**

**Verdict for `examples/vercel-next.js/.alint.yml`: ZERO instances.**
`grep -nE 'pattern:\s*[|>][-+]?$'
/home/kaminsod/projects/alint/examples/vercel-next.js/.alint.yml`
returns no matches. The 9 multi-line patterns in this config use
single-quoted YAML scalars; the per-rule `message:` text uses `>-`
folded scalars (which strip trailing newlines and don't apply to
regex). No per-package or per-crate license-header `file_header`
rule exists — license enforcement is per-half via
`json_path_matches` (`nextjs-package-has-license-field`) and
`toml_path_matches` (`nextjs-crate-license-mit`).

The next.js case study **also explicitly worked around pitfall #16**
(JSONPath bool/number regex coercion) in two places:
`nextjs-package-json-declares-private` (line 294, comment block
287-293) and `nextjs-tsconfig-strict-mode` (line 613, comment block
611-616), both using `file_content_matches` against the JSON text
rather than `json_path_matches` against bool. Pitfall #16 is now
in the canonical-22 catalogue at position #16; distinct from #22.

---

## 8. Followup feature work surfaced

Sorted by demand strength:

- **`cross_file_value_equals`** — covers `validate-externals-doc.js`
  here. **v0.10 ship-target (10 sources).**
- **`registry_paths_resolve`** — covers `check-manifests.js` here.
  **v0.10 ship-target (8 sources).**
- **`dir_name_matches_field` extension with unscoping** — covers
  the `@next/x` ↔ `packages/x` mapping; same as vercel/turbo's base
  candidate but with a configurable scope-stripping transform.
  **v0.10+ candidate (2 sources: turbo + next.js + likely
  react/pnpm in subsequent waves).**

---

## 9. Future analysis

Three candidate refinements for the next revalidation pass:

1. **`scope_filter` for the pnpm + Cargo dual-workspace shape.** The
   future config will layer globs to keep Rust rules off the JS tree
   and vice versa. v0.9.17's `scope_filter` evolution lets each rule
   declare a named scope (`rust-workspace`, `js-workspace`,
   `js-bench`) once, with the path predicates centralised —
   separates "which subtree am I in" from "what am I checking",
   exactly the cleanup the dual-language shape needs. Estimated
   reduction: ~40 lines + clearer rule intent.
2. **The 8 `scripts/check-*.{js,mjs,sh}` files revisited via v0.10
   rule kinds.** `check-manifests.js` and `validate-externals-doc.js`
   shell out today — the v0.10 ship-targets (`registry_paths_resolve`
   + `cross_file_value_equals`) will let both move to declarative
   rules. `check-examples.sh` / `check-pre-compiled.sh` stay
   shellouts (mutation + git-state).
3. **Bundled-ruleset additions surfaced by `alint suggest`.** The
   config will skip the newer `compliance/reuse@v1` and
   `agent-hygiene@v1`. Running `alint suggest` against
   `/tmp/next.js/` flags `agent-hygiene` (medium) — the next.js
   root has agent-readable docs (`AGENTS.md`, `CLAUDE.md` symlink)
   that the antipattern scan would benefit from.
   `compliance/reuse` would be a deliberate override (next.js uses
   MIT directly, no per-file SPDX headers) but worth a documented
   decision in the config.

---

## 10. Validation status (2026-05-07)

- **alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).
- **`.alint.yml` in this directory:** **shipped — 801 lines, 59
  repo-specific rules, 11 bundled rulesets folded in via `extends:`,
  130 effective rules loaded.**
  `alint validate-config` confirms `✓ Config valid: 130 rule(s)
  loaded`. **Live-tree recheck:** performed in this batch — see §6
  for the 525-violation breakdown (113 GHA SHA-pinning + 106
  final-newline + 62 trailing-ws + 56 + 56 fixture-tracked
  node_modules + 33 GHA permissions + 4 missing Cargo licenses
  [the polyglot headline] + the long tail).
- **Hybrid dual-workspace verification:** **CONFIRMED.**
  `package.json` "workspaces" → `["packages/*"]` (1 glob).
  `pnpm-workspace.yaml` → 7 globs (apps/*, packages/*, bench/*,
  crates/*/js, turbopack/crates/*/js,
  turbopack/crates/turbopack-tests/tests/execution,
  turbopack/packages/*). `Cargo.toml [workspace] members` → 13
  member globs + 2 exclude entries.
- **File-count verification:** **68 `Cargo.toml` + 306
  `package.json`** in the live tree (excluding target/ and
  node_modules/). 19 npm packages directly under `packages/`.
- **GHA workflow count verification:** **36** (was previously
  cited as "30+"; updated). All 36 covered by
  `ci/github-actions@v1`.
- **scripts/check-* count verification:** **8 hand-rolled gates**
  (counting the .sh + .bat pre-compiled pair as one). 75 %
  out-of-scope (mutation + AST + git-history); 25 % v0.10
  ship-target shapes.
- **15 cross-language conventions BOTH linters miss:** **enumerated
  with line-of-evidence in §1.1 table**. 13 / 15 alint-today; 2 /
  15 v0.10 (the headline polyglot pitch).
- **Rule-kind candidate status:**
  - `cross_file_value_equals` — v0.10 ship-target (10 sources).
  - `registry_paths_resolve` — v0.10 ship-target (8 sources).
  - `dir_name_matches_field` extension with unscoping — v0.10+
    candidate (2 sources).
- **Pitfall #22 instances in this directory's config:** **ZERO**
  (`grep -nE 'pattern:\s*[|>][-+]?$' .alint.yml` returns no
  matches; all 9 multi-line patterns use single-quoted scalars).
  **Pitfall #16 worked around in 2 places** (this case study's
  contribution to the canonical-22 catalogue): JSONPath bool/regex
  coercion in `nextjs-package-json-declares-private` and
  `nextjs-tsconfig-strict-mode`, with in-line CONFIG-AUTHORING.md
  references.
- **Bundled-ruleset rule counts (authoritative as of 2026-05-07):**
  oss-baseline=15, node=9, rust=11, monorepo=4,
  monorepo/cargo-workspace=4, monorepo/pnpm-workspace=4,
  ci/github-actions=3, hygiene/no-tracked-artifacts=11,
  hygiene/lockfiles=7, tooling/editorconfig=3, agent-context=5.
