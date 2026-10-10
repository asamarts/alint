# `dotnet@v1` — .NET ecosystem baseline (bundled ruleset)

Status: **Implemented** — lands with the ruleset in v0.10 (this
commit; item #10 of the case-study coverage push, the **final
v0.10 content item** — after this v0.10 is content-complete and
only the release itself remains). Was a design draft
(2026-05-18). v0.10 demand #10 (1 source, huge adopter surface,
ROADMAP-canonical). A **bundled ecosystem ruleset** composed of
existing rule kinds — **not** a new rule kind (rule-kind count
stays 79; bundled-ruleset count 20 → 21). Open questions
resolved on implementation: ecosystem-gated via
`facts.has_dotnet` like `go@v1`/`rust@v1` (Q1); per-
`PackageReference` `Version` deliberately **not** enforced —
Central Package Management makes it absent by design (Q2);
conservative levels (no `error`) given the adopter surface (Q3);
`xml_path_*` / `json_path_*` carry the structural checks — the
concrete payoff of #7 (Q4); composes with
`hygiene/no-tracked-artifacts@v1` (Q5).

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
("`dotnet@v1` | dotnet/runtime | adopter surface: every dotnet/*
+ every Azure SDK + every microsoft/* .NET project") and the
per-repo tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`dotnet@v1` row: dotnet-runtime — "1,091 `.csproj` + 234
solution files + 257 `Directory.Build.{props,targets}` ≈ 2,300
XML manifests"). Canonical scope:
[`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push)
(#10; "Single demand source but huge adopter surface … Depends
on `xml_path_*`" — now shipped as #7).

## Problem

The .NET ecosystem encodes project conventions in MSBuild XML
(`.csproj` / `Directory.Build.props` / `Directory.Packages.props`)
and a JSON SDK pin (`global.json`). Until #7
(`xml_path_*`) shipped, alint could not assert anything
structural about a `.csproj`; it now can, and the .NET adopter
surface is enormous (every `dotnet/*`, every Azure SDK, every
`microsoft/*` .NET repo) and each re-implements the same
baseline by hand or via bespoke MSBuild targets:

- an SDK pin (`global.json`) so the build is reproducible;
- SDK-style projects (`<Project Sdk="Microsoft.NET.Sdk">`),
  not legacy `.csproj`;
- nullable reference types on;
- if Central Package Management is used, it is actually enabled;
- no committed `bin/` / `obj/` build output;
- an `.editorconfig` (the .NET analyzer/style convention every
  Microsoft repo ships).

There is no ruleset a .NET repo can adopt in one line to get
that baseline — and it must **no-op cleanly** in the non-.NET
parts of a polyglot monorepo.

## Surface area

A new **bundled ecosystem ruleset**
`alint://bundled/dotnet@v1` (top-level name, like
`rust`/`node`/`python`/`go`/`java` — *not* namespaced;
`crates/alint-dsl/rulesets/v1/dotnet.yml`, registered in
`bundled.rs`'s `REGISTRY` in the "Ecosystem / project-shape
baselines" block). Composed **entirely of existing rule kinds**
— no Rust, no new rule kind, rule-kind count unchanged (79).
`version: 1`.

```yaml
extends:
  - alint://bundled/dotnet@v1
```

**Ecosystem-gated** exactly like `go@v1`: a `facts.has_dotnet`
fact (`any_file_exists` over `**/*.csproj` / `**/*.fsproj` /
`**/*.vbproj` / `*.sln` / `global.json`) and **every rule
`when: facts.has_dotnet`**, so the ruleset is a **silent no-op**
in a non-.NET repo (the "ecosystem-gated" contract README
documents). No fact gate beyond that — adopting it is the
signal. `dotnet-*` ids so it composes with
`hygiene/no-tracked-artifacts@v1` without collision.

### Rules (v1) — existing kinds only

| id | kind | check | level |
|---|---|---|---|
| `dotnet-global-json-exists` | `file_exists` | `global.json` at root (SDK pin) | warning |
| `dotnet-global-json-pins-sdk` | `json_path_matches` | `global.json` `$.sdk.version` ~ `^\d+\.\d+\.\d+` | warning |
| `dotnet-csproj-sdk-style` | `xml_path_matches` | `**/*.csproj` `$.Project['@Sdk']` ~ `Microsoft\.NET\.Sdk` | warning |
| `dotnet-csproj-nullable-enabled` | `xml_path_equals` | `**/*.csproj` `$.Project.PropertyGroup.Nullable` == `enable` | info |
| `dotnet-central-package-management` | `xml_path_equals` | `Directory.Packages.props` `…ManagePackageVersionsCentrally` == `true` | info |
| `dotnet-no-build-output-committed` | `dir_absent` | no `**/bin` / `**/obj` | warning |
| `dotnet-editorconfig-exists` | `file_exists` | `.editorconfig` at root | info |

Three of seven use the **structured-query family** (`json_path_matches`
on `global.json`, `xml_path_matches` + `xml_path_equals` on
`.csproj` / `.props`) — the concrete payoff that made #10 depend
on #7. The structured-query rules carry **`if_present: true`**:
they flag a *misconfiguration* (an SDK-style attr that is not
`Microsoft.NET.Sdk`, `Nullable` explicitly not `enable`, a
`Directory.Packages.props` that doesn't actually enable CPM) but
never force a property to be present — critical given the
adopter surface.

## Semantics

Standard bundled-ruleset semantics + the `go@v1` ecosystem
pattern:

- **Gating.** `facts.has_dotnet` short-circuits the entire
  ruleset off in non-.NET trees (the no-op scenario asserts
  `violations: []` on a README-only repo). `.csproj`-scoped
  rules are inherently scoped (their `paths` target `**/*.csproj`).
- **Conservative levels.** No `error` anywhere — one demand
  source but a vast adopter surface (every `microsoft/*` .NET
  repo); a baseline that hard-blocks them on first adoption is
  user-hostile. `warning` for the load-bearing-but-tunable
  (SDK pin, SDK-style, bin/obj), `info` for the
  recommendations (nullable, CPM, editorconfig). The ruleset
  header documents "upgrade severity in your own config".
- **`if_present` on every structured-query rule.** A `.csproj`
  that doesn't set `Nullable` is *not* flagged; only one that
  sets it to `disable`/`warnings` is. Likewise SDK attr and
  CPM. This is the single most important false-positive control.

## False-positive surface

- **Central Package Management (the big one).** dotnet/runtime
  and most large .NET repos use CPM: `<PackageReference>` then
  has **no `Version` attribute** (versions live in
  `Directory.Packages.props`). A "every `PackageReference` has
  `@Version`" rule would false-positive across the entire
  flagship adopter. v1 therefore **does not** check per-
  `PackageReference` versions at all; it instead checks that
  *if* `Directory.Packages.props` exists it actually enables
  CPM. The per-reference-pinning-without-CPM case is Open
  question 2 (needs cross-file conditional logic).
- **Centralised TFM.** `<TargetFramework>` is frequently set in
  `Directory.Build.props`, not each `.csproj`. v1 does **not**
  require a per-`.csproj` TFM (would FP on every centralised
  repo). Documented.
- **SDK attr via import.** Some repos set the SDK via
  `<Import>` / `Directory.Build.props` rather than the
  `Sdk=""` attribute. `dotnet-csproj-sdk-style` is
  `if_present` — a `.csproj` with no `@Sdk` is silent; only a
  non-`Microsoft.NET.Sdk` value (legacy `.csproj`, an odd
  third-party SDK) fires. Documented.
- **`bin/`/`obj` legitimately vendored.** Rare, but a repo that
  commits a vendored tool's `bin/` sets that one rule
  `level: off`. The generic `hygiene/no-tracked-artifacts@v1`
  also catches build dirs; `dotnet-no-build-output-committed`
  is the .NET-targeted, ecosystem-gated companion (safe to
  adopt both — namespaced ids).
- **`global.json` with comments / BOM.** The .NET SDK tolerates
  a UTF-8 BOM; strict JSON parse handles it. global.json is
  strict JSON (no comments) — a repo using JSON5 there is
  already non-conformant; documented.

## Implementation notes

- File `crates/alint-dsl/rulesets/v1/dotnet.yml` (top-level,
  in-crate so `include_str!` / `cargo publish` bundle it; cf.
  `feedback_include_str_stays_in_crate`).
- `bundled.rs`: one `("dotnet", "v1",
  include_str!("../rulesets/v1/dotnet.yml"))` row in the
  ecosystem block (after `java`).
- Pure config: no Rust, no new rule kind, not spawn-capable
  (the `SPAWNING_RULE_KINDS` checklist item is N/A — bundled
  rulesets can't introduce `command`-class rules; `bundled.rs`
  + `reject_command_rules_in` enforce this).
- `facts: - id: has_dotnet / any_file_exists: [...]` then every
  rule `when: facts.has_dotnet` — identical mechanism to
  `go@v1` (`facts.has_go`).
- Bundled-ruleset count `20 → 21` (README ×2 + the enumerated
  list at README:44 + `docs/site/about/index.md`);
  `coverage_audit_readme_claims` derives the number from the
  `rulesets/v1/` `.yml` count, so the prose must match. The
  **rule-kind count is unchanged (79)**.

## Tests

- `coverage_audit_bundled_rulesets`: a **WELL-FORMED** scenario
  (a .NET tree with global.json pinning an SDK, an SDK-style
  nullable-enabled `.csproj`, a CPM `Directory.Packages.props`,
  `.editorconfig`, no `bin/obj` ⇒ `violations: []`) and an
  **ILL-FORMED** scenario (a `.csproj` present but no
  `global.json`, a legacy non-SDK `.csproj`, `Nullable` set to
  `disable`, a committed `obj/` ⇒ concrete expected
  violations).
- A **no-op-in-non-.NET-repo** scenario (a README-only tree ⇒
  `violations: []`) — asserts the `facts.has_dotnet` gate, the
  ecosystem contract (mirrors `go_ruleset_no_ops_in_non_go_repo`).
- `docs/rules.md` gains an `### alint://bundled/dotnet@v1`
  subsection; CHANGELOG `[Unreleased]` Added (the tenth and
  final v0.10 content item). `xtask docs-export --check`
  unaffected (bundled rulesets aren't rule kinds); existing
  bundled audits stay green. Full preflight + dogfood green.
- **Bench-compare threshold:** adds no rule kind, no hot-path
  code; opt-in config. No bench impact (`xtask bench-gate` per
  `RELEASING.md`).

## Open questions

Resolve inline when implementation lands.

1. **Ecosystem gating.** Resolved: `facts.has_dotnet`
   (`any_file_exists` over `.csproj`/`.fsproj`/`.vbproj`/`.sln`/
   `global.json`) + per-rule `when:`, exactly like `go@v1`.
2. **`PackageReference` version pinning without CPM.** "Every
   `PackageReference` has a `Version` *unless*
   `Directory.Packages.props` exists" is a cross-file
   conditional v1's per-rule model can't express cleanly; a
   future `cross_file_value_equals`-style or fact-gated rule
   could. Deferred — enforcing it naively FPs across
   CPM repos (dotnet/runtime).
3. **`azure-pipelines@v1`.** The case-study tracker lists a
   future `azure-pipelines@v1` (CI YAML) for the .NET adopter
   surface. Separate ruleset, out of scope for `dotnet@v1`.
4. **TFM currency.** "Don't target EOL frameworks
   (`netcoreapp3.1`)" is date/version-relative — needs a
   fact/clock. Deferred; v1 checks shape, not currency.
5. **Relationship to `hygiene/no-tracked-artifacts@v1`.**
   Composable, namespaced ids; the generic ruleset catches
   build dirs broadly, `dotnet@v1` adds the .NET-gated
   `bin/`/`obj` companion. Documented, not merged.
