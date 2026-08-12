---
title: 'file_absent'
description: 'No file matching paths may exist in the walked tree. alint file_absent rule, existence family.'
sidebar:
  order: 2
categories: ['existence']
---

No file matching `paths` may exist in the walked tree. The inverse of `file_exists`.

Fix: `file_remove` — delete every violating file.

**Optional `root_only: true`** (like `file_exists`) restricts the check to the repository root: a file forbidden at the root does not fire on nested copies of the same name.
**Optional `git_tracked_only: true`** restricts the check to files in git's index. With it set, the rule fires only on tracked paths regardless of `.gitignore` state — closing the gap where a `git add -f`'d file slips past the walker's gitignore filter. Outside a git repo the rule becomes a silent no-op.

**Optional `content_prefix_hex`** narrows a name match with a content check: a matching file fires only if its bytes begin with one of the listed hex signatures. This separates real binary junk from unrelated files that share a name pattern — macOS AppleDouble sidecars (`._*`) start with `00 05 16 07` and `.DS_Store` with `00 00 00 01` `"Bud1"`, whereas Hadoop writes `._<name>.crc` checksum files that begin with `crc\0`. A file that cannot be read, or is shorter than every signature, does not match; an empty list (the default) keeps the name-only behaviour.

**What "exists" means**: alint walks the filesystem and honours `.gitignore` by default, so a `file_absent` rule fires whenever a matching file is **present in the walked tree**, not when it's tracked in git. Files filtered by `.gitignore` are invisible to the rule. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics, the `--no-gitignore` flag, and the gap between this and git's actual index.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `content_prefix_hex` | list of string |  | `[]` | When non-empty, a file matching `paths` is reported only if its raw content begins with one of these byte signatures, each given as an even-length hex string (e.g. `"00051607"`). This separates genuine binary junk from unrelated files that merely share the name pattern: macOS `AppleDouble` sidecars start with `00 05 16 07` and `.DS_Store` with `00 00 00 01 "Bud1"`, whereas Hadoop writes `._<name>.crc` checksum files that begin with `crc\0` and are not macOS junk. A file that cannot be read, or is shorter than every signature, does not match. Empty (the default) keeps the historical name-only behaviour. |
| `git_tracked_only` | boolean |  | `false` | Restrict matches to files tracked in git's index: entries present in the walked tree but not in `git ls-files` are skipped. No effect outside a git repo. Default `false`. |
| `root_only` | boolean |  | `false` | If true, only a file matching `paths` directly at the repository root is forbidden; a nested match with the same name is allowed. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A repository with stray .bak files

The rule fires on this repository:

```text
Cargo.lock.bak
Cargo.toml
src/
src/main.rs.bak
```

`Cargo.lock.bak`:

```text
# generated
```

`Cargo.toml`:

```toml
[package]
name = "demo"
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-bak-files
    kind: file_absent
    paths: "**/*.bak"
    level: error
```

### A repository with no .bak files

This repository is compliant:

```text
Cargo.toml
src/
src/main.rs
```

`Cargo.toml`:

```toml
[package]
name = "demo"
```

`src/main.rs`:

```rust
fn main() {}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-bak-files
    kind: file_absent
    paths: "**/*.bak"
    level: error
```

