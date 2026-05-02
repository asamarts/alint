# v0.10 — Design pass

Status: Working drafts, written 2026-05-02 immediately after the v0.9 cut
closed (v0.9.6 + the post-release `scope_filter:` runtime fix). Each file
in this directory is a per-feature design that should be reviewed and
revised before implementation starts.

## What v0.10 ships

The first user-visible IDE / agent integration. v0.9 was engine-internal
performance work; v0.10 turns the per-file dispatch hot path into a
single-file re-evaluation contract that an LSP server can drive cheaply.

| File | Sub-theme |
|---|---|
| [`lsp_server.md`](./lsp_server.md) | New crate `alint-lsp` implementing the Language Server Protocol — diagnostics on save / on change, hover with rule documentation, code actions for "apply fix" and "add rule to ignore". |
| [`single_file_reevaluation.md`](./single_file_reevaluation.md) | Engine-side contract: given an unchanged `FileIndex` and a single edited file, evaluate only the rules whose `path_scope` (and `scope_filter`, post-v0.9.6) match that file. Reuses the per-file dispatch path from v0.9.3. |
| [`vscode_extension.md`](./vscode_extension.md) | Thin VS Code extension that ships the bundled `alint-lsp` binary, registers the LSP client, and surfaces alint's eight output formats inline. |

## Cross-cutting decisions

A few questions touch multiple sub-themes and benefit from being settled
once.

### LSP crate choice

Two candidates:

- **[`tower-lsp`](https://crates.io/crates/tower-lsp)** — async, Tower-based,
  the most popular choice in the Rust LSP ecosystem (rust-analyzer, taplo,
  ruff-server, biome). Brings a tokio runtime dep.
- **[`lsp-server`](https://crates.io/crates/lsp-server)** — sync,
  stdio-only, used by rust-analyzer's lower layers. Smaller surface, no
  tokio.

Recommendation: **`tower-lsp`**. The async runtime cost is small (~5 MiB
binary), the ecosystem is more active, and the Tower middleware story
makes adding tracing / metrics / cancellation later much cheaper.
Workspace dep is added in the same commit as this design pass:
`tower-lsp = "0.20"`.

If profile data later shows tokio overhead is unacceptable for the
file-change-throttling hot path, the engine wrapper is small enough to
swap to `lsp-server`. Worth measuring; not worth pre-optimising.

### Single-file re-evaluation contract

The LSP server cannot afford a full repo scan on every keystroke. v0.9.6
already gave us most of the primitives:

- `FileIndex::contains_file` is O(1) (v0.9.5 lazy path-index).
- Per-file rules are the file-major dispatch path (v0.9.3).
- `ScopeFilter::matches` walks ancestors via O(1) lookups (v0.9.6, plus
  the post-release fix that actually wires it through `Rule::scope_filter()`).

What's missing: a `Engine::run_for_file(root, index, file_path)` method
that evaluates the changed file against rules whose `path_scope` matches,
ignoring cross-file rules that need a full re-walk. Cross-file rules
re-evaluate only on save (and even then, only when the file's directory
membership in their `paths:` glob matters).

`single_file_reevaluation.md` settles the cross-file boundary policy.

### Heuristic vs. precise

LSP server work is precise — the LSP protocol has well-defined
diagnostic / hover / code-action shapes. No heuristic surface in v0.10.

### Schema versioning

No `.alint.yml` schema changes. Every v0.9.6 config runs unchanged on
v0.10. `version: 1` covers the entire v0.10 cut.

## Out of scope for v0.10

Explicitly held back to keep the cut tight:

- **WASM plugin tier** — v0.11. PROPOSAL §4.9 anticipates this; the
  `command` plugin (tier 1, shell out per matched file) shipped in
  v0.5.1 and has been the only plugin tier so far.
- **`detect: linguist` and `detect: askalono` facts** — PROPOSAL §4.6
  items still open. They're orthogonal to LSP work; can ship in a
  v0.10.x point release if a contributor picks them up.
- **Live `xtask docs-export` from inside the LSP** (so an editor can
  surface the same hover content the docs site renders). Tempting but
  out of proportion — the docs site is the canonical surface.

## How to use these docs

Each design doc has the same shape as the v0.7 / v0.9 design passes:

1. **Problem** — what user pain this addresses, sourced from the v0.9
   field test (scope_filter feedback, polyglot-monorepo onboarding, agent
   integration loops).
2. **Surface area** — what changes inside the engine / new crate.
3. **Semantics** — what the engine / LSP server does on each request.
4. **False-positive surface** — what could go wrong (LSP cancellation
   races, single-file re-eval missing cross-file dependencies, VS Code
   extension UX gaps) and the planned mitigations.
5. **Implementation notes** — crate location, dependencies, complexity
   estimate.
6. **Tests** — what to cover, including the bench-compare thresholds the
   phase commits to.
7. **Open questions** — decisions to make before implementation.

When implementation starts, the doc gets a `Status: Implemented in
<commit>` header line and any open questions get resolved in the doc
itself, mirroring the v0.7 / v0.9 convention.
