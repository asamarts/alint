---
title: Editors
description: 'In-editor diagnostics, hover-to-explain, and apply-fix code actions via the alint LSP server.'
sidebar:
  order: 4
---

alint ships an LSP server (`alint lsp`, in the `alint-lsp` crate) plus
extensions, configs, and snippets across nine editors. The server
streams `.alint.yml` diagnostics into your editor as you type, surfaces
rule descriptions on hover, and exposes apply-fix code actions for
auto-fixable rules.

All editor surfaces are versioned in lockstep with the `alint` binary;
the latest release across every channel is **0.11.0**.

## Tier 1 — packaged extension, marketplace install

| Editor | Install | Notes |
|---|---|---|
| **VS Code** | [Marketplace](https://marketplace.visualstudio.com/items?itemName=asamarts.alint) / [Open VSX](https://open-vsx.org/extension/asamarts/alint) | The extension auto-downloads a matching `alint` binary on first run if one isn't on `PATH`. |
| **JetBrains** (IDEA, PyCharm, GoLand, WebStorm, RustRover, CLion, Rider, Android Studio) | [JetBrains Marketplace](https://plugins.jetbrains.com/plugin/31995-alint) | Built on [LSP4IJ](https://github.com/redhat-developer/lsp4ij); one plugin covers the whole JetBrains suite. |
| **Zed** | `zed-industries/extensions` registry (search for `alint`) | The extension is a thin wasm wrapper around the LSP server. |

These three are the **packaged-extension** tier: install through the
editor's normal marketplace UI, no extra configuration needed.

## Tier 2 — config snippet, generic LSP client

| Editor | What ships in the alint repo |
|---|---|
| **Neovim** (0.11+) | [`editors/nvim/lsp/alint.lua`](https://github.com/asamarts/alint/tree/main/editors/nvim) — drop into `~/.config/nvim/lsp/` and `vim.lsp.enable("alint")`. |
| **Sublime Text** | [`editors/sublime/LSP-alint.sublime-settings`](https://github.com/asamarts/alint/tree/main/editors/sublime) — add to the LSP package's settings. |
| **Emacs** | [`editors/emacs/alint.el`](https://github.com/asamarts/alint/tree/main/editors/emacs) — uses `lsp-mode` or `eglot`. |
| **Helix** | [`editors/helix/languages.toml`](https://github.com/asamarts/alint/tree/main/editors/helix) — merge into your `~/.config/helix/languages.toml`. |
| **Eclipse** | [`editors/eclipse/`](https://github.com/asamarts/alint/tree/main/editors/eclipse) — install LSP4E first, then configure. |

These rely on a generic LSP client the editor already has; the alint
repo ships a tested config snippet you copy in.

## What the server provides

- **Diagnostics on save and on change.** Every `.alint.yml` violation in
  your active file lands as an editor diagnostic with the rule ID and
  the canonical message.
- **Hover-to-explain.** Hovering a diagnostic shows the rule's
  description, fix availability, and a link to its rule-reference page.
- **Apply-fix code actions.** For rules that ship an auto-fix
  (`final_newline`, `no_trailing_whitespace`, `line_endings`, etc.), the
  editor offers a code action that runs the fix in-place.
- **Informational notes.** Skipped non-literal entries (e.g. a registry
  path containing `${MODULE}`) surface as one-line notes in the
  server's stderr; pass `--show-notes` to expand them.
- **Auto-discovery.** The server walks up from the open file to find
  the nearest `.alint.yml`, so it works in monorepos and nested
  package roots without configuration.

## Configuration

The server reads everything from `.alint.yml` the same way the CLI
does — no editor-specific configuration is required for the rule set
itself. The only editor-side settings are:

- **`alint.path`** — absolute path to the `alint` binary, if not on
  `PATH`. Auto-resolved by the VS Code + JetBrains extensions on first
  run.
- **`alint.args`** — extra args to pass to `alint lsp` on launch
  (unused in normal operation; useful for `--show-notes`).

## Authoring rules with editor support

Editor diagnostics are read-only previews of `alint check` output, so
the workflow is "edit `.alint.yml`, see diagnostics live, save". For
the full rule-authoring workflow (designing a new rule kind, writing
fixtures, the `xtask` audits) see
[Rule authoring](/docs/development/rule-authoring/).
