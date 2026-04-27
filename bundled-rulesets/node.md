---
title: 'node@v1'
description: Bundled alint ruleset at alint://bundled/node@v1.
---

Hygiene checks for Node.js / npm / pnpm / yarn projects. Adopt
it with:

```yaml
extends:
  - alint://bundled/node@v1
```

Every rule is gated with `when: facts.is_node`, so it's safe to
extend from a polyglot repo — rules don't fire unless
`package.json` is present. Override `is_node` with your own
`facts:` block if you need a different heuristic (e.g. detect
`deno.json` or `bun.lock`).

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

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/node.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/node.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/node@v1
#
# Hygiene checks for Node.js / npm / pnpm / yarn projects. Adopt
# it with:
#
#     extends:
#       - alint://bundled/node@v1
#
# Every rule is gated with `when: facts.is_node`, so it's safe to
# extend from a polyglot repo — rules don't fire unless
# `package.json` is present. Override `is_node` with your own
# `facts:` block if you need a different heuristic (e.g. detect
# `deno.json` or `bun.lock`).

version: 1

facts:
  - id: is_node
    any_file_exists: [package.json]

rules:
  # --- Manifest + lockfiles -----------------------------------------
  - id: node-package-json-exists
    when: facts.is_node
    kind: file_exists
    paths: package.json
    root_only: true
    level: error
    message: "Node project: package.json at the root is required."

  - id: node-has-lockfile
    when: facts.is_node
    # Accept any of the four common lockfiles — npm, pnpm, yarn,
    # bun. At least one should be committed for reproducible installs.
    kind: file_exists
    paths:
      - "package-lock.json"
      - "pnpm-lock.yaml"
      - "yarn.lock"
      - "bun.lock"
      - "bun.lockb"
    root_only: true
    level: warning
    message: "A lockfile should be committed (package-lock.json / pnpm-lock.yaml / yarn.lock / bun.lock)."
    policy_url: "https://docs.npmjs.com/cli/v10/configuring-npm/package-lock-json"

  # --- Build artefacts must not be tracked --------------------------
  - id: node-no-tracked-node-modules
    when: facts.is_node
    kind: dir_absent
    paths: "**/node_modules"
    level: error
    message: "`node_modules/` must not be committed; add it to .gitignore."

  - id: node-no-tracked-dist
    when: facts.is_node
    # Common build-output directory names. Users with legitimate
    # reasons to ship a built `dist/` (e.g. a typed-package
    # preview) can set this rule's `level: off`.
    kind: dir_absent
    paths: ["**/dist", "**/.next", "**/.nuxt", "**/coverage", "**/.turbo"]
    level: info
    message: >-
      Build output directories are usually generated and shouldn't
      be tracked. Override with `level: off` if this one is
      intentionally shipped.

  # --- Node version pinning ----------------------------------------
  - id: node-engine-or-nvmrc
    when: facts.is_node
    kind: file_exists
    paths: [".nvmrc", ".node-version", ".tool-versions"]
    root_only: true
    level: info
    message: >-
      Pin the Node.js version so local and CI installs match
      (`.nvmrc`, `.node-version`, or `.tool-versions`). An
      `engines.node` field in package.json is an alternative but
      is not detected by this rule.

  # --- Source-file hygiene on JS / TS sources -----------------------
  - id: node-sources-final-newline
    when: facts.is_node
    kind: final_newline
    paths: ["src/**/*.{js,jsx,ts,tsx,mjs,cjs}", "lib/**/*.{js,jsx,ts,tsx,mjs,cjs}"]
    level: info
    fix:
      file_append_final_newline: {}

  - id: node-sources-no-trailing-whitespace
    when: facts.is_node
    kind: no_trailing_whitespace
    paths: ["src/**/*.{js,jsx,ts,tsx,mjs,cjs}", "lib/**/*.{js,jsx,ts,tsx,mjs,cjs}"]
    level: info
    fix:
      file_trim_trailing_whitespace: {}

  - id: node-sources-no-bidi
    when: facts.is_node
    kind: no_bidi_controls
    paths: ["src/**/*.{js,jsx,ts,tsx,mjs,cjs}", "lib/**/*.{js,jsx,ts,tsx,mjs,cjs}"]
    level: error
    message: "Trojan Source (CVE-2021-42574): bidi override chars are rejected in JS/TS sources."
    policy_url: "https://trojansource.codes/"
```
