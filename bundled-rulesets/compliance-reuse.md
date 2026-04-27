---
title: 'compliance/reuse@v1'
description: Bundled alint ruleset at alint://bundled/compliance/reuse@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/compliance/reuse@v1
```

## Rules

### `reuse-licenses-dir-exists`

- **kind**: [`dir_exists`](/docs/rules/existence/dir_exists/)
- **level**: `error`
- **policy**: <https://reuse.software/spec/#license-files>

> REUSE-compliant projects need a `LICENSES/` directory containing the full text of each license referenced by `SPDX-License-Identifier:` headers (named e.g. `LICENSES/Apache-2.0.txt`). Run `reuse download --all` to populate it from the SPDX corpus.

### `reuse-source-has-spdx-identifier`

- **kind**: [`file_header`](/docs/rules/content/file_header/)
- **level**: `warning`
- **policy**: <https://reuse.software/spec/#comment-headers>

> REUSE: every source file should declare its license with an `SPDX-License-Identifier:` header in the first few lines (in a comment). Use a `.license` companion file or a `REUSE.toml` mapping if the file format can't carry comments.

### `reuse-source-has-copyright-text`

- **kind**: [`file_header`](/docs/rules/content/file_header/)
- **level**: `warning`
- **policy**: <https://reuse.software/spec/#format>

> REUSE: every source file should declare its copyright with an `SPDX-FileCopyrightText: <year> <holder>` header alongside the SPDX-License-Identifier.

