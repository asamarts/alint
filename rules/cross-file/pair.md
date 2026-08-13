---
title: 'pair'
description: 'For every file matching primary, a file matching the partner template must exist. alint pair rule, cross-file family.'
sidebar:
  order: 1
categories: ['cross-file']
---

For every file matching `primary`, a file matching the `partner` template must exist.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `partner` | string | yes |  | Path template resolved per primary match. Example: "{dir}/{stem}.h". |
| `primary` | string | yes |  | Glob selecting the primary files. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A C source file with no matching header

The rule fires on this repository:

```text
src/
src/alpha.c
src/alpha.h
src/beta.c
```

`src/alpha.c`:

```c
int a;
```

`src/alpha.h`:

```c
int a;
```

`src/beta.c`:

```c
int b;
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: c-requires-h
    kind: pair
    primary: "src/**/*.c"
    partner: "{dir}/{stem}.h"
    level: error
```

### Every C source file has its header partner

This repository is compliant:

```text
src/
src/alpha.c
src/alpha.h
src/beta.c
src/beta.h
```

`src/alpha.c`:

```c
int a;
```

`src/alpha.h`:

```c
int a;
```

`src/beta.c`:

```c
int b;
```

`src/beta.h`:

```c
int b;
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: c-requires-h
    kind: pair
    primary: "src/**/*.c"
    partner: "{dir}/{stem}.h"
    level: error
```

