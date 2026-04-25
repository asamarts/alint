---
title: 'max_directory_depth'
description: 'alint rule kind `max_directory_depth` (Structure family).'
sidebar:
  order: 1
---

Tree depth from repo root may not exceed `max`. A shallow depth stops deeply-nested imports and keeps CI path globs sane.

```yaml
- id: shallow-tree
  kind: max_directory_depth
  paths: "**"
  max: 6
  level: warning
```

