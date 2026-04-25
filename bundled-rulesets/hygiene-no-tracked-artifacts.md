---
title: 'hygiene/no-tracked-artifacts@v1'
description: Bundled alint ruleset at alint://bundled/hygiene/no-tracked-artifacts@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/hygiene/no-tracked-artifacts@v1
```

## Rules

### `hygiene-no-node-modules`

- **kind**: `dir_absent`
- **level**: `error`

> `node_modules/` must not be committed. Add it to .gitignore.

### `hygiene-no-python-cache`

- **kind**: `dir_absent`
- **level**: `error`

> Python caches and virtualenvs must not be committed.

### `hygiene-no-ruby-bundler-cache`

- **kind**: `dir_absent`
- **level**: `warning`

### `hygiene-no-cargo-target`

- **kind**: `dir_absent`
- **level**: `error`

### `hygiene-no-js-build-outputs`

- **kind**: `dir_absent`
- **level**: `warning`

### `hygiene-no-go-build-cache`

- **kind**: `dir_absent`
- **level**: `info`

### `hygiene-no-macos-junk`

- **kind**: `file_absent`
- **level**: `error`

> macOS Finder metadata must not be committed.

### `hygiene-no-windows-junk`

- **kind**: `file_absent`
- **level**: `error`

> Windows shell metadata must not be committed.

### `hygiene-no-editor-backups`

- **kind**: `file_absent`
- **level**: `warning`

> Editor backup or merge-conflict-orig files must not be committed.

### `hygiene-no-env-files`

- **kind**: `file_absent`
- **level**: `error`

> Environment files containing real values must not be committed. Use `.env.example` (or similar) for shared non-secret defaults.

### `hygiene-no-huge-files`

- **kind**: `file_max_size`
- **level**: `warning`

> Committed files larger than 10 MiB should be reviewed. Consider Git LFS for binaries.

