# Contributing to alint

Thanks for your interest in alint. This doc covers how to file bugs, propose
features, and submit code.

## Quick links

- **Bugs / unexpected behaviour** — open an [issue](https://github.com/asamarts/alint/issues/new/choose) using the bug-report template
- **Feature ideas / new rule kinds** — open an issue with the feature-request template, or start a [Discussion](https://github.com/asamarts/alint/discussions) under "Ideas" if it's still half-formed
- **Help with a config** — open a Discussion under "Q&A" or an issue with the config-help template
- **Security vulnerabilities** — see [`SECURITY.md`](SECURITY.md), do **not** file a public issue
- **General chat / show-and-tell** — Discussions

## Filing a bug report

The bug template asks for:

- alint version (`alint --version`) and platform (Linux/macOS/Windows + arch)
- the smallest config that reproduces
- expected vs actual output
- relevant log output (run with `RUST_LOG=alint_core=info` for engine timing)

If you can't pull the offending repo into a minimal reproduction, attach
`alint debug bundle <path>` output (planned — until that lands, just describe
the tree shape).

## Proposing a new rule kind

The bar: a rule kind ships when it covers a use case at least 3 real
production repos need. The launch-prep validation pass
([`docs/launch-prep.md`](docs/launch-prep.md)) catalogues real-repo use cases
that don't have alint coverage today; new rule kinds usually surface from there.

If you have a use case that isn't on that list, open an issue with the
feature-request template and link to:

- the repo (or 2-3 repos) that have the use case
- the existing tooling they use (custom shell script, eslint plugin, etc.)
- a sketch of how the rule would be configured in YAML

Bundled-ruleset additions (a new ecosystem ruleset, e.g. `swift@v1`) follow the
same pattern — link to ≥3 production repos that would adopt it.

## Submitting code

### Setup

```sh
git clone https://github.com/asamarts/alint
cd alint
cargo test --workspace            # ~5s
cargo run -p alint -- check       # dogfood: alint lints itself
```

Rust 1.95+ required (see `rust-toolchain.toml`).

### Pre-commit checklist

The release.yml preflight gate runs the same checks CI runs:

```sh
ci/scripts/fmt.sh    # cargo fmt --check
ci/scripts/clippy.sh # cargo clippy --workspace --all-targets -- -D warnings
ci/scripts/test.sh   # cargo test --workspace + bash CLI tests
ci/scripts/docs.sh   # cargo doc -D warnings + xtask docs-export --check
```

All four must pass before opening a PR. If clippy gates seem aggressive,
that's intentional — `-D warnings` keeps the codebase quiet by default.

### Where the code lives

- `crates/alint-core/` — engine, walker, rule trait, config AST. The structural
  fixes live here (e.g., v0.9.10's `Scope` ownership of `ScopeFilter`).
- `crates/alint-rules/` — rule kinds. Adding a rule kind starts here.
- `crates/alint-dsl/` — config-file parser + `extends:` resolver.
- `crates/alint-output/` — formatters (human/json/sarif/...).
- `crates/alint/` — the CLI binary.
- `crates/alint-bench/` — synthetic-tree generator + criterion micro-benches.
- `crates/alint-e2e/` — full end-to-end scenarios in `scenarios/check/<family>/`.
- `crates/alint-testkit/` — proptest strategies + shared test fixtures.
- `xtask/` — meta-tooling (bench-scale, docs-export, publish-benches).

See [`docs/development/RULE-AUTHORING.md`](docs/development/RULE-AUTHORING.md)
for the rule-author workflow (4 steps: parse → build → evaluate → e2e).

### Testing requirements

Every new rule kind needs:

- Unit tests in the rule's source file (covering the happy path + edge cases)
- A pass scenario + a fail scenario in `crates/alint-e2e/scenarios/check/<family>/`
- An entry in the bundled rulesets if applicable
- Coverage via `coverage_audit_*.rs` (the audit tests will fail if a new rule
  kind ships without scenarios — that's intentional)

For per-file rules, add the rule to the `S6` macro bench scenario list if it's
a content rule that fans out over `**/*.rs`.

### PR conventions

- Conventional Commits style: `feat(rules): add no_lockfile_drift`,
  `fix(engine): scope_filter must consult ctx.index`, `perf(core): #[inline] Scope::matches`
- One logical change per PR. Refactors land separately from feature work.
- The PR description goes in the body, not the title.

### Branch protection

`main` is protected; PRs require:

- Passing `release.yml`-equivalent CI (preflight + cross-platform tests)
- One approving review (currently a single-maintainer project — this is the
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
