---
title: 'for_each_dir'
description: 'alint rule kind `for_each_dir` (Cross-file family).'
sidebar:
  order: 2
---

For every matching directory / file, evaluate a nested `require:` block with the entry as context. Template tokens (`{dir}`, `{stem}`, `{ext}`, `{basename}`, `{path}`, `{parent_name}`) expand against each match.

```yaml
- id: every-pkg-has-readme
  kind: for_each_dir
  paths: "packages/*"
  require:
    - kind: file_exists
      paths: "{path}/README.md"
```

## See also

- [`for_each_file`](/docs/rules/cross-file/for_each_file/)
