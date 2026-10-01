---
title: 'alint init'
description: 'alint init writes a starter .alint.yml for the ecosystem it detects, extending the matching bundled rulesets. Add --monorepo for workspace overlays.'
---

`alint init` detects the repository's ecosystem and writes a starter
`.alint.yml` that extends the matching bundled rulesets. It never overwrites
an existing config. Pair it with [`alint suggest`](/docs/cli/suggest/) to
find rules the starter doesn't cover.

## Examples

Write a starter config for the detected ecosystem:

```bash
alint init
```

In a workspace: add the monorepo overlays and enable nested configs:

```bash
alint init --monorepo
```

## Reference

```
Scaffold a starter `.alint.yml` for the detected ecosystem.

Detects the ecosystem (and optionally workspace shape) from the repo. Refuses to overwrite an
existing config - delete the existing one first if you really mean it.

Usage: alint init [OPTIONS] [PATH]

Arguments:
  [PATH]
          Root of the repository to write the config into. Defaults to the current directory

          [default: .]

Options:
      --monorepo
          Detect workspace shape (Cargo `[workspace]`, pnpm-workspace.yaml, or `package.json`
          `workspaces`) and add the corresponding `monorepo@v1` + `monorepo/<flavor>-workspace@v1`
          overlays. `nested_configs: true` is set on the generated config so each subdirectory can
          layer its own `.alint.yml` on top
```

The [global options](/docs/cli/#global-options) apply to `alint init` too, where they are relevant.

## See also

- [Quickstart](/docs/getting-started/quickstart/)
- [Bundled rulesets](/docs/bundled-rulesets/)
