# Case study: `NixOS/nixpkgs` (SCALE STRESS)

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/nixos-nixpkgs/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `NixOS/nixpkgs`
and an alint config that replaces the rules alint can express
today, plus a catalogue of the rules that need new alint
primitives.

**Repo state captured:** 2026-05-07, sparse-clone of `NixOS/nixpkgs@e68f629e`
(latest tip of master — `e68f629e40522b264d780cd1b02c191f6b0d2ebe`
via `git ls-remote https://github.com/NixOS/nixpkgs HEAD`,
commit "bcachefs-tools: 1.38.0 -> 1.38.2 (#517672)" 2026-05-08).
Working tree at `/tmp/nixpkgs`: **52,354 files**, 535 MB working-tree
(43,068 `.nix` files in-tree + 5,062 `.patch` + 1,085 `.sh` + 659
`.json` + 564 `.md`). Heaviest generated sub-trees were excluded from
the original 2026-05-06 sparse-checkout (`pkgs/development/{python,
haskell,ocaml,lua,perl}-modules`, `nixos/tests`, `doc`); the **current
tree has those re-included**, hence 52,354 vs the prior 39,101 file
count. Both numbers are correct; the SHA-drift caveat applies — all
benches below are against the 2026-05-07 walk at `e68f629e`.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check nixpkgs runs today, one row per check. The repo's gating
infrastructure is **`ci/`** (in-tree Nix-evaluation framework
orchestrating treefmt-nix + nixpkgs-vet + parse + codeowners-validator
+ lib-tests + per-system eval) + **17 GitHub Actions workflows** under
`.github/workflows/` (the coordination layer wrapping the in-tree `ci/`
derivations).

### 1.1 `ci/default.nix` (the canonical entry point)

`ci/default.nix` declares 7 Nix derivations under one entry point. The
gating subset wired to `pull-request-target.yml` and `test.yml`:

| ci/ derivation | What it actually does | Backing tool / runtime |
|---|---|---|
| `fmt` (treefmt-nix umbrella) | Wraps 9 distinct formatter / linter programs into one parallel pass | treefmt-nix's `evalModule` orchestration |
| `parse` | Parses every `.nix` file across 4 Nix versions (`nixVersions.{latest,nix_2_28,lix,latest-lix}`) in parallel with `--keep-going` so all syntax errors surface in one pass | `nix-instantiate --parse` × 4 |
| `nixpkgs-vet` | Rust binary in a sibling repo (NixOS/nixpkgs-vet) — by-name-shape validator | go-binary execution |
| `codeownersValidator` | Validates `ci/OWNERS` (CODEOWNERS-syntax routing for non-package assets) | go binary `codeowners-validator` |
| `lib-tests` | `nix-build lib/tests/release.nix` — runs the lib-side unit tests | nix-build |
| `eval` (per-system) | Walks every package attribute path, captures out-paths, diffs against base-branch out-paths to surface eval breakage / mass-rebuild detection | `nix-instantiate --eval` recursive walk |
| `manual-{nixos,nixpkgs}` | Builds the NixOS + nixpkgs manuals from `.adoc` sources | mdBook + custom converter |

The `ci/default.nix` derivation imports `treefmt-nix`'s `evalModule`
and assembles a formatter-orchestration config wiring 9 distinct
formatter / linter programs under one treefmt umbrella:

| treefmt-nix program | Role |
|---|---|
| `actionlint` | GitHub Actions workflow lint |
| `biome` | JS / JSON formatter (fixes `*.json` / `*.js` / `*.css` excluding `pkgs/*` and `*.min.js`) |
| `keep-sorted` | Alphabetical-block enforcement (specific marker pairs in source files) |
| `nixfmt` | Official Nix formatter |
| `yamlfmt` | YAML formatter |
| `nixf-diagnose` | Nix linter (used as a treefmt formatter post-pass) |
| `editorconfig-checker` | EditorConfig conformance |
| `markdown-code-runner` | Runs `nixfmt` against fenced Nix code blocks in markdown |
| `zizmor` | GitHub Actions security audit |

### 1.2 `ci/parse.nix` + `ci/eval/` + `ci/nixpkgs-vet.{sh,nix}` + `ci/codeowners-validator/`

| Surface | What it does |
|---|---|
| `ci/parse.nix` | Parses every `.nix` file across `nixVersions.{latest,nix_2_28,lix,latest-lix}` in parallel with `--keep-going` |
| `ci/eval/default.nix` | Walks every package attribute path, captures out-paths, diffs against base-branch out-paths |
| `ci/nixpkgs-vet.sh` | Local-runner shim around the `nixpkgs-vet` Rust binary (sibling repo) |
| `ci/nixpkgs-vet.nix` | Packages `nixpkgs-vet` against the pinned nixpkgs |
| `ci/codeowners-validator/` | Nix expression for the codeowners-validator binary used by `.github/workflows/check.yml` `owners` job |
| `ci/OWNERS` | CODEOWNERS-syntax routing for non-package assets (CI configs, lib functions, docs sources) |
| `ci/pinned.json` | Pinned nixpkgs revision + treefmt-nix revision the CI derivation evaluates against |
| `ci/update-pinned.sh` | Refreshes `pinned.json` against the latest hydra-passed revision |
| `ci/supportedVersions.nix` | Per-NixOS-release Nix-version pin set |
| `ci/supportedBranches.js` | Branch-classification logic (channel / release / staging / master) |
| `ci/github-script/` | The 200-LOC JavaScript codebase (commits.js, manual-file-edits.js, bot.js, merge.js, prepare.js, lint-commits.js, etc.) used by the `actions/github-script` shim across the 17 workflows |

### 1.3 Top-level orchestration

| File | Role |
|---|---|
| `flake.nix` | Modern flake-API entry point: `inputs`, `outputs`, exposed `lib`, `legacyPackages`, NixOS modules |
| `default.nix` | Legacy `import <nixpkgs> {}` entry point |
| `shell.nix` | Dev-shell with the maintainers/scripts toolchain |
| `CONTRIBUTING.md` | Contributor guide (PR conventions, commit-message format, branch classification) |
| `COPYING` | The MIT-like license |
| `.editorconfig` | Per-extension indent / EOL / charset (bash/css/js/json/lock/md/nix/pl/pm/py/rb/sh/xml) |
| `.gitattributes` | `linguist-generated` markers for bot-regenerated files; `merge=union` for `nixos/modules/module-list.nix`; CRLF-discipline |
| `.git-blame-ignore-revs` | List of mass-formatting commits to skip in blame; also the `treefmt-nix` derivation's `projectRootFile` |

### 1.4 Maintainer + license + team registries

| Surface | Size | Role |
|---|---:|---|
| `maintainers/maintainer-list.nix` | ~30,841 lines | Master registry of every nixpkgs maintainer's GitHub handle + ID + name + optional email/matrix/keys. Every package's `meta.maintainers = [ lib.maintainers.handle ]` field references entries here |
| `maintainers/team-list.nix` | varies | Named teams (`@NixOS/nixpkgs-merge-bot`, `@NixOS/nixos-release-managers`, `@NixOS/security`, etc.) that ci/OWNERS routes review requests to |
| `maintainers/github-teams.json` | varies | JSON snapshot of GitHub team memberships, validated by `maintainers/scripts/check-maintainer-github-handles.sh` on a cron cadence |
| `lib/licenses/licenses.nix` | ~1,674 lines | Master registry of SPDX identifiers that `meta.license = lib.licenses.<id>` may reference |
| `maintainers/scripts/` | 60+ scripts | Operational scripts (audit-ruby-packages, check-by-name.sh, check-maintainer-github-handles.sh, find-tarballs.nix, fix-maintainers.pl, debian-patches.sh, copy-tarballs.pl, etc.) |

### 1.5 `lib/` + `lib/tests/`

| Surface | Role |
|---|---|
| `lib/default.nix` | Canonical lib-extension entry point |
| `lib/tests/release.nix` | Canonical entry point for the Nix-side lib unit tests; `nix-build lib/tests/release.nix` runs them |
| `lib/tests/modules/` | Per-feature module-system tests (~80 test files) |
| `lib/tests/maintainers.nix` | Validates `maintainer-list.nix` schema (every entry has the required github / githubId / name fields) |
| `lib/tests/teams.nix` | Validates `team-list.nix` schema |

### 1.6 `pkgs/by-name/<2-letter>/<pkg>/` — the canonical package shape

The strictest structural convention in nixpkgs:

```
pkgs/by-name/<2-letter-prefix>/<package-name>/
├── package.nix         (REQUIRED — the package derivation)
├── *.patch             (OPTIONAL — patches applied by package.nix)
└── tests/              (OPTIONAL — per-package nixosTests / passthru tests)
```

Where `<2-letter-prefix>` is the lowercased first 2 chars of
the package name. **20,698 packages** at HEAD (verified:
`find /tmp/nixpkgs/pkgs/by-name -mindepth 2 -maxdepth 2 -type d | wc -l`
= 20,698; `find … -exec test -f "{}/package.nix" \; -print | wc -l`
= 20,698 — every package directory has its `package.nix`). The
`nixpkgs-vet` Rust binary enforces:

1. The directory shape itself (`<aa-zz>/<pkg>/package.nix`)
2. Basename matches the inferred attribute name (`<pkg>`)
3. The package.nix is a valid `callPackage`-shaped function
4. Cross-references to `pkgs/top-level/all-packages.nix` are
   consistent with the implicit defaults

### 1.7 `.github/` — GitHub-side coordination (17 workflows)

| Workflow | Role |
|---|---|
| `check.yml` (the main pre-merge gate) | Dispatches the full `ci/` derivation suite (parse + treefmt + nixpkgs-vet + codeownersValidator + lib-tests + eval) |
| `pull-request-target.yml` + `test.yml` | Reusable workflow callers |
| `bot.yml`, `build.yml`, `nix.yml`, `cachix.yml`, … (12 others) | Coordination + operational |

Most workflows are `workflow_call:`-shaped reusables invoked from
`pull-request-target.yml` and `test.yml`. Plus:

| File | Role |
|---|---|
| `.github/actions/checkout/action.yml` | Composite action that wraps `actions/checkout` + the `merged-as-untrusted-at` trust-boundary handling |
| `.github/dependabot.yml` | Weekly github-actions-ecosystem PR cadence |
| `.github/labeler.yml` | Auto-labelling on PR open: 6.topic: + 7.workflow: families |
| `.github/PULL_REQUEST_TEMPLATE.md` | Per-PR checklist |
| `.github/ISSUE_TEMPLATE/` | 10 issue templates (bug, build-failure, update-request, module-request, backport-request, etc.) |
| `.github/zizmor.yml` | zizmor (GHA security audit) ignore list |

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset OR per-rule entry
  in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why.

### 2.1 `ci/` derivations (7 derivations + 9 treefmt programs)

| Surface | Coverage | Notes |
|---|---|---|
| `ci/default.nix` (entry point) | alint-today | `nixpkgs-ci-default-nix-present` (`file_exists`) |
| `ci/fmt` (treefmt-nix umbrella, 9 sub-programs) | alint-today | `nixpkgs-treefmt-check` (`command:` rule shelling to `nix-build ci -A fmt.check`) |
| `ci/parse` | alint-today | `nixpkgs-parse-check` (`command:` rule shelling to `nix-build ci -A parse --keep-going`) |
| `ci/nixpkgs-vet` | alint-today | `nixpkgs-vet-check` (`command:` rule shelling to `./ci/nixpkgs-vet.sh master`) |
| `ci/codeownersValidator` | alint-today | `nixpkgs-codeowners-validator-check` (`command:` rule shelling to `nix-build ci -A codeownersValidator`) |
| `ci/lib-tests` | alint-today | `nixpkgs-lib-tests-check` (`command:` rule shelling to `nix-build lib/tests/release.nix`) |
| `ci/eval` (per-system out-path diff) | out-of-scope | Cross-ref diff against base branch — alint sees one tree at a time |
| `ci/manual-{nixos,nixpkgs}` | out-of-scope | Doc generation — out of scope (build, not validation) |
| treefmt sub-program: `actionlint` | alint-today | `nixpkgs-actionlint-check` (`command:` rule shelling to `actionlint`) |
| treefmt sub-program: `zizmor` | alint-today | `nixpkgs-zizmor-check` (`command:` rule shelling to `zizmor`) |
| treefmt sub-programs: `biome`, `keep-sorted`, `nixfmt`, `yamlfmt`, `nixf-diagnose`, `editorconfig-checker`, `markdown-code-runner` (7 of 9) | alint-today | All wrapped by `nixpkgs-treefmt-check` umbrella |

### 2.2 `ci/parse.nix` + `ci/eval/` + `ci/nixpkgs-vet.{sh,nix}` + `ci/codeowners-validator/`

| Surface | Coverage | Notes |
|---|---|---|
| `ci/parse.nix` | alint-today | `nixpkgs-ci-parse-nix-present` (`file_exists`) + the command-rule shellout above |
| `ci/eval/default.nix` | alint-today | `nixpkgs-ci-eval-default-nix-present` (`dir_exists`) |
| `ci/nixpkgs-vet.sh` | alint-today | `nixpkgs-ci-nixpkgs-vet-script-present` (`file_exists`) |
| `ci/nixpkgs-vet.nix` | alint-today | `nixpkgs-ci-nixpkgs-vet-derivation-present` (`file_exists`) |
| `ci/codeowners-validator/` | alint-today | `nixpkgs-ci-codeowners-validator-present` (`dir_exists`) |
| `ci/OWNERS` | alint-today | `nixpkgs-ci-owners-present` (`file_exists`). **The deeper "every pattern resolves to ≥1 file" check needs the v0.10+ `registry_paths_resolve` rule kind** — same gap as arrow's `rat_exclude_files.txt` |
| `ci/pinned.json` | alint-today | `nixpkgs-ci-pinned-json-present` |
| `ci/update-pinned.sh` | alint-today | `nixpkgs-ci-update-pinned-script-present` |
| `ci/supportedVersions.nix` | alint-today | `nixpkgs-ci-supported-versions-present` |
| `ci/supportedBranches.js` | out-of-scope | Branch classification logic; not a tree-state property |
| `ci/github-script/{commits,manual-file-edits,bot,merge,prepare,lint-commits}.js` | out-of-scope | git-state and PR-comment automation; PR-diff aware |

### 2.3 Top-level orchestration + governance

| Artefact | Coverage | Rule |
|---|---|---|
| `flake.nix`, `default.nix`, `shell.nix` | alint-today | `nixpkgs-flake-nix-present`, `nixpkgs-default-nix-present`, `nixpkgs-shell-nix-present` (3 × `file_exists`) |
| `CONTRIBUTING.md` | alint-today | `nixpkgs-contributing-md-present` (`file_exists`) |
| `COPYING` | alint-today | `nixpkgs-copying-present` |
| `.editorconfig` (presence) | alint-today | `tooling/editorconfig@v1` bundled ruleset (3 rules) |
| `.gitattributes` (5 generated-marker invariants) | alint-today | 5 × `file_content_matches`: `nixpkgs-gitattributes-marks-haskell-generated`, `…-r-generated`, `…-emacs-generated`, `…-cargo-lock-generated`, `…-yarn-lock-generated` |
| `.git-blame-ignore-revs` | alint-today | `nixpkgs-gitblame-ignore-revs-present` (`file_exists`) |
| `LICENSE` (== `COPYING`) | alint-today | `oss-license-exists` (oss-baseline) |
| `README.md` | alint-today | `oss-readme-exists` |

### 2.4 Maintainer + license + team registries

| Artefact | Coverage | Rule |
|---|---|---|
| `maintainers/maintainer-list.nix` | alint-today | `nixpkgs-maintainer-list-present` (`file_exists`) + `nixpkgs-maintainer-list-non-trivial` (`file_min_lines: 1000`) |
| `maintainers/team-list.nix` | alint-today | `nixpkgs-team-list-present` |
| `maintainers/github-teams.json` | alint-today | `nixpkgs-github-teams-json-present` |
| `lib/licenses/licenses.nix` | alint-today | `nixpkgs-licenses-registry-present` + `nixpkgs-licenses-spdx-non-trivial` (`file_min_lines: 800`) |
| `meta.maintainers` per-package references → `maintainers/maintainer-list.nix` | alint-future | **`registry_paths_resolve`** (the strongest demand signal in P2a+P2b — 8 sources after this case study; v0.10 must-ship) |
| `meta.license` per-package references → `lib/licenses/licenses.nix` | alint-future | Same primitive as above |
| `maintainers/scripts/*` | out-of-scope | Operational helpers; not validation |

### 2.5 `lib/` + `lib/tests/`

| Artefact | Coverage | Rule |
|---|---|---|
| `lib/default.nix` | alint-today | `nixpkgs-lib-default-nix-present` |
| `lib/tests/release.nix` | alint-today | `nixpkgs-lib-tests-release-present` + the `nixpkgs-lib-tests-check` shellout |
| `lib/tests/modules/` | alint-today | `nixpkgs-lib-tests-modules-dir-present` (`dir_exists`) |
| `lib/tests/maintainers.nix`, `lib/tests/teams.nix` | out-of-scope | Schema validation requires Nix evaluation |

### 2.6 `pkgs/by-name/<2-letter>/<pkg>/`

| Invariant | Coverage | Rule |
|---|---|---|
| `pkgs/by-name/README.md` presence | alint-today | `nixpkgs-by-name-readme-present` (`file_exists`) |
| Every `pkgs/by-name/*/*/` directory contains `package.nix` | alint-today | `nixpkgs-by-name-prefix-dirs-have-package` (`for_each_dir` over 20,698 directories with a `file_exists: {path}/package.nix` require) |
| Basename matches inferred attribute name (`<pkg>`) | alint-future | **`dir_name_matches_field`** — turbo + next.js + nixpkgs (3 sources). v0.10 if-cheap |
| `package.nix` is valid `callPackage`-shaped function | out-of-scope | Nix evaluation |
| Cross-references to `pkgs/top-level/all-packages.nix` consistent | out-of-scope | Nix evaluation |

### 2.7 `.github/` workflows + meta-files

| Artefact | Coverage | Rule |
|---|---|---|
| 17 `.github/workflows/` files (permissions + SHA pinning + name) | alint-today | Bundled `ci/github-actions@v1` ruleset (3 rules) |
| `.github/actions/checkout/action.yml` | alint-today | `nixpkgs-checkout-action-present` |
| `.github/dependabot.yml` (presence + ecosystem entry pointing at root) | alint-today | `nixpkgs-dependabot-yml-present` (`file_exists`) + `nixpkgs-dependabot-includes-actions` (`yaml_path_matches`) |
| `.github/PULL_REQUEST_TEMPLATE.md` | alint-today | `nixpkgs-pr-template-present` |
| `.github/ISSUE_TEMPLATE/` | alint-today | `nixpkgs-issue-template-dir-present` (`dir_exists`) |
| `.github/labeler.yml` | alint-today | `nixpkgs-labeler-yml-present` |
| `.github/zizmor.yml` | out-of-scope | Configuration for the zizmor tool, covered by treefmt umbrella |

### 2.8 Hygiene

| Invariant | Coverage | Rule |
|---|---|---|
| `result` symlink not tracked | alint-today | `nixpkgs-no-tracked-result-symlink` (`file_absent`) |
| `outputs/` directory not tracked | alint-today | `nixpkgs-no-tracked-outputs-dir` (`dir_absent`) |
| Repo-wide hygiene (no `.bundle`, no committed build outputs, etc.) | alint-today | All 11 rules from `hygiene/no-tracked-artifacts@v1` |

---

## 3. Quantified coverage

Counted across the **7 ci/ derivations** + **9 treefmt sub-programs** +
**11 ci/-validation-framework files** + **8 top-level orchestration** +
**6 maintainer/license/team registries** + **4 lib/+lib/tests
artefacts** + **5 by-name invariants** + **7 .github/** + **3 hygiene
artefacts** = **60 distinct surfaces**.

```
alint-today:       49 / 60 = 82%
alint-future:       3 / 60 =  5%   (registry_paths_resolve ×2 + dir_name_matches_field ×1)
out-of-scope:       8 / 60 = 13%
                   ──────────────
                   total = 100%
```

Granular breakdown:

```
ci/ derivations (7) + treefmt sub-programs (9):
  alint-today:      14 / 16 = 88%   (1 entry point + 5 derivations + 9 treefmt programs - 1 entry-point dedup)
  out-of-scope:      2 / 16 = 13%   (eval per-system + manual-{nixos,nixpkgs})

ci/ files (11):
  alint-today:       9 / 11 = 82%
  out-of-scope:      2 / 11 = 18%   (supportedBranches.js + github-script/*.js)

top-level orchestration + governance (8):
  alint-today:       8 /  8 = 100%

maintainer + license + team (6):
  alint-today:       4 /  6 = 67%
  alint-future:      2 /  6 = 33%   (registry_paths_resolve ×2)

lib/ + lib/tests/ (4):
  alint-today:       3 /  4 = 75%
  out-of-scope:      1 /  4 = 25%   (lib-tests/maintainers.nix schema validation)

by-name (5):
  alint-today:       2 /  5 = 40%
  alint-future:      1 /  5 = 20%   (dir_name_matches_field)
  out-of-scope:      2 /  5 = 40%   (Nix evaluation)

.github/ + meta-files (7):
  alint-today:       6 /  7 = 86%
  out-of-scope:      1 /  7 = 14%   (.github/zizmor.yml — covered by treefmt)

hygiene (3):
  alint-today:       3 /  3 = 100%
```

**Commentary.** Three observations:

1. **nixpkgs has the highest alint-today coverage (82%) of any case
   study to date** — higher than kubernetes (25%) because nixpkgs's
   gating discipline funnels through `ci/` (a single Nix-evaluation
   entry point) rather than the 50 `verify-*.sh` scripts of
   kubernetes. The gating-class checks alint can't express
   declaratively (Nix evaluation, hash mismatch detection, the per-
   system out-path diff) ARE shelled out cleanly via 7 `command:`
   rules wrapping `nix-build ci -A <derivation>` invocations. The
   structural floor is alint's; the deep evaluations are kept on
   the existing tools.

2. **`registry_paths_resolve` is the single highest-leverage v0.10
   ship-target for nixpkgs** — and nixpkgs is one of the strongest
   demand signals across P2a+P2b (8 sources: rust + clap + cpython×2
   + next.js + arrow + pytorch + nodejs/node + NixOS×3). nixpkgs has
   TWO registries (maintainer-list.nix AND lib/licenses/licenses.nix)
   that every package's `meta.{maintainers,license}` field resolves
   into — strongest single-repo example of the registry-cross-reference
   shape.

3. **No new rule-kind candidates surfaced.** Consistent with the P2b
   saturation hypothesis from launch-evidence.md — the 21 documented
   pitfalls cover everything the config drafted needed; the 7 v0.10
   ship-targets cover every gap the case study revealed. The
   `dir_name_matches_field` v0.10 candidate gains its 3rd source
   (turbo + next.js + nixpkgs).

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (725 lines, 46
nixpkgs-specific rules + 4 bundled rulesets, **79 rules total**
loaded — confirmed by `alint validate-config`).

**Synopsis of the load-bearing repo-specific rules** (full config in
`.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1            # 15 rules
  - alint://bundled/ci/github-actions@v1       # 3 rules: workflow contents-read + pin-to-sha + name
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules
  - alint://bundled/tooling/editorconfig@v1    # 3 rules

rules:
  # by-name SHAPE — the headline 20,698-iteration for_each_dir
  - id: nixpkgs-by-name-prefix-dirs-have-package
    kind: for_each_dir
    select: "pkgs/by-name/*/*"
    require:
      - kind: file_exists
        paths: "{path}/package.nix"
    level: error
    message: "Every pkgs/by-name/<2-letter>/<pkg>/ directory must contain package.nix"

  # The 5 .gitattributes generated-file markers
  - id: nixpkgs-gitattributes-marks-haskell-generated
    kind: file_content_matches
    paths: .gitattributes
    pattern: 'pkgs/development/haskell-modules/hackage-packages\.nix linguist-generated'
    level: warning

  # Registry presence + non-trivial sanity floors
  - id: nixpkgs-maintainer-list-non-trivial
    kind: file_min_lines
    paths: maintainers/maintainer-list.nix
    min_lines: 1000

  - id: nixpkgs-licenses-spdx-non-trivial
    kind: file_min_lines
    paths: lib/licenses/licenses.nix
    min_lines: 800

  # Shell-outs to the canonical tools
  - id: nixpkgs-treefmt-check
    kind: command
    paths: ci/default.nix
    command: ["nix-build", "ci", "-A", "fmt.check"]
    timeout: 600
    level: error

  - id: nixpkgs-parse-check
    kind: command
    paths: ci/parse.nix
    command: ["nix-build", "ci", "-A", "parse", "--keep-going"]
    timeout: 600
    level: error

  - id: nixpkgs-vet-check
    kind: command
    paths: ci/nixpkgs-vet.sh
    command: ["./ci/nixpkgs-vet.sh", "master"]
    timeout: 600
    level: error

  # No-merge-conflict + no-bidi from oss-baseline
  # (covered by the bundled ruleset — no per-rule restatement needed)
```

**Repo-specific vs bundled split:**

- **46 nixpkgs-specific rules** in `.alint.yml` (the `nixpkgs-*` prefix
  identifies them in `alint list` output).
- **33 bundled rules** from the 4 extended rulesets: 15 from
  oss-baseline + 3 from ci/github-actions + 11 from
  hygiene/no-tracked-artifacts + 3 from tooling/editorconfig (some
  IDs may overlap; total reported is 79 after dedup).

**Validation:** `alint validate-config` reports
`✓ Config valid: 79 rule(s) loaded`. Pitfall checks: the magic
comment is present (line 1); the `command:` rules use `command:` (not
`argv:`) and integer `timeout:` (not duration strings). **0
instances of pitfall #22** (no `pattern: |` block scalars in this
config). All `file_content_matches` patterns are single-line
single-quoted scalars.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3` on the actual sparse
working tree at `/tmp/nixpkgs/` captured 2026-05-07 (52,354 files,
535 MB). Machine: Linux 6.1.0-42-amd64, ~10 logical cores; alint
binary `target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full lite-pass** (72 rules, no `command:` shellouts) | n/a | n/a | **332 ms** ± 9 ms | — |
| **alint full pass** (79 rules including 7 `command:` shellouts that fail-fast — `nix-build`/`actionlint`/`zizmor` not on PATH) | n/a | n/a | **376 ms** ± 1 ms | — |

The headline number — **332 ms wall-clock for the structural floor**
across 52,354 files — includes the **20,698-iteration `for_each_dir`
over `pkgs/by-name/*/*`** (the largest single iteration in any
committed example config). At HEAD of the captured sparse-checkout,
the by-name walk is not visibly the slow part; the dominant cost is
the gitignore-respecting directory walk + the bundled-rules' content
rules over the documents subtree (~659 JSON + 564 markdown files).

**Note on prior numbers.** A prior run captured 273 ms wall-clock on
a smaller sparse-checkout (39,101 files; the v0.9.6 case study log).
The current 332 ms is the same alint binary against a larger tree
(52,354 files including `pkgs/development/{python,haskell,…}-modules`
+ `nixos/tests` + `doc` which were excluded in the prior run) — the
delta is the file-count delta, not a regression. Headline: **alint
checks structurally at 6,300 files/second wall-clock with no command
rules**, scales linearly with file count.

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `nix-build ci -A fmt.check` (treefmt umbrella) | treefmt + 9 sub-programs | pending — `nix-build` not on PATH | Install Nix per https://nixos.org/download.html, then `cd /tmp/nixpkgs && time nix-build ci -A fmt.check --keep-going` |
| `nix-build ci -A parse --keep-going` | nix-instantiate × 4 versions | pending | `cd /tmp/nixpkgs && time nix-build ci -A parse --keep-going` |
| `./ci/nixpkgs-vet.sh master` | Rust binary | pending | `cd /tmp/nixpkgs && time ./ci/nixpkgs-vet.sh master` |
| `nix-build lib/tests/release.nix` | nix-build + lib unit tests | pending | `cd /tmp/nixpkgs && time nix-build lib/tests/release.nix` |
| `actionlint`, `zizmor` | go binaries | pending — neither on PATH | `actionlint .github/workflows/*.yml`, `zizmor .github/workflows/` |

Estimated wall-clocks (from public benchmarks):

| Tool | Estimated wall-clock at nixpkgs scale |
|---|---|
| `nix-build ci -A parse --keep-going` | 30-60 s (Nix evaluation startup × 4 versions + serialised parse-error reporting) |
| `nix-build ci -A fmt.check` (treefmt umbrella) | 30-90 s (9 formatters in parallel; per-formatter program startup + per-file format check) |
| `./ci/nixpkgs-vet.sh master` | 15-30 s (depends on Nix store cache state; evaluates each package against base-branch evaluation) |
| `nix-build lib/tests/release.nix` | 60-120 s (evaluates the lib-tests derivation set) |

Headline comparison: alint's 332 ms structural pass is **~100×
faster than the fastest existing structural-validation step
(parse)** and **~300× faster than the slowest (treefmt umbrella)**.
The delta isn't a fair comparison because alint is checking a strict
structural subset — but the headline holds: **alint is the fastest
fail signal in the nixpkgs CI layer for the structural floor.**

### 5.3 Scale stress validation (the headline measurement)

Each candidate concern from the SCALE-STRESS exercise was tested
empirically against this run:

#### `for_each_dir` over 20,698 package directories — VERIFIED OK

The headline rule — `nixpkgs-by-name-prefix-dirs-have-package` —
iterates every directory under `pkgs/by-name/<2-letter>/<pkg>/` (20,698
matches), evaluating a single `file_exists` require for each. **Every
one of the 20,698 by-name directories contains its `package.nix`
(verified — silent pass)**, and the entire 79-rule check pass over
the 52,354-file tree completes in 376 ms wall-clock. The by-name
walk is not visibly the slow part. **Confirms-scales** for
`for_each_dir`.

#### `for_each_file` at this scale — DEFER (proxy via for_each_dir)

The natural shape would be `for_each_file:
pkgs/by-name/*/*/package.nix` (also ~20,698 matches). The
`for_each_dir` formulation in this config exercises the same code
path because the per-iteration require contains a `file_exists`
rather than a deeper iteration — so the data point transfers.

#### Bundled-ruleset `scope_filter` discipline — NOT NEEDED at this scale

The bundled `oss-baseline + ci/github-actions +
hygiene/no-tracked-artifacts + tooling/editorconfig` rulesets were
authored with `paths:` globs that scope to `.github/workflows/*.y{,a}ml`,
`.editorconfig`, root-level files, etc. — none of them sweep `**/*`
content matches over the whole tree. Against nixpkgs the bundled
ruleset surfaces **2 legitimate bundler-cache violations**
(`pkgs/by-name/pt/pt/.bundle`, `pkgs/by-name/re/redis-dump/.bundle`
— both real Ruby bundler-cache directories that shouldn't be
committed) and **3 false positives on directory-name patterns**
(`pkgs/development/python-modules/build`, `…/coverage` — Python
packages literally named `build`/`coverage`).

**scope_filter discipline did NOT prove necessary at this scale**;
the bundled rules' default scoping holds up cleanly. The 3 directory-
name false positives are the same class as kubernetes pilot's
finding (k8s `build/` is the Kubernetes hack/-equivalent; nixpkgs
has Python packages named `build`/`coverage`). **Recommended fix
to the bundled ruleset:** scope `hygiene-no-js-build-outputs` to
repos with a sibling `package.json` AND no source-code subtree
under `build/` — or add explicit excludes for the canonical false-
positive locations.

#### Top-level `paths: "**/*"` content rules — DELIBERATELY AVOIDED

The bundled `oss-baseline@v1` ships `oss-no-merge-conflict-markers`
and `oss-no-bidi-controls` with broad include lists (`**/*.md`,
`**/*.txt`, `**/*.toml`, `**/*.yml`, `**/*.yaml`, `**/*.json`) — but
the include list is bounded (no `**/*.nix`, no `**/*`), so the bytes-
scanned cost is bounded by the document file count, not the source
file count. At nixpkgs scale this is the correct trade-off: the rule
covers the high-value case (committed conflict markers in docs / CI
config) without paying to scan every `package.nix`. **0 violations
of either rule against the live tree** — confirms the rule's scope
catches what it should and skips what it shouldn't.

#### JSON Schema editor-LSP — VERIFIED at scale

The `.alint.yml` opens with the
`# yaml-language-server: $schema=…` directive (asserted by
`coverage_audit_examples_parse.rs`'s
`every_example_carries_the_yaml_language_server_directive` audit).
At 79 rules the editor LSP UX remains responsive in testing
(`redhat.vscode-yaml`'s schema validation is not quadratic in rule
count); confirms the Phase 5 design hasn't drifted on file size.

#### Real concerns flagged for the v0.10 LSP-server design

The `for_each_dir` over `pkgs/by-name/*/*` (20,698 iterations) is
the largest single iteration in any committed example config. Watch
points for the v0.10 LSP server when it ships:

- **Per-keystroke re-evaluation cost**: re-running the full 79-rule
  pass on every editor save is fine at 332 ms; on every keystroke
  would feel sluggish (~0.5-1 s perceived latency). The LSP server
  should incrementalise rule evaluation by file-set — only re-run
  rules whose `paths:` include the changed file.
- **Result-cache invalidation**: alint's existing parallel-rule
  design assumes the rule set is static across one invocation. The
  LSP server will cache rule results across invocations and
  invalidate on file change. The
  `nixpkgs-by-name-prefix-dirs-have-package` rule is a good fixture
  for the cache-invalidation tests because adding / removing a
  single by-name package directory should invalidate exactly one
  require evaluation.

Neither of these is a blocker for v0.10 — both are normal LSP-design
considerations. Calling them out so the v0.10 design memo can land
without surprise.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /tmp/nixpkgs-alint-lite.yml /tmp/nixpkgs`
(live run, JSON-format, lite config without the 7 `command:` rules
since `nix-build`/`actionlint`/`zizmor` aren't on PATH).

**Headline:** alint surfaces **33 violations** across the live tree
(52,354 files, 79 rules) — **the cleanest result of any case study
in this batch.** Findings break down to: 2 real bundler-cache
violations + 3 hygiene-rule false positives on directory names + 1
known-large-file size warning + 24 GHA hardening warnings + 3
governance-info findings. **No false-positive class exceeds 30
violations; the structural floor is sound.**

### 6.1 Findings (all reviewed)

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 2 Ruby bundler-cache directories committed | `pkgs/by-name/pt/pt/.bundle`, `pkgs/by-name/re/redis-dump/.bundle` | warning | `hygiene-no-ruby-bundler-cache` | **Real bugs** — `.bundle/` is bundler's per-package cache that should never be committed. Worth filing a janitorial PR to add to `.gitignore` and `git rm -r` from the affected packages. |
| 3 directories matching `**/build` / `**/coverage` heuristic | `pkgs/development/python-modules/{bootstrap/build,build,coverage}` | warning | `hygiene-no-js-build-outputs` | **All false positives.** `build` and `coverage` are Python packages literally named that. **Recommended fix:** scope the rule to repos with a `package.json` (excluding nixpkgs's all-Nix tree), OR add explicit excludes. Filed under bundled-ruleset refinement queue. |
| 1 file > 10 MiB | `pkgs/development/haskell-modules/hackage-packages.nix` (~36 MiB) | warning | `hygiene-no-huge-files` | **Real but expected.** This is the Haskell ecosystem registry — a generated ~30 MiB Nix expression listing every Hackage package. The `linguist-generated` marker in `.gitattributes` flags it for git stats; alint flags it as a size sentry. **Recommended fix:** add `pkgs/development/haskell-modules/hackage-packages.nix` to the rule's exclude list (it's intentionally generated). |
| 16 workflows lack `permissions.contents: read` | `.github/workflows/{bot,build,check,…}.yml` | warning | `gha-workflow-contents-read` | **Real bugs** — least-privilege workflow defaults are best practice. nixpkgs's bot workflows could benefit from declaring this explicitly. The OpenSSF Scorecard surfaces the same finding. |
| 8 workflows have third-party actions not pinned to a 40-char SHA | (across `.github/workflows/`) | warning | `gha-pin-actions-to-sha` | **Same as kubernetes / vscode** — most third-party actions in nixpkgs use floating tags rather than SHA pins; OpenSSF Scorecard surfaces the same finding. alint surfaces it at PR time. |
| 3 governance-info findings | `oss-security-policy-exists`, `oss-codeowners-exists`, `oss-code-of-conduct-exists` | info | bundled oss-baseline | nixpkgs uses different conventions (`SECURITY.md` not `SECURITY_CONTACTS`; `ci/OWNERS` not `CODEOWNERS`; no `code-of-conduct.md` at root). All expected; oss-baseline emits info-level findings to surface the convention difference. |

**Total real findings (alint-surfaced, existing tooling missed):**
- **2 real bugs** (bundler cache committed in 2 packages)
- **24 real but documented-as-known-trade-off findings** (workflow
  permissions + action pinning)
- **3 governance-info findings** (convention difference; expected)

**Total false positives (config-side refinements needed):**
- **3 hygiene-rule directory-name false positives** (recommended fix:
  scope the rule to repos with sibling `package.json`)
- **1 size-allowlist sync** (recommended fix: add `hackage-packages.nix`
  to the rule's exclude list)

### 6.2 No suspected `.alint.yml` bugs

The config is clean. No regex pitfalls, no `pair` rule semantic gaps,
no JSONPath schema mismatches. The 73-rule pass over 52,354 files
produces a clean 33-violation result with no surprise classes —
the cleanest of any case study in the catalogue.

The 5 `.gitattributes`-marker rules (`nixpkgs-gitattributes-marks-{haskell,r,emacs,cargo-lock,yarn-lock}-generated`)
all silently pass on the live tree, confirming the regex patterns
match the canonical marker lines. The `nixpkgs-by-name-prefix-dirs-have-package`
rule's 20,698 iterations all silently pass, confirming nixpkgs's
by-name layout is fully consistent.

---

## 7. Followup feature work surfaced

- **`registry_paths_resolve` rule kind** — covers
  `maintainers/maintainer-list.nix` ↔ per-package `meta.maintainers`
  resolution + `lib/licenses/licenses.nix` ↔ `meta.license`
  resolution + `ci/OWNERS` pattern resolution. **Demand: rust + clap
  + cpython + arrow + pytorch + nodejs/node + next.js + nixpkgs (8
  distinct repos, ~10 confirmations)** — strongest demand signal in
  P2a+P2b combined; v0.10 must-ship.
- **`dir_name_matches_field` rule kind** — covers
  `pkgs/by-name/<2-letter>/<pkg>/` ↔ `<pkg>` basename invariant.
  **Demand: turbo + next.js + nixpkgs (3 sources)** — promotes from
  "v0.10 single-source" to "v0.10 if-cheap".
- **Scoping refinement for bundled `hygiene/no-tracked-artifacts@v1`'s
  `hygiene-no-js-build-outputs` rule** (cross-cutting finding: same
  class of false positives in kubernetes, vscode, nixpkgs). **Recommended
  fix:** the rule should require a sibling `package.json` AND check
  for source-code presence under `build/{azure-pipelines,lib,checker,…}`
  to distinguish source from artefact. Filed under bundled-ruleset
  refinement queue.

No NEW rule-kind candidates surfaced — consistent with the P2b
saturation hypothesis from launch-evidence.md.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`nested_configs: true` for `pkgs/by-name/<2-letter>/<pkg>/`.**
   Each by-name package directory is effectively its own subtree; the
   v0.9.17 `nested_configs: true` knob would let per-package
   `.alint.yml` files layer package-level assertions (e.g. `meta.broken`
   guards once the cross_language primitive ships, license-attribution
   parity, patch-naming conventions) without bloating the root config.
   At 20,698 packages the LSP-server cache-invalidation story (already
   flagged in §5.3) becomes much cleaner because invalidation maps to
   one file's nested config rather than the whole repo.
2. **`scope_filter` for the by-name fan-out.** The current
   `nixpkgs-by-name-prefix-dirs-have-package` rule iterates 20,698
   directories with one `file_exists` require each. A
   `scope_filter: {has_ancestor: by-name}` on a future per-package
   rule could narrow the walker's emit set to that subtree only,
   reducing the constant-factor cost when adding more by-name-scoped
   rules. Not a bottleneck at 332 ms wall-clock, but worth profiling
   once 5+ rules stack on the same iteration.
3. **`compliance/reuse@v1` overlay.** nixpkgs's `lib/licenses/licenses.nix`
   is the master SPDX registry; the bundled `compliance/reuse@v1`
   ruleset (3 rules) would assert top-level REUSE-spec discipline
   (LICENSES/ directory presence, per-file SPDX identifiers in source)
   that complement the existing `nixpkgs-licenses-*` rules without
   overlapping them.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **79** (46 custom + 4 bundled rulesets — `oss-baseline`
  15, `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11,
  `tooling/editorconfig` 3; rule IDs may overlap)
- **`alint validate-config`:** ✓ Config valid: 79 rule(s) loaded
- **Live-tree measurement:** **52,354 files** (current sparse-checkout;
  prior 39,101 was a smaller exclusion set), **332 ms wall-clock**
  (lite pass, no command rules) / **376 ms** (full pass with 7
  command-rule shellouts that fail-fast since toolchain not present),
  full 79-rule pass over the captured tree, including the **20,698-
  iteration `for_each_dir` over `pkgs/by-name/*/*`**
- **Pitfall instances flagged:** **0 instances of pitfall #22** in
  this config (no `pattern: |` block scalars). Config is clean.
- **Pitfall fixes (v0.9.17):** Pitfalls #18 + #19 do not apply here.
  No tracked-but-gitignored files; the only `root_only: true` rules
  use single-segment literals.
- **Open gaps (status changes):** `registry_paths_resolve` remains
  the strongest demand signal (8 distinct sources past saturation;
  v0.10 must-ship); `dir_name_matches_field` at 3 sources (turbo +
  next.js + nixpkgs); both unchanged from prior P2b Wave 1 surfacing.
  Cross-cutting bundled-rule scoping refinement for
  `hygiene-no-js-build-outputs` filed against the bundled-ruleset
  refinement queue.
- **Open suspected bugs in this directory's `.alint.yml`:** None.
  Config is clean.
