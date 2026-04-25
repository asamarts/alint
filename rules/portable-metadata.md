---
title: 'Portable metadata'
description: 'Rule reference: the portable metadata family.'
sidebar:
  order: 10
---

Checks that reject tree shapes which work on one OS but break checkouts elsewhere.

### `no_case_conflicts`

Flag paths that differ only by case (e.g. `README.md` + `readme.md`). They can't coexist on macOS HFS+/APFS or Windows NTFS defaults, so a Linux-only dev committing both breaks checkouts for teammates.

### `no_illegal_windows_names`

Reject path components Windows can't represent:

- Reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`) — case-insensitive, regardless of extension. `con.txt` fails; `COM10` and `confused` correctly pass.
- Trailing dots (`foo.`) or trailing spaces (`foo `) — Windows silently strips these on checkout.
- Reserved chars: `<`, `>`, `:`, `"`, `|`, `?`, `*`.

```yaml
- id: portable-names
  kind: no_illegal_windows_names
  paths: "**"
  level: warning
```

---

