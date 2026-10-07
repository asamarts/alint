---
title: 'ordered_block'
description: 'alint ordered_block rule (cross-file): The lines between a start / end marker pair must stay sorted (and, with unique: true, free of duplicates) under...'
sidebar:
  order: 7
categories: ['cross-file', 'text-hygiene']
---

The lines between a `start` / `end` marker pair must stay sorted (and, with `unique: true`, free of duplicates) under `comparator` (`lexical` / `lexical-ci` / `numeric`). **Both markers are optional**: omit `end` to sort from `start` to EOF, omit both to sort the whole file (the markerless "this file is one sorted list" form — dictionaries, allow-lists, a fully-sorted `CODEOWNERS`). The generic form of per-project keep-sorted scripts (protobuf `failure_lists`, sorted `.gitignore` / `CODEOWNERS` / dependency lists). Per-file: with markers, a file with no `start` marker is silently fine; markers match the trimmed line; blank lines inside a block are ignored; one violation per out-of-order block; a fully-delimited block that never sees its `end` is reported `unclosed` (a block with an absent `end` runs to EOF by design). An optional `select:` regex restricts the sortable entries to lines matching it — other lines inside the block (comments, group headers) pass through untouched (the sectioned / keep-sorted-subset shape).

Optional `require:` — a list of exact lines the block must CONTAIN (in addition to being sorted), for the "a managed sorted list must have these entries" shape (a `CODEOWNERS`, an allow-list). Currently markerless-only (omit `start`/`end`; a marker makes the insert target ambiguous). A missing one is a separate, independently-fixable finding. Presence is exact-string (trimmed), independent of `comparator` — under `lexical-ci`, requiring `bravo` is not satisfied by an existing `Bravo` (the case-variant is inserted). A required line that could never round-trip to an entry is refused at load: one that carries an embedded line break, or (with `select:` set) one that does not itself match `select:`.

Fix: `sort` — reorder each block's entries under the rule's `comparator` (dropping duplicates when `unique`), reusing the rule's own `start` / `end` / `comparator` / `unique` / `select` so the fix reorders exactly what the check flags; markers, blank lines, and `select`-excluded lines stay in place, and every line keeps its terminator (LF vs CRLF) and the file its trailing-newline state. `Safe` by default (a keep-sorted block is order-independent). The `unclosed` finding is not sort-fixable — `sort` cannot invent a missing `end` marker — so it is reported but not advertised as auto-fixable.

Fix: `insert_line` — splice a missing `require:` line at its SORTED position (using the `comparator`), preserving every other line's terminator and the file's trailing-newline state. The differentiator over `file_append` (which only appends at EOF), for order-sensitive lists. **`Unsafe` by default**: it adds ruleset-authored content at a COMPUTED position, and position is load-bearing in exactly the order-sensitive formats it targets (a `.gitignore` negation only works AFTER the pattern it re-includes), so a mis-placed insert can silently change file meaning. A bare `fix` therefore SUGGESTS it; `--unsafe-fixes`, or a per-rule `applicability: safe` for an order-tolerant list such as `CODEOWNERS`, applies it. Also demoted to a suggestion from an untrusted remote `extends:` (the `require:` lines are ruleset-authored). It fixes only the missing-required-line findings — an out-of-order entry is `sort`'s job, so pair two rules (`sort` + `insert_line`) for a fully-managed list. With no fix declared, violations are unfixable.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `comparator` | one of `lexical` \| `lexical-ci` \| `numeric` |  | `lexical` | Comparator used to order entries: lexical (default), lexical-ci, or numeric. |
| `end` | string |  | `null` | Marker line closing a block. Optional - omit to run the block to EOF. |
| `require` | list of string |  | `null` | Exact lines that must be PRESENT in the block (in addition to it being sorted). A missing one is repaired by the `insert_line` fix, which splices it at its sorted position. Currently supported only for a MARKERLESS rule (the whole file is one sorted list -- the `CODEOWNERS` / allow-list shape). Presence is EXACT-string (trimmed), independent of `comparator`: under `lexical-ci`, requiring `bravo` is NOT satisfied by an existing `Bravo` (the distinct case-variant is inserted) -- a required line is inserted verbatim, so it must match itself exactly. For the same reason, with `select:` set every required line must itself match `select:` (else the inserted line could never be recognized as an entry); a non-matching line is rejected at load. |
| `select` | string |  | `null` | Regex; when set, only lines inside a block matching it are sortable entries (others, such as comments or group headers, pass through). The sectioned / keep-sorted-subset shape. |
| `start` | string |  | `null` | Marker line opening a block (matched on the trimmed line). Optional - omit to anchor the block at the start of the file. |
| `unique` | boolean |  | `false` | When true, also forbid duplicate (equal) entries within a block. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A keep-sorted block left out of order

The rule fires on this repository:

```text
.gitignore
```

```text title=".gitignore"
/target
# keep-sorted start
*.log
node_modules/
*.tmp
# keep-sorted end
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: keep-sorted
    kind: ordered_block
    paths: "**/.gitignore"
    start: "# keep-sorted start"
    end: "# keep-sorted end"
    comparator: lexical
    level: warning
```

`alint check` reports:

```ansi
[2m--- .gitignore -----------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mkeep-sorted[0m
              [2m5:1[0m  ordered_block (start at line 2): "*.tmp" is out of order (it comes
              after "node_modules/")

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### A keep-sorted block in lexical order

This repository is compliant:

```text
.gitignore
```

```text title=".gitignore"
/target
# keep-sorted start
*.log
*.tmp
node_modules/
# keep-sorted end
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: keep-sorted
    kind: ordered_block
    paths: "**/.gitignore"
    start: "# keep-sorted start"
    end: "# keep-sorted end"
    comparator: lexical
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

