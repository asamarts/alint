---
title: 'for_each_match'
description: 'For each line matching select (a regex), the line must satisfy the nested require: predicates. alint for_each_match rule, cross-file family.'
sidebar:
  order: 7
---

For each line matching `select` (a regex), the line must satisfy the nested `require:` predicates. The in-file line quantifier — the dual of `ordered_block`'s `select:` (where `ordered_block` *orders* selected lines, this asserts a *conjunction of predicates* over each). `require:` takes at least one of: `matches` (the line must match **all** listed regexes), `forbid` (the line must match **none**), and `equal` (the listed named `select` captures must all be **equal** — checked on **every** `select` match on the line, so a line carrying two PR links validates both). One violation per offending line; lines `select` does not match are ignored. It closes two shapes no `file_content_*` kind can: a per-line changelog grammar ("**every** `* ` entry must *also* end with a linked PR ref" — `file_content_matches` asserts existence, not a per-line conjunction) and intra-line capture equality ("the display number must equal the `/pull/` URL number" — the Rust `regex` engine is RE2: no backreferences). Per-file (the `PerFileRule` fast path).

```yaml
- id: changelog-entries-well-formed
  kind: for_each_match
  paths: ["CHANGELOG.md"]
  select: '^[*-] .*\[#(?P<disp>\d+)\]\([^)]*pull/(?P<url>\d+)\)'
  require:
    matches: ['\)\.$']            # every entry line ends with ").":
    forbid:  ['\[Fix #\d+\]']     # ...never uses the "[Fix #N]" form
    equal:   [disp, url]          # ...and its display number == its URL number
  level: warning
```

