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
  select: "packages/*"
  require:
    - kind: file_exists
      paths: "{path}/README.md"
```

**`when_iter:` — per-iteration filter.** Optional expression in the `when:` grammar, with one extra namespace: `iter.*` references the entry currently being iterated. Iterations whose verdict is false are skipped before any nested rule is built — the canonical use case for monorepos shaped like Cargo / pnpm / Bazel workspaces:

```yaml
- id: workspace-member-has-readme
  kind: for_each_dir
  select: "crates/*"
  when_iter: 'iter.has_file("Cargo.toml")'
  require:
    - kind: file_exists
      paths: "{path}/README.md"
  level: error
```

The `iter` namespace exposes:

| Reference | Type | Notes |
|---|---|---|
| `iter.path` | string | Relative path of the iterated entry. |
| `iter.basename` | string | Basename. |
| `iter.parent_name` | string | Parent dir name. |
| `iter.stem` | string | Basename minus the final extension (mainly useful for files). |
| `iter.ext` | string | Final extension without the dot. |
| `iter.is_dir` | bool | True for `for_each_dir`, false for `for_each_file`; always available. |
| `iter.has_file(pattern)` | bool | Glob match relative to the iterated dir. `iter.has_file("Cargo.toml")`, `iter.has_file("**/*.bzl")`. Always false for file iteration. |

`when_iter:` composes with the rule's outer `when:` (whole-rule gate, evaluated once) and with each nested rule's `when:` (which now also sees the same `iter.*` context). Same field is available on `for_each_file` and `every_matching_has`.

## See also

- [`for_each_file`](/docs/rules/cross-file/for_each_file/)
