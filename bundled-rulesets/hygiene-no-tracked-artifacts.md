---
title: 'hygiene/no-tracked-artifacts@v1'
description: Bundled alint ruleset at alint://bundled/hygiene/no-tracked-artifacts@v1.
---

The set of paths/files that essentially no repository should
track: build outputs, dependency caches, editor/OS junk,
secrets-shaped files, oversized blobs. All rules ship with
reasonable defaults at unambiguous severities; use field-level
override to tweak.

Each `dir_absent` rule walks the *tracked* tree (respecting
`.gitignore`), so a properly-gitignored directory trivially
passes — these checks catch the case where someone committed
an artifact AND forgot the `.gitignore` entry.

## Adopt with

```yaml
extends:
  - alint://bundled/hygiene/no-tracked-artifacts@v1
```

## Rules

### `hygiene-no-node-modules`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`

> `node_modules/` must not be committed. Add it to .gitignore.

### `hygiene-no-python-cache`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`

> Python caches and virtualenvs must not be committed.

### `hygiene-no-ruby-bundler-cache`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `warning`

### `hygiene-no-cargo-target`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`

### `hygiene-no-js-build-outputs`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `warning`

### `hygiene-no-go-build-cache`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `info`

### `hygiene-no-macos-junk`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `error`

> macOS Finder metadata must not be committed.

### `hygiene-no-windows-junk`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `error`

> Windows shell metadata must not be committed.

### `hygiene-no-editor-backups`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`

> Editor backup or merge-conflict-orig files must not be committed.

### `hygiene-no-env-files`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `error`

> Environment files containing real values must not be committed. Use `.env.example` (or similar) for shared non-secret defaults.

### `hygiene-no-huge-files`

- **kind**: [`file_max_size`](/docs/rules/content/file_max_size/)
- **level**: `warning`

> Committed files larger than 10 MiB should be reviewed. Consider Git LFS for binaries.

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/hygiene/no-tracked-artifacts.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/hygiene/no-tracked-artifacts.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/hygiene/no-tracked-artifacts@v1
#
# The set of paths/files that essentially no repository should
# track: build outputs, dependency caches, editor/OS junk,
# secrets-shaped files, oversized blobs. All rules ship with
# reasonable defaults at unambiguous severities; use field-level
# override to tweak.
#
# Each `dir_absent` rule walks the *tracked* tree (respecting
# `.gitignore`), so a properly-gitignored directory trivially
# passes — these checks catch the case where someone committed
# an artifact AND forgot the `.gitignore` entry.

version: 1

rules:
  # --- Dependency caches ----------------------------------------------
  - id: hygiene-no-node-modules
    kind: dir_absent
    paths: "**/node_modules"
    level: error
    message: "`node_modules/` must not be committed. Add it to .gitignore."

  - id: hygiene-no-python-cache
    kind: dir_absent
    paths: ["**/__pycache__", "**/.venv", "**/venv", "**/.mypy_cache", "**/.pytest_cache", "**/.ruff_cache"]
    level: error
    message: "Python caches and virtualenvs must not be committed."

  - id: hygiene-no-ruby-bundler-cache
    kind: dir_absent
    paths: ["**/.bundle", "**/vendor/bundle"]
    level: warning

  # --- Build outputs --------------------------------------------------
  - id: hygiene-no-cargo-target
    # Rust's build output, which is large and host-specific.
    kind: dir_absent
    paths: "**/target"
    level: error

  - id: hygiene-no-js-build-outputs
    # Common JS/TS bundler output dirs. Some teams legitimately
    # commit `dist/` for published packages — disable this rule
    # on those repos.
    kind: dir_absent
    paths: ["**/dist", "**/build", "**/out", "**/.next", "**/.nuxt", "**/.svelte-kit", "**/.turbo", "**/coverage"]
    level: warning

  - id: hygiene-no-go-build-cache
    kind: dir_absent
    paths: ["**/.go-build"]
    level: info

  # --- OS / editor junk -----------------------------------------------
  - id: hygiene-no-macos-junk
    kind: file_absent
    paths: ["**/.DS_Store", "**/._*"]
    level: error
    message: "macOS Finder metadata must not be committed."
    fix:
      file_remove: {}

  - id: hygiene-no-windows-junk
    kind: file_absent
    paths: ["**/Thumbs.db", "**/desktop.ini"]
    level: error
    message: "Windows shell metadata must not be committed."
    fix:
      file_remove: {}

  - id: hygiene-no-editor-backups
    # Emacs (*~), Vim (*.swp, *.swo), JetBrains (.idea/workspace.xml
    # — but .idea/ as a whole is project-specific), generic *.bak.
    kind: file_absent
    paths: ["**/*~", "**/*.swp", "**/*.swo", "**/*.bak", "**/*.orig"]
    level: warning
    message: "Editor backup or merge-conflict-orig files must not be committed."
    fix:
      file_remove: {}

  # --- Secrets-shaped files -------------------------------------------
  - id: hygiene-no-env-files
    # Canonical .env + the *.local variants. `.env.example` /
    # `.env.template` are explicitly allowed (via the include
    # list) because they're the convention for documenting what
    # env vars a project expects.
    kind: file_absent
    paths:
      - "**/.env"
      - "**/.env.local"
      - "**/.env.*.local"
      - "**/.env.development"
      - "**/.env.production"
      - "**/.env.staging"
    level: error
    message: >-
      Environment files containing real values must not be
      committed. Use `.env.example` (or similar) for shared
      non-secret defaults.

  # --- Size gate ------------------------------------------------------
  - id: hygiene-no-huge-files
    # Conservative default. Binary fixtures / large test inputs
    # are the main legitimate exception — override or disable on
    # those repos.
    kind: file_max_size
    paths: "**"
    max_bytes: 10485760   # 10 MiB
    level: warning
    message: "Committed files larger than 10 MiB should be reviewed. Consider Git LFS for binaries."
```
