---
title: 'go@v1'
description: Bundled alint ruleset at alint://bundled/go@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/go@v1
```

## Rules

### `go-mod-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **when**: `facts.is_go`
- **policy**: <https://go.dev/ref/mod#go-mod-file>

> Go module: go.mod at the repo root is required.

### `go-sum-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **when**: `facts.is_go`
- **policy**: <https://go.dev/ref/mod#go-sum-files>

> go.sum pins the hashes of every transitive dependency; commit it for reproducible builds. Modules with zero dependencies legitimately omit it.

### `go-mod-declares-module-path`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `error`
- **when**: `facts.is_go`

> go.mod must declare a `module <path>` on its first directive.

### `go-mod-declares-go-version`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `warning`
- **when**: `facts.is_go`
- **policy**: <https://go.dev/ref/mod#go-mod-file-go>

> go.mod should declare a `go <version>` directive (e.g. `go 1.22`) so the toolchain version is explicit.

### `go-sources-no-bidi`

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **when**: `facts.is_go`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars in Go sources are rejected.

### `go-sources-no-zero-width`

- **kind**: [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/)
- **level**: `error`
- **when**: `facts.is_go`

> Zero-width characters in Go sources are rejected (review hazard).

### `go-sources-final-newline`

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `info`
- **when**: `facts.is_go`

