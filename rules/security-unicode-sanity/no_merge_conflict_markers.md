---
title: 'no_merge_conflict_markers'
description: 'Flag <<<<<<<, =======, >>>>>>>, ||||||| markers at the start of a line, almost always left over from an unresolved merge.'
sidebar:
  order: 1
categories: ['security-unicode-sanity', 'text-hygiene']
---

Flag `<<<<<<< `, `=======`, `>>>>>>> `, `||||||| ` markers at the start of a line — almost always left over from an unresolved merge. The anchor markers carry a trailing ref (`<<<<<<< HEAD`), so they never collide with prose; a bare `=======` is reported only when the file also contains one of those anchors, because on its own a seven-character `=======` is indistinguishable from a reST/Markdown setext heading underline (so docs trees no longer need to be excluded).

```yaml
- id: no-conflicts
  kind: no_merge_conflict_markers
  paths: "**"
  level: error
```

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
