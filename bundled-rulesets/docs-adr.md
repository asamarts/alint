---
title: 'docs/adr@v1'
description: Bundled alint ruleset at alint://bundled/docs/adr@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/docs/adr@v1
```

## Rules

### `adr-filename-pattern`

- **kind**: [`filename_regex`](/docs/rules/naming/filename_regex/)
- **level**: `warning`
- **policy**: <https://adr.github.io/madr/>

> ADR filename should match `NNNN-kebab-case-title.md` (four-digit number + hyphen + lowercase kebab title).

### `adr-has-status-section`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `info`

> MADR ADRs include a ## Status section.

### `adr-has-context-section`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `info`

> MADR ADRs include a ## Context section.

### `adr-has-decision-section`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `info`

> MADR ADRs include a ## Decision section.

