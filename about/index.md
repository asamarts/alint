---
title: About alint
description: Origin, non-goals, license, links.
sidebar:
  order: 1
---

## Why alint exists

Most linters check the code inside files; alint checks the files themselves. The *filesystem shape* of a repository — which files exist, what they're called, what's inside them at the structural level, how they relate to each other — turns out to be where a lot of structural correctness lives, and where existing tooling is patchy. alint covers that surface in one declarative `.alint.yml`: 85 rule kinds across 13 families, 21 bundled ecosystem rulesets, structured queries (RFC 9535 JSONPath over JSON / YAML / TOML / XML), cross-file relational rules, conditional `when:` gates on per-run facts, and auto-fix.

When [Repolinter](https://github.com/todogroup/repolinter) was archived in early 2026 it took a piece of the OSS-baseline checking tooling with it; alint's `oss-baseline@v1` ruleset is a strict superset of Repolinter's default rules for users migrating in. See the [Repolinter-alternative landing](/repolinter-alternative/) and the [step-by-step migration guide](/migrating-from/repolinter/) for the full mapping.

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
