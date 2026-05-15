---
title: 'no_illegal_windows_names'
description: 'alint rule kind `no_illegal_windows_names` (Portable metadata family).'
sidebar:
  order: 2
---

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

