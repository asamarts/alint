---
title: 'monorepo/yarn-workspace@v1'
description: 'monorepo/yarn-workspace@v1 bundled alint ruleset: Workspace-aware overlay for Yarn / npm workspaces...'
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

## What it checks

3 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [yarn-workspace-declares-workspaces](#yarn-workspace-declares-workspaces)<br>`error` | Yarn / npm workspace's root `package.json` must declare a non-empty `workspaces` array. |
| [yarn-workspace-member-has-readme](#yarn-workspace-member-has-readme)<br>`warning` | Yarn / npm workspace members should have a README.md so the package's purpose is discoverable from the directory tree. |
| [yarn-workspace-member-declares-name](#yarn-workspace-member-declares-name)<br>`warning` | Workspace member's package.json must declare a `name` field. |

All 3 rules run only when `facts.is_yarn_workspace` holds, so the ruleset stays quiet in repositories it doesn't apply to.

## Rules

### `yarn-workspace-declares-workspaces`

The workspaces field must be present and non-empty — a bare `"workspaces": []` doesn't actually declare anything. `$.workspaces[*]` returns each entry of the array form (`["packages/*"]`), and matches `.+` checks each is a non-empty string. The object form (`{"packages": [...]}`) is rarer and not validated here; the fact gate ensures the field at least exists.

- **kind**: [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- **level**: `error`
- **when**: `facts.is_yarn_workspace`
- **policy**: <https://yarnpkg.com/features/workspaces>

> Yarn / npm workspace's root `package.json` must declare a non-empty `workspaces` array.

```yaml
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
```

### `yarn-workspace-member-has-readme`

Every actual workspace member (a `packages/*` or `apps/*` directory with a `package.json`) needs a README.

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_yarn_workspace`

> Yarn / npm workspace members should have a README.md so the package's purpose is discoverable from the directory tree.

```yaml
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
```

### `yarn-workspace-member-declares-name`

Each member's package.json should declare a name. Yarn / npm use it for filtering and graph resolution.

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_yarn_workspace`

> Workspace member's package.json must declare a `name` field. Workspace tooling uses it for graph resolution.

```yaml
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
    field. Workspace tooling uses it for graph resolution.
```

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  - id: is_yarn_workspace
    file_content_matches:
      paths: package.json
      pattern: '"workspaces"\s*:'
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/monorepo/yarn-workspace@v1
rules:
  - id: yarn-workspace-declares-workspaces
    level: off
  - id: yarn-workspace-member-has-readme
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/monorepo/yarn-workspace@v1
    except: [yarn-workspace-declares-workspaces]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/monorepo/yarn-workspace.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/monorepo/yarn-workspace.yml) in the alint repo.
