# Case study: `pnpm/pnpm`

> Marketing/positioning writeup at https://alint.org/examples/pnpm-pnpm/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `pnpm/pnpm` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-clone of `pnpm/pnpm@HEAD`.

---

## Summary

pnpm IS the canonical pnpm-workspace monorepo: 169 packages spread
across ~50 functional root directories (`cache/`, `cli/`, `store/`,
`engine/pm/`, `installing/dedupe/`, `deps/compliance/`, etc.) rather
than the usual `packages/*` flat layout. Whatever convention pnpm
enforces on itself becomes the reference shape for the hundreds of
downstream TS/JS monorepos that copy it.

The structural-validation surface centres on **`.meta-updater/`** — a
~470-line in-tree TypeScript plugin (`@pnpm-private/updater`) backed
by `@pnpm/meta-updater` that rewrites cross-package fields on every
`pn install` / `pn update-manifests` run: `license`, `funding`,
`bugs`, `engines.node`, `type`, `homepage`, `repository`,
`keywords`, the `jest.preset`, the `dependencies` sort order,
external deps remapped to `catalog:`, internal deps remapped to
`workspace:*`, the `scripts.lint`/`scripts.test`/`scripts.compile`
shape, and a per-package `tsconfig.json#references` tree derived from
the `pnpm-lock.yaml`. CI fires the same logic in `--test` mode (`pn
lint:meta`); fail if `pn update-manifests` would change anything.

Concrete count: **24 distinct structural-validation surfaces** across
the husky chain, the `meta-updater` invariants, the workspace YAML
shape, the changesets shape, the GitHub workflows, and the
`.editorconfig` / `.gitattributes` / `.pnpmfile.cjs` triple. Of those
surfaces:

- **17 fit alint directly** (~71%) — per-package field shape,
  workspace YAML keys, husky hook presence + content, changeset
  frontmatter shape, gitattributes EOL lockdown, and the four
  `command:` shell-outs to the existing tools (`pn lint`,
  `pn lint:meta`, `cspell`, `pn audit`).
- **3 need new alint primitives** (~13%) — the cross-package field
  sync ("every package's `engines.node` MUST equal the workspace
  root's"), the catalog-protocol completeness check ("every dep
  not `workspace:*` MUST be a key in `pnpm-workspace.yaml#$.catalog`"),
  and the `tsconfig.json#references` graph-traversal check.
- **4 are out of scope** (~16%) — eslint TS/JS lints (AST), cspell
  dictionary lookup, commitlint commit-msg checks (PR-time), and
  `meta-updater`'s 11 special-case packages with carved-out script
  shapes (per-package `switch (manifest.name)` carve-outs in
  `.meta-updater/src/index.ts`).

**Cross-cutting finding:** pnpm's `meta-updater` plugin enforces 13
cross-package field invariants on every install — exactly the
`cross_file_value_equals` shape now a v0.10 ship-target on alint's
roadmap (10 sources past saturation: airflow, tokio, clap, uv, react,
pnpm, nodejs/node, pytorch, vscode, istio). Until it ships, alint
covers 11 of the 13 invariants by asserting against the *expected
literal value* rather than against "whatever the workspace root
declares".

---

## Existing tooling inventory

### `.meta-updater/` — the load-bearing structural-validation tool

`@pnpm-private/updater` is a 470-line TypeScript plugin
(`.meta-updater/src/index.ts`) backed by `@pnpm/meta-updater`. It
runs as a `pnpm` lifecycle hook on every install via the root
`update-manifests` script. The CI lint job (`pn lint:meta`)
re-runs it in `--test` mode; non-zero exit if any file would
change.

The plugin enforces 13 distinct cross-package invariants:

| Invariant | Notes | alint replacement |
|---|---|---|
| `license: "MIT"` on every published member | Same literal value across 169 packages. | ✅ `pnpm-package-license-mit` (literal regex match) |
| `type: "module"` on every member | Repo is ESM end-to-end. | ✅ `pnpm-package-type-module` |
| `funding: "https://opencollective.com/pnpm"` | Same literal value. | ✅ `pnpm-package-funding` |
| `bugs.url: "https://github.com/pnpm/pnpm/issues"` | Same literal value. | ✅ `pnpm-package-bugs-url` |
| `engines.node: ">=22.13"` | Tracks the workspace `nodeVersion`. | ✅ `pnpm-package-engines-node` (matches `^>=22`; literal match would need cross_file_value_equals) |
| `keywords[0]: "pnpm"` | meta-updater rewrites the array. | ✅ `pnpm-package-keywords-pnpm` |
| `repository: "https://github.com/pnpm/pnpm/tree/main/<dir>"` | The `<dir>` part is per-package. | ✅ `pnpm-package-repository-shape` (matches the URL prefix; the `<dir>` exact match would need cross_file_value_equals against the package's own path) |
| `homepage` follows the same scheme + `#readme` suffix | Per-package URL synthesis. | ✅ `pnpm-package-homepage-shape` |
| `dependencies` sorted alphabetically | meta-updater rewrites the object key order. | ❌ Out of scope (key-order is a JSON-AST property; alint doesn't parse JSON sufficiently). |
| External deps remapped to `catalog:` | Read every dep value; if not `workspace:*`/`link:`, set to `catalog:`. | ❌ Needs `cross_file_value_equals` (verify the remap is in sync with `pnpm-workspace.yaml#$.catalog`). |
| Internal deps remapped to `workspace:*` | Same shape, opposite direction. | ❌ Same gap. |
| `scripts.lint` / `scripts.test` / `scripts.compile` rewritten | 11 special-case packages with custom shapes. | ⚠ Partial (we can verify the *common* shape; the carve-outs need per-name allow-listing). |
| `tsconfig.json#references` derived from `pnpm-lock.yaml` | Each ref points at an in-workspace dep. | ❌ Needs `registry_paths_resolve` (already on the v0.10+ list) — every reference must resolve to a member with `composite: true`. |

### `pnpm-workspace.yaml` — the canonical workspace manifest

The 487-line `pnpm-workspace.yaml` declares the workspace shape AND
a long supply-chain-strict default block that's load-bearing for
the project's security posture:

| Key | Why it matters | alint replacement |
|---|---|---|
| `packages: [...]` | Lists every workspace member (52 glob patterns). | ✅ Bundled `monorepo/pnpm-workspace@v1` enforces non-empty. |
| `catalog: {...}` | The strict-catalog map — every external dep version lives here. | ❌ Cross-file consistency needs `cross_file_value_equals`. |
| `catalogMode: strict` | Forces every external dep to come from the catalog. | ✅ `pnpm-catalog-mode-strict` (literal-value match) |
| `engineStrict: true` | Hard-fail installs on wrong Node major. | ✅ `pnpm-engine-strict` |
| `nodeVersion: 22.13.0` | Pins the Node binary `pnpm runtime` resolves. | ✅ `pnpm-workspace-pins-node` |
| `minimumReleaseAge: 1440` | Supply-chain mitigation: refuse packages younger than 1 day. | ✅ `pnpm-minimum-release-age` |
| `trustPolicy: no-downgrade` | Refuse to install a downgrade of an already-trusted package. | ✅ `pnpm-trust-policy` |
| `auditConfig.ignoreGhsas: [...]` | Allow-list of accepted advisories. | (out of scope: list-of-strings doesn't have a structural property to lint) |
| `overrides: {...}` | Pin transitive deps to specific versions. | (out of scope: each override is a domain decision) |
| `packageExtensions: {...}` | Patch upstream package metadata. | (out of scope) |
| `patchedDependencies: {...}` | Per-dep patch files. | ❌ Needs `registry_paths_resolve` — verify each `.patch` file actually exists. Same shape as the airflow / clap candidates. |

### Husky hooks

Four hooks in `.husky/`:

| Hook | What it does | alint replacement |
|---|---|---|
| `commit-msg` | `pn commitlint --edit --config=commitlint.config.cjs` | ✅ `pnpm-husky-commit-msg-hook` + `pnpm-husky-commit-msg-runs-commitlint` |
| `pre-commit` | `pn run compile-only && pn run lint --quiet` | ✅ `pnpm-husky-pre-commit-hook` + `pnpm-husky-pre-commit-runs-compile-and-lint` |
| `pre-push` | `pn run compile-only && pn run lint --quiet` | ✅ `pnpm-husky-pre-push-hook` |
| `prepare-commit-msg` | Block Claude Code from `--amend`ing | ✅ `pnpm-prepare-commit-msg-hook` + `pnpm-prepare-commit-msg-blocks-claude-amend` (the agent guardrail) |

The agent guardrails (Claude-Code amend block + main-branch
direct-commit block in `pre-commit`) are pnpm-specific and a notable
case study for the agentic-era hygiene story: pnpm has *explicitly*
chosen to refuse certain agent operations in-tree, the only
multi-hook setup like this we've seen across the 10 P2a repos so far.

### `.changeset/` — release notes pipeline

| Item | What it checks | alint replacement |
|---|---|---|
| `.changeset/config.json` exists with `access`/`baseBranch`/`changelog` | Required for `pn changeset` to work. | ✅ `pnpm-changeset-config-present` + 2 shape rules |
| Each `.changeset/*.md` carries YAML frontmatter | Empty frontmatter is silently dropped from changelog. | ✅ `pnpm-changeset-files-have-bump-frontmatter` (regex match for `<pkg>: <bump>` + closing `---`) |
| Bump kind is `major`/`minor`/`patch` | Other strings silently ignored. | ✅ Captured in the same regex |
| Package name on LHS exists in workspace | Typos = silent drop. | ❌ Needs `registry_paths_resolve` cross-file form (every `<pkg>:` LHS must match a `name:` from a workspace package.json) |

### `.github/workflows/` (9 workflows)

| Workflow | Purpose | alint replacement |
|---|---|---|
| `ci.yml` | Compile & Lint orchestrator (calls `test.yml`) | ✅ `pnpm-ci-workflow-runs-lint` + `pnpm-ci-workflow-runs-compile` |
| `test.yml` | Reusable test workflow (matrix over Node versions + platforms) | ✅ `pnpm-test-workflow-present` |
| `audit.yml` | Runs `pn audit` on every push | ✅ `pnpm-audit-workflow-present` |
| `release.yml` | Release on `v*.*.*` tag (publishes to npm with provenance) | (out of scope: release-time check) |
| `update-lockfile.yml` | Renovate-style lockfile autoupdate | (out of scope: bot orchestration) |
| `update-latest.yml` | Bumps the `latest` dist-tag | (out of scope) |
| `benchmark.yml` | Performance perf bench | (out of scope) |
| `codeql-analysis.yml` | CodeQL static analysis | (out of scope: security scanner) |
| `docker.yml` | Builds the Docker image | (out of scope) |

The bundled `ci/github-actions@v1` ruleset already enforces every
workflow has `permissions:` declared and every `uses:` is pinned by
SHA. The pnpm-specific rules above only restate the gates pnpm itself
depends on.

### `.editorconfig` + `.gitattributes` + `.pnpmfile.cjs`

| File | What it enforces | alint replacement |
|---|---|---|
| `.editorconfig` | `end_of_line=lf`, `insert_final_newline=true`, `trim_trailing_whitespace=true`, `indent_size=2` for `*.{ts,js,cjs,json}` | ✅ Bundled `tooling/editorconfig@v1` + `pnpm-source-final-newline` / `pnpm-source-no-trailing-whitespace` / `pnpm-source-line-endings-lf` (apply the .editorconfig content to actual sources) |
| `.gitattributes` | `* text eol=lf` + `*.tgz binary` | ✅ `pnpm-gitattributes-eol-lf` (the eol=lf line was a recurring cross-platform diff churn source before it was added) |
| `.pnpmfile.cjs` | install-time `readPackage` + `beforePacking` hooks | ✅ `pnpm-pnpmfile-present` + `pnpm-pnpmfile-keeps-before-packing-guard` (the `beforePacking` guard rejects `pnpm` package publishes when peerDependencies are non-empty — load-bearing for the bundled-publish flow) |

### `cspell.json` — spellcheck

| Item | What it checks | alint replacement |
|---|---|---|
| `cspell.json` exists | Required for `pn spellcheck` to work. | ✅ `pnpm-cspell-config-present` |
| `$.words[]` is non-empty | Empty dictionary makes spellcheck either explode with false positives or silently pass everything. | ✅ `pnpm-cspell-words-non-empty` |
| Each word in dictionary actually used somewhere in repo | Stale entries accumulate. | ❌ Out of scope (cross-file token-presence check; same shape as clap's `regex_resolves_in_file` candidate). |

### `commitlint.config.cjs`

| Item | What it checks | alint replacement |
|---|---|---|
| Config file exists | Required for husky commit-msg hook. | ✅ `pnpm-commitlint-config-present` |
| Extends `@commitlint/config-conventional` | Without the extend, the hook applies no rules. | ✅ `pnpm-commitlint-config-extends-conventional` |
| Commit message follows Conventional Commits | Per-commit check, not repo-state. | ❌ Out of scope (PR-/commit-time check; same as turbo's `lint-pr-title.yml` gate). |

### Per-package layout (169 workspace members)

| Convention | What | alint replacement |
|---|---|---|
| Each member has `package.json` | Required by pnpm. | ✅ Bundled `monorepo@v1` covers `packages/*` shape; pnpm's polyrepo layout needs the explicit `for_each_dir` over the ~33 functional root dirs. |
| Each member has `tsconfig.json` | Required for `tsgo --build`. | ✅ `pnpm-workspace-member-has-tsconfig` (long brace-alternation glob over the functional root dirs) |
| Each member has `tsconfig.lint.json` (auto-generated by meta-updater) | Smoking-gun signal that `pn update-manifests` was skipped. | ✅ `pnpm-workspace-member-has-tsconfig-lint` |
| Each member's `tsconfig.json` extends `@pnpm/tsconfig` | Drift = custom config. | ✅ `pnpm-workspace-member-tsconfig-extends-shared` |
| Each member has `README.md` | Convention. | ✅ Bundled `monorepo/pnpm-workspace@v1` covers the canonical `packages/*` shape; pnpm's layout drifts so we don't restate. |

Of the 169 workspace members, **19 currently lack a README.md**
(verified at HEAD): `.meta-updater`, `__typings__`, `__typecheck__`,
`store/commands`, `global/packages`, `text/tree-renderer`,
`network/web-auth`, `pnpm/dev`, `releasing/commands`,
`__utils__/get-release-text`, `__utils__/scripts`, `__utils__/prepare`,
`__utils__/jest-config`, `__utils__/prepare-temp-dir`,
`auth/commands`, `cli/commands`, `testing/mock-agent`,
`testing/command-defaults`, `building/commands`. The bundled
`monorepo/pnpm-workspace@v1` rule fires on `packages/*` only and
doesn't catch these (pnpm's layout is the off-the-beaten-path case
the bundled overlay was designed *not* to over-fire on).

### Maps-to-alint vs needs-new-primitive vs out-of-scope

24 surfaces inventoried (including the `pnpm-workspace.yaml` fields
counted as one row each):

- **17 fit alint directly** (71%)
- **3 need new alint primitives** (13%)
- **4 are out of scope** (16%)

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Adopts the bundled
`oss-baseline + node + monorepo + monorepo/pnpm-workspace + ci/github-actions
+ hygiene/no-tracked-artifacts + hygiene/lockfiles + tooling/editorconfig
+ agent-context` overlays, then layers ~40 pnpm-specific rules on top.

Notable rules:

- **`pnpm-catalog-mode-strict`** — locks `catalogMode: strict` so the
  catalog-protocol discipline can't silently drift to `manual`. The
  one rule that, if it fires, indicates a *policy* regression rather
  than an editing slip.
- **`pnpm-package-{license,type,funding,bugs-url,engines-node,keywords-pnpm,repository-shape,homepage-shape}`** —
  eight per-member rules iterating the 33-direction brace-alternation
  glob (`{cache,cli,config,…}/*/package.json`) covering the literal-value
  invariants meta-updater enforces.
- **`pnpm-workspace-member-has-{tsconfig,tsconfig-lint}`** — every
  member needs `tsconfig.json` (real package) AND `tsconfig.lint.json`
  (auto-generated by meta-updater). The latter is the canary: missing
  it means the package was added without `pn update-manifests`.
- **`pnpm-workspace-member-tsconfig-extends-shared`** — each member's
  tsconfig must extend `@pnpm/tsconfig` (the workspace's shared
  TypeScript config in `__utils__/tsconfig/`).
- **`pnpm-meta-updater-{package-present,source-present,in-workspace}`** +
  **`pnpm-root-has-lint-meta-script`** — the meta-updater plugin's
  presence + invocation gates. Together they catch "someone removed
  the .meta-updater package and `pn lint:meta` silently passes".
- **`pnpm-changeset-files-have-bump-frontmatter`** — regex match for
  the `---\n"<pkg>": <bump>\n---` shape with `(?s)` so `.` matches
  newlines.
- **`pnpm-{husky-*-hook,commitlint-config-*,pnpmfile-*}`** — gate
  rules around the husky chain + commitlint + .pnpmfile.cjs invariants.
- **`pnpm-prepare-commit-msg-blocks-claude-amend`** — the
  Claude-Code amend-block guardrail. Notable: pnpm is the only
  P2a repo so far that has explicitly chosen to refuse certain
  agent operations in-tree.
- **`pnpm-{lint,lint-meta,spellcheck,audit}`** — four `command:`
  rules that wrap the existing tools. Together with the static
  rules above, `alint check` covers the structural floor + the
  existing toolchain.

---

## What needs new alint primitives

Three patterns specific to pnpm that don't fit any current rule:

### 1. `cross_file_value_equals` — the meta-updater shape (now v0.10 ship-target, 10 sources)

Eight of meta-updater's invariants are shaped "value at JSONPath X
in file A must equal value at JSONPath Y in file B" or "the value at
X in file A must equal a literal derived from A's own path":

- `engines.node` in every per-package `package.json` MUST equal
  `pnpm-workspace.yaml#$.nodeVersion` (with the patch component
  stripped to `>=<major>.<minor>`).
- `repository` in every per-package `package.json` MUST equal
  `https://github.com/pnpm/pnpm/tree/main/${dir}` where `${dir}`
  is the package's path relative to the workspace root.
- `homepage` MUST equal the same shape with `#readme` suffix
  (except for the CLI which uses `https://pnpm.io`).
- For external deps in every `dependencies`/`devDependencies`,
  the value MUST be `catalog:` AND the dep name MUST appear as a
  key under `pnpm-workspace.yaml#$.catalog`.
- For internal deps (deps whose name appears as a `name:` field
  in *some* workspace member's package.json), the value MUST be
  `workspace:*`.

This is **`cross_file_value_equals`, now a v0.10 ship-target with 10
sources past saturation** (airflow + tokio + clap + uv + react + pnpm
+ nodejs/node + pytorch + vscode + istio). pnpm pushes it from
"useful" to "critical": it's the dominant shape of meta-updater's
enforcement and it's how every downstream pnpm-workspace monorepo
will want to express the same conventions at lower cost than rolling
their own meta-updater plugin.

**Catalog completeness sub-shape (potential new rule kind variant):**
"every dep value not equal to `workspace:*`/`link:` MUST appear as
a key in `pnpm-workspace.yaml#$.catalog`". This is one direction
deeper than the standard cross_file_value_equals — it's "value at
X in file A must be present in the *key set* under Y in file B".
Worth scoping the cross_file_value_equals design with this variant
in mind.

### 2. `registry_paths_resolve` — patched-dependencies + tsconfig references + changeset package names

Three different uses, same primitive:

- `patchedDependencies` in `pnpm-workspace.yaml` maps each patched
  dep to a `<patches-dir>/<dep>@<version>.patch` file. Every
  referenced patch file MUST exist on disk under `__patches__/`.
- `tsconfig.json#references[].path` in every workspace member must
  point at a directory that contains a `tsconfig.json` with
  `composite: true`.
- Every changeset `<pkg-name>:` LHS must match a `name:` field in
  some workspace member's `package.json`.

All three covered by `registry_paths_resolve`, **now a v0.10
ship-target with 8 sources** (rust, clap, cpython×2, next.js, arrow,
pytorch, nodejs/node, NixOS×3). pnpm reinforces the demand and adds
the patched-deps + tsconfig-refs sub-cases.

### 3. `json_key_sort_order` — alphabetical dependencies

meta-updater rewrites every per-package `dependencies` /
`devDependencies` / `optionalDependencies` in alphabetical key
order. There's no current alint rule that asserts JSON object
key order — it's a property of the AST, not the deserialised
value, and the JSON deserialisers alint uses (serde_json into
Value) drop key order on the floor.

This is **NEW** for the v0.10+ candidate list. Demand so far is
just pnpm; airflow's pre-commit hook for pyproject.toml key
ordering is the closest parallel. Worth tracking but not a v0.10
priority — `cross_file_value_equals` covers more ground for less
machinery.

---

## What's out of alint's scope (kept on the existing tool)

- **eslint** (`eslint.config.mjs` + `@pnpm/eslint-config`) — TS/JS
  AST analysis. `command:` shell-out via `pnpm-lint`.
- **cspell** dictionary lookup — even if alint had a "every word
  in dictionary X must appear in tree" rule, cspell does the
  inverse (every word in tree must be in dictionary X) which is
  fundamentally a token-stream operation. Out of scope.
- **commitlint** — runs against the in-flight commit message at
  hook time, not against repo state.
- **`meta-updater`'s 11 special-case packages** — the `switch
  (manifest.name)` carve-outs in `.meta-updater/src/index.ts`
  give 11 specific package names custom `scripts.test` shapes
  (e.g. `@pnpm/installing.deps-installer` runs jest with a
  different registry-mock port). Alint can verify the *common*
  shape but the carve-outs would need per-name allow-listing —
  not worth the rule-config complexity for 11 manifests.
- **PR-/commit-time checks** — release.yml content, lint-pr-title
  workflow, every CI job that operates on a `git diff` rather
  than the working tree. Same class as turbo's
  `pr-modified-files.yml`.
- **`pnpm-lock.yaml` content checks** — alint can verify the file
  exists (the bundled `hygiene/lockfiles@v1` ruleset does) and
  that its content matches a regex, but the lockfile is generated
  and content-hashed by pnpm itself; structural lint of the
  YAML is the wrong tool for the job.

---

## Performance comparison (placeholder — bench when validation pass scales)

The repo is medium-large by P2a standards:
- **169 packages** across ~50 functional root dirs
- **~12 MiB** of working-tree content after sparse-checkout
- **~2,500 source files** (TS) under per-package `src/`

The published S3 bench (100k files, mixed languages) hits 1.13 s
for the workspace bundle on a stock CI runner. At ~10k files
post-sparse-checkout, expect 200-500 ms for the structural rules
on the pnpm tree, vs.:

- `pn lint` (eslint + cspell + lint:meta chained sequentially):
  ~30-90 s cold cache.
- `pn lint:meta` alone: ~3-5 s (meta-updater is fast — it's a
  single Node process iterating the workspace).

Where alint shines specifically on pnpm: the **per-package field
shape rules** run against 169 manifests in tens of milliseconds
(sequential `jq` on 169 files with shell would be ~5-10 s). The
cross-cutting structural floor pays back the most when the repo
size is dominated by many small homogeneous package directories
— exactly pnpm's shape.

To benchmark for real: `time pn lint:meta` against
`time alint check` on the same checkout. Deferred to the per-repo
measurement pass.

---

## Followup feature work

Marketing/positioning context for this case study lives at
https://alint.org/examples/pnpm-pnpm/. The engineering follow-up
work surfaced is consolidated below.

- **`cross_file_value_equals` rule kind** — v0.10 ship-target (10
  sources past saturation). pnpm reinforces the demand from a Tier-1
  npm-ecosystem repo and adds the **catalog-completeness sub-shape**
  (value at X must appear in the *key set* under Y in file B) as a
  variant worth scoping into the design.
- **`registry_paths_resolve` rule kind** — v0.10 ship-target (8
  sources). pnpm adds three new use cases:
  patchedDependencies → `__patches__/` files,
  tsconfig.json#references → composite-tsconfig-having dirs,
  changeset frontmatter → workspace package names.
- **`json_key_sort_order`** (NEW for the candidate list) — assert
  alphabetical key order on a JSON object. Demand: pnpm
  meta-updater. Not a priority but worth noting as the unique-to-pnpm
  gap.

No new schema or language pitfalls hit while writing this config —
the 21 documented in `docs/development/CONFIG-AUTHORING.md` cover
everything that came up. Notable confirmations:

- The brace-alternation glob `{cache,cli,config,core,…}/*` worked
  cleanly across 33 functional root dirs (no escaping issues
  inside YAML scalars).
- JSONPath bracket notation needed for `pnpm-workspace.yaml#$['catalogMode']`,
  `$['engineStrict']`, `$['nodeVersion']`, `$['minimumReleaseAge']`,
  `$['trustPolicy']` — all are root-level keys without dashes
  (camelCase), but defensive bracket-notation guarded against
  the future case where someone renames one with a dash.
- The `(?s)` modifier on the changeset-frontmatter regex worked
  as documented (option B in pitfall #14).
- `scope_filter.has_ancestor: package.json` on the source-file
  hygiene rules correctly scoped to "files inside a Node package"
  in the polyrepo layout — the bundled `node@v1` overlay's
  pattern carries through.

---

## Future analysis

- **Per-binding `nested_configs:` split.** The 9 bundled rulesets +
  ~40 pnpm-specific rules sit in one 913-line `.alint.yml`. Splitting
  the per-package-shape rules (per-functional-root brace-alternation
  globs) into per-directory `.alint.yml` files via `nested_configs:
  true` would let each functional area (`cache/`, `cli/`,
  `engine/pm/`, etc.) evolve independently. Worth considering as the
  config grows.
- **Pre-commit fastpath.** pnpm's husky chain runs `pn run
  compile-only && pn run lint --quiet` on pre-commit. Layering
  `alint check --changed` (uses `git ls-files --modified --others
  --exclude-standard` — exactly the pre-commit shape) would give a
  fast structural-floor check at the same hook point.
- **`hygiene/lockfiles@v1` (7 rules)** — already extended; covers
  the pnpm-lock.yaml presence shape but not deep schema. When a
  pnpm-aware rule kind ships, revisit.

## Validation status (2026-05-07)

- alint binary: v0.9.17 (built 2026-05-07).
- `validate-config` reports **112 rules** loaded from `.alint.yml`
  (51 pnpm-specific + 61 from 9 bundled rulesets: oss-baseline 15 +
  node 9 + monorepo 4 + monorepo/pnpm-workspace 4 + ci/github-actions
  3 + hygiene/no-tracked-artifacts 11 + hygiene/lockfiles 7 +
  tooling/editorconfig 3 + agent-context 5).
- No `respect_gitignore: false` or `root_only: true` patterns in this
  config. Pitfalls #18 (FIXED v0.9.17) and #19 (FIXED v0.9.17) do
  not apply.
- Live-tree recheck not performed (no /tmp/pnpm checkout available).
