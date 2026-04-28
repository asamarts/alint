---
title: 'alint facts'
description: '`alint facts` — captured from `alint facts --help`.'
---

```
Evaluate every `facts:` entry in the effective config and print the resolved value. Debugging aid for `when:` clauses

Usage: alint facts [OPTIONS] [PATH]

Arguments:
  [PATH]  Root of the repository to evaluate facts against [default: .]

Options:
  -c, --config <CONFIG>  Path to a config file (repeatable; later overrides earlier)
  -f, --format <FORMAT>  Output format [default: human]
      --no-gitignore     Disable .gitignore handling (overrides config)
      --fail-on-warning  Treat warnings as errors for exit-code purposes
      --color <WHEN>     When to emit ANSI color codes in human output. `auto` (the default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`. Only affects the `human` format; `json` / `sarif` / `github` / `markdown` / `junit` / `gitlab` / `agent` are always plain bytes [default: auto] [possible values: auto, always, never]
      --ascii            Force ASCII glyphs in human output (e.g. `x` instead of `✗`). Auto-enabled when `TERM=dumb`
      --compact          Compact one-line-per-violation human output, suitable for piping into editors / grep / `wc -l`. Format: `path:line:col: level: rule-id: message`
  -h, --help             Print help
```
