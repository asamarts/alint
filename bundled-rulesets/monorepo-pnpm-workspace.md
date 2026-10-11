---
title: 'monorepo/pnpm-workspace@v1'
description: 'monorepo/pnpm-workspace@v1 bundled alint ruleset: Workspace-aware overlay for pnpm workspaces.'
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

## What it checks

3 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [pnpm-workspace-declares-packages](#pnpm-workspace-declares-packages)<br>`error` | `pnpm-workspace.yaml` must declare `packages: [...]`. |
| [pnpm-workspace-member-has-readme](#pnpm-workspace-member-has-readme)<br>`warning` | pnpm workspace members should have a README.md so the package's purpose is discoverable from the directory tree. |
| [pnpm-workspace-member-declares-name](#pnpm-workspace-member-declares-name)<br>`warning` | Workspace member's package.json must declare a `name` field. |

All 3 rules run only when `facts.is_pnpm_workspace` holds, so the ruleset stays quiet in repositories it doesn't apply to.

## Rules

### `pnpm-workspace-declares-packages`

pnpm-workspace.yaml is meaningless without a `packages:` list — this catches workspaces that committed an empty config file.

- **kind**: [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/)
- **level**: `error`
- **when**: `facts.is_pnpm_workspace`
- **policy**: <https://pnpm.io/pnpm-workspace_yaml>

> `pnpm-workspace.yaml` must declare `packages: [...]`. Without it, pnpm doesn't know which subdirs are members.

```yaml
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
```

### `pnpm-workspace-member-has-readme`

Every actual workspace member (a `packages/*` directory that has a `package.json` of its own) needs a README.

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_pnpm_workspace`

> pnpm workspace members should have a README.md so the package's purpose is discoverable from the directory tree.

```yaml
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
```

### `pnpm-workspace-member-declares-name`

Each member's package.json should declare a name. pnpm uses the name for filtering (`pnpm --filter <name>`) and graph resolution.

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_pnpm_workspace`

> Workspace member's package.json must declare a `name` field. pnpm's filter and graph resolution use it.

```yaml
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
    field. pnpm's filter and graph resolution use it.
```

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  - id: is_pnpm_workspace
    any_file_exists: ["pnpm-workspace.yaml", "pnpm-workspace.yml"]
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/monorepo/pnpm-workspace@v1
rules:
  - id: pnpm-workspace-declares-packages
    level: off
  - id: pnpm-workspace-member-has-readme
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/monorepo/pnpm-workspace@v1
    except: [pnpm-workspace-declares-packages]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/monorepo/pnpm-workspace.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/monorepo/pnpm-workspace.yml) in the alint repo.
