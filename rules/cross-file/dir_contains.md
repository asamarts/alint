---
title: 'dir_contains'
description: 'Every directory matching select: must contain files matching every glob in require. alint dir_contains rule, cross-file family.'
sidebar:
  order: 13
categories: ['cross-file', 'structure']
---

Every directory matching `select:` must contain files matching every glob in `require:`. Sugar for a common `for_each_dir` shape.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `require` | RequireList | yes |  | Basename glob(s): every dir matching `select` must have at least one child matching each. |
| `select` | string | yes |  | Glob selecting the directories to check. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A package directory missing its README or license

The rule fires on this repository:

```text
packages/
packages/alpha/
packages/alpha/LICENSE
packages/alpha/README.md
packages/beta/
packages/beta/README.md
packages/gamma/
packages/gamma/LICENSE
```

`packages/alpha/LICENSE`:

```text
MIT
```

`packages/alpha/README.md`:

```markdown
# alpha
```

`packages/beta/README.md`:

```markdown
# beta
```

`packages/gamma/LICENSE`:

```text
MIT
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: packages-have-readme-and-license
    kind: dir_contains
    select: "packages/*"
    require: ["README.md", "LICENSE*"]
    level: error
```

`alint check` reports:

```ansi
[2m--- packages/beta --------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mpackages-have-readme-and-license[0m
              packages/beta is missing a child matching "LICENSE*"

[2m--- packages/gamma -------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mpackages-have-readme-and-license[0m
              packages/gamma is missing a child matching "README.md"

[2mSummary (2 violations):[0m
  [1m[31mx 2 errors[0m
  0 passing [2m*[0m 1 failing
```

### Every package directory has a README and a license

This repository is compliant:

```text
packages/
packages/alpha/
packages/alpha/LICENSE
packages/alpha/README.md
packages/beta/
packages/beta/LICENSE-APACHE
packages/beta/README.md
```

`packages/alpha/LICENSE`:

```text
MIT
```

`packages/alpha/README.md`:

```markdown
# alpha
```

`packages/beta/LICENSE-APACHE`:

```text
Apache-2.0 text
```

`packages/beta/README.md`:

```markdown
# beta
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: packages-have-readme-and-license
    kind: dir_contains
    select: "packages/*"
    require: ["README.md", "LICENSE*"]
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

