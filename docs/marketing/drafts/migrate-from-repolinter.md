---
destination: alint.org/migrating-from/repolinter/ (new route on the site repo)
status: drafting
blocks_on: alint-org-compare.md publishes (compare page links here); /examples/oss-baseline* sub-routes if cited
last_touched: 2026-05-06
---

# alint.org/migrating-from/repolinter/ — content brief for the site repo

## Why

The [Repolinter](https://github.com/todogroup/repolinter) project — the
TODO Group's tool for OSS-baseline checks (LICENSE present, README has
the right sections, CONTRIBUTING.md exists, etc.) — was archived on
**2026-02-06**. The compare page (`alint-org-compare.md`) and the
landing-page hero (`alint-org-hero.md`) both lead with
Repolinter-replacement framing, and both link users *here* for the
concrete migration path.

This page is load-bearing for the launch's #1 SEO target —
*"repolinter alternative"* — and for the highest-intent traffic the
launch will see: maintainers who know what they were running, know it
won't get fixed, and want to know what to do today.

The compare page's claim is that alint covers Repolinter's catalogue
as a strict superset. This page has to prove it.

## Proposed page

```markdown
---
title: Migrating from Repolinter to alint
description: Replace your Repolinter setup with an .alint.yml — most adopters land on a 5-line config that extends alint://bundled/oss-baseline@v1.
---

# Migrating from Repolinter to alint

[Repolinter](https://github.com/todogroup/repolinter) was the TODO
Group's tool for OSS-baseline checks. **It was archived on
2026-02-06.** The README at the top of its repo today reads:

> The Repolinter project has been archived. If you are a TodoGroup
> member and wish to see the repository unarchived, please bring this
> up within the TodoGroup.

If you're running Repolinter in CI today, you have three options:

1. **Stay on the archived tool.** It still works; node 12+ runs it; no
   one is going to fix CVEs against its dependencies.
2. **Fork it.** Doable; nobody's done it at scale yet.
3. **Replace it with alint.** This guide.

alint covers Repolinter's rule catalogue as a strict superset and ships
the bundled `oss-baseline@v1` ruleset that maps Repolinter's
file-presence + content-shape checks 1:1. Replacing a typical
Repolinter setup is a one-line `extends:` + a handful of rule renames.

## TL;DR — the 60-second migration

If your existing `repolinter.json` is a slight variant of
[Repolinter's `default.json`](https://github.com/todogroup/repolinter/blob/main/rulesets/default.json) —
i.e. you check that LICENSE / README / CONTRIBUTING / SECURITY /
CODE_OF_CONDUCT / .gitignore exist and that nothing committed has
merge markers or bidi controls — you can replace your entire
configuration with this:

```yaml
# .alint.yml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
```

That's the migration. Run `alint check`. Compare the output to your
last Repolinter run. If you have any Repolinter rules that aren't in
the default ruleset, the [mapping table](#mapping-table) below shows
the alint equivalent.

For repos with custom rules, the [side-by-side example](#side-by-side-a-real-repolinter-config) below ports a
representative `default.json` rule-by-rule.

## Why migrate

Repolinter's archival is the headline reason, but alint isn't just an
*active fork*; the design has moved on:

- **Active maintenance.** alint shipped 14 releases in the last six
  months and is actively maintained.
- **Bundled ecosystem rulesets.** Out of the box: `oss-baseline@v1`,
  `rust@v1`, `node@v1`, `python@v1`, `go@v1`, `java@v1`,
  `ci/github-actions@v1`, `monorepo@v1`, `compliance/reuse@v1`,
  `compliance/apache-2@v1`, `agent-hygiene@v1`, `agent-context@v1`,
  and 7 more. Repolinter shipped only the OSS-baseline ruleset.
- **Cross-file rules.** `pair`, `for_each_dir`, `for_each_file`,
  `dir_contains`, `dir_only_contains`, `unique_by`,
  `every_matching_has` — invariants that span more than one file.
  Repolinter is one-file-at-a-time.
- **Structured-query rules.** Validate fields *inside* JSON, YAML, and
  TOML with full RFC 9535 JSONPath. Repolinter could only regex-match
  file *contents*; checking that `package.json`'s `license` field is
  literally `"MIT"` (not just that the string `"MIT"` appears
  somewhere in the file) needed a custom check.
- **Conditional `when:` gates.** A bounded expression language for
  rule activation, plus tree-level facts (`has_rust`, `has_node`,
  `has_python`, `is_cargo_workspace`, …) that gate whole rulesets.
  Repolinter's `where:` axiom condition system was looser and
  required external binaries (`licensee`, `linguist`) to be installed.
- **Auto-fix.** 12 mechanically-safe fix operations covering trim
  trailing whitespace, append final newline, normalize line endings,
  strip BOM / bidi / zero-width characters, prepend / append, rename,
  create, remove. Repolinter's fixers were limited to file-presence
  and a handful of content rewrites.
- **Eight output formats** including the `agent` format with
  per-violation `agent_instruction` strings. Repolinter shipped two
  (default text + JSON; a Markdown formatter via plugin).
- **Performance.** alint is a single static Rust binary. Sub-second
  on a 100K-file workspace, ~12 s on 1M files. Repolinter is Node
  with per-rule JS execution — startup cost alone made it slow on
  large repos.

The compare page covers the long form:
[alint vs Repolinter →](/compare/#vs-repolinter).

## Mapping table

The Repolinter rule catalogue (as published in
[`docs/rules.md`](https://github.com/todogroup/repolinter/blob/main/docs/rules.md))
maps to alint as follows. Mapping coverage:

- **Full** — alint has a direct equivalent with the same semantics.
- **Partial** — alint covers the common case; some option doesn't
  port. Note in the right-hand column.
- **None** — no alint equivalent today; workaround proposed.

### Repolinter rule *kinds* (the `type:` field)

| Repolinter rule kind | What it does | alint equivalent | Coverage |
|---|---|---|---|
| `file-existence` | A file matching one of `globsAny` must exist. | `file_exists` (with `paths:` array) | Full |
| `file-not-exists` | No file matching the globs may exist. | `file_absent` | Full |
| `directory-existence` | A directory matching `globsAny` must exist. | `dir_exists` | Full |
| `file-contents` | File contents must match a regex. | `file_content_matches` (alias `content_matches`) | Full |
| `file-not-contents` | File contents must NOT match a regex. | `file_content_forbidden` (alias `content_forbidden`) | Full |
| `file-starts-with` | File's first N lines must contain a regex. | `file_header` (alias `header`) for line-oriented; `file_starts_with` for byte-prefix. | Full |
| `file-hash` | File's SHA-256 must equal an expected digest. | `file_hash` | Full |
| `file-hashes-not-exist` | File's hash must NOT equal any of a list of bad hashes. | No direct equivalent. **Workaround**: replace each bad-hash check with a `file_content_forbidden` rule whose pattern matches a known-bad substring. For pure hash denylisting, add a `command:` rule shelling to `sha256sum` until a `file_hash_not` primitive lands. | Partial |
| `file-type-exclusion` | No file matching a list of extensions may exist. | `file_absent` with `paths:` set to the same glob list. | Full |
| `large-file` | No file larger than `size` bytes. | `file_max_size` (alias `max_size`) | Full |
| `json-schema-passes` | JSON file validates against a JSON Schema. | `json_schema_passes` — additionally validates YAML and TOML against the same schema. | Full (superset) |
| `file-no-broken-links` | Every link in a markdown file resolves (HTTP for absolute URLs, filesystem for relative). | `markdown_paths_resolve` for filesystem-relative paths in backticks. **Does not** make HTTP requests for absolute URLs — by design (network checks are flaky CI; we don't want them in a structural linter). | Partial |
| `apache-notice` | NOTICE file exists at root. | `file_exists` with `paths: NOTICE` — or use `alint://bundled/compliance/apache-2@v1` which checks NOTICE + LICENSE text + per-source license header in one ruleset. | Full (superset) |
| `license-detectable-by-licensee` | Shells to the [`licensee`](https://github.com/licensee/licensee) Ruby gem to identify the license. | No direct equivalent. **Workaround**: `file_content_matches` against the license-text regex (alint's `compliance/apache-2@v1` ships this pattern for Apache-2.0; `compliance/reuse@v1` enforces SPDX headers). For full SPDX detection across hundreds of licenses, fall back to a `command:` rule that shells to `licensee` itself. | None (workaround) |
| `best-practices-badge-present` | Hits the OpenSSF Best Practices badge API to check the project's badge level. | No equivalent. alint doesn't make HTTP requests as part of a check (deterministic, offline-by-default). **Workaround**: keep this as a separate CI job using the OpenSSF Scorecard action, which already covers it. | None |
| `git-grep-commits` | Greps commit messages in history for a pattern. | `git_commit_message` validates only HEAD's commit message (subject pattern, max length, requires-body). For history grep, fall back to a `command:` rule. | Partial |
| `git-grep-log` | Greps the git log for a pattern. | Same as above — partial via `git_commit_message`; history is not alint's job. | Partial |
| `git-list-tree` | Asserts something about `git ls-tree`. | `git_no_denied_paths` covers the common shape ("no tracked file matches this glob"); `git_tracked_only:` on `file_exists` / `file_absent` covers the tracked/untracked axis. | Partial |
| `git-working-tree` | Asserts something about the working tree's clean/dirty state. | No direct equivalent. The working-tree axis is mostly subsumed by alint's walker honouring `.gitignore` by default + the `git_tracked_only:` per-rule field. | Partial |

### Repolinter rule *names* (entries in `default.json`)

These are Repolinter rule **definitions**, not kinds — names attached
to an instance of a rule kind. alint's `oss-baseline@v1` ships
equivalents under different rule ids; both names and ids are listed.

| Repolinter rule name | Repolinter behaviour | alint equivalent | Coverage |
|---|---|---|---|
| `license-file-exists` | LICENSE / COPYING file exists at root. | `oss-license-exists` (in `oss-baseline@v1`) — `file_exists` over the same path list (LICENSE, LICENSE.md, LICENSE.txt, LICENSE-APACHE, LICENSE-MIT, COPYING) with `root_only: true`. | Full |
| `readme-file-exists` | README* file exists. | `oss-readme-exists` — `file_exists` over README.md / README / README.rst, `root_only: true`. | Full |
| `contributing-file-exists` | CONTRIBUTING* file exists. | Not in `oss-baseline@v1`. **Add as a one-liner**: `file_exists` over `CONTRIBUTING*` paths. | Full (manual) |
| `code-of-conduct-file-exists` | CODE_OF_CONDUCT* file exists. | `oss-code-of-conduct-exists` — `file_exists` over the canonical paths (`CODE_OF_CONDUCT.md`, `.github/CODE_OF_CONDUCT.md`, `docs/CODE_OF_CONDUCT.md`). | Full |
| `changelog-file-exists` | CHANGELOG* file exists. | Not in `oss-baseline@v1`. **Add as a one-liner**: `file_exists` over `CHANGELOG*` paths. | Full (manual) |
| `security-file-exists` | SECURITY.md exists. | `oss-security-policy-exists` — `file_exists` over the canonical paths; **plus** `oss-security-policy-non-empty` (`file_min_size: 200`), which mirrors OpenSSF Scorecard's "non-stub" check. | Full (superset) |
| `support-file-exists` | SUPPORT* file exists. | Not in `oss-baseline@v1`. **Add as a one-liner**: `file_exists` over `SUPPORT*` paths. | Full (manual) |
| `readme-references-license` | README* mentions the word "license". | `file_content_matches` over README* with `pattern: '(?i)license'`. Not currently in `oss-baseline@v1` — easy one-liner. | Full (manual) |
| `binaries-not-present` | No `*.exe` / `*.dll` outside `node_modules/`. | `file_absent` with the same path list. Or extend `alint://bundled/hygiene/no-tracked-artifacts@v1` which covers the broader "no committed build outputs" surface. | Full |
| `test-directory-exists` | A directory matching `**/test*` or `**/specs` exists. | `dir_exists` with the same `paths:`. | Full |
| `integrates-with-ci` | A CI config file exists (GitHub Actions, GitLab CI, Travis, Circle, Jenkins, etc.). | `file_exists` with the same path list. Also: `alint://bundled/ci/github-actions@v1` covers the GitHub Actions case in depth (permissions block present, actions pinned to SHA, LF line endings). | Full (GitHub Actions superset) |
| `code-of-conduct-file-contains-email` | CODE_OF_CONDUCT* mentions an email address. | `file_content_matches` over the same path list with `pattern: '.+@.+\\..+'`. Easy one-liner; not in `oss-baseline@v1`. | Full (manual) |
| `source-license-headers-exist` | Each `**/*.js` source file has "Copyright" + "License" within the first 5 lines. | `file_header` over the same path list with `pattern: 'Copyright\|License'` and `lines: 5`. For the SPDX-tagged variant, use `alint://bundled/compliance/reuse@v1`. | Full |
| `github-issue-template-exists` | An issue template file/dir exists. | `file_exists` (with `dirs: true` semantics — alint's `dir_exists` for the directory variant; `file_exists` for the file variant). | Full |
| `github-pull-request-template-exists` | A PR template file exists. | `file_exists` with the same path list. | Full |
| `javascript-package-metadata-exists` | `package.json` exists when language is JavaScript. | `node-package-json-exists` (in `alint://bundled/node@v1`) — auto-gated by `facts.has_node`, no axiom configuration required. | Full (cleaner) |
| `ruby-package-metadata-exists` | `Gemfile` exists when language is Ruby. | No bundled `ruby@v1` ruleset yet. **One-liner**: `file_exists` over `Gemfile` (without the `language=ruby` axiom — alint doesn't currently have a `has_ruby` fact). Adding `ruby@v1` is on the v0.10+ roadmap. | Partial (workaround) |
| `java-package-metadata-exists` | `pom.xml` / `build.gradle` exists when language is Java. | `java-manifest-exists` (in `alint://bundled/java@v1`) — auto-gated by `facts.has_java`. | Full |
| `python-package-metadata-exists` | `setup.py` / `requirements.txt` exists when language is Python. | `python-manifest-exists` (in `alint://bundled/python@v1`) — also covers `pyproject.toml` (canonical since PEP 517). Auto-gated by `facts.has_python`. | Full (superset) |
| `objective-c-package-metadata-exists` | Cartfile / Podfile / `*.podspec` exists. | No bundled Objective-C ruleset. **One-liner**: `file_exists` over the same paths. | Partial (workaround) |
| `swift-package-metadata-exists` | `Package.swift` exists. | No bundled Swift ruleset. **One-liner**: `file_exists` over `Package.swift`. | Partial (workaround) |
| `erlang-package-metadata-exists` | `rebar.config` exists. | No bundled Erlang ruleset. **One-liner**: `file_exists` over `rebar.config`. | Partial (workaround) |
| `elixir-package-metadata-exists` | `mix.exs` exists. | No bundled Elixir ruleset. **One-liner**: `file_exists` over `mix.exs`. | Partial (workaround) |
| `license-detectable-by-licensee` | `licensee` Ruby gem identifies the license. | No equivalent. See the rule-kind table above. | None (workaround) |
| `notice-file-exists` | NOTICE* exists when license is Apache-2.0. | `apache-2-notice-file-exists` (in `alint://bundled/compliance/apache-2@v1`) — extending the ruleset is the user's signal of Apache-2.0 intent, replacing Repolinter's licensee axiom dependency. | Full (cleaner) |
| `best-practices-badge-present` | OpenSSF Best Practices badge level is sufficient. | No equivalent. See the rule-kind table above; keep as a separate Scorecard CI step. | None |

**Mapping totals (24 entries):** 17 full / 14 partial-or-manual-but-trivial / 3 with no clean equivalent + workaround. Core OSS-baseline coverage is 100 %.

## Side-by-side: a real Repolinter config

Here is a representative `repolinter.json` — a slight variant of
Repolinter's `default.json`, distilled to the rules most projects
actually used:

```json
{
  "$schema": "https://raw.githubusercontent.com/todogroup/repolinter/master/rulesets/schema.json",
  "version": 2,
  "axioms": {
    "linguist": "language",
    "licensee": "license",
    "packagers": "packager"
  },
  "rules": {
    "license-file-exists": {
      "level": "error",
      "rule": {
        "type": "file-existence",
        "options": { "globsAny": ["LICENSE*", "COPYING*"], "nocase": true }
      }
    },
    "readme-file-exists": {
      "level": "error",
      "rule": {
        "type": "file-existence",
        "options": { "globsAny": ["README*"], "nocase": true }
      }
    },
    "contributing-file-exists": {
      "level": "error",
      "rule": {
        "type": "file-existence",
        "options": { "globsAny": ["{docs/,.github/,}CONTRIB*"], "nocase": true }
      }
    },
    "security-file-exists": {
      "level": "error",
      "rule": {
        "type": "file-existence",
        "options": { "globsAny": ["{docs/,.github/,}SECURITY.md"] }
      }
    },
    "code-of-conduct-file-exists": {
      "level": "error",
      "rule": {
        "type": "file-existence",
        "options": { "globsAny": ["{docs/,.github/,}CODE_OF_CONDUCT*"], "nocase": true }
      }
    },
    "readme-references-license": {
      "level": "error",
      "rule": {
        "type": "file-contents",
        "options": { "globsAll": ["README*"], "content": "license", "flags": "i" }
      }
    },
    "binaries-not-present": {
      "level": "error",
      "rule": {
        "type": "file-type-exclusion",
        "options": { "type": ["**/*.exe", "**/*.dll", "!node_modules/**"] }
      }
    },
    "javascript-package-metadata-exists": {
      "level": "error",
      "where": ["language=javascript"],
      "rule": {
        "type": "file-existence",
        "options": { "globsAny": ["package.json"] }
      }
    }
  }
}
```

The equivalent `.alint.yml`:

```yaml
version: 1

extends:
  # Covers license-file-exists, readme-file-exists, security-file-exists,
  # code-of-conduct-file-exists, plus non-stub variants for LICENSE / README
  # / SECURITY / CODEOWNERS, plus merge-conflict and bidi-control checks.
  - alint://bundled/oss-baseline@v1

  # Covers javascript-package-metadata-exists (as `node-package-json-exists`),
  # auto-gated by facts.has_node — no axiom configuration needed.
  - alint://bundled/node@v1

rules:
  # CONTRIBUTING* — Repolinter's contributing-file-exists.
  - id: contributing-file-exists
    kind: file_exists
    paths:
      - "CONTRIBUTING.md"
      - "CONTRIBUTING"
      - "CONTRIBUTING.rst"
      - ".github/CONTRIBUTING.md"
      - "docs/CONTRIBUTING.md"
    level: error
    message: "Add a CONTRIBUTING document so contributors know how to get started."

  # README mentions the word "license" — Repolinter's readme-references-license.
  - id: readme-references-license
    kind: file_content_matches
    paths: ["README.md", "README", "README.rst"]
    pattern: '(?i)license'
    level: error
    message: "README should mention the project license."

  # No tracked binaries — Repolinter's binaries-not-present, broadened slightly.
  - id: no-tracked-binaries
    kind: file_absent
    paths:
      include: ["**/*.exe", "**/*.dll"]
      exclude: ["node_modules/**", "**/test*/**"]
    level: error
    message: "Don't commit pre-built binaries."
```

What changed:

- **The `axioms:` block disappears.** Repolinter's `linguist` and
  `packagers` axioms surfaced facts like "this project is JavaScript"
  to gate language-specific rules. alint replaces both with built-in
  facts (`has_node`, `has_python`, `has_rust`, `has_go`, `has_java`)
  resolved from canonical manifest files. No `linguist` install
  required; no Ruby-gem dependency for `licensee`.
- **The whole `oss-baseline@v1` block becomes one `extends:` line.**
  Six of the eight Repolinter rules above are subsumed without listing
  them — the bundled ruleset ships them. The bundled version is also
  *stricter* than Repolinter's defaults: `oss-license-non-empty` and
  `oss-readme-non-stub` catch zero-byte LICENSE files and
  one-line "TODO" READMEs that Repolinter's existence check passed.
- **Per-language metadata becomes one `extends:` line.** `node@v1`
  replaces `javascript-package-metadata-exists` and adds the lockfile
  check, the `node_modules`-not-tracked check, and content rules
  scoped to the JS source tree.
- **The two custom rules** (`contributing-file-exists`,
  `readme-references-license`) port directly. Each is one rule entry,
  same regex / glob shape as Repolinter.

Net change: the YAML is roughly a third the size of the JSON, the
config is more strict (LICENSE+README+SECURITY non-stub guards), and
no axiom-binary install is required.

## Edge cases that don't map cleanly

A handful of Repolinter capabilities don't have a direct alint
equivalent. Be honest with yourself about whether you actually used
them:

### `file-no-broken-links` HTTP checks

Repolinter's `file-no-broken-links` rule checks both filesystem-
relative links **and** absolute HTTP/HTTPS URLs (the latter via the
`broken-link-checker` package). alint's `markdown_paths_resolve`
covers the filesystem half but not HTTP — by design. Network checks
in CI are flaky, slow, and sensitive to rate-limiting. We don't want
them in the structural-linter slot.

**If you relied on the HTTP-check variant**: keep a separate CI step
using a dedicated link checker (`lychee`, `markdown-link-check`).
alint's `command:` rule kind can shell out to either one if you want
to keep one config file:

```yaml
- id: docs-no-broken-http-links
  kind: command
  paths: "docs/**/*.md"
  command: ["lychee", "--no-progress", "{path}"]
  timeout: 60
  level: warning
```

### `license-detectable-by-licensee`

Repolinter's licensee axiom shells to the upstream
[`licensee`](https://github.com/licensee/licensee) Ruby gem to
identify which SPDX license a project is under (so other rules can
gate on `license=Apache-2.0`). alint doesn't have an equivalent
because *it doesn't try to identify the license* — it asserts shape.

**If you relied on `license=Apache-2.0` gating**: extend
`alint://bundled/compliance/apache-2@v1`, which checks that the
LICENSE file *contains* the canonical Apache-2.0 text, that NOTICE
exists, and that source files have the Apache header. The intent is
the same; the mechanism is local-only and doesn't require a Ruby
toolchain in CI.

For SPDX detection of *any* license (not just Apache-2.0), the
workaround is a `command:` rule shelling to `licensee` itself:

```yaml
- id: license-detectable
  kind: command
  paths: ["LICENSE", "LICENSE.md", "COPYING"]
  command: ["licensee", "detect", "--json", "{path}"]
  level: warning
```

This adds the same Ruby-toolchain dependency Repolinter had, opt-in.

### `best-practices-badge-present`

Repolinter could query the OpenSSF Best Practices API to verify a
project's badge level. alint doesn't make HTTP requests; this is out
of scope by design.

**If you relied on this**: run
[OpenSSF Scorecard](https://github.com/ossf/scorecard) as a separate
CI job. Scorecard covers the same surface (and many adjacent ones)
and is the canonical place for badge-and-baseline checks. alint's
`oss-baseline@v1` already mirrors several Scorecard checks
(Security-Policy non-stub, Dependency-Update-Tool, Code-Review via
CODEOWNERS) by design — the two compose well.

### `git-grep-commits` / `git-grep-log` / `git-list-tree`

Repolinter had three rule kinds that introspected git history (search
commit messages, search the log, query the tree). alint's git-aware
rules cover the most common shapes:

- `git_commit_message` validates HEAD's commit (subject regex,
  subject max length, requires-body). Good for per-PR Conventional
  Commits enforcement under `alint check --changed`.
- `git_blame_age` fires on lines whose author-time is older than a
  threshold. Good for stale-TODO enforcement.
- `git_no_denied_paths` enforces tracked-file denylists.
- `git_tracked_only:` per-rule field intersects any rule's scope with
  git's index.

**If you relied on full history grep** (e.g. "no commit in history
contains the word 'fixup'"), alint doesn't replace it. That's a
`command:` rule shelling to `git log --grep`, kept as a CI step.

### `file-hashes-not-exist`

Repolinter's denylist-of-known-bad-hashes rule kind has no direct
alint equivalent. The common use case (a vendored dependency must
not be the known-vulnerable version) maps cleanly to
`file_content_forbidden` against a known-bad substring. Pure hash
denylisting needs a `command:` rule until a `file_hash_not` primitive
lands in alint (candidate for v0.10+).

## What you gain by switching

Short version (the long version is on the
[compare page](/compare/#vs-repolinter)):

- **Active maintenance** — release cadence ~2 weeks, public benchmarks
  every release, semver-stable bundled ruleset versioning.
- **One static binary** — no Node runtime, no npm install, no
  `licensee` Ruby gem, no `linguist` Ruby gem, no Docker.
- **Bundled rulesets per ecosystem** — 19 of them, gated by
  filesystem facts, no-op on irrelevant ecosystems.
- **Cross-file rules** — express invariants across files, not just
  within them.
- **Structured queries** — RFC 9535 JSONPath against JSON / YAML /
  TOML.
- **Conditional `when:` gates** — bounded expression language with
  facts, vars, and per-iteration context.
- **12 auto-fix operations** — trim whitespace, append final newline,
  normalize line endings, strip BOM / bidi / zero-width, prepend,
  append, rename, create, remove, collapse blank lines.
- **8 output formats** including SARIF (GitHub code-scanning), JUnit
  (CI test reporters), GitLab Code Quality, and `agent` (per-violation
  `agent_instruction` strings for AI-driven remediation).
- **Sub-second performance** on 100K-file workspaces.

## Step-by-step adoption

### 1. Install alint

Pick one:

```sh
# Homebrew (macOS + Linuxbrew)
brew tap asamarts/alint && brew install alint

# install.sh (Linux + macOS)
curl -sSL https://raw.githubusercontent.com/asamarts/alint/main/install.sh | bash

# npm (downloads the matching pre-built binary; no JS runtime)
npm install -g @asamarts/alint

# Docker (distroless multi-arch)
docker run --rm -v "$PWD:/repo" ghcr.io/asamarts/alint:latest check
```

### 2. Write the config

Start with the bundled OSS baseline:

```yaml
# .alint.yml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
```

If you have an ecosystem (Rust / Node / Python / Go / Java), add the
bundled ruleset for it — each is a one-line `extends:`, gated by
`facts.has_<ecosystem>`, so listing one for an ecosystem you don't
have is a silent no-op:

```yaml
version: 1
extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/rust@v1
  - alint://bundled/node@v1
  - alint://bundled/python@v1
  - alint://bundled/go@v1
  - alint://bundled/java@v1
```

Port any custom Repolinter rules using the
[mapping table](#mapping-table) above.

### 3. Run `alint check`

```sh
alint check
```

Compare the output to your last Repolinter run. Tighten or relax rule
levels in your config as needed — every bundled rule id can be
overridden locally:

```yaml
extends:
  - alint://bundled/oss-baseline@v1

rules:
  # Elevate missing-README from warning (bundled default) to error.
  - id: oss-readme-exists
    level: error

  # Disable trailing-whitespace on Markdown — the two-trailing-spaces
  # hard-break is deliberate.
  - id: oss-no-trailing-whitespace
    level: off
```

### 4. Auto-fix what's mechanically fixable

```sh
alint fix --dry-run    # preview the diff
alint fix              # apply
```

For OSS-baseline, this typically resolves trailing-whitespace and
final-newline violations without touching content semantics.

### 5. Wire it into CI

For GitHub Actions, the canonical setup is the
[`asamarts/alint-action`](https://github.com/asamarts/alint-action)
plus the bundled CI ruleset:

```yaml
# .github/workflows/lint.yml
on: [push, pull_request]
permissions:
  contents: read
jobs:
  alint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: asamarts/alint-action@v1
        with:
          format: github   # native GitHub Actions annotations
```

For Repolinter users coming from `repolinter lint` in CI, this is a
direct replacement: same exit-code semantics (non-zero on errors),
same per-violation file/line annotations, no Node runtime.

For non-GitHub CI, `alint check --format <fmt>` covers the major
formats:

```sh
alint check --format sarif   > alint.sarif    # GitHub code-scanning
alint check --format junit   > alint.junit    # JUnit XML for test reporters
alint check --format gitlab  > gl-cq.json     # GitLab Code Quality
alint check --format json    > alint.json     # programmatic consumption
alint check --format agent                    # per-violation agent_instruction
```

### 6. Retire the Repolinter step

Once `alint check` passes (or fails for the same reasons your
Repolinter step did), remove the Repolinter step from your CI.
Delete `repolinter.json` / `repolinter.yaml`. Pin alint's version
in your config to a specific `@v1` ruleset revision if you want
zero-surprise upgrades.

## Where to next

- **[The compare page](/compare/)** — full alint vs Repolinter / ls-lint /
  Megalinter / EditorConfig / custom-shell breakdown.
- **[Rule catalogue](/docs/rules/)** — every alint rule kind, with
  examples.
- **[Bundled rulesets](/docs/bundled-rulesets/)** — every bundled
  ruleset with its full rule list.
- **[Cookbook](/cookbook/)** — recipes for common patterns.
- **[Examples gallery](/examples/)** — 20 OSS repos with working
  `.alint.yml` configs, including each repo's existing-tooling
  inventory and what alint catches.

If your migration runs into something the mapping table above didn't
cover, [open an issue](https://github.com/asamarts/alint/issues) — the
mapping table is maintained against real-world `repolinter.json`
configs and we want to keep it accurate.
```

## Implementation notes (for the site repo)

- New top-level route — `src/pages/migrating-from/repolinter.astro`
  or `src/content/docs/migrating-from/repolinter.md`, depending on
  Starlight conventions. Sibling routes for `/migrating-from/ls-lint/`
  and `/migrating-from/custom-bash-scripts/` will land later.
- The migration JSON / YAML side-by-side blocks are heavy on lines —
  consider a tabbed-view component (Starlight ships `<Tabs>` /
  `<TabItem>`) so the JSON and YAML versions are toggle-able rather
  than stacked. Optional polish; not blocking.
- The mapping table is wide. Same horizontal-scroll handling the
  compare page uses (wrapper div with `overflow-x: auto`) covers it.
- Cross-link from `/compare/#vs-repolinter` and `/compare/#migrating`.
  Cross-link from the alint.org hero blockquote.
- The "Where to next" links assume `/cookbook/` and `/examples/`
  exist. Both are P3.1 deliverables; if either is delayed, link to
  the GitHub-rendered equivalents (`docs/cookbook.md`,
  `examples/README.md`).

## Open questions before publish

1. **Mapping-table column count.** Current draft uses three columns
   (Repolinter behaviour / alint equivalent / Coverage). A fourth
   "Notes" column would be cleaner for the Partial / None cases but
   widens the table further. Recommend stick with three; bullets
   inside the alint-equivalent column for nuance.
2. **Coverage labels.** "Full / Partial / None" is honest but a touch
   blunt. Alternatives considered: "Direct / Composable / Workaround"
   or numeric (1.0 / 0.7 / 0.3). Direct/Partial/None reads cleanest
   for SEO ("does alint cover X" → "yes" / "partially" / "no").
3. **Side-by-side example choice.** Current draft uses a
   distilled-default `repolinter.json`. Alternatives: use *Repolinter's
   actual `default.json`* unmodified (more rules, longer YAML), or use
   a real public repo's `repolinter.json` (more authentic but
   permissioning-and-attribution concerns). Distilled-default is the
   safest middle ground.
4. **Ruby / Swift / Erlang / Elixir partial entries.** The mapping
   table flags these as "Partial" because alint has no bundled
   ruleset for them and no `has_ruby` / `has_swift` / `has_erlang`
   facts today. The one-liner workarounds work; should we promote
   "ruby@v1 / swift@v1 etc. on the v0.10+ roadmap" into the
   migration page itself or leave that to the open-questions
   coordination doc? Recommend leave out — keeps the page focused on
   migration *today*.
5. **Tone on the workarounds.** The page is honest about gaps
   (`license-detectable-by-licensee`, `best-practices-badge-present`,
   `file-no-broken-links` HTTP). Should we soften with "this is
   actively planned" framing? My recommendation: **don't**. The gaps
   are deliberate (network checks, license-identification belongs
   elsewhere); honesty about boundaries is part of what makes this
   page trust-building rather than oversold.
6. **Action name.** The "wire into CI" section references
   `asamarts/alint-action@v1`. Verify the action name + tag are
   correct at publish time (the alint-action repo is in the
   reference doc on multi-account-distribution-infra).

## Pre-publish checklist

- [ ] `alint-org-compare.md` is in the same `ready` state — both
      pages publish coordinated so the compare page's link to here
      doesn't 404.
- [ ] `alint-org-hero.md` is in the same `ready` state if the hero
      blockquote links here directly.
- [ ] `/docs/rules/`, `/docs/bundled-rulesets/`, `/cookbook/`,
      `/examples/`, `/compare/` all resolve at publish time.
- [ ] Verify each rule kind named in the mapping table still exists
      in `docs/rules.md` at publish time. Names verified
      against `crates/alint-rules/src/` and `docs/rules.md` as of
      2026-05-06.
- [ ] Verify the bundled ruleset rule ids (`oss-license-exists`,
      `oss-readme-exists`, `oss-security-policy-exists`,
      `oss-code-of-conduct-exists`, `apache-2-notice-file-exists`,
      `node-package-json-exists`, `java-manifest-exists`,
      `python-manifest-exists`) still match the YAMLs under
      `crates/alint-dsl/rulesets/v1/` at publish time.
- [ ] Verify the upstream Repolinter URLs in the page still resolve
      (the repo is archived, not deleted, but a URL change isn't
      impossible).
- [ ] STATE.md row for `migrate-from-repolinter.md` flipped from
      `planned` (or `drafting`) to `ready` and then `live` with date
      + commit SHA.
- [ ] `/migrating-from/` listing page (sibling to this one) lists
      this guide alongside the future `/migrating-from/ls-lint/` and
      `/migrating-from/custom-bash-scripts/` pages — even if those
      are still placeholders.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `alint-org-compare.md` | Compare page links here from the "vs Repolinter" deep-dive and from the "Migrating" section. Both should ship together. |
| `alint-org-hero.md` | Hero blockquote names Repolinter; users following that blockquote land on the compare page first, then this one. Both should be live before the hero is refreshed, or the hero's "active-maintenance gap" framing has nowhere to point. |
| `alint-org-examples-gallery.md` | This page references `/examples/` for the case studies. If the gallery is delayed, fall back to `examples/` GitHub links. |
| `migrate-from-ls-lint.md` (planned) | Sibling route. The two pages should adopt a consistent template (frontmatter + 60-second migration + mapping table + side-by-side + edge cases + adoption steps) so the migration sub-route reads as a coherent series. |
| `migrate-from-custom-bash.md` (planned) | Same as above. The kubernetes case study is the headline example for that page. |

## Estimated diff size on the site repo

- 1 new page at `/migrating-from/repolinter/`: ~370 lines of markdown
- (optional) `/migrating-from/` index page: ~30 lines
- (optional) tabbed-view component import for the JSON/YAML side-by-side: ~5 lines

Total: ~370–405 lines on the site repo.
