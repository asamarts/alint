---
title: 'max_directory_depth'
description: 'Tree depth from repo root may not exceed max. alint max_directory_depth rule, structure family.'
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

