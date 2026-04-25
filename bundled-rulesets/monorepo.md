---
title: 'monorepo@v1'
description: Bundled alint ruleset at alint://bundled/monorepo@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/monorepo@v1
```

## Rules

### `monorepo-packages-have-readme`

- **kind**: `for_each_dir`
- **level**: `warning`

> Every monorepo package directory should have a README.md.

### `monorepo-packages-have-package-json`

- **kind**: `for_each_dir`
- **level**: `error`

> Every `packages/*` entry should have a package.json.

### `monorepo-crates-have-cargo-toml`

- **kind**: `for_each_dir`
- **level**: `error`

> Every `crates/*` entry should have a Cargo.toml.

### `monorepo-unique-package-names`

- **kind**: `unique_by`
- **level**: `warning`

> Package-directory basenames should be unique across the monorepo.

