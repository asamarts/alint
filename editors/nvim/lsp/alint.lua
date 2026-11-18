-- alint language-server config for Neovim's built-in LSP client
-- (Neovim 0.11+). Drop this file on your runtimepath (e.g. add
-- `editors/nvim` to rtp, or copy it to `~/.config/nvim/lsp/alint.lua`),
-- then enable it once with:
--
--     vim.lsp.enable("alint")
--
-- This is also the definition upstreamed to nvim-lspconfig, which now
-- consumes `lsp/<server>.lua` configs of exactly this shape.

---@type vim.lsp.Config
return {
  -- Launch the alint language server. Requires `alint` on PATH; install
  -- via Homebrew / `cargo install alint` / `npm i -g @asamarts/alint` /
  -- Docker / the install script.
  cmd = { "alint", "lsp" },

  -- alint is repo-structural (it lints every file), but Neovim attaches
  -- language servers by filetype — there is no "all files" wildcard.
  -- This lists a broad set of common filetypes; extend it for languages
  -- you want alint diagnostics in.
  filetypes = {
    "rust",
    "python",
    "javascript",
    "typescript",
    "typescriptreact",
    "go",
    "c",
    "cpp",
    "ruby",
    "json",
    "yaml",
    "toml",
    "markdown",
  },

  -- Root is the nearest ancestor holding a config (or the repo root).
  root_markers = { ".alint.yml", ".git" },

  -- Until it is told the workspace is trusted, the server runs nothing the
  -- repository's config could start (command rules, rules that run Git,
  -- custom facts), fetches no remote `extends:` and reads no environment
  -- value. To trust every workspace, add this in your own config (never a
  -- repository's `.nvim.lua`):
  --
  --     vim.lsp.config("alint", { init_options = { trusted = true } })
}
