---
title: 'generated_file_fresh'
description: 'A committed artefact must equal what a declared command generator produces, in one of two modes (exactly one of file / outputs).'
sidebar:
  order: 8
categories: ['cross-file', 'security-unicode-sanity']
---

A committed artefact must equal what a declared `command` generator produces, in one of two modes (exactly one of `file` / `outputs`). **alint never leaves regenerated files behind** — it *verifies* freshness, it does not run codegen as a build step. Either mode runs a user-declared, maintainer-trusted process, so the kind is trust-gated to your own top-level config (same tier as the `command` rule). Single-shot, opt-in. Spawn-failure / non-zero exit / timeout are each a clear, distinct violation. `normalize` (`none` / `trim` / `final-newline`) absorbs trailing-newline churn.

- **stdout mode** (`file:`) — the generator writes its single output to stdout; alint captures it and compares to the one committed `file`. Never writes the tree.
- **mutating / in-place mode** (`outputs:`, a glob or list) — for the common `make gen && git diff --exit-code` pattern, where the generator rewrites files in place. alint **snapshots** the `outputs`, runs the generator, **diffs** (flagging each stale / newly-created / removed file), and **restores the snapshot** — so `alint check` leaves the working tree byte-identical (the restore is panic-safe). The generator must confine its writes to `outputs`.

```yaml
# stdout mode — diff the generator's stdout against one committed file
- id: bindings-fresh
  kind: generated_file_fresh
  file: crates/ffi/include/core.h
  command: ["cbindgen", "--config", "cbindgen.toml", "crates/core"]
  normalize: final-newline
  level: error

# mutating mode — run the in-place generator and assert nothing changed
- id: commands-def-fresh
  kind: generated_file_fresh
  outputs: "src/commands.def"          # glob or list; selects mutating mode
  command: ["make", "commands.def"]
  timeout: 300
  level: error
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `command` | list of string | yes |  | Generator argv (no shell). STDOUT mode: emit the file's contents to stdout. MUTATING mode: write the `outputs` in place. |
| `file` | string |  |  | STDOUT mode: the committed generated file to verify against the generator's stdout. |
| `normalize` | one of `none` \| `trim` \| `final-newline` |  |  | Normalization applied before comparison to absorb trailing-newline churn: `none`, `trim`, or `final-newline`. |
| `outputs` | string or list of string |  |  | MUTATING mode: the glob (or list of globs) the in-place generator rewrites; its presence selects the mutating mode. alint snapshots these, runs the generator, diffs, and restores them. |
| `timeout` | integer (>= 1) |  |  | Generator timeout in seconds (default 120). On timeout the child is killed and one violation is emitted. |
| `workdir` | string |  |  | Generator cwd, relative to the lint root (default: lint root). |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
