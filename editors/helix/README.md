# alint for Helix

Repository-structure linting from [alint](https://github.com/asamarts/alint)
in [Helix](https://helix-editor.com), via the `alint lsp` language
server. Helix speaks LSP natively, so this is a **config-only**
integration (an honorable mention, not a first-party package).

Requires the `alint` binary on `PATH`.

## Setup

Merge [`languages.toml`](./languages.toml) into
`~/.config/helix/languages.toml`: it declares the `alint`
language-server and adds it to a few languages' server lists.

## Workspace trust

Until the server is told a workspace is trusted, it runs nothing the
repository's `.alint.yml` could start: command rules, rules that run Git
and `custom:` facts are skipped, a remote `extends:` is not fetched, and
`{{env.*}}` values are left unset. Every other rule lints. See
[Workspace trust](https://alint.org/docs/cli/lsp/#workspace-trust).

To turn them on, uncomment `config = { trusted = true }` under
`[language-server.alint]` in `~/.config/helix/languages.toml` (Helix
sends `config` as the server's initialization options).
Set it in your user-level config, never in a file a repository can
supply, and only if you open repositories you trust: it trusts every
workspace alint lints.
A workspace `.helix/languages.toml` must not be the place.

## Known limitation

Helix attaches language servers **per-language** — there is no "all
files" wildcard. alint is repo-structural, so add `"alint"` to the
`language-servers` list of every language you want it to lint (see the
examples in `languages.toml`).
