---
title: 'no_merge_conflict_markers'
description: 'Flag <<<<<<<, =======, >>>>>>> markers at the start of a line, almost always left over from an unresolved merge.'
sidebar:
  order: 1
---

Flag `<<<<<<< `, `=======`, `>>>>>>> ` markers at the start of a line — almost always left over from an unresolved merge.

```yaml
- id: no-conflicts
  kind: no_merge_conflict_markers
  paths: "**"
  level: error
```

