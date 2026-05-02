# `alint-lsp` — Language Server Protocol implementation

Status: Design draft, written 2026-05-02 after v0.9.6.

## Problem

alint's CLI surface (`check`, `fix`, `explain`, `list`, `facts`,
`suggest`, `export-agents-md`) covers batch / pre-commit / CI use. Inline
editor diagnostics, hover-to-explain, and one-click "apply fix" are the
gap that the v0.5 field test repeatedly surfaced — adopters who'd happily
run alint locally still wanted feedback in the editor before the
pre-commit hook fires.

The agent-era pivot reinforces this: an LLM with editor access via LSP
gets per-violation `agent_instruction` strings (`--format agent`), but
the round-trip is asymmetric — agents see diagnostics in the LSP feed,
not from a CLI invocation. Without an LSP server, alint is invisible to
in-editor agents.

## Surface area

A new crate `crates/alint-lsp/` implementing the LSP 3.17 spec subset
relevant to a static analyser:

- **`textDocument/didOpen`, `didChange`, `didSave`, `didClose`** —
  manage the LSP server's view of in-flight files.
- **`textDocument/publishDiagnostics`** — push violation lists per file.
  Triggered by `didChange` (debounced) and `didSave` (immediate).
- **`textDocument/hover`** — show the rule's `policy_url`, `message`,
  and a one-line summary on hover over a violation marker.
- **`textDocument/codeAction`** — emit "Apply fix" actions for rules
  with a `Fixer`, plus "Add rule to ignore" for any violation.
- **`workspace/didChangeWatchedFiles`** — invalidate the in-memory
  `FileIndex` when files are added / removed / renamed outside the editor.

Out of scope for v0.10: completion, semantic tokens, definition/
references (alint isn't a code-intelligence tool — those belong to
language-specific LSPs).

## Semantics

The server holds:

1. **Workspace `FileIndex`** — built once on `initialize`, refreshed on
   `didChangeWatchedFiles` (incremental — add/remove entries by path
   rather than re-walking).
2. **Loaded `Engine`** — built once from the workspace's `.alint.yml`
   (and `extends:` chain). Reloaded on `didChange` of `.alint.yml`.
3. **Per-file diagnostic cache** — last violations published per URI.

On `didChange`/`didSave`, the server:

1. Updates its in-memory copy of the file's bytes.
2. Calls `Engine::run_for_file(root, index, file_path, bytes)` (new
   contract, see [`single_file_reevaluation.md`](./single_file_reevaluation.md)).
3. Diffs the new violation list against the cache; publishes via
   `textDocument/publishDiagnostics`.

Cross-file rules (`pair`, `for_each_dir`, `every_matching_has`,
`unique_by`, `dir_contains`, `dir_only_contains`, existence rules) are
re-evaluated only on save AND only when the saved file's path
participates in the rule's scope. Specifics in
`single_file_reevaluation.md`.

## False-positive surface

- **Stale `Engine` after `.alint.yml` edit** — debounce the reload (200
  ms) but force a full re-evaluate after reload completes. UI shows a
  "Reloading config…" status during the gap so users know stale
  diagnostics aren't being trusted.
- **Cancellation races** — a `didChange` while a previous evaluation is
  in flight should cancel the previous one. Tower-LSP supports
  `CancellationToken`; thread it through `Engine::run_for_file`.
- **Cross-file invalidation** — a `pair` rule with `primary: README.md`,
  `pair_with: LICENSE`. Editing README.md should re-check the pair rule;
  editing `unrelated.rs` should not. The server walks loaded rules'
  `path_scope().matches(changed_path)` to decide which cross-file rules
  to re-run.

## Implementation notes

- New crate `crates/alint-lsp/`, `publish = false` initially (promote
  to public once the API surface is stable).
- Workspace dep `tower-lsp = "0.20"` (added with this design pass).
- Built into the alint binary via a `lsp` subcommand (`alint lsp`),
  speaking LSP over stdio. The VS Code extension launches `alint lsp`
  as the language server.
- Initial `tracing` instrumentation for every LSP request — surfaces in
  the editor's "Output → alint" channel for debugging.

Complexity estimate: ~2,000 lines for the core server, ~300 lines for
the `alint lsp` subcommand wiring, ~200 lines for the VS Code
extension. Two-week scope for a single contributor; one week with two.

## Tests

- Unit: `tower-lsp`'s `LspService::test_service` harness for each handler.
- Integration: a stdio harness in `crates/alint-lsp/tests/` that
  spawns the server, sends LSP requests, and asserts on responses.
- E2E: `crates/alint-e2e/tests/lsp_smoke.rs` that spawns `alint lsp`
  as a subprocess and exercises the open → change → diagnostic loop.
- VS Code: a `tests/extension/` directory using `@vscode/test-electron`
  to drive the extension end-to-end.

Bench-compare thresholds:

- `Engine::run_for_file` micro-bench (new): floor at 5 ms for a
  single-file evaluation against a 100k-file index.
- LSP `didChange` → `publishDiagnostics` round-trip wall-time: floor at
  50 ms (95th percentile across 100 didChange messages on a 100k-file
  workspace).

## Open questions

1. **One LSP process per workspace, or per-folder?** VS Code's LSP
   client default is one-per-workspace. Multi-root workspaces with
   different `.alint.yml` per folder need either two server instances
   or a server that supports multiple `workspaceFolders`. tower-lsp
   supports the latter; defaulting to one process is simpler.
2. **Should `apply fix` write the fix immediately or defer to the
   editor's "Save with fix" flow?** Defer — modifying buffers without
   user awareness violates editor norms. The code action returns a
   `WorkspaceEdit`; the editor applies it.
3. **Should we ship a `--port` mode for tcp/socket LSP?** No for v0.10
   (stdio is universal). Add later if a contributor surfaces a need
   (Eclipse, some Vim plugins).
