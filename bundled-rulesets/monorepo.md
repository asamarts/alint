---
title: 'monorepo@v1'
description: 'monorepo@v1 bundled alint ruleset: Hygiene checks for repositories that host multiple packages under common subdirectories...'
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

## What it checks

4 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [monorepo-packages-have-readme](#monorepo-packages-have-readme)<br>`warning` | Every monorepo package directory should have a README.md. |
| [monorepo-packages-have-package-json](#monorepo-packages-have-package-json)<br>`error` | Every `packages/*` entry should have a package.json. |
| [monorepo-crates-have-cargo-toml](#monorepo-crates-have-cargo-toml)<br>`error` | Every `crates/*` entry should have a Cargo.toml. |
| [monorepo-unique-package-names](#monorepo-unique-package-names)<br>`warning` | Package-directory basenames should be unique across the monorepo. |

## Rules

### `monorepo-packages-have-readme`

`{a,b,c}` brace alternation in globs matches any of the listed directories, so this fires for each entry under any of the four common monorepo layout roots.

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`

> Every monorepo package directory should have a README.md.

```yaml
- id: monorepo-packages-have-readme
  kind: for_each_dir
  select: "{packages,crates,apps,services}/*"
  level: warning
  message: "Every monorepo package directory should have a README.md."
  require:
    - kind: file_exists
      paths: "{path}/README.md"
```

### `monorepo-packages-have-package-json`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `error`

> Every `packages/*` entry should have a package.json.

```yaml
- id: monorepo-packages-have-package-json
  kind: for_each_dir
  select: "packages/*"
  level: error
  message: "Every `packages/*` entry should have a package.json."
  require:
    - kind: file_exists
      paths: "{path}/package.json"
```

### `monorepo-crates-have-cargo-toml`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `error`

> Every `crates/*` entry should have a Cargo.toml.

```yaml
- id: monorepo-crates-have-cargo-toml
  kind: for_each_dir
  select: "crates/*"
  level: error
  message: "Every `crates/*` entry should have a Cargo.toml."
  require:
    - kind: file_exists
      paths: "{path}/Cargo.toml"
```

### `monorepo-unique-package-names`

A package directory name should be globally unique so tooling and contributors can refer to a package unambiguously.

- **kind**: [`unique_by`](/docs/rules/cross-file/unique_by/)
- **level**: `warning`

> Package-directory basenames should be unique across the monorepo.

```yaml
- id: monorepo-unique-package-names
  kind: unique_by
  select: "{packages,crates,apps,services}/*"
  key: "{basename}"
  level: warning
  message: "Package-directory basenames should be unique across the monorepo."
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/monorepo@v1
rules:
  - id: monorepo-packages-have-readme
    level: off
  - id: monorepo-unique-package-names
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/monorepo@v1
    except: [monorepo-packages-have-readme]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/monorepo.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/monorepo.yml) in the alint repo.
