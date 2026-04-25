---
title: 'file_content_matches'
description: 'alint rule kind `file_content_matches` (Content family).'
sidebar:
  order: 1
---

File contents must contain at least one match for a regex.

```yaml
- id: crate-is-2024-edition
  kind: content_matches
  paths: "Cargo.toml"
  pattern: 'edition\s*=\s*"2024"'
  level: error
```

Fix: `file_append` — append declared content.

