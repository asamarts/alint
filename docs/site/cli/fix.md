---
title: 'alint fix'
description: 'alint fix applies the automatic fixes that rules declare, like appending a final newline or renaming a file into the right case. Preview with a dry run.'
---

`alint fix` runs the same checks as `alint check`, then applies the fix each
failing rule declares: create or remove a file, prepend or append content,
trim trailing whitespace, normalise line endings, rename a file into the
configured case, and so on. Rules without a fixer are reported and left alone.

## Examples

Show what would change, without writing anything:

```bash
alint fix --dry-run
```

Apply every available fix:

```bash
alint fix
```

Fix only the files changed on this branch:

```bash
alint fix --changed --base origin/main
```

## See also

- [Fixing](/docs/concepts/adoption/fixing/): how fixes are chosen and applied
- [Fix operations](/docs/concepts/fix-operations/): every fix op and its options
