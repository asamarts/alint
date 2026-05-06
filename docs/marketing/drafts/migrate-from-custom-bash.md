---
destination: alint.org/migrating-from/custom-bash-scripts/ (new route on the site repo)
status: drafting
blocks_on: alint-org-compare.md publishes (compare page links here); examples/kubernetes-kubernetes/ sub-route exists for the headline link
last_touched: 2026-05-06
---

# alint.org/migrating-from/custom-bash-scripts/ — content brief for the site repo

## Why

The `compare` page already links here (*"From custom bash scripts →"*).
Of the three migration-from pages, this is the **broadest** — Repolinter
and ls-lint are concrete tools with concrete configs to translate;
"custom bash scripts" is a *pattern* (script sprawl) not a tool, so the
page has to teach a way of thinking rather than walk a mechanical
mapping.

The page's job is to talk to engineers who already maintain a
`hack/verify-*.sh` (or `scripts/check-*.py`, or `tools/lint-*.js`)
directory and have evaluated whether tooling helps them. They've heard
the sales pitch before. They will not finish reading anything that
overpromises.

So the framing is **consolidation, not replacement** — borrowed
verbatim from the compare-page draft:

> *alint replaces the structural subset. The realistic outcome isn't
> 100 % replacement — it's consolidation of the declarative subset,
> which usually means smaller `hack/` directory, faster CI, and one
> config that new contributors can read instead of spelunking through
> bash.*

The strongest single piece of evidence we have for this story is the
[kubernetes case study](/examples/kubernetes-kubernetes/) (50 verify
scripts → 17 declarative replacements). This page leans on it heavily,
backstops it with cpython (12 of 56 surfaces) and airflow (~40 of 109
hooks), and stays honest about what alint deliberately doesn't cover.

## Proposed page

```markdown
---
title: Migrating custom bash validation scripts to alint
description: How to consolidate hack/verify-*.sh / scripts/check-*.py / tools/lint-*.js sprawl into one declarative .alint.yml — without losing the flexibility of "I can do anything in bash."
---

# Migrating custom bash validation scripts to alint

Many repos accumulate a `hack/verify-*.sh` (or `scripts/check-*.py`, or
`tools/lint-*.js`) directory over time. Each script earned its place
solving a real problem when bash + grep was the only thing within reach.
The maintenance cost grows quietly: the patterns are usually structural,
the scripts are usually copy-paste, and a new contributor staring at 30+
files in `hack/` has to read all of them to understand the structural
contract.

**This isn't usually 100 % replacement — it's consolidation of the
declarative subset.** alint takes the structural subset; the AST-aware,
runtime, generator, and cross-API-version checks stay where they are.

## The kubernetes story

[kubernetes/kubernetes](/examples/kubernetes-kubernetes/) maintains
**50 `hack/verify-*.sh` scripts** that gate every PR. After an inventory:

- **17 scripts move to alint.** 12 map directly to alint rule kinds
  (`file_header`, `file_max_size`, `for_each_dir`, `pair`, `filename_regex`,
  …); 5 more compose with the `command` rule kind to wrap existing tools
  (`shellcheck`, `gofmt`, `golangci-lint`, `govulncheck`, `misspell`).
- **7 stay as scripts** because they need primitives alint deliberately
  doesn't ship: language-aware import gates, cross-API-version diffs,
  AST-aware metric-name regex, vendor-tree readonly enforcement.
- **18 stay as scripts** because they're out of alint's scope by design:
  codegen drift, OpenAPI generation, Go module-graph analysis, runtime
  binary symbol checks.
- **6 + 2 are duplicates / pre-existing** (`verify-all.sh` is just a
  runner, vuln scanning belongs to `cargo audit`-style tools).

Net: a 50-script directory shrinks to **33 scripts + one `.alint.yml`**.
The 33 surviving scripts run faster (alint runs declarative rules in
parallel; the shell pipeline doesn't), and a new contributor reads the
config instead of grepping through 17 files.

[cpython](/examples/python-cpython/) tells the same story at a different
shape: 56 distinct structural-validation surfaces today (a 122-target
`Makefile.pre.in`, 35 pre-commit hooks, 9 ruff configs, 7
`Tools/build/*.py` checkers, `.gitattributes`, `.editorconfig`, 25 GH
workflows). 38 % map to alint; the rest are AST-aware, codegen, or
binary-symbol checks that stay on the existing tooling.

[apache/airflow](/examples/apache-airflow/) tells it from the
pre-commit-driven Python ecosystem: 109 pre-commit hooks, ~40 % map.
The most alint-shaped surface (the 101-provider package layout
contract) lives today in a 1085-line Python script that imports every
provider; alint expresses the same contract in 25 lines of
`for_each_file` + nested `require:`.

---

## Pattern catalogue

Common bash patterns and their alint equivalents. Every rule kind named
here exists today; verify your version supports the listed options
([rule catalogue](/docs/rules/)).

| Common bash pattern | alint rule | Notes |
|---|---|---|
| `grep -rn 'TODO\|XXX\|FIXME' src/` | `file_content_forbidden` | Direct map. Pair with `git_blame_age` if you want "TODOs older than 6 months" instead of "any TODO". |
| `find . -name '*.tmp' -or -name '*.swp'` | `file_absent` | Direct map. The `agent-hygiene@v1` bundled ruleset already covers `*.bak`, `*.orig`, `*~`, `*.swp` in one `extends:` line. |
| `find . -name '.DS_Store' -delete` | `file_absent` with `fix: file_remove` | Auto-fix supported. The `hygiene/no-tracked-artifacts@v1` bundled ruleset ships this rule. |
| `for d in packages/*/; do test -f "$d/README.md" \|\| exit 1; done` | `for_each_dir` + `file_exists` | Direct map. `monorepo@v1` bundled ruleset ships this for `packages/*`, `crates/*`, `apps/*`, `services/*`. |
| `for d in crates/*/; do grep -q 'license =' "$d/Cargo.toml" \|\| exit 1; done` | `for_each_dir` + `toml_path_matches` | Direct map. RFC 9535 JSONPath against TOML. |
| `for f in $(find . -name '*.sh'); do [ -x "$f" ] \|\| exit 1; done` | `shebang_has_executable` / `executable_has_shebang` | Direct map. Pair both for the bidirectional invariant. |
| `grep -rE '[ \t]+$' --include='*.rs' src/` | `no_trailing_whitespace` | Direct map. Auto-fixable via `file_trim_trailing_whitespace`. |
| `find . -name '*.md' \| xargs file \| grep -v 'CRLF'` | `line_endings` (`target: lf`) | Direct map. Auto-fixable via `file_normalize_line_endings`. |
| `git diff --check` (in CI) | `no_merge_conflict_markers` | Direct map. Already in `oss-baseline@v1`. |
| `for f in test/parallel/*.js; do [[ "$f" =~ ^test-.*\.js$ ]] \|\| exit 1; done` | `filename_regex` | Direct map. (This is the actual nodejs/node convention — silent test discovery on a misnamed file.) |
| `jq -e '.license == "MIT"' package.json` | `json_path_equals` | Direct map. Use `*_matches` if a regex is needed instead of equality. |
| `python -c "import json; json.load(open('config.json'))"` | `json_schema_passes` (or any structured-path rule) | Parse-error becomes a single violation per file rather than a 1-line pass/fail. |
| `for f in .github/workflows/*.yml; do yq '.permissions.contents' "$f"; done \| grep -qv read` | `yaml_path_equals` (or `gha-workflows-have-permissions` from `ci/github-actions@v1`) | Direct map. Bundled ruleset already encodes this. |
| `grep -E '^pin: [a-f0-9]{40}' .github/workflows/*.yml` | `yaml_path_matches` (with a SHA-pin regex) | Direct map. Bundled `gha-workflows-pin-actions`. |
| `head -1 *.rs \| grep -q 'SPDX-License-Identifier'` | `file_header` | Direct map. Add a fix block (`file_prepend`) to auto-insert. |
| `du -k . \| awk '$1 > 5120 {print $2}'` (no file > 5 MB) | `file_max_size` | Direct map. The kubernetes config uses this with `paths.exclude` for vendor + testdata. |
| `find packages/ -name package.json -exec ...` (every dir contains foo) | `dir_contains` / `for_each_dir` | Direct map. `dir_contains` is sugar for the common shape. |
| `find . -name README.md \| awk -F/ '{print $NF}' \| sort \| uniq -d` (no basename collisions) | `unique_by` (`key: "{stem}"`) | Direct map. |

### Patterns with no clean alint mapping

These are the kinds of script you keep. alint deliberately doesn't try
to do them:

| Common bash pattern | Why alint can't / won't |
|---|---|
| `kubectl version --client \| grep '^Client Version: v1.2'` | Runtime probe. alint is a static checker; shelling out to a binary at lint time crosses the runtime/static line. Use `command:` if you want it inside the same gate, but be aware it's a static-tool wrapping a runtime probe. |
| Cross-API-version diffs (kubernetes' `verify-types-aliases.sh`) | Requires parsed Go code + cross-version semantic comparison. AST territory; out of scope. |
| `kustomize build ... \| diff -` (codegen freshness) | Running the generator + diffing the output is on the v0.10+ candidate list (`generated_file_fresh`); not in v0.9. Keep your script. |
| `grep -E 'import "github.com/foo/bar"' --include='*.go' some/path/` (forbid an import path in a directory) | This is the `import_gate` rule kind on the v0.10+ list. Today: keep your script, or wrap it in `command:`. |
| Most `git log` / `git blame` analysis (rebase-graph checks, contributor-stat gates) | alint has narrow git hygiene support (`git_no_denied_paths`, `git_commit_message`, `git_blame_age`); broader history-aware checks are out of scope. |
| Vendor-tree readonly enforcement (kubernetes' `verify-readonly-packages.sh`) | Requires hashing each vendored entry against a pinned manifest. `pair_hash` is on the v0.10+ list; for now `file_hash` covers single files only. |

---

## Side-by-side: a real script

### Example 1 — every package has a README

The `for d in packages/*/; do …; done` shape is one of the most common
in OSS monorepos. Concretely, this is the bash that ships in roughly
every JS workspace's pre-commit:

```bash
#!/usr/bin/env bash
set -euo pipefail
fail=0
for d in packages/*/; do
  if [ ! -f "$d/README.md" ]; then
    echo "Missing: $d/README.md" >&2
    fail=1
  fi
done
exit $fail
```

The alint equivalent — five lines of YAML — supports the same
invariant, but also handles ignore filtering, parallel evaluation,
JSON / SARIF / GitHub-annotation output formats, and `--changed`-mode
incremental runs:

```yaml
- id: every-package-has-readme
  kind: for_each_dir
  select: "packages/*"
  require:
    - kind: file_exists
      paths: "{path}/README.md"
  level: error
```

If your monorepo lives under `packages/` AND `apps/`, the
`monorepo@v1` bundled ruleset already covers this with one `extends:`
line:

```yaml
extends:
  - alint://bundled/monorepo@v1
```

### Example 2 — every Cargo.toml declares a license

Slightly more complex. The bash version typically looks like:

```bash
#!/usr/bin/env bash
set -euo pipefail
fail=0
for f in $(find crates -name Cargo.toml); do
  if ! grep -qE '^license\s*=\s*"' "$f"; then
    echo "Missing license field: $f" >&2
    fail=1
  fi
done
exit $fail
```

This script has two subtle bugs: it doesn't catch `license-file =` (the
alternate form), and it doesn't catch a TOML-comment-shaped license
line that grep matches but TOML parsers reject. The alint version is
TOML-aware:

```yaml
- id: every-crate-declares-license
  kind: for_each_dir
  select: "crates/*"
  when_iter: 'iter.has_file("Cargo.toml")'
  require:
    - kind: toml_path_matches
      paths: "{path}/Cargo.toml"
      path: "$.package.license"
      matches: '^[A-Za-z0-9.+\-]+( OR [A-Za-z0-9.+\-]+)*$'
  level: error
```

`when_iter:` filters out non-package directories under `crates/` (a
local `notes/` or `scratch/`) without any extra grep. The TOML path
query handles the parsing; the regex pins the license to a SPDX-shaped
value. **The replacement isn't shorter — it's stricter.**

### Example 3 — composability for the long tail

For checks that don't map (an in-house tool, an AST-aware Python
script, a runtime probe), `command:` keeps them inside the same gate:

```yaml
- id: shellcheck-all-scripts
  kind: command
  paths: "**/*.sh"
  command: ["shellcheck", "-x", "{path}"]
  timeout: 30
  level: error

- id: keep-our-ast-checker
  kind: command
  paths: "src/**/*.py"
  command: ["scripts/check-templated-fields.py", "{path}"]
  level: warning
```

Now `alint check` runs both the declarative rules and the surviving
scripts; CI calls `alint check` once instead of running `bash
hack/verify-all.sh` and getting one accumulated exit code.

---

## What you gain by consolidating

- **One config to read.** A new contributor reads `.alint.yml` instead
  of grepping through every script in `hack/`. The structural contract
  becomes a single artifact, not a directory.
- **Faster CI.** alint runs declarative rules in parallel; the typical
  shell pipeline runs scripts sequentially, each with its own
  filesystem walk. On a 100k-file repo (the kubernetes shape), the
  alint-replaceable subset benchmarks at ~1-2 seconds; the equivalent
  shell pipeline measures in tens of seconds dominated by N redundant
  walks.
- **Output formats CI can read.** `human`, `json`, `sarif`, `github`,
  `markdown`, `junit`, `gitlab`, `agent` — eight formats, no `echo`
  glue. SARIF lights up GitHub's Code-Scanning panel; JUnit shows up in
  the test summary; `agent` is structured for AI-agent-driven workflows.
- **Auto-fix where it's safe.** 12 mechanically-safe ops (trim
  whitespace, normalize line endings, strip BOM/bidi/zero-width,
  prepend / append, rename, remove). For the patterns where bash usually
  has a `--fix` companion script, alint runs the fix in-process.
- **A declarative DSL anyone can edit.** Adding a rule is a 5-line YAML
  block. Editing one is changing a regex. New contributors don't need
  bash fluency to ratchet your structural contract forward.
- **Composition.** `extends:` pulls bundled rulesets, local files, or
  HTTPS+SRI URLs. You can publish your project's structural contract as
  a ruleset and let downstream forks inherit it.

---

## What you keep as scripts

alint's non-goals are deliberate. These are the categories it doesn't
try to handle, and the migration path is to keep them where they are:

- **AST-aware checks.** "Every `BaseOperator` subclass declares
  `templated_fields` containing only valid attribute names." Python /
  Go / Rust AST analysis. Use the language's own tooling
  (pylint / golangci-lint / clippy) or keep your custom AST script.
- **Cross-API-version diffs.** Kubernetes' `verify-types-aliases.sh`
  compares Go types across API versions. Out of scope.
- **Runtime probes.** Network reachability, binary version checks,
  process-state assertions, CGO-link probes. alint is a static checker.
- **Codegen / generator-freshness checks.** "Run the generator and diff
  the output." On the v0.10+ candidate list as `generated_file_fresh`;
  not shipping in v0.9. Keep your `make regen-cases-check`-style
  scripts.
- **Most git-history-aware checks.** alint has narrow git support
  (`git_no_denied_paths`, `git_commit_message`, `git_blame_age`,
  `git_tracked_only` modifiers). Rebase-graph analysis,
  contributor-stat gates, blame-history aggregation: out of scope.
- **Build-system-aware checks.** "vendor/ packages must not be
  modified after import" needs `pair_hash` (v0.10+); Go module-graph
  analysis (`vendor.sh`, `no-vendor-cycles.sh`) is out of scope; CGO
  link-time checks are out of scope.
- **Domain-specific semantic checks.** alint can enforce that a
  Prometheus metric *exists* in source via regex; it can't enforce that
  the metric's *name* matches your project's naming convention if that
  requires AST-level semantic reasoning.
- **Codegen drift / SBOM regen / OpenAPI freshness.** All
  generator-output-vs-source checks. Keep your existing tooling.

If you find yourself wanting any of these, file an issue against the
v0.10+ candidate list — many of them are already there with multiple
case-study confirmations.

---

## Composability: alint orchestrates, scripts handle the long tail

The single most useful pattern when migrating is the `command:` rule
kind. alint shells out to a child process per matched file; non-zero
exit becomes one violation whose message is the (truncated) stdout +
stderr. So the survivors don't leave the gate — they ride along inside
`alint check`.

```yaml
- id: keep-our-cross-api-checker
  kind: command
  paths: "pkg/api/**/*.go"
  command: ["./hack/verify-types-aliases.sh"]
  timeout: 60
  level: error
```

A few notes:

- **Trust gate.** `command:` rules are only allowed in the user's own
  top-level config. A `kind: command` rule introduced via `extends:`
  (local file, HTTPS URL, or `alint://bundled/`) is rejected at load
  time. Adopting a published ruleset never grants arbitrary process
  execution.
- **`--changed` interaction.** `command` is a per-file rule, so under
  `alint check --changed` it spawns only for files in the diff — your
  expensive shell-out is automatically incremental in CI.
- **Environment threaded into the child.** `ALINT_PATH`, `ALINT_ROOT`,
  `ALINT_RULE_ID`, `ALINT_LEVEL`, plus any top-level `vars:` you
  define. Existing scripts usually need no changes — most read `$1` or
  `$ALINT_PATH` directly.

The migration path is incremental: replace what's declarative, keep
what isn't, add `command:` rules to invoke the survivors, delete the
master `verify-all.sh` runner.

---

## Step-by-step migration

A real-world plan, scoped to roughly one afternoon for a 30-script
directory.

### 1. Inventory and categorise

For each script in `hack/verify-*.sh` (or wherever yours live), ask:

- Is this **structural**? (file existence, content patterns, manifest
  fields, filename grammar, line endings.) → maps to alint
- Is this **AST-aware**? → keep, optionally wrap in `command:`
- Is this **runtime**? (binary version, network probe, image build) →
  keep, optionally wrap in `command:`
- Is this a **generator** (`update-*`, `regen-*`, `generate-*`)? → not
  alint's job; keep
- Is this a **cross-API-version diff** or **module-graph analysis**? →
  out of scope; keep

The kubernetes case study tags each of its 50 scripts with one of
these categories — copy that template.

### 2. Write `.alint.yml` with bundled rulesets first

Start with the rulesets that already encode 80 % of what most projects
want:

```yaml
version: 1

extends:
  - alint://bundled/oss-baseline@v1
  - alint://bundled/<your-language>@v1   # rust, node, python, go, java
  - alint://bundled/ci/github-actions@v1
  - alint://bundled/hygiene/no-tracked-artifacts@v1

rules:
  # your project-specific rules go here
```

Run `alint list --config .alint.yml` to see the rule expansion. Many
of the scripts you'd planned to translate will already be covered.

### 3. Translate the structural subset

Walk your inventory's "structural" pile, one script at a time, and
add a rule per script. Keep IDs human (`team-go-license-header`,
`team-no-todo-in-prod`); they show up in violation messages and CI
logs. Reuse the [pattern catalogue](#pattern-catalogue) above.

### 4. Reconcile with current `main`

Run `alint check` against the current tip. You will surface either:

- **A handful of true positives.** Your script's tolerances were
  slightly off (or it had a subtle bug); alint catches what it
  silently let through. Fix the violations or relax the rule.
- **A flood of false positives.** Your `paths:` scope is too broad.
  Narrow it (`scope_filter:`, `paths.exclude`, `when:`).
- **Existing live failures.** Some bash scripts are silently broken —
  the script ran, exited 0, but its grep was hitting nothing. Decide
  whether the rule was wrong or the codebase was wrong; the alint
  reconciliation pass usually surfaces both.

### 5. Wrap surviving scripts in `command:` rules

For the AST / runtime / cross-API-version / generator scripts you
deliberately kept, add one `command:` rule each:

```yaml
- id: keep-our-templated-fields-check
  kind: command
  paths: "src/**/*.py"
  command: ["scripts/check_templated_fields.py", "{path}"]
  timeout: 30
  level: error
```

Now both the declarative and survivor checks run under `alint check`.

### 6. Update CI

Replace your master `bash hack/verify-all.sh` invocation with `alint
check`. Drop the per-script CI steps; one binary, one exit code, one
output file.

### 7. (Optional) retire the surviving scripts organically

Leave them in the repo. Surviving scripts that become candidates for
new alint primitives (`registry_paths_resolve`, `import_gate`,
`generated_file_fresh`, `cross_file_value_equals` — all on the v0.10+
list) will retire themselves over the next few releases.

---

## See also

- [kubernetes case study](/examples/kubernetes-kubernetes/) — the
  headline 50-to-17 story, with the full inventory.
- [cpython case study](/examples/python-cpython/) — 56 surfaces
  across Make targets / pre-commit / `Tools/build/*` / `.gitattributes`.
- [airflow case study](/examples/apache-airflow/) — 109 pre-commit
  hooks, ~40 % map.
- [Rule catalogue](/docs/rules/) — every alint rule kind, with
  examples.
- [Bundled rulesets](/docs/bundled-rulesets/) — what `extends:` ships
  out of the box.
- [How alint compares to other tools](/compare/) — including
  Repolinter, ls-lint, Megalinter, EditorConfig.
```

## Implementation notes (for the site repo)

- New page at `/migrating-from/custom-bash-scripts/`. Sibling to the
  Repolinter and ls-lint migration pages; they share the
  `/migrating-from/` route established by the compare-page draft.
- The compare page (`alint-org-compare.md`) already links here as
  *"From custom bash scripts →"* — both should ship coordinated.
- The headline kubernetes link (`/examples/kubernetes-kubernetes/`)
  resolves only after the examples-gallery draft lands. If the gallery
  ships first, the link works as written; otherwise a fallback to
  `https://github.com/asamarts/alint/tree/main/examples/kubernetes-kubernetes/`
  is acceptable for a soft launch.
- Link to `/docs/rules/` (rule catalogue) and `/docs/bundled-rulesets/`
  — both already live on alint.org.
- The pattern catalogue table is wide enough that mobile rendering
  will need a horizontal-scroll wrapper (same handling as the compare
  page's feature matrix). Starlight default is fine; just verify.
- The "Step-by-step migration" numbered list is intentionally
  pragmatic — not a sales funnel. Don't shorten step 4 ("Reconcile
  with current main"); the "you will surface live failures" pattern is
  one of the most useful selling points and we should not soften it.

## Open questions before publish

1. **Where does the kubernetes link resolve?** The hard dependency for
   the headline-finding callout is `/examples/kubernetes-kubernetes/`
   on alint.org. Status of the examples-gallery draft determines
   whether this page can publish standalone or needs to wait for the
   gallery. Recommendation: ship together with the gallery + compare
   page as the P3.1 wave, since all three reference each other.
2. **Should we surface the v0.10+ candidate list?** The "What you
   keep as scripts" + "Patterns with no clean alint mapping" sections
   name-drop `import_gate`, `pair_hash`, `generated_file_fresh`,
   `cross_file_value_equals`, `registry_paths_resolve` — all on the
   v0.10+ candidate list per `docs/launch-prep.md`. Worth confirming
   we want these in public-facing copy as future commitments. The
   compare page draft already does this implicitly.
3. **Tone calibration on "your bash scripts have bugs."** Step 4 of
   the migration ("Reconcile with current main") gently surfaces that
   bash scripts often have subtle bugs (the grep-vs-TOML-parse
   example earlier; the silent-no-match-exits-0 pattern). This is
   honest and high-utility but might read as condescending depending
   on tone. Worth a second-eyeball pass.
4. **Do we want a benchmark number in the perf section?** "On a 100k
   file repo, the alint-replaceable subset benchmarks at ~1-2
   seconds." This number is from the v0.9.13 published S3 bench
   (`docs/benchmarks/HISTORY.md`); confirm it stands at v0.9.15
   publish time. The compare page uses the same number.
5. **`pre-commit` framing.** This page treats pre-commit hooks as
   "bash-shaped from the migrator's perspective" via the airflow
   example (109 hooks, ~40 % map). Worth deciding whether the page
   should also cover pre-commit explicitly with its own subsection,
   or whether airflow-as-data-point is enough. Recommend: keep it
   implicit for v1; consider a dedicated `migrate-from-pre-commit.md`
   for v2 if HN/Reddit traffic asks.

## Pre-publish checklist

- [ ] `/examples/kubernetes-kubernetes/` resolves on alint.org (or
      the link falls back to the GitHub URL).
- [ ] `/examples/python-cpython/` resolves on alint.org.
- [ ] `/examples/apache-airflow/` resolves on alint.org.
- [ ] `/docs/rules/` link still resolves (already live; spot-check).
- [ ] `/docs/bundled-rulesets/` link still resolves (already live).
- [ ] `/compare/` link points to the published compare page (the
      compare page also links *here* — both must publish coordinated
      to avoid bidirectional 404s).
- [ ] Every named rule kind in the pattern catalogue verified against
      `docs/rules.md` at publish time (the catalogue is locked-in for
      v0.9.6+; v0.10+ rule additions don't invalidate this page).
- [ ] Every named bundled ruleset (`oss-baseline@v1`, `monorepo@v1`,
      `agent-hygiene@v1`, `hygiene/no-tracked-artifacts@v1`,
      `ci/github-actions@v1`) verified against
      `docs/bundled-rulesets/` at publish time.
- [ ] Pattern-catalogue table renders with horizontal scroll on
      mobile (same fix as compare-page matrix).
- [ ] STATE.md row for `migrate-from-custom-bash.md` flipped from
      `planned` to `live` with date + commit SHA.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `alint-org-compare.md` | The compare page links here as *"From custom bash scripts →"*. Both should publish in the same wave so no link 404s in either direction. |
| `alint-org-examples-gallery.md` | This page links into `/examples/kubernetes-kubernetes/`, `/examples/python-cpython/`, `/examples/apache-airflow/` heavily. Gallery is the source of those routes. |
| `migrate-from-repolinter.md` (planned) | Sibling page; shares the `/migrating-from/` route. Tone calibration should match across the three migration pages. |
| `migrate-from-ls-lint.md` (planned) | Sibling page; same structure, much narrower scope (ls-lint is one tool, this page is a pattern). |
| `launch-post-dev-to.md` (P4) | The dev.to post is *"How we replaced 50 verify scripts with one .alint.yml"* — same kubernetes data point, longer-form narrative. Both pages should share the same numbers, link bidirectionally. |

## Estimated diff size on the site repo

- 1 new page at `/migrating-from/custom-bash-scripts/`: ~310 lines of
  markdown.
- (optional) horizontal-scroll wrapper CSS for the wide pattern
  catalogue: shared with compare-page work, ~0 incremental.
- (optional) nav config: this page is reachable through the compare
  page's link table, no top-level nav addition required.

Total: ~310 lines on the site repo. No infrastructure change beyond
adding the page file.
