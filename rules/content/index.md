---
title: 'Content'
description: 'Rule reference: the content family.'
sidebar:
  order: 2
  label: 'Content'
---

Rule kinds in the **Content** family. Each entry below has its own page with options, an example, and any auto-fix support.

- [`file_content_matches`](/docs/rules/content/file_content_matches/) — File contents must contain at least one match for a regex.
- [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/) — File contents must NOT match a regex.
- [`file_header`](/docs/rules/content/file_header/) — The first N lines must match a regex (line-oriented).
- [`file_starts_with`](/docs/rules/content/file_starts_with/) — Byte-level prefix / suffix check.
- [`file_ends_with`](/docs/rules/content/file_ends_with/) — Byte-level prefix / suffix check.
- [`file_hash`](/docs/rules/content/file_hash/) — Content SHA-256 must equal the expected digest.
- [`file_max_size`](/docs/rules/content/file_max_size/) — File must be at most `max_bytes` in size.
- [`file_min_size`](/docs/rules/content/file_min_size/) — File must be at least `min_bytes` in size.
- [`file_min_lines`](/docs/rules/content/file_min_lines/) — File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more — `wc -l` semantics).
- [`json_path_equals`](/docs/rules/content/json_path_equals/) — Query a structured document (JSON / YAML / TOML) with a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) expression and assert every match deep-equals the supplied value.
- [`yaml_path_equals`](/docs/rules/content/yaml_path_equals/) — Query a structured document (JSON / YAML / TOML) with a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) expression and assert every match deep-equals the supplied value.
- [`toml_path_equals`](/docs/rules/content/toml_path_equals/) — Query a structured document (JSON / YAML / TOML) with a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) expression and assert every match deep-equals the supplied value.
- [`json_path_matches`](/docs/rules/content/json_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values.
- [`yaml_path_matches`](/docs/rules/content/yaml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values.
- [`toml_path_matches`](/docs/rules/content/toml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values.
- [`file_is_text`](/docs/rules/content/file_is_text/) — Content is detected as text (magic bytes + UTF-8 validity check) — fails on binary files matched by `paths`.
- [`file_is_ascii`](/docs/rules/content/file_is_ascii/) — Every byte in the file must be < 0x80.
