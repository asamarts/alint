---
title: 'monorepo/yarn-workspace@v1'
description: Bundled alint ruleset at alint://bundled/monorepo/yarn-workspace@v1.
---

Workspace-aware overlay for Yarn / npm workspaces (both
encode the workspace declaration in the root `package.json`
under `"workspaces"`). Layered on top of `monorepo@v1` and
`node@v1`. Adopt with:

```yaml
extends:
  - alint://bundled/monorepo@v1
  - alint://bundled/node@v1
  - alint://bundled/monorepo/yarn-workspace@v1
```

Gated by `facts.is_yarn_workspace` — root `package.json`
must contain a `"workspaces"` field. Covers both the array
form (`"workspaces": ["packages/*"]`) and the object form
(`"workspaces": { "packages": [...] }`). Outside a workspace,
the rules silently no-op.

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

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/monorepo/yarn-workspace.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/monorepo/yarn-workspace.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/monorepo/yarn-workspace@v1
#
# Workspace-aware overlay for Yarn / npm workspaces (both
# encode the workspace declaration in the root `package.json`
# under `"workspaces"`). Layered on top of `monorepo@v1` and
# `node@v1`. Adopt with:
#
#     extends:
#       - alint://bundled/monorepo@v1
#       - alint://bundled/node@v1
#       - alint://bundled/monorepo/yarn-workspace@v1
#
# Gated by `facts.is_yarn_workspace` — root `package.json`
# must contain a `"workspaces"` field. Covers both the array
# form (`"workspaces": ["packages/*"]`) and the object form
# (`"workspaces": { "packages": [...] }`). Outside a workspace,
# the rules silently no-op.

version: 1

facts:
  - id: is_yarn_workspace
    file_content_matches:
      paths: package.json
      pattern: '"workspaces"\s*:'

rules:
  # The workspaces field must be present and non-empty —
  # a bare `"workspaces": []` doesn't actually declare
  # anything. `$.workspaces[*]` returns each entry of the
  # array form (`["packages/*"]`), and matches `.+` checks
  # each is a non-empty string. The object form
  # (`{"packages": [...]}`) is rarer and not validated here;
  # the fact gate ensures the field at least exists.
  - id: yarn-workspace-declares-workspaces
    when: facts.is_yarn_workspace
    kind: json_path_matches
    paths: package.json
    path: "$.workspaces[*]"
    matches: ".+"
    level: error
    message: >-
      Yarn / npm workspace's root `package.json` must declare
      a non-empty `workspaces` array.
    policy_url: "https://yarnpkg.com/features/workspaces"

  # Every actual workspace member (a `packages/*` or `apps/*`
  # directory with a `package.json`) needs a README.
  - id: yarn-workspace-member-has-readme
    when: facts.is_yarn_workspace
    kind: for_each_dir
    select: "{packages,apps}/*"
    when_iter: 'iter.has_file("package.json")'
    require:
      - kind: file_exists
        paths: "{path}/README.md"
    level: warning
    message: >-
      Yarn / npm workspace members should have a README.md so
      the package's purpose is discoverable from the directory
      tree.

  # Each member's package.json should declare a name. Yarn /
  # npm use it for filtering and graph resolution.
  - id: yarn-workspace-member-declares-name
    when: facts.is_yarn_workspace
    kind: for_each_dir
    select: "{packages,apps}/*"
    when_iter: 'iter.has_file("package.json")'
    require:
      - kind: json_path_matches
        paths: "{path}/package.json"
        path: "$.name"
        matches: ".+"
    level: warning
    message: >-
      Workspace member's package.json must declare a `name`
      field — workspace tooling uses it for graph resolution.
```
