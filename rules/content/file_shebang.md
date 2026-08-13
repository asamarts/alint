---
title: 'file_shebang'
description: 'First line of each file in scope must match the shebang regex. alint file_shebang rule, content family.'
sidebar:
  order: 12
categories: ['content', 'unix-metadata']
---

First line of each file in scope must match the `shebang` regex. Pairs with `executable_has_shebang` (which checks shebang *presence* on `+x` files) — `file_shebang` checks shebang *shape*.

Default `shebang:` is `^#!`, which only enforces presence; almost every useful config supplies a tighter regex pinning the interpreter.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `shebang` | string |  | `^#!` | Rust regex; the first line of every matched file must match. Default `^#!` only enforces shebang presence. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Scripts without the required shebang

The rule fires on this repository:

```text
scripts/
scripts/hardcoded.sh
scripts/missing.sh
scripts/ok.sh
```

`scripts/hardcoded.sh`:

```bash
#!/bin/bash
echo legacy
```

`scripts/missing.sh`:

```bash
echo no shebang
```

`scripts/ok.sh`:

```bash
#!/usr/bin/env bash
echo ok
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: scripts-use-env-bash
    kind: file_shebang
    paths: "scripts/*.sh"
    shebang: '^#!/usr/bin/env bash$'
    level: error
```

`alint check` reports:

```ansi
[2m--- scripts/hardcoded.sh -------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mscripts-use-env-bash[0m
              [2m1:1[0m  first line "#!/bin/bash" does not match required shebang
              /^#!/usr/bin/env bash$/

[2m--- scripts/missing.sh ---------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mscripts-use-env-bash[0m
              [2m1:1[0m  first line "echo no shebang" does not match required shebang
              /^#!/usr/bin/env bash$/

[2mSummary (2 violations):[0m
  [1m[31mx 2 errors[0m
  0 passing [2m*[0m 1 failing
```

### Every script uses the env-style shebang

This repository is compliant:

```text
scripts/
scripts/build.sh
scripts/ci.sh
```

`scripts/build.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
echo ok
```

`scripts/ci.sh`:

```bash
#!/usr/bin/env bash
echo ok
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: scripts-use-env-bash
    kind: file_shebang
    paths: "scripts/*.sh"
    shebang: '^#!/usr/bin/env bash$'
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

