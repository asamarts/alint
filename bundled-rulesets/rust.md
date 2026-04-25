---
title: 'rust@v1'
description: Bundled alint ruleset at alint://bundled/rust@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/rust@v1
```

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

