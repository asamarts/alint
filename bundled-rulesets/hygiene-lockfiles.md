---
title: 'hygiene/lockfiles@v1'
description: Bundled alint ruleset at alint://bundled/hygiene/lockfiles@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/hygiene/lockfiles@v1
```

## Rules

### `lockfiles-no-nested-yarn`

- **kind**: `file_absent`
- **level**: `warning`

> Nested `yarn.lock` outside the workspace root — usually a tooling mishap. If this is intentional, disable the rule.

### `lockfiles-no-nested-pnpm`

- **kind**: `file_absent`
- **level**: `warning`

### `lockfiles-no-nested-npm`

- **kind**: `file_absent`
- **level**: `warning`

### `lockfiles-no-nested-bun`

- **kind**: `file_absent`
- **level**: `warning`

### `lockfiles-no-nested-cargo`

- **kind**: `file_absent`
- **level**: `warning`

> Nested `Cargo.lock` — only the workspace-root Cargo.lock is honored by Cargo; nested ones drift and confuse contributors.

### `lockfiles-no-nested-poetry`

- **kind**: `file_absent`
- **level**: `warning`

### `lockfiles-no-nested-uv`

- **kind**: `file_absent`
- **level**: `warning`

