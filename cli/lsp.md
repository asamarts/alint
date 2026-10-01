---
title: 'alint lsp'
description: 'alint lsp runs the alint language server over stdio, so VS Code, Zed, Neovim and other LSP clients show alint findings as you open, edit and save files.'
---

You don't run `alint lsp` yourself: your editor starts it and talks to it over
the Language Server Protocol on stdio. Point your editor's LSP client at the
command `alint lsp`, and findings from the workspace's `.alint.yml` appear as
diagnostics as you open, edit and save files.

How `alint lsp` serves an editor over LSP:

<likec4-view view-id="lspFlow"></likec4-view>

## Reference

```
Start the alint language server (LSP over stdio).

Editor integrations (VS Code, Zed, Neovim, and others) spawn this and drive it via the Language
Server Protocol; it is not meant to be run interactively. Publishes diagnostics for the workspace's
`.alint.yml` rules on document open and save.

Usage: alint lsp [OPTIONS]
```

The [global options](/docs/cli/#global-options) apply to `alint lsp` too, where they are relevant.

## See also

- [Editors](/docs/integrations/editors/): setup for VS Code, Zed, Neovim and others
