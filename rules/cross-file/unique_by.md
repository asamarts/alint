---
title: 'unique_by'
description: 'No two files matching select may share the value of key (a path template; tokens {path}/{dir}/{basename}/{stem}/{ext}/{parent_name}).'
sidebar:
  order: 15
categories: ['cross-file']
---

No two files matching `select` may share the value of `key` (a path template; tokens `{path}`/`{dir}`/`{basename}`/`{stem}`/`{ext}`/`{parent_name}`). Catches basename collisions across subdirectories. With `case_insensitive: true` the key is folded to lowercase before grouping, so `README.md` and `readme.md` collide — the case-insensitive-filesystem hazard (Windows / macOS).

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `case_insensitive` | boolean |  | `false` | Fold the key to lowercase before grouping, so keys that collide only under case-folding count as duplicates — the case-insensitive-filesystem hazard (Windows / macOS). |
| `key` | string |  | `{basename}` | Path-template producing a key per matched file. Default: {basename}. |
| `select` | string | yes |  | Glob selecting the files to deduplicate. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Two files colliding on the same stem

The rule fires on this repository:

```text
a/
a/util.rs
b/
b/util.rs
c/
c/other.rs
```

`a/util.rs`:

```rust
pub fn a() {}
```

`b/util.rs`:

```rust
pub fn b() {}
```

`c/other.rs`:

```rust
pub fn c() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: unique-rs-stems
    kind: unique_by
    select: "**/*.rs"
    key: "{stem}"
    level: warning
```

`alint check` reports:

```ansi
[2m--- a/util.rs ------------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2munique-rs-stems[0m
              duplicate key "util" shared by 2 file(s): a/util.rs, b/util.rs

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Every file has a distinct stem

This repository is compliant:

```text
src/
src/alpha.rs
src/beta.rs
src/gamma.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: unique-rs-stems
    kind: unique_by
    select: "**/*.rs"
    key: "{stem}"
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

