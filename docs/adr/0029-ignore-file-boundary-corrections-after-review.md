---
status: accepted
date: 2026-11-04
decision-makers: asamarts
---

# 0029. Ignore-file boundary corrections after review

## Status

Accepted (2026-11-04). Corrects [ADR-0028](0028-ignore-files-stop-at-the-repository-boundary.md)
decisions 1, 3, 5 and 6 and two of its consequences; the rest of ADR-0028 stands.

## Context

A review of the ADR-0028 implementation against git found that the walk matches git on the
layouts ADR-0028 is about: a `*` `.gitignore` or an exclude file above the repository, nested
repositories, submodules, linked worktrees, a root linted from a subdirectory, `.jj`. It also
found five places where ADR-0028 says more than the walk does, and cases where `fix --dry-run`
still disagreed with the real `fix`. Every finding reproduces on the walker before ADR-0028, so
none is a regression. Accepted ADRs are immutable, so the corrections are recorded here.

1. `ignore` 0.4.34 reads the `.gitignore`, `.ignore` and `info/exclude` files of every directory
   above the root before the walk starts, whether or not it applies them (`Ignore::add_parents`
   runs whenever git ignore files are on). ADR-0028 decision 3 says no directory above the root
   is read: the files are read and not applied. A line `ignore` cannot parse in one of them
   (`a\`, `[z-a]`, `x{`), or a `.git` file there it cannot read, was a walk error that ended
   the run with exit 2, while the same line in the tree's own `.gitignore` is skipped.
2. `info/exclude` is read where `ignore` finds it: in a `.git` directory, or through a linked
   worktree's `.git` file whose git directory has a `commondir` file. A submodule's git
   directory (`.git/modules/<name>`) and a `--separate-git-dir` repository have none, so their
   exclude file is not read, and a worktree made with `--relative-paths` resolves its `gitdir:`
   against the directory alint runs in. ADR-0028 decision 1 names the submodule's `.git` file
   as a repository marker, which it is, but its exclude file does not apply.
3. The global excludes file is not applied as git applies it (decision 5). `ignore` anchors its
   patterns (`/build`) at the directory alint runs in, not at the repository's top, and when
   `GIT_CONFIG_GLOBAL` names a file without `core.excludesFile` it still falls back to
   `~/.gitconfig`, which git does not.
4. A root inside an ignored directory (`alint check out/sub` with `out/` ignored) is linted:
   `ignore` never matches the root or its ancestors against the ignore files. With 2 and 3, this
   limits ADR-0028's consequence that "is this path ignored?" has the answer `git check-ignore`
   gives: it holds for the paths below the root, outside those differences.
5. ADR-0028's last consequence counts only alint's own repository probe at the walk start ("up to
   two `stat` calls per directory above it"). `ignore` also opens up to three files per
   directory above the root, as it did before ADR-0028.

Decision 6 said `fix --dry-run` predicts the same walk. The review found cases where the walk
is not what decides: a rule that looks for its literal path on disk itself, a
`git_tracked_only` rule, a `file_create` target outside the root, an ignore file created
through a symlinked directory, and two creates of one ignore file.

## Decision

1. **A bad ignore file above the root does not stop the walk.** A walk error is skipped when
   every part of it names a file above the root: a pattern `ignore` could not parse there, or a
   `.git` file it could not read. git skips a pattern it cannot parse and never reads above the
   repository. The good
   lines of such a file still apply where the file applies (a `.ignore` above the repository).
   Any other walk error, such as a directory in the tree that cannot be listed, still stops the
   run.
2. **Decision 3 of ADR-0028 reads "applies" for "is read".** Outside a repository no ignore file
   above the root applies. `ignore` still reads them; decision 1 makes that harmless.
3. **Decisions 1 and 5 of ADR-0028 hold with the differences in findings 2 to 4.** They come
   from how `ignore` 0.4.34 finds the git directory and the global excludes file, and are
   documented, not changed here. Changing them means finding both the way git does, a
   follow-up.
4. **`fix --dry-run` predicts what the rule, not only the walk, decides.**
   - A rule that looks for the file on disk itself (`file_exists` with
     `respect_gitignore: false` on a literal path, the pitfall #18 pattern) is resolved by a
     create the walk leaves out, unless the path resolves out of the root
     (`Rule::sees_path_past_walk`).
   - A `git_tracked_only` rule is resolved only by a create of a path git tracks (a tracked file
     deleted from the worktree).
   - A `file_create` target outside the root, which `allow_out_of_root` permits and which has
     no editor form, is predicted unresolved (`Fixer::created_file`). An absolute target inside
     the root counts as the root-relative path.
   - An ignore file created through an in-tree symlinked directory applies where it lands,
     under every name the walk lists that directory by. Of two creates of one ignore file the
     first is written, as in the real run, which skips a file that is already there.
5. **`dir_create` requires its directory in normal form.** `paths: docs/adr`, not `docs/`,
   `./docs`, `docs//adr`, `docs/./adr` or `/docs`. The rule matches `paths` against the walk's
   normal form, so those never passed `check` even with the directory present, and the fix
   could not converge. Such a configuration now fails to load, and the message names the form.

## Consequences

- A malformed `.gitignore` in a home directory or a parent workspace no longer makes every
  repository below it fail with exit 2. The bad line is skipped without a warning, as in the
  tree's own ignore files.
- The concepts page "The walker and git" and the design document list where the walk and
  `git check-ignore` can disagree: a `.ignore` above the repository, a submodule's or separate
  git directory's exclude file, anchored global excludes, and a root inside an ignored directory.
- A `dir_create` configuration with a `paths:` not in normal form stops loading. It never worked.
- A dry run agrees with the real `fix` in more cases. A cascade through another rule, content
  the rule still rejects, and an edit to an existing ignore file remain only found by running
  `fix`.

## Considered Options

- Warn about a bad ignore file above the root instead of skipping it silently: the walker has no
  warning channel, and a bad line in the tree is already silent. It can join the empty-walk
  warning follow-up.
- Stop `ignore` from reading above the root: 0.4.34 has no option for it short of turning git
  ignore files off and matching them outside its per-directory chain, the larger change
  ADR-0028 deferred.
- Normalize `dir_create`'s `paths` instead of rejecting it: the fixer would create `docs` while
  the rule still matched `docs/` against the walk and never passed. The rule's matching would
  have to change too, for every existence kind.

## More Information

- Design: [`ignore-files-repository-boundary.md`](../design/ignore-files-repository-boundary.md)
  section 8.
- Implementation: `is_ignore_file_error` and `DirIgnores::with_created` in
  `crates/alint-core/src/walker.rs`; `predict_unresolved_creates` in
  `crates/alint-core/src/engine/fix.rs`; `Rule::sees_path_past_walk` and `Fixer::created_file`
  in `crates/alint-core/src/rule.rs`; `single_literal_dir` in
  `crates/alint-rules/src/dir_exists.rs`.
