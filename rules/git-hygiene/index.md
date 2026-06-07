---
title: 'Git hygiene'
description: 'Rule reference: the git hygiene family.'
sidebar:
  order: 11
  label: 'Git hygiene'
---

Rule kinds in the **Git hygiene** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`no_submodules`](/docs/rules/git-hygiene/no_submodules/) — Flag the presence of `.gitmodules` at the repo root — always, regardless of `paths`.
- [`commented_out_code`](/docs/rules/git-hygiene/commented_out_code/) — Heuristic detector for blocks of commented-out source code (as opposed to prose comments, license headers, doc comments, or ASCII banners).
- [`markdown_paths_resolve`](/docs/rules/git-hygiene/markdown_paths_resolve/) — Validate that backticked workspace paths in markdown files resolve to real files or directories in the repo.
- [`git_no_denied_paths`](/docs/rules/git-hygiene/git_no_denied_paths/) — Fire when any tracked file matches a configured glob denylist.
- [`git_commit_message`](/docs/rules/git-hygiene/git_commit_message/) — Validate commit-message shape via regex, max-subject-length, or required-body.
- [`git_commit_signed_off`](/docs/rules/git-hygiene/git_commit_signed_off/) — Assert every commit in scope carries a DCO (Developer Certificate of Origin) `Signed-off-by:` trailer — required by every CNCF / Linux Foundation / kernel-style project.
- [`git_commit_no_fixup`](/docs/rules/git-hygiene/git_commit_no_fixup/) — Fail on residual `fixup!` / `squash!` / `amend!` commits left in scope — the ones `git commit --fixup` / `--squash` produce, meant to be collapsed by `git rebase --autosquash` before merging.
- [`git_commit_subject_matches`](/docs/rules/git-hygiene/git_commit_subject_matches/) — Each commit's subject line (the first line of its message) must match the `matches:` regex — the subject-grammar member of the commit family.
- [`git_commit_author_allowlist`](/docs/rules/git-hygiene/git_commit_author_allowlist/) — Assert every commit author in scope matches an allowed email and/or name pattern.
- [`git_commit_gpg_signed`](/docs/rules/git-hygiene/git_commit_gpg_signed/) — Assert every commit in scope has a verifying signature (`git verify-commit` exits 0).
- [`git_blame_age`](/docs/rules/git-hygiene/git_blame_age/) — Fire on lines matching a regex whose `git blame` author-time is older than `max_age_days`.
- [`changeset_requires_path`](/docs/rules/git-hygiene/changeset_requires_path/) — The `<since>...HEAD` diff must **add** (git status `A`) at least one path matching `add_glob:` — the "did you add a changelog entry?" gate.
- [`pair_changed_together`](/docs/rules/git-hygiene/pair_changed_together/) — If the `<since>...HEAD` diff changes any path matching `if_changed:`, at least one path matching `then_changed:` must change in the same range — the **co-change** gate.
