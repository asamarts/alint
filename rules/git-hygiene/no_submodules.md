---
title: 'no_submodules'
description: 'Flag the presence of .gitmodules at the repo root, always, regardless of paths. alint no_submodules rule, git hygiene family.'
sidebar:
  order: 1
categories: ['git-hygiene']
---

Flag the presence of `.gitmodules` at the repo root — always, regardless of `paths`. For general "file X must not exist" checks, use `file_absent`.

Note the fix only deletes `.gitmodules`; `git submodule deinit` and cleaning `.git/modules/` are still on the user.

## Options

_This rule takes no kind-specific options._

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A repo that declares a git submodule

The rule fires on this repository:

```text
.gitmodules
README.md
vendor/
vendor/lib/
vendor/lib/.keep
```

`.gitmodules`:

```text
[submodule "vendor/lib"]
  path = vendor/lib
  url = https://example.com/lib.git
```

`README.md`:

```markdown
# demo
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-submods
    kind: no_submodules
    level: error
```

`alint check` reports:

```ansi
[2m--- .gitmodules ----------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-submods[0m
              `.gitmodules` present — git submodules are forbidden

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### A repo with no submodules declared

This repository is compliant:

```text
.gitignore
README.md
src/
src/main.rs
```

`.gitignore`:

```text
/target
```

`README.md`:

```markdown
# demo
```

`src/main.rs`:

```rust
fn main() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-submods
    kind: no_submodules
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

