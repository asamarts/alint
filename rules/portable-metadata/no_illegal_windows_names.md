---
title: 'no_illegal_windows_names'
description: 'Reject path components Windows can''t represent. alint no_illegal_windows_names rule, portable metadata family.'
sidebar:
  order: 2
categories: ['portable-metadata', 'naming']
---

Reject path components Windows can't represent:

- Reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`) — case-insensitive, regardless of extension. `con.txt` fails; `COM10` and `confused` correctly pass.
- Trailing dots (`foo.`) or trailing spaces (`foo `) — Windows silently strips these on checkout.
- Reserved chars: `<`, `>`, `:`, `"`, `|`, `?`, `*`.

---

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Files using reserved Windows device names and a trailing dot

The rule fires on this repository:

```text
COM1.md
NUL
README.md
con.txt
trailing_dot.
```

`README.md`:

```markdown
# fine
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-win-reserved
    kind: no_illegal_windows_names
    paths: "**"
    level: error
```

`alint check` reports:

```ansi
[2m--- COM1.md --------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-win-reserved[0m
              clashes with a Windows reserved device name: "COM1.md"

[2m--- NUL ------------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-win-reserved[0m
              clashes with a Windows reserved device name: "NUL"

[2m--- con.txt --------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-win-reserved[0m
              clashes with a Windows reserved device name: "con.txt"

[2m--- trailing_dot. --------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2mno-win-reserved[0m
              Windows strips trailing dots on checkout: "trailing_dot."

[2mSummary (4 violations):[0m
  [1m[31mx 4 errors[0m
  0 passing [2m*[0m 1 failing
```

### Names that resemble reserved Windows names but stay legal

This repository is compliant:

```text
COM10.cfg
README.md
src/
src/controllers.rs
src/lib.rs
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-win-reserved
    kind: no_illegal_windows_names
    paths: "**"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

