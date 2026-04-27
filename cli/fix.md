---
title: 'alint fix'
description: '`alint fix` — captured from `alint fix --help`.'
---

```
Apply automatic fixes for violations whose rules declare one

Usage: alint fix [OPTIONS] [PATH]

Arguments:
  [PATH]  Root of the repository to operate on [default: .]

Options:
  -c, --config <CONFIG>  Path to a config file (repeatable; later overrides earlier)
      --dry-run          Print what would be done without writing anything
      --changed          Restrict the fix pass to files in the working-tree diff (see `alint check --changed`). Cross-file + existence rules still see the full tree
  -f, --format <FORMAT>  Output format [default: human]
      --base <REF>       Base ref for `--changed`. Implies `--changed`
      --no-gitignore     Disable .gitignore handling (overrides config)
      --fail-on-warning  Treat warnings as errors for exit-code purposes
      --color <WHEN>     When to emit ANSI color codes in human output. `auto` (the default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`. Only affects the `human` format; `json` / `sarif` / `github` / `markdown` / `junit` / `gitlab` are always plain bytes [default: auto] [possible values: auto, always, never]
      --ascii            Force ASCII glyphs in human output (e.g. `x` instead of `✗`). Auto-enabled when `TERM=dumb`
      --compact          Compact one-line-per-violation human output, suitable for piping into editors / grep / `wc -l`. Format: `path:line:col: level: rule-id: message`
  -h, --help             Print help
```
