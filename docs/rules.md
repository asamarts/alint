# alint rule catalogue

The full list of rule kinds shipped in alint, organised by family.
Each rule is one line in your `.alint.yml` under `rules:` — see
[ARCHITECTURE.md §DSL](design/ARCHITECTURE.md#dsl) for the common
fields (`id`, `level`, `paths`, `message`, `policy_url`, `when`,
`fix`). The JSON Schema at [`schemas/v1/config.json`](../schemas/v1/config.json)
is the authoritative source for option types.

## Contents

- [Existence](#existence)
- [Content](#content)
- [Naming](#naming)
- [Text hygiene](#text-hygiene)
- [Security / Unicode sanity](#security--unicode-sanity)
- [Encoding](#encoding)
- [Structure](#structure)
- [Portable metadata](#portable-metadata)
- [Unix metadata](#unix-metadata)
- [Git hygiene](#git-hygiene)
- [Cross-file](#cross-file)
- [Fix operations](#fix-operations)
- [Bundled rulesets](#bundled-rulesets)
- [Nested `.alint.yml` (monorepo layering)](#nested-alintyml-monorepo-layering)

---

## Existence

### `file_exists`

Every glob match in `paths` must correspond to a real file. Use an array to accept any of several names.

```yaml
- id: readme-exists
  kind: file_exists
  paths: ["README.md", "README", "README.rst"]
  root_only: true
  level: error
```

Fix: `file_create` — write a declared `content`. With an array of `paths`, the fix creates the first entry.

**Optional `git_tracked_only: true`** further requires that the matching file be in git's index — useful for rules like "every release must commit a CHANGELOG entry" where local-only files shouldn't satisfy the requirement. Outside a git repo, the rule fails (no file qualifies). See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

### `file_absent`

No file matching `paths` may exist in the walked tree. The inverse of `file_exists`.

```yaml
- id: no-backup-files
  kind: file_absent
  paths: "**/*.bak"
  level: warning
```

Fix: `file_remove` — delete every violating file.

**Optional `git_tracked_only: true`** restricts the check to files in git's index. With it set, the rule fires only on tracked paths regardless of `.gitignore` state — closing the gap where a `git add -f`'d file slips past the walker's gitignore filter. Outside a git repo the rule becomes a silent no-op.

```yaml
- id: no-tracked-env
  kind: file_absent
  paths: ".env"
  git_tracked_only: true
  level: error
```

**What "exists" means**: alint walks the filesystem and honours `.gitignore` by default, so a `file_absent` rule fires whenever a matching file is **present in the walked tree**, not when it's tracked in git. Files filtered by `.gitignore` are invisible to the rule. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics, the `--no-gitignore` flag, and the gap between this and git's actual index.

### `dir_exists`

Directory counterpart of `file_exists`. Every match must correspond to a real directory in the walked tree.

```yaml
- id: docs-dir-exists
  kind: dir_exists
  paths: "docs"
  root_only: true
  level: error
```

**Optional `git_tracked_only: true`** further requires that the directory contain at least one tracked file. A tree with a `docs/` checked out from a stale clone where every file was later removed via `git rm` would fail under this stricter check. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

### `dir_absent`

Directory counterpart of `file_absent`. The match-and-fire semantics are the same as `file_absent` — including the `.gitignore` interaction. A `dir_absent` rule with `paths: "**/target"` only fires when `target/` exists in the walked tree; if it's gitignored, the walker filters it out and the rule stays silent.

```yaml
- id: no-tracked-target
  kind: dir_absent
  paths: "**/target"
  level: error
```

**Optional `git_tracked_only: true`** restricts the check to directories that contain at least one git-tracked file. With it set, a developer's locally-built `target/` (gitignored, no tracked content) doesn't trigger; a `target/` whose contents made it into git's index does. This is the canonical "don't let `target/` be committed" semantic.

```yaml
- id: no-tracked-target
  kind: dir_absent
  paths: "**/target"
  git_tracked_only: true
  level: error
```

See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

---

## Content

### `file_content_matches` (alias: `content_matches`)

File contents must contain at least one match for a regex.

```yaml
- id: crate-is-2024-edition
  kind: content_matches
  paths: "Cargo.toml"
  pattern: 'edition\s*=\s*"2024"'
  level: error
```

Fix: `file_append` — append declared content.

### `file_content_forbidden` (alias: `content_forbidden`)

File contents must NOT match a regex.

```yaml
- id: no-dbg-macros
  kind: content_forbidden
  paths: "crates/**/src/**/*.rs"
  pattern: '\bdbg!\('
  level: warning
```

### `file_header` (alias: `header`)

The first N lines must match a regex (line-oriented). For a byte-level prefix check, prefer `file_starts_with`.

```yaml
- id: spdx-header
  kind: header
  paths: "src/**/*.rs"
  pattern: "^// SPDX-License-Identifier: MIT"
  level: error
```

Fix: `file_prepend` — inject declared content at the top (preserves UTF-8 BOM).

### `file_starts_with` / `file_ends_with`

Byte-level prefix / suffix check. Works on any bytes (binary safe, unlike `file_header`).

```yaml
- id: generated-sentinel
  kind: file_ends_with
  paths: "build/**/*.generated.md"
  suffix: "<!-- generated by alint -->\n"
  level: error
```

Check-only: a fix would risk silently duplicating a near-matching prefix. Pair with `file_prepend` / `file_append` explicitly if you want auto-repair.

### `file_hash`

Content SHA-256 must equal the expected digest. Rules-as-tripwire for generated / vendored files that should never drift.

```yaml
- id: schema-frozen
  kind: file_hash
  paths: "schemas/v1/config.json"
  sha256: "b7d0...c2e1"   # 64 hex chars
  level: error
```

### `file_max_size` (alias: `max_size`)

File must be at most `max_bytes` in size. Catches accidental large-blob commits.

```yaml
- id: no-huge-blobs
  kind: max_size
  paths: "**"
  max_bytes: 5242880   # 5 MiB
  level: warning
```

### `file_min_size` (alias: `min_size`)

File must be at least `min_bytes` in size. Catches placeholder / stub files that pass existence checks but add no information (a 0-byte `LICENSE`, a `README.md` with only a title).

```yaml
- id: license-non-empty
  kind: min_size
  paths: ["LICENSE", "LICENSE.md", "LICENSE-APACHE", "LICENSE-MIT"]
  min_bytes: 200
  level: warning
```

### `file_min_lines` (alias: `min_lines`)

File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more — `wc -l` semantics). Use for "README has more than a title and a TODO".

```yaml
- id: readme-non-stub
  kind: min_lines
  paths: ["README.md", "README"]
  min_lines: 5
  level: info
```

### `file_max_lines` (alias: `max_lines`)

File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. Catches the everything-module anti-pattern — a `lib.rs` / `index.ts` / `helpers.py` that grew unbounded.

```yaml
- id: cap-source-file-size
  kind: max_lines
  paths: "src/**/*.rs"
  max_lines: 800
  level: warning
```

### `file_footer` (alias: `footer`)

Last `lines` lines of each file in scope must match a regex. Mirror of `file_header` anchored at the end of the file. Use for license footers, signed-off-by trailers, generated-file sentinels.

```yaml
- id: license-footer
  kind: footer
  paths: "src/**/*.rs"
  pattern: "Licensed under the Apache License, Version 2\\.0"
  lines: 3
  level: error
```

Fix: `file_append` — append a declared `content`. With no fix declared, violations are unfixable.

### `file_shebang` (alias: `shebang`)

First line of each file in scope must match the `shebang` regex. Pairs with `executable_has_shebang` (which checks shebang *presence* on `+x` files) — `file_shebang` checks shebang *shape*.

```yaml
- id: scripts-use-env-bash
  kind: shebang
  paths: "scripts/*.sh"
  shebang: '^#!/usr/bin/env bash$'
  level: error
```

Default `shebang:` is `^#!`, which only enforces presence; almost every useful config supplies a tighter regex pinning the interpreter.

### `json_path_equals`, `yaml_path_equals`, `toml_path_equals`

Query a structured document (JSON / YAML / TOML) with a [JSONPath](https://datatracker.ietf.org/doc/html/rfc9535) expression and assert every match deep-equals the supplied value. YAML and TOML are parsed through serde and then treated as JSON-shaped trees, so the same JSONPath engine handles all three formats.

```yaml
- id: require-mit-license
  kind: json_path_equals
  paths: "packages/*/package.json"
  path: "$.license"
  equals: "MIT"
  level: error

- id: workflow-contents-read
  kind: yaml_path_equals
  paths: ".github/workflows/*.yml"
  path: "$.permissions.contents"
  equals: "read"
  level: error

- id: rust-edition-2024
  kind: toml_path_equals
  paths: "crates/*/Cargo.toml"
  path: "$.package.edition"
  equals: "2024"
  level: warning
```

**Semantics**:
- Multiple matches — every match must equal the expected value.
- Zero matches — counts as a violation (the key the rule is enforcing doesn't exist).
- Unparseable files — one violation per file (not silently skipped).

### `json_path_matches`, `yaml_path_matches`, `toml_path_matches`

Same shape as the `*_equals` variants, but the asserted value is a **regex** matched against string values. Non-string matches produce a clear "value is not a string" violation.

```yaml
- id: semver-version
  kind: json_path_matches
  paths: "packages/*/package.json"
  path: "$.version"
  matches: '^\d+\.\d+\.\d+$'
  level: error

- id: pin-actions-to-sha
  kind: yaml_path_matches
  paths: ".github/workflows/*.yml"
  path: "$.jobs.*.steps[*].uses"
  matches: '^[a-zA-Z0-9._/-]+@[a-f0-9]{40}$'
  level: warning
```

### `json_schema_passes`

Validate every JSON / YAML / TOML file in `paths` against a JSON Schema document. Targets coerce through serde into the same `serde_json::Value` tree the schema sees, so a JSON-format schema can validate a YAML config (Kubernetes manifests, GitHub Actions workflows, Helm `values.schema.json`) or a TOML manifest (`Cargo.toml`, `pyproject.toml`) without separate schemas per format. The schema is loaded + compiled lazily on first evaluation and cached on the rule.

Each schema-validation error becomes one violation, with the failing instance path and the schema's error description in the message. A target that fails to parse produces a single parse-error violation, not a flood of schema errors against junk. Format is detected from the target's extension (`.json` / `.yaml` / `.yml` / `.toml`); pass `format:` to override.

```yaml
- id: package-json-shape
  kind: json_schema_passes
  paths: "packages/*/package.json"
  schema_path: "schemas/package.schema.json"
  level: error

- id: workflow-shape
  kind: json_schema_passes
  paths: ".github/workflows/*.yml"
  schema_path: "schemas/workflow.schema.json"
  format: yaml
  level: warning
```

Check-only — fixing schema violations is a "the user knows what value belongs there" problem, not alint's.

### `file_is_text` (alias: `is_text`)

Content is detected as text (magic bytes + UTF-8 validity check) — fails on binary files matched by `paths`.

```yaml
- id: configs-are-text
  kind: file_is_text
  paths: ".github/**/*.{yml,yaml}"
  level: error
```

### `file_is_ascii`

Every byte in the file must be < 0x80. Strict variant of `is_text` for configs that must round-trip through strictly-ASCII tools.

```yaml
- id: licences-are-ascii
  kind: file_is_ascii
  paths: "LICENSE*"
  level: error
```

---

## Naming

### `filename_case`

Basename (stem only or full) matches a case convention: `snake`, `kebab`, `pascal`, `camel`, `screaming-snake`, `flat`, `lower`, `upper`.

```yaml
- id: rust-snake-case
  kind: filename_case
  paths: "crates/**/src/**/*.rs"
  case: snake
  level: error
```

Fix: `file_rename` — converts the stem to the configured case, preserving extension.

### `filename_regex`

Basename matches a regex. Use `stem: true` to match the stem only.

```yaml
- id: toml-kebab-or-cargo
  kind: filename_regex
  paths: "**/*.toml"
  stem: true
  pattern: "[a-z][a-z0-9_-]*|Cargo"
  level: warning
```

---

## Text hygiene

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

## Security / Unicode sanity

### `no_merge_conflict_markers`

Flag `<<<<<<< `, `=======`, `>>>>>>> ` markers at the start of a line — almost always left over from an unresolved merge.

```yaml
- id: no-conflicts
  kind: no_merge_conflict_markers
  paths: "**"
  level: error
```

### `no_bidi_controls`

Flag Trojan-Source bidi override characters (U+202A–202E, U+2066–2069). Defense against [CVE-2021-42574](https://trojansource.codes/).

```yaml
- id: no-bidi
  kind: no_bidi_controls
  paths: "crates/**/src/**/*.rs"
  level: error
  fix:
    file_strip_bidi: {}
```

### `no_zero_width_chars`

Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). A leading U+FEFF is `no_bom`'s concern.

```yaml
- id: no-zwsp
  kind: no_zero_width_chars
  paths: "crates/**/src/**/*.rs"
  level: error
  fix:
    file_strip_zero_width: {}
```

---

## Encoding

### `no_bom`

Flag a leading UTF-8 / UTF-16 LE/BE / UTF-32 LE/BE byte-order mark. The fixer strips whichever BOM is detected.

```yaml
- id: no-bom
  kind: no_bom
  paths: ["**/*.rs", "**/*.toml", "**/*.yml"]
  level: warning
  fix:
    file_strip_bom: {}
```

---

## Structure

### `max_directory_depth`

Tree depth from repo root may not exceed `max`. A shallow depth stops deeply-nested imports and keeps CI path globs sane.

```yaml
- id: shallow-tree
  kind: max_directory_depth
  paths: "**"
  max: 6
  level: warning
```

### `max_files_per_directory`

Per-directory fanout may not exceed `max`. Useful for vendor directories that accidentally grow to thousands of entries.

### `no_empty_files`

Flag zero-byte files. Fixable via `file_remove`.

```yaml
- id: no-empty
  kind: no_empty_files
  paths: "**"
  level: warning
  fix:
    file_remove: {}
```

---

## Portable metadata

Checks that reject tree shapes which work on one OS but break checkouts elsewhere.

### `no_case_conflicts`

Flag paths that differ only by case (e.g. `README.md` + `readme.md`). They can't coexist on macOS HFS+/APFS or Windows NTFS defaults, so a Linux-only dev committing both breaks checkouts for teammates.

### `no_illegal_windows_names`

Reject path components Windows can't represent:

- Reserved device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`) — case-insensitive, regardless of extension. `con.txt` fails; `COM10` and `confused` correctly pass.
- Trailing dots (`foo.`) or trailing spaces (`foo `) — Windows silently strips these on checkout.
- Reserved chars: `<`, `>`, `:`, `"`, `|`, `?`, `*`.

```yaml
- id: portable-names
  kind: no_illegal_windows_names
  paths: "**"
  level: warning
```

---

## Unix metadata

All rules in this family are no-ops on Windows — the +x bit and symlinks don't have a portable cross-platform story, so configs stay identical either way.

### `no_symlinks`

Flag tracked paths that are symbolic links. Symlinks are a portability footgun: Windows NTFS needs admin rights to create them, git-for-Windows can silently flatten them, CI runners vary.

```yaml
- id: no-symlinks
  kind: no_symlinks
  paths: "**"
  level: warning
  fix:
    file_remove: {}   # unlinks the symlink; target is untouched
```

### `executable_bit`

Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`).

```yaml
- id: ci-scripts-exec
  kind: executable_bit
  paths: "ci/**/*.sh"
  require: true
  level: warning
```

No fix op — chmod auto-apply is deferred.

### `executable_has_shebang`

Every file with `+x` set must begin with `#!`. Catches plain text files accidentally marked executable.

### `shebang_has_executable`

Every file starting with `#!` must have `+x` set. Catches scripts that got their `+x` bit stripped by `git add --chmod=-x`, a tar round-trip, or a `cp` across filesystems.

```yaml
- id: scripts-wired
  kind: shebang_has_executable
  paths: "ci/**/*.sh"
  level: warning
```

---

## Git hygiene

### `no_submodules`

Flag the presence of `.gitmodules` at the repo root — always, regardless of `paths`. For general "file X must not exist" checks, use `file_absent`.

```yaml
- id: no-submods
  kind: no_submodules
  level: warning
  fix:
    file_remove: {}
```

Note the fix only deletes `.gitmodules`; `git submodule deinit` and cleaning `.git/modules/` are still on the user.

### `commented_out_code`

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

### `markdown_paths_resolve`

Validate that backticked workspace paths in markdown files resolve to real files or directories in the repo. Targets the AGENTS.md / CLAUDE.md / `.cursorrules` staleness problem: agent-context files reference paths in inline backticks (`` `src/api/users.ts` ``), and those paths drift as the codebase evolves. The `agent-context-no-stale-paths` rule shipped in v0.6 surfaces *candidates* via a regex; this rule does the precise existence check.

```yaml
- id: agents-md-paths-resolve
  kind: markdown_paths_resolve
  paths:
    - AGENTS.md
    - CLAUDE.md
    - .cursorrules
    - "docs/**/*.md"
  prefixes:
    - src/
    - crates/
    - docs/
  level: warning
```

The `prefixes` list is **required** — a backticked token must start with one of these to be considered a path candidate. No defaults: every project's layout differs, and a missing prefix is silent while a wrong default trips false positives.

The scanner skips fenced code blocks (```` ``` ```` / `~~~`) and 4-space-indented blocks; those contain code samples, not factual claims about the tree. Trailing `:line` / `#L<n>` location suffixes are stripped before lookup, as are trailing punctuation and trailing slashes. Glob characters (`*`, `?`, `[`) trigger globset matching against the file index — pass if at least one file matches.

By default the rule skips backticked tokens containing template-variable markers (`{{ }}`, `${ }`, `<…>`). Set `ignore_template_vars: false` to validate them as literal paths.

Check-only — auto-fixing a stale path means guessing the new location, which is unsafe.

### `git_no_denied_paths`

Fire when any tracked file matches a configured glob denylist. The absence-axis companion of `git_tracked_only`: instead of asking "does this tracked path exist?", it asks "is anything tracked that matches my denylist?" One rule covers what would otherwise need one `file_absent` per pattern. Reports every matching denylist entry per offending path so a single file hitting two patterns surfaces both.

```yaml
- id: no-secrets-or-keys
  kind: git_no_denied_paths
  denied:
    - "*.env"
    - ".env*"
    - "*.pem"
    - "id_rsa"
    - "secrets/**"
  level: error
  message: "Don't commit secrets or credentials."
```

Outside a git repo (or when `git` isn't on `PATH`) the rule silently no-ops — the rule's intent only makes sense inside a tracked working tree. Check-only — `git rm --cached` is too destructive to automate.

### `git_commit_message`

Validate HEAD's commit-message shape via regex, max-subject-length, or required-body. At least one of the three must be set; combine all three for full Conventional-Commits-style enforcement. Subject length counts characters, not bytes (a 50-char emoji subject is 50, not 200).

```yaml
- id: conventional-commit
  kind: git_commit_message
  pattern: '^(feat|fix|chore|docs|refactor|test)(\([a-z-]+\))?: '
  subject_max_length: 72
  level: warning

- id: bug-fixes-need-context
  kind: git_commit_message
  pattern: '^fix:'
  requires_body: true
  level: error
  message: "fix: commits must explain what was broken in the body."
```

Outside a git repo, with no commits yet, or when `git` isn't on `PATH`, the rule silently no-ops. Pairs naturally with `alint check --changed` for per-PR enforcement: every PR's tip commit gets validated automatically.

### `git_blame_age`

Fire on lines matching a regex whose `git blame` author-time is older than `max_age_days`. Same regex match shape as `file_content_forbidden`, but with a per-line age gate: a TODO added yesterday passes silently; a TODO that has sat in tree for 18 months fires. Closes the gap between `level: warning` on every TODO (too noisy) and `level: off` (accepts unbounded debt accumulation).

```yaml
- id: stale-todos
  kind: git_blame_age
  paths:
    include: ["**/*.{rs,ts,tsx,js,jsx,py,go,java,kt,rb}"]
    exclude:
      - "**/*test*/**"
      - "**/fixtures/**"
      - "vendor/**"
      - "third_party/**"
  pattern: '\b(TODO|FIXME|XXX|HACK)\b'
  max_age_days: 180
  level: warning
  message: "`{{ctx.match}}` has been here for over 180 days — resolve, convert to a tracked issue, or remove."
```

`{{ctx.match}}` substitutes the regex capture group 1 when present, otherwise the full match — useful for surfacing which marker was caught (`TODO` vs `FIXME` vs …).

Heuristic notes:

- **Formatting passes reset blame age.** `cargo fmt` / `prettier` rewrites every touched line, attributing it to the format commit rather than the original author. List the formatting-sweep commits in `.git-blame-ignore-revs` and git applies the right history automatically.
- **Vendored / imported code** carries the import commit's timestamp — exclude `vendor/`, `third_party/`, generated trees.
- **Squash-merged PRs** collapse to a single commit date, so the squash date wins over the actual edit date.
- **Performance.** `git blame` is O(file_size × commits_touching_file) per file. On large monorepos pair with `alint check --changed` so blame only runs over modified files in CI.

Outside a git repo, on untracked files, or when blame fails for any other reason, the rule silently no-ops per file. Check-only — auto-removing matched lines is destructive and pinning a line as "do nothing" doesn't help.

---

## Cross-file

### `pair`

For every file matching `primary`, a file matching the `partner` template must exist.

```yaml
- id: every-impl-has-test
  kind: pair
  primary: "src/**/*.rs"
  partner: "tests/{stem}.test.rs"
  level: warning
```

### `for_each_dir` / `for_each_file`

For every matching directory / file, evaluate a nested `require:` block with the entry as context. Template tokens (`{dir}`, `{stem}`, `{ext}`, `{basename}`, `{path}`, `{parent_name}`) expand against each match.

```yaml
- id: every-pkg-has-readme
  kind: for_each_dir
  select: "packages/*"
  require:
    - kind: file_exists
      paths: "{path}/README.md"
```

**`when_iter:` — per-iteration filter.** Optional expression in the `when:` grammar, with one extra namespace: `iter.*` references the entry currently being iterated. Iterations whose verdict is false are skipped before any nested rule is built — the canonical use case for monorepos shaped like Cargo / pnpm / Bazel workspaces:

```yaml
- id: workspace-member-has-readme
  kind: for_each_dir
  select: "crates/*"
  when_iter: 'iter.has_file("Cargo.toml")'
  require:
    - kind: file_exists
      paths: "{path}/README.md"
  level: error
```

The `iter` namespace exposes:

| Reference | Type | Notes |
|---|---|---|
| `iter.path` | string | Relative path of the iterated entry. |
| `iter.basename` | string | Basename. |
| `iter.parent_name` | string | Parent dir name. |
| `iter.stem` | string | Basename minus the final extension (mainly useful for files). |
| `iter.ext` | string | Final extension without the dot. |
| `iter.is_dir` | bool | True for `for_each_dir`, false for `for_each_file`; always available. |
| `iter.has_file(pattern)` | bool | Glob match relative to the iterated dir. `iter.has_file("Cargo.toml")`, `iter.has_file("**/*.bzl")`. Always false for file iteration. |

`when_iter:` composes with the rule's outer `when:` (whole-rule gate, evaluated once) and with each nested rule's `when:` (which now also sees the same `iter.*` context). Same field is available on `for_each_file` and `every_matching_has`.

### `dir_contains`

Every directory matching `paths` must contain files matching `require:`. Sugar for a common `for_each_dir` shape.

### `dir_only_contains`

Every directory matching `paths` may contain only files matching `allow:`. Catches stray test data in `src/`.

### `unique_by`

No two files matching `paths` may share the value of `key` (a path template). Catches basename collisions across subdirectories.

```yaml
- id: unique-basenames
  kind: unique_by
  paths: "src/**/*.rs"
  key: "{stem}"
  level: warning
```

### `every_matching_has`

For every file matching `paths`, at least one of `require:` must also exist (at a template-derived location). Lightweight sibling of `pair`.

---

## Plugin (tier 1)

### `command`

Shell out to an external CLI per matched file. Exit `0` is a pass; non-zero is one violation whose message is the (truncated) stdout+stderr. Working directory is the repo root; stdin is closed.

```yaml
- id: workflows-clean
  kind: command
  paths: ".github/workflows/*.{yml,yaml}"
  command: ["actionlint", "{path}"]
  level: error
```

Argv tokens accept the same path-template substitutions as `pair` and `for_each_dir`: `{path}`, `{dir}`, `{stem}`, `{ext}`, `{basename}`, `{parent_name}`. The first token is the program (looked up via `PATH` if it's a bare name).

Environment threaded into the child:

| Var | Value |
|---|---|
| `ALINT_PATH` | matched path (relative to root) |
| `ALINT_ROOT` | absolute repo root |
| `ALINT_RULE_ID` | the rule's `id:` |
| `ALINT_LEVEL` | `error` / `warning` / `info` |
| `ALINT_VAR_<NAME>` | one per top-level `vars:` entry |
| `ALINT_FACT_<NAME>` | one per resolved fact, stringified |

`timeout: <seconds>` (default 30) bounds each invocation; past the limit the child is killed and a violation reports the timeout.

**Trust gate.** `command` rules are only allowed in the user's own top-level config. A `kind: command` rule introduced via `extends:` (local file, HTTPS URL, or `alint://bundled/`) is a load-time error — the same gate that protects `custom:` facts. Adopting a published ruleset must never imply granting it arbitrary process execution.

`--changed` interaction: `command` is a per-file rule, so under `alint check --changed` it spawns only for files in the diff. The expensive check is automatically incremental in CI.

---

## Fix operations

Every `fix:` block uses one of these ops. See [ARCHITECTURE.md](design/ARCHITECTURE.md#fix-operations) for the full cross-reference of which op pairs with which rule kind.

**Path-only** (ignore `fix_size_limit`):

- `file_create: {content, path?, create_parents?}`
- `file_remove: {}`
- `file_rename: {}` (target derived from rule config)

**Content-editing** (skipped on files over `fix_size_limit`; default 1 MiB, `null` disables the cap):

- `file_prepend: {content}`
- `file_append: {content}`
- `file_trim_trailing_whitespace: {}`
- `file_append_final_newline: {}`
- `file_normalize_line_endings: {}` (target read from parent rule)
- `file_strip_bidi: {}`
- `file_strip_zero_width: {}`
- `file_strip_bom: {}`
- `file_collapse_blank_lines: {}` (max read from parent rule)

`fix_size_limit` is a top-level config field:

```yaml
version: 1
fix_size_limit: 1048576   # 1 MiB — the default; `null` disables
rules:
  - ...
```

Over-limit files report `Skipped` with a stderr warning rather than applying the fix.

---

## Bundled rulesets

alint ships a small catalog of pre-built rulesets embedded in the binary. Reference them from `extends:` via the `alint://bundled/<name>@<rev>` scheme:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
```

Bundled rulesets:

- **Resolve offline** — no network fetch, no SRI needed, no cache entry.
- **Are leaf-only** — they don't declare `extends:` of their own.
- **Are versioned independently** — the `@v1` suffix lets rulesets evolve on a separate cadence from the binary. A single binary can ship multiple revisions of the same ruleset.
- **Can be overridden locally** — any rule id declared in your `.alint.yml` wins over the bundled definition. Set `level: off` on a bundled rule id to disable it, or redefine it to tighten severity / change scope.

### `alint://bundled/oss-baseline@v1`

The minimal hygiene baseline most open-source repos want. Nine rules:

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `oss-readme-exists` | `file_exists` | warning | — |
| `oss-readme-non-stub` | `file_min_lines` (3) | info | — |
| `oss-license-exists` | `file_exists` | warning | — |
| `oss-license-non-empty` | `file_min_size` (200 B) | info | — |
| `oss-security-policy-exists` | `file_exists` | info | — |
| `oss-code-of-conduct-exists` | `file_exists` | info | — |
| `oss-gitignore-exists` | `file_exists` | info | — |
| `oss-no-merge-conflict-markers` | `no_merge_conflict_markers` | error | — |
| `oss-no-bidi-controls` | `no_bidi_controls` | error | `file_strip_bidi` |
| `oss-final-newline` | `final_newline` | info | `file_append_final_newline` |
| `oss-no-trailing-whitespace` | `no_trailing_whitespace` | info | `file_trim_trailing_whitespace` |

**Typical overrides:**

```yaml
extends:
  - alint://bundled/oss-baseline@v1

rules:
  # Elevate missing-README from warning to error.
  - id: oss-readme-exists
    level: error

  # Disable trailing-whitespace on Markdown — the two-trailing-spaces
  # hard-break is deliberate.
  - id: oss-no-trailing-whitespace
    level: off
```

### `alint://bundled/rust@v1`

Hygiene checks for Rust projects. Every rule is gated with `when: facts.is_rust` (declared inside the ruleset as `any_file_exists: [Cargo.toml]`), so extending the ruleset from a polyglot repo's root config is safe — rules don't fire unless Rust is actually present.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `rust-cargo-toml-exists` | `file_exists` | error | — |
| `rust-cargo-lock-exists` | `file_exists` | warning | — |
| `rust-toolchain-pinned` | `file_exists` | info | — |
| `rust-no-tracked-target` | `dir_absent` | error | — |
| `rust-sources-snake-case` | `filename_case` | error | `file_rename` |
| `rust-sources-final-newline` | `final_newline` | warning | `file_append_final_newline` |
| `rust-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `file_trim_trailing_whitespace` |
| `rust-sources-no-bidi` | `no_bidi_controls` | error | — |
| `rust-sources-no-zero-width` | `no_zero_width_chars` | error | — |
| `rust-no-merge-markers-in-manifests` | `no_merge_conflict_markers` | error | — |

### `alint://bundled/node@v1`

Hygiene checks for Node.js / npm / pnpm / yarn / bun projects. Every rule is gated with `when: facts.is_node` (via `any_file_exists: [package.json]`), so the ruleset is a safe no-op when `package.json` is absent.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `node-package-json-exists` | `file_exists` | error | — |
| `node-has-lockfile` | `file_exists` | warning | — |
| `node-no-tracked-node-modules` | `dir_absent` | error | — |
| `node-no-tracked-dist` | `dir_absent` | info | — |
| `node-engine-or-nvmrc` | `file_exists` | info | — |
| `node-sources-final-newline` | `final_newline` | info | `file_append_final_newline` |
| `node-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `file_trim_trailing_whitespace` |
| `node-sources-no-bidi` | `no_bidi_controls` | error | — |

### `alint://bundled/python@v1`

Hygiene checks for Python projects. Gated with `when: facts.is_python` (any of `pyproject.toml`, `setup.py`, `setup.cfg`, `requirements.txt` present), so silent no-op when none of those exist.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `python-pyproject-or-setup` | `file_exists` | warning | — |
| `python-no-tracked-pycache` | `dir_absent` | error | — |
| `python-no-tracked-venv` | `dir_absent` | error | — |
| `python-no-tracked-egg-info` | `dir_absent` | warning | — |
| `python-snake-case-modules` | `filename_case` | info | `file_rename` |
| `python-sources-final-newline` | `final_newline` | info | `file_append_final_newline` |
| `python-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `file_trim_trailing_whitespace` |
| `python-sources-no-bidi` | `no_bidi_controls` | error | — |

### `alint://bundled/go@v1`

Hygiene checks for Go modules. Gated with `when: facts.is_go` (any of `go.mod`, `go.sum`).

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `go-go-mod-exists` | `file_exists` | error | — |
| `go-no-tracked-vendor` | `dir_absent` | warning | — |
| `go-no-tracked-bin` | `dir_absent` | info | — |
| `go-sources-final-newline` | `final_newline` | info | `file_append_final_newline` |
| `go-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `file_trim_trailing_whitespace` |
| `go-sources-no-bidi` | `no_bidi_controls` | error | — |

### `alint://bundled/java@v1`

Hygiene checks for Java / Kotlin projects (Gradle or Maven). Gated with `when: facts.is_java` (any of `pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`).

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `java-build-tool-exists` | `file_exists` | error | — |
| `java-no-tracked-build-output` | `dir_absent` | error | — |
| `java-no-tracked-idea` | `dir_absent` | info | — |
| `java-pascal-case-classes` | `filename_case` | info | — |
| `java-sources-final-newline` | `final_newline` | info | `file_append_final_newline` |
| `java-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `file_trim_trailing_whitespace` |
| `java-sources-no-bidi` | `no_bidi_controls` | error | — |

### `alint://bundled/ci/github-actions@v1`

Hygiene checks for `.github/workflows/*.yml`. Gated with `when: facts.has_github_actions` (`.github/workflows/` present), so the ruleset is a safe no-op outside repos using Actions.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `gha-workflows-have-permissions` | `yaml_path_matches` | warning | — |
| `gha-workflows-pin-actions` | `file_content_forbidden` | warning | — |
| `gha-workflows-final-newline` | `final_newline` | info | `file_append_final_newline` |
| `gha-workflows-lf-line-endings` | `line_endings` (lf) | info | `file_normalize_line_endings` |

### `alint://bundled/agent-hygiene@v1`

Catches the canonical agent-driven-development cruft surface — backup-suffix files, scratch docs, debug residue, AI-affirmation prose, model-attributed TODO markers. Composable from existing primitives (`file_absent`, `filename_regex`, `file_content_forbidden`); no new rule kinds. Fires on every repo regardless of language; not gated.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `agent-no-backup-files` | `file_absent` (`*.bak`, `*.orig`, `*~`, `*.swp`) | error | — |
| `agent-no-versioned-duplicates` | `filename_regex` (`*_v2.ts`, `*_old.py`, …) | warning | — |
| `agent-no-scratch-docs-at-root` | `file_absent` (`PLAN.md`, `NOTES.md`, `ANALYSIS.md`, …) | warning | — |
| `agent-no-tracked-env-files` | `git_no_denied_paths` (`*.env`, `.env*`) | error | — |
| `agent-no-debug-residue` | `file_content_forbidden` (`console.log`, `debugger`, `breakpoint()`) | warning | — |
| `agent-no-affirmation-prose` | `file_content_forbidden` (`"You're absolutely right"`, …) | info | — |
| `agent-no-model-attributed-todos` | `file_content_forbidden` (`TODO(claude:)`, `TODO(cursor:)`, …) | warning | — |

The most-cited gripes about agent-generated code surface as a single one-line `extends:` adoption — pair with the per-language ruleset that fits the project.

### `alint://bundled/agent-context@v1`

Hygiene rules for agent-context files (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`). Existence recommended, stub guard via `file_min_lines`, bloat guard via `file_max_lines` (per Augment Code research, context files >300 lines correlate with worse agent performance), stale-path heuristic via regex. Subsumes `ctxlint`'s niche with no new rule kinds — composes `file_exists` / `file_min_lines` / `file_max_lines` / `file_content_forbidden`.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `agent-context-recommend-agents-md` | `file_exists` | info | — |
| `agent-context-not-a-stub` | `file_min_lines` | warning | — |
| `agent-context-not-bloated` | `file_max_lines` (300) | warning | — |
| `agent-context-no-stale-paths` | `file_content_forbidden` (regex heuristic) | info | — |

For precise stale-path detection, layer `markdown_paths_resolve` (a v0.7.1 rule kind) on top of this ruleset — the regex above flags candidates; the rule kind verifies them against the file index.

### `alint://bundled/monorepo@v1`

Language-agnostic monorepo-shape checks. Fires for every directory under `packages/*`, `crates/*`, `apps/*`, or `services/*`. Pair with `rust@v1` / `node@v1` for ecosystem-specific checks on the packages themselves.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `monorepo-packages-have-readme` | `for_each_dir` | warning | — |
| `monorepo-packages-have-package-json` | `for_each_dir` | error | — |
| `monorepo-crates-have-cargo-toml` | `for_each_dir` | error | — |
| `monorepo-unique-package-names` | `unique_by` | warning | — |

### `alint://bundled/monorepo/cargo-workspace@v1`

Workspace-aware overlay for Cargo workspaces. Layered on top of `monorepo@v1` and `rust@v1`. Gated by `facts.is_cargo_workspace` (the root `Cargo.toml` declares `[workspace]`); silently no-ops otherwise. Uses `when_iter: 'iter.has_file("Cargo.toml")'` to scope per-member checks to actual package directories — `crates/notes/` (or any other non-package dir under `crates/`) is filtered out without firing false positives.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `cargo-workspace-members-declared` | `toml_path_matches` | error | — |
| `cargo-workspace-member-has-readme` | `for_each_dir` | warning | — |
| `cargo-workspace-member-declares-name` | `for_each_dir` | warning | — |

### `alint://bundled/monorepo/pnpm-workspace@v1`

Workspace-aware overlay for pnpm workspaces. Gated by `facts.is_pnpm_workspace` (root `pnpm-workspace.yaml` exists). Same `when_iter:` filter pattern, scoped to `packages/*` with `package.json`.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `pnpm-workspace-declares-packages` | `yaml_path_matches` | error | — |
| `pnpm-workspace-member-has-readme` | `for_each_dir` | warning | — |
| `pnpm-workspace-member-declares-name` | `for_each_dir` | warning | — |

### `alint://bundled/monorepo/yarn-workspace@v1`

Workspace-aware overlay for Yarn / npm workspaces (both encode the workspace declaration in the root `package.json`'s `workspaces` field). Gated by `facts.is_yarn_workspace`. Filters per-member iteration to `packages/*` and `apps/*` directories that contain a `package.json`.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `yarn-workspace-declares-workspaces` | `json_path_matches` | error | — |
| `yarn-workspace-member-has-readme` | `for_each_dir` | warning | — |
| `yarn-workspace-member-declares-name` | `for_each_dir` | warning | — |

### `alint://bundled/compliance/reuse@v1`

License-compliance overlay for the FSFE [REUSE Specification](https://reuse.software/) — every licensable file declares its license + copyright via SPDX headers, and the full license texts live under `LICENSES/`. No fact gate; extending the ruleset is the user's signal of intent.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `reuse-licenses-dir-exists` | `dir_exists` | error | — |
| `reuse-source-has-spdx-identifier` | `file_header` | warning | — |
| `reuse-source-has-copyright-text` | `file_header` | warning | — |

Source-file rules cover common code extensions and exclude vendored / build / dist directories. If your project uses `.license` companion files or `REUSE.toml` mappings to license files that can't carry inline headers (binaries, generated code), narrow `paths:` on the source rules.

### `alint://bundled/compliance/apache-2@v1`

License-compliance overlay for projects distributed under the Apache License, Version 2.0. Verifies the three artefacts the license text itself requires of redistributors: a LICENSE with the Apache-2.0 text, a root NOTICE file, and the canonical Apache header on each source file.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `apache-2-license-text-present` | `file_content_matches` | error | — |
| `apache-2-notice-file-exists` | `file_exists` | warning | — |
| `apache-2-source-has-license-header` | `file_header` | warning | — |

Pattern-matches the canonical "Licensed under the Apache License, Version 2.0" substring rather than full bit-for-bit comparison so SPDX templates, apache.org's template, and GitHub's auto-init all parse as compliant. Dual-licensed projects (e.g. Apache-2.0 OR MIT) can extend this ruleset and use `level: off` on rules they don't want firing strictly.

### `alint://bundled/hygiene/no-tracked-artifacts@v1`

The set of paths / files that essentially no repository should commit: build outputs, dependency caches, OS & editor junk, secret-shaped files, oversized blobs. Gitignored directories pass trivially — these rules catch the case where someone committed an artefact and forgot the `.gitignore` entry.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `hygiene-no-node-modules` | `dir_absent` | error | — |
| `hygiene-no-python-cache` | `dir_absent` | error | — |
| `hygiene-no-ruby-bundler-cache` | `dir_absent` | warning | — |
| `hygiene-no-cargo-target` | `dir_absent` | error | — |
| `hygiene-no-js-build-outputs` | `dir_absent` | warning | — |
| `hygiene-no-go-build-cache` | `dir_absent` | info | — |
| `hygiene-no-macos-junk` | `file_absent` | error | `file_remove` |
| `hygiene-no-windows-junk` | `file_absent` | error | `file_remove` |
| `hygiene-no-editor-backups` | `file_absent` | warning | `file_remove` |
| `hygiene-no-env-files` | `file_absent` | error | — |
| `hygiene-no-huge-files` | `file_max_size` | warning (10 MiB) | — |

### `alint://bundled/hygiene/lockfiles@v1`

Lockfiles belong at the workspace root only; nested ones almost always indicate a tooling misconfiguration and cause version drift. One rule per common package manager (npm / pnpm / yarn / bun / Cargo / Poetry / uv). Each uses an `include/exclude` path pair so the root lockfile is exempted while nested copies are flagged.

### `alint://bundled/tooling/editorconfig@v1`

Cross-editor standardization at the root: `.editorconfig` + `.gitattributes` (with a `text=` normalization directive). Three info-level rules — useful as nudges, non-blocking by default.

### `alint://bundled/docs/adr@v1`

Architecture Decision Records following [MADR](https://adr.github.io/madr/) conventions. Files under `docs/adr/` match `NNNN-kebab-case-title.md`; each ADR has `## Status`, `## Context`, and `## Decision` sections. Gap-free numbering is a planned addition once the `numeric_sequence` primitive lands.

## Nested `.alint.yml` (monorepo layering)

Opt into per-subtree configs by setting `nested_configs: true` on the root `.alint.yml`:

```yaml
# /.alint.yml (root)
version: 1
nested_configs: true
rules:
  - id: readme-exists
    kind: file_exists
    paths: ["README.md"]
    root_only: true
    level: warning
```

```yaml
# /packages/frontend/.alint.yml
version: 1
rules:
  - id: frontend-ts-final-newline
    kind: final_newline
    paths: "**/*.ts"
    level: warning
```

```yaml
# /packages/backend/.alint.yml
version: 1
rules:
  - id: backend-rust-snake-case
    kind: filename_case
    paths: "src/**/*.rs"
    case: snake
    level: error
```

At load time, alint walks the tree (respecting `.gitignore` + `ignore:`), picks up every nested `.alint.yml` / `.alint.yaml`, and **prefixes each nested rule's path-like fields** (`paths`, `select`, `primary`) with the relative directory the config lives in. So the frontend rule above evaluates as if it were `paths: "packages/frontend/**/*.ts"` at the root — it fires only on frontend TypeScript files.

### Restrictions (MVP)

- Only the root config sets `nested_configs: true`. Nested configs can't spawn further nesting.
- Nested configs can only declare `version:` and `rules:` — `extends:`, `facts:`, `vars:`, `ignore:`, `respect_gitignore:`, and `fix_size_limit:` are root-only.
- Every rule in a nested config must have a path-like scope field (`paths`, `select`, or `primary`). Rules without any (e.g. `no_submodules`, which is hardcoded to repo root) can't be nested.
- Absolute paths and `..`-prefixed globs are rejected — they'd escape the subtree the config is supposed to confine.
- Rule-id collisions across configs are rejected with a clear error. Per-subtree overrides aren't supported yet; if you want to disable a root rule under one subtree, use a `when:` gate on the root rule for now.

### Planned rulesets (v0.5)

- `alint://bundled/python@v1` — `pyproject.toml`, no `__pycache__`, no committed venv.
- `alint://bundled/java@v1` — Maven / Gradle manifest, standard source layout.
- `alint://bundled/go@v1` — `go.mod`, `go.sum`, no committed `vendor/` without the official workflow.
- `alint://bundled/compliance/reuse@v1` — FSFE REUSE specification (SPDX headers + `LICENSES/`).
- `alint://bundled/compliance/apache-2@v1` — Apache 2.0 headers + `NOTICE` file.

Until those ship, you can compose any of them yourself by pairing `extends:` against an HTTPS URL (with SHA-256 SRI) or a local path.
