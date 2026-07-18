---
title: CLI
description: alint's subcommands and global flags, captured from the binary itself.
sidebar:
  order: 1
---

Language-agnostic linter for repository structure, existence, naming, and content rules

```
Usage: alint [OPTIONS] [COMMAND]
```

## Commands

| Command | Description |
| --- | --- |
| [`check`](/docs/cli/check/) | Run linters against the current (or given) directory. Default command |
| [`list`](/docs/cli/list/) | List the rules loaded from THIS repo's effective config. To browse the full catalog of rule kinds alint ships, use `alint rules list` |
| [`explain`](/docs/cli/explain/) | Show a rule's definition |
| [`fix`](/docs/cli/fix/) | Apply automatic fixes for violations whose rules declare one |
| [`baseline`](/docs/cli/baseline/) | Snapshot the current violations into a baseline file, so a later `alint check --baseline <file>` fails only on NEW violations. The one-step way to adopt alint as a blocking gate on a legacy repo: `alint baseline` (commit it), then gate on the delta. The baseline is whole-tree; `--changed` is not accepted |
| [`facts`](/docs/cli/facts/) | Evaluate every `facts:` entry in the effective config and print the resolved value. Debugging aid for `when:` clauses |
| [`init`](/docs/cli/init/) | Scaffold a starter `.alint.yml` based on the repo's detected ecosystem (and optionally workspace shape). Refuses to overwrite an existing config — delete the existing one first if you really mean it |
| [`export-agents-md`](/docs/cli/export-agents-md/) | Generate (or maintain a section of) `AGENTS.md` from the active rule set, so the agent's pre-prompt directives stay in sync with the lint config. Outputs to stdout by default; use `--output PATH` to write a file or `--inline --output PATH` to splice between `<!-- alint:start -->` / `<!-- alint:end -->` markers |
| [`suggest`](/docs/cli/suggest/) | Scan the repo for known antipatterns and propose rules that would catch them. Prints proposals to stdout for review — never edits the user's config. Pairs naturally with `alint init` for a smarter cold-start adoption flow |
| [`validate-config`](/docs/cli/validate-config/) | Parse-validate an `.alint.yml` (resolves `extends:`, builds every rule, parses every `when:`) and report any errors — without walking the tree. For editor LSP, pre-commit hooks, and fail-fast CI steps that just want to know "is the config loadable?". Exit 0 on success; exit 1 on validation failure |
| [`lsp`](/docs/cli/lsp/) | Start the alint language server, speaking LSP over stdio. Editor integrations (VS Code, Zed, Neovim, and others) spawn this and drive it via the Language Server Protocol; it is not meant to be run interactively. Publishes diagnostics for the workspace's `.alint.yml` rules on document open and save |
| [`rules`](/docs/cli/rules/) | Browse the catalog of rule kinds alint ships (config-independent). Use `alint list` for the rules configured in THIS repo; `alint rules` never reads a config and works anywhere |
| `help` | Print this message or the help of the given subcommand(s) |

## Global options

These apply to every subcommand.

| Flag | Description |
| --- | --- |
| `-c, --config <CONFIG>` | Path to a config file |
| `-f, --format <FORMAT>` | Output format [default: human] |
| `--no-gitignore` | Disable .gitignore handling (overrides config) |
| `--fail-on-warning` | Treat warnings as errors for exit-code purposes |
| `--show-notes` | List informational notes (non-violation findings, e.g. entries a rule skipped rather than failed on) in full on stderr. By default only a one-line count is shown |
| `--color <WHEN>` | When to emit ANSI color codes in human output. `auto` (the default) inspects TTY + `NO_COLOR` + `CLICOLOR_FORCE`. Only affects the `human` format; `json` / `sarif` / `github` / `markdown` / `junit` / `gitlab` / `agent` are always plain bytes [default: auto] [possible values: auto, always, never] |
| `--ascii` | Force ASCII glyphs in human output (e.g. `x` instead of `✗`). Auto-enabled when `TERM=dumb` |
| `--compact` | Compact one-line-per-violation human output, suitable for piping into editors / grep / `wc -l`. Format: `path:line:col: level: rule-id: message` (the `:line:col` is omitted for findings with no specific location) |
| `--width <COLS>` | Override the human-output column width. Default: detected terminal width (TTY only) or 80. Useful for reproducible captures (asciinema/screen recordings) and for piping into fixed-width log viewers. Clamped to [40, 120] |
| `--no-docs` | Suppress per-violation `docs:` URLs in human output. Useful for narrow terminals, screen recordings, and CI logs where long URLs disrupt visual alignment. URLs remain in JSON / SARIF / GitHub / markdown output regardless |
| `--progress <WHEN>` | When to render progress on stderr for slow operations (currently `alint suggest`). `auto` (the default) renders when stderr is a TTY; `always` forces; `never` silences. Progress always lives on stderr — `--format` JSON output on stdout stays byte-clean [default: auto] [possible values: auto, always, never] |
| `-q, --quiet` | Suppress progress and any stderr summary lines. Alias for `--progress=never` plus suppression of the "found N proposals in Ts" footer that `suggest` prints |
| `--baseline <FILE>` | Suppress violations recorded in the given baseline file (see `alint baseline`), reporting only new ones. Pre-existing findings are grandfathered so `check` can gate a legacy repo on new violations only. A missing or unreadable baseline is an error (never a silent no-op). The path is resolved relative to the current directory (not the checked PATH); the `baseline:` config key, by contrast, resolves against the repo root |
| `--strict-baseline` | With `--baseline`, fail (exit 1) when the baseline has stale entries (recorded findings that no longer fire — usually because they were fixed). Forces the committed baseline to stay exactly accurate. Off by default: fixing things never fails the build |
| `--show-baselined` | With `--baseline`, list the suppressed (baselined) findings on stderr in full, rather than just a one-line count. Parallels `--show-notes` |
| `--only <RULE_ID>` | Restrict the run to the named rule id(s) from the effective config (repeatable). Other rules are skipped entirely. An id that matches no loaded rule is an error, so typos fail loudly rather than silently linting nothing. Applies to `check` and `fix` (the `agent` format emits `fix --only <rule-id>`); rejected on any other subcommand. Global so the bare `alint --only <id>` lints the current directory like `alint check --only <id>` — to lint a different path, use the explicit form `alint check --only <id> <path>` |
| `-h, --help` | Print help |
| `-V, --version` | Print version |

<sub>Generated from `alint --help`.</sub>
