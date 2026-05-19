---
title: 'dotnet@v1'
description: 'Baseline conventions for .NET projects. alint bundled ruleset dotnet@v1.'
---

Baseline conventions for .NET projects. Adopt it with:

```yaml
extends:
  - alint://bundled/dotnet@v1
```

Gated with `when: facts.has_dotnet` (true if any .csproj /
.fsproj / .vbproj / .sln / global.json exists anywhere in the
tree) so the whole ruleset is a silent no-op in non-.NET
repositories — useful in polyglot monorepos where a .NET
project sits alongside other ecosystems. Override `has_dotnet`
with your own `facts:` block if your project uses a
non-standard layout.

The structural checks use the structured-query family
(`json_path_matches` on global.json, `xml_path_*` on the
MSBuild XML). The structured-query rules are `if_present: true`
— they flag a *misconfiguration*, never force a property to
exist — because this ruleset's adopter surface is every
dotnet/* + every Azure SDK + every microsoft/* .NET project.
Levels are deliberately non-blocking (no `error`); upgrade
severity in your own config when you are ready to enforce.

Central Package Management note: this ruleset deliberately does
NOT require a `Version` on each `<PackageReference>` — CPM
(Directory.Packages.props) makes that attribute absent by
design. Instead it checks that, *if* a Directory.Packages.props
exists, CPM is actually enabled.

## Rules

### `dotnet-global-json-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **when**: `facts.has_dotnet`
- **policy**: <https://learn.microsoft.com/dotnet/core/tools/global-json>

> .NET: a root global.json pins the SDK version so the build is reproducible across machines and CI.

### `dotnet-global-json-pins-sdk`

- **kind**: [`json_path_matches`](/docs/rules/structured-query/json_path_matches/)
- **level**: `warning`
- **when**: `facts.has_dotnet`
- **policy**: <https://learn.microsoft.com/dotnet/core/tools/global-json#version>

> .NET: global.json should pin a concrete SDK version (e.g. "sdk": { "version": "8.0.100" }) for reproducible builds.

### `dotnet-csproj-sdk-style`

- **kind**: [`xml_path_matches`](/docs/rules/structured-query/xml_path_matches/)
- **level**: `warning`
- **when**: `facts.has_dotnet`
- **policy**: <https://learn.microsoft.com/dotnet/core/project-sdk/overview>

> .NET: prefer SDK-style projects (`<Project Sdk="Microsoft.NET.Sdk">`). Legacy non-SDK-style .csproj is harder to maintain and tool.

### `dotnet-csproj-nullable-enabled`

- **kind**: [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/)
- **level**: `info`
- **when**: `facts.has_dotnet`
- **policy**: <https://learn.microsoft.com/dotnet/csharp/nullable-references>

> .NET: enable nullable reference types (`<Nullable>enable</Nullable>`) to catch null-dereference bugs at compile time.

### `dotnet-central-package-management`

- **kind**: [`xml_path_equals`](/docs/rules/structured-query/xml_path_equals/)
- **level**: `info`
- **when**: `facts.has_dotnet`
- **policy**: <https://learn.microsoft.com/nuget/consume-packages/central-package-management>

> .NET: a Directory.Packages.props should set <ManagePackageVersionsCentrally>true</ManagePackageVersionsCentrally> so Central Package Management is actually in effect.

### `dotnet-no-build-output-committed`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `warning`
- **when**: `facts.has_dotnet`
- **policy**: <https://learn.microsoft.com/dotnet/core/tools/>

> .NET: build output (`bin/`, `obj/`) must not be committed. Add it to .gitignore (the standard dotnet `.gitignore` excludes both).

### `dotnet-editorconfig-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **when**: `facts.has_dotnet`
- **policy**: <https://learn.microsoft.com/dotnet/fundamentals/code-analysis/configuration-files>

> .NET: ship a root .editorconfig — it is how .NET analyzer severities and `dotnet format` style are configured.

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/dotnet.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/dotnet.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/dotnet@v1
#
# Baseline conventions for .NET projects. Adopt it with:
#
#     extends:
#       - alint://bundled/dotnet@v1
#
# Gated with `when: facts.has_dotnet` (true if any .csproj /
# .fsproj / .vbproj / .sln / global.json exists anywhere in the
# tree) so the whole ruleset is a silent no-op in non-.NET
# repositories — useful in polyglot monorepos where a .NET
# project sits alongside other ecosystems. Override `has_dotnet`
# with your own `facts:` block if your project uses a
# non-standard layout.
#
# The structural checks use the structured-query family
# (`json_path_matches` on global.json, `xml_path_*` on the
# MSBuild XML). The structured-query rules are `if_present: true`
# — they flag a *misconfiguration*, never force a property to
# exist — because this ruleset's adopter surface is every
# dotnet/* + every Azure SDK + every microsoft/* .NET project.
# Levels are deliberately non-blocking (no `error`); upgrade
# severity in your own config when you are ready to enforce.
#
# Central Package Management note: this ruleset deliberately does
# NOT require a `Version` on each `<PackageReference>` — CPM
# (Directory.Packages.props) makes that attribute absent by
# design. Instead it checks that, *if* a Directory.Packages.props
# exists, CPM is actually enabled.

version: 1

facts:
  - id: has_dotnet
    any_file_exists:
      - "*.sln"
      - "**/*.csproj"
      - "**/*.fsproj"
      - "**/*.vbproj"
      - global.json

rules:
  # --- SDK pin -----------------------------------------------------
  - id: dotnet-global-json-exists
    when: facts.has_dotnet
    kind: file_exists
    paths: global.json
    root_only: true
    level: warning
    message: >-
      .NET: a root global.json pins the SDK version so the build
      is reproducible across machines and CI.
    policy_url: "https://learn.microsoft.com/dotnet/core/tools/global-json"

  - id: dotnet-global-json-pins-sdk
    when: facts.has_dotnet
    # global.json is JSON — use the structured-query family.
    # `if_present` so a global.json that only declares
    # `msbuild-sdks` (no `sdk.version`) is not flagged; the
    # existence rule above covers a missing file.
    kind: json_path_matches
    paths: global.json
    path: "$.sdk.version"
    matches: '^\d+\.\d+\.\d+'
    if_present: true
    level: warning
    message: >-
      .NET: global.json should pin a concrete SDK version
      (e.g. "sdk": { "version": "8.0.100" }) for reproducible
      builds.
    policy_url: "https://learn.microsoft.com/dotnet/core/tools/global-json#version"

  # --- Project shape (.csproj via xml_path_*) ----------------------
  - id: dotnet-csproj-sdk-style
    when: facts.has_dotnet
    # SDK-style projects open with `<Project Sdk="Microsoft.NET.Sdk...">`.
    # `if_present`: a .csproj that imports its SDK via
    # Directory.Build.props (no `Sdk=` attribute) is silent;
    # only an explicit non-Microsoft SDK (legacy / odd
    # third-party) fires. Matches `Microsoft.NET.Sdk`,
    # `Microsoft.NET.Sdk.Web`, `.Worker`, etc.
    kind: xml_path_matches
    paths: "**/*.csproj"
    path: "$.Project['@Sdk']"
    matches: 'Microsoft\.NET\.Sdk'
    if_present: true
    level: warning
    message: >-
      .NET: prefer SDK-style projects
      (`<Project Sdk="Microsoft.NET.Sdk">`). Legacy
      non-SDK-style .csproj is harder to maintain and tool.
    policy_url: "https://learn.microsoft.com/dotnet/core/project-sdk/overview"

  - id: dotnet-csproj-nullable-enabled
    when: facts.has_dotnet
    # `if_present`: only flags a .csproj that explicitly sets
    # Nullable to something other than `enable` (a regression);
    # a project that sets it centrally / omits it is not nagged.
    kind: xml_path_equals
    paths: "**/*.csproj"
    path: "$.Project.PropertyGroup.Nullable"
    equals: "enable"
    if_present: true
    level: info
    message: >-
      .NET: enable nullable reference types
      (`<Nullable>enable</Nullable>`) to catch null-dereference
      bugs at compile time.
    policy_url: "https://learn.microsoft.com/dotnet/csharp/nullable-references"

  - id: dotnet-central-package-management
    when: facts.has_dotnet
    # If the repo ships a Directory.Packages.props it should
    # actually enable CPM — a props file that doesn't is a
    # half-migrated state where package versions silently still
    # come from the .csproj files.
    kind: xml_path_equals
    paths: ["Directory.Packages.props", "**/Directory.Packages.props"]
    path: "$.Project.PropertyGroup.ManagePackageVersionsCentrally"
    equals: "true"
    if_present: true
    level: info
    message: >-
      .NET: a Directory.Packages.props should set
      <ManagePackageVersionsCentrally>true</ManagePackageVersionsCentrally>
      so Central Package Management is actually in effect.
    policy_url: "https://learn.microsoft.com/nuget/consume-packages/central-package-management"

  # --- Build-output hygiene (.NET-specific, ecosystem-gated) -------
  - id: dotnet-no-build-output-committed
    when: facts.has_dotnet
    # `bin/` and `obj/` are .NET build output. The generic
    # hygiene/no-tracked-artifacts@v1 catches build dirs broadly;
    # this is the .NET-targeted, ecosystem-gated companion (safe
    # to adopt both — namespaced ids). Disable on the rare repo
    # that vendors a tool's bin/.
    kind: dir_absent
    paths: ["**/bin", "**/obj"]
    level: warning
    message: >-
      .NET: build output (`bin/`, `obj/`) must not be committed.
      Add it to .gitignore (the standard dotnet `.gitignore`
      excludes both).
    policy_url: "https://learn.microsoft.com/dotnet/core/tools/"

  # --- Analyzer / style convention --------------------------------
  - id: dotnet-editorconfig-exists
    when: facts.has_dotnet
    # The .NET ecosystem drives analyzer + formatting rules
    # through .editorconfig (every microsoft/* .NET repo ships
    # one; `dotnet format` reads it).
    kind: file_exists
    paths: .editorconfig
    root_only: true
    level: info
    message: >-
      .NET: ship a root .editorconfig — it is how .NET analyzer
      severities and `dotnet format` style are configured.
    policy_url: "https://learn.microsoft.com/dotnet/fundamentals/code-analysis/configuration-files"
```
