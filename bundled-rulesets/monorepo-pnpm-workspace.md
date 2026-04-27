---
title: 'monorepo/pnpm-workspace@v1'
description: Bundled alint ruleset at alint://bundled/monorepo/pnpm-workspace@v1.
---

Workspace-aware overlay for pnpm workspaces. Layered on top
of `monorepo@v1` and `node@v1`. Adopt with:

```yaml
extends:
  - alint://bundled/monorepo@v1
  - alint://bundled/node@v1
  - alint://bundled/monorepo/pnpm-workspace@v1
```

Gated by `facts.is_pnpm_workspace` — a `pnpm-workspace.yaml`
(or `.yml`) must exist at the repo root. Outside one, the
rules silently no-op.

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

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/monorepo/pnpm-workspace.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/monorepo/pnpm-workspace.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/monorepo/pnpm-workspace@v1
#
# Workspace-aware overlay for pnpm workspaces. Layered on top
# of `monorepo@v1` and `node@v1`. Adopt with:
#
#     extends:
#       - alint://bundled/monorepo@v1
#       - alint://bundled/node@v1
#       - alint://bundled/monorepo/pnpm-workspace@v1
#
# Gated by `facts.is_pnpm_workspace` — a `pnpm-workspace.yaml`
# (or `.yml`) must exist at the repo root. Outside one, the
# rules silently no-op.

version: 1

facts:
  - id: is_pnpm_workspace
    any_file_exists: ["pnpm-workspace.yaml", "pnpm-workspace.yml"]

rules:
  # pnpm-workspace.yaml is meaningless without a `packages:`
  # list — this catches workspaces that committed an empty
  # config file.
  - id: pnpm-workspace-declares-packages
    when: facts.is_pnpm_workspace
    kind: yaml_path_matches
    paths: ["pnpm-workspace.yaml", "pnpm-workspace.yml"]
    path: "$.packages[*]"
    matches: ".+"
    level: error
    message: >-
      `pnpm-workspace.yaml` must declare `packages: [...]`.
      Without it, pnpm doesn't know which subdirs are members.
    policy_url: "https://pnpm.io/pnpm-workspace_yaml"

  # Every actual workspace member (a `packages/*` directory
  # that has a `package.json` of its own) needs a README.
  - id: pnpm-workspace-member-has-readme
    when: facts.is_pnpm_workspace
    kind: for_each_dir
    select: "packages/*"
    when_iter: 'iter.has_file("package.json")'
    require:
      - kind: file_exists
        paths: "{path}/README.md"
    level: warning
    message: >-
      pnpm workspace members should have a README.md so the
      package's purpose is discoverable from the directory tree.

  # Each member's package.json should declare a name. pnpm
  # uses the name for filtering (`pnpm --filter <name>`) and
  # graph resolution.
  - id: pnpm-workspace-member-declares-name
    when: facts.is_pnpm_workspace
    kind: for_each_dir
    select: "packages/*"
    when_iter: 'iter.has_file("package.json")'
    require:
      - kind: json_path_matches
        paths: "{path}/package.json"
        path: "$.name"
        matches: ".+"
    level: warning
    message: >-
      Workspace member's package.json must declare a `name`
      field — pnpm's filter and graph resolution use it.
```
