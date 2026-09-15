# Design doc: host-linter integration (ESLint)

Status: Proposed (2026-09-14). (Draft | Proposed | Accepted | Implemented in `<commit>` | Superseded by `<doc>`.)
Decisions: [ADR-0018](../adr/0018-eslint-integration-boundary.md) (proposed) records the integration boundary, what we ship, and what we decline.
Demand evidence: an adopter asking to run alint "as a plugin of some sort" inside an existing, rather extensive ESLint config "rather than integrate yet another new tool to our dev environment". One request so far, but the shape is expected to recur: both [`CONTRIBUTING.md`](../../CONTRIBUTING.md) and the feature-request template already ask adopters what they do today ("custom shell script? eslint plugin? README convention?"), and the README's third "where alint shines" class is explicitly repos with mature per-language tooling and no structural layer.
Scope: evergreen integration-strategy doc (like [`distribution.md`](distribution.md)), not a per-version rule-kind spec. It records the ESLint-plugin investigation, the measured failure modes of that path, the prior art, and the integration we propose instead. Every number below was produced by a spike rather than inferred; section 8 carries the reproduction so the register can be re-verified.

Environment for every measurement in this document: alint 0.16.1, ESLint 10.10.0, Node 26.5.0, Linux.

---

## 1. Problem

alint arrives as a separate tool. For a JS/TS shop that already runs an extensive ESLint
config, that reads as cost: another devDependency, another config file, another CI step,
another editor extension, another output format to learn. The natural request is "make it
an ESLint plugin", and it is a reasonable request to make.

Some of that cost is already paid down. alint ships on npm (`@asamarts/alint`, a shim
around the static binary with no JS runtime behaviour), so it is already a devDependency
rather than a new install channel, and `alint lsp` already drives in-editor diagnostics
across eight editors. What remains is genuinely two things: a second config file, which is
irreducible because the rules have to live somewhere, and a second command with a second
report, which is what this document addresses.

The question this document answers is not "would a plugin be nice" but "can a plugin be
*correct*". It cannot, for most of what alint checks, and the failure is silent. That is
the finding that drives the decision.

## 2. Why ESLint cannot host alint

Every ESLint extension point is scoped to one physical file, evaluated synchronously:

- **Rules** run per file. `context.report()` requires a `node` or a `loc`, both of which
  address a position inside the file currently being linted. A rule receives
  `context.filename`, `context.physicalFilename`, `context.cwd`, `context.sourceCode`,
  `context.options` and `context.settings`, and no handle on the run as a whole.
- **Processors** split one file into blocks of one file.
- **Languages** (the v9+ `Language` interface) parse one file's text. `fileType` is
  `"text"`; there is no multi-file mode.
- **Configs, settings and `linterOptions`** carry data, not behaviour.

There is no project-level rule API, no way to report on a file that does not exist, and no
way to attach a message to a file other than the one in hand. This is not an oversight
pending a fix: the nearest proposal, the [prelint plugins
RFC](https://github.com/eslint/rfcs/pull/105) (opened February 2023, still unmerged as of
this writing), is explicitly file-scoped and states that prelints cannot report violations
at all.

alint holds the opposite position deliberately, and the CLI says so:

```console
$ alint check src/index.js
alint: src/index.js is a file, but `check`/`fix`/`baseline` take a repository root
(a directory), not a single file
```

A rule like "this repo must have a `LICENSE`" is a property of a tree. So is "every
`packages/*` has a README", "`dist/` is not tracked", and every cross-file kind. ESLint's
unit of work is a file; alint's is a repository. An ESLint plugin has to bridge that, and
the bridge is where the correctness goes.

## 3. What a plugin actually does: the spike

A plugin was built rather than reasoned about. The shape is the only one available: a stub
parser returning an empty `Program` so ESLint will open non-JS files, a memoized
`alint check --format json` shellout, per-file findings routed to their files, and
everything else routed to a sentinel file. The sentinel is `.alint.yml`, which is what
`alint-lsp` already does for path-less findings (`group_findings` in
`crates/alint-lsp/src/lib.rs` anchors them to the config file so a "missing required file"
still shows somewhere).

### 3.1 Most findings have nowhere to land

The fixture is a representative Node workspace: `workspaces` in `package.json`, a
`packages/ui` member, a GitHub workflow, a tracked `dist/`, debug residue in `src/`, a
stray `.bak`, a scratch `PLAN.md`. Config extends six bundled rulesets (`oss-baseline`,
`node`, `ci/github-actions`, `hygiene/no-tracked-artifacts`, `agent-hygiene`,
`monorepo/yarn-workspace`). It produces 19 violations:

| Anchor | Count | ESLint can address it? |
|---|---|---|
| No path at all (tree-wide) | 7 | No. There is no file. |
| A directory (`dist`, `packages/ui`) | 3 | No. ESLint reports on files. |
| A file, no line | 5 | Only as `1:1`. |
| A file and a line | 4 | Yes. |

**10 of 19 have no ESLint-addressable location.** The tree-wide seven are the missing
`LICENSE`, `SECURITY.md`, `CODEOWNERS`, `CODE_OF_CONDUCT.md`, a dependency-update tool, a
Node version pin, and a README.

The sharper point is *which* four map cleanly: trailing whitespace, `console.log`,
`debugger;`, and a TODO marker. ESLint already ships `no-trailing-spaces`, `no-console`,
`no-debugger` and `no-warning-comments` for exactly those. The findings that fit ESLint's
model are the ones ESLint already covers; the findings that are alint's reason to exist are
the ones it cannot host.

### 3.2 Four failure modes, all silent

On a clean full-repo run the plugin works: 19 of 19 findings surface. Run it the way an
extensive config actually gets run and it stops working.

1. **Subset linting drops repo-level findings, unreported.** `eslint .` reports 19
   problems; `eslint src/` reports **5**. The sentinel is not in the lint set, so every
   tree-wide finding vanishes and the build is green. This is the lint-staged shape, the
   pre-commit shape, and the editor shape.

2. **`--cache` reports violations that no longer exist.** ESLint's cache is keyed on each
   file's own content. The sentinel's bytes do not change when the repo around it does, so
   its messages replay. Create a `README.md` and re-run with `--cache`: ESLint still prints
   `[oss-readme-exists] An open-source repo should have a README`, while `alint check`
   reports that rule passing. A phantom failure the user cannot clear.

3. **A long-lived process freezes findings at the first lint.** Memoizing the shellout is
   mandatory, otherwise every file pays a full repo scan. With it, a process that outlives
   one run, which is exactly the VS Code ESLint server, never re-runs alint. Two
   `lintFiles()` calls in one process with a `README.md` created in between both report the
   README missing. Fixing this requires a filesystem watcher and an invalidation model
   inside an ESLint plugin, which is what `alint lsp` already is.

4. **Rule identity and severity collapse.** Every finding arrives under one ESLint rule id,
   so `eslint-disable` comments, per-rule severity overrides, `--max-warnings` and the
   per-rule `policy_url` stop discriminating. ESLint has two severities and alint has
   three, and a rule's severity is fixed in the ESLint config rather than per finding, so
   `info` findings print as `error`. Splitting into `alint/error` / `alint/warning` /
   `alint/info` recovers severity but not identity. `--fix` is unavailable either way: the
   check report carries no byte ranges, and the fix ops include `file_create`,
   `file_remove` and `file_rename`, which ESLint's single-file text fixer cannot express.

Failure modes 1 to 3 are the disqualifying ones. A linter that quietly stops checking is
worse than no linter, and all three produce a green build from a tree that alint says is
failing.

## 4. Prior art

Two packages wrap an external checker as an ESLint plugin, and which one survived is not a
question of effort.

- **`eslint-plugin-dependency-cruiser`** ran dependency-cruiser per linted file. Its README
  lists what it had to drop: "violations that require cruising the whole dependency tree,
  e.g. circular dependencies", plus the `allowed` and `required` rule types, that is, every
  "this must exist" check. Published July 2022, four versions, last release July 2022, no
  `repository` field. It had to discard precisely the categories alint is built around.

- **`eslint-plugin-publint`** wraps publint, which checks one file: `package.json`. The
  plugin is `files: ["**/package.json"]`, a JSONC parser, and rules split by severity.
  Nothing to bridge, because the finding was always a property of a file. Published August
  2024, 26 versions, last release May 2026.

The third data point is `eslint-plugin-project-structure`, which is popular and does
enforce file existence inside ESLint. The mechanism is a stub parser, `fs.existsSync` from
inside rules, and a `projectStructure.cache.json` that the plugin **writes into the user's
repository during linting** to deduplicate errors across files, because ESLint gives a
plugin nowhere else to keep cross-file state. That is what the workaround looks like when
someone commits to it.

Incidental but blocking: `eslint-plugin-alint` is taken on npm by an unrelated package from
2015, last published 2022. A first-party plugin would have to ship as
`@asamarts/eslint-plugin-alint`.

## 5. Proposed integration: merge at the report layer

Every failure mode in section 3.2 comes from forcing a repository-scoped run through a
file-scoped extension point. The fix is to stop doing that. ESLint's Node API returns a
plain `LintResult[]` before formatting; run alint once, over the repository, outside
ESLint's per-file loop, and append its findings to that array.

The result keeps real rule ids, real severities, correct columns, and renders
directory-anchored findings as their own entries, because ESLint's formatters do not care
that `packages/ui` is a directory:

```text
.../fixture/.alint.yml
  1:1  warning  An open-source repo should declare a license at the root   alint/oss-license-exists
  1:1  warning  Consider adding a CODEOWNERS file so PR reviews are auto-routed
                                                                           alint/oss-codeowners-exists

.../fixture/packages/ui
  1:1  warning  expected a file matching [packages/ui/README.md]           alint/yarn-workspace-member-has-readme

.../fixture/src/index.js
  1:1  error    `debugger;` / `breakpoint()` must not be committed          alint/agent-no-debugger-statements
```

To preserve ESLint's 48-flag CLI, the wrapper should spawn it rather than reimplement it:
`eslint "$@" -f json`, merge, render, own the exit code. Verified with
`--cache --max-warnings 999 src/` forwarded verbatim: ESLint linted only `src/`, alint still
scanned the whole tree, and every finding surfaced. Subset linting is sound here precisely
because alint's scope no longer depends on what ESLint was asked to look at.

For adopters who want less, `"lint": "eslint . && alint check"` is one line, and it is what
every comparable Node tool does. knip, syncpack, dependency-cruiser and madge all ship a
CLI and no ESLint plugin.

## 6. Known costs of the proposed path

The happy path is not the whole story. Two of these silently pass a broken build if the
obvious version is written, and one rules out the variant most people reach for first.
Shipping this first-party is worth doing largely so adopters do not discover 6.1 and 6.2
themselves.

### 6.1 A failing alint makes the build pass (blocker)

On a config or internal error alint exits 2 with **empty stdout** and the message on
stderr. The obvious `catch (e) { JSON.parse(e.stdout) }`, written for the exit-1 case where
stdout *is* the report, yields `null`, merges zero findings, and goes green. The same
happens when the binary is missing (`ENOENT`, `e.stdout` undefined).

Mitigation: only exit 0 and 1 carry a report. Treat exit >= 2 and any spawn error as a hard
failure of the lint run. alint's own CLI is rigorously fail-loud (the v0.14 hardening pass);
a naive wrapper is where that gets discarded.

### 6.2 Half the formatters reject synthetic results (blocker)

ESLint v10 checks result provenance. `getRulesMetaForResults` looks up a cached config array
for every result's `filePath` and throws `Results object was not created from this ESLint
instance.` when there is none. Unknown rule ids are fine (the lookup explicitly ignores
them); unlinted **paths** are not, and that is every alint finding on a path ESLint did not
open.

- Safe: `stylish`, `json`. Neither reads `rulesMeta`.
- Throws: `html`, `json-with-metadata`, and any third-party formatter loaded through
  `loadFormatter` that reads `rulesMeta`. Confirmed with
  `@microsoft/eslint-formatter-sarif`, which is the Code Scanning path.

Mitigation: `require()` third-party formatters directly and pass your own `rulesMeta`. The
`rulesMeta` field on the formatter context is a lazy getter, so supplying it bypasses the
provenance check entirely. This is also an upgrade rather than a workaround, because it lets
each alint rule's `policy_url` become that rule's `docs.url`. Verified against SARIF: 18
rules in the driver, 22 results. There is no mitigation for the two built-ins, since v10's
`exports` map blocks `eslint/lib/cli-engine/formatters/*` and `use-at-your-own-risk` exposes
only `builtinRules` and `shouldUseFlatConfig`.

### 6.3 A custom formatter can never fail the build (rules out a variant)

The tempting shape is to keep the real `eslint` CLI and merge alint in from a custom
formatter, getting all 48 flags for free. It is fail-open by construction:
`countErrors(results)` runs before the formatter in `lib/cli.js`, and `bin/eslint.js`
overwrites `process.exitCode` with the value `cli.execute()` already computed. Tested with a
formatter that injects a finding and sets `process.exitCode = 1`: the finding prints and the
process exits 0. This shape must not ship.

### 6.4 `info` becomes a warning, and strict configs fail on it

ESLint has two severities; alint has three. In the fixture, 8 of 19 findings are `info`,
advisory things like "consider adding a CODEOWNERS file". Merged, they are
indistinguishable from warnings, so a config running `--max-warnings 0` fails the moment
alint is added. That is not hypothetical: the lint script alint's own
[`examples/microsoft-typescript/`](../../examples/microsoft-typescript/) config shells out
to is `eslint --cache --report-unused-disable-directives --max-warnings 0 .`, which is both
this hazard and the 3.2 cache hazard in one line. Whether `info` merges at all should be a
documented, deliberate default.

### 6.5 Overlapping rules report everything twice

`node@v1` and `agent-hygiene@v1` cover ground a normal ESLint config already covers, and
after a merge both print. The same three problems appear as `no-console` / `no-debugger` /
`no-warning-comments` and as `alint/agent-no-console-log` /
`alint/agent-no-debugger-statements` / `alint/agent-no-model-todos`. ESLint's locations are
also better: it puts the `debugger` at `2:1`, alint at `1:1`, because the alint rule is a
file-level content rule. The overlapping alint rules want `level: off` when ESLint owns
them, and that list belongs in the docs page.

### 6.6 Residual limits

- **It does not remove the second editor extension.** The VS Code ESLint extension calls
  the ESLint API itself and never sees the wrapper, so in-editor diagnostics still come
  from `alint lsp`. Part of the original request survives.
- **`eslint-disable` does not apply**, nor does ESLint's suppressions file
  (`applySuppressions`, v10.1). `alint baseline` is the equivalent, so there are two
  suppression systems.
- **`--fix` fixes only ESLint's half.** `alint fix` remains a separate step.
- **cwd sensitivity.** alint does not search upward for `.alint.yml`; it fails outright
  when run from `src/`, while ESLint resolves its config upward from the linted file. The
  wrapper must resolve the repository root explicitly rather than trusting
  `process.cwd()`.
- **Windows.** `execFileSync("alint")` misses the `.cmd` shim npm installs; resolve through
  `node_modules/.bin` or spawn through a shell.
- **Monorepo task caching.** A per-package `lint` task under turbo or nx that also runs a
  repository-wide scan reads files outside its declared inputs, so the cache key is wrong.
  The alint step belongs in a root task, or its inputs must be declared.
- **Cost.** One extra full-tree scan per lint invocation: about 30 ms on the fixture, and
  the published macro benchmarks put a 100K-file workspace near 1.1 s. Fine in CI,
  noticeable in a watch loop.

## 7. Scope: what we ship, what we decline

Ranked by value over cost. The decision is recorded in
[ADR-0018](../adr/0018-eslint-integration-boundary.md).

| | Effort | Buys | Costs |
|---|---|---|---|
| **Ship: docs page**, "alint alongside ESLint", under `docs/site/integrations/` | Half a day | Answers this request and the ones after it. The directory currently has Docker, editors, GitHub Actions and pre-commit, and nothing for the Node toolchain. | Nothing. |
| **Ship: merge helper**, published beside the npm shim | 3 to 5 days | One command, one report, one exit code, full fidelity, none of the section 3.2 failures. | The 6.1 and 6.2 blockers must be handled rather than discovered. A small JS surface to track across ESLint majors. |
| **Reconsider later: per-file plugin**, honestly scoped | 1 to 2 weeks | A real plugin restricted to the `PerFileRule` set, which is sound under subsetting and caching. Needs a per-file CLI surface the engine already has internally (`reeval_file` in `alint-lsp`) but does not expose. | Ships the subset that overlaps most with rules ESLint already has, while the differentiated half stays outside. Reads as "alint, but worse". |
| **Decline: full plugin with a sentinel** | 2 to 4 weeks plus upkeep | The literal request. | All four failure modes, each needing a mitigation ESLint will not help with. A second front-end that cannot carry fixes, rule ids, severities or docs links. |

The per-file tier stays on the shelf until several more adopters ask, and it should land
with a `--file` mode on the CLI rather than before one.

## 8. Reproduction

The fixture is a Node workspace with `packages/*`, a `.github/workflows/ci.yml` with an
unpinned action and no `name:`, a tracked `dist/`, `src/index.js` carrying a `console.log`
with trailing whitespace plus a `debugger;` and a `TODO(claude):` marker, `src/index.js.bak`,
a root `PLAN.md`, and a `.gitignore` covering `node_modules/`. Its `.alint.yml` extends the
six rulesets named in section 3.1. Against alint 0.16.1 it yields the 19 findings in the
table there.

- **Anchor split**: `alint check --format json`, then bucket each violation by whether
  `path` is absent, names a directory, or names a file with or without `line`.
- **Subset drop**: build the plugin per section 3, then compare `eslint .` (19 problems)
  with `eslint src/` (5 problems).
- **Cache phantom**: `rm -f .eslintcache && eslint . --cache`, create `README.md`, re-run
  `eslint . --cache`, and grep for `oss-readme-exists` while `alint check` reports it
  passing.
- **Process staleness**: one `ESLint` instance, two `lintFiles(["."])` calls, `README.md`
  created between them; both report it missing.
- **Provenance guard**: call `eslint.getRulesMetaForResults([r])` with a hand-built
  `LintResult` whose `filePath` ESLint did not lint.
- **Formatter exit code**: a formatter that sets `process.exitCode = 1`, run against a
  config where ESLint itself finds nothing; observe exit 0.

## 9. Open questions

1. **Does `info` merge by default?** Section 6.4. Dropping it protects
   `--max-warnings 0` adopters but hides advisory findings from the merged report. Leaning
   toward merging it and documenting the interaction, since silently dropping a severity
   tier is the kind of quiet behaviour this document argues against.
2. **Where does the helper live?** A second entry point inside `@asamarts/alint`, or its own
   package. The shim currently ships zero JS runtime behaviour on purpose
   ([ADR-0015](../adr/0015-distribution-strategy.md)), and a merge helper would be the first
   JS we actually maintain.
3. **Is the overlap list generated or hand-written?** Section 6.5 needs a mapping from alint
   rule ids to the ESLint rules that supersede them. Hand-written is fine at this size; it
   is also exactly the "second list" hazard [ADR-0001](../adr/0001-adopt-spec-driven-development.md)
   warns about, so it should carry a test if it grows.
4. **Does the same reasoning generalise to Biome, Ruff and stylelint?** All three are
   file-scoped, so the boundary in ADR-0018 is written to cover any host linter. Worth
   confirming before a second adopter asks about a different host.
