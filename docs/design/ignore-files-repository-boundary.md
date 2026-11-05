# Design doc: ignore files stop at the repository boundary

Status: Implemented (see the pull request that adds this document).
Decisions: [ADR-0028](../adr/0028-ignore-files-stop-at-the-repository-boundary.md).

## 1. Problem

Every rule reads the walked index, so whatever the walk leaves out no rule can see. The walk
applied the `.gitignore` and `info/exclude` files of every directory above the walk root, up to
the filesystem root. A `*` in any of them hid the whole tree:

```text
~/.gitignore          *            (a home directory kept in git)
~/src/app/.git/
~/src/app/.alint.yml  file_absent: .env
~/src/app/.env
```

`alint check` in `~/src/app` indexed nothing and printed `All 1 rule(s) passed`, exit 0. git
does not apply `~/.gitignore` to the repository in `~/src/app`; `git status` there lists `.env`.
The result depended on directories outside the repository, so the same commit could pass on one
machine and fail in CI, and a silent pass is the worst way for a linter to be wrong.

The cause is `require_git(false)` in `build_walk_builder`, set in v0.1 so a tree without `.git`
still honors its `.gitignore` files. With it, `ignore` 0.4.34 never marks a directory as a
repository, and its rule "stop reading `.gitignore` files at the repository's top" never fires,
upward or at a nested repository below the root.

## 2. Surface area

No configuration changes. `respect_gitignore` and `ignore:` keep their meaning, and
`respect_gitignore: false` still turns every ignore file off. What changes is which ignore files
a walk with `respect_gitignore: true` reads.

## 3. Semantics

A directory *holds a repository* when it contains `.git` (a directory, or the file a linked
worktree or submodule has) or `.jj`, the markers `ignore` checks. The root *is in a repository*
when it or a directory above it holds one.

| Source | Root in a repository | Root outside any repository |
|---|---|---|
| `.gitignore`, `info/exclude` in the root and below | apply, up to and including the nearest directory holding a repository | apply |
| `.gitignore`, `info/exclude` above the root | apply up to and including the repository's top | not read |
| `.ignore` in the root and below | apply | apply |
| `.ignore` above the root | apply (ripgrep semantics) | not read |
| global excludes file | applies | applies |
| config `ignore:` | applies first | applies first |

In a repository this is `require_git(true)` with `parents(true)`; outside one,
`require_git(false)` with `parents(false)`. The nearest match of each kind still wins, and the
kinds still rank `.ignore`, `.gitignore`, `info/exclude`, then global.

Outside a repository nothing below the root is a boundary either: `ignore` reads repository
markers only under `require_git`, so the root's `.gitignore` also applies inside a repository
nested below it (a directory of clones linted from its top). Each nested repository linted
from its own directory gets git's semantics.

The root and the directories below it are listed, and `ignore` takes an ignore file or a
repository marker there only under its exact name (`.GITIGNORE` on a case-insensitive
filesystem is not one). Directories above the root are probed by path.

`would_walk_skip` (the `fix --dry-run` prediction) reads the same files: it skips the
directories above the root outside a repository, and stops each `.gitignore` and `info/exclude`
search after the nearest directory that holds a repository.

The 2026-11 review of this change found four more ways a dry run disagreed with the real `fix`,
fixed here too:

- A created ignore file. A `file_create` of `cache/.gitignore` with `*` leaves itself out of the
  walk after the fix. The prediction reads the ignore files the pass creates (`.gitignore`,
  `.ignore`) in their directories, so a created `.ignore` of `*.log` also predicts that another
  rule's created `x.log` stays unlinted.
- `dir_create`. The dry run checked only `file_create`; a directory created into a gitignored
  path is now predicted too (`Fixer::created_dir`).
- `..` in a `file_create` path. The fixer writes to the root joined with the lexically
  normalized path, and the prediction now checks that path, not each written component.
- Symlinks. The walk does not enter a symlink that loops back to a directory on its own path,
  is broken, or leads out of the root; a create below one is predicted unresolved.

Edits to an existing ignore file (`file_append` to `.gitignore`) stay a documented limit of the
dry run, as other cascades are.

## 4. False-positive surface

The change can only make the walk include files it used to leave out, in three layouts:

- **Above the repository.** Files hidden by an ignore file outside the repository are linted.
  This is the fix.
- **Nested repositories and submodules.** The outer repository's `.gitignore` no longer applies
  inside them. A vendored checkout with its own `.git` and no `.gitignore` of its own may now
  show build output or logs the outer file used to hide. git treats it as a separate repository
  as well; the user adds the patterns to the nested repository's ignore files or to the
  config's `ignore:`. The CHANGELOG entry says so.
- **Outside any repository.** A tree with no `.git` above it no longer reads ignore files above
  its root. alint is almost always run inside a repository (CI checkouts included), and a tree
  that is not one is usually linted from its top.

A `.ignore` above the repository still applies, so it can still hide files (section 7).

## 5. Implementation notes

`crates/alint-core/src/walker.rs`: `holds_repository(dir, names)` and `in_repository(root)` (a
listing of the root, one canonicalization and up to two `stat` calls per directory above it),
used by `build_walk_builder` to set `require_git` and `parents`, and by `would_walk_skip`, whose
per-directory `DirIgnores` records whether the directory holds a repository and reads a listed
directory's files by exact name. `would_walk_skip` takes the ignore files a fix creates and
checks symlinks on the way (`symlink_stops_the_walk`). `crates/alint-core/src/engine/fix.rs`
(`predict_unresolved_creates`) gathers the pass's creates, `file_create` and `dir_create`, first.
No new dependency.

## 6. Tests

- `walker_tests.rs`: `ignore_files_stop_at_the_repository_boundary` (a `*` `.gitignore` and an
  exclude file in a home repository above the root; a nested repository and a submodule `.git`
  file; a root linted from a subdirectory), `ignore_files_outside_a_repository_stop_at_the_root`
  (a tree with no repository, and a `.jj` repository), and
  `a_dot_ignore_above_the_repository_still_applies`. Each compares `would_walk_skip` with the
  walk on every entry and checks the expected paths.
- `crates/alint/tests/walk_boundary.rs`: `alint check` reports a `file_absent` violation that a
  `*` `.gitignore` above the repository, or above a tree outside any repository, used to hide.
- Review follow-ups. `walker_tests.rs`: `a_nearer_ignore_file_above_the_root_outranks_a_farther_one`,
  `ignore_files_above_the_root_match_the_canonical_root`, `ignore_files_count_only_under_their_exact_name`
  (meaningful on a case-insensitive filesystem), `a_repository_marker_counts_only_under_its_exact_name_when_listed`,
  `would_walk_skip_reads_the_ignore_files_a_fix_creates` and
  `would_walk_skip_stops_at_a_symlink_the_walk_does_not_enter`. `engine/tests/fix.rs`:
  `dry_run_predicts_each_create_the_real_fix_finds_unresolved` runs a dry run and a real fix on
  the same tree and compares each rule's outcome. `crates/alint/tests/fix_flags.rs` does the same
  through the CLI with the shipped fixers, below a symlink loop, and with an `info/exclude` that
  outranks the global excludes file.
- The expected values in the walker tests assume no global excludes file and a temporary
  directory outside any repository and `.ignore`; elsewhere only agreement with the walk is
  checked (`neutral_environment`).

The first two unit tests and both CLI tests fail on the previous walker.

## 7. Open questions

- **Bounding `.ignore`.** `ignore` 0.4.34 reads `.ignore` files from every directory above the
  root and offers no option to stop them at the repository. Bounding them would mean walking
  from the repository's top and pruning to the root, or matching ignore files outside
  `ignore`'s per-directory chain. Deferred: `.ignore` files outside a repository are rare.
- **Warning on an empty walk.** A warning when ignore files exclude every entry under the root
  would catch the remaining silent passes (a parent `.ignore`, or a repository whose own
  `.gitignore` is `*`). Follow-up.
