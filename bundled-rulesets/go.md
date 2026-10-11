---
title: 'go@v1'
description: 'go@v1 bundled alint ruleset: Hygiene checks for Go modules.'
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

## What it checks

7 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [go-mod-exists](#go-mod-exists)<br>`error` | Go module: go.mod at the repo root is required. |
| [go-sum-exists](#go-sum-exists)<br>`warning` | go.sum pins the hashes of every transitive dependency; commit it for reproducible builds. |
| [go-mod-declares-module-path](#go-mod-declares-module-path)<br>`error` | go.mod must declare a `module <path>` on its first directive. |
| [go-mod-declares-go-version](#go-mod-declares-go-version)<br>`warning` | go.mod should declare a `go <version>` directive (e.g. `go 1.22`) so the toolchain version is explicit. |
| [go-sources-no-bidi](#go-sources-no-bidi)<br>`error` | Trojan Source (CVE-2021-42574): bidi override chars in Go sources are rejected. |
| [go-sources-no-zero-width](#go-sources-no-zero-width)<br>`error` | Zero-width characters in Go sources are rejected (review hazard). |
| [go-sources-final-newline](#go-sources-final-newline)<br>`info` | File must end with a single \n. |

All 7 rules run only when `facts.has_go` holds, so the ruleset stays quiet in repositories it doesn't apply to.

## Rules

### `go-mod-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **when**: `facts.has_go`
- **policy**: <https://go.dev/ref/mod#go-mod-file>

> Go module: go.mod at the repo root is required.

```yaml
- id: go-mod-exists
  when: facts.has_go
  kind: file_exists
  paths: go.mod
  root_only: true
  level: error
  message: "Go module: go.mod at the repo root is required."
  policy_url: "https://go.dev/ref/mod#go-mod-file"
```

### `go-sum-exists`

go.sum pins every transitive dependency's hash — missing it means non-reproducible builds. Modules with zero deps legitimately omit go.sum; disable via `level: off` in that case.

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **when**: `facts.has_go`
- **policy**: <https://go.dev/ref/mod#go-sum-files>

> go.sum pins the hashes of every transitive dependency; commit it for reproducible builds. Modules with zero dependencies legitimately omit it.

```yaml
- id: go-sum-exists
  when: facts.has_go
  kind: file_exists
  paths: go.sum
  root_only: true
  level: warning
  message: >-
    go.sum pins the hashes of every transitive dependency;
    commit it for reproducible builds. Modules with zero
    dependencies legitimately omit it.
  policy_url: "https://go.dev/ref/mod#go-sum-files"
```

### `go-mod-declares-module-path`

Every go.mod starts with `module <path>`. Absent or empty module path means `go build` will refuse the module.

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `error`
- **when**: `facts.has_go`

> go.mod must declare a `module <path>` on its first directive.

```yaml
- id: go-mod-declares-module-path
  when: facts.has_go
  kind: file_content_matches
  paths: go.mod
  pattern: '(?m)^module\s+\S+'
  level: error
  message: "go.mod must declare a `module <path>` on its first directive."
```

### `go-mod-declares-go-version`

Every go.mod should declare a `go <major>.<minor>` toolchain floor. Missing it means the toolchain selects its default, which changes across Go releases.

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `warning`
- **when**: `facts.has_go`
- **policy**: <https://go.dev/ref/mod#go-mod-file-go>

> go.mod should declare a `go <version>` directive (e.g. `go 1.22`) so the toolchain version is explicit.

```yaml
- id: go-mod-declares-go-version
  when: facts.has_go
  kind: file_content_matches
  paths: go.mod
  pattern: '(?m)^go\s+\d+\.\d+'
  level: warning
  message: >-
    go.mod should declare a `go <version>` directive (e.g.
    `go 1.22`) so the toolchain version is explicit.
  policy_url: "https://go.dev/ref/mod#go-mod-file-go"
```

### `go-sources-no-bidi`

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **when**: `facts.has_go`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars in Go sources are rejected.

```yaml
- id: go-sources-no-bidi
  when: facts.has_go
  kind: no_bidi_controls
  paths: "**/*.go"
  scope_filter:
    has_ancestor: go.mod
  level: error
  message: "Trojan Source (CVE-2021-42574): bidi override chars in Go sources are rejected."
  policy_url: "https://trojansource.codes/"
```

### `go-sources-no-zero-width`

- **kind**: [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/)
- **level**: `error`
- **when**: `facts.has_go`

> Zero-width characters in Go sources are rejected (review hazard).

```yaml
- id: go-sources-no-zero-width
  when: facts.has_go
  kind: no_zero_width_chars
  paths: "**/*.go"
  scope_filter:
    has_ancestor: go.mod
  level: error
  message: "Zero-width characters in Go sources are rejected (review hazard)."
```

### `go-sources-final-newline`

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `info`
- **when**: `facts.has_go`
- **fix**: `file_append_final_newline` (applied by `alint fix`)

```yaml
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

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  - id: has_go
    any_file_exists: [go.mod, "**/go.mod"]
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/go@v1
rules:
  - id: go-mod-exists
    level: off
  - id: go-sum-exists
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/go@v1
    except: [go-mod-exists]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/go.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/go.yml) in the alint repo.
