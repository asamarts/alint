# alint for VS Code

In-editor diagnostics, hover-to-explain, and quick-fixes for
[alint](https://github.com/asamarts/alint) — the language-agnostic
linter for repository structure, file existence, filename conventions,
and file content rules.

The extension is a thin client: it launches `alint lsp` (the alint
language server) and lets VS Code render the results. All linting logic
lives in alint itself.

## Features

- **Diagnostics** for your workspace's `.alint.yml` rules, on open and
  save, with live per-keystroke updates for per-file rules.
- **Hover** over a violation to see the rule id, severity, message, and
  a link to the rule's policy URL.
- **Quick fixes** ("Apply fix") for any violation whose rule declares a
  fixer — applied to the buffer as an editor edit you can undo.

## Requirements

VS Code 1.91 or newer (or a VS Code-based editor at that API level). The
extension needs the `alint` binary. It looks for it in this order:

1. the `alint.path` setting,
2. `alint` on your `PATH`,
3. a copy it previously downloaded,
4. otherwise it offers to download the matching release (opt-in) or to
   locate an existing binary.

You can also install alint yourself via Homebrew, `cargo install alint`,
`npm i -g @asamarts/alint`, Docker, or the install script — see the
[project README](https://github.com/asamarts/alint#install).

## Settings

| Setting | Default | Description |
|---|---|---|
| `alint.path` | `""` | Absolute path to the `alint` binary (overrides discovery). |
| `alint.serverArgs` | `[]` | Extra arguments appended after `alint lsp`. |
| `alint.trace.server` | `"off"` | Trace client/server messages in the 'alint' output channel. |

## Commands

- **alint: Restart language server**
- **alint: Show effective rules** (runs `alint list`)
- **alint: Open .alint.yml**

## Versioning

The extension version tracks the alint release it ships against
(`0.10.x` of the extension targets `alint 0.10.x`).
