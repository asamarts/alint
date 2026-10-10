---
title: 'alint fix'
description: 'alint fix applies the automatic fixes that rules declare, like appending a final newline or renaming a file into the right case. Preview with a dry run.'
---

`alint fix` runs the same checks as `alint check`, then applies the fix each
failing rule declares: create or remove a file, prepend or append content,
trim trailing whitespace, normalize line endings, rename a file into the
configured case, and so on. Rules without a fixer are reported and left alone.

## Examples

Show what would change, without writing anything:

```bash
alint fix --dry-run
```

With `--format json`, the report's top-level `dry_run` field is `true` for a
preview (each `applied` item is a fix that would be applied) and `false` for a
real run. Reports from older alint releases omit the field; treat that as
`false`.

Apply every available fix:

```bash
alint fix
```

Limit the pass to the files changed on this branch. Existence and cross-file
rules still see the whole tree, but a fix aimed outside the changed set is
reported as a suggestion instead of being applied:

```bash
alint fix --changed --base origin/main
```

## See also

- [Fixing](/docs/concepts/adoption/fixing/): how fixes are chosen and applied
- [Fix operations](/docs/concepts/fix-operations/): the fix ops and their options
