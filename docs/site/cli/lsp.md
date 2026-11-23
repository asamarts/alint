---
title: 'alint lsp'
description: 'alint lsp runs the alint language server over stdio, so VS Code, Zed, Neovim and other LSP clients show alint findings as you open, edit and save files.'
---

You don't run `alint lsp` yourself: your editor starts it and talks to it over
the Language Server Protocol on stdio. Point your editor's LSP client at the
command `alint lsp`, and findings from the workspace's `.alint.yml` appear as
diagnostics as you open, edit and save files.

Each workspace folder is linted by its own `.alint.yml`, rooted at that config's directory, as
`alint check` run there would. A folder with no config of its own (a directory that holds several
repositories) lints each file with the config at the top of the nearest repository above it inside
the folder (the nearest directory holding a `.git` directory or file), as `alint check` run in that
repository would; a file in no such repository, or in one without a config, is not linted. A config
further down (a package's, or one vendored under `node_modules/`, which holds no `.git`) governs
nothing by itself: the governing config adds it only when it sets `nested_configs: true`, and then
the file gets both configs' rules, matching `alint check` from the root. Pass `--show-notes` (for
example through your editor's `alint.serverArgs` setting) to list informational notes on the
server's stderr after each check instead of a one-line count. Following the LSP specification, the
server exits with code 0 after a `shutdown` request and `exit` notification, and with code 1 when
`exit` arrives without `shutdown`.

## Workspace trust

Opening a folder in an editor should not run its code. So until the editor says the workspace is
trusted, the server lints with everything that could run a command or reach outside the repository
turned off:

- rules that run a process (`kind: command`, `generated_file_fresh`, `command_idempotent`, at any
  `require:` depth) are skipped, and a rule whose fix alone runs one (`git_untrack`, `command`)
  lints without its fix;
- rules that run Git (the `git_*` kinds, `changeset_requires_path`, `pair_changed_together`,
  `git_tracked_only`, `scope_filter.changed_since`) are skipped, since Git runs the commands a
  repository's `.git/config` names;
- `custom:` facts are skipped, so they read as unset;
- a remote `extends:` is not fetched, and the config reports why instead of loading, as it does
  for `allow_out_of_root:`, which would let it read and write outside the repository;
- environment values read nothing: `{{env.NAME}}` takes its `default(...)` (or fails to load
  without one), and `env.NAME` in a `when:` is unset.

Every other rule lints as usual. When anything was turned off, the server says so once, in an
information message naming how to turn it on.

The editor tells the server through the `initialize` request's `initializationOptions`:

```json
{ "trusted": true }
```

Only the JSON value `true` turns trust on; a missing option, or any other value, leaves the
workspace untrusted. The VS Code extension sends VS Code's own Workspace Trust state and the
JetBrains plugin sends IntelliJ's Trusted Projects state, each restarting the server automatically
when trust changes, so a trusted VS Code workspace or JetBrains project behaves as before. In other
editors, set the option in your user-level server settings (see
[Editors](/docs/integrations/editors/)).
Set it only where a repository cannot: an editor that applies settings checked into the
repository (a workspace `.helix/languages.toml`, `.zed/settings.json`, `.nvim.lua` under
`exrc`) would let a cloned repository turn trust on for itself.

Starting the server with `--config <FILE>` (`-c`) lints every workspace folder with that config
instead of the folder's own, rooted at the folder. The file is yours rather than the repository's,
so the session is trusted whatever the editor says, as `alint check -c` runs it. The path must exist
when the server starts.

Starting the server with `--untrusted` makes every workspace untrusted, whatever the editor says
and even with `--config`.

## Flags

`alint lsp` honours these global flags:

- `-c, --config <FILE>`: lint every workspace folder with this config, trusted (see above).
- `--no-gitignore`: lint files the workspace's `.gitignore` excludes.
- `--no-spawn`: refuse a config whose rules, fixes or facts start processes or run Git, even in a
  trusted workspace; the error appears on the config, naming the rule.
- `--untrusted`: take every workspace as untrusted (see above).
- `--as-of YYYY-MM-DD`: see below.
- `--show-notes`: list informational notes on the server's stderr.

## Finding locations

When a rule supplies an exact primary range, the diagnostic highlights that range instead of only
its starting point. Relationship findings can also point to labeled secondary files or ranges.
alint publishes those as `Diagnostic.relatedInformation` only when the editor advertises
`publishDiagnostics.relatedInformation`; clients without that capability receive the same primary
diagnostic without the optional relationships. Positions are converted from alint's Unicode-scalar
coordinates to LSP UTF-16 coordinates against open buffers or the current on-disk file.

## Reference date for time-dependent rules

Start the server as `alint lsp --as-of YYYY-MM-DD` to pin one strict UTC date
for the session. The flag overrides the editor process's `ALINT_AS_OF`;
without either, each workspace config uses its repository HEAD commit's
committer timestamp, resolved once for that session. An explicit date removes
that HEAD-time lookup, but `git_blame_age` still requires a usable Git
repository for tracked-file and blame data; when unavailable, the LSP reports
an Error-severity analysis-failure diagnostic.

## See also

- [Editors](/docs/integrations/editors/): setup for VS Code, Zed, Neovim and others
