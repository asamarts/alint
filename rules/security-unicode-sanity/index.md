---
title: 'Security / Unicode sanity'
description: 'Rule reference: the security / unicode sanity family.'
sidebar:
  order: 6
  label: 'Security / Unicode sanity'
---

Rule kinds in the **Security / Unicode sanity** family. Each rule below links to its own page with options, an example, and any auto-fix support.

| Rule | Description |
| --- | --- |
| [`file_content_forbidden`](/docs/rules/content/file_content_forbidden/) | File contents must NOT match a regex. |
| [`file_hash`](/docs/rules/content/file_hash/) | Content SHA-256 must equal the expected digest. |
| [`file_is_ascii`](/docs/rules/content/file_is_ascii/) | Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in `allow:`. |
| [`no_merge_conflict_markers`](/docs/rules/security-unicode-sanity/no_merge_conflict_markers/) | Flag `<<<<<<< `, `=======`, `>>>>>>> `, `\|\|\|\|\|\|\| ` markers at the start of a line, almost always left over from an unresolved merge. |
| [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/) | Flag Trojan-Source bidi override characters (U+202A,202E, U+2066,2069). |
| [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/) | Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). |
| [`no_symlinks`](/docs/rules/unix-metadata/no_symlinks/) | Flag tracked paths that are symbolic links. |
| [`git_no_denied_paths`](/docs/rules/git-hygiene/git_no_denied_paths/) | Fire when any tracked file matches a configured glob denylist. |
| [`git_commit_author_allowlist`](/docs/rules/git-hygiene/git_commit_author_allowlist/) | Assert every commit author in scope matches an allowed email and/or name pattern. |
| [`git_commit_gpg_signed`](/docs/rules/git-hygiene/git_commit_gpg_signed/) | Assert every commit in scope has a verifying signature (`git verify-commit` exits 0). |
| [`pair_hash`](/docs/rules/cross-file/pair_hash/) | The `algorithm` digest (`sha256` default / `sha512`) of every file matching `source` must appear in the single `target` file, either as an embedded hex substring (`format: contains`, default) or a `<hex>  <path>` manifest line (`format: sums-line`, where the path token must be the source's path; a leading `*` binary marker and a `./` prefix are tolerated). |
| [`generated_file_fresh`](/docs/rules/cross-file/generated_file_fresh/) | A committed artefact must equal what a declared `command` generator produces, in one of two modes (exactly one of `file` / `outputs`). |
| [`import_gate`](/docs/rules/cross-file/import_gate/) | Forbid imports whose **extracted target** matches a `forbid` regex, within the `paths` scope, an architectural import firewall (staging-layer isolation, core/providers separation, private-API gates). |
