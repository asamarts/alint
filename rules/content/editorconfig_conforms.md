---
title: 'editorconfig_conforms'
description: 'alint editorconfig_conforms rule (content): Enforce selected formatting properties from the repository''s own .editorconfig files without repeating those...'
sidebar:
  order: 15
categories: ['content', 'text-hygiene']
---

Enforce selected formatting properties from the repository's own `.editorconfig` files without repeating those values in `.alint.yml`.

The rule discovers only `.editorconfig` files between each selected file and the lint root; a developer's parent-directory policy can never affect the result. It follows EditorConfig section order, nested-file precedence, `root = true`, `unset`, slash-relative globs, `*`/`**`/`?`, literal character classes (including the specification's non-separator treatment of `/` inside a complete class), brace alternatives, and ascending numeric ranges. Policy files follow the EditorConfig grammar: UTF-8 with LF or CRLF separators, `=` property delimiters, and ignored non-`root` preamble pairs. Every finding points back to the declaration that supplied its effective policy. An unreadable or malformed policy file and an invalid value for a supported property are `resource_read` or `resource_parse` analysis failures rather than silent passes.

Supported properties are `indent_style` (plus numeric `indent_size` / `tab_width`), `end_of_line` (`lf`, `crlf`, or `cr`), `charset` (`latin1`, `utf-8`, `utf-8-bom`, `utf-16be`, or `utf-16le`), `trim_trailing_whitespace`, `insert_final_newline`, and positive-integer `max_line_length` (`off` disables it). Space indentation enforces numeric `indent_size`; tab indentation uses as many tabs as the effective tab width permits and allows only the remaining sub-tab-width spaces. Target-file text checks recognize LF, CRLF, and CR as physical line endings. A false `trim_trailing_whitespace` imposes no prohibition; `insert_final_newline: false` requires a non-empty file not to end in a line break. Unknown section properties remain available to other EditorConfig plugins and are ignored. Check-only: a policy-derived multi-operation repair needs the later transactional edit-plan surface.

## Options

_This rule takes no kind-specific options._

Plus the common `paths`, `level`, `id`, `when`, `scope_filter`, and `expect_matches` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### Files that violate their EditorConfig policy are reported

The rule fires on this repository:

```text
.editorconfig
bad.txt
```

```text title=".editorconfig"
root = true

[*.txt]
indent_style = space
indent_size = 2
trim_trailing_whitespace = true
insert_final_newline = true
```

```text title="bad.txt"
	bad trailing   
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: editor-policy
    kind: editorconfig_conforms
    paths: "**/*.txt"
    level: error
```

`alint check` reports:

```ansi
[2m--- bad.txt --------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2meditor-policy[0m
              [2m1:1[0m  line 1 does not use EditorConfig space indentation
              [2mrelated:[0m EditorConfig `indent_style` policy: .editorconfig:4:1-4:21
  [1m[31mx  error  [0m  [2meditor-policy[0m
              [2m1:14[0m  line 1 has trailing whitespace
              [2mrelated:[0m EditorConfig `trim_trailing_whitespace` policy: .editorconfig:6:1-6:32
  [1m[31mx  error  [0m  [2meditor-policy[0m
              [2m1:17[0m  file does not end with a newline
              [2mrelated:[0m EditorConfig `insert_final_newline` policy: .editorconfig:7:1-7:28

[2mSummary (3 violations):[0m
  [1m[31mx 3 errors[0m
  0 passing [2m*[0m 1 failing
```

### Nested EditorConfig sections merge and unset inherited policy

This repository is compliant:

```text
.editorconfig
docs/
docs/.editorconfig
docs/guide.txt
```

```text title=".editorconfig"
root = true

[*.txt]
indent_style = space
indent_size = 2
trim_trailing_whitespace = true
insert_final_newline = true
```

```text title="docs/.editorconfig"
[*.txt]
trim_trailing_whitespace = unset
```

```text title="docs/guide.txt"
  aligned trailing spaces   
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: editor-policy
    kind: editorconfig_conforms
    paths: "**/*.txt"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

