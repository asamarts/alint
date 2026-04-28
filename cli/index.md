---
title: CLI
description: alint's subcommands and global flags, captured from the binary itself.
sidebar:
  order: 1
---

```
Language-agnostic linter for repository structure, existence, naming, and content rules

Usage: alint [OPTIONS] [COMMAND]

Commands:
  check    Run linters against the current (or given) directory. Default command
  list     List all rules loaded from the effective config
  explain  Show a rule's definition
  fix      Apply automatic fixes for violations whose rules declare one
  facts    Evaluate every `facts:` entry in the effective config and print the resolved value. Debugging aid for `when:` clauses
  init     Scaffold a starter `.alint.yml` based on the repo's detected ecosystem (and optionally workspace shape). Refuses to overwrite an existing config — delete the existing one first if you really mean it
  suggest  Scan the repo for known antipatterns and propose rules that would catch them. Prints proposals to stdout for review — never edits the user's config. Pairs naturally with `alint init` for a smarter cold-start adoption flow
  help     Print this message or the help of the given subcommand(s)

Options:
  -c, --config <CONFIG>  Path to a config file (repeatable; later overrides earlier)
  -f, --format <FORMAT>  Output format [default: human]
      --no-gitignore     Disable .gitignore handling (overrides config)
      --fail-on-warning  Treat warnings as errors for exit-code purposes
      --color <WHEN>     When to emit ANSI color codes in human output. `auto` (the default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`. Only affects the `human` format; `json` / `sarif` / `github` / `markdown` / `junit` / `gitlab` / `agent` are always plain bytes [default: auto] [possible values: auto, always, never]
      --ascii            Force ASCII glyphs in human output (e.g. `x` instead of `✗`). Auto-enabled when `TERM=dumb`
      --compact          Compact one-line-per-violation human output, suitable for piping into editors / grep / `wc -l`. Format: `path:line:col: level: rule-id: message`
      --progress <WHEN>  When to render progress on stderr for slow operations (currently `alint suggest`). `auto` (the default) renders when stderr is a TTY; `always` forces; `never` silences. Progress always lives on stderr — `--format` JSON / YAML output on stdout stays byte-clean [default: auto] [possible values: auto, always, never]
  -q, --quiet            Suppress progress and any stderr summary lines. Alias for `--progress=never` plus suppression of the "found N proposals in Ts" footer that `suggest` prints
  -h, --help             Print help
  -V, --version          Print version
```
