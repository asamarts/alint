# Contributing to alint

Thanks for your interest in alint. This doc covers how to file bugs, propose
features, and submit code.

## Quick links

- **Bugs / unexpected behaviour**: open an [issue](https://github.com/asamarts/alint/issues/new/choose) using the bug-report template
- **Feature ideas / new rule kinds**: open an issue with the feature-request template, or start a [Discussion](https://github.com/asamarts/alint/discussions) under "Ideas" if it's still half-formed
- **Help with a config**: open a Discussion under "Q&A" or an issue with the config-help template
- **Security vulnerabilities**: see [`SECURITY.md`](SECURITY.md), do **not** file a public issue
- **General chat / show-and-tell**: Discussions

## Filing a bug report

The bug template asks for:

- alint version (`alint --version`) and platform (Linux/macOS/Windows + arch)
- the smallest config that reproduces
- expected vs actual output
- relevant log output (run with `RUST_LOG=alint_core=info` for engine timing)

If you can't pull the offending repo into a minimal reproduction, attach
`alint debug bundle <path>` output (planned; until that lands, just describe
the tree shape).

## Proposing a new rule kind

The bar: a rule kind ships when it covers a use case at least 3 real
production repos need. The launch-prep validation pass
([`docs/development/launch-evidence.md`](docs/development/launch-evidence.md)) catalogues real-repo use cases
that don't have alint coverage today; new rule kinds usually surface from there.

If you have a use case that isn't on that list, open an issue with the
feature-request template and link to:

- the repo (or 2-3 repos) that have the use case
- the existing tooling they use (custom shell script, eslint plugin, etc.)
- a sketch of how the rule would be configured in YAML

Bundled-ruleset additions (a new ecosystem ruleset, e.g. `swift@v1`) follow the
same pattern: link to ≥3 production repos that would adopt it.

## Submitting code

### Setup

```sh
git clone https://github.com/asamarts/alint
cd alint
cargo test --workspace            # ~5s
cargo run -p alint -- check       # dogfood: alint lints itself
```

Rust 1.88+ required (the workspace MSRV: `[workspace.package].rust-version` in `Cargo.toml`; `rust-toolchain.toml` pins the `stable` channel for development). The MSRV rises only when a dependency or language feature needs it, to a stable release at least six months old, in a minor release or a security patch, with a CHANGELOG line (the policy in [`RELEASING.md`](RELEASING.md#release-version-policy)).

### Pre-push checklist

`ci/scripts/preflight.sh` bundles the gates CI's `ci.yml` workflow runs
against pushes to `main`:

```sh
ci/scripts/preflight.sh   # fmt + clippy + test + docs + version-pins + dep-floors + dogfood
```

Under the hood it runs:

```sh
ci/scripts/fmt.sh                          # cargo fmt --check
ci/scripts/clippy.sh                       # cargo clippy --workspace --all-targets -- -D warnings
                                           #   + cargo clippy -p alint-core --all-targets
                                           #     --features yaml-reference -- -D warnings
ci/scripts/test.sh                         # cargo test --workspace + bash CLI tests
ci/scripts/docs.sh                         # cargo doc -D warnings + public-api-parser-gate
                                           #   + docs-export/gen-schema/gen-facts/gen-categories/
                                           #     gen-roadmap/gen-arch/gen-model --check
                                           #   + check-version-pins.sh + likec4 validate
                                           #   + gen-mermaid --check + strip-since tests
                                           #   + test-pin-docs-counts.sh
ci/scripts/check-version-pins.sh           # README/SECURITY/docs/site install snippets + npm pin to workspace version
ci/scripts/check-workspace-dep-floors.sh   # [workspace.dependencies] floors <= workspace.package.version
ci/scripts/dogfood.sh                      # cargo build --release + alint check on this repo
```

Not in `preflight.sh`: the release preflight (`release.yml`) runs these as well, and CI runs
all but the release-only ones. Each is slower, needs the network or a tool a laptop may lack, or
checks a release. `ci/scripts/test-release-preflight-parity.sh` fails when the release preflight
runs a script that is neither in `preflight.sh` nor in this list.

- `ci/scripts/yaml-compare.sh`: the workspace's tests again, in YAML compare mode (ADR-0041),
  which checks every decode against libyaml and saphyr. Run it when a change touches how alint
  reads YAML (see [Testing requirements](#testing-requirements)).
- `ci/scripts/msrv.sh`: compiles the workspace with the exact toolchain named by
  `[workspace.package].rust-version`, so the MSRV is never hand-copied into CI.
- `ci/scripts/shell-tests.sh`: every `ci/scripts/test-*.sh` harness, about 3 minutes. Run it when
  you change a script under `ci/scripts/`.
- `ci/scripts/build.sh`: `cargo build --workspace --locked`, which `test.sh` mostly covers.
- `ci/scripts/deny.sh` and `ci/scripts/audit.sh`: the advisory gates, which need the network (see
  [Dependency advisories](#dependency-advisories)).
- `ci/scripts/examples-validate.sh`: every `examples/*/.alint.yml` against the schema.
- `ci/scripts/demo-drift.sh`: the README demo against the release binary.
- `ci/scripts/packaging-check.sh` and `ci/scripts/test-npm-install.sh`: the installer, the npm
  shim, the Dockerfile and the pre-commit hooks, and an install of the npm packages from a local
  registry.
- `ci/scripts/install-witforge.sh`, `ci/scripts/witforge-consumer.sh` and
  `ci/scripts/test-witforge-consumer.sh`: the plugin contract, against a pinned witforge.
- `ci/scripts/check-release-version.sh` and `ci/scripts/pin-release-toolchain.sh`: release only;
  the tag must match the workspace version, and the release builds with a pinned toolchain.
- `ci/scripts/yaml-memory-release.sh`: release only; decoding YAML, ayaml's peak heap must be
  within 1.2x of libyaml's (ADR-0042). CI runs the same test against today's ratio in
  `yaml-compare.sh`.

### Private native macOS validation

Maintainers with access to a privately managed Mac may supplement
`cross-platform.yml` with native tests before pushing platform-sensitive changes
or release candidates. The host identity and connection details stay in local
SSH configuration and MUST NOT appear in the repository, CI variables, issues,
or logs.

Only immutable, maintainer-reviewed commits may run on a personal host. Cargo
build scripts and tests execute native code; arbitrary fork or pull-request code
requires a disposable VM or dedicated unprivileged account with no personal
files, credentials, unlocked Keychain items, or forwarded SSH agent. Follow the
source-transfer, environment-isolation, test, evidence, and cleanup requirements
in [`docs/development/private-macos-validation.md`](docs/development/private-macos-validation.md).

This private run supplements rather than replaces GitHub-hosted macOS CI. Linux
`ci/scripts/preflight.sh` remains required because it owns the Ubuntu-defined
docs-export and generated-artifact gates.

### Dependency advisories

`ci/scripts/deny.sh` (cargo-deny) and `ci/scripts/audit.sh` (cargo audit) fail
CI and the release preflight on any RustSec **vulnerability** advisory against
a crate in `Cargo.lock`; `unmaintained` / `unsound` advisories and yanked
crates are reported as warnings. Fix a red advisory gate by upgrading the
affected dependency. Only when the advisory provably does not affect alint,
waive it in `deny.toml`, the single waiver list both gates read:

```toml
[advisories]
ignore = [
    { id = "RUSTSEC-YYYY-NNNN", reason = "why alint is unaffected; tracking issue" },
]
```

Remove the waiver as soon as a fixed version is available.

### Bumping the workspace version

Single source of truth: `[workspace.package].version` in `Cargo.toml`. The
README, SECURITY.md, and `docs/site/integrations/**` install snippets must
pin to the same value. To bump in one pass:

```sh
bash ci/scripts/bump-version.sh 0.9.21   # NEW workspace version
```

That updates Cargo.toml, every install snippet, `npm/package.json`, and
inserts a stub CHANGELOG entry. Manual followups it doesn't do (printed
at the end of the run): fill in the CHANGELOG body, and bump the matching
string in alint.org's `src/pages/index.astro` JSON-LD `softwareVersion`
and `src/pages/roadmap.astro` "Latest release" line (cross-repo). Both
cross-repo refs are gated by alint.org's `check-pins.yml` workflow (runs
on PR + push + daily cron), so an alint release without the matching
alint.org bump fails CI there.

`ci/scripts/check-version-pins.sh` runs in preflight and as a dogfood alint
rule (`install-snippets-match-workspace-version` in `.alint.yml`), so any
drift fails CI before publish.

All seven must pass before opening a PR. The wrapper falls through on
failure (rather than fast-exiting on the first one) so a single run shows
the full set of things to fix instead of fix-rerun-fix-rerun. Skip a
specific check while debugging: `PREFLIGHT_SKIP=clippy bash ci/scripts/preflight.sh`.

To wire preflight into a `git push` hook so an unformatted block bounces
locally instead of consuming a CI minute:

```sh
git config core.hooksPath ci/githooks
```

Skip the hook for one push (e.g. WIP branch): `git push --no-verify`.

If clippy gates seem aggressive, that's intentional. `-D warnings` plus
pedantic clippy keeps the codebase quiet by default.

### Editing the roadmap

`docs/design/ROADMAP.md` is the canonical roadmap. The generated public
version at alint.org/docs/about/roadmap/ is produced by `xtask
gen-public-roadmap`, invoked automatically by the `docs-bundle`
workflow on every push to `main`. To elide engineering-process notes,
holding-bay backlogs, or any other content that belongs only in the
internal source, wrap the section in paired HTML-comment markers:

```text
<!-- alint:internal-start -->
... content visible only in canonical ROADMAP.md ...
<!-- alint:internal-end -->
```

The markers are code-fence-aware (example markers inside ```` ``` ```` blocks
are literal, not parsed as block delimiters). Nested, orphan, and
unclosed markers fail at generator time with a line-numbered error.
Full convention in [`docs/design/v0.11/roadmap_generator.md`](docs/design/v0.11/roadmap_generator.md).

Verify locally before pushing:

```sh
cargo run -p xtask --release -- gen-public-roadmap --output /tmp/public-roadmap.md
diff docs/design/ROADMAP.md /tmp/public-roadmap.md   # shows what got elided
```

The `docs-bundle.yml` workflow runs the same generator on every push to
`main`, so a malformed marker pair (nested / orphan / unclosed) fails CI
there before alint.org rebuilds.

### Where the code lives

- `crates/alint-core/`: engine, walker, rule trait, config AST. The structural
  fixes live here (e.g., v0.9.10's `Scope` ownership of `ScopeFilter`).
- `crates/alint-rules/`: rule kinds. Adding a rule kind starts here.
- `crates/alint-dsl/`: config-file parser + `extends:` resolver.
- `crates/alint-output/`: formatters (human/json/sarif/...).
- `crates/alint/`: the CLI binary.
- `crates/alint-bench/`: synthetic-tree generator + criterion micro-benches.
- `crates/alint-e2e/`: full end-to-end scenarios in `scenarios/check/<family>/`.
- `crates/alint-testkit/`: proptest strategies + shared test fixtures.
- `xtask/`: meta-tooling (bench-scale, docs-export, publish-benches).

See [`docs/development/rule-authoring.md`](docs/development/rule-authoring.md)
for the rule-author workflow (4 steps: parse → build → evaluate → e2e).

### Testing requirements

Every new rule kind needs:

- Unit tests in the rule's source file (covering the happy path + edge cases)
- A pass scenario + a fail scenario in `crates/alint-e2e/scenarios/check/<family>/`
- An entry in the bundled rulesets if applicable
- Coverage via `coverage_audit_*.rs` (the audit tests will fail if a new rule
  kind ships without scenarios, which is intentional)

For per-file rules, add the rule to the `S2` (`s2_content`) macro bench scenario
if it's a content rule that fans out over `**/*.rs`.

Tests create their repositories under the temporary directory, so `TMPDIR` must
not point inside a Git work tree: a test that expects to run outside any
repository would find that one and fail. Tests run `git` without the host's
system or global configuration and walk without the global excludes file
(`GIT_CONFIG_NOSYSTEM`, `GIT_CONFIG_GLOBAL` and `XDG_CONFIG_HOME` on the command;
`with_global_git_excludes(false)` on the walk), so what they set up does not
depend on the machine's Git setup. The CLI's tests start every process, `alint`
included, through `crates/alint/tests/hermetic.rs`, which also gives it an empty
`HOME`. Follow the same pattern in a new test, unless it tests the global
excludes file itself.

A change to how alint reads YAML, or a bump of ayaml, also runs the workspace's
tests in compare mode (ADR-0041), which checks every decode against libyaml and
saphyr. CI's `YAML compare` job and the release preflight run it; locally:

```sh
ci/scripts/yaml-compare.sh   # ALINT_YAML_PARSER=compare + the yaml-reference feature
```

A difference it reports is either ayaml's to fix or a deviation to document in
the differential harness's allowlist (`docs/design/yaml-parser.md`), which the
owner approves; `ALINT_YAML_COMPARE_LOG=<file>` lists the differences it
explained.

### PR conventions

- Conventional Commits style: `feat(rules): add no_lockfile_drift`,
  `fix(engine): scope_filter must consult ctx.index`, `perf(core): #[inline] Scope::matches`
- One logical change per PR. Refactors land separately from feature work.
- The PR description goes in the body, not the title.
- A squash merge writes the title and ` (#N)` as the commit subject on `main`, which must fit in
  120 characters; the `Pull request title` check measures it.

### Branch protection

`main` is protected; PRs require:

- Passing the PR CI workflows: `ci.yml` (preflight: fmt/clippy/test/docs/version-pins/dogfood, plus the `bench-smoke` and advisory `perf-gate` jobs) and `cross-platform.yml` (Linux/macOS/Windows tests). (`release.yml` is tag-triggered only and does not gate PRs.)
  - **Temporary security hold:** every PR currently runs the portable gate
    (fmt/msrv/clippy/test/build/audit/deny/docs/dogfood/examples/shell-tests) on
    ephemeral GitHub-hosted `ubuntu-latest` runners. The legacy fixed-label
    listener is offline, so box-only `bench-smoke` and `coverage` are skipped
    for PRs. The load-immune `perf-gate` is portable and runs on hosted
    capacity. Exact admitted human PRs may return to local compute
    only after the disposable one-job executor passes its isolation canaries.
    See `docs/design/v0.14/ci-fork-pr-isolation.md`.
  - Every workflow declares its `GITHUB_TOKEN` permissions. Ordinary test and
    build workflows are `contents: read`; release, package, docs and benchmark
    writers grant only the scopes used by the individual writer job. The
    repository's combined "create and approve pull requests" setting remains
    enabled solely because the benchmark recorder opens a review PR. Workflow
    code may create that PR but must never approve one; the shell-test policy
    gate rejects permission drift and approval operations.
- One approving review (currently a single-maintainer project; this is the
  spot to call out if you'd like to be added as a co-maintainer)

## Maintainership and governance

Currently a single-maintainer project (`@asamarts`). Decisions are made by the
maintainer, in consultation with anyone who's invested time in the affected
area. This will move to a more formal governance model once there are 3+
regular contributors.

Disagreements: file an issue or Discussion explaining the alternative; concrete
rationale wins over abstract preference.

## License

By contributing you agree that your contributions are licensed under the same
dual Apache-2.0 OR MIT license that covers the rest of the project. No CLA;
the inbound = outbound model applies.
