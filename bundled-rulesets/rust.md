---
title: 'rust@v1'
description: Bundled alint ruleset at alint://bundled/rust@v1.
---

Hygiene checks for Rust projects. Adopt it with:

```yaml
extends:
  - alint://bundled/rust@v1
```

This ruleset is gated with `when: facts.is_rust` where a rule
wouldn't make sense outside a Rust tree, so it's safe to extend
from a polyglot repo's root config — rules that don't apply stay
quiet. (`is_rust` is declared below; override with your own
`facts:` block if you need a different heuristic.)

## Rules

### `rust-cargo-toml-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **when**: `facts.is_rust`

> Rust project: Cargo.toml at the repo root is required.

### `rust-cargo-lock-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **when**: `facts.is_rust`
- **policy**: <https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html>

> Committing Cargo.lock ensures reproducible builds for binary crates. Library-only workspaces may legitimately opt out — set this rule's `level: off` in that case.

### `rust-toolchain-pinned`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **when**: `facts.is_rust`

> Pinning a toolchain (rust-toolchain.toml) makes local and CI builds reproducible.

### `rust-no-tracked-target`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`
- **when**: `facts.is_rust`

> `target/` is Cargo's build dir and must never be committed.

### `rust-sources-snake-case`

- **kind**: [`filename_case`](/docs/rules/naming/filename_case/)
- **level**: `error`
- **when**: `facts.is_rust`

> Rust module filenames must be snake_case.

### `rust-sources-final-newline`

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `warning`
- **when**: `facts.is_rust`

### `rust-sources-no-trailing-whitespace`

- **kind**: [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/)
- **level**: `info`
- **when**: `facts.is_rust`

### `rust-sources-no-bidi`

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **when**: `facts.is_rust`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars in Rust sources are rejected.

### `rust-sources-no-zero-width`

- **kind**: [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/)
- **level**: `error`
- **when**: `facts.is_rust`

> Zero-width characters in Rust sources are rejected (review hazard).

### `rust-no-merge-markers-in-manifests`

- **kind**: [`no_merge_conflict_markers`](/docs/rules/security-unicode-sanity/no_merge_conflict_markers/)
- **level**: `error`
- **when**: `facts.is_rust`

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/rust.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/rust.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/rust@v1
#
# Hygiene checks for Rust projects. Adopt it with:
#
#     extends:
#       - alint://bundled/rust@v1
#
# This ruleset is gated with `when: facts.is_rust` where a rule
# wouldn't make sense outside a Rust tree, so it's safe to extend
# from a polyglot repo's root config — rules that don't apply stay
# quiet. (`is_rust` is declared below; override with your own
# `facts:` block if you need a different heuristic.)

version: 1

facts:
  - id: is_rust
    any_file_exists: [Cargo.toml]

rules:
  # --- Workspace / package manifest ---------------------------------
  - id: rust-cargo-toml-exists
    when: facts.is_rust
    kind: file_exists
    paths: Cargo.toml
    root_only: true
    level: error
    message: "Rust project: Cargo.toml at the repo root is required."

  - id: rust-cargo-lock-exists
    when: facts.is_rust
    kind: file_exists
    paths: Cargo.lock
    root_only: true
    level: warning
    message: >-
      Committing Cargo.lock ensures reproducible builds for binary
      crates. Library-only workspaces may legitimately opt out — set
      this rule's `level: off` in that case.
    policy_url: "https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html"

  - id: rust-toolchain-pinned
    when: facts.is_rust
    kind: file_exists
    paths: ["rust-toolchain.toml", "rust-toolchain"]
    root_only: true
    level: info
    message: "Pinning a toolchain (rust-toolchain.toml) makes local and CI builds reproducible."

  # --- Build artefacts must not be tracked --------------------------
  - id: rust-no-tracked-target
    when: facts.is_rust
    kind: dir_absent
    paths: "**/target"
    level: error
    message: "`target/` is Cargo's build dir and must never be committed."

  # --- Source-file conventions --------------------------------------
  - id: rust-sources-snake-case
    when: facts.is_rust
    kind: filename_case
    paths: "**/src/**/*.rs"
    case: snake
    level: error
    message: "Rust module filenames must be snake_case."
    fix:
      file_rename: {}

  - id: rust-sources-final-newline
    when: facts.is_rust
    kind: final_newline
    paths: "**/*.rs"
    level: warning
    fix:
      file_append_final_newline: {}

  - id: rust-sources-no-trailing-whitespace
    when: facts.is_rust
    kind: no_trailing_whitespace
    paths: "**/*.rs"
    level: info
    fix:
      file_trim_trailing_whitespace: {}

  # --- Trojan Source defense on Rust sources ------------------------
  - id: rust-sources-no-bidi
    when: facts.is_rust
    kind: no_bidi_controls
    paths: "**/*.rs"
    level: error
    message: "Trojan Source (CVE-2021-42574): bidi override chars in Rust sources are rejected."
    policy_url: "https://trojansource.codes/"

  - id: rust-sources-no-zero-width
    when: facts.is_rust
    kind: no_zero_width_chars
    paths: "**/*.rs"
    level: error
    message: "Zero-width characters in Rust sources are rejected (review hazard)."

  # --- Workspace-level niceties -------------------------------------
  - id: rust-no-merge-markers-in-manifests
    when: facts.is_rust
    kind: no_merge_conflict_markers
    paths: ["Cargo.toml", "**/Cargo.toml", "Cargo.lock", "**/Cargo.lock"]
    level: error
```
