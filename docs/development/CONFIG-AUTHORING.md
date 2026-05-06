# Authoring `.alint.yml` configs — common pitfalls + canonical patterns

Surfaced by the P2a + P2b launch-prep validation passes — **19 distinct
schema / language pitfalls** hit while writing configs for production
repos. 12 surfaced in the P2a pilot (kubernetes, rust-lang/rust, deno,
airflow, turbo); 3 in P2a Wave 1 (clap, tokio, ruff, uv, typescript);
1 in P2a Wave 2 (next.js); 1 in P2a Wave 3 (helm); 2 in P2b Wave 1
(bazel + tensorflow). All configs ultimately parse + run, but the
iteration cost was high. This doc captures every one with the canonical
correct form.

> **Note on pitfall numbering.** The original *pitfall #18*
> claim (JSONPath outer-parens filter) was investigated during
> v0.9.15 Phase 4 and proven to be a misdiagnosis —
> `serde_json_path` 0.7.x accepts outer-parens filters; the
> original report had mis-attributed a dashed-key error inside
> the filter to the parens. The slot was left vacant until P2b
> Wave 1 surfaced two new walker/builder pitfalls that took
> #18 + #19.

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

## The 19 pitfalls

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
dotted, and other non-identifier keys must use bracket notation —
in **any** segment, top-level or inside a filter expression.

**Wrong (top-level):**
```yaml
- id: provider-package-name
  kind: yaml_path_matches
  paths: "providers/**/provider.yaml"
  path: "$.package-name"                 # ← parser fails on dash
  matches: '^apache-airflow-providers-'
```

**Right (top-level):**
```yaml
- id: provider-package-name
  kind: yaml_path_matches
  paths: "providers/**/provider.yaml"
  path: "$['package-name']"              # ← bracket notation
  matches: '^apache-airflow-providers-'
```

**Wrong (inside a filter):**
```yaml
- id: dependabot-actions-grouped
  kind: yaml_path_matches
  paths: ".github/dependabot.yml"
  path: "$.updates[?(@.package-ecosystem == 'github-actions')]"   # ← @.package-ecosystem dashed
  matches: '.+'
```

**Right (inside a filter):**
```yaml
- id: dependabot-actions-grouped
  kind: yaml_path_matches
  paths: ".github/dependabot.yml"
  path: "$.updates[?(@['package-ecosystem'] == 'github-actions')]"
  matches: '.+'
```

Either with or without the outer parens around the predicate is
fine for `serde_json_path` 0.7.x; the load-bearing fix is the
bracket-notation key access.

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

### 16. `*_path_matches` cannot regex-match against non-string values (bool, number, null) — use `*_path_equals` for typed values

`json_path_matches`, `yaml_path_matches`, and `toml_path_matches` apply
their `matches:` regex to the *string representation* of the matched
value. The implementation requires the underlying `serde_json::Value` /
`serde_yaml::Value` / `toml::Value` to be a string variant. Against a
bool field (e.g. `[package].publish`, `compilerOptions.strict`,
`engineStrict`), the rule fires this runtime error on **every match**:

```
value at path is not a string (got bool), can't apply regex
```

The kicker: this is a runtime semantic error, not a parse/build error,
so the `coverage_audit_examples_parse.rs` audit doesn't catch it. The
config "works" (parses, builds, evaluates) but emits the wrong signal —
flagging files that satisfy the intended check rather than ones that
don't. **Six instances of this bug surfaced in committed pilot + Wave 1
configs** before next.js's case study made the pattern explicit.

**Wrong:**
```yaml
- id: internal-crate-not-publishable
  kind: toml_path_matches
  paths: "internal/Cargo.toml"
  path: "$.package.publish"
  matches: '^false$'                       # ← runtime error: bool, not string
```

**Right (single-value bool — the common case):**
```yaml
- id: internal-crate-not-publishable
  kind: toml_path_equals                   # ← *_equals handles any Value type
  paths: "internal/Cargo.toml"
  path: "$.package.publish"
  equals: false                            # ← native YAML bool literal
```

`*_path_equals` deserialises the right-hand side into a generic
`serde_yaml::Value` and compares with `==`, so it transparently handles
booleans, numbers, null, arrays, and objects in addition to strings.

**Right (either-of-many bools — `*_path_equals` only matches one literal,
so fall back to a JSON-text regex):**
```yaml
- id: example-meta-declares-maintenance
  kind: file_content_matches
  paths: "examples/*/meta.json"
  pattern: '"maintainedByCoreTeam"\s*:\s*(true|false)\b'
```

The `file_content_matches` workaround sacrifices JSON-aware key resolution
(it can't follow `$.foo.bar.baz`), but for shallow top-level fields the
text regex is unambiguous.

**Rule of thumb:** if the field's value will be a bool, number, or null
(in JSON/YAML/TOML terms), use `*_path_equals`. Reach for `*_path_matches`
only when the value is genuinely a string AND you want a regex (substring
match, set membership via `^(a|b|c)$`, prefix anchor, etc.).

Source: `crates/alint-rules/src/structured_path.rs::check_match`.

### 17. `*_path_equals` against `[*]` JSONPath fires "wrong" on every non-matching element

`*_path_equals` against a JSONPath that returns multiple matches (e.g.
`$.formatters.enable[*]`) requires **every match** to equal the target.
The natural reading "is this value present in the array?" is *not*
what the rule does — it's "is every element of the array this value?".

Hit by helm's `helm-golangci-config-has-{gofmt,goimports}` first-draft
rules and silently broken in deno's `deno-dlint-includes-camelcase` for
weeks before Wave 3 caught it. Same flavour of silent runtime bug as
pitfall #16 — the rule loads + builds, but its semantics are inverted.

**Wrong (intent: "camelcase must be present in rules.include"):**
```yaml
- id: dlint-includes-camelcase
  kind: json_path_equals
  paths: .dlint.json
  path: "$.rules.include[*]"               # ← returns one match per element
  equals: "camelcase"                      # ← every element must equal "camelcase";
                                           #   fires on every other rule listed
```

**Right (option A — `file_content_matches` against the JSON text):**
```yaml
- id: dlint-includes-camelcase
  kind: file_content_matches
  paths: .dlint.json
  pattern: '"include"\s*:\s*\[[^\]]*"camelcase"'
```

**Right (option B — when "every element must satisfy a regex" *is* the
intent, `*_path_matches` is fine because the regex can validly cover the
union of legal values):**
```yaml
- id: cargo-deny-allowed-licenses
  kind: toml_path_matches
  paths: deny.toml
  path: "$.licenses.allow[*]"
  matches: '^(MIT|Apache-2\.0|BSD-3-Clause)$'
```

**Rule of thumb:** if the JSONPath ends in `[*]` or otherwise returns
multiple matches, ask "is the intent *all* or *any*?":
- *All* (every element must satisfy a constraint) → `*_path_matches` with
  a regex that matches every legal element, OR `*_path_equals` with a
  single literal that every element must equal (rare).
- *Any* (one element must satisfy a constraint) → `file_content_matches`
  on the raw text, OR wait for the v0.10+ `*_path_contains` primitive
  (proposed in `docs/launch-prep.md`'s rule-kind candidate table).

### 18. `.gitignore` masks tracked-file presence checks

A file can be both `git ls-files`-tracked AND listed in `.gitignore` —
a legitimate pattern when a tracked file ships a default that
contributors are expected to override locally without committing the
override. Bazel's own `.bazelversion` is the canonical example
(tracked by upstream, gitignored line 34 so a `bazel-7.0.0` override
locally doesn't show up in `git status`).

The `ignore` crate that powers the walker honours `.gitignore` by
default, so `file_exists` reports "no match" against a file that's on
disk *and* in `git ls-files` output. Worse: `git_tracked_only: true`
does NOT help — the engine pre-filters from the walker's emit set, so
a file the walker never sees can't be intersected back in.

**Wrong:**
```yaml
- id: bazel-version-pinned
  kind: file_exists
  paths: .bazelversion       # ← "no match" if .gitignore lists it
  level: error
```

**Right (option A — disable gitignore handling for the rule, when v0.10+ ships `respect_gitignore: false` per-rule):**
```yaml
- id: bazel-version-pinned
  kind: file_exists
  paths: .bazelversion
  respect_gitignore: false   # ← v0.10+ candidate
  level: error
```

**Right (option B — workspace-wide via the top-level config):**
```yaml
respect_gitignore: false      # ← already supported at config root
rules:
  - id: bazel-version-pinned
    kind: file_exists
    paths: .bazelversion
    level: error
```

**Right (option C — shell out for the existence check):**
```yaml
- id: bazel-version-pinned
  kind: command
  paths: .       # any anchor that exists; the script does the work
  command: ["test", "-f", ".bazelversion"]
  level: error
```

The trade-off with option B is global — every other rule in the config
also stops honouring `.gitignore`, which is rarely what you want.
Option A (per-rule) is the v0.10+ candidate motivated by this pitfall;
option C is the available workaround until then.

Source: surfaced by `examples/bazelbuild-bazel/.alint.yml` while
authoring the `gha-pin-actions-to-sha` companion check for
`.bazelversion`.

### 19. `file_exists` with `root_only: true` silently no-matches multi-component literal paths

`root_only: true` is an opt-in shortcut that limits the walk to the
repo root (no recursion). Internally the build path
(`file_exists::build` → `literal_is_nested(p)`) treats every
multi-component literal entry as "not at root" and produces "no match"
violations — but the message is the generic `file does not exist`,
not `path is not at root`, so the user sees a stale-config-style
error instead of the configuration mistake.

**Wrong:**
```yaml
- id: api-goldens-present
  kind: file_exists
  root_only: true                                 # ← root_only filters to repo root
  paths:                                           #   but the literals are nested
    - tensorflow/python/tools/api/golden/v1/      # ← "no match" — silently
    - tensorflow/python/tools/api/golden/v2/
  level: error
```

**Right (option A — drop `root_only:` for nested literals):**
```yaml
- id: api-goldens-present
  kind: file_exists
  paths:
    - tensorflow/python/tools/api/golden/v1/
    - tensorflow/python/tools/api/golden/v2/
  level: error
```

**Right (option B — `root_only:` ONLY where every literal is at root):**
```yaml
- id: bazel-version-at-root
  kind: file_exists
  root_only: true                  # ← OK; `.bazelversion` is at the root
  paths: .bazelversion
  level: error
```

A v0.9.16+ candidate is for `file_exists::build` to surface a
parse-time warning when it sees `root_only: true` with multi-component
literals: *"`.tensorflow/python/tools/api/golden/v1/` is multi-component;
either drop `root_only: true` or move the rule to a single-segment
literal at the repo root."* That'd land at-edit-time before the user
hits the silent-no-match.

Source: surfaced by `examples/tensorflow-tensorflow/.alint.yml` while
authoring the API-goldens existence check (1,185 textproto goldens
under `tensorflow/python/tools/api/golden/{v1,v2}/`).

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
- [ ] Every `*_path_matches` rule's target field is a string. For bool,
      number, or null fields, use `*_path_equals` with a YAML-native
      literal. For "either-of-many" bools, fall back to
      `file_content_matches` against the raw JSON text.
- [ ] Every `*_path_equals` rule's path is single-valued (no `[*]` /
      `..` / multi-match selectors), OR the intent is genuinely "every
      match must equal X". For "any element of array contains X", use
      `file_content_matches` (or wait for `*_path_contains`).
- [ ] No `file_exists` rule references a tracked-but-gitignored file
      without either `respect_gitignore: false` (workspace-level) or
      a `command:` shellout. `git_tracked_only: true` does not help
      — the walker pre-filters.
- [ ] `root_only: true` is only set when every entry in `paths:` is a
      single-segment literal at the repo root (no `dir/file` literals).

The `coverage_audit_examples_parse.rs` audit (added in v0.9.15) enforces
the first item by re-validating every `examples/*/.alint.yml` on every
CI run. The schema-level items (1-12, 15) are caught by the same parse,
so the audit covers them transitively. **Items 13, 14, 16, 17, 18, 19
are NOT caught by parse-validation** — they produce silently-wrong
runtime behaviour (regex never matches; `*_path_matches` against a bool
fires "not a string" on every match; `*_path_equals` against `[*]` flips
intent from "any" to "all"; `.gitignore` masks tracked files;
`root_only: true` silently no-matches multi-component literals) — see
§ "Parse-validation is necessary but not sufficient" below.

---

## Parse-validation is necessary but not sufficient

Four pitfalls in the catalogue above produce silently-wrong runtime
behaviour that the `coverage_audit_examples_parse.rs` audit cannot
catch:

- **#13** — regex `^`/`$` defaults to file-level anchoring; without `(?m)`
  the pattern silently never matches.
- **#14** — single-quoted YAML strings don't expand `\n`; the regex
  compiles into a literal `\n` two-char match that never appears in real
  files.
- **#16** — `*_path_matches` against a bool/number/null field emits a
  "not a string" runtime violation on every match, completely inverting
  the intended signal.
- **#17** — `*_path_equals` against a `[*]` JSONPath flips intent from
  "any element matches" to "every element must match", firing on every
  element that doesn't.
- **#18** — a tracked-but-`.gitignore`'d file is invisible to the
  walker by default; `file_exists` reports "no match" silently, and
  `git_tracked_only: true` doesn't help because the walker pre-filters.
- **#19** — `file_exists` with `root_only: true` silently no-matches
  every multi-component literal in `paths:`; the user sees a generic
  "file does not exist" error, not the configuration mistake.

All six classes share the same shape: the audit verifies the rule
*loads and builds*; nothing verifies the rule *fires correctly* against
realistic input.

For configs that depend on regex semantics or JSONPath value types —
`file_content_matches`, `file_content_forbidden`, `file_header`,
`file_footer`, every `*_path_matches`, every `*_path_equals` — a
second-pass smoke test against representative input is the only way to
catch the silent-failure modes. Two practical forms:

1. **Negative example file** — drop a file under `examples/<repo>/` that
   *should* trigger the rule, run `alint check`, confirm a violation.
2. **Local repo run** — clone the actual upstream repo, run the config
   against it, confirm rule counts are non-zero where expected. Use
   `alint list --config <path>` to see the full set of rules the engine
   *will* run (the human-readable text format prints all rules);
   `alint check --format json` filters out passing per-file rules
   entirely as an output optimisation, which can mislead an author into
   thinking a rule isn't loaded when it actually is — passing.

**v0.9.15 Phase 7 ships exactly that audit** — see
`crates/alint-e2e/fixtures/smoke/` and
`crates/alint-e2e/tests/coverage_audit_smoke_fixtures.rs`. Each
fixture is a self-contained config + file tree + `expected.toml`
declaring the canonical violation counts; the audit runs the engine
over each tree and asserts the actuals match. A regression in any of
the runtime-semantic pitfalls (#13/#14/#16/#17) — for instance, a
refactor that drops `(?m)` from a `file_content_matches` rule —
changes the violation count and fails the audit at PR time.

Phase 5 JSON Schema work catches pitfall #16 at editor-keystroke time
(the schema rejects `matches:` on `*_path_equals` rules); the smoke-
test audit is the runtime-correctness backstop for the regex /
multiline / array-semantics class.

Adding a fixture for a new pitfall is the right way to expand
coverage — see the README in `crates/alint-e2e/fixtures/smoke/` for
the format + a worked example.

---

## `alint validate-config` (v0.9.15)

Parse-validate a config without walking the tree:

```sh
# Default: discover from cwd, human format
alint validate-config

# Specific file (most explicit; what editor LSP integrations pass)
alint validate-config path/to/.alint.yml

# Specific directory (discovers `.alint.yml` inside)
alint validate-config path/to/repo

# JSON envelope for programmatic consumers (editor LSP, pre-commit, CI)
alint validate-config -f json
```

Three exit codes:

- `0` — config valid; all rules built cleanly
- `1` — config invalid (load / build / when-parse error). The error
  message carries the v0.9.15 Phase 3 + Phase 4 enrichments
  (did-you-mean, JSONPath dashed-key bracket-notation hints, `&&` →
  `and` keyword hints, etc.)
- `2` — invocation error (file missing, etc.)

The JSON shape is stable:

```json
{
  "valid": true,
  "rule_count": 70,
  "config_path": "examples/clap-rs-clap/.alint.yml",
  "error": null
}
```

Use this in a pre-commit hook to fail-fast on the way in, or wire it
into your editor's LSP runner to surface errors without paying for the
full tree walk that `alint check` does.

---

## Editor LSP via the JSON Schema (v0.9.15)

The full surface area of `.alint.yml` is described as a JSON Schema at
[`schemas/v1/config.json`](../../schemas/v1/config.json). Editors that
support YAML LSP (VS Code via the
[`redhat.vscode-yaml`](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml)
extension; JetBrains via the bundled YAML plugin; neovim via
[`coc-yaml`](https://github.com/neoclide/coc-yaml)) catch the bulk of
the schema-level pitfalls above at keystroke time — before you ever run
`alint check`.

To opt in, drop a one-line directive at the top of your `.alint.yml`:

```yaml
# yaml-language-server: $schema=https://raw.githubusercontent.com/asamarts/alint/main/schemas/v1/config.json
version: 1
extends:
  - alint://bundled/oss-baseline@v1
# …
```

All 20 case studies under `examples/*/.alint.yml` ship with this line
as a working reference.

### What the schema catches at edit time

The schema uses `allOf` of `rule_common` + a per-kind dispatch oneOf,
plus `unevaluatedProperties: false` at the top, so any unknown property
on a rule surfaces immediately. Concretely:

| Pitfall | Schema verdict |
|---|---|
| #1 `argv:` on `command` | rejected (unknown field) |
| #4 `secondary:` on `pair` | rejected (unknown field) |
| #5 `require:` on `pair` | rejected (unknown field) |
| #8 `style:` on `line_endings` | rejected (unknown field) |
| #9 `pattern:` on `file_starts_with` / `file_ends_with` | rejected (unknown field) |
| #15 empty `prefix:` on `file_starts_with` | rejected (`minLength: 1`) |
| #16 `matches:` on `*_path_equals` | rejected (unknown field) |

The continuously-verified spot-check list is in
`crates/alint-e2e/tests/coverage_audit_schema_drift.rs`. Drift between
the registry and the schema dispatch surfaces as a CI failure in the
same audit.

### What the schema does NOT catch

The runtime-semantic pitfalls (#13 regex anchoring, #14 YAML `\n` in
regex, #17 `*_path_equals + [*]`) compile into syntactically-valid
configs that misbehave at evaluation time. The schema can't see the
regex semantics or the JSONPath result-set cardinality. v0.9.15 Phase 7
adds a smoke-test fixture audit that closes that gap.

---

## See also

- [`docs/rules.md`](../rules.md) — full rule catalogue
- [alint.org/docs/rules/](https://alint.org/docs/rules/) — per-rule documentation
- [`crates/alint-rules/src/<kind>.rs`](../../crates/alint-rules/src/) — canonical schema (the `struct Options` block in each file)
- [`crates/alint-core/src/config.rs`](../../crates/alint-core/src/config.rs) — top-level `RuleSpec`, `FixSpec`, `NestedRuleSpec` schemas
- [`crates/alint-core/src/when.rs`](../../crates/alint-core/src/when.rs) — `when:` / `when_iter:` expression language
- [`crates/alint-core/src/scope_filter.rs`](../../crates/alint-core/src/scope_filter.rs) — `scope_filter:` semantics
