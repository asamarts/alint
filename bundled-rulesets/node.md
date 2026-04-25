---
title: 'node@v1'
description: Bundled alint ruleset at alint://bundled/node@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/node@v1
```

## Rules

### `node-package-json-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **when**: `facts.is_node`

> Node project: package.json at the root is required.

### `node-has-lockfile`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **when**: `facts.is_node`
- **policy**: <https://docs.npmjs.com/cli/v10/configuring-npm/package-lock-json>

> A lockfile should be committed (package-lock.json / pnpm-lock.yaml / yarn.lock / bun.lock).

### `node-no-tracked-node-modules`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`
- **when**: `facts.is_node`

> `node_modules/` must not be committed; add it to .gitignore.

### `node-no-tracked-dist`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `info`
- **when**: `facts.is_node`

> Build output directories are usually generated and shouldn't be tracked. Override with `level: off` if this one is intentionally shipped.

### `node-engine-or-nvmrc`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **when**: `facts.is_node`

> Pin the Node.js version so local and CI installs match (`.nvmrc`, `.node-version`, or `.tool-versions`). An `engines.node` field in package.json is an alternative but is not detected by this rule.

### `node-sources-final-newline`

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `info`
- **when**: `facts.is_node`

### `node-sources-no-trailing-whitespace`

- **kind**: [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/)
- **level**: `info`
- **when**: `facts.is_node`

### `node-sources-no-bidi`

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **when**: `facts.is_node`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars are rejected in JS/TS sources.

