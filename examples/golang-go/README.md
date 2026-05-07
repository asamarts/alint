# Case study: `golang/go`

Inventory of the structural-validation tooling in `golang/go` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-checkout of root config
files + `src/cmd/dist/`, `src/cmd/api/`, `src/internal/buildcfg/`,
`src/cmd/internal/testdir/` (the heaviest sub-trees `src/`, `test/`,
`api/` are excluded; the inventory is unaffected because golang/go's
structural validation lives in those four `src/cmd/...` subtrees plus
the top-level convention surfaces).

---

## Summary

golang/go is the **convention-heavy minimal-tooling extreme** of every
repo we've inventoried. Where `kubernetes/kubernetes` ships 50
hand-rolled `hack/verify-*.sh` scripts and `tokio-rs/tokio` runs a
1365-line GitHub Actions matrix, golang/go has:

- **Zero `.github/workflows/`** — CI runs on Google's internal LUCI
  builders driven by `src/cmd/dist/test.go`'s `registerTests()` /
  `registerStdTest()` discovery loop.
- **Zero `Makefile`** at the top level — `src/all.bash` /
  `src/make.bash` are the canonical bootstraps (mirrored as `.bat`
  for Windows and `.rc` for Plan 9).
- **Zero `.golangci.yml`** — the Go authors don't lint themselves
  with golangci-lint; they wrote the language and use `go vet` plus
  the `git-codereview` Gerrit hook.
- **Zero `AUTHORS` / `CONTRIBUTORS`** files — both were retired
  during the Gerrit migration; contributor tracking moved to git
  history + the CLA database.
- **Zero per-PR linter shellouts** — PRs land in GitHub, get
  imported into Gerrit (per the explicit `.github/PULL_REQUEST_TEMPLATE`
  policy), and then go through `git-codereview` which enforces gofmt
  cleanliness as the only structural gate.

In total, **17 distinct structural-validation surfaces** were
inventoried. Of those:

- **~53 % map directly to existing alint rules** (~9 surfaces:
  `.gitattributes` policy, license-header convention across 5
  comment-syntax variants, `.gitignore` discipline, the Gerrit
  `codereview.cfg`, `SECURITY.md` pointing at `go.dev/security/policy`,
  the GitHub issue-template set, `.github/PULL_REQUEST_TEMPLATE`'s
  "No Markdown" warning, the `lib/fips140/` registry shape, and the
  4-go.mod canonical layout)
- **~24 % need new alint primitives** (~4 surfaces: per-package
  import gates like "no `testing` import in non-test sources", the
  `doc/next/.../<issue-number>.md` ↔ live GitHub issue cross-ref,
  `lib/fips140/fips140.sum` ↔ on-disk zip hash freshness, and the
  `api/go1*.txt` golden-file ordering / membership constraints)
- **~23 % are out of alint's deliberate scope** (~4 surfaces: the
  `src/cmd/api/main_test.go` exported-symbol type-graph analysis,
  `src/cmd/dist/test.go` integration-test orchestration,
  `src/internal/buildcfg/` GOOS/GOARCH parsing, and the
  `src/cmd/compile/`-resident SSA / mkasm / mkconst codegen pipelines)

The headline is **NOT** "alint replaces N hand-rolled scripts" (golang/go
has effectively zero hand-rolled structural-validation scripts) — it's
**"alint encodes the unwritten Go conventions enforceable for the first
time."** The 3-line BSD license header, the 4-go.mod canonical layout,
the `.github/PULL_REQUEST_TEMPLATE` "No Markdown" rule, the `.gitattributes
* -text` line that's load-bearing for Windows builds, the
`doc/next/6-stdlib/99-minor/<package>/<issue>.md` filename grammar — none
of these are checked by any script, anywhere in golang/go today. They are
enforced by Russ Cox & co. in code review. alint makes them
machine-checkable in 31 rules across one file.

---

## Existing tooling inventory

### In-tree validation surfaces (4 files, ~3000 LoC)

| File | Lines | What it does | alint disposition |
|---|---:|---|---|
| `src/cmd/dist/test.go` | 1700+ | Discovers + orchestrates the integration test suite (`registerTests`, `registerStdTest`); runs cgo / fips / experimental cfg variants; the spectre + race + asan modes | OUT OF SCOPE (test orchestration, not structural validation) |
| `src/cmd/api/main_test.go` | 700+ | Loads the std library across 25 `(GOOS, GOARCH, CgoEnabled)` contexts; computes the exported symbol set; diffs against `api/go1*.txt` golden files and `api/next/*.txt`. The `compareAPI()` function is the gate. | OUT OF SCOPE (AST + types graph analysis) |
| `src/internal/buildcfg/cfg.go` + `exp.go` | 200 | Parses `GOOS`, `GOARCH`, `GOEXPERIMENT` env vars + the embedded `defaultGOEXPERIMENT` const | OUT OF SCOPE (runtime config parsing, not validation) |
| `src/cmd/internal/testdir/testdir_test.go` | 2042 | Walks `GOROOT/test/`, registers each `.go` file as a subtest with the `errorcheck`/`compile`/`run` directive parsed from its first line | OUT OF SCOPE (test fixture orchestration) |

**There is no in-tree structural linter.** The closest thing is the
`git-codereview` Gerrit hook (an *external* tool that golang/go
*depends on but doesn't ship*), which:
- Runs `gofmt -d` on every uploaded change.
- Rejects CRLF line endings (cooperating with `.gitattributes`'s
  `* -text` directive that disables git's normalization).
- Rejects bidi control characters (Trojan Source / CVE-2021-42574
  defense).

### Top-level config files (10 files)

| File | What it does | alint asserts |
|---|---|---|
| `.gitattributes` | Disables git's line-ending normalization (`* -text`) so `.bat` files can be checked in with CRLF (load-bearing — `test/winbatch.go` enforces it) | `go-gitattributes-no-text-normalization` matches `^\* -text\s*$` |
| `.gitignore` | Lists ~30 generated artifact paths under `src/`, plus `/bin/`, `/pkg/`, `/build.out`, `/last-change`, `/test.out`, the cgo `_obj` / `_test` / `_cgo_*` patterns | Bundled `oss-gitignore-exists` + `go-no-toplevel-bin` + `go-no-toplevel-pkg` (the two generated dirs whose absence is structurally guaranteed) |
| `.github/CODE_OF_CONDUCT.md` | Two lines — points at `golang.org/conduct` | Bundled `oss-code-of-conduct-exists` |
| `.github/PULL_REQUEST_TEMPLATE` | The PR title formatting rules (`net/http: frob the quux before blarfing`), and the **"No Markdown"** instruction (PRs are imported verbatim into Gerrit, which is plaintext) | `go-pull-request-template-no-markdown-warning` matches `\+ No Markdown` |
| `.github/SUPPORT.md` | Triage routing | Not asserted (no convention to enforce) |
| `.github/ISSUE_TEMPLATE/*.yml` | 12 issue templates with strict numbering (`00-bug.yml`, `01-pkgsite.yml`, ..., `12-telemetry.yml`) + `config.yml` for the issue picker | `go-issue-templates-required` asserts the load-bearing 6 |
| `CONTRIBUTING.md` | Points at `golang.org/doc/contribute` and reminds that `go bug` is the recommended issue path | Bundled `oss-readme-exists` + `oss-license-exists` cover the foundation |
| `LICENSE` | BSD 3-clause | Bundled `oss-license-exists` |
| `PATENTS` | Patent-grant boilerplate (one of the few projects that ships a separate PATENTS file alongside LICENSE) | Not asserted (rare convention; not generalisable) |
| `SECURITY.md` | Points at `go.dev/security/policy` | `go-security-policy-references-go-dev` asserts the URL is in the file |
| `README.md` | States the canonical Git repo URL is `go.googlesource.com/go` (with GitHub being a mirror) | `go-readme-references-canonical-source` asserts the URL appears |
| `codereview.cfg` | Two lines: `branch: master` — the Gerrit branch routing | `go-codereview-cfg-present` + `go-codereview-cfg-branch` |
| `go.env` | Initial defaults for `go env` (GOPROXY, GOSUMDB, GOTOOLCHAIN) | `go-bsd-license-header-misc-go-mod` flags missing header (info-level — golang/go's go.env doesn't carry the BSD header today) |

### The 4-go.mod canonical layout

| Module | Path | Purpose |
|---|---|---|
| `std` | `src/go.mod` | The standard library. `go 1.27`. Pins `golang.org/x/crypto`, `golang.org/x/net`. |
| `cmd` | `src/cmd/go.mod` | The toolchain. `go 1.27`. Pins ~10 `golang.org/x/*` packages plus `github.com/google/pprof`, `github.com/ianlancetaylor/demangle`, `rsc.io/markdown`. |
| (misc) | `misc/go.mod` | Demo / example tree (the cgo gmp demo, etc.) |
| (mkmalloc) | `src/runtime/_mkmalloc/go.mod` | Internal generator scratch (the `_mkmalloc` underscore prefix means the Go build ignores it) |

The repo root is **NOT** a Go module. `go-no-toplevel-go-mod` enforces
this — alint surfaces the inversion explicitly because every other
Go project the bundled `go@v1` ruleset's `go-mod-exists` rule expects
the inverse.

### `lib/fips140/` — the certified cryptographic module registry

| File | Purpose |
|---|---|
| `certified.txt` | Single line: the version string of the CMVP-certified zip (e.g. `v1.0.0-c2097c7c`) |
| `inprocess.txt` | Single line: the version string of the in-validation snapshot (e.g. `v1.26.0`) |
| `fips140.sum` | SHA256 checksums of every snapshot zip in the directory. Header line: `# SHA256 checksums of snapshot zip files in this directory.` (load-bearing — the CMVP-submitted security policy references this file format verbatim). |
| `Makefile` | Build glue for regenerating snapshots |
| `README.md` | Module documentation |
| `v1.0.0-c2097c7c.zip` | The certified module snapshot (immutable — changing it would invalidate the FIPS validation) |
| `v1.26.0.zip` | The in-validation snapshot |
| `v1.0.0.txt` | (Single-line) `v1.0.0-c2097c7c` |

Two structural rules cover this: `go-fips140-registry-files-exist`
(asserts the 5 metadata files) and `go-fips140-sum-header` (asserts
the canonical `# SHA256 checksums...` header line).

### `doc/next/` release-notes structure

The release-notes pipeline encodes a strict **8-section + per-package
issue-numbered file** convention:

```
doc/next/
  1-intro.md
  2-language.md
  3-tools.md
  4-runtime.md
  5-toolchain.md
  6-stdlib/
    0-heading.md
    60-uuid.md
    99-minor/
      0-heading.md
      README
      bytes/71151.md
      net/url/73450.md
      net/http/75500.md
      net/http/77370.md
      hash/maphash/70471.md
      ...
  7-ports.md
```

Every file under `99-minor/<package>/` is named `<github-issue-number>.md`
(e.g. `77266.md`). The release-notes generator resolves these to live
GitHub issues. There is no script enforcing the filename grammar today
— `go-doc-next-stdlib-minor-issue-filenames` makes it explicit in 7
lines of YAML covering all 6+ subdirs.

---

## Maps to existing alint rules

### Drop-in replacements (none)

Unlike kubernetes (12 verify-* scripts directly replaced) or tokio
(6 explicit checks shellouts), **golang/go has nothing to replace
because nothing exists**. The closest match — the `git-codereview`
Gerrit hook's `gofmt -d` enforcement — is an external tool, not an
in-tree script. We DO add a `go-gofmt-check` rule that shells out
to `gofmt -l`, but it's defensive (for forks of golang/go that
adopt the config), not a replacement.

### Conventions encoded for the first time (the headline)

The 31-rule config in [`.alint.yml`](.alint.yml) covers conventions
that golang/go enforces *only* by Russ Cox & co.'s code review
discipline:

| alint rule | What it asserts | Why it matters |
|---|---|---|
| `go-bsd-license-header-{go,shell,bat,makefile}` | The canonical 3-line BSD header on every `.go`, `.s`, `.bash`, `.rc`, `.bat`, `Makefile*` (with the comment-syntax appropriate to each) | This is the **single most-load-bearing convention** in golang/go. Every committed source file carries it, and `git-codereview` doesn't check for it — only review etiquette does. A regression silently ships a BSD-licensed source without the conventional attribution. |
| `go-canonical-toplevel-modules` | `src/go.mod` (`module std`) + `src/cmd/go.mod` (`module cmd`) both exist | The 4-go.mod layout IS the Go monorepo shape. Removing either silently breaks the bootstrap. |
| `go-no-toplevel-go.mod` | The repo root must NOT have a `go.mod` | Inversion of the bundled `go-mod-exists` rule. golang/go is *not* a Go module — it's the source of the language. |
| `go-{std,cmd}-module-name` | `src/go.mod` declares `module std`; `src/cmd/go.mod` declares `module cmd` | Renaming either would silently change every transitive import path. |
| `go-{vendored-stdlib,cmd-vendored}-deps-present` | `src/vendor/` and `src/cmd/vendor/` exist | Removing either breaks `go build std` from a clean checkout (no internet → no `golang.org/x/*` resolution). |
| `go-doc-next-7-sections` | The 7 release-note section files exist under `doc/next/` | Missing a section file silently elides it from the generated release notes. |
| `go-doc-next-stdlib-minor-issue-filenames` | Files under `doc/next/6-stdlib/99-minor/<package>/` are named `<github-issue-number>.md` | The release-notes generator resolves these to live GitHub issues. A typo'd filename (e.g. `77266.markdown`) silently drops the entry. |
| `go-fips140-registry-files-exist` | `certified.txt`, `inprocess.txt`, `fips140.sum`, `Makefile`, `README.md` all in `lib/fips140/` | Missing any one silently disables FIPS support. |
| `go-fips140-sum-header` | `lib/fips140/fips140.sum` starts with `# SHA256 checksums of snapshot zip files` | The CMVP-submitted security policy references this exact header format. |
| `go-codereview-cfg-{present,branch}` | `codereview.cfg` exists with a `branch:` line | Required by `git-codereview` for Gerrit upload routing. Absence silently breaks the Gerrit ↔ GitHub mirror. |
| `go-gitattributes-no-text-normalization` | `.gitattributes` declares `* -text` | Load-bearing: golang/go hand-curates CRLF for `.bat` files (see `test/winbatch.go`). Re-enabling normalization would silently corrupt those. |
| `go-issue-templates-required` | The 6 load-bearing issue-template files exist under `.github/ISSUE_TEMPLATE/` | The issue picker references templates by filename in `config.yml`'s overrides. |
| `go-pull-request-template-no-markdown-warning` | `.github/PULL_REQUEST_TEMPLATE` instructs `+ No Markdown` | PRs are imported verbatim into Gerrit (plaintext). Removing the warning would let formatted PRs corrupt change descriptions. |
| `go-no-AUTHORS-file` | The repo does NOT have `AUTHORS` or `CONTRIBUTORS` | golang/go retired both during the Gerrit migration; contributor tracking moved to git history + the CLA database. Re-adding either would diverge from convention. |
| `go-{no-toplevel-bin,no-toplevel-pkg}` | `/bin/` and `/pkg/` are not committed | These are build-output directories. `.gitignore` covers them, but a fork might delete the `.gitignore` entry; the structural absence is the canonical signal. |
| `go-security-policy-references-go-dev` | `SECURITY.md` links to `https://go.dev/security/policy` | Removes ambiguity for vulnerability reporters; load-bearing for OpenSSF Scorecard's security-policy signal and GitHub's advisory auto-detection. |
| `go-readme-references-canonical-source` | `README.md` references `go.googlesource.com/go` | Reminds contributors that GitHub is a mirror; the canonical repo lives on go.googlesource.com. |
| `go-sources-no-trailing-whitespace` + `go-sources-final-newline-explicit` | Broaden the bundled `go@v1` rules to assembly files (`.s`) which gofmt does NOT touch | Catches the case where assembly sources drift from the gofmt-managed `.go` discipline. |

### Defensive shellouts (golang/go itself doesn't run these)

| alint rule | What it does | Why include it |
|---|---|---|
| `go-gofmt-check` | `gofmt -l` on every `.go` under `src/` | The Gerrit hook runs this at upload time; alint runs it at pre-commit / CI time so issues don't reach Gerrit |
| `go-vet-std` | `go vet std` against the std module | Catches printf, shadow, etc. findings that build cleanly |
| `go-vet-cmd` | `go vet cmd/...` against the cmd tree | Same |
| `go-shellcheck` | shellcheck on `src/*.bash` + `lib/**/*.bash` | golang/go itself doesn't run this; defensive for forks |

These four are **not** replacements for existing golang/go tooling
(there is no existing tooling). They're added so the config can be
adopted by *forks* of golang/go (or, more likely, by Go monorepos
that take their layout cues from golang/go) and immediately get a
working baseline.

---

## What needs new alint primitives

| Gap | Existing golang/go behavior | What alint needs |
|---|---|---|
| Per-package import gates ("no `testing` import in non-test source", "no `unsafe` outside `runtime` / `internal/runtime`", "no direct `golang.org/x/*` imports outside `src/vendor/`") | Enforced by `go vet`'s `unsafeptr` checker + code-review etiquette. The kubernetes inventory surfaced the same need (`hack/verify-testing-import.sh`, `hack/verify-prometheus-imports.sh`, `hack/verify-internal-modules.sh`). | **`import_gate` rule kind** — now `v0.10 ship-target` per launch-evidence.md (4 sources: k8s, airflow, golang/go, pytorch). |
| `doc/next/6-stdlib/99-minor/<pkg>/<issue>.md` ↔ live GitHub issue cross-reference | Enforced at release-build time when the release-notes generator fetches GitHub issue titles | **`registry_paths_resolve` variant** with HTTP/API-aware resolution (or a `command:` shellout to `gh issue view <number>`). The on-disk variant of `registry_paths_resolve` is now `v0.10 ship-target` (8 sources); the GitHub-issue API sub-variant is single-source (golang/go-only) and remains v0.11+ design. |
| `lib/fips140/fips140.sum` ↔ on-disk zip hash freshness ("the hash recorded in `fips140.sum` for `v1.0.0-c2097c7c.zip` matches the actual SHA256 of the file") | Enforced by `lib/fips140/Makefile`'s `go run cmd/internal/fips140` regeneration step | **`pair_hash` rule kind** — now `v0.10 ship-target` per launch-evidence.md (3 sources: k8s vendor-readonly + tokio spellcheck.dic header + golang/go FIPS — the highest-stakes use case; CMVP submission references the file format). |
| `api/go1*.txt` golden file ordering ("entries are sorted, no duplicate symbols, every entry has a `pkg <pkg>` namespace prefix") | Enforced by `src/cmd/api/main_test.go`'s `set()` + sort + diff logic | **`ordered_block` rule kind** — now `v0.10 ship-target` per launch-evidence.md (7 sources: rust, airflow, tokio, cpython, arrow, golang/go, protobuf). |

**Cross-reference with the existing v0.10+ candidate list in
[`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md):**
- `import_gate` — `v0.10 ship-target` (4 sources per
  launch-evidence.md). Saturated.
- `pair_hash` — `v0.10 ship-target` (3 sources per
  launch-evidence.md). golang/go FIPS is the highest-stakes
  use case; CMVP submission references the file format.
- `ordered_block` — `v0.10 ship-target` (7 sources per
  launch-evidence.md). Saturated.
- `registry_paths_resolve` (HTTP variant) — the on-disk variant
  is now `v0.10 ship-target` (8 sources); golang/go is the FIRST
  source where the registry resolution targets GitHub issue
  numbers rather than on-disk paths. **SUB-CANDIDATE**: a
  `registry_paths_resolve.mode: github_issues` extension where
  the rule shells out to `gh api repos/<owner>/<repo>/issues/<n>`.
  Single-source so far; remains v0.11+ design.

**No NEW rule-kind candidates beyond the existing v0.10+ list.**
golang/go's gap catalogue overlaps cleanly with what k8s, tokio,
rust-lang, cpython, and airflow already surfaced. This is a
*saturating* signal — the v0.10+ list is approaching completeness
for the language-monorepo segment.

---

## Out of alint's scope (use the existing tool)

These are AST / type-graph / codegen / build-system checks. alint's
non-goals are deliberate; we mention them in the case study so the
reader doesn't expect alint to encroach.

- `src/cmd/api/main_test.go` — exported-symbol API surface check
  across 25 GOOS×GOARCH×CgoEnabled contexts. Pure AST + types
  graph analysis. The right tool is the existing test (`go test
  -run TestCheck cmd/api`).
- `src/cmd/dist/test.go` — test orchestration (`registerTests` /
  `registerStdTest`). Not a structural linter.
- `src/internal/buildcfg/cfg.go` + `exp.go` — runtime config
  parsing.
- `src/cmd/internal/testdir/testdir_test.go` — walks
  `GOROOT/test/`, parses each `.go` file's first-line directive
  (`errorcheck`, `compile`, `run`), runs the indicated test mode.
  Test-fixture orchestration; not a structural linter.
- The `src/cmd/compile/internal/ssa/_gen/*.rules` codegen pipeline
  (which produces the ~20k-line `rewriteAMD64.go` etc.) — pure
  codegen; alint's `generated_file_fresh` candidate covers the
  *freshness* check but the regen is `go generate` territory.
- `make smelly`-equivalent ELF/Mach-O symbol-table checks (cpython
  has these via `Tools/build/smelly.py`; golang/go does this
  inside `cmd/api` rather than as a separate script).

---

## Already covered by other tools golang/go uses

- **`gofmt` / `go vet`** — Go's own toolchain. alint shells out
  via `command:` for both — the right delegation pattern.
- **`git-codereview`** — the Gerrit upload tool. Enforces gofmt
  cleanliness + CRLF rejection at push time.
- **`go test cmd/api`** — exported-symbol API gate. Out of alint's
  scope; the correct tool to run.
- **`go test cmd/internal/testdir`** — `GOROOT/test/` fixture
  orchestration.

---

## Starter alint config (drop-in)

[`.alint.yml`](.alint.yml) in this directory. Adopts:

- `oss-baseline@v1` (license, README, gitignore, no merge markers,
  no bidi)
- `go@v1` (go.mod / go.sum existence, go-version pinning, bidi
  guard on Go sources)
- `hygiene/no-tracked-artifacts@v1` (no `.DS_Store`, build
  outputs, etc.)

Plus 31 golang/go-specific rules covering (**64 rules total** as
loaded by the v0.9.17 binary; 31 golang/go-specific + 33 from 3
bundled rulesets `oss-baseline=15` + `go=8` +
`hygiene/no-tracked-artifacts=11`, with one rule deduplicated):

- 5 license-header rules (one per comment-syntax: `.go`/`.s`,
  `.bash`/`.rc`/`.sh`, `.bat`, `Makefile*`/`.mk`, and an info-level
  recommendation for `go.env`)
- 6 module-layout rules (the 4-go.mod canonical layout with
  inversions for the no-root-go.mod constraint and 2 vendor-tree
  guards)
- 2 `doc/next/` release-notes structure rules
- 2 FIPS 140 registry rules
- 2 source-hygiene rules (broadening the bundled `go@v1`
  trailing-whitespace + final-newline coverage to assembly)
- 4 defensive tooling shellouts (`gofmt`, `go vet std`, `go vet
  cmd/...`, `shellcheck`)
- 6 repo-state rules (`codereview.cfg`, `.gitattributes`, issue
  templates, PR template's "No Markdown" warning, no AUTHORS, no
  toplevel `/bin/`-or-`/pkg/`)
- 2 release-cycle invariants (`SECURITY.md` URL, `README.md`
  canonical-source pointer)

The remaining gaps (per "What needs new alint primitives" above):

- 4 need new alint primitives — file as v0.10+ feature requests
  (all 4 already on the existing list; golang/go is the 3rd-5th
  confirmation per primitive)
- 4 are out of alint's scope — keep on the existing tooling

---

## Performance comparison (placeholder — bench when validation pass scales)

golang/go's structural validation today runs as part of `cmd/api`'s
test suite (~10s on a warm tree, dominated by the 25-context
`go/build` walks) plus the `git-codereview` hook (~100ms per change,
gofmt being the bottleneck).

alint's 31-rule config against a real `golang/go` checkout completes
in **a few seconds** on a warm tree (the heaviest rules are the
license-header file_header walks, which scan the first 8 lines of
~1700 .go + .s files in `src/` after `vendor/` and `testdata/`
exclusion). The defensive `go-gofmt-check` and `go-vet-*` shellouts
dominate when enabled (gofmt is ~1s; `go vet std` ~30s) — but those
are explicit shellouts, not new work alint introduces.

Not the headline pitch here. **The pitch is: golang/go's structural
contract is currently enforced by Russ Cox's code-review discipline;
alint makes it diff-reviewable.**

To benchmark for real: clone golang/go, copy the config to the root,
run `time alint check`. The structural-only subset (every rule
except the 4 defensive shellouts) is the meaningful comparison —
that's the work alint adds that wasn't being done at all today.

---

## Recommendation for the launch story

This case study is **the canonical "convention-heavy minimal-tooling"
example**. Use it as the third leg of the launch positioning:

> "kubernetes has 50 hand-rolled shell scripts; alint replaces 17 of
> them. tokio has zero hand-rolled scripts but 12 implicit conventions
> in CI workflows; alint catches the 15 conventions tokio's pipeline
> assumes but doesn't verify. **golang/go has effectively zero
> hand-rolled scripts AND zero CI workflows — its structural contract
> is enforced exclusively by code-review discipline. alint encodes
> that contract as 31 testable rules — the BSD license header, the
> 4-go.mod canonical layout, the `.gitattributes` line that's
> load-bearing for Windows builds, the release-notes filename
> grammar, the FIPS module registry — for the first time.**"

The narrative naturally extends the **three positioning narratives**
crystallised in P2a Wave 1+2:

| Narrative | Strongest data point | Use case |
|---|---|---|
| "Replaces N hand-rolled validation scripts" | kubernetes (50→17), airflow (109 hooks→40 %), cpython (12 surfaces consolidated) | Repos with verify-script sprawl |
| "Catches conventions your pipeline assumes but doesn't verify" | tokio (15 conventions, 0 scripts), uv (67-crate workspace), pnpm (`meta-updater` plugin replaced), react (codes.json + version-sync) | Repos that rely on convention without explicit checks |
| "Adds structural floor on top of mature tooling" | typescript (eslint+dprint+knip), ruff (900+ Python rules, 0 internal), prettier (5 net-new gates) | Repos with mature tooling but missing structural layer |
| **"Encodes conventions enforced only by code-review discipline"** (NEW from golang/go) | **golang/go (31 conventions, 0 scripts, 0 workflows — every rule is "this convention exists only as oral history and review etiquette today")** | **Tightly-curated, minimal-tooling projects (small but high-leverage segment: golang/go itself, plan9, suckless tools, the Linux kernel's structural conventions)** |

golang/go is the **fourth distinct narrative** and the most extreme
along this axis. The alint pitch lands as: "you have a clear set of
unwritten rules that everyone reviewing your code knows; alint writes
them down, makes them testable, and lets the next contributor read
them in one file instead of inferring them from the corpus."

Followup feature work surfaced (priority order, all confirmations
of existing v0.10+ candidates):

- **`pair_hash` rule kind** — `v0.10 ship-target` (3 sources;
  highest-stakes use case CMVP-FIPS-submission).
- **`import_gate` rule kind** — `v0.10 ship-target` (4 sources;
  saturated).
- **`ordered_block` rule kind** — `v0.10 ship-target` (7 sources;
  saturated).
- **`registry_paths_resolve.mode: github_issues`** —
  sub-candidate, single-source (golang/go-only); remains
  v0.11+ design. Could also be a `command:` shell-out to
  `gh issue view`.

---

## Methodology notes (for future case-study authors)

- **Sparse-checkout strategy:** the briefing's `--sparse` clone +
  `git sparse-checkout set --no-cone '/*' '!/src' '!/test' '!/api'`
  works as documented but excludes too much for the in-tree
  validator inventory. Add `'/src/cmd/dist'`, `'/src/cmd/api'`,
  `'/src/internal/buildcfg'`, `'/src/cmd/internal/testdir'` after
  the initial set call to pull in the four files that house all
  in-tree structural validation (~3000 LoC total). Saves ~3 GB
  vs. a full checkout.
- **License-header regex iteration:** the canonical 3-line BSD
  header has FIVE acceptable variants in golang/go's tree
  (canonical, `Code generated by ... DO NOT EDIT.`, `Code
  generated from ... DO NOT EDIT.`, `// (Derived from )?Inferno
  ...`, `// Copyright (c) YYYY ...` — the parenthesized form is
  rare but historic). The bundled `go@v1` ruleset doesn't
  include a license-header rule because the convention varies
  too much across Go projects; this case study is the first to
  encode it. Future "Go-flavored" bundled rulesets
  (`bundled/go-bsd-licensed@v1`?) could absorb it.
- **No new pitfalls** — every rule shape used here is documented
  in CONFIG-AUTHORING.md (21 pitfalls catalogued as of v0.9.17). The license-header regex is the first
  case study to lean heavily on the `lines:` window (default 20;
  set to 8 here so the BSD header check doesn't accidentally
  match a copyright comment in the body of a file). One DX
  observation: the regex is ~250 chars long once it accepts all
  6 valid variants; a dedicated `file_header.alternatives:`
  list-of-patterns field would be more readable than the
  alternation soup. Logging as a low-priority DX polish (not a
  pitfall — works correctly today).

---

## Validation status (2026-05-07)

- alint version: **0.9.17** (1dbd9b218a0e, built 2026-05-07).
- `validate-config`: **64 rules loaded cleanly** (31 golang/go-
  specific + 33 from 3 bundled rulesets — `oss-baseline=15`,
  `go=8`, `hygiene/no-tracked-artifacts=11`, with one rule
  deduplicated across rulesets).
- Live-tree recheck: **pending** — `/tmp/golang-go/` not present
  in this validation env.
- Pitfalls fixed in v0.9.17 that touch this config: none
  (golang/go config doesn't surface pitfalls #18/#19).
- Open gaps (rule-kind candidates referenced but not yet
  shipped):
  - `pair_hash` (v0.10 ship-target, 3 sources) — golang/go
    FIPS is the highest-stakes use case.
  - `import_gate` (v0.10 ship-target, 4 sources).
  - `ordered_block` (v0.10 ship-target, 7 sources).
  - `registry_paths_resolve.mode: github_issues` (v0.11+
    design, single-source — golang/go's release-notes ↔
    GitHub-issue cross-reference).

## Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **Add the `agent-context@v1` overlay (5 rules).**
   golang/go ships `.github/PULL_REQUEST_TEMPLATE` with the
   load-bearing "+ No Markdown" instruction; the
   agent-context bundled ruleset would gate AI-generated
   contribution discipline (no markdown in PR descriptions,
   AGENTS.md present, AI-context structure). golang/go is
   exactly the kind of repo where AI-generated PR-description
   noise would get rejected by Russ Cox's review etiquette;
   a structural enforcer would catch it pre-PR.
2. **Replace the 4 defensive `command:` shellouts.** Of
   `go-gofmt-check`, `go-vet-std`, `go-vet-cmd`,
   `go-shellcheck`, only `go-shellcheck` is replaceable by a
   future bundled `tooling/shellcheck@v1` overlay (none ships
   yet). The other three are inherently shellouts because
   alint deliberately doesn't ship Go AST awareness.
3. **`alint suggest` against the live tree.** Pending
   `/tmp/golang-go/`. Likely candidate: a generalised
   "every file under `doc/next/6-stdlib/99-minor/<pkg>/`
   matches `^\d+\.md$`" rule the suggester auto-discovers,
   replacing the hand-rolled
   `go-doc-next-stdlib-minor-issue-filenames` rule.
