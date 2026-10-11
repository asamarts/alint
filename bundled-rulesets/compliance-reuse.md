---
title: 'compliance/reuse@v1'
description: 'compliance/reuse@v1 bundled alint ruleset: Hygiene checks for repositories that follow the FSFE REUSE Specification (https://reuse.software/), every...'
---

Hygiene checks for repositories that follow the FSFE REUSE
Specification (https://reuse.software/) — every licensable
file declares its license + copyright via an SPDX header (or
a `.license` companion / REUSE.toml entry), and the full
license texts live under `LICENSES/`.

Adopt with:

```yaml
extends:
  - alint://bundled/compliance/reuse@v1
```

This ruleset has no fact gate — extending it is the user's
signal that they intend to be REUSE-compliant. If you need
narrower coverage, override `paths:` on the SPDX-header rule
(e.g. limit to one source tree) or `level: off` rules you
don't want.

## What it checks

3 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [reuse-licenses-dir-exists](#reuse-licenses-dir-exists)<br>`error` | REUSE-compliant projects need a `LICENSES/` directory containing the full text of each license referenced by `SPDX-License-Identifier:` headers (named e.g. `LICENSES/Apache-2.0.txt`). |
| [reuse-source-has-spdx-identifier](#reuse-source-has-spdx-identifier)<br>`warning` | REUSE: every source file should declare its license with an `SPDX-License-Identifier:` header in the first few lines (in a comment). |
| [reuse-source-has-copyright-text](#reuse-source-has-copyright-text)<br>`warning` | REUSE: every source file should declare its copyright with an `SPDX-FileCopyrightText: <year> <holder>` header alongside the SPDX-License-Identifier. |

## Rules

### `reuse-licenses-dir-exists`

The REUSE spec mandates a top-level `LICENSES/` directory containing the full text of every license used in the project (named by SPDX identifier, e.g. `LICENSES/MIT.txt`). `reuse lint` rejects a project without it, and so does this rule.

- **kind**: [`dir_exists`](/docs/rules/existence/dir_exists/)
- **level**: `error`
- **policy**: <https://reuse.software/spec/#license-files>

> REUSE-compliant projects need a `LICENSES/` directory containing the full text of each license referenced by `SPDX-License-Identifier:` headers (named e.g. `LICENSES/Apache-2.0.txt`). Run `reuse download --all` to populate it from the SPDX corpus.

```yaml
- id: reuse-licenses-dir-exists
  kind: dir_exists
  paths: "LICENSES"
  level: error
  message: >-
    REUSE-compliant projects need a `LICENSES/` directory
    containing the full text of each license referenced by
    `SPDX-License-Identifier:` headers (named e.g.
    `LICENSES/Apache-2.0.txt`). Run `reuse download --all`
    to populate it from the SPDX corpus.
  policy_url: "https://reuse.software/spec/#license-files"
```

### `reuse-source-has-spdx-identifier`

Every common-source-extension file should carry an SPDX-License-Identifier header in its first \~10 lines. Files that license-via-companion (`*.license`) or via `REUSE.toml` mappings legitimately lack inline headers — narrow `paths:` to your source trees if your project uses those mechanisms heavily.

- **kind**: [`file_header`](/docs/rules/content/file_header/)
- **level**: `warning`
- **policy**: <https://reuse.software/spec/#comment-headers>

> REUSE: every source file should declare its license with an `SPDX-License-Identifier:` header in the first few lines (in a comment). Use a `.license` companion file or a `REUSE.toml` mapping if the file format can't carry comments.

```yaml
- id: reuse-source-has-spdx-identifier
  kind: file_header
  paths:
    include:
      ["**/*.{rs,py,js,jsx,ts,tsx,go,java,kt,c,cc,cpp,h,hpp,hh,sh,rb,swift}"]
    exclude:
      - "**/vendor/**"
      - "**/node_modules/**"
      - "**/target/**"
      - "**/build/**"
      - "**/dist/**"
      - "**/.cargo/**"
  lines: 10
  pattern: "SPDX-License-Identifier:"
  level: warning
  message: >-
    REUSE: every source file should declare its license
    with an `SPDX-License-Identifier:` header in the first
    few lines (in a comment). Use a `.license` companion
    file or a `REUSE.toml` mapping if the file format
    can't carry comments.
  policy_url: "https://reuse.software/spec/#comment-headers"
```

### `reuse-source-has-copyright-text`

Every common-source-extension file should also carry a SPDX FileCopyrightText header naming the copyright holder. Required by the REUSE spec alongside `SPDX-License-Identifier:`.

- **kind**: [`file_header`](/docs/rules/content/file_header/)
- **level**: `warning`
- **policy**: <https://reuse.software/spec/#format>

> REUSE: every source file should declare its copyright with an `SPDX-FileCopyrightText: <year> <holder>` header alongside the SPDX-License-Identifier.

```yaml
- id: reuse-source-has-copyright-text
  kind: file_header
  paths:
    include:
      ["**/*.{rs,py,js,jsx,ts,tsx,go,java,kt,c,cc,cpp,h,hpp,hh,sh,rb,swift}"]
    exclude:
      - "**/vendor/**"
      - "**/node_modules/**"
      - "**/target/**"
      - "**/build/**"
      - "**/dist/**"
      - "**/.cargo/**"
  lines: 10
  pattern: "SPDX-FileCopyrightText:"
  level: warning
  message: >-
    REUSE: every source file should declare its copyright
    with an `SPDX-FileCopyrightText: <year> <holder>`
    header alongside the SPDX-License-Identifier.
  policy_url: "https://reuse.software/spec/#format"
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/compliance/reuse@v1
rules:
  - id: reuse-licenses-dir-exists
    level: off
  - id: reuse-source-has-spdx-identifier
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/compliance/reuse@v1
    except: [reuse-licenses-dir-exists]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/compliance/reuse.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/compliance/reuse.yml) in the alint repo.
