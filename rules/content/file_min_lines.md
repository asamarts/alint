---
title: 'file_min_lines'
description: 'alint rule kind `file_min_lines` (Content family).'
sidebar:
  order: 9
---

File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more — `wc -l` semantics). Use for "README has more than a title and a TODO".

```yaml
- id: readme-non-stub
  kind: min_lines
  paths: ["README.md", "README"]
  min_lines: 5
  level: info
```

