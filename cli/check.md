---
title: 'alint check'
description: '`alint check` — captured from `alint check --help`.'
---

```
Run linters against the current (or given) directory. Default command

Usage: alint check [OPTIONS] [PATH]

Arguments:
  [PATH]  Root of the repository to lint. Defaults to the current directory [default: .]

Options:
  -c, --config <CONFIG>  Path to a config file (repeatable; later overrides earlier)
      --changed          Restrict the check to files in the working-tree diff. Without `--base`, uses `git ls-files --modified --others --exclude-standard` (right shape for pre-commit). With `--base`, uses `git diff --name-only <base>...HEAD` (right shape for PR checks). Cross-file rules (`pair`, `for_each_dir`, `every_matching_has`, `unique_by`, `dir_contains`, `dir_only_contains`) and existence rules (`file_exists` et al.) still consult the full tree by definition
      --base <REF>       Base ref for `--changed` (uses three-dot `<base>...HEAD`, i.e. merge-base diff). Implies `--changed`
  -f, --format <FORMAT>  Output format [default: human]
      --no-gitignore     Disable .gitignore handling (overrides config)
      --fail-on-warning  Treat warnings as errors for exit-code purposes
      --color <WHEN>     When to emit ANSI color codes in human output. `auto` (the default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`. Only affects the `human` format; `json` / `sarif` / `github` are always plain bytes [default: auto] [possible values: auto, always, never]
      --ascii            Force ASCII glyphs in human output (e.g. `x` instead of `✗`). Auto-enabled when `TERM=dumb`
      --compact          Compact one-line-per-violation human output, suitable for piping into editors / grep / `wc -l`. Format: `path:line:col: level: rule-id: message`
  -h, --help             Print help
```
