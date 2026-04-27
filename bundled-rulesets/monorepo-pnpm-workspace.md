---
title: 'monorepo/pnpm-workspace@v1'
description: Bundled alint ruleset at alint://bundled/monorepo/pnpm-workspace@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/monorepo/pnpm-workspace@v1
```

## Rules

### `pnpm-workspace-declares-packages`

- **kind**: [`yaml_path_matches`](/docs/rules/content/yaml_path_matches/)
- **level**: `error`
- **when**: `facts.is_pnpm_workspace`
- **policy**: <https://pnpm.io/pnpm-workspace_yaml>

> `pnpm-workspace.yaml` must declare `packages: [...]`. Without it, pnpm doesn't know which subdirs are members.

### `pnpm-workspace-member-has-readme`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_pnpm_workspace`

> pnpm workspace members should have a README.md so the package's purpose is discoverable from the directory tree.

### `pnpm-workspace-member-declares-name`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_pnpm_workspace`

> Workspace member's package.json must declare a `name` field — pnpm's filter and graph resolution use it.

