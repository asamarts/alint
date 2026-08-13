---
title: 'for_each_match'
description: 'For each line matching select (a regex), the line must satisfy the nested require: predicates. alint for_each_match rule, cross-file family.'
sidebar:
  order: 7
categories: ['cross-file']
---

For each line matching `select` (a regex), the line must satisfy the nested `require:` predicates. The in-file line quantifier — the dual of `ordered_block`'s `select:` (where `ordered_block` *orders* selected lines, this asserts a *conjunction of predicates* over each). `require:` takes at least one of: `matches` (the line must match **all** listed regexes), `forbid` (the line must match **none**), and `equal` (the listed named `select` captures must all be **equal** — checked on **every** `select` match on the line, so a line carrying two PR links validates both). One violation per offending line; lines `select` does not match are ignored. It closes two shapes no `file_content_*` kind can: a per-line changelog grammar ("**every** `* ` entry must *also* end with a linked PR ref" — `file_content_matches` asserts existence, not a per-line conjunction) and intra-line capture equality ("the display number must equal the `/pull/` URL number" — the Rust `regex` engine is RE2: no backreferences). Per-file (the `PerFileRule` fast path).

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `require` | object | yes |  | Predicates applied to each selected line; at least one of `matches` / `forbid` / `equal` is required. |
| `select` | string | yes |  | Regex; a line is a checked element iff it matches. Named captures are available to `require.equal`. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A changelog line that breaks the entry grammar

The rule fires on this repository:

```text
CHANGELOG.md
```

`CHANGELOG.md`:

```markdown
- Add a feature ([#12](https://github.com/x/pull/12)).
- Typo fix ([#5](https://github.com/x/pull/9)).
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: changelog-entries-well-formed
    kind: for_each_match
    paths: ["CHANGELOG.md"]
    select: '^- .*\[#(?P<disp>\d+)\]\([^)]*pull/(?P<url>\d+)\)'
    require:
      matches: ['\)\.$']
      equal: [disp, url]
    level: error
```

### Every changelog entry matches the required grammar

This repository is compliant:

```text
CHANGELOG.md
```

`CHANGELOG.md`:

```markdown
- Add a feature ([#12](https://github.com/x/pull/12)).
- Fix a bug ([#34](https://github.com/x/pull/34)).
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: changelog-entries-well-formed
    kind: for_each_match
    paths: ["CHANGELOG.md"]
    select: '^- .*\[#(?P<disp>\d+)\]\([^)]*pull/(?P<url>\d+)\)'
    require:
      matches: ['\)\.$']
      equal: [disp, url]
    level: error
```

