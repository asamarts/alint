---
title: 'git_blame_age'
description: 'Fire on lines matching a regex whose git blame author-time is older than max_age_days. alint git_blame_age rule, git hygiene family.'
sidebar:
  order: 11
categories: ['git-hygiene']
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

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `max_age_days` | integer (>= 1) | yes |  | Minimum line age (in days) for a matching line to fire as a violation. Lines younger than this pass silently. Common values: 90 (one quarter), 180 (half a year), 365 (a full year of debt). |
| `pattern` | string | yes |  | Rust-regex pattern applied to each blame line's content. Capture group 1 (when present) is exposed as `{{ctx.match}}` in the message template; without a capture group, `{{ctx.match}}` substitutes the full match. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.
