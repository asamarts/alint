---
title: 'alint explain'
description: '`alint explain` — captured from `alint explain --help`.'
---

```
Show a rule's definition

Usage: alint explain [OPTIONS] <RULE_ID>

Arguments:
  <RULE_ID>  Rule id to describe

Options:
  -c, --config <CONFIG>  Path to a config file (repeatable; later overrides earlier)
  -f, --format <FORMAT>  Output format [default: human]
      --no-gitignore     Disable .gitignore handling (overrides config)
      --fail-on-warning  Treat warnings as errors for exit-code purposes
      --color <WHEN>     When to emit ANSI color codes in human output. `auto` (the default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`. Only affects the `human` format; `json` / `sarif` / `github` / `markdown` / `junit` / `gitlab` are always plain bytes [default: auto] [possible values: auto, always, never]
      --ascii            Force ASCII glyphs in human output (e.g. `x` instead of `✗`). Auto-enabled when `TERM=dumb`
      --compact          Compact one-line-per-violation human output, suitable for piping into editors / grep / `wc -l`. Format: `path:line:col: level: rule-id: message`
  -h, --help             Print help
```
