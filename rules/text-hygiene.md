---
title: 'Text hygiene'
description: 'Rule reference: the text hygiene family.'
sidebar:
  order: 6
---

### `no_trailing_whitespace`

No line may end with space or tab.

```yaml
- id: rust-no-trailing-ws
  kind: no_trailing_whitespace
  paths: "crates/**/src/**/*.rs"
  level: warning
  fix:
    file_trim_trailing_whitespace: {}
```

### `final_newline`

File must end with a single `\n`. Fixable via `file_append_final_newline`.

### `line_endings`

Every line ending matches `target`: `lf` or `crlf`. Mixed endings in a single file fail.

```yaml
- id: lf-only
  kind: line_endings
  paths: ["**/*.rs", "**/*.md"]
  target: lf
  level: warning
  fix:
    file_normalize_line_endings: {}
```

### `line_max_width`

Cap line length in characters (not bytes — code points). Optional `tab_width` for tab expansion.

```yaml
- id: docs-80-col
  kind: line_max_width
  paths: "docs/**/*.md"
  max: 80
  level: info
```

### `indent_style`

Every non-blank line indents with the configured `style` (`tabs` or `spaces`). When `style: spaces`, optional `width` enforces a multiple.

```yaml
- id: yaml-2sp
  kind: indent_style
  paths: "**/*.yml"
  style: spaces
  width: 2
  level: warning
```

Check-only: tab-width-aware reindentation is language-specific. Pair with your editor's "reindent on save" for remediation.

### `max_consecutive_blank_lines`

Cap runs of blank lines to `max`. A blank line is empty or whitespace-only.

```yaml
- id: md-tidy
  kind: max_consecutive_blank_lines
  paths: "**/*.md"
  max: 1
  level: warning
  fix:
    file_collapse_blank_lines: {}
```

---

