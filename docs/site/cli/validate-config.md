---
title: 'alint validate-config'
description: 'alint validate-config checks that an .alint.yml loads: it resolves extends:, builds every rule and parses every when: clause, without walking the tree.'
---

`alint validate-config` answers "does this config load?" in milliseconds,
without linting anything. It exits 0 when the config is valid and 1 when it
isn't, which makes it a cheap first CI step and a good pre-commit check for
the config file itself.

## Examples

The config in the current directory:

```bash
alint validate-config
```

A specific file:

```bash
alint validate-config path/to/.alint.yml
```

## See also

- [Configuration](/docs/configuration/): what goes in the config file
