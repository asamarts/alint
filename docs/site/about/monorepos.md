---
title: alint and monorepos
description: Where alint fits in monorepo workflows, how it sits next to Bazel / Cargo / pnpm / CI / Scorecard / pre-commit, and where its scope deliberately ends.
sidebar:
  order: 2
---

alint's scope is **the filesystem shape and contents of a repository**, not the semantics of the code inside it. In a monorepo, that means alint enforces conventions like *"every package has a README"*, *"every workflow pins actions to SHAs"*, *"build artifacts aren't committed"*, *"workspace members agree on a license format"* — and stops there. Code-level concerns belong to the language toolchain; build-graph concerns belong to the build system.

This page is the framing for that boundary. If you're deciding whether alint belongs in your monorepo's CI, start here.

## Does alint fit?

| Your monorepo | Fit | Why |
|---|---|---|
| Cargo workspace | Strong | One-line bundled adoption + nested configs cover most needs. |
| pnpm / yarn / npm workspaces | Strong | Same — `node@v1` plus workspace conventions. |
| Polyglot OSS monorepos (Rust + Node + Python + Go + Java) | Strong | Multi-ecosystem rulesets layer cleanly; each is fact-gated, so unused languages contribute nothing. |
| Lerna / Nx / Turborepo | Good | Treat as workspaces; bundled adoption + nested configs. |
| Bazel / Buck2 / Pants hyperscale monorepos | Partial | Tree-shape rules apply. Package-iteration rules currently iterate every directory matching a glob; a per-iteration `when:` filter ("only iterate dirs containing a `BUILD` file") is on the v0.5 roadmap. |
| Custom-build kernels (Linux, Chromium, FreeBSD) | Partial | Tree-shape rules apply. Expect to write more custom rules and fewer ecosystem extends. |

If your top concern is build-graph correctness, dependency resolution, or code-level safety, the [Honest limits](#honest-limits) section below is the part to read first — alint isn't the right tool for those, and we'd rather you know now than after wiring it in.

## How alint fits with the rest of your toolchain

| Concern | Tools that own it | What alint adds |
|---|---|---|
| Build graph (targets, deps, caching, hermetic builds) | Bazel, Buck2, Pants, Cargo, pnpm, Make | alint runs *alongside*. It checks the *shape* of your `BUILD` / `Cargo.toml` / `package.json` tree (filenames, structure, required keys), not the targets they declare or how they build. |
| Code lint and format | ESLint, Clippy, ruff, gofmt, Prettier | Different scope. alint cares whether files exist, are well-named, have required headers, and don't contain forbidden content patterns; the language linter cares about the code inside them. |
| Static analysis / SAST | Semgrep, CodeQL | Out of scope. |
| Secrets | gitleaks, trufflehog | Out of scope for content scanning. alint *can* fail a committed `.env.production` via `hygiene/no-tracked-artifacts@v1` — the file shouldn't be there in the first place — but it doesn't grep for credential-shaped strings. |
| Dependency resolution and vulnerabilities | cargo-deny, npm audit, Renovate, Dependabot, Trivy | Out of scope. alint can require a `renovate.json` or `dependabot.yml` *exists*; the resolver tells you whether the dependencies in it are healthy. |
| Supply-chain signals | OpenSSF Scorecard | Adjacent, with overlap. alint's `ci/github-actions@v1` enforces Scorecard's Token-Permissions and Pinned-Dependencies checks at PR time; Scorecard runs as a periodic audit. They report the same finding from different angles. |
| Repo conventions / community health | Repolinter (archived 2026-02-06) | alint covers the same ground under active maintenance, with structured-query primitives, bundled rulesets, and auto-fix. |
| Pre-commit framework | pre-commit | alint ships a `.pre-commit-hooks.yaml` (`alint` + `alint-fix` hooks). |
| GitHub Actions | (the platform) | alint ships an [official action](../integrations/github-actions/) and emits SARIF + GitHub annotations natively. |

The boundary that holds across the table: **alint reasons about the tree as data, not the build graph as logic, not the code as semantics.**

## Adoption pattern

A progression the design points toward — start small, add layers as the value justifies the config surface. You don't have to climb all the way; steps 1–3 already produce a useful baseline.

1. **One-line start.** `extends: alint://bundled/oss-baseline@v1` in `.alint.yml`, plus a workflow that runs `alint check` on PRs.
2. **Ecosystem overlay.** Add `rust@v1` / `node@v1` / `python@v1` / `go@v1` / `java@v1` for the languages in your tree. Each defines a `facts.is_<lang>` gate, so a Java ruleset in a tree without Java contributes zero rules.
3. **CI hardening.** Add `ci/github-actions@v1` to require `permissions.contents: read` and SHA-pinned actions in every workflow.
4. **Field-level overrides.** When a bundled rule almost fits but you want it as a warning instead of an error, override just the `level:` (the rest of the rule inherits).
5. **Custom structured-query rules.** Write a few `json_path_*` / `yaml_path_*` / `toml_path_*` rules for repo-specific invariants — e.g., every `Cargo.toml` declares `edition = "2024"`.
6. **Pre-commit + GHA wiring.** Run alint locally via the [pre-commit hook](../integrations/pre-commit/) so contributors catch issues before pushing.
7. **Tighten absence rules with `git_tracked_only: true`.** Stop noisy false positives on locally-built artifacts (`target/`, `node_modules/`). See the [walker and `.gitignore`](../concepts/walker-and-gitignore/) page for the full semantics.
8. **Turn on `nested_configs: true` for monorepos.** Subtree-local conventions live in nested `.alint.yml` files; the root config focuses on tree-wide invariants.

## Honest limits

Things alint deliberately does not do, and what to use instead.

- **Build graph and dependency analysis.** Bazel, Buck2, Pants, and `cargo deny` already do this well. alint reads your `BUILD` / `Cargo.toml` / `package.json` files as data, not as graphs — it can require they exist and have certain keys, but it won't tell you if your dependency graph has a cycle.
- **Code-content linting.** ESLint, Clippy, ruff, gofmt, Prettier cover the language surface. alint checks file existence, naming, headers, and forbidden-pattern matches — not the meaning of the code.
- **Per-package iteration with conditions on the iteration itself.** Today, `for_each_dir` iterates every directory matching a `paths:` glob; the inner rules then short-circuit if a marker file is missing. A per-iteration `when:` predicate ("iterate only directories that contain a `BUILD` file") is on the v0.5 roadmap. Until then, `every_matching_has` covers the most common case (one anchor file in a directory ⇒ another file required nearby).
- **Tested scale ceiling.** alint's walker is fast — it honors `.gitignore` and reads files in parallel — but the design center is workspace-tier monorepos, not 1M-file Bazel monorepos. An incremental `--changed` mode that diffs against a base ref (so PR-time runs stay sub-second on large trees) is on the v0.5 roadmap; until then, plan for full-tree latency on each invocation. Published scale benchmarks are tracked as part of the same cut.

If a limit above describes your concern, alint isn't the right tool — reaching for the right neighbor will save you time.

## Recipes

### Cargo workspace

```yaml
# .alint.yml
version: 1
nested_configs: true
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1
  - alint://bundled/hygiene/no-tracked-artifacts@v1
  - alint://bundled/ci/github-actions@v1

rules:
  - id: cargo-edition-2024
    kind: toml_path_equals
    paths: "**/Cargo.toml"
    path: "$.package.edition"
    equals: "2024"
    level: warning

  - id: workspace-target-not-tracked
    kind: dir_absent
    paths: "**/target"
    git_tracked_only: true
    level: error
```

`git_tracked_only: true` keeps the rule silent on a developer's locally-built `target/` (which is in `.gitignore` and not tracked) and only fires if `target/` was ever committed.

### pnpm / yarn / npm workspaces

```yaml
# .alint.yml
version: 1
nested_configs: true
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/node@v1
  - alint://bundled/hygiene/no-tracked-artifacts@v1
  - alint://bundled/hygiene/lockfiles@v1
  - alint://bundled/ci/github-actions@v1

rules:
  - id: every-package-has-license
    kind: json_path_matches
    paths: "packages/*/package.json"
    path: "$.license"
    matches: "^(Apache-2.0|MIT|BSD-3-Clause)$"
    level: error
```

`hygiene/lockfiles@v1` exempts the root lockfile and flags nested ones — a common workspace-misconfiguration smell.

### Polyglot OSS monorepo

```yaml
# .alint.yml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1
  - alint://bundled/node@v1
  - alint://bundled/python@v1
  - alint://bundled/go@v1
  - alint://bundled/java@v1
  - alint://bundled/ci/github-actions@v1
  - alint://bundled/tooling/editorconfig@v1
  - alint://bundled/docs/adr@v1
```

Each ecosystem ruleset is gated by its own `facts.is_<lang>` check, so a tree without Java contributes no Java rules. Layering is cheap.

### Bazel / Buck / Pants monorepos

alint runs on tree shape, not the build graph — so it pairs with, doesn't replace, your build system. Today, expect to write more custom rules and fewer ecosystem extends:

```yaml
# .alint.yml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/ci/github-actions@v1

rules:
  - id: top-level-services-have-build-file
    kind: every_matching_has
    select: "services/*"
    require:
      - kind: file_exists
        paths: "{path}/BUILD"
    level: error

  - id: workspace-bazel-version-pinned
    kind: file_content_matches
    paths: ".bazelversion"
    pattern: '^\d+\.\d+\.\d+$'
    level: error
```

`every_matching_has` works well for known-shape glob patterns like `services/*` or `apps/*`. For arbitrary-depth Bazel packages (`BUILD` files anywhere in the tree), the per-iteration `when:` filter on `for_each_dir` planned for v0.5 lands the right idiom: "for every directory containing a `BUILD` file, require X."
