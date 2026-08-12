---
title: 'executable_has_shebang'
description: 'Every file with +x set must begin with #!. alint executable_has_shebang rule, unix metadata family.'
sidebar:
  order: 3
categories: ['unix-metadata', 'content']
---

Every file with `+x` set must begin with `#!`. Catches plain text files accidentally marked executable.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### An executable script missing its shebang

The rule fires on this repository:

```text
README.md
scripts/
scripts/run  (executable)
```

`README.md`:

```markdown
# demo
```

`scripts/run`:

```text
echo hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: exec-shebang
    kind: executable_has_shebang
    paths: "scripts/**"
    level: error
```

### An executable script that has its shebang

This repository is compliant:

```text
README.md
scripts/
scripts/hello.sh  (executable)
```

`README.md`:

```markdown
# demo
```

`scripts/hello.sh`:

```bash
#!/bin/sh
echo hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: exec-shebang
    kind: executable_has_shebang
    paths: "scripts/**"
    level: error
```

