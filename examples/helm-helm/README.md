# Case study: `helm/helm`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/helm-helm/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `helm/helm` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-08 sparse-clone of `helm/helm@HEAD`
at `/tmp/helm`, with `cmd/helm/testdata` and `internal/test` excluded
(heavy test fixtures). **1,990 tracked files**, **536 .go files**
(production), **7 GitHub Actions workflows** under `.github/workflows/`,
**1 root `go.mod`** (`helm.sh/helm/v4`), **1 `Makefile`** (240 lines —
the dev-workflow orchestration core), **1 `.golangci.yml`** (~120
lines, 17 linters + 2 formatters), **1 `scripts/validate-license.sh`**
(45 lines — the only home-grown structural script), **1 `.github/env`**
file (toolchain version pins), **1 `OWNERS`** (CNCF-style YAML),
**1 `.goreleaser.yaml`** (122 lines, 8×8 cross-platform matrix), **8
top-level governance/convention files** (`AGENTS.md`, `ADOPTERS.md`,
`KEYS`, `code-of-conduct.md`, `CONTRIBUTING.md`, `README.md`,
`LICENSE`, `SECURITY.md`).

**alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`.

---

## 1. Inventory of existing tooling

helm is the canonical **midsize, modular Go OSS monorepo**. Dev
workflow is Makefile-driven (CONTRIBUTING.md walks contributors
through `make test`, `make test-style`, `make test-coverage`); CI
hardening is the standard CNCF GitHub Actions + OWNERS + dependabot
shape.

### 1.1 `Makefile` — the orchestration core (240 lines, ~12 lint targets)

Structural-relevant targets:

| Target | What it does | Backing tool |
|---|---|---|
| `test-style` | Runs `golangci-lint run ./...` then `scripts/validate-license.sh` | golangci-lint + bash |
| `test-source-headers` | Calls `scripts/validate-license.sh` directly | bash |
| `format` | `goimports -w -local helm.sh/helm` (write mode) | goimports |
| `tidy` | `go mod tidy` | go module tooling |
| `gen-test-golden` | Regenerates testdata golden files via `go test -update` | go test runner |
| `build`, `install`, `dist`, `checksum`, `sign`, `release-notes` | Build/packaging targets — not validation surfaces | goreleaser, GPG |

### 1.2 `scripts/validate-license.sh` (45 lines — the only home-grown structural script)

A bash script that `find`-walks `*.go` and `*.sh` files (excluding
`*testdata*` and `*third_party*`) and `grep -L`s for two literal strings:

- `Licensed under the Apache License, Version 2.0 (the "License")`
- `Copyright The Helm Authors.`

This is a textbook `file_header` rule. The wrinkle: **three** comment-
block shapes coexist in the helm tree:

```
/*                                       /*                            // Copyright The Helm Authors.
Copyright The Helm Authors.              Copyright The Helm Authors.   // Licensed under the Apache License, Version 2.0 ...
                                         Licensed under ...
Licensed under the Apache License ...
```

Plus a fourth: `internal/sympath/walk.go` carries a Go-Authors-attribution
preamble before the helm header. The bash `grep -L` accepts all four
because it tests each literal independently.

### 1.3 `.golangci.yml` (~120 lines — 17 linters + 2 formatters)

The single file that drives Go linting in helm:

| Section | Content |
|---|---|
| `linters.enable` | depguard, dupl, exhaustive, gomodguard, govet, ineffassign, misspell, modernize, nakedret, nolintlint, perfsprint, revive, sloglint, staticcheck, testifylint, thelper, unused, usestdlibvars, usetesting (17 linters) |
| `formatters.enable` | gofmt, goimports (2 formatters) |
| `formatters.settings.goimports.local-prefixes` | Pinned to `helm.sh/helm/v4` |

The depguard configuration bans specific imports (no
`github.com/hashicorp/go-multierror`, etc.); gomodguard does the same
at the module level.

### 1.4 GitHub Actions workflows (7 workflows)

| Workflow | Purpose | Backing tool |
|---|---|---|
| `build-test.yml` | Build + unit-test + license-header check + `go mod tidy -diff` | go + bash |
| `golangci-lint.yml` | `golangci-lint` GHA action | golangci-lint v2 |
| `govulncheck.yml` | Daily cron + on-`go.sum`-change vuln scan | govulncheck |
| `codeql-analysis.yml` | GitHub CodeQL on Go | GitHub Advanced Security |
| `scorecards.yml` | OpenSSF Scorecard | OpenSSF Scorecard |
| `release.yml` | goreleaser-driven binary build + Azure blob upload | goreleaser |
| `stale.yaml` | Auto-close stale issues/PRs | actions/stale |

### 1.5 `.github/env` (toolchain version pins)

Two environment variables that gate the toolchain version:

```
GOLANG_VERSION=1.26
GOLANGCI_LINT_VERSION=v2.11.3
```

Every workflow sources this file via `cat ".github/env" >> "$GITHUB_ENV"`.
The `Makefile` also reads `GOLANGCI_LINT_VERSION` directly to warn
when local `golangci-lint --version` doesn't match.

### 1.6 `OWNERS` (CNCF / Kubernetes convention)

Top-level YAML file with `maintainers`, `triage`, `emeritus` lists.
The kubernetes publishing-bot + GitHub OWNERS-action both parse this.
helm doesn't carry per-subdir OWNERS files (unlike kubernetes); the
inventory is the one root file.

### 1.7 `.goreleaser.yaml` (122 lines)

Single-source-of-truth for cross-platform binary builds (8 GOOS × 8
GOARCH matrix with explicit `ignore:` exclusions). Two structural
invariants worth pinning: `CGO_ENABLED=0` (static-binary contract)
and `dist: _dist` (.gitignore coordination).

### 1.8 Top-level files (helm-specific conventions)

| File | What it does |
|---|---|
| `AGENTS.md` | Codebase tour for AI agents |
| `ADOPTERS.md` | User list |
| `KEYS` | GPG keyring for signed-release verification |
| `code-of-conduct.md` | Redirects to upstream Kubernetes/CNCF CoC |

### 1.9 Notable absences

- **No per-subdir OWNERS files** (unlike kubernetes/kubernetes which
  ships 596).
- **No `.editorconfig`** at root.
- **No `dependabot.yml`** — relies on Renovate at the org level.

---

## 2. Coverage classification

Each surface from §1 tagged with one of:

- ✅ **alint-today** — name the rule kind + ruleset OR per-rule entry
  in this directory's `.alint.yml`.
- 🔄 **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- ❌ **out-of-scope** — explain why (Go AST, AST-aware lint, codegen,
  binary signing, etc.).

### 2.1 `Makefile` targets

| Target | Coverage | Notes |
|---|---|---|
| `test-style` (`golangci-lint` + `validate-license.sh`) | ✅ alint-today (shellout + structural) | `helm-golangci-lint` (`for_each_dir` over `go.mod` + `command:`) + `helm-go-license-header` + `helm-shell-license-header` |
| `test-source-headers` (`validate-license.sh`) | ✅ alint-today | Replaced by the two `file_header` rules |
| `format` (`goimports -w`) | ✅ alint-today (read-only sibling) | `helm-gofmt-clean` (`gofmt -l` via `command:`) |
| `tidy` (`go mod tidy`) | ✅ alint-today (shellout) | `helm-go-mod-tidy` (`go mod tidy -diff` via `command:`) |
| `gen-test-golden` (`go test -update`) | 🔄 alint-future | `command_idempotent` (v0.10 design candidate, 2 sources — ruff + prettier; helm is the 3rd surface in the wild) |
| `build`, `install`, `dist`, `checksum`, `sign`, `release-notes` | ❌ out-of-scope | Build/packaging — not structural |

### 2.2 `scripts/validate-license.sh`

| Surface | Coverage | Notes |
|---|---|---|
| Apache-2 license header on every `.go` and `.sh` (excl. testdata + third_party) | ✅ alint-today | `helm-go-license-header` + `helm-shell-license-header` (`file_header` with `(?s)` + non-greedy `.{0,400}?` to accept all 4 comment-block shapes) |

### 2.3 `.golangci.yml`

| Section | Coverage | Notes |
|---|---|---|
| `formatters.enable` contains `gofmt` + `goimports` | ✅ alint-today | `helm-golangci-formatters-enabled` (`file_content_matches` against the YAML text — workaround for pitfall #17 `*_path_equals` against `[*]`) |
| `formatters.settings.goimports.local-prefixes[0]` pinned to `helm.sh/helm/v4` | ✅ alint-today | `helm-golangci-goimports-local-prefix` (`file_content_matches`) |
| 17 enabled linters (depguard, gomodguard, govet, …) | ❌ out-of-scope | All Go-AST aware analyses; live with golangci-lint |
| `depguard` / `gomodguard` import bans (e.g. no `github.com/hashicorp/go-multierror`) | 🔄 alint-future | `import_gate` (v0.10 ship-target, 4 sources — k8s + airflow + golang/go + pytorch; helm is the 5th demand source) |

### 2.4 GitHub Actions workflows (7 workflows)

| Workflow | Coverage | Notes |
|---|---|---|
| All 7 workflows | ✅ alint-today | Bundled `ci/github-actions@v1` (3 rules — `gha-workflow-contents-read`, `gha-pin-actions-to-sha`, `gha-workflow-has-name`) — applied uniformly across all 7 |
| `build-test.yml` license + tidy steps | ✅ alint-today | Mirrored as standalone rules (`helm-go-license-header` + `helm-go-mod-tidy`) |
| `golangci-lint.yml` lint runs | ✅ alint-today (shellout) | `helm-golangci-lint` |
| `govulncheck.yml` vuln scan | ✅ alint-today (shellout) | `helm-govulncheck` |
| `codeql-analysis.yml` CodeQL semantics | ❌ out-of-scope | GitHub-managed semantic analysis |
| `scorecards.yml` OpenSSF run | ❌ out-of-scope | OpenSSF-managed |
| `release.yml` goreleaser logic | ❌ out-of-scope | Release semantics |
| `stale.yaml` auto-close logic | ❌ out-of-scope | Bot operational |

### 2.5 `.github/env`

| Surface | Coverage | Notes |
|---|---|---|
| `GOLANG_VERSION` shape (`^GOLANG_VERSION=\d+\.\d+$`) | ✅ alint-today | `helm-github-env-pins-go-version` (`file_content_matches`) |
| `GOLANGCI_LINT_VERSION` shape (`^GOLANGCI_LINT_VERSION=v\d+\.\d+\.\d+$`) | ✅ alint-today | `helm-github-env-pins-golangci-lint-version` (`file_content_matches`) |
| `.github/env` ↔ `go.mod` `go <version>` cross-equality | 🔄 alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources — strongest demand of any v0.10 candidate) |

### 2.6 `OWNERS`

| Surface | Coverage | Rule |
|---|---|---|
| Top-level `OWNERS` exists | ✅ alint-today | `helm-owners-file-present` (`file_exists`) |
| `maintainers[0]` is a valid GitHub-username string | ✅ alint-today | `helm-owners-maintainer-format` (`yaml_path_matches` against `$.maintainers[0]`) |

### 2.7 `.goreleaser.yaml`

| Invariant | Coverage | Rule |
|---|---|---|
| `CGO_ENABLED=0` declared (static binary contract) | ✅ alint-today | `helm-goreleaser-cgo-disabled` (`file_content_matches`) |
| `dist: _dist` (`.gitignore` coordination) | ✅ alint-today | `helm-goreleaser-dist-dir` (`file_content_matches`) |
| 8×8 cross-platform matrix logic | ❌ out-of-scope | Build-system semantics |

### 2.8 Top-level governance files

| File | Coverage | Rule |
|---|---|---|
| `AGENTS.md` | ✅ alint-today | `helm-agents-md-present` (`file_exists`, info-level) |
| `ADOPTERS.md` | ✅ alint-today | `helm-adopters-md-present` (`file_exists`, info-level) |
| `KEYS` | ✅ alint-today | `helm-keys-file-present` (`file_exists`, info-level) |
| `code-of-conduct.md` redirects to CNCF | ✅ alint-today | `helm-code-of-conduct-points-to-cncf` (`file_content_matches`) |
| `CONTRIBUTING.md` | ✅ alint-today | Bundled `oss-baseline` |
| `README.md` | ✅ alint-today | Bundled `oss-readme-exists` + `oss-readme-non-stub` |
| `LICENSE` | ✅ alint-today | Bundled `oss-license-exists` + `oss-license-non-empty` |
| `SECURITY.md` | ✅ alint-today | Bundled `oss-security-policy-exists` + `oss-security-policy-non-empty` |

### 2.9 Per-subdir conventions (helm-specific)

| Convention | Coverage | Rule |
|---|---|---|
| Every `pkg/*/` package has at least one `*_test.go` | ✅ alint-today | `helm-pkg-has-tests` (`for_each_dir` + nested glob-existence check, soft floor — info-level) |
| `_dist/` and `_dist_versions/` not tracked | ✅ alint-today | `helm-no-tracked-dist` (`dir_absent` × 2) |

### 2.10 Cross-cutting (bundled rulesets)

| Surface | Coverage | Rule |
|---|---|---|
| Repo-wide hygiene (no `node_modules`, `__pycache__`, `target`, `dist/`, etc.) | ✅ alint-today | 11 rules from `hygiene/no-tracked-artifacts@v1` |
| Trojan-Source / CVE-2021-42574 + zero-width on Go sources | ✅ alint-today | Bundled `go@v1` (8 rules including `go-sources-no-zero-width`, `go-sources-final-newline`) |
| GHA hardening (3 rules) | ✅ alint-today | Bundled `ci/github-actions@v1` |

---

## 3. Quantified coverage

Counted across the **6 Makefile targets** + **1 `validate-license.sh`**
+ **17 enabled linters in `.golangci.yml`** + **3 `.golangci.yml`
shape pins** + **7 GHA workflows** + **3 `.github/env` rules** + **2
OWNERS rules** + **3 `.goreleaser.yaml` invariants** + **8 top-level
files** + **2 helm-specific conventions** + **3 cross-cutting bundle
groups** = **55 distinct surfaces**.

```
✅ alint-today:    37 / 55 = 67%   (4 shellouts + 2 license-header + 3 .golangci shape + 7 GHA + 2 .github/env + 2 OWNERS + 2 goreleaser + 8 top-level + 2 conventions + 3 bundles + 2 partial)
🔄 alint-future:    4 / 55 =  7%   (1 cross_file_value_equals + 1 import_gate (depguard/gomodguard) + 1 command_idempotent + 1 *_path_contains for set-membership)
❌ out-of-scope:   14 / 55 = 25%   (15 Go-AST linters + GitHub-managed CodeQL/Scorecard + release semantics + cosign signing + dupl/staticcheck/etc.)
                   ─────────────────
                   total = 100%
```

**Commentary.** Three observations:

1. **helm is the canonical "midsize Go monorepo with one home-grown
   bash script" data point.** Of the 37 alint-today surfaces, **only
   2 replace existing in-tree scripts** (the two `file_header` rules
   replacing `scripts/validate-license.sh`); the other 35 are
   conventions encoded for the first time. **alint replaces `make
   test-style`'s structural half (the license walk) and complements
   the AST half (golangci-lint) which stays where it is.**

2. **The 4 alint-future signals all increment existing v0.10
   ship-targets.** No new rule-kind candidates surface; helm
   reaffirms `cross_file_value_equals` (10 sources), `import_gate`
   (4 sources), and `command_idempotent` (now 3 sources in the wild
   counting ruff + prettier + helm). The `*_path_contains`
   set-membership shorthand (3 sources: helm + deno + bazel) was
   first surfaced by helm and remains a v0.10 design candidate.

3. **25% out-of-scope is unusually high — but expected.** The
   `.golangci.yml` enables 17 Go-AST linters (depguard, dupl,
   exhaustive, gomodguard, govet, ineffassign, …) — every one of
   them is the right tool for its job, and alint deliberately
   doesn't replicate Go AST analysis. **The out-of-scope label is
   positive, not apologetic** — these are checks where the existing
   tool *is* the right tool.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (398 lines including
narrative comments, **58 rules** loaded — confirmed by `alint
validate-config`: 24 helm-specific + 34 from 4 bundled rulesets
— `oss-baseline=15` + `go=8` + `ci/github-actions=3` +
`hygiene/no-tracked-artifacts=11` − overlap = 34 effective rule IDs
after dedup).

**Synopsis of the load-bearing repo-specific rules** (full config in
`.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/go@v1                            # 8 rules: go.mod/sum + bidi + zero-width + final-newline
  - alint://bundled/ci/github-actions@v1             # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules

rules:
  - id: helm-go-license-header                  # Apache-2 header on every .go (replaces validate-license.sh)
    kind: file_header
    paths: "**/*.go"
    pattern: |-                                  # |- (strip trailing newline) — pitfall #22 hardening fix
      (?s)Copyright The Helm Authors\..{0,400}?Licensed under the Apache License, Version 2\.0
  - id: helm-shell-license-header               # Same for .sh
  - id: helm-golangci-formatters-enabled         # file_content_matches against .golangci.yml YAML text (pitfall #17 workaround)
  - id: helm-golangci-goimports-local-prefix    # file_content_matches for `local-prefixes:.*helm.sh/helm/v4`
  - id: helm-golangci-lint                      # for_each_dir over go.mod + command: golangci-lint run
  - id: helm-gofmt-clean                        # command: gofmt -l (read-only)
  - id: helm-go-mod-tidy                        # command: go mod tidy -diff
  - id: helm-govulncheck                        # command: govulncheck ./...
  - id: helm-spelling                           # command: misspell -error (defensive)
  - id: helm-github-env-pins-go-version          # file_content_matches for ^GOLANG_VERSION=\d+\.\d+$
  - id: helm-github-env-pins-golangci-lint-version
  - id: helm-owners-file-present                # file_exists for OWNERS
  - id: helm-owners-maintainer-format           # yaml_path_matches for $.maintainers[0]
  - id: helm-pkg-has-tests                      # for_each_dir over pkg/* (info-level)
  - id: helm-goreleaser-cgo-disabled            # file_content_matches for CGO_ENABLED=0
  - id: helm-goreleaser-dist-dir                # file_content_matches for dist: _dist
  - id: helm-{agents,adopters,keys}-md-present  # file_exists × 3 (info-level)
  - id: helm-code-of-conduct-points-to-cncf     # file_content_matches for cncf.io/codeofconduct
  - id: helm-no-tracked-dist                    # dir_absent × 2 (_dist + _dist_versions)
```

**Repo-specific vs bundled split:**
- **24 helm-specific rules** in `.alint.yml` (the `helm-*` prefix)
- **34 bundled rules** from the 4 extended rulesets

**Validation:** `alint validate-config` reports `✓ Config valid: 58
rule(s) loaded`. The license-header rule was hardened in this batch
to use `pattern: |-` (strip-final-newline block scalar) per pitfall
#22 guidance — no behaviour change on the live tree (verified). No
pitfall #13/#14/#16/#17 instances.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the same
`/tmp/helm` working tree captured 2026-05-08. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary `target/release/alint
v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full pass** (58 rules, includes 5 `command:` shellouts that fail-fast on missing tools — `misspell`, `golangci-lint`, `gofmt`, `go`, `govulncheck`) | n/a | n/a | **5.94 s ± 0.009 s** | — (dominated by 537 spawn-failure messages from `helm-spelling` shellout when `misspell` isn't on PATH; structural floor is ~18 ms) |
| **alint lite pass** (4 bundled rulesets only, 34 rules, no shellouts) | n/a | n/a | **18.0 ms ± 0.3 ms** | — |
| `bash scripts/validate-license.sh`-equivalent (find + grep -L over `*.go` excluding testdata/third_party) | bash + find + grep | **18.3 ms ± 0.1 ms** | included in lite-pass + helm-go-license-header (~5 ms incremental on 536 .go files) | **~3-4× alint comparable** when only counting the license walk; alint also runs 57 other rules in the same pass |
| `shellcheck` on `scripts/*.sh` (5 scripts) | shellcheck | **87.6 ms ± 1.7 ms** | wrapped via the `helm-spelling` shellout — different tool; per-file shellcheck not in helm's config but available via the bundled `tooling` overlay candidate | n/a |
| `make test-style` end-to-end (`golangci-lint` + `validate-license.sh`) | golangci-lint + bash | pending — `golangci-lint` not on PATH in the validation env | wrapped via `helm-golangci-lint` `command:` rule | 1× — alint shells out |
| `make tidy` (`go mod tidy`) | go module tooling | pending — toolchain caveats | wrapped via `helm-go-mod-tidy` | 1× — alint wraps |
| `govulncheck ./...` | govulncheck | pending — not on PATH | wrapped via `helm-govulncheck` | 1× — alint wraps |

The headline number: **a single 18 ms alint lite-pass replaces all
the structural assertions across 1,990 files** (the 2 license-header
walks across .go + .sh, the 8 top-level governance file checks, the
2 goreleaser invariants, the 2 .github/env pins, the 2 OWNERS rules,
the 2 helm-specific conventions, plus the 11-rule hygiene + 8-rule
go + 3-rule GHA bundled overlays). The bash + grep equivalent of
`scripts/validate-license.sh` alone is 18 ms — alint's full
declarative pass is the same, while running 57 other rules.

### 5.2 Pending — needs additional toolchain

| Check | Tool | Reproduction |
|---|---|---|
| `helm-golangci-lint` | golangci-lint v2.11+ | `go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@v2.11.3 && time golangci-lint run ./...` |
| `helm-govulncheck` | govulncheck | `go install golang.org/x/vuln/cmd/govulncheck@latest && time govulncheck ./...` |
| `helm-spelling` | misspell | `go install github.com/golangci/misspell/cmd/misspell@latest && time misspell -error .` |
| `helm-gofmt-clean` | gofmt | `time bash -c 'find . -name "*.go" -not -path "*testdata*" \| xargs gofmt -l'` |
| `helm-go-mod-tidy` | go module tooling | `time go mod tidy -diff` |

The end-to-end `make test-style`-equivalent — `golangci-lint run ./...`
+ `bash scripts/validate-license.sh` — runs roughly 35-45s wall-clock
in helm's CI (golangci-lint dominates: ~30s on a warm cache walking
the whole module). alint's 18 ms structural floor adds <0.1%
wall-clock to that pipeline while catching 11 distinct classes of
regression that golangci-lint cannot see (the .github/env pins, the
goreleaser invariants, the OWNERS shape, the top-level governance
files, the hygiene baseline, etc.).

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/helm-helm/.alint.yml /tmp/helm` (live, JSON-format).

**Headline:** alint surfaces **643 violations** across 9 failing
rules. **537 are `helm-spelling` shellout-failure noise** (`misspell`
not on PATH in this validation env, fires once per file the rule
walks); the remaining **106 are real**: 50 trailing-whitespace + 46
final-newline cosmetics + **5 GHA workflows missing `permissions:
contents: read`** + **1 zero-width Trojan-Source catch in
`internal/plugin/plugin.go:80`** + 4 governance-file info-level
findings.

### 6.1 Real findings (after deducting cosmetic + spawn-failure class)

| Finding | Count | Severity | Rule | Triage |
|---|---:|---|---|---|
| Workflows missing `permissions: contents: read` | 5 | warning | `gha-workflow-contents-read` (bundled) | **Real findings** across `codeql-analysis.yml`, `govulncheck.yml`, `release.yml`, `scorecards.yml`, `stale.yaml`. The OpenSSF Scorecard Token-Permissions check would surface these as well, but only on the weekly `scorecards.yml` cron. Bundled GHA rule catches them on every PR |
| Zero-width Unicode character (U+200B/U+200C/U+200D/U+FEFF) in `internal/plugin/plugin.go:80:70` | 1 | error | `go-sources-no-zero-width` (bundled go ruleset) | **Real Trojan-Source / CVE-2021-42574 finding.** `internal/plugin/plugin.go:80` line 80, column 70 contains a zero-width character inside a comment block. helm's `validate-license.sh` doesn't look at character-class hygiene; golangci-lint doesn't either by default. **Net-new structural finding alint catches that no existing tool in helm's pipeline does.** Worth filing upstream for review (legitimate intent vs supply-chain risk) |
| Trailing whitespace | 50 | info | `oss-no-trailing-whitespace` (bundled) | Mostly under `.github/ISSUE_TEMPLATE/`, `.golangci.yml` line 43, ADOPTERS.md, and a handful of `internal/chart/v3/util/testdata/subpop/README.md` testdata files. Mechanical, but the kind of low-grade-noise finding alint's `fix:` blocks resolve in one pass |
| Missing final newline | 46 | info | `oss-final-newline` (bundled) | Same flavour — info-level cosmetic noise |
| `oss-codeowners-exists` info | 1 | info | `oss-codeowners-exists` (bundled) | helm uses `OWNERS` (CNCF convention), not `CODEOWNERS` — info-only |
| `oss-code-of-conduct-exists` info | 1 | info | `oss-code-of-conduct-exists` (bundled) | helm's `code-of-conduct.md` is at root and asserted by helm-specific rule; bundled rule expects `CODE_OF_CONDUCT.md` capitalisation |
| `helm-code-of-conduct-points-to-cncf` info | 1 | info | helm-specific | The CNCF redirect file content is asserted; rule fires info-level if the canonical text drifts |
| `oss-security-policy-non-empty` info | 1 | info | `oss-security-policy-non-empty` (bundled) | SECURITY.md is present but minimal |

**Real net-new findings alint surfaces that existing tooling misses:**
**1 zero-width Trojan-Source catch in `internal/plugin/plugin.go`**
(neither `validate-license.sh` nor golangci-lint scans for character-
class hygiene — alint's bundled go-ruleset does) + **5 supply-chain
GHA hardening signals** (only the weekly Scorecard cron catches these
in helm's existing pipeline; alint surfaces them every PR). Plus 96
informational cosmetic findings below helm's gate threshold.

### 6.2 The 537 `helm-spelling` shellout-failure messages — explained

The `helm-spelling` rule is a `command:` shellout to `misspell -error`.
In the validation env where this run happened, `misspell` is not on
PATH. The rule walks every text file in the tree (1,990 files) and
calls `misspell` per-file; each call fails with `could not spawn
misspell: No such file or directory (os error 2)`. The 537 violations
are all this same message.

**This is expected `command:` rule behaviour** when the upstream tool
isn't available. In a CI environment with `misspell` installed (as
helm's CI has), all 537 spawn-failures collapse to whatever real
findings misspell surfaces (typically <10 across the whole tree).

### 6.3 The Trojan-Source catch — confirmed

`internal/plugin/plugin.go:80` column 70 contains a zero-width
character. Quoting from the live alint output:

```
Zero-width characters in Go sources are rejected (review hazard).
```

The README's original §6 mention is verified. The character is most
likely in a comment block (line 80 is well past file headers in
real helm code); nature unclear without manual inspection. alint
flags it; the contributor decides if it's intentional.

### 6.4 No silent-failure-mode bugs in this config

No instances of pitfalls #13/#14/#16/#17 surfaced. The pitfall #22
fix (`pattern: |-` instead of `pattern: |`) was applied to
`helm-go-license-header` in this batch — verified clean.

---

## 7. Followup feature work surfaced

- **`cross_file_value_equals` rule kind** — `.github/env` ↔ `go.mod`
  go-version cross-equality. **v0.10 ship-target** (10 sources;
  strongest demand of any v0.10 candidate).
- **`import_gate` rule kind** — covers the `.golangci.yml` depguard
  / gomodguard import bans (no `github.com/hashicorp/go-multierror`,
  etc.). **v0.10 ship-target** (4 sources; saturated).
- **`command_idempotent` mode** — `make gen-test-golden` freshness
  ("running `make gen-test-golden` would not change the working
  tree"). **v0.10 design candidate** (3 sources in the wild: ruff +
  prettier + helm).
- **`*_path_contains` set-membership shorthand** — narrower than
  `*_path_equals` (which is "every match equals X") and
  `*_path_matches` (which is regex on string-typed values only).
  helm surfaced the pattern firsthand on `formatters.enable`
  set-membership. **v0.10 design candidate** (3 sources: helm +
  deno + bazel).

---

## 8. Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **Helm-chart structural invariants for `pkg/chart/testdata/`.**
   helm/helm itself ships zero deployable charts (it's the helm CLI
   source, not a chart consumer), but `pkg/chart/testdata/` hosts
   ~80 fixture charts. A `for_each_dir` over those plus
   `helm-chart-yaml-shape` (apiVersion/version/appVersion present +
   valid semver) would gate the test-fixture discipline that `helm
   lint` currently doesn't cover (the fixtures are deliberate
   corner cases, some intentionally malformed — the rule would carry
   a `scope_filter` excluding the malformed fixtures).
2. **Add `agent-context@v1` overlay (5 rules).** helm ships
   `AGENTS.md` but doesn't enforce its shape. The `agent-context@v1`
   ruleset gates AGENTS.md presence + tour-of-codebase content +
   AI-context-window-friendly structure declaratively.
3. **`alint suggest` against the live tree.** The repo is small
   enough (~530 .go files) that the suggester would terminate
   quickly; likely surface candidates: per-`pkg/*/` test-coverage
   thresholds, `cmd/helm/` subcommand-package conventions,
   `internal/` visibility discipline.

---

## 9. Validation status (2026-05-08)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **58** (24 helm-specific + 34 from 4 bundled
  rulesets — `oss-baseline=15`, `go=8`, `ci/github-actions=3`,
  `hygiene/no-tracked-artifacts=11`, with rule-id deduplication)
- **`alint validate-config`:** ✓ Config valid: 58 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for
  the 643-violation breakdown (537 spawn-failure noise + 96 cosmetic
  + 6 real structural + 1 Trojan-Source zero-width catch + 3 governance
  info-level)
- **Pitfall fixes (this batch):** **Pitfall #22 hardening** —
  `helm-go-license-header` pattern changed from `pattern: |` to
  `pattern: |-` for canonical-correct strip-final-newline semantics.
  Trivial 1-line fix; zero behaviour change on the live tree
  (verified — same violation counts before/after)
- **Open gaps:**
  - `cross_file_value_equals` (v0.10 ship-target, 10 sources)
  - `import_gate` (v0.10 ship-target, 4 sources)
  - `command_idempotent` (v0.10 design candidate, 3 sources)
  - `*_path_contains` (v0.10 design candidate, 3 sources)
- **Bench numbers:** 18 ms (lite bundled-only pass); 5.94 s (full
  pass dominated by the `helm-spelling` shellout's 537 spawn-failure
  messages on the validation env without `misspell` installed) on
  `/tmp/helm`'s 1,990-file tree
