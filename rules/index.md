---
title: Rules
description: Every rule kind alint ships, with one-line summaries and links to family + per-rule pages.
sidebar:
  order: 1
  label: 'Index'
---

alint ships 58 rule kinds across 12 families. Each rule is one entry in your `.alint.yml` under `rules:`.

## By family

- [Existence](/docs/rules/existence/) — 4 rules
- [Content](/docs/rules/content/) — 21 rules
- [Naming](/docs/rules/naming/) — 2 rules
- [Text hygiene](/docs/rules/text-hygiene/) — 6 rules
- [Security / Unicode sanity](/docs/rules/security-unicode-sanity/) — 3 rules
- [Encoding](/docs/rules/encoding/) — 1 rule
- [Structure](/docs/rules/structure/) — 3 rules
- [Portable metadata](/docs/rules/portable-metadata/) — 2 rules
- [Unix metadata](/docs/rules/unix-metadata/) — 4 rules
- [Git hygiene](/docs/rules/git-hygiene/) — 4 rules
- [Cross-file](/docs/rules/cross-file/) — 7 rules
- [Plugin (tier 1)](/docs/rules/plugin-tier-1/) — 1 rule

## Alphabetical

- [`command`](/docs/rules/plugin-tier-1/command/) — Shell out to an external CLI per matched file. _(Plugin (tier 1))_
- [`dir_absent`](/docs/rules/existence/dir_absent/) — Directory counterpart of `file_absent`. _(Existence)_
- [`dir_contains`](/docs/rules/cross-file/dir_contains/) — Every directory matching `paths` must contain files matching `require:`. _(Cross-file)_
- [`dir_exists`](/docs/rules/existence/dir_exists/) — Directory counterpart of `file_exists`. _(Existence)_
- [`dir_only_contains`](/docs/rules/cross-file/dir_only_contains/) — Every directory matching `paths` may contain only files matching `allow:`. _(Cross-file)_
- [`every_matching_has`](/docs/rules/cross-file/every_matching_has/) — For every file matching `paths`, at least one of `require:` must also exist (at a template-derived location). _(Cross-file)_
- [`executable_bit`](/docs/rules/unix-metadata/executable_bit/) — Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`). _(Unix metadata)_
- [`executable_has_shebang`](/docs/rules/unix-metadata/executable_has_shebang/) — Every file with `+x` set must begin with `#!`. _(Unix metadata)_
- [`file_absent`](/docs/rules/existence/file_absent/) — No file matching `paths` may exist in the walked tree. _(Existence)_
- [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/) — File contents must NOT match a regex. _(Content)_
- [`file_content_matches`](/docs/rules/content/file_content_matches/) — File contents must contain at least one match for a regex. _(Content)_
- [`file_ends_with`](/docs/rules/content/file_ends_with/) — Byte-level prefix / suffix check. _(Content)_
- [`file_exists`](/docs/rules/existence/file_exists/) — Every glob match in `paths` must correspond to a real file. _(Existence)_
- [`file_footer`](/docs/rules/content/file_footer/) — Last `lines` lines of each file in scope must match a regex. _(Content)_
- [`file_hash`](/docs/rules/content/file_hash/) — Content SHA-256 must equal the expected digest. _(Content)_
- [`file_header`](/docs/rules/content/file_header/) — The first N lines must match a regex (line-oriented). _(Content)_
- [`file_is_ascii`](/docs/rules/content/file_is_ascii/) — Every byte in the file must be < 0x80. _(Content)_
- [`file_is_text`](/docs/rules/content/file_is_text/) — Content is detected as text (magic bytes + UTF-8 validity check) — fails on binary files matched by `paths`. _(Content)_
- [`file_max_lines`](/docs/rules/content/file_max_lines/) — File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. _(Content)_
- [`file_max_size`](/docs/rules/content/file_max_size/) — File must be at most `max_bytes` in size. _(Content)_
- [`file_min_lines`](/docs/rules/content/file_min_lines/) — File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more — `wc -l` semantics). _(Content)_
- [`file_min_size`](/docs/rules/content/file_min_size/) — File must be at least `min_bytes` in size. _(Content)_
- [`file_shebang`](/docs/rules/content/file_shebang/) — First line of each file in scope must match the `shebang` regex. _(Content)_
- [`file_starts_with`](/docs/rules/content/file_starts_with/) — Byte-level prefix / suffix check. _(Content)_
- [`filename_case`](/docs/rules/naming/filename_case/) — Basename (stem only or full) matches a case convention: `snake`, `kebab`, `pascal`, `camel`, `screaming-snake`, `flat`, `lower`, `upper`. _(Naming)_
- [`filename_regex`](/docs/rules/naming/filename_regex/) — Basename matches a regex. _(Naming)_
- [`final_newline`](/docs/rules/text-hygiene/final_newline/) — File must end with a single `\n`. _(Text hygiene)_
- [`for_each_dir`](/docs/rules/cross-file/for_each_dir/) — For every matching directory / file, evaluate a nested `require:` block with the entry as context. _(Cross-file)_
- [`for_each_file`](/docs/rules/cross-file/for_each_file/) — For every matching directory / file, evaluate a nested `require:` block with the entry as context. _(Cross-file)_
- [`git_commit_message`](/docs/rules/git-hygiene/git_commit_message/) — Validate HEAD's commit-message shape via regex, max-subject-length, or required-body. _(Git hygiene)_
- [`git_no_denied_paths`](/docs/rules/git-hygiene/git_no_denied_paths/) — Fire when any tracked file matches a configured glob denylist. _(Git hygiene)_
- [`indent_style`](/docs/rules/text-hygiene/indent_style/) — Every non-blank line indents with the configured `style` (`tabs` or `spaces`). _(Text hygiene)_
- [`json_path_equals`](/docs/rules/content/json_path_equals/) — Query a structured document (JSON / YAML / TOML) with a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) expression and assert every match deep-equals the supplied value. _(Content)_
- [`json_path_matches`](/docs/rules/content/json_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. _(Content)_
- [`json_schema_passes`](/docs/rules/content/json_schema_passes/) — Validate every JSON / YAML / TOML file in `paths` against a JSON Schema document. _(Content)_
- [`line_endings`](/docs/rules/text-hygiene/line_endings/) — Every line ending matches `target`: `lf` or `crlf`. _(Text hygiene)_
- [`line_max_width`](/docs/rules/text-hygiene/line_max_width/) — Cap line length in characters (not bytes — code points). _(Text hygiene)_
- [`markdown_paths_resolve`](/docs/rules/git-hygiene/markdown_paths_resolve/) — Validate that backticked workspace paths in markdown files resolve to real files or directories in the repo. _(Git hygiene)_
- [`max_consecutive_blank_lines`](/docs/rules/text-hygiene/max_consecutive_blank_lines/) — Cap runs of blank lines to `max`. _(Text hygiene)_
- [`max_directory_depth`](/docs/rules/structure/max_directory_depth/) — Tree depth from repo root may not exceed `max`. _(Structure)_
- [`max_files_per_directory`](/docs/rules/structure/max_files_per_directory/) — Per-directory fanout may not exceed `max`. _(Structure)_
- [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/) — Flag Trojan-Source bidi override characters (U+202A–202E, U+2066–2069). _(Security / Unicode sanity)_
- [`no_bom`](/docs/rules/encoding/no_bom/) — Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. _(Encoding)_
- [`no_case_conflicts`](/docs/rules/portable-metadata/no_case_conflicts/) — Flag paths that differ only by case (e.g. _(Portable metadata)_
- [`no_empty_files`](/docs/rules/structure/no_empty_files/) — Flag zero-byte files. _(Structure)_
- [`no_illegal_windows_names`](/docs/rules/portable-metadata/no_illegal_windows_names/) — Reject path components Windows can't represent: _(Portable metadata)_
- [`no_merge_conflict_markers`](/docs/rules/security-unicode-sanity/no_merge_conflict_markers/) — Flag `<<<<<<< `, `=======`, `>>>>>>> ` markers at the start of a line — almost always left over from an unresolved merge. _(Security / Unicode sanity)_
- [`no_submodules`](/docs/rules/git-hygiene/no_submodules/) — Flag the presence of `.gitmodules` at the repo root — always, regardless of `paths`. _(Git hygiene)_
- [`no_symlinks`](/docs/rules/unix-metadata/no_symlinks/) — Flag tracked paths that are symbolic links. _(Unix metadata)_
- [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/) — No line may end with space or tab. _(Text hygiene)_
- [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/) — Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). _(Security / Unicode sanity)_
- [`pair`](/docs/rules/cross-file/pair/) — For every file matching `primary`, a file matching the `partner` template must exist. _(Cross-file)_
- [`shebang_has_executable`](/docs/rules/unix-metadata/shebang_has_executable/) — Every file starting with `#!` must have `+x` set. _(Unix metadata)_
- [`toml_path_equals`](/docs/rules/content/toml_path_equals/) — Query a structured document (JSON / YAML / TOML) with a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) expression and assert every match deep-equals the supplied value. _(Content)_
- [`toml_path_matches`](/docs/rules/content/toml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. _(Content)_
- [`unique_by`](/docs/rules/cross-file/unique_by/) — No two files matching `paths` may share the value of `key` (a path template). _(Cross-file)_
- [`yaml_path_equals`](/docs/rules/content/yaml_path_equals/) — Query a structured document (JSON / YAML / TOML) with a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) expression and assert every match deep-equals the supplied value. _(Content)_
- [`yaml_path_matches`](/docs/rules/content/yaml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. _(Content)_
