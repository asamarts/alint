---
title: 'file_hash'
description: 'Content SHA-256 must equal the expected digest. alint file_hash rule, content family.'
sidebar:
  order: 6
categories: ['content', 'security-unicode-sanity']
---

Content SHA-256 must equal the expected digest. Rules-as-tripwire for generated / vendored files that should never drift.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `sha256` | string | yes |  | Expected SHA-256 in lowercase hex (64 chars). Accepting uppercase and the `sha256:` prefix keeps the field forgiving. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A pinned file that drifted from its hash

The rule fires on this repository:

```text
reference.txt
```

`reference.txt`:

```text
hello world
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: locked-content
    kind: file_hash
    paths: reference.txt
    # SHA-256 of "hello world\n" is:
    # a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447
    # We assert a DIFFERENT hash to force a failure.
    sha256: "0000000000000000000000000000000000000000000000000000000000000000"
    level: error
```

### A file matching its pinned SHA-256

This repository is compliant:

```text
reference.txt
```

`reference.txt`:

```text
hello world
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: locked-content
    kind: file_hash
    paths: reference.txt
    sha256: "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447"
    level: error
```

