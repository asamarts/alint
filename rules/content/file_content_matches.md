---
title: 'file_content_matches'
description: 'File contents must contain at least one match for a regex. alint file_content_matches rule, content family.'
sidebar:
  order: 1
categories: ['content']
---

File contents must contain at least one match for a regex.

Fix: `file_append` — append declared content.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `pattern` | string | yes |  | Rust regex. File contents must match. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A README missing its SPDX license tag

The rule fires on this repository:

```text
README.md
```

`README.md`:

```markdown
# Project

A short description.
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: spdx-in-readme
    kind: file_content_matches
    paths: "README.md"
    pattern: "SPDX-License-Identifier"
    level: warning
```

`alint check` reports:

```ansi
[2m--- README.md ------------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mspdx-in-readme[0m
              content does not match required pattern /SPDX-License-Identifier/

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### A README that declares its SPDX license

This repository is compliant:

```text
README.md
```

`README.md`:

```markdown
# Project

SPDX-License-Identifier: Apache-2.0

A short description.
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: spdx-in-readme
    kind: file_content_matches
    paths: "README.md"
    pattern: "SPDX-License-Identifier"
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

