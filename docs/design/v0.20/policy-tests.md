# Design doc: public policy tests

Status: Accepted design (2026-10-18); P1-P3 and P7 implemented.
Decisions: [ADR-0019](../../adr/0019-general-linter-program-and-plugin-architecture.md)
DEC-6 and [ADR-0022](../../adr/0022-policy-authoring-loop-contracts.md).
Demand evidence: [`../general-linter.md` section 5.8](../general-linter.md#58-policy-authoring-testing-and-debugging),
[`../general-linter.md` section 7.7](../general-linter.md#77-policy-tests), the existing
578-scenario corpus, and the 24 shipped bundled v1 rulesets.

## 1. Problem

alint already has a strong policy scenario language, but it is an unpublished, repository-internal
test harness. A downstream ruleset author must reproduce engine setup or run shell commands against
a real checkout. Neither path gives a versioned fixture contract, structured expectation matching,
hermetic capability control, or a reusable Rust runner. Bundled ruleset tests are centralized away
from the policy files they protect, so ownership and coverage can drift.

The existing harness is the right substrate. It already models repository trees, Git history,
check/fix/check sequences, violations, analysis failures, fix outcomes, and final trees; 207 of its
scenarios also drive the real rule documentation examples under ADR-0014. v0.20 promotes that
format instead of introducing the illustrative `cases:` wrapper sketched earlier in the program
document.

## 2. Surface area

### 2.1 Command

The public command is:

```text
alint test [PATH ...]
  [--tag TAG]...
  [--filter TEXT]
  [--update-snapshots]
  [--allow-spawn]
  [--allow-host-keyring]
  [--format human|json]
```

With no path, discovery searches `alint-tests/**/*.yml` and `alint-tests/**/*.yaml` below the
current directory. A path may name a YAML file or a directory; directory traversal uses the same
extensions and does not follow directory symlinks. Recursive discovery considers a YAML file a
scenario only when its top-level mapping carries the `test_version` discriminator. This lets
package-local config chains and repository fixtures contain YAML, including malformed YAML used as
lint input, without becoming scenarios themselves. A malformed document carrying a top-level
discriminator remains an invalid scenario; an explicit file is always validated as a scenario.
Explicit paths are de-duplicated after canonical confinement and results sort by forward-slash path
bytes, then scenario name. Finding no scenario, or filtering every discovered scenario out, is a
configuration error rather than a successful empty run.

`--tag` is an AND filter when repeated. `--filter` is a case-sensitive substring match against the
scenario name or normalized source path. Filtering changes selection only; it cannot relax schema,
confinement, or capability checks. Execution is sequential in v1. Parallel scheduling can be added
later without changing the document contract once deterministic resource limits are designed.

The command does not consume the caller's `--config`, `--as-of`, `--changed`, `--base`, baseline,
or fix flags. Each scenario owns those inputs. Passing an incompatible global option is an error,
not a silently ignored setting.

Command exit codes are:

| Exit | Meaning |
|---:|---|
| 0 | Every selected scenario was valid and all expectations and snapshots matched. |
| 1 | At least one scenario executed but an expectation or snapshot did not match. |
| 2 | Discovery, test schema, capability grant, or harness-owned fixture/config-file/snapshot path was invalid. |
| 3 | The runner could not establish or operate its sandbox, or encountered an internal error. |

A configuration error produced by the config under test, including its own local-extends/resource
confinement or the runner's no-remote policy, is a step observation and may be expected. An
unexpected one is an expectation mismatch (exit 1). Harness-owned source paths and missing
capability grants are test-definition errors (exit 2), not expectable config behavior.
Filesystem, process-control, serialization, or engine invariant failures in the runner are
operational errors and cannot be blessed as expected behavior.

Discovery, YAML/schema parsing, package-path validation, regex compilation, and duplicate snapshot
target checks complete for the whole selected set before any scenario executes. Any failure in that
phase emits all discovered validation errors, executes no rule/command, and exits 2. Expectation
mismatches do not short-circuit later scenarios; an operational error aborts because subsequent
results would not be trustworthy.

### 2.2 Versioned YAML document

One YAML document defines one scenario. The required root discriminator is independent of config
`version`:

```yaml
test_version: 1
name: local policy rejects an unpinned action
tags: [actions, check]

capabilities:
  spawn: false
  host_keyring: false

given:
  tree:
    .github:
      workflows:
        ci.yml: |
          jobs:
            build:
              steps:
                - uses: actions/checkout@v4
  config: |
    version: 1
    rules:
      - id: pins
        kind: pinned_references
        paths: .github/workflows/*.yml
        level: error
  env:
    RELEASE_CHANNEL: test
  as_of: 2026-10-18

when: [check]

expect:
  - exit: policy_failure
    violations:
      - rule: pins
        level: error
        path: .github/workflows/ci.yml
        line: 4
        message:
          regex: "immutable.*commit"
    analysis_failures: []
```

Unknown fields and unknown enum values are rejected. Duplicate YAML keys, explicit tags and nulls,
aliases whose expanded shape exceeds the shared bound, and excessive flow nesting are rejected by
the guarded parser contract. Optional fields are omitted rather than set to `null`.
`test_version: 1` is an integer, not a quoted range. A newer test version fails with the source path
and running runner version.

The committed JSON Schema is `schemas/v1/policy-test.json`. It is generated from the Rust model,
contains `additionalProperties: false` at every closed object, and includes the bounded scalar and
path constraints which JSON Schema can express. Runtime validation remains authoritative for
canonical paths, symlinks, mutual exclusion, capability use, and cross-field step rules.

### 2.3 Fixture and config sources

`given` requires exactly one repository source:

- `tree`: the existing recursive `TreeSpec` mapping;
- `fixture`: a relative path to a directory below the scenario document's directory.

It also requires exactly one config source:

- `config`: the existing inline YAML scalar;
- `config_file`: a relative path to a config file below the scenario document's directory.

`fixture` and `config_file` reject absolute paths, parent components, Windows prefixes, NULs,
non-UTF-8 path components, and canonical or symlink resolution outside the scenario package root.
The fixture is copied into a new temporary repository root. Config bytes are read through a
read-only package source provider confined to the canonical scenario-package root. A loader context
carries the distinct roots:

- repository-relative selectors, reads, fixes, baseline references, and Git operations use the
  temporary repository root;
- config-relative `extends` resolution uses the package source provider and cannot enter the
  temporary repository or escape into a parent of the package.

The provider assigns inline config a virtual path in the scenario document's directory, so local
extends use the same package-relative rules as `config_file`. It canonicalizes and confines each
requested local source before reading it, memoizes the bytes for the scenario run, and exposes only
YAML sources actually reached by production `extends` resolution. This avoids both copying the
whole package and implementing a second closure resolver. Bundled `alint://` sources are allowed.
`https://` sources are rejected before cache lookup, DNS, or a fetcher call, including when a
matching cache entry exists.

The fixture copy refuses `.git` entries (any name a file system opens as `.git`, such as `.GIT`,
`.git.` or `GIT~1`) and escaping symlinks. It preserves only relative in-root file symlinks whose
complete chain resolves inside the fixture and whose unchanged link text resolves inside the copied
temporary tree; absolute links, directory links, dangling links, cycles, and links not
representable on the host are rejected. The copy writes symlinks after the other entries of their
directory, then reads where each copied link leads and refuses one that leads outside the temporary
repository: link text that resolves inside the fixture can resolve elsewhere from the copy's place.

An inline `tree` follows the same rules, with directory links allowed. A symlink's target resolves
as the file system resolves it, through each symlink it reaches before a later `..` (with
`a/c/t -> ../../b`, `a/c/t/../../../x` is above the root, not `x`), and must reach an entry of the
tree. No entry may sit under a symlink, and the tree writes its symlinks after every other entry, so
no write follows one. The runner checks the written tree's links as it checks a fixture's, and
checks every link in the repository again after each fix step, since a fix that moves a link
(`relocate`) changes where its relative text leads. No link is rewritten to a source
checkout path. An unsupported fixture is skipped only under an explicit platform tag already used
by the corpus, never silently weakened. Device nodes, sockets, FIFOs, and setuid/setgid bits are
rejected. Executable permission is preserved on Unix; other permissions are normalized.
`given.git` creates all repository metadata after the copy.

`allow_out_of_root` is forced off in public policy tests. A config which requests it is rejected,
even with a host capability flag. Tests for that escape hatch remain internal security tests; a
shareable policy package must not read the package checkout or host filesystem.

### 2.4 Deterministic inputs

`given.env` is an optional string-to-string mapping. The config interpolation provider and any
allowed command receive that mapping, not the host process environment. Every value returned by
the provider carries sensitive provenance when used as option, message, condition, command, or
other dynamic evidence; resulting findings/errors are redacted before observations or diagnostics
reach the runner. Configured rule ids and repository paths remain visible structural identity under
the W5 contract and therefore must not contain secrets. Unknown host variables behave as absent.
The runner supplies only the minimum platform variables needed to locate an explicitly granted
executable (`PATH`, and on Windows the documented process-runtime variables); those values are not
available to config interpolation, `when:` conditions (`env.X`) or `${VAR}` expansions, which read
`given.env` and the runner's own sandbox values only.

`given.as_of` accepts the same `YYYY-MM-DD` value as the CLI and is injected directly into the
engine. When absent, the production HEAD-commit fallback applies inside the temporary Git
repository. No scenario reads wall time.

The existing `given.git` model remains intact: `init`, `add`, `add_force`, `commit`, and an ordered
`commits` chain with messages, staged paths, and dates. Git runs with hooks disabled, an isolated
empty HOME and config, no system/global config, no global excludes or attributes file (with the host
keyring granted HOME is the host's, so `XDG_CONFIG_HOME` points into the sandbox), no credential or
signing helper, no replacement refs, no optional locks inherited from the host, and fixed
author/committer identity. Its repository discovery stops at the sandbox
(`GIT_CEILING_DIRECTORIES` is the sandbox's parent), so a test without `given.git` never reads a
repository the host's temporary directory is in, and a fix never writes or reads a Git directory,
so a test cannot make one for the next Git command to read. User-controlled paths follow `--`;
messages and dates are environment/arguments, never shell text.

### 2.5 Capability declaration and grant

The v1 document has a closed capability map:

```yaml
capabilities:
  spawn: true
  host_keyring: false
```

Both values default to false. A configured rule whose `KindMeta` says it can spawn, a fixer with a
spawn operation, or a host-keyring consumer is rejected before any configured rule or fixer
executes unless the corresponding document value is true. The declaration alone is not authority:
the invoker must also pass `--allow-spawn` or `--allow-host-keyring`. Conversely, a host flag cannot
grant a capability the document did not declare. Diagnostics identify the rule/fix operation and
both missing grants.

Spawned commands start with the temporary repository as their working directory, inherit the
deterministic environment, receive bounded stdin/stdout/stderr and the production timeout, and are
killed as a process group on timeout. This is not an OS sandbox: after both grants, native code may
access any host filesystem or network resource allowed to the invoking user. CLI help states that
`--allow-spawn` grants full code-execution authority. Shell interpretation occurs only where the
production rule kind's documented contract uses a shell; the test runner never adds one.
Command output follows the normal W5 contract: it is visible unless the rule is explicitly
`sensitive: true` or contains other sensitive provenance. The spawn grant alone does not imply
redaction.
Host-keyring access retains the production verification behavior but cannot use host environment
interpolation.

Git setup does not require `spawn: true`: it is fixed runner machinery with a closed executable and
argument surface. Git-aware rule evaluation is likewise a declared built-in engine dependency, not
arbitrary process authority. Any future plugin capability requires a new test schema version or an
explicitly backward-compatible closed-map extension plus its own two-key host grant.

### 2.6 Steps and expectations

v1 preserves the existing ordered string steps:

- `check`;
- `fix` (Safe applicability);
- `fix_unsafe` (Safe plus Unsafe applicability);
- `fix_dry_run` (Safe applicability, no writes);
- `check_changed` (working-tree changed set, requiring `given.git`).

`when` and `expect` have equal nonzero length. Every expectation requires `exit` plus at least one
step-specific inline assertion beyond `exit`: an explicit present result/fix list (including an
empty list), `non_convergent`, `diff`, or the required configuration-error message. Each step
reloads the effective config from the provider's immutable byte snapshot and walks the current
temporary tree, matching a new production command invocation. Fix mutations are therefore visible
to later steps while source-package changes cannot alter a run in progress. A fix scenario must
assert a fix outcome, a diff, or `expect_tree`; an all-wildcard fix is invalid.

Each expectation has this shape:

```yaml
exit: success # success | policy_failure | fix_non_convergent | configuration_error
violations: []
analysis_failures: []
notes: []
applied: []
skipped: []
suggested: []
unfixable: []
non_convergent: false
diff: ""
```

`exit` projects the corresponding production command with its default warning threshold:
`policy_failure` means an error-level violation or any analysis failure would produce exit 1.
Warning/info violations may coexist with `exit: success`. `fix_non_convergent` is valid only for a
fix step whose complete `FixReport` has `non_convergent: true`; it projects the production fix
exit 2 while retaining all fix-result assertion channels.

An expected config failure uses a one-step scenario and must include a redacted message matcher:

```yaml
when: [check]
expect:
  - exit: configuration_error
    configuration_error:
      message:
        regex: 'requires alint 0\.21\.0'
```

No report/fix channels are allowed on that expectation because evaluation never started. Exact and
regex semantics match finding messages. A bare `exit: configuration_error` is invalid: it would
only test that some error occurred and could bless the wrong failure. Operational runner errors are
not an expected exit class.

A present result list is exact: every actual record must match one expected matcher and every
matcher must match its `count` (default 1). `count` expands to repeated matcher nodes and matching
uses a deterministic maximum bipartite assignment, so overlapping broad matchers pass only when a
complete one-to-one assignment exists; declaration order cannot choose a false pass or failure.
Diagnostics show canonical unmatched actual and expectation sets. An omitted list does not assert
that channel. Authors must use `violations: []` or `analysis_failures: []` to assert absence. This
preserves focused tests while enabling ADR-0012 positive completeness where needed.

A violation matcher supports the existing `rule`, `level`, `path`, and one-based half-open source
coordinates, plus:

```yaml
message:
  exact: "..."       # exactly one of exact or regex
count: 1
related:
  - label: "declared here"
    path: VERSION
    line: 1
    column: 1
    end_line: 1
    end_column: 6
```

Omitting `message` is an explicit wildcard; an empty string is an exact empty message. Regex uses
Rust regex syntax, is compiled once at document validation, and is subject to the regex size bound.
A present `related` list is exact after production canonical ordering. Notes support `kind`,
`message`, `path`, and range matchers. Analysis failures support `rule`, stable failure `kind`,
`path`, range, and message matching. Sensitive actual text is redacted before matching and before a
mismatch diagnostic; a test cannot assert a secret value by exact or regex text.

`diff` is the canonical unified diff emitted by the fix report for that step, with forward-slash
paths and LF line endings. `expect_tree` and `expect_tree_mode` retain their existing strict/contains
semantics after the last step. A content mismatch names the path and mismatch class but never emits
expected/actual file bytes or a content hash; this prevents generated secrets from leaking through
assertion diagnostics. Config sources and caches are outside the repository root, so runner
machinery no longer needs to be filtered from tree comparison.

The optional `docs` block remains in the public schema because the same document is the executable
ADR-0014 rule example. `alint test` validates it but does not render documentation. alint's docs
exporter consumes `title`, `case`, canonical built-in `kind`, and `order` from the same parsed model.
It is metadata, not a second assertion channel.

### 2.7 Snapshots

A scenario may add one package-relative `snapshot` JSON path. The snapshot contains the canonical,
already-redacted structured observations for all steps. It deliberately excludes final-tree bytes
and content digests; `expect_tree` is the tree contract and hashes could disclose equality for
low-entropy secrets. It also excludes temp paths, timestamps, process IDs, cache paths, and
source-checkout absolute paths. The snapshot uses its own `schema_version: 1` and stable
`kind: "policy-test-snapshot"` envelope.

Snapshots supplement inline expectations. The required `exit` alone is not a positive assertion;
every step must carry at least one of the explicit inline observation assertions defined in
section 2.6 even when it has a snapshot. `--update-snapshots` renders updates into temporary files
only after every selected scenario's inline expectations pass; if any scenario fails, no target is
replaced.
Targets are preflighted, then replaced in canonical order with an
atomic per-file rename. A process/filesystem failure may leave an already-renamed subset, is reported
as exit 3, and is never described as a transactional multi-file update. Writes are symlink-safe and
confined below the scenario package root. Discovery rejects two scenarios targeting the same
snapshot. A normal run never writes the source package.

### 2.8 Human and JSON reports

Human output prints one source/name header, one line per step, and only the details needed to explain
a mismatch. Paths are package- or fixture-relative; text is terminal-sanitized; tree mismatches
never render file bytes or hashes. A final count shows passed, failed, invalid, and skipped-platform
tests. The command is quiet about passing finding details unless verbose output is added in a
future release.

JSON uses `schemas/v1/policy-test-report.json`:

```json
{
  "schema_version": 1,
  "kind": "policy-test-report",
  "tests": [],
  "summary": {
    "selected": 0,
    "passed": 0,
    "failed": 0,
    "invalid": 0,
    "skipped": 0
  }
}
```

Each executed test record includes normalized source, name, tags, status, ordered step observations,
and structured mismatches. Invalid records carry structured issues; discovery or batch-operational
issues are top-level because no test may own them. The report contains no generated-at field. Stable
policy-test report JSON follows the normal additive-before-breaking schema discipline; snapshots are
stable within their declared schema. Human wording remains non-API.

### 2.9 Published runner crate

The existing `alint-testkit` crate becomes publishable at the workspace version. It exposes the
versioned `PolicyTest` model, schema generation entry points, `Runner`/`RunnerOptions`, structured
policy-test results and mismatches, tree fixture types, aggregate report types, and validation
errors. Public errors are typed and non-exhaustive. The high-level `Runner::run` always validates,
confines, and applies capability policy; an unchecked public execution path is not exposed.
`Runner::preflight` lets batch callers establish definition, package, config, and capability safety
without evaluating a rule. Deferred snapshot updates execute once and expose an explicit commit
step after every selected test's inline assertions pass.

`proptest` strategies and repository-only generators move behind a non-default `strategies`
feature or into test code. The crate's default dependency graph contains only runtime runner needs.
Release ordering publishes it after `alint-core`, `alint-dsl`, and `alint-rules` and before the CLI.
A package-content check verifies licenses, README, schemas/docs links, and no policy-test corpus or
build artifact is accidentally shipped.

The CLI depends on this runner. `alint-e2e` also loads all policy-test documents through it.
Programmatic invariant tests use the public validated execution path rather than retaining an
unchecked validation bypass.

### 2.10 Bundled ruleset ownership

Bundled policy tests live in a parallel ownership tree in the same crate:

```text
crates/alint-dsl/rulesets/v1/<bundle>.yml
crates/alint-dsl/ruleset-tests/v1/<bundle>/fail.yml
crates/alint-dsl/ruleset-tests/v1/<bundle>/pass.yml
```

The test tree is deliberately outside `rulesets/v1/`: the bundle embedder and facts generator scan
that source tree recursively for YAML, so placing scenarios below it would accidentally publish and
count tests as rulesets.

Each test config extends `alint://bundled/<bundle>@v1`, exercising the compiled/packaged resolver,
not an inline copy of its rules. Every shipped bundle requires at least one fail and one pass
scenario. Additional boundary, fix, or composition cases are demand-driven. Catalog-wide tests
cover bundle filtering/composition and every declared `requires.alint` value without duplicating the
same assertion for each bundle.

A runtime-catalog parity gate compares every shipped `(name, revision)` with its owned-test
directory and rejects orphaned tests or untested bundles. The original 48 centralized bundled
scenarios migrated into the ownership tree, which now contains 93 scenarios. ADR-0014 examples may
point at these documents; docs export still executes the exact fixture.

## 3. Semantics

The runner pipeline is fixed:

1. Discover and canonicalize source documents without following directory symlinks.
2. Parse with shared YAML depth/alias/duplicate-key guards and deserialize the declared test
   version.
3. Validate cross-field invariants, package-relative paths, snapshot uniqueness, regex bounds,
   steps/expectations, and declared capabilities.
4. Create the confined, read-only, memoizing package config provider and the distinct temporary
   repository root; copy or materialize only the repository fixture.
5. Create isolated Git state and deterministic input providers.
6. For each step, load the effective config through the production recursive loader and no-network
   policy, walk the repository, then invoke the production engine check/fix path.
7. Redact and canonicalize observations before matcher evaluation, mismatch construction, JSON,
   human output, or snapshot comparison.
8. Verify the final tree, then optionally compare or atomically update the snapshot.
9. Delete the complete temporary repository/cache root on normal and error paths.

A scenario passes only when setup, every step expectation, final tree, and snapshot match. Platform
skips are allowed only for the existing explicit tags (`linux-only`, `unix-only`, and
`requires-case-sensitive-fs`) and are reported. An unsupported platform without a matching tag is a
failure, not an implicit skip.

Scenario source parsing and config parsing remain separate. `test_version` never changes config
semantics; `version` and `requires.alint` are evaluated by the config loader documented in
[`config-requirements.md`](config-requirements.md). A runner may support multiple test versions in
the future, but it must dispatch by the declared version before typed deserialization.

## 4. False-positive surface

Expectation matching can pass incorrectly if two broad matchers consume the same actual finding,
if omitted channels are mistaken for empty channels, or if warning-level findings are conflated with
process exit. One-to-one counted matching, explicit omission semantics, and separate `exit` checks
remove those ambiguities. Diagnostics list all unmatched records rather than stopping at a broad
first match.

Fixture isolation can itself create false behavior. Copying config into the repository would make it
walk-visible; resolving local extends without a confined, memoizing package provider could admit
undeclared parent files or let source edits change later steps; inheriting HOME, Git config,
credentials, or environment could satisfy a policy only on the author's machine. Separate roots and
injected providers make those dependencies explicit.

A snapshot can overfit wording and conceal missing assertions. Inline positive assertions remain
mandatory, snapshots are structured/redacted, and updates require those assertions to pass first.
Snapshots do not replace behavior checks.

Capability declarations can be confused with authority. The two-key model ensures a committed test
cannot execute a command or read a keyring merely because a developer runs `alint test`. Conversely,
the CLI grant alone cannot widen an undeclared scenario.

## 5. Implementation notes

P1 adds the versioned model and schema beside the repository-internal scenario model so it does not
expose a partially capable runner. P2 migrates all 578 documents and the existing runner in one
mechanical cutover, then removes the internal model. The guarded parser builds one untyped value,
checks its version discriminator, and dispatches that same value to the v1 type; it does not parse
untrusted YAML twice through different guard policies.

`LoadOptions` gains injected environment lookup, remote policy, config source provider/confinement,
and repository root instead of testkit-specific loader forks. Production defaults retain current
behavior. Policy tests pass a memoizing package provider, a no-network fetch policy whose invocation
is itself a test failure, and an environment provider backed only by `given.env`. The production
recursive loader remains the only component which interprets `extends`.

Fixture copies, package-provider reads, and snapshot writes use descriptor-relative or
open-parent-then-verify helpers already used for safe report/baseline writes where possible. Every
component is revalidated after canonicalization to close symlink races. On platforms without the
required primitive, the runner refuses unsafe fixture/config shapes instead of falling back to
lexical checks.

Structured mismatch types are the source for both renderers. Assertion code must not compare
formatted human strings. Canonical paths use `/`; ranges remain one-based and half-open to match the
scenario corpus. Ordering uses source path, step ordinal, channel, rule id, finding path bytes,
range, message, and original stable occurrence ordinal.

P1/P2/P3 are a large change split at the serialized model, reusable runner, and CLI boundaries.
They add no new third-party workspace dependency: schema derivation, regex matching, temporary
directories, serialization, and path handling reuse already-approved workspace crates and the
standard library. Any proposed descriptor-relative filesystem crate requires a separate dependency
and cargo-deny review before it can replace the conservative reject-unsupported fallback.

## 6. Tests

Permanent coverage includes:

- JSON Schema golden generation and deserialize/schema parity for every field, enum, default,
  mutual exclusion, and unknown-field rejection;
- migration parsing for all 578 current scenarios, with a gate that no alternate corpus parser
  exists;
- inline tree and directory fixture parity, binary/empty/executable files, case-sensitive tags,
  in-root symlinks, escaping symlinks, special files, absolute/parent/Windows-prefixed paths, and
  source mutation checks;
- inline and file configs, multi-level local extends, bundled extends, cycles, depth bounds,
  missing sources, remote URLs with a fetcher that panics if called, and repository/config-root
  separation;
- absent host env versus explicit `given.env`, sensitive interpolation, fixed analysis time, unborn
  and committed Git histories, disabled hooks/config/credentials/signing, changed-path behavior,
  and cleanup after failures;
- spawn and keyring with all four declaration/grant combinations, nested/template capability
  discovery from `KindMeta`, timeouts, output bounds, process-group cleanup, and no shell added by
  the runner;
- every step kind, warning-versus-error exit projection, converged/non-convergent fix reports,
  expected and unexpected config errors, required config-error message matching, terminal-error
  validation, fix state across reloads, canonical diff, and strict/contains final trees;
- exact/regex/omitted message matching, duplicate counted findings, one-to-one broad matchers,
  notes, every analysis-failure kind, related locations, ranges, omitted versus empty channels, and
  fully redacted mismatch diagnostics;
- snapshot compare/update, required inline assertions, duplicate targets, confinement, symlink
  replacement, atomic-write failure, canonical JSON, and planted-secret scans;
- discovery default/explicit files/directories, `.yml`/`.yaml`, duplicate arguments, symlinked
  directories, deterministic ordering, tag/filter behavior, zero-selection errors, and command
  exit codes;
- positive human/JSON completeness, terminal-control sanitization, report schema validation, and
  byte-identical repeated JSON runs;
- runner crate package contents and a consumer crate that depends on the packaged artifact and runs
  a scenario through its public API;
- installed CLI smoke in a fresh downstream repository on Linux, macOS, and Windows;
- bundle/test catalog parity and execution of each bundle's pass/fail cases through its
  `alint://bundled` URI.

A separate adversarial smoke plants one unique canary in each host environment, fixture secret,
config environment interpolation, explicitly sensitive command output, sensitive source value, fix
detail, and source checkout path. It scans human output, JSON, snapshots, diagnostics, and every
runner-created artifact outside the isolated temporary repository. After cleanup the temporary
repository must no longer exist; no protected canary may remain in an output or retained artifact.

## 7. Open questions

All Phase 1b policy-test questions are resolved:

- one existing scenario per YAML document, not a new `cases:` wrapper;
- `test_version` is independent from config `version`;
- the current `alint-testkit` is published rather than wrapped by a second crate;
- real directory fixtures and config files are supported under package confinement;
- remote extends are always disabled;
- process and keyring access use separate two-key grants;
- exact, regex, and omitted message semantics are explicit;
- snapshots are optional supplements, never the only assertion;
- bundle tests move to bundle-owned directories and use the packaged resolver.

Deferred beyond v0.20: parallel execution/jobs, remote fixture registries, plugin capabilities,
parameterized cases, shell-specific matrix expansion, test sharding protocol, and a second test
schema version. None is required to implement or safely use v1.
