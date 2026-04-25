---
title: 'Cross-file'
description: 'Rule reference: the cross-file family.'
sidebar:
  order: 13
---

### `pair`

For every file matching `primary`, a file matching the `partner` template must exist.

```yaml
- id: every-impl-has-test
  kind: pair
  primary: "src/**/*.rs"
  partner: "tests/{stem}.test.rs"
  level: warning
```

### `for_each_dir` / `for_each_file`

For every matching directory / file, evaluate a nested `require:` block with the entry as context. Template tokens (`{dir}`, `{stem}`, `{ext}`, `{basename}`, `{path}`, `{parent_name}`) expand against each match.

```yaml
- id: every-pkg-has-readme
  kind: for_each_dir
  paths: "packages/*"
  require:
    - kind: file_exists
      paths: "{path}/README.md"
```

### `dir_contains`

Every directory matching `paths` must contain files matching `require:`. Sugar for a common `for_each_dir` shape.

### `dir_only_contains`

Every directory matching `paths` may contain only files matching `allow:`. Catches stray test data in `src/`.

### `unique_by`

No two files matching `paths` may share the value of `key` (a path template). Catches basename collisions across subdirectories.

```yaml
- id: unique-basenames
  kind: unique_by
  paths: "src/**/*.rs"
  key: "{stem}"
  level: warning
```

### `every_matching_has`

For every file matching `paths`, at least one of `require:` must also exist (at a template-derived location). Lightweight sibling of `pair`.

---

