---
title: 'monorepo@v1'
description: Bundled alint ruleset at alint://bundled/monorepo@v1.
---

Hygiene checks for repositories that host multiple packages
under common subdirectories (`packages/*`, `crates/*`,
`apps/*`, `services/*`). Language-agnostic about the packages
themselves — pair with `rust@v1` / `node@v1` / etc. when you
know the ecosystem.

Conventions:
- `packages/*` is the npm / JS convention — each entry should
  have a `package.json` and a `README.md`.
- `crates/*` is the Rust workspace convention — each entry
  should have a `Cargo.toml` and a `README.md`.
- `apps/*` and `services/*` are polyglot buckets — README
  required, manifest optional (no universal convention).

## Adopt with

```yaml
extends:
  - alint://bundled/monorepo@v1
```

## Rules

### `monorepo-packages-have-readme`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`

> Every monorepo package directory should have a README.md.

### `monorepo-packages-have-package-json`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `error`

> Every `packages/*` entry should have a package.json.

### `monorepo-crates-have-cargo-toml`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `error`

> Every `crates/*` entry should have a Cargo.toml.

### `monorepo-unique-package-names`

- **kind**: [`unique_by`](/docs/rules/cross-file/unique_by/)
- **level**: `warning`

> Package-directory basenames should be unique across the monorepo.

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/monorepo.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/monorepo.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/monorepo@v1
#
# Hygiene checks for repositories that host multiple packages
# under common subdirectories (`packages/*`, `crates/*`,
# `apps/*`, `services/*`). Language-agnostic about the packages
# themselves — pair with `rust@v1` / `node@v1` / etc. when you
# know the ecosystem.
#
# Conventions:
# - `packages/*` is the npm / JS convention — each entry should
#   have a `package.json` and a `README.md`.
# - `crates/*` is the Rust workspace convention — each entry
#   should have a `Cargo.toml` and a `README.md`.
# - `apps/*` and `services/*` are polyglot buckets — README
#   required, manifest optional (no universal convention).

version: 1

rules:
  # --- README per package directory --------------------------------
  # `{a,b,c}` brace alternation in globs matches any of the listed
  # directories, so this fires for each entry under any of the
  # four common monorepo layout roots.
  - id: monorepo-packages-have-readme
    kind: for_each_dir
    select: "{packages,crates,apps,services}/*"
    level: warning
    message: "Every monorepo package directory should have a README.md."
    require:
      - kind: file_exists
        paths: "{path}/README.md"

  # --- Ecosystem-specific manifests --------------------------------
  - id: monorepo-packages-have-package-json
    kind: for_each_dir
    select: "packages/*"
    level: error
    message: "Every `packages/*` entry should have a package.json."
    require:
      - kind: file_exists
        paths: "{path}/package.json"

  - id: monorepo-crates-have-cargo-toml
    kind: for_each_dir
    select: "crates/*"
    level: error
    message: "Every `crates/*` entry should have a Cargo.toml."
    require:
      - kind: file_exists
        paths: "{path}/Cargo.toml"

  # --- Uniqueness across the workspace -----------------------------
  - id: monorepo-unique-package-names
    # A package directory name should be globally unique so tooling
    # and contributors can refer to a package unambiguously.
    kind: unique_by
    select: "{packages,crates,apps,services}/*"
    key: "{basename}"
    level: warning
    message: "Package-directory basenames should be unique across the monorepo."
```
