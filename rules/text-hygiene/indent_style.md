---
title: 'indent_style'
description: 'Every non-blank line indents with the configured style (tabs or spaces). alint indent_style rule, text hygiene family.'
sidebar:
  order: 5
categories: ['text-hygiene']
---

Every non-blank line indents with the configured `style` (`tabs` or `spaces`). When `style: spaces`, optional `width` enforces a multiple.

Fix: `indent_style` — reindents tab-indented lines to spaces, for a `style: spaces` + `width: N` rule only (the `width` is the spaces-per-tab). It converts a PURE-TAB leading run to `N` spaces per tab (1 tab → N, 2 tabs → 2N), preserving the rest of the line, its terminator, and the trailing-newline state; it declines the genuinely ambiguous cases — a mixed tab+space lead, or a pure-space run that isn't a multiple of `width` — so `check` does not advertise those as auto-fixable. A `tabs`-style or width-less rule with a `fix` is rejected at load (`spaces → tabs` has no spaces-per-tab). **`Unsafe` by default** — a mis-aimed reindent hard-breaks an indent-significant file (a `Makefile` recipe requires a literal tab), so a bare `alint fix` suggests it and `--unsafe-fixes` (or a per-rule `applicability: safe`) applies it; it is also demoted to a suggestion from an untrusted remote `extends:`. With no fix declared, violations are unfixable; pair with your editor's "reindent on save".

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

```go title="src/bad.go"
package main

func y() {
    return
}
```

```go title="src/ok.go"
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

```go title="src/a.go"
package main

func x() {
	return
}
```

```go title="src/b.go"
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

