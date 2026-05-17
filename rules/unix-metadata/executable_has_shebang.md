---
title: 'executable_has_shebang'
description: 'Every file with +x set must begin with #!. alint executable_has_shebang rule, unix metadata family.'
sidebar:
  order: 3
---

Every file with `+x` set must begin with `#!`. Catches plain text files accidentally marked executable.

```yaml
- id: executables-have-shebangs
  kind: executable_has_shebang
  paths: "**"
  level: error
```

