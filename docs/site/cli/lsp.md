---
title: 'alint lsp'
description: 'alint lsp runs the alint language server over stdio, so VS Code, Zed, Neovim and other LSP clients show alint findings as you open, edit and save files.'
---

You don't run `alint lsp` yourself: your editor starts it and talks to it over
the Language Server Protocol on stdio. Point your editor's LSP client at the
command `alint lsp`, and findings from the workspace's `.alint.yml` appear as
diagnostics as you open, edit and save files.

Each open file is linted by the nearest `.alint.yml` above it, across every
workspace folder. When a config between that one and the workspace folder's
own config sets `nested_configs: true`, the outermost such config governs the
file instead (its rules plus the nested config's), matching `alint check` run
from the workspace root. Pass `--show-notes` (for example through your editor's
`alint.args` setting) to list informational notes on the server's stderr
after each check instead of a one-line count. Following the LSP
specification, the server exits with code 0 after a `shutdown` request and
`exit` notification, and with code 1 when `exit` arrives without `shutdown`.

## See also

- [Editors](/docs/integrations/editors/): setup for VS Code, Zed, Neovim and others
