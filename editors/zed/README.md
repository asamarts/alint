# alint for Zed

Repository-structure linting from [alint](https://github.com/asamarts/alint)
inside [Zed](https://zed.dev), via the `alint lsp` language server.

Zed has no config-only way to launch a custom LSP binary, so this is a
small Rust→wasm extension that tells Zed how to start `alint lsp` and
resolves the binary. Diagnostics, hover, and quick-fixes are rendered by
Zed from the server.

## How the binary is found

1. `binary.path` in the worktree's LSP settings for the `alint` server,
2. `alint` on `PATH`,
3. a copy the extension downloaded earlier,
4. otherwise the matching release is downloaded from GitHub.

You can also install alint yourself via Homebrew, `cargo install alint`,
`npm i -g @asamarts/alint`, Docker, or the install script.

## Workspace trust

Until the server is told a workspace is trusted, it runs nothing the
repository's `.alint.yml` could start: command rules, rules that run Git
and `custom:` facts are skipped, a remote `extends:` is not fetched,
and `{{env.*}}` values are left unset. Every other rule lints. See
[Workspace trust](https://alint.org/docs/cli/lsp/#workspace-trust).

To turn them on, add this to your user `settings.json`:

```json
"lsp": { "alint": { "initialization_options": { "trusted": true } } }
```

Set it in your user-level config, never in a file a repository can
supply, and only if you open repositories you trust: it trusts every
workspace alint lints.
A repository's `.zed/settings.json` must not be the place.

## Known limitation

Zed attaches language servers per-language; there is no "all files"
wildcard. alint is repo-structural, so `extension.toml` registers it
against a broad set of common languages. Files in languages **not**
listed there won't get alint diagnostics in Zed — extend the
`languages` list as needed.

## Building / publishing

> Authored without running Zed's registry build. Validate first:

```sh
cd editors/zed
rustup target add wasm32-wasip1
cargo build --release --target wasm32-wasip1
```

Confirm the `zed_extension_api` version in `Cargo.toml` matches what the
registry currently builds against. Publishing is a **manual PR** to
[`zed-industries/extensions`](https://github.com/zed-industries/extensions)
(add an entry pointing at this repo + subdirectory); the registry builds
the wasm from source. There is no automated release job for Zed.

## Versioning

The extension version tracks the alint release it ships against.
