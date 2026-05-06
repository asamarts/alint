# Authoring `.alint.yml` configs — common pitfalls + canonical patterns

Surfaced by the P2a launch-prep validation pass — **15 distinct schema /
language pitfalls** hit while writing configs for production repos. The
first 12 surfaced in the pilot (kubernetes, rust-lang/rust, deno, airflow,
turbo); the next 3 in Wave 1 (clap, tokio, ruff, uv, typescript). All
configs ultimately parse + run, but the iteration cost was high. This doc
captures every one with the canonical correct form.

> **TL;DR for AI agents writing alint configs:** read this entire doc
> before drafting an `.alint.yml`. The fields documented in
> [`docs/rules.md`](../rules.md) and on [alint.org](https://alint.org)
> are *examples*, not the canonical spec. The canonical spec lives in
> `crates/alint-rules/src/<kind>.rs::struct Options` for each rule
> kind. When in doubt, read the source. After drafting, **always**
> parse-validate via `alint check --config <path> <root>` before
> declaring done.

## How to use this doc

1. Reading existing configs? Skim the bug catalogue below — most
   real-world configs you'll see in the wild have at least one of
   these.
2. Writing a new config? Read § "Canonical patterns" at the end
   first; it's the cheat sheet.
3. Reviewing a PR that adds a config? Check against § "Pre-merge
   checklist".

---

## The 15 pitfalls

### 1. `command` rule: field is `command:` not `argv:`

The `command` rule's `Options` struct names the argv field
`command:`. Many writers (including AI agents drawing on Go/Rust/JS
ecosystem memory where `argv` is the conventional name) reach for
`argv:` first.

**Wrong:**
```yaml
- id: shellcheck
  kind: command
  paths: "**/*.sh"
  argv: ["shellcheck", "-x", "{path}"]   # ← schema rejects `argv`
```

**Right:**
```yaml
- id: shellcheck
  kind: command
  paths: "**/*.sh"
  command: ["shellcheck", "-x", "{path}"]
```

Source: `crates/alint-rules/src/command.rs::struct Options`.

### 2. `command` rule: `timeout:` is seconds-as-integer, not a duration string

`timeout:` is `Option<u64>` — seconds, integer. Go-style
`30s`/`1m` and Rust-style `Duration::from_secs(30)` literals are
both rejected by serde.

**Wrong:**
```yaml
- id: golangci-lint
  kind: command
  command: ["golangci-lint", "run", "{dir}/..."]
  timeout: 600s                          # ← string, schema wants u64
```

**Right:**
```yaml
- id: golangci-lint
  kind: command
  command: ["golangci-lint", "run", "{dir}/..."]
  timeout: 600                           # ← integer seconds
```

If `timeout:` is omitted, the default is 30 seconds.

### 3. `command` rule: there is no `expect_empty_stdout:` field

A natural-feeling addition for tools like `gofmt -l` (which exits 0
even for unformatted files; the violation signal is non-empty stdout)
— but this field doesn't exist. The `command` rule **already** treats
non-empty stdout *or* non-empty stderr *or* non-zero exit as a
violation by default, so this case is handled without a knob.

**Wrong:**
```yaml
- id: gofmt
  kind: command
  paths: "**/*.go"
  command: ["gofmt", "-l", "{path}"]
  expect_empty_stdout: true              # ← not a field; serde rejects
```

**Right:**
```yaml
- id: gofmt
  kind: command
  paths: "**/*.go"
  command: ["gofmt", "-l", "{path}"]     # default behaviour catches non-empty stdout
```

### 4. `pair` rule: secondary file is `partner:` not `secondary:`

The pair rule pairs a `primary:` glob match with a `partner:` path
template; many writers default to `secondary:` because "secondary"
is the natural English word.

**Wrong:**
```yaml
- id: header-pair
  kind: pair
  primary: "**/*.c"
  secondary: "{dir}/{stem}.h"            # ← schema wants `partner:`
```

**Right:**
```yaml
- id: header-pair
  kind: pair
  primary: "**/*.c"
  partner: "{dir}/{stem}.h"
```

Source: `crates/alint-rules/src/pair.rs::struct Options`.

### 5. `pair` rule: there is no `require:` field

The pair rule's contract IS "every primary must have a partner";
that's the whole rule. Adding `require: secondary` (or any value)
is a cargo-cult from `for_each_dir`'s `require:` block, but `pair`
doesn't have one.

**Wrong:**
```yaml
- id: c-h-pair
  kind: pair
  primary: "**/*.c"
  partner: "{dir}/{stem}.h"
  require: partner                       # ← not a field; rejected
```

**Right:** drop the line — the assertion is implicit.

### 6. `for_each_dir` / `for_each_file` / `every_matching_has`: `level:` belongs on the OUTER rule, not the nested `require:` block

The cross-file iteration rules wrap a `require:` block of nested
rules. The outer rule (the `for_each_dir`) is what gets the
violation reported under, so its `level:` is what alint uses.
Putting `level:` on the inner nested rule places the field where
serde *can* deserialize it but the engine then complains the outer
rule is missing `level`.

**Wrong:**
```yaml
- id: golangci-lint-per-module
  kind: for_each_dir
  select: "**/go.mod"
  require:
    - kind: command
      paths: "{dir}"
      command: ["golangci-lint", "run", "{dir}/..."]
      timeout: 600
      level: error                       # ← wrong placement
                                          #   serde error on the outer rule:
                                          #   "missing field `level`"
```

**Right:**
```yaml
- id: golangci-lint-per-module
  kind: for_each_dir
  select: "**/go.mod"
  require:
    - kind: command
      paths: "{dir}"
      command: ["golangci-lint", "run", "{dir}/..."]
      timeout: 600
  level: error                           # ← outer level
```

### 7. `fix:` is a tagged-mapping enum, not a bare string

`fix:` accepts a mapping whose key names the variant and whose value
is the variant's options struct. Even when an op takes no options,
the empty mapping `{}` is required.

**Wrong:**
```yaml
- id: trim-whitespace
  kind: no_trailing_whitespace
  paths: "**/*"
  fix: file_trim_trailing_whitespace     # ← string; schema wants tagged-mapping
```

**Right:**
```yaml
- id: trim-whitespace
  kind: no_trailing_whitespace
  paths: "**/*"
  fix:
    file_trim_trailing_whitespace: {}    # ← mapping with empty options
```

Source: `crates/alint-core/src/config.rs::pub enum FixSpec`.

### 8. `line_endings`: target field is `target:` not `style:`

**Wrong:**
```yaml
- id: lf-only
  kind: line_endings
  paths: "**/*"
  style: lf                              # ← schema wants `target:`
```

**Right:**
```yaml
- id: lf-only
  kind: line_endings
  paths: "**/*"
  target: lf                             # ← `lf` or `crlf`
```

Source: `crates/alint-rules/src/line_endings.rs::struct Options`.

### 9. `file_starts_with` / `file_ends_with`: literal anchor field is `prefix:` / `suffix:`, not `pattern:`

The bounded-read rules anchor on a literal byte string (no regex).
Many writers reach for `pattern:` because the broader content rules
(`file_content_matches`, `file_header`) use `pattern:` for their
regex.

**Wrong:**
```yaml
- id: rustdoc-gui-prefix
  kind: file_starts_with
  paths: "tests/rustdoc-gui/**/*.goml"
  pattern: '^// '                        # ← schema wants `prefix:`, no regex
```

**Right:**
```yaml
- id: rustdoc-gui-prefix
  kind: file_starts_with
  paths: "tests/rustdoc-gui/**/*.goml"
  prefix: "// "                          # ← literal byte prefix
```

Source: `crates/alint-rules/src/file_starts_with.rs::struct Options`.

### 10. JSONPath dashed-key access requires bracket notation

alint's JSONPath impl is RFC 9535-compliant (`serde_json_path`
crate). Per the spec, dot-notation is reserved for keys matching
the identifier production `[A-Za-z_][A-Za-z0-9_]*`. Dashed,
dotted, and other non-identifier keys must use bracket notation.

**Wrong:**
```yaml
- id: provider-package-name
  kind: yaml_path_matches
  paths: "providers/**/provider.yaml"
  path: "$.package-name"                 # ← parser fails on dash
  matches: '^apache-airflow-providers-'
```

**Right:**
```yaml
- id: provider-package-name
  kind: yaml_path_matches
  paths: "providers/**/provider.yaml"
  path: "$['package-name']"              # ← bracket notation
  matches: '^apache-airflow-providers-'
```

This applies to *any* segment with a non-identifier character,
including dots inside keys (rare but real in some YAML configs).

### 11. `scope_filter.has_ancestor:` is a basename only

`has_ancestor: <name>` walks the path's ancestors looking for any
directory that contains a file or directory with this exact
basename. The value MUST be a basename — no path separators. Want
"files specifically under `airflow-core/`"? Use `paths: "airflow-core/**/*.py"`
on the rule's main scope, not `scope_filter`.

**Wrong:**
```yaml
- id: airflow-core-no-base-operator
  kind: file_content_forbidden
  paths: "**/*.py"
  scope_filter:
    has_ancestor: airflow-core/pyproject.toml   # ← rejected: contains `/`
  pattern: 'from airflow\.models import.* BaseOperator\b'
```

**Right (option A — basename match):**
```yaml
- id: any-python-package-no-base-operator
  kind: file_content_forbidden
  paths: "**/*.py"
  scope_filter:
    has_ancestor: pyproject.toml         # ← basename only
  pattern: 'from airflow\.models import.* BaseOperator\b'
```

**Right (option B — paths glob):**
```yaml
- id: airflow-core-no-base-operator
  kind: file_content_forbidden
  paths: "airflow-core/**/*.py"          # ← scope by paths instead
  pattern: 'from airflow\.models import.* BaseOperator\b'
```

Source: `crates/alint-core/src/scope_filter.rs`.

### 12. `when:` / `when_iter:` use word operators + a fixed `iter.*` accessor set

The `when:` expression language is bounded — exactly because we
don't want users writing arbitrary code in their lint config. Two
specific gotchas:

#### 12a. Operators are keywords (`and`/`or`/`not`), not symbols (`&&`/`||`/`!`)

**Wrong:**
```yaml
when_iter: 'iter.parent_name != "" && iter.is_dir'
```

**Right:**
```yaml
when_iter: 'iter.parent_name != "" and iter.is_dir'
```

#### 12b. `iter.*` accessors are a fixed set; no method calls

The supported `iter.*` shapes are:

| Accessor | Returns |
|---|---|
| `iter.path` | path of the iterated entry, as a string |
| `iter.basename` | last component of `iter.path` |
| `iter.parent_name` | name of parent directory |
| `iter.is_dir` | bool |
| `iter.has_file(<pattern>)` | bool — does the iterated entry contain a file matching `<pattern>`? |

There are no `.contains(...)`, `.starts_with(...)`, `.ends_with(...)`
or other string method calls. Use `matches` for regex matching:

**Wrong:**
```yaml
when_iter: 'not iter.path.contains("node_modules")'
```

**Right:**
```yaml
when_iter: 'not iter.path matches "node_modules"'
```

(Or — better — use the rule's `paths:` exclude list to filter out
those paths entirely, before `when_iter:` even fires.)

Source: `crates/alint-core/src/when.rs`.

### 13. `file_content_matches` / `file_content_forbidden`: regex `^` and `$` anchor file-start / file-end by default, NOT line-start / line-end

alint compiles patterns with the `regex` crate's default mode, where `^`
matches the start of the *input* (the whole file as one string) and `$`
matches the end. To make `^` and `$` match line boundaries, prefix the
pattern with the `(?m)` multi-line flag.

This is the single highest-cost regex pitfall in practice — `pattern: '^edition'`
"works" against a file that starts with `edition = "2021"` on the first byte,
then silently fails for every other file in the tree. Parse-validation
catches the schema error but cannot catch the semantic miss.

**Wrong (matches only files where `[lints]` IS the first line):**
```yaml
- id: per-crate-workspace-lints
  kind: file_content_matches
  paths: "crates/**/Cargo.toml"
  pattern: '^\[lints\]'
```

**Right (matches `[lints]` anywhere as a line start):**
```yaml
- id: per-crate-workspace-lints
  kind: file_content_matches
  paths: "crates/**/Cargo.toml"
  pattern: '(?m)^\[lints\]'
```

When in doubt, default to `(?m)` for any pattern that uses `^` or `$`
unless you explicitly mean "anchored to byte 0" or "anchored to EOF".

### 14. YAML scalar strings do NOT expand `\n` to a literal newline inside regex patterns

YAML single-quoted and plain scalars treat `\n` as a literal two-character
sequence (`\` followed by `n`). A pattern like `'^\[lints\]\s*$\n\s*workspace'`
compiles successfully (it's a valid regex matching `\n` literally) but never
matches a real file because real files contain a U+000A newline byte, not
the two-character `\n` sequence.

Three correct forms:

**Right (option A — `\s+` spans line breaks):**
```yaml
- id: lints-workspace-true
  kind: file_content_matches
  paths: "crates/**/Cargo.toml"
  pattern: '(?m)^\[lints\]\s+workspace\s*=\s*true'
```

**Right (option B — `(?s)` + `.+?` lets `.` match newlines):**
```yaml
- id: lints-workspace-true
  kind: file_content_matches
  paths: "crates/**/Cargo.toml"
  pattern: '(?s)\[lints\].+?workspace\s*=\s*true'
```

**Right (option C — YAML double-quoted string explicitly expands `\n`):**
```yaml
- id: lints-workspace-true
  kind: file_content_matches
  paths: "crates/**/Cargo.toml"
  pattern: "(?m)^\\[lints\\]\\s*$\nworkspace\\s*=\\s*true"
```

Option A is the most readable; option C requires escaping every regex
metacharacter twice. Default to A unless you need exact line-spacing
semantics.

### 15. `file_starts_with.prefix:` rejects an empty string at build time

A natural-feeling shorthand for "file is non-empty" is `prefix: ""` —
every non-empty file trivially starts with the empty string. The schema
rejects this at registry-build time (the rule has no useful semantics
with an empty prefix). Use the right rule for the job:

**Wrong:**
```yaml
- id: spelling-dict-non-empty
  kind: file_starts_with
  paths: "spellcheck.dic"
  prefix: ""                               # ← rejected at build time
```

**Right (option A — minimum line count):**
```yaml
- id: spelling-dict-non-empty
  kind: file_min_lines
  paths: "spellcheck.dic"
  min_lines: 1
```

**Right (option B — content-shape assertion if you know the literal first chars):**
```yaml
- id: spelling-dict-has-header
  kind: file_starts_with
  paths: "spellcheck.dic"
  prefix: "1"                              # at minimum a digit-as-line-count header
```

(A `file_non_empty` convenience rule is on the v0.10+ candidate list — see
each `examples/*/README.md` for the gap catalogue feeding it.)

---

## Honourable mention: JSONPath regex matching uses `match()`, not `=~`

Not in the 12 above because it's a docs gap rather than a writer
mistake — but worth knowing. RFC 9535 doesn't define a `=~`
operator for regex match. The standard form is the `match()`
function:

**Wrong:**
```yaml
path: "$.jobs.*.steps[?(@.uses=~'^actions/checkout')]"
```

**Right:**
```yaml
path: "$.jobs.*.steps[?match(@.uses, '^actions/checkout')]"
```

---

## Canonical patterns (the cheat sheet)

When writing a new rule, start from the closest pattern below.

### Per-file content check

```yaml
- id: my-rule
  kind: file_content_matches              # or file_content_forbidden, file_header, file_footer
  paths:
    include: ["src/**/*.py"]
    exclude: ["src/**/_generated/**"]
  scope_filter:                            # optional: ancestor-manifest gate
    has_ancestor: pyproject.toml           # MUST be a basename
  pattern: '^# Copyright'
  level: error
  message: "Every Python file under src/ must have a copyright header."
  fix:                                     # optional
    file_prepend:
      content: "# Copyright (c) ...\n"
```

### Cross-file iteration (every dir/file matching a select satisfies a require block)

```yaml
- id: every-pkg-has-readme
  kind: for_each_dir                       # or for_each_file, every_matching_has
  select: "packages/*"
  when_iter: 'iter.has_file("package.json")'   # optional per-iteration filter
  require:
    - kind: file_exists
      paths: "{path}/README.md"
    - kind: file_exists
      paths: "{path}/CHANGELOG.md"
  level: error                             # ← OUTER level, NOT inside require
```

### Pair primary ↔ partner

```yaml
- id: c-needs-h
  kind: pair
  primary: "src/**/*.c"
  partner: "{dir}/{stem}.h"                # NOT `secondary:`
  level: warning
  # NO `require:` field — the assertion that the partner exists IS the rule.
```

### Shell out to an external tool

```yaml
- id: shellcheck-shell-scripts
  kind: command
  paths: "**/*.sh"
  command: ["shellcheck", "-x", "{path}"]  # NOT `argv:`
  timeout: 30                              # integer seconds, NOT "30s"
  level: error
```

### JSONPath structured query

```yaml
- id: cargo-edition
  kind: toml_path_matches                  # or json/yaml variants
  paths: "**/Cargo.toml"
  path: "$.package.edition"                # bracket-notation for dashed/dotted keys: $['package-name']
  matches: '^(2021|2024)$'
  level: error
```

### Auto-fix shape

```yaml
- id: trim-trailing-ws
  kind: no_trailing_whitespace
  paths: "**/*.md"
  fix:
    file_trim_trailing_whitespace: {}      # tagged mapping, not bare string
  level: warning
```

---

## Pre-merge checklist (CI-enforced for `examples/*/.alint.yml`)

Before merging a PR that adds or modifies an `.alint.yml`:

- [ ] `alint check --config <path> <root>` exits without a `building rule "..."`,
      `loading config`, or `invalid options` error. Tool-not-on-PATH errors
      from `command:` rules ARE expected and indicate the rule structure is
      correct.
- [ ] Every `command:` rule's argv uses `command:`, not `argv:`.
- [ ] Every `command:` rule's `timeout:` is an integer, not a duration string.
- [ ] Every `pair` rule uses `partner:`, not `secondary:`, and has no
      `require:` field.
- [ ] Every cross-file iteration rule (`for_each_dir`, `for_each_file`,
      `every_matching_has`) has `level:` on the outer rule, not inside
      `require:`.
- [ ] Every `fix:` is a tagged mapping (`{ <variant>: {...} }`), not a
      bare string.
- [ ] `line_endings.target:` not `style:`.
- [ ] `file_starts_with.prefix:` / `file_ends_with.suffix:`, not
      `pattern:`.
- [ ] JSONPath segments with dashes/dots use bracket notation.
- [ ] `scope_filter.has_ancestor:` is a basename (no `/`).
- [ ] `when:` / `when_iter:` use `and`/`or`/`not` keywords, no `&&`/`||`/`!`.
- [ ] `iter.*` references only use the documented accessor set.
- [ ] Every regex with `^` or `$` either uses `(?m)` (line anchors) or has
      a comment explaining why file-level anchoring is intended.
- [ ] No regex pattern contains a literal `\n` inside a single-quoted YAML
      scalar. Use `\s+` to span line breaks, `(?s)` + `.+?`, or a
      double-quoted YAML scalar with explicit escaping.
- [ ] No `file_starts_with.prefix: ""` (use `file_min_lines: 1` instead).

The `coverage_audit_examples_parse.rs` audit (added in v0.9.15) enforces
the first item by re-validating every `examples/*/.alint.yml` on every
CI run. The schema-level items (1-12, 15) are caught by the same parse,
so the audit covers them transitively. **Items 13-14 are NOT caught by
parse-validation** — the regex compiles fine; it just never matches —
see § "Parse-validation is necessary but not sufficient" below.

---

## Parse-validation is necessary but not sufficient

Pitfalls 13 and 14 (regex anchoring + YAML `\n` escape semantics) compile
into a syntactically-valid regex that produces zero matches against any
realistic input. The `coverage_audit_examples_parse.rs` audit catches
schema errors but cannot tell the difference between "this rule fires
correctly" and "this rule silently never fires."

For configs that depend on regex semantics — `file_content_matches`,
`file_content_forbidden`, `file_header`, `file_footer`, the `matches:`
field on every JSONPath rule — a second-pass smoke test against
representative input is the only way to catch this. Two practical forms:

1. **Negative example file** — drop a file under `examples/<repo>/` that
   *should* trigger the rule, run `alint check`, confirm a violation.
2. **Local repo run** — clone the actual upstream repo, run the config
   against it, confirm rule counts are non-zero where expected.

A future audit (v0.9.16+ candidate) would automate this: each example
case study ships a small "rule smoke-test fixture" tree with expected
violation counts, and the audit asserts `actual == expected` rather
than just "config parses." Tracked as a follow-up to the v0.9.15 DX
hardening sweep.

---

## See also

- [`docs/rules.md`](../rules.md) — full rule catalogue
- [alint.org/docs/rules/](https://alint.org/docs/rules/) — per-rule documentation
- [`crates/alint-rules/src/<kind>.rs`](../../crates/alint-rules/src/) — canonical schema (the `struct Options` block in each file)
- [`crates/alint-core/src/config.rs`](../../crates/alint-core/src/config.rs) — top-level `RuleSpec`, `FixSpec`, `NestedRuleSpec` schemas
- [`crates/alint-core/src/when.rs`](../../crates/alint-core/src/when.rs) — `when:` / `when_iter:` expression language
- [`crates/alint-core/src/scope_filter.rs`](../../crates/alint-core/src/scope_filter.rs) — `scope_filter:` semantics
