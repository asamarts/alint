---
title: 'no_bom'
description: 'alint no_bom rule (encoding): Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark.'
sidebar:
  order: 1
categories: ['encoding', 'text-hygiene']
---

Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. The fixer strips whichever BOM is detected.

As of v0.18, the fixer strips only a leading UTF-8 BOM (a stacked run of them in one pass); a UTF-16 / UTF-32 BOM is reported but not auto-fixed, since removing it without transcoding the file would corrupt it (the byte-level text fixers likewise leave UTF-16 / UTF-32 files alone).

---

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A Markdown file that opens with a UTF-8 byte-order mark

The rule fires on this repository:

```text
docs/
docs/clean.md
docs/with_bom.md
```

```markdown title="docs/clean.md"
# hi
```

```markdown title="docs/with_bom.md"
<U+FEFF># hi
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-bom
    kind: no_bom
    paths: "docs/**/*.md"
    level: warning
```

`alint check` reports:

```ansi
[2m--- docs/with_bom.md -----------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mno-bom[0m
              [2m1:1[0m  file begins with a UTF-8 BOM

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Markdown files saved as plain UTF-8 with no BOM

This repository is compliant:

```text
docs/
docs/a.md
docs/b.md
```

```markdown title="docs/a.md"
# hi
```

```markdown title="docs/b.md"
content
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-bom
    kind: no_bom
    paths: "docs/**/*.md"
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

