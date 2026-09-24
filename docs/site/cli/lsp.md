---
title: 'alint lsp'
description: 'alint lsp runs the alint language server over stdio, so VS Code, Zed, Neovim and other LSP clients show alint findings as you open, edit and save files.'
---

You don't run `alint lsp` yourself: your editor starts it and talks to it over
the Language Server Protocol on stdio. Point your editor's LSP client at the
command `alint lsp`, and findings from the workspace's `.alint.yml` appear as
diagnostics as you open, edit and save files.

## See also

- [Editors](/docs/integrations/editors/): setup for VS Code, Zed, Neovim and others
