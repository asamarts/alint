---
status: proposed
date: 2026-09-14
decision-makers: asamarts
---

# 0018. Host-linter integration boundary (no ESLint plugin tier)

## Status

Proposed (2026-09-14). Companion design doc:
[`docs/design/eslint-integration.md`](../design/eslint-integration.md), which carries the
spike, the measured failure modes, the prior-art survey, and the reproduction recipes. The
decision below covers any file-scoped host linter; ESLint is the instance that prompted it.

## Context

An adopter asked to run alint "as a plugin of some sort" inside an existing, extensive
ESLint config rather than "integrate yet another new tool". The request is reasonable and
the shape will recur: both `CONTRIBUTING.md` and the feature-request template already ask
adopters what they use today ("custom shell script? eslint plugin?"), and the README's third
"where alint shines" class is repos with mature per-language tooling and no structural
layer. A durable answer beats reacting to each request.

The investigation built the plugin rather than reasoning about it, and found three things.

- **ESLint's unit of work is a file; alint's is a repository.** Every ESLint extension
  point is file-scoped and synchronous. Rules report on a `node` or a `loc` inside the file
  in hand, processors split one file, languages parse one file, and there is no
  project-level rule API, no way to report on a file that does not exist, and no way to
  attach a message to another file. The nearest proposal, the prelint plugins RFC (open
  since February 2023), is explicitly file-scoped and cannot report violations. alint takes
  the opposite position on purpose: `alint check src/index.js` refuses a single file.

- **The bridge loses most of the findings, silently.** On a representative Node workspace
  with six bundled rulesets, 10 of 19 violations have no ESLint-addressable location (7
  tree-wide, 3 anchored to a directory). Of the 4 that map cleanly to a file and line, all
  4 are things ESLint already has core rules for. Worse, the plugin is unsound under three
  standard workflows: `eslint src/` reports 5 problems where `eslint .` reports 19, with
  nothing said about the 14 that were skipped; `--cache` replays "README missing" after a
  README exists, because the sentinel file's own bytes never changed; and a long-lived
  process (the VS Code ESLint server) freezes findings at the first lint, because the
  shellout must be memoized to be affordable. Each produces a green build from a failing
  tree.

- **The prior art divides on exactly this line.** `eslint-plugin-dependency-cruiser`
  wrapped a whole-tree tool and had to drop whole-tree analysis and every "must exist"
  rule; four versions, abandoned after two days in July 2022.
  `eslint-plugin-publint` wraps a tool that checks one file, `package.json`; 26 versions,
  still shipping in 2026. The popular `eslint-plugin-project-structure` does enforce file
  existence inside ESLint, by writing a `projectStructure.cache.json` into the user's
  repository during linting, because ESLint gives a plugin nowhere else to keep cross-file
  state.

The decision driver is alint's own fail-loudly contract (the v0.14 hardening pass, and
[ADR-0012](0012-output-completeness-testing.md) on output completeness). A tier that
silently stops checking under `--cache` or a path argument contradicts the property the
rest of the tool is built to hold.

## Decision

We will **integrate with host linters at the report layer, not the rule layer**, and ship
no plugin tier for any file-scoped host linter.

Concretely:

1. **Decline the ESLint plugin.** alint will not publish an `eslint-plugin-*` that maps its
   findings onto ESLint rules via a sentinel file. The four failure modes are properties of
   ESLint's execution model, not of an implementation, and three of them fail open.
   (`eslint-plugin-alint` is also already taken on npm by an unrelated 2015 package, so the
   name would have to be `@asamarts/eslint-plugin-alint` regardless.)

2. **Ship a documented merge recipe and a helper.** alint runs once, over the repository,
   outside the host's per-file loop; its findings are appended to the host's own results
   array before formatting. For ESLint this means `eslint "$@" -f json`, merge, render, and
   own the exit code, which preserves the host's full CLI and keeps alint's rule ids,
   severities, and `policy_url` links intact. The helper must handle the two blockers in
   design-doc section 6 rather than leave adopters to find them: alint exits 2 with empty
   stdout on a config error (so only exit 0 and 1 carry a report, and exit >= 2 or a spawn
   error must fail the run), and ESLint v10 rejects results for files it did not lint in
   any formatter that reads `rulesMeta`, including SARIF (so third-party formatters are
   required directly with a self-supplied `rulesMeta`).

3. **Never ship the formatter variant.** Merging from a custom ESLint formatter is
   fail-open by construction: `countErrors` runs before the formatter and `bin/eslint.js`
   overwrites `process.exitCode` with the already-computed value, so an alint-only failure
   exits 0.

4. **Keep a per-file plugin tier on the shelf, gated on demand and on a CLI surface.** A
   plugin restricted to alint's `PerFileRule` set would be sound under subsetting and
   caching. It requires a per-file evaluation mode the engine already has internally
   (`reeval_file` in `alint-lsp`) but does not expose on the CLI. We revisit it only if
   several more adopters ask, and only together with that `--file` mode. It is explicitly
   not a promise.

5. **Keep `alint lsp` as the editor integration.** The host's editor extension calls the
   host's own API and never sees a wrapper, so in-editor diagnostics stay with the language
   server. This tier does not remove the second editor extension, and we should say so
   rather than imply otherwise.

This changes no rule behaviour, no engine semantics, and no `.alint.yml` schema. It adds a
docs page, a small JS helper beside the npm shim, and a documented overlap list of alint
rules that a normal ESLint config supersedes.

## Consequences

Easier:

- **A decision rule replaces case-by-case debate.** "Make it a plugin for X" resolves
  against one test: is the host's unit of work a file? If so, integrate at the report layer.
  The same answer covers Biome, Ruff and stylelint without a fresh investigation.
- **Adopters get one command and one report** without giving up rule ids, the three
  severity tiers, `policy_url` links, `alint fix`, or `alint baseline`.
- **The integration cannot fail open.** alint always sees the whole tree regardless of what
  the host was asked to lint, so subset linting, `--cache` and watch loops stay correct.
- **Maintenance is bounded.** A merge helper is a few dozen lines against a stable
  `LintResult` shape, not a plugin tracking ESLint's rule, processor and language APIs
  across majors.

Harder, and accepted:

- **We are saying no to the literal request.** Some adopters will read "not a plugin" as
  "not integrated", and the answer only lands if the reasoning travels with it. The docs
  page has to carry the why, not just the recipe.
- **The helper is the first JS we actually maintain.** The npm package ships zero JS runtime
  behaviour on purpose ([ADR-0015](0015-distribution-strategy.md)); this adds a real, if
  small, JS surface with its own ESLint-major compatibility risk.
- **Two suppression systems coexist.** `eslint-disable` and ESLint's suppressions file do
  not apply to alint findings; `alint baseline` is the parallel mechanism, and adopters have
  to learn which is which.
- **A second editor extension remains.** Part of the original complaint is not addressed by
  this decision and will not be.
- **`info` has no home in a two-severity host.** Merged findings collapse `info` into
  warning, which can fail a `--max-warnings 0` config on advisory output. Whichever default
  we pick is a compromise, documented rather than solved.
- **Duplicate findings are the adopter's to tune.** Where alint and the host overlap
  (`no-console` and `agent-no-console-log`, and similar), both report, and the host's
  locations are usually better. We ship a list of alint rules to set `level: off`, and that
  list is hand-maintained.

## Considered Options

- **Report-layer merge (chosen)** vs **full plugin with a sentinel file** vs **per-file
  plugin** vs **nothing beyond the current CLI**. The full plugin is unsound for 10 of 19
  findings on a representative repo and fails open under three standard workflows. The
  per-file plugin is sound but ships only the subset ESLint already covers, while the
  differentiated half stays outside; kept on the shelf per decision 4. Doing nothing is
  defensible, since `"lint": "eslint . && alint check"` already works, and it is what knip,
  syncpack, dependency-cruiser and madge all do, but it leaves the two blockers for every
  adopter to rediscover.
- **Wrapper owns the process (chosen)** vs **custom formatter** vs **reimplementing the
  host's CLI**. The formatter variant cannot fail a build. Reimplementing ESLint's 48 flags
  is real work for no gain when the Node API exposes every option and the CLI can simply be
  spawned with argv forwarded verbatim.
- **Self-supplied `rulesMeta` via direct `require` (chosen)** vs **restricting findings to
  files the host already linted**. Restricting would satisfy the provenance guard, but it
  reintroduces the subset hole for exactly the findings this decision exists to preserve.
- **A boundary stated for all host linters (chosen)** vs **an ESLint-specific decision**.
  The property that matters, a file-scoped unit of work, is shared by every host linter we
  are likely to be asked about, so the general form costs nothing extra and saves a repeat
  investigation.

## More Information

- Design doc with the spike, the measured failure modes, the prior-art survey, the scope
  table and the reproduction recipes:
  [`docs/design/eslint-integration.md`](../design/eslint-integration.md).
- Related: [ADR-0015](0015-distribution-strategy.md) (distribution channel tiering) applies
  the same own/automate/delegate reasoning to packaging; this applies it to tool
  integration. [ADR-0009](0009-rule-discovery-cli-config-vs-catalog.md) made the adjacent
  call of keeping two surfaces distinct (`alint rules` over the catalog, `alint list` over
  the config) rather than collapsing them into one command that serves neither; the same
  instinct applies here to two units of work.
- Key anchors: `crates/alint-lsp/src/lib.rs` (`group_findings` anchors path-less findings
  to the config file, which is the sentinel pattern a plugin would need; `reeval_file` is
  the per-file evaluation the shelved tier would expose), `schemas/v1/check-report.json`
  (the report contract a merge helper consumes), `npm/` (where a helper would ship),
  `docs/site/integrations/` (where the docs page goes).
