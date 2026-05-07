# Case study: `helm/helm`

> Marketing writeup (narrative, headline catch, competitive framing)
> lives at <https://alint.org/examples/helm-helm/>. This README is
> the engineering reference: tooling inventory, mapping table, gap
> catalogue, validation status.

Inventory of the structural-validation tooling in `helm/helm` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-checkout via
`git clone --depth=1 --filter=blob:none --sparse`, with
`cmd/helm/testdata` and `internal/test` excluded.

---

## Summary

helm is the canonical **midsize, modular Go OSS monorepo**: ~530
production `.go` files, one root `go.mod` (`helm.sh/helm/v4`),
~150k LoC, a single 240-line Makefile that drives the dev workflow,
one ~120-line `.golangci.yml` enabling 17 linters + 2 formatters,
one ~50-line `scripts/validate-license.sh`, and the standard
GitHub Actions + OWNERS + dependabot CNCF-project shape.

The structural-validation surface is **deliberately small**:
**~22 distinct checks** live across the Makefile (`test-style`,
`test-source-headers`, `tidy`), the `.golangci.yml` formatters
block, `scripts/validate-license.sh`, the GitHub Actions workflows
(`build-test.yml`, `golangci-lint.yml`, `govulncheck.yml`,
`scorecards.yml`, `codeql-analysis.yml`, `release.yml`,
`stale.yaml`), and the on-disk metadata files (`OWNERS`,
`.github/env`, `.github/dependabot.yml`, `.goreleaser.yaml`,
`AGENTS.md`, `ADOPTERS.md`, `KEYS`, `code-of-conduct.md`).

Roughly **70 % map directly to existing alint rules** (license
header, formatters-block shape, `.github/env` version pinning,
OWNERS shape, top-level files, GitHub Actions hardening, hygiene
floor), **~10 % shell out via the `command` rule kind** to existing
tools (`golangci-lint`, `gofmt -l`, `go mod tidy -diff`,
`misspell`, `govulncheck`), and **~20 % are out of alint's scope by
design** (depguard / gomodguard / revive / modernize / sloglint —
the Go-AST-aware checks alint isn't trying to do, mirroring the
kubernetes / golang-go non-goals catalogues).

The 58-rule starter config (24 helm-specific + 34 from 4 bundled
rulesets — `oss-baseline=15` + `go=8` + `ci/github-actions=3` +
`hygiene/no-tracked-artifacts=11`, with a few rules deduplicated)
in [`/.alint.yml`](.alint.yml) replaces
**every structural assertion helm makes about its own tree** that
isn't a Go-AST analysis. Net: one declarative file replaces the
Makefile's `test-style` orchestration plus the
`scripts/validate-license.sh` 50-liner plus the half-dozen
shape-implicit assertions buried inside `.golangci.yml` and the
release pipeline.

---

## Existing tooling inventory

### `Makefile` — the orchestration core

helm's dev workflow is Makefile-driven (CONTRIBUTING.md walks
contributors through `make test`, `make test-style`,
`make test-coverage`). The structural-relevant targets:

| Target | What it does | alint replacement |
|---|---|---|
| `test-style` | Runs `golangci-lint run ./...` then `scripts/validate-license.sh` | `helm-golangci-lint` (for_each_dir over `go.mod` + command) + `helm-go-license-header` + `helm-shell-license-header` |
| `test-source-headers` | Calls `scripts/validate-license.sh` directly | Same — `helm-go-license-header` + `helm-shell-license-header` |
| `format` | `goimports -w -local helm.sh/helm` (write mode) | Read-only sibling `helm-gofmt-clean` (`gofmt -l`) |
| `tidy` | `go mod tidy` | `helm-go-mod-tidy` (`go mod tidy -diff` to assert no drift) |
| `gen-test-golden` | Regenerates testdata golden files | **Out of scope** — needs a `command_idempotent` / generator-diff primitive (v0.10+ candidate) |
| `build`, `install`, `dist`, `checksum`, `sign`, `release-notes` | Build / packaging | **Out of scope** — not structural |

### `scripts/validate-license.sh`

A 45-line bash script that `find`-walks `*.go` and `*.sh` files
(excluding `*testdata*` and `*third_party*`) and `grep -L`s for
two literal strings:

- `Licensed under the Apache License, Version 2.0 (the "License")`
- `Copyright The Helm Authors.`

This is a textbook `file_header` rule. The wrinkle is that
**three** comment-block shapes coexist in the helm tree:

```
/*                                       /*                            // Copyright The Helm Authors.
Copyright The Helm Authors.              Copyright The Helm Authors.   // Licensed under the Apache License, Version 2.0 ...
                                         Licensed under ...
Licensed under the Apache License ...
```

Plus a fourth: `internal/sympath/walk.go` carries a
Go-Authors-attribution preamble before the helm header. The bash
`grep -L` accepts all four because it tests each literal
independently. The alint replacement uses a `(?s)` regex with a
`.{0,400}?` non-greedy gap so the rule matches every variant the
script accepts.

### `.golangci.yml`

This is **the** file that drives linting in helm. 120 lines, 17
linters enabled (`depguard`, `dupl`, `exhaustive`, `gomodguard`,
`govet`, `ineffassign`, `misspell`, `modernize`, `nakedret`,
`nolintlint`, `perfsprint`, `revive`, `sloglint`, `staticcheck`,
`testifylint`, `thelper`, `unused`, `usestdlibvars`,
`usetesting`), 2 formatters (`gofmt`, `goimports`).

**alint's coverage of `.golangci.yml`** is the **shape, not the
semantics**: alint asserts the `formatters.enable` array contains
`gofmt` and `goimports`, and that
`formatters.settings.goimports.local-prefixes[0]` is pinned to
`helm.sh/helm/v4`. The actual lint runs stay with golangci-lint
itself, invoked via `helm-golangci-lint` (a `for_each_dir` over
`go.mod` that `command:`s out to `golangci-lint run ./...`).

### GitHub Actions — 7 workflows

| Workflow | Purpose | alint disposition |
|---|---|---|
| `build-test.yml` | Build + unit-test + license-header check + `go mod tidy -diff` | Structure (permissions, action SHA pin, name) covered by `ci/github-actions@v1`; license & tidy mirrored as standalone alint rules |
| `golangci-lint.yml` | `golangci-lint` GHA action | Structure covered; lint via `helm-golangci-lint` shellout |
| `govulncheck.yml` | Daily cron + on-`go.sum`-change vuln scan | Structure covered; mirrored as `helm-govulncheck` for local execution |
| `codeql-analysis.yml` | GitHub CodeQL on Go | Structure only — CodeQL is GitHub-managed |
| `scorecards.yml` | OpenSSF Scorecard | Structure only |
| `release.yml` | goreleaser-driven binary build + Azure blob upload | Structure only — release semantics out of scope |
| `stale.yaml` | Auto-close stale issues/PRs | Structure only |

The bundled `ci/github-actions@v1` ruleset provides the hardening
floor (3 rules: workflow-level `permissions.contents: read`,
`uses:` SHA pinning, `name:` presence) — applied uniformly across
all 7 workflows in one pass.

### `.github/env`

Two environment variables that gate the toolchain version:

```
GOLANG_VERSION=1.26
GOLANGCI_LINT_VERSION=v2.11.3
```

Every workflow sources this file via
`cat ".github/env" >> "$GITHUB_ENV"`. The `Makefile` also reads
`GOLANGCI_LINT_VERSION` directly to warn when local
`golangci-lint --version` doesn't match. **Two sources of truth
for the Go version** (this file plus `go.mod`'s `go` directive)
that drift apart silently — alint's `helm-github-env-pins-go-version`
rule asserts the file's shape, and a future
`cross_file_value_equals` primitive would close the
cross-reference loop.

### `OWNERS` (CNCF / Kubernetes convention)

Top-level YAML file with `maintainers`, `triage`, `emeritus`
lists. The kubernetes publishing-bot + GitHub OWNERS-action both
parse this. helm doesn't carry per-subdir OWNERS files (unlike
kubernetes/kubernetes), so the inventory is the one root file.

### `.goreleaser.yaml`

122 lines. Single-source-of-truth for cross-platform binary builds
(8 GOOS × 8 GOARCH matrix with explicit `ignore:` exclusions).
alint asserts two structural invariants: `CGO_ENABLED=0`
(static-binary contract) and `dist: _dist` (.gitignore
coordination).

### Top-level files (helm-specific conventions)

`AGENTS.md` (codebase tour for AI agents), `ADOPTERS.md` (user
list), `KEYS` (GPG keyring for signed-release verification),
`code-of-conduct.md` (redirects to upstream Kubernetes/CNCF CoC).
Each declared as a `file_exists` rule with `info` severity — these
are conventions, not blockers.

---

## Maps to existing alint rules (what the starter config covers)

58 rules total in [`/.alint.yml`](.alint.yml) (24 helm-specific +
34 from bundled rulesets), broken down:

- **4 bundled rulesets** (`oss-baseline`, `go`, `ci/github-actions`,
  `hygiene/no-tracked-artifacts`) — pull in 34 rules between
  them (`oss-baseline=15` + `go=8` + `ci/github-actions=3` +
  `hygiene/no-tracked-artifacts=11` = 37 raw, deduplicated to 34),
  including the trojan-source / zero-width / final-newline /
  trailing-whitespace floor and the workflow-permissions / SHA-pin /
  name hardening
- **2 license-header rules** (`helm-go-license-header`,
  `helm-shell-license-header`) — exact replacement for
  `scripts/validate-license.sh`, with regex tolerance for the four
  comment-block shapes that coexist in the tree
- **3 `.golangci.yml` shape assertions** — formatters.enable
  contains gofmt + goimports, local-prefixes pinned to
  `helm.sh/helm/v4`
- **5 `command` shellouts** — `golangci-lint run ./...`,
  `gofmt -l`, `go mod tidy -diff`, `govulncheck ./...`,
  `misspell -error`
- **2 `.github/env` pinning rules** — `GOLANG_VERSION` and
  `GOLANGCI_LINT_VERSION` formats
- **2 OWNERS rules** — top-level OWNERS exists +
  `maintainers[0]` is a valid GitHub-username string
- **1 cross-package-test convention** — every `pkg/*/` package has
  at least one `*_test.go` (a soft floor; helm is well above 90%
  on this already)
- **2 `.goreleaser.yaml` invariants** — `CGO_ENABLED=0` +
  `dist: _dist`
- **4 top-level-file presence rules** — `AGENTS.md`,
  `ADOPTERS.md`, `KEYS`, code-of-conduct CNCF redirect
- **1 helm-specific hygiene rule** — `_dist/` and
  `_dist_versions/` must not be tracked (extends the bundled
  no-tracked-artifacts floor)

---

## Real findings against the live tree (2026-05-06 snapshot)

Running the config against the cloned helm tree (with
`misspell`/`golangci-lint`/`gofmt`/`go`/`govulncheck` not on
PATH in the validation env, so the `command:` rules surface as
"could not spawn" warnings — expected) surfaces **two genuine
structural-hygiene findings** the existing tooling misses:

1. **Zero-width character (U+200B) in `internal/plugin/plugin.go:80`** —
   line 80 column 70 contains zero-width spaces inside a comment
   block. Caught by the bundled `go-sources-no-zero-width` rule
   (Trojan-Source CVE-2021-42574 defence). validate-license.sh
   doesn't look at character-class hygiene; golangci-lint doesn't
   either by default. **Net-new structural finding alint catches
   that no existing tool in helm's pipeline does.**

2. **5 GitHub workflows declare `permissions: read-all` or
   `permissions: {}` instead of `contents: read`** —
   `codeql-analysis.yml`, `govulncheck.yml`, `release.yml`,
   `scorecards.yml`, `stale.yaml`. The OpenSSF Scorecard
   Token-Permissions check would surface these as well, but it's
   only run weekly (the scorecards.yml cron). The bundled
   `gha-workflow-contents-read` rule catches them on every PR.

Plus a structural drift the bundled `oss-no-trailing-whitespace`
rule catches: `.golangci.yml` line 43 has trailing whitespace.
Mechanical, but the kind of low-grade-noise finding alint's
`fix:` blocks resolve in one pass.

---

## Needs new alint primitive

helm's structural surface is small enough that only a **handful**
of gaps surface — and most are duplicates of needs already filed
from earlier case studies:

| Need | What it would check | What alint needs |
|---|---|---|
| **`.golangci.yml` `depguard` / `gomodguard` import bans** | "no file under `pkg/**/*.go` may import `github.com/hashicorp/go-multierror` or `github.com/pkg/errors`" + "no file may import `github.com/evanphx/json-patch` (use v5)" | The `import_gate` rule kind — now `v0.10 ship-target` per launch-evidence.md (4 sources: k8s, airflow, golang/go, pytorch). helm surfaces the same depguard shape but is not yet a named source in the launch-evidence.md table. |
| **`gen-test-golden` freshness** | "running `make gen-test-golden` would not change the working tree" | The `command_idempotent` rule kind — `v0.10 design candidate` per launch-evidence.md (2 sources: ruff + prettier). helm is the 3rd surface in the wild. |
| **`.github/env` ↔ `go.mod` go-version cross-reference** | "the `GOLANG_VERSION` value in `.github/env` is the same `<major>.<minor>` as the `go <version>` directive in `go.mod`" | The `cross_file_value_equals` rule kind — now `v0.10 ship-target` per launch-evidence.md (10 sources). helm increments the demand signal. |
| **YAML array set-membership** | "`$.formatters.enable` contains `gofmt` AND `goimports`" — without the per-element `equals:` semantics that flag the *other* elements | A `*_path_contains` set-membership shorthand — narrower than `*_path_equals` (which is "every match equals X") and `*_path_matches` (which is regex on string-typed values only). Now `v0.10 design candidate` per launch-evidence.md (3 sources: helm, deno, bazel). Workaround used in this config: `file_content_matches` against the YAML text. |

The first three are duplicates of needs already filed from earlier
case studies — helm increments their demand signal but doesn't
introduce new rule-kind candidates.

The fourth — `*_path_contains` for set-membership — was new at
helm's original-write time. **Surfaced first by helm**, since
saturated to 3 sources (helm + deno + bazel per
launch-evidence.md) and now `v0.10 design candidate`. Pattern:
pinning the *presence* of a specific value in an array without
making per-element equality assertions about the rest. Common in
YAML config files (`enabled-linters`, `allow-list`, `tags`,
etc.). The `file_content_matches` workaround is robust but
loses JSON/YAML-aware key resolution; the `*_path_contains`
primitive would express the intent cleanly.

---

## Out of alint's scope (use the existing tool)

helm's out-of-scope list is short — much of what golangci-lint
does is already AST-aware Go analysis that alint's non-goals
exclude:

- `depguard` / `gomodguard` import bans — Go-AST aware (per-file
  import-spec analysis). alint can't see imports without parsing
  Go; `import_gate` (v0.10+) would close this gap with a
  declarative manifest.
- `revive` rules (especially `var-naming` with the helm-specific
  initialism overrides) — Go-AST aware
- `staticcheck` — full Go SSA analysis; lives with staticcheck
- `modernize`, `perfsprint`, `usestdlibvars`, `usetesting` — Go-AST
  rewriters; lives with golangci-lint
- `testifylint`, `thelper` — testing-package conventions; AST-aware
- `dupl` — duplicate-code detector; semantic
- `exhaustive` — switch-statement coverage; type-aware
- `unused` / `ineffassign` / `nakedret` — flow-aware
- `sloglint` — Go's structured logging; AST-aware
- `nolintlint` — checks the `//nolint:` directives themselves;
  meta-linter
- `gen-test-golden` — runs unit tests in `-update` mode and asserts
  no diff. Tension with alint's "no codegen" non-goal; addressed
  by the v0.10+ `command_idempotent` candidate.
- The CI matrix dimensions (`linux × 1.26 × {push, pr}`) — policy,
  not structure
- goreleaser's per-arch build matrix — policy, not structure
- The `make sign` GPG-detached-sig pipeline — release semantics

---

## Already covered by other linters helm uses

- `golangci-lint` (orchestrator for 17 linters) — alint shells out
  via `command:` for one-shot orchestration; the deep checks stay
  inside golangci-lint
- `govulncheck` — RUSTSEC-equivalent for Go modules; security
  scanner territory, alint orchestrates via `command:`
- `misspell` — spell-checker; alint orchestrates via `command:` so
  contributors running `alint check` against a docs-only PR get
  feedback even when no Go files are in scope (golangci-lint's
  misspell skips invocation in that case)
- `cosign` / GPG signing of releases — out of scope; not structural

---

## Performance comparison (placeholder — bench when validation pass scales)

helm's existing pipeline structure: `make test-style` runs
`golangci-lint run ./...` (which is the bottleneck — ~30s on a
warm cache, golangci-lint walks every Go file in the module) and
then `scripts/validate-license.sh` (which `find`-walks the tree
twice, once per literal). A typical PR sees `make test-style` take
35-45s wall-clock.

alint's parallel-rule dispatch (v0.9.3+) collapses both the
license-walk and the GHA-workflow shape checks into a single
filesystem walk. Expected: well under 1 second for the alint
subset on the helm tree (compare to the v0.9.13-published S3 100k
bench: 1.13s for the workspace bundle on a 100k-file tree;
helm is ~1k files). The `golangci-lint` shellout via the
`helm-golangci-lint` rule remains the wall-clock bottleneck — but
that's the existing tool's runtime, unchanged.

To benchmark wall-clock for real:
`time { make test-source-headers && bash scripts/validate-license.sh; }`
vs the alint subset that replaces them, against the same checkout.
Deferred to the per-repo measurement pass.

---

## Followup feature work surfaced (de-duplicated against earlier case-study gap lists)

- **`*_path_contains` set-membership shorthand** — `v0.10
  design candidate` per launch-evidence.md (3 sources: helm,
  deno, bazel). Cleanest abstraction over the "yaml_path_equals
  against an array element" pitfall this case study hit
  firsthand.
- **`import_gate` rule kind** — `v0.10 ship-target` (4 sources
  per launch-evidence.md; saturated).
- **`cross_file_value_equals` rule kind** — `v0.10 ship-target`
  (10 sources; strongest demand).
- **`command_idempotent` mode** — `v0.10 design candidate` (2
  sources in launch-evidence.md table; helm is the 3rd surface
  in the wild).

No NEW schema/language pitfalls hit beyond the existing 21
catalogued in `docs/development/CONFIG-AUTHORING.md`. Two pitfalls
were rediscovered firsthand during config authoring:

1. **YAML-array `*_path_equals` semantics** — a
   `yaml_path_equals` against `$.formatters.enable[*]` returns one
   match per array element, and EVERY match must equal the target.
   For `[gofmt, goimports]`, asserting `equals: gofmt` flags the
   `goimports` element as a violation. **Already documented as
   pitfall #17 in the catalogue** (the P2a Wave 3 promotion);
   helm rediscovered it firsthand. Workaround captured in the
   pitfall entry.
2. **License-header regex tolerance for multiple comment styles** —
   the rediscovery confirms pitfall #13 (file-level vs line-level
   anchoring) — `(?s)` + non-greedy `.{0,N}?` between anchor
   strings is the canonical pattern when multiple comment shapes
   coexist. Already documented; no schema gap.

---

## Validation status (2026-05-07)

- alint version: **0.9.17** (1dbd9b218a0e, built 2026-05-07).
- `validate-config`: **58 rules loaded cleanly** (24 helm-
  specific + 34 from 4 bundled rulesets — `oss-baseline=15`,
  `go=8`, `ci/github-actions=3`, `hygiene/no-tracked-artifacts=11`,
  with rule-id deduplication across overlapping rulesets).
- Live-tree recheck: **pending** — `/tmp/helm-helm/` not present
  in this validation env.
- Pitfalls fixed in v0.9.17 that touch this config: none
  (helm config doesn't surface pitfalls #18/#19).
- Open gaps (rule-kind candidates referenced but not yet
  shipped):
  - `*_path_contains` (v0.10 design candidate, 3 sources;
    helm is the first source).
  - `import_gate` (v0.10 ship-target, 4 sources).
  - `cross_file_value_equals` (v0.10 ship-target, 10 sources).
  - `command_idempotent` (v0.10 design candidate; helm is the
    3rd surface in the wild).

## Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **Helm-chart structural invariants for `pkg/chart/testdata/`.**
   helm/helm itself ships zero deployable charts (it's the helm
   CLI source, not a chart consumer), so the `manifests/charts/`
   polyglot pattern doesn't apply. But helm/helm DOES ship
   reference test chart trees under `pkg/chart/testdata/`
   (~80 fixture charts). A `for_each_dir` over those plus
   `helm-chart-yaml-shape` (apiVersion/version/appVersion
   present + valid semver) would gate the test-fixture
   discipline that `helm lint` currently doesn't cover (the
   fixtures are deliberate corner cases, some intentionally
   malformed; the rule would carry a `scope_filter` excluding
   the malformed fixtures).
2. **Add `agent-context@v1` overlay (5 rules).** helm ships
   `AGENTS.md` (line 30, 165) but doesn't enforce its shape.
   The `agent-context@v1` ruleset gates AGENTS.md presence +
   tour-of-codebase content + AI-context-window-friendly
   structure declaratively.
3. **`alint suggest` against the live tree.** Pending
   `/tmp/helm-helm/`. The repo is small enough (~530 .go
   files) that the suggester would terminate quickly; likely
   surface candidates: per-`pkg/*/` test-coverage thresholds,
   `cmd/helm/` subcommand-package conventions, `internal/`
   visibility discipline.
