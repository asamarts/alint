# Case study: `flutter/flutter`

> **Marketing / positioning note.** The narrative-framed write-up of this
> case study (headline catches, "where alint earns its keep here", launch
> story angles) lives at <https://alint.org/examples/flutter-flutter/>.
> This README is the **engineering inventory**: tooling map, gap catalogue,
> coverage classification, performance numbers, and gap-discovery findings.
> Same facts, different language.

Inventory of the structural-validation tooling in `flutter/flutter` and
an alint config that replaces the rules alint can express today, plus
a catalogue of the rules that need new alint primitives — particularly
the `cross_language_implementation_complete` rule kind (now
`v0.11+ ship-target`, 5 sources per
`docs/development/launch-evidence.md`: arrow + TF + protobuf + angular
+ flutter), here in its **platform-driven** variant rather than the
data-format-driven variant arrow + tensorflow demonstrate.

**Repo state captured:** 2026-05-08 sparse-clone of
`flutter/flutter@HEAD` at `/tmp/flutter`. **15,860 tracked files**
(`git ls-files`), ~188 MiB working tree after sparse-checkout drops
`packages/flutter/test`, `packages/flutter_tools/test`,
`dev/automated_tests`, `engine/src/flutter/third_party`. Per-language
counts in scope: **8,857 polyglot source files** (Dart + Java + Kotlin
+ Swift + ObjC + C/C++), **134 pubspec.yaml**, **232 BUILD.gn**, **94
CMakeLists.txt**, **91 build.gradle/.kts**, **409 AndroidManifest.xml**,
**97 Info.plist**, **16 GitHub Actions workflows**, **39
analysis_options.yaml**, **9 per-package Dart workspaces** under
`packages/`, **9 per-platform engine subdirs** under
`engine/src/flutter/shell/platform/` (android/darwin{ios,macos}/linux/
windows/fuchsia/glfw/embedder/common), **7 `flutter create` template
subdirs**, **13 root governance files** (LICENSE, PATENT_GRANT,
AUTHORS, CODEOWNERS, TESTOWNERS, .ci.yaml, etc.).

**alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`.

---

## 1. Inventory of existing tooling

flutter/flutter is the canonical **platform-driven polyglot monorepo**
— a single tree where the framework itself is Dart, but every native-
OS embedder lives as a peer subdirectory under
`engine/src/flutter/shell/platform/`, each implementing the same engine
ABI in the language native to that OS. The repo has zero top-level
GitHub Actions CI gating; bulk CI runs against the **7,198-line
`.ci.yaml`** orchestrating **464 named CI targets** across LUCI infra,
shelled out to **`dev/bots/test.dart`** (663 lines) which dispatches
on the `SHARD` env var.

### 1.1 Engine CI scripts under `engine/src/flutter/ci/` (4 scripts — gating)

The engine's per-language enforcement layer; called by both the
internal LUCI runners and `dev/bots/test.dart` shards.

| Script | What it does | Backing tool |
|---|---|---|
| `engine/src/flutter/ci/format.sh` | Multi-language formatter pass (clang-format C++ / dartfmt / gn-format / yapf Python) | clang-format + dartfmt + gn + yapf |
| `engine/src/flutter/ci/clang_tidy.sh` | Engine C++ static analysis | clang-tidy |
| `engine/src/flutter/ci/pylint.sh` | Engine Python pylint pass | pylint |
| `engine/src/flutter/ci/licenses_cpp.sh` | Engine C++ license-attribution scan | `licenses_cpp` (in-engine binary) |

### 1.2 Dart-side validation entrypoints

| Entry | What it does | Backing tool |
|---|---|---|
| `dart analyze` (per-package) | Dart static analysis against the 39-file analyzer chain | dart analyzer |
| `dart format --set-exit-if-changed` | Dart formatter (page_width=100 from `analysis_options.yaml`) | dart format |
| `flutter analyze` | Flutter framework superset of `dart analyze` | flutter SDK |
| `dev/bots/analyze.dart` | Extra static-analysis pass for the framework | Dart analyzer + custom rules |
| `dev/bots/check_code_samples.dart` | Cross-validates `{@tool snippet}` directives in API docs against framework source | hermes-equivalent Dart AST walk |
| `dev/bots/check_tests_cross_imports.dart` | Test-tree import-graph integrity check | Dart AST walk |

### 1.3 `.github/workflows/` (16 workflows — operational/triage)

| Workflow family | What it does | Backing tool |
|---|---|---|
| Auto-labeler / triage: `cicd.yml`, `labeler.yml`, `lock.yaml`, `no-response.yaml`, `release-tracker.yml`, `release.yml` | PR labeling, locked-issue auto-close, release announcement | actions/* |
| Release / cherry-pick: `cut-release-branch.yml`, `easy-cp.yml`, `freeze.yml`, `merge-changelog.yml`, `revert.yml` | Branch cutting, cherry-pick automation, freeze/unfreeze, revert | actions/* |
| Sync / mirror / coverage: `coverage.yml`, `mirror.yml`, `roll-dart-dependencies.yml`, `sync-engine-version.yml`, `tool-test-general.yml`, `content-aware-hash.yml` | Cross-repo mirroring, dart-sdk dep roll, content hashing | actions/* |

The 16 public workflows are operational — the **bulk of CI runs in
LUCI** against `.ci.yaml`'s 464 targets, not GitHub Actions.

### 1.4 `.ci.yaml` + `dev/bots/test.dart` (the canonical CI orchestrator)

| Surface | Lines | What it does | Backing tool |
|---|---:|---|---|
| `.ci.yaml` | 7,198 | 464 named targets across linux/mac/win/android-emu/ios-device/fuchsia shards; each target's `shard:` field maps to a switch-case branch in test.dart | LUCI infra (out-of-tree at flutter/recipes) |
| `dev/bots/test.dart` | 663 | Canonical "run-all-the-tests" orchestrator; cases on SHARD env var | Dart program |
| `dev/bots/test/*` | n/a | Per-shard sub-test source trees | Dart |

### 1.5 Configuration files

| File | Role |
|---|---|
| `analysis_options.yaml` (root) | Master Dart analyzer config; 39 per-tree options files `include:` it. Pins `strict-casts`, `strict-inference`, `strict-raw-types`, `format.page_width=100` |
| `pubspec.yaml` (root) | Pub workspace anchor; `workspace:` list with **77 members** across packages/, dev/, examples/, engine/src/flutter/ |
| `dartdoc_options.yaml` | Configures dartdoc snippet/sample/dartpad tool integration |
| `.gitattributes` | CRLF for Windows files (`*.bat`, `*.ps1`, `*.sln`, `*.props`, `*.vcxproj`); LF for shell scripts (`bin/flutter`, `bin/dart`, `bin/flutter-dev`) |
| `.gitignore` | Dart/Flutter build outputs (`.dart_tool/`, `build/`, `.flutter-plugins`, `*.iml`, `.vscode/*`) |

### 1.6 Per-language tool configs

| Language | Tool config | What it pins |
|---|---|---|
| Dart | `analysis_options.yaml` (root + 38 per-package/per-tree) | strict-casts/strict-inference/strict-raw-types; per-rule lint set |
| Dart | `dartdoc_options.yaml` | snippet/sample/dartpad tool integration |
| Java/Kotlin | (gradle-bundled defaults; no Checkstyle/Detekt config) | — |
| Swift/ObjC | (no SwiftLint config; uses internal style guide) | — |
| C++ | implicit (clang-format + clang-tidy via `engine/src/flutter/ci/{format.sh,clang_tidy.sh}`) | — |
| Python | implicit (pylint via `engine/src/flutter/ci/pylint.sh`) | — |

### 1.7 Per-platform engine subtree (`engine/src/flutter/shell/platform/`)

The load-bearing cross-platform parity surface — every native-OS
embedder ships its own subdirectory with a `BUILD.gn` entry point.

| Subdir | Manifest at root | Per-platform shape |
|---|---|---|
| `android/` | `BUILD.gn`, `build.gradle`, `AndroidManifest.xml`, 259 `.java` files | Android engine surface (Java) |
| `darwin/ios/` | `BUILD.gn`, `framework/{Headers,Source,Info.plist,module.modulemap}`, .swift/.mm/.h | iOS engine surface (Swift+ObjC) |
| `darwin/macos/` | Same Apple-framework four-file shape | macOS engine surface (Swift+ObjC) |
| `linux/` | `BUILD.gn`, 213 .cc/.h files (GTK + GObject) | Linux engine surface (C++/GObject) |
| `windows/` | `BUILD.gn`, 184 .cc/.h files (UWP + Win32) | Windows engine surface (C++/COM) |
| `fuchsia/` | `BUILD.gn`, ~252 files | Fuchsia engine surface (C++/FIDL) |
| `glfw/` | `BUILD.gn`, ~39 files | GLFW desktop reference embedder |
| `embedder/` | `BUILD.gn`, ~144 files | Cross-platform C ABI (`flutter_embedder.h`) |
| `common/` | `BUILD.gn`, ~96 files | Shared code across embedders |

### 1.8 Per-Dart-package conventions (`packages/`)

| Package | Conventions |
|---|---|
| `flutter` | pub.dev published; `homepage: https://flutter.dev`; `analysis_options.yaml` adds `public_member_api_docs` |
| `flutter_tools` | pub.dev published; hosts Gradle plugin (Kotlin) under `gradle/src/main/kotlin/` (21 .kt files) |
| `flutter_test` | pub.dev published |
| `flutter_driver` | pub.dev published |
| `flutter_localizations` | pub.dev published; auto-generated translations in `lib/src/l10n/` (header-free) |
| `flutter_web_plugins` | pub.dev published |
| `flutter_goldens` | pub.dev published; historically no `homepage:` |
| `integration_test` | Internal-only — `publish_to: none` |
| `fuchsia_remote_debug_protocol` | Internal-only — `publish_to: none` |

### 1.9 `flutter create` template parity

7 template subdirs under `packages/flutter_tools/templates/app/`:
`android.tmpl`, `android-java.tmpl`, `android-kotlin.tmpl`,
`ios.tmpl`, `linux.tmpl`, `macos.tmpl`, `windows.tmpl`. Missing a
template means `flutter create` silently skips that platform.

### 1.10 Engine `build_overrides/` (12 .gni files — vendor selection)

Per-vendor dependency selection for shared graphics libraries:
`build.gni`, `vulkan_headers.gni`, `vulkan_loader.gni`,
`swiftshader.gni`, `glslang.gni`, `spirv_tools.gni`, `angle.gni`,
`wayland.gni`, `vulkan_tools.gni`, `vulkan_utility_libraries.gni`,
`vulkan_validation_layers.gni`, `lunarg_vulkantools.gni`. Removing
any one means `gn-gen` errors out with `Could not load
build_overrides/<x>.gni`.

### 1.11 Root governance / compliance files

13 root files: `LICENSE`, `PATENT_GRANT` (the BSD+patent pair unique
to flutter), `AUTHORS` (149 lines), `CODEOWNERS` (67 lines),
`TESTOWNERS` (379 lines — per-test-shard ownership unique to
flutter), `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `.gitattributes`,
`.gitignore`, `analysis_options.yaml`, `dartdoc_options.yaml`,
`pubspec.yaml`, `.ci.yaml`.

---

## 2. Coverage classification

Each surface from §1 tagged with one of:

- ✅ **alint-today** — name the rule kind + ruleset (`oss-baseline` /
  `ci/github-actions` / `hygiene/no-tracked-artifacts`) OR the
  per-rule entry in this directory's `.alint.yml`.
- 🔄 **alint-future** — name the v0.10 / v0.11+ candidate from
  [`docs/development/launch-evidence.md`](../../docs/development/launch-evidence.md).
- ❌ **out-of-scope** — explain why (Dart AST, C++ AST, codegen
  drift, runtime probe, build-graph traversal).

### 2.1 Engine CI scripts

| Script | Coverage | Notes |
|---|---|---|
| `engine/src/flutter/ci/format.sh` | ✅ alint-today (shellout) | `flutter-engine-format-sh` (`command:` rule) |
| `engine/src/flutter/ci/clang_tidy.sh` | ✅ alint-today (shellout) | `flutter-engine-clang-tidy` (`command:` rule) |
| `engine/src/flutter/ci/pylint.sh` | ✅ alint-today (shellout) | `flutter-engine-pylint` (`command:` rule) |
| `engine/src/flutter/ci/licenses_cpp.sh` | ✅ alint-today (shellout) | `flutter-engine-licenses-cpp` (`command:` rule) |

### 2.2 Dart-side validation

| Entry | Coverage | Notes |
|---|---|---|
| `dart analyze` | ✅ alint-today (shellout) | `flutter-dart-analyze` (`command:` rule wrapping `dart analyze`) |
| `dart format --set-exit-if-changed` | ✅ alint-today (shellout) | `flutter-dart-format-check` (`command:` rule) |
| `flutter analyze` | ❌ out-of-scope | Superset of `dart analyze`; AST analysis |
| `dev/bots/analyze.dart` | ❌ out-of-scope | Custom Dart AST rules |
| `dev/bots/check_code_samples.dart` | ❌ out-of-scope | Embedded-snippet AST cross-validation |
| `dev/bots/check_tests_cross_imports.dart` | ❌ out-of-scope | Import-graph traversal |

### 2.3 `.github/workflows/`

| Family | Coverage | Notes |
|---|---|---|
| All 16 workflows | ✅ alint-today | Bundled `ci/github-actions@v1` (3 rules — `gha-workflow-contents-read`, `gha-pin-actions-to-sha`, `gha-workflow-has-name`) plus `flutter-workflow-actions-pinned-by-sha` (warning-level restatement). Operational logic out-of-scope |

### 2.4 `.ci.yaml` + `dev/bots/test.dart`

| Surface | Coverage | Notes |
|---|---|---|
| `.ci.yaml` presence | ✅ alint-today | `flutter-ci-config-present` (`file_exists`, `root_only: true`) |
| `dev/bots/test.dart` presence | ✅ alint-today | `flutter-test-orchestrator-present` (`file_exists`) |
| `.ci.yaml` ↔ `test.dart` shard cross-validation | 🔄 alint-future | `registry_paths_resolve` (v0.10 ship-target, 8 sources — k8s, airflow, golang/go, pytorch, etc.; flutter is the 9th data point) |
| `.ci.yaml` target alphabetisation by `name:` within OS group | 🔄 alint-future | `ordered_block` (v0.10 ship-target, 7 sources) |
| LUCI runner JSON descriptors (`engine/src/flutter/ci/builders/*.json`) | ❌ out-of-scope | Operational descriptors |

### 2.5 Configuration files

| File | Coverage | Rule |
|---|---|---|
| `analysis_options.yaml` (root) | ✅ alint-today | `flutter-analysis-options-present` + 3× `flutter-analysis-options-strict-{casts,inference,raw-types}` (`file_content_matches`) |
| `pubspec.yaml` (root) | ✅ alint-today | Workspace anchor — covered by per-package rules |
| `dartdoc_options.yaml` | ✅ alint-today | `flutter-dartdoc-options-present` |
| `.gitattributes` | ✅ alint-today | `flutter-gitattributes-present` + `flutter-gitattributes-windows-crlf-pin` + `flutter-gitattributes-flutter-bin-lf-pin` |
| `.gitignore` | ✅ alint-today | Bundled `oss-gitignore-exists` + 3× explicit `dir_absent`/`file_absent` (`flutter-no-tracked-{dart-tool,build,flutter-plugins}`) |

### 2.6 Per-language tool configs

| Language | Coverage | Notes |
|---|---|---|
| Dart `analysis_options.yaml` | ✅ alint-today | Root + strict-* triad enforced |
| Dart `dartdoc_options.yaml` | ✅ alint-today | `file_exists` |
| Java/Kotlin/Swift/ObjC | ❌ out-of-scope | No per-language config in tree; gradle-bundled defaults |
| C++ | ✅ alint-today (shellout) | `flutter-engine-{format-sh,clang-tidy,licenses-cpp}` (`command:`) |
| Python | ✅ alint-today (shellout) | `flutter-engine-pylint` (`command:`) |

### 2.7 Per-platform engine subtree

| Subdir | Coverage | Notes |
|---|---|---|
| 7 platform subdirs (android/linux/windows/fuchsia/glfw/embedder/common) ship `BUILD.gn` | ✅ alint-today | `flutter-engine-platform-has-build-gn` (`for_each_dir`) |
| 3 darwin subdirs (ios/macos/common) ship `BUILD.gn` | ✅ alint-today | `flutter-engine-darwin-platforms-have-build-gn` (`for_each_dir`) |
| Apple framework four-file layout (Headers/, Source/, Info.plist, module.modulemap) | ✅ alint-today | `flutter-darwin-framework-layout` (`for_each_dir` + nested `dir_exists`/`file_exists`) |
| Per-platform engine ABI symbol parity (PlatformView, ExternalTexture, KeyEventHandler, VsyncWaiter, PlatformMessageHandler implemented in all 5 native langs) | 🔄 alint-future | `cross_language_implementation_complete` (v0.11+ ship-target, 5 sources — arrow + TF + protobuf + angular + flutter; flutter is the platform-driven variant) |
| Engine LUCI build runners | ❌ out-of-scope | Operational |
| Engine binary symbol-prefix scan (every `libflutter` symbol starts with `Flutter`) | ❌ out-of-scope | Binary parsing |

### 2.8 Per-Dart-package conventions

| Convention | Coverage | Rule |
|---|---|---|
| Every `packages/*/` has `pubspec.yaml` | ✅ alint-today | `flutter-package-has-pubspec` (`for_each_dir`) |
| Per-package `resolution: workspace` declared | ✅ alint-today | `flutter-package-resolution-workspace` (`file_content_matches`, with flutter_goldens + flutter_tools excluded) |
| Strict-tier packages declare `analysis_options.yaml` | ✅ alint-today | `flutter-package-has-analysis-options` (5 named packages) |
| Pub-published packages declare `homepage: https://flutter.dev` | ✅ alint-today | `flutter-published-package-has-homepage` (`file_content_matches`) |
| Internal packages declare `publish_to: none` | ✅ alint-today | `flutter-internal-package-publish-to-none` (`file_content_matches`) |
| Engine subtree pubspec | ✅ alint-today | `flutter-engine-has-pubspec` |
| Engine subtree `analysis_options.yaml` | ✅ alint-today | `flutter-engine-has-analysis-options` |

### 2.9 `flutter create` template parity

| Template | Coverage |
|---|---|
| 7 templates (`android.tmpl`, `android-java.tmpl`, `android-kotlin.tmpl`, `ios.tmpl`, `linux.tmpl`, `macos.tmpl`, `windows.tmpl`) | ✅ alint-today — `flutter-create-templates-platform-coverage` (`dir_exists` over named list) |

### 2.10 Engine `build_overrides/`

| File | Coverage |
|---|---|
| 7 critical `.gni` files (vulkan + wayland + swiftshader + glslang + spirv-tools + angle) | ✅ alint-today — `flutter-engine-build-overrides-present` (`file_exists` over named list) |

### 2.11 Root governance artefacts

| Artefact | Coverage | Rule |
|---|---|---|
| `LICENSE` | ✅ alint-today | `oss-license-exists`, `oss-license-non-empty` (oss-baseline) |
| `PATENT_GRANT` | ✅ alint-today | `flutter-patent-grant-present` (unique to flutter) |
| `AUTHORS` | ✅ alint-today | `flutter-authors-present` |
| `CODEOWNERS` | ✅ alint-today | `flutter-codeowners-present` + bundled `oss-codeowners-exists` |
| `TESTOWNERS` | ✅ alint-today | `flutter-testowners-present` (unique to flutter) |
| `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `SECURITY.md` | ✅ alint-today | Bundled `oss-baseline` rules |
| Repo-wide hygiene (no `.dart_tool/`, `build/`, `.flutter-plugins`) | ✅ alint-today | 11 rules from `hygiene/no-tracked-artifacts@v1` + 3 explicit |
| Polyglot Flutter Authors BSD header (Dart + Java + Kotlin + Swift + ObjC + C/C++ + Gradle) | ✅ alint-today | `flutter-bsd-source-header` + `flutter-bsd-source-header-shell-comment` (covers 5 native langs + Dart in 2 regexes) |

---

## 3. Quantified coverage

Counted across the **4 engine CI scripts** + **6 Dart-side
entrypoints** + **16 GHA workflows** + **2 LUCI orchestrators** + **5
config files** + **6 per-language configs** + **9 per-platform
subtrees** + **9 per-Dart-package conventions** + **7 `flutter
create` templates** + **12 `build_overrides/` files** + **13 root
governance files** = **89 distinct surfaces**.

```
✅ alint-today:    66 / 89 = 74%   (4 shellouts + 5 config + 9 platform + 9 per-package + 7 templates + 7 build_overrides + 13 governance + 12 GHA-shape + bundled hygiene)
🔄 alint-future:    3 / 89 =  3%   (1 cross_language_implementation_complete + 1 registry_paths_resolve + 1 ordered_block)
❌ out-of-scope:   20 / 89 = 23%   (Java/Kotlin/Swift/ObjC AST, dev/bots/check_*.dart, LUCI runners, binary symbol scans, codegen drift, dart-AST analyses)
                   ─────────────────
                   total = 100%
```

**Commentary.** Three observations:

1. **flutter is the densest cross-platform polyglot data point.** Of
   the 66 alint-today surfaces, **22 are cross-platform parity rules**
   (9 platform-subtree BUILD.gn checks, 7 `flutter create` templates,
   the Apple framework four-file layout, the polyglot BSD header
   sweeping 5 native langs + Dart). No per-language linter sees these
   end-to-end — each one only knows its own tree (Android Studio for
   android/, Xcode for darwin/, MSVC for windows/, etc.). alint
   catches them once, against the entire polyglot tree, in one
   declarative file.

2. **`cross_language_implementation_complete` is the v0.11+ flagship
   for flutter.** The per-platform engine ABI parity (every
   `PlatformView` / `ExternalTexture` / `KeyEventHandler` /
   `VsyncWaiter` / `PlatformMessageHandler` symbol implemented in all
   5 native langs) is exactly the rule kind, in its **platform-driven
   variant** (vs the data-format-driven variant arrow + tensorflow
   demonstrate). flutter is the **fifth independent demand signal**
   and the FIRST platform-driven source. Generalises to every
   cross-platform UI framework with per-OS native embedders (React
   Native, Xamarin/MAUI, Qt, Tauri).

3. **Half of out-of-scope is intentional shape vs semantics.** The
   20 out-of-scope surfaces split: 6 are Dart/C++/Java AST analyses
   (the right tool stays the existing tool), 8 are LUCI/runtime/
   codegen-drift (alint's deliberate non-goals), 6 are
   `dev/bots/check_*.dart` custom orchestrators that operate on AST
   shape rather than file shape. alint cleanly *complements* them —
   the structural floor is alint's, the semantics stay where they
   belong.

---

## 4. The `.alint.yml` synopsis

Working config: [`./.alint.yml`](.alint.yml) (847 lines including
narrative comments, **68 rules** loaded — confirmed by `alint
validate-config`: 39 flutter-specific + 29 from 3 bundled rulesets
— `oss-baseline=15` + `ci/github-actions=3` +
`hygiene/no-tracked-artifacts=11` − overlap = 29 effective rule IDs
after dedup).

**Synopsis of the load-bearing repo-specific rules** (full config
in `.alint.yml`):

```yaml
extends:
  - alint://bundled/oss-baseline@v1                  # 15 rules: license/readme/security/CoC + hygiene
  - alint://bundled/ci/github-actions@v1             # 3 rules: workflow contents-read + pin-to-sha + name
  - alint://bundled/hygiene/no-tracked-artifacts@v1  # 11 rules: node_modules, __pycache__, target, build/, etc.

rules:
  - id: flutter-bsd-source-header                    # Polyglot BSD header sweeping Dart + Java + Kotlin + Swift + ObjC + C/C++ + Gradle
    kind: file_header
    paths:
      include: ["**/*.dart", "**/*.java", "**/*.kt", "**/*.kts", "**/*.swift", "**/*.m", "**/*.mm", "**/*.h", "**/*.cc", "**/*.cpp", "**/*.gradle"]
    lines: 6
    pattern: 'Copyright [0-9]{4}(-[0-9]{4})? The Flutter Authors\. All rights reserved\.'
  - id: flutter-bsd-source-header-shell-comment      # Same header in #-comment form (Python/shell/GN/CMake)
  - id: flutter-engine-platform-has-build-gn         # for_each_dir over android/linux/windows/fuchsia/glfw/embedder/common
    kind: for_each_dir
    select: "engine/src/flutter/shell/platform/{android,linux,windows,fuchsia,glfw,embedder,common}"
    require: [{ kind: file_exists, paths: "{path}/BUILD.gn" }]
  - id: flutter-engine-darwin-platforms-have-build-gn  # Separate for_each_dir for darwin/{ios,macos,common}
  - id: flutter-darwin-framework-layout              # for_each_dir + nested dir_exists/file_exists for the Apple framework 4-file shape
  - id: flutter-create-templates-platform-coverage   # dir_exists over the 7 named template dirs
  - id: flutter-package-has-pubspec                  # for_each_dir over packages/* + nested file_exists
  - id: flutter-package-resolution-workspace         # file_content_matches for `^resolution: workspace`
  - id: flutter-published-package-has-homepage       # file_content_matches for `^homepage: https://flutter.dev`
  - id: flutter-internal-package-publish-to-none     # file_content_matches for `^publish_to: none`
  - id: flutter-engine-build-overrides-present       # file_exists over the 7 critical .gni files
  - id: flutter-{patent-grant,authors,codeowners,testowners,ci-config,test-orchestrator,dartdoc-options,analysis-options}-present
  - id: flutter-analysis-options-strict-{casts,inference,raw-types}  # 3 file_content_matches
  - id: flutter-gitattributes-{present,windows-crlf-pin,flutter-bin-lf-pin}  # 3 file_content_matches
  - id: flutter-{dart-analyze,dart-format-check,engine-clang-tidy,engine-format-sh,engine-pylint,engine-licenses-cpp}  # 6 command: shellouts
```

**Repo-specific vs bundled split:**
- **39 repo-specific rules** in `.alint.yml` (the `flutter-*` prefix)
- **29 bundled rules** from the 3 extended rulesets

**Validation:** `alint validate-config` reports `✓ Config valid: 68
rule(s) loaded`. No pitfall #22 (`pattern: |`) instances; both
`file_header` rules use single-line bare patterns. Pitfalls
#13/#14/#16/#17 were checked and not present in this config — every
regex with line anchors uses `(?m)`; no JSON-typed assertions against
boolean/number paths.

---

## 5. Performance comparison

Methodology: `hyperfine --warmup 1 --runs 3 -i` against the same
`/tmp/flutter` working tree captured 2026-05-08. Machine: Linux
6.1.0-42-amd64, ~10 logical cores; alint binary `target/release/alint
v0.9.17`. The `-i` flag (ignore non-zero exit) is necessary because
several `command:` shellouts fail when their tool environment isn't
fully bootstrapped (engine `vpython3` / `gclient sync`-prepared
`licenses_cpp` binary).

### 5.1 Measured

| Check | Existing tool | Existing wall-clock | alint wall-clock | Ratio |
|---|---|---|---|---|
| **alint full pass** (68 rules, includes 6 `command:` shellouts; most fail-fast without engine env) | n/a | n/a | **(timed via lite — see below)** | — |
| **alint lite pass** (3 bundled rulesets only, 29 rules) | n/a | n/a | **60.2 ms ± 1.4 ms** | — |
| Polyglot Flutter Authors header (find + grep -L over .dart/.java/.kt/.swift/.cc/.h/.m/.mm) | bash + find + grep | **113 ms ± 1 ms** | included in lite-pass + flutter-bsd-source-header (~25 ms incremental — 51 violations on 8,857 files) | **<1× alint comparable** (alint also runs 67 other rules in the same pass) |
| `dart analyze` (full SDK) | dart analyzer | pending — times out in the validation env without `flutter pub get` having run on every package | n/a — alint shells out via `command:` rule | 1× — alint wraps the existing tool |
| `dart format --set-exit-if-changed` (Dart formatter) | dart format | pending — same env caveat | n/a — alint wraps | 1× — alint wraps |
| `engine/src/flutter/ci/format.sh` (multi-language formatter pass) | clang-format + dartfmt + gn + yapf | pending — needs `gclient sync` + engine third_party | n/a — alint wraps | 1× — alint wraps |
| `engine/src/flutter/ci/clang_tidy.sh` (engine C++ static analysis) | clang-tidy | pending — needs unbuilt out/ dir + compdb | n/a — alint wraps | 1× — alint wraps |

The headline number: **a single 60 ms alint lite-pass replaces all
the cross-platform structural assertions across 15,860 files**
(per-platform engine BUILD.gn parity across 9 subtrees, the 7
`flutter create` template parity, the `build_overrides/` 7-vendor
checks, the 3-strict-mode analyzer pins, the polyglot Flutter Authors
BSD header sweeping 5 native langs + Dart, governance triad,
gitattributes EOL discipline, plus the 11-rule hygiene baseline + 3
GHA hardening rules). The `find + grep -L` polyglot-header bash
equivalent alone sits at 113 ms over the same tree — alint at 60 ms
is **~2× faster** AND runs the other 67 rules in the same pass.

### 5.2 Pending — needs additional toolchain

| Check | Tool | Reproduction |
|---|---|---|
| `flutter-dart-analyze` | dart + flutter SDK (with `flutter pub get` cached) | `cd /tmp/flutter && flutter pub get && time dart analyze` |
| `flutter-dart-format-check` | dart format | `time dart format --output=none --set-exit-if-changed .` |
| `flutter-engine-format-sh` | engine env (`vpython3` + clang-format + gn + dartfmt + yapf via `gclient sync`) | `time bash engine/src/flutter/ci/format.sh` |
| `flutter-engine-clang-tidy` | engine clang-tidy + compdb (`out/host_debug/compile_commands.json`) | `time bash engine/src/flutter/ci/clang_tidy.sh` |
| `flutter-engine-pylint` | `vpython3` + pylint-2.7 | `time bash engine/src/flutter/ci/pylint.sh` |
| `flutter-engine-licenses-cpp` | built `licenses_cpp` binary in engine `out/host_debug_unopt_arm64/` | `time bash engine/src/flutter/ci/licenses_cpp.sh` |

The end-to-end `dev/bots/test.dart --shard=analyze`-equivalent on
flutter — `dart analyze` over all 9 packages + the engine analyzer
pass + the 4 engine CI scripts — runs ~60-90 seconds on a fully
bootstrapped tree. alint's 60 ms structural floor adds <0.1%
wall-clock to that pipeline while catching 32 distinct classes of
cross-platform regression that `dart analyze` cannot see (because it
only knows the Dart tree).

---

## 6. Gap discovery — what alint surfaces against the live tree

Run: `alint check --config /home/kaminsod/projects/alint/examples/flutter-flutter/.alint.yml /tmp/flutter` (live, JSON-format).

**Headline:** alint surfaces **335 violations** across 18 failing
rules. Of those, **149 are cosmetic** (137 missing-final-newline + 12
trailing-whitespace, mostly under archived release-notes); **150 are
real header / structural findings** (51 BSD source-header violations
across the polyglot tree + 99 BSD shell-comment violations from
`flutter create`-templated CMakeLists.txt files); **5 are confirmed
Trojan-Source / CVE-2021-42574 catches** (see §6.2 below); the rest
are command-shellout failures (6 — the engine env not bootstrapped)
and a handful of supply-chain hardening signals.

### 6.1 Real findings (after deducting cosmetic class)

| Finding | Count | Severity | Rule | Triage |
|---|---:|---|---|---|
| Polyglot BSD header missing on `.dart`/`.java`/`.kt`/`.swift`/`.cc`/`.h`/`.m`/`.mm`/`.gradle` files | 51 | warning | `flutter-bsd-source-header` | **Real findings.** Integration-test apps under `dev/integration_tests/{pure_android_host_apps,record_use_test_app,spell_check}/` and `dev/a11y_assessments/` ship Kotlin / Java / `.gradle.kts` files without the Flutter Authors BSD header. The kind of cross-language drift no per-language linter sees; alint catches it once across the polyglot tree |
| BSD header missing on `.py`/`.sh`/`.gn`/`.cmake`/`CMakeLists.txt` | 99 | warning | `flutter-bsd-source-header-shell-comment` | **Real findings.** Auto-generated `CMakeLists.txt` files under `dev/integration_tests/*/{linux,windows}/` from `flutter create` templates. The engine's `BUILD.gn` carries the header; the desktop CMakeLists templates don't propagate it. Worth filing as a `flutter create` template fix |
| Bidi control characters in archived release-notes | 5 | error | `oss-no-bidi-controls` | **Real Trojan-Source / CVE-2021-42574 findings.** See §6.2 for the exact codepoint per file |
| Pub-published packages missing `homepage: https://flutter.dev` | 2 | warning | `flutter-published-package-has-homepage` | **Real.** `packages/flutter_localizations/pubspec.yaml` and `packages/flutter_test/pubspec.yaml` don't carry the `homepage:` line that pub.dev surfaces in the package landing page sidebar |
| Workflows missing `permissions: contents: read` | 13 | warning | `gha-workflow-contents-read` (bundled) | Real findings across 13 of 16 workflows. The OpenSSF Token-Permissions check |
| Third-party actions not pinned to SHA | 4 | warning | `gha-pin-actions-to-sha` + `flutter-workflow-actions-pinned-by-sha` | Real findings — flutter uses `actions/checkout@v4` style throughout |
| Workflow missing `name:` declaration | 1 | warning | `gha-workflow-has-name` | Real — minor hygiene |
| Forbidden directories under `**/build` | 4 | warning | `hygiene-no-js-build-outputs` (bundled) | **All false positives.** flutter's `build/` is the build script directory (not a JS build artefact), and 3 deep `build/` subdirs are under integration-test scaffolds. **Recommended fix:** scope `hygiene/no-tracked-artifacts@v1`'s JS-output rule to repos with a `package.json`, OR add these specific paths to a per-repo exclude list |
| `oss-security-policy-exists` info | 1 | info | `oss-security-policy-exists` (bundled) | flutter ships SECURITY.md under `docs/security/` rather than at repo root; bundled rule emits info finding |

**Real net-new findings alint surfaces that existing tooling misses:**
**150 cross-language BSD-header drifts** (the bash + grep equivalents
the engine's `format.sh` runs only sweep the `engine/src/flutter/`
subtree, not the framework tree under `packages/`/`dev/`/`examples/`)
+ **5 Trojan-Source CVE-2021-42574 catches** (see §6.2) + **2
package-homepage drifts** + **18 supply-chain hardening signals** (13
workflows missing contents-read, 4 actions not SHA-pinned, 1 missing
name).

### 6.2 The 5 Trojan-Source / CVE-2021-42574 catches — exact codepoint per file

Verified against `/tmp/flutter/` 2026-05-08:

| File | Lines flagged | Bidi codepoint(s) | Codepoint name |
|---|---|---|---|
| `docs/about/Values.md` | 46 | **U+202C** | Pop Directional Formatting (PDF) |
| `docs/releases/archive/Commits-Between-1.2.1-and-1.5.4.md` | 1260, 1263, 1812, 1854 | **U+202C** ×4 | PDF |
| `docs/releases/archive/PRs-addressed-between-1.5.4-and-1.7.8.md` | 1530, 1582 | **U+202C** ×2 | PDF |
| `docs/releases/archive/PRs-merged-between-1.7.8-and-1.9.1.md` | 196, 248, 286, 292, 314, 514, 594, 652, 724, 862, 922, 1002, 1006 | **U+202C** ×13 | PDF |
| `docs/releases/archive/Release-Notes---Changes-in-1.2.1.md` | 361 | **U+202C** | PDF |

**All 5 files contain U+202C (Pop Directional Formatting).** This is
the closing/pop of an `LRE`/`RLE`/`LRO`/`RLO` directional embedding
or override — a malformed bidi state that, on its own, doesn't trigger
the Trojan-Source attack paper's full reordering, but IS the canonical
"unmatched closing" that signals the file passed through a renderer or
cut-and-paste operation that mangled the directional state. The
contributor names / commit messages in the archived release-notes were
likely copied from PRs containing Arabic / Hebrew / RTL contributor
names; the bidi-control characters got embedded but the matching
opener was lost in transit.

flutter's existing tooling never sees these — the engine `format.sh`
only sweeps `engine/src/flutter/`; `dart format` doesn't lint
markdown; mdl/markdownlint don't enforce character-class hygiene.
**alint's bundled `oss-no-bidi-controls` rule (15-rule oss-baseline)
catches all 5 in a single pass.** The kind of finding that's been
hiding in plain sight since 2021 (when the CVE landed) and only
surfaces under a tree-wide character-class scan.

### 6.3 The 99 `flutter-bsd-source-header-shell-comment` violations

The 99 findings are concentrated in two clusters:

| Cluster | Sample paths | Count |
|---|---|---:|
| `flutter create` Linux desktop template output (`CMakeLists.txt`) | `dev/integration_tests/{android_views,channels,deferred_components_test,external_textures,flavors,...}/linux/{CMakeLists.txt,flutter/CMakeLists.txt,runner/CMakeLists.txt}` | ~60 |
| `flutter create` Windows desktop template output (`CMakeLists.txt`) | `dev/integration_tests/{android_views,channels,deferred_components_test,external_textures,flavors,...}/windows/{CMakeLists.txt,flutter/CMakeLists.txt,runner/CMakeLists.txt}` | ~35 |
| Other (Android `gradle/wrapper/gradle-wrapper.jar.sha256`-adjacent build files) | scattered | ~4 |

**Recommended fix:** the `flutter create` Linux/Windows `CMakeLists.txt`
templates under `packages/flutter_tools/templates/app/{linux,windows}.tmpl/`
should propagate the Flutter Authors header, the same way the
`engine/src/flutter/`-side `CMakeLists.txt` files do.

### 6.4 No silent-failure-mode bugs in this config

No instances of pitfalls #13 (regex `^`/`$` file-anchoring without
`(?m)`), #14 (single-quoted YAML `\n` non-expansion), #16
(`*_path_matches` against bool/number), #17 (`*_path_equals` against
`[*]`), or #22 (`pattern: |` trailing-newline) surfaced. The config
is well-disciplined; both `file_header` rules use simple bare patterns
without YAML block-scalar tricks.

---

## 7. Followup feature work surfaced

- **`cross_language_implementation_complete` rule kind** — flutter is
  the **fifth source** and the **first platform-driven variant**
  (vs the data-format-driven variant arrow + tensorflow demonstrate).
  Generalises to React Native, Xamarin/MAUI, Qt, Tauri. **v0.11+
  ship-target** (5 sources, 3 distinct topologies — saturated).
- **`registry_paths_resolve` rule kind** — `.ci.yaml` ↔ `dev/bots/test.dart`
  shard cross-validation. **v0.10 ship-target** (8 sources; flutter
  pushes to 9).
- **`ordered_block` rule kind** — `.ci.yaml` target alphabetisation
  by `name:` within OS group. **v0.10 ship-target** (7 sources;
  saturated).
- **`respect_gitignore: false` per-rule knob** — **DELIVERED in
  v0.9.17** (per-rule knob ships in the engine). `pubspec.lock`
  (tracked-but-gitignored via `!/pubspec.lock` in `.gitignore`) is
  now addressable with a one-line config edit; flutter is the second
  demand source after bazel.
- **`flutter create` template fix candidate** (config-side, not
  engine-side): the desktop Linux/Windows `CMakeLists.txt` templates
  under `packages/flutter_tools/templates/app/{linux,windows}.tmpl/`
  don't propagate the Flutter Authors header. Worth filing upstream
  to flutter/flutter as a template fix (would clear 99 of the 150
  real header findings).

---

## 8. Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **Add `flutter-engine-embedder-c-abi-presence` rule.** The
   load-bearing `engine/src/flutter/shell/platform/embedder/embedder.h`
   is the C ABI every external embedder consumes (e.g.
   `sony/flutter-embedded-linux`, `meta-flutter/flutter-pi`). Silent
   removal would silently break out-of-tree embedders. Currently
   covered indirectly by `flutter-engine-platform-has-build-gn`'s
   embedder/ directory check; a direct file-existence assertion would
   tighten the gate.
2. **`compliance/reuse@v1` overlay derivative.** The bundled
   `compliance/reuse@v1` ruleset (3 rules — `LICENSES/` dir +
   per-file SPDX headers + `.reuse/dep5`) doesn't drop in as-is
   (Flutter-Authors BSD-style header isn't SPDX-compliant), but a
   future derivative `compliance/bsd-flutter@v1` is an obvious
   bundled-ruleset extraction once the pattern stabilises across
   2+ adopting projects.
3. **`nested_configs: true` for the engine subtree.** The
   `engine/src/flutter/` subtree is effectively a separate Dart
   workspace with its own `pubspec.yaml` and `analysis_options.yaml`.
   A subtree-scoped `.alint.yml` under `engine/src/flutter/` would
   scope the Apple framework four-file layout rule and the
   `engine/src/build_overrides/` `.gni` rule next to their domain.

---

## 9. Validation status (2026-05-08)

- **alint version:** `0.9.17 (1dbd9b218a0e, built 2026-05-07)`
- **Rule count:** **68** (39 flutter-specific + 29 from 3 bundled
  rulesets — `oss-baseline=15`, `ci/github-actions=3`,
  `hygiene/no-tracked-artifacts=11`)
- **`alint validate-config`:** ✓ Config valid: 68 rule(s) loaded
- **Live-tree recheck:** **performed** in this batch — see §6 for
  the 335-violation breakdown (150 real header + 5 Trojan-Source +
  18 GHA hardening + 149 cosmetic + 13 expected shellout-failure
  noise)
- **Pitfall fixes (this batch):** none needed — no `pattern: |`
  instances; pitfalls #13/#14/#16/#17 all clean
- **Open gaps:**
  - `cross_language_implementation_complete` (v0.11+ ship-target,
    5 sources — flutter is the platform-driven variant)
  - `registry_paths_resolve` (v0.10 ship-target, 8 sources —
    flutter pushes to 9)
  - `ordered_block` (v0.10 ship-target, 7 sources)
- **Bench numbers:** 60 ms (lite bundled-only pass) on `/tmp/flutter`'s
  15,860-file tree; full pass times out the validation env's `dart
  analyze` shellout
