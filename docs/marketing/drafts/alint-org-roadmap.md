---
destination: alint.org/roadmap/ (new top-level route on the site repo)
status: drafting
blocks_on: alint-org-hero.md publishes (hero's "what's next" CTA points here); v0.10 design doc set is the source-of-truth for the LSP section so it's stable enough to summarise
last_touched: 2026-05-06
---

# alint.org/roadmap/ — content brief for the site repo

## Why

A public roadmap is a **trust signal**. Three audiences read it for
three reasons:

1. **Prospective adopters** — "is this tool actively going somewhere or
   am I picking up an abandoned project?" Repolinter was archived in
   2026-02; alint inherits that scrutiny by virtue of replacing it. A
   visible "next major version is X, the one after is Y" answers the
   question without forcing a click into commit history.
2. **Existing adopters** — "should I plan to upgrade or hold?" Knowing
   v0.10 ships LSP and v0.11 ships WASM plugins lets users sequence
   their adoption (add to CI now; add editor integration when v0.10
   ships; build custom plugin when v0.11 ships).
3. **Contributors / maintainers of related tooling** — "where should I
   point my PRs / what's on the table?" The won't-do list is as
   load-bearing as the will-do list — every "use X instead" line
   redirects a malformed contribution before it's written.

This page is **explicitly separate** from the internal
`docs/design/ROADMAP.md` (~46 KB; per-version exhaustive engineering
history with "shipped 2026-04-22, commit `261dda5`" granularity).
That doc is for contributors; this page is for everyone else.

**Convention to honor:** the internal ROADMAP states *"This roadmap is
scope-based; dates are deliberately omitted. Each version is a closed
cut — work that doesn't fit moves to a later version."* The public
roadmap MUST follow the same convention. Hard dates create blowback
when they slip; scope buckets create buy-in.

## Proposed page

```markdown
---
title: alint roadmap
description: What's next for alint — v0.10 (LSP server), v0.11 (WASM plugins), ongoing work, and explicit non-goals.
---

# alint roadmap

> Scope-based; dates are deliberately omitted. Each version is a closed
> cut — work that doesn't fit moves to a later version. (Same
> convention as the [internal engineering
> roadmap](https://github.com/asamarts/alint/blob/main/docs/design/ROADMAP.md).)

## Where alint stands today

**Latest release: v0.9.16** (2026-05-06). 60 rule kinds across 13
families, 19 bundled ecosystem rulesets, 12 auto-fix ops, 8 output
formats including agent-aware. **19 config-authoring pitfalls
catalogued and prevented across the toolchain** (schema at edit time,
parse error at load time, runtime audit at PR time, smoke-test fixture
at commit time). **25 production OSS case studies** under
[`examples/`](https://github.com/asamarts/alint/tree/main/examples) —
20 single-language (P2a) + 5 polyglot monorepos (P2b Wave 1: NixOS/
nixpkgs, bazel, TensorFlow, apache/spark, vscode). **Sub-second on
100K-file repos** (~1.1 s on a 100K-file workspace bundle, ~12 s at
1M files; nixpkgs at 39,101 files runs the full 79-rule pass in 273
ms wall-clock).

[Full release history →](https://github.com/asamarts/alint/blob/main/CHANGELOG.md)
[Full benchmarks history →](/benchmarks/)

---

## v0.10 — LSP server (next)

The first user-visible IDE / agent integration. v0.9 was engine-internal
performance work; v0.10 turns the per-file dispatch hot path (built out
in v0.9.3-v0.9.6) into a single-file re-evaluation contract that an
LSP server can drive cheaply.

**Headline features:**

- **Inline diagnostics** — `alint lsp` speaks LSP 3.17 over stdio.
  `textDocument/didChange` triggers debounced per-file evaluation;
  `textDocument/didSave` triggers cross-file re-check. The same
  violation list as `alint check`, surfaced inline as red squigglies.
- **Hover with rule docs** — `textDocument/hover` over a violation
  marker shows the rule's `policy_url`, `message`, and a one-line
  summary. Same content the docs site renders, surfaced where the
  developer is already looking.
- **Code actions** — `textDocument/codeAction` emits "Apply fix" for
  rules with a `Fixer` (12 fix ops today) plus "Add rule to ignore"
  for any violation. Editor handles the buffer rewrite via the
  standard `WorkspaceEdit` flow.
- **VS Code extension** — thin extension that bundles the `alint lsp`
  binary and registers the LSP client. Coming to the marketplace
  alongside the v0.10 release.

**Plus four new rule kinds** validated by the P2a/P2b case-study sweep
(8+ source repos converge on each):

- **`registry_paths_resolve`** — every path/key in a registry file
  resolves to an on-disk artefact. Demand from rust-lang/rust
  (tidy::triagebot), cpython, nodejs/node (deps tracking), arrow
  (`rat_exclude_files.txt`), nixpkgs (3 registries), TensorFlow,
  apache/spark (8+ source repos).
- **`cross_file_value_equals`** — value at JSONPath X in file A
  matches value at JSONPath Y in file B. Demand from airflow, tokio,
  clap, uv, react, pnpm, nodejs/node, pytorch, vscode (`checkCopilotEnginesVersion`)
  (9 source repos — past saturation).
- **`ordered_block`** — lines between marker pairs sorted unique
  under configurable comparator. Demand from tidy::alphabetical,
  cpython `Modules/Setup`, golang/go `api/go1*.txt` golden files,
  arrow, airflow allowed-imports, tokio (6 source repos).
- **`xml_path_matches` / `xml_path_equals`** — completes the
  structured-query family (currently json/yaml/toml). Surfaced by
  apache/spark; generalises to every Maven `pom.xml`, Ant `build.xml`,
  Gradle XML, NPM `.nuspec`, .NET `.csproj`.

**Plus one new bundled ruleset:**

- **`apache/governance@v1`** — LICENSE + NOTICE + KEYS + RAT
  discipline. 3 Apache TLPs converge (arrow + spark + airflow): 9 of
  12 governance artefacts shared. Replaces hand-rolled Apache-RAT
  shellouts.

[v0.10 design pass →](https://github.com/asamarts/alint/tree/main/docs/design/v0.10/)

---

## v0.11 — WASM plugins (after that)

The plugin tier that completes alint's extensibility story. The
`command` plugin (tier 1, shell out per matched file) shipped in
v0.5.1 and has been the only plugin tier so far. v0.11 adds:

- **`wasm` plugin kind** — `wasmtime` host, stable WIT interface.
  Author plugins in any language that compiles to WASI (Rust, Go,
  TypeScript via Javy, Python via componentize-py).
- **Plugin sandbox** — the WASM tier ships full filesystem isolation
  by default (no host-fs access; the engine pipes file content into
  the plugin and reads structured violations back out).
- **Plugin registry scaffolding** — signature verification for
  community plugins; the existing SRI-pinned `extends:` URL pattern
  generalises naturally.

**Plus one v0.11 flagship rule kind:**

- **`cross_language_implementation_complete`** — every type in a
  schema spec has a per-language test fixture. Validated by 2
  sources from the polyglot case studies: arrow's `format/Schema.fbs`
  with C++/Java/Python/Rust/Go/JS implementations, and TensorFlow's
  10 distinct API-bearing language surfaces locked by 1,185 textproto
  goldens. The rule kind that justifies the polyglot positioning in
  one primitive.

[v0.11 design pass →](https://github.com/asamarts/alint/tree/main/docs/design/ROADMAP.md#v011--wasm-plugins)
*(promoted from internal ROADMAP when v0.11 design opens)*

---

## Post-launch ongoing work

Things on the table without a fixed version slot:

- **MCP server** — [Model Context Protocol](https://modelcontextprotocol.io/)
  lets agents query tools directly. An `alint` MCP server could
  expose `get_rule_doc(rule_kind)`, `validate_config(yaml)`,
  `suggest_rules_for(repo_path)` — agent-native integration without
  an editor in the loop. ~3-5 days of work.
- **P2b polyglot drip content** — 15 more polyglot monorepo case
  studies (angular, vscode, nx, electron, beam, prisma, temporal,
  istio, grafana, cockroachdb, directus, supabase, terraform,
  flutter, dotnet/runtime, protobuf). Each ships as an
  `examples/<owner>-<repo>/` case study + a blog post + a `dev.to`
  article. Demand-drives the post-v0.11 rule-kind backlog.
- **`alint init` enhancements** — detect existing tooling
  (`.eslintrc`, `Makefile` targets, `verify-*.sh` directories) and
  propose a starter `.alint.yml` that matches the repo's existing
  conventions. Feeds into the migration-guide story.
- **Versioned docs** — alint.org currently shows current docs; a
  `/docs/v0.10/` switcher would let users on older versions land on
  accurate pages.

---

## Won't do (deliberate non-goals)

alint's scope is **the filesystem shape and contents of a repository**,
not the semantics of the code inside it. The following are explicit
non-goals:

| Won't do | Use instead |
|---|---|
| **AST-aware code linting** (variable names, unused imports, type checks) | ESLint, Clippy, ruff, golangci-lint, mypy — every language has one |
| **SAST** (security-focused code analysis: tainted-data flow, injection, dangerous APIs) | [Semgrep](https://semgrep.dev/), [CodeQL](https://codeql.github.com/) |
| **IaC scanning** (Terraform / Kubernetes / Docker security policies) | [Checkov](https://www.checkov.io/), [Conftest](https://www.conftest.dev/), [tfsec](https://aquasecurity.github.io/tfsec/) |
| **Secret scanning** (find API keys / tokens in tracked files) | [gitleaks](https://github.com/gitleaks/gitleaks), [trufflehog](https://github.com/trufflesecurity/trufflehog) |
| **Commit-message linting** (Conventional Commits etc.) | [commitlint](https://commitlint.js.org/), [committed](https://github.com/crate-ci/committed) |
| **Build-system orchestration** (running 70+ language linters in containers) | [Megalinter](https://megalinter.io/), [pre-commit](https://pre-commit.com/), `lintrunner`, `bazel build //...` |
| **Codegen / generator running** ("regenerate the file and assert it matches") | We're considering an opt-in `generated_file_fresh` primitive in v0.10+ but the *deliberate* non-goal stays: alint doesn't run your generators by default |
| **Dependency-graph problems** (import cycles, unused deps, version conflicts) | [`cargo deny`](https://github.com/EmbarkStudios/cargo-deny), [`bazel mod`](https://bazel.build/external/), [`buildifier`](https://github.com/bazelbuild/buildtools), [`knip`](https://knip.dev/), [`madge`](https://github.com/pahen/madge) |

The "use X instead" pointers exist so a misguided contribution
("alint should add a JS AST visitor!") can be redirected before
it's written. If your need is on this list, it's not that alint
is incapable — it's that alint is *deliberately* the wrong shape for
that need, and the listed tool is the right shape.

---

## How decisions get made

alint is currently a single-maintainer project with structured
decision-capture:

- **Design first, code second.** Major work lands as a design doc
  under [`docs/design/v<MAJOR>/`](https://github.com/asamarts/alint/tree/main/docs/design)
  before implementation. The doc surfaces open questions, false-positive
  surfaces, and bench-compare thresholds the implementation commits
  to. v0.7, v0.9, and v0.10 all shipped this way.
- **Demand-driven rule kinds.** New rule kinds need ≥3 distinct
  source repos showing the same need (saturation signal). The
  candidate-rule-kind table in
  [`docs/launch-prep.md`](https://github.com/asamarts/alint/blob/main/docs/launch-prep.md#rule-kind-candidates-surfaced-by-p2a-final--20-of-20-done)
  aggregates demand from the 25 OSS case studies; each candidate
  carries a per-source citation. v0.10's four headline rule kinds
  all crossed the saturation threshold.
- **5 narratives anchor scope.** The five positioning narratives
  (P2a-derived: replaces N hand-rolled scripts / catches assumed
  conventions / structural floor on top of mature tooling /
  replaces structural subset of custom orchestration / encodes
  code-review-only conventions) act as the fit-test for new ideas.
  If a candidate doesn't slot into one of the five, it goes in the
  won't-do list.
- **Public discussion.** [GitHub Discussions](https://github.com/asamarts/alint/discussions)
  is the low-friction support + feature-request channel. [GitHub
  Issues](https://github.com/asamarts/alint/issues) is the bug
  channel. Both feed the candidate table.
- **No private prioritisation.** Every roadmap-affecting decision
  lands in a public commit (typically to `docs/design/` or
  `docs/launch-prep.md`) before it's encoded in a release. There is
  no off-list backlog.

---

## When does X ship?

We don't put dates on the roadmap. Two reasons:

1. **Slipping a public date is worse than not having one.** Users
   plan around dates; missed dates erode trust.
2. **Scope-based cuts ship better software.** Each version is a
   closed cut — work that doesn't fit moves to a later version.
   Filling-time-to-meet-a-date is how feature creep enters.

You can watch
[GitHub Releases](https://github.com/asamarts/alint/releases) for
the actual ship signal — every release ships with a CHANGELOG entry,
a refreshed bench-history row, and a tagged commit.
```

## Implementation notes (for the site repo)

- New top-level route — `src/pages/roadmap.astro` or
  `src/content/docs/roadmap.md`, depending on Starlight conventions.
- Add to top-level nav (sibling to "Docs", "Examples", "Cookbook",
  "Compare", "Benchmarks").
- The won't-do table benefits from a callout / aside styling
  (Starlight's `:::note` or `:::tip` blocks) — visually separates the
  "deliberate non-goals" framing from the will-do content.
- All `https://github.com/asamarts/alint/...` links should resolve
  against the same alint repo URL the site is built from. Verify
  Starlight's link-handling does the right thing.
- The "How decisions get made" section links into specific anchors
  in `docs/launch-prep.md` — confirm those anchors are stable across
  the docs-bundle pipeline. If the launch-prep doc gets renamed or
  the section gets renumbered, the links 404.

## Open questions before publish

1. **v0.12+ speculation?** Default in this draft: hard-stop at v0.11.
   The internal ROADMAP has a v1.0 section ("DSL schema committed,
   plugin ABI committed, alint-core public API frozen, documentation
   site") but it's deliberately scope-only with no concrete features.
   Surfacing v1.0 publicly *now* signals stability without committing
   to features; surfacing it post-launch (after v0.10 + v0.11 ship)
   keeps the public roadmap honest. **Recommend hard-stop at v0.11
   for the launch publish; add a v1.0 stability section in a later
   refresh.**
2. **MCP server placement.** Currently in "Post-launch ongoing." It
   could be promoted to a v0.10.x point release if the agent-era
   positioning warrants. **Default: keep ongoing for now; promote
   when the design pass lands.**
3. **Rule-kind candidates list.** This page lists 4 v0.10 rule kinds
   + 1 v0.11 rule kind, citing source repos. The full candidate
   catalogue (~28 candidates across all P2a + migration work) lives
   in `launch-prep.md`. Should this page surface more of them, or
   stay tight to the demand-validated headline four? **Default: stay
   tight; the full catalogue is a click away in launch-prep.**
4. **Won't-do list scope.** Currently 8 entries. We could add
   `pre-commit` (it's a hook orchestrator, not a linter — but
   adopters often confuse the two), `Renovate/Dependabot` (dependency
   updates, not structural lint), or `git-hooks-only` tools (we ship
   pre-commit hooks but don't *replace* git's hook system). Recommend
   YES for `pre-commit` (high confusion risk); SKIP the other two for
   MVP.
5. **"Latest release" version reference.** Currently v0.9.16. This
   page becomes stale the moment v0.9.17 / v0.10.0 lands. The site
   repo should auto-substitute from the `docs-bundle` pipeline if
   possible — same mechanism as the rules / rulesets auto-generated
   pages. If not, accept the staleness window and bump on each
   release.

## Pre-publish checklist

- [ ] alint.org repo identified + new `/roadmap/` route created
- [ ] Top-level nav updated to surface `/roadmap/`
- [ ] All 8 won't-do tool URLs resolve at publish time
- [ ] All `docs/design/v0.10/*.md` links resolve in the
      docs-bundle-rendered docs.alint.org URLs
- [ ] `docs/launch-prep.md` anchor links resolve
- [ ] CHANGELOG link resolves to v0.9.16 (or whatever the latest
      tagged release is at publish time)
- [ ] STATE.md row for `alint-org-roadmap.md` flipped to `live`
      with date + commit SHA
- [ ] (optional) "Subscribe to releases" CTA at the bottom that
      points to `/releases.atom` once that ships in P3.3

## Estimated diff size on the site repo

- 1 new page at `/roadmap/`: ~200-220 lines of markdown
- Top-level nav config: ~5 lines
- (optional) callout CSS for the won't-do table: ~10 lines

Total: ~215-235 lines on the site repo.

## Coordination with other drafts

| Draft | Why coordinate |
|---|---|
| `alint-org-hero.md` | The hero's "what's next" CTA points to this page. Both should ship coordinated, OR this page can ship first and the hero CTA gets a bidirectional link added on publish. |
| `alint-org-compare.md` | The won't-do list overlaps significantly with compare's "When alint is NOT the right tool" — the same tool URLs need to resolve in both places. Single source-of-truth for the URL list lives in this draft. |
| `alint-org-benchmarks.md` (this batch's other draft) | The "Where alint stands" headline cites benchmark numbers; both pages should agree on the canonical numbers. Recommend: this page references `/benchmarks/` for the per-release table, doesn't duplicate it. |
| `releases-atom.md` (P3.3) | The "Subscribe to releases" optional CTA depends on the Atom feed existing. |
| Internal `docs/design/ROADMAP.md` | Stays exhaustive engineering-history; this public page summarises. Keep them in sync at the v0.10/v0.11 scope-boundary level — if v0.11 changes scope materially, this page needs an update. |
