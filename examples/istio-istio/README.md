# Case study: `istio/istio`

> Marketing writeup (narrative, headline catch, competitive framing)
> lives at <https://alint.org/examples/istio-istio/>. This README is
> the engineering reference: tooling inventory, mapping table, gap
> catalogue, validation status.

Inventory of the structural-validation tooling in `istio/istio` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-checkout via
`git clone --depth=1 --filter=blob:none --sparse`, with `tests/`,
`operator/cmd/mesh/testdata/`, and `pilot/pkg/security/` excluded
(heavy test fixtures).

---

## Summary

istio is the **canonical CNCF service-mesh polyglot**: a single-module
Go monorepo (one root `go.mod`, ~1,238 production `.go` files,
~6,400 tracked files) with **per-component subdirectories** (pilot/,
cni/, ztunnel-helm-chart, istioctl/, operator/, security/, tools/,
samples/) rather than separate Go modules; **9 Helm Chart.yaml
files** under `manifests/charts/` that all share the
`version: 1.0.0 / appVersion: 1.0.0` placeholder template that
istio/release-builder substitutes at build time; **29 Dockerfiles**
across the component dirs; **~1,699 release-note YAML files** under
`releasenotes/notes/` with a fixed schema; **NO GitHub Actions
workflows** — istio runs CI exclusively in Prow (out-of-tree); **NO
k8s-style OWNERS files** — uses the GitHub-native `CODEOWNERS` at the
repo root.

The structural-validation surface lives in **two Makefiles** (the
top-level `Makefile.core.mk` and the vendored
`common/Makefile.common.mk` from the istio/common-files repo) plus
**6 lint configs** under `common/config/` (`.golangci.yml`,
`.yamllint.yml`, `.hadolint.yml`, `mdl.rb`, `license-lint.yml`,
`sass-lint.yml`) plus **3 home-grown bash scripts** under
`common/scripts/` (`lint_copyright_banner.sh`, `format_go.sh`,
`check_clean_repo.sh`) plus **1 sample-validator script** at
`bin/check_samples.sh`.

The 9 lint sub-targets the Makefile fans out to:
`lint-dockerfiles` (hadolint), `lint-scripts` (shellcheck),
`lint-yaml` (yamllint), `lint-helm-global` (helm lint),
`lint-copyright-banner` (custom bash + grep), `lint-go`
(golangci-lint), `lint-python` (autopep8), `lint-markdown` (mdl),
`lint-licenses` (license-lint).

Roughly **65 % map directly to existing alint rules** (license
header, OWNERS/CODEOWNERS shape, Chart.yaml shape, top-level
files, release-note schema, hygiene floor), **~15 % shell out via
the `command` rule kind** to existing tools (`golangci-lint`,
`gofmt`, `helm lint`, `hadolint`, `shellcheck`, `yamllint`,
`license-lint`), and **~20 % are out of alint's scope by design**
(depguard's 16+ banned-import rules, the
DenyOperatorAndIstioctl per-directory import boundaries,
`bin/check_samples.sh`'s `istioctl validate -x` per-sample, the
codegen-drift checks the `gen-check` Make target enforces).

The 65-rule starter config in [`/.alint.yml`](.alint.yml) replaces
**every structural assertion `make lint` makes about istio's own
tree** that isn't a Go-AST analysis or Kubernetes-object-aware
validation. Net: one declarative file replaces 9 sub-Makefile targets
(`lint-go` / `lint-helm-global` / `lint-copyright-banner` / etc.)
plus 4 home-grown bash scripts (`lint_copyright_banner.sh`,
`check_clean_repo.sh`, `bin/check_samples.sh` is shell-out only,
`format_go.sh` mirrored as a `command:` rule) plus the half-dozen
shape-implicit assertions buried inside `common/config/.golangci.yml`
and the per-chart values.yaml family.

---

## Existing tooling inventory

### `Makefile` + `Makefile.core.mk` + `common/Makefile.common.mk` — the orchestration core

istio's dev workflow is Makefile-driven (CONTRIBUTING.md walks
contributors through `make lint`, `make test`, `make gen-check`).
The structural-relevant targets:

| Target | What it does | alint replacement |
|---|---|---|
| `lint` | Fans out to `lint-python lint-copyright-banner lint-scripts lint-go lint-dockerfiles lint-markdown lint-yaml lint-licenses lint-helm-global` + `bin/check_samples.sh` + `testlinter` | Direct mapping below |
| `lint-go` | `golangci-lint run -c ./common/config/.golangci.yml` per-file | `istio-golangci-lint` (for_each_dir over `go.mod` + command) |
| `lint-helm-global` | `find manifests -name 'Chart.yaml' \| xargs -L 1 dirname \| xargs helm lint` | `istio-helm-lint` (for_each_dir over `manifests/charts/**/Chart.yaml` + command) |
| `lint-copyright-banner` | Bash + grep for "Apache License" + "Copyright" in *.go, *.cc, *.h, *.proto, *.py, *.sh, *.rs (excluding *.gen.go, *.pb.go, *_pb2.py) | `istio-go-license-header` + `istio-shell-license-header` (file_header) — **stricter than the bash variant**: catches the cobra-cli `Copyright © 2021 NAME HERE <EMAIL ADDRESS>` placeholder in `istioctl/pkg/precheck/precheck.go` that the bash script accepts (see "Real findings") |
| `lint-scripts` | `find . -name '*.sh' \| xargs shellcheck` | `istio-shellcheck` (command) |
| `lint-yaml` | `find . -name '*.yml' -o -name '*.yaml' -not -exec grep -q -e '{{' \| xargs yamllint` | `istio-yamllint` (command, with `manifests/charts/**/templates/**` excluded since templates contain `{{`) |
| `lint-dockerfiles` | `find . -name 'Dockerfile*' \| xargs hadolint -c ./common/config/.hadolint.yml` | `istio-hadolint` (command) |
| `lint-licenses` | `if test -d licenses; then license-lint --config common/config/license-lint.yml; fi` | `istio-license-lint` (command) |
| `lint-markdown` | `mdl --ignore-front-matter --style common/config/mdl.rb` | **Out of scope** — markdown linting is well-served by mdl/markdownlint already |
| `lint-python` | `autopep8 --max-line-length 160 --exit-code -d` | **Out of scope** — Python linting is well-served by black/ruff already (only ~10 .py files in tree) |
| `format-go` | `goimports -w -local istio.io/istio` (write mode) | Read-only sibling `istio-gofmt-clean` (`gofmt -l` via command) |
| `tidy-go` | `find -name go.mod -execdir go mod tidy \;` | `istio-go-mod-tidy` (`go mod tidy -diff`) |
| `check-clean-repo` | `git status --porcelain` after `make gen` to assert generators are idempotent | **Out of scope** — needs `command_idempotent` / generator-diff primitive (v0.10+ candidate) |
| `bin/check_samples.sh` | `istioctl validate -x -f` per-sample under `samples/**/*.yaml` (skipping helm templates) | **Out of scope** — Kubernetes-object-aware validation; lives with istioctl |

### `common/scripts/lint_copyright_banner.sh`

A 14-line bash script that `find`-walks the source tree (excluding
`*.gen.go`, `*.pb.go`, `*_pb2.py`, `common-protos`, `licenses/`,
`vendor/`) and `grep -L`s for two literal strings:

- `Apache License, Version 2`
- `Copyright`

This is a textbook `file_header` rule — but the bash variant is
**much weaker** than alint's regex form: the literal-grep accepts
ANY file that contains both substrings anywhere, which lets a
cobra-cli scaffolding placeholder (`Copyright © 2021 NAME HERE
<EMAIL ADDRESS>`) pass cleanly. alint's regex is anchored to the
Istio-Authors literal, catching the placeholder leak.

The wrinkle: **three** comment-block shapes coexist in the istio tree:

```
// Copyright 2017 Istio Authors          /*                            // Copyright 2019 gRPC authors.
//                                        Copyright 2017 The Kubernetes Authors.    (vendored grpc-go)
// Licensed under the Apache License,     Licensed under the Apache License...      → fails the rule;
//   Version 2.0...                                                                    file is upstream-vendored
                                          */
                                          (vendored k8s leader-election code)
```

The alint replacement uses a `(?s)` regex with a `.{0,500}?`
non-greedy gap so the rule matches every Istio-Authors / Kubernetes-
Authors variant. Files with non-Apache-2 headers (e.g. the gRPC-Authors
preamble in `pkg/channels/unbounded.go`) are flagged as needing
Istio-Authors banner — which is a real upstream-merge regression to
catch.

### `common/config/.golangci.yml`

This is **the** file that drives Go linting in istio. ~250 lines, 13
linters enabled (`copyloopvar`, `depguard`, `errcheck`, `gocritic`,
`gosec`, `govet`, `ineffassign`, `lll`, `misspell`, `revive`,
`staticcheck`, `unconvert`, `unparam`, `unused`).

The depguard configuration is **extensive** — 16+ banned packages
including `gomodules.xyz/jsonpatch/v3` (use v2), `k8s.io/utils/sets`
(use `istio.io/istio/pkg/util/sets`), `k8s.io/utils/strings/slices`,
`gopkg.in/yaml.v2` (use `sigs.k8s.io/yaml`), `golang.org/x/exp/maps`,
the stdlib `maps` and `slices` packages (istio prefers its own
helpers), `go.opencensus.io` (use OpenTelemetry), and the
`DenyOperatorAndIstioctl` rule that forbids importing
`istio.io/istio/operator` or `istio.io/istio/istioctl` from anywhere
except those packages themselves and a small allowlist
(pkg/test/framework, pkg/url, etc.).

**alint's coverage of `.golangci.yml`** is the **shape, not the
semantics**: the actual lint runs stay with golangci-lint itself,
invoked via `istio-golangci-lint` (a `for_each_dir` over `go.mod`
that `command:`s out to `golangci-lint run ./...`). The
depguard / DenyOperatorAndIstioctl rules need the v0.10+ `import_gate`
primitive — see "Needs new alint primitive" below.

### Helm-chart structural surface (`manifests/charts/`)

**This is the load-bearing structural surface for istio.** Nine
charts under `manifests/charts/`:

```
manifests/charts/
├── base/                          # CRDs + cluster-wide RBAC
├── default/                       # umbrella that depends on others
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
the real semver at build time. **alint asserts those four invariants
declaratively via `yaml_path_equals` rules — surfacing the live drift
where `manifests/charts/gateways/istio-ingress/Chart.yaml` declares
`sources: [http://github.com/istio/istio]` (HTTP, not HTTPS) instead
of the canonical https URL every other chart uses.**

The cross-component image-pinning convention every chart's
`_internal_defaults_do_not_set.global.hub` defaults to
`registry.istio.io/testing` (the Prow dev-build registry); every
chart's `global.tag` defaults to `latest`. The release pipeline
substitutes `gcr.io/istio-release` / the real semver tag. The
**path of `hub:` differs per chart** — some carry it at the
top-level under `_internal_defaults_do_not_set.hub`, others under
`_internal_defaults_do_not_set.global.hub`, depending on whether
the chart is a service-mesh data-plane component (ztunnel) or a
control-plane component (istio-control/istio-discovery). One
declarative `cross_file_value_equals` would express the contract;
in the meantime the config asserts the literal via
`file_content_matches` against the YAML text (the workaround
captured in pitfall #20 below).

### CRDs (`manifests/charts/base/files/crd-all.gen.yaml`)

The base chart ships every Istio CRD in a single concatenated
file generated from the istio/api repo. The file is
`linguist-generated=true` per `.gitattributes`. Stripping it
silently breaks `helm install` for the base chart (which ships CRDs
via plain templates, not the Helm `crds/` directory, since istio
self-manages CRD upgrades). alint asserts `file_exists` for this
file — a canary the Make-target architecture can't easily express.

### Release-note schema (`releasenotes/notes/`)

~1,699 YAML files, one per PR with user-facing changes. Schema is
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

alint asserts `apiVersion: release-notes/v2` (literal) plus
`kind: ^(bug-fix|security-fix|feature|test|promotion)$` (regex
enum). The release-notes generator (`release/template.yaml`) parses
these files at release time; any drift breaks the generator.

**Live findings against the snapshot:**

- `releasenotes/notes/27430.yaml` declares `piVersion: release-notes/v2`
  (typo: missing leading `a`). The release-notes generator accepts
  this silently because YAML unknown keys are ignored by default.
  alint surfaces it as `apiVersion` mismatch.
- `releasenotes/notes/31336.yaml` declares `kind: bug` (should be
  `bug-fix` per the template enum).
- `releasenotes/notes/31797.yaml` and
  `releasenotes/notes/v1-read-crd.yaml` declare `kind: enhancement`
  (not in the template enum either).
- `releasenotes/notes/50328.yaml` is a multi-document YAML file with
  `---` separator, which the engine's `yaml_path_*` runtime refuses
  with "deserializing from YAML containing more than one document
  is not supported" — see pitfall #21 below.

### `CODEOWNERS` (GitHub-native, not k8s-style)

Unlike kubernetes/kubernetes (which uses k8s-style YAML OWNERS files
per-subdir) and helm/helm (which uses a top-level YAML OWNERS), istio
uses GitHub's native `CODEOWNERS` at the repo root — a 68-line file
with ~25 path patterns routed to `@istio/wg-*-maintainers` teams.
alint asserts the fallback `* @owner ...` pattern is present (the
load-bearing line that catches every PR not matched by a more
specific rule).

### `.github/workflows/` — empty by design

istio runs all CI in Prow (configured out-of-tree at
istio/test-infra), not GitHub Actions. The bundled
`ci/github-actions@v1` ruleset no-ops on the tree. Listed in
`extends:` for consistency with the other case studies; if istio
ever migrates a workflow to GHA the ruleset will start firing
without config changes.

### `prow/`

`prow/config/` carries 9 KIND cluster topology + CNI / addons configs
used by the istio/test-infra Prow jobs. `prow/lib.sh`,
`prow/integ-suite-kind.sh`, `prow/release-test.sh`,
`prow/release-commit.sh`, `prow/coverage.sh`, `prow/benchtest.sh`
are bash entry points the Prow jobs invoke. alint applies the
shellcheck + license-header rules to this directory uniformly.

### `istio.deps`

A 24-line JSON file declaring the SHAs (or image tags) of three
sibling repos that ship binaries istio links against at build/deploy
time:

```json
[
  { "name": "PROXY_REPO_SHA",   "lastStableSHA": "6c282620c04cd118ed13547d6d7d5250b37c67b0" },
  { "name": "ZTUNNEL_REPO_SHA", "lastStableSHA": "3adc2175027a1c4be717391ecf31c287b63cfdff" },
  { "name": "AGENTGATEWAY_IMAGE", "lastStableSHA": "v1.0.1" }
]
```

`bin/update_proxy.sh` and `bin/update_ztunnel.sh` rewrite these on
release. alint asserts the SHA / semver shape of `[0]` and `[1]` —
catching the regression where a contributor forgets to fill in the
new SHA after running `update_proxy.sh`.

### `common/` — vendored from `istio/common-files`

Every file under `common/` carries a "DO NOT EDIT, THIS FILE IS
PROBABLY A COPY" banner pointing at `istio/common-files`. The
vendoring is one-way: `make update-common` clones common-files and
overwrites every file under `common/`. alint asserts the banner
(at info severity) so a contributor making a local edit gets the
right escalation path — upstream the change to common-files first,
then re-run update-common.

### Top-level files (istio-specific conventions)

`VERSION` (semver source for Makefile.core.mk), `istio.deps`
(cross-repo SHA pins, above), `Makefile.core.mk` (the canonical
build entry), `BUGS-AND-FEATURE-REQUESTS.md`, `RELEASE_BRANCHES.md`,
`SUPPORT.md`, `CONTRIBUTING.md`, `CODEOWNERS`. Each declared as a
`file_exists` rule.

---

## Maps to existing alint rules (what the starter config covers)

65 rules in [`/.alint.yml`](.alint.yml), broken down:

- **4 bundled rulesets** (`oss-baseline`, `go`, `ci/github-actions`,
  `hygiene/no-tracked-artifacts`) — pull in 37 rules between
  them (`oss-baseline=15` + `go=8` + `ci/github-actions=3` +
  `hygiene/no-tracked-artifacts=11`), including the trojan-
  source / zero-width / final-newline / trailing-whitespace
  floor and the GHA hardening (no-ops here since istio uses Prow)
- **2 license-header rules** (`istio-go-license-header`,
  `istio-shell-license-header`) — stricter replacement for
  `common/scripts/lint_copyright_banner.sh`, with regex tolerance for
  the three comment-block shapes that coexist in the tree (Istio-line-
  comment, Istio-block-comment, vendored-Kubernetes-block-preamble)
- **4 Chart.yaml shape assertions** — apiVersion/version/appVersion
  pinned to literal placeholders + sources[0] pinned to the canonical
  HTTPS URL
- **3 cross-component conventions** — internal_defaults_do_not_set
  wrapper present, hub: registry.istio.io/testing literal, tag: latest
  literal (the v0.10+ `cross_file_value_equals` candidate would
  collapse the hub + tag rules into one declarative assertion each)
- **2 chart-helper structural rules** — base CRDs file present,
  every chart has templates/_helpers.tpl (except base, which omits)
- **2 istio.deps invariants** — proxy + ztunnel lastStableSHA
  declared as 40-char hex or semver
- **7 `command` shellouts** — `golangci-lint run`, `gofmt -l`,
  `go mod tidy -diff`, `helm lint` (per-chart), `hadolint`,
  `shellcheck`, `yamllint`, `license-lint`
- **2 release-note schema rules** — `apiVersion: release-notes/v2`
  literal + `kind:` enum regex
- **4 top-level file presence rules** — `VERSION`, `istio.deps`,
  `Makefile.core.mk`, plus the VERSION file shape assertion
  (semver-line regex)
- **1 CODEOWNERS shape** — fallback `* @owner ...` pattern present
- **1 common-files vendored marker** — info-level pointer at
  `istio/common-files` for contributors editing common/ directly

---

## Real findings against the live tree (2026-05-06 snapshot)

Running the config against the cloned istio tree (with
`hadolint`/`shellcheck`/`yamllint`/`license-lint`/`golangci-lint`/`gofmt`/`go`/`helm`
not on PATH in the validation env, so the `command:` rules surface
as "could not spawn" warnings — expected) surfaces **nine genuine
structural-hygiene findings** the existing tooling misses or accepts
silently:

1. **`istioctl/pkg/precheck/precheck.go` carries the cobra-cli
   placeholder header** — `// Copyright © 2021 NAME HERE
   <EMAIL ADDRESS>`. The cobra-CLI scaffolder injects this when
   you run `cobra-cli add <command>`; the developer is supposed to
   replace it before the PR. istio's
   `lint_copyright_banner.sh` accepts the file because it just
   greps for the literals "Apache License" + "Copyright" — both
   present. alint's regex-anchored `file_header` catches the
   placeholder leak that the existing bash + grep pipeline does
   not.

2. **`pkg/channels/unbounded.go` and
   `pkg/channels/unbounded_test.go` carry the gRPC-Authors header**
   — vendored from `grpc/grpc-go/internal/buffer/unbounded.go`. The
   in-file comment acknowledges this ("Heavily inspired by the
   private library from gRPC… Original license:") but the file
   itself never gets an Istio-Authors banner added. Same flavour as
   #1: the bash script accepts it because both literals are
   present; alint's regex catches the missing Istio-Authors anchor.

3. **`manifests/charts/gateways/istio-ingress/Chart.yaml` declares
   `sources: [http://github.com/istio/istio]`** (HTTP, not HTTPS).
   Every other Chart.yaml uses `https://`. The drift is invisible
   to `helm lint` (which doesn't validate the URL scheme) and to
   the existing Make pipeline (no shape-pinning rule exists). alint
   surfaces it via `yaml_path_equals` against `$.sources[0]`.

4. **`releasenotes/notes/27430.yaml` declares `piVersion:
   release-notes/v2`** — typo with missing leading `a`. The
   release-notes generator parses YAML and silently ignores the
   unknown key, so the file is invisible to the generator's own
   schema check (the file's actual `apiVersion` field is missing,
   which means the generator falls back to its default — silent
   regression). alint's `yaml_path_equals` against `$.apiVersion`
   surfaces it as a literal mismatch.

5. **`releasenotes/notes/31336.yaml` declares `kind: bug`** —
   should be `bug-fix` per the template enum. The release-notes
   generator probably falls through to "uncategorised" for this
   entry.

6. **`releasenotes/notes/31797.yaml` and
   `releasenotes/notes/v1-read-crd.yaml` declare
   `kind: enhancement`** — not in the template enum either. Same
   silent-fallback behaviour as #5.

7. **23 `info`-level final-newline / trailing-whitespace findings
   under `manifests/charts/`** — the bundled
   `oss-final-newline` / `oss-no-trailing-whitespace` rules catch
   the .gitignore'd-but-tracked drift in chart templates. The
   existing `yamllint` configuration (`new-line-at-end-of-file:
   disable`, `trailing-spaces: disable`) explicitly disables both
   rules, so this is the entire long tail of "below
   yamllint's signal floor but caught by alint's hygiene baseline".
   Mechanical, but the kind of low-grade-noise finding alint's
   `fix:` blocks resolve in one pass.

Plus a **structural drift the bundled `oss-code-of-conduct-exists`
rule catches**: istio carries no `CODE_OF_CONDUCT.md` (or
`.github/CODE_OF_CONDUCT.md`, etc.) — it points at the upstream
CNCF / Istio-website CoC by reference rather than a local file.
Info-level finding; not a blocker.

---

## Needs new alint primitive

istio's structural surface is large enough that the existing
high-priority rule-kind candidates (already filed from earlier case
studies) all increment their demand signal here:

| Need | What it would check | What alint needs |
|---|---|---|
| **`.golangci.yml` `depguard.AllGoFiles.deny` import bans** (16+ packages including `gomodules.xyz/jsonpatch/v3`, `k8s.io/utils/sets`, `gopkg.in/yaml.v2`, `golang.org/x/exp/maps`, the stdlib `maps`/`slices` packages — replace with istio.io/istio/pkg/* equivalents) | per-package import-allowlist gates | The `import_gate` rule kind — now `v0.10 ship-target` per launch-evidence.md (4 sources: k8s, airflow, golang/go, pytorch). istio surfaces the same depguard shape as a saturating signal. |
| **`.golangci.yml` `depguard.DenyOperatorAndIstioctl`** ("operator/ and istioctl/ packages may not be imported from outside themselves except a small allowlist") | per-directory Go-import-boundary | Same `import_gate` rule kind, with the per-directory mode. |
| **`make check-clean-repo` after `make gen`** ("running `make gen` would not change the working tree") | The `command_idempotent` rule kind — `v0.10 design candidate` per launch-evidence.md (2 sources in the table; istio is the 4th surface in the wild). |
| **Cross-chart `global.hub` value equality** ("every chart that declares `_internal_defaults_do_not_set.global.hub` uses the same literal across charts") | `cross_file_value_equals` rule kind — `v0.10 ship-target` per launch-evidence.md (10 sources). istio is the **named source** for the per-file `value_extractor:` design refinement (`v0.10 design candidate`); see launch-evidence.md table. |
| **YAML multi-document support in `*_path_*` rules** | "yaml_path_equals against `releasenotes/notes/50328.yaml` — a multi-doc file separated by `---`" | The `serde_yaml::from_str::<Value>` engine call rejects multi-doc YAML with "deserializing from YAML containing more than one document is not supported". The `multi_doc_mode: { error | first | every }` knob on `yaml_path_*` rules is now a `v0.10 design candidate` per launch-evidence.md (istio is the named source). **Surfaced first by istio.** |

The first three are duplicates of needs already filed from earlier
case studies — istio increments their demand signal but doesn't
introduce new rule-kind candidates.

The fourth — `cross_file_value_equals` for chart hub/tag pinning —
already has the strongest demand signal of any v0.10 candidate
(now `v0.10 ship-target`, 10 sources per launch-evidence.md);
istio is the **named source for the per-file `value_extractor:`
refinement** because some charts have
`$._internal_defaults_do_not_set.hub` while others have
`$._internal_defaults_do_not_set.global.hub`. The v0.10 design
slot for `value_extractor:` is captured in launch-evidence.md
under the "v0.10 design candidates" table.

The fifth — multi-document YAML support in `*_path_*` rules — was
new at istio's original-write time. **Surfaced first by istio**;
now a `v0.10 design candidate` per launch-evidence.md
(`multi_doc_mode:` knob on `yaml_path_*` with values
`error | first | every`). The `serde_yaml::from_str::<Value>`
single-document engine call surfaces "deserializing from YAML
containing more than one document is not supported" as a runtime
violation per match. See pitfall #21 below for the user-facing
surface; the engine fix is targeted for v0.10.

---

## Out of alint's scope (use the existing tool)

istio's out-of-scope list is the longest of any case study so far,
because the depguard configuration is unusually elaborate:

- **16+ depguard `AllGoFiles.deny` rules** — Go-AST aware (per-file
  import-spec analysis). alint can't see imports without parsing
  Go; `import_gate` (v0.10+) would close the gap with a declarative
  manifest.
- **`depguard.DenyOpenTelemetry`** ("OpenTelemetry direct usage is
  forbidden outside `pkg/monitoring/` and `pkg/tracing/`") — same
  primitive, with a per-directory allowlist.
- **`depguard.DenyOperatorAndIstioctl`** ("operator/ and istioctl/
  may not be imported except by themselves and a small allowlist")
  — same primitive.
- **`depguard.DenyProtobufV1`** ("don't use
  `github.com/golang/protobuf/ptypes`; use
  `google.golang.org/protobuf/types/known` instead") — same primitive.
- **`gocritic`, `revive`, `staticcheck`, `unused`, `unparam`,
  `unconvert`, `errcheck`, `gosec`, `lll`, `copyloopvar`,
  `ineffassign`** — Go-AST / SSA / type-aware analyses; live with
  golangci-lint
- **`bin/check_samples.sh`** (`istioctl validate -x` per-sample
  YAML under `samples/**/*.yaml` excluding helm templates) —
  Kubernetes-object-aware validation; lives with istioctl
- **`make gen` codegen drift** (Go AST + protobuf + CRD generators) —
  out of alint's "no codegen" non-goal; addressed by the v0.10+
  `command_idempotent` candidate
- **`testlinter`** (the istio-internal test-package convention
  enforcer; checks every `*_test.go` carries the `+build` tag and
  the right TestMain shape) — Go-AST aware, lives in the
  `tools/testlinter/` directory of this repo
- **The Prow CI matrix dimensions** (KIND topology × Kubernetes
  version × ambient/sidecar mode × Envoy version) — policy, not
  structure
- **Per-chart `helm template` rendering vs golden-file diff** — the
  `pkg/helm/testdata/` golden-file infrastructure; out of scope (no
  generator-diff primitive yet)
- **The release pipeline's per-arch image-build matrix** — policy,
  not structure
- **`make sign` cosign / GPG-detached-sig pipeline** — release
  semantics

---

## Already covered by other linters istio uses

- `golangci-lint` (orchestrator for 13 linters + extensive depguard)
  — alint shells out via `command:` for one-shot orchestration; the
  deep checks stay inside golangci-lint
- `helm lint` — alint shells out per-chart via `for_each_dir` over
  Chart.yaml + command
- `hadolint` — Dockerfile linting; alint shells out per-Dockerfile
- `shellcheck` — bash linting; alint shells out per-script
- `yamllint` — YAML linting; alint shells out per-non-template YAML
  file
- `license-lint` — Go module-license SPDX classifier; alint shells
  out for one-shot orchestration
- `mdl` — markdown linting; out of scope (alint has its own
  bundled ruleset; markdownlint handles the deeper checks)
- `autopep8` — Python formatting; out of scope (~10 .py files; the
  existing make target is sufficient)
- `cosign` / GPG signing of releases — out of scope; not structural

---

## Performance comparison (placeholder — bench when validation pass scales)

istio's existing pipeline structure: `make lint` runs the 9
sub-targets sequentially:
1. `lint-python` — autopep8 walks ~10 .py files
2. `lint-copyright-banner` — bash + grep walks ~1,300 source files
3. `lint-scripts` — shellcheck walks 65 .sh files (256-batch)
4. `lint-go` — golangci-lint walks the whole module (~30s warm cache)
5. `lint-dockerfiles` — hadolint walks 29 Dockerfiles
6. `lint-markdown` — mdl walks ~99 .md files
7. `lint-yaml` — yamllint walks ~3,000 YAML files (filtered)
8. `lint-licenses` — license-lint walks the licenses/ tree
9. `lint-helm-global` — helm lint runs 9 times (one per Chart.yaml)

Each shell script does its own filesystem walk, which dominates
wall time for 6,400-file repos like istio. A typical `make lint` PR
gate sees 90-120s wall-clock.

alint's parallel-rule dispatch (v0.9.3+) collapses the
license-walk, the Chart.yaml shape checks, the release-note
schema checks, and the GHA-workflow shape checks (no-op in this
case) into a single filesystem walk. Expected: ~1-3 seconds for the
alint subset on a istio-scale repo (compare to the v0.9.6 published
S3 100k bench: 1.13s for the workspace bundle; istio is ~6.4k files).
The `golangci-lint` shellout via `istio-golangci-lint` remains the
wall-clock bottleneck — but that's the existing tool's runtime,
unchanged.

To benchmark wall-clock for real:
`time make lint` against
`time alint check && time golangci-lint run -c ./common/config/.golangci.yml ./...`
on the same checkout. Deferred to the per-repo measurement pass.

---

## Followup feature work surfaced (de-duplicated against earlier case-study gap lists)

- **`cross_file_value_equals` rule kind** — `v0.10 ship-target`
  (10 sources per launch-evidence.md). istio is the named source
  for the per-file `value_extractor:` refinement (`v0.10 design
  candidate`).
- **`import_gate` rule kind** — `v0.10 ship-target` (4 sources
  per launch-evidence.md; saturated). istio surfaces the same
  depguard shape.
- **`command_idempotent` mode** — `v0.10 design candidate` (2
  sources in launch-evidence.md table; istio is the 4th surface
  in the wild).
- **YAML multi-document support in `*_path_*` rules** —
  `v0.10 design candidate` per launch-evidence.md
  (`multi_doc_mode:` knob; istio is the named source). See
  pitfall #21 in CONFIG-AUTHORING.md.

---

## Pitfalls catalogued during config authoring

No NEW schema/language pitfalls hit beyond the existing 21
catalogued in `docs/development/CONFIG-AUTHORING.md` (the catalogue
was at 19 entries when this case study was originally written;
istio's two newly-surfaced pitfalls became #20 and #21 in the
v0.9.16/v0.9.17 catalogue update). **Two pitfalls
were rediscovered firsthand** and **two genuinely new pitfalls
surfaced** (now formalised as #20 and #21):

### Rediscovered

1. **License-header regex tolerance for multiple comment styles** —
   the rediscovery confirms pitfall #13 (file-level vs line-level
   anchoring): `(?s)` + non-greedy `.{0,N}?` between anchor strings
   is the canonical pattern when multiple comment shapes coexist.
   istio extends the pattern with an optional year capture
   `Copyright (?:\d{4} )?Istio Authors` because istio's actual
   headers freely interpolate the year while the canonical template
   in `common/scripts/copyright-banner-go.txt` is bare. Already
   documented; no schema gap.

2. **YAML `*_path_equals` against `[*]` semantics** (pitfall #17) —
   rediscovered while drafting the chart-hub-pin rules. The cleanest
   workaround for "every chart that declares hub: uses the same
   literal" is `file_content_matches` against the YAML text, which
   side-steps the `[*]`-each-must-equal trap. The v0.10+
   `*_path_contains` set-membership shorthand and the
   `cross_file_value_equals` primitive together would close this
   gap declaratively.

### NEW pitfalls (now formalised as #20 and #21 in the catalogue)

20. **Cross-file value-equality across structurally-different files
    requires per-file value extraction** — istio's per-chart
    `_internal_defaults_do_not_set.hub` lives at one JSONPath in
    ztunnel's values.yaml (top-level under
    `_internal_defaults_do_not_set`) but at a deeper path in
    istio-control/istio-discovery's values.yaml (under
    `_internal_defaults_do_not_set.global`). A future
    `cross_file_value_equals` primitive can't assume one JSONPath
    across all files; it needs a per-file-pattern `value_extractor:`
    block. **Now formally documented as pitfall #20 in
    CONFIG-AUTHORING.md**, with the `value_extractor:` refinement
    captured as a `v0.10 design candidate` in launch-evidence.md
    (istio is the named source). Workaround used in this config:
    5 separate `file_content_matches` rules asserting the literal
    text appears in each chart's values.yaml. **NOT YET FIXED IN
    ENGINE**; v0.10 ship target.

21. **`yaml_path_*` rules emit "more than one document is not
    supported" runtime error per multi-document YAML file** — the
    engine's `serde_yaml::from_str::<Value>` single-document call
    rejects YAML files with `---` document separators. The error
    surfaces as one per-file violation regardless of which sub-
    document the rule's path would have matched. **Surfaced first
    by istio**; **now formally documented as pitfall #21 in
    CONFIG-AUTHORING.md**, with the `multi_doc_mode:` knob
    captured as a `v0.10 design candidate` in launch-evidence.md
    (istio is the named source). Hit at runtime against
    `releasenotes/notes/50328.yaml` (a legitimate two-document
    file collapsing two related changes into one PR-numbered
    release-note entry). **NOT YET FIXED IN ENGINE**; v0.10 ship
    target. Mitigations: (a) treat as a known benign violation
    and `# noqa`-style suppress per-file (not yet supported);
    (b) wait for the v0.10 `multi_doc_mode:` knob; (c) workaround
    — pre-split the file into single-doc form before alint runs
    (defeats the purpose).

The `coverage_audit_examples_parse.rs` audit catches neither: both
are runtime-semantic, not parse-build, errors. The
`crates/alint-e2e/fixtures/smoke/` smoke-test infrastructure (Phase
7 of v0.9.15) is the right venue for adding fixtures — a
representative multi-doc YAML file + an `expected.toml` declaring
the canonical violation count would catch any future regression in
the engine's multi-doc handling.

---

## Validation status (2026-05-07)

- alint version: **0.9.17** (1dbd9b218a0e, built 2026-05-07).
- `validate-config`: **65 rules loaded cleanly** (28 istio-
  specific + 37 from 4 bundled rulesets — `oss-baseline=15`,
  `go=8`, `ci/github-actions=3`, `hygiene/no-tracked-artifacts=11`).
- Live-tree recheck: **pending** — `/tmp/istio-istio/` not
  present in this validation env.
- Pitfalls fixed in v0.9.17 that touch this config:
  - **Pitfall #18** (per-rule `respect_gitignore: false`)
    — DELIVERED but not used in this config.
  - **Pitfall #19** (literal_is_nested runtime guard) —
    DELIVERED but not used.
- **Open gaps with active workarounds (NOT YET FIXED in
  v0.9.17):**
  - **Pitfall #20** — cross-file value-equality across
    structurally-different files. Workaround: 5 separate
    `file_content_matches` rules. Engine fix targeted for
    v0.10 via `value_extractor:` block on
    `cross_file_value_equals` (see launch-evidence.md
    "v0.10 design candidates"; istio is the named source).
  - **Pitfall #21** — `yaml_path_*` multi-document YAML
    failure. Workaround: pre-split or accept per-file
    runtime violation. Engine fix targeted for v0.10 via
    `multi_doc_mode:` knob (see launch-evidence.md "v0.10
    design candidates"; istio is the named source).

## Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **`nested_configs: true` for the per-component subtree.**
   istio's per-component subdirs (pilot/, cni/, istioctl/,
   operator/, security/, tools/) are effectively peer
   subprojects under one root go.mod. A subtree-scoped
   `.alint.yml` under `manifests/charts/` (for the chart
   discipline) and `releasenotes/notes/` (for the release-note
   schema) would let those rules live next to their domain
   instead of in the root config — particularly relevant
   because the chart-shape rules currently repeat per chart,
   and a single subtree config under `manifests/charts/`
   would express the contract once and apply it to all 9
   charts.
2. **`compliance/apache-2@v1` overlay** — istio is Apache
   2.0 licensed and ships a `licenses/` tree (the
   `lint-licenses` Make target points at it). The bundled
   `compliance/apache-2@v1` ruleset (3 rules — LICENSE
   present + NOTICE present + per-file SPDX header) would
   partially replace `istio-go-license-header` +
   `istio-shell-license-header` with declarative shape
   coverage. The year-extractor istio adds (`Copyright
   (?:\d{4} )?Istio Authors`) is istio-specific and the
   bundled ruleset doesn't carry it; a future
   `compliance/apache-2-istio` derivative could fold it in.
3. **v0.9.6+ rule kinds replacing `command:` shellouts.**
   Of istio's 7 `command:` shellouts (`golangci-lint`,
   `gofmt`, `go mod tidy`, `helm lint`, `hadolint`,
   `shellcheck`, `yamllint`, `license-lint`), `helm lint`
   is the most interesting candidate for a future bundled
   replacement: launch-evidence.md lists `cncf/owners@v1`
   on the v0.10 design table (helm is the source); a
   sibling `helm/chart-structure@v1` overlay would fold
   the per-chart shape pinning that this case study
   currently does inline. Worth proposing for v0.10/v0.11.
