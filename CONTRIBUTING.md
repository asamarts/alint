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

Rust 1.85+ required (the workspace MSRV: `[workspace.package].rust-version` in `Cargo.toml`; `rust-toolchain.toml` pins the `stable` channel for development).

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
ci/scripts/test.sh                         # cargo test --workspace + bash CLI tests
ci/scripts/docs.sh                         # cargo doc -D warnings + docs-export/gen-schema/gen-facts/gen-roadmap/gen-arch/gen-model --check + likec4 validate + gen-mermaid --check
ci/scripts/check-version-pins.sh           # README/SECURITY/docs/site install snippets + npm pin to workspace version
ci/scripts/check-workspace-dep-floors.sh   # [workspace.dependencies] floors <= workspace.package.version
ci/scripts/dogfood.sh                      # cargo build --release + alint check on this repo
```

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

For per-file rules, add the rule to the `S6` macro bench scenario list if it's
a content rule that fans out over `**/*.rs`.

### PR conventions

- Conventional Commits style: `feat(rules): add no_lockfile_drift`,
  `fix(engine): scope_filter must consult ctx.index`, `perf(core): #[inline] Scope::matches`
- One logical change per PR. Refactors land separately from feature work.
- The PR description goes in the body, not the title.

### Branch protection

`main` is protected; PRs require:

- Passing the PR CI workflows: `ci.yml` (preflight: fmt/clippy/test/docs/version-pins/dogfood, plus the `bench-smoke` and advisory `perf-gate` jobs) and `cross-platform.yml` (Linux/macOS/Windows tests). (`release.yml` is tag-triggered only and does not gate PRs.)
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
