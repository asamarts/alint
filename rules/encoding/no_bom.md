---
title: 'no_bom'
description: 'Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. alint no_bom rule, encoding family.'
sidebar:
  order: 1
categories: ['encoding', 'text-hygiene']
---

Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. The fixer strips whichever BOM is detected.

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

`docs/clean.md`:

```markdown
# hi
```

`docs/with_bom.md`:

```markdown
﻿# hi
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

### Markdown files saved as plain UTF-8 with no BOM

This repository is compliant:

```text
docs/
docs/a.md
docs/b.md
```

`docs/a.md`:

```markdown
# hi
```

`docs/b.md`:

```markdown
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

