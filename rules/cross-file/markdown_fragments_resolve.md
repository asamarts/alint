---
title: 'markdown_fragments_resolve'
description: 'alint markdown_fragments_resolve rule (cross-file): Require every live local Markdown link fragment to name a heading anchor in its resolved target document.'
sidebar:
  order: 4
categories: ['cross-file', 'content']
---

Require every live local Markdown link fragment to name a heading anchor in its resolved target document.

The rule shares `markdown_links_resolve`'s CommonMark link parsing, percent-decoded path resolution, and optional rendered-site `root` mapping, then parses each Markdown target once and generates GitHub-style anchors. Duplicate headings receive `-1`, `-2`, and later suffixes; percent-encoded fragments are decoded before comparison. External destinations and non-Markdown targets are outside this kind's contract. A missing target file is left to `markdown_links_resolve`, so the two rules compose without duplicate findings. Every bad fragment is anchored on the source destination and includes the target document as a related location.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `relative` | one of `resolve` \| `forbid` |  | `resolve` | How to handle relative destinations. Under `forbid`, another-file links are left to `markdown_links_resolve`; same-document fragments still run. |
| `root` | object { `dir`, `url_prefix` } |  |  | Optional rendered-site root URL to repository source mapping. |

Plus the common `paths`, `level`, `id`, `when`, and `expect_matches` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A fragment that names no target heading is reported

The rule fires on this repository:

```text
README.md
docs/guide.md
```

```markdown title="README.md"
# Project

Read the [installation guide](docs/guide.md#installing).
```

```markdown title="docs/guide.md"
# Guide

## Setup
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: heading-links
    kind: markdown_fragments_resolve
    paths: "**/*.md"
    level: error
```

`alint check` reports:

```ansi
[2m--- README.md ------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mheading-links[0m
              [2m3:10[0m  Markdown fragment `#installing` does not name a heading in
              `docs/guide.md`
              [2mrelated:[0m Markdown fragment target document: docs/guide.md

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Encoded fragments and duplicate-anchor suffixes resolve

This repository is compliant:

```text
README.md
docs/guide.md
```

```markdown title="README.md"
# Project

Read [Café](docs/guide.md#caf%C3%A9) and [the second API](docs/guide.md#api-1).
```

```markdown title="docs/guide.md"
# Guide

## Café

## API

## API
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: heading-links
    kind: markdown_fragments_resolve
    paths: "**/*.md"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

