---
title: 'markdown_no_duplicate_headings'
description: 'alint markdown_no_duplicate_headings rule (content): Reject a later Markdown heading whose generated base anchor collides with an earlier heading.'
sidebar:
  order: 19
categories: ['content']
---

Reject a later Markdown heading whose generated base anchor collides with an earlier heading.

Anchors lowercase Unicode text, turn whitespace into `-`, and remove ASCII punctuation except `-` and `_`; explicit `{#id}` heading attributes use that id. The finding cites the first colliding heading. This catches case and punctuation variants that render as competing deep-link anchors while allowing repeated visible labels with deliberately distinct explicit ids.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, `when`, `scope_filter`, and `expect_matches` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Headings that normalize to the same anchor are reported

The rule fires on this repository:

```text
README.md
```

```markdown title="README.md"
# Project

## API: HTTP

## API HTTP
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: unique-headings
    kind: markdown_no_duplicate_headings
    paths: "**/*.md"
    level: error
```

`alint check` reports:

```ansi
[2m--- README.md ------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2munique-headings[0m
              [2m5:1[0m  Markdown heading generates duplicate anchor `#api-http`
              [2mrelated:[0m first heading with this anchor: README.md:3:1-4:1

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Repeated labels with distinct explicit anchors remain valid

This repository is compliant:

```text
README.md
```

```markdown title="README.md"
# Project

## Examples {#basic-examples}

## Examples {#advanced-examples}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: unique-headings
    kind: markdown_no_duplicate_headings
    paths: "**/*.md"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

