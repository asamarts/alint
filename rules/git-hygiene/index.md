---
title: 'Git hygiene'
description: 'Rule reference: the git hygiene family.'
sidebar:
  order: 10
  label: 'Git hygiene'
---

Rule kinds in the **Git hygiene** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`no_submodules`](/docs/rules/git-hygiene/no_submodules/) — Flag the presence of `.gitmodules` at the repo root — always, regardless of `paths`.
- [`git_no_denied_paths`](/docs/rules/git-hygiene/git_no_denied_paths/) — Fire when any tracked file matches a configured glob denylist.
- [`git_commit_message`](/docs/rules/git-hygiene/git_commit_message/) — Validate HEAD's commit-message shape via regex, max-subject-length, or required-body.
