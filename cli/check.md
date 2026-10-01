---
title: 'alint check'
description: 'alint check lints a repository against its .alint.yml: the whole tree or only changed files, with exit codes and output formats built for CI.'
---

`alint check` is the default command: it walks the repository once, evaluates
every rule in the effective config (after `extends:`), and exits non-zero when
an error-level rule fails. Run it bare locally, with `--changed` as a fast
pre-commit or PR gate, and with a machine format in CI.

The pipeline `alint check` runs:

<likec4-view view-id="checkFlow"></likec4-view>

## Examples

Lint the repository in the current directory:

```bash
alint check
```

A fast PR gate, where per-file rules check only the files changed on this branch:

```bash
alint check --changed --base origin/main
```

SARIF for GitHub code scanning:

```bash
alint check --format sarif > alint.sarif
```

Fail only on violations that aren't in the committed baseline:

```bash
alint check --baseline .alint-baseline.json
```

## Reference

```
Run linters against the current (or given) directory. Default command

Usage: alint check [OPTIONS] [PATH]

Arguments:
  [PATH]
          Root of the repository to lint. Defaults to the current directory

          [default: .]

Options:
      --changed
          Restrict the check to files in the working-tree diff. Without `--base`, uses `git ls-files
          --modified --others --exclude-standard` (right shape for pre-commit). With `--base`, uses
          `git diff --name-only <base>...HEAD` (right shape for PR checks). Cross-file rules
          (`pair`, `for_each_dir`, `every_matching_has`, `unique_by`, `dir_contains`,
          `dir_only_contains`) and existence rules (`file_exists` et al.) still consult the full
          tree by definition

      --base <REF>
          Base ref for `--changed` (uses three-dot `<base>...HEAD`, i.e. merge-base diff). Implies
          `--changed`
```

The [global options](/docs/cli/#global-options) apply to `alint check` too, where they are relevant.

## See also

- [Changed mode](/docs/concepts/targeting/changed-mode/): what `--changed` narrows and what it doesn't
- [Severity and exit codes](/docs/concepts/start-here/severity-and-exit-codes/)
- [Output formats](/docs/reference/output-formats/)
- [GitHub Actions](/docs/integrations/github-actions/) and [pre-commit](/docs/integrations/pre-commit/)
