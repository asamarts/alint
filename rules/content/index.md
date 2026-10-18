---
title: 'Content'
description: 'Rule reference: the content family.'
sidebar:
  order: 2
  label: 'Content'
---

Rule kinds in the **Content** family. Each rule below links to its own page with options, an example, and any auto-fix support.

| Rule | Description |
| --- | --- |
| [`file_content_matches`](/docs/rules/content/file_content_matches/) | File contents must contain at least one match for a regex. |
| [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/) | File contents must NOT match a regex. |
| [`file_header`](/docs/rules/content/file_header/) | The first N lines must match a regex (line-oriented). |
| [`file_starts_with`](/docs/rules/content/file_starts_with/) | Byte-level prefix / suffix check. |
| [`file_ends_with`](/docs/rules/content/file_ends_with/) | Byte-level prefix / suffix check. |
| [`file_hash`](/docs/rules/content/file_hash/) | Content SHA-256 must equal the expected digest. |
| [`file_max_size`](/docs/rules/content/file_max_size/) | File must be at most `max_bytes` in size. |
| [`file_min_size`](/docs/rules/content/file_min_size/) | File must be at least `min_bytes` in size. |
| [`file_min_lines`](/docs/rules/content/file_min_lines/) | File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more, `wc -l` semantics). |
| [`file_max_lines`](/docs/rules/content/file_max_lines/) | File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. |
| [`file_footer`](/docs/rules/content/file_footer/) | The configured number of final lines in each file in scope must match a regex. |
| [`file_shebang`](/docs/rules/content/file_shebang/) | First line of each file in scope must match the `shebang` regex. |
| [`file_is_text`](/docs/rules/content/file_is_text/) | Content is detected as text (magic bytes + UTF-8 validity check), fails on binary files matched by `paths`. |
| [`file_is_ascii`](/docs/rules/content/file_is_ascii/) | Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in `allow:`. |
| [`editorconfig_conforms`](/docs/rules/content/editorconfig_conforms/) | Enforce selected formatting properties from the repository's own `.editorconfig` files without repeating those values in `.alint.yml`. |
| [`pinned_references`](/docs/rules/content/pinned_references/) | Require executable references in supported CI and container files to identify immutable artifacts rather than mutable tags or branches. |
| [`markdown_images_have_alt`](/docs/rules/content/markdown_images_have_alt/) | Require every live CommonMark image to have non-empty rendered alt text. |
| [`markdown_required_headings`](/docs/rules/content/markdown_required_headings/) | Require the complete rendered heading sequence to match an ordered `headings` template. |
| [`markdown_no_duplicate_headings`](/docs/rules/content/markdown_no_duplicate_headings/) | Reject a later Markdown heading whose generated base anchor collides with an earlier heading. |
| [`spdx_identifier_valid`](/docs/rules/content/spdx_identifier_valid/) | Validate every `SPDX-License-Identifier:` value as a strict SPDX license expression. |
| [`executable_has_shebang`](/docs/rules/unix-metadata/executable_has_shebang/) | Every file with `+x` set must begin with `#!`. |
| [`shebang_has_executable`](/docs/rules/unix-metadata/shebang_has_executable/) | Every file starting with `#!` must have `+x` set. |
| [`commented_out_code`](/docs/rules/git-hygiene/commented_out_code/) | Heuristic detector for blocks of commented-out source code (as opposed to prose comments, license headers, doc comments, or ASCII banners). |
| [`markdown_links_resolve`](/docs/rules/cross-file/markdown_links_resolve/) | Validate live Markdown link, image, and reference-definition destinations against the repository tree. |
| [`markdown_fragments_resolve`](/docs/rules/cross-file/markdown_fragments_resolve/) | Require every live local Markdown link fragment to name a heading anchor in its resolved target document. |
