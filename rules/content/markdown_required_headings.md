---
title: 'markdown_required_headings'
description: 'alint markdown_required_headings rule (content): Require the complete rendered heading sequence to match an ordered headings template.'
sidebar:
  order: 18
categories: ['content']
---

Require the complete rendered heading sequence to match an ordered `headings` template.

Each entry is `# Text` through `###### Text`; `*` matches zero or more complete headings. Matching is exact after CommonMark rendering, so `# A *title*` satisfies `"# A title"`, but a different level or extra heading outside a wildcard fails. This is the structural replacement for repeating `file_content_matches` rules when every ADR or runbook must follow one document shape.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `headings` | list of string | yes |  | Exact rendered headings (`# Text` through `###### Text`) in document order. `*` matches zero or more complete headings. |

Plus the common `paths`, `level`, `id`, `when`, `scope_filter`, and `expect_matches` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A required ordered heading is missing

The rule fires on this repository:

```text
docs/adr/0001-choice.md
```

```markdown title="docs/adr/0001-choice.md"
# Choice

## Status

Accepted.
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: adr-shape
    kind: markdown_required_headings
    paths: "docs/adr/*.md"
    headings: ["# Choice", "## Status", "*", "## Context", "## Decision"]
    level: error
```

`alint check` reports:

```ansi
[2m--- docs/adr/0001-choice.md ----------------------------------------------------[0m
  [1m[31mx  error  [0m  [2madr-shape[0m
              [2m1:1[0m  Markdown heading sequence does not match the required template

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Required headings remain ordered around arbitrary intermediate sections

This repository is compliant:

```text
docs/adr/0001-choice.md
```

```markdown title="docs/adr/0001-choice.md"
# Choice

## Status

## Consequences

### Positive

## Context

## Decision
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: adr-shape
    kind: markdown_required_headings
    paths: "docs/adr/*.md"
    headings: ["# Choice", "## Status", "*", "## Context", "## Decision"]
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

