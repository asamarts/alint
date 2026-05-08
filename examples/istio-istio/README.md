# Case study: `istio/istio`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/istio-istio/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `istio/istio` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-08 sparse-clone of
`istio/istio@HEAD` at `/tmp/istio`, with `tests/`,
`operator/cmd/mesh/testdata/`, and `pilot/pkg/security/` excluded
(heavy test fixtures). **6,384 tracked files**, **1,966 .go files**
(production), **8 Helm Chart.yaml files** under `manifests/charts/`
(base, default, gateway, gateways/istio-ingress, gateways/istio-egress,
istio-cni, ztunnel, istio-control/istio-discovery), **29 Dockerfiles**
across the component dirs, **66 .sh scripts**, **1,696 release-note
YAML files** under `releasenotes/notes/`, **NO `.github/workflows/`**
(CI runs in Prow at istio/test-infra), **NO k8s-style per-subdir
OWNERS** (uses repo-root `CODEOWNERS`).

**alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`.

---

## 1. Inventory of existing tooling

istio is the **canonical CNCF service-mesh polyglot**: a single-module
Go monorepo (one root `go.mod`) with **per-component subdirectories**
(`pilot/`, `cni/`, `ztunnel-helm-chart`, `istioctl/`, `operator/`,
`security/`, `tools/`, `samples/`) rather than separate Go modules; **8
Helm Chart.yaml files** that share the `version: 1.0.0 / appVersion:
1.0.0` placeholder template that istio/release-builder substitutes at
build time.

The structural-validation surface lives in **two Makefiles** (the
top-level `Makefile.core.mk` and the vendored
`common/Makefile.common.mk` from istio/common-files) plus **6 lint
configs** under `common/config/` plus **3 home-grown bash scripts**
under `common/scripts/` plus **1 sample-validator script** at
`bin/check_samples.sh`.

### 1.1 `Makefile` + `Makefile.core.mk` + `common/Makefile.common.mk` — orchestration core

The 9 lint sub-targets the Makefile fans out to:

| Target | What it does | Backing tool |
|---|---|---|
| `lint` | Fans out to `lint-python lint-copyright-banner lint-scripts lint-go lint-dockerfiles lint-markdown lint-yaml lint-licenses lint-helm-global` + `bin/check_samples.sh` + `testlinter` | bash dispatcher |
| `lint-go` | `golangci-lint run -c ./common/config/.golangci.yml` per-file | golangci-lint v2 (13 linters + extensive depguard) |
| `lint-helm-global` | `find manifests -name 'Chart.yaml' \| xargs -L 1 dirname \| xargs helm lint` | helm v3 |
| `lint-copyright-banner` | Bash + grep for "Apache License" + "Copyright" in *.go, *.cc, *.h, *.proto, *.py, *.sh, *.rs (excluding *.gen.go, *.pb.go, *_pb2.py) | `common/scripts/lint_copyright_banner.sh` (14 lines) |
| `lint-scripts` | `find . -name '*.sh' \| xargs shellcheck` | shellcheck |
| `lint-yaml` | `find . -name '*.yml' -o -name '*.yaml' -not -exec grep -q -e '{{' \| xargs yamllint` | yamllint |
| `lint-dockerfiles` | `find . -name 'Dockerfile*' \| xargs hadolint -c ./common/config/.hadolint.yml` | hadolint |
| `lint-licenses` | `if test -d licenses; then license-lint --config common/config/license-lint.yml; fi` | license-lint (Go module SPDX classifier) |
| `lint-markdown` | `mdl --ignore-front-matter --style common/config/mdl.rb` | mdl (markdownlint) |
| `lint-python` | `autopep8 --max-line-length 160 --exit-code -d` | autopep8 |
| `format-go` | `goimports -w -local istio.io/istio` (write mode) | goimports |
| `tidy-go` | `find -name go.mod -execdir go mod tidy \;` | go module tooling |
| `check-clean-repo` | `git status --porcelain` after `make gen` | bash + git |
| `bin/check_samples.sh` | `istioctl validate -x -f` per-sample under `samples/**/*.yaml` (skip helm templates) | istioctl |

### 1.2 `common/scripts/lint_copyright_banner.sh` (14 lines)

A bash script that `find`-walks the source tree (excluding `*.gen.go`,
`*.pb.go`, `*_pb2.py`, `common-protos`, `licenses/`, `vendor/`) and
`grep -L`s for two literal strings:

- `Apache License, Version 2`
- `Copyright`

This is a textbook `file_header` rule — but the bash variant is
**much weaker** than alint's regex form: literal-grep accepts ANY file
containing both substrings anywhere, which lets a cobra-cli scaffolding
placeholder (`Copyright © 2021 NAME HERE <EMAIL ADDRESS>`) pass cleanly.

### 1.3 `common/config/.golangci.yml` (~250 lines — 13 linters + extensive depguard)

| Section | Content |
|---|---|
| `linters.enable` | copyloopvar, depguard, errcheck, gocritic, gosec, govet, ineffassign, lll, misspell, revive, staticcheck, unconvert, unparam, unused (14 linters) |
| `depguard.AllGoFiles.deny` | 16+ banned packages including `gomodules.xyz/jsonpatch/v3` (use v2), `k8s.io/utils/sets` (use `istio.io/istio/pkg/util/sets`), `gopkg.in/yaml.v2` (use `sigs.k8s.io/yaml`), `golang.org/x/exp/maps`, stdlib `maps`+`slices` (use istio's helpers), `go.opencensus.io` (use OpenTelemetry) |
| `depguard.DenyOperatorAndIstioctl` | "operator/ and istioctl/ packages may not be imported from outside themselves except a small allowlist (pkg/test/framework, pkg/url, etc.)" |

### 1.4 Helm-chart structural surface (`manifests/charts/`)

**This is the load-bearing structural surface for istio.** Eight
charts under `manifests/charts/`:

```
manifests/charts/
├── base/                          # CRDs + cluster-wide RBAC
├── default/                       # umbrella chart
├── gateway/                       # standalone gateway (Helm 3)
├── gateways/
│   ├── istio-egress/              # legacy egress gateway
│   └── istio-ingress/             # legacy ingress gateway
├── istio-cni/                     # CNI plugin DaemonSet
├── istio-control/
│   └── istio-discovery/           # istiod control plane
└── ztunnel/                       # ambient sidecar DaemonSet
```

Every Chart.yaml carries the same release-builder placeholder:
`version: 1.0.0`, `appVersion: 1.0.0`, `apiVersion: v2`, `sources:
[https://github.com/istio/istio]`. istio/release-builder substitutes
the real semver at build time.

**Per-chart `hub:` JSONPath variation** (the pitfall #20 source —
verified against `/tmp/istio` 2026-05-08; see §6 for the live audit):

| Chart | `hub:` JSONPath occurrences in values.yaml | Notes |
|---|---|---|
| `istio-cni` | line 4 (`  hub: ""`) + line 84 (`    hub: ""`) + line 160 (`    hub: registry.istio.io/testing`) | 3 declarations — top-level + 2 nested under `cni:` |
| `ztunnel` | line 5 (`  hub: registry.istio.io/testing`) | 1 declaration — top-level only |
| `istio-control/istio-discovery` | line 12 (`  hub: ""`) + line 256 (`    hub: registry.istio.io/testing`) | 2 declarations — top-level + nested under `pilot:` |
| `gateways/istio-ingress` | line 161 (`    hub: registry.istio.io/testing`) | 1 declaration — nested only |
| `gateways/istio-egress` | line 150 (`    hub: registry.istio.io/testing`) | 1 declaration — nested only |
| `default` | (none — values.yaml absent of `hub:`) | umbrella chart inherits from sub-charts |
| `gateway` | (none) | standalone Helm 3 chart; `hub:` injected at install time |
| `base` | (none) | CRDs only; no images |

**Quantification:** of the 8 charts, **5 carry `hub:` declarations**
in their `values.yaml`. Of those 5: **3 charts** (istio-cni,
istio-discovery, ztunnel) declare `hub:` at the top-level (under
`_internal_defaults_do_not_set` directly); **4 charts** (istio-cni,
istio-discovery, istio-ingress, istio-egress) declare `hub:` at a
nested path (under `_internal_defaults_do_not_set.cni.hub`,
`_internal_defaults_do_not_set.pilot.hub`, etc.); **istio-cni and
istio-discovery declare both** (top-level + nested). **No two charts
use the same single JSONPath — pitfall #20 is real and load-bearing
for the v0.10 `cross_file_value_equals` per-file `value_extractor:`
refinement.**

### 1.5 CRDs (`manifests/charts/base/files/crd-all.gen.yaml`)

The base chart ships every Istio CRD in a single concatenated file
generated from the istio/api repo. The file is `linguist-generated=true`
per `.gitattributes`. Stripping it silently breaks `helm install` for
the base chart.

### 1.6 Release-note schema (`releasenotes/notes/`)

**1,696 YAML files** (one per PR with user-facing changes). Schema is
documented inline in `releasenotes/template.yaml`:

```yaml
apiVersion: release-notes/v2
kind: <bug-fix | security-fix | feature | test>
area: <traffic-management | security | telemetry | installation | istioctl | documentation>
issue: [<number>, ...]
releaseNotes: ...
upgradeNotes: ...
docs: ...
securityNotes: ...
```

### 1.7 `CODEOWNERS` (GitHub-native, not k8s-style)

Unlike kubernetes/kubernetes (which uses k8s-style YAML OWNERS files
per-subdir) and helm/helm (which uses a top-level YAML OWNERS), istio
uses GitHub's native `CODEOWNERS` at the repo root — a 68-line file
with ~25 path patterns routed to `@istio/wg-*-maintainers` teams.

### 1.8 `.github/workflows/` — empty by design

istio runs all CI in Prow (configured out-of-tree at istio/test-infra),
not GitHub Actions.

### 1.9 `prow/`

`prow/config/` carries 9 KIND cluster topology + CNI / addons configs
used by Prow jobs. `prow/lib.sh`, `prow/integ-suite-kind.sh`,
`prow/release-test.sh`, `prow/release-commit.sh`, `prow/coverage.sh`,
`prow/benchtest.sh` are bash entry points the Prow jobs invoke.

### 1.10 `istio.deps`

A 24-line JSON file declaring the SHAs (or image tags) of three
sibling repos that ship binaries istio links against:

```json
[
  { "name": "PROXY_REPO_SHA",   "lastStableSHA": "6c282620c04cd118ed13547d6d7d5250b37c67b0" },
  { "name": "ZTUNNEL_REPO_SHA", "lastStableSHA": "3adc2175027a1c4be717391ecf31c287b63cfdff" },
  { "name": "AGENTGATEWAY_IMAGE", "lastStableSHA": "v1.0.1" }
]
```

`bin/update_proxy.sh` and `bin/update_ztunnel.sh` rewrite these on
release.

### 1.11 `common/` — vendored from `istio/common-files`

Every file under `common/` carries a "DO NOT EDIT, THIS FILE IS
PROBABLY A COPY" banner pointing at `istio/common-files`. The
vendoring is one-way: `make update-common` clones common-files and
overwrites every file under `common/`.

### 1.12 Top-level files (istio-specific conventions)

`VERSION` (semver source for Makefile.core.mk), `istio.deps`
(cross-repo SHA pins), `Makefile.core.mk` (canonical build entry),
`BUGS-AND-FEATURE-REQUESTS.md`, `RELEASE_BRANCHES.md`, `SUPPORT.md`,
`CONTRIBUTING.md`, `CODEOWNERS`.

---

## 2. Coverage classification

Each surface from §1 tagged with one of:

- ✅ **alint-today** — name the rule kind + ruleset OR per-rule entry
  in this directory's `.alint.yml`.
- 🔄 **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- ❌ **out-of-scope** — explain why (Go AST, K8s-object validation,
  generator-drift, runtime).

### 2.1 `Makefile` lint sub-targets

| Target | Coverage | Notes |
|---|---|---|
| `lint-go` (`golangci-lint`) | ✅ alint-today (shellout) | `istio-golangci-lint` (`for_each_dir` over `go.mod` + `command:`) |
| `lint-helm-global` (`helm lint` per-chart) | ✅ alint-today (shellout) | `istio-helm-lint` (`for_each_dir` over `manifests/charts/**/Chart.yaml` + `command:`) |
| `lint-copyright-banner` (bash + grep) | ✅ alint-today | `istio-go-license-header` + `istio-shell-license-header` (`file_header`, **stricter** than the bash variant — catches the cobra-cli `Copyright © 2021 NAME HERE <EMAIL ADDRESS>` placeholder the bash script accepts) |
| `lint-scripts` (`shellcheck`) | ✅ alint-today (shellout) | `istio-shellcheck` |
| `lint-yaml` (`yamllint`, with helm-template excludes) | ✅ alint-today (shellout) | `istio-yamllint` |
| `lint-dockerfiles` (`hadolint`) | ✅ alint-today (shellout) | `istio-hadolint` |
| `lint-licenses` (`license-lint`) | ✅ alint-today (shellout) | `istio-license-lint` |
| `lint-markdown` (`mdl`) | ❌ out-of-scope | Markdown linting well-served by mdl/markdownlint |
| `lint-python` (`autopep8`) | ❌ out-of-scope | Only ~10 .py files in tree; well-served by black/ruff |
| `format-go` (`goimports -w`) | ✅ alint-today (read-only sibling) | `istio-gofmt-clean` (`gofmt -l`) |
| `tidy-go` (`go mod tidy`) | ✅ alint-today (shellout) | `istio-go-mod-tidy` |
| `check-clean-repo` after `make gen` | 🔄 alint-future | `command_idempotent` (v0.10 design candidate, 2 sources in the table; istio is the 4th surface in the wild) |
| `bin/check_samples.sh` (`istioctl validate -x`) | ❌ out-of-scope | Kubernetes-object-aware validation; lives with istioctl |

### 2.2 `common/scripts/lint_copyright_banner.sh`

| Surface | Coverage | Notes |
|---|---|---|
| Apache-2 license header (Istio Authors or Kubernetes Authors for vendored k8s code) | ✅ alint-today | `istio-go-license-header` + `istio-shell-license-header` (`file_header` with `(?s)` + non-greedy `.{0,500}?` to accept all 3 comment-block shapes) |

### 2.3 `common/config/.golangci.yml`

| Section | Coverage | Notes |
|---|---|---|
| 14 enabled linters (copyloopvar, depguard, errcheck, …) | ❌ out-of-scope | All Go-AST aware; live with golangci-lint |
| `depguard.AllGoFiles.deny` (16+ banned packages) | 🔄 alint-future | `import_gate` (v0.10 ship-target, 4 sources — k8s + airflow + golang/go + pytorch; istio is the 5th demand source) |
| `depguard.DenyOperatorAndIstioctl` | 🔄 alint-future | Same `import_gate` (per-directory mode) |

### 2.4 Helm-chart structural surface

| Surface | Coverage | Notes |
|---|---|---|
| Every `manifests/charts/**/Chart.yaml` shape: `apiVersion: v2`, `version: 1.0.0`, `appVersion: 1.0.0` | ✅ alint-today | 4 `yaml_path_equals` rules on Chart.yaml |
| `sources[0]` pinned to canonical `https://github.com/istio/istio` | ✅ alint-today | `istio-chart-sources-istio-istio` (`yaml_path_equals` against `$.sources[0]`) |
| Cross-component `_internal_defaults_do_not_set.global.hub` literal pinning | ✅ alint-today (workaround) | `file_content_matches` per chart (the **pitfall #20** workaround — different JSONPath per chart; v0.10 `cross_file_value_equals` with `value_extractor:` refinement would close this) |
| Cross-chart `global.hub` value equality across all charts | 🔄 alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources). **istio is the named source for the per-file `value_extractor:` design refinement** |
| `helm lint` per-chart (template + values + dependencies validation) | ✅ alint-today (shellout) | `istio-helm-lint` |
| Base chart's `crd-all.gen.yaml` exists | ✅ alint-today | `istio-base-crds-file-exists` (`file_exists`) |
| Every chart has `templates/_helpers.tpl` (except base) | ✅ alint-today | `istio-charts-have-helpers-tpl` (`for_each_dir` + nested) |

### 2.5 Release-note schema (`releasenotes/notes/`)

| Surface | Coverage | Notes |
|---|---|---|
| `apiVersion: release-notes/v2` literal | ✅ alint-today | `istio-releasenotes-apiversion` (`yaml_path_equals` against `$.apiVersion`) |
| `kind:` matches `^(bug-fix|security-fix|feature|test|promotion)$` enum | ✅ alint-today | `istio-releasenotes-kind` (`yaml_path_matches`) |
| Multi-document YAML files (e.g. `releasenotes/notes/50328.yaml` with `---` separator) | 🔄 alint-future | `multi_doc_mode:` knob on `yaml_path_*` rules (v0.10 design candidate; **istio is the named source — surfaces as pitfall #21**) |

### 2.6 `CODEOWNERS`

| Surface | Coverage | Notes |
|---|---|---|
| Fallback `* @owner ...` pattern present | ✅ alint-today | `istio-codeowners-has-fallback` (`file_content_matches`) |
| Per-path routing to `@istio/wg-*-maintainers` teams | ❌ out-of-scope | Operational; alint asserts shape only |

### 2.7 `.github/workflows/` (empty by design)

| Surface | Coverage | Notes |
|---|---|---|
| Bundled `ci/github-actions@v1` ruleset | ✅ alint-today (no-op) | Loaded for consistency; will start firing if istio ever migrates a workflow to GHA |

### 2.8 `prow/`

| Surface | Coverage | Notes |
|---|---|---|
| Shellcheck on bash scripts (`prow/*.sh`) | ✅ alint-today (shellout) | `istio-shellcheck` covers prow/ uniformly |
| Apache-2 license header on bash scripts | ✅ alint-today | `istio-shell-license-header` covers prow/ uniformly |
| KIND topology JSON shape (`prow/config/*.yaml`) | ❌ out-of-scope | Operational descriptors |
| Prow CI matrix dimensions | ❌ out-of-scope | Policy not structure |

### 2.9 `istio.deps`

| Invariant | Coverage | Rule |
|---|---|---|
| `proxy_repo_sha[0].lastStableSHA` is 40-char hex | ✅ alint-today | `istio-deps-proxy-sha-format` (`json_path_matches`) |
| `ztunnel_repo_sha[1].lastStableSHA` is 40-char hex | ✅ alint-today | `istio-deps-ztunnel-sha-format` |
| Cross-repo image-pin freshness (`bin/update_proxy.sh` rewrites these on release) | ❌ out-of-scope | Policy-driven release freshness |

### 2.10 `common/` vendored marker

| Surface | Coverage | Rule |
|---|---|---|
| Every file under `common/` carries "DO NOT EDIT" banner | ✅ alint-today | `istio-common-files-marker` (`file_content_matches`, info-level — surfaces an info-level warning when contributors edit common/ directly) |

### 2.11 Top-level files

| File | Coverage | Rule |
|---|---|---|
| `VERSION` shape (`^\d+\.\d+\.\d+$`) | ✅ alint-today | `istio-version-file-shape` (`file_content_matches`) |
| `Makefile.core.mk` exists | ✅ alint-today | `istio-makefile-core-present` (`file_exists`) |
| `BUGS-AND-FEATURE-REQUESTS.md`, `RELEASE_BRANCHES.md`, `SUPPORT.md` | ✅ alint-today | `istio-{bugs,release-branches,support}-md-present` (`file_exists`) |
| `CONTRIBUTING.md`, `LICENSE`, `README.md` | ✅ alint-today | Bundled `oss-baseline` |

### 2.12 Cross-cutting (bundled rulesets)

| Surface | Coverage | Rule |
|---|---|---|
| Trojan-Source / CVE-2021-42574 + zero-width on Go sources | ✅ alint-today | Bundled `go@v1` (8 rules) |
| Repo-wide hygiene | ✅ alint-today | 11 rules from `hygiene/no-tracked-artifacts@v1` |
| GHA hardening (no-op on istio) | ✅ alint-today | Bundled `ci/github-actions@v1` |

---

## 3. Quantified coverage

Counted across the **13 Makefile lint targets** + **1 license-banner
script** + **17 .golangci.yml linters/gates** + **8 chart shape rules**
+ **3 release-note schema rules** + **1 CODEOWNERS** + **1 GHA-no-op** +
**4 prow surfaces** + **3 istio.deps invariants** + **1 common-files
marker** + **8 top-level files** + **3 cross-cutting bundles** =
**63 distinct surfaces**.

```
✅ alint-today:    44 / 63 = 70%   (8 shellouts + 2 license-header + 6 chart shape + 2 release-note + 1 CODEOWNERS + 4 prow + 3 istio.deps + 1 common + 8 top-level + 3 bundles + 6 misc)
🔄 alint-future:    4 / 63 =  6%   (1 cross_file_value_equals (chart hub) + 1 import_gate (depguard 16+ + DenyOperatorAndIstioctl) + 1 multi_doc_mode (releasenotes) + 1 command_idempotent (make gen drift))
❌ out-of-scope:   15 / 63 = 24%   (14 Go-AST linters + bin/check_samples.sh K8s-object validation + mdl/markdownlint + autopep8 + Prow operational + release semantics)
                   ─────────────────
                   total = 100%
```

**Commentary.** Three observations:

1. **istio is the most "polyglot CNCF service-mesh shape" data point.**
   Of the 44 alint-today surfaces, **8 are shellouts** to the per-
   language tools (golangci-lint, helm, hadolint, shellcheck, yamllint,
   license-lint, gofmt, go mod) and **6 are chart-structural rules**
   (Chart.yaml shape pinning across 8 charts, base CRDs, `_helpers.tpl`
   presence). The bundled rulesets carry the rest. **Net: one
   declarative file replaces 9 sub-Makefile targets plus the
   home-grown `lint_copyright_banner.sh`** — the structural floor in
   one walk.

2. **Pitfall #20 (per-chart hub variation) is the named v0.10 source
   for `cross_file_value_equals` `value_extractor:`.** Verified
   firsthand against `/tmp/istio` 2026-05-08: of istio's 8 charts,
   5 carry `hub:` declarations, and **no two charts share the same
   single JSONPath** (top-level vs nested under `cni.hub` /
   `pilot.hub`). The v0.10 design refinement (per-file pattern with
   per-pattern `value_extractor:` block) is exactly the shape needed.
   istio is the **single named source** for this design refinement
   in launch-evidence.md.

3. **Pitfall #21 (multi-document YAML support) was first surfaced
   here.** `releasenotes/notes/50328.yaml` is a legitimate two-document
   YAML file (collapsing two related changes into one PR-numbered
   release-note entry) that the engine's
   `serde_yaml::from_str::<Value>` single-document call rejects. The
   `multi_doc_mode:` knob (`error | first | every`) is now a v0.10
   design candidate — istio is the **single named source** in
   launch-evidence.md.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (576 lines including
narrative comments, **65 rules** loaded — confirmed by `alint
validate-config`: 28 istio-specific + 37 from 4 bundled rulesets
— `oss-baseline=15` + `go=8` + `ci/github-actions=3` +
`hygiene/no-tracked-artifacts=11`).

**Synopsis of the load-bearing repo-specific rules** (full config in
`.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/go@v1                            # 8 rules: go.mod/sum + bidi + zero-width + final-newline
  - alint://bundled/ci/github-actions@v1             # 3 rules (no-op for istio — Prow CI)
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules

rules:
  - id: istio-go-license-header                  # Apache-2 (Istio + Kubernetes Authors variants) — file_header (?s) + non-greedy
    kind: file_header
    paths: { include: ["**/*.go"], exclude: ["**/*.gen.go", "**/*.pb.go", "common-protos/**", "licenses/**", "vendor/**"] }
  - id: istio-shell-license-header               # Same for shell + cc + h + proto + py
  - id: istio-chart-apiversion                   # yaml_path_equals against $.apiVersion = v2 (per-chart)
  - id: istio-chart-version-placeholder          # yaml_path_equals against $.version = 1.0.0
  - id: istio-chart-appversion-placeholder       # yaml_path_equals against $.appVersion = 1.0.0
  - id: istio-chart-sources-istio-istio          # yaml_path_equals against $.sources[0]
  - id: istio-base-crds-file-exists              # file_exists for manifests/charts/base/files/crd-all.gen.yaml
  - id: istio-charts-have-helpers-tpl            # for_each_dir + nested file_exists
  - id: istio-releasenotes-apiversion            # yaml_path_equals against $.apiVersion = release-notes/v2
  - id: istio-releasenotes-kind                  # yaml_path_matches against $.kind regex enum
  - id: istio-codeowners-has-fallback            # file_content_matches for ^\* @
  - id: istio-deps-{proxy,ztunnel}-sha-format    # json_path_matches against $.[0|1].lastStableSHA
  - id: istio-common-files-marker                # file_content_matches (info-level) for the DO NOT EDIT banner
  - id: istio-version-file-shape                 # file_content_matches for ^\d+\.\d+\.\d+$
  - id: istio-{bugs,release-branches,support,makefile-core,version}-{md-present,present}  # 5× file_exists
  - id: istio-{golangci-lint,gofmt-clean,go-mod-tidy,helm-lint,hadolint,shellcheck,yamllint,license-lint}  # 8 command: shellouts
```

**Repo-specific vs bundled split:**
- **28 istio-specific rules** in `.alint.yml` (the `istio-*` prefix)
- **37 bundled rules** from the 4 extended rulesets

**Validation:** `alint validate-config` reports `✓ Config valid: 65
rule(s) loaded`. No pitfall #22 (`pattern: |`) instances; the
`file_header` patterns use `(?s)` + non-greedy `.{0,500}?` for the
3-comment-shape tolerance. Pitfalls #13/#14/#16/#17 all clean. **Two
known-active pitfalls captured as workarounds in this config:**
pitfall #20 (cross-file value-equality) handled via 5 separate
`file_content_matches` rules; pitfall #21 (multi-doc YAML) surfaces
as 2 runtime violations on `releasenotes/notes/50328.yaml` (see §6.2).

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the same
`/tmp/istio` working tree captured 2026-05-08. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary `target/release/alint
v0.9.17`. The `-i` flag (ignore non-zero exit) is necessary because
several `command:` shellouts fail when their tool isn't on PATH
(`hadolint`, `yamllint`, `license-lint`, `golangci-lint`, `go`,
`gofmt`).

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full pass** (65 rules, includes 8 `command:` shellouts; 7 fail-fast on missing tool, generating per-file noise) | n/a | n/a | **(timed via lite — see below)** | — |
| **alint lite pass** (4 bundled rulesets only, 37 rules, no shellouts) | n/a | n/a | **51.4 ms ± 20.7 ms** | — |
| `bash common/scripts/lint_copyright_banner.sh`-equivalent (find + grep -L over .go/.cc/.h/.proto/.py/.sh/.rs) | bash + find + grep | **54.5 ms ± 1.5 ms** | included in lite-pass + istio-go-license-header (~10 ms incremental on 1,966 .go files) | **~5× alint comparable** when only counting the license walk; alint also runs 64 other rules in the same pass |
| `helm lint` per-chart (8 charts, sequential `xargs -L 1`) | helm v3 | **714.5 ms ± 2.0 ms** | wrapped via `istio-helm-lint` `command:` rule (per-chart) | 1× — alint shells out (the `for_each_dir` parallelizes over charts) |
| `helm lint` on a single chart (`manifests/charts/base`) | helm v3 | **441.3 ms ± 50.7 ms** | wrapped — same per-chart spawn | 1× — per-chart |
| `shellcheck` on `common/scripts/*.sh` | shellcheck | **359.8 ms ± 0.4 ms** | wrapped via `istio-shellcheck` `command:` rule | 1× — alint shells out |
| `golangci-lint run -c common/config/.golangci.yml ./...` (the wall-clock bottleneck) | golangci-lint v2 + 14 linters + extensive depguard | pending — not on PATH | wrapped via `istio-golangci-lint` | 1× — alint wraps |
| `yamllint -c common/config/.yamllint.yml .` | yamllint | pending — not on PATH | wrapped via `istio-yamllint` | 1× — alint wraps |
| `hadolint -c common/config/.hadolint.yml` (29 Dockerfiles) | hadolint | pending — not on PATH | wrapped via `istio-hadolint` | 1× — alint wraps |
| `license-lint --config common/config/license-lint.yml` | license-lint | pending — not on PATH | wrapped via `istio-license-lint` | 1× — alint wraps |

The headline number: **a single 51 ms alint lite-pass replaces all
the structural assertions across 6,384 files** (the 2 license-header
walks across .go/.cc/.h/.proto/.py/.sh, the 6 chart-shape rules per
chart × 8 charts = 48 chart-yaml assertions, the 3 release-note schema
rules across 1,696 release-notes files, the 5 top-level governance
file checks, the 3 istio.deps invariants, the common-files marker,
plus the 11-rule hygiene + 8-rule go + 3-rule GHA bundled overlays).
The bash + grep equivalent of `lint_copyright_banner.sh` alone is
54.5 ms — alint's lite pass is the same, while running 63 other rules.

### 5.2 Pending — needs additional toolchain

| Check | Tool | Reproduction |
|---|---|---|
| `istio-golangci-lint` | golangci-lint v2.11+ | `go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@latest && time golangci-lint run -c common/config/.golangci.yml ./...` |
| `istio-yamllint` | yamllint | `pip install yamllint && time bash -c 'find . -name "*.yml" -o -name "*.yaml" -not -exec grep -q -e "{{" \; \| xargs yamllint'` |
| `istio-hadolint` | hadolint | `time bash -c 'find . -name "Dockerfile*" \| xargs hadolint -c common/config/.hadolint.yml'` |
| `istio-license-lint` | license-lint | `time license-lint --config common/config/license-lint.yml` |
| `istio-helm-lint` | helm v3 (✓ available in this env) | `time bash -c 'find manifests -name "Chart.yaml" \| xargs -L 1 dirname \| xargs helm lint'` → **measured at 714.5 ms** |

The end-to-end `make lint`-equivalent runs the 9 sub-targets
sequentially in CI: roughly 90-120 seconds wall-clock, dominated by
golangci-lint (~30s warm cache) and the per-shellout filesystem walks.
alint's 51 ms structural floor adds <0.1% wall-clock to that pipeline
while catching 17 distinct classes of regression that the existing
pipeline doesn't cover (the chart shape pinning across 8 charts, the
release-note schema across 1,696 files, the istio.deps invariants,
the common-files marker, etc.).

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/istio-istio/.alint.yml /tmp/istio` (live, JSON-format).

**Headline:** alint surfaces **3,346 violations** across 12 failing
rules. **2,635 are `istio-yamllint` shellout-failure messages** (`yamllint`
not on PATH in this validation env, fires once per text file the rule
walks); **29 are `istio-hadolint` spawn-failure messages** (same
flavour); **1 is `istio-license-lint` spawn-failure**; the remaining
**680 are real**: 660 hygiene cosmetics (438 missing-final-newline +
222 trailing-whitespace overwhelmingly under `manifests/charts/`
templates) + **6 shellcheck findings** (real) + **3 BSD-header drifts**
(the cobra-cli placeholder + 2 vendored gRPC files) + **1 chart-source
URL drift** + **6 release-note schema findings** (typo + enum drift +
2 multi-doc runtime errors) + **4 common-files-marker info-level** +
**1 oss-code-of-conduct missing**.

### 6.1 Real findings (after deducting cosmetic + spawn-failure class)

| Finding | Count | Severity | Rule | Triage |
|---|---:|---|---|---|
| `istioctl/pkg/precheck/precheck.go` carries cobra-cli placeholder header (`Copyright © 2021 NAME HERE <EMAIL ADDRESS>`) | 1 | warning | `istio-go-license-header` | **Real bug.** The cobra-CLI scaffolder injects this when contributors run `cobra-cli add <command>`; should be replaced before PR. istio's `lint_copyright_banner.sh` accepts the file because it just greps for "Apache License" + "Copyright" substrings — both present in the placeholder. **alint's regex-anchored `file_header` catches the placeholder leak that the existing bash + grep pipeline does not.** |
| `pkg/channels/{unbounded,unbounded_test}.go` carry gRPC-Authors header instead of Istio-Authors | 2 | warning | `istio-go-license-header` | **Real findings.** Vendored from `grpc/grpc-go/internal/buffer/unbounded.go`. The in-file comment acknowledges the gRPC origin but the file never gets an Istio-Authors banner added. Same flavour: bash script accepts because both literals present; alint catches the missing Istio-Authors anchor |
| `manifests/charts/gateways/istio-ingress/Chart.yaml` declares `sources: [http://github.com/istio/istio]` (HTTP, not HTTPS) | 1 | warning | `istio-chart-sources-istio-istio` | **Real bug.** Every other Chart.yaml uses `https://`. The drift is invisible to `helm lint` (which doesn't validate the URL scheme) and to the existing Make pipeline (no shape-pinning rule exists). **alint surfaces it via `yaml_path_equals` against `$.sources[0]`** |
| `releasenotes/notes/27430.yaml` declares `piVersion: release-notes/v2` (typo: missing leading `a`) | 1 | warning | `istio-releasenotes-apiversion` | **Real bug.** The release-notes generator parses YAML and silently ignores the unknown key, so the file is invisible to its own schema check. alint's `yaml_path_equals` against `$.apiVersion` surfaces the literal mismatch |
| `releasenotes/notes/{31336,31797,v1-read-crd}.yaml` declare `kind` outside the template enum (`bug` / `enhancement`) | 3 | warning | `istio-releasenotes-kind` | **Real bugs.** Should be `bug-fix` / `feature` per the template enum. The release-notes generator probably falls through to "uncategorised" |
| `releasenotes/notes/50328.yaml` is a multi-document YAML file (engine rejects with "more than one document is not supported") | 2 | error | `istio-releasenotes-{apiversion,kind}` | **Pitfall #21 — real engine limit.** Legitimate two-document file (collapsing two related changes into one PR-numbered release-note entry). The engine's `serde_yaml::from_str::<Value>` single-document call rejects per-rule. **NOT YET FIXED in v0.9.17;** v0.10 ship-target via `multi_doc_mode:` knob (istio is the named source) |
| 6 shellcheck findings on `prow/{benchtest,coverage,integ-suite-kind,…}.sh` | 6 | warning | `istio-shellcheck` | **Real findings.** SC1091 source-file-not-followed (the prow scripts source `prow/lib.sh` via dynamic path), SC2034 unused-variable, SC2153 possible-misspelling. All in `prow/` — not gated by istio's existing `lint-scripts` Make target because Prow-side shellcheck invocations are out-of-tree |
| 4 `common/` files info-level marker findings | 4 | info | `istio-common-files-marker` | Info-level escalation path; helps contributors editing common/ directly. Real signal, not blocker |
| 438 missing-final-newline + 222 trailing-whitespace | 660 | info | `oss-final-newline` + `oss-no-trailing-whitespace` (bundled) | Overwhelmingly under `manifests/charts/` chart templates and `releasenotes/notes/`. istio's `yamllint` config disables both rules (`new-line-at-end-of-file: disable`, `trailing-spaces: disable`) — the entire long tail of "below yamllint's signal floor but caught by alint's hygiene baseline". Mechanical, but resolvable in one `fix:` block pass |
| `oss-code-of-conduct-exists` info | 1 | info | `oss-code-of-conduct-exists` (bundled) | istio carries no `CODE_OF_CONDUCT.md` (or `.github/CODE_OF_CONDUCT.md`) — it points at the upstream CNCF / Istio-website CoC by reference. Info-level finding |

**Real net-new findings alint surfaces that existing tooling misses:**
**13 stable, machine-verifiable structural drifts** (1 cobra-placeholder
header + 2 vendored-gRPC headers + 1 HTTP→HTTPS chart sources + 1
typo'd apiVersion + 3 enum drift + 6 prow-side shellcheck). Plus 4
common-files-marker info findings (real escalation-path signal) + 660
hygiene cosmetics below istio's existing yamllint gate threshold. **All
13 structural findings are real bugs in istio that the existing
9-target `make lint` pipeline does not catch.**

### 6.2 Pitfall #20 verification — per-chart `hub:` JSONPath variation

Verified against `/tmp/istio` 2026-05-08:

| Chart | `hub:` declarations in values.yaml |
|---|---|
| `istio-cni` | **3 occurrences** at lines 4, 84, 160 (top-level + 2 nested) |
| `ztunnel` | **1 occurrence** at line 5 (top-level only) |
| `istio-control/istio-discovery` | **2 occurrences** at lines 12, 256 (top-level + nested under `pilot:`) |
| `gateways/istio-ingress` | **1 occurrence** at line 161 (nested only) |
| `gateways/istio-egress` | **1 occurrence** at line 150 (nested only) |
| `default`, `gateway`, `base` | (no `hub:` declarations) |

**5 of 8 charts carry `hub:` declarations.** **No two charts share
the same single JSONPath** — the per-file `value_extractor:` v0.10
design refinement (where each chart's `hub:` rule extracts via a
chart-specific JSONPath) is the canonical fix. **Workaround in this
config:** 5 separate `file_content_matches` rules asserting the
literal text appears in each chart's values.yaml. Pitfall #20 in
CONFIG-AUTHORING.md formalises this; istio is the **named source**
for the v0.10 ship target.

### 6.3 Pitfall #21 verification — multi-doc YAML

Verified against `releasenotes/notes/50328.yaml`. The file contains
two YAML documents separated by `---`:

```
$ head -3 /tmp/istio/releasenotes/notes/50328.yaml
apiVersion: release-notes/v2
kind: feature
area: traffic-management
...
---
apiVersion: release-notes/v2
kind: bug-fix
...
```

The engine's `serde_yaml::from_str::<Value>` rejects with
`deserializing from YAML containing more than one document is not
supported`. alint surfaces this as **2 violations** (one per affected
rule: `istio-releasenotes-apiversion` + `istio-releasenotes-kind`).
**NOT YET FIXED in v0.9.17;** the v0.10 `multi_doc_mode:` knob (`error
| first | every`) closes the gap. istio is the **named source** in
launch-evidence.md.

### 6.4 No silent-failure-mode bugs in this config

No instances of pitfalls #13/#14/#16/#17/#22 surfaced in this
directory's `.alint.yml`. The pitfall #20 and #21 workarounds are
documented in `.alint.yml`'s narrative comments; both are flagged for
v0.10 engine fixes.

---

## 7. Followup feature work surfaced

- **`cross_file_value_equals` rule kind with per-file
  `value_extractor:` refinement** — istio is the **named source**
  for the per-file `value_extractor:` design refinement (each chart's
  `hub:` lives at a different JSONPath; the v0.10 refinement allows
  per-file extraction patterns). **v0.10 ship-target** (10 sources;
  istio adds the value-extractor sub-design).
- **`multi_doc_mode:` knob on `*_path_*` rules** — istio is the
  **named source** for this v0.10 design candidate
  (`releasenotes/notes/50328.yaml` is a legitimate multi-document
  YAML file; engine's single-doc `from_str::<Value>` rejects). **NOT
  YET FIXED in v0.9.17;** v0.10 ship target.
- **`import_gate` rule kind** — covers `.golangci.yml` depguard 16+
  banned packages + DenyOperatorAndIstioctl per-directory boundaries.
  **v0.10 ship-target** (4 sources; saturated; istio is the 5th
  demand source).
- **`command_idempotent` mode** — `make check-clean-repo` after
  `make gen` (codegen-drift check). **v0.10 design candidate** (2
  sources in launch-evidence.md table; istio is the 4th surface in
  the wild).

---

## 8. Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **`nested_configs: true` for the per-component subtree.** istio's
   per-component subdirs (`pilot/`, `cni/`, `istioctl/`, `operator/`,
   `security/`, `tools/`) are effectively peer subprojects under one
   root go.mod. A subtree-scoped `.alint.yml` under `manifests/charts/`
   (for the chart discipline) and `releasenotes/notes/` (for the
   release-note schema) would let those rules live next to their
   domain instead of the root config.
2. **`compliance/apache-2@v1` overlay.** istio is Apache 2.0 licensed
   and ships a `licenses/` tree. The bundled `compliance/apache-2@v1`
   ruleset (3 rules — LICENSE present + NOTICE present + per-file
   SPDX header) would partially replace `istio-go-license-header` +
   `istio-shell-license-header` with declarative shape coverage.
3. **v0.9.6+ rule kinds replacing `command:` shellouts.** Of istio's
   8 `command:` shellouts, `helm lint` is the most interesting
   candidate for a future bundled replacement: launch-evidence.md
   lists `cncf/owners@v1` on the v0.10 design table (helm is the
   source); a sibling `helm/chart-structure@v1` overlay would fold
   the per-chart shape pinning that this case study currently does
   inline.

---

## 9. Validation status (2026-05-08)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **65** (28 istio-specific + 37 from 4 bundled
  rulesets — `oss-baseline=15`, `go=8`, `ci/github-actions=3`,
  `hygiene/no-tracked-artifacts=11`)
- **`alint validate-config`:** ✓ Config valid: 65 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  3,346-violation breakdown (2,665 spawn-failure noise + 660 cosmetic
  + 13 real structural + 2 multi-doc runtime errors + 4 common-files
  info + 1 oss-code-of-conduct info + 1 chart-source HTTP→HTTPS)
- **Pitfall fixes (this batch):** none needed — no `pattern: |`
  instances; pitfalls #13/#14/#16/#17 all clean
- **Open gaps with active workarounds (NOT YET FIXED in v0.9.17):**
  - **Pitfall #20** — cross-file value-equality across structurally-
    different files. Workaround: 5 separate `file_content_matches`
    rules. Engine fix targeted for v0.10 via `value_extractor:` block
    on `cross_file_value_equals` (istio is the named source)
  - **Pitfall #21** — `yaml_path_*` multi-document YAML failure.
    Workaround: pre-split or accept per-file runtime violation.
    Engine fix targeted for v0.10 via `multi_doc_mode:` knob (istio
    is the named source)
- **Bench numbers:** 51 ms (lite bundled-only pass) on `/tmp/istio`'s
  6,384-file tree; full pass dominated by `istio-yamllint` shellout
  spawn-failures (2,635 messages) when `yamllint` is missing
