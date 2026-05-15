---
title: 'git_blame_age'
description: 'alint rule kind `git_blame_age` (Git hygiene family).'
sidebar:
  order: 6
---

Fire on lines matching a regex whose `git blame` author-time is older than `max_age_days`. Same regex match shape as `file_content_forbidden`, but with a per-line age gate: a TODO added yesterday passes silently; a TODO that has sat in tree for 18 months fires. Closes the gap between `level: warning` on every TODO (too noisy) and `level: off` (accepts unbounded debt accumulation).

```yaml
- id: stale-todos
  kind: git_blame_age
  paths:
    include: ["**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,rb}"]
    exclude:
      - "**/*test*/**"
      - "**/fixtures/**"
      - "vendor/**"
      - "third_party/**"
  pattern: '\b(TODO|FIXME|XXX|HACK)\b'
  max_age_days: 180
  level: warning
  message: "`{{ctx.match}}` has been here for over 180 days — resolve, convert to a tracked issue, or remove."
```

`{{ctx.match}}` substitutes the regex capture group 1 when present, otherwise the full match — useful for surfacing which marker was caught (`TODO` vs `FIXME` vs …).

Heuristic notes:

- **Formatting passes reset blame age.** `cargo fmt` / `prettier` rewrites every touched line, attributing it to the format commit rather than the original author. List the formatting-sweep commits in `.git-blame-ignore-revs` and git applies the right history automatically.
- **Vendored / imported code** carries the import commit's timestamp — exclude `vendor/`, `third_party/`, generated trees.
- **Squash-merged PRs** collapse to a single commit date, so the squash date wins over the actual edit date.
- **Performance.** `git blame` is O(file_size × commits_touching_file) per file. On large monorepos pair with `alint check --changed` so blame only runs over modified files in CI.

Outside a git repo, on untracked files, or when blame fails for any other reason, the rule silently no-ops per file. Check-only — auto-removing matched lines is destructive and pinning a line as "do nothing" doesn't help.

---

