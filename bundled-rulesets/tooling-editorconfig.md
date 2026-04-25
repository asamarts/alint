---
title: 'tooling/editorconfig@v1'
description: Bundled alint ruleset at alint://bundled/tooling/editorconfig@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/tooling/editorconfig@v1
```

## Rules

### `tooling-editorconfig-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://editorconfig.org/>

> Add a root `.editorconfig` so contributors on different editors produce files with consistent indentation and line endings.

### `tooling-gitattributes-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://git-scm.com/docs/gitattributes>

> Add a root `.gitattributes` to normalize line endings across Windows/macOS/Linux checkouts. A typical minimum is `* text=auto eol=lf`.

### `tooling-gitattributes-normalizes-line-endings`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `info`

> `.gitattributes` exists but has no `* text=...` line; line-ending normalization is the main reason to ship a `.gitattributes` in the first place.

