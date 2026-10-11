---
title: 'hygiene/no-tracked-artifacts@v1'
description: 'hygiene/no-tracked-artifacts@v1 bundled alint ruleset: The set of paths/files that essentially no repository should track: build outputs, dependency...'
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

## What it checks

11 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [hygiene-no-node-modules](#hygiene-no-node-modules)<br>`error` | `node_modules/` must not be committed. |
| [hygiene-no-python-cache](#hygiene-no-python-cache)<br>`error` | Python caches and virtualenvs must not be committed. |
| [hygiene-no-ruby-bundler-cache](#hygiene-no-ruby-bundler-cache)<br>`warning` | Directory counterpart of file_absent. |
| [hygiene-no-cargo-target](#hygiene-no-cargo-target)<br>`error` | Directory counterpart of file_absent. |
| [hygiene-no-js-build-outputs](#hygiene-no-js-build-outputs)<br>`warning` | Directory counterpart of file_absent. |
| [hygiene-no-go-build-cache](#hygiene-no-go-build-cache)<br>`info` | Directory counterpart of file_absent. |
| [hygiene-no-macos-junk](#hygiene-no-macos-junk)<br>`error` | macOS Finder metadata must not be committed. |
| [hygiene-no-windows-junk](#hygiene-no-windows-junk)<br>`error` | Windows shell metadata must not be committed. |
| [hygiene-no-editor-backups](#hygiene-no-editor-backups)<br>`warning` | Editor backup or merge-conflict-orig files must not be committed. |
| [hygiene-no-env-files](#hygiene-no-env-files)<br>`error` | Environment files containing real values must not be committed. |
| [hygiene-no-huge-files](#hygiene-no-huge-files)<br>`warning` | Committed files larger than 10 MiB should be reviewed. |

## Rules

### `hygiene-no-node-modules`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`

> `node_modules/` must not be committed. Add it to .gitignore.

```yaml
- id: hygiene-no-node-modules
  kind: dir_absent
  paths: "**/node_modules"
  level: error
  message: "`node_modules/` must not be committed. Add it to .gitignore."
```

### `hygiene-no-python-cache`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`

> Python caches and virtualenvs must not be committed.

```yaml
- id: hygiene-no-python-cache
  kind: dir_absent
  paths: ["**/__pycache__", "**/.venv", "**/venv", "**/.mypy_cache", "**/.pytest_cache", "**/.ruff_cache"]
  level: error
  message: "Python caches and virtualenvs must not be committed."
```

### `hygiene-no-ruby-bundler-cache`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `warning`

```yaml
- id: hygiene-no-ruby-bundler-cache
  kind: dir_absent
  paths: ["**/.bundle", "**/vendor/bundle"]
  level: warning
```

### `hygiene-no-cargo-target`

Rust's build output, which is large and host-specific.

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`

```yaml
- id: hygiene-no-cargo-target
  kind: dir_absent
  paths: "**/target"
  level: error
```

### `hygiene-no-js-build-outputs`

Common JS/TS bundler output dirs. Some teams legitimately commit `dist/` for published packages — disable this rule on those repos.

Changed in v0.9.18: gated on `has_ancestor: package.json` so the rule only fires inside JS-package contexts. Without this gate, the rule produced false positives in 8 polyglot monorepos (k8s `build/`, golang/go `src/cmd/dist`, dotnet `bin/`, bazel `tools/build_defs/`, vscode `extensions/*/build/`, nixpkgs `pkgs/development/python-modules/build`, deno `tests/testdata/dist/`, angular `dev-infra/.../build/`) — directories literally named `build`/`dist`/etc. in non-JS contexts.

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `warning`

```yaml
- id: hygiene-no-js-build-outputs
  kind: dir_absent
  paths: ["**/dist", "**/build", "**/out", "**/.next", "**/.nuxt", "**/.svelte-kit", "**/.turbo", "**/coverage"]
  scope_filter:
    has_ancestor: package.json
  level: warning
```

### `hygiene-no-go-build-cache`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `info`

```yaml
- id: hygiene-no-go-build-cache
  kind: dir_absent
  paths: ["**/.go-build"]
  level: info
```

### `hygiene-no-macos-junk`

Verify the file actually IS macOS junk before flagging it: the `._*` glob otherwise collides with Hadoop's `._<name>.crc` checksum files (which begin "crc\\0"). Require the AppleDouble magic (00 05 16 07) or the .DS\_Store "Bud1" magic (00 00 00 01 42 75 64 31) at byte 0.

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `error`
- **fix**: `file_remove` (applied by `alint fix`)

> macOS Finder metadata must not be committed.

```yaml
- id: hygiene-no-macos-junk
  kind: file_absent
  paths: ["**/.DS_Store", "**/._*"]
  content_prefix_hex: ["00051607", "0000000142756431"]
  level: error
  message: "macOS Finder metadata must not be committed."
  fix:
    file_remove: {}
```

### `hygiene-no-windows-junk`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `error`
- **fix**: `file_remove` (applied by `alint fix`)

> Windows shell metadata must not be committed.

```yaml
- id: hygiene-no-windows-junk
  kind: file_absent
  paths: ["**/Thumbs.db", "**/desktop.ini"]
  level: error
  message: "Windows shell metadata must not be committed."
  fix:
    file_remove: {}
```

### `hygiene-no-editor-backups`

Emacs (\*\~), Vim (\*.swp, \*.swo), JetBrains (.idea/workspace.xml — but .idea/ as a whole is project-specific), generic \*.bak.

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`
- **fix**: `file_remove` (applied by `alint fix`)

> Editor backup or merge-conflict-orig files must not be committed.

```yaml
- id: hygiene-no-editor-backups
  kind: file_absent
  paths: ["**/*~", "**/*.swp", "**/*.swo", "**/*.bak", "**/*.orig"]
  level: warning
  message: "Editor backup or merge-conflict-orig files must not be committed."
  fix:
    file_remove: {}
```

### `hygiene-no-env-files`

Canonical .env + the \*.local variants. `.env.example` / `.env.template` are explicitly allowed (via the include list) because they're the convention for documenting what env vars a project expects.

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `error`

> Environment files containing real values must not be committed. Use `.env.example` (or similar) for shared non-secret defaults.

```yaml
- id: hygiene-no-env-files
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
```

### `hygiene-no-huge-files`

Conservative default. Binary fixtures / large test inputs are the main legitimate exception — override or disable on those repos.

- **kind**: [`file_max_size`](/docs/rules/content/file_max_size/)
- **level**: `warning`

> Committed files larger than 10 MiB should be reviewed. Consider Git LFS for binaries.

```yaml
- id: hygiene-no-huge-files
  kind: file_max_size
  paths: "**"
  max_bytes: 10485760   # 10 MiB
  level: warning
  message: "Committed files larger than 10 MiB should be reviewed. Consider Git LFS for binaries."
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/hygiene/no-tracked-artifacts@v1
rules:
  - id: hygiene-no-node-modules
    level: off
  - id: hygiene-no-ruby-bundler-cache
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/hygiene/no-tracked-artifacts@v1
    except: [hygiene-no-node-modules]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/hygiene/no-tracked-artifacts.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/hygiene/no-tracked-artifacts.yml) in the alint repo.
