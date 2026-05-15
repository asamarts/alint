---
title: 'go@v1'
description: Bundled alint ruleset at alint://bundled/go@v1.
---

Hygiene checks for Go modules. Adopt it with:

```yaml
extends:
  - alint://bundled/go@v1
```

Gated with `when: facts.has_go` (true if any `go.mod` exists
anywhere in the tree) plus a per-rule
`scope_filter: { has_ancestor: go.mod }` on per-file content
rules so they only apply to files inside a Go module — useful
in polyglot monorepos where Go modules sit alongside
Rust / Node / Python subdirectories. Override `has_go` with
your own `facts:` block if your project uses a non-standard
location.

go.mod is a Go-specific format (not TOML/YAML/JSON), so shape
checks use `file_content_matches` rather than the structured-
query family.

## Rules

### `go-mod-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **when**: `facts.has_go`
- **policy**: <https://go.dev/ref/mod#go-mod-file>

> Go module: go.mod at the repo root is required.

### `go-sum-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **when**: `facts.has_go`
- **policy**: <https://go.dev/ref/mod#go-sum-files>

> go.sum pins the hashes of every transitive dependency; commit it for reproducible builds. Modules with zero dependencies legitimately omit it.

### `go-mod-declares-module-path`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `error`
- **when**: `facts.has_go`

> go.mod must declare a `module <path>` on its first directive.

### `go-mod-declares-go-version`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `warning`
- **when**: `facts.has_go`
- **policy**: <https://go.dev/ref/mod#go-mod-file-go>

> go.mod should declare a `go <version>` directive (e.g. `go 1.22`) so the toolchain version is explicit.

### `go-sources-no-bidi`

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **when**: `facts.has_go`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars in Go sources are rejected.

### `go-sources-no-zero-width`

- **kind**: [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/)
- **level**: `error`
- **when**: `facts.has_go`

> Zero-width characters in Go sources are rejected (review hazard).

### `go-sources-final-newline`

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `info`
- **when**: `facts.has_go`

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/go.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/go.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/go@v1
#
# Hygiene checks for Go modules. Adopt it with:
#
#     extends:
#       - alint://bundled/go@v1
#
# Gated with `when: facts.has_go` (true if any `go.mod` exists
# anywhere in the tree) plus a per-rule
# `scope_filter: { has_ancestor: go.mod }` on per-file content
# rules so they only apply to files inside a Go module — useful
# in polyglot monorepos where Go modules sit alongside
# Rust / Node / Python subdirectories. Override `has_go` with
# your own `facts:` block if your project uses a non-standard
# location.
#
# go.mod is a Go-specific format (not TOML/YAML/JSON), so shape
# checks use `file_content_matches` rather than the structured-
# query family.

version: 1

facts:
  - id: has_go
    any_file_exists: [go.mod, "**/go.mod"]

rules:
  # --- Module manifest ---------------------------------------------
  - id: go-mod-exists
    when: facts.has_go
    kind: file_exists
    paths: go.mod
    root_only: true
    level: error
    message: "Go module: go.mod at the repo root is required."
    policy_url: "https://go.dev/ref/mod#go-mod-file"

  - id: go-sum-exists
    when: facts.has_go
    # go.sum pins every transitive dependency's hash — missing it
    # means non-reproducible builds. Modules with zero deps
    # legitimately omit go.sum; disable via `level: off` in that
    # case.
    kind: file_exists
    paths: go.sum
    root_only: true
    level: warning
    message: >-
      go.sum pins the hashes of every transitive dependency;
      commit it for reproducible builds. Modules with zero
      dependencies legitimately omit it.
    policy_url: "https://go.dev/ref/mod#go-sum-files"

  # --- go.mod shape ------------------------------------------------
  - id: go-mod-declares-module-path
    when: facts.has_go
    # Every go.mod starts with `module <path>`. Absent or empty
    # module path means `go build` will refuse the module.
    kind: file_content_matches
    paths: go.mod
    pattern: '(?m)^module\s+\S+'
    level: error
    message: "go.mod must declare a `module <path>` on its first directive."

  - id: go-mod-declares-go-version
    when: facts.has_go
    # Every go.mod should declare a `go <major>.<minor>` toolchain
    # floor. Missing it means the toolchain selects its default,
    # which changes across Go releases.
    kind: file_content_matches
    paths: go.mod
    pattern: '(?m)^go\s+\d+\.\d+'
    level: warning
    message: >-
      go.mod should declare a `go <version>` directive (e.g.
      `go 1.22`) so the toolchain version is explicit.
    policy_url: "https://go.dev/ref/mod#go-mod-file-go"

  # --- Trojan Source defense on Go sources -------------------------
  - id: go-sources-no-bidi
    when: facts.has_go
    kind: no_bidi_controls
    paths: "**/*.go"
    scope_filter:
      has_ancestor: go.mod
    level: error
    message: "Trojan Source (CVE-2021-42574): bidi override chars in Go sources are rejected."
    policy_url: "https://trojansource.codes/"

  - id: go-sources-no-zero-width
    when: facts.has_go
    kind: no_zero_width_chars
    paths: "**/*.go"
    scope_filter:
      has_ancestor: go.mod
    level: error
    message: "Zero-width characters in Go sources are rejected (review hazard)."

  # --- Source-file hygiene on Go sources ---------------------------
  - id: go-sources-final-newline
    when: facts.has_go
    kind: final_newline
    paths: "**/*.go"
    scope_filter:
      has_ancestor: go.mod
    level: info
    fix:
      file_append_final_newline: {}
```
