# alint for Neovim

Repository-structure linting from [alint](https://github.com/asamarts/alint)
in Neovim, via the `alint lsp` language server and Neovim's built-in LSP
client. This is a **config** integration (Tier 2) — no plugin to
install, just a server definition.

Requires the `alint` binary on `PATH` (Homebrew, `cargo install alint`,
`npm i -g @asamarts/alint`, Docker, or the install script).

## Neovim 0.11+ (built-in `vim.lsp.config`)

Put [`lsp/alint.lua`](./lsp/alint.lua) on your runtimepath, then enable
it once:

```lua
-- e.g. in init.lua, after adding this directory to the runtimepath,
-- or by copying lsp/alint.lua to ~/.config/nvim/lsp/alint.lua
vim.lsp.enable("alint")
```

That's it — `lsp/alint.lua` carries `cmd = { "alint", "lsp" }`,
`root_markers = { { ".git" }, { ".alint.yml", ".alint.yaml", "alint.yml", "alint.yaml" } }`,
and the filetypes to attach to.

## nvim-lspconfig

`nvim-lspconfig` now consumes `lsp/<server>.lua` configs of the same
shape, so the upstream contribution is simply
[`lsp/alint.lua`](./lsp/alint.lua). Once merged there,
`vim.lsp.enable("alint")` works without copying anything. Until then,
use the runtimepath approach above.

## Workspace trust

Until the server is told a workspace is trusted, it runs nothing the
repository's `.alint.yml` could start: command rules, rules that run Git
and `custom:` facts are skipped, a remote `extends:` is not fetched,
`allow_out_of_root:` is refused, and `{{env.*}}` values are left unset.
Every other rule lints. See
[Workspace trust](https://alint.org/docs/cli/lsp/#workspace-trust).

To turn them on, add this to your `init.lua`:

```lua
vim.lsp.config("alint", { init_options = { trusted = true } })
```

Set it in your user-level config, never in a file a repository can
supply, and only if you open repositories you trust: it trusts every
workspace alint lints.
With `exrc`, a repository's `.nvim.lua` must not be the place.

## Known limitation

Neovim attaches language servers **by filetype** — there is no "all
files" wildcard. alint is repo-structural, so `lsp/alint.lua` lists a
broad set of common filetypes; add any others you want alint to lint.
