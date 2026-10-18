---
title: 'markdown_images_have_alt'
description: 'alint markdown_images_have_alt rule (content): Require every live CommonMark image to have non-empty rendered alt text.'
sidebar:
  order: 17
categories: ['content']
---

Require every live CommonMark image to have non-empty rendered alt text.

Formatted label text counts, while whitespace-only labels do not. Fenced and indented code, inline code syntax, comments, and front matter are excluded by the CommonMark parser. Raw HTML `<img>` elements are outside this kind's contract. Check-only because useful alternative text requires author intent.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, `when`, `scope_filter`, and `expect_matches` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A Markdown image without rendered alt text is reported

The rule fires on this repository:

```text
README.md
architecture.svg
```

```markdown title="README.md"
# Project

![](architecture.svg)
```

```text title="architecture.svg"
<svg/>
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: image-alt
    kind: markdown_images_have_alt
    paths: "**/*.md"
    level: error
```

`alint check` reports:

```ansi
[2m--- README.md ------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mimage-alt[0m
              [2m3:1[0m  Markdown image has no alt text

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Formatted image alt text is recognized by the CommonMark parser

This repository is compliant:

```text
README.md
architecture.svg
```

```markdown title="README.md"
# Project

![Component **architecture**](architecture.svg)
`![](example-only.svg)`
```

```text title="architecture.svg"
<svg/>
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: image-alt
    kind: markdown_images_have_alt
    paths: "**/*.md"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

