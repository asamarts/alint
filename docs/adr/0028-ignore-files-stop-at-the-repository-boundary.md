---
status: accepted
date: 2026-11-04
decision-makers: asamarts
---

# 0028. Ignore files stop at the repository boundary

## Status

Accepted (2026-11-04).

## Context

alint's walker is the `ignore` crate (0.4.34). Since v0.1 it has been built with
`require_git(false)`, so that a tree with no `.git` still honors its `.gitignore` files, as the
concepts page "The walker and git" documents. A side effect is that `ignore` never marks a
directory as a repository. It therefore applied the `.gitignore` and `info/exclude` files of
every directory above the walk root, up to the filesystem root, and applied an outer
repository's rules inside a nested repository or submodule.

The 2026-11 audit found the failure this allows: a `*` in a `.gitignore` above the repository
(a home directory kept in git, or a parent workspace) made alint index nothing and report every
rule as passed. git does not apply that file to the repository below it. The same commit could
pass on one machine and fail on another, depending on directories outside the repository.

The design is in [`ignore-files-repository-boundary.md`](../design/ignore-files-repository-boundary.md).

## Decision

1. **In a repository, git's boundary.** When the walk root, or a directory above it, holds a
   repository (a `.git` directory, the `.git` file of a linked worktree or submodule, or a
   `.jj` directory), `.gitignore` and `info/exclude` files apply from the entry's directory up
   to the nearest directory that holds a repository, that directory included, and no further.
   This is `ignore`'s `require_git(true)`. It holds in both directions: nothing above a
   repository's top applies to it, and a repository's rules stop at a nested repository or
   submodule below it.
2. **A subdirectory root still reads its repository.** A root linted from inside a repository
   (a monorepo package with its own `.alint.yml`) keeps reading the repository's ignore files
   between it and the repository's top (`parents(true)`).
3. **Outside a repository, the root is the boundary.** The `.gitignore` and `.ignore` files in
   the root and below it apply without git, as documented since v0.1, and no directory above
   the root is read (`parents(false)`).
4. **`.ignore` keeps ripgrep's semantics.** Inside a repository, `.ignore` files apply from
   every directory above the entry, across repository boundaries, as they do for ripgrep.
   `ignore` 0.4.34 offers no way to bound them separately, and this decision does not widen
   them.
5. **The global excludes file still applies,** as git applies it.
6. **`fix --dry-run` predicts the same walk.** `would_walk_skip` mirrors these rules, so a dry
   run and a real `fix` agree on whether a created file is linted.

## Consequences

- A repository's lint result no longer depends on `.gitignore` or `info/exclude` files outside
  it, and "is this path ignored?" has the answer `git check-ignore` gives inside the repository.
- A nested repository or submodule no longer inherits the outer repository's `.gitignore`: with
  `*.log` in the outer file, `vendor/lib/x.log` in a nested `vendor/lib` repository is now
  linted unless `vendor/lib`'s own ignore files exclude it. git treats `vendor/lib` as a
  separate repository too.
- A tree outside any repository no longer honors ignore files above its root. Below its root
  nothing is a boundary: `ignore` reads repository markers only under `require_git`, so the
  root's `.gitignore` still applies inside a repository nested below it.
- A `.ignore` file above the repository can still hide files from every rule. It is rare outside
  ripgrep users' setups, and a warning when the walk indexes nothing is a follow-up.
- The walk start costs a listing of the root and up to two `stat` calls per directory above it.

## Considered Options

- Keep `require_git(false)`: rejected, it is the silent all-pass.
- `parents(false)` everywhere: a monorepo package linted from its own directory would lose the
  repository's `.gitignore` and lint `node_modules/` and build output.
- `require_git(true)` everywhere: a tree without git would lose its own `.gitignore` files,
  against the documented behavior.
- Walk from the repository's top and prune to the root, or match ignore files without `ignore`'s
  per-directory chain, so `.ignore` could be bounded too: a much larger change to the walker's
  core for a rare case. Deferred.

## More Information

- Design: [`ignore-files-repository-boundary.md`](../design/ignore-files-repository-boundary.md).
- Implementation: `build_walk_builder`, `in_repository` and `would_walk_skip` in
  `crates/alint-core/src/walker.rs`; tests in `crates/alint-core/src/walker_tests.rs` and
  `crates/alint/tests/walk_boundary.rs`.
