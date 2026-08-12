---
title: 'executable_bit'
description: 'Assert every file in scope either has the +x bit set (require: true) or does not (require: false). alint executable_bit rule, unix metadata family.'
sidebar:
  order: 2
categories: ['unix-metadata']
---

Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`).

No fix op — chmod auto-apply is deferred.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `require` | boolean | yes |  | `true` → +x must be set; `false` → +x must NOT be set. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A docs file that is unexpectedly executable

The rule fires on this repository:

```text
README.md
docs/
docs/generate.sh  (executable)
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: docs-not-exec
    kind: executable_bit
    paths: "docs/**"
    level: error
    require: false
```

### Docs files that are not executable

This repository is compliant:

```text
README.md
docs/
docs/intro.md
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: docs-not-exec
    kind: executable_bit
    paths: "docs/**"
    level: error
    require: false
```

