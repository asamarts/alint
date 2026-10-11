---
title: 'monorepo/cargo-workspace@v1'
description: 'monorepo/cargo-workspace@v1 bundled alint ruleset: Workspace-aware overlay for Cargo workspaces.'
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

v0.9.18 SCOPE NOTE — non-canonical workspace layouts:

The per-member rules below use `select: "crates/*"` — the
canonical Cargo workspace convention used by rust-lang/rust,
alint itself, and most published workspaces. Workspaces with
non-canonical layouts (deno's `ext/*` + `libs/*` + `runtime/`
\+ `cli/`; clap's `clap_builder`, `clap_derive`, etc.) will
see the bundled per-member checks silently no-op against
their tree.

Non-canonical adopters should redefine the per-member rules
in their `.alint.yml` with the correct `select:` glob (rule
IDs reused at the user level OVERRIDE the bundled definition
while keeping the rule active). For example, deno's config
carries:

```yaml
- id: cargo-workspace-member-has-readme
  kind: for_each_dir
  select: "{ext/*,libs/*,runtime,cli}"
  when_iter: 'iter.has_file("Cargo.toml")'
  require: [{ kind: file_exists, paths: "{path}/README.md" }]
```

(`for_each_dir.select:` accepts a single glob; brace expansion
handles multi-pattern intent.)

The principled fix — a `select_from: "$.workspace.members"`
refinement that derives the iteration set from the root
Cargo.toml's `[workspace] members` array — ships in v0.10
alongside the `toml_path_array_iter` rule-kind design.
Tracking: v0.10 design candidate "*_path_array_iter".

## What it checks

3 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [cargo-workspace-members-declared](#cargo-workspace-members-declared)<br>`error` | Cargo workspace must declare `members = [...]` under `[workspace]`. |
| [cargo-workspace-member-has-readme](#cargo-workspace-member-has-readme)<br>`warning` | Cargo workspace members should have a README.md so the crate's purpose is discoverable from the directory tree. |
| [cargo-workspace-member-declares-name](#cargo-workspace-member-declares-name)<br>`warning` | Workspace member's Cargo.toml must declare `[package]` with a `name` field. |

All 3 rules run only when `facts.is_cargo_workspace` holds, so the ruleset stays quiet in repositories it doesn't apply to.

## Rules

### `cargo-workspace-members-declared`

The workspace root must declare its members. A `[workspace]` table without `members = [...]` is broken — Cargo rejects it. Catching this here keeps the failure local to alint output.

- **kind**: [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/)
- **level**: `error`
- **when**: `facts.is_cargo_workspace`
- **policy**: <https://doc.rust-lang.org/cargo/reference/workspaces.html>

> Cargo workspace must declare `members = [...]` under `[workspace]`. Without it, `cargo build` fails to resolve the package graph.

```yaml
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
```

### `cargo-workspace-member-has-readme`

Every actual workspace member (a `crates/*` directory that has a Cargo.toml of its own) needs a README. Pre-v0.5.2, this would have fired on `crates/notes/` even if it wasn't a real package — `when_iter:` scopes the iteration to just the package dirs.

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_cargo_workspace`

> Cargo workspace members should have a README.md so the crate's purpose is discoverable from the directory tree.

```yaml
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
```

### `cargo-workspace-member-declares-name`

Each member's Cargo.toml should declare its package name and edition — the two fields most commonly missing from hand-rolled members.

- **kind**: [`for_each_dir`](/docs/rules/cross-file/for_each_dir/)
- **level**: `warning`
- **when**: `facts.is_cargo_workspace`

> Workspace member's Cargo.toml must declare `[package]` with a `name` field.

```yaml
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

## Facts

The `when:` clauses above read these facts. Each is resolved once per run; [`alint facts`](/docs/cli/facts/) prints what they resolved to in your repository.

```yaml
facts:
  - id: is_cargo_workspace
    file_content_matches:
      paths: Cargo.toml
      pattern: '(?m)^\[workspace\]'
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/monorepo/cargo-workspace@v1
rules:
  - id: cargo-workspace-members-declared
    level: off
  - id: cargo-workspace-member-has-readme
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/monorepo/cargo-workspace@v1
    except: [cargo-workspace-members-declared]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/monorepo/cargo-workspace.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/monorepo/cargo-workspace.yml) in the alint repo.
