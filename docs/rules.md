# alint rule catalogue

> **Reading this on GitHub?** This file is the source for the rendered
> rule reference at <https://alint.org/docs/rules/>, where every rule
> gets its own page with a generated options table. The root-absolute
> cross-links below (e.g. `/docs/concepts/...`) resolve there, not on
> GitHub — open the site for the full, navigable reference.

The full list of rule kinds shipped in alint, organised by family.
Each rule is one line in your `.alint.yml` under `rules:` — see
[ARCHITECTURE.md §DSL](design/ARCHITECTURE.md#dsl) for the common
fields (`id`, `level`, `paths`, `message`, `policy_url`, `when`,
`fix`). The JSON Schema at [`schemas/v1/config.json`](../schemas/v1/config.json)
is the authoritative source for option types.

## Contents

- [Existence](#existence)
- [Content](#content)
- [Structured query](#structured-query)
- [Naming](#naming)
- [Text hygiene](#text-hygiene)
- [Security / Unicode sanity](#security--unicode-sanity)
- [Encoding](#encoding)
- [Structure](#structure)
- [Portable metadata](#portable-metadata)
- [Unix metadata](#unix-metadata)
- [Git hygiene](#git-hygiene)
- [Cross-file](#cross-file)
- [Plugin (tier 1)](#plugin-tier-1)
- [Fix operations](#fix-operations)
- [Bundled rulesets](#bundled-rulesets)
- [Nested `.alint.yml` (monorepo layering)](#nested-alintyml-monorepo-layering)

---

## Existence

### `file_exists`

**Categories:** Existence

Every glob match in `paths` must correspond to a real file. Use an array to accept any of several names.

Fix: `file_create` — write a declared `content`. With an array of `paths`, the fix creates the first entry.

**Optional `git_tracked_only: true`** further requires that the matching file be in git's index — useful for rules like "every release must commit a CHANGELOG entry" where local-only files shouldn't satisfy the requirement. Outside a git repo, the rule fails (no file qualifies). See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

### `file_absent`

**Categories:** Existence

No file matching `paths` may exist in the walked tree. The inverse of `file_exists`.

Fix: `file_remove` — delete every violating file.

**Optional `root_only: true`** (like `file_exists`) restricts the check to the repository root: a file forbidden at the root does not fire on nested copies of the same name.
**Optional `git_tracked_only: true`** restricts the check to files in git's index. With it set, the rule fires only on tracked paths regardless of `.gitignore` state — closing the gap where a `git add -f`'d file slips past the walker's gitignore filter. Outside a git repo the rule becomes a silent no-op.

**Optional `content_prefix_hex`** narrows a name match with a content check: a matching file fires only if its bytes begin with one of the listed hex signatures. This separates real binary junk from unrelated files that share a name pattern — macOS AppleDouble sidecars (`._*`) start with `00 05 16 07` and `.DS_Store` with `00 00 00 01` `"Bud1"`, whereas Hadoop writes `._<name>.crc` checksum files that begin with `crc\0`. A file that cannot be read, or is shorter than every signature, does not match; an empty list (the default) keeps the name-only behaviour.

**What "exists" means**: alint walks the filesystem and honours `.gitignore` by default, so a `file_absent` rule fires whenever a matching file is **present in the walked tree**, not when it's tracked in git. Files filtered by `.gitignore` are invisible to the rule. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics, the `--no-gitignore` flag, and the gap between this and git's actual index.

### `dir_exists`

**Categories:** Existence

Directory counterpart of `file_exists`. Every match must correspond to a real directory in the walked tree.

**Optional `root_only: true`** (like `file_exists`) requires the match to be a
directory directly at the repository root, not nested.
**Optional `git_tracked_only: true`** further requires that the directory contain at least one tracked file. A tree with a `docs/` checked out from a stale clone where every file was later removed via `git rm` would fail under this stricter check. See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

### `dir_absent`

**Categories:** Existence

Directory counterpart of `file_absent`. The match-and-fire semantics are the same as `file_absent` — including the `.gitignore` interaction. A `dir_absent` rule with `paths: "**/target"` only fires when `target/` exists in the walked tree; if it's gitignored, the walker filters it out and the rule stays silent.

**Optional `root_only: true`** (like `dir_exists`) restricts the check to the repository root: a directory forbidden at the root does not fire on nested directories of the same name.
**Optional `git_tracked_only: true`** restricts the check to directories that contain at least one git-tracked file. With it set, a developer's locally-built `target/` (gitignored, no tracked content) doesn't trigger; a `target/` whose contents made it into git's index does. This is the canonical "don't let `target/` be committed" semantic.

See [The walker and `.gitignore`](/docs/concepts/walker-and-gitignore/) for the full semantics.

---

## Content

### `file_content_matches` (alias: `content_matches`)

**Categories:** Content

File contents must contain at least one match for a regex.

```yaml
- id: crate-is-2024-edition
  kind: file_content_matches
  paths: "Cargo.toml"
  pattern: 'edition\s*=\s*"2024"'
  level: error
```

Fix: `file_append` — append declared content.

### `file_content_forbidden` (alias: `content_forbidden`)

**Categories:** Content, Security / Unicode sanity

File contents must NOT match a regex.

```yaml
- id: no-dbg-macros
  kind: file_content_forbidden
  paths: "crates/**/src/**/*.rs"
  pattern: '\bdbg!\('
  level: warning
```

### `file_header` (alias: `header`)

**Categories:** Content

The first N lines must match a regex (line-oriented). For a byte-level prefix check, prefer `file_starts_with`.

```yaml
- id: spdx-header
  kind: file_header
  paths: "src/**/*.rs"
  pattern: "^// SPDX-License-Identifier: MIT"
  level: error
```

Fix: `file_prepend` — inject declared content at the top (preserves UTF-8 BOM).

### `file_starts_with` / `file_ends_with`

**Categories:** Content

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

**Categories:** Content, Security / Unicode sanity

Content SHA-256 must equal the expected digest. Rules-as-tripwire for generated / vendored files that should never drift.

```yaml
- id: schema-frozen
  kind: file_hash
  paths: "schemas/v1/config.json"
  sha256: "0000000000000000000000000000000000000000000000000000000000000000"   # 64 hex chars
  level: error
```

### `file_max_size` (alias: `max_size`)

**Categories:** Content, Structure

File must be at most `max_bytes` in size. Catches accidental large-blob commits.

```yaml
- id: no-huge-blobs
  kind: file_max_size
  paths: "**"
  max_bytes: 5242880   # 5 MiB
  level: warning
```

### `file_min_size` (alias: `min_size`)

**Categories:** Content, Structure

File must be at least `min_bytes` in size. Catches placeholder / stub files that pass existence checks but add no information (a 0-byte `LICENSE`, a `README.md` with only a title).

```yaml
- id: license-non-empty
  kind: file_min_size
  paths: ["LICENSE", "LICENSE.md", "LICENSE-APACHE", "LICENSE-MIT"]
  min_bytes: 200
  level: warning
```

### `file_min_lines` (alias: `min_lines`)

**Categories:** Content, Structure

File must have at least `min_lines` lines (`\n`-terminated, with an unterminated trailing segment counting as one more — `wc -l` semantics). Use for "README has more than a title and a TODO".

```yaml
- id: readme-non-stub
  kind: file_min_lines
  paths: ["README.md", "README"]
  min_lines: 5
  level: info
```

### `file_max_lines` (alias: `max_lines`)

**Categories:** Content, Structure

File must have at most `max_lines` lines, using the same accounting as `file_min_lines`. Catches the everything-module anti-pattern — a `lib.rs` / `index.ts` / `helpers.py` that grew unbounded.

```yaml
- id: cap-source-file-size
  kind: file_max_lines
  paths: "src/**/*.rs"
  max_lines: 800
  level: warning
```

### `file_footer` (alias: `footer`)

**Categories:** Content

Last `lines` lines of each file in scope must match a regex. Mirror of `file_header` anchored at the end of the file. Use for license footers, signed-off-by trailers, generated-file sentinels.

```yaml
- id: license-footer
  kind: file_footer
  paths: "src/**/*.rs"
  pattern: "Licensed under the Apache License, Version 2\\.0"
  lines: 3
  level: error
```

Fix: `file_append` — append a declared `content`. With no fix declared, violations are unfixable.

### `file_shebang` (alias: `shebang`)

**Categories:** Content, Unix metadata

First line of each file in scope must match the `shebang` regex. Pairs with `executable_has_shebang` (which checks shebang *presence* on `+x` files) — `file_shebang` checks shebang *shape*.

```yaml
- id: scripts-use-env-bash
  kind: file_shebang
  paths: "scripts/*.sh"
  shebang: '^#!/usr/bin/env bash$'
  level: error
```

Default `shebang:` is `^#!`, which only enforces presence; almost every useful config supplies a tighter regex pinning the interpreter.

### `file_is_text` (alias: `is_text`)

**Categories:** Content, Encoding

Content is detected as text (magic bytes + UTF-8 validity check) — fails on binary files matched by `paths`.

```yaml
- id: configs-are-text
  kind: file_is_text
  paths: ".github/**/*.{yml,yaml}"
  level: error
```

### `file_is_ascii`

**Categories:** Content, Encoding, Security / Unicode sanity

Every byte in the file must be < 0x80 (pure ASCII), except codepoints listed in `allow:`. Strict variant of `is_text` for configs that must round-trip through strictly-ASCII tools. `allow:` exempts specific non-ASCII codepoints — each entry a single character (`"ö"`), a `U+XXXX` codepoint, or a `U+XXXX-U+YYYY` inclusive range (curl keeps its source ASCII but allows `ö` in "Björn"; the recurring need across llvm / vscode / elixir). With `allow:` the file is decoded as UTF-8 and checked per character; without it, the strict byte-level fast path is used.

```yaml
- id: source-ascii-but-allow-accents
  kind: file_is_ascii
  paths: "src/**"
  allow: ["ö", "U+00E9", "U+2010-U+2015"]   # ö, é, and the dash block
  level: error
```

---

## Structured query

JSONPath queries over structured documents per [RFC 9535](https://datatracker.ietf.org/doc/html/rfc9535). JSON / YAML / TOML / XML targets coerce into the same `serde_json::Value` tree, so a single rule works across all four formats — Kubernetes manifests, GitHub Actions workflows, `package.json`, `Cargo.toml`, `pyproject.toml`, Maven `pom.xml`, .NET `.csproj` / `.props` / `.targets`. JSON/YAML/TOML coerce through serde; XML maps via the [XML-mapping convention](#xml-mapping) documented below. JSON parsing is **JSONC-tolerant**: a `.json` file that carries `//` or `/* … */` comments or trailing commas (`tsconfig.json`, `.vscode/*.json`, and other JS/TS-ecosystem files) still parses — strict JSON is tried first (and is byte-identical), the tolerant retry only kicks in on failure, and a genuinely-malformed document still reports the strict parser's error. The same tolerance applies to the `json:` extract used by `cross_file` / `registry_paths_resolve`.

### `json_path_equals`, `yaml_path_equals`, `toml_path_equals`, `xml_path_equals`

**Categories:** Structured query

Query a structured document with a JSONPath expression and assert every match deep-equals the supplied value.

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

- id: csproj-targets-net8
  kind: xml_path_equals
  paths: "**/*.csproj"
  path: "$.Project.PropertyGroup.TargetFramework"
  equals: "net8.0"
  level: error
```

**Semantics**:
- Multiple matches — every match must equal the expected value.
- Zero matches — counts as a violation (the key the rule is enforcing doesn't exist).
- Unparseable files — one violation per file (not silently skipped).

<a id="xml-mapping"></a>
**XML mapping** (`xml_path_*`): XML is mapped to the queryable tree with the xmltodict-style convention so the JSONPath reads like the XML — the document is `{ <root-element>: … }` (`$.Project…`, `$.project…`); attributes are `@name` keys (`['@Version']`); a leaf element collapses to its text (`<TargetFramework>net8.0</TargetFramework>` → `"net8.0"`); repeated sibling elements become an array (use `dependency[*]`, which works for one or many); namespaces flatten to the local name (Maven's default `pom.xml` namespace just works). **Every XML leaf value is a string** — quote the expected value (`equals: "4.0.0"`, not `equals: 4.0.0`) or use `xml_path_matches`. Full rationale and edge cases: `docs/design/v0.10/xml_path.md`.

### `json_path_matches`, `yaml_path_matches`, `toml_path_matches`, `xml_path_matches`

**Categories:** Structured query

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

- id: packageref-has-version
  kind: xml_path_matches
  paths: "**/*.csproj"
  path: "$.Project.ItemGroup.PackageReference[*]['@Version']"
  matches: '^\d'
  level: error

- id: crate-version-is-semver
  kind: toml_path_matches
  paths: "crates/*/Cargo.toml"
  path: "$.package.version"
  matches: '^\d+\.\d+\.\d+$'
  level: error
```

### `json_schema_passes`

**Categories:** Structured query

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

---

## Naming

### `filename_case`

**Categories:** Naming

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

**Categories:** Naming

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

**Categories:** Text hygiene

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

**Categories:** Text hygiene

File must end with a single `\n`. Fixable via `file_append_final_newline`.

```yaml
- id: text-files-final-newline
  kind: final_newline
  paths: "**/*.{md,yml,yaml,toml,sh}"
  level: warning
  fix:
    file_append_final_newline: {}
```

### `line_endings`

**Categories:** Text hygiene, Portable metadata

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

**Categories:** Text hygiene

Cap line length in characters (not bytes — code points). Optional `tab_width` for tab expansion.

```yaml
- id: docs-80-col
  kind: line_max_width
  paths: "docs/**/*.md"
  max_width: 80
  level: info
```

### `indent_style`

**Categories:** Text hygiene

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

**Categories:** Text hygiene

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

**Categories:** Security / Unicode sanity, Text hygiene

Flag `<<<<<<< `, `=======`, `>>>>>>> `, `||||||| ` markers at the start of a line — almost always left over from an unresolved merge. The anchor markers carry a trailing ref (`<<<<<<< HEAD`), so they never collide with prose; a bare `=======` is reported only when the file also contains one of those anchors, because on its own a seven-character `=======` is indistinguishable from a reST/Markdown setext heading underline (so docs trees no longer need to be excluded).

```yaml
- id: no-conflicts
  kind: no_merge_conflict_markers
  paths: "**"
  level: error
```

### `no_bidi_controls`

**Categories:** Security / Unicode sanity, Encoding

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

**Categories:** Security / Unicode sanity, Encoding

Flag body-internal zero-width characters (U+200B, U+200C, U+200D, and non-leading U+FEFF). A leading U+FEFF is `no_bom`'s concern.

As of v0.14 the detection set also covers U+2060 (word joiner) and U+180E (Mongolian vowel separator).

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

**Categories:** Encoding, Text hygiene

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

**Categories:** Structure

Tree depth from repo root may not exceed `max_depth`. A shallow depth stops deeply-nested imports and keeps CI path globs sane.

```yaml
- id: shallow-tree
  kind: max_directory_depth
  paths: "**"
  max_depth: 6
  level: warning
```

### `max_files_per_directory`

**Categories:** Structure

Per-directory fanout may not exceed `max_files`. Useful for vendor directories that accidentally grow to thousands of entries.

```yaml
- id: vendor-dir-fanout-cap
  kind: max_files_per_directory
  paths: "vendor/**"
  max_files: 200
  level: warning
```

### `no_empty_files`

**Categories:** Structure

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

**Categories:** Portable metadata, Naming

Flag paths that differ only by case (e.g. `README.md` + `readme.md`). They can't coexist on macOS HFS+/APFS or Windows NTFS defaults, so a Linux-only dev committing both breaks checkouts for teammates.

```yaml
- id: no-case-colliding-paths
  kind: no_case_conflicts
  paths: "**"
  level: error
```

### `no_illegal_windows_names`

**Categories:** Portable metadata, Naming

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

**Categories:** Unix metadata, Portable metadata, Security / Unicode sanity

Flag tracked paths that are symbolic links. Symlinks are a portability footgun: Windows NTFS needs admin rights to create them, git-for-Windows can silently flatten them, CI runners vary.

Caveat: a symlink whose target escapes the repository root (`link -> /etc`) is pruned by the walker *before* indexing, so it is **not** flagged — the rule reports in-tree symlinks (to files or directories), not escaping ones. The escaping symlink can't be read out-of-root either (path confinement blocks that), so this is a reporting gap, not a disclosure one; recording escaping symlinks safely is a tracked follow-up.

Fix: `file_remove` — unlinks the symlink; the target is untouched.

### `executable_bit`

**Categories:** Unix metadata

Assert every file in scope either has the `+x` bit set (`require: true`) or does not (`require: false`).

No fix op — chmod auto-apply is deferred.

### `executable_has_shebang`

**Categories:** Unix metadata, Content

Every file with `+x` set must begin with `#!`. Catches plain text files accidentally marked executable.

### `shebang_has_executable`

**Categories:** Unix metadata, Content

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

**Categories:** Git hygiene

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

**Categories:** Git hygiene, Content

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

**Categories:** Git hygiene, Cross-file

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

**Categories:** Git hygiene, Security / Unicode sanity

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

An optional `since: <git-ref>` scopes the check to denied paths that changed in the `<ref>...HEAD` diff — the PR-scoped shape, which catches a secret added in the PR even if HEAD's tree still tracks an older one. It accepts the `{{env.X}}` interpolation (e.g. `since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"`); an unresolvable ref hard-fails with a shallow-clone hint.

Outside a git repo (or when `git` isn't on `PATH`) the rule silently no-ops — the rule's intent only makes sense inside a tracked working tree. Check-only — `git rm --cached` is too destructive to automate.

### `git_commit_message`

**Categories:** Git hygiene

Validate commit-message shape via regex, max-subject-length, or required-body. At least one of the three must be set; combine all three for full Conventional-Commits-style enforcement. Subject length counts characters, not bytes (a 50-char emoji subject is 50, not 200).

Two modes, selected by the optional `since:` field:

- **HEAD-only** (default, `since:` omitted): validate the tip commit. Right shape for push-trigger CI and post-commit hooks.
- **Range** (`since:` set): validate every commit reachable from HEAD but not from `since`. Right shape for `pull_request`-trigger CI, where `actions/checkout` checks out a synthetic merge commit whose subject the rule would always flag.

#### `since:` semantics

`since:` accepts anything `git rev-parse` resolves: a 40-char or abbreviated SHA, a branch (`origin/main`), a tag (`v1.2.3`), or a relative ref (`HEAD~5`). The rule walks `<since>..HEAD` oldest-first, validates each commit, and emits one violation per failing commit with the short SHA + a subject snippet so you know which to amend.

POSIX-style env-var interpolation is supported:

- `${VAR}` substitutes the value of `VAR`. Unset (or empty) is a hard error with a CI-friendly hint.
- `${VAR:-default}` substitutes `VAR`, or `default` when `VAR` is unset or empty.

The GitHub Actions double-brace template syntax `${{ ... }}` is **not** interpolated by alint; it has to be rendered by Actions before the YAML is read, which only works in workflow files, not in `.alint.yml`. Use the single-brace `${VAR}` form and export the var in a workflow step.

#### `include_merges:`

In range mode, merge commits are skipped by default (`include_merges: false`). Merge subjects in PR contexts are typically `actions/checkout`-generated or maintainer-resolved and uninteresting. Set `include_merges: true` to lint them too. Has no effect when `since:` is unset; combining `include_merges: true` with no `since:` is a load-time error.

#### Failure modes

- **No git, or `git` not on PATH**: silent no-op. The rule's intent only makes sense inside a git repo.
- **`since:` ref doesn't resolve**: hard error with a shallow-clone hint. The common cause is `actions/checkout@v4` with its default `fetch-depth: 1`, which doesn't fetch the base ref's commits. Use `fetch-depth: 0` to fetch full history.
- **Range is empty** (`since` == HEAD on a force-push, or no non-merge commits): silent no-op. No commits, no policy to apply.

#### GitHub Actions PR-validation recipe

```yaml
# .github/workflows/lint.yml
name: lint
on:
  pull_request:
    branches: [main]
jobs:
  alint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          # Range mode walks <base>..HEAD. fetch-depth: 0 makes
          # both refs reachable; the default depth of 1 leaves
          # the base ref out of local objects and the rule errors.
          fetch-depth: 0
      - name: alint check
        env:
          ALINT_BASE_SHA: ${{ github.event.pull_request.base.sha }}
        uses: asamarts/alint@v0.13.0
```

### `git_commit_signed_off`

**Categories:** Git hygiene

Assert every commit in scope carries a DCO (Developer Certificate of Origin) `Signed-off-by:` trailer — required by every CNCF / Linux Foundation / kernel-style project. A commit lacking the trailer fires one violation, with the short SHA + subject snippet so you know which to amend (`git commit --amend -s` or `git rebase --signoff`).

```yaml
# HEAD-only: the tip commit must be signed off.
- id: dco
  kind: git_commit_signed_off
  level: error

# Range mode for PR CI: every commit in the PR must be signed off.
- id: pr-dco
  kind: git_commit_signed_off
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

The default `pattern:` is the canonical DCO shape `(?m)^Signed-off-by: .+ <.+@.+>$`. Override `pattern:` to enforce a stricter form (e.g. a corporate-domain email). Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes (silent outside a git repo; a bad `since:` ref hard-fails with a shallow-clone hint). See [variable interpolation](/docs/concepts/variable-interpolation/) for the `{{env.X}}` form.

### `git_commit_no_fixup`

**Categories:** Git hygiene

Fail on residual `fixup!` / `squash!` / `amend!` commits left in scope — the ones `git commit --fixup` / `--squash` produce, meant to be collapsed by `git rebase --autosquash` before merging. Forgetting to rebase is the universal case; this rule catches the leftover so it doesn't land on the main branch.

```yaml
# Range mode for PR CI: no un-squashed fixups may merge.
- id: no-fixup
  kind: git_commit_no_fixup
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

No configuration knobs — the matched subject prefixes are exactly what `--autosquash` understands. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes.

### `git_commit_subject_matches`

**Categories:** Git hygiene

Each commit's subject line (the first line of its message) must match the `matches:` regex — the subject-grammar member of the commit family. Enforces a prefix + shape convention like go / Gerrit's `pkg/path: lowercase summary`, node's `subsystem: description`, or conventional-commit types. The regex is anchored to the **subject alone** (so `^…$` describes the first line exactly), unlike `git_commit_message`'s `pattern:` which matches the whole subject + body; for a subject-length cap use `git_commit_message`'s `subject_max_length:`. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes (HEAD-only when `since:` is unset, `<since>..HEAD` when set; silent outside a git repo; a bad `since:` ref hard-fails with a shallow-clone hint).

```yaml
- id: subject-grammar
  kind: git_commit_subject_matches
  matches: '^[a-z0-9_/.-]+: [a-z].{0,70}$'   # `component: lowercase summary`
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

### `git_commit_author_allowlist`

**Categories:** Git hygiene, Security / Unicode sanity

Assert every commit author in scope matches an allowed email and/or name pattern. At least one of `email_pattern:` / `name_pattern:` is required; specifying both means BOTH must match (AND). A commit whose author fails any specified pattern fires one violation. Demand: enterprise repos enforcing contributor identity against a corporate domain; OSS projects catching commits from sock-puppet or compromised accounts.

```yaml
# Every commit in the PR must be authored from the corporate domain.
- id: org-authors-only
  kind: git_commit_author_allowlist
  email_pattern: '^.+@example\.com$'
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

`email_pattern:` matches `git log %ae`; `name_pattern:` matches `git log %an`. Both are Rust regexes. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes (silent outside a git repo; a bad `since:` ref hard-fails with a shallow-clone hint).

### `git_commit_gpg_signed`

**Categories:** Git hygiene, Security / Unicode sanity

Assert every commit in scope has a verifying signature (`git verify-commit` exits 0). A commit that is unsigned — or signed with a key that doesn't verify against the local keyring — fires one violation. Demand: kernel maintainers, security-sensitive OSS, anyone using GitHub's "Require signed commits" branch protection.

```yaml
# Every commit in the PR must carry a verifying signature.
- id: signed-commits
  kind: git_commit_gpg_signed
  since: "{{env.ALINT_BASE_SHA | default('origin/main')}}"
  level: error
```

The rule reflects git's own verdict and deliberately does **not** distinguish "unsigned" from "signed with an untrusted key" — trust is git's GPG config / `.git/allowed_signers`, not this rule's job. No configuration knobs. Shares the commit-validation family's `since:` / `include_merges:` semantics and failure modes.

### `git_blame_age`

**Categories:** Git hygiene

Fire on lines matching a regex whose `git blame` author-time is older than `max_age_days`. Same regex match shape as `file_content_forbidden`, but with a per-line age gate: a TODO added yesterday passes silently; a TODO that has sat in tree for 18 months fires. Closes the gap between `level: warning` on every TODO (too noisy) and `level: off` (accepts unbounded debt accumulation).

`{{ctx.match}}` substitutes the regex capture group 1 when present, otherwise the full match — useful for surfacing which marker was caught (`TODO` vs `FIXME` vs …).

Heuristic notes:

- **Formatting passes reset blame age.** `cargo fmt` / `prettier` rewrites every touched line, attributing it to the format commit rather than the original author. List the formatting-sweep commits in `.git-blame-ignore-revs` and git applies the right history automatically.
- **Vendored / imported code** carries the import commit's timestamp — exclude `vendor/`, `third_party/`, generated trees.
- **Squash-merged PRs** collapse to a single commit date, so the squash date wins over the actual edit date.
- **Performance.** `git blame` is O(file_size × commits_touching_file) per file. On large monorepos pair with `alint check --changed` so blame only runs over modified files in CI.

Outside a git repo, on untracked files, or when blame fails for any other reason, the rule silently no-ops per file. Check-only — auto-removing matched lines is destructive and pinning a line as "do nothing" doesn't help.

### `changeset_requires_path`

**Categories:** Git hygiene, Cross-file

The `<since>...HEAD` diff must **add** (git status `A`) at least one path matching `add_glob:` — the "did you add a changelog entry?" gate. Three corpus signals: prettier's `changelog_unreleased/`, cpython's `Misc/NEWS.d/next/`, pnpm's `.changeset/*.md`. `since:` (the base ref) is required — the rule asserts about the *set of files a contribution adds*, so it's diff-scoped. An optional `when_changed:` gates the requirement on some other glob having changed (don't demand a changelog for a docs-only PR); with no gate, any non-empty changeset triggers it. Builds on the same `<since>...HEAD` three-dot (merge-base) diff as `alint check --changed`. Silent no-op outside a git repo or when nothing relevant changed; a `since:` that fails to resolve hard-fails with a shallow-clone hint.

---

### `pair_changed_together`

**Categories:** Git hygiene, Cross-file

If the `<since>...HEAD` diff changes any path matching `if_changed:`, at least one path matching `then_changed:` must change in the same range — the **co-change** gate. Corpus signals: rust's `rustdoc-json-types` `FORMAT_VERSION` must bump when the format struct changes; "`version.txt` and the lockfile change together" release guards. Both globs and `since:` (the base ref) are required. **Directional** — the trigger is `if_changed`, the obligation is `then_changed`; a `then_changed`-only change never fires it, so add a second rule with the globs swapped for a bidirectional pact. The `changeset_requires_path` sibling, built on the same merge-base diff as `alint check --changed`. Silent no-op outside a git repo or when `if_changed` didn't change; a `since:` that fails to resolve hard-fails with a shallow-clone hint.

---

## Cross-file

### `pair`

**Categories:** Cross-file

For every file matching `primary`, a file matching the `partner` template must exist.

```yaml
- id: every-impl-has-test
  kind: pair
  primary: "src/**/*.rs"
  partner: "tests/{stem}.test.rs"
  level: warning
```

### `pair_hash`

**Categories:** Cross-file, Security / Unicode sanity

The `algorithm` digest (`sha256` default / `sha512`) of every file matching `source` must appear in the single `target` file — either as an embedded hex substring (`format: contains`, default) or a `<hex>  <path>` manifest line (`format: sums-line`, where the path token must be the source's path; a leading `*` binary marker and a `./` prefix are tolerated). The sums-line parser accepts **either order** — coreutils / go-`.sum` `<hex> <path>` *and* the Go FIPS snapshot's path-first `<path> <hex>` — by identifying the digest token by its shape (the algorithm fixes its hex length). One violation per source whose digest is absent or mismatched; a missing `target` is one violation anchored on `target`. Raw bytes are hashed (a CRLF/newline change *is* a digest change — it is an integrity pin). Detection-only: alint never regenerates the manifest (same posture as `file_hash`). The sibling of `file_hash` (one file vs a *literal* hash in the config) and `generated_file_fresh` (a *generator's* stdout); `pair_hash` is the cross-file "B carries A's current digest" relation. golang/go FIPS `fips140.sum` is the canonical, highest-stakes use.

```yaml
- id: fips-sum-pins-module
  kind: pair_hash
  source: "src/crypto/internal/fips140/v1.0.0/**/*.go"
  target: "src/crypto/internal/fips140/fips140.sum"
  algorithm: sha256
  format: sums-line
  level: error
```

### `registry_paths_resolve`

**Categories:** Cross-file

A manifest file enumerates path entries; each must resolve to an on-disk artefact. `extract` pulls the entry list via a structured query (`toml` / `json` / `yaml` RFC 9535 JSONPath), a line list (`lines`, optional `comment` prefix), or a regex capture (`regex`, group 1). `expect` (`any` / `file` / `dir`) and `must_contain` constrain the resolved kind; `exclude_query` subtracts entries; `entries_are_globs` expands each entry as a glob. Non-literal entries (interpolation / antiquotation) are skipped, not failed. Optional `orphans` adds the reverse-completeness check: on-disk artefacts under the `space` glob that no entry references (the "new crate not wired into the workspace" detector). Cross-file: reads one manifest, resolves against the file index.

```yaml
- id: workspace-members-resolve
  kind: registry_paths_resolve
  source: Cargo.toml
  extract: { toml: "$.workspace.members[*]" }
  expect: dir
  must_contain: Cargo.toml
  orphans: { space: "crates/*/Cargo.toml", unreferenced: warn }
  level: error
```

### `cross_file` (alias: `cross_file_value_equals`)

**Categories:** Cross-file

A `source` must hold a `relation` to one or more `targets` (or, for `resolves`, the filesystem). The `source` is a single `{ file, extract }` — **or**, for the set relations only (`subset` / `superset` / `set_equals`), `{ files: <glob>, extract }`, whose matches are read and whose extracted values are **unioned into one set** (the "every `*hl-X*` across `runtime/doc/*.txt` must equal the `default link X` set in `highlight.c`" shape — symbol-set / cross-language parity). For the value relations, `targets` is either a `{ files: <glob>, extract }` map (one query applied per glob match) or a sequence of `{ file, extract }` (heterogeneous pins); `extract` is the same one-of as `registry_paths_resolve` (`toml`/`json`/`yaml` JSONPath, `lines`, `regex` group 1) plus `whole_file: {}` (the entire file content as one value — for byte-level content equality without `identical`'s no-`extract`/no-`normalize` constraint). `relation` (default `equals`) selects the assertion, checked independently per target; the *shape* (which of `source.file`/`source.files`, `source.extract`, `targets` is present) follows the relation and is validated at load:

| `relation` | source ⇒ | asserts (per target) |
|---|---|---|
| `equals` | exactly one value `v` | every target value `== v` |
| `subset` | a set `S` | `S ⊆ T` (singleton `S` = membership) |
| `superset` | a set `S` | `S ⊇ T` (the source covers every target value) |
| `set_equals` | a set `S` | `S == T` |
| `identical` | whole file content | the target is byte-identical (no `extract`; optional `skip_header_lines`) |
| `resolves` | a set of paths | each path exists on disk (no `targets`; the forward half of `registry_paths_resolve`) |

`normalize` relaxes the value comparison — a single transform or an ordered list applied left-to-right (`normalize: [trim, semver-minor]`): `trim`, `lower`, `semver-major` (the leading `MAJOR` band — the dotnet SDK shape), and `semver-minor` (the leading `MAJOR.MINOR` band, each token's leading digits with a non-digit prefix stripped — so `4.36-dev`, `4.36.0`, `pnpm@11.3.0` and `>=22.13` reconcile; the protobuf / pnpm version-format case). `identical` reads whole files byte-for-byte (`normalize`/`extract` do not apply), with an optional `skip_header_lines` to ignore a differing license/header; `resolves` extracts paths from the `source` (no `targets`) and checks each exists relative to the source file's directory. Non-literal extracted values (interpolation / antiquotation) are skipped, not failed — **except** a `whole_file: {}` source/target, whose single value (the entire file content) is compared verbatim even when it embeds `${…}`/`{{…}}` markers (those mark interpolated *paths*, not content); `whole_file` still honours `normalize`, so it sits between a query-extract and a no-normalize `identical`. `allow_missing_target` controls absent files/values. `equals` requires the `source` to extract **exactly one** value; to pull the latest entry from a multi-match file (e.g. the newest version in a multi-release `CHANGELOG.md`), anchor the regex so it captures only the first match — `regex: '(?s)\A.*?## (\d+\.\d+\.\d+)'` (`(?s)` makes `.` cross newlines, `\A` anchors at the start, `.*?` reaches the first heading lazily) yields a single value (a leading `## Unreleased` is skipped because the version pattern doesn't match it). The released `cross_file_value_equals` is a **byte-compatible alias** (`relation` defaults to `equals`). Cross-file.

```yaml
# equals (the default; the cross_file_value_equals shape)
- id: workspace-versions-coherent
  kind: cross_file
  source:  { file: Cargo.toml, extract: { toml: "$.workspace.package.version" } }
  targets: { files: "crates/*/Cargo.toml", extract: { toml: "$.package.version" } }
  relation: equals
  level: error

# subset — every catalog reference must resolve to a declared catalog key
- id: pnpm-catalog-refs-resolve
  kind: cross_file
  source:  { file: pnpm-workspace.yaml, extract: { yaml: "$.catalog.*" } }
  targets: { files: "packages/**/package.json", extract: { regex: 'catalog:(\S+)' } }
  relation: subset
  level: error

# identical — each crate README must mirror the workspace README byte-for-byte
- id: readme-mirrors-root
  kind: cross_file
  source:  { file: README.md }
  targets: { files: "crates/*/README.md" }
  relation: identical
  level: error

# resolves — every declared workspace member path must exist on disk
- id: workspace-members-exist
  kind: cross_file
  source: { file: Cargo.toml, extract: { toml: "$.workspace.members[*]" } }
  relation: resolves
  level: error
```

### `file_graph`

**Categories:** Cross-file

Assemble the repo's *file → file* reference graph and assert a global structural property the 1-level cross-file kinds can't express. `nodes` (a glob) selects the graph's files. The `edges` block takes one of two extractors: `from_content` (extract one reference per match — `extract` is the same one-of as `registry_paths_resolve`: `toml` / `json` / `yaml` JSONPath, `lines`, `regex` capture group 1 — then `resolve` it to a path, `relative_to_file` default or `relative_to_repo_root`) for the reference-graph modes, or `derive_target` (`{ from: <regex on the node path>, to: <template, e.g. $1.pb.go> }`) for the `fresh` codegen-freshness mode **or** the `no_dangling` derived-sibling-existence mode. Bare module names, absolute paths, URLs, and computed/interpolated references are **dropped, not mis-resolved** (resolving module *names* is the package-graph non-goal — nodes stay path-based). `require` is a closed set — three bare-string modes and three configured map modes: `acyclic` (no dependency cycle among the nodes, each reported once as a rotation-canonical path list); `no_dangling` (every path-shaped edge must resolve to a path that exists on disk — the doc-cross-link / generic `markdown_paths_resolve` integrity check; with `edges.derive_target` it instead asserts each node's *derived* sibling exists, e.g. every `licenses/X-LICENSE.txt` needs an `X-NOTICE.txt`); `no_orphans` (no node is unreferenced by another node, except those matching a `roots:` glob — the registry / staging orphan detector); `{ forbidden_edges: [{ from, to }] }` (one violation per edge whose source matches `from` and resolved target matches `to` — the whole-repo layering firewall, where `import_gate` is the cheap per-file version); `{ no_orphans: { roots: [...] } }` (the `no_orphans` form with declared entry points); and `{ fresh: { hash, marker } }` (needs `edges.derive_target`: the generated file must embed the source's current `hash` digest, captured by `marker` group 1 — content-hash, never mtime; the alint-native form of generate-then-`git diff`, with no generator run). Pure-parse and extraction-based: it never shells out. Cross-file (whole-index).

```yaml
# Layering: domain code must not reach into infra (file → file).
- id: domain-not-depend-on-infra
  kind: file_graph
  nodes: "src/**/*.ts"
  edges:
    from_content:
      extract: { regex: 'from\s+"(\.[^"]+)"' }
      resolve: relative_to_file
  require:
    forbidden_edges:
      - { from: "src/domain/**", to: "src/infra/**" }
  level: error

# Acyclicity: the clearest capability gap — no current kind detects cycles.
- id: no-proto-import-cycles
  kind: file_graph
  nodes: "proto/**/*.proto"
  edges:
    from_content:
      extract: { regex: 'import\s+"([^"]+)"' }
      resolve: relative_to_repo_root
  require: acyclic

# Integrity: every doc cross-link resolves, and no doc is unreferenced
# except the declared entry points.
- id: docs-links-resolve
  kind: file_graph
  nodes: "docs/**/*.md"
  edges:
    from_content:
      extract: { regex: '\]\((\.[^)]+\.md)\)' }
      resolve: relative_to_file
  require: no_dangling

- id: no-orphan-docs
  kind: file_graph
  nodes: "docs/**/*.md"
  edges:
    from_content:
      extract: { regex: '\]\((\.[^)]+\.md)\)' }
      resolve: relative_to_file
  require:
    no_orphans:
      roots: ["docs/index.md", "docs/README.md"]

# Freshness: each generated *.pb.go must embed the sha256 of its .proto
# source (the alint-native, no-spawn form of `make gen && git diff`).
- id: generated-stays-fresh
  kind: file_graph
  nodes: "proto/**/*.proto"
  edges:
    derive_target:
      from: '(.*)\.proto'
      to: '$1.pb.go'
  require:
    fresh:
      hash: sha256
      marker: 'sha256:([0-9a-f]{64})'
```

### `ordered_block`

**Categories:** Cross-file, Text hygiene

The lines between a `start` / `end` marker pair must stay sorted (and, with `unique: true`, free of duplicates) under `comparator` (`lexical` / `lexical-ci` / `numeric`). **Both markers are optional**: omit `end` to sort from `start` to EOF, omit both to sort the whole file (the markerless "this file is one sorted list" form — dictionaries, allow-lists, a fully-sorted `CODEOWNERS`). The generic form of per-project keep-sorted scripts (protobuf `failure_lists`, sorted `.gitignore` / `CODEOWNERS` / dependency lists). Per-file: with markers, a file with no `start` marker is silently fine; markers match the trimmed line; blank lines inside a block are ignored; one violation per out-of-order block; a fully-delimited block that never sees its `end` is reported `unclosed` (a block with an absent `end` runs to EOF by design). An optional `select:` regex restricts the sortable entries to lines matching it — other lines inside the block (comments, group headers) pass through untouched (the sectioned / keep-sorted-subset shape).

```yaml
- id: keep-sorted
  kind: ordered_block
  paths: ["**/.gitignore", "CODEOWNERS"]
  start: "# keep-sorted start"
  end: "# keep-sorted end"
  comparator: lexical
  unique: false
  select: '^\s*require '   # sort only the `require '…'` lines
  level: warning
```

### `for_each_match`

**Categories:** Cross-file

For each line matching `select` (a regex), the line must satisfy the nested `require:` predicates. The in-file line quantifier — the dual of `ordered_block`'s `select:` (where `ordered_block` *orders* selected lines, this asserts a *conjunction of predicates* over each). `require:` takes at least one of: `matches` (the line must match **all** listed regexes), `forbid` (the line must match **none**), and `equal` (the listed named `select` captures must all be **equal** — checked on **every** `select` match on the line, so a line carrying two PR links validates both). One violation per offending line; lines `select` does not match are ignored. It closes two shapes no `file_content_*` kind can: a per-line changelog grammar ("**every** `* ` entry must *also* end with a linked PR ref" — `file_content_matches` asserts existence, not a per-line conjunction) and intra-line capture equality ("the display number must equal the `/pull/` URL number" — the Rust `regex` engine is RE2: no backreferences). Per-file (the `PerFileRule` fast path).

```yaml
- id: changelog-entries-well-formed
  kind: for_each_match
  paths: ["CHANGELOG.md"]
  select: '^[*-] .*\[#(?P<disp>\d+)\]\([^)]*pull/(?P<url>\d+)\)'
  require:
    matches: ['\)\.$']            # every entry line ends with ").":
    forbid:  ['\[Fix #\d+\]']     # ...never uses the "[Fix #N]" form
    equal:   [disp, url]          # ...and its display number == its URL number
  level: warning
```

### `generated_file_fresh`

**Categories:** Cross-file, Security / Unicode sanity

A committed artefact must equal what a declared `command` generator produces, in one of two modes (exactly one of `file` / `outputs`). **alint never leaves regenerated files behind** — it *verifies* freshness, it does not run codegen as a build step. Either mode runs a user-declared, maintainer-trusted process, so the kind is trust-gated to your own top-level config (same tier as the `command` rule). Single-shot, opt-in. Spawn-failure / non-zero exit / timeout are each a clear, distinct violation. `normalize` (`none` / `trim` / `final-newline`) absorbs trailing-newline churn.

- **stdout mode** (`file:`) — the generator writes its single output to stdout; alint captures it and compares to the one committed `file`. Never writes the tree.
- **mutating / in-place mode** (`outputs:`, a glob or list) — for the common `make gen && git diff --exit-code` pattern, where the generator rewrites files in place. alint **snapshots** the `outputs`, runs the generator, **diffs** (flagging each stale / newly-created / removed file), and **restores the snapshot** — so `alint check` leaves the working tree byte-identical (the restore is panic-safe). The generator must confine its writes to `outputs`.

```yaml
# stdout mode — diff the generator's stdout against one committed file
- id: bindings-fresh
  kind: generated_file_fresh
  file: crates/ffi/include/core.h
  command: ["cbindgen", "--config", "cbindgen.toml", "crates/core"]
  normalize: final-newline
  level: error

# mutating mode — run the in-place generator and assert nothing changed
- id: commands-def-fresh
  kind: generated_file_fresh
  outputs: "src/commands.def"          # glob or list; selects mutating mode
  command: ["make", "commands.def"]
  timeout: 300
  level: error
```

### `import_gate`

**Categories:** Cross-file, Security / Unicode sanity

Forbid imports whose **extracted target** matches a `forbid` regex, within the `paths` scope — an architectural import firewall (staging-layer isolation, core/providers separation, private-API gates). Matches the import target, not the raw line (so a comment or string mentioning the path doesn't fire — the low-false-positive specialisation of `file_content_forbidden`). `language` (`go`/`python`/`rust`/`js`/`scala`/`java`/`dart`/`nix`) supplies a built-in import-line pattern; `import_pattern` overrides it (capture group 1 = target; required for `generic`). The `js` preset (whose pattern is unanchored, to catch dynamic `import("m")` / `require("m")`) additionally blanks `//` and `/* … */` comments before matching, so a JSDoc `@typedef {import("../x")}` type annotation isn't mistaken for a real import. `allow` globs exempt sanctioned files. One violation per offending import.

```yaml
- id: staging-no-main-module
  kind: import_gate
  paths: "staging/src/k8s.io/**/*.go"
  language: go
  forbid: "^k8s\\.io/kubernetes/"
  allow: ["staging/src/k8s.io/legacy/**"]
  level: error
```

### `command_idempotent`

**Categories:** Cross-file

Run a user-declared formatter/checker in its **`--check`
(idempotence) mode** once: exit `0` ⇒ the tree is
formatter-clean (silent); non-zero ⇒ violation(s). The sibling
of `generated_file_fresh` — that rule diffs a *generator's*
captured stdout against a committed file; this trusts a
*checker's* own `--check` exit code. **alint never runs a
mutating formatter and never writes the tree.** With
`files_from` (`stdout`/`stderr`) the tool's own offender list is
parsed into one violation **per file** (optional `files_pattern`
regex, capture group 1 = path, for tools that wrap the path in a
message like `cargo fmt`'s `Diff in <path> at line N`); without
it, one violation for the whole invocation. A non-zero exit is
never swallowed into a pass. Single-shot, opt-in. Trust-gated
like `command` (see below): declarable only in your own
top-level config.

```yaml
- id: code-is-formatted
  kind: command_idempotent
  command: ["cargo", "fmt", "--all", "--", "--check"]
  workdir: "."
  files_from: stderr
  files_pattern: "Diff in (.+) at"
  level: error
  message: "run `cargo fmt` — code is not formatter-clean"
```

### `for_each_dir` / `for_each_file`

**Categories:** Cross-file

For every matching directory / file, evaluate a nested `require:` block with the entry as context. Template tokens (`{dir}`, `{stem}`, `{ext}`, `{basename}`, `{path}`, `{parent_name}`) expand against each match. `select:` is a single glob or a list with `!`-prefixed excludes (e.g. `["src/*", "!src/internal"]`).

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

**Categories:** Cross-file, Structure

Every directory matching `select:` must contain files matching every glob in `require:`. Sugar for a common `for_each_dir` shape.

```yaml
- id: packages-have-readme-and-license
  kind: dir_contains
  select: "packages/*"
  require: ["README.md", "LICENSE*"]
  level: error
```

### `dir_only_contains`

**Categories:** Cross-file, Structure

Every direct-child file of a directory matching `select:` must match at least one glob in `allow:`. Catches stray test data in `src/`.

```yaml
- id: src-only-rs
  kind: dir_only_contains
  select: "src/*"
  allow: ["*.rs", "README.md"]
  level: error
```

### `unique_by`

**Categories:** Cross-file

No two files matching `select` may share the value of `key` (a path template; tokens `{path}`/`{dir}`/`{basename}`/`{stem}`/`{ext}`/`{parent_name}`). Catches basename collisions across subdirectories. With `case_insensitive: true` the key is folded to lowercase before grouping, so `README.md` and `readme.md` collide — the case-insensitive-filesystem hazard (Windows / macOS).

```yaml
- id: unique-basenames
  kind: unique_by
  select: "src/**/*.rs"
  key: "{stem}"
  level: warning
```

### `every_matching_has`

**Categories:** Cross-file

For every file or directory matching `select:`, every nested rule under `require:` must be satisfied. Lightweight sibling of `pair` that iterates both file and directory entries. `select:` is a single glob or a list with `!`-prefixed excludes (e.g. `["packages/*", "!packages/internal"]`).

```yaml
- id: every-pkg-has-readme
  kind: every_matching_has
  select: "packages/*"
  require:
    - kind: file_exists
      paths: "{path}/README.md"
  level: error
```

---

## Plugin (tier 1)

### `command`

**Categories:** Plugin (tier 1)

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

**Trust gate.** Every process-spawning rule kind — `command`, `generated_file_fresh`, and `command_idempotent` — is allowed only in the user's own top-level config. Any of them introduced via `extends:` (local file, HTTPS URL, or `alint://bundled/`) is a load-time error — the same gate that protects `custom:` facts. Adopting a published ruleset must never imply granting it arbitrary code execution.

**Path confinement + `allow_out_of_root`.** Every config-declared path is confined to the repo root — a rule can't read or resolve a file outside the tree it was pointed at. The top-level-only `allow_out_of_root:` key relaxes this for *reads* (`json_schema_passes` `schema_path:`, `pair_hash` `target:`, `registry_paths_resolve` `source:`) when a trusted config must reference an external file. It is **rejected from `extends:`'d rulesets** (same trust model as the spawn gate above) and a permitted read emits a note. See [Configuration → `allow_out_of_root`](/docs/configuration/#allow_out_of_root).

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

<!-- alint:ignore-example -->
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

The minimal hygiene baseline most open-source repos want. Fifteen rules:

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `oss-readme-exists` | `file_exists` | warning | — |
| `oss-readme-non-stub` | `file_min_lines` (3) | info | — |
| `oss-license-exists` | `file_exists` | warning | — |
| `oss-license-non-empty` | `file_min_size` (200 B) | info | — |
| `oss-security-policy-exists` | `file_exists` | info | — |
| `oss-security-policy-non-empty` | `file_min_size` | info | — |
| `oss-dependency-update-tool` | `file_exists` (Dependabot OR Renovate) | info | — |
| `oss-codeowners-exists` | `file_exists` | info | — |
| `oss-codeowners-non-empty` | `file_min_size` | info | — |
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

Hygiene checks for Rust projects. Tree-level gate: `when: facts.has_rust` (true if any `Cargo.toml` exists *anywhere* in the tree — declared inside the ruleset as `any_file_exists: [Cargo.toml, "**/Cargo.toml"]`). Per-file content rules layer a `scope_filter: { has_ancestor: Cargo.toml }` on top so they only fire on `.rs` files inside an ancestor-`Cargo.toml` directory subtree — useful in polyglot monorepos where Rust packages sit alongside Node / Python / Go subdirectories.

| Rule id | Kind | Default level | scope_filter | Fix |
|---|---|---|---|---|
| `rust-cargo-toml-exists` | `file_exists` | error | — | — |
| `rust-cargo-lock-exists` | `file_exists` | warning | — | — |
| `rust-toolchain-pinned` | `file_exists` | info | — | — |
| `rust-no-tracked-target` | `dir_absent` | error | — | — |
| `rust-sources-snake-case` | `filename_case` | error | — | `file_rename` |
| `rust-sources-final-newline` | `final_newline` | warning | `Cargo.toml` | `file_append_final_newline` |
| `rust-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `Cargo.toml` | `file_trim_trailing_whitespace` |
| `rust-sources-no-bidi` | `no_bidi_controls` | error | `Cargo.toml` | — |
| `rust-sources-no-zero-width` | `no_zero_width_chars` | error | `Cargo.toml` | — |
| `rust-no-merge-markers-in-manifests` | `no_merge_conflict_markers` | error | `Cargo.toml` | — |

### `alint://bundled/node@v1`

Hygiene checks for Node.js / npm / pnpm / yarn / bun projects. Tree-level gate: `when: facts.has_node` (`any_file_exists: [package.json, "**/package.json"]`). Per-file content rules layer `scope_filter: { has_ancestor: package.json }` so they only fire on JS/TS files inside an ancestor-`package.json` directory subtree.

| Rule id | Kind | Default level | scope_filter | Fix |
|---|---|---|---|---|
| `node-package-json-exists` | `file_exists` | error | — | — |
| `node-has-lockfile` | `file_exists` | warning | — | — |
| `node-no-tracked-node-modules` | `dir_absent` | error | — | — |
| `node-no-tracked-dist` | `dir_absent` | info | `package.json` | — |
| `node-engine-or-nvmrc` | `file_exists` | info | — | — |
| `node-sources-final-newline` | `final_newline` | info | `package.json` | `file_append_final_newline` |
| `node-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `package.json` | `file_trim_trailing_whitespace` |
| `node-sources-no-bidi` | `no_bidi_controls` | error | `package.json` | — |

### `alint://bundled/python@v1`

Hygiene checks for Python projects. Tree-level gate: `when: facts.has_python` — any of `pyproject.toml`, `setup.py`, `setup.cfg`, `requirements.txt` (each broadened with a `**/<manifest>` companion) present anywhere in the tree. Per-file content rules layer `scope_filter: { has_ancestor: [pyproject.toml, setup.py, requirements.txt] }` (note: `setup.cfg` stays in the broad fact list but not in scope_filter — `scope_filter` narrows to canonical package markers).

| Rule id | Kind | Default level | scope_filter | Fix |
|---|---|---|---|---|
| `python-manifest-exists` | `file_exists` | error | — | — |
| `python-has-lockfile` | `file_exists` | warning | — | — |
| `python-pyproject-declares-name` | `toml_path_matches` | warning | — | — |
| `python-pyproject-declares-requires-python` | `toml_path_matches` | info | — | — |
| `python-module-snake-case` | `filename_case` | info | — | — |
| `python-sources-final-newline` | `final_newline` | info | `[pyproject.toml, setup.py, requirements.txt]` | `file_append_final_newline` |
| `python-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `[pyproject.toml, setup.py, requirements.txt]` | `file_trim_trailing_whitespace` |
| `python-sources-no-bidi` | `no_bidi_controls` | error | `[pyproject.toml, setup.py, requirements.txt]` | — |

### `alint://bundled/go@v1`

Hygiene checks for Go modules. Tree-level gate: `when: facts.has_go` (`any_file_exists: [go.mod, "**/go.mod"]`). Per-file content rules layer `scope_filter: { has_ancestor: go.mod }` so they only fire on `.go` files inside an ancestor-`go.mod` module subtree.

| Rule id | Kind | Default level | scope_filter | Fix |
|---|---|---|---|---|
| `go-mod-exists` | `file_exists` | error | — | — |
| `go-sum-exists` | `file_exists` | warning | — | — |
| `go-mod-declares-module-path` | `file_content_matches` | error | — | — |
| `go-mod-declares-go-version` | `file_content_matches` | warning | — | — |
| `go-sources-no-bidi` | `no_bidi_controls` | error | `go.mod` | — |
| `go-sources-no-zero-width` | `no_zero_width_chars` | error | `go.mod` | — |
| `go-sources-final-newline` | `final_newline` | info | `go.mod` | `file_append_final_newline` |

### `alint://bundled/java@v1`

Hygiene checks for Java / Kotlin projects (Gradle or Maven). Tree-level gate: `when: facts.has_java` — any Java build manifest variant (`pom.xml`, `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts`, each broadened with a `**/<manifest>` companion). Per-file content rules layer `scope_filter: { has_ancestor: [pom.xml, build.gradle, build.gradle.kts] }` — note that `settings.gradle*` are in the broad fact list but excluded from scope_filter (workspace-level files, not per-module manifests).

| Rule id | Kind | Default level | scope_filter | Fix |
|---|---|---|---|---|
| `java-manifest-exists` | `file_exists` | error | — | — |
| `java-build-wrapper-committed` | `file_exists` | info | — | — |
| `java-no-tracked-target` | `dir_absent` | error | — | — |
| `java-no-tracked-build` | `dir_absent` | error | — | — |
| `java-no-class-files` | `file_absent` | error | — | — |
| `java-sources-pascal-case` | `filename_case` | warning | — | — |
| `java-sources-final-newline` | `final_newline` | info | `[pom.xml, build.gradle, build.gradle.kts]` | `file_append_final_newline` |
| `java-sources-no-trailing-whitespace` | `no_trailing_whitespace` | info | `[pom.xml, build.gradle, build.gradle.kts]` | `file_trim_trailing_whitespace` |
| `java-sources-no-bidi` | `no_bidi_controls` | error | `[pom.xml, build.gradle, build.gradle.kts]` | — |
| `java-sources-no-zero-width` | `no_zero_width_chars` | error | `[pom.xml, build.gradle, build.gradle.kts]` | — |

### `alint://bundled/dotnet@v1`

Baseline conventions for .NET projects. Tree-level gate: `when: facts.has_dotnet` — any `*.sln` / `**/*.csproj` / `**/*.fsproj` / `**/*.vbproj` / `global.json`, so the ruleset is a silent no-op in non-.NET repos (and in the non-.NET parts of a polyglot monorepo). The structural checks use the structured-query family (`json_path_matches` on `global.json`, `xml_path_*` on the MSBuild XML) — the concrete payoff of the v0.10 `xml_path_*` rule kinds. Every structured-query rule is `if_present: true` (it flags a *misconfiguration*, never forces a property to exist) and levels are deliberately non-blocking (no `error`) given the adopter surface (every `dotnet/*` + every Azure SDK + every `microsoft/*` .NET project).

| Rule id | Kind | Default level |
|---|---|---|
| `dotnet-global-json-exists` | `file_exists` | warning |
| `dotnet-global-json-pins-sdk` | `json_path_matches` | warning |
| `dotnet-csproj-sdk-style` | `xml_path_matches` | warning |
| `dotnet-csproj-nullable-enabled` | `xml_path_equals` | info |
| `dotnet-central-package-management` | `xml_path_equals` | info |
| `dotnet-no-build-output-committed` | `dir_absent` | warning |
| `dotnet-editorconfig-exists` | `file_exists` | info |

**Central Package Management:** the ruleset deliberately does **not** require a `Version` on each `<PackageReference>` — CPM (`Directory.Packages.props`) makes that attribute absent by design, so enforcing it would false-positive across CPM repos (dotnet/runtime). It instead checks that, *if* a `Directory.Packages.props` exists, CPM is actually enabled. Composes with `hygiene/no-tracked-artifacts@v1` (namespaced `dotnet-*` ids; `dotnet-no-build-output-committed` is the .NET-gated `bin/`/`obj/` companion).

### `alint://bundled/php@v1`

Baseline conventions for PHP / Composer projects. Tree-level gate: `when: facts.has_php` — any `composer.json` (root or nested), so the ruleset is a silent no-op in non-PHP repos (and in the non-PHP parts of a polyglot monorepo). Its heart is the **"Composer-fatals" invariants**: `composer.json` declares autoload roots and console binaries, and Composer aborts at install/autoload time if any path is missing — pure cross-file path-existence checks (`registry_paths_resolve`) alint expresses natively, without running Composer (the same checks laravel and phpstan hand-roll). The structured-query `name` check is `if_present: true` (an application `composer.json` may omit a published `name`); the path-resolve rules are naturally silent when their field is absent. Levels are non-blocking (no `error`) given the broad adopter surface.

| Rule id | Kind | Default level |
|---|---|---|
| `php-composer-name-format` | `json_path_matches` | warning |
| `php-composer-psr4-dirs-resolve` | `registry_paths_resolve` | warning |
| `php-composer-autoload-files-resolve` | `registry_paths_resolve` | warning |
| `php-composer-autoload-dev-files-resolve` | `registry_paths_resolve` | info |
| `php-composer-bin-resolve` | `registry_paths_resolve` | warning |
| `php-no-vendor-committed` | `dir_absent` | warning |

**Scope:** the path-resolve rules read the *root* `composer.json`; in a monorepo of sub-packages each carrying its own `composer.json`, add per-package rules (or a `for_each_dir`) in your own config. Composes with `hygiene/no-tracked-artifacts@v1` (namespaced `php-*` ids; `php-no-vendor-committed` is the Composer-gated `vendor/` companion).

### `alint://bundled/ci/github-actions@v1`

Hardening for `.github/workflows/*.y{,a}ml`, guided by the two OpenSSF Scorecard checks with the strongest supply-chain signal (Token-Permissions + Pinned-Dependencies) plus a readability nudge. Scoped to workflow files, so the ruleset is a safe no-op in repos that don't use GitHub Actions.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `gha-workflow-contents-read` | `yaml_path_equals` (`$.permissions.contents == "read"`) | warning | — |
| `gha-pin-actions-to-sha` | `yaml_path_matches` (40-hex SHA on every `uses:`) | warning | — |
| `gha-workflow-has-name` | `yaml_path_matches` (`$.name`) | info | — |

### `alint://bundled/agent-hygiene@v1`

Catches the canonical agent-driven-development cruft surface — backup-suffix files, scratch docs, debug residue, AI-affirmation prose, model-attributed TODO markers. Composable from existing primitives (`file_absent`, `filename_regex`, `file_content_forbidden`); no new rule kinds. Fires on every repo regardless of language; not gated.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `agent-no-versioned-duplicates` | `file_absent` (`*_old.*`, `*_FINAL.*`, `*_copy.*`, `*_backup.*`, …) | warning | — |
| `agent-no-scratch-docs-at-root` | `file_absent` (`PLAN.md`, `NOTES.md`, `ANALYSIS.md`, `SUMMARY.md`, `FIX.md`, `DECISION.md`, `TODO.md`, `SCRATCH.md`, `DEBUG.md`, `TEMP.md`, `WIP.md`) | warning | — |
| `agent-no-affirmation-prose` | `file_content_forbidden` (`"You're absolutely right"`, …) | info | — |
| `agent-no-console-log` | `file_content_forbidden` (`console.log` / `.debug` / `.trace` in non-test JS/TS) | warning | — |
| `agent-no-debugger-statements` | `file_content_forbidden` (`debugger;`, `breakpoint()`) | error | — |
| `agent-no-model-todos` | `file_content_forbidden` (`TODO(claude:)`, `TODO(cursor:)`, `TODO(gpt:)`, `TODO(copilot:)`, `TODO(gemini:)`, `TODO(codex:)`, `TODO(aider:)`, `TODO(chatgpt:)`) | warning | — |

The most-cited gripes about agent-generated code surface as a single one-line `extends:` adoption — pair with the per-language ruleset that fits the project.

### `alint://bundled/agent-context@v1`

Hygiene rules for agent-context files (`AGENTS.md`, `CLAUDE.md`, `.cursorrules`). Existence recommended, stub guard via `file_min_lines`, bloat guard via `file_max_lines` (per Augment Code research, context files >300 lines correlate with worse agent performance), stale-path heuristic via regex. Subsumes `ctxlint`'s niche with no new rule kinds — composes `file_exists` / `file_min_lines` / `file_max_lines` / `file_content_forbidden`.

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `agent-context-recommended` | `file_exists` (any of `AGENTS.md` / `CLAUDE.md` / `.cursorrules`) | info | — |
| `agent-context-non-stub` | `file_min_lines` (10) | warning | — |
| `agent-context-not-bloated` | `file_max_lines` (300) | info | — |
| `agent-context-no-stale-paths` | `file_content_forbidden` (regex heuristic over backticked workspace paths) | info | — |

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

### `alint://bundled/apache/governance@v1`

Apache **Top-Level Project (TLP) governance** discipline — the governance / release-artefact baseline an Apache TLP is expected to ship, that arrow + spark + airflow each re-implement by hand. The *governance* superset, distinct from `compliance/apache-2@v1`'s *license-redistribution* focus: it additionally asserts the NOTICE *content* (the ASF attribution line, not just existence), the release-signing `KEYS` file, the no-compiled-binaries source-release rule, and release-notes discipline. Eight rules:

| Rule id | Kind | Default level | Fix |
|---|---|---|---|
| `apache-gov-license-exists` | `file_exists` | error | — |
| `apache-gov-notice-exists` | `file_exists` | error | — |
| `apache-gov-notice-asf-attribution` | `file_content_matches` | warning | — |
| `apache-gov-keys-exists` | `file_exists` | warning | — |
| `apache-gov-source-license-header` | `file_header` | warning | — |
| `apache-gov-no-binaries-in-source` | `file_absent` | warning | — |
| `apache-gov-readme-exists` | `file_exists` | warning | — |
| `apache-gov-changelog-exists` | `file_exists` | info | — |

Rule ids are namespaced `apache-gov-*`, so it is safe to adopt **alongside** `compliance/apache-2@v1` (no id collision; the LICENSE/header overlap is intentional and each id is independently `level: off`-able). The `apache-gov-source-license-header` rule reuses `compliance/apache-2@v1`'s v0.9.18-broadened ASF-preamble pattern verbatim (short form **or** the long ASF-preamble form), so it does not reintroduce the short-form-only false positives. Targets graduated TLPs; incubating podlings additionally need a `DISCLAIMER` (layer that on yourself). No fact gate — adopting it is the signal that the repo is an Apache TLP.

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
- Nested configs can only declare `version:` and `rules:` — `extends:`, `facts:`, `vars:`, `ignore:`, `respect_gitignore:`, `fix_size_limit:`, and `allow_out_of_root:` are root-only.
- Every rule in a nested config must have a path-like scope field (`paths`, `select`, or `primary`). Rules without any (e.g. `no_submodules`, which is hardcoded to repo root) can't be nested.
- Absolute paths and `..`-prefixed globs are rejected — they'd escape the subtree the config is supposed to confine.
- Rule-id collisions across configs are rejected with a clear error. Per-subtree overrides aren't supported yet; if you want to disable a root rule under one subtree, use a `when:` gate on the root rule for now.
