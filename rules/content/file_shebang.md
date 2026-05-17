---
title: 'file_shebang'
description: 'First line of each file in scope must match the shebang regex. alint file_shebang rule, content family.'
sidebar:
  order: 12
---

First line of each file in scope must match the `shebang` regex. Pairs with `executable_has_shebang` (which checks shebang *presence* on `+x` files) — `file_shebang` checks shebang *shape*.

```yaml
- id: scripts-use-env-bash
  kind: shebang
  paths: "scripts/*.sh"
  shebang: '^#!/usr/bin/env bash$'
  level: error
```

Default `shebang:` is `^#!`, which only enforces presence; almost every useful config supplies a tighter regex pinning the interpreter.

