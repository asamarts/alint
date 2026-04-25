---
title: About alint
description: Origin, non-goals, license, links.
sidebar:
  order: 1
---

## Why alint exists

[Repolinter](https://github.com/todogroup/repolinter) was archived in early 2026, leaving an active-maintenance gap in repo-structure tooling. alint fills that gap with a superset of Repolinter's rule catalogue, plus first-class structured-query primitives (JSONPath over JSON / YAML / TOML), composable bundled rulesets, conditional rules gated on repo facts, and auto-fix.

## Non-goals

alint is deliberately **not**:

- a code / AST linter — use [ESLint](https://eslint.org/), [Clippy](https://doc.rust-lang.org/clippy/), [ruff](https://docs.astral.sh/ruff/)
- a SAST scanner — use [Semgrep](https://semgrep.dev/), [CodeQL](https://codeql.github.com/)
- an IaC scanner — use [Checkov](https://www.checkov.io/), [Conftest](https://www.conftest.dev/), [tfsec](https://aquasecurity.github.io/tfsec/)
- a commit-message linter — use [commitlint](https://commitlint.js.org/)
- a secret scanner — use [gitleaks](https://github.com/gitleaks/gitleaks), [trufflehog](https://github.com/trufflesecurity/trufflehog)

Scope is the filesystem shape and contents of a repository, not the semantics of the code inside it. For where alint fits in monorepo workflows specifically — including when to reach for Bazel, Cargo, pre-commit, or OpenSSF Scorecard instead — see [alint and monorepos](./monorepos/).

## Project links

- **Source**: [github.com/asamarts/alint](https://github.com/asamarts/alint)
- **Crates**: [crates.io/crates/alint](https://crates.io/crates/alint)
- **Rust API docs**: [docs.rs/alint](https://docs.rs/alint), [docs.rs/alint-core](https://docs.rs/alint-core)
- **Container**: [ghcr.io/asamarts/alint](https://ghcr.io/asamarts/alint)
- **Homebrew**: [asamarts/homebrew-alint](https://github.com/asamarts/homebrew-alint)

## License

alint is dual-licensed under either of:

- [Apache License 2.0](https://github.com/asamarts/alint/blob/main/LICENSE-APACHE) (SPDX `Apache-2.0`)
- [MIT License](https://github.com/asamarts/alint/blob/main/LICENSE-MIT) (SPDX `MIT`)

at your option. Contributions are dual-licensed the same way unless explicitly stated otherwise.
