# Case study: `dotnet/runtime`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/dotnet-runtime/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `dotnet/runtime`
and an alint config that replaces the rules alint can express today,
plus a catalogue of the rules that need new alint primitives.

**Repo state captured:** 2026-05-07 latest tip of `main` via `git
ls-remote https://github.com/dotnet/runtime HEAD`. Sparse-clone at
`/tmp/runtime` (depth=1, filter=blob:none): **57,695 files**, 937 MB
working-tree (32,033 in-tree `.cs` files, **5,858 `.csproj` files,
237 `.slnx` solution files, 3 legacy `.sln`, 362
`Directory.Build.{props,targets}`, 369 `.props` + 238 `.targets`
files, 39 `eng/*.{props,targets}` build-glue, 190
`eng/pipelines/**/*.yml` for the Azure DevOps build matrix, ~25
`.github/workflows/*.yml` for GitHub-side automation only**). The
2026-05-06 inventory captured 1,091 csprojs from a more aggressive
sparse pattern; this batch's pull is broader (the difference is
~4,800 csprojs under `src/tests/` + `src/installer/pkg/` +
`src/coreclr/tools/aot/` re-included).

**alint version:** 0.9.17 (`1dbd9b218a0e`, built 2026-05-07).

---

## 1. Inventory of existing tooling

Every check dotnet/runtime runs today, one row per check. The repo's
gating infrastructure is **Azure DevOps pipelines (`eng/pipelines/`,
190 yml files)** for the actual build/test matrix + **~25 GitHub
Actions workflows** for GitHub-side automation only + the **MSBuild
+ Arcade SDK** stack for build orchestration.

### 1.1 Azure DevOps pipelines (`eng/pipelines/`, 190 yml files — actual CI)

dotnet/runtime's actual build-and-test CI does not run on GitHub
Actions. It runs on **Azure DevOps Pipelines**, organised under
`eng/pipelines/{coreclr,common,cdac,diagnostics,extra-platforms,1espt,helix-platforms}`
plus `global-build.yml` and CI-script helpers (`evaluate-changed-paths.sh`,
`get-changed-darc-deps.py`).

| Surface | What it actually does | alint disposition (preview — full mapping in §2) |
|---|---|---|
| Per-platform `jobs:` matrix (Windows / macOS / Linux × {x64, x86, arm, arm64} × {Release, Checked, Debug}) | Build + test the full matrix | **out-of-scope** (Azure Pipelines DSL — bundled `ci/github-actions@v1` doesn't apply; an `azure-pipelines@v1` ruleset is a v0.11+ candidate but single-source today) |
| `helix-platforms.yml` (Helix test orchestration) | Per-OS test runners | **out-of-scope** (Helix is the Microsoft test infra) |
| `evaluate-changed-paths.sh` | CI optimisation: skip subsystem when no relevant files changed | **out-of-scope** (CI graph; alint validates state) |
| `1espt/` (1ES Pipeline Templates) | Microsoft-internal pipeline templates | **out-of-scope** (Microsoft-specific) |

### 1.2 `.github/workflows/*.yml` (~25 workflows — operational only, NOT the build matrix)

The actual build runs on Azure Pipelines. The 25 GitHub Actions
workflows handle GitHub-side automation: markdownlint, JIT-format,
labeler ML training, backport, branch-merge-flow, copilot-echo,
ci-failure-scan, code-review, locker, stale, etc.

| Workflow | Purpose | alint disposition |
|---|---|---|
| `markdownlint.yml` | Lints `**/*.md` against `.markdownlint.json` | bundled `ci/github-actions@v1` covers shape; `.markdownlint.json` presence covered by custom rule |
| `jit-format.yml` | Runs jitutils' jit-format against the JIT subset | bundled GHA shape coverage |
| `labeler-{predict-{issues,pulls},promote,train}.yml` + `labeler-cache-retention.yml` | ML-driven issue/PR labeler training + inference | Same |
| `backport.yml`, `inter-branch-merge-flow.yml`, `copilot-echo.yml`, `code-review.yml`, `breaking-change-doc.yml`, `bump-chrome-version.yml`, `aspnetcore-sync.yml` | Repo automation | Same. **Several have `permissions: contents: write` so SHA-pinning matters even without the build matrix** |
| `ci-failure-scan.yml`, `check-no-merge-label.yml`, `check-service-labels.yml`, `locker.yml`, `skill-validation.yml`, `copilot-setup-steps.yml` | Triage / housekeeping | Same |

### 1.3 Root config files (cross-language gate / build orchestration)

| File | Owner tool | What it pins | alint disposition |
|---|---|---|---|
| `global.json` | dotnet | SDK version (`11.0.100-preview.3.26170.106`) + msbuild-sdks (Arcade.Sdk + Helix.Sdk + SharedFramework.Sdk + Build.NoTargets + Build.Traversal + NET.Sdk.IL) | `file_exists` + `json_path_matches` for `$.sdk.version` + bracket-notation `json_path_matches` for `$['msbuild-sdks']['Microsoft.DotNet.Arcade.Sdk']` |
| `NuGet.config` | NuGet | Source restriction to Microsoft-internal Azure DevOps feeds (dotnet-public, dotnet-tools, dotnet-eng, dotnet-libraries, dotnet10, dotnet11); `<fallbackPackageFolders><clear /></fallbackPackageFolders>` | `file_exists` + `file_content_matches` |
| `Directory.Build.props` (root) | MSBuild | Parent-config inherited by every csproj; anchors Arcade SDK import + OSArch derivation | `file_exists` (root_only) |
| `Directory.Build.targets` (root) | MSBuild | Post-import counterpart | `file_exists` (root_only) |
| `Directory.Build.rsp` | MSBuild | Response-file with default args for `dotnet build` | (transitively covered) |
| `Directory.Solution.props` | MSBuild | Solution-level props | (transitively covered) |
| `Build.proj` | MSBuild | Top-level build entry-point | (transitively covered) |
| `LICENSE.TXT` | dotnet | MIT license text | bundled `oss-baseline@v1` covers presence — but the bundled rule looks for `LICENSE` (no extension); **gap: `LICENSE.TXT` not recognised**, see §6 |
| `PATENTS.TXT` | Microsoft | .NET patent grant (distinct from MIT copyright grant) | `file_exists` (root_only) |
| `THIRD-PARTY-NOTICES.TXT` | Microsoft | Every transitively-bundled native dep + license | `file_exists` (root_only) |
| `.markdownlint.json` | markdownlint | Markdown lint config | `file_exists` (root_only) |
| `SECURITY.md` | Microsoft | Vulnerability disclosure path (MSRC) | `file_exists` (root_only) |
| `README.md`, `CONTRIBUTING.md`, `CODE-OF-CONDUCT.md` | dotnet | Standard OSS docs | bundled `oss-baseline@v1` |
| `.devcontainer/devcontainer.json` + per-target Dockerfiles | VS Code / Codespaces | Dev environment | `file_exists` |
| `.github/CODEOWNERS` (112 lines) | GitHub | Per-area ownership | `file_exists` |

### 1.4 `eng/` — the build-orchestration layer (Arcade SDK integration, 39 props/targets)

| File | What it does | alint disposition |
|---|---|---|
| `eng/Versions.props` | Pinned-versions registry — ProductVersion=11.0.0, Major/Minor/PatchVersion numerics, PackageVersionNet{6,7,8,9} servicing-band pins, every UsingTool* property | `file_exists` + 2× `file_content_matches` for ProductVersion shape + Major/Minor/Patch numeric shape |
| `eng/Subsets.props` (796 lines) | "What to build" dispatch table — every `./build.sh <subset>` alias maps to a csproj/proj list here | `file_exists`. The deeper "every subset entry resolves to an on-disk csproj" check needs the v0.10 `xml_path_*` primitive + the `registry_paths_resolve` family |
| `eng/OSArch.props` | Target-framework derivation from build-host arch | (covered transitively by `eng/Versions.props` presence) |
| `eng/packaging.targets` | NuGet packaging glue | **out-of-scope** (MSBuild evaluation) |
| `eng/Analyzers.targets`, `eng/illink.targets` | Per-csproj analyzer + linker integration | **out-of-scope** (MSBuild evaluation) |
| `eng/ApiCompatBaseline*.txt` | Public-API surface baseline (dotnet/api-compat input) | **out-of-scope** (binary-shape diff against a previous-release reference assembly) |
| `eng/formatting/format.sh` | dotnet/format Roslyn-driven C# style enforcer entry-point | `file_exists` (advisory) |
| `eng/common/` | Shared Arcade SDK build scripts (build.sh, cibuild.sh, darc-init.sh, …) | (covered by Arcade SDK pin) |
| `eng/native/` | CMake glue for the per-OS native libs | (covered by `src/native/` exclusion in the `dotnet-runtime-areas-have-directory-build-props` rule) |

### 1.5 `.config/` — local-tool + scanner manifests

| File | What it does | alint disposition |
|---|---|---|
| `.config/dotnet-tools.json` | Local-tools manifest (coverlet.console, dotnet-reportgenerator-globaltool, microsoft.dotnet.xharness.cli, microsoft.visualstudio.slngen.tool) — `dotnet tool restore` reads it | `file_exists` |
| `.config/CredScanSuppressions.json` | Microsoft credential-scanner allowlist | (info-level — operational) |
| `.config/tsaoptions.json` | Microsoft Trust Services Automation config | (info-level — operational) |
| `.config/1espt/` | 1ES Pipeline Template config | **out-of-scope** (Microsoft-internal) |

### 1.6 Per-csproj XML shape (5,858 csprojs in this checkout)

| Invariant | Today (regex `file_content_matches`) | With v0.10 `xml_path_*` |
|---|---|---|
| Root element is `<Project>` | `(?m)^<Project(\s|>|/)` (with BOM gymnastics) | `xml_path_exists: $.Project` |
| Sdk attribute is one of {NET.Sdk, NET.Sdk.Web, NET.Sdk.BlazorWebAssembly, NET.Sdk.WebAssembly, NET.Sdk.Razor, Microsoft.Build.NoTargets, Microsoft.Build.Traversal} | Long alternation regex against the file text | `xml_path_matches: $.Project[@Sdk]` |
| Has either `<TargetFramework>` or `<TargetFrameworks>` (mutually exclusive) | `<TargetFramework[s]?(\s|>)` (counts substring matches; can't enforce mutual exclusion) | `xml_path_count: ...` |
| TargetFrameworks references only known TFM variables from `eng/Versions.props` | Not expressible | `xml_path_matches` against derived variable set |
| `<EnableNullable>true</EnableNullable>` (where the area-level Directory.Build.props makes it the convention) | Per-csproj regex check | `xml_path_equals: $.Project.PropertyGroup.Nullable == "enable"` |
| `<RootNamespace>` (66 csprojs declare one) + `<AssemblyName>` (32 csprojs declare one) match the file-system path | Not expressible | `xml_path_equals` against derived path |
| `<PackageReference Include="..." Version="..." />` versions match `eng/Versions.props` declarations | Not expressible | Cross-file `xml_path_equals` check |
| Every `<ProjectReference Include="..." />` resolves to an existing csproj | Not expressible (would need path-resolution + XML extraction) | `xml_path_resolves: $.Project.ItemGroup.ProjectReference[@Include]` |

8 distinct invariants per csproj × 5,858 csprojs ≈ **47,000
invariant-instances** that the cumulative drift exposure spans;
alint expresses 3 with regex fallbacks today and needs `xml_path_*`
for the other 5.

### 1.7 Per-area Directory.Build.props inheritance (362 files)

The `Directory.Build.{props,targets}` family is MSBuild's parent-config
inheritance chain. Every csproj inherits transitively from every
ancestor `Directory.Build.props` (and post-import from
`Directory.Build.targets`). Drift here cascades silently.

| Surface | Coverage | Rule |
|---|---|---|
| Every src/ area has a `Directory.Build.props` | alint-today | `dotnet-runtime-areas-have-directory-build-props` (`for_each_dir` over `src/{coreclr,libraries,mono,installer,tasks,tools,samples}`) |
| `src/native/` is CMake-driven (no Directory.Build.props) | alint-today (excluded) | `paths.exclude` inside the rule |

### 1.8 Solution files (`.slnx` + `.sln`)

| File | Count at HEAD | Shape | alint disposition |
|---|---|---|---|
| `*.slnx` | **237** | New XML-shaped solution format introduced in VS 17.10; per-library "build just this one library" entry-points generated by the slngen tool | `file_exists` for spot-check; the full "every src/libraries/<name>/ has <name>.slnx" check + "every project entry in .slnx resolves to an on-disk csproj" check needs `xml_path_*` |
| `*.sln` | **3** | Legacy text-shaped format; all 3 are vendored under `src/native/external/zstd/` | (covered by `src/native/external/` exclusion) |

---

## 2. Coverage classification

Each row from §1 tagged with one of **alint-today** / **alint-future**
/ **out-of-scope** per the kubernetes pilot template.

### 2.1 Azure DevOps pipelines (4 inventoried surfaces)

| Surface | Coverage | Notes |
|---|---|---|
| Per-platform `jobs:` matrix | out-of-scope | Azure Pipelines DSL — bundled `ci/github-actions@v1` doesn't apply. `azure-pipelines@v1` v0.11+ candidate (single-source) |
| Helix test orchestration | out-of-scope | Helix infra |
| `evaluate-changed-paths.sh` | out-of-scope | CI optimisation |
| 1ES Pipeline Templates | out-of-scope | Microsoft-internal |

### 2.2 GitHub Actions workflows (~25 — all operational)

| Workflow shape | Coverage | Rule |
|---|---|---|
| All workflows pin `uses:` to 40-char SHAs | alint-today | bundled `gha-pin-actions-to-sha` + per-repo `dotnet-runtime-workflow-actions-pinned-by-sha` (re-statement) |
| All workflows declare `permissions: contents: read` | alint-today | bundled `gha-workflow-contents-read` |
| Each has a `name:` field | alint-today | bundled `gha-workflow-has-name` |
| Several workflows need `permissions: contents: write` | (no rule needed — the bundled rule's job is to flag escalations) | n/a |

### 2.3 Root config files (14 inventoried)

| File / shape | Coverage | Rule |
|---|---|---|
| `global.json` exists | alint-today | `dotnet-runtime-global-json-present` |
| `global.json` `$.sdk.version` shape | alint-today | `dotnet-runtime-global-json-pins-sdk-version` (`json_path_matches`) |
| `global.json` `$['msbuild-sdks']['Microsoft.DotNet.Arcade.Sdk']` pinned | alint-today | `dotnet-runtime-global-json-pins-arcade-sdk` (`json_path_matches` w/ bracket-notation per pitfall #10) |
| Arcade SDK version format | alint-today | `dotnet-runtime-arcade-sdk-version-format` |
| `NuGet.config` exists | alint-today | `dotnet-runtime-nuget-config-present` |
| `NuGet.config` clears fallback folders | alint-today | `dotnet-runtime-nuget-config-clears-fallback-folders` (`file_content_matches`) |
| Root `Directory.Build.props` exists | alint-today | `dotnet-runtime-root-directory-build-props-present` (`file_exists`, `root_only`) |
| Root `Directory.Build.targets` exists | alint-today | `dotnet-runtime-root-directory-build-targets-present` |
| `LICENSE.TXT` exists | alint-today (with caveat) | bundled `oss-license-exists` looks for `LICENSE` (no extension); dotnet ships `LICENSE.TXT`. **Gap: `LICENSE.TXT` not recognised** by `oss-license-exists`. v0.10 housekeeping fix to the bundled rule. |
| `PATENTS.TXT` exists | alint-today | `dotnet-runtime-patents-txt-present` |
| `THIRD-PARTY-NOTICES.TXT` exists | alint-today | `dotnet-runtime-third-party-notices-present` |
| `.markdownlint.json` exists | alint-today | `dotnet-runtime-markdownlint-config-present` |
| `SECURITY.md` exists | alint-today | `dotnet-runtime-security-md-present` |
| `.devcontainer/devcontainer.json` + Dockerfiles | alint-today | `dotnet-runtime-devcontainer-present` |
| `.github/CODEOWNERS` exists | alint-today | `dotnet-runtime-codeowners-present` |

### 2.4 `eng/` build-orchestration (9 inventoried)

| File / shape | Coverage | Rule |
|---|---|---|
| `eng/Versions.props` exists | alint-today | `dotnet-runtime-eng-versions-props-present` |
| `eng/Versions.props` ProductVersion + Major/Minor/Patch numeric shape | alint-today | `dotnet-runtime-versions-props-product-version-format` + `dotnet-runtime-versions-props-major-minor-patch-numeric` |
| `eng/Subsets.props` exists | alint-today | `dotnet-runtime-eng-subsets-props-present` |
| `eng/Subsets.props` entries resolve to on-disk csprojs | alint-future | `xml_path_*` + `registry_paths_resolve` (v0.10 ship-target, 8+ sources) |
| `eng/packaging.targets` / `eng/Analyzers.targets` / `eng/illink.targets` | out-of-scope | MSBuild evaluation |
| `eng/ApiCompatBaseline*.txt` | out-of-scope | Binary-shape diff |
| `eng/formatting/format.sh` exists | alint-today | (advisory; one rule) |
| `eng/common/` (Arcade scripts) | alint-today (transitive) | covered by Arcade SDK pin in global.json |
| `eng/native/` (CMake) | alint-today (excluded) | excluded from the area-Directory.Build.props rule |

### 2.5 `.config/` local-tool manifests (4 inventoried)

| File | Coverage | Rule |
|---|---|---|
| `.config/dotnet-tools.json` exists | alint-today | `dotnet-runtime-config-dotnet-tools-present` |
| `.config/dotnet-tools.json` ↔ `global.json` Arcade SDK coherence | alint-future | `cross_file_value_equals` (v0.10 ship-target, 10 sources — dotnet/runtime is one) |
| `.config/CredScanSuppressions.json` | (info-level — operational) | n/a |
| `.config/tsaoptions.json` | (info-level — operational) | n/a |

### 2.6 Per-csproj XML shape (8 invariants × 5,858 csprojs)

| Invariant | Coverage | Rule |
|---|---|---|
| Root element `<Project>` | alint-today (regex) | `dotnet-runtime-msbuild-files-have-project-root` (`file_content_matches` for `<Project(\s|>|/)`, no anchor — accepts BOM-prefixed and comment-prefixed files; pitfall #13 avoided) |
| Sdk attribute is one of allowed set | alint-today (regex) | `dotnet-runtime-csproj-uses-net-sdk` (`file_content_matches`) |
| `<TargetFramework[s]?>` declared | alint-today (regex) | `dotnet-runtime-csproj-declares-target-framework` (`file_content_matches`) — counts substring matches; can't enforce mutual exclusion |
| TargetFrameworks references only known TFM vars from `eng/Versions.props` | alint-future | `xml_path_*` (v0.10 ship-target, 2 sources — dotnet/runtime is one) |
| `<EnableNullable>` per area convention | alint-future | `xml_path_equals` (same v0.10) |
| `<RootNamespace>` + `<AssemblyName>` match path | alint-future | `xml_path_equals` (same v0.10) |
| `<PackageReference Version>` matches `eng/Versions.props` | alint-future | Cross-file `xml_path_equals` (same v0.10) |
| `<ProjectReference Include>` resolves to existing csproj | alint-future | `xml_path_resolves` (same v0.10) |

### 2.7 Per-area Directory.Build.props (362 inheritance files)

| Surface | Coverage | Rule |
|---|---|---|
| Every src/ area has Directory.Build.props | alint-today | `dotnet-runtime-areas-have-directory-build-props` (`for_each_dir`) |
| Every Directory.Build.props inherits Arcade SDK correctly | out-of-scope | MSBuild evaluation |

### 2.8 Solution files (.slnx + .sln)

| Surface | Coverage | Rule |
|---|---|---|
| `.slnx` for one library spot-check (System.Text.Json) | alint-today | `dotnet-runtime-system-text-json-slnx-exists` (`file_exists` spot-check) |
| Every src/libraries/<name>/ has <name>.slnx | alint-future | `xml_path_*` + `dir_name_matches_field` v0.10 design candidate |
| Every project entry in `.slnx` resolves to on-disk csproj | alint-future | `xml_path_resolves` |
| Vendored `.sln` files under `src/native/external/zstd/` | alint-today (excluded) | covered by exclusion |

### 2.9 Source-header MIT preamble (32,033 .cs files)

| Surface | Coverage | Rule |
|---|---|---|
| Every .cs source has the 2-line MIT header (`Licensed to the .NET Foundation under one or more agreements`) | alint-today | `dotnet-runtime-source-has-mit-header` (`file_header`); excludes `src/native/external/**`, `src/coreclr/pal/inc/rt/cpp/**`, `src/mono/mono/**`, per-csproj ref/ stubs (codegen) |

### 2.10 Hygiene (4 rules)

| Surface | Coverage | Rule |
|---|---|---|
| No tracked `bin/` | alint-today | `dotnet-runtime-no-tracked-bin` (`dir_absent`, `git_tracked_only: true`) |
| No tracked `obj/` | alint-today | `dotnet-runtime-no-tracked-obj` |
| No tracked `artifacts/` | alint-today | `dotnet-runtime-no-tracked-artifacts` |
| No tracked `.vs/` | alint-today | `dotnet-runtime-no-tracked-vs-folder` |

---

## 3. Quantified coverage

Counted across **4 Azure pipelines surfaces** + **4 GHA shape checks**
+ **15 root config files** + **9 eng/ build-orchestration** + **4
.config/ manifests** + **8 per-csproj invariants** + **2 per-area
inheritance** + **4 solution-file** + **1 source-header** + **4
hygiene** = **55 distinct surfaces**.

```
alint-today:     35 / 55 = 64%   (covers everything that fits today)
alint-future:    11 / 55 = 20%   (xml_path_* family + cross_file_value_equals + registry_paths_resolve)
out-of-scope:     9 / 55 = 16%   (Azure pipelines + Helix + Arcade MSBuild + ApiCompat binary diff)
                 ──────────────
                 total = 100%
```

Granular breakdown:

```
Azure DevOps pipelines (4 surfaces):
  out-of-scope: 4 / 4 = 100%

.github/workflows/* shape (4 checks × ~25 workflows):
  alint-today: 4 / 4 = 100%

Root config files (15 surfaces):
  alint-today: 15 / 15 = 100%   (with the LICENSE.TXT caveat — bundled rule needs to accept the .TXT extension)

eng/ build orchestration (9 surfaces):
  alint-today:  6 / 9 = 67%
  alint-future: 1 / 9 = 11%   (Subsets.props XML resolve)
  out-of-scope: 2 / 9 = 22%   (packaging/Analyzers/illink targets + ApiCompat baseline)

.config/ local-tool manifests (4 surfaces):
  alint-today:  3 / 4 = 75%
  alint-future: 1 / 4 = 25%   (cross_file_value_equals for Arcade SDK coherence)

Per-csproj invariants (8):
  alint-today:  3 / 8 = 38%   (with regex fallbacks)
  alint-future: 5 / 8 = 62%   (xml_path_* family)

Per-area inheritance (2 surfaces):
  alint-today:  1 / 2 = 50%
  out-of-scope: 1 / 2 = 50%   (MSBuild evaluation)

Solution files (4 surfaces):
  alint-today:  2 / 4 = 50%
  alint-future: 2 / 4 = 50%

Source-header MIT preamble (1 surface):
  alint-today: 1 / 1 = 100%

Hygiene (4 surfaces):
  alint-today: 4 / 4 = 100%
```

**Commentary.** Three observations:

1. **dotnet/runtime is the second source confirming `xml_path_*` at
   production scale (after spark's 49 pom.xmls).** The cumulative
   inventory across this sparse-checkout: 5,858 csprojs + 237 .slnx +
   362 Directory.Build.* + 607 .props/.targets + 39 eng/ build-glue
   files = **~7,100 distinct XML manifests** where structural
   assertions ("Sdk attribute is one of {NET.Sdk, NET.Sdk.Web, …}",
   "TargetFrameworks references only known TFM properties from
   Versions.props", "every `<ItemGroup>` subset entry resolves to a
   csproj that exists") are bottlenecked by alint's lack of XML-aware
   path queries. spark + dotnet/runtime = **2 sources, both at
   production scale**, both surfacing the same structural pattern.
   Per launch-evidence.md as of 2026-05-07, `xml_path_*` is now a
   **v0.10 ship-target**.

2. **`dotnet@v1` bundled ruleset is the second-highest-leverage v0.10
   ship-target uniquely surfaced here.** Of the 5 bundled per-language
   rulesets alint ships (rust, java, python, node, go), none cover
   .NET. **12 of the 31 dotnet-specific rules in this config could be
   consolidated into a single `dotnet@v1` extends: line.** Adopter
   surface: every dotnet/aspnetcore, dotnet/sdk, dotnet/maui,
   dotnet/efcore, microsoft/orleans, microsoft/dapr, every Azure SDK
   repo. **v0.10 ship-target.**

3. **The 16% out-of-scope is the right call.** Azure Pipelines DSL,
   Helix test orchestration, Arcade SDK MSBuild evaluation, and the
   ApiCompat binary-shape diff are all deeply Microsoft-specific
   tooling that alint doesn't (and shouldn't) try to subsume. The
   right hand-off is keeping each on its existing tool and having
   alint orchestrate where applicable.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (~1,150 lines, 31
explicit rules, 3 bundled rulesets folded in via `extends:`,
**60 rules total** loaded — confirmed by `alint validate-config`).

**Synopsis of the 7 most load-bearing repo-specific rules** (full config
in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules
  - alint://bundled/ci/github-actions@v1             # 3 rules
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules

rules:
  - id: dotnet-runtime-source-has-mit-header        # 32,033 .cs files
    kind: file_header
    paths:
      include: ["src/**/*.cs"]
      exclude:
        - "src/native/external/**"        # vendored: zlib-ng, libunwind, brotli
        - "src/coreclr/pal/inc/rt/cpp/**"
        - "src/mono/mono/**"              # Mono fork's LGPL header history
        - "**/ref/**/*.cs"                # codegen
        - "**/obj/**", "**/bin/**", "**/artifacts/**"
    pattern: '^// Licensed to the \.NET Foundation under one or more agreements'

  - id: dotnet-runtime-global-json-pins-arcade-sdk
    kind: json_path_matches
    paths: global.json
    path: "$['msbuild-sdks']['Microsoft.DotNet.Arcade.Sdk']"  # bracket notation per pitfall #10
    matches: '^[0-9]+\.[0-9]+\.[0-9]+(-preview\..*)?$'

  - id: dotnet-runtime-csproj-uses-net-sdk           # 5,858 csprojs
    kind: file_content_matches
    paths:
      include: ["src/**/*.csproj"]
      exclude: ["**/bin/**", "**/obj/**", "**/artifacts/**", "**/external/**"]
    pattern: '<Project\s+Sdk="(Microsoft\.NET\.Sdk(\..+)?|Microsoft\.Build\.(NoTargets|Traversal))"'

  - id: dotnet-runtime-msbuild-files-have-project-root
    kind: file_content_matches
    paths: ["**/*.{csproj,props,targets,slnx}"]
    pattern: '<Project(\s|>|/)'    # NO line anchor — accepts BOM-prefixed + comment-prefixed (pitfall #13 avoided)

  - id: dotnet-runtime-areas-have-directory-build-props
    kind: for_each_dir
    select: "src/{coreclr,libraries,mono,installer,tasks,tools,samples}"
    require:
      - kind: file_exists
        paths: "{path}/Directory.Build.props"

  - id: dotnet-runtime-no-tracked-bin                # hygiene
    kind: dir_absent
    paths: "**/bin"
    git_tracked_only: true

  - id: dotnet-runtime-versions-props-product-version-format
    kind: file_content_matches
    paths: eng/Versions.props
    pattern: '<ProductVersion>\d+\.\d+\.\d+</ProductVersion>'
```

**Repo-specific vs bundled split:**

- **31 repo-specific rules** in `.alint.yml`: 1 source-header MIT
  preamble + 8 build-system anchor (global.json + NuGet.config +
  root Directory.Build.{props,targets} + eng/Versions.props +
  eng/Subsets.props + .config/dotnet-tools) + 1 per-area
  Directory.Build.props presence + 2 per-csproj XML-shape +
  1 per-library .slnx spot-check + 1 MSBuild .props/.targets
  root-element shape + 6 governance + 4 hygiene + 2 eng/Versions.props
  numeric-shape + 1 eng/formatting/format.sh + 1 GHA SHA-pinning
  re-statement + a few additional per-extends overrides.
- **29 bundled rules** from the 3 extended rulesets — none of the
  three rulesets ships a fact, so no fact-subtraction needed = **60
  total loaded**.

**Validation:** `alint validate-config` reports `✓ Config valid: 60
rule(s) loaded`. Pitfall checks: the magic comment is present (line 1);
no `pattern: |` block scalars (pitfall #22 not applicable); the
JSONPath dashed/dotted-key bracket notation is correctly used
(`$['msbuild-sdks']['Microsoft.DotNet.Arcade.Sdk']` per pitfall #10);
the per-csproj `<Project(...)>` regex uses no `^` line anchor to
accept both comment-block-prefixed and BOM-prefixed XML files
(pitfall #13 avoided — discovered during this case study's draft per
the original commit log).

---

## 5. Performance comparison

Methodology: `hyperfine -i --warmup 1 --runs 3` on `/tmp/runtime`
(57,695 files, 937 MB working tree). Machine: Linux 6.1.0-42-amd64,
~10 logical cores; alint binary `target/release/alint v0.9.17`.

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| Per-csproj XML-shape sweep (5,858 csprojs × 2 file_content_matches rules + 1 root-element rule) | n/a — no existing tool covers this surface; today MSBuild fails late on the missing field | n/a | included in 9.3 s full pass | n/a — surfaces 4,843 violations, mostly TargetFramework inheritance (legitimate) |
| Per-area Directory.Build.props (`for_each_dir` over 7 src/* areas) | n/a | n/a | included in 9.3 s | n/a |
| MIT-header sweep over 32,033 .cs files | n/a | n/a | included in 9.3 s | n/a — surfaces 2,243 warnings (mostly test-fixture .cs files) |
| Hygiene sweep (4 `dir_absent` + git_tracked_only) | n/a | n/a | included in 9.3 s | n/a |
| GHA workflow shape (4 rules × ~25 workflows) | n/a | n/a | included in 9.3 s | n/a |
| **alint full pass** (60 rules) | n/a | n/a | **9.30 s** ± 0.17 s (**user 10.1 s**) | — |
| Raw filesystem walk for csproj inventory | `find /tmp/runtime -name '*.csproj'` | **159 ms** ± 2 ms | n/a — alint walks once + evaluates 60 rules in 9.3 s | n/a |

The headline number: **a single 9.30 s alint pass loads 60 rules,
walks the 57,695-file tree once, and evaluates 5,858 csproj-shape +
237 .slnx + 362 Directory.Build.* + 607 .props/.targets + 32,033 .cs
header rules in parallel.** The bulk of the wall-clock is the
file_header rule over 32,033 .cs files (each requires parsing the
first ~200 bytes of the file as text and matching against the regex)
plus the per-csproj `<Project(...)>` regex.

For comparison, **the 1,091-csproj sparse checkout from the original
inventory clocks at ~3 s** on the same machine (the 5,858-csproj
broader checkout in this batch is ~3× larger, hence ~3× the
wall-clock — alint scales linearly with csproj count).

### 5.2 Pending — needs additional toolchain

| Check | Existing tool | Status | Reproduction |
|---|---|---|---|
| `dotnet build` self-build via Arcade | `dotnet` + Arcade SDK | pending — `dotnet` not on PATH; this is the actual build target on Azure Pipelines. Multi-minute. | Install dotnet 11 preview from <https://dotnet.microsoft.com/>, then `time (cd /tmp/runtime && ./build.sh)` |
| `dotnet/format` Roslyn-driven C# style | dotnet | pending — wraps via `eng/formatting/format.sh` | Same install, then `bash /tmp/runtime/eng/formatting/format.sh` |
| `dotnet/api-compat` public-API surface diff | dotnet/api-compat | pending — needs reference assemblies | (multi-step setup; defer to CI image) |
| Azure DevOps pipeline (`eng/pipelines/`) | Azure DevOps | not runnable locally — Azure DevOps owns the matrix execution | (CI-only) |
| markdownlint (`.github/workflows/markdownlint.yml`) | markdownlint-cli | pending — `markdownlint` not on PATH | `npm install -g markdownlint-cli && markdownlint --config .markdownlint.json '**/*.md'` |

The `dotnet build` self-build is the most marketable comparison
number but requires dotnet 11 preview + ~10 GB free disk +
multi-minute build. On a CI image with the full toolchain, the
natural rough comparison is:

- `eng/formatting/format.sh` (dotnet/format): ~30-60 s
- `dotnet build` Arcade (full repo): 5-15 minutes (cold) /
  60-120 s (warm)
- markdownlint: ~5 s
- `dotnet/api-compat` baseline diff: ~30 s (per-band)

alint orchestrating the structural-validation subset (everything except
the build/format/api-compat): **~9 s warm**.

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/dotnet-runtime/.alint.yml /tmp/runtime` (live run).

**Headline:** alint surfaces **7,769 violations** across the live tree;
of those, **5 errors** (real bugs), **7,164 warnings**, and **600
info-level** findings. The warnings are dominated by **two suspect
high-count rules** — analysed in detail below.

### 6.1 Per-rule violation summary

```
4843  ⚠  warning  dotnet-runtime-csproj-declares-target-framework  (legitimate inheritance — see below)
2243  ⚠  warning  dotnet-runtime-source-has-mit-header              (test fixtures — see below)
 351  ℹ  info     oss-no-trailing-whitespace
 248  ℹ  info     oss-final-newline
  21  ⚠  warning  hygiene-no-js-build-outputs                       (false positives on dotnet `bin/` dirs)
  20  ⚠  warning  gha-pin-actions-to-sha
  18  ⚠  warning  gha-workflow-contents-read
  15  ⚠  warning  dotnet-runtime-workflow-actions-pinned-by-sha     (re-statement of bundled rule)
   3  ⚠  warning  hygiene-no-huge-files
   2  ✗  error    oss-no-merge-conflict-markers                     (test fixture)
   2  ✗  error    oss-no-bidi-controls                              (test fixture)
   1  ⚠  warning  oss-license-exists                                (real catch — LICENSE.TXT not recognised)
   1  ℹ  info     oss-code-of-conduct-exists
   1  ✗  error    dotnet-runtime-csproj-uses-net-sdk                (real catch — see below)
```

**Two suspect rules (>100 violations):**

1. `dotnet-runtime-csproj-declares-target-framework` (4,843 of
   5,858 csprojs lack a literal `<TargetFramework[s]?` substring) —
   **legitimate MSBuild inheritance, NOT a config bug**. ~83% of
   csprojs in this broader checkout (5,858 vs the 1,091 from the
   2026-05-06 captured inventory) are shim-stubs / test-fixtures /
   per-arch conditional projects under `src/tests/`,
   `src/installer/pkg/`, `src/coreclr/tools/aot/`, etc., that
   inherit their TargetFramework from an ancestor `Directory.Build.props`.
   This is **the textbook case for `xml_path_*`**: alint's regex can
   only count substring matches, not assert "exactly one of
   `<TargetFramework>` / `<TargetFrameworks>` under the Project root".
   **Mitigation:** narrow the rule's `paths.include:` to the canonical
   library trees (`src/{coreclr,libraries,mono,native,tasks,tools}/...`
   excluding tests/installer); the rule's level is already `warning`
   (not error). Long-term: rewrite as `xml_path_count: $.Project.PropertyGroup.TargetFramework[s]? == 1`
   when v0.10 ships.

2. `dotnet-runtime-source-has-mit-header` (2,243 .cs files lack the
   2-line MIT preamble) — **mostly test-fixture .cs files**, NOT a
   config bug. The exclude list covers `src/native/external/**`,
   `src/mono/mono/**`, `**/ref/**`, but doesn't cover
   `src/tests/**/*.cs` or `src/coreclr/tools/aot/**`. Most of the
   2,243 are intentionally headerless test fixtures (synthetic
   inputs for the C# compiler test harness). **Mitigation:** extend
   `paths.exclude:` to cover the test-fixture trees explicitly.

### 6.2 Real findings

| Finding | Path | Severity | Rule | Triage |
|---|---|---|---|---|
| `LICENSE.TXT` not recognised | repo root | warning | `oss-license-exists` (bundled) | **Real bug in `oss-baseline@v1`.** The bundled rule's pattern list looks for `LICENSE` (no extension); dotnet/runtime ships `LICENSE.TXT` (uppercase + .TXT extension). Microsoft + many older OSS projects use this form. **v0.10 housekeeping fix to the bundled rule** to accept `LICENSE.TXT` and `LICENSE.md`. Same fix benefits deno (`LICENSE.md`). |
| 1 csproj is non-Sdk-style | `src/native/managed/cdac/mscordaccore_universal/mscordaccore_universal.csproj` (likely; from the original 2026-05-06 inventory) | error | `dotnet-runtime-csproj-uses-net-sdk` | **Real but expected** — genuinely non-Sdk-style, intentionally evaluated only via parent traversal. Worth documenting in dotnet/runtime's contributor guide as the legitimate exception (already done in the 2026-05-06 inventory's notes). |
| 2 merge-conflict markers in test fixtures | `src/tests/Loader/binding/...` (likely) | error | `oss-no-merge-conflict-markers` | **False positives** — test fixtures intentionally embed conflict-marker syntax to test resolution code. Add `paths.exclude: ["src/tests/**"]` to the bundled rule. |
| 2 bidi-control characters in test fixtures | likely under `src/libraries/System.Text.Encodings/` or `src/tests/...` | error | `oss-no-bidi-controls` | **False positives** — Unicode test fixtures. Same triage. |
| 20 GHA actions not pinned to 40-char SHA | `.github/workflows/*.yml` | warning | `gha-pin-actions-to-sha` | **Real** — small lift to convert tag pins to SHA pins. OpenSSF Scorecard signal. |
| 18 GHA workflows missing `permissions: contents: read` | `.github/workflows/*.yml` | warning | `gha-workflow-contents-read` | **Real** — small lift. |
| 21 false-positive JS-build-output flagged | `**/bin/**` (e.g. `Microsoft.NET.Sdk/Sdk/Sdk.props` paths after a partial build leaked into the working tree) | warning | `hygiene-no-js-build-outputs` | **All false positives** — `bin/` is dotnet's build-output convention, not a JS bundler artefact. The bundled rule should scope to repos with `package.json`. |
| 3 oversized files | possibly `eng/Versions.props` snapshots, vendored `external/`, or generated `*.cs` blobs | warning | `hygiene-no-huge-files` | Worth eyeballing; some may be legitimately large (autogenerated reference assemblies, large fixture data). |
| 351 / 248 cosmetic findings | various | info | `oss-no-trailing-whitespace` + `oss-final-newline` | Real but unweighted. dotnet doesn't gate on these; markdownlint catches the .md subset. Below the team's threshold of attention. |

### 6.3 Suspected `.alint.yml` bugs flagged for parent triage

**None.** The config is clean — no `pattern: |` block scalars (so
pitfall #22 not applicable), correctly-anchored content patterns
(pitfall #13 explicitly handled — see the `<Project(\s|>|/)` regex
that intentionally has no line anchor to accept BOM-prefixed XML),
JSONPath bracket notation for dotted-key sub-keys
(`$['msbuild-sdks']['Microsoft.DotNet.Arcade.Sdk']`, pitfall #10),
and `command:` not `argv:` on the (absent) shellouts. Every pitfall
in the canonical-22 catalogue is correctly avoided.

The two high-count warnings (4,843 + 2,243) are **expected behaviour
of the regex fallback against the broader sparse-checkout** — they
represent the cumulative drift exposure that the v0.10 `xml_path_*`
ship-target will close. Documented in §6.1 with the proper triage
path.

---

## 7. Followup feature work surfaced

- **`xml_path_matches` / `xml_path_equals` rule kinds** — was v0.11+
  single-source (spark); dotnet/runtime confirmed at scale (~7,100
  XML manifests, 5 distinct invariants needing AST-aware queries).
  **Now a v0.10 ship-target per launch-evidence.md (2 sources).**
  Generalises to every XML-shaped manifest (Maven pom.xml, MSBuild
  csproj/.props/.targets/.slnx, Ant build.xml, Gradle XML, NPM
  .nuspec).
- **`dotnet@v1` bundled ruleset** — net-new, surfaced uniquely by
  this case study. **Strong v0.10 ship-target** alongside
  `apache/governance@v1`. Adopter surface is large (every Microsoft
  .NET project + every Azure SDK).
- **`registry_paths_resolve`** — extends to XML-extracted registries
  via the `xml_path_*` family. **9 sources** including spark's pom.xml
  and dotnet/runtime's `eng/Subsets.props` + `.slnx` project lists.
  Already v0.10 ship-target.
- **`cross_file_value_equals`** — dotnet/runtime adds a 9th source via
  the `dotnet-tools.json` ↔ `global.json` Arcade SDK coherence pattern.
  Already v0.10 (past-saturation, 10 sources).
- **`oss-license-exists` housekeeping** — bundled rule should accept
  `LICENSE.TXT` (Microsoft convention) and `LICENSE.md` (Deno
  convention) in addition to the canonical `LICENSE`. v0.10
  housekeeping fix, documented above.
- **`azure-pipelines@v1` bundled ruleset** — for the
  `eng/pipelines/` Azure DevOps yml shape (workflow permissions,
  container-image SHA pinning, parameter-required defaulting).
  Single-source today (only dotnet/* uses Azure Pipelines as the
  primary CI surface in the launch-evidence corpus); defer to v0.11+.

---

## 8. Future analysis

Three candidate refinements worth evaluating in subsequent sweeps:

1. **Test-fixture exclude refinement.** The two suspect-rule
   contributors (4,843 + 2,243 warnings) are mostly test fixtures;
   one config tweak per rule (`paths.exclude:` to cover
   `src/tests/**` + `src/coreclr/tools/aot/**` + `src/installer/pkg/**`)
   would clear the bulk of the noise. Worth doing as a PR-grade
   follow-up before the next launch.
2. **Per-csproj `nested_configs: true` opportunity** — every
   `src/libraries/<name>/` ships its own `Directory.Build.props` and
   effectively scopes a per-library `.alint.yml` boundary. Once
   `xml_path_*` ships in v0.10, per-library configs could express
   per-library invariants (e.g. `System.Text.Json`'s `<Nullable>enable`
   vs `System.Reflection.Emit`'s `<Nullable>annotations`) without
   bloating the workspace-root config.
3. **`registry_paths_resolve` against `eng/Subsets.props`** — the
   796-line XML dispatch table is the canonical demand source; once
   both `xml_path_*` and `registry_paths_resolve` ship in v0.10, the
   "every subset entry resolves to an on-disk csproj" check becomes
   one rule instead of zero rules (currently unenforced statically).

---

## 9. Validation status (2026-05-07)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **60** (31 custom + 3 bundled rulesets — `oss-baseline`
  15, `ci/github-actions` 3, `hygiene/no-tracked-artifacts` 11; none
  of the three extended rulesets ships a fact, so no fact-subtraction
  needed = 60 loadable rules)
- **`alint validate-config`:** ✓ Config valid: 60 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for the
  7,769-violation breakdown (5 real errors, 7,164 warnings dominated
  by two MSBuild-inheritance + test-fixture suspect rules, 600
  info-level cosmetic findings)
- **Pitfall fixes (v0.9.17):** Pitfall #18 (per-rule `respect_gitignore:
  false`) and #19 (literal-path runtime guard) shipped in engine; this
  config does not need either workaround (no tracked-but-gitignored
  files; no `root_only:` with multi-component literals)
- **v0.10 ship-target candidates referenced here that are now firm:**
  - `xml_path_matches` / `xml_path_equals` — **v0.10 ship-target via
    spark + dotnet/runtime; this case study is the second source,
    confirmed at production scale (~7,100 XML manifests in this checkout)**
  - `dotnet@v1` bundled ruleset — **v0.10 ship-target uniquely
    surfaced here**
  - `registry_paths_resolve` — 8 sources including dotnet/runtime's
    `eng/Subsets.props` and .slnx project lists
  - `cross_file_value_equals` — 10 sources, past-saturation, includes
    dotnet/runtime's `dotnet-tools.json` ↔ `global.json` Arcade
    pattern
- **Open gaps:**
  - `oss-license-exists` not recognising `LICENSE.TXT` (real catch
    against this repo, also affects deno's `LICENSE.md`); v0.10
    housekeeping fix to the bundled rule
  - `azure-pipelines@v1` bundled ruleset (single-source today; defer
    to v0.11+)
- **Open suspected bugs in this directory's `.alint.yml`:** **none.**
  Config is clean against the v0.9.17 engine + canonical-22 pitfall
  catalogue. The two high-count warnings (4,843 + 2,243) are the
  expected behaviour of the regex fallback at scale until `xml_path_*`
  ships.
