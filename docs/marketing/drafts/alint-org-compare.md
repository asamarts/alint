---
destination: alint.org/compare/ (new top-level route on the site repo)
status: drafting
blocks_on: alint-org-hero.md publishes (the Repolinter blockquote in the hero links here); examples gallery for the case-study links to resolve
last_touched: 2026-05-06
---

# alint.org/compare/ — content brief for the site repo

## Why

Two of the three heroes (`README.md` + `alint-org-hero.md`) lead with
Repolinter-replacement framing. That positioning needs somewhere to
land — a comparison page that:

1. Honours the framing without sounding like marketing pap. Each
   comparator gets a fair "when X is right" callout; alint is presented
   as one tool in an ecosystem, not a universal answer.
2. Carries SEO weight on three of launch-prep's named target keywords:
   *"repolinter alternative"*, *"monorepo linter"*, *"language-agnostic
   linter"*.
3. Backs every claim with evidence the reader can verify — case studies
   from `/examples/`, public benchmarks from `/docs/benchmarks/`, the
   per-tool repos themselves.

## Proposed page

```markdown
---
title: alint vs other repo-level linters
description: Honest comparison — when alint is the right tool, when Repolinter / ls-lint / Megalinter / EditorConfig / custom shell scripts are.
---

# alint vs other repo-level linters

## TL;DR

alint is the *active-maintenance, language-agnostic, fast, structurally-aware*
slot in the repo-linting ecosystem. It is **not** trying to replace
ESLint, Clippy, ruff, or your favourite per-language linter — those
operate on code; alint operates on filesystem shape and file content.

If you're shopping because…

| If you're shopping because… | Look at… |
|---|---|
| **Repolinter was archived in early 2026 and you need a replacement** | alint (this page explains why) |
| **You want to enforce filename + directory conventions only** | [ls-lint](https://ls-lint.org/) — narrower scope, even smaller binary |
| **You want one tool that orchestrates 70+ language-specific linters in containers** | [Megalinter](https://megalinter.io/) — different shape entirely (orchestrator, not linter) |
| **You want indent / line-endings / charset conventions enforced inside the editor** | [EditorConfig](https://editorconfig.org/) — alint *consumes* `.editorconfig` via its `tooling/editorconfig` bundled ruleset; both can coexist |
| **You have a `hack/verify-*.sh` directory and want to keep it** | Stay where you are — but read [the kubernetes case study](/examples/kubernetes-kubernetes/) to see what 17 of those 50 scripts could become declaratively |

The rest of this page goes into the why.

---

## Feature matrix

|  | **alint** | **Repolinter** | **ls-lint** | **Megalinter** | **EditorConfig** | **Custom shell** |
|---|---|---|---|---|---|---|
| **Maintenance status** | active (v0.9.x; last release 2026-05) | **archived 2026-02** | active | active | universal standard | as much as you maintain |
| **Scope** | filesystem shape, file content, cross-file relationships | OSS-baseline files (LICENSE, README, etc.) | filenames + directory layout only | orchestrates ~70 native linters | text-format conventions | whatever you write |
| **Languages** | language-agnostic | language-agnostic | language-agnostic | one container per language | text-format only | whatever you write |
| **Install footprint** | one static Rust binary (~10 MB) | Node.js + npm install | one Go binary (~5 MB) | Docker (heavy) | editor-native (zero install) | bash + greps |
| **Rule count** | 60 rule kinds | ~30 rules | ~5 rule kinds (filename/dir conventions) | (N/A — orchestrator) | 7 properties | unbounded |
| **Bundled per-ecosystem rulesets** | 19 (rust, node, python, go, java, +monorepo +CI +hygiene +agent +compliance) | none | none | per-language container sets | none | none |
| **Cross-file rules** | yes (`pair`, `for_each_dir`, `for_each_file`, `dir_contains`, `unique_by`, `every_matching_has`) | no | no | (per-tool) | no | hand-rolled |
| **Structured-query rules over JSON/YAML/TOML** | yes (RFC 9535 JSONPath) | partial (jsonpath-plus) | no | (per-tool) | no | hand-rolled |
| **Conditional `when:` gates** | yes (bounded expression language) | no | no | (per-tool config) | no | hand-rolled |
| **Composition (`extends:` / nested configs)** | yes (local + HTTPS+SRI + bundled URL scheme) | partial (axiom inheritance) | no | (per-tool config) | no | source/include |
| **Auto-fix** | 12 file ops (whitespace, line endings, BOM, bidi, prepend/append, rename, …) | partial (file presence) | no | (per-tool) | (editor-side) | hand-rolled |
| **Output formats** | 8 (human, json, sarif, github, markdown, junit, gitlab, **agent**) | json, markdown | text | (varies per tool) | (N/A) | however you `echo` |
| **Agent-aware output** (per-violation `agent_instruction`) | **yes** | no | no | no | (N/A) | hand-rolled |
| **Performance** | sub-second on 100K files; 12 s on 1M files (public benches per release) | not benchmarked | fast (Go + minimal scope) | slow (container per linter) | editor-instant | varies |
| **Public benchmark history** | [`/docs/benchmarks/`](/docs/benchmarks/) — per-release | none | none | none | (N/A) | none |
| **Production case studies** | [20 OSS repos](/examples/) (kubernetes, rust, go, cpython, node, arrow, pytorch, react, …) | (community list, not maintained since archive) | (homepage examples) | (homepage examples) | universal | (yours) |

---

## When alint is the right tool

alint earns its keep when at least one of these is true:

- **Your repo has accumulated `verify-*.sh` / `check-*.py` /
  `tools/lint-*.js` scripts** that mostly do *structural* checks
  (filename grammars, content patterns, manifest field shape, registry
  consistency) rather than *semantic* checks (AST analysis, type
  checking). [kubernetes consolidates 17 of 50 verify scripts](/examples/kubernetes-kubernetes/);
  [airflow ~40 % of 109 pre-commit hooks](/examples/apache-airflow/);
  [cpython 12 validation surfaces into one config](/examples/python-cpython/).
- **Your repo relies on conventions that nothing in CI actually
  checks.** [tokio has zero hand-rolled scripts but assumes 15
  conventions alint catches](/examples/tokio-rs-tokio/);
  [uv's 67-crate workspace conventions are enforced
  nowhere](/examples/astral-sh-uv/).
- **Your repo is polyglot** and no per-language linter sees the
  cross-language conventions. [arrow has 6 languages, 21 lint hooks
  across 14 tool repos, and 0 tools that see cross-language
  shape](/examples/apache-arrow/).
- **You want structured output for AI agents** that suggests both the
  fix *and* the rule context. The `agent` output format ships per-
  violation `agent_instruction` strings; the `agent-hygiene` and
  `agent-context` rulesets specifically target AI-touched repos.
- **You want one fast static binary** instead of a Node/JVM/Python/Docker
  runtime in your CI pipeline.

## When alint is NOT the right tool

- **You need AST-aware linting.** Use ESLint, Clippy, ruff, etc. alint
  deliberately operates on bytes/structure, not parsed code.
- **You need SAST (security-focused code analysis).** Use Semgrep,
  CodeQL.
- **You need IaC scanning.** Use Checkov, Conftest, tfsec.
- **You need secret scanning.** Use gitleaks, trufflehog.
- **Your only need is filename conventions.** ls-lint is smaller and
  more focused.
- **Your only need is text-format conventions** (indent, line endings,
  charset). EditorConfig lives in the editor, no CI step needed —
  though alint's `tooling/editorconfig` ruleset can enforce
  compliance at PR time as a backstop.

---

## Per-tool deep dives

### vs Repolinter

[Repolinter](https://github.com/todogroup/repolinter) was the TODO
Group's tool for OSS-baseline checks (LICENSE exists, README has the
right sections, CONTRIBUTING.md present, etc.). **It was archived in
February 2026.** Users actively shopping for a replacement tend to land
here.

**alint covers Repolinter's rule catalogue as a strict superset.** The
bundled `oss-baseline@v1` ruleset (15 rules) maps Repolinter's
file-presence + content-shape axioms; the rest of alint's catalogue
(60 rule kinds total, 19 bundled rulesets) is net-additional.

What you gain by switching:

- **Active maintenance.** alint shipped 14 releases in the last 6
  months; Repolinter's last release was 2024.
- **Cross-file rules.** Repolinter checks one file at a time. alint's
  `pair`, `for_each_dir`, `for_each_file`, `dir_contains`, `unique_by`,
  `every_matching_has` cover invariants Repolinter can't express.
- **Structured-query rules.** Validate fields *inside* JSON/YAML/TOML
  with full RFC 9535 JSONPath. (Repolinter has partial jsonpath-plus
  support.)
- **Bundled per-ecosystem rulesets.** Out-of-the-box rust / node /
  python / go / java / monorepo / CI / hygiene rulesets — Repolinter
  ships only OSS-baseline.
- **Auto-fix.** 12 mechanically-safe ops (trim whitespace, normalize
  line endings, strip BOM/bidi, prepend/append, rename). Repolinter has
  partial fix support for file presence.
- **Performance.** Sub-second on 100K-file workspaces vs Repolinter's
  Node startup + per-rule JS execution.
- **Agent-aware output.** First-class `agent` output format with
  per-violation `agent_instruction` strings.

What you lose: nothing material. The migration path is straightforward —
see [the Repolinter migration guide](/migrating-from/repolinter/).

### vs ls-lint

[ls-lint](https://ls-lint.org/) is a Go binary that enforces filesystem
naming conventions — file and directory name patterns, depth limits,
etc. It's narrower than alint and faster *at its specific job*.

**Use ls-lint if** filename conventions are *the only thing* you care
about. The config is one YAML file, the tool is one binary, and it
ships with a tight scope that's easy to reason about.

**Use alint if** filename conventions are *one of many things* you
care about. alint's `filename_case` and `filename_regex` rules cover
the same ground, plus content checks, structured queries, cross-file
rules, etc. There's no need to run both.

If you're already on ls-lint and want to add structural checks without
giving up your existing config, alint's `command` rule kind can shell
out to `ls-lint` per file or per dir — composability over replacement.

### vs Megalinter

[Megalinter](https://megalinter.io/) is a Docker-based **orchestrator**
that wraps ~70 native linters in containers and runs them against your
repo. It's a different shape entirely from alint.

**Use Megalinter if** you want one CI step that runs eslint + prettier
+ pylint + golangci-lint + shellcheck + …(70 more)… in one
docker-compose-y orchestration, and you're OK with the Docker-runtime
weight.

**Use alint if** you want structural checks specifically, with one
static binary, no Docker. The two coexist cleanly: Megalinter
orchestrates the language-specific lint stack; alint runs as one
additional check inside Megalinter for structural coverage. We've seen
this pattern in [pytorch](/examples/pytorch-pytorch/) — they built
`lintrunner` for the same orchestration role and ≈86 % of its 57
adapters are structural; alint sits beneath as the structural floor.

### vs EditorConfig

[EditorConfig](https://editorconfig.org/) is the universal standard for
text-format conventions (indent style, indent size, line endings,
charset, trim trailing whitespace, insert final newline). It lives in
your editor. There's no CLI.

alint **consumes** `.editorconfig` via the bundled `tooling/editorconfig`
ruleset, which enforces the same conventions at PR time as a backstop
for editors that don't natively support EditorConfig (or contributors
who never installed the plugin).

**Use both.** EditorConfig keeps the developer's editor honest in real
time; alint keeps CI honest at merge time. There's no overlap to
resolve.

### vs custom shell scripts

If your repo has a `hack/verify-*.sh` (or `scripts/check-*.py`,
`tools/lint-*.js`, …) directory, you've built the right thing for what
existed when you started. The maintenance burden tends to grow over
time, though, and the patterns are usually structural.

**alint replaces the structural subset.** [kubernetes converted 17 of
50 verify scripts](/examples/kubernetes-kubernetes/) into declarative
rules; the remaining 33 stay as scripts because they do AST analysis,
runtime probes, or cross-API-version diffs that alint deliberately
doesn't cover. [cpython consolidated 12 of 56 validation
surfaces](/examples/python-cpython/); [pytorch's
`lintrunner.toml`](/examples/pytorch-pytorch/) shows the same pattern at
the orchestration layer.

The realistic outcome isn't 100 % replacement — it's *consolidation of
the declarative subset*, which usually means smaller `hack/` directory,
faster CI, and one config that new contributors can read instead of
spelunking through bash.

---

## Use them together — patterns

| Stack pattern | When | Example case study |
|---|---|---|
| alint + ESLint + prettier + dprint + knip | TypeScript / JS monorepos with mature lint discipline | [microsoft/typescript](/examples/microsoft-typescript/) |
| alint + golangci-lint + custom Make targets | Go monorepos | [helm/helm](/examples/helm-helm/), [kubernetes](/examples/kubernetes-kubernetes/) |
| alint + ruff + cargo-clippy + cargo-deny + pre-commit | mixed Rust / Python tooling | [astral-sh/ruff](/examples/astral-sh-ruff/), [astral-sh/uv](/examples/astral-sh-uv/) |
| alint + lintrunner + clang-format + clang-tidy + custom AST checks | C++/Python ML mega-repos | [pytorch/pytorch](/examples/pytorch-pytorch/) |
| alint + Apache RAT + Sphinx + per-language native linters | Apache polyglot projects | [apache/arrow](/examples/apache-arrow/), [apache/airflow](/examples/apache-airflow/) |

---

## Migrating

Step-by-step migration guides:

- [From Repolinter →](/migrating-from/repolinter/)
- [From ls-lint →](/migrating-from/ls-lint/)
- [From custom bash scripts →](/migrating-from/custom-bash-scripts/)

Each one shows the source-tool config side-by-side with the equivalent
`.alint.yml`, plus notes on rules that don't have a 1:1 mapping.
```

## Implementation notes (for the site repo)

- New top-level route — `src/pages/compare.astro` or
  `src/content/docs/compare.md`, depending on Starlight conventions.
- Add to top-level nav (sibling to "Docs", "Examples", "Cookbook").
- The big feature matrix may need horizontal-scroll handling on mobile
  Starlight's default table styling is tight; a wrapper div with
  `overflow-x: auto` is usually enough.
- Per-tool repo links should open in a new tab (`target="_blank"`)
  via Starlight's link-handling config.

## Open questions before publish

1. **Tone calibration.** The current draft tries hard to be fair to
   each comparator (each gets a "when X is right" callout). Worth a
   second-eyeball pass — does it read as honest or as
   competitor-trashing? Specific worry: the Repolinter section leans
   heaviest, justified by the archived-status framing, but could be
   softened if needed.
2. **Megalinter detail.** The current draft frames Megalinter as
   "different shape entirely (orchestrator)". If we want to be more
   substantive about it (e.g., a 1-2 paragraph deep dive on when
   container-per-linter weight is worth it vs. one-binary speed), we
   could expand. Default is to keep it short.
3. **Comparator coverage.** Should we add `pre-commit` to the matrix?
   pre-commit is a hook orchestrator, not a linter — but it's the
   canonical "I have a bash + python + js stack and want one entry
   point" answer, and the comparison would be useful. Recommend YES
   for v2 of the page; OUT of MVP to ship faster.
4. **"Why a separate tool" section.** alint.org currently has this
   section. After the comparison page ships, the "Why a separate tool"
   content should probably *redirect* / *short-form summarise* and
   point here. Recommend: keep "Why a separate tool" as a short
   ~100-word block on the landing, linking to `/compare/` for detail.

## Pre-publish checklist

- [ ] `/migrating-from/repolinter/` exists (otherwise the migration
      callout 404s) — that's `migrate-from-repolinter.md` draft.
- [ ] `/migrating-from/ls-lint/` exists.
- [ ] `/migrating-from/custom-bash-scripts/` exists.
- [ ] `/examples/kubernetes-kubernetes/`, `/examples/apache-airflow/`,
      `/examples/python-cpython/`, `/examples/tokio-rs-tokio/`,
      `/examples/astral-sh-uv/`, `/examples/apache-arrow/`,
      `/examples/pytorch-pytorch/`, `/examples/microsoft-typescript/`,
      `/examples/helm-helm/`, `/examples/astral-sh-ruff/` all resolve
      (most-cited from this page).
- [ ] `/docs/benchmarks/` exists with v0.9.16 numbers (currently public
      data is at `docs/benchmarks/HISTORY.md` in the alint repo —
      needs site-side rendering).
- [ ] Verify the per-comparator URLs still resolve at publish time:
      `https://github.com/todogroup/repolinter`, `https://ls-lint.org/`,
      `https://megalinter.io/`, `https://editorconfig.org/`.
- [ ] Top-level nav updated to surface `/compare/`.
- [ ] STATE.md row for `alint-org-compare.md` flipped to `live` with
      date + commit SHA.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `alint-org-hero.md` | The Repolinter blockquote in the hero points users *somewhere* — that "somewhere" is this compare page. Both should ship coordinated. |
| `alint-org-examples-gallery.md` | This page links into `/examples/` heavily. Gallery needs to be ready (or fall back to GitHub links). |
| `alint-org-benchmarks.md` (later) | Performance row in the matrix cites `/docs/benchmarks/`. If that route doesn't exist, link to the GitHub-rendered `docs/benchmarks/HISTORY.md` as a fallback. |
| `migrate-from-{repolinter,ls-lint,custom-bash}.md` (later) | The "Migrating" section links into all three. Could ship this page with stub migration pages OR with the migration links suppressed until the guides land. |

## Estimated diff size on the site repo

- 1 new page at `/compare/`: ~250 lines of markdown
- Top-level nav config: ~5 lines
- (optional) mobile-table-scroll CSS: ~10 lines

Total: ~265 lines on the site repo.
