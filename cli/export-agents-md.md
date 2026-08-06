---
title: 'alint export-agents-md'
description: 'Generate an AGENTS.md section from the active rule set. alint export-agents-md CLI reference and flags.'
---

```
Generate an `AGENTS.md` section from the active rule set.

Keeps the agent's pre-prompt directives in sync with the lint config. Outputs to stdout by default;
use `--output PATH` to write a file, or `--inline --output PATH` to splice between `<!-- alint:start
-->` / `<!-- alint:end -->` markers.

Usage: alint export-agents-md [OPTIONS]

Options:
  -c, --config <CONFIG>
          Path to a config file

      --output <PATH>
          Output destination. Without `--inline`, the file is overwritten. Omit for stdout

      --inline
          Splice the generated section between `<!-- alint:start -->` and `<!-- alint:end -->`
          markers in `--output PATH`. Markers are auto-created (with a stderr warning) when the
          target file lacks them

      --no-gitignore
          Disable .gitignore handling (overrides config)

      --section-title <TEXT>
          Heading text for the generated section. Default: "Lint rules enforced by alint"

      --fail-on-warning
          Treat warnings as errors for exit-code purposes

      --include-info
          Include `level: info` rules. Default omits them — info-level rules are nudges, not
          directives

  -f, --format <FORMAT>
          Output format. `markdown` (default) is the canonical `AGENTS.md` shape; `json` is parallel
          to `suggest`'s JSON envelope for agent consumption
          
          [default: markdown]
          [possible values: markdown, json]

      --show-notes
          List informational notes in full on stderr.
          
          Notes are non-violation findings — e.g. entries a rule skipped rather than failed on. By
          default only a one-line count is shown.

      --color <WHEN>
          When to emit ANSI color codes in human output.
          
          `auto` (the default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`. Only affects the
          `human` format; `json` / `sarif` / `github` / `markdown` / `junit` / `gitlab` / `agent`
          are always plain bytes.
          
          [default: auto]
          [possible values: auto, always, never]

      --ascii
          Force ASCII glyphs in human output.
          
          E.g. `x` instead of `✗`. Auto-enabled when `TERM=dumb`.

      --compact
          Compact one-line-per-violation human output.
          
          Suitable for piping into editors / grep / `wc -l`. Format: `path:line:col: level: rule-id:
          message` (the `:line:col` is omitted for findings with no specific location).

      --width <COLS>
          Override the human-output column width.
          
          Default: detected terminal width (TTY only) or 80. Useful for reproducible captures
          (asciinema/screen recordings) and for piping into fixed-width log viewers. Clamped to [40,
          120].

      --no-docs
          Suppress per-violation `docs:` URLs in human output.
          
          Useful for narrow terminals, screen recordings, and CI logs where long URLs disrupt visual
          alignment. URLs remain in JSON / SARIF / GitHub / markdown output regardless.

      --progress <WHEN>
          When to render progress on stderr for slow operations.
          
          Currently just `alint suggest`. `auto` (the default) renders when stderr is a TTY;
          `always` forces; `never` silences. Progress always lives on stderr — `--format` JSON
          output on stdout stays byte-clean.
          
          [default: auto]
          [possible values: auto, always, never]

  -q, --quiet
          Suppress progress and stderr summary lines.
          
          Alias for `--progress=never` plus suppression of the "found N proposals in Ts" footer that
          `suggest` prints.

      --baseline <FILE>
          Suppress violations recorded in a baseline file.
          
          See `alint baseline` to create one; only new violations are reported. Pre-existing
          findings are grandfathered so `check` can gate a legacy repo on new violations only. A
          missing or unreadable baseline is an error (never a silent no-op). The path is resolved
          relative to the current directory (not the checked PATH); the `baseline:` config key, by
          contrast, resolves against the repo root.

      --strict-baseline
          With `--baseline`, fail on stale baseline entries.
          
          Exits 1 when the baseline has stale entries: recorded findings that no longer fire,
          usually because they were fixed. Forces the committed baseline to stay exactly accurate.
          Off by default: fixing things never fails the build.

      --show-baselined
          With `--baseline`, list suppressed findings on stderr.
          
          Lists them in full, rather than just a one-line count. Parallels `--show-notes`.

      --only <RULE_ID>
          Restrict the run to the named rule id(s).
          
          Taken from the effective config (repeatable). Other rules are skipped entirely. An id that
          matches no loaded rule is an error, so typos fail loudly rather than silently linting
          nothing. Applies to `check` and `fix` (the `agent` format emits `fix --only <rule-id>`);
          rejected on any other subcommand. Global, so the bare `alint --only <id>` lints the
          current directory like `alint check --only <id>` — to lint a different path, use the
          explicit form `alint check --only <id> <path>`.

  -h, --help
          Print help (see a summary with '-h')
```
