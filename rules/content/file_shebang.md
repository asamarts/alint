---
title: 'file_shebang'
description: 'First line of each file in scope must match the shebang regex. alint file_shebang rule, content family.'
sidebar:
  order: 12
categories: ['content', 'unix-metadata']
---

First line of each file in scope must match the `shebang` regex. Pairs with `executable_has_shebang` (which checks shebang *presence* on `+x` files) — `file_shebang` checks shebang *shape*.

```yaml
- id: scripts-use-env-bash
  kind: file_shebang
  paths: "scripts/*.sh"
  shebang: '^#!/usr/bin/env bash$'
  level: error
```

Default `shebang:` is `^#!`, which only enforces presence; almost every useful config supplies a tighter regex pinning the interpreter.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `shebang` | string |  | `^#!` | Rust regex; the first line of every matched file must match. Default `^#!` only enforces shebang presence. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
