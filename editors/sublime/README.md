# alint for Sublime Text

Repository-structure linting from [alint](https://github.com/asamarts/alint)
in Sublime Text, via the `alint lsp` language server and the
[`LSP`](https://packagecontrol.io/packages/LSP) package. This is a
**config** integration (Tier 2).

Requires the `alint` binary on `PATH` (Homebrew, `cargo install alint`,
`npm i -g @asamarts/alint`, Docker, or the install script).

## Setup

1. Install the **LSP** package via Package Control.
2. Open **Preferences → Package Settings → LSP → Settings** and add the
   `clients.alint` block from [`LSP.sublime-settings`](./LSP.sublime-settings)
   (merge it into your existing `clients` map).
3. Reload Sublime / restart the LSP server.

The client runs `alint lsp` and attaches to the scopes in `selector`.

## Workspace trust

Until the server is told a workspace is trusted, it runs nothing the
repository's `.alint.yml` could start: command rules, rules that run Git
and `custom:` facts are skipped, a remote `extends:` is not fetched,
`allow_out_of_root:` is refused, and `{{env.*}}` values are left unset.
Every other rule lints. See
[Workspace trust](https://alint.org/docs/cli/lsp/#workspace-trust).

To turn them on, set `"trusted": true` in the client's
`initializationOptions` (the snippet ships it as `false`).
Set it in your user-level config, never in a file a repository can
supply, and only if you open repositories you trust: it trusts every
workspace alint lints.
A `.sublime-project` from a repository must not be the place.

## Known limitation

Sublime's `LSP` attaches by **scope selector**, with no "all files"
wildcard. alint is repo-structural, so the `selector` lists a broad set
of common language scopes; add others you want alint to lint.

## Optional: an `LSP-alint` helper package

Mirroring `LSP-pylsp` / `LSP-ruff`, a thin Package Control helper
(`LSP-alint`) could bundle this config plus binary resolution so users
don't paste a snippet. That package is **not yet built** — tracked as a
follow-up; the config snippet above is the supported path today.
