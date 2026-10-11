---
title: 'hygiene/lockfiles@v1'
description: 'hygiene/lockfiles@v1 bundled alint ruleset: Lockfile discipline: exactly one package-manager''s lockfile per workspace, and lockfiles only...'
---

Lockfile discipline: exactly one package-manager's lockfile
per workspace, and lockfiles only at the workspace root.
Nested lockfiles almost always indicate a tooling
misconfiguration (e.g., a transitive install ran npm when
the parent is pnpm) and cause version drift.

This ruleset only addresses the "where" question. The
one-version rule ("every package.json has the same
dependency version") needs structured-query primitives and
ships in a later release.

## Adopt with

```yaml
extends:
  - alint://bundled/hygiene/lockfiles@v1
```

## What it checks

7 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [lockfiles-no-nested-yarn](#lockfiles-no-nested-yarn)<br>`warning` | Nested `yarn.lock` outside the workspace root. |
| [lockfiles-no-nested-pnpm](#lockfiles-no-nested-pnpm)<br>`warning` | No file matching paths may exist in the walked tree. |
| [lockfiles-no-nested-npm](#lockfiles-no-nested-npm)<br>`warning` | No file matching paths may exist in the walked tree. |
| [lockfiles-no-nested-bun](#lockfiles-no-nested-bun)<br>`warning` | No file matching paths may exist in the walked tree. |
| [lockfiles-no-nested-cargo](#lockfiles-no-nested-cargo)<br>`warning` | Nested `Cargo.lock`. |
| [lockfiles-no-nested-poetry](#lockfiles-no-nested-poetry)<br>`warning` | No file matching paths may exist in the walked tree. |
| [lockfiles-no-nested-uv](#lockfiles-no-nested-uv)<br>`warning` | No file matching paths may exist in the walked tree. |

## Rules

### `lockfiles-no-nested-yarn`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

> Nested `yarn.lock` outside the workspace root. This is usually a tooling mishap. If intentional, disable the rule.

```yaml
- id: lockfiles-no-nested-yarn
  kind: file_absent
  paths:
    include: "**/yarn.lock"
    exclude: "yarn.lock"
  level: warning
  message: >-
    Nested `yarn.lock` outside the workspace root. This is
    usually a tooling mishap. If intentional, disable the rule.
```

### `lockfiles-no-nested-pnpm`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

```yaml
- id: lockfiles-no-nested-pnpm
  kind: file_absent
  paths:
    include: "**/pnpm-lock.yaml"
    exclude: "pnpm-lock.yaml"
  level: warning
```

### `lockfiles-no-nested-npm`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

```yaml
- id: lockfiles-no-nested-npm
  kind: file_absent
  paths:
    include: "**/package-lock.json"
    exclude: "package-lock.json"
  level: warning
```

### `lockfiles-no-nested-bun`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

```yaml
- id: lockfiles-no-nested-bun
  kind: file_absent
  paths:
    include: ["**/bun.lock", "**/bun.lockb"]
    exclude: ["bun.lock", "bun.lockb"]
  level: warning
```

### `lockfiles-no-nested-cargo`

Library crates in a workspace don't ship their own Cargo.lock — Cargo itself ignores per-crate lockfiles inside a workspace. A nested Cargo.lock is almost always an accidentally-committed dev artefact.

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

> Nested `Cargo.lock`. Only the workspace-root Cargo.lock is honored by Cargo; nested ones drift and confuse contributors.

```yaml
- id: lockfiles-no-nested-cargo
  kind: file_absent
  paths:
    include: "**/Cargo.lock"
    exclude: "Cargo.lock"
  level: warning
  message: >-
    Nested `Cargo.lock`. Only the workspace-root Cargo.lock
    is honored by Cargo; nested ones drift and confuse
    contributors.
```

### `lockfiles-no-nested-poetry`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

```yaml
- id: lockfiles-no-nested-poetry
  kind: file_absent
  paths:
    include: "**/poetry.lock"
    exclude: "poetry.lock"
  level: warning
```

### `lockfiles-no-nested-uv`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

```yaml
- id: lockfiles-no-nested-uv
  kind: file_absent
  paths:
    include: "**/uv.lock"
    exclude: "uv.lock"
  level: warning
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/hygiene/lockfiles@v1
rules:
  - id: lockfiles-no-nested-yarn
    level: off
  - id: lockfiles-no-nested-pnpm
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/hygiene/lockfiles@v1
    except: [lockfiles-no-nested-yarn]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/hygiene/lockfiles.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/hygiene/lockfiles.yml) in the alint repo.
