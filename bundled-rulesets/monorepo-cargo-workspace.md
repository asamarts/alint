---
title: 'monorepo/cargo-workspace@v1'
description: Bundled alint ruleset at alint://bundled/monorepo/cargo-workspace@v1.
---

Workspace-aware overlay for Cargo workspaces. Layered on
top of `monorepo@v1` (which covers polyglot package-dir
conventions) and `rust@v1` (which covers Rust source
hygiene). Adopt with:

```yaml
extends:
  - alint://bundled/monorepo@v1
  - alint://bundled/rust@v1
  - alint://bundled/monorepo/cargo-workspace@v1
```

Gated by `facts.is_cargo_workspace` — the root Cargo.toml
must declare a `[workspace]` table. Outside an actual
workspace (e.g. a single-crate repo, or a polyglot tree
whose Rust portion isn't a workspace), the rules silently
no-op so the ruleset is safe to extend unconditionally.

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

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/monorepo/cargo-workspace.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/monorepo/cargo-workspace.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/monorepo/cargo-workspace@v1
#
# Workspace-aware overlay for Cargo workspaces. Layered on
# top of `monorepo@v1` (which covers polyglot package-dir
# conventions) and `rust@v1` (which covers Rust source
# hygiene). Adopt with:
#
#     extends:
#       - alint://bundled/monorepo@v1
#       - alint://bundled/rust@v1
#       - alint://bundled/monorepo/cargo-workspace@v1
#
# Gated by `facts.is_cargo_workspace` — the root Cargo.toml
# must declare a `[workspace]` table. Outside an actual
# workspace (e.g. a single-crate repo, or a polyglot tree
# whose Rust portion isn't a workspace), the rules silently
# no-op so the ruleset is safe to extend unconditionally.

version: 1

facts:
  - id: is_cargo_workspace
    file_content_matches:
      paths: Cargo.toml
      pattern: '(?m)^\[workspace\]'

rules:
  # The workspace root must declare its members. A `[workspace]`
  # table without `members = [...]` is broken — Cargo rejects it.
  # Catching this here keeps the failure local to alint output.
  - id: cargo-workspace-members-declared
    when: facts.is_cargo_workspace
    kind: toml_path_matches
    paths: Cargo.toml
    path: "$.workspace.members[*]"
    matches: ".+"
    level: error
    message: >-
      Cargo workspace must declare `members = [...]` under
      `[workspace]`. Without it, `cargo build` fails to resolve
      the package graph.
    policy_url: "https://doc.rust-lang.org/cargo/reference/workspaces.html"

  # Every actual workspace member (a `crates/*` directory that
  # has a Cargo.toml of its own) needs a README. Pre-v0.5.2,
  # this would have fired on `crates/notes/` even if it wasn't a
  # real package — `when_iter:` scopes the iteration to just
  # the package dirs.
  - id: cargo-workspace-member-has-readme
    when: facts.is_cargo_workspace
    kind: for_each_dir
    select: "crates/*"
    when_iter: 'iter.has_file("Cargo.toml")'
    require:
      - kind: file_exists
        paths: "{path}/README.md"
    level: warning
    message: >-
      Cargo workspace members should have a README.md so the
      crate's purpose is discoverable from the directory tree.

  # Each member's Cargo.toml should declare its package name and
  # edition — the two fields most commonly missing from
  # hand-rolled members.
  - id: cargo-workspace-member-declares-name
    when: facts.is_cargo_workspace
    kind: for_each_dir
    select: "crates/*"
    when_iter: 'iter.has_file("Cargo.toml")'
    require:
      - kind: toml_path_matches
        paths: "{path}/Cargo.toml"
        path: "$.package.name"
        matches: '^[A-Za-z][A-Za-z0-9_-]*$'
    level: warning
    message: >-
      Workspace member's Cargo.toml must declare
      `[package]` with a `name` field.
```
