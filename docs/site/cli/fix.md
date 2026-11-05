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
preview and `false` for a real run. Reports from older alint releases omit the
field; treat that as `false`.

A dry run is a single pass over the current tree. A real `fix` writes, re-walks
and re-checks until nothing changes, and a fix that ran but left its violation
standing is reported `skipped` (`skip_kind: "unresolved"` in JSON) with a
nonzero exit. The dry run predicts the common case of that: a `file_create`,
`dir_create` or `create_and_register` member that the walk would index under
no name, neither the path as written nor where the file lands once symlinks
resolve (gitignored, including by an ignore file another fix in the same run
creates, matched by `ignore:`, or below a symlink the walk does not follow), a
target outside the rule's `paths:` or landing outside the root, or a created
file a `git_tracked_only` rule cannot see because git does not track it, is
reported the same way, not as applied. A
`file_exists` rule with `respect_gitignore: false` looks for its literal path
on disk itself, so a create the walk leaves out still resolves it, in the dry
run as in the real one. Anything else only
the real run's re-check can reveal, so a dry-run `applied` item means "the
first pass would apply this", not "this resolves the violation": a fix that
only makes progress on a later pass, a cascade where one rule's fix creates or
resolves another's violation, an edit to an existing ignore file, or content a
fix writes that the rule still rejects. `--diff` shows that first pass's edits
without the prediction, so its exit code can be `0` where a real `fix` exits
`1`. It also leaves out every file a rule with redacted findings would change
([`sensitive: true`](/docs/configuration/#sensitive-all-rules-v019), or a field
read from the environment), so the diff cannot show a secret beside an edit;
`alint fix` still applies those fixes.

If alint cannot obtain a policy verdict, the fix report carries a typed
`analysis_failure` instead of a fix item. It is never passed to a fixer and
forces exit `1` under normal fix, `--dry-run`, `--diff`, and `--fix-only`.
Non-convergence retains exit `2`.

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

## Reference date for time-dependent rules

`alint fix --as-of YYYY-MM-DD` pins one strict UTC reference date for every
pass, including the final re-check. The flag overrides `ALINT_AS_OF`; without
either, alint uses the current HEAD commit's committer timestamp. An explicit
date removes that HEAD-time lookup, but `git_blame_age` still requires a usable
Git repository for tracked-file and blame data.

## See also

- [Fixing](/docs/concepts/adoption/fixing/): how fixes are chosen and applied
- [Fix operations](/docs/concepts/fix-operations/): the fix ops and their options
