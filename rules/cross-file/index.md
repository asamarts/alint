---
title: 'Cross-file'
description: 'Rule reference: the cross-file family.'
sidebar:
  order: 12
  label: 'Cross-file'
---

Rule kinds in the **Cross-file** family. Each rule below links to its own page with options, an example, and any auto-fix support.

| Rule | Description |
| --- | --- |
| [`markdown_paths_resolve`](/docs/rules/git-hygiene/markdown_paths_resolve/) | Validate that backticked workspace paths in markdown files resolve to real files or directories in the repo. |
| [`changeset_requires_path`](/docs/rules/git-hygiene/changeset_requires_path/) | The `<since>...HEAD` diff must **add** (git status `A`) at least one path matching `add_glob:`, the "did you add a changelog entry?" gate. |
| [`pair_changed_together`](/docs/rules/git-hygiene/pair_changed_together/) | If the `<since>...HEAD` diff changes any path matching `if_changed:`, at least one path matching `then_changed:` must change in the same range, the **co-change** gate. |
| [`pair`](/docs/rules/cross-file/pair/) | For every file matching `primary`, a file matching the `partner` template must exist. |
| [`pair_hash`](/docs/rules/cross-file/pair_hash/) | The `algorithm` digest (`sha256` default / `sha512`) of every file matching `source` must appear in the single `target` file, either as an embedded hex substring (`format: contains`, default) or a `<hex>  <path>` manifest line (`format: sums-line`, where the path token must be the source's path; a leading `*` binary marker and a `./` prefix are tolerated). |
| [`registry_paths_resolve`](/docs/rules/cross-file/registry_paths_resolve/) | A manifest file enumerates path entries; each must resolve to an on-disk artefact. |
| [`cross_file`](/docs/rules/cross-file/cross_file/) | A `source` must hold a `relation` to one or more `targets` (or, for `resolves`, the filesystem). |
| [`file_graph`](/docs/rules/cross-file/file_graph/) | Assemble the repo's *file → file* reference graph and assert a global structural property the 1-level cross-file kinds can't express. |
| [`ordered_block`](/docs/rules/cross-file/ordered_block/) | The lines between a `start` / `end` marker pair must stay sorted (and, with `unique: true`, free of duplicates) under `comparator` (`lexical` / `lexical-ci` / `numeric`). |
| [`for_each_match`](/docs/rules/cross-file/for_each_match/) | For each line matching `select` (a regex), the line must satisfy the nested `require:` predicates. |
| [`generated_file_fresh`](/docs/rules/cross-file/generated_file_fresh/) | A committed artefact must equal what a declared `command` generator produces, in one of two modes (exactly one of `file` / `outputs`). |
| [`import_gate`](/docs/rules/cross-file/import_gate/) | Forbid imports whose **extracted target** matches a `forbid` regex, within the `paths` scope, an architectural import firewall (staging-layer isolation, core/providers separation, private-API gates). |
| [`command_idempotent`](/docs/rules/cross-file/command_idempotent/) | Run a user-declared formatter/checker in its **`--check` (idempotence) mode** once: exit `0` ⇒ the tree is formatter-clean (silent); non-zero ⇒ violation(s). |
| [`for_each_dir`](/docs/rules/cross-file/for_each_dir/) | For every matching directory / file, evaluate a nested `require:` block with the entry as context. |
| [`for_each_file`](/docs/rules/cross-file/for_each_file/) | For every matching directory / file, evaluate a nested `require:` block with the entry as context. |
| [`dir_contains`](/docs/rules/cross-file/dir_contains/) | Every directory matching `select:` must contain files matching every glob in `require:`. |
| [`dir_only_contains`](/docs/rules/cross-file/dir_only_contains/) | Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`. |
| [`unique_by`](/docs/rules/cross-file/unique_by/) | No two files matching `select` may share the value of `key` (a path template; tokens `{path}`/`{dir}`/`{basename}`/`{stem}`/`{ext}`/`{parent_name}`). |
| [`every_matching_has`](/docs/rules/cross-file/every_matching_has/) | For every file or directory matching `select:`, every nested rule under `require:` must be satisfied. |
