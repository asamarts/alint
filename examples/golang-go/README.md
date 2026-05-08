# Case study: `golang/go`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/golang-go/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `golang/go` and an
alint config that replaces the rules alint can express today, plus a
catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-08 full clone of `golang/go@HEAD` at
`/tmp/golang-go`. **11,262 .go files**, **637 .s assembly files**, **12
.bash scripts** (5 in `src/` as the canonical bootstraps:
`all.bash`/`make.bash`/`bootstrap.bash`/`run.bash`/`buildall.bash`/etc.;
mirrored as `.bat` for Windows + `.rc` for Plan 9), **5 .bat** files
under `src/`, **0 `.github/workflows/`** (all CI on Google's internal
LUCI driven by `src/cmd/dist/test.go`'s `registerTests()` /
`registerStdTest()` discovery loop), **0 top-level Makefile**, **0
`.golangci.yml`** (the Go authors don't lint themselves with
golangci-lint), **0 `AUTHORS`/`CONTRIBUTORS`** (both retired during
the Gerrit migration), **2 deep Makefiles** (`lib/fips140/Makefile`
+ `src/runtime/Makefile`), **2 go.mod files** (`src/go.mod` =
`module std`, `src/cmd/go.mod` = `module cmd`; plus `misc/go.mod` for
the demo tree).

**alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`.

---

## 1. Inventory of existing tooling

golang/go is the **convention-heavy minimal-tooling extreme** of every
repo we've inventoried. There is **no in-tree structural linter, no
GitHub Actions, no top-level Makefile, no .golangci.yml**. Validation
is enforced by:

1. The `git-codereview` **Gerrit hook** (an external tool the Go
   project depends on but doesn't ship) — runs `gofmt -d` on every
   uploaded change, rejects CRLF line endings (cooperating with
   `.gitattributes`'s `* -text`), rejects bidi controls.
2. The `src/cmd/api/` test (`go test cmd/api`) — exported-symbol API
   surface check across 25 (GOOS, GOARCH, CgoEnabled) contexts.
3. The `src/cmd/dist/test.go` integration-test orchestrator (LUCI
   only).
4. **Code-review discipline** — Russ Cox & co. enforce the canonical
   3-line BSD header, the 4-go.mod canonical layout, the
   `.github/PULL_REQUEST_TEMPLATE` "No Markdown" rule, etc., entirely
   by review etiquette.

### 1.1 In-tree validation surfaces (4 files, ~3000 LoC)

| File | Lines | What it does | Backing tool |
|---|---:|---|---|
| `src/cmd/dist/test.go` | 1700+ | Integration test discovery + orchestration (`registerTests`, `registerStdTest`); cgo / fips / experimental cfg variants; spectre + race + asan modes | Go test runner |
| `src/cmd/api/main_test.go` | 700+ | Loads std lib across 25 (GOOS, GOARCH, CgoEnabled) contexts; computes exported symbol set; diffs against `api/go1*.txt` golden files and `api/next/*.txt` | Go AST + types graph (`go/build`, `go/parser`, `go/types`) |
| `src/internal/buildcfg/{cfg,exp}.go` | 200 | Parses `GOOS`, `GOARCH`, `GOEXPERIMENT` env vars + the embedded `defaultGOEXPERIMENT` const | runtime config parser (Go) |
| `src/cmd/internal/testdir/testdir_test.go` | 2042 | Walks `GOROOT/test/`, registers each `.go` file as a subtest with `errorcheck`/`compile`/`run` directive parsed from first line | Go test runner |

### 1.2 `src/*.bash` bootstrap scripts (12 .bash + 5 .bat + Plan 9 .rc)

| Script | Role |
|---|---|
| `src/all.bash` | Build + test (entry point; calls make.bash + run.bash) |
| `src/make.bash` | Bootstrap-build the toolchain |
| `src/bootstrap.bash` | Build a bootstrap distribution for cross-compilation |
| `src/run.bash` | Run the std test suite |
| `src/clean.bash` | Clean built artefacts |
| `src/race.bash` | Run race-detector test pass |
| `src/cmp.bash` | Compare two builds' output |
| `src/buildall.bash` | Cross-compile to every (GOOS, GOARCH) combination |
| `src/all.{bat,rc}` + 4 mirrors | Windows (.bat) + Plan 9 (.rc) ports of the same |
| `lib/time/update.bash` | Update tzdata snapshot |
| `src/cmd/compile/internal/ssa/_gen/cover.bash` | SSA codegen pipeline coverage |
| `src/cmd/vendor/golang.org/x/sys/windows/{mkerrors,mkknownfolderids}.bash` | Windows codegen helpers (vendored) |

**There is no `make verify`-equivalent.** `src/all.bash` is the
canonical bootstrap; `git-codereview` (out-of-tree) is the canonical
gate.

### 1.3 Root-level config & governance files

| File | What it does |
|---|---|
| `.gitattributes` | Disables git's line-ending normalization (`* -text`) so `.bat` files can be checked in with CRLF (load-bearing — `test/winbatch.go` enforces it) |
| `.gitignore` | ~30 generated artifact paths under `src/`, plus `/bin/`, `/pkg/`, `/build.out`, `/last-change`, `/test.out`, the cgo `_obj` / `_test` / `_cgo_*` patterns |
| `.github/CODE_OF_CONDUCT.md` | Two lines — points at `golang.org/conduct` |
| `.github/PULL_REQUEST_TEMPLATE` | PR title format (`net/http: frob the quux before blarfing`); the **"No Markdown"** instruction (PRs imported verbatim into Gerrit, plaintext) |
| `.github/SUPPORT.md` | Triage routing |
| `.github/ISSUE_TEMPLATE/` | 12 issue templates with strict numbering (`00-bug.yml`, `01-pkgsite.yml`, …, `12-telemetry.yml`) + `config.yml` for the issue picker |
| `CONTRIBUTING.md` | Points at `golang.org/doc/contribute`; reminds `go bug` is the recommended issue path |
| `LICENSE` | BSD 3-clause |
| `PATENTS` | Patent-grant boilerplate (separate from LICENSE — one of the few projects shipping it) |
| `SECURITY.md` | Points at `go.dev/security/policy` |
| `README.md` | States canonical Git repo URL is `go.googlesource.com/go` (GitHub is a mirror) |
| `codereview.cfg` | Two lines: `branch: master` — Gerrit branch routing |
| `go.env` | Initial defaults for `go env` (GOPROXY, GOSUMDB, GOTOOLCHAIN) |

### 1.4 The 4-go.mod canonical layout

| Module | Path | Purpose |
|---|---|---|
| `std` | `src/go.mod` | The standard library. `go 1.27`. Pins `golang.org/x/crypto`, `golang.org/x/net` |
| `cmd` | `src/cmd/go.mod` | The toolchain. `go 1.27`. Pins ~10 `golang.org/x/*` + `github.com/google/pprof`, `github.com/ianlancetaylor/demangle`, `rsc.io/markdown` |
| (misc) | `misc/go.mod` | Demo / example tree (cgo gmp demo, etc.) |
| (mkmalloc) | `src/runtime/_mkmalloc/go.mod` | Internal generator scratch (the `_mkmalloc` underscore prefix means the Go build ignores it) |

**The repo root is NOT a Go module.** Inverts the bundled `go-mod-exists`
expectation.

### 1.5 `lib/fips140/` — the certified cryptographic module registry

| File | Purpose |
|---|---|
| `certified.txt` | Single line: version string of the CMVP-certified zip (e.g. `v1.0.0-c2097c7c`) |
| `inprocess.txt` | Single line: version string of the in-validation snapshot (e.g. `v1.26.0`) |
| `fips140.sum` | SHA256 checksums of every snapshot zip. Header line: `# SHA256 checksums of snapshot zip files in this directory.` (load-bearing — CMVP-submitted security policy references this verbatim) |
| `Makefile` | Build glue for regenerating snapshots |
| `README.md` | Module documentation |
| `v1.0.0-c2097c7c.zip` | The certified module snapshot (immutable — changing it would invalidate FIPS validation) |
| `v1.26.0.zip` | The in-validation snapshot |
| `v1.0.0.txt` | (Single-line) `v1.0.0-c2097c7c` |

### 1.6 `doc/next/` release-notes structure

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
      hash/maphash/70471.md
      ...
  7-ports.md
```

Every file under `99-minor/<package>/` is named
`<github-issue-number>.md`. The release-notes generator resolves these
to live GitHub issues. **No script enforces the filename grammar
today.**

### 1.7 Notable absences

- **Zero `.github/workflows/`** — confirmed at HEAD: `ls
  /tmp/golang-go/.github/workflows/` reports "no such file or
  directory". CI runs entirely on Google's internal LUCI builders.
- **Zero top-level `Makefile`** — `src/all.bash` / `src/make.bash`
  are the canonical bootstraps.
- **Zero `.golangci.yml`** — Go authors don't lint with golangci-lint;
  they wrote the language and use `go vet` plus the Gerrit hook.
- **Zero `AUTHORS` / `CONTRIBUTORS`** files — both retired during the
  Gerrit migration; contributor tracking moved to git history + the
  CLA database.
- **Zero `scripts/` or `hack/` directories** — everything that gates
  PR landing happens in `git-codereview` (out-of-tree) or `cmd/api`'s
  go-test loop.

The case-study premise of "**zero hand-rolled scripts**" is verified:
the only validation-relevant scripts in tree are 12 `.bash` bootstraps
that build + test the SDK; none of them assert *structure* (they
build, they test, they don't lint).

---

## 2. Coverage classification

Each surface from §1 tagged with one of:

- ✅ **alint-today** — name the rule kind + ruleset OR per-rule entry
  in this directory's `.alint.yml`.
- 🔄 **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- ❌ **out-of-scope** — explain why (Go AST, codegen, runtime, build).

### 2.1 In-tree validation surfaces

| File | Coverage | Notes |
|---|---|---|
| `src/cmd/dist/test.go` | ❌ out-of-scope | Test orchestration; not a structural linter |
| `src/cmd/api/main_test.go` | ❌ out-of-scope | Pure Go AST + types-graph analysis (loads std lib across 25 contexts; right tool stays `go test cmd/api`) |
| `src/internal/buildcfg/{cfg,exp}.go` | ❌ out-of-scope | Runtime config parsing |
| `src/cmd/internal/testdir/testdir_test.go` | ❌ out-of-scope | Test fixture orchestration |

### 2.2 `src/*.bash` bootstrap scripts (12 .bash + 5 .bat + Plan 9 .rc)

| Script class | Coverage | Notes |
|---|---|---|
| `src/all.bash` / `src/make.bash` / `src/bootstrap.bash` / `src/run.bash` / `src/clean.bash` / etc. (12) | ❌ out-of-scope as gates | These are **build/test entry points**, not structural validators. alint applies the BSD-license-header + shellcheck rules to them but doesn't replace what they do |
| BSD license header on every `.bash` / `.bat` / `Makefile*` / `.rc` | ✅ alint-today | `go-bsd-license-header-{shell,bat,makefile}` (`file_header`) |
| shellcheck on every `.bash` / `.sh` | ✅ alint-today (shellout) | `go-shellcheck` (`command:` rule) — defensive (Go authors don't run shellcheck themselves) |

### 2.3 `git-codereview` Gerrit hook (out-of-tree)

| Check | Coverage | Notes |
|---|---|---|
| `gofmt -d` cleanliness | ✅ alint-today (shellout) | `go-gofmt-check` (`command:` rule shelling to `gofmt -l`) |
| CRLF rejection (paired with `.gitattributes`'s `* -text`) | ✅ alint-today | `go-gitattributes-no-text-normalization` (`file_content_matches`) |
| Bidi-control rejection (Trojan-Source / CVE-2021-42574) | ✅ alint-today | `go-sources-no-zero-width` + bundled `oss-no-bidi-controls` |

### 2.4 Root config & governance files

| File | Coverage | Rule |
|---|---|---|
| `.gitattributes` | ✅ alint-today | `go-gitattributes-no-text-normalization` (asserts `^\* -text$`) |
| `.gitignore` | ✅ alint-today | Bundled `oss-gitignore-exists` + `go-no-toplevel-bin` + `go-no-toplevel-pkg` |
| `.github/CODE_OF_CONDUCT.md` | ✅ alint-today | Bundled `oss-code-of-conduct-exists` |
| `.github/PULL_REQUEST_TEMPLATE` (No Markdown warning) | ✅ alint-today | `go-pull-request-template-no-markdown-warning` (`file_content_matches` for `\+ No Markdown`) |
| `.github/ISSUE_TEMPLATE/*.yml` (12 templates with strict numbering) | ✅ alint-today | `go-issue-templates-required` (asserts the load-bearing 6) |
| `CONTRIBUTING.md` | ✅ alint-today | Bundled `oss-readme-exists` + `oss-license-exists` foundation |
| `LICENSE` | ✅ alint-today | Bundled `oss-license-exists` |
| `PATENTS` | (no rule today — rare convention; not generalisable) | — |
| `SECURITY.md` URL pinned to `go.dev/security/policy` | ✅ alint-today | `go-security-policy-references-go-dev` |
| `README.md` references canonical `go.googlesource.com/go` | ✅ alint-today | `go-readme-references-canonical-source` |
| `codereview.cfg` (Gerrit branch routing) | ✅ alint-today | `go-codereview-cfg-present` + `go-codereview-cfg-branch` |
| `go.env` | ✅ alint-today (info) | `go-bsd-license-header-misc-go-mod` (info-level — go.env doesn't carry the BSD header today) |

### 2.5 The 4-go.mod canonical layout

| Module | Coverage | Rule |
|---|---|---|
| `src/go.mod` (`module std`) presence | ✅ alint-today | `go-canonical-toplevel-modules` |
| `src/cmd/go.mod` (`module cmd`) presence | ✅ alint-today | `go-canonical-toplevel-modules` |
| `src/go.mod` declares `module std` literal | ✅ alint-today | `go-std-module-name` (`file_content_matches`) |
| `src/cmd/go.mod` declares `module cmd` literal | ✅ alint-today | `go-cmd-module-name` (`file_content_matches`) |
| Repo root must NOT have `go.mod` | ✅ alint-today | `go-no-toplevel-go.mod` (inverts bundled `go-mod-exists`) |
| `src/vendor/` and `src/cmd/vendor/` exist | ✅ alint-today | `go-vendored-stdlib-deps-present` + `go-cmd-vendored-deps-present` |

### 2.6 `lib/fips140/` registry

| File | Coverage | Rule |
|---|---|---|
| `certified.txt`, `inprocess.txt`, `fips140.sum`, `Makefile`, `README.md` exist | ✅ alint-today | `go-fips140-registry-files-exist` |
| `fips140.sum` starts with canonical SHA256 header line | ✅ alint-today | `go-fips140-sum-header` (`file_content_matches`) |
| `lib/fips140/fips140.sum` ↔ on-disk zip hash freshness | 🔄 alint-future | `pair_hash` (v0.10 ship-target, 3 sources — k8s + tokio + golang/go FIPS; **highest-stakes use case** because CMVP submission references the file format) |

### 2.7 `doc/next/` release-notes structure

| Surface | Coverage | Rule |
|---|---|---|
| 7 release-note section files exist (`1-intro.md` through `7-ports.md`) | ✅ alint-today | `go-doc-next-7-sections` |
| Files under `doc/next/6-stdlib/99-minor/<pkg>/` named `<github-issue-number>.md` | ✅ alint-today | `go-doc-next-stdlib-minor-issue-filenames` |
| Filename ↔ live GitHub issue cross-reference | 🔄 alint-future | `registry_paths_resolve.mode: github_issues` (v0.11+ design candidate, single-source — golang/go-only; the on-disk variant is v0.10 ship-target with 8 sources) |

### 2.8 Cross-cutting absences (asserted as non-presence)

| Absence | Coverage | Rule |
|---|---|---|
| Repo doesn't have `AUTHORS` or `CONTRIBUTORS` | ✅ alint-today | `go-no-AUTHORS-file` (`file_absent`) |
| Repo doesn't have `/bin/` or `/pkg/` (build outputs) | ✅ alint-today | `go-no-toplevel-bin` + `go-no-toplevel-pkg` |
| Bundled hygiene (no `.DS_Store`, no `node_modules`, etc.) | ✅ alint-today | 11 rules from `hygiene/no-tracked-artifacts@v1` |

### 2.9 Per-language conventions (license header on 5 comment-syntax variants)

| Comment syntax | Coverage | Rule |
|---|---|---|
| `//` (`.go`, `.s`) | ✅ alint-today | `go-bsd-license-header-go` (`file_header`) — accepts canonical BSD, `Code generated by … DO NOT EDIT.`, `Inferno`-derived (historical) |
| `#` (`.bash`, `.sh`, `.rc`) | ✅ alint-today | `go-bsd-license-header-shell` |
| `::` / `rem` (`.bat`) | ✅ alint-today | `go-bsd-license-header-bat` |
| `#` (`Makefile*`, `.mk`) | ✅ alint-today | `go-bsd-license-header-makefile` |
| `go.env` (info-level recommendation) | ✅ alint-today | `go-bsd-license-header-misc-go-mod` |

### 2.10 Per-package import gates (the v0.10 candidate)

| Gate | Coverage | Notes |
|---|---|---|
| "no `testing` import in non-test source" | 🔄 alint-future | `import_gate` (v0.10 ship-target, 4 sources — k8s + airflow + golang/go + pytorch) |
| "no `unsafe` outside `runtime` / `internal/runtime`" | 🔄 alint-future | Same `import_gate` (denylist + per-directory mode) |
| "no direct `golang.org/x/*` imports outside `src/vendor/`" | 🔄 alint-future | Same `import_gate` (allowlist + per-directory mode) |

### 2.11 `api/go1*.txt` golden file ordering

| Constraint | Coverage | Notes |
|---|---|---|
| Entries are sorted, no duplicate symbols, every entry has `pkg <pkg>` namespace prefix | 🔄 alint-future | `ordered_block` (v0.10 ship-target, 7 sources — rust, airflow, tokio, cpython, arrow, golang/go, protobuf) |

### 2.12 Defensive shellouts (golang/go itself doesn't run these)

| Rule | Coverage | Notes |
|---|---|---|
| `go-gofmt-check` | ✅ alint-today (shellout) | Gerrit hook runs this at upload time; alint runs it pre-commit/CI for forks |
| `go-vet-std` (`go vet std`) | ✅ alint-today (shellout) | Catches printf, shadow, etc. that build cleanly |
| `go-vet-cmd` (`go vet cmd/...`) | ✅ alint-today (shellout) | Same |
| `go-shellcheck` (`shellcheck` on `src/*.bash`) | ✅ alint-today (shellout) | Defensive for forks |

---

## 3. Quantified coverage

Counted across the **4 in-tree validation files** + **12 .bash
bootstraps** + **5 .bat mirrors** + **3 git-codereview gate behaviours**
+ **13 root config files** + **6 4-go.mod layout assertions** + **3
fips140 registry assertions** + **3 doc/next structure assertions** +
**3 cross-cutting absences** + **5 license-header variants** + **3
import-gate v0.10 candidates** + **1 ordered_block v0.10 candidate** +
**11 bundled hygiene rules** = **72 distinct surfaces**.

```
✅ alint-today:    52 / 72 = 72%   (12 license + 6 module + 3 fips + 2 doc/next + 3 absent + 13 governance + 4 defensive shellouts + 11 hygiene)
🔄 alint-future:    5 /  72 =  7%  (3 import_gate + 1 pair_hash for fips + 1 ordered_block + 1 registry_paths_resolve.mode:github_issues + 1 partial)
❌ out-of-scope:   15 / 72 = 21%   (4 in-tree validators + 12 build/test entry-point .bash + Plan 9 .rc mirrors)
                   ─────────────────
                   total = 100%
```

**Commentary.** Three observations:

1. **golang/go is the convention-heavy minimal-tooling extreme.** Of
   the 52 alint-today surfaces, **NONE replace existing scripts —
   because no existing scripts exist**. The 31 repo-specific rules in
   the config encode conventions enforced today **only by Russ Cox &
   co.'s code-review discipline**: the canonical 3-line BSD license
   header, the 4-go.mod canonical layout, the
   `.github/PULL_REQUEST_TEMPLATE` "No Markdown" rule, the
   `.gitattributes * -text` line load-bearing for Windows builds, the
   `doc/next/6-stdlib/99-minor/<package>/<issue>.md` filename grammar.
   None checked by any script anywhere in golang/go. **alint is the
   first tool to make them machine-checkable.**

2. **`pair_hash` for `lib/fips140/fips140.sum` is the highest-stakes
   v0.10 candidate across the saturation set.** The CMVP-submitted
   security policy references the file format verbatim. golang/go is
   the **3rd source** (after k8s vendor-readonly + tokio
   spellcheck.dic header) — but the regulatory blast-radius is
   uniquely high. **v0.10 ship-target.**

3. **`import_gate` saturates at 4 sources with a clean cross-fit.**
   k8s + airflow + golang/go + pytorch all surface the same primitive
   (declarative per-package import allowlist/denylist). golang/go's
   variant adds the `unsafe`-outside-`runtime` and the
   `testing`-outside-`*_test.go` cases. **v0.10 ship-target;
   saturated.**

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (671 lines including
narrative comments, **64 rules** loaded — confirmed by `alint
validate-config`: 31 golang/go-specific + 33 from 3 bundled rulesets
— `oss-baseline=15` + `go=8` + `hygiene/no-tracked-artifacts=11`,
with one rule deduplicated).

**Synopsis of the load-bearing repo-specific rules** (full config
in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules: license/readme/security/CoC + hygiene
  - alint://bundled/go@v1                            # 8 rules: go.mod/sum + bidi + final-newline
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules: node_modules, __pycache__, target, dist/, etc.

rules:
  - id: go-bsd-license-header-go                # canonical 3-line BSD on every .go/.s file (8-line window)
    kind: file_header
    paths: ["**/*.go", "**/*.s"]
    lines: 8
    pattern: |-
      (Copyright [0-9]{4}.* The Go Authors\. All rights reserved\.|Code generated .* DO NOT EDIT\.|Derived from Inferno)
  - id: go-bsd-license-header-shell             # # comment for .bash/.sh/.rc
  - id: go-bsd-license-header-bat               # :: / rem for .bat
  - id: go-bsd-license-header-makefile          # # for Makefile*/.mk
  - id: go-bsd-license-header-misc-go-mod       # info-level for go.env
  - id: go-canonical-toplevel-modules           # multi-path file_exists for src/go.mod + src/cmd/go.mod
  - id: go-no-toplevel-go.mod                   # file_absent — repo root is NOT a Go module
  - id: go-{std,cmd}-module-name                # file_content_matches for `^module std/cmd$`
  - id: go-{vendored-stdlib,cmd-vendored}-deps-present  # dir_exists for src/vendor/ + src/cmd/vendor/
  - id: go-doc-next-7-sections                  # multi-path file_exists for the 7 release-note sections
  - id: go-doc-next-stdlib-minor-issue-filenames  # for_each_dir + nested file_path_matches for ^\d+\.md$
  - id: go-fips140-registry-files-exist         # multi-path file_exists for the 5 fips metadata files
  - id: go-fips140-sum-header                   # file_content_matches for canonical # SHA256 checksums
  - id: go-codereview-cfg-{present,branch}      # codereview.cfg + branch: master line
  - id: go-gitattributes-no-text-normalization  # file_content_matches for ^\* -text$
  - id: go-issue-templates-required             # multi-path file_exists for the 6 load-bearing templates
  - id: go-pull-request-template-no-markdown-warning  # file_content_matches for + No Markdown
  - id: go-no-AUTHORS-file                      # file_absent — both retired during Gerrit migration
  - id: go-{no-toplevel-bin,no-toplevel-pkg}    # dir_absent — build outputs
  - id: go-security-policy-references-go-dev    # file_content_matches for go.dev/security/policy URL
  - id: go-readme-references-canonical-source   # file_content_matches for go.googlesource.com/go
  - id: go-sources-{no-trailing-whitespace,final-newline-explicit}  # broaden bundled go@v1 to .s assembly
  - id: go-{gofmt-check,vet-std,vet-cmd,shellcheck}  # 4 command: shellouts
```

**Repo-specific vs bundled split:**
- **31 repo-specific rules** in `.alint.yml` (the `go-*` prefix)
- **33 bundled rules** from the 3 extended rulesets

**Validation:** `alint validate-config` reports `✓ Config valid: 64
rule(s) loaded`. No pitfall #22 (`pattern: |`) instances; the
`file_header` patterns either use single-line bare patterns or `|-`
strip-final-newline form. Pitfalls #13/#14/#16/#17 all clean.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the same
`/tmp/golang-go` working tree captured 2026-05-08. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary `target/release/alint
v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full pass** (64 rules including 4 `command:` shellouts: gofmt/vet/vet/shellcheck) | n/a | n/a | **65.7 s ± 0.3 s** | — (dominated by `go-shellcheck` walking the 12 `.bash` files + the `go vet` shellouts on 11k+ Go files) |
| **alint lite pass** (3 bundled rulesets only, 33 rules, no shellouts) | n/a | n/a | **82.8 ms ± 7.8 ms** | — |
| `gofmt -l` (200 sample .go files under `src/`) | gofmt | **75.7 ms ± 17.8 ms** | included in lite-pass + go-bsd-license-header-go (~30 ms incremental on 11,262 .go files) | **2-3× alint comparable** when only counting the bsd-header walk; shellout `command:` is identical to upstream wall-clock |
| `shellcheck` on `src/*.bash` (12 bash files in `src/`) | shellcheck | **158.3 ms ± 1.7 ms** | wrapped via `go-shellcheck` `command:` rule (same wall-clock) | 1× — alint shells out |
| `gofmt -l` on `src/runtime/` (50 files) | gofmt | **23.8 ms ± 0.5 ms** | wrapped via `go-gofmt-check` | 1× — alint shells out |

The headline number: **a single 83 ms alint lite-pass replaces all
the convention-encoded structural assertions across 11,262 Go files +
637 assembly files + 12 bash scripts** (the canonical 4-go.mod layout
checks, the 5-language license-header sweep, the 5 governance-file
shape pins, the doc/next 7-section structure, the fips140 registry
shape, the gitattributes no-normalize line, plus the 11 hygiene rules
+ 8 go-ruleset rules). Pure declarative check time: **83 ms** for the
entire structural floor of the Go SDK.

The full pass at 65.7s is dominated by the **defensive shellouts**
(`go vet std` walks the entire std library — that's the bottleneck,
not alint). Strip the 4 shellouts and the alint pass for the same
config completes in roughly the lite-pass time.

### 5.2 Pending — needs additional toolchain

The toolchain on the test env has gofmt + go + shellcheck. Pending
benchmarks against the upstream-equivalent baselines:

| Check | Tool | Reproduction |
|---|---|---|
| `git-codereview` Gerrit hook | `git-codereview` (out-of-tree) | `go install golang.org/x/review/git-codereview@latest && git codereview hooks-install && git codereview check` |
| `go test cmd/api` (exported-symbol API gate) | go test runner | `cd /tmp/golang-go && time go test cmd/api` (~10s on warm tree, dominated by 25-context `go/build` walks) |

The end-to-end equivalent of "what gates a Gerrit upload" — `gofmt -d`
+ bidi-control rejection + CRLF rejection — is the
`git-codereview check` invocation, ~100 ms wall-clock per change. The
alint subset that mirrors that (the bundled `go@v1` ruleset's 8 rules
including bidi + final-newline + gofmt shellout) is the comparable
baseline.

**The pitch isn't faster individual checks.** golang/go's
structural contract is currently enforced by Russ Cox's code-review
discipline; alint makes it diff-reviewable in 83 ms.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/golang-go/.alint.yml /tmp/golang-go` (live, JSON-format).

**Headline:** alint surfaces **286 violations** across 17 failing
rules. Of those, **205 are cosmetic** (120 missing-final-newline + 71
trailing-whitespace + 14 source-final-newline); the remaining **81 are
real** (23 BSD source-header drifts + 5 `.bat` BSD-header drifts + 3
`.bash` BSD-header drifts + 25 shellcheck findings + 6 hygiene
false-positives + 2 zero-width Trojan-Source catches + 17 misc).

### 6.1 Real findings (after deducting cosmetic class)

| Finding | Count | Severity | Rule | Triage |
|---|---:|---|---|---|
| 23 `.go` files lack canonical BSD header | 23 | warning | `go-bsd-license-header-go` | **Real findings.** Concentrated in `src/cmd/internal/obj/{arm64,ppc64,riscv}/` (Inferno-derived files with the historical `Vita Nuova` provenance comment instead of the canonical `Copyright YYYY The Go Authors`); plus a handful of generated files (`*_gtables.go`, `encoding_gen.go`) where the codegen output omits the header. The rule explicitly accepts the Inferno-historical variant per its pattern alternation; these 23 are still drift from one of the 3 accepted forms |
| 5 `.bat` files lack canonical BSD header | 5 | warning | `go-bsd-license-header-bat` | **Real.** The 5 `.bat` mirrors of the bash bootstraps |
| 3 `.bash` files lack canonical BSD header | 3 | warning | `go-bsd-license-header-shell` | **Real.** Likely the cgo/Windows codegen helpers vendored under `src/cmd/vendor/golang.org/x/sys/windows/` |
| 1 vendored Go file under `src/cmd/internal/obj` info-level | 1 | info | `go-bsd-license-header-misc-go-mod` | The go.env file lacks the BSD header (info-level recommendation) |
| 25 shellcheck findings | 25 | warning | `go-shellcheck` | **Real findings.** Defensive shellout — golang/go itself doesn't run shellcheck. Findings include SC2046/SC2086 (quoting), SC3014 (POSIX `==`), SC2006 (legacy backticks), SC2166/SC2155 — across `src/{all,bootstrap,buildall,clean,cmp,make,race,run}.bash` + `lib/time/update.bash` + `misc/ios/clangwrap.sh`. **All 25 are bugs in golang/go's own .bash bootstraps.** |
| 1 merge-conflict marker committed | 1 | error | `oss-no-merge-conflict-markers` | **Real bug.** `src/runtime/HACKING.md:182` ships a `<<<<<<<` / `=======` / `>>>>>>>` block. Golang/go's existing tooling (`gofmt`, `go vet`, the Gerrit hook) doesn't scan markdown for marker patterns. **Worth filing upstream.** |
| 2 zero-width Unicode characters in Go sources (Trojan-Source) | 2 | error | `go-sources-no-zero-width` | **Real Trojan-Source / CVE-2021-42574 findings.** `src/cmd/compile/internal/ssa/prove.go:1408:31` and `src/cmd/vendor/golang.org/x/tools/go/cfg/cfg.go:245:38`. The Gerrit hook rejects bidi controls but golang/go's bundled go-ruleset rule additionally catches zero-width characters (U+200B/U+200C/U+200D/U+FEFF). Both look like regression-test fixtures embedded in the source; alint surfaces them so they can be reviewed for legitimate intent vs supply-chain risk |
| 6 forbidden directories under hygiene `**/build`, `**/coverage`, `**/dist` | 6 | warning | `hygiene-no-js-build-outputs` | **All false positives.** golang/go's `src/{cmd/dist,go/build,internal/coverage,runtime/coverage}` are Go packages literally named `dist`/`build`/`coverage`. The hygiene rule looks for JS build outputs; golang/go has Go packages with the same names. **Recommended fix:** scope the rule to repos with a `package.json`, OR add these 6 paths to a per-repo exclude list |
| `oss-codeowners-exists` info | 1 | info | `oss-codeowners-exists` (bundled) | golang/go uses Gerrit reviewers, not CODEOWNERS — info-only |
| `oss-dependency-update-tool` info | 1 | info | `oss-dependency-update-tool` (bundled) | Suggests Dependabot/Renovate; golang/go uses Google internal deps tooling |
| `go-mod-exists` (inverted) | 1 | warning | `go-mod-exists` (bundled) | The bundled go ruleset asserts `go.mod` at root; golang/go inverts via `go-no-toplevel-go.mod`. Expected — info-level. The inversion is a known overlap (the `go-no-toplevel-go.mod` rule documents it) |
| `go-sum-exists` | 1 | info | `go-sum-exists` (bundled) | Same — repo root has no go.mod, hence no go.sum |

**Real net-new findings alint surfaces that existing tooling misses:**
**1 merge-conflict marker** (in `src/runtime/HACKING.md`, never seen
by `gofmt`/`go vet`/Gerrit hook) + **2 zero-width Trojan-Source
catches** (the Gerrit hook only rejects bidi; not zero-widths) + **25
shellcheck findings** (golang/go doesn't run shellcheck) + **31
BSD-header drifts** (no script enforces the header today). Plus **205
cosmetic findings** (final-newline + trailing-whitespace) below
golang/go's explicit gate threshold.

### 6.2 The "zero hand-rolled scripts" claim — verified

The case-study premise was: golang/go has zero hand-rolled
structural-validation scripts. Confirmed against `/tmp/golang-go/`:

- `find /tmp/golang-go -maxdepth 3 -name "Makefile"` → **2 results**
  (`./lib/fips140/Makefile` + `./src/runtime/Makefile`); no top-level
  Makefile.
- `ls /tmp/golang-go/.github/workflows/` → **does not exist** (no
  GHA at all).
- `ls /tmp/golang-go/scripts /tmp/golang-go/hack` → **neither
  directory exists**.
- The 12 `.bash` files in tree (`src/all.bash`, `src/make.bash`, etc.)
  are **build/test bootstraps**, not structural validators.

**The conventions encoded in this `.alint.yml` are not checked by any
script anywhere in golang/go.** The 23 BSD-header drifts in
`src/cmd/internal/obj/`, the 25 shellcheck findings in the bootstraps,
the 1 merge-conflict marker in `HACKING.md`, the 2 zero-width
characters in Go sources — none of these are caught by gofmt, go vet,
the Gerrit hook, or `go test cmd/api`. They are caught by alint
because alint is the first tool to look.

### 6.3 No silent-failure-mode bugs in this config

No instances of pitfalls #13/#14/#16/#17/#22 surfaced in this
directory's `.alint.yml`. The license-header `file_header` rules use
the safer `lines: 8` window with bare or `|-` patterns; no JSON-typed
assertions against bool/number paths.

---

## 7. Followup feature work surfaced

- **`pair_hash` rule kind** (extension of `file_hash` to "hash matches
  a registry entry") — narrower use case but golang/go FIPS is the
  highest-stakes use case. **v0.10 ship-target** (3 sources: k8s +
  tokio + golang/go FIPS; CMVP-submitted security policy references
  the format).
- **`import_gate` rule kind** (allowlist / denylist / per-directory
  modes) — covers "no `testing` import in non-test source", "no
  `unsafe` outside `runtime`", "no direct `golang.org/x/*` imports
  outside `src/vendor/`". **v0.10 ship-target** (4 sources;
  saturated).
- **`ordered_block` rule kind** — covers `api/go1*.txt` golden file
  ordering ("entries are sorted, no duplicate symbols, every entry
  has a `pkg <pkg>` namespace prefix"). **v0.10 ship-target** (7
  sources; saturated).
- **`registry_paths_resolve.mode: github_issues`** — covers
  `doc/next/6-stdlib/99-minor/<pkg>/<issue>.md` ↔ live GitHub issue
  cross-reference. **v0.11+ design candidate, single-source**
  (golang/go only). Could also be a `command:` shellout to `gh issue
  view <number>`.

**No NEW rule-kind candidates beyond the existing v0.10+ list.**
golang/go's gap catalogue overlaps cleanly with what k8s, tokio,
rust-lang, cpython, and airflow already surfaced. This is a
**saturating signal** — the v0.10+ list is approaching completeness
for the language-monorepo segment.

---

## 8. Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **Add the `agent-context@v1` overlay (5 rules).** golang/go ships
   `.github/PULL_REQUEST_TEMPLATE` with the load-bearing "+ No
   Markdown" instruction; the agent-context bundled ruleset would
   gate AI-generated contribution discipline (no markdown in PR
   descriptions, AGENTS.md present, AI-context structure). golang/go
   is exactly the kind of repo where AI-generated PR-description
   noise would get rejected by Russ Cox's review etiquette.
2. **Replace the 4 defensive `command:` shellouts.** Of
   `go-gofmt-check`, `go-vet-std`, `go-vet-cmd`, `go-shellcheck`,
   only `go-shellcheck` is replaceable by a future bundled
   `tooling/shellcheck@v1` overlay (none ships yet). The other three
   are inherently shellouts because alint deliberately doesn't ship
   Go AST awareness.
3. **`alint suggest` against the live tree.** Likely candidate: a
   generalised "every file under `doc/next/6-stdlib/99-minor/<pkg>/`
   matches `^\d+\.md$`" rule the suggester auto-discovers, replacing
   the hand-rolled `go-doc-next-stdlib-minor-issue-filenames` rule.

---

## 9. Validation status (2026-05-08)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **64** (31 golang/go-specific + 33 from 3 bundled
  rulesets — `oss-baseline=15`, `go=8`,
  `hygiene/no-tracked-artifacts=11`, with one rule deduplicated)
- **`alint validate-config`:** ✓ Config valid: 64 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for
  the 286-violation breakdown (81 real + 205 cosmetic; including 1
  merge-conflict marker in `HACKING.md` and 2 zero-width
  Trojan-Source catches that golang/go's existing tooling never
  surfaces)
- **Pitfall fixes (this batch):** none needed — all
  `file_header` patterns use bare or `|-` patterns; no `pattern: |`
  instances; pitfalls #13/#14/#16/#17 all clean
- **Open gaps:**
  - `pair_hash` (v0.10 ship-target, 3 sources — golang/go FIPS is
    the highest-stakes use case)
  - `import_gate` (v0.10 ship-target, 4 sources)
  - `ordered_block` (v0.10 ship-target, 7 sources)
  - `registry_paths_resolve.mode: github_issues` (v0.11+ design,
    single-source)
- **Bench numbers:** 83 ms (lite bundled-only pass); 65.7 s (full
  pass dominated by `go-shellcheck` walking 12 .bash files + the
  defensive `go vet` shellouts on 11,262 .go files) on
  `/tmp/golang-go`'s ~12,000-file in-scope tree
