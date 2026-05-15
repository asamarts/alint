---
title: 'tooling/editorconfig@v1'
description: Bundled alint ruleset at alint://bundled/tooling/editorconfig@v1.
---

Cross-editor standardization: an `.editorconfig` at the root
plus a `.gitattributes` that normalizes line endings. Both
are near-universal in well-run repos because they prevent
the most common style-churn PR comments before an author
even hits save.

## Adopt with

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

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/tooling/editorconfig.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/tooling/editorconfig.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/tooling/editorconfig@v1
#
# Cross-editor standardization: an `.editorconfig` at the root
# plus a `.gitattributes` that normalizes line endings. Both
# are near-universal in well-run repos because they prevent
# the most common style-churn PR comments before an author
# even hits save.

version: 1

rules:
  - id: tooling-editorconfig-exists
    kind: file_exists
    paths: .editorconfig
    root_only: true
    level: info
    message: >-
      Add a root `.editorconfig` so contributors on different
      editors produce files with consistent indentation and
      line endings.
    policy_url: "https://editorconfig.org/"

  - id: tooling-gitattributes-exists
    kind: file_exists
    paths: .gitattributes
    root_only: true
    level: info
    message: >-
      Add a root `.gitattributes` to normalize line endings
      across Windows/macOS/Linux checkouts. A typical minimum
      is `* text=auto eol=lf`.
    policy_url: "https://git-scm.com/docs/gitattributes"

  - id: tooling-gitattributes-normalizes-line-endings
    # When .gitattributes exists, it should contain the `text`
    # normalization directive — otherwise it's providing little
    # value beyond the file's existence.
    kind: file_content_matches
    paths: .gitattributes
    pattern: '(?m)^\s*\*\s+text='
    level: info
    message: >-
      `.gitattributes` exists but has no `* text=...` line;
      line-ending normalization is the main reason to ship a
      `.gitattributes` in the first place.
```
