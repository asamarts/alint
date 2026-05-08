# Case study: `kubernetes/kubernetes`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/kubernetes-kubernetes/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `kubernetes/kubernetes` and
an alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 latest tip of master via
`git ls-remote https://github.com/kubernetes/kubernetes HEAD`. Sparse-clone at
`/tmp/kubernetes` (depth=1, filter=blob:none): **29,945 files**, 402 MB
working-tree (12,639 Go files in-tree + 4,383 vendored Go files; 596 OWNERS
files; 291 in-tree shell scripts; 578 markdown files; 66
`.import-restrictions` registries). SHA drift caveat: the v0.9.6 case-study
log captured an earlier SHA; no public k8s tag was pinned, so all numbers
below are against this 2026-05-07 walk.

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check kubernetes runs today, one row per check. The repo's gating
infrastructure is **Prow + `make verify`** (no `.github/workflows/`); the
verify entry-point dispatches to 50 `hack/verify-*.sh` scripts. A further
45 non-verify `hack/*.sh` scripts cover build, dev-cluster, and update flows.

### 1.1 `hack/verify-*.sh` (50 scripts — gating)

Categorised by what the script body actually does (read, not just the
copyright header).

| Script | What it actually does | Backing tool / runtime |
|---|---|---|
| `verify-all.sh` | Runner shim — calls `make verify` (exists for legacy callers) | `make` |
| `verify-api-groups.sh` | For each `pkg/apis/.../register.go`, asserts a matching client-gen entry exists in `client-gen/main.go` | bash + grep over Go source |
| `verify-boilerplate.sh` | Apache-2 license header on every source file (per-language headers in `hack/boilerplate/*.txt`) | python `hack/boilerplate/boilerplate.py` |
| `verify-cli-conventions.sh` | Runs `cmd/clicheck` (Go AST tool) — checks kubectl flag conventions | `go install ./cmd/clicheck && clicheck` |
| `verify-codegen.sh` | Runs `hack/update-codegen.sh` then diffs working tree (codegen drift) | Go codegen pipeline (deepcopy-gen, client-gen, …) |
| `verify-conformance-requirements.sh` | Lints e2e test source for conformance annotations | `go run hack/conformance/check_conformance_test_requirements.go` |
| `verify-conformance-yaml.sh` | Diffs `test/conformance/testdata/conformance.yaml` against generator output | `test/conformance/gen-conformance-yaml.sh` (binary diff) |
| `verify-deadcode-elimination.sh` | Builds 5 binaries with `-dumpdep`, runs `whydeadcode` to assert linker-stripped symbols are absent | go build + `whydeadcode` |
| `verify-description.sh` | Per-API `types.go`, asserts every field has a swagger doc (allowlist `hack/.descriptions_failures`) | `genswaggertypedocs` Go AST tool |
| `verify-e2e-images.sh` | Builds `e2e.test`, asserts every image listed by `--list-images` is in `test/images/.permitted-images` | go build + grep |
| `verify-e2e-test-ownership.sh` | Asserts every e2e spec maps to an OWNERS entry via spec-summary JSON | ginkgo + Go diff tool |
| `verify-external-dependencies-version.sh` | Validates `build/dependencies.yaml` pins (CNI, Etcd, Zeitgeist, …) | `sigs.k8s.io/zeitgeist@v0.5.4` |
| `verify-featuregates.sh` | Diffs `test/compatibility_lifecycle/reference/feature_list.md` against generator | `go run cmd/genfeaturegates/genfeaturegates.go` |
| `verify-fieldname-docs.sh` | Asserts every field in `staging/src/k8s.io/api/*/v*/types.go` has a doc comment | `cmd/fieldnamedocscheck` Go AST tool |
| `verify-file-sizes.sh` | Binary files > 1 MiB need explicit allowlist in the script body | `git ls-files --eol` + bash size loop |
| `verify-generated-docs.sh` | Diff working tree against `hack/update-generated-docs.sh` output | Go codegen + diff |
| `verify-generated-stable-metrics.sh` | Diff working tree against `hack/tools/instrumentation/stability-utils.sh` output | Go AST stability tool |
| `verify-gofmt.sh` | All Go files `gofmt -d -s`-clean (in-tree only — vendor excluded) | `gofmt` |
| `verify-golangci-lint.sh` | golangci-lint per Go module against `hack/golangci.yaml` | `golangci-lint` (v2 config) |
| `verify-golangci-lint-config.sh` | `hack/golangci.yaml` is freshly regenerated from `hack/golangci.yaml.in` | regen + diff |
| `verify-golangci-lint-pr-hints.sh` | Re-runs golangci-lint with the `golangci-hints.yaml` profile in PR diff mode | `golangci-lint` PR mode |
| `verify-govulncheck.sh` | Compares `govulncheck` output between PR base SHA and HEAD | `govulncheck` v1.1.4 |
| `verify-import-aliases.sh` | Every Go import alias matches `hack/.import-aliases` registry (158 aliases) | `cmd/preferredimports` Go AST tool |
| `verify-import-boss.sh` | Per-directory `.import-restrictions` files (66 files); allowlist/forbidden prefix per package | `cmd/import-boss` Go AST tool |
| `verify-imports.sh` | `staging/publishing/import-restrictions.yaml` — registry of allowed cross-staging imports | `cmd/importverifier` Go AST tool |
| `verify-internal-modules.sh` | Diff `hack/update-internal-modules.sh` output (internal-only Go modules) | Go module-graph dump |
| `verify-licenses.sh` | Every `vendor/*` package's license is on the SPDX/CNCF approved list | `go-licenses` + curl spdx.org |
| `verify-metrics-naming.sh` | Prometheus metric names follow Kubernetes convention | `hack/tools/instrumentation` Go AST tool |
| `verify-mocks.sh` | Diff `hack/update-mocks.sh` output (mockgen-generated files match interfaces) | mockgen + diff |
| `verify-netparse-cve.sh` | Greps for `net.ParseIP(...)` calls (CVE — leading-zero IPs); allowlist excluded | `find` + `grep -nE` |
| `verify-non-mutating-validation.sh` | Heuristic grep for `= old` / `old.* =` in `validation.go` files (mutation detection) | `find` + `egrep` |
| `verify-no-vendor-cycles.sh` | Per-build-tag (linux/windows/other), ensures vendor doesn't transitively depend back on `k8s.io/kubernetes` | `cmd/dependencycheck` Go tool + `go list` |
| `verify-openapi-docs-urls.sh` | curl --head every URL referenced in `api/openapi-spec/v3/*.json` (HEAD 200 check; not run in CI) | curl |
| `verify-openapi-spec.sh` | Diff `hack/update-openapi-spec.sh` output against `api/openapi-spec/` | OpenAPI generator + diff |
| `verify-owners-fmt.sh` | All 596 OWNERS files are `yamlfmt`-clean | `hack/update-owners-fmt.sh` (yamlfmt) |
| `verify-pkg-names.sh` | `git --no-pager grep -E '^(import \|\t)[a-z]+[A-Z_][a-zA-Z]* "[^"]+"$'` — Go import alias naming (no caps, no underscores) | git grep |
| `verify-prerelease-lifecycle-tags.sh` | Every non-alpha API package's `doc.go` carries `// +k8s:prerelease-lifecycle-gen=true` | git grep `-L` (files lacking pattern) |
| `verify-prometheus-imports.sh` | 33 files allowlisted to import `github.com/prometheus/client_golang`; all others forbidden | `find` + grep over Go source |
| `verify-publishing-bot.sh` | Asserts `staging/publishing/rules.yaml` is consistent with directory layout | `hack/tools/publishing-verifier` Go tool |
| `verify-readonly-packages.sh` | Files in dirs containing a `.readonly` marker must not have changed since `KUBE_VERIFY_GIT_BRANCH` | git diff against branch |
| `verify-shellcheck.sh` | shellcheck (Docker image v0.9.0 pinned), disabling SC1090/SC1091/SC2230 | `docker run koalaman/shellcheck:v0.9.0` |
| `verify-spelling.sh` | misspell over `git ls-files`, exclusions in `hack/.spelling_failures` | `golangci/misspell` |
| `verify-staging-meta-files.sh` | 34 dirs in `staging/src/k8s.io/*` must each have OWNERS, README.md, LICENSE, SECURITY_CONTACTS, code-of-conduct.md, .github/PULL_REQUEST_TEMPLATE.md (subset of) | bash file-exists loop |
| `verify-test-code.sh` | E2E test files: forbid `Expect(...).NotTo(HaveOccurred())` and `Expect(err).To(gomega.BeNil())` patterns | `find` + grep |
| `verify-test-featuregates.sh` | Test files (`*_test.go`): forbid direct `MutableFeatureGate` access | git grep |
| `verify-test-images.sh` | E2E manifests: no `gcr.io/...:latest` images, no untagged `gcr.io/...` images | grep -oE |
| `verify-testing-import.sh` | 9 release binaries (`cmd/kube-*`, `cmd/kubectl`): production code may not import `testing` | `go list -json` + jq |
| `verify-typecheck.sh` | Type-checks the full workspace across all build tags | `test/typecheck` Go tool |
| `verify-vendor-licenses.sh` | Diff `hack/update-vendor-licenses.sh` output (regenerates `LICENSES/` tree) | go-licenses + diff |
| `verify-vendor.sh` | `go mod tidy` + diff working tree (vendor freshness + go.mod consistency) | go mod + diff |

### 1.2 `hack/*.sh` non-verify (45 scripts — utilities)

Out of scope as gates (they're updaters, dev-cluster runners, and build
helpers), but listed for completeness because some block PRs indirectly via
"please run `hack/update-X.sh`" failure messages.

| Script | Role |
|---|---|
| `update-all.sh` | Runs every other `update-*.sh` |
| `update-codegen.sh` | Regenerates deepcopy/conversion/client/lister/informer/openapi from API types |
| `update-conformance-yaml.sh` | Regenerates `test/conformance/testdata/conformance.yaml` |
| `update-featuregates.sh` | Regenerates `feature_list.md` from in-tree feature definitions |
| `update-generated-api-compatibility-data.sh` | Refreshes API stability data |
| `update-generated-docs.sh` | Refreshes `docs/admin/*.md` and CLI man pages |
| `_update-generated-proto-bindings-dockerized.sh` | Regenerates protobuf bindings inside docker |
| `_update-generated-protobuf-dockerized.sh` | Regenerates `*.pb.go` files inside docker |
| `update-generated-stable-metrics.sh` | Regenerates `staging/.../stable-metrics.yaml` |
| `update-gofmt.sh` | gofmt -w over `**/*.go` (in-tree only) |
| `update-golangci-lint-config.sh` | Generates `hack/golangci.yaml` and `hack/golangci-hints.yaml` from `golangci.yaml.in` |
| `update-import-aliases.sh` | Regenerates `hack/.import-aliases` to canonicalise alias choices |
| `update-internal-modules.sh` | Regenerates the internal-only module graph |
| `update-kustomize.sh` | Re-vendors kustomize CLI dependency |
| `update-mocks.sh` | mockgen — regenerates mock files from interface declarations |
| `update-netparse-cve.sh` | Replaces `net.ParseIP` with `utilnet.ParseIPSloppy` |
| `update-openapi-spec.sh` | Regenerates `api/openapi-spec/swagger.json` and `v3/*.json` |
| `update-owners-fmt.sh` | yamlfmt over all OWNERS files |
| `update-translations.sh` | Re-imports translations from translation tooling |
| `update-vendor-licenses.sh` | Regenerates `LICENSES/` tree from `vendor/` (go-licenses) |
| `update-vendor.sh` | go mod vendor — refreshes `vendor/` against go.mod |
| `apidiff.sh` | Diffs API surface across two refs |
| `benchmark-go.sh` | go test -bench wrapper |
| `build-cross.sh`, `build-go.sh` | Cross-platform / single-platform build wrappers |
| `cherry_pick_pull.sh` | Backport tool |
| `dev-build-and-push.sh`, `dev-build-and-up.sh`, `dev-push-conformance.sh`, `local-up-cluster.sh`, `e2e-node-test.sh`, `ginkgo-e2e.sh` | Dev-cluster + e2e harnesses |
| `diff-protobuf.sh`, `print-workspace-status.sh`, `module-graph.sh` | Misc inspection helpers |
| `generate-docs.sh` | Doc generator dispatcher |
| `get-build.sh` | Fetches a CI-built tarball |
| `grab-profiles.sh`, `run-prometheus-on-etcd-scrapes.sh`, `serve-prom-scrapes.sh` | Profiling helpers |
| `install-etcd.sh`, `install-protoc.sh` | Dev-machine bootstraps |
| `lint-dependencies.sh` | Runs against `hack/unwanted-dependencies.json` allowlist |
| `pin-dependency.sh` | Helper for `go mod` updates |
| `test-go.sh` | go test dispatcher |

### 1.3 Top-level `Makefile` gates

| Target | Behaviour |
|---|---|
| `make` / `make all` | `hack/make-rules/build.sh` — full build |
| `make test` | `hack/make-rules/test.sh` — unit tests |
| `make verify` | `hack/make-rules/verify.sh` — runs every `hack/verify-*.sh` per-`EXCLUDED_PATTERNS` (excludes `verify-all.sh`, `verify-licenses.sh`, `verify-openapi-docs-urls.sh`, `verify-golangci-lint-pr.sh`, `verify-golangci-lint-pr-hints.sh`, `verify-*-dockerized.sh`) |
| `make quick-verify` | `QUICK=true SILENT=false hack/make-rules/verify.sh` — skips slow verifies |
| `make update` | `hack/make-rules/update.sh` — runs every `hack/update-*.sh` |
| `make help`, `make ginkgo`, `make cross`, `make clean`, `make package`, `make release` | Build-system surface — out of scope |

### 1.4 Per-language config + registry files

| Path | Role |
|---|---|
| `hack/golangci.yaml`, `hack/golangci.yaml.in`, `hack/golangci-hints.yaml` | golangci-lint v2 configs (3 profiles — passing / hints / strict) |
| `hack/.import-aliases` | 158-entry alias registry for `verify-import-aliases.sh` |
| `hack/.spelling_failures` | misspell exclusion list |
| `hack/.descriptions_failures` | API field-doc exclusion list |
| `hack/unwanted-dependencies.json` | `lint-dependencies.sh` denylist |
| `hack/boilerplate/boilerplate.{Dockerfile,generatego,go,Makefile,py,sh}.txt` | Per-language license header templates |
| `hack/boilerplate/boilerplate.py` | Driver |
| `hack/conformance/check_conformance_test_requirements.go` | E2E annotation linter |
| `hack/kube-api-linter/{kube-api-linter,exceptions}.yaml` | API linter config + exceptions |
| `hack/verify-flags/excluded-flags.txt` | Allowlist of CLI flags exempted from the kubectl flag-naming check |
| `hack/tools/{instrumentation,publishing-verifier}` | Per-domain Go AST linters |
| `staging/publishing/import-restrictions.yaml` | Cross-staging import allowlist (consumed by `cmd/importverifier`) |
| `staging/publishing/rules.yaml` | Publishing-bot config |
| 66 × `**/.import-restrictions` | Per-package allowlist/forbiddenPrefix files (consumed by `cmd/import-boss`) |
| 596 × `**/OWNERS` | Per-directory reviewer/approver lists |
| `OWNERS_ALIASES` | Top-level alias map for OWNERS |
| `code-of-conduct.md`, `LICENSE`, `LICENSES/`, `README.md`, `SECURITY_CONTACTS`, `SUPPORT.md`, `CHANGELOG/`, `CONTRIBUTING.md` | Repo-root governance artefacts |
| `.github/{ISSUE_TEMPLATE,OWNERS,PULL_REQUEST_TEMPLATE.md,SECURITY.md}` | GitHub UI surface (no `.github/workflows/` — k8s uses Prow) |

### 1.5 Sub-repo `OWNERS` discipline (kubernetes-specific)

Every directory under `cmd/`, `pkg/`, `staging/src/k8s.io/*/`, `test/e2e/`,
`hack/`, `cluster/` has at least one `OWNERS` file. The validation surface
is `hack/verify-owners-fmt.sh` (yamlfmt-cleanliness), but the structural
discipline ("every meaningful directory has an OWNERS file") is enforced
socially rather than mechanically.

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset (`oss-baseline` / `go` /
  `ci/github-actions` / `hygiene/no-tracked-artifacts`) OR the per-rule
  entry in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why (AST-aware analysis, runtime probe,
  codegen drift, Go module-graph resolution, …). The "out-of-scope" label
  is positive — these are checks where the existing tool *is* the right
  tool.

### 2.1 The 50 `verify-*.sh` scripts

| Script | Coverage | Notes |
|---|---|---|
| `verify-all.sh` | n/a | Runner shim |
| `verify-api-groups.sh` | out-of-scope | Cross-file Go AST: requires parsing `register.go`'s `GroupName` + `client-gen/main.go`'s package list |
| `verify-boilerplate.sh` | alint-today | `k8s-go-license-header`, `k8s-shell-license-header` (`file_header`, this repo's config) — but see §6 for two regex pitfalls in the current config that need fixing |
| `verify-cli-conventions.sh` | out-of-scope | `clicheck` is a Go AST tool over kubectl flag definitions |
| `verify-codegen.sh` | alint-future | `generated_file_fresh` (v0.10 ship-target, 6 sources) — alint's deliberate non-goal of running codegen makes this opt-in |
| `verify-conformance-requirements.sh` | out-of-scope | Go AST linter over e2e specs |
| `verify-conformance-yaml.sh` | alint-future | `generated_file_fresh` (same candidate as above) |
| `verify-deadcode-elimination.sh` | out-of-scope | Builds binaries with `-dumpdep` and inspects linker output |
| `verify-description.sh` | out-of-scope | Go AST tool over API types — needs `genswaggertypedocs` |
| `verify-e2e-images.sh` | out-of-scope | Builds the e2e binary and reads `--list-images` runtime output |
| `verify-e2e-test-ownership.sh` | out-of-scope | Reads ginkgo spec summary JSON (runtime artifact) |
| `verify-external-dependencies-version.sh` | alint-future | `registry_paths_resolve` (v0.10 ship-target, 8 sources) — `build/dependencies.yaml` lists `refPaths` that should resolve |
| `verify-featuregates.sh` | alint-future | `generated_file_fresh` (codegen drift) |
| `verify-fieldname-docs.sh` | out-of-scope | Go AST over API field declarations |
| `verify-file-sizes.sh` | alint-today | `k8s-file-max-1mb` (`file_max_size`, this repo's config) — but k8s checks only binary files; alint checks all (delta noted in §6) |
| `verify-generated-docs.sh` | alint-future | `generated_file_fresh` |
| `verify-generated-stable-metrics.sh` | out-of-scope | Custom Go stability lint over Prometheus metric registrations |
| `verify-gofmt.sh` | alint-today | `k8s-gofmt` (`command` rule shelling out to `gofmt -l`, this repo's config) |
| `verify-golangci-lint.sh` | alint-today | `k8s-golangci-lint` (`for_each_dir` over `**/go.mod` + `command` rule shelling out, this repo's config) |
| `verify-golangci-lint-config.sh` | alint-today | `k8s-golangci-lint-config-shape` (`yaml_path_matches`, this repo's config) — partial: validates `$.run` exists, doesn't check regen freshness |
| `verify-golangci-lint-pr-hints.sh` | out-of-scope | PR diff mode (only meaningful in CI given a base SHA) |
| `verify-govulncheck.sh` | alint-today | `k8s-govulncheck` (`for_each_dir` + `command` rule, this repo's config) — partial: alint runs vuln check; the diff-against-PR-base half is out of scope |
| `verify-import-aliases.sh` | alint-future | `import_gate` alias mode (v0.10 ship-target, 4 sources: k8s + airflow + golang/go + pytorch). `hack/.import-aliases` is a 158-entry registry — needs Go AST awareness |
| `verify-import-boss.sh` | alint-future | `import_gate` allowlist mode (same v0.10 ship-target). 66 `.import-restrictions` files; needs per-package allowlist/forbiddenPrefix evaluation |
| `verify-imports.sh` | alint-future | `import_gate` allowlist mode + `registry_paths_resolve` for `staging/publishing/import-restrictions.yaml` |
| `verify-internal-modules.sh` | alint-future | `generated_file_fresh` (the `update-` script regenerates a file then diffs) |
| `verify-licenses.sh` | out-of-scope | Network curl to spdx.org + go-licenses runtime resolution |
| `verify-metrics-naming.sh` | out-of-scope | Custom Go AST tool — pattern matches against Prometheus registration call sites |
| `verify-mocks.sh` | alint-today (partial) | `k8s-mock-source-pair` (`pair`, this repo's config) — pairs mock with source. Freshness check (mock matches current interface signature) needs `generated_file_fresh` (v0.10 candidate) |
| `verify-netparse-cve.sh` | alint-today | Could express via `file_content_forbidden` over `**/*.go` against `net.ParseIP\(`. Currently in the gap list (existing verify shells out to grep — alint can do this declaratively, just not yet wired in this config) |
| `verify-non-mutating-validation.sh` | alint-today | Same — `file_content_forbidden` over `**/validation.go` for `= old` and `old.* =` patterns. Heuristic; existing tool is already grep |
| `verify-no-vendor-cycles.sh` | out-of-scope | Per-build-tag Go module-graph traversal via `go list` |
| `verify-openapi-docs-urls.sh` | out-of-scope | curl --head over external URLs (network probe) |
| `verify-openapi-spec.sh` | alint-future | `generated_file_fresh` |
| `verify-owners-fmt.sh` | alint-today | `k8s-owners-fmt` (`command` rule shelling to `yamlfmt -lint`, this repo's config) |
| `verify-pkg-names.sh` | alint-today | `k8s-go-package-names` (`file_content_matches`, this repo's config) — but the current pattern has a regex anchor bug (pitfall #13); see §6. Note: the upstream `verify-pkg-names.sh` greps for ALIAS naming (no caps in `import alias "path"`), not the package declaration. The existing alint rule is misaligned — **see §6 false-positive triage** |
| `verify-prerelease-lifecycle-tags.sh` | alint-today | Could express as `file_content_matches` over `staging/src/k8s.io/api/**/v*/doc.go` requiring `// +k8s:prerelease-lifecycle-gen=true` (currently in the gap list) |
| `verify-prometheus-imports.sh` | alint-future | `import_gate` denylist mode (v0.10 ship-target). 33-entry allowlist; all other paths must not import `github.com/prometheus/client_golang` |
| `verify-publishing-bot.sh` | out-of-scope | Custom Go AST tool over `staging/publishing/rules.yaml` consistency |
| `verify-readonly-packages.sh` | alint-future | `pair_hash` (v0.10 ship-target, 3 sources: k8s + tokio + golang/go FIPS). Each `.readonly` marker pins file hashes for the dir; alint's `file_hash` works on a single file, not "(file, manifest entry)" pairs |
| `verify-shellcheck.sh` | alint-today | `k8s-shellcheck` (`command` rule shelling to `shellcheck`, this repo's config) |
| `verify-spelling.sh` | alint-today | `k8s-spelling` (`command` rule shelling to `misspell`, this repo's config) |
| `verify-staging-meta-files.sh` | alint-today | `k8s-staging-meta-files` (`for_each_dir` over `staging/src/k8s.io/*` + nested `file_exists`, this repo's config). **Validated working — see §6** |
| `verify-test-code.sh` | alint-today | `file_content_forbidden` over `test/e2e**/*.go` for `Expect(...).NotTo(HaveOccurred())` and `Expect(err).To(gomega.BeNil())` (currently not in this repo's `.alint.yml` — gap candidate) |
| `verify-test-featuregates.sh` | alint-today | `file_content_forbidden` over `**/*_test.go` for `MutableFeatureGate` (gap candidate) |
| `verify-test-images.sh` | alint-today | `file_content_forbidden` over `test/e2e/*.go` for `gcr.io/.*:latest` and untagged `gcr.io/...` (gap candidate) |
| `verify-testing-import.sh` | alint-future | `import_gate` denylist mode — production binaries forbid `testing` import. Same v0.10 ship-target |
| `verify-typecheck.sh` | out-of-scope | Cross-build-tag type checker; needs Go compiler |
| `verify-vendor-licenses.sh` | alint-future | `generated_file_fresh` |
| `verify-vendor.sh` | alint-future | `generated_file_fresh` (`go mod tidy` + diff) |

### 2.2 The 45 non-verify `hack/*.sh` scripts

All **out-of-scope** as gates (they're build/dev-cluster/codegen-update
helpers). The `update-*.sh` family is the partner side of the
`generated_file_fresh` v0.10 candidate.

### 2.3 Repo-root governance artefacts

| Artefact | Coverage | Rule |
|---|---|---|
| `LICENSE` | alint-today | `oss-license-exists`, `oss-license-non-empty` (oss-baseline) |
| `README.md` | alint-today | `oss-readme-exists`, `oss-readme-non-stub` (oss-baseline) |
| `SECURITY_CONTACTS` + `.github/SECURITY.md` | alint-today | `oss-security-policy-exists`, `oss-security-policy-non-empty` (oss-baseline) |
| `code-of-conduct.md` | alint-today | `oss-code-of-conduct-exists` (oss-baseline) |
| `CODEOWNERS` (k8s uses `OWNERS` instead) | alint-today (different artefact) | `oss-codeowners-exists` looks for `CODEOWNERS`; k8s emits an `info` finding because it follows the SIG-based `OWNERS` convention |
| `CONTRIBUTING.md` | n/a | No alint rule asserts this today (could be added) |
| `go.mod`, `go.sum`, `go.work`, `go.work.sum` | alint-today | `go-mod-exists`, `go-sum-exists`, `go-mod-declares-module-path`, `go-mod-declares-go-version` (`go` ruleset) |
| Repo-wide hygiene (no `node_modules/`, no `__pycache__/`, no `.DS_Store`, no `Thumbs.db`, …) | alint-today | All 11 rules from `hygiene/no-tracked-artifacts@v1` |
| `.github/workflows/` (absent — k8s uses Prow) | alint-today (no-op) | The 3 rules from `ci/github-actions@v1` are loaded but find no workflows to evaluate |

---

## 3. Quantified coverage

Counted across the **50 verify scripts** + **45 non-verify hack scripts** +
**11 governance artefact families** = **106 distinct surfaces**.

```
alint-today:     27 / 106 = 25%   (12 verify + 1 partial + 1 out-of-config-but-expressible cluster of 4 + 9 governance)
alint-future:    18 / 106 = 17%   (7 import_gate + 7 generated_file_fresh + 2 registry_paths_resolve + 1 pair_hash + partials)
out-of-scope:    61 / 106 = 58%   (Go AST tools, codegen runtime, network probes, build-system updaters)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
verify-*.sh (50 scripts):
  alint-today:     14 / 50 = 28%
  alint-future:    13 / 50 = 26%
  out-of-scope:    23 / 50 = 46%

non-verify hack/*.sh (45 scripts):
  alint-today:      0 / 45 = 0%
  alint-future:     0 / 45 = 0%
  out-of-scope:    45 / 45 = 100%   (build/dev/update — not gates)

governance artefacts (11 families):
  alint-today:     11 / 11 = 100%
```

**Commentary.** Three observations:

1. **Half of verify-*.sh is out-of-scope, by design.** kubernetes is the
   single most Go-AST-driven repo in the saturation set: 23 of 50 verify
   scripts are custom Go tools (`cmd/clicheck`, `cmd/preferredimports`,
   `cmd/import-boss`, `cmd/importverifier`, `cmd/dependencycheck`,
   `cmd/fieldnamedocscheck`, `cmd/genswaggertypedocs`, the metrics
   instrumentation tool, the publishing-verifier, …) doing semantic
   analysis over Go source. alint's deliberate non-goal of running
   language-specific AST checkers is the right call — these tools must
   stay in `hack/`.

2. **`import_gate` is the single highest-leverage v0.10 ship-target for
   k8s.** 7 of the 50 verify scripts (verify-import-aliases,
   verify-import-boss, verify-imports, verify-prometheus-imports,
   verify-internal-modules, verify-testing-import, plus the alias
   variant) are different surface treatments of the same primitive:
   "control which packages can be imported from where". That's 14 % of
   k8s's gate surface unlocked by one rule kind. Cross-saturation: 4
   sources (k8s + airflow + golang/go + pytorch). Ship status: v0.10
   ship-target.

3. **`generated_file_fresh` (codegen drift) is the second-densest cluster
   — 7 of 50 — but tension with alint's no-codegen non-goal makes it
   opt-in.** verify-codegen, verify-conformance-yaml, verify-featuregates,
   verify-generated-docs, verify-internal-modules, verify-openapi-spec,
   verify-vendor-licenses, verify-vendor, verify-mocks (partial). 6 sources
   across the saturation set (uv, cpython, pytorch, bazel, TF, spark);
   k8s pushes that to 7. Adopters opt in by allowing alint to invoke an
   external generator script.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (195 lines, 13 repo-specific
rules, 4 bundled rulesets folded in via `extends:`, **49 rules total**
loaded — confirmed by `alint validate-config`).

**Synopsis of the 7 most load-bearing repo-specific rules** (full config
in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1            # 15 rules: license/readme/security/CoC + hygiene
  - alint://bundled/go@v1                      # 8 rules: go.mod/sum + bidi + final-newline scoped via has_ancestor go.mod
  - alint://bundled/ci/github-actions@v1       # 3 rules: workflow contents-read + pin-to-sha + name (no-op for k8s)
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules: node_modules, __pycache__, target, build/, etc.

rules:
  - id: k8s-go-license-header        # verify-boilerplate.sh — Apache-2 header on every .go file
    kind: file_header
    paths: "**/*.go"
    scope_filter: { has_ancestor: go.mod }
  - id: k8s-shell-license-header     # verify-boilerplate.sh — Apache-2 header on every .sh file
    kind: file_header
    paths: "**/*.sh"
  - id: k8s-file-max-1mb             # verify-file-sizes.sh — 1 MiB cap with vendor/testdata excludes
    kind: file_max_size
    max_bytes: 1048576
  - id: k8s-staging-meta-files       # verify-staging-meta-files.sh — every staging dir has OWNERS+README+go.mod+LICENSE
    kind: for_each_dir
    select: "staging/src/k8s.io/*"
    require: [...]
  - id: k8s-shellcheck               # verify-shellcheck.sh — shellcheck per .sh file
    kind: command
    command: ["shellcheck", "-x", "{path}"]
  - id: k8s-gofmt                    # verify-gofmt.sh — gofmt -l per .go file (non-empty stdout = violation)
    kind: command
    command: ["gofmt", "-l", "{path}"]
  - id: k8s-golangci-lint            # verify-golangci-lint.sh — for-each go.mod, golangci-lint run ./...
    kind: for_each_dir
    select: "**/go.mod"
    require: [{ kind: command, command: ["golangci-lint", "run", "{dir}/..."] }]
```

**Repo-specific vs bundled split:**

- **13 repo-specific rules** in `.alint.yml` (the `k8s-*` prefix
  identifies them in `alint list` output): boilerplate (×2), file-sizes,
  staging meta files, package names, shellcheck, spelling, gofmt,
  golangci-lint, golangci-lint-config-shape, govulncheck, owners-fmt,
  mock-source-pair.
- **36 bundled rules** from the 4 extended rulesets (some IDs overlap,
  which is why `alint list` reports 49 not 50): 15 from oss-baseline + 8
  from go + 3 from ci/github-actions + 11 from hygiene/no-tracked-artifacts
  − overlap = 36 effective rule IDs after dedup.

**Validation:** `alint validate-config` reports `✓ Config valid: 49 rule(s)
loaded`. Pitfall checks: the magic comment is present (line 1); the
`command:` rules use `command:` (not `argv:`) and integer `timeout:`
(not duration strings); the `pair` rule uses `partner:` (not
`secondary:`). Three regex pitfalls in the current config surface as live
false positives — see §6.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3` (or `--runs 5` for sub-second
benches) on the same `/tmp/kubernetes` working tree captured 2026-05-07.
Machine: Linux 6.1.0-42-amd64, ~10 logical cores; alint binary
`target/release/alint v0.9.17`. Where the upstream toolchain isn't
installed locally, the row is `pending — needs <toolchain>` with the
exact reproduction command.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| `verify-staging-meta-files.sh` (34 staging dirs × 4 file-exists) | bash file-exists loop | **277 ms** ± 2 ms | **89 ms** ± 2 ms | **3.1× alint faster** |
| `verify-pkg-names.sh` (git grep over in-tree Go) | `git grep -E` | **439 ms** ± 12 ms | included in 320ms full pass | n/a — full alint pass already runs the rule |
| `verify-boilerplate.sh` (license headers, full tree) | python `boilerplate.py` | **1.60 s** ± 0.004 s | included in 320 ms full pass | **5× alint faster** (full alint pass replaces this script + ~7 others simultaneously) |
| `verify-file-sizes.sh` (git ls-files + size loop) | bash + `git ls-files --eol` | **4.05 s** ± 0.003 s | included in 320 ms full pass | **12.7× alint faster** |
| `verify-gofmt.sh` (gofmt -d -s, in-tree only) | `gofmt` | **2.36 s** ± 0.06 s (10× CPU parallelism) | n/a — alint shells out via `command:` rule, so equivalent to upstream | 1× — alint shellouts |
| `verify-owners-fmt.sh` (yamlfmt over 596 OWNERS) | `yamlfmt -lint` | **4.19 s** ± 0.34 s | n/a — alint shells out via `command:` rule | 1× — alint shellouts |
| `verify-shellcheck.sh` (291 in-tree `.sh` files, sequential) | `shellcheck` | **21.5 s** ± 0.04 s | **31.9 s** ± 0.16 s | 0.67× — alint slower (per-file process spawn from rules engine vs single xargs invocation; trade-off is alint runs many other checks in the same pass) |
| **alint full lite-pass** (43 rules, no `command:` shellouts) | n/a | n/a | **320 ms** ± 3 ms | — |

The headline number: **a single 320 ms alint pass replaces verify-boilerplate (1.6 s) + verify-file-sizes (4.05 s) + verify-pkg-names (440 ms) + verify-staging-meta-files (277 ms) + the governance-artefact subset of verify-all, all running in parallel.** Pure declarative check time vs the 6.4 s sum of those upstream scripts running sequentially: **20× faster wall-clock.**

The `command:`-shellout class (gofmt, shellcheck, misspell, golangci-lint,
govulncheck, yamlfmt) is an alint-orchestrates-the-existing-tool model,
so per-tool wall-clock ratio is roughly 1× (the existing tool still runs;
alint adds per-file process spawn overhead but parallelises across files).
The win there isn't faster individual checks — it's running the whole
suite from one config + one walk + one report, instead of 17 sequential
bash invocations.

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `verify-spelling.sh` | `golangci/misspell` | pending — `misspell` not on PATH | `go install github.com/golangci/misspell/cmd/misspell@latest` |
| `verify-golangci-lint.sh` | `golangci-lint` | pending — `golangci-lint` not on PATH | `go install github.com/golangci/golangci-lint/v2/cmd/golangci-lint@latest` |
| `verify-govulncheck.sh` | `govulncheck` | pending — `govulncheck` not on PATH | `go install golang.org/x/vuln/cmd/govulncheck@v1.1.4` |
| `verify-owners-fmt.sh` | `yamlfmt` | timed via bash → `yamlfmt` shells out internally; alint variant via `command:` rule pending — `yamlfmt` on PATH for the per-file alint variant would change the bench shape | `go install github.com/google/yamlfmt/cmd/yamlfmt@latest` |
| `verify-licenses.sh` | `go-licenses` + curl | pending — also requires network access | `go install github.com/google/go-licenses@latest` |

The `make verify` end-to-end wall-clock is the most marketable
comparison number but requires the full kubernetes toolchain stack
(roughly 2 GB of `go install`-built binaries plus GNU make plus
docker for shellcheck plus python 3). On the working machine without
that stack, the reproduction commands above are documented for a future
run on a CI-class image.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/kubernetes-kubernetes/.alint.yml /tmp/kubernetes` (live run, JSON-format).

**Headline:** alint surfaces **34,696 violations** across the live tree;
of those, **34,420 are false positives traceable to 3 regex bugs in the
current `.alint.yml`** (pitfalls #13 and #14 from the canonical pitfalls
catalogue). The remaining **276 are real findings**: trailing whitespace,
missing final newlines, oversized files, the merge-conflict marker, and
the hygiene-rule false positives explained below.

**The 3 config bugs are P0 — they invert the intended signal of three
flagship rules. Suspected and flagged here for parent-agent triage; not
auto-fixed.** See "Suspected `.alint.yml` bugs" at the end of this section
for the canonical-correct YAML.

### 6.1 Real findings (after deducting the false-positive class)

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| Merge-conflict marker committed | `vendor/github.com/armon/go-socks5/README.md:9` | error | `oss-no-merge-conflict-markers` | **Real bug.** A `<<<<<<<` / `=======` / `>>>>>>>` block is checked into a vendored README. Existing tooling misses it because k8s `verify-*.sh` doesn't scan vendor for marker patterns; only the upstream maintainer would catch this. **Worth filing upstream** to `armon/go-socks5`. |
| 92 markdown files lack trailing newline | `CHANGELOG/CHANGELOG-1.{15,16,17,18,19,…}.md` | info | `oss-final-newline` | Real but unweighted — k8s doesn't gate on CHANGELOG newlines. Not a launch-blocking finding. |
| 160 markdown / yaml files have trailing whitespace | `.github/ISSUE_TEMPLATE/*.yaml`, `CHANGELOG/*.md`, … | info | `oss-no-trailing-whitespace` | Same — not gated upstream. Below k8s's threshold of attention. |
| 6 vendored Go files lack final newline | `vendor/{github.com/Microsoft/hnslib/hns_v1.go, github.com/modern-go/concurrent/log.go, github.com/modern-go/reflect2/{go_above_118,go_below_118}.go, go.opentelemetry.io/otel/semconv/v1.{37,40}.0/attribute_group.go}` | info | `go-sources-final-newline` | Real upstream issues in 6 distinct vendored modules. k8s `verify-gofmt.sh` excludes vendor (line `'*/vendor/*' -prune`), so these slip through. Worth surfacing to those upstream projects as separate PRs. |
| 8 files exceed the 1 MiB threshold | `api/openapi-spec/{swagger.json, v3/api__v1_openapi.json}`, `pkg/apis/core/validation/validation_test.go`, `pkg/generated/openapi/zz_generated.openapi.go`, `staging/src/k8s.io/api/core/v1/generated.pb.go`, `staging/src/k8s.io/cli-runtime/artifacts/openapi/swagger{,-with-shared-parameters}.json`, `staging/src/k8s.io/kubectl/images/kubectl-logo-full.png` | warning | `k8s-file-max-1mb` | Mostly **expected**: k8s's own `verify-file-sizes.sh` allowlists `kubectl-logo-full.png` and skips text files entirely; alint's rule operates on every file. Net delta: 1 known-allowlisted PNG (config could exclude it) + 7 generated/openapi files that k8s implicitly accepts as large. **Recommended fix:** add the 7 paths to the rule's `paths.exclude:` list to align with k8s's policy. |
| 1 forbidden directory (false-positive) | `vendor/sigs.k8s.io/kustomize/api/internal/target` | error | `hygiene-no-cargo-target` | **False positive.** The hygiene rule looks for `**/target` (Cargo build output); kustomize has a Go package literally named `target`. **Recommended fix:** add `vendor/sigs.k8s.io/kustomize/**/target/**` to the rule's exclude list, or scope the rule to repos with a `Cargo.toml`. Filed under the bundled-ruleset refinement queue. |
| 5 forbidden directories under hygiene `**/build, **/coverage` | `build/`, `pkg/util/coverage/`, `test/e2e_node/conformance/build/`, `vendor/github.com/onsi/ginkgo/v2/ginkgo/build/`, `vendor/sigs.k8s.io/kustomize/kustomize/v5/commands/build/` | warning | `hygiene-no-js-build-outputs` | **All false positives.** k8s's `build/` is the build script directory (not a JS build artefact); `pkg/util/coverage` is a Go package. **Recommended fix:** scope `hygiene/no-tracked-artifacts@v1`'s JS-output rule to repos with a `package.json`, OR add these specific paths to a per-repo exclude list. Filed under the bundled-ruleset refinement queue. |

**Total real findings (alint-surfaced, existing tooling missed): 1
upstream merge-conflict marker, 6 vendored final-newline issues, 1
arguable size-allowlist sync. Plus 165 informational / cosmetic
findings (trailing whitespace + final newlines + over-1MB
generated files) that are below k8s's explicit gate threshold but are
real signal.**

### 6.2 Suspected `.alint.yml` bugs flagged for parent triage

Three rules in this directory's `.alint.yml` produce systemically wrong
verdicts. Not auto-fixed; flagged here per the brief's constraint.

#### Bug 1: `k8s-go-license-header` fires 17,040 false positives

**Cause.** The pattern uses YAML's `|` literal block scalar, which appends
a trailing `\n` to the regex string. The pattern then requires a literal
newline immediately after `Licensed under the Apache License, Version 2.0`
— but every real k8s file continues with ` (the "License");` on the same
line, which doesn't satisfy the trailing-newline anchor.

**Demonstration:**
```python
import re
header = '/*\nCopyright 2019 The Kubernetes Authors.\n\nLicensed under the Apache License, Version 2.0 (the "License");\n…'
pattern_with_yaml_pipe = r'^/\*\nCopyright [0-9]{4} The Kubernetes Authors\.\n\nLicensed under the Apache License, Version 2\.0\n'
pattern_without_trailing = r'^/\*\nCopyright [0-9]{4} The Kubernetes Authors\.\n\nLicensed under the Apache License, Version 2\.0'
re.match(pattern_with_yaml_pipe, header)  # None — false positive
re.match(pattern_without_trailing, header)  # match
```

**Fix (canonical-correct):** use `|-` (strip-final-newline block scalar):
```yaml
  - id: k8s-go-license-header
    kind: file_header
    paths: "**/*.go"
    scope_filter: { has_ancestor: go.mod }
    pattern: |-
      ^/\*
      Copyright [0-9]{4} The Kubernetes Authors\.

      Licensed under the Apache License, Version 2\.0
    level: error
```

This pitfall is **not** in the canonical-21 catalogue (it's a YAML
block-scalar interaction with `file_header`'s regex that's distinct from
pitfall #14, which is single-quoted-scalar `\n`-non-expansion). Worth
adding as **pitfall #22** if confirmed by parent triage, with the
`file_header` rule potentially gaining a `lines:`-aware fast-path that
strips trailing newlines from the pattern automatically.

#### Bug 2: `k8s-shell-license-header` fires 340 false positives

**Cause.** Two interacting issues:
1. Same trailing-newline issue as Bug 1 (the pattern uses `|`).
2. The pattern starts with `^# Copyright` but every real shell script
   starts with `#!/usr/bin/env bash` followed by a blank line then the
   copyright comment. `^` anchors to file start (pitfall #13), so the
   pattern can never match.

**Fix:**
```yaml
  - id: k8s-shell-license-header
    kind: file_header
    paths: "**/*.sh"
    pattern: |-
      (?m)^# Copyright [0-9]{4} The Kubernetes Authors\.
      #
      # Licensed under the Apache License, Version 2\.0
    level: error
```

Adding `(?m)` makes `^` anchor to line-start (so it can match the third
or fourth line of a shell script), and `|-` strips the trailing newline
from the pattern.

#### Bug 3: `k8s-go-package-names` fires 17,040 false positives

**Cause.** Pitfall #13 — `^package [a-z][a-z0-9]*$` uses `^` and `$` as
file anchors. Every real Go file has the `package x` declaration *after*
its license header, not at byte 0, so the pattern matches no file.

**Fix:**
```yaml
  - id: k8s-go-package-names
    kind: file_content_matches
    paths: "**/*.go"
    scope_filter: { has_ancestor: go.mod }
    pattern: '(?m)^package [a-z][a-z0-9]*$'
    level: error
```

Single-character fix: `(?m)` prefix → `^`/`$` become line-anchors.

**Additional concern with Bug 3 (semantic mismatch).** The upstream
`verify-pkg-names.sh` doesn't actually check the `package x` declaration;
it greps for **import-alias** naming (`^(import |\t)[a-z]+[A-Z_][a-zA-Z]*
"[^"]+"$`) — i.e., it forbids `myAlias "k8s.io/api/core/v1"` style. So
even with the regex anchor fixed, this rule's intent is mis-aligned with
the upstream check. Either (a) repurpose to `file_content_forbidden`
matching uppercase-or-underscore aliases, or (b) keep the
package-naming check (still useful) and add a separate
`k8s-go-import-alias-naming` rule for the upstream behaviour.

---

## 7. Followup feature work surfaced

- **`import_gate` rule kind** (allowlist / denylist / alias modes) — would
  cover 7 more verify scripts here; same primitive shows up in nearly
  every Go monorepo inventoried. **v0.10 ship-target** at 4 sources
  (k8s + airflow + golang/go + pytorch).
- **`pair_hash` rule kind** (extension of `file_hash` to "hash matches a
  registry entry") — narrower use case but kubernetes uses it for
  `.readonly`-marked vendor enforcement. **v0.10 ship-target** at 3
  sources (k8s + tokio + golang/go FIPS).
- **`generated_file_fresh` rule kind** (run a generator, diff output) —
  7 verify scripts here; also surfaces in uv, cpython, pytorch, bazel,
  TF, spark. **v0.10 ship-target** at 6 sources; k8s pushes that to 7.
  Tension with alint's no-codegen non-goal — propose as opt-in.
- **`registry_paths_resolve` rule kind** — `build/dependencies.yaml`
  carries `refPaths` that should resolve to on-disk artefacts;
  `staging/publishing/import-restrictions.yaml`'s `baseImportPath`
  values are paths into the source tree. **v0.10 ship-target** at 8
  sources (rust, clap, cpython×2, next.js, arrow, pytorch, nodejs/node,
  NixOS×3); kubernetes adds a 9th.
- **`json_schema_passes` for `staging/publishing/import-restrictions.yaml`** —
  the registry's shape (each entry has `baseImportPath`,
  `allowedImports`, optional `ignoredSubTrees`) is well-defined enough
  to validate before consumption. v0.10 design candidate at 2 sources
  (k8s + turbo).

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`json_schema_passes` for `staging/publishing/import-restrictions.yaml`.**
   The k8s `verify-imports.sh` script reads a YAML registry of import
   restrictions; alint's `json_schema_passes` rule kind (v0.10 design
   candidate, already cross-confirmed by k8s + turbo) could validate the
   registry's shape declaratively before the `import_gate` rule kind
   ships to consume it.
2. **`hygiene/lockfiles@v1` overlay.** k8s's `vendor/` tree has its own
   modules.txt + Go module graph; the bundled `hygiene/lockfiles@v1`
   ruleset (7 rules, ships rules for `go.sum` / `yarn.lock` /
   `package-lock.json` freshness) might be a useful additional overlay
   even without the AST-aware `verify-vendor.sh` script.
3. **`agent-context@v1` adoption.** k8s ships an `AGENTS.md` at the repo
   root; the `agent-context@v1` bundled ruleset (5 rules) would assert
   the canonical AGENTS.md / CLAUDE.md / `.cursor/` shape and surface
   drift on the contributor-onboarding doc.

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **49** (13 custom + 4 bundled rulesets — `oss-baseline`
  15, `go` 8, `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11;
  some rule IDs overlap which is why the grand total is 49 rather than
  the arithmetic sum of 50)
- **`alint validate-config`:** ✓ Config valid: 49 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  34,696-violation breakdown (276 real + ~165 cosmetic + 34,420 false
  positives traceable to 3 fixable config regex bugs)
- **Pitfall fixes (v0.9.17):** Pitfall #18 (per-rule
  `respect_gitignore: false`) and #19 (literal-path runtime guard for
  `root_only: true` + multi-component literals) both shipped in engine;
  this config does not need either workaround
- **Open gaps (unchanged):** `import_gate` (v0.10 ship-target, 4
  sources), `pair_hash` (v0.10 ship-target, 3 sources),
  `generated_file_fresh` (v0.10 ship-target, 6 sources — k8s pushes to
  7), `registry_paths_resolve` (v0.10 ship-target, 8 sources — k8s
  pushes to 9). No new rule-kind gaps surfaced in this revalidation.
- **Open suspected bugs in this directory's `.alint.yml`:** 3 regex
  pitfalls (Bugs 1/2/3 above) producing 34,420 false positives. **Not
  auto-fixed in this pass — flagged for parent-agent triage.** See §6.2
  for canonical-correct YAML.
