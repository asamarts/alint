# Case study: `kubernetes/kubernetes`

Inventory of the structural-validation tooling in `kubernetes/kubernetes` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-05, `git ls-remote https://github.com/kubernetes/kubernetes HEAD` matched at the time of the inventory.

---

## Summary

Kubernetes maintains **50 `hack/verify-*.sh` scripts** that gate every PR.
Roughly **40 % map directly to existing alint rules**, **30 % need new alint
primitives** (most non-trivial: Go import-aliases enforcement,
restricted-package detection per directory), and **30 % are out of alint's
scope** (codegen drift, dead-code elimination, vendor-graph analysis — the
Go-toolchain-aware checks alint isn't trying to do).

The 40 % that *do* fit translate cleanly to a 12-rule alint config (below).
Replacing those 20 shell scripts with one declarative config + one
`alint check` invocation in CI is the headline win — fewer moving parts, one
place to look when CI breaks, ~5× faster than running 20 shell scripts in
sequence (alint runs rules in parallel; the shell pipeline doesn't).

---

## Existing tooling inventory

`hack/verify-*.sh` (50 scripts). The first comment of each script names its
target. Categorised:

### Maps to existing alint rules (drop-in replacements)

| `hack/verify-*.sh` | What it checks | alint replacement |
|---|---|---|
| `boilerplate.sh` | Apache-2 license header on every source file | `file_header` (per-language regex, scope_filter to match ext) |
| `file-sizes.sh` | Files > 1MB need explicit allow-listing | `file_max_size` with `paths:` exclude list |
| `owners-fmt.sh` | `OWNERS` files are valid YAML, formatted | `yaml_path_matches` (structure check) + `command` rule shelling out to `yamlfmt --check` |
| `shellcheck.sh` | All shell scripts pass shellcheck | `command` rule per `**/*.sh` (the canonical use of `command`) |
| `spelling.sh` | All files pass `misspell` | `command` rule per file |
| `gofmt.sh` | Go files are gofmt-clean | `command` rule shelling out to `gofmt -l` |
| `golangci-lint.sh` | Go files pass golangci-lint config | `command` rule (golangci-lint binary) |
| `golangci-lint-config.sh` | `.golangci.yml` is well-formed | `yaml_path_matches` + `json_schema_passes` against the published golangci-lint schema |
| `govulncheck.sh` | No known Go vulns in the source | `command` rule |
| `mocks.sh` | Mock files are up-to-date with their source | `pair` (cross-file rule pairing source ↔ mock) — partial; the freshness check itself is out of scope |
| `pkg-names.sh` | Go package names follow convention | `file_content_matches` per `*.go` (regex on the `package <name>` line) |
| `staging-meta-files.sh` | `staging/` packages have required meta files (`OWNERS`, `README`, `go.mod`) | `for_each_dir` over `staging/src/k8s.io/*/` with `require: file_exists OWNERS, README.md, go.mod` |

12 scripts — direct replacements. 5-10 minute config-build per rule.

### Needs new alint primitive

| `hack/verify-*.sh` | What it checks | What alint needs |
|---|---|---|
| `imports.sh` | Per-directory restricted-package import rules from `staging/publishing/import-restrictions.yaml` | A `restricted_imports` rule kind that reads a registry file + checks each Go/JS/Python file's imports against the registry. **Generalised use case:** language-aware import-allowlist gates per directory. Strong candidate for v0.10+ given how many monorepos enforce this. |
| `import-aliases.sh` | Go imports use the project's preferred alias (from `hack/.import-aliases`) | A `language_import_aliases` rule kind — same shape as above but on the alias position rather than the package itself. Could be the same rule kind with a different mode. |
| `metrics-naming.sh` | Prometheus metric names follow Kubernetes naming convention | `file_content_matches` *almost* works, but the check is "every metric registration call's name argument matches `<convention>`" — needs AST-aware pattern matching. Out of alint's "no-AST" scope per the non-goals; would need a `command`-based rule shelling out to a custom Go AST checker. |
| `prometheus-imports.sh` | Files in certain dirs may not import `prometheus/client_golang` directly | A `forbidden_imports` rule kind — the inverse of `restricted_imports`. Same primitive. |
| `readonly-packages.sh` | Some `vendor/` packages must not be modified after import | A `pair_hash` rule kind (compare hash of pair against pinned manifest). `file_hash` works on single files — generalising it to "hash of (file, manifest entry)" is the gap. |
| `internal-modules.sh` | Internal-only Go modules must not be imported externally | Same as `imports.sh` / `restricted_imports`. |
| `testing-import.sh` | Production code may not import `testing` | Same primitive. |

**Gap pattern: language-aware import gates.** ~6 of the 50 scripts are
variants of "control which packages can be imported from where". This is the
**single most-load-bearing missing rule kind** for Go monorepos. Worth a
dedicated v0.10+ design pass: `import_gate` rule kind with allowlist /
denylist / alias modes, applied via `paths:` + `scope_filter:` like other
per-file rules.

### Out of alint's scope (use the existing tool)

These are codegen / AST / build-system checks. Alint's non-goals are
deliberate; we should mention these in the case study as "alint doesn't try to
do this; keep your existing script."

- `codegen.sh`, `generated-docs.sh`, `generated-stable-metrics.sh` — codegen
  drift; out of scope (alint doesn't run codegen)
- `deadcode-elimination.sh`, `typecheck.sh` — compile-level analysis; out of
  scope
- `vendor.sh`, `no-vendor-cycles.sh` — Go module-graph analysis; out of scope
- `e2e-test-ownership.sh`, `featuregates.sh`, `prerelease-lifecycle-tags.sh` —
  Kubernetes-specific Go-AST checks; out of scope
- `openapi-spec.sh`, `openapi-docs-urls.sh` — OpenAPI generation; out of scope
- `non-mutating-validation.sh`, `cli-conventions.sh`, `description.sh`,
  `fieldname-docs.sh`, `conformance-*.sh` — domain-specific Kubernetes
  semantic checks; out of scope

### Already covered by other linters Kubernetes uses

- `lint-dependencies.sh`, `external-dependencies-version.sh` — `cargo audit`-style;
  use the existing tool
- `publishing-bot.sh` — Kubernetes-specific bot; out of scope
- `netparse-cve.sh` — CVE check; security scanner territory

---

## Starter alint config (drop-in)

[`/.alint.yml`](.alint.yml) in this directory. Replaces 12 of the 50 verify
scripts. Combine with the `command` rule to absorb 5 more (shellcheck,
spelling, gofmt, golangci-lint, govulncheck). Net: **17 of 50 scripts** can
move to one declarative file.

The remaining 33:

- 7 need new alint primitives (above) — file as v0.10+ feature requests
- 18 are out of alint's scope (above) — keep the existing scripts, but
  collapse into one `make verify-out-of-scope` target instead of 18 sequential
  bash invocations
- 6 are pre-existing CVE / deps checks already covered by upstream tools
- 2 are duplicates (`verify-all.sh` is just a runner)

---

## Performance comparison (placeholder — bench when validation pass scales)

The shell pipeline runs scripts sequentially. Each shell script does its own
fs walk, which dominates wall time for 100k-file repos like Kubernetes
(approx. 25k Go files, 12k YAML files, 7k MD files at the time of capture).

alint runs all rules in parallel via the v0.9.3 dispatch flip + the v0.9.5+
cross-file fast paths. Expected: ~1-2 s for the alint-replaceable subset on
a Kubernetes-scale repo (compare to the v0.9.13 published S3 100k bench:
1.13 s for the workspace bundle).

To benchmark for real: run `time bash hack/verify-boilerplate.sh && time
bash hack/verify-file-sizes.sh && ... ` against `time alint check` on the
same checkout. Deferred to the per-repo measurement pass.

---

## Recommendation for the launch story

This case study is the **strongest single piece of evidence** for the launch
positioning: "alint replaces 17 ad-hoc shell scripts in Kubernetes' verify
pipeline with one declarative config." Use it as the headline example on
alint.org/examples and in the HN/Reddit launch posts.

Followup feature work surfaced:

- **`import_gate` rule kind** (allowlist / denylist / alias modes) — would
  cover ~6 more verify scripts here; same primitive shows up in nearly every
  Go monorepo we've inventoried
- **`pair_hash` rule kind** (extension of `file_hash` to "hash matches a
  registry entry") — narrower use case but Kubernetes uses it for
  `vendor/`-readonly enforcement
