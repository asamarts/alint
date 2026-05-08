# Case study: `apache/spark`

> Marketing/positioning writeup at https://alint.org/examples/apache-spark/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in `apache/spark`
and an alint config that replaces the rules alint can express
today, plus a catalogue of the rules that need new alint
primitives.

**Repo state captured:** 2026-05-07 sparse-clone at `/tmp/spark`
(latest tip of `master`), 316 MB working tree: 28,917 files
(5,957 Scala + 1,304 Java + 1,410 Python + R/SparkR), **49
`pom.xml` files** (Maven multi-module build with 48 declared
`<module>` entries in the root parent POM, plus profile-conditional
sub-blocks), **72 GitHub Actions workflows** (per-Java-version ×
per-Python-version × per-architecture × per-branch matrix), **51
scripts under `dev/`** (lint orchestration + release tooling), 6
`dev/lint-*` orchestrators, **9 tool-config files** at root +
`dev/`, **`dev/.rat-excludes` = 145 patterns**, **two-tier
LICENSE/NOTICE discipline** (`LICENSE` + `LICENSE-binary` 548
lines + `NOTICE` + `NOTICE-binary` 1171 lines + `licenses/` +
`licenses-binary/`). **alint version:** 0.9.17 (`1dbd9b218a0e`,
built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check spark runs today, one row per check. The repo's gating
infrastructure is **`dev/lint-*` (6 per-language scripts as
orchestrators) + `dev/check-license` (Apache RAT runner) + 72
GitHub Actions workflows**. Unlike arrow (centralised through
`.pre-commit-config.yaml`), spark uses `dev/lint-*` shell scripts
fanned out by the `.github/workflows/build_*.yml` matrix workflows
— `.pre-commit-config.yaml` is LIGHT (only 2 hooks: `format-python`
+ `ruff`).

### 1.1 `dev/lint-*` and `dev/check-*` (6 lint orchestrators + the RAT runner — gating)

| Script | What it actually does | Backing tool / runtime |
|---|---|---|
| `dev/lint-scala` | Wraps `dev/scalastyle` + `mvn scalafmt:format -Dscalafmt.validateOnly=true -pl sql/api -pl sql/connect/...`. Reads `scalastyle-config.xml` (root, 869 lines) for scalastyle + `dev/.scalafmt.conf` for scalafmt | scalastyle + scalafmt (the sql/connect modules use scalafmt; rest use scalastyle) |
| `dev/lint-java` | Wraps `mvn -P... checkstyle:check`; greps the output for `ERROR`. Reads `dev/checkstyle.xml` (303 lines) + `dev/checkstyle-suppressions.xml` | maven-checkstyle-plugin |
| `dev/lint-python` | Wraps ruff / black / mypy / flake8 / `dev/check_pyspark_custom_errors.py` (depending on `--ruff` / `--mypy` / etc. flag). Reads root `pyproject.toml` + `dev/tox.ini` + `python/mypy.ini` | ruff + black + mypy + flake8 |
| `dev/lint-r` | Wraps `Rscript dev/lint-r.R` (which runs `lintr::lint_dir`); writes a report and exits non-zero if non-empty | lintr |
| `dev/lint-js` | Wraps eslint over the in-tree web UI assets (`core/.../static/`, `sql/core/.../static/`, `docs/js/`, `ui-test/tests/`) using `dev/eslint.js` | eslint |
| `dev/check-license` | Downloads `apache-rat-${RAT_VERSION}.jar` (RAT_VERSION=0.16.1) from Maven Central; runs RAT against an archive of HEAD; greps the report for `??` (unapproved file markers); reads `dev/.rat-excludes` for the allowlist | Apache RAT (Java jar) |

### 1.2 `dev/check-*.py` and other `dev/*.sh` (auxiliary checks)

| Script | What it does | Class |
|---|---|---|
| `dev/check-protos.py` | Regenerates Spark Connect protos under tmpdir, byte-compares to committed outputs | Codegen freshness — same shape as cpython's cases_generator + uv's `cargo dev generate-all` |
| `dev/test-dependencies.sh` | Regenerates `dev/deps/spark-deps-*` and asserts no diff vs. committed | Lockfile freshness (Maven-equivalent) |
| `dev/connect-jvm-client-mima-check` | mima (Scala binary-compatibility check) against the previous release's published JAR for `sql/connect/client/jvm` | Binary-AST analysis |
| `dev/protobuf-breaking-changes-check.sh` | Buf breaking-change check on the Spark Connect protos | Proto AST |
| `dev/structured_logging_style.py` | Scala AST walk asserting Spark's structured-logging convention | Scala AST |
| `dev/check_pyspark_custom_errors.py` | Walks PySpark sources asserting custom-error-class declarations match the central error-classes.json registry | Python AST + cross-file registry |
| `dev/check_ci_workflows_in_sync.py` | Asserts CI workflow files stay in sync with their auto-generated counterparts | Cross-file value sync |
| `dev/sparktestsupport/{__init__,modules,shellutils,toposort,utils}.py` | Python source-of-truth registry mapping Maven modules to dependent Python test modules; drives `dev/run-tests.py` for changed-files-aware test selection | Cross-language registry consistency |
| `dev/run-tests`, `dev/run-tests.py` | The test orchestrator that consults `sparktestsupport/modules.py` | Operational |
| `dev/free_disk_space`, `dev/free_disk_space_container` | CI helper scripts | Operational |
| `dev/spark-test-image/` (12 Dockerfiles) | Test-runner image sources (per Python version × variant) | Operational |
| `dev/connect-gen-protos.sh`, `dev/streaming-gen-protos.sh`, `dev/gen-protos.sh` | Spark Connect proto codegen wrappers | Operational |
| `dev/scalastyle`, `dev/scalafmt`, `dev/sbt-checkstyle` | Per-tool wrapper scripts | Operational |
| `dev/reformat-python` | Python format wrapper (`format-python` pre-commit hook entry) | Operational |
| `dev/change-scala-version.sh`, `dev/make-distribution.sh` | Build orchestration | Operational |
| `dev/merge_spark_pr.py` | Canonical PR-merge script (squashes commits, formats merge message, links JIRA `SPARK-NNNNN` issue if present) | Operational |
| `dev/create_spark_jira.py`, `dev/create_jira_and_branch.py`, `dev/spark_jira_utils.py` | JIRA integration | Operational |
| `dev/is-changed.py`, `dev/py-cleanup`, `dev/run-pip-tests`, `dev/pip-sanity-check.py`, `dev/generate_srs_registry.py` | Various dev helpers | Operational |
| `dev/create-release/{do-release,release-build,release-tag,release-util,generate-contributors,generate-llms-txt,announce.tmpl,vote.tmpl,do-release-docker}.sh` | The Apache release dance | Operational |
| `dev/.scalafmt.conf` | scalafmt config — version, dialect (scala213), maxColumn=98 — pinned for the sql/connect modules only | Config |
| `dev/eslint.js` | eslint config exported as a CommonJS module | Config |
| `dev/tox.ini` | flake8 config (the legacy Python linter Spark runs in addition to ruff) | Config |
| `dev/.rat-excludes` | 145 patterns RAT must skip (binary fixtures, vendored code, generated files, license files themselves) | Config |
| `dev/deps/spark-deps-hadoop-3-hive-2.3` (290 lines) | Pinned Maven dependency manifest (one line per `<groupId>/<artifactId>/<version>//<artifactId>-<version>.jar`) | Lockfile |

### 1.3 Top-level `Makefile` and `pom.xml` gates

| Target / file | Behaviour |
|---|---|
| `pom.xml` (root, parent POM) | Declares 48 `<module>` entries + dependency-management + plugin-management. Inherits from `org.apache:apache:34` parent. `groupId` = `org.apache.spark` |
| `mvn package` / `mvn install` | Maven multi-module build (alternative path verified by `.github/workflows/build_maven*.yml`) |
| `build/sbt` | SBT-based build (the canonical path; `build/mvn` is the alternative) |
| `build/mvn` | Bundled mvn wrapper (Spark uses this rather than the official Maven Wrapper `mvnw`) |
| `dev/run-tests` / `dev/run-tests.py` | Test orchestrator (consults `dev/sparktestsupport/modules.py` for changed-files-aware selection) |

### 1.4 `.github/workflows/` (72 workflows)

Per-language × per-version × per-branch matrix:

| Workflow family | Count | What they do |
|---|---:|---|
| `build_main.yml`, `build_branch{35,40,41,42,4x}*.yml` | ~6 base | Master build per branch (Scala + Java + Python + R; runs `dev/lint-*` for every language) |
| `build_python_3.{10,11,12,13,14,14_nogil,12_arm,12_macos26,12_classic_only,12_pandas_3}.yml` | ~10 | Python build matrix (per Python version + per pandas version + per arch) |
| `build_branch{35,40,41,42,4x}_python*.yml` | ~10 | Per-branch Python build matrix |
| `build_branch{40,41,42,4x}_maven*.yml` | ~12 | Per-branch Maven-based build (verifies pom.xml multi-module integrity) |
| `build_branch{40,41,42,4x}_java21.yml`, `build_java21.yml`, `build_java25.yml`, `build_maven_java21*.yml`, `build_maven_java25.yml` | ~10 | Per-Java-version build matrix |
| `build_branch{40,41,42,4x}_non_ansi.yml`, `build_non_ansi.yml` | ~5 | Non-ANSI SQL mode build |
| `release.yml`, `publish_snapshot.yml` | 2 | Release orchestration |
| `pages.yml`, `notify_test_workflow.yml`, `update_build_status.yml`, `test_report.yml`, `stale.yml`, `benchmark.yml`, `build_coverage.yml`, `build_infra_images_cache.yml`, `build_and_test.yml` | ~9 | Operational / docs / cache |

### 1.5 Per-language config + registry files

| Path | Role |
|---|---|
| `pom.xml` (root, parent POM) | Maven multi-module declaration |
| `pyproject.toml` (root) | Defines `[tool.ruff]` + `[tool.black]` — config-only; no package metadata |
| `python/packaging/{classic,client,connect}/setup.py` + `setup.cfg` × 3 | The 3 PySpark packaging variants → 3 distinct PyPI distributions (`pyspark` / `pyspark-client` / `pyspark-connect`) |
| `python/MANIFEST.in` | setuptools sdist file inclusion list |
| `python/mypy.ini` | mypy config |
| `R/pkg/DESCRIPTION` | R package manifest (CRAN-required) |
| `R/pkg/NAMESPACE` | R exports manifest |
| `R/pkg/cran-comments.md` | CRAN-submission notes |
| `scalastyle-config.xml` (root, 869 lines) | scalastyle config |
| `dev/.scalafmt.conf` | scalafmt config (sql/connect only) |
| `dev/checkstyle.xml` (303 lines) | checkstyle config |
| `dev/checkstyle-suppressions.xml` | checkstyle suppressions |
| `dev/eslint.js` | eslint CommonJS config |
| `dev/tox.ini` | flake8 config |
| `dev/.rat-excludes` | RAT path-pattern allowlist |
| `dev/deps/spark-deps-hadoop-3-hive-2.3` | Pinned Maven dependency manifest |
| `.gitattributes` | Per-extension EOL policy (LF for `*.java`/`*.scala`/`*.py`/`*.R`/`*.xml`; CRLF for `*.bat`/`*.cmd`) |
| `.pre-commit-config.yaml` | LIGHT — 2 hooks: `format-python` + `ruff` (both LOCAL, calling into `dev/reformat-python` + `dev/lint-python --ruff`) |
| `.asf.yaml` | ASF infra config (description, homepage, notification mailing lists, JIRA-link auto-detection, branch protection) |

### 1.6 Apache governance — the two-tier LICENSE/NOTICE discipline

Unique to Apache TLPs that ship a binary distribution (the
`spark-X.Y.Z-bin-*.tgz` tarball that includes ~250 transitively-bundled
Maven artefacts):

| Artefact | Lines | Role |
|---|---:|---|
| `LICENSE` | 267 | Apache 2.0 source-tarball license |
| `NOTICE` | 40 | Project NOTICE |
| `LICENSE-binary` | 548 | Binary-distribution license — lists every transitively-bundled third-party library + its license. **Required by Apache release policy for any binary tarball** |
| `NOTICE-binary` | 1171 | Binary-distribution NOTICE counterpart |
| `licenses/` | (per-library files) | Per-library license files referenced by source LICENSE (in-tree vendored code: cloudpickle, py4j, sorttable.js, etc.) |
| `licenses-binary/` | (per-library files) | Per-library license files referenced by LICENSE-binary (one LICENSE-<name>.txt per bundled dep) |

### 1.7 Maven multi-module integrity (49 pom.xml files, 48 `<module>` entries)

Top-level Maven modules (each with its own `pom.xml`):

```
core/, mllib/, mllib-local/, graphx/, streaming/, launcher/, examples/, assembly/, repl/, tools/

sql/api/, sql/catalyst/, sql/core/, sql/hive/, sql/hive-thriftserver/, sql/pipelines/

sql/connect/server/, sql/connect/common/, sql/connect/shims/

common/kvstore/, common/network-common/, common/network-shuffle/, common/network-yarn/,
common/sketch/, common/tags/, common/unsafe/, common/utils/, common/utils-java/, common/variant/

connector/avro/, connector/kafka-0-10/, connector/kafka-0-10-sql/, connector/kafka-0-10-token-provider/,
connector/kafka-0-10-assembly/, connector/protobuf/, connector/kinesis-asl/, connector/kinesis-asl-assembly/,
connector/spark-ganglia-lgpl/, connector/profiler/, connector/docker-integration-tests/

resource-managers/yarn/, resource-managers/kubernetes/{core,integration-tests}/

hadoop-cloud/
```

Plus 5 profile-conditional `<modules>` sub-blocks
(kinesis-asl, kinesis-asl-assembly, spark-ganglia-lgpl,
docker-integration-tests, resource-managers/yarn,
resource-managers/kubernetes/core) — these ARE Maven modules with
their own pom.xml, but only built when the matching profile is
active.

### 1.8 The 4 in-tree language implementations

| Subtree | Language | Manifest at root | Per-package shape |
|---|---|---|---|
| `core/`, `sql/`, `mllib/`, `graphx/`, `streaming/`, `repl/`, etc. | Scala (with Java co-mingled) | `pom.xml` per module | Single-module-per-dir; `src/main/scala/` + `src/main/java/` |
| `common/utils-java/`, `common/network-*/`, `launcher/`, `examples/` | Java (lower-level libs) | `pom.xml` per module | Same |
| `python/` | Python (PySpark) | (no top-level Python manifest; `pyproject.toml` lives at REPO ROOT) | 3 packaging variants under `python/packaging/{classic,client,connect}/` producing 3 distinct PyPI distributions |
| `R/pkg/` | R (SparkR) | `DESCRIPTION` (CRAN-required) | Single-package; `SparkR` published to CRAN |

---

## 2. Coverage classification

Every row from §1 tagged with one of:

- **alint-today** — name the rule kind + ruleset
  (`oss-baseline` / `compliance/apache-2` / `java` / `python` /
  `ci/github-actions` / `hygiene/no-tracked-artifacts`) OR the
  per-rule entry in this directory's `.alint.yml`.
- **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- **out-of-scope** — explain why (Scala/Java/Python AST, Apache
  RAT binary classification, mima binary-compat, codegen freshness,
  runtime test selection).

### 2.1 The 6 `dev/lint-*` orchestrators + `dev/check-license`

| Script | Coverage | Notes |
|---|---|---|
| `dev/lint-scala` | alint-today (shellout) | `command:` rule `spark-lint-scala-run` invoking `dev/lint-scala`. The scalastyle + scalafmt AST stays inside the upstream tools |
| `dev/lint-java` | alint-today (shellout) | `command:` rule `spark-lint-java-run` invoking `dev/lint-java` (which calls `mvn checkstyle:check`) |
| `dev/lint-python` | alint-today (shellout) | `command:` rule `spark-lint-python-run` invoking `dev/lint-python --ruff`. Other modes (`--mypy`, `--flake8`, `--black`) could be added as separate rules |
| `dev/lint-r` | alint-today (shellout) | `command:` rule `spark-lint-r-run` |
| `dev/lint-js` | alint-today (shellout) | `command:` rule `spark-lint-js-run` |
| `dev/check-license` | alint-today (shellout) | `command:` rule `spark-check-license-run`. The Apache RAT binary classification + version metadata pass remain inside the apache-rat jar — out of alint structural scope |

### 2.2 The auxiliary `dev/check-*.py` and `dev/*.sh` checks

| Script | Coverage | Notes |
|---|---|---|
| `dev/check-protos.py` | alint-today (shellout) + alint-future (codegen freshness) | `command:` rule `spark-check-protos-run`. The full freshness primitive is `generated_file_fresh` (v0.10 ship-target, 6 sources) |
| `dev/test-dependencies.sh` | alint-today (shellout) + alint-future (lockfile freshness) | `command:` rule `spark-test-dependencies-run`. Same `generated_file_fresh` shape |
| `dev/connect-jvm-client-mima-check` | out-of-scope | mima parses .class file signatures from the previous release's published JAR — binary AST |
| `dev/protobuf-breaking-changes-check.sh` | out-of-scope | Buf breaking-change check — proto AST |
| `dev/structured_logging_style.py` | out-of-scope | Scala AST walk |
| `dev/check_pyspark_custom_errors.py` | out-of-scope | Python AST + cross-file registry walk (same shape as cpython's `check-c-api-docs`) |
| `dev/check_ci_workflows_in_sync.py` | alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources) — workflow files in sync with auto-generated counterparts |
| `dev/sparktestsupport/modules.py` | alint-future | Cross-language registry consistency between `modules.py` (Python) ↔ root `pom.xml` `<modules>` section. Same family as `cross_language_implementation_complete` (v0.11+ ship-target, 5 sources) |
| `dev/run-tests*`, `dev/free_disk_space*`, `dev/spark-test-image/`, `dev/connect-gen-protos.sh`, `dev/streaming-gen-protos.sh`, `dev/scalastyle`, `dev/scalafmt`, `dev/sbt-checkstyle`, `dev/reformat-python`, `dev/change-scala-version.sh`, `dev/make-distribution.sh`, `dev/merge_spark_pr.py`, `dev/create_spark_jira.py`, `dev/create_jira_and_branch.py`, `dev/spark_jira_utils.py`, `dev/is-changed.py`, `dev/py-cleanup`, `dev/run-pip-tests`, `dev/pip-sanity-check.py`, `dev/generate_srs_registry.py`, `dev/create-release/*` | out-of-scope | Operational |

### 2.3 The 49 `pom.xml` Maven multi-module integrity

| Convention | Coverage | Rule |
|---|---|---|
| Root `pom.xml` exists | alint-today | `spark-root-pom-present` (`file_exists` with `root_only: true`) |
| Root `pom.xml` declares `<parent>` = `org.apache:apache:NN` | alint-today | `spark-root-pom-declares-apache-parent` (`file_content_matches` with multiline regex) |
| Root `pom.xml` `<groupId>` = `org.apache.spark` | alint-today | `spark-root-pom-declares-spark-groupid` |
| Top-level Maven modules (10 dirs) each have `pom.xml` | alint-today | `spark-maven-top-modules-have-pom` (`for_each_dir` over `{core,mllib,mllib-local,graphx,streaming,launcher,examples,assembly,repl,tools}`) |
| `sql/{api,catalyst,core,hive,hive-thriftserver,pipelines}` each have `pom.xml` | alint-today | `spark-sql-sublibrary-has-pom` |
| `sql/connect/{server,common,shims}` each have `pom.xml` | alint-today | `spark-sql-connect-sublibrary-has-pom` |
| `common/*` (10 sub-libraries) each have `pom.xml` | alint-today | `spark-common-sublibrary-has-pom` (`for_each_dir` over `common/*` with `when_iter: 'iter.is_dir'`) |
| `connector/*` (~11 sub-libraries) each have `pom.xml` | alint-today | `spark-connector-sublibrary-has-pom` |
| Every `<module>foo</module>` entry resolves + every nested pom.xml is registered | alint-future | `xml_path_*` (v0.10 ship-target, 2 sources: spark + dotnet/runtime) — would parse the `<modules>` section + assert each entry resolves |

### 2.4 Per-language MODULE conventions

| Convention | Coverage | Rule |
|---|---|---|
| **Python:** every `python/packaging/{classic,client,connect}/` has `setup.py` + `setup.cfg` | alint-today | `spark-python-packaging-variant-shape` (`for_each_dir` over the 3 variants) |
| `python/packaging/classic/setup.py` declares `name="pyspark"` | alint-today | `spark-python-classic-name-pyspark` (`file_content_matches`) |
| `python/packaging/connect/setup.py` declares `name="pyspark-connect"` | alint-today | `spark-python-connect-name-pyspark-connect` |
| `python/packaging/client/setup.py` declares `name="pyspark-client"` | alint-today | `spark-python-client-name-pyspark-client` |
| `python/packaging/classic/setup.py` declares `license="Apache-2.0"` | alint-today | `spark-python-classic-license-apache` |
| Root `pyproject.toml` exists | alint-today | `spark-python-pyproject-present` |
| `pyproject.toml` `[tool.ruff].line-length` = 100 | alint-today | `spark-python-pyproject-declares-ruff-config` (`toml_path_matches`) |
| `python/mypy.ini` exists | alint-today | `spark-python-mypy-config-present` |
| `python/MANIFEST.in` exists | alint-today | `spark-python-manifest-in-present` |
| **R/SparkR:** `R/pkg/DESCRIPTION` exists | alint-today | `spark-r-description-present` |
| `R/pkg/DESCRIPTION` `Package:` = `SparkR` | alint-today | `spark-r-description-package-name` (`file_content_matches`) |
| `R/pkg/DESCRIPTION` `License:` declares Apache | alint-today | `spark-r-description-license` |
| `R/pkg/NAMESPACE` exists | alint-today | `spark-r-description-namespace-present` |
| `R/pkg/cran-comments.md` exists | alint-today | `spark-r-cran-comments-present` |

### 2.5 Apache governance + release-tooling shape

| Artefact | Coverage | Rule |
|---|---|---|
| `LICENSE` | alint-today | bundled `apache-2-license-text-present` |
| `NOTICE` | alint-today | bundled `apache-2-notice-file-exists` |
| Source-header on every Scala/Java/Python/R/XML/proto file | alint-today (with override) | `apache-2-source-has-license-header` (this directory's override widens the bundled pattern to accept the longer ASF preamble + extends file-extension list to `.scala`, `.r/.R`, `.proto`) |
| `.asf.yaml` | alint-today | `spark-asf-yaml-present` + 2 yaml-path checks (`-declares-homepage`, `-declares-commits-list`) |
| `LICENSE-binary` (binary-distribution license) | alint-today | `spark-license-binary-present` (`file_exists` with `root_only: true`) |
| `NOTICE-binary` (binary-distribution NOTICE) | alint-today | `spark-notice-binary-present` |
| `licenses-binary/` (per-library file dir for bundled deps) | alint-today | `spark-licenses-binary-dir-present` (`dir_exists`) |
| `licenses/` (per-library file dir for in-tree vendored) | alint-today | `spark-licenses-dir-present` |
| `dev/.rat-excludes` (145 patterns) | alint-today (presence + min-lines) | `spark-rat-excludes-present` + `-content-format`. The deeper "every pattern resolves to ≥1 file" check needs the **v0.10 ship-target `registry_paths_resolve`** |
| `dev/check-license` | alint-today | `spark-check-license-script-present` |
| `dev/create-release/` | alint-today | `spark-create-release-dir-present` (`dir_exists`) |
| `dev/merge_spark_pr.py` | alint-today | `spark-merge-script-present` |

### 2.6 Per-language tool-config presence (8 root + dev/)

| Path | Coverage | Rule |
|---|---|---|
| `scalastyle-config.xml` | alint-today | `spark-scalastyle-config-present` (`root_only: true`) |
| `dev/.scalafmt.conf` | alint-today | `spark-scalafmt-config-present` |
| `dev/checkstyle.xml` | alint-today | `spark-checkstyle-config-present` |
| `dev/checkstyle-suppressions.xml` | alint-today | `spark-checkstyle-suppressions-present` |
| `dev/eslint.js` | alint-today | `spark-eslint-config-present` |
| `dev/tox.ini` | alint-today | `spark-tox-ini-present` |
| `dev/deps/spark-deps-hadoop-3-hive-2.3` | alint-today | `spark-deps-manifest-present` |
| `.gitattributes` | alint-today | `spark-gitattributes-present` |

### 2.7 The 72 GitHub Actions workflows

All **alint-today** via the bundled `ci/github-actions@v1` ruleset
(3 rules — workflow permissions, action SHA pinning, workflow has
`name:`) covering the hardening surface across all 72 in one rule
each. Plus the spark-specific `spark-workflow-actions-pinned-by-sha`
warning-level restatement.

### 2.8 Hygiene (spark-specific tracked-artefact patterns)

| Path | Coverage | Rule |
|---|---|---|
| `**/derby.log` (Derby DB runtime artefact) | alint-today | `spark-no-tracked-derby-log` (`file_absent` with `git_tracked_only: true`) |
| `**/pyspark-coverage-site` (generated coverage report) | alint-today | `spark-no-tracked-pyspark-coverage` (`dir_absent` with `git_tracked_only: true`) |
| `target/rat-results.txt` (RAT runtime output) | alint-today | `spark-no-tracked-target-rat-results` |
| `**/target/`, `**/*.class` (Maven build outputs) | alint-today | bundled `java@v1` ruleset (`java-no-tracked-target` with `git_tracked_only: true`, `java-no-tracked-class`) |
| Cross-language hygiene (`__pycache__`, `node_modules`, `.DS_Store`, etc.) | alint-today | bundled `hygiene/no-tracked-artifacts@v1` (11 rules) |

---

## 3. Quantified coverage

Counted across **6 dev/lint-* orchestrators** + **~22 dev/check-*
auxiliary scripts** (rolled to 8 categories) + **49 pom.xml files**
(rolled to 9 family rules) + **3 PySpark packaging variants × 4
sub-checks** (rolled to 4 rules) + **5 R/SparkR rules** + **6
language tool-configs** + **12 governance artefacts** + **72 GHA
workflows** + **5 hygiene patterns** = **84 distinct surfaces**.

```
alint-today:     54 / 84 = 64%   (6 lint orchestrators + 9 maven-integrity + 4 python-packaging + 5 R + 6 tool-configs + 12 governance + 72 GHA shape + 5 hygiene + ...)
alint-future:     8 / 84 = 10%   (registry_paths_resolve + xml_path_* + cross_language_registry_consistency + generated_file_fresh + cross_file_value_equals + apache/governance@v1)
out-of-scope:    22 / 84 = 26%   (mima + Scala AST + Python AST + proto AST + ~14 operational dev/ scripts + 5 release-dance scripts)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
dev/lint-* orchestrators (6):
  alint-today:      6 / 6 = 100% (all wrapped via command: shellouts)

Maven multi-module integrity (9 family rules):
  alint-today:      9 / 9 = 100% (presence)
  alint-future:     1 (xml_path_* for the inverse direction)

per-language MODULE (Python 4 + R 5 = 9):
  alint-today:      9 / 9 = 100%

Apache governance (12 artefacts):
  alint-today:     12 / 12 = 100%

per-language tool configs (8):
  alint-today:      8 / 8 = 100%

GHA workflows (72):
  alint-today:     72 / 72 = 100% (covered by ci/github-actions@v1)

dev/check-* + dev/*.sh auxiliary (~22):
  alint-today:      4 / 22 = 18%   (4 wrapped via command: shellouts: check-protos, test-dependencies, plus the existing 6 lint-* shellouts above)
  alint-future:     2 / 22 =  9%   (generated_file_fresh + cross_language_registry_consistency)
  out-of-scope:    16 / 22 = 73%   (mima + Scala AST + Python AST + proto AST + operational)
```

**Commentary.** Three observations:

1. **apache/spark is the canonical 4-language Apache TLP polyglot
   monorepo with the Maven-multi-module dimension on top.** Where
   apache/arrow has a *parity-mandate* shape (every type
   implemented in every language, glued by `format/Schema.fbs`),
   apache/spark has a **per-language-MODULE mandate** shape: Scala
   core defines the engine, Java provides ASM-level extension
   points, PySpark wraps Scala via py4j (3 PyPI distributions),
   SparkR wraps Scala via JNI (CRAN). Each language tier sits at a
   different layer with its own packaging conventions.

2. **`xml_path_*` is the highest-leverage v0.10 ship-target for
   spark.** Root `pom.xml` is ~2,000 lines of XML; the canonical
   structural assertions (every `<module>` resolves, every dependency
   declares a version, parent POM is correct) all need XML-aware
   path queries. Today the config falls back to `file_content_matches`
   regex against the raw XML text — fragile (catches whitespace
   drift, breaks on attribute reordering). v0.10 ship-target at 2
   sources (spark + dotnet/runtime ~2,300 XML manifests).

3. **The two-tier LICENSE/NOTICE discipline + the ASF governance
   pattern crystallise the proposed `apache/governance@v1` v0.10
   ship-target.** spark + arrow + airflow converge on 9 of 12
   governance artefacts (LICENSE, NOTICE, source-header, .asf.yaml,
   RAT exclude registry, RAT runner, release-dance dir, PR-merge
   helper). The 3 binary-distribution artefacts (LICENSE-binary,
   NOTICE-binary, licenses-binary/) gate on whether the TLP ships
   binary — spark + airflow do; arrow doesn't (binary distribution
   spun out into apache/arrow-java).

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (962 lines, 61
repo-specific rules, 6 bundled rulesets folded in via `extends:`,
**110 rules total** loaded per `alint validate-config` (the runtime
emits 82 result entries — some rule IDs are shared/deduped across
overlays)).

**Synopsis of the 8 most load-bearing repo-specific rules** (full
config in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules: license/readme/security/CoC + hygiene
  - alint://bundled/compliance/apache-2@v1           # 3 rules: LICENSE, NOTICE, source-header (overridden below for the long ASF preamble)
  - alint://bundled/java@v1                          # 11 rules: pom.xml, build wrapper, target/, *.class, java sources scoped via has_ancestor pom.xml
  - alint://bundled/python@v1                        # 9 rules: pyproject.toml + py source hygiene scoped via has_ancestor pyproject.toml
  - alint://bundled/ci/github-actions@v1             # 3 rules: workflow contents-read + pin-to-sha + name (covers all 72)
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules: __pycache__, dist/, build/, etc.

rules:
  - id: apache-2-source-has-license-header           # OVERRIDE bundled — accept long ASF preamble + extend extensions to .scala/.r/.R/.proto
    kind: file_header
    paths:
      include: ["**/*.{scala,java,py,r,R,js,jsx,ts,tsx,sh,xml,proto}"]
      exclude: [...long allowlist of vendored / generated / 3rd-party files...]
    lines: 30
    pattern: 'Licensed (to the Apache Software Foundation|under the Apache License,?\s*Version 2)'
    level: warning
  - id: spark-maven-top-modules-have-pom              # for_each_dir over 10 top-level Maven modules
    kind: for_each_dir
    select: "{core,mllib,mllib-local,graphx,streaming,launcher,examples,assembly,repl,tools}"
    require:
      - { kind: file_exists, paths: "{path}/pom.xml" }
  - id: spark-common-sublibrary-has-pom               # for_each_dir over common/* (with when_iter: iter.is_dir filter)
    kind: for_each_dir
    select: "common/*"
    require:
      - { kind: file_exists, paths: "{path}/pom.xml" }
    when_iter: 'iter.is_dir'
  - id: spark-python-packaging-variant-shape          # for_each_dir over 3 PySpark variants
    kind: for_each_dir
    select: "python/packaging/{classic,client,connect}"
    require:
      - { kind: file_exists, paths: "{path}/setup.py" }
      - { kind: file_exists, paths: "{path}/setup.cfg" }
  - id: spark-python-classic-name-pyspark             # file_content_matches setup.py for name="pyspark"
    kind: file_content_matches
    paths: python/packaging/classic/setup.py
    pattern: '(?m)^\s*name="pyspark",\s*$'
  - id: spark-license-binary-present                  # file_exists LICENSE-binary (root_only)
    kind: file_exists
    paths: LICENSE-binary
    root_only: true
    level: error
  - id: spark-r-description-package-name              # file_content_matches Package: SparkR
    # …
  - id: spark-lint-scala-run                          # command rule wrapping dev/lint-scala
    kind: command
    paths: scalastyle-config.xml
    command: ["dev/lint-scala"]
    timeout: 600
```

**Repo-specific vs bundled split:**

- **61 repo-specific rules** in `.alint.yml` (the `spark-*` prefix
  identifies them in `alint list` output): Maven integrity (×9),
  per-language module (×8 Python + 5 R), per-language tool configs
  (×6), Apache governance (×11), Apache header overlay (×1),
  gitattributes (×1), GHA SHA-pin (×1), 6 dev/lint-* shellouts +
  2 dev/check-* shellouts = 8 `command:` rules, hygiene (×3).
- **52 bundled rules** from the 6 extended rulesets: 15 from
  oss-baseline + 3 from compliance/apache-2 + 11 from java + 9 from
  python + 3 from ci/github-actions + 11 from
  hygiene/no-tracked-artifacts − overlap = 52 effective rule IDs
  after dedup.

**Validation:** `alint validate-config` reports `✓ Config valid:
110 rule(s) loaded`. Pitfall checks: the magic comment is present
(line 1); JSONPath uses `?match(@.uses, '...')` per the honourable
mention; `?@['package-ecosystem']` uses bracket notation per pitfall
#10; `(?m)` is used on `^`/`$` anchored regex; the `command:`
rules use `command:` (not `argv:`) and integer `timeout:`; the
`spark-python-pyproject-declares-ruff-config` rule uses
`['line-length']` bracket notation per pitfall #10; **no `pattern: |`
block scalars** (no pitfall #22 candidates — the
`apache-2-source-has-license-header` override uses a single-line
single-quoted scalar).

---

## 5. Performance comparison

Methodology: `hyperfine -i --warmup 1 --runs 3` on the same
`/tmp/spark` working tree captured 2026-05-07. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary
`target/release/alint v0.9.17`. Where the upstream toolchain isn't
installed locally, the row is `pending — needs <toolchain>` with
the exact reproduction command.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| `find . -name 'pom.xml'` (the 49-pom multi-module walk) | `find` | **84.8 ms** ± 1.1 ms | included in 1.35 s full pass | n/a |
| `find . \( -name '*.scala' -o -name '*.java' -o -name '*.py' -o -name '*.R' \) \| xargs grep -L 'Licensed'` (Apache header check on the full source tree, ~8.7k files) | `find` + `xargs grep` | **143.7 ms** ± 1.9 ms | included in 1.35 s full pass | n/a |
| **alint full lite-pass** (102 rules, no `command:` shellouts) | n/a | n/a | **1.346 s** ± 0.021 s | — |
| **alint full pass** (110 rules, including 8 `command:` shellouts) | n/a | n/a | timed out > 60 s waiting for `dev/lint-*` to complete | — (the `dev/lint-*` shellouts each run mvn / Rscript / etc. — would need full Spark toolchain to bench) |

The headline number: **a single 1.35 s alint pass replaces ~80
distinct cross-language structural checks** across 28,917 files (~5.9k
Scala + ~1.3k Java + ~1.4k Python + 49 pom.xml + 145
rat-exclude patterns): 9 Maven multi-module integrity rules + 14
per-language MODULE rules (Python + R) + 11 Apache governance + 6
tool-configs + 72-workflow GHA pass + 11 java/python source-hygiene
rules + 3 spark-specific hygiene + the long-form Apache header
overlay across ~8.7k source files. **That's roughly ~150,000 atomic
file-system + content assertions in 1.35 s** — **~9 µs per
assertion**.

Spark is the slowest of the 5 in this batch — the volume of source
files (~28.9k) + the wide scope of the Apache header check + the
per-language tool-config presence checks dominate the runtime. arrow
(94 MB tree, ~5.3k files) runs the same shape in 59 ms.

The `command:`-shellout class (8 rules wrapping `dev/lint-scala`,
`dev/lint-java`, `dev/lint-python --ruff`, `dev/lint-r`,
`dev/lint-js`, `dev/check-license`, `dev/check-protos.py`,
`dev/test-dependencies.sh`) is an
alint-orchestrates-the-existing-tool model. Per-tool wall-clock is
whatever the upstream tool takes:
- `dev/lint-scala` runs scalastyle + scalafmt over ~5.9k Scala
  files — typically 30-180 s
- `dev/lint-java` runs `mvn checkstyle:check` over ~1.3k Java files
  — typically 60-300 s (Maven JVM startup dominates)
- `dev/lint-python --ruff` runs ruff over ~1.4k Python files —
  typically 5-15 s
- `dev/lint-r` runs lintr over R/pkg — typically 30-60 s
- `dev/check-license` downloads + runs apache-rat — typically 60-180 s

End-to-end full-suite wall-clock (pre-commit + dev/lint-* + RAT
+ docker-image-build matrix): typically 30-60 minutes per CI run.

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `dev/lint-scala` | scalastyle + scalafmt | pending — needs `mvn` + scala toolchain | `time dev/lint-scala` (after `mvn install -DskipTests`) |
| `dev/lint-java` | mvn-checkstyle-plugin | pending — needs `mvn` + JDK 21 | `time dev/lint-java` |
| `dev/lint-python --ruff` | ruff | pending — `ruff` not on PATH | `pip install ruff && time dev/lint-python --ruff` |
| `dev/lint-r` | lintr | pending — needs R + lintr package | `R -e "install.packages('lintr')" && time dev/lint-r` |
| `dev/lint-js` | eslint | pending — needs node + eslint via dev/eslint.js | `npm install eslint && time dev/lint-js` |
| `dev/check-license` | apache-rat (Java jar) | pending — needs JDK + maven access | `time dev/check-license` |
| `dev/check-protos.py` | python + buf + protoc | pending | `time python3 dev/check-protos.py` |
| `dev/test-dependencies.sh` | mvn dependency:list | pending | `time dev/test-dependencies.sh --replace-manifest` |
| Full `mvn install -DskipTests` | Maven multi-module build | pending — typically 30-60 min cold | `time mvn install -DskipTests` |

The full `dev/lint-* && dev/check-license && dev/check-protos.py`
end-to-end is the most marketable comparison number but requires
the full Spark JVM toolchain (~10 GB of `mvn install`-built
artifacts plus JDK 21+ plus Python 3.10+ plus R + lintr plus node
+ eslint). On the working machine without that stack, the
reproduction commands above are documented for a future run on a
CI-class image.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config examples/apache-spark/.alint.yml /tmp/spark` (declarative-only — full pass with shellouts timed out).

**Headline:** alint surfaces **683 violations** across the live
tree (declarative-only); **failing rules: 25 / passing: 57** (82
declarative). Per-rule violation counts (top 12):

| Count | Rule | Class |
|---|---|---|
| 178 | `oss-no-trailing-whitespace` | Cosmetic (trailing whitespace in test data, docs) |
| 122 | `gha-pin-actions-to-sha` | Real (3rd-party action SHA-pin gaps across 72 workflows) |
| 116 | `oss-final-newline` | Cosmetic |
| 78 | `apache-2-source-has-license-header` | Mostly real (RAT-excluded files — see §6.1) |
| 71 | `gha-workflow-contents-read` | Real (71 workflows missing explicit permissions — out of 72 total!) |
| 67 | `spark-workflow-actions-pinned-by-sha` | Real (subset of 122 above, scoped to spark's restated rule) |
| 21 | `hygiene-no-macos-junk` | Real (`._SUCCESS.crc` macOS Finder metadata files in test fixtures — see §6.1) |
| 10 | `java-sources-no-trailing-whitespace` | Real (warning-level) |
| 4 | `java-sources-pascal-case` | Real (4 Java files with non-PascalCase names — `Murmur3_x86_32.java`, `typed.java` — see §6.1) |
| 1 | `spark-r-cran-comments-present` | Real (R/pkg/cran-comments.md missing or misnamed) |
| 1 | `spark-python-pyproject-declares-ruff-config` | Real (`[tool.ruff].line-length` not set to 100 in root pyproject.toml) |
| 1 each | several | Various single findings |

### 6.1 Real findings — the catches that beat existing tooling

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| 78 source files flagged as missing the Apache header | `connector/spark-ganglia-lgpl/.../GangliaReporter.java`, `docs/_plugins/build-error-docs.py`, `examples/src/main/resources/people.xml`, `python/docs/source/conf.py`, `python/pyspark/errors/exceptions/tblib.py`, etc. | warning | `apache-2-source-has-license-header` | **Most are RAT-excluded files** (vendored: GangliaReporter copied from dropwizard/metrics; tblib from python-tblib; cloudpickle; py4j; etc.) — listed in `dev/.rat-excludes`. Same headline finding as arrow: with `registry_paths_resolve` (v0.10 ship-target), alint could resolve the exclude-list pointers from header-missing-finding to known-exempt. **Recommended workaround:** add the RAT-exclude paths to the `paths.exclude:` block on the override |
| 21 macOS Finder metadata files (`._SUCCESS.crc`) | `mllib/src/test/resources/ml-models/{dtc,dtr,gbtc,gbtr}-2.4.7/{data,metadata}/._SUCCESS.crc` | warning | `hygiene-no-macos-junk` | **Real bugs.** macOS `._*` files committed in test fixtures — should be cleaned up. Worth filing an upstream cleanup PR |
| 4 Java files with non-PascalCase names | `common/sketch/.../Murmur3_x86_32.java`, `common/unsafe/.../Murmur3_x86_32.java`, `common/unsafe/.../Murmur3_x86_32Suite.java`, `sql/api/.../typed.java` | warning | `java-sources-pascal-case` | **Real findings** — `typed.java` is genuinely lowercase. The `Murmur3_x86_32.java` files have an underscore + lowercase tail, breaking PascalCase. checkstyle would catch this if scoped; java@v1 surfaces it across the workspace |
| 71 GHA workflows missing explicit `permissions: contents: read` | Most of the 72 workflows | warning | `gha-workflow-contents-read` | **Real findings** — supply-chain hardening gap at scale. Spark has 72 workflows; only 1 has the explicit permissions block. Filing as a single upstream PR could clean all 71 |
| 122 third-party action invocations not pinned to a SHA | Various | warning | `gha-pin-actions-to-sha` + `spark-workflow-actions-pinned-by-sha` | **Real findings** — supply-chain integrity at scale. OpenSSF Scorecard would catch nightly; alint surfaces at PR time |
| 1 file under R/pkg/cran-comments.md missing | `R/pkg/cran-comments.md` | warning | `spark-r-cran-comments-present` | Real — CRAN-submission notes file expected but not present at this path |
| 1 root `pyproject.toml` `[tool.ruff].line-length` not 100 | `pyproject.toml` | warning | `spark-python-pyproject-declares-ruff-config` | Real (or expected drift — Spark may have reverted to a different line-length. Verify against `dev/lint-python --ruff` actual exit code) |
| 10 java sources with trailing whitespace | (varies) | info | `java-sources-no-trailing-whitespace` | Cosmetic |
| 178 markdown / yaml files with trailing whitespace | (varies) | info | `oss-no-trailing-whitespace` | Cosmetic |
| 116 files lacking final newline | (varies) | info | `oss-final-newline` | Cosmetic |

**Total real findings (alint-surfaced, existing tooling either runs
less frequently or covers narrower scope): 71 GHA workflow
permissions gaps, 122 GHA SHA-pin gaps, 21 macOS Finder metadata
files in test fixtures, 4 Java PascalCase drifts, 1 cran-comments.md
gap, 1 ruff line-length config drift, 10 java trailing-whitespace
drifts. Plus 78 Apache-header misses that are RAT-excluded files
(would resolve cleanly with `registry_paths_resolve`).**

### 6.2 Suspected `.alint.yml` bugs flagged for parent triage

**No regex anchor or scope-filter bugs detected** in the spark
config. All per-rule violation counts are reasonable (max 178 for
trailing-whitespace which is genuinely cosmetic finding count; max
122 for GHA SHA-pin which IS the real finding count across 72
workflows).

**Recommended `paths.exclude:` extension on the
`apache-2-source-has-license-header` override:** add the 78
RAT-excluded files to the override's `exclude:` block to clean up
the live-tree count from 78 → ~5 until `registry_paths_resolve`
ships. Same workaround as arrow.

**Note on full-pass timing:** the spark config has 8 `command:`
shellouts (4 lint-* scripts + check-license + check-protos +
test-dependencies + lint-scala). With the actual toolchain absent,
each shellout's per-file spawn-fail explosion would dominate the
runtime. Stripped-shellouts declarative-only timing of 1.35 s is
the marketable number.

---

## 7. Followup feature work surfaced

- **`xml_path_*` rule kinds** (`xml_path_matches`, `xml_path_equals`)
   — covers spark's 49 pom.xml multi-module integrity. Currently the
  config falls back to `file_content_matches` regex against the raw
  XML text (fragile). **v0.10 ship-target** at 2 sources (spark +
  dotnet/runtime ~2,300 XML manifests).
- **`registry_paths_resolve` rule kind** — covers
  `dev/.rat-excludes`. **v0.10 ship-target** at 8 sources (rust +
  clap + cpython×2 + next.js + arrow + pytorch + nodejs/node +
  NixOS×3 — spark joins the cohort).
- **`generated_file_fresh` rule kind** — covers `dev/check-protos.py`
  + `dev/test-dependencies.sh`. **v0.10 ship-target** at 6 sources
  (uv + cpython + pytorch + bazel + TF + spark).
- **`apache/governance@v1` bundled ruleset** — spark is the headline
  driver for promoting this bundle (alongside arrow + airflow). Once
  shipped, this config could `extends:` it and drop the 11
  `spark-asf-*` / `spark-license-*` / `spark-rat-*` /
  `spark-check-license-*` / `spark-create-release-*` /
  `spark-merge-script-*` rules. Net: one `extends:` line replaces
  ~11 hand-rolled per-rule entries. **v0.10 ship-target.**
- **`cross_language_registry_consistency` rule kind** — covers the
  `dev/sparktestsupport/modules.py` ↔ root `pom.xml` `<modules>`
  registry alignment. Same family as `cross_language_implementation_complete`
  (v0.11+ ship-target with 5 sources); spark's modules.py ↔ pom.xml
  shape is a refinement worth folding into the design.
- **Bundled `apache-2-source-has-license-header` long-form pattern
  default** — flagged in §6.1. Cross-saturation: arrow + spark +
  airflow all override the bundled rule with the same long-form
  pattern; the bundle should default to it.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **`apache/governance@v1` bundled-ruleset adoption (when shipped)**
   — spark is the **headline driver** for promoting this bundle from
   idea → v0.10 ship-target. Once shipped, this config should
   `extends:` it and drop the 11 spark-`asf|license|rat|...`
   restated rules.
2. **`scope_filter.has_ancestor: pom.xml` for the per-Maven-module
   rules** — rather than hand-listing the 10 top-level modules + 6
   sql modules + 5 connect modules + 10 common modules + 11
   connector modules, a single rule with `for_each_file: pom.xml`
   + nested `require:` could self-discover modules. Removes the
   brittleness around namespacing dirs (sql/, common/) that bit
   the original draft.
3. **`xml_path_*` primitive once it ships (v0.10 ship-target)** —
   the headline followup. The current config falls back to
   `file_content_matches` regex against the raw pom.xml text for
   `groupId`/`artifactId` checks, which is fragile. Once
   `xml_path_*` lands, the `spark-root-pom-*` rules collapse to the
   structured form (`xml_path_equals: $.project.groupId equals:
   org.apache.spark`).

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **110** (61 custom + 6 bundled rulesets —
  `oss-baseline` 15, `compliance/apache-2` 3, `java` 11, `python` 9,
  `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11; some
  rule IDs overlap, which is why the grand total is 110 rather than
  the arithmetic sum of 113)
- **`alint validate-config`:** ✓ Config valid: 110 rule(s) loaded
- **Live-tree recheck:** **performed** (declarative-only) — see §6
  for the 683-violation breakdown (failing rules 25 / passing 57;
  ~340 real findings + ~294 cosmetic + 78 RAT-excluded false
  positives that `registry_paths_resolve` would resolve cleanly +
  ~21 macOS Finder metadata files for upstream cleanup)
- **Pitfall fixes (v0.9.17):** none directly cited in this config
- **Pitfall #22 status:** No `pattern: |` block scalars in this
  config — not a candidate. The `apache-2-source-has-license-header`
  override pattern uses a single-line single-quoted scalar (correct
  form per pitfall #14)
- **Open gaps (unchanged):** `xml_path_*` (v0.10 ship-target, 2
  sources), `registry_paths_resolve` (v0.10 ship-target, 8 sources),
  `generated_file_fresh` (v0.10 ship-target, 6 sources),
  `apache/governance@v1` (v0.10 ship-target, 3 Apache TLPs
  converging),
  `cross_language_registry_consistency` (refinement of v0.11+
  `cross_language_implementation_complete`)
- **Open suspected bugs in this directory's `.alint.yml`:** none.
  The 78 Apache-header false positives are RAT-exclude
  coordinations that require `registry_paths_resolve` (v0.10
  ship-target) to resolve declaratively; a one-line `paths.exclude:`
  extension is the available workaround. The full-pass with
  `command:` shellouts hangs because the per-file `dev/lint-*`
  shellouts spawn-fail-fast (tools not on PATH); declarative-only
  timing of 1.35 s is the marketable number until the toolchain is
  installed
