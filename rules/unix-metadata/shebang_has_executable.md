---
title: 'shebang_has_executable'
description: 'Every file starting with #! must have +x set. alint shebang_has_executable rule, unix metadata family.'
sidebar:
  order: 4
categories: ['unix-metadata', 'content']
---

Every file starting with `#!` must have `+x` set. Catches scripts that got their `+x` bit stripped by `git add --chmod=-x`, a tar round-trip, or a `cp` across filesystems.

---

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Scripts with a shebang but no executable bit

The rule fires on this repository:

```text
README.md
scripts/
scripts/build.py
scripts/hello.sh
scripts/plain.md
```

`README.md`:

```markdown
# demo
```

`scripts/build.py`:

```python
#!/usr/bin/env python3
print('x')
```

`scripts/hello.sh`:

```bash
#!/bin/sh
echo hi
```

`scripts/plain.md`:

```markdown
no shebang
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: shebang-needs-x
    kind: shebang_has_executable
    paths: "scripts/**"
    level: error
```

### A shebang script that carries the executable bit

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
  - id: shebang-needs-x
    kind: shebang_has_executable
    paths: "scripts/**"
    level: error
```

