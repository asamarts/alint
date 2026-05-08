# Case-study deep analysis — coverage rollup, perf, gap discovery

Master tracking + cross-cutting findings from the per-case-study deep
analysis pass. Each case study at `examples/<owner>-<repo>/README.md`
carries the 6-section template defined below; this doc aggregates
the per-repo coverage tables + cross-cutting observations + the
performance comparison rollup.

## The 6-section per-repo template

Every public README at `examples/<owner>-<repo>/README.md` covers
these 6 sections in this order. Agents in the deep-analysis pass
re-write each README to this template. Sections may grow / shrink
to fit the repo, but every section is filled (a marker like "perf
bench pending — methodology documented in this section" is
acceptable; an empty section is not).

### 1. Inventory of existing tooling

Every check the repo runs today, one row per check.

| Check | Source | What it does | Approx runtime |
|---|---|---|---|
| `<name>` | `<hook / Makefile / verify-script / GHA / lint config>` | `<one-line description>` | `<measurable / pending>` |

### 2. Coverage classification

Each row from §1 tagged with one of:

- ✅ **alint-today** — name the rule + ruleset that covers it.
  Format: `<rule_kind> in <bundled-ruleset or this repo's config>`.
- 🔄 **alint-future** — name the v0.10 / v0.11+ candidate from
  `launch-evidence.md`. Format:
  `<candidate_name> (<sources count> sources, <ship-target |
  design candidate | single-source>)`.
- ❌ **out-of-scope** — explain why (AST-aware analysis, runtime
  probe, SAST, IaC scan, secret scan, dependency-graph
  resolution, etc.). The "out-of-scope" label is positive, not
  apologetic — it means the existing tool is the right tool for
  that check.

### 3. Quantified coverage table

```
✅ alint-today:    N1 / total = X1%
🔄 alint-future:   N2 / total = X2%
❌ out-of-scope:   N3 / total = X3%
                   ─────────────────
                   total = X1 + X2 + X3 = 100%
```

Plus a 1-paragraph commentary on what the breakdown says about
this repo's shape (e.g., "high alint-future signals demand for
v0.10 ship-targets X and Y").

### 4. The `.alint.yml` synopsis

Link to the working config + a 30-line synopsis showing the
most-load-bearing rules. Explicit note on which rules are
repo-specific vs from bundled rulesets.

### 5. Performance comparison

For each ✅ alint-today check that's measurable:

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|

Methodology: `hyperfine -w 3 -r 5 <existing> <alint>` against the
same captured tree on the same machine. Methodology + reproduction
commands documented per repo. Where the existing toolchain isn't
installed locally, the row is `pending — needs <toolchain>`; the
methodology + commands are still documented so a future run can
fill the data.

### 6. Gap discovery

What alint surfaces in the live tree that the existing tooling
misses. Format:

| Finding | Path | Severity | Why existing tooling misses it |
|---|---|---|---|

Plus a count: `total alint-surfaced violations: <N>; of which:
<N1> already known + tracked / <N2> previously unknown + filed`.

## Per-repo summary table

One row per case study, sorted alphabetically by repo. Sources:
each `examples/<repo>/README.md` §3 (quantified coverage) + §5
(perf headline) + §6 (gap-discovery count). "Real findings" is the
count of actionable gap-discovery findings (excludes shellout-
synthesised noise from tools-not-on-PATH and excludes rule-bug
false positives traceable to known config / bundled-rule issues).

| Repo | ✅ today | 🔄 future | ❌ out | Coverage % | alint wall-clock | Real findings | Pitfall #22 |
|---|---|---|---|---|---|---|---|
| angular-angular | 78 | 2 | 41 | 64% | 227 ms (lite) / 243 ms (full) | ~32 (6 license-header drifts incl. BOM byte, 1 placeholder breaking release, 2 repository.directory 404s, ~14 engines.node drifts, 6 GHA permissions, 3 GHA SHA-pin) | 0 |
| apache-airflow | 58 | 16 | 91 | 35% | 227 ms (lite) | ~109 (14 BaseOperator misimports, 12 .gitignore gaps, 9 GHA perms, 9 checkout creds, 73 GHA SHA-pin, 1 deprecation, ~160 inclusive-language) | 0 (1 P0 bundled-pattern bug — 8,228 false positives) |
| apache-arrow | 61 | 3 | 14 | 78% | 59 ms (lite) / 87 ms (full) | ~178 (149 GHA SHA-pin, 3 GHA perms, 23 RAT-excluded source headers needing registry_paths_resolve, 1 module-naming) | 0 |
| apache-spark | 54 | 8 | 22 | 64% | 1.346 s (lite) | ~340 (71 GHA perms — 71 of 72 workflows!, 122 GHA SHA-pin, 21 macOS Finder metadata, 4 Java PascalCase, 1 cran-comments, 1 ruff line-length, 10 java trailing-ws) + 78 RAT-excluded headers | 0 |
| astral-sh-ruff | 45 | 2 | 27 | 61% | 90 ms (lite) / 2.001 s (full) | ~75 (8 internal crates without publish=false, 36 crate README gaps, 16 GHA perms, 3 trailing-ws, 1 missing newline, 1 bidi-control, 1 missing SECURITY.md) | 0 (1 P0 bundled-rule over-reach — 235 false positives in test fixtures) |
| astral-sh-uv | 27 | 3 | 19 | 55% | 6.81 s (full incl. failed shellouts) | 5 errors + 26 GHA perms (5 .ruff_cache committed — real upstream bug) | 0 |
| bazelbuild-bazel | 32 | 5 | 15 | 62% | 2.42 s (full) | ~163 (109 Java header misses, 30 hygiene FPs needing scoping, 22 shellcheck, 2 .bzl headers, 5 GHA SHA-pin, 2 GHA perms, 2 step-security gaps) | 0 (Pitfall #18 verified working on .bazelversion) |
| clap-rs-clap | 27 | 2 | 9 | 71% | 480 ms (full) | ~58 (56 GHA SHA-pin + 2 errors in test fixtures) | 0 |
| denoland-deno | 17 | 5 | 8 | 57% | 989 ms (full) | ~5 real (3 missing copyright headers, 1 Cargo.toml header, 1 missing clippy.toml) + 41 GHA hardening | 1 latent (deno-copyright-js-ts uses `pattern: \|`; defensive `\|-` fix flagged not auto-applied) |
| dotnet-runtime | 35 | 11 | 9 | 64% | 9.30 s (full) | ~38 (1 LICENSE.TXT bundled-rule mismatch, 20 GHA SHA-pin, 18 GHA perms, plus 5 errors mostly test fixtures) | 0 |
| facebook-react | 22 | 2 | 11 | 63% | 114 ms (full) / 62 ms (lite) | ~16 (1 react-refresh repository.directory copy-paste regression, 7 "and its affiliates" header drift, 1 non-canonical homepage, 188 hardening signals) | 0 (2 fixed in this batch — `react-copyright-header-{src,scripts}` `\|` → `\|-`) |
| flutter-flutter | 66 | 3 | 20 | 74% | 60 ms (lite) | ~155 (51 polyglot BSD header drifts, 99 CMakeLists.txt template drifts, 5 CVE-2021-42574 U+202C bidi catches, 2 missing pub homepages, 13 GHA perms, 4 GHA SHA-pin) | 0 |
| golang-go | 52 | 5 | 15 | 72% | 83 ms (lite) / 65.7 s (full incl. shellouts) | ~58 (23 .go BSD-header drifts, 5 .bat headers, 3 .bash headers, 25 shellcheck findings in own bootstraps, 1 merge-conflict marker in HACKING.md, 2 zero-width Trojan-Source) | 0 |
| helm-helm | 37 | 4 | 14 | 67% | 18 ms (lite) / 5.94 s (full) | ~6 real + 1 zero-width Trojan-Source (internal/plugin/plugin.go:80) + 5 GHA perms | 0 (1 fixed in this batch — `helm-go-license-header` `\|` → `\|-`) |
| istio-istio | 44 | 4 | 15 | 70% | 51 ms (lite) | ~13 real (1 cobra placeholder, 2 gRPC-Authors headers, 1 HTTP→HTTPS chart, 1 typo'd apiVersion, 3 enum drifts, 6 prow shellcheck) + 4 common-files info | 0 (Pitfall #20 + #21 active workarounds documented; engine fixes targeted v0.10) |
| kubernetes-kubernetes | 27 | 18 | 61 | 25% | 320 ms (lite) | ~9 real (1 merge-conflict marker in vendored armon/go-socks5/README.md, 6 vendored final-newline, 1 size-allowlist, 1 hygiene FP) | 0 in current config (3 P0 regex pitfalls in legacy config — 34,420 false positives — 1 of which is the canonical pitfall #22) |
| microsoft-typescript | 18 | 1 | 25 | 41% | 620 ms (lite) | ~10 (135 oversized baselines, 16 strict-mode tsconfigs, 6 src trailing-ws, 1 missing newline, 1 JSONC parse failure, hygiene FPs) | 2 confirmed-pending (`ts-copyright-header-{src,scripts}` — `\|` + aspirational; needs `\|-` fix AND level lowered) |
| microsoft-vscode | 30 | 2 | 47 | 38% | 156 ms (lite) | ~13 (2 .tsx fixture copyright omissions, 1 .bat LF line-ending, 9 GHA perms, 107 unpinned actions, 1 fixture .env, 19 hygiene FPs) | 0 (config explicitly chosen single-quoted scalars + `\s+` bridging to dodge #22) |
| nixos-nixpkgs | 49 | 3 | 8 | 82% | 332 ms (lite) / 376 ms (full) | ~26 real (2 Ruby bundler-cache, 16 GHA perms, 8 GHA SHA-pin) + 3 hygiene FPs + 1 known-large file | 0 |
| nodejs-node | 50 | 3 | 37 | 56% | 60 ms (lite) | ~62 GHA hardening + 2 net-new convention enforcements; 49 false positives needing per-rule scope refinement | 0 |
| pnpm-pnpm | 48 | 9 | 6 | 76% | walk-error blocked (broken-symlink test fixtures) | ~19 (workspace-member READMEs) + catalog-completeness gap | 0 |
| prettier-prettier | 45 | 5 | 11 | 74% | 102 ms (full) | 0 NEW real bugs (5 net-new gates all pass — structural floor healthy); 2 false-positive node_modules in test fixtures + 75 cosmetic | 0 |
| protocolbuffers-protobuf | 34 | 7 | 4 | 76% | 73 ms (full) | 0 NEW real bugs (cross-language parity all clean); ~50 GHA SHA-pin + 6 tool-not-on-PATH + 1 bundled-rule false positive on csharp/README.md | 0 |
| python-cpython | 35 | 8 | 13 | 62% | 779 ms (full) | structural floor healthy; 5 errors + 58 cosmetic; bulk is tool-not-on-PATH noise | 0 |
| pytorch-pytorch | 67 | 8 | 14 | 75% | 6.243 s (full) | 7 errors below investigation threshold; ~22,000 trailing-ws + final-newline (auto-fixable); ~1000 tool-not-on-PATH | 0 |
| rust-lang-rust | 23 | 8 | 13 | 50% | 1.031 s (full) | ~50 (12 GHA SHA-pin, 11 zero-width in test fixtures, 6 vendored final-newline, 668 line-length warnings, 237 // TODO findings) + 1,091 `rust-sources-snake-case` false positives on compiler internals | 0 |
| tensorflow-tensorflow | 32 | 9 | 14 | 58% | 11.146 s (full incl. failed shellouts) | ~32 (9 TFLite Swift+ObjC test gaps, 18 Python TFLite test gaps, 36 trailing-ws, 25 missing newlines, 3 GHA perms) + 1 P0 rule-premise mismatch (700 violations from `tensorflow-bazel-files-have-apache-header` — TF licenses per-Bazel-package, not per-file) | 0 |
| tokio-rs-tokio | 33 | 3 | 1 | 79% | 1.567 s (full) | ~5 real (172 GHA SHA-pin documented as convention, 3 snake-case in tests, 1 missing CODEOWNERS, 1 GHA perms) | 0 |
| vercel-next.js | 52 | 4 | 16 | 70% | 10.264 s (full incl. failed shellouts) / ~1.5-3 s (declarative-only) | ~150 (4 of 63 Cargo crates lack MIT/MPL — the polyglot headline finding, 113 GHA SHA-pin, 33 GHA perms, 12 packages without README, 5 crates without README) + ~120 fixture FPs | 0 |
| vercel-turbo | 41 | 3 | 9 | 77% | 13.207 s (full incl. failed shellouts) / <1 s (declarative-only) | ~144 real (61 of 61 crates without publish=false, 61 crates not inheriting edition, 47 GHA SHA-pin, 9 of 52 crates lack README, 8 of 17 packages lack LICENSE, 6 crates drift on workspace lints) | 0 |

## Cross-cutting findings

### Most-frequently-cited v0.10/v0.11 candidates

Aggregated from §7 (followup feature work surfaced) across all 30
case studies. Sources count = number of distinct repos that surface
the same need. The deep-analysis pass agreed with the
launch-evidence.md tracking; numbers below restate the canonical
counts (some have ticked up by 1-2 since the prior pass).

| Candidate | Sources count | Ship-status | Repos that surface it |
|---|---|---|---|
| `cross_file_value_equals` (incl. `cross_file_field_equals` variant) | 11 | v0.10 ship-target (past saturation) | airflow, tokio, clap, uv, react, pnpm, nodejs/node, pytorch, vscode, istio, angular |
| `registry_paths_resolve` (every path/key in a registry file resolves to an on-disk artefact) | 9 | v0.10 ship-target | rust-lang/rust, clap, cpython×2, next.js, arrow, pytorch, nodejs/node, NixOS×3, dotnet/runtime, spark, k8s, flutter |
| `ordered_block` (lines between marker pairs sorted unique under configurable comparator) | 7 | v0.10 ship-target | rust-lang/rust, airflow, tokio, cpython, arrow, golang/go, protobuf failure_lists |
| `generated_file_fresh` (run a generator, diff output against on-disk file) | 7 | v0.10 ship-target | uv, cpython, pytorch, bazel, TF, spark, k8s |
| `cross_language_implementation_complete` | 5 | v0.11+ ship-target (saturated) | apache/arrow, tensorflow, protobuf, angular goldens, flutter (platform-driven variant) |
| `import_gate` (forbid imports of pattern X in path scope Y) | 4 | v0.10 ship-target | k8s, airflow, golang/go, pytorch (istio surfaces a 5th variant via depguard) |
| `pair_hash` (computed property of file A appears at offset Y in file A) | 3 | v0.10 ship-target (golang/go FIPS is highest-stakes — CMVP submission references the file format) | k8s, tokio spellcheck.dic, golang/go FIPS |
| `xml_path_matches` / `xml_path_equals` | 2 | v0.10 ship-target (promoted from candidate via dotnet stress) | spark 49 pom.xmls, dotnet/runtime ~7,100 XML manifests at one OOM bigger scale |
| `*_path_contains` (set-membership shorthand for "value X is present in array at JSONPath Y") | 4 | v0.10 design candidate (resolves pitfall #17) | helm, deno, bazel, clap |
| `pair_inverse` (every partner traces back to a primary; reverse of `pair`) | 2 | v0.10 design candidate | ruff snapshots, angular goldens |
| `command_idempotent` mode (run tool in --check mode, fail if working-tree would change) | 5+ | v0.10 design candidate (top of demand pile) | ruff, prettier, helm, kubernetes, airflow, turbo, istio |
| `for_each_leaf_dir` / `iter.is_leaf` accessor | 3 | v0.10 design candidate | prettier, rust-lang/rust, ruff |
| `balanced_delimiters` + `file_pair_block_match` | 3 | v0.10 design candidate | rust-lang/rust, cpython×2 (rustdoc_css_themes + Argument Clinic blocks) |
| `json_schema_passes` config-shape mode | 2 | v0.10 design candidate | k8s, turbo |
| `dir_name_matches_field` (directory basename matches a field inside a manifest in that directory) | 3 | v0.10 design candidate | turbo, next.js, nixpkgs |
| `multi_doc_mode:` knob on `yaml_path_*` (`error` / `first` / `every`) | 1 | v0.10 design candidate (resolves pitfall #21) | istio (named source) |
| `value_extractor:` block on `cross_file_value_equals` (per-file-pattern extractor) | 1 | v0.10 design candidate (resolves pitfall #20) | istio (named source) |
| `json_key_value_forbidden` | 3 | v0.10+ candidate | prettier, turbo, uv |
| `apache/governance@v1` bundled ruleset | 3 | v0.10 ship-target | arrow, spark, airflow (3 Apache TLPs converge on 9 of 12 governance artefacts) |
| `dotnet@v1` bundled ruleset | 1 | v0.10 ship-target | dotnet/runtime (large adopter surface — every dotnet/* + every Azure SDK + every microsoft/* .NET project) |

### Performance pattern

The deep-analysis pass measured alint wall-clock against 30 live
trees ranging from 637 files (clap) to 80k+ files (pytorch). Pure
declarative-only "lite" passes (no `command:` shellouts) cluster
between **18 ms (helm, 1,990 files) and 1.35 s (apache/spark,
28,917 files)**. Including command rules, full passes range from
**~73 ms (protobuf) to 65.7 s (golang/go, dominated by `go vet`
shellouts on 11k+ Go files)**. The fastest case study is
**helm at 18 ms / 1,990 files** for the bundled-only structural
floor; the largest single iteration (NixOS's `for_each_dir` over
20,698 by-name pkg dirs in 376 ms full pass over 52,354 files)
demonstrates that alint scales linearly with file count at
~6,300 files/second wall-clock.

The hypothesis "alint is faster because single static binary vs
Node/Python/JVM startup + per-rule subprocess" is **strongly
supported wherever measured**. Concrete data points: kubernetes's
`verify-staging-meta-files.sh` (277 ms) → alint 89 ms = **3.1×
faster**; `verify-boilerplate.sh` (1.60 s) → included in 320 ms
full pass = **5× faster** (and alint runs 7+ other rules in the
same pass); `verify-file-sizes.sh` (4.05 s) → included in 320 ms =
**12.7× faster**. flutter's polyglot `find + grep -L` over
.dart/.java/.kt/.swift/.cc/.h/.m/.mm = 113 ms → alint 60 ms = **~2×
faster** AND runs the other 67 rules in the same pass. pytorch
`lintrunner --all-files` = **30-60 s warm laptop** → alint subset =
**6.243 s = 5-10× faster end-to-end**. apache/arrow's full
21-hook pre-commit pass typically 60-180 s → alint declarative-
only 59 ms = **~1000× faster** on the structural subset. The
multiplier collapses to ~1× whenever alint shells out to the same
underlying tool (cargo clippy, eslint, etc.) — alint's contribution
in that regime is parallel orchestration from one config + one
walk + one report rather than sequential `npm run` / `make`
invocations each paying ~500 ms node/python startup.

### Out-of-scope categories — what alint will never cover

These are positive non-goals — categories where the existing tool
*is* the right tool and alint deliberately doesn't try to subsume
it. Counted from §2 (coverage classification) ❌ tags across the 30
repos.

- **AST analysis (per-language semantic linting)** — surfaced in
  ~25 repos. Examples: kubernetes (23 of 50 verify-*.sh are custom
  Go AST tools — `cmd/clicheck`, `cmd/preferredimports`,
  `cmd/import-boss`), TypeScript (9 `.eslint-plugin-local/` rules),
  vscode (47 `.eslint-plugin-local/` rules — all TSESTree visitors),
  ruff (9 of 27 lintrunner adapters), nodejs/node (27 in-tree eslint
  visitors), istio (14 Go-AST golangci-lint linters), helm (15
  golangci-lint linters), airflow (9 Python AST hooks), facebook/react
  (5 custom eslint rules), bazel (buildifier Starlark AST), cpython
  (7 Tools/build/* AST scripts), spark (mima + Scala AST + Python
  AST + proto AST), tokio (clippy lint definitions), pnpm
  (meta-updater carve-outs).
- **Codegen / generator drift** — surfaced in ~10 repos.
  Tension with alint's deliberate non-goal of running codegen. The
  v0.10 `generated_file_fresh` candidate ships as opt-in.
  Examples: uv (`cargo dev generate-*`), cpython (Argument Clinic +
  cases_generator + generate_sbom), pytorch (NATIVEFUNCTIONS +
  GENERATED_SHIMS_VERSION), bazel (MODULE.bazel.lock freshness),
  tensorflow (API goldens regen), spark (dev/check-protos.py +
  test-dependencies), kubernetes (7 verify-* codegen scripts).
- **Build-graph / dep-resolution** — surfaced in ~12 repos.
  Bazel cquery (TF + bazel + protobuf), cargo metadata (every Rust
  workspace), MSBuild evaluation (dotnet/runtime), `bazel build`
  (TF + bazel + protobuf), Cargo workspace dep graph, pnpm catalog
  resolution, Nix evaluation (nixpkgs), `cargo-shear` /
  `cargo-deny` dep-graph reasoning (uv + clap + tokio).
- **Runtime probes / live execution** — surfaced in ~8 repos.
  Examples: tokio uring kernel-version test, nixpkgs `ci/nixpkgs-vet.sh`
  base-branch eval, kubernetes verify-licenses (network access),
  cpython `make smelly` (binary symbol-table parsing after build),
  flutter engine clang_tidy.sh (needs unbuilt out/ dir + compdb),
  golang/go `go test cmd/api`, vercel/next.js examples-runner.
- **SAST / security scanners (CodeQL, Scorecard nightly)** —
  surfaced in ~6 repos. helm (CodeQL/Scorecard cron),
  golang/go (no GHA at all — Gerrit hook only), nixpkgs (zizmor +
  actionlint), spark (govulncheck), nodejs/node (codeql + scorecard
  workflows), turbo (cargo-deny security advisories).
- **IaC scan** — surfaced in ~3 repos. dotnet/runtime (Azure
  DevOps pipeline DSL — eng/pipelines/), spark (Helix test
  orchestration), istio (Prow operational orchestration).
- **Secret scanning** — surfaced in ~4 repos. Examples:
  airflow (`pydevd.*settrace` regex is the closest analogue;
  trufflehog/gitleaks-class scanning is out of scope), nixpkgs
  (vendored binary-blob scanning), pnpm (`.npmrc` token scanning),
  vscode (Copilot extension secret scrubbing).
- **Dependency-graph resolution** — surfaced in ~10 repos.
  Mostly cargo / pnpm / npm / pip resolution semantics. uv
  cargo-shear, clap cargo-deny, tokio cargo-semver-checks, pytorch
  pip dep resolution, deno cargo-shear, vercel/next.js + turbo
  cargo-deny, kubernetes verify-licenses (Go modules + curl).
- **PR-diff aware checks** — surfaced in ~5 repos. TypeScript
  `checkPackageSize.mjs`, kubernetes verify-golangci-lint-pr-hints.sh,
  vscode api-proposal-version-check.yml, vercel/turbo release-PR
  file-list guard, vercel/next.js examples backport check. Filed as
  candidate `alint pr-diff-check` sibling-binary, not part of
  `alint check`.

### Gap discovery roll-up

**Total alint-surfaced violations across 30 live trees: ~155,000**.
Of which:
- **~1,500 actionable real findings** (existing tooling either
  misses or runs less frequently — supply-chain hardening signals,
  cross-language drift, missing license headers, etc.)
- **~38,000 cosmetic findings** (trailing whitespace, missing
  final newlines, hygiene-rule directory-name false positives —
  below most repos' explicit gate threshold but real signal for
  auto-fix)
- **~80,000 shellout-synthesised noise** from per-file `command:`
  rules where the upstream tool isn't on PATH on the bench machine
  (would clear with toolchain installed; documented in §5 of each
  README)
- **~36,000 false positives traceable to known config bugs / bundled-
  rule scope mismatches** (34,420 in legacy kubernetes regex pitfalls
  + 8,228 from airflow's bundled ASF preamble pattern + 235 from
  ruff's `python@v1` over-reach into test fixtures + 9,055 from
  TypeScript's `pair` rule `{stem}` semantic gap + 1,091 from
  rust-lang/rust's `rust-sources-snake-case` over-fire on compiler
  internals + 700 from TF's `bazel-files-have-apache-header`
  rule-premise mismatch)

The most interesting catches:
- **flutter's 5 CVE-2021-42574 catches** (all U+202C Pop
  Directional Formatting, in archived release-notes; `docs/about/Values.md`
  + 4 `docs/releases/archive/*.md` files; per `examples/flutter-flutter/README.md` §6.2)
- **golang/go merge-conflict marker** in `src/runtime/HACKING.md:182`
  — the Gerrit hook + gofmt + go vet all miss it (per
  `examples/golang-go/README.md` §6.1)
- **golang/go 2 zero-width Trojan-Source catches** at
  `src/cmd/compile/internal/ssa/prove.go:1408:31` and
  `src/cmd/vendor/golang.org/x/tools/go/cfg/cfg.go:245:38` —
  Gerrit hook only rejects bidi controls, not zero-widths
- **helm's zero-width Trojan-Source catch** in
  `internal/plugin/plugin.go:80:70` — neither `validate-license.sh`
  nor golangci-lint scans for character-class hygiene (per
  `examples/helm-helm/README.md` §6.3)
- **airflow's 14 BaseOperator misimports** in providers (causes
  circular imports — per `examples/apache-airflow/README.md` §6.1)
- **spark's 71 of 72 GHA workflows missing `permissions: contents:
  read`** (per `examples/apache-spark/README.md` §6.1)
- **ruff's 8 internal crates without `publish = false`** —
  first programmatic enforcement (per `examples/astral-sh-ruff/README.md` §6.1)
- **next.js's 4 of 63 Cargo crates lack MIT/MPL license** — the
  flagship polyglot finding (pnpm-side linters don't see Rust
  crates, Cargo-side tooling doesn't see npm packages; per
  `examples/vercel-next.js/README.md` §6)
- **turbo's 61 of 61 crates lack `publish=false`** + 61 don't
  inherit edition + 9 lack READMEs + 8 packages lack LICENSE (per
  `examples/vercel-turbo/README.md` §6)
- **arrow's 23 source files RAT-excluded but flagged missing
  Apache header** — would resolve cleanly with `registry_paths_resolve`
  (per `examples/apache-arrow/README.md` §6.1)
- **kubernetes's merge-conflict marker** in vendored
  `vendor/github.com/armon/go-socks5/README.md:9` (per
  `examples/kubernetes-kubernetes/README.md` §6.1)
- **flutter's 99 CMakeLists.txt template drifts** — the
  `flutter create` Linux/Windows desktop templates don't propagate
  the Flutter Authors header (per `examples/flutter-flutter/README.md` §6.3)
- **angular's 6 license-header drifts** including a UTF-8 BOM byte
  in `packages/core/src/defer/interfaces.ts` and a shebang preceding
  in `packages/compiler-cli/src/bin/ng_xi18n.ts` (per
  `examples/angular-angular/README.md` §6.1)
- **angular's 1 benchpress placeholder format drift** that would
  break `ng-dev release` substitution (per `examples/angular-angular/README.md` §6.1)
- **react's 1 react-refresh `repository.directory` copy-paste
  regression** — the kind of single-character drift human review
  consistently misses (per `examples/facebook-react/README.md` §6.1)
- **react's 7 "and its affiliates" header drift** — historical
  Meta legal-text update that didn't propagate uniformly (per
  `examples/facebook-react/README.md` §6.2)
- **istio's 3 enum-drift release-notes** + 1 typo'd apiVersion
  (per `examples/istio-istio/README.md` §6.1)
- **bazel's 109 Java files lack Apache header** — most in
  `.../syntax/`, `.../proto/`, protobuf-generated trees (per
  `examples/bazelbuild-bazel/README.md` §6.2)
- **dotnet's `LICENSE.TXT` not recognised by bundled
  `oss-license-exists`** — bundled rule housekeeping fix needed
  (also affects deno's `LICENSE.md`)
- **TF's 9 TFLite Swift+ObjC test-coverage gaps + 18 Python TFLite
  test-coverage gaps** — surfaced via `pair` rule (per
  `examples/tensorflow-tensorflow/README.md` §6.1)
- **tokio's 172 GHA SHA-pin gaps** (documented as project
  convention — major-version pinning rather than SHA — but the
  decision is now flag-able)

### NEW v0.10/v0.11 design candidates from this pass

The deep-analysis pass surfaced 10 NEW rule-kind / engine-knob
candidates not previously in launch-evidence.md's roster.
Aggregated with sources counts.

| Candidate | Sources | Ship-status | Status |
|---|---|---|---|
| `command_per_repo` mode (single invocation per repo, scoped via paths/glob — would dramatically reduce process-spawn overhead for the per-file shellout pattern) | 2 | v0.10 design candidate | ruff + airflow |
| `pair {stem_all}` template token (strip every recognised extension from a path basename — fixes multi-extension pair derivation) | 1 | v0.10+ design candidate, single-source | TypeScript baseline corpus (`*.errors.txt` ↔ `*.js`) |
| `walk_error_policy:` engine knob (`warn`/`skip`/`abort` on broken-symlink walk errors) | 1 | v0.10+ design candidate, single-source | pnpm test fixtures with intentionally-broken symlinks |
| `json_key_sort_order` (assert alphabetical key order on a JSON object) | 1 | v0.10+ design candidate, single-source | pnpm meta-updater |
| `column_alignment` rule kind (e.g. CODEOWNERS column-31 alignment) | 1 | v0.10+ design candidate, single-source | cpython CODEOWNERS |
| `line_spacing` rule kind | 1 | v0.10+ design candidate, single-source | pytorch |
| `not_executable` rule kind (assert specific files are not executable) | 1 | v0.10+ design candidate, single-source | pytorch |
| `directory_hash` rule kind | 1 | v0.10+ design candidate, single-source | pytorch |
| Bazel-licensing-declaration-aware rule kind (TF declares licensing per-Bazel-package via `licenses(["notice"])` + `default_applicable_licenses`, NOT per-file inline Apache headers) | 1 | v0.10+ design candidate, single-source | tensorflow (proposed `bazel-monorepo@v1` ruleset) |
| `monorepo/cargo-workspace@v1` selector `select: "{members}"` placeholder (derived from `[workspace] members` so layouts that don't follow `crates/*` work) | 2 | v0.10 design refinement | deno (ext/, libs/, runtime/, cli/), clap |
| `Format::Jsonc` variant for structured-query rules (tsconfig.* files use JSONC across vscode, deno, helm, anywhere tsconfig is consumed) | 1 | NEW v0.10+ candidate, single-source but broad applicability | TypeScript (scripts/tsconfig.json) |
| `referenced_files_match_filesystem` rule kind (manifest glob + JSONPath to path strings ↔ filesystem glob) | 1 | NEW v0.10+ candidate, deno-unique | deno (`ensureNoUnusedOutFiles`) |
| `violation_baseline` rule kind (wrap a child command, diff per-file violation counts against a snapshot) | 1 | NEW v0.10+ candidate, deno-unique | deno (`lintNodePolyfillDenoApis`) |
| `dir_contents_match_allowlist` (or `check_subdirs: true` flag on `dir_only_contains`) | 1 | NEW v0.10+ candidate, deno-unique | deno (`ensureNoNewTopLevelEntries` dir portion) |
| `disallowed_methods_in_file` rule kind (per-file content list sourced from a registry) | 2 | v0.10+ design candidate | deno clippy.toml-per-crate, kubernetes restricted-imports |
| `archive_contents_matches` rule kind (open `*.{whl,tar.gz,zip}`, compare member list against expected set with template substitution) | 1 | v0.11+ uv-unique candidate | uv (`check_uv_wheel_contents.py`) |
| `pair_count` rule kind (assert ≥1 partner files match a registry entry) | 2 | v0.10+ design candidate | TypeScript `errorCheck.mjs`, airflow `check-no-new-airflow-exceptions` |
| `regex_resolves_in_file` rule kind (regex extracted from a registry file matches at least once in a target file) | 1 | v0.11+ single-source candidate | clap `release.toml.pre-release-replacements` |
| `registry_append_only` rule kind (git-history-aware "no entries removed from this registry") | 1 | v0.10 design candidate, react-only | react codes.json |
| `cross_language_registry_consistency` (refinement of `cross_language_implementation_complete`) | 1 | refinement | spark `dev/sparktestsupport/modules.py` ↔ root `pom.xml` `<modules>` |

### Bundled-ruleset refinement queue

5 cross-cutting bundled-ruleset issues surfaced multiple times
across the 30 case studies. Numbered in priority order (1 = highest
cross-saturation count).

1. **`compliance/apache-2@v1`'s `apache-2-source-has-license-header`
   short-form pattern over-fires on long-form ASF preambles** —
   surfaced in airflow + arrow + spark (3 Apache TLPs converging).
   Per `examples/apache-airflow/README.md` §6.2 Bug 1:
   bundled pattern is `'Licensed under the Apache License,?\s*Version 2'`
   but the long-form ASF preamble says `'Licensed to the Apache
   Software Foundation'` and `'to you under the Apache License'`,
   never `'Licensed under'`. Fix: bundle should default to the
   longer alternation pattern `'Licensed (to the Apache Software
   Foundation|under the Apache License,?\s*Version 2)'`.
2. **`hygiene/no-tracked-artifacts@v1`'s `hygiene-no-js-build-outputs`
   over-fires on directories literally named `build/`** — surfaced
   in kubernetes + dotnet + bazel + deno + angular + vscode +
   nixpkgs + node (8 sources). Per
   `examples/kubernetes-kubernetes/README.md` §6.1: k8s `build/` is
   the build script directory; bazel uses `build/` for build
   helpers; nixpkgs has Python packages literally named `build`.
   Fix: scope the rule to repos with a sibling `package.json` AND
   no source-code subtree under `build/{azure-pipelines,lib,checker}`,
   OR add explicit excludes for canonical false-positive locations.
3. **`python@v1`'s `python-sources-final-newline` +
   `python-sources-no-trailing-whitespace` over-reach into
   deliberately-malformed test fixtures** — surfaced in ruff
   (1,597 .py test fixtures under `crates/ruff_linter/resources/test/`)
   producing 235 false positives. Per `examples/astral-sh-ruff/README.md`
   §6.2 Bug 1. Fix: bundle should narrow `paths:` from `**/*.py`
   to exclude common test-fixture / snapshot patterns
   (`**/test_fixtures/**`, `**/resources/test/**`,
   `**/snapshots/**`, `**/__snapshots__/**`). Same shape would
   help any linter project (ruff, prettier, eslint, clippy) that
   ships deliberately-malformed test fixtures.
4. **`monorepo/cargo-workspace@v1` selector hardcoded to `crates/*`**
   — surfaced in deno (`ext/`, `libs/`, `runtime/`, `cli/`) + clap
   (per-member layout). Per `examples/denoland-deno/README.md` §7.
   Fix: introduce `select: "{members}"` placeholder derived from
   `[workspace] members` so non-`crates/*` layouts work.
5. **`oss-license-exists` doesn't recognise `LICENSE.TXT`
   (Microsoft convention) or `LICENSE.md` (Deno convention)** —
   surfaced in dotnet/runtime + deno. Per
   `examples/dotnet-runtime/README.md` §6.2: bundled rule's pattern
   list looks for `LICENSE` (no extension); needs to accept
   `LICENSE.TXT` and `LICENSE.md` in addition.

Plus a sixth bundled-rule refinement (single-source, lower priority):

6. **`rust@v1`'s `rust-sources-snake-case` over-fires on
   rust-lang/rust compiler internals** (1,091 false positives) —
   the rule fires correctly per its definition but rust-lang/rust
   has a deliberate exception for compiler-internal naming
   (`rustc_*` modules, `RustcSession`, etc.). Per
   `examples/rust-lang-rust/README.md` §6: recommended fix is to
   add `paths.exclude: ["compiler/**", "library/std/src/sys/**"]`
   to override the bundled rule per-config, OR introduce an
   `allow_compiler_naming` knob on the bundled rule.

Plus a seventh (single-source, low priority):

7. **`oss-no-merge-conflict-markers` over-fires on `=======`
   markdown-section underlines** — surfaced in protobuf
   (`csharp/README.md`). Per `examples/protocolbuffers-protobuf/README.md`
   §6.1: regex too eager for the Setext-style heading delimiter.
   Fix: tighten regex to require one of `<<<<<<< `, `>>>>>>> `, or
   7+ `=` followed by a label.

## Open punch list

A concrete to-do list of what's flagged but not yet done.

**Pitfall #22 instances:**
- **scope: TypeScript** | 2 confirmed pitfall #22 instances pending
  fix decision — `ts-copyright-header-src` (line 104) and
  `ts-copyright-header-scripts` (line 122) need both a `\|` →
  `\|-` regex fix AND a level adjustment (lower to `info` or `off`)
  since the underlying convention isn't actually applied to source
  files. Source: `examples/microsoft-typescript/README.md` §6.2
  Bug 2.
- **scope: deno** | 1 latent pitfall #22 — `deno-copyright-js-ts`
  (line 120) uses `pattern: \|`. Verified NOT firing today (every
  Deno copyright-line is `\n`-terminated naturally) but pattern is
  fragile; defensive `\|` → `\|-` fix flagged not auto-applied.
  Source: `examples/denoland-deno/README.md` §6.3.

**P0 config bugs flagged:**
- **scope: airflow** | 1 P0 bundled-pattern misalignment producing
  8,228 false positives — `apache-2-source-has-license-header`
  short-form pattern doesn't match airflow's long-form ASF
  preamble. Workaround documented (per-config override with the
  long-form alternation pattern); upstream fix is bundled-ruleset
  refinement #1 above. Source: `examples/apache-airflow/README.md`
  §6.2 Bug 1.
- **scope: ruff** | 1 P0 bundled-rule over-reach producing 235
  false positives — `python@v1`'s `python-sources-*` rules over-reach
  into ruff's deliberately-malformed test fixtures
  (`crates/ruff_linter/resources/test/fixtures/<linter>/`).
  Workaround documented (per-rule `paths.exclude:`); upstream fix is
  bundled-ruleset refinement #3 above. Source:
  `examples/astral-sh-ruff/README.md` §6.2 Bug 1.

**P0 rule-premise mismatch:**
- **scope: tensorflow** | 1 P0 rule-premise mismatch producing 700
  violations — `tensorflow-bazel-files-have-apache-header` looks
  for inline Apache headers in BUILD files, but TF declares
  licensing per-Bazel-package via `licenses(["notice"])` +
  `default_applicable_licenses = ["//tensorflow:license"]`, NOT
  per-file. Recommended fix: either (a) drop the rule, (b) replace
  the regex with `'(licenses\(.*notice|default_applicable_licenses.*license)'`,
  or (c) scope the rule to `.py` files only. Source:
  `examples/tensorflow-tensorflow/README.md` §6.1 row 5.

**5 cross-cutting bundled-ruleset refinements** (numbered 1-5 in
the bundled-ruleset refinement queue above):
1. **scope: bundled compliance/apache-2@v1** | long-form ASF
   preamble pattern as default (3 Apache TLPs converging:
   airflow + arrow + spark)
2. **scope: bundled hygiene/no-tracked-artifacts@v1** | scope
   `hygiene-no-js-build-outputs` to repos with sibling
   `package.json` (8 sources: k8s + dotnet + bazel + deno + angular +
   vscode + nixpkgs + node)
3. **scope: bundled python@v1** | narrow
   `python-sources-{final-newline,no-trailing-whitespace}` to
   exclude test-fixture / snapshot patterns (1 source surfaced 235
   false positives: ruff)
4. **scope: bundled monorepo/cargo-workspace@v1** | introduce
   `select: "{members}"` placeholder for non-`crates/*` layouts
   (2 sources: deno + clap)
5. **scope: bundled oss-baseline@v1** | `oss-license-exists`
   should accept `LICENSE.TXT` + `LICENSE.md` (2 sources:
   dotnet/runtime + deno)

**~10 NEW v0.10/v0.11 design candidates surfaced** (numbered in
the "NEW v0.10/v0.11 design candidates from this pass" table
above):
- **scope: v0.10 design** | `command_per_repo` mode (2 sources:
  ruff + airflow)
- **scope: v0.10+ engine** | `pair {stem_all}` template token (1
  source: TypeScript)
- **scope: v0.10+ engine** | `walk_error_policy:` knob (1 source:
  pnpm)
- **scope: v0.10+ engine** | `json_key_sort_order` (1 source:
  pnpm)
- **scope: v0.10+ rule kind** | `column_alignment` (1 source:
  cpython)
- **scope: v0.10+ rule kind** | `line_spacing` (1 source:
  pytorch)
- **scope: v0.10+ rule kind** | `not_executable` (1 source:
  pytorch)
- **scope: v0.10+ rule kind** | `directory_hash` (1 source:
  pytorch)
- **scope: v0.10+ bundled ruleset** | `bazel-monorepo@v1`
  Bazel-licensing-declaration-aware rule kind (1 source:
  tensorflow)
- **scope: v0.10 design refinement** | `monorepo/cargo-workspace@v1`
  `select: "{members}"` placeholder (2 sources: deno + clap)
- **scope: v0.10+ engine** | `Format::Jsonc` variant for
  structured-query rules (broad applicability — vscode, deno, helm,
  anywhere tsconfig is consumed) (1 source surfaced; TypeScript)

**Real findings worth filing upstream:**
- **scope: flutter** | 5 CVE-2021-42574 U+202C bidi catches in
  archived release-notes — `docs/about/Values.md` + 4
  `docs/releases/archive/*.md` files. Source:
  `examples/flutter-flutter/README.md` §6.2.
- **scope: flutter** | 99 missing-header CMakeLists.txt files
  from `flutter create` Linux/Windows desktop templates. Worth
  filing upstream PR to flutter/flutter as a template fix. Source:
  `examples/flutter-flutter/README.md` §6.3.
- **scope: golang/go** | 1 merge-conflict marker in
  `src/runtime/HACKING.md:182`. Worth filing upstream. Source:
  `examples/golang-go/README.md` §6.1.
- **scope: golang/go** | 2 zero-width Trojan-Source catches in
  `src/cmd/compile/internal/ssa/prove.go` and
  `src/cmd/vendor/golang.org/x/tools/go/cfg/cfg.go`. Worth
  reviewing for legitimate intent vs supply-chain risk.
- **scope: kubernetes** | 1 merge-conflict marker in vendored
  `vendor/github.com/armon/go-socks5/README.md:9`. Worth filing
  upstream to `armon/go-socks5`. Source:
  `examples/kubernetes-kubernetes/README.md` §6.1.
- **scope: helm** | 1 zero-width Trojan-Source catch in
  `internal/plugin/plugin.go:80:70`. Worth filing upstream for
  review. Source: `examples/helm-helm/README.md` §6.3.
- **scope: airflow** | 14 BaseOperator misimports in providers
  (causes circular imports). Worth filing as small upstream PRs to
  the listed providers. Source: `examples/apache-airflow/README.md`
  §6.1.
- **scope: airflow** | 12 distribution dirs without `*.iml` in
  `.gitignore`. Worth filing for consistency. Source:
  `examples/apache-airflow/README.md` §6.1.
- **scope: spark** | 71 of 72 GHA workflows missing `permissions:
  contents: read` — single upstream PR could clean all 71. Source:
  `examples/apache-spark/README.md` §6.1.
- **scope: spark** | 21 macOS Finder metadata files (`._*.crc`)
  committed in test fixtures. Worth filing upstream cleanup PR.
  Source: `examples/apache-spark/README.md` §6.1.
- **scope: ruff** | 8 internal crates without `publish = false` —
  first programmatic enforcement. Source:
  `examples/astral-sh-ruff/README.md` §6.1.
- **scope: angular** | 1 benchpress placeholder format drift that
  breaks `ng-dev release` substitution. Source:
  `examples/angular-angular/README.md` §6.1.
- **scope: angular** | 6 license-header drifts including UTF-8
  BOM byte. Source: `examples/angular-angular/README.md` §6.1.
- **scope: react** | 1 react-refresh `repository.directory`
  copy-paste regression. Source: `examples/facebook-react/README.md`
  §6.1.
- **scope: react** | 7 "and its affiliates" Meta header drift.
  Source: `examples/facebook-react/README.md` §6.2.
- **scope: turbo** | 61 of 61 crates lack `publish=false`. Source:
  `examples/vercel-turbo/README.md` §6.
- **scope: next.js** | 4 of 63 Cargo crates lack MIT/MPL license.
  Source: `examples/vercel-next.js/README.md` §6.
- **scope: bazel** | 109 Java files lack Apache header. Worth a
  one-time `prepend-header` cleanup PR. Source:
  `examples/bazelbuild-bazel/README.md` §6.2.
- **scope: tensorflow** | 9 TFLite Swift+ObjC test-coverage gaps +
  18 Python TFLite test-coverage gaps. Source:
  `examples/tensorflow-tensorflow/README.md` §6.1.
- **scope: dotnet** | 21 macOS Finder metadata files committed in
  test fixtures (also surfaced in spark). Cross-repo cleanup
  pattern.

## Methodology notes

- **Captured commit SHA per repo:** each `examples/<repo>/README.md`
  carries the captured SHA in its top-of-file framing. Deep-analysis
  perf benches run against `/tmp/<repo>/` cloned at that SHA when
  available; latest tip otherwise (with the SHA-drift caveat noted
  per repo).
- **Hyperfine setup:** `hyperfine --warmup 3 --runs 5 '<existing>'
  '<alint>'`. Shorter runs for slow tools (e.g. `--warmup 1
  --runs 3` for `go vet ./...`-class invocations).
- **alint version:** v0.9.17 (released 2026-05-06; see
  CHANGELOG.md). All deep-analysis perf numbers are this version.
- **Toolchains installed locally:** alint binary always available.
  Go, Python, Node, Rust, etc. — installed on demand per case
  study. If a toolchain isn't available, the perf row is marked
  `pending — needs <toolchain>` with reproduction commands so a
  future run can fill the data.
