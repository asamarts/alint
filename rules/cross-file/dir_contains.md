---
title: 'dir_contains'
description: 'Every directory matching select: must contain files matching every glob in require:. alint dir_contains rule, cross-file family.'
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

