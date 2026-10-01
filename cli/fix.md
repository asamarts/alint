---
title: 'alint fix'
description: 'alint fix applies the automatic fixes that rules declare, like appending a final newline or renaming a file into the right case. Preview with a dry run.'
---

`alint fix` runs the same checks as `alint check`, then applies the fix each
failing rule declares: create or remove a file, prepend or append content,
trim trailing whitespace, normalize line endings, rename a file into the
configured case, and so on. Rules without a fixer are reported and left alone.

How `alint fix` applies fixes and re-checks:

<likec4-view view-id="fixFlow"></likec4-view>

## Examples

Show what would change, without writing anything:

```bash
alint fix --dry-run
```

Apply every available fix:

```bash
alint fix
```

Limit the pass to the files changed on this branch. Existence and cross-file
rules still see the whole tree, so their fixes can reach other files:

```bash
alint fix --changed --base origin/main
```

## Reference

```
Apply automatic fixes for violations whose rules declare one

Usage: alint fix [OPTIONS] [PATH]

Arguments:
  [PATH]
          Root of the repository to operate on

          [default: .]

Options:
      --dry-run
          Print what would be done without writing anything

      --changed
          Restrict the fix pass to files in the working-tree diff (see `alint check --changed`).
          Cross-file + existence rules still see the full tree

      --base <REF>
          Base ref for `--changed`. Implies `--changed`

      --unsafe-fixes
          Also apply Unsafe-tier fixes, not just Safe ones. Unsafe fixes may be destructive or
          change behavior, so they are opt-in: without this flag they are surfaced as suggestions
          instead of applied. `file_remove` (the fix for `file_absent` / `no_empty_files` /
          `no_submodules` / `no_symlinks`) is Unsafe, so this flag is what applies it

      --fix-only
          Report only the fixes that were applied: suppress the residual (skipped / unfixable)
          findings and exit 0 unless a fix errored. For "apply what you can and move on" workflows

      --diff
          Show a unified diff of the fixes that would be applied, writing nothing. Reflects the
          composed result at the chosen tier; the exit code matches a real `fix`. Output is a
          unified diff regardless of `--format`
```

The [global options](/docs/cli/#global-options) apply to `alint fix` too, where they are relevant.

## See also

- [Fixing](/docs/concepts/adoption/fixing/): how fixes are chosen and applied
- [Fix operations](/docs/concepts/fix-operations/): the fix ops and their options
