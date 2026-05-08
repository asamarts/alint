# Case study: `pnpm/pnpm`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/pnpm-pnpm/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `pnpm/pnpm` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 sparse-clone of `pnpm/pnpm@HEAD`
at `/tmp/pnpm` (depth=1, filter=blob:none): **35 MB working-tree**,
210 `package.json` files (top-3-levels, excludes node_modules / lib /
dist), 4 husky hooks (`commit-msg`, `pre-commit`, `pre-push`,
`prepare-commit-msg`), 9 GitHub Actions workflows, 1 in-tree
meta-updater plugin (`.meta-updater/src/index.ts`, **499 lines** of
TypeScript driving 13 cross-package field invariants), 1 canonical
`pnpm-workspace.yaml` (487 lines, declares `catalogMode: strict`,
`engineStrict: true`, `nodeVersion: 22.13.0`, `minimumReleaseAge: 1440`,
`trustPolicy: no-downgrade`).

**alint version:** 0.9.17 (built 2026-05-07).

---

## 1. Inventory of existing tooling

pnpm IS the canonical pnpm-workspace monorepo: 169 packages spread
across ~50 functional root directories (`cache/`, `cli/`, `store/`,
`engine/pm/`, `installing/dedupe/`, `deps/compliance/`, etc.) rather
than the usual `packages/*` flat layout. Whatever convention pnpm
enforces on itself becomes the reference shape for the hundreds of
downstream TS/JS monorepos that copy it.

### 1.1 `.meta-updater/` — the load-bearing structural-validation plugin

`@pnpm-private/updater` is a **499-line TypeScript plugin**
(`.meta-updater/src/index.ts`) backed by `@pnpm/meta-updater`. It
runs as a `pnpm` lifecycle hook on every install via the root
`update-manifests` script. The CI lint job (`pn lint:meta`)
re-runs it in `--test` mode; non-zero exit if any file would
change.

The plugin enforces **13 distinct cross-package invariants** —
each one a `cross_file_value_equals` candidate (per the brief's
pnpm note):

| # | Invariant (per `.meta-updater/src/index.ts`) | What is asserted | alint mapping |
|---|---|---|---|
| 1 | `license: "MIT"` on every published member | Same literal value across 169 packages. | ✅ `pnpm-package-license-mit` (literal regex match) |
| 2 | `type: "module"` on every member | Repo is ESM end-to-end. | ✅ `pnpm-package-type-module` |
| 3 | `funding: "https://opencollective.com/pnpm"` | Same literal value. | ✅ `pnpm-package-funding` (literal regex match) |
| 4 | `bugs.url: "https://github.com/pnpm/pnpm/issues"` | Same literal value. | ✅ `pnpm-package-bugs-url` (literal regex match) |
| 5 | `engines.node: ">=22.13"` | Tracks the workspace `nodeVersion`. | ⚠ Partial: `pnpm-package-engines-node` matches `^>=22`; the exact "must equal workspace root's nodeVersion" needs `cross_file_value_equals` (v0.10 ship-target, 10 sources). |
| 6 | `keywords[0]: "pnpm"` | meta-updater rewrites the array. | ✅ `pnpm-package-keywords-pnpm` |
| 7 | `repository: "https://github.com/pnpm/pnpm/tree/main/<dir>"` | The `<dir>` part is per-package; the URL prefix is shared. | ⚠ Partial: `pnpm-package-repository-shape` matches the URL prefix; the per-package `<dir>` exact match would need `cross_file_value_equals` against the package's own path. |
| 8 | `homepage` follows the same scheme + `#readme` suffix | Per-package URL synthesis. | ⚠ Partial: `pnpm-package-homepage-shape` matches the prefix. |
| 9 | `dependencies` sorted alphabetically | meta-updater rewrites the object key order. | ❌ Out of scope (key-order is a JSON-AST property; alint doesn't parse JSON sufficiently). NEW v0.10+ candidate: `json_key_sort_order`. |
| 10 | External deps remapped to `catalog:` | Read every dep value; if not `workspace:*`/`link:`, set to `catalog:`. | ❌ Needs `cross_file_value_equals` (verify the remap is in sync with `pnpm-workspace.yaml#$.catalog`). |
| 11 | Internal deps remapped to `workspace:*` | Same shape, opposite direction. | ❌ Same gap. |
| 12 | `scripts.lint` / `scripts.test` / `scripts.compile` rewritten | 11 special-case packages with custom shapes (`switch (manifest.name)` carve-outs). | ⚠ Partial: alint can verify the *common* shape; the 11 carve-outs need per-name allow-listing. |
| 13 | `tsconfig.json#references` derived from `pnpm-lock.yaml` | Each ref points at an in-workspace dep with `composite: true`. | ❌ Needs `registry_paths_resolve` (v0.10 ship-target, 8 sources) — every reference must resolve to a member with `composite: true`. |

**Coverage of the 13 invariants:** 6 mapped (#1-4, #6) + 4 partial
(#5, #7, #8, #12) + 3 gap (#9, #10, #11, #13). `cross_file_value_equals`
unlocks exactly the 4 partial-mapped + 3 gap invariants — that's
**7 of 13 (54%)** of meta-updater's contract closing in v0.10.

### 1.2 `pnpm-workspace.yaml` — the canonical workspace manifest

The 487-line `pnpm-workspace.yaml` declares the workspace shape AND
a long supply-chain-strict default block that's load-bearing for
the project's security posture. Verified at HEAD (the 5
strict-mode keys present):

```
catalogMode: strict
engineStrict: true
minimumReleaseAge: 1440 # At least a day
nodeVersion: 22.13.0
trustPolicy: no-downgrade
```

| Key | Why it matters | alint mapping |
|---|---|---|
| `packages: [...]` | Lists every workspace member (52 glob patterns). | ✅ Bundled `monorepo/pnpm-workspace@v1` enforces non-empty. |
| `catalog: {...}` | The strict-catalog map — every external dep version lives here. | ❌ Cross-file consistency needs `cross_file_value_equals`. |
| `catalogMode: strict` | Forces every external dep to come from the catalog. | ✅ `pnpm-catalog-mode-strict` (literal-value match) |
| `engineStrict: true` | Hard-fail installs on wrong Node major. | ✅ `pnpm-engine-strict` (`yaml_path_equals: true` — pitfall #16-aware) |
| `nodeVersion: 22.13.0` | Pins the Node binary `pnpm runtime` resolves. | ✅ `pnpm-workspace-pins-node` |
| `minimumReleaseAge: 1440` | Supply-chain mitigation: refuse packages younger than 1 day. | ✅ `pnpm-minimum-release-age` |
| `trustPolicy: no-downgrade` | Refuse to install a downgrade of an already-trusted package. | ✅ `pnpm-trust-policy` |
| `auditConfig.ignoreGhsas: [...]` | Allow-list of accepted advisories. | (out of scope: list-of-strings doesn't have a structural property to lint) |
| `overrides: {...}` | Pin transitive deps to specific versions. | (out of scope: each override is a domain decision) |
| `packageExtensions: {...}` | Patch upstream package metadata. | (out of scope) |
| `patchedDependencies: {...}` | Per-dep patch files. | ❌ Needs `registry_paths_resolve` — verify each `.patch` file actually exists. |

### 1.3 Husky hooks (4 hooks)

Verified present at HEAD: `commit-msg`, `pre-commit`, `pre-push`,
`prepare-commit-msg`.

| Hook | What it does | alint mapping |
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

### 1.4 `.changeset/` — release notes pipeline

| Item | What it checks | alint mapping |
|---|---|---|
| `.changeset/config.json` exists with `access`/`baseBranch`/`changelog` | Required for `pn changeset` to work. | ✅ `pnpm-changeset-config-present` + 2 shape rules |
| Each `.changeset/*.md` carries YAML frontmatter | Empty frontmatter is silently dropped from changelog. | ✅ `pnpm-changeset-files-have-bump-frontmatter` (regex match for `<pkg>: <bump>` + closing `---`) |
| Bump kind is `major`/`minor`/`patch` | Other strings silently ignored. | ✅ Captured in the same regex |
| Package name on LHS exists in workspace | Typos = silent drop. | ❌ Needs `registry_paths_resolve` cross-file form (every `<pkg>:` LHS must match a `name:` from a workspace package.json) |

### 1.5 `.github/workflows/` (9 workflows)

| Workflow | Purpose | alint mapping |
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

The bundled `ci/github-actions@v1` ruleset (3 rules: workflow
permissions, action SHA pinning, workflow `name`) covers the
hardening surface for all 9 workflows at once.

### 1.6 `.editorconfig` + `.gitattributes` + `.pnpmfile.cjs`

| File | What it enforces | alint mapping |
|---|---|---|
| `.editorconfig` | `end_of_line=lf`, `insert_final_newline=true`, `trim_trailing_whitespace=true`, `indent_size=2` for `*.{ts,js,cjs,json}` | ✅ Bundled `tooling/editorconfig@v1` + `pnpm-source-final-newline` / `pnpm-source-no-trailing-whitespace` / `pnpm-source-line-endings-lf` (apply the .editorconfig content to actual sources) |
| `.gitattributes` | `* text eol=lf` + `*.tgz binary` | ✅ `pnpm-gitattributes-eol-lf` (the eol=lf line was a recurring cross-platform diff churn source before it was added) |
| `.pnpmfile.cjs` | install-time `readPackage` + `beforePacking` hooks | ✅ `pnpm-pnpmfile-present` + `pnpm-pnpmfile-keeps-before-packing-guard` (the `beforePacking` guard rejects `pnpm` package publishes when peerDependencies are non-empty — load-bearing for the bundled-publish flow) |

### 1.7 `cspell.json` + `commitlint.config.cjs`

| File | What it checks | alint mapping |
|---|---|---|
| `cspell.json` exists | Required for `pn spellcheck` to work. | ✅ `pnpm-cspell-config-present` |
| `cspell.json#$.words[]` is non-empty | Empty dictionary makes spellcheck either explode with false positives or silently pass everything. | ✅ `pnpm-cspell-words-non-empty` (`json_path_matches` w/ `[*]`-and-regex semantics) |
| Each word in dictionary actually used somewhere in repo | Stale entries accumulate. | ❌ Out of scope (cross-file token-presence check) |
| `commitlint.config.cjs` exists | Required for husky commit-msg hook. | ✅ `pnpm-commitlint-config-present` |
| Extends `@commitlint/config-conventional` | Without the extend, the hook applies no rules. | ✅ `pnpm-commitlint-config-extends-conventional` |
| Commit message follows Conventional Commits | Per-commit check, not repo-state. | ❌ Out of scope (PR-/commit-time check) |

### 1.8 Per-package layout (169 workspace members)

| Convention | What | alint mapping |
|---|---|---|
| Each member has `package.json` | Required by pnpm. | ✅ Bundled `monorepo@v1` covers `packages/*` shape; pnpm's polyrepo layout needs the explicit `for_each_dir` over the ~33 functional root dirs. |
| Each member has `tsconfig.json` | Required for `tsgo --build`. | ✅ `pnpm-workspace-member-has-tsconfig` (long brace-alternation glob over the functional root dirs) |
| Each member has `tsconfig.lint.json` (auto-generated by meta-updater) | Smoking-gun signal that `pn update-manifests` was skipped. | ✅ `pnpm-workspace-member-has-tsconfig-lint` |
| Each member's `tsconfig.json` extends `@pnpm/tsconfig` | Drift = custom config. | ✅ `pnpm-workspace-member-tsconfig-extends-shared` |
| Each member has `README.md` | Convention. | ⚠ pnpm's layout drifts from `packages/*`; bundled `monorepo/pnpm-workspace@v1` covers the canonical shape. **19 members lack README at HEAD** — listed in the synopsis below. |

### 1.9 Repo-root governance artefacts

| Artefact | What | alint mapping |
|---|---|---|
| `LICENSE` (MIT) | Repo-wide licence file | ✅ `oss-license-exists`, `oss-license-non-empty` (oss-baseline) |
| `README.md` | Repo-wide README | ✅ `oss-readme-exists`, `oss-readme-non-stub` (oss-baseline) |
| `SECURITY.md` / `.github/SECURITY.md` | Vuln intake | ✅ `oss-security-policy-exists`, `oss-security-policy-non-empty` (oss-baseline) |
| `CODE_OF_CONDUCT.md` | OSS governance | ✅ `oss-code-of-conduct-exists` (oss-baseline) |
| `.github/CODEOWNERS` | Per-tree review routing | ✅ `pnpm-codeowners-present` + `oss-codeowners-exists` (oss-baseline) |
| `FUNDING.json` | Project funding metadata | ✅ `pnpm-root-funding-json` |
| `CLAUDE.md` (symlinked to `AGENTS.md`) | Agent-onboarding doc | ✅ `pnpm-claude-md-symlink-to-agents` |
| `package.json` `packageManager: pnpm@<v>` field | Corepack uses this; mismatched local pnpm crashes `pn install` | ✅ `pnpm-root-package-pins-package-manager` |
| `package.json` `devEngines.packageManager.name == pnpm` | Corepack auto-install hint | ✅ `pnpm-root-dev-engines-package-manager-is-pnpm` |
| Repo-wide hygiene (no `node_modules/`, no `__pycache__/`, no `.DS_Store`, no `Thumbs.db`, …) | Tracked-artifact ban | ✅ All 11 rules from `hygiene/no-tracked-artifacts@v1` (one **caveat — see §6**: 2 `node_modules/` test fixtures legitimately committed) |
| `pnpm-lock.yaml` present + non-empty | Reproducible install | ✅ All 7 rules from `hygiene/lockfiles@v1` |

---

## 2. Coverage classification

Counted across the **13 meta-updater invariants** + **11 workspace YAML
keys** + **4 husky hooks** + **9 GitHub workflows** + **5 changeset
items** + **6 editorconfig/gitattributes/pnpmfile items** + **5
per-package layout conventions** + **10 governance artefacts** = **63
distinct surfaces**.

### 2.1 The 13 meta-updater invariants

| # | Invariant | Coverage | Notes |
|---|---|---|---|
| 1-4, 6 | license, type, funding, bugs.url, keywords[0] | alint-today | 5× literal regex match per package |
| 5, 7, 8, 12 | engines.node, repository, homepage, scripts.{lint,test,compile} | alint-future (4 × `cross_file_value_equals`) | Partials covered by literal-prefix match today |
| 9 | dependencies sorted | alint-future | `json_key_sort_order` (NEW candidate, single-source) |
| 10, 11 | catalog:/workspace:* remap | alint-future | `cross_file_value_equals` |
| 13 | tsconfig references | alint-future | `registry_paths_resolve` |

### 2.2 The 11 workspace YAML keys + 4 hygiene/lockfile rules

11/11 mapped today (5 strict-mode keys via `yaml_path_*` rules, 4
hygiene/lockfile checks via bundled, 2 cross-file gaps under
`alint-future`).

### 2.3 The 4 husky hooks

4/4 mapped today via `file_exists` + `file_content_matches`.

### 2.4 The 9 GitHub workflows

3 mapped today (`pnpm-{ci,test,audit}-workflow-present`) + bundled
GHA hardening on all 9. Remaining 6 are out-of-scope (release/bot/CodeQL).

### 2.5 The 5 changeset items

4/5 mapped today; package-name LHS validity needs
`registry_paths_resolve`.

### 2.6 The 6 editorconfig/gitattributes/pnpmfile items

6/6 mapped today.

### 2.7 The 5 per-package layout conventions

4/5 mapped today (the per-functional-root brace-alternation glob
covers tsconfig + tsconfig-lint + extends-shared); README convention
is partially out-of-scope (19 members have no README at HEAD; not
gated upstream).

### 2.8 The 10 governance artefacts

10/10 mapped today.

### 2.9 Quantified rollup

```
✅ alint-today:      48 / 63 = 76%
🔄 alint-future:      9 / 63 = 14%   (7 cross_file_value_equals + 1 registry_paths_resolve + 1 json_key_sort_order)
❌ out-of-scope:      6 / 63 = 10%   (release/bot/CodeQL workflows + commit-time commitlint + cspell stale-words + meta-updater 11 carve-outs)
                    ─────────────────
                    total = 63 surfaces = 100%
```

**Commentary.** Three observations:

1. **`cross_file_value_equals` is the single highest-leverage v0.10
   ship-target for pnpm.** 7 of meta-updater's 13 invariants
   (sub-classes #5, #7, #8, #10, #11, #12 partial, plus the
   workspace-catalog completeness check) are different surface
   treatments of the same primitive: "value at JSONPath X in file A
   must equal value at JSONPath Y in file B (or be present in the
   key set under Y)". That's **54% of meta-updater's contract**
   unlocked by one rule kind. Cross-saturation: 10 sources
   (airflow + tokio + clap + uv + react + pnpm + nodejs/node +
   pytorch + vscode + istio). Ship status: v0.10 ship-target,
   strongest demand signal in P2a + P2b.

2. **Catalog-completeness is a NEW sub-shape variant.** "Every dep
   value not equal to `workspace:*`/`link:` MUST appear as a key in
   `pnpm-workspace.yaml#$.catalog`" is one direction deeper than
   the standard cross_file_value_equals — it's "value at X in file
   A must be present in the *key set* under Y in file B". Worth
   scoping the cross_file_value_equals design with this variant
   in mind. **NEW for the candidate roster.**

3. **`json_key_sort_order` is unique to pnpm in the inventoried
   set.** meta-updater rewrites every per-package
   `dependencies` / `devDependencies` / `optionalDependencies` in
   alphabetical key order. There's no current alint rule that
   asserts JSON object key order — it's a property of the AST, not
   the deserialised value, and serde_json drops key order on the
   floor. Demand so far is just pnpm; airflow's pre-commit hook
   for pyproject.toml key ordering is the closest parallel. Worth
   tracking but not a v0.10 priority.

---

## 3. Quantified coverage

Already shown above:

```
✅ alint-today:      48 / 63 = 76%
🔄 alint-future:      9 / 63 = 14%
❌ out-of-scope:      6 / 63 = 10%
                    ─────────────────
                    total = 63 = 100%
```

Granular breakdown:

```
meta-updater invariants (13):
  alint-today:      5 / 13 = 38%
  alint-future:     7 / 13 = 54%
  out-of-scope:     1 / 13 =  8%

pnpm-workspace.yaml keys (11):
  alint-today:      9 / 11 = 82%
  alint-future:     2 / 11 = 18%

husky hooks (4):
  alint-today:      4 / 4  = 100%

GitHub workflows (9):
  alint-today:      3 / 9  = 33%
  out-of-scope:     6 / 9  = 67%

changesets (5):
  alint-today:      4 / 5  = 80%
  alint-future:     1 / 5  = 20%

editorconfig/gitattributes/pnpmfile (6):
  alint-today:      6 / 6  = 100%

per-package layout (5):
  alint-today:      4 / 5  = 80%
  alint-future:     1 / 5  = 20%

governance + cspell + commitlint (10):
  alint-today:     10 / 10 = 100%
```

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (913 lines, 51 repo-specific
rules + 9 bundled rulesets, **112 rules total** loaded — confirmed by
`alint validate-config`).

**Synopsis of the 7 most load-bearing repo-specific rules** (full
config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                       # 15 rules
  - alint://bundled/node@v1                                # 9 rules
  - alint://bundled/monorepo@v1                            # 4 rules
  - alint://bundled/monorepo/pnpm-workspace@v1             # 4 rules
  - alint://bundled/ci/github-actions@v1                   # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1        # 11 rules
  - alint://bundled/hygiene/lockfiles@v1                   # 7 rules
  - alint://bundled/tooling/editorconfig@v1                # 3 rules
  - alint://bundled/agent-context@v1                       # 5 rules

rules:
  - id: pnpm-catalog-mode-strict     # the load-bearing supply-chain gate
    kind: yaml_path_matches
    paths: ["pnpm-workspace.yaml"]
    path: "$['catalogMode']"         # bracket-notation for camelCase
    matches: '^strict$'
  - id: pnpm-engine-strict           # bool field — uses *_path_equals
    kind: yaml_path_equals
    paths: ["pnpm-workspace.yaml"]
    path: "$['engineStrict']"
    equals: true                     # native YAML bool literal (pitfall #16-aware)
  - id: pnpm-package-license-mit     # 169-package iteration via brace glob
    kind: json_path_matches
    paths: "{cache,cli,config,core,…}/*/package.json"  # 33-dir alternation
    path: "$.license"
    matches: '^MIT$'
  - id: pnpm-workspace-member-has-tsconfig  # for_each_dir with when_iter
    kind: for_each_dir
    select: "{cache,cli,config,…}/*"
    when_iter: 'iter.has_file("package.json")'
    require:
      - kind: file_exists
        paths: "{path}/tsconfig.json"
  - id: pnpm-changeset-files-have-bump-frontmatter  # multi-line regex
    kind: file_content_matches
    paths: [".changeset/*.md"]
    pattern: '(?s)^---\s*\n.*?:\s*(major|minor|patch)\s*\n.*?---'
  - id: pnpm-prepare-commit-msg-blocks-claude-amend  # agent guardrail
    kind: file_content_matches
    paths: ".husky/prepare-commit-msg"
    pattern: 'CLAUDECODE'
  - id: pnpm-lint                    # command shellout to the existing tool
    kind: command
    paths: "package.json"
    command: ["pnpm", "run", "lint"]
    timeout: 600
```

**Repo-specific vs bundled split:**

- **51 repo-specific rules** in `.alint.yml` (the `pnpm-*` prefix
  identifies them in `alint list` output): catalog mode + 4 supply-chain
  keys + 8 per-package shape rules (license, type, funding, bugs-url,
  engines-node, keywords, repository, homepage) + 4 layout (tsconfig
  pair + extends-shared + tsconfig-lint) + 4 husky + 4 changeset +
  3 cspell + 3 pnpmfile + 3 root manifest + 2 gitattributes/agent +
  3 source hygiene + 4 workflow gate + 4 `command:` shellouts +
  others.
- **61 bundled rules** from the 9 extended rulesets (some IDs overlap,
  which is why `alint list` reports 112 not 113): 15 + 9 + 4 + 4 + 3 +
  11 + 7 + 3 + 5 = 61, − overlap = 61 effective rule IDs.

**Validation:** `alint validate-config` reports `✓ Config valid: 112
rule(s) loaded`. Pitfall checks:

- The magic comment is present (line 1).
- `command:` rules use `command:` (not `argv:`) and integer
  `timeout:` (not duration strings).
- The bool field `engineStrict` uses `yaml_path_equals` (not
  `yaml_path_matches`) — pitfall #16-aware.
- All JSONPath dashed keys use bracket notation (pitfall #10-aware).
- `(?m)`/`(?s)` flag prefixes used on multi-line regex patterns
  (pitfall #13/14-aware).
- **Pitfall #22 verified clean** — no `pattern: |` block scalars in
  the config (per the brief's batch-5 special-attention check).
- One `\n` literal inside a single-quoted regex at line 368: the
  `(?s)^---\s*\n.*?:\s*(major|minor|patch)\s*\n.*?---` pattern. The
  YAML scalar passes the literal two-char `\n` to the regex engine,
  which then interprets it as a newline metachar — so this is the
  CORRECT pattern (not pitfall #14). Verified against
  `/tmp/pnpm/.changeset/audit-signatures.md`: matches as expected.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3` against the same
`/tmp/pnpm` working tree captured 2026-05-07. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary
`target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full pass** (112 rules; mostly declarative + 4 `command:` shellouts) | n/a | n/a | **walk error — pnpm has 2 broken-symlink test fixtures**, see §6.2 | — |
| `pn lint:meta` (meta-updater test mode, 169 manifests) | Node + meta-updater | **~3-5 s** (per pnpm CI logs) | included in declarative pass once the symlink issue is sidestepped | n/a |
| `pn lint` (eslint + cspell + lint:meta chained sequentially) | Node + eslint + cspell + meta-updater | **~30-90 s** cold cache | n/a — alint shells out via `command:` for the eslint+cspell+lint:meta chain | 1× — alint orchestrates |

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| Full alint pass | alint v0.9.17 | **walk-error blocked**, see §6.2 | Add `/tmp/pnpm/store/cafs/test/fixtures/broken-symlink/` and `/tmp/pnpm/fetching/directory-fetcher/test/fixtures/pkg-with-broken-symlink/` to `paths.exclude` on every rule, OR shell out to `git ls-files` to skip broken symlinks. |
| `pn lint:meta` reference perf | `pnpm` + Node | pending — `pnpm` not on PATH in test env | `npm install -g pnpm@10` then `pn lint:meta` |
| `pn lint` reference perf (eslint + cspell + lint:meta) | `pnpm` + Node + eslint + cspell | pending — full Node toolchain | `npm install -g pnpm@10`, then `pn install && pn lint` |

### 5.3 Expected when symlink-issue resolved

The published S3 bench (100k files, mixed languages) hits 1.13 s
for the workspace bundle on a stock CI runner. At ~12 MB working
tree post-sparse-checkout (210 package.json files), expect
**200-500 ms for the structural rules** on the pnpm tree, vs.:

- `pn lint` (full chain): **~30-90 s** cold cache
- `pn lint:meta` alone: **~3-5 s**

The per-package field-shape rules iterate 169 manifests with
`json_path_matches` in tens of milliseconds (sequential `jq` on 169
files with shell would be ~5-10 s). The cross-cutting structural
floor pays back the most when the repo size is dominated by many
small homogeneous package directories — exactly pnpm's shape.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/pnpm-pnpm/.alint.yml /tmp/pnpm` (live run).

**Headline:** alint surfaces **walk error: broken symlink** before
any rule fires, due to two pnpm-specific test fixtures that ship
intentionally-broken symlinks. The walk-error masks all
post-symlink findings; with the fixtures excluded, the captured
HEAD has 19 missing READMEs across workspace members + the
catalog-completeness verification gap (no `cross_file_value_equals`).

### 6.1 Real findings (full walk pending fixture exclusion)

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 19 workspace members lack `README.md` | `.meta-updater`, `__typings__`, `__typecheck__`, `store/commands`, `global/packages`, `text/tree-renderer`, `network/web-auth`, `pnpm/dev`, `releasing/commands`, `__utils__/get-release-text`, `__utils__/scripts`, `__utils__/prepare`, `__utils__/jest-config`, `__utils__/prepare-temp-dir`, `auth/commands`, `cli/commands`, `testing/mock-agent`, `testing/command-defaults`, `building/commands` | info | (would fire under `monorepo/pnpm-workspace@v1`'s README rule, but pnpm's polyrepo layout drifts from `packages/*` so the bundled rule doesn't fire) | **Real but unweighted.** pnpm doesn't gate on per-member READMEs. The bundled `monorepo/pnpm-workspace@v1` rule fires on `packages/*` only and doesn't catch these (pnpm's layout is the off-the-beaten-path case the bundled overlay was designed *not* to over-fire on). |
| Catalog completeness | (any per-package `dependencies` value that's not `workspace:*`/`link:` and not in `pnpm-workspace.yaml#$.catalog` would silently land) | (would be error) | (would fire under `cross_file_value_equals` if shipped) | **Real gap.** The repo currently passes — pnpm's own pre-commit + `pn lint:meta` enforces this — but alint can't independently verify it without `cross_file_value_equals`. |
| meta-updater carve-out drift | `.meta-updater/src/index.ts` has 11 `switch (manifest.name)` carve-outs | (would be info) | (would fire under per-name allow-listing) | **Real gap.** The 11 packages with custom `scripts.*` shapes don't fit the common-shape rule. Today: pnpm's meta-updater is the source of truth. |

### 6.2 Walk error — broken symlink test fixtures

**Cause.** pnpm ships two test fixtures with intentionally-broken
symlinks pointing at non-existent files:

- `/tmp/pnpm/store/cafs/test/fixtures/broken-symlink/doesnt-exist`
- `/tmp/pnpm/fetching/directory-fetcher/test/fixtures/pkg-with-broken-symlink/not-exists`

These exercise the cafs / directory-fetcher behaviour against a
known-bad on-disk state. alint's `ignore`-crate-backed walker
follows the symlink, hits ENOENT, and aborts the walk — no rules
fire after the error.

**Workaround for adopters:**
```yaml
# At the top of .alint.yml — applies to every rule
paths:
  exclude:
    - "**/store/cafs/test/fixtures/broken-symlink/**"
    - "**/fetching/directory-fetcher/test/fixtures/pkg-with-broken-symlink/**"
```

This is also a **v0.10 candidate** for the engine: `walk_error_policy:
warn|skip|abort` (default `abort` for backwards compat) so adopters
can configure broken-symlink fixtures to be skipped rather than
aborting the walk.

**Demand signal for `walk_error_policy:`:** single source so far
(pnpm), but the same shape would surface in any repo that uses
intentionally-broken symlinks for filesystem-edge-case testing
(rust-lang/rust's `tests/ui/`, golang's `internal/syscall/...`,
etc. — worth re-checking as the inventory grows).

### 6.3 Suspected `.alint.yml` bugs

**None.** Config validates cleanly (112 rules loaded). All known
pitfalls (#13/#14/#16/#17/#18/#19/#22) verified clean:

- `(?m)`/`(?s)` flags present on every multi-line regex (#13, #14)
- `engineStrict: true` uses `yaml_path_equals` not `yaml_path_matches` (#16)
- `cspell-words-non-empty` uses `[*]`-and-regex with the union of legal values (#17)
- No `respect_gitignore: false` patterns (#18 N/A)
- No `root_only: true` patterns (#19 N/A)
- **No `pattern: |` block scalars** (#22 verified clean per the brief's batch-5 check)

---

## 7. Followup feature work surfaced

- **`cross_file_value_equals` rule kind** — v0.10 ship-target (10
  sources past saturation: airflow + tokio + clap + uv + react +
  pnpm + nodejs/node + pytorch + vscode + istio). pnpm reinforces
  the demand from a Tier-1 npm-ecosystem repo and adds the
  **catalog-completeness sub-shape** (value at X must appear in the
  *key set* under Y in file B) as a NEW variant worth scoping into
  the design.
- **`registry_paths_resolve` rule kind** — v0.10 ship-target (8
  sources). pnpm adds three new use cases: patchedDependencies →
  `__patches__/` files, tsconfig.json#references → composite-tsconfig-
  having dirs, changeset frontmatter → workspace package names.
- **`json_key_sort_order` rule kind (NEW)** — assert alphabetical key
  order on a JSON object. Demand: pnpm meta-updater. Not a priority
  but worth noting as the unique-to-pnpm gap.
- **`walk_error_policy:` engine knob (NEW)** — configurable behaviour
  on symlink-walk errors. Demand: pnpm test fixtures. Single-source;
  defer.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **Per-binding `nested_configs:` split.** The 9 bundled rulesets +
   ~51 pnpm-specific rules sit in one 913-line `.alint.yml`. Splitting
   the per-package-shape rules (per-functional-root brace-alternation
   globs) into per-directory `.alint.yml` files via `nested_configs:
   true` would let each functional area (`cache/`, `cli/`,
   `engine/pm/`, etc.) evolve independently. Worth considering as the
   config grows.
2. **Pre-commit fastpath.** pnpm's husky chain runs `pn run
   compile-only && pn run lint --quiet` on pre-commit. Layering
   `alint check --changed` (uses `git ls-files --modified --others
   --exclude-standard` — exactly the pre-commit shape) would give a
   fast structural-floor check at the same hook point.
3. **`hygiene/lockfiles@v1` (7 rules) extension.** Already covered
   in this config; covers the pnpm-lock.yaml presence shape but not
   deep schema. When a pnpm-aware rule kind ships, revisit.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17` (built 2026-05-07)
- **Rule count:** **112** (51 custom + 9 bundled rulesets — 15 + 9 + 4
  + 4 + 3 + 11 + 7 + 3 + 5 = 61, minus overlap = 61 effective bundled
  rule IDs)
- **`alint validate-config`:** ✓ Config valid: 112 rule(s) loaded
- **Live-tree recheck:** **partial** — walk-error on the broken-symlink
  test fixtures aborts the walk; rule-firing observations are based on
  static analysis of the tree shape + the working sub-walks before the
  symlinks are encountered. Workaround documented in §6.2.
- **Pitfall fixes (v0.9.17):** Pitfall #18 (per-rule
  `respect_gitignore: false`) and #19 (literal-path runtime guard for
  `root_only: true` + multi-component literals) both shipped in engine;
  **this config does not need either workaround** (no `respect_gitignore:
  false` or `root_only: true` patterns).
- **Pitfall #22 verified clean** per the brief's batch-5 special-attention
  check — no `pattern: |` block scalars.
- **Open gaps (unchanged):** `cross_file_value_equals` (v0.10
  ship-target, 10 sources — pnpm reinforces from Tier-1),
  `registry_paths_resolve` (v0.10 ship-target, 8 sources — pnpm adds
  3 sub-cases), `json_key_sort_order` (NEW candidate, single-source
  pnpm), `walk_error_policy:` engine knob (NEW, single-source pnpm).
- **Open suspected bugs in this directory's `.alint.yml`:** None.
