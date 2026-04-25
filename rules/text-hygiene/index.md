---
title: 'Text hygiene'
description: 'Rule reference: the text hygiene family.'
sidebar:
  order: 4
  label: 'Text hygiene'
---

Rule kinds in the **Text hygiene** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/) — No line may end with space or tab.
- [`final_newline`](/docs/rules/text-hygiene/final_newline/) — File must end with a single `\n`.
- [`line_endings`](/docs/rules/text-hygiene/line_endings/) — Every line ending matches `target`: `lf` or `crlf`.
- [`line_max_width`](/docs/rules/text-hygiene/line_max_width/) — Cap line length in characters (not bytes — code points).
- [`indent_style`](/docs/rules/text-hygiene/indent_style/) — Every non-blank line indents with the configured `style` (`tabs` or `spaces`).
- [`max_consecutive_blank_lines`](/docs/rules/text-hygiene/max_consecutive_blank_lines/) — Cap runs of blank lines to `max`.
