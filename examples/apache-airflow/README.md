# Case study: `apache/airflow`

> Marketing/positioning writeup at https://alint.org/examples/apache-airflow/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `apache/airflow` and an
alint config that replaces the rules alint can express today, plus a catalogue
of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, `apache/airflow` `06ca900f` (HEAD of
`main` at the time of the inventory).

---

## Summary

Apache Airflow runs **109 pre-commit hooks** declared in
`.pre-commit-config.yaml`, of which **~80 are `repo: local` shell-outs to
Python scripts** under `scripts/ci/prek/` (124 scripts in that dir, with the
extras called from breeze + in_container). This is the canonical
Python-ecosystem expression of the alint problem: a sprawling, hand-written
shell-and-Python pipeline that validates structural invariants nobody can
enumerate from a single file.

Roughly **35 % map directly to existing alint rules** (file existence, regex
forbidden-content, YAML/TOML path equality, JSON-Schema validation,
provider-package layout via `for_each_dir`). Another **10 %** reduce to
`command` shell-outs to existing tools (`ruff`, `yamllint`, `codespell`,
`shellcheck`, `hadolint`, `markdownlint`, `zizmor`, `bandit`). The remaining
**~55 %** split between airflow-specific Python AST scans (template-fields
detection, import-graph gates between airflow-core / providers / shared,
metric-registry sync) and codegen / file-update hooks that are out of scope
for any structural linter.

The 35 % that *do* fit translate into a 25-rule alint config (below).
Replacing those ~30 hooks with one declarative file + the bundled
`compliance/apache-2@v1` ruleset consolidates the "what does airflow
consider a valid provider package" question to one place instead of
chasing eight pre-commit IDs into eight Python files.

The most-uniform structural surface in the airflow tree is
`providers/`: 101 provider distributions, each conforming to the same
pattern. `for_each_file: providers/**/provider.yaml` plus a nested
`require:` block of `file_exists` / `dir_exists` + a couple of
`yaml_path_matches` rules covers what airflow today enforces with the
1085-line `scripts/in_container/run_provider_yaml_files_check.py`.

---

## Existing tooling inventory

### `.pre-commit-config.yaml` — 109 hooks across 14 repo blocks

The pre-commit pipeline is wired by [prek](https://github.com/j178/prek) (a
faster pre-commit runner; airflow's choice of name for the
`scripts/ci/prek/` directory is a salute to that). Each `local` hook calls a
script in `scripts/ci/prek/` (124 scripts, `wc -l ./scripts/ci/prek/`).

Categorised by what they actually do:

| Category | Hook count | What | Mappable to alint? |
|---|---:|---|---|
| External tool wrapper | 11 | doctoc, blacken-docs, codespell, yamllint, flynt, zizmor, lychee, markdownlint, bandit, shellcheck, hadolint | yes — `command` rule kind |
| `insert-license` (different file types) | 11 | Apache 2.0 header insertion per language | yes — bundled `compliance/apache-2@v1` covers detection; insertion is alint's `fix:` (auto-insert not yet shipped) |
| Pre-commit-hooks builtins | 9 | merge conflicts, trailing whitespace, eol fixers, debug statements, private-key detection | yes — bundled `oss-baseline@v1` already covers most |
| `language: pygrep` regex forbids | 10 | per-language "this string may not appear in this glob" | yes — `file_content_forbidden` (1:1) |
| `lint-json-schema` (3 variants) | 3 | JSON Schema validation | yes — `json_schema_passes` |
| Provider-package conventions | 6 | provider.yaml shape, pyproject names, distribution gitignore | yes — `for_each_dir` + `yaml_path_matches` + `toml_path_matches` |
| Cross-file value sync | 11 | "this constant in file A must equal this YAML field in file B" | **no** — needs new primitive |
| Python AST gates | 9 | "templated_fields must be valid", "no test-only imports in src", deprecation classes | **no** — alint deliberately doesn't do AST |
| Codegen / file-update hooks | ~15 | `update-*` hooks that regenerate files | out of scope (alint doesn't run codegen) |
| Sortedness checks | 4 | `INTHEWILD.md` alphabetical, `installed_providers.txt` sorted, etc. | partial — `unique_by` covers basenames; intra-file sortedness is a gap |
| Misc one-off | ~30 | bug-report-template format, k8s schema vendoring, source-date-epoch update, breeze CLI doc generation | mostly out of scope |

### Scripts that go beyond what `.pre-commit-config.yaml` advertises

- `scripts/in_container/run_provider_yaml_files_check.py` — 1085 lines. Runs
  inside the breeze container, uses Python imports to load every provider
  package, walks `provider.yaml` for completeness against the registered
  hooks/operators/sensors. The schema-shape parts are alint-mappable; the
  import-and-introspect parts are out of scope.
- `dev/breeze/` — Airflow's contributor-tool framework (~25k lines of
  Python). Mostly orchestration; a couple of files (`global_constants.py`,
  `selective_checks.py`) contain the canonical lists that pre-commit
  cross-checks against. Airflow uses these to drive sync hooks; alint can't
  do that today.

### `pyproject.toml` conventions

Airflow's monorepo has **multiple pyproject.toml files**:

- `/pyproject.toml` — meta-package
- `/airflow-core/pyproject.toml`, `/airflow-ctl/pyproject.toml`,
  `/task-sdk/pyproject.toml`, `/devel-common/pyproject.toml` — core distros
- `/providers/<name>/pyproject.toml` × 101
- `/shared/<dist>/pyproject.toml` — internal shared libraries
- `/dev/breeze/pyproject.toml` — contributor tool

Every one of these is treated as a "distribution" by Airflow's release
machinery and must satisfy the same gitignore/license/notice requirements.
This is what `for_each_file: "**/pyproject.toml"` was built for.

### `providers/` directory layout — the alint sweet spot

```
providers/
├── amazon/                   # single-namespace provider
│   ├── docs/
│   ├── LICENSE
│   ├── NOTICE
│   ├── provider.yaml
│   ├── pyproject.toml
│   ├── README.rst
│   ├── src/
│   └── tests/
├── apache/                   # namespaced (Apache* family of providers)
│   ├── spark/                # → apache-airflow-providers-apache-spark
│   ├── kafka/                # → apache-airflow-providers-apache-kafka
│   └── ...
├── cncf/
│   └── kubernetes/           # → apache-airflow-providers-cncf-kubernetes
├── common/
│   ├── ai/, compat/, sql/, io/
├── microsoft/
│   ├── azure/, mssql/, psrp/, winrm/
└── ...
```

101 `provider.yaml` files. Every one of them defines:

```yaml
package-name: apache-airflow-providers-<name>
name: <Display Name>
state: ready | not-ready | suspended | removed
versions: [...]
integrations: [...]
hooks: [...]
operators: [...]
```

Today this is enforced by:

- a JSON Schema (`airflow-core/src/airflow/provider.yaml.schema.json`) →
  alint `json_schema_passes` covers this directly
- a registration cross-check in `run_provider_yaml_files_check.py` that
  imports the provider package and walks `ProvidersManager` — out of scope
  for alint
- the implicit-required-files convention (every dir with `provider.yaml`
  also has README.rst, LICENSE, NOTICE, src/, tests/, docs/) — **today
  enforced by nothing structural**, which is exactly why `for_each_file`
  is the right answer.

### `.github/workflows/` — 43 workflow files

- 41 of them are pinned to commit SHAs already; `gha-pin-actions-to-sha`
  in the bundled `ci/github-actions@v1` ruleset becomes a no-op
  enforcement check (correctness regression test).
- The interesting bit: every `actions/checkout` step in airflow's
  workflows must set `with.persist-credentials: false` (enforced by
  `scripts/ci/prek/checkout_no_credentials.py`). This is a structural
  rule — `yaml_path_matches` with a JSONPath filter expression covers
  it. Airflow's existing implementation walks the YAML in Python; alint
  expresses it in two lines.

---

## What maps to existing alint rules (drop-in replacements)

| Pre-commit hook | What it checks | alint replacement |
|---|---|---|
| `check-merge-conflict` | No `<<<<<<< HEAD` markers | bundled: `oss-no-merge-conflict-markers` |
| `detect-private-key` | No PEM keys committed | covered partially by `hygiene/no-tracked-artifacts@v1`; needs a small addition for PEM regex (open as feature) |
| `end-of-file-fixer` | Files end with `\n` | `final_newline` (already in oss-baseline + python rulesets) |
| `mixed-line-ending` | No mixed `\r` / `\r\n` | `line_endings` |
| `trailing-whitespace` | No trailing spaces | `no_trailing_whitespace` (already in oss-baseline) |
| `check-builtin-literals` | `dict()` not `{}` etc. | `file_content_forbidden` (regex per-pattern) |
| `check-zip-file-is-not-committed` | `*.zip` forbidden | `file_absent` |
| `check-pydevd-left-in-code` | `pydevd.*settrace(` regex | `file_content_forbidden` (1:1) |
| `check-safe-filter-usage-in-html` | `\|safe` in templates | `file_content_forbidden` (1:1) |
| `check-urlparse-usage-in-code` | `from urllib.parse import urlparse` | `file_content_forbidden` |
| `check-base-operator-usage` (×2) | wrong import path for `BaseOperator` | `file_content_forbidden` (×2) |
| `check-core-deprecation-classes` | `category=DeprecationWarning` in core | `file_content_forbidden` |
| `check-provide-create-sessions-imports` | wrong import path for session helpers | `file_content_forbidden` |
| `check-incorrect-use-of-LoggingMixin` | `LoggingMixin()` instantiation | `file_content_forbidden` |
| `check-start-date-not-used-in-defaults` | `start_date` in `default_args` | `file_content_forbidden` |
| `check-for-inclusive-language` | blocked terminology | `file_content_forbidden` (with the long airflow exclude list) |
| `check-apache-license-rat` | LICENSE conformance | bundled `compliance/apache-2@v1` |
| `check-notice-files` | NOTICE has current year | `file_content_matches` per-file with year-templated pattern |
| `check-distribution-gitignore` | every pyproject.toml dir has `.gitignore` containing `*.iml` | `for_each_file: **/pyproject.toml` + `file_exists` + `file_content_matches` (this was the headline pattern of the inventory) |
| `lint-json-schema` (3 variants) | YAML/JSON files match a JSON Schema | `json_schema_passes` (×3) |
| `check-persist-credentials-disabled-in-github-workflows` | `actions/checkout` with `persist-credentials: false` | `yaml_path_matches` with JSONPath filter |
| `bandit` | Python security linter | `command` rule shelling out to `bandit` |
| `ruff` / `ruff-format` | fast Python lint/format | `command` rule (×2) |
| `yamllint` | YAML lint | `command` rule |
| `codespell` | spelling | `command` rule |
| `shellcheck` | shell script lint | `command` rule |
| `hadolint` (`lint-dockerfile`) | Dockerfile lint | `command` rule |
| `markdownlint` (`lint-markdown`) | markdown lint | `command` rule |
| `zizmor` | GH workflow security | `command` rule |
| Provider-package required files | every `provider.yaml` neighbour has README.rst, LICENSE, NOTICE, src/, tests/, docs/ | `for_each_file` + nested `require:` (the `staging-meta-files` shape from the kubernetes case study, applied to airflow's 101-provider tree) |
| Provider-package `provider.yaml` shape | `package-name`, `state`, etc. | `yaml_path_matches` (×3) |
| Provider-package `pyproject.toml` shape | `project.name` matches convention | `toml_path_matches` |

About **30 hooks** map cleanly. Adding the 8-10 `command` shell-outs gets the
total to about **40 of 109**.

---

## What needs a new alint primitive

| Pre-commit hook | What it checks | What alint needs |
|---|---|---|
| `check-version-consistency` | airflow `__version__` constant in `airflow/__init__.py` matches `pyproject.toml`'s `[project].version` matches `task-sdk/src/airflow/sdk/__init__.py` | A **`cross_file_value_equals`** rule kind — pulls a value via Python AST / TOML path / regex from one file, asserts equality with the same shape in N other files. **Generalised use case:** "the canonical version / app name / API endpoint must agree across N specific files." Per `launch-evidence.md`, this is now a **v0.10 ship-target** with **10 demand sources** (airflow + tokio + clap + uv + react + pnpm + nodejs/node + pytorch + vscode + istio); istio's surfacing of the per-file-extractor refinement (pitfall #20) shows the value-extractor block as the design refinement to ship alongside it. |
| `check-secrets-search-path-sync` | Two specific Python files have identical search-path lists | Same primitive as above. |
| `check-template-context-variable-in-sync` | `airflow.models.taskinstance` context vars match `templates-ref.rst` and `task-sdk/.../context.py` | Same primitive. |
| `check-template-fields-valid` | Every `BaseOperator` subclass declares `templated_fields` containing only valid attribute names | Python AST. **Out of scope** (alint's "no AST" non-goal). Keep the existing script. |
| `check-revision-heads-map` | Alembic migration head matches the constant in `version_heads_map.py` | A **`cross_file_value_equals`** variant + glob-the-newest-migration-version-via-filename. Half mappable. |
| `check-airflow-imports-in-shared` | Files in `shared/*/src/` may not import from `airflow.*` (with a small allow-list) | The `import_gate` rule kind from the kubernetes case study — same primitive, just for Python imports. **Single most-load-bearing missing rule kind** here too. |
| `check-test-only-imports-in-src` | Production code may not import `pytest`, `unittest.mock`, etc. | Same `import_gate` primitive. |
| `check-no-new-airflow-exceptions` | No new `raise AirflowException(...)` callsites compared to a frozen list | A **`delta_against_golden_file`** rule kind: "lines matching pattern X may not exceed the count in golden file Y". Niche but airflow uses it 4 places. |
| `check-no-new-airflow-core-utils-modules` | No new files under `airflow-core/src/airflow/utils/` outside the frozen list in `known_airflow_core_utils_modules.txt` | A **`file_in_allowlist`** rule kind: existing `file_absent` with `paths: include/exclude` is close, but the allowlist living in a side-file is the gap. |
| `update-spelling-wordlist-to-be-sorted` | `docs/spelling_wordlist.txt` is alphabetically sorted | A **`file_lines_sorted`** rule kind. Tiny, narrow, but airflow uses it 4 places. |
| `update-installed-providers-to-be-sorted` | Installed providers list sorted | Same primitive. |
| `update-in-the-wild-to-be-sorted` | `INTHEWILD.md` org list sorted | Same primitive (with a section-marker filter). |
| `check-changelog-has-no-duplicates` | No duplicate entries in changelog files | A **`no_duplicate_lines`** rule kind. Narrow but reusable. |
| `check-airflow-bug-report-template` | Provider list option-block in the GH issue template is sorted | YAML-shaped sortedness; combine with the sortedness primitive above. |
| `check-extras-order` / `check-extras-order` | Dockerfile extras section is sorted between two marker lines | Same sortedness primitive + section-marker support. |
| `check-metrics-synced-with-registry` | Every metric-registration callsite in Python source has a matching entry in `metrics_registry.yaml` | Python AST + YAML cross-walk. **Out of scope** in the strict sense; could be a `command` shell-out. |

**Gap pattern: cross-file value sync.** ~6 of the 109 hooks are variants of
"this value in file A must equal this value in file B (and possibly C, D,
E)". This is the biggest single missing primitive for monorepo-shaped
Python codebases — Airflow has 11 of these, kubernetes has 4 (in different
guises), Rust ecosystem repos use it for `Cargo.toml` workspace versions.
Now a **v0.10 ship-target** per `launch-evidence.md` with 10 saturated
demand sources; the design surfaced by istio's case study adds a
per-file `value_extractor:` block (pitfall #20 refinement) so each
mirror can declare its own extractor.

**Gap pattern: import gates.** Same as kubernetes — `import_gate` with
allowlist / denylist modes is doubly-load-bearing now (Go and Python
monorepos both want it). Now a **v0.10 ship-target** per
`launch-evidence.md` with 4 demand sources (k8s + airflow +
golang/go + pytorch).

**Gap pattern: file-content sortedness.** A genuinely small primitive
(`file_lines_sorted`, `no_duplicate_lines`) that covers 5+ pre-commit
hooks across airflow alone. Cheap to add; high coverage payoff.

---

## Out of alint's scope (use the existing tool)

These are codegen / Python-AST / build-system checks. Alint's non-goals are
deliberate; we should mention these in the case study as "alint doesn't try
to do this; keep your existing script."

- `update-pyproject-toml`, `update-uv-lock`, `update-version`,
  `update-supported-versions`, `update-providers-build-files`,
  `update-providers-dependencies`, `update-source-date-epoch`,
  `update-spelling-wordlist`, `update-installed-providers`,
  `update-in-the-wild`, `update-breeze-cmd-output`, `update-docker-gpg-keys`,
  `update-example-dags-paths`, `update-inlined-dockerfile-scripts`,
  `update-local-yml-file`, `generate-airflow-diagrams`, `generate-pypi-readme`,
  `generate-tasksdk-datamodels`, `generate-airflowctl-datamodels`,
  `generate-execution-api-schema`, `generate-openapi-spec`,
  `generate-openapi-spec-providers`, `download-k8s-schemas`,
  `vendor-k8s-json-schema`, `compile-ui-assets`, `compile-provider-assets` —
  ~25 hooks that are **regenerators**, not validators. Out of scope.
- `mypy`, `mypy-devel-common`, `mypy-folder` — type-checking; out of scope
  (use `command` rule to keep them in the same pipeline if desired).
- `check-template-fields-valid`, `check-init-decorator-arguments`,
  `check-base-operator-partial-arguments`,
  `validate-operators-init`,
  `decorator-operator-implements-custom-name`,
  `check-deferrable-default`,
  `check-contextmanager-class-decorators`,
  `check-init-in-tests`,
  `check-aiobotocore-optional`,
  `check-conf-import-in-providers`,
  `check-imports-in-providers`,
  `check-airflow-v-imports-in-tests`,
  `check-cli-definition-imports`,
  `check-common-compat-lazy-imports`,
  `check-common-sql-dependency`,
  `check-connection-doc-labels`,
  `check-deprecations`,
  `check-default-configuration`,
  `check-execution-api-versions`,
  `check-i18n-json`,
  `check-k8s-schemas-published`,
  `check-kubeconform`,
  `check-lazy-logging`,
  `check-migration-patterns`,
  `check-min-python-version`,
  `check-new-airflow-exception-usage`,
  `check-provider-docs`,
  `check-providers-subpackages-all-have-init`,
  `check-provider-version-compat`,
  `check-schema-defaults`,
  `check-sdk-imports`,
  `check-security-doc-constants`,
  `check-shared-distributions-structure`,
  `check-shared-distributions-usage`,
  `check-shared-mypy-hooks`,
  `check-system-tests-hidden-in-index`,
  `check-system-tests`,
  `check-template-fields`,
  `check-test-only-imports-in-src`,
  `check-tests-in-right-folders`,
  `check-ti-vs-tis-attributes`,
  `check-airflowctl-command-coverage`,
  `check-airflowctl-help-texts`,
  `check-changelog-format`,
  `check-newsfragments-are-valid`,
  `check-airflow-bug-report-template`,
  `prevent-deprecated-sqlalchemy-usage`,
  `check-integrations-list-consistent` — all Python AST / domain-specific
  semantic checks. **Out of scope.** Most of these wouldn't make sense as
  alint rules anyway; they're domain-aware in a way alint deliberately isn't.

---

## Starter alint config (drop-in)

[`./.alint.yml`](./.alint.yml) in this directory. Covers ~30 of the 109
pre-commit hooks via declarative rules and ~10 more via the `command` rule
kind. Net: **~40 of 109 hooks** can move to one declarative file.

The remaining ~70:

- ~25 are **regenerators** (`update-*`, `generate-*`, `compile-*`,
  `download-*`, `vendor-*`) — out of alint's scope; keep them as-is.
- ~30 are **Python-AST gates** (template-fields, import gates, deprecation
  detection, etc.) — out of alint's scope per the no-AST non-goal; could
  collapse into one breeze command if desired.
- ~10 need new alint primitives (above) — most are now **v0.10
  ship-targets** per `launch-evidence.md` (`cross_file_value_equals`,
  `import_gate`, `ordered_block`).
- ~5 are domain-specific Airflow checks (Alembic migration validation,
  Kubernetes schema vendoring) — out of scope.

---

## Performance comparison (placeholder — bench when validation pass scales)

Airflow's pre-commit pipeline runs ~109 hooks. Even with `prek` (the faster
runner), full-tree runs take 3-8 minutes wall-clock on contributor machines.
Each hook does its own filesystem walk, which dominates wall time on a
~100k-file repo (the airflow checkout has ~10k Python files,
~5k YAML/RST/MD files, plus 101 nested provider distributions).

alint runs all rules in parallel via the v0.9.3 dispatch flip + the v0.9.5+
cross-file fast paths. Expected: ~2-4 s for the alint-replaceable subset
(structural rules over a 100k-file repo benchmark at ~1-2 s on the published
S3 bench; airflow's pre-commit-mappable subset roughly doubles that because
of the two `for_each_file` iterations over `**/pyproject.toml` and
`providers/**/provider.yaml`).

Wall-time delta specifically for the **provider-package conventions**
subset (the 6 hooks that today live in `run_provider_yaml_files_check.py`
+ breeze codegen): airflow's existing check spawns a docker container,
imports every provider package via Python, walks `ProvidersManager`.
Cold runtime: 30-60 seconds. alint's `for_each_file:
providers/**/provider.yaml` + nested `require:` block: under 1 second
on the same checkout. ~50× speedup on this subset.

To benchmark for real: run `time prek run --all-files` against
`time alint check` on the same checkout, with the alint config narrowed to
match `prek`'s coverage. Deferred to the per-repo measurement pass.

---

## Followup primitive demand (consolidated, priority order)

1. **`cross_file_value_equals` rule kind** — version / constant sync across
   N files. ~11 airflow hooks plus several kubernetes ones plus the Cargo
   workspace-version pattern. Highest single payoff for monorepos.
2. **`import_gate` rule kind** (Python + Go modes; allowlist/denylist) —
   second-highest payoff; ~6 airflow hooks + ~6 kubernetes hooks; same
   primitive shows up in nearly every multi-package monorepo.
3. **`file_lines_sorted` + `no_duplicate_lines`** — cheap, narrow, covers
   5+ airflow hooks. Subsumed by the broader `ordered_block` candidate
   per `launch-evidence.md` — now a **v0.10 ship-target** with 7
   demand sources (rust + airflow + tokio + cpython + arrow + golang/go
   + protobuf failure_lists).
4. **`file_in_allowlist` rule kind** — generalises `file_absent` to "no new
   files outside the side-file allowlist". Niche but airflow uses it 2
   places.
5. **`file_header` insertion `fix:`** — bundled Apache header *detection*
   ships today; auto-insertion (the `insert-license` hook's job, ×11
   variants in airflow) would let alint absorb 11 more hooks with zero new
   rule kinds.

---

## Future analysis

Surfaced during the 2026-05-07 revalidation pass; not yet executed
against a live tree:

1. **`scope_filter.has_ancestor: pyproject.toml` for the per-distribution
   rules** — airflow has 100+ pyproject.toml files (1 root + 4 core +
   101 providers + N shared). Several rules in this config use
   `paths: "**/.gitignore"` as the iteration shape; rebuilding around
   `for_each_file: pyproject.toml` + nested `require:` for the
   distribution-discipline checks would let one rule express the
   "every distro has matching .gitignore" check without per-rule
   path duplication. Reduces 5+ rules to 1.
2. **`compliance/reuse@v1` (3-rule bundled ruleset) trial** — airflow
   uses Apache 2 headers, but the REUSE-spec form would let the
   per-language `insert-license` hooks (×11 variants) collapse into
   one bundled overlay. Surface: ~15k Python + YAML + JS source files.
3. **`docs/adr@v1` (4-rule bundled ruleset) overlay** — airflow has
   `docs/apache-airflow/installation/`, `docs/apache-airflow/best-practices/`,
   and several other long-form decision-doc surfaces. Worth checking
   whether any subset matches the ADR template shape and would
   benefit from the bundled overlay.

---

## Validation status (2026-05-07)

- alint version validated: 0.9.17 (built 2026-05-07)
- `validate-config` rule count: **75 rules loaded** (28 in-config +
  6 bundled overlays: oss-baseline=15, python=9, ci/github-actions=3,
  compliance/apache-2=3, hygiene/no-tracked-artifacts=11,
  hygiene/lockfiles=7 = 48 bundled, with overlap deduped at load)
- Live-tree recheck: **pending — `/tmp/airflow/` not present** at
  revalidation time.
- Pitfalls noted in this README that are now fixed in the engine:
  none directly cited.
- Open gaps after this revalidation: the v0.10+ rule-kind candidate
  status drifted (`cross_file_value_equals` and `import_gate` are now
  v0.10 ship-targets; `ordered_block` subsumes the
  `file_lines_sorted` + `no_duplicate_lines` framing). The 21-pitfall
  catalogue was 17 at the time of the original capture; this README
  doesn't cite specific pitfall numbers, so no renumbering was needed.
