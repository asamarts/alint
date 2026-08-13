---
title: 'commented_out_code'
description: 'Heuristic detector for blocks of commented-out source code (as opposed to prose comments, license headers, doc comments, or ASCII banners).'
sidebar:
  order: 2
categories: ['git-hygiene', 'content']
---

Heuristic detector for blocks of commented-out source code (as opposed to prose comments, license headers, doc comments, or ASCII banners). For each consecutive run of comment lines (`min_lines+`), counts the fraction of non-whitespace characters that are structural punctuation strongly biased toward code (`( ) { } [ ] ; = < > & | ^`). Scores ≥ `threshold` mark the block as code-shaped.

The scorer deliberately ignores identifier-token density (English prose has identifier-shaped words too) and excludes backticks / quotes (rustdoc / TSDoc prose uses backticks to delimit code references). Runs of 5+ identical characters (`============`, `----`, `####`) are dropped before scoring so ASCII-art separator banners don't flag as code.

Doc-comment blocks (`///`, `//!`, `/** */`) are skipped automatically. Files whose extension the language resolver doesn't recognise are skipped silently — pass `language:` explicitly to override the auto-detection.

Heuristic, with a non-zero false-positive surface — defaults are `warning`-level only, never `error`. Tune `threshold` per codebase: lower widens the catch (more FPs), higher narrows it. Check-only — auto-removing commented-out code is destructive.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `language` | one of `auto` \| `rust` \| `typescript` \| `javascript` \| `python` \| `go` \| `java` \| `c` \| `cpp` \| `ruby` \| `shell` |  | `auto` | `auto` (default) infers the comment-marker set from each file's extension. Explicit override useful for embedded DSLs or cases where the extension lies. |
| `min_lines` | integer (>= 2) |  | `3` | Minimum consecutive comment-line count for a block to be considered. 1-2 line comments are almost always prose; 3+ starts looking like dead code. Default 3. |
| `skip_leading_lines` | integer (>= 0) |  | `30` | Skip blocks whose first line is at or before this line number. Default 30 - covers typical license headers without false-positive flagging them as commented-out code. |
| `threshold` | number (0..1) |  | `0.5` | Density floor for code-shapedness. Higher = stricter. Default 0.5 sits at the midpoint between obvious-prose (0.0) and obvious-code (1.0); lower it to widen the catch (more FPs), raise it to narrow. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Blocks of real code left behind as comments

The rule fires on this repository:

```text
src/
src/api.ts
```

`src/api.ts`:

```ts
// SPDX-License-Identifier: MIT
// Copyright 2026 Acme Corp
//
// This file is licensed under MIT.

export function api(input: string): string {
  // const oldRate = lookupOldRate(input);
  // if (oldRate > 0.5) { return input.toUpperCase(); }
  // log("legacy path:", oldRate);
  return input;
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-commented-code
    kind: commented_out_code
    paths: "src/**/*.{ts,tsx,js,jsx}"
    min_lines: 3
    threshold: 0.5
    skip_leading_lines: 5
    level: warning
```

`alint check` reports:

```ansi
[2m--- src/api.ts -----------------------------------------------------------------[0m
  [1m[33m!  warning[0m  [2mno-commented-code[0m
              [2m7:1[0m  block of 3 commented-out lines (density 0.72); remove or convert
              to runtime-checked branch

[2mSummary (1 violation):[0m
  [1m[33m! 1 warning[0m
  0 passing [2m*[0m 1 failing
```

### Only prose comments, license headers, and doc comments

This repository is compliant:

```text
src/
src/api.ts
```

`src/api.ts`:

```ts
// SPDX-License-Identifier: MIT
// Copyright 2026 Acme Corp
//
// This file is licensed under MIT.

// ============================================
// Section: Public API
// ============================================

// This module exports the api() function.
// It accepts a string input and returns a string.
// Validation happens at the boundary; see input-types.

export function api(input: string): string {
  /// This rustdoc-style comment block is fine.
  /// Documentation, not commented-out code.
  /// Should not fire even though it's three lines.
  return input;
}
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: no-commented-code
    kind: commented_out_code
    paths: "src/**/*.{ts,tsx,js,jsx}"
    min_lines: 3
    threshold: 0.5
    skip_leading_lines: 5
    level: warning
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

