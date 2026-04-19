# Tree Spec — YAML format for directory-tree fixtures

Status: **v0 (internal).** Implemented by the `treespec` module of `alint-testkit`. Kept isolated from the alint-specific scenario runner so the module can be spun out as its own crate if demand warrants.

## Model

A *tree* is a recursive structure where each node is either:

- **A file**, represented by a YAML scalar whose string value is the file's contents.
- **A directory**, represented by a YAML mapping whose keys are child names.

Node names come from the mapping keys of the enclosing directory. The top-level value must be a mapping (i.e. the root is always a directory).

### Minimal example

```yaml
Cargo.toml: "[package]\nname = \"demo\"\n"
src:
  main.rs: "fn main() {}\n"
  lib.rs: ""
docs: {}
```

Materialized: three files (`Cargo.toml`, `src/main.rs`, `src/lib.rs`) and one empty directory (`docs/`). `lib.rs` is an empty file.

## Operations

Three operations round-trip over this model. Implementations live in `treespec/{materialize,verify,extract}.rs` and are deliberately independent of any alint-specific types.

### `materialize(spec, root)`

Creates the described tree on disk under `root`. Intermediate directories are created as needed. Pre-existing entries at `root` are left untouched unless a spec node would overwrite one (in which case the existing file is replaced).

### `verify(spec, root, mode)`

Compares the on-disk tree under `root` to `spec`. Two modes:

- **`Strict`** (default): `root` must contain exactly the paths in `spec`, with byte-identical contents. Extra files, missing files, and content mismatches are all failures.
- **`Contains`**: every path in `spec` must be present and match; extra paths on disk are permitted. Useful when a scenario only cares about one artifact and the surrounding tree is noisy.

Returns a `VerifyReport` listing each discrepancy so test failures point at the specific byte-level delta.

### `extract(root, opts) -> spec`

The inverse: walk `root` and emit a `TreeSpec` matching it. Used for capturing a known-good directory state as a committable fixture.

- Text files are inlined as their UTF-8 content.
- **Binary / non-UTF-8 files are skipped** with a comment placeholder (see "Limitations" below). Inlining megabytes of base64 in a YAML file pollutes scenarios; binary fixtures should live outside the spec.
- Symlinks are currently **not represented** (skipped during extract, ignored during materialize).

## Limitations (v0)

The following are deliberately unsupported in the initial version. Each can be added later without breaking existing specs because the additions take the form of a new tagged node shape.

1. **Symlinks** — no representation. Planned: `{ $link: "/target/path" }` once we need it for a scenario.
2. **Sidecar content** — no way to say "the content of this file lives in `fixtures/big.bin`." Planned: `{ $content_from: "fixtures/big.bin" }` for large or binary content.
3. **File metadata** — mode / mtime / uid / gid are ignored. Scenarios that depend on these should check them separately.
4. **Non-UTF-8 file contents** — must be either skipped during extract or encoded explicitly once sidecars land.
5. **Platform-specific nodes** — Windows reparse points, FIFOs, device files: out of scope.

Tree specs have no version field in v0; the format is append-only and backwards-compatible additions (new `$`-prefixed node types) will not require a version bump.

## Disambiguation rule

File and directory nodes are distinguished by YAML *type*, not by a sigil on the key:

- A scalar value → file.
- A mapping value → directory.

This means a file literally named `link` inside a directory is fine — it's a scalar value. Future tagged node types (`$link`, `$content_from`) use a `$` prefix on the **key** of a single-entry mapping; since legitimate filenames virtually never start with `$`, this keeps the disambiguation unambiguous.

## Relation to other tools

- **`build-fs-tree`** uses the same scalar-file / mapping-directory convention. Tree specs produced here should materialize cleanly with `build-fs-tree create < spec.yaml`, and vice versa, modulo the tagged-node extensions.
- **`snapbox` / `trycmd`**'s `*.in/` + `*.out/` dir layout solves a different problem (CLI command output snapshots). Tree specs are orthogonal — a scenario can use a tree spec for its setup and a `trycmd` harness for its command invocation.
- **`assert_fs`** is a fluent builder, not a declarative spec. Tree specs can be *materialized into* an `assert_fs::TempDir` if predicate-style assertions are wanted alongside.
