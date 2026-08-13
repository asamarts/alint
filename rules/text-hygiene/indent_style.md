---
title: 'indent_style'
description: 'Every non-blank line indents with the configured style (tabs or spaces). alint indent_style rule, text hygiene family.'
sidebar:
  order: 5
categories: ['text-hygiene']
---

Every non-blank line indents with the configured `style` (`tabs` or `spaces`). When `style: spaces`, optional `width` enforces a multiple.

Check-only: tab-width-aware reindentation is language-specific. Pair with your editor's "reindent on save" for remediation.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `style` | one of `tabs` \| `spaces` | yes |  | Required indentation style: `tabs` rejects any leading space; `spaces` rejects any leading tab. |
| `width` | integer (>= 1) |  | `null` | When `style: spaces`, the leading-space count on every non-blank line must be a multiple of this. Ignored for `style: tabs`. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A Go file indented with spaces instead of tabs

The rule fires on this repository:

```text
src/
src/bad.go
src/ok.go
```

`src/bad.go`:

```go
package main

func y() {
    return
}
```

`src/ok.go`:

```go
package main

func x() {
	return
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: go-tabs
    kind: indent_style
    paths: "src/**/*.go"
    style: tabs
    level: error
```

`alint check` reports:

```ansi
[2m--- src/bad.go -----------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mgo-tabs[0m
              [2m4:1[0m  line 4 indented with the wrong character (expected tabs)

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every Go file is indented with tabs

This repository is compliant:

```text
src/
src/a.go
src/b.go
```

`src/a.go`:

```go
package main

func x() {
	return
}
```

`src/b.go`:

```go
package main

func y() {
	if true {
		return
	}
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: go-tabs
    kind: indent_style
    paths: "src/**/*.go"
    style: tabs
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

