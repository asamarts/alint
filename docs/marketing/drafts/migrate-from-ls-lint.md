---
destination: alint.org/migrating-from/ls-lint/ (new route on the site repo)
status: drafting
blocks_on: alint-org-compare.md publishes (compare page links here)
last_touched: 2026-05-06
---

# alint.org/migrating-from/ls-lint/ — content brief for the site repo

## Why

The compare page deliberately does **not** try to convert ls-lint
users. Its line is: *"use ls-lint if filename conventions are the only
thing you care about."* That position has to hold — ls-lint is a
focused Go binary that does its job well, and pressuring its users to
switch would undermine the compare page's credibility.

This guide is for the *other* shape of reader: someone who already runs
ls-lint, has now decided they also want to enforce structural and
content checks (file presence, manifest fields, content patterns,
cross-file invariants), and is asking whether they should bolt a second
tool onto ls-lint or migrate the naming rules into a single config.

It also rounds out the launch's migration-guide trio (Repolinter +
ls-lint + custom bash) and is the target for the SEO keyword
*"ls-lint alternative"* called out in launch-prep.

## Proposed page

```markdown
---
title: Migrating from ls-lint to alint
description: Step-by-step guide for ls-lint users adding structural and content checks. Includes a side-by-side config diff, a rule-by-rule mapping table, and notes on what alint does NOT 1:1 replicate.
---

# Migrating from ls-lint to alint

> **First, the honest framing.** If filename conventions are the
> *only* thing you care about, **stay on ls-lint**. It's a tight Go
> binary with a small config surface and it does that one job
> well — alint won't make naming-only enforcement faster or simpler.
>
> This guide is for ls-lint users who have decided they also want to
> enforce *structural* checks (required files, manifest fields,
> content patterns, cross-file invariants) and don't want to run two
> separate config files for two separate tools.

ls-lint enforces filesystem naming conventions: file and directory
basenames, depth-aware globbing, ignore lists. alint covers the same
ground via `filename_case` and `filename_regex`, plus the rest of its
[60-rule catalogue](/docs/rules/). If you're broadening scope past
naming, this page shows the migration path.

There's also a [composability path](#keep-ls-lint-add-alint) at the
end — alint and ls-lint can coexist if you'd rather keep your existing
`.ls-lint.yml` and only adopt alint for what ls-lint doesn't cover.

---

## Mapping table — ls-lint primitives to alint rules

| ls-lint primitive | alint equivalent | Coverage |
|---|---|---|
| `ls:` (top-level) | `rules:` (top-level) | full |
| `ls.<ext>: snake_case` | `kind: filename_case` + `case: snake_case` | full |
| `ls.<ext>: kebab-case` | `kind: filename_case` + `case: kebab-case` | full |
| `ls.<ext>: camelCase` | `kind: filename_case` + `case: camelCase` | full |
| `ls.<ext>: PascalCase` | `kind: filename_case` + `case: PascalCase` | full |
| `ls.<ext>: SCREAMING_SNAKE_CASE` | `kind: filename_case` + `case: SCREAMING_SNAKE_CASE` | full |
| `ls.<ext>: lowercase` | `kind: filename_case` + `case: lowercase` | full |
| `ls.<ext>: regex:<pattern>` | `kind: filename_regex` + `pattern: <pattern>` | full (alint anchors `^…$` automatically) |
| `ls.<ext>: point.case` | `kind: filename_regex` + `pattern: "[a-z0-9.]+\\.<ext>"` | partial — no built-in `point` case in alint; falls back to regex |
| `ls.<ext>: <a> \| <b>` (alternatives) | two rules with overlapping `paths:` and `level: warning`, OR one `filename_regex` whose pattern covers both shapes | partial — no native alternation operator; regex is the ergonomic substitute |
| `ls.<dir>.dir: snake_case` | `kind: filename_case` with `paths: "<dir>/**"` and a `scope_filter` to scope to directories | partial — alint's `filename_case` runs on file basenames; for directory-name conventions use `filename_regex` against `<dir>/**/*` paths or use `for_each_dir` with a nested check |
| `ls.<dir>.<ext>: …` (nested per-directory rule) | one rule per nested-config block, scoped via `paths: "<dir>/**/*.<ext>"` | full |
| `ls.<glob>/**: …` (deep-glob block) | one rule with `paths: "<glob>/**/*.<ext>"` | full |
| `ls.<ext>: exists:0` (forbid that extension) | `kind: file_absent` + `paths: "**/*.<ext>"` | full |
| `ls.<ext>: exists:N` (require N files) | no exact equivalent — closest is `kind: dir_contains` (require *at least one*) or `kind: max_files_per_directory` (cap upper bound) | partial |
| `ignore: [<patterns>]` (top-level) | top-level `ignore: [<patterns>]` (same key) plus implicit `.gitignore` (controllable via `respect_gitignore: false`) | full |
| `--warn` CLI flag (treat all rules as warnings) | per-rule `level: error \| warning \| info` | partial — alint sets severity per rule, not via a global flag (and there is no notion of `ls.error:` / `ls.warning:` blocks) |

The two rule kinds doing most of the work are
[`filename_case`](/docs/rules/naming/filename_case/) for the named
conventions and [`filename_regex`](/docs/rules/naming/filename_regex/)
for everything else. alint's `filename_case` accepts ls-lint's
keywords directly (`snake_case`, `kebab-case`, `camelCase`,
`PascalCase`, `SCREAMING_SNAKE_CASE`, `lowercase`, `UPPERCASE`,
`flatcase`) plus aliases (`pascal`, `pascal-case`, `UpperCamelCase`,
etc.) — config copy-paste from `.ls-lint.yml` to `.alint.yml` works
without rewriting the case keyword.

---

## Side-by-side: migrating ls-lint's own `.ls-lint.yml`

ls-lint's own [canonical config](https://github.com/loeffel-io/ls-lint/blob/main/.ls-lint.yml)
is a representative example — it covers the four shapes you'll find in
real configs (top-level extension rules, directory-only rules, nested
per-directory blocks, and an `ignore` list).

**Before — `.ls-lint.yml`:**

```yaml
ls:
  .dir: snake_case
  .*: snake_case
  .*.*: snake_case
  .*.*.*: exists:0
  .png: exists:0
  .jpg: exists:0
  .md: SCREAMING_SNAKE_CASE
  .bazel: SCREAMING_SNAKE_CASE
  .bazel.lock: SCREAMING_SNAKE_CASE

  examples/**: # allow only .yml files
    .dir: snake_case
    .*: exists:0
    .yml: kebab-case

  assets/**: # allow only .png files
    .dir: snake_case
    .*: exists:0
    .png: kebab-case

ignore:
  - .git
  - .github
  - genhtml
  - bazel-*
  - gha-*
  - deployments/npm/pnpm-lock.yaml
  - deployments/docker
```

**After — `.alint.yml`:**

```yaml
version: 1

# Top-level ignore list — same key, same semantics. .gitignore is
# also respected by default; set `respect_gitignore: false` to disable.
ignore:
  - .git
  - .github
  - genhtml
  - bazel-*
  - gha-*
  - deployments/npm/pnpm-lock.yaml
  - deployments/docker

rules:
  # `.dir: snake_case` — directory names must be snake_case.
  # filename_case checks basenames, so we use filename_regex against
  # any path component to enforce a directory naming rule.
  - id: dirs-snake-case
    kind: filename_regex
    paths: "**/*"
    pattern: "[a-z0-9_]+(\\.[a-z0-9_]+)?"
    stem: false
    level: error
    message: "Directory and file basenames must be snake_case."

  # `.*: snake_case` — every file basename must be snake_case.
  - id: files-snake-case
    kind: filename_case
    paths: "**/*"
    case: snake_case
    level: error

  # `.png: exists:0` and `.jpg: exists:0` — forbid those extensions
  # at the top level. (Top-level only; the assets/** block re-allows
  # .png inside assets/.)
  - id: no-png-at-root
    kind: file_absent
    paths:
      include: ["**/*.png"]
      exclude: ["assets/**/*.png"]
    level: error

  - id: no-jpg
    kind: file_absent
    paths: "**/*.jpg"
    level: error

  # `.md: SCREAMING_SNAKE_CASE` — Markdown filenames must shout.
  - id: markdown-screaming
    kind: filename_case
    paths: "**/*.md"
    case: SCREAMING_SNAKE_CASE
    level: error

  # `.bazel` / `.bazel.lock` — Bazel manifest names.
  - id: bazel-screaming
    kind: filename_case
    paths: ["**/*.bazel", "**/*.bazel.lock"]
    case: SCREAMING_SNAKE_CASE
    level: error

  # examples/** — only .yml files allowed; they must be kebab-case.
  - id: examples-only-yml
    kind: dir_only_contains
    select: "examples/**"
    allow: ["*.yml"]
    level: error

  - id: examples-yml-kebab
    kind: filename_case
    paths: "examples/**/*.yml"
    case: kebab-case
    level: error

  # assets/** — only .png files allowed; they must be kebab-case.
  - id: assets-only-png
    kind: dir_only_contains
    select: "assets/**"
    allow: ["*.png"]
    level: error

  - id: assets-png-kebab
    kind: filename_case
    paths: "assets/**/*.png"
    case: kebab-case
    level: error
```

**What changed:**

- ls-lint's two-axis indexing (extension key + scoping by directory)
  is split into one alint rule per (case-convention, scope) pair.
  Reads more verbose; trades implicit indexing for explicit `id:`s and
  per-rule `level:`s.
- `.dir: snake_case` becomes a `filename_regex` rule applied to *all
  paths* with a snake-case pattern that admits dotted segments. ls-lint
  applies `.dir` to directories specifically; alint's filename rules
  run on file basenames, but checking every path component via regex
  is functionally equivalent for the "directories must be snake_case"
  intent.
- `.*: exists:0` inside `examples/**` and `assets/**` becomes a
  [`dir_only_contains`](/docs/rules/cross-file/dir_only_contains/)
  rule. This is more declarative than ls-lint's "forbid every
  extension except the allowed ones" pattern.
- `.png: exists:0` at top level uses an
  [include/exclude path pair](/docs/concepts/paths/) — top-level PNGs
  forbidden, `assets/**/*.png` exempted. ls-lint's nested `assets/**`
  block does the equivalent implicitly.
- `ignore:` is kept verbatim. alint's top-level `ignore:` has the same
  semantics as ls-lint's.

---

## What alint adds beyond naming

This is the part the page promised wouldn't be pushy — keeping that
promise. If you've adopted alint to absorb your naming rules, here's a
tour of what *else* the catalogue covers, framed as "now that you have
the config, here's what's also available." None of this needs to be
adopted day one.

- **Required files + content shape.** `file_exists`, `file_absent`,
  `file_min_lines`, `file_max_size`, `file_header`, `file_starts_with`
  / `file_ends_with`, `file_hash`. The bundled
  [`oss-baseline@v1`](/docs/bundled-rulesets/oss-baseline/) ruleset
  packages 14 of these into a single `extends:` line for OSS-hygiene
  basics (LICENSE / README / SECURITY.md / CODEOWNERS / etc.).
- **Structured queries inside JSON / YAML / TOML.** `json_path_equals`,
  `yaml_path_matches`, `toml_path_equals`, `json_schema_passes` — full
  [RFC 9535 JSONPath](/docs/concepts/structured-queries/) for asserting
  fields inside `package.json`, `Cargo.toml`, GitHub workflows.
- **Cross-file invariants.** `pair`, `for_each_file`, `for_each_dir`,
  `dir_contains`, `dir_only_contains`, `unique_by`, `every_matching_has`
  — invariants like "every `tests/unit/*.rs` has a matching
  `tests/snapshots/{stem}.snap`" or "every `packages/*/` has both a
  README and a license."
- **Text hygiene + encoding.** `no_trailing_whitespace`,
  `final_newline`, `line_endings`, `no_bom`, `no_bidi_controls`,
  `no_zero_width_chars`, `no_merge_conflict_markers`. Twelve of these
  ship auto-fixers.
- **Per-ecosystem bundled rulesets.** rust / node / python / go / java
  / monorepo / CI — drop-in `extends:` lines for the conventions a
  given ecosystem expects, no copy-paste of dozens of rules from a
  cookbook. See [bundled rulesets](/docs/bundled-rulesets/).
- **Conditional `when:` gates.** Run rules only when a fact holds —
  e.g. `when: facts.has_node` to skip Node-flavoured rules in pure-Rust
  workspaces. ls-lint has nothing analogous.
- **Auto-fix.** `alint fix` rewrites in place for 12 mechanically-safe
  ops (whitespace, line endings, BOM, bidi, prepend/append, rename).
  ls-lint reports problems but doesn't fix them.

This is opt-in catalogue, not a migration requirement.

---

## Edge cases — things alint doesn't replicate 1:1

In the spirit of being honest:

1. **No native `extends:`-from-zero starting point.** ls-lint's
   `.ls-lint.yml` is self-contained — every rule is in the file. alint
   *can* be self-contained the same way, but its convention is to use
   the `extends:` mechanism to layer bundled or shared rulesets. If
   you migrate verbatim and don't extend anything, that's fine —
   alint doesn't require you to use `extends:`.
2. **No `error:` / `warning:` config blocks.** ls-lint v2.x has a
   global `--warn` flag and (in some 2.x versions) `error:` /
   `warning:` config sections that downgrade severity for groups of
   rules. alint expresses severity per-rule via `level: error |
   warning | info`. There is no global "treat everything as
   warning" flag; you express it by setting `level:` on each rule.
3. **No `point.case` keyword.** ls-lint defines a `point.case`
   convention (lowercase letters, digits, dots only). alint's
   `filename_case` doesn't ship that convention by name — fall back
   to `filename_regex` with `[a-z0-9.]+`.
4. **No alternation operator on the case rule itself.** ls-lint's
   `kebab-case | PascalCase` says "either is OK." alint's
   `filename_case` accepts one convention; for an "either" relationship
   write a single `filename_regex` whose pattern covers both shapes.
5. **Directory-name rules require a different rule shape.** ls-lint's
   `.dir:` keyword scopes a case rule to directory names specifically.
   alint's `filename_case` runs on file basenames. To enforce a
   directory naming convention, use `filename_regex` on path components
   (matching every file under that directory) or use `for_each_dir`
   iterating the directories you care about.
6. **`exists:N` for N > 0 doesn't have a clean equivalent.** ls-lint
   can require *exactly N* files of a given extension in a directory.
   alint's closest primitives are `dir_contains` (at-least-one) and
   `max_files_per_directory` (at-most-N). An "exactly N" check would
   need a `command` rule shelling out — usually a sign the constraint
   is over-specified.
7. **Slightly different glob dialect.** ls-lint and alint both use
   doublestar globs (`**`, `*`, `{a,b}`); there are tiny edge cases in
   trailing-slash handling and brace expansion. Test your `.alint.yml`
   in dry-run mode against your repo before flipping CI to alint.

---

## <a id="keep-ls-lint-add-alint"></a>Composability — keep ls-lint, add alint

If you're happy with `.ls-lint.yml` and just want to add structural
checks without rewriting it, the two tools coexist cleanly. alint's
[`command`](/docs/rules/plugin/command/) rule shells out to an external
CLI per matched file; you can register `ls-lint` as one of those:

```yaml
version: 1

rules:
  # Delegate naming enforcement to ls-lint, keep the existing
  # .ls-lint.yml. alint handles everything else.
  - id: ls-lint-passes
    kind: command
    paths: "**/*"  # one invocation; ls-lint walks the whole tree
    command: ["ls-lint", "--workdir", "{path}"]
    level: error

  # …and add alint's structural / content / cross-file rules below.
  - id: oss-baseline
    extends:
      - alint://bundled/oss-baseline@v1
```

(In practice you'd only need the `ls-lint-passes` rule once at the
repo root, not per-file. The `command` rule kind is per-file; if you
want one ls-lint invocation per run rather than per-file, run
ls-lint as a separate CI step before alint and use alint just for the
non-naming checks. Either pattern works.)

This is a real adoption path — many users will want to keep their
existing tool and just add capability rather than migrate.

---

## Step-by-step adoption

Pick one of the two paths.

### Path A — full migration (one config)

1. **Install alint.** `brew install alint` / `cargo install alint` /
   download a [release binary](https://github.com/asamarts/alint/releases).
2. **Copy `.ls-lint.yml` → `.alint.yml`** as a starting point.
3. **Translate each rule** using the [mapping table above](#mapping-table)
   — most lines port verbatim with a wrapper `kind: filename_case`
   block; a handful (`.dir:`, `exists:N`, alternation) need the
   targeted edits called out in [Edge cases](#edge-cases).
4. **Add the OSS structural baseline** if you want it:
   ```yaml
   extends:
     - alint://bundled/oss-baseline@v1
   ```
   This is the `oss-readme-exists`, `oss-license-exists`,
   `oss-no-merge-conflict-markers`, `oss-no-bidi-controls`,
   `oss-final-newline`, `oss-no-trailing-whitespace`, plus a few more
   conservative defaults. Override severities locally if needed.
5. **Run `alint check`** and audit. Use `alint check --output github`
   for a PR-friendly diff or `alint check --output agent` if you're
   piping into an AI assistant.
6. **Retire `.ls-lint.yml`** and the `ls-lint` CI step once `.alint.yml`
   is green.

### Path B — composability (keep ls-lint, add alint)

1. **Install alint.** Same as above.
2. **Add a minimal `.alint.yml`** with `extends: alint://bundled/oss-baseline@v1`
   (or whichever bundled rulesets fit your stack).
3. **Add `alint check` as a second CI step** alongside the existing
   ls-lint step.
4. **Iterate** — over time, if you find yourself maintaining the same
   naming rules in two places, you can revisit Path A. If you don't,
   composability is the resting state.

Either path is correct. The migration guide isn't pushing one over the
other.

---

## Help / questions

- Full rule reference: [/docs/rules/](/docs/rules/)
- Bundled ruleset catalogue: [/docs/bundled-rulesets/](/docs/bundled-rulesets/)
- Tool comparison: [/compare/](/compare/) (covers ls-lint, Repolinter,
  Megalinter, EditorConfig, custom shell)
- Issue tracker: [github.com/asamarts/alint](https://github.com/asamarts/alint/issues)
```

## Implementation notes (for the site repo)

- New route under `/migrating-from/ls-lint/`. Sibling pages:
  `/migrating-from/repolinter/` and `/migrating-from/custom-bash-scripts/`
  — see those drafts for layout conventions if any are already in
  flight.
- Astro/Starlight: anchor links inside the page (`#mapping-table`,
  `#edge-cases`, `#keep-ls-lint-add-alint`) rely on Starlight's default
  heading-id derivation. Verify the slugs render as expected; the
  inline `<a id="…">` on the composability heading is a defensive
  override since "Composability — keep ls-lint, add alint" produces a
  long auto-slug.
- The "Before / After" config blocks are the page's centrepiece —
  they need horizontal-scroll handling on mobile if Starlight's
  default code-block styling clips long lines. Same fix pattern as
  the compare page's feature matrix (`overflow-x: auto`).
- Cross-link from `/compare/`'s "vs ls-lint" deep dive (already in the
  compare draft as a "see the migration guide" callout) — the link
  target is this page's URL.

## Open questions before publish

1. **Does the side-by-side use *ls-lint's own* config or a synthetic
   one?** Currently uses ls-lint's own `.ls-lint.yml` from the
   `loeffel-io/ls-lint` repo. Pros: real, recognisable, links back to
   the source for verifiability. Cons: ls-lint's own config is unusual
   in places (the `.bazel.lock` extension key isn't a typical
   ls-lint pattern). Alternative: synthesise a 5-rule config that
   exercises the four common shapes (top-level case, nested
   per-directory, `exists:0` forbid, `ignore`). Recommend keeping the
   real config — verifiability beats aesthetic cleanness.
2. **Tone calibration.** The TL;DR explicitly says "stay on ls-lint if
   naming is your only need." Is that strong enough? Worth a second
   eyeball — does the rest of the page accidentally drift into
   selling? Specific worry: the "What alint adds beyond naming"
   section is bullet-heavy and could read as pitchy if skimmed.
3. **`scope_filter` for directory-name rules.** The mapping table
   currently routes `.dir:` rules through `filename_regex` against all
   paths. A cleaner option might be `for_each_dir` with a nested rule
   asserting a basename pattern — but that's heavier syntax for users
   who just want "all my dirs are snake_case." Worth a usability
   second-look. Could ship the simpler regex-based answer first and
   refactor later.
4. **Does the composability section need a working example we've
   actually tested?** The `command:` rule's per-file invocation
   semantics are real but using it to wrap ls-lint as one global pass
   is awkward (the per-file invocation pattern doesn't fit a tool that
   walks the whole tree itself). The page acknowledges this in
   parentheses; could expand to a worked CI-step example if reviewers
   feel the parenthetical is too hand-wavy.

## Pre-publish checklist

- [ ] `/compare/` page is live (otherwise the cross-link in the help
      footer 404s) — that's `alint-org-compare.md` draft.
- [ ] `/docs/rules/naming/filename_case/` and
      `/docs/rules/naming/filename_regex/` resolve (auto-generated
      from docs-bundle; should already be live).
- [ ] `/docs/rules/cross-file/dir_only_contains/`,
      `/docs/rules/cross-file/dir_contains/`,
      `/docs/rules/plugin/command/` all resolve.
- [ ] `/docs/bundled-rulesets/oss-baseline/` resolves.
- [ ] `/docs/concepts/paths/` and `/docs/concepts/structured-queries/`
      resolve (or replace with whichever URL the site repo actually
      uses for those pages).
- [ ] Verify that the canonical-config link
      (`https://github.com/loeffel-io/ls-lint/blob/main/.ls-lint.yml`)
      is still pointing at the same content at publish time — ls-lint
      could refactor their config in a way that makes the side-by-side
      stale.
- [ ] Anchor IDs render correctly under Starlight (`#mapping-table`,
      `#edge-cases`, `#keep-ls-lint-add-alint`).
- [ ] STATE.md row for `migrate-from-ls-lint.md` flipped to `live`
      with date + commit SHA.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `alint-org-compare.md` | The "vs ls-lint" deep dive on the compare page links here. Compare page must be either ready-to-publish or already-live before this guide ships. |
| `migrate-from-repolinter.md` (sibling guide) | Layout conventions should match across the migration-guide trio. If the Repolinter guide ships first and chooses a slightly different structure (mapping table → side-by-side → edge cases → adoption), align this guide to whatever that draft sets. |
| `migrate-from-custom-bash.md` (sibling guide) | Same — alignment on layout. |
| `keyword-landing-pages.md` (P3.2) | Includes a `/ls-lint-alternative/` landing page that should redirect to (or strongly link to) this guide as the canonical resource for that intent. |

## Estimated diff size on the site repo

- 1 new page at `/migrating-from/ls-lint/`: ~210 lines of markdown
- (possibly) 1 line in the migration-guide nav config / index page

Total: ~210-220 lines on the site repo.
