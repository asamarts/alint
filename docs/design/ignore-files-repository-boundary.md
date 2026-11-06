# Design doc: ignore files stop at the repository boundary

Status: Implemented (see the pull request that adds this document).
Decisions: [ADR-0028](../adr/0028-ignore-files-stop-at-the-repository-boundary.md), corrected by
[ADR-0029](../adr/0029-ignore-file-boundary-corrections-after-review.md) (section 8).

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
| `.gitignore`, `info/exclude` above the root | apply up to and including the repository's top | do not apply |
| `.ignore` in the root and below | apply | apply |
| `.ignore` above the root | apply (ripgrep semantics) | do not apply |
| global excludes file | applies | applies |
| config `ignore:` | applies first | applies first |

In a repository this is `require_git(true)` with `parents(true)`; outside one,
`require_git(false)` with `parents(false)`. The nearest match of each kind still wins, and the
kinds still rank `.ignore`, `.gitignore`, `info/exclude`, then global. "Do not apply" is not "not
read": `ignore` reads the ignore files of every directory above the root either way (section 8).

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
listing of the root, one canonicalization and up to two `stat` calls per directory above it, on
top of the up to three files per directory above the root `ignore` opens anyway),
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

The first two unit tests and both CLI tests fail on the previous walker. The ADR-0029 tests are
listed in section 8.

## 7. Open questions

- **Bounding `.ignore`.** `ignore` 0.4.34 reads `.ignore` files from every directory above the
  root and offers no option to stop them at the repository. Bounding them would mean walking
  from the repository's top and pruning to the root, or matching ignore files outside
  `ignore`'s per-directory chain. Deferred: `.ignore` files outside a repository are rare.
- **Warning on an empty walk.** A warning when ignore files exclude every entry under the root
  would catch the remaining silent passes (a parent `.ignore`, or a repository whose own
  `.gitignore` is `*`). Follow-up. A bad ignore file above the root, now skipped silently
  (section 8), could be reported through the same channel.
- **Finding the git directory and global excludes as git does.** Section 8 lists the three
  places the walk follows `ignore` rather than git. Each needs its own resolution of the git
  directory (`gitdir:` without `commondir`, relative to the `.git` file) or of the global
  excludes file (anchored at the repository's top, no `~/.gitconfig` fallback under
  `GIT_CONFIG_GLOBAL`), applied outside `ignore`'s matcher. Follow-up.

## 8. Review follow-ups (ADR-0029)

A review of the implementation against git (real repositories, submodules, worktrees, and a
fuzz harness comparing `would_walk_skip` with the walk) found no regression. It found where this
document and ADR-0028 claimed more than the walk does, and more dry-run disagreements.

**What `ignore` reads above the root.** `Ignore::add_parents` reads the `.gitignore`, `.ignore`
and `info/exclude` of every directory above the root whenever git ignore files are on, before
the walk, whatever `parents` and `require_git` say; those decide only what applies. Its errors
(a bad line, or a `.git` file it cannot read) reach the visitor as a walk error naming the file
(several make a `Partial`), with no depth, where an ignore file in the tree only marks its
directory's entry. So a bad line in a home directory's `.gitignore` ended every run below it
with exit 2. `is_ignore_file_error` now skips a walk error whose every part names a path outside
the root (every entry the walk yields is under the root), and the walk goes on with the file's
good lines, as git does with a pattern it cannot parse. An error from the walk itself names an
entry under the root and is never skipped this way, so a directory in the tree that cannot be
listed still stops the run. The path decides, not the depth: most walk errors carry one, but a
failed stat on a followed symlink arrives as a path-tagged error around it.

**Where the walk differs from git.** These come from `ignore` 0.4.34, predate ADR-0028, and are
documented rather than fixed:

- `info/exclude` is found through a `.git` directory, or a linked worktree's `.git` file whose
  git directory has a `commondir` file. A submodule (`.git/modules/<name>`) and a
  `--separate-git-dir` repository have no `commondir`, so their exclude file is not read; a
  worktree made with `--relative-paths` resolves its `gitdir:` against the process's working
  directory. `git_exclude_file` mirrors this so the dry run agrees.
- The global excludes file is matched from the process's working directory, so its anchored
  patterns are anchored there and not at the repository's top. With `GIT_CONFIG_GLOBAL` set to
  a file without `core.excludesFile`, `ignore` still falls back to `~/.gitconfig`.
- A root inside an ignored directory is linted: `ignore` matches entries below the root only.

**Dry-run predictions.** `predict_unresolved_creates` now:

- skips the walk check for a create the rule looks for on disk itself
  (`Rule::sees_path_past_walk`, implemented by `file_exists` for a literal path with
  `respect_gitignore: false`), checking only that the path does not resolve out of the root, as
  the rule's stat does;
- predicts a create unresolved for a rule with a `git_tracked_only` mode unless `git ls-files`
  lists the path, running it once and only when such a rule creates a file;
- reads each created file and its content from `Fixer::created_file(violation, root,
  allow_out_of_root)`, resolved as `apply` resolves them under the rule's permission (`fix_edit`
  is the editor form, which never leaves the root for the target or a `content_from`), so a
  target outside the root and an ignore file whose content comes from outside it are predicted
  like the real run; `file_create` and `create_and_register` (a missing member) implement it;
- judges a create by two names (`created_names`): the path as written, when it reads as one
  inside the root (an absolute path stripped of the root as given or of its canonical form),
  and where the file lands once symlinks resolve. It is resolved when the rule sees it under
  either, and outside the root when it lands there, so a create through `sub/up -> ..`,
  through an ignored symlinked directory, or by an absolute or `..` path that leads back into
  the root is predicted applied, as the real run finds it. A rule without a `paths:` scope
  (`dir_exists`, `cross_file`) looks for the path as written, normalized as it normalizes it,
  so only that name counts, and a member written out of the root never matches (follow-up
  below);
- keeps the first create of each ignore file, keyed by its resolved path, since the real fix
  skips a file that is already there; and `DirIgnores::with_created` compares directories
  resolved (`pathsafe::resolve_existing_prefix`), so an ignore file created through an in-tree
  symlinked directory applies under every name the walk lists it by;
- checks a created directory against the rule's `paths:` as it does a file. `dir_create` now
  requires a normal-form directory (`single_literal_dir`), so its target always matches.

**`dir_create` in normal form.** The rule matches `paths` against the walk's `docs/adr`, so
`docs/`, `./docs`, `docs//adr`, `docs/./adr` and `/docs` never passed `check`, and `dir_create`
could not converge. `dir_exists` now refuses them with `dir_create` when the configuration loads.

**Tests.** `walker_tests.rs`: `a_bad_ignore_file_above_the_root_does_not_stop_the_walk`,
`an_unreadable_file_above_the_root_does_not_stop_the_walk`, the control
`an_unreadable_directory_still_stops_the_walk`, and
`would_walk_skip_reads_an_ignore_file_created_through_a_symlink`. `engine/tests/fix.rs`:
`dry_run_takes_the_first_of_two_creates_of_one_ignore_file`,
`dry_run_reads_an_absolute_create_inside_the_root_as_relative`,
`dry_run_trusts_a_rule_that_looks_past_the_walk` and
`dry_run_resolves_a_tracked_only_rule_only_with_a_tracked_path`; its mock fixer now declines an
editor form outside the root, as `file_create` does. `crates/alint/tests/fix_flags.rs`: a rule
that looks past the walk, a create outside the root and below a symlink out of it, an ignore file
created through a symlink with two creates of one file, and `git_tracked_only`.
`crates/alint/tests/walk_boundary.rs`: `a_bad_ignore_file_above_the_root_does_not_stop_the_run`.
`dir_exists.rs`: `build_rejects_dir_create_on_a_directory_not_in_normal_form`. Both CLI test
files now point git's global and system config, `XDG_CONFIG_HOME` and `HOME` at an empty
directory, so a developer's global excludes file (`.env` is a common entry) cannot change their
results. Each new test except the control fails on the walker and fixer before this change.

**Second review (2026-11-05).** A differential review of `fix --dry-run` against `fix` on
adversarial layouts found the cases the two bullets above now cover: a created `.gitignore` whose
`content_from` lies outside the root was missed (the editor form declined it, so no content was
known), a `create_and_register` member outside the root had no create to check, and a create was
judged only by its written path, so one through an in-tree symlink, or an absolute or `..` path
landing inside the root, was predicted unresolved although the real run resolves it. Tests:
`engine/tests/fix.rs` `dry_run_judges_a_create_by_where_it_lands` and
`dry_run_names_an_absolute_create_through_the_canonical_root`, `fix_flags.rs`
`dry_run_agrees_with_fix_for_an_ignore_file_read_from_outside_the_root` and
`dry_run_agrees_with_fix_for_a_member_created_outside_the_root`, and unit tests for both fixers'
`created_file`. `fix --diff` does not make the prediction; its help no longer claims its exit
code matches a real `fix`.

**Third review (after v0.21.1).** The second review's "resolved under either name" was right only
for a rule with a `paths:` scope. `dir_exists` and `cross_file` have none, so the landed name was
accepted unchecked: a `dir_create` through an ignored symlinked directory or through
`deep/x/up -> ..`, and a `create_and_register` member below an ignored alias, were predicted
applied while the real run left them unresolved. Such a rule now counts only the path as written,
normalized as the rule normalizes it, and a member written out of the root (absolute, or `..`
back in) never matches, as `cross_file`'s `member_exists` finds. `pathsafe::resolve_existing_prefix`
now lets a `..` cancel a directory that does not exist yet (`../repo/newdir/../x.txt` under
`allow_out_of_root` and `create_parents`; a path that stays inside is normalized lexically before
it gets there), where it returned `None` and the create read as outside the root. `fix --diff`
also hides a create outside the root from a rule with redacted findings: it found the hidden files
through the editor form, which never leaves the root, and now adds `Fixer::created_file` and
`created_dir`. Limits left documented in `cli/fix.md`: an ignore file created outside the root,
a created `.git/info/exclude` or repository marker, `root_only`, other names the walk lists a file
under through in-tree symlinks, and case differences on a case-insensitive filesystem. Tests:
`fix_flags.rs` `dry_run_agrees_with_fix_for_a_rule_without_a_paths_scope_through_a_symlink`,
`dry_run_agrees_with_fix_for_a_member_written_back_into_the_root` and
`dry_run_agrees_with_fix_for_a_create_through_a_directory_it_makes`; `evidence_redaction.rs`
`fix_diff_omits_a_sensitive_create_outside_the_root`; `pathsafe.rs`
`a_dotdot_after_a_missing_directory_cancels_it` and
`a_leftover_dotdot_steps_up_from_where_a_symlink_lands`.

**Fourth review (after the third).** `resolve_existing_prefix` walked backwards from the deepest
ancestor that exists, so once a `..` had cancelled a missing directory, every component after it
was re-attached lexically: `new/../alias/x.txt` stayed under `alias` rather than following the
symlink to `real`, and `new/../link/../x.txt` stepped back to the root rather than up from where
`link` lands. It now walks forward as the OS does: each existing component resolves against the
real location so far, a missing one starts a pending tail in which a `..` cancels the last name,
and once the tail empties the next component may exist again. `created_names` also normalized an
absolute path lexically after stripping the root, cancelling a `..` against a symlink before it,
where the OS resolves the symlink first; an absolute path with a `..` is now resolved as written.
A `file_create` with `create_parents: false` into a missing directory was predicted applied (and
shown by `fix --diff`) while `fix` failed on the write; the fixer now declines it in every mode.
Tests: `pathsafe.rs` `a_symlink_after_a_cancelled_missing_directory_still_resolves`;
`engine/tests/fix.rs` `dry_run_judges_a_create_by_where_it_lands` (`absolute-link-up`,
`back-in-alias`, `back-in-link-up`); `creators.rs`
`file_create_without_create_parents_declines_a_missing_directory_in_every_mode`.
