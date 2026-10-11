---
title: 'tooling/editorconfig@v1'
description: 'tooling/editorconfig@v1 bundled alint ruleset: Cross-editor standardization: an .editorconfig at the root plus a .gitattributes that normalizes line endings.'
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

## What it checks

3 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [tooling-editorconfig-exists](#tooling-editorconfig-exists)<br>`info` | Add a root `.editorconfig` so contributors on different editors produce files with consistent indentation and line endings. |
| [tooling-gitattributes-exists](#tooling-gitattributes-exists)<br>`info` | Add a root `.gitattributes` to normalize line endings across Windows/macOS/Linux checkouts. |
| [tooling-gitattributes-normalizes-line-endings](#tooling-gitattributes-normalizes-line-endings)<br>`info` | `.gitattributes` exists but has no `* text=...` line; line-ending normalization is the main reason to ship a `.gitattributes` in the first place. |

## Rules

### `tooling-editorconfig-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://editorconfig.org/>

> Add a root `.editorconfig` so contributors on different editors produce files with consistent indentation and line endings.

```yaml
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
```

### `tooling-gitattributes-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://git-scm.com/docs/gitattributes>

> Add a root `.gitattributes` to normalize line endings across Windows/macOS/Linux checkouts. A typical minimum is `* text=auto eol=lf`.

```yaml
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
```

### `tooling-gitattributes-normalizes-line-endings`

When .gitattributes exists, it should contain the `text` normalization directive — otherwise it's providing little value beyond the file's existence.

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `info`

> `.gitattributes` exists but has no `* text=...` line; line-ending normalization is the main reason to ship a `.gitattributes` in the first place.

```yaml
- id: tooling-gitattributes-normalizes-line-endings
  kind: file_content_matches
  paths: .gitattributes
  pattern: '(?m)^\s*\*\s+text='
  level: info
  message: >-
    `.gitattributes` exists but has no `* text=...` line;
    line-ending normalization is the main reason to ship a
    `.gitattributes` in the first place.
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/tooling/editorconfig@v1
rules:
  - id: tooling-editorconfig-exists
    level: off
  - id: tooling-gitattributes-exists
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/tooling/editorconfig@v1
    except: [tooling-editorconfig-exists]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/tooling/editorconfig.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/tooling/editorconfig.yml) in the alint repo.
