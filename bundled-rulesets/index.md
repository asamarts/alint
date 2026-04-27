---
title: Bundled Rulesets
description: One-line ecosystem baselines built into the alint binary.
sidebar:
  order: 1
---

Adopt with `extends: [alint://bundled/<name>@v1]`. Each ruleset's full rule list lives on its own page below.

## Currently shipped

- [`ci/github-actions@v1`](/docs/bundled-rulesets/ci-github-actions/) — GitHub Actions hardening.
- [`compliance/apache-2@v1`](/docs/bundled-rulesets/compliance-apache-2/) — Hygiene checks for repositories distributed under the Apache License, Version 2.0.
- [`compliance/reuse@v1`](/docs/bundled-rulesets/compliance-reuse/) — Hygiene checks for repositories that follow the FSFE REUSE Specification (https://reuse.software/) — every licensable file declares its license + copyright via an SPDX header (or a `.license` companion / REUSE.toml entry), and the full license texts live under `LICENSES/`.
- [`docs/adr@v1`](/docs/bundled-rulesets/docs-adr/) — Architecture Decision Records following the MADR ("Markdown Architectural Decision Records") convention: files named NNNN-title.md under docs/adr/, each with at least Status, Context, and Decision sections.
- [`go@v1`](/docs/bundled-rulesets/go/) — Hygiene checks for Go modules.
- [`hygiene/lockfiles@v1`](/docs/bundled-rulesets/hygiene-lockfiles/) — Lockfile discipline: exactly one package-manager's lockfile per workspace, and lockfiles only at the workspace root.
- [`hygiene/no-tracked-artifacts@v1`](/docs/bundled-rulesets/hygiene-no-tracked-artifacts/) — The set of paths/files that essentially no repository should track: build outputs, dependency caches, editor/OS junk, secrets-shaped files, oversized blobs.
- [`java@v1`](/docs/bundled-rulesets/java/) — Hygiene checks for Java projects (Maven + Gradle).
- [`monorepo@v1`](/docs/bundled-rulesets/monorepo/) — Hygiene checks for repositories that host multiple packages under common subdirectories (`packages/*`, `crates/*`, `apps/*`, `services/*`).
- [`monorepo/cargo-workspace@v1`](/docs/bundled-rulesets/monorepo-cargo-workspace/) — Workspace-aware overlay for Cargo workspaces.
- [`monorepo/pnpm-workspace@v1`](/docs/bundled-rulesets/monorepo-pnpm-workspace/) — Workspace-aware overlay for pnpm workspaces.
- [`monorepo/yarn-workspace@v1`](/docs/bundled-rulesets/monorepo-yarn-workspace/) — Workspace-aware overlay for Yarn / npm workspaces (both encode the workspace declaration in the root `package.json` under `"workspaces"`).
- [`node@v1`](/docs/bundled-rulesets/node/) — Hygiene checks for Node.js / npm / pnpm / yarn projects.
- [`oss-baseline@v1`](/docs/bundled-rulesets/oss-baseline/) — A minimal OSS-hygiene baseline — the documents and conventions most open-source repositories are expected to follow.
- [`python@v1`](/docs/bundled-rulesets/python/) — Hygiene checks for Python projects (pyproject / setuptools / Poetry / PDM / uv).
- [`rust@v1`](/docs/bundled-rulesets/rust/) — Hygiene checks for Rust projects.
- [`tooling/editorconfig@v1`](/docs/bundled-rulesets/tooling-editorconfig/) — Cross-editor standardization: an `.editorconfig` at the root plus a `.gitattributes` that normalizes line endings.
