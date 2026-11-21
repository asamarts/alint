---
title: Editors
description: 'In-editor diagnostics, hover-to-explain, and apply-fix code actions via the alint LSP server.'
sidebar:
  order: 4
---

alint ships an LSP server (`alint lsp`, in the `alint-lsp` crate) plus
extensions, configs, and snippets across eight editors. The server
streams `.alint.yml` diagnostics into your editor as you type, surfaces
rule descriptions on hover, and exposes apply-fix code actions for
auto-fixable rules.

All editor surfaces are versioned in lockstep with the `alint` binary,
so every channel ships the same release.

<likec4-view view-id="editorArch"></likec4-view>

## Tier 1 — packaged extension, marketplace install

| Editor | Install | Notes |
|---|---|---|
| **VS Code** | [Marketplace](https://marketplace.visualstudio.com/items?itemName=asamarts.alint) / [Open VSX](https://open-vsx.org/extension/asamarts/alint) | The extension auto-downloads a matching `alint` binary on first run if one isn't on `PATH`. |
| **JetBrains** (IDEA, PyCharm, GoLand, WebStorm, RustRover, CLion, Rider, Android Studio) | [JetBrains Marketplace](https://plugins.jetbrains.com/plugin/31995-alint) | Built on [LSP4IJ](https://github.com/redhat-developer/lsp4ij); one plugin covers the whole JetBrains suite. |

These are the **packaged-extension** tier: install through the
editor's normal marketplace UI, no extra configuration needed.

## Zed — source extension

The repository includes a tested Zed wasm extension under
[`editors/zed`](https://github.com/asamarts/alint/tree/main/editors/zed). It is
not yet listed in Zed's public extension registry, so install it as a dev
extension from a local checkout: open the command palette, choose **zed:
install dev extension**, and select the `editors/zed` directory. The extension
launches `alint lsp`, preferring an explicitly configured binary, then `alint`
on `PATH`, and finally a managed download of the latest GitHub release.

## Tier 2 — config snippet, generic LSP client

| Editor | What ships in the alint repo |
|---|---|
| **Neovim** (0.11+) | [`editors/nvim/lsp/alint.lua`](https://github.com/asamarts/alint/tree/main/editors/nvim) — drop into `~/.config/nvim/lsp/` and `vim.lsp.enable("alint")`. |
| **Sublime Text** | [`editors/sublime/LSP.sublime-settings`](https://github.com/asamarts/alint/tree/main/editors/sublime) — add to the LSP package's settings. |
| **Emacs** | [`editors/emacs/alint.el`](https://github.com/asamarts/alint/tree/main/editors/emacs) — uses `lsp-mode` or `eglot`. |
| **Helix** | [`editors/helix/languages.toml`](https://github.com/asamarts/alint/tree/main/editors/helix) — merge into your `~/.config/helix/languages.toml`. |
| **Eclipse** | [`editors/eclipse/`](https://github.com/asamarts/alint/tree/main/editors/eclipse) — install LSP4E first, then configure. |

These rely on a generic LSP client the editor already has; the alint
repo ships a tested config snippet you copy in.

## What the server provides

How an open, change, or save in your editor drives a check and publishes
diagnostics:

<likec4-view view-id="lspFlow"></likec4-view>

- **Diagnostics on save and on change.** Every `.alint.yml` violation in
  your active file lands as an editor diagnostic with the rule ID and
  the canonical message. A full check (on open and save) lints your
  unsaved buffers, not the stale copy on disk. Findings that are not
  tied to one file (for example a missing `LICENSE`) are shown on the
  `.alint.yml` itself.
- **Same results as `alint check`.** The config's `baseline:` file is
  honored, so grandfathered findings stay hidden in the editor too.
  If the config fails to load, the error is shown on `.alint.yml` and
  the files it governs show no findings until it loads again.
- **Hover-to-explain.** Hovering a diagnostic shows the rule's
  description, fix availability, and a link to its rule-reference page
  (plus the rule's `policy_url`, when it declares one).
- **Apply-fix code actions.** For rules that ship an auto-fix
  (`final_newline`, `no_trailing_whitespace`, `line_endings`, etc.), the
  editor offers a code action that runs the fix in-place. Files larger
  than the config's `fix_size_limit` get no fix, as with `alint fix`.
- **Informational notes.** Skipped non-literal entries (e.g. a registry
  path containing `${MODULE}`) are reported after each full check as a
  one-line count on the server's stderr (usually the editor's LSP log);
  launch the server as `alint lsp --show-notes` to list them all.
- **Auto-discovery.** Each workspace folder is linted by its own
  `.alint.yml`, as `alint check` run in that folder would. A config
  further down (a package's, or one vendored under `node_modules/`)
  governs nothing by itself: under a folder config with
  `nested_configs: true`, files get that config's rules plus the nested
  ones, as `alint check` from the workspace root does. A folder without
  a config of its own lints each file with the config at the top of its
  repository (the nearest directory holding `.git`) inside the folder,
  so a folder of several repositories lints each by its own config.
  Every workspace folder of a multi-root workspace is linted; files
  outside all workspace folders, and files with no config to govern
  them, are not.
- **Workspace trust.** Until the editor says the workspace is trusted,
  the server runs nothing the repository's config could start: command
  rules, rules that run Git and custom facts are skipped, a rule whose fix
  alone runs a command lints without the fix, a remote `extends:` is not
  fetched, `allow_out_of_root:` is refused, and environment values are left
  unset. Everything else lints.
  See [Workspace trust](#workspace-trust) below.

## Configuration

The server reads everything from `.alint.yml` the same way the CLI
does; no editor-specific configuration is required for the rule set
itself. The editor-side settings are:

- **`alint.path`**: absolute path to the `alint` binary, if not on
  `PATH`. Auto-resolved by the VS Code + JetBrains extensions on first
  run.
- **`alint.serverArgs`**: extra args to pass to `alint lsp` on launch,
  such as `--show-notes` (lists informational notes on the server's
  stderr), `--no-spawn`, `--untrusted`, `--no-gitignore` or
  `-c <your config>`; see
  [`alint lsp`](/docs/cli/lsp/#flags).
- **Workspace trust**: the `trusted` initialization option, below.

## Workspace trust

A repository's `.alint.yml` can run commands (`kind: command` and the
other spawning rules, `custom:` facts), fetch a remote `extends:`, set
`allow_out_of_root:`, and read your environment. The language server turns
all of that off until
the editor tells it the workspace is trusted, with the initialization
option `{"trusted": true}`; with no option the workspace is untrusted.
The [`alint lsp`](/docs/cli/lsp/#workspace-trust) page lists exactly
what is off.

**VS Code** sends its own Workspace Trust state: in Restricted Mode the
server is untrusted, and granting trust restarts it trusted. Nothing to
configure. The workspace's own `alint.path` and `alint.serverArgs` are
ignored in Restricted Mode.

In other editors, opt in per editor in your **user-level** config. Do not
put it in settings the repository can supply (a workspace
`.helix/languages.toml`, `.zed/settings.json`, an `exrc` `.nvim.lua`):
the repository could then trust itself. Each snippet below trusts every
workspace the editor opens with alint, so use it only if you open
repositories you trust.

- **Neovim** (0.11+):

  ```lua
  vim.lsp.config("alint", { init_options = { trusted = true } })
  ```

- **Helix** (`~/.config/helix/languages.toml`):

  ```toml
  [language-server.alint]
  command = "alint"
  args = ["lsp"]
  config = { trusted = true }
  ```

- **Sublime Text** (LSP settings, in `clients.alint`):

  ```json
  "initializationOptions": { "trusted": true }
  ```

- **Zed** (user `settings.json`):

  ```json
  "lsp": { "alint": { "initialization_options": { "trusted": true } } }
  ```

- **Emacs**, Eglot: add `:initializationOptions (:trusted t)` after the
  command, for example `("alint" "lsp" :initializationOptions (:trusted t))`
  (with the bundled `alint.el`, set `alint-trusted` to `t`). lsp-mode:
  pass `:initialization-options (lambda () (list :trusted t))` to
  `make-lsp-client`.
- **JetBrains** and **Eclipse** (LSP4E): the plugin and LSP4E's settings
  do not send the option yet, so the server stays untrusted there. To
  lint with command rules, start the server with your own config instead
  (`alint lsp -c <file>`), which is trusted as `alint check -c` is.

## Authoring rules with editor support

Editor diagnostics are read-only previews of `alint check` output, so
the workflow is "edit `.alint.yml`, see diagnostics live, save". For
the full rule-authoring workflow (designing a new rule kind, writing
fixtures, the `xtask` audits) see
[Rule authoring](/docs/development/rule-authoring/).
