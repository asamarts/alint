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

With `--format json`, the report's top-level `dry_run` field is `true` for a
preview and `false` for a real run. Reports from older alint releases omit the
field; treat that as `false`.

A dry run is a single pass over the current tree. A real `fix` writes, re-walks
and re-checks until nothing changes, and a fix that ran but left its violation
standing is reported `skipped` (`skip_kind: "unresolved"` in JSON) with a
nonzero exit. The dry run predicts the common case of that: a `file_create`
whose target is gitignored or matched by `ignore:` (the walk never indexes it)
or lies outside the rule's `paths:` is reported the same way, not as applied.
Anything else only the real run's re-check can reveal, so a dry-run `applied`
item means "the first pass would apply this", not "this resolves the
violation": a fix that only makes progress on a later pass, a cascade where
one rule's fix creates or resolves another's violation, or content a fix
writes that the rule still rejects.

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
