---
title: 'executable_has_shebang'
description: 'Every file with +x set must begin with #!. alint executable_has_shebang rule, unix metadata family.'
sidebar:
  order: 3
categories: ['unix-metadata', 'content']
---

Every file with `+x` set must begin with `#!`. Catches plain text files accidentally marked executable.

```yaml
- id: executables-have-shebangs
  kind: executable_has_shebang
  paths: "**"
  level: error
```

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
