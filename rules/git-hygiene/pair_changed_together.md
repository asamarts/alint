---
title: 'pair_changed_together'
description: 'If the <since>...HEAD diff changes any path matching if_changed:, at least one path matching then_changed: must change in the same range,.'
sidebar:
  order: 13
categories: ['git-hygiene', 'cross-file']
---

If the `<since>...HEAD` diff changes any path matching `if_changed:`, at least one path matching `then_changed:` must change in the same range — the **co-change** gate. Corpus signals: rust's `rustdoc-json-types` `FORMAT_VERSION` must bump when the format struct changes; "`version.txt` and the lockfile change together" release guards. Both globs and `since:` (the base ref) are required. **Directional** — the trigger is `if_changed`, the obligation is `then_changed`; a `then_changed`-only change never fires it, so add a second rule with the globs swapped for a bidirectional pact. The `changeset_requires_path` sibling, built on the same merge-base diff as `alint check --changed`. Silent no-op outside a git repo or when `if_changed` didn't change; a `since:` that fails to resolve hard-fails with a shallow-clone hint.

---

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `if_changed` | string | yes |  | Glob; the trigger. When the diff changes a path matching this, a `then_changed` co-change is required. |
| `since` | string | yes |  | Base ref for the `<since>...HEAD` diff. Use the canonical `{{env.X}}` interpolation, e.g. `since: "{{env.ALINT_BASE_SHA \| default('origin/main')}}"`. |
| `then_changed` | string | yes |  | Glob; the obligation. At least one changed path must match it whenever `if_changed` fired. |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A format change without the version bump

The rule fires on this repository:

```text
src/
src/format.rs
```

`src/format.rs`:

```rust
pub struct Format;
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: format-version
    kind: pair_changed_together
    if_changed: "src/format.rs"
    then_changed: "FORMAT_VERSION"
    since: HEAD~1
    level: error
```

committed with this history (oldest first):

```text
chore: base
change the format struct  (adds src/format.rs)
```

`alint check` reports:

```ansi
[2m--- Repository-level -----------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mformat-version[0m
              the changeset `HEAD~1...HEAD` changes a path matching
              `src/format.rs` but no path matching `FORMAT_VERSION` changed with
              it

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A format change with the version bump

This repository is compliant:

```text
FORMAT_VERSION
src/
src/format.rs
```

`FORMAT_VERSION`:

```text
2
```

`src/format.rs`:

```rust
pub struct Format;
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: format-version
    kind: pair_changed_together
    if_changed: "src/format.rs"
    then_changed: "FORMAT_VERSION"
    since: HEAD~1
    level: error
```

committed with this history (oldest first):

```text
chore: base
change the format struct and bump the version  (adds src/format.rs, FORMAT_VERSION)
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

