---
title: The walker and `.gitignore`
description: How alint discovers files, what `.gitignore` filters out by default, and how rules like `file_absent` / `dir_absent` interpret git state.
sidebar:
  order: 5
---

Every alint run starts the same way: walk the repo, build an in-memory index of files, then evaluate rules against that index. The walker is a thin wrapper around the [`ignore`](https://docs.rs/ignore/) crate (the same crate that powers `ripgrep`), and its filtering behaviour is the most common source of confusion when a rule "doesn't fire when I expected it to."

## What the walker sees by default

Starting at the path you pass to `alint check` (or the current directory), the walker yields every regular file under that root, **except** for paths matched by any of the following:

- The repo's `.gitignore` files (root and per-directory)
- `.git/info/exclude`
- Your global gitignore (`~/.config/git/ignore`, or whatever `core.excludesFile` points at)
- `.ignore` files (the `ignore` crate's own convention; same syntax as `.gitignore`)
- The `.git/` directory itself
- Anything added under the config's `ignore:` field (see below)

Hidden files (those starting with `.`) **are** included — alint walks `.github/`, `.editorconfig`, `.cargo/`, etc. by default. Symlinks are followed.

The walker does *not* require a git repo to function. Rules run identically on a plain directory, a tarball extraction, or a fresh git clone — the only difference is that without `.gitignore` files, no paths get filtered out.

## The `respect_gitignore` config field

The default is the equivalent of:

```yaml
version: 1
respect_gitignore: true   # the default
```

Set it to `false` to disable every gitignore source above (per-directory, root, info/exclude, global, `.ignore`):

```yaml
version: 1
respect_gitignore: false
```

The CLI's `--no-gitignore` flag overrides whatever's in config to `false` for one invocation. Useful when you want to lint files that *would* be committed if `.gitignore` weren't there — e.g. for a one-off audit of a build directory.

## The `ignore:` config field

`ignore:` adds patterns *on top of* whatever `.gitignore` already excludes. Same gitignore-style syntax. Use it for repo-specific exclusions you don't want to put in `.gitignore` itself (because they're an alint thing, not a git thing):

```yaml
version: 1
ignore:
  - "vendor/**"
  - "**/*.snapshot.json"
  - "fixtures/golden/**"
```

These patterns are excluded *regardless* of `respect_gitignore`. Setting `respect_gitignore: false` disables `.gitignore`-sourced filters but leaves `ignore:` filters in place.

## How this affects rules

Every rule sees a **pre-filtered file index**. If a path was excluded by the walker, no rule can act on it — they don't get a chance.

For most rules — `file_exists`, `file_content_matches`, `filename_case`, `for_each_dir` — this is exactly what you want. You don't care about gitignored caches, you care about the files git would actually track.

For **absence-style rules** (`file_absent`, `dir_absent`, `no_*` rules), the implication is sharper:

> A `dir_absent` rule with `paths: "**/target"` fires whenever `target/` exists in the walked tree. If `target/` is in `.gitignore`, the walker filters it out, and the rule never sees it — no violation, even if `target/` is sitting on disk full of build artefacts.

That's the intent. When your `.gitignore` is correct, build artefacts are invisible to alint, and the rule effectively means "this directory wouldn't be committed." When `.gitignore` is missing or wrong, the directory becomes visible, the rule fires, and you've caught a hygiene gap.

The rule's name often reads as "no committed `target/`" — that's a useful mental model, but the actual implementation is **"no un-ignored `target/`"**. The two coincide in well-configured repos. They diverge in the edge cases below.

## What this is *not*: a check against git's index

alint doesn't read `.git/index` and doesn't shell out to `git ls-files`. The walker observes the filesystem; `.gitignore` is a coarse approximation of "what would be committed." Two cases where this approximation drifts:

- **Tracked-then-gitignored files.** `.gitignore` only affects *untracked* files. If a file was added to git first and then later listed in `.gitignore`, git still tracks it on every commit — but alint's walker filters it out, so absence-style rules don't fire and content rules don't inspect it. `git ls-files <path>` would still report the file.
- **`git add -f`'d files.** Adding a file with `--force` overrides `.gitignore`. The file is in git's index, but alint's walker still filters it out by the matching gitignore entry.

In a healthy repo neither case is common. If you suspect either, `git ls-files <path>` is the authoritative answer.

## When to use `respect_gitignore: false`

Rare, but legitimate cases:

- **Auditing a CI runner's working tree** where build outputs accumulated and you want to enforce content rules on everything, including gitignored caches.
- **Linting a directory that isn't a git repo** but happens to contain a stray `.gitignore` you don't want to honour.
- **Running absence-style rules deliberately on the full disk state**, e.g. as a pre-package check that "no `.env` is sitting in this directory regardless of `.gitignore`."

Don't reach for `--no-gitignore` casually. With it on, every `dir_absent` / `file_absent` rule fires on any developer who has built locally — `target/`, `node_modules/`, `__pycache__/`, `.next/` all become violations. That's almost never what you want during normal development.

## The git-aware future

The current model — observe the filesystem, filter by `.gitignore` — is fast, simple, and works on non-git directories. Its blind spot is the index/working-tree distinction.

[ROADMAP.md](https://github.com/asamarts/alint/blob/main/docs/design/ROADMAP.md) under v0.5 lists *git-aware primitives* as a planned addition: `git_tracked_only` (a scope modifier that filters to files actually in git's index), `git_no_denied_paths`, and `git_commit_message`. When those land, you'll be able to write rules like:

```yaml
# (planned for v0.5 — not yet shipped)
- id: target-not-tracked
  kind: dir_absent
  paths: "**/target"
  git_tracked_only: true
  level: error
```

…which would fire only when `target/` is actually in git's index, regardless of `.gitignore` state. Until then, the gitignore-based approximation is the supported path.
