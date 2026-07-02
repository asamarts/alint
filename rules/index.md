---
title: Rules
description: Every rule kind alint ships, with one-line summaries and links to family + per-rule pages.
sidebar:
  order: 1
  label: 'Index'
---

alint ships 89 rule kinds across 13 families (78 distinct rule behaviors plus 11 short-name aliases like `content_matches` → `file_content_matches`). Each rule is one entry in your `.alint.yml` under `rules:`.

## By family

- [Existence](/docs/rules/existence/) — 4 rules
- [Content](/docs/rules/content/) — 14 rules
- [Structured query](/docs/rules/structured-query/) — 9 rules
- [Naming](/docs/rules/naming/) — 2 rules
- [Text hygiene](/docs/rules/text-hygiene/) — 6 rules
- [Security / Unicode sanity](/docs/rules/security-unicode-sanity/) — 3 rules
- [Encoding](/docs/rules/encoding/) — 1 rule
- [Structure](/docs/rules/structure/) — 3 rules
- [Portable metadata](/docs/rules/portable-metadata/) — 2 rules
- [Unix metadata](/docs/rules/unix-metadata/) — 4 rules
- [Git hygiene](/docs/rules/git-hygiene/) — 13 rules
- [Cross-file](/docs/rules/cross-file/) — 16 rules
- [Plugin (tier 1)](/docs/rules/plugin-tier-1/) — 1 rule

## Alphabetical

- [`changeset_requires_path`](/docs/rules/git-hygiene/changeset_requires_path/) — The `<since>...HEAD` diff must **add** (git status `A`) at least one path matching `add_glob:` — the "did you add a changelog entry?" gate. _(Git hygiene)_
- [`command`](/docs/rules/plugin-tier-1/command/) — Shell out to an external CLI per matched file. _(Plugin (tier 1))_
- [`command_idempotent`](/docs/rules/cross-file/command_idempotent/) — Run a user-declared formatter/checker in its **`--check` (idempotence) mode** once: exit `0` ⇒ the tree is formatter-clean (silent); non-zero ⇒ violation(s). _(Cross-file)_
- [`commented_out_code`](/docs/rules/git-hygiene/commented_out_code/) — Heuristic detector for blocks of commented-out source code (as opposed to prose comments, license headers, doc comments, or ASCII banners). _(Git hygiene)_
- [`cross_file`](/docs/rules/cross-file/cross_file/) — A `source` must hold a `relation` to one or more `targets` (or, for `resolves`, the filesystem). _(Cross-file)_
- [`dir_absent`](/docs/rules/existence/dir_absent/) — Directory counterpart of `file_absent`. _(Existence)_
- [`dir_contains`](/docs/rules/cross-file/dir_contains/) — Every directory matching `select:` must contain files matching every glob in `require:`. _(Cross-file)_
- [`dir_exists`](/docs/rules/existence/dir_exists/) — Directory counterpart of `file_exists`. _(Existence)_
- [`dir_only_contains`](/docs/rules/cross-file/dir_only_contains/) — Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`. _(Cross-file)_
- [`every_matching_has`](/docs/rules/cross-file/every_matching_has/) — For every file or directory matching `select:`, every nested rule under `require:` must be satisfied. _(Cross-file)_
- [`executable_bit`](/docs/rules/unix-metadata/executable_bit/) — Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`). _(Unix metadata)_
- [`executable_has_shebang`](/docs/rules/unix-metadata/executable_has_shebang/) — Every file with `+x` set must begin with `#!`. _(Unix metadata)_
- [`file_absent`](/docs/rules/existence/file_absent/) — No file matching `paths` may exist in the walked tree. _(Existence)_
- [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/) — File contents must NOT match a regex. _(Content)_
- [`file_content_matches`](/docs/rules/content/file_content_matches/) — File contents must contain at least one match for a regex. _(Content)_
- [`file_ends_with`](/docs/rules/content/file_ends_with/) — Byte-level prefix / suffix check. _(Content)_
- [`file_exists`](/docs/rules/existence/file_exists/) — Every glob match in `paths` must correspond to a real file. _(Existence)_
- [`file_footer`](/docs/rules/content/file_footer/) — Last `lines` lines of each file in scope must match a regex. _(Content)_
- [`file_graph`](/docs/rules/cross-file/file_graph/) — Assemble the repo's *file → file* reference graph and assert a global structural property the 1-level cross-file kinds can't express. _(Cross-file)_
- [`file_hash`](/docs/rules/content/file_hash/) — Content SHA-256 must equal the expected digest. _(Content)_
- [`file_header`](/docs/rules/content/file_header/) — The first N lines must match a regex (line-oriented). _(Content)_
- [`file_is_ascii`](/docs/rules/content/file_is_ascii/) — Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in `allow:`. _(Content)_
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
- [`for_each_match`](/docs/rules/cross-file/for_each_match/) — For each line matching `select` (a regex), the line must satisfy the nested `require:` predicates. _(Cross-file)_
- [`generated_file_fresh`](/docs/rules/cross-file/generated_file_fresh/) — A committed artefact must equal what a declared `command` generator produces, in one of two modes (exactly one of `file` / `outputs`). _(Cross-file)_
- [`git_blame_age`](/docs/rules/git-hygiene/git_blame_age/) — Fire on lines matching a regex whose `git blame` author-time is older than `max_age_days`. _(Git hygiene)_
- [`git_commit_author_allowlist`](/docs/rules/git-hygiene/git_commit_author_allowlist/) — Assert every commit author in scope matches an allowed email and/or name pattern. _(Git hygiene)_
- [`git_commit_gpg_signed`](/docs/rules/git-hygiene/git_commit_gpg_signed/) — Assert every commit in scope has a verifying signature (`git verify-commit` exits 0). _(Git hygiene)_
- [`git_commit_message`](/docs/rules/git-hygiene/git_commit_message/) — Validate commit-message shape via regex, max-subject-length, or required-body. _(Git hygiene)_
- [`git_commit_no_fixup`](/docs/rules/git-hygiene/git_commit_no_fixup/) — Fail on residual `fixup!` / `squash!` / `amend!` commits left in scope — the ones `git commit --fixup` / `--squash` produce, meant to be collapsed by `git rebase --autosquash` before merging. _(Git hygiene)_
- [`git_commit_signed_off`](/docs/rules/git-hygiene/git_commit_signed_off/) — Assert every commit in scope carries a DCO (Developer Certificate of Origin) `Signed-off-by:` trailer — required by every CNCF / Linux Foundation / kernel-style project. _(Git hygiene)_
- [`git_commit_subject_matches`](/docs/rules/git-hygiene/git_commit_subject_matches/) — Each commit's subject line (the first line of its message) must match the `matches:` regex — the subject-grammar member of the commit family. _(Git hygiene)_
- [`git_no_denied_paths`](/docs/rules/git-hygiene/git_no_denied_paths/) — Fire when any tracked file matches a configured glob denylist. _(Git hygiene)_
- [`import_gate`](/docs/rules/cross-file/import_gate/) — Forbid imports whose **extracted target** matches a `forbid` regex, within the `paths` scope — an architectural import firewall (staging-layer isolation, core/providers separation, private-API gates). _(Cross-file)_
- [`indent_style`](/docs/rules/text-hygiene/indent_style/) — Every non-blank line indents with the configured `style` (`tabs` or `spaces`). _(Text hygiene)_
- [`json_path_equals`](/docs/rules/structured-query/json_path_equals/) — Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. _(Structured query)_
- [`json_path_matches`](/docs/rules/structured-query/json_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. _(Structured query)_
- [`json_schema_passes`](/docs/rules/structured-query/json_schema_passes/) — Validate every JSON / YAML / TOML file in `paths` against a JSON Schema document. _(Structured query)_
- [`line_endings`](/docs/rules/text-hygiene/line_endings/) — Every line ending matches `target`: `lf` or `crlf`. _(Text hygiene)_
- [`line_max_width`](/docs/rules/text-hygiene/line_max_width/) — Cap line length in characters (not bytes — code points). _(Text hygiene)_
- [`markdown_paths_resolve`](/docs/rules/git-hygiene/markdown_paths_resolve/) — Validate that backticked workspace paths in markdown files resolve to real files or directories in the repo. _(Git hygiene)_
- [`max_consecutive_blank_lines`](/docs/rules/text-hygiene/max_consecutive_blank_lines/) — Cap runs of blank lines to `max`. _(Text hygiene)_
- [`max_directory_depth`](/docs/rules/structure/max_directory_depth/) — Tree depth from repo root may not exceed `max_depth`. _(Structure)_
- [`max_files_per_directory`](/docs/rules/structure/max_files_per_directory/) — Per-directory fanout may not exceed `max_files`. _(Structure)_
- [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/) — Flag Trojan-Source bidi override characters (U+202A–202E, U+2066–2069). _(Security / Unicode sanity)_
- [`no_bom`](/docs/rules/encoding/no_bom/) — Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. _(Encoding)_
- [`no_case_conflicts`](/docs/rules/portable-metadata/no_case_conflicts/) — Flag paths that differ only by case (e.g. _(Portable metadata)_
- [`no_empty_files`](/docs/rules/structure/no_empty_files/) — Flag zero-byte files. _(Structure)_
- [`no_illegal_windows_names`](/docs/rules/portable-metadata/no_illegal_windows_names/) — Reject path components Windows can't represent: _(Portable metadata)_
- [`no_merge_conflict_markers`](/docs/rules/security-unicode-sanity/no_merge_conflict_markers/) — Flag `<<<<<<< `, `=======`, `>>>>>>> `, `||||||| ` markers at the start of a line — almost always left over from an unresolved merge. _(Security / Unicode sanity)_
- [`no_submodules`](/docs/rules/git-hygiene/no_submodules/) — Flag the presence of `.gitmodules` at the repo root — always, regardless of `paths`. _(Git hygiene)_
- [`no_symlinks`](/docs/rules/unix-metadata/no_symlinks/) — Flag tracked paths that are symbolic links. _(Unix metadata)_
- [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/) — No line may end with space or tab. _(Text hygiene)_
- [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/) — Flag body-internal zero-width characters (U+200B, U+200C, U+200D, U+2060, U+180E, and non-leading U+FEFF). _(Security / Unicode sanity)_
- [`ordered_block`](/docs/rules/cross-file/ordered_block/) — The lines between a `start` / `end` marker pair must stay sorted (and, with `unique: true`, free of duplicates) under `comparator` (`lexical` / `lexical-ci` / `numeric`). _(Cross-file)_
- [`pair`](/docs/rules/cross-file/pair/) — For every file matching `primary`, a file matching the `partner` template must exist. _(Cross-file)_
- [`pair_changed_together`](/docs/rules/git-hygiene/pair_changed_together/) — If the `<since>...HEAD` diff changes any path matching `if_changed:`, at least one path matching `then_changed:` must change in the same range — the **co-change** gate. _(Git hygiene)_
- [`pair_hash`](/docs/rules/cross-file/pair_hash/) — The `algorithm` digest (`sha256` default / `sha512`) of every file matching `source` must appear in the single `target` file — either as an embedded hex substring (`format: contains`, default) or a `<hex>  <path>` manifest line (`format: sums-line`, where the path token must be the source's path; a leading `*` binary marker and a `./` prefix are tolerated). _(Cross-file)_
- [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/) — A manifest file enumerates path entries; each must resolve to an on-disk artefact. _(Cross-file)_
- [`shebang_has_executable`](/docs/rules/unix-metadata/shebang_has_executable/) — Every file starting with `#!` must have `+x` set. _(Unix metadata)_
- [`toml_path_equals`](/docs/rules/structured-query/toml_path_equals/) — Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. _(Structured query)_
- [`toml_path_matches`](/docs/rules/structured-query/toml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. _(Structured query)_
- [`unique_by`](/docs/rules/cross-file/unique_by/) — No two files matching `select` may share the value of `key` (a path template; tokens `{path}`/`{dir}`/`{basename}`/`{stem}`/`{ext}`/`{parent_name}`). _(Cross-file)_
- [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/) — Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. _(Structured query)_
- [`xml_path_matches`](/docs/rules/structured-query/xml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. _(Structured query)_
- [`yaml_path_equals`](/docs/rules/structured-query/yaml_path_equals/) — Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value. _(Structured query)_
- [`yaml_path_matches`](/docs/rules/structured-query/yaml_path_matches/) — Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. _(Structured query)_
