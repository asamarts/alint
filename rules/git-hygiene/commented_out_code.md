---
title: 'commented_out_code'
description: 'alint rule kind `commented_out_code` (Git hygiene family).'
sidebar:
  order: 2
---

Heuristic detector for blocks of commented-out source code (as opposed to prose comments, license headers, doc comments, or ASCII banners). For each consecutive run of comment lines (`min_lines+`), counts the fraction of non-whitespace characters that are structural punctuation strongly biased toward code (`( ) { } [ ] ; = < > & | ^`). Scores ≥ `threshold` mark the block as code-shaped.

```yaml
- id: no-commented-code
  kind: commented_out_code
  paths:
    include: ["src/**/*.{ts,tsx,js,jsx,rs,py,go,java}"]
    exclude:
      - "**/*test*/**"
      - "**/__tests__/**"
      - "**/fixtures/**"
  language: auto              # auto | rust | typescript | python | go | java | c | cpp | ruby | shell
  min_lines: 3                # consecutive comment lines required (default 3)
  threshold: 0.5              # 0.0-1.0 (default 0.5 = midpoint between obvious-prose and obvious-code)
  skip_leading_lines: 30      # skip the first N lines (license headers — default 30)
  level: warning
```

The scorer deliberately ignores identifier-token density (English prose has identifier-shaped words too) and excludes backticks / quotes (rustdoc / TSDoc prose uses backticks to delimit code references). Runs of 5+ identical characters (`============`, `----`, `####`) are dropped before scoring so ASCII-art separator banners don't flag as code.

Doc-comment blocks (`///`, `//!`, `/** */`) are skipped automatically. Files whose extension the language resolver doesn't recognise are skipped silently — pass `language:` explicitly to override the auto-detection.

Heuristic, with a non-zero false-positive surface — defaults are `warning`-level only, never `error`. Tune `threshold` per codebase: lower widens the catch (more FPs), higher narrows it. Check-only — auto-removing commented-out code is destructive.

