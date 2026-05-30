---
title: 'alint validate-config'
description: 'Parse-validate an .alint.yml (resolves extends:, builds every rule, parses every when:) and report any errors, without.'
---

```
Parse-validate an `.alint.yml` (resolves `extends:`, builds every rule, parses every `when:`) and report any errors — without walking the tree. For editor LSP, pre-commit hooks, and fail-fast CI steps that just want to know "is the config loadable?". Exit 0 on success; exit 1 on validation failure

Usage: alint validate-config [OPTIONS] [PATH]

Arguments:
  [PATH]  Path to the config file to validate. Defaults to the `.alint.yml` discovered upward from the current directory (same discovery rules as `alint check`)

Options:
  -c, --config <CONFIG>  Path to a config file (repeatable; later overrides earlier)
  -f, --format <FORMAT>  Output format. `human` prints a one-line success or a rich error trace; `json` emits a stable `{"valid": bool, "rule_count": N, "config_path": ..., "error": "...?"}` envelope for editor / CI consumption [default: human] [possible values: human, json]
      --no-gitignore     Disable .gitignore handling (overrides config)
      --fail-on-warning  Treat warnings as errors for exit-code purposes
      --show-notes       List informational notes (non-violation findings, e.g. entries a rule skipped rather than failed on) in full on stderr. By default only a one-line count is shown
      --color <WHEN>     When to emit ANSI color codes in human output. `auto` (the default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`. Only affects the `human` format; `json` / `sarif` / `github` / `markdown` / `junit` / `gitlab` / `agent` are always plain bytes [default: auto] [possible values: auto, always, never]
      --ascii            Force ASCII glyphs in human output (e.g. `x` instead of `✗`). Auto-enabled when `TERM=dumb`
      --compact          Compact one-line-per-violation human output, suitable for piping into editors / grep / `wc -l`. Format: `path:line:col: level: rule-id: message`
      --width <COLS>     Override the human-output column width. Default: detected terminal width (TTY only) or 80. Useful for reproducible captures (asciinema/screen recordings) and for piping into fixed-width log viewers. Clamped to [40, 120]
      --no-docs          Suppress per-violation `docs:` URLs in human output. Useful for narrow terminals, screen recordings, and CI logs where long URLs disrupt visual alignment. URLs remain in JSON / SARIF / GitHub / markdown output regardless
      --progress <WHEN>  When to render progress on stderr for slow operations (currently `alint suggest`). `auto` (the default) renders when stderr is a TTY; `always` forces; `never` silences. Progress always lives on stderr — `--format` JSON / YAML output on stdout stays byte-clean [default: auto] [possible values: auto, always, never]
  -q, --quiet            Suppress progress and any stderr summary lines. Alias for `--progress=never` plus suppression of the "found N proposals in Ts" footer that `suggest` prints
  -h, --help             Print help
```
