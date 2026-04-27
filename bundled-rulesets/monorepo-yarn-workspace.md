---
title: 'monorepo/yarn-workspace@v1'
description: Bundled alint ruleset at alint://bundled/monorepo/yarn-workspace@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/monorepo/yarn-workspace@v1
```

## Rules

### `yarn-workspace-declares-workspaces`

- **kind**: [`json_path_matches`](/docs/rules/content/json_path_matches/)
- **level**: `error`
- **when**: `facts.is_yarn_workspace`
- **policy**: <https://yarnpkg.com/features/workspaces>

> Yarn / npm workspace's root `package.json` must declare a non-empty `workspaces` array.

### `yarn-workspace-member-has-readme`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_yarn_workspace`

> Yarn / npm workspace members should have a README.md so the package's purpose is discoverable from the directory tree.

### `yarn-workspace-member-declares-name`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_yarn_workspace`

> Workspace member's package.json must declare a `name` field — workspace tooling uses it for graph resolution.

