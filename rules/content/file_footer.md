---
title: 'file_footer'
description: 'alint rule kind `file_footer` (Content family).'
sidebar:
  order: 11
---

Last `lines` lines of each file in scope must match a regex. Mirror of `file_header` anchored at the end of the file. Use for license footers, signed-off-by trailers, generated-file sentinels.

```yaml
- id: license-footer
  kind: footer
  paths: "src/**/*.rs"
  pattern: "Licensed under the Apache License, Version 2\\.0"
  lines: 3
  level: error
```

Fix: `file_append` — append a declared `content`. With no fix declared, violations are unfixable.

