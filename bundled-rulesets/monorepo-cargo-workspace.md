---
title: 'monorepo/cargo-workspace@v1'
description: Bundled alint ruleset at alint://bundled/monorepo/cargo-workspace@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/monorepo/cargo-workspace@v1
```

## Rules

### `cargo-workspace-members-declared`

- **kind**: [`toml_path_matches`](/docs/rules/content/toml_path_matches/)
- **level**: `error`
- **when**: `facts.is_cargo_workspace`
- **policy**: <https://doc.rust-lang.org/cargo/reference/workspaces.html>

> Cargo workspace must declare `members = [...]` under `[workspace]`. Without it, `cargo build` fails to resolve the package graph.

### `cargo-workspace-member-has-readme`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_cargo_workspace`

> Cargo workspace members should have a README.md so the crate's purpose is discoverable from the directory tree.

### `cargo-workspace-member-declares-name`

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_cargo_workspace`

> Workspace member's Cargo.toml must declare `[package]` with a `name` field.

