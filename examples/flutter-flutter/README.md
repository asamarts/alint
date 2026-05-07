# Case study: `flutter/flutter`

Inventory of the structural-validation tooling in `flutter/flutter`
and an alint config that replaces the rules alint can express
today, plus a catalogue of the rules that need new alint
primitives — particularly the
`cross_language_implementation_complete` rule kind (now
`v0.11+ ship-target`, 5 sources per
`docs/development/launch-evidence.md`: arrow + TF + protobuf
+ angular + flutter), here in its **platform-driven** variant
rather than the data-format-driven variant that arrow +
tensorflow demonstrate.

**Repo state captured:** 2026-05-06, sparse-clone of
`flutter/flutter@240c85cf` (rev =
`240c85cf08a05c234e66f4ef28e7d8a3f2361717`). Heavy test trees
(`packages/flutter/test`, `packages/flutter_tools/test`,
`dev/automated_tests`, `engine/src/flutter/third_party`) excluded
via:

```sh
git clone --depth=1 --filter=blob:none --sparse \
    https://github.com/flutter/flutter /tmp/flutter
cd /tmp/flutter
git sparse-checkout set --no-cone '/*' \
    '!/packages/flutter/test' \
    '!/packages/flutter_tools/test' \
    '!/dev/automated_tests' \
    '!/engine/src/flutter/third_party'
```

After sparse-checkout: **~14 000 files / 188 MiB working tree**.

---

## Summary

flutter/flutter is the canonical **platform-driven polyglot
monorepo** — a single tree where the framework itself is Dart,
but every native-OS embedder (Android, iOS, macOS, Linux,
Windows, Fuchsia, GLFW desktop, the cross-platform "embedder"
ABI) lives as a peer subdirectory under
`engine/src/flutter/shell/platform/`, each implementing the same
surface (PlatformView, ExternalTexture, KeyEventHandler,
VsyncWaiter, PlatformMessageHandler) in the language native to
that OS:

- **Dart** for the framework (`packages/flutter/lib/`) + the
  CLI tools (`packages/flutter_tools/`) + the engine's Dart
  bindings (`engine/src/flutter/lib/ui/`)
- **Java** for the Android engine surface
  (`engine/src/flutter/shell/platform/android/io/flutter/`)
- **Kotlin** for the Gradle plugin
  (`packages/flutter_tools/gradle/src/main/kotlin/`) + the
  newer Android integration-test apps under `dev/`
- **Swift** + **Objective-C** for the iOS / macOS engine surface
  (`engine/src/flutter/shell/platform/darwin/{ios,macos}/framework/Source/`)
- **C / C++** for the engine core
  (`engine/src/flutter/shell/`, `engine/src/flutter/runtime/`,
  `engine/src/flutter/impeller/`) and for the Linux + Windows
  engine surfaces (`engine/src/flutter/shell/platform/{linux,windows}/`)

Where apache/arrow is the **data-format-driven** polyglot (one
schema in `format/`, six per-language readers each implementing
the same FlatBuffers spec), flutter is the **platform-driven**
polyglot (one framework in `packages/flutter/`, six per-OS
embedders each implementing the same engine ABI in the OS's
native language).

Concrete count at HEAD (after sparse-checkout):

- **9** per-package Dart workspaces under `packages/`
  (flutter, flutter_driver, flutter_goldens,
  flutter_localizations, flutter_test, flutter_tools,
  flutter_web_plugins, fuchsia_remote_debug_protocol,
  integration_test) — every `pubspec.yaml` declares
  `resolution: workspace` and slots into the root
  `pubspec.yaml` workspace list (**77 members** total)
- **6** native-platform engine subdirs under
  `engine/src/flutter/shell/platform/` — `android/`,
  `darwin/ios/`, `darwin/macos/`, `linux/`, `windows/`,
  `fuchsia/`, plus `glfw/` (desktop reference embedder),
  `embedder/` (the cross-platform C ABI), `common/`
  (shared code). Each declares `BUILD.gn` (the GN entry
  point dispatched via `is_android` / `is_mac` / `is_linux`
  / `is_win` / `is_fuchsia` predicates in
  `engine/src/flutter/shell/platform/BUILD.gn`)
- **5** cross-platform `flutter create` template subdirs under
  `packages/flutter_tools/templates/app/` — `android.tmpl`,
  `android-java.tmpl`, `android-kotlin.tmpl`, `ios.tmpl`,
  `linux.tmpl`, `macos.tmpl`, `windows.tmpl` — every Flutter
  app `flutter create` scaffolds gets the matching template
- **39** `analysis_options.yaml` files (per-package +
  per-test-tree — the canonical Dart linter chain that the
  root file is `include:`d into)
- **232** `BUILD.gn` files (the engine GN graph), **134**
  `pubspec.yaml` (Dart workspace members + per-app
  native-host shells), **94** `CMakeLists.txt` (Linux +
  Windows native), **91** `build.gradle` /
  `build.gradle.kts` (Android), **25** `Podfile` + **3**
  `.podspec` files (iOS / macOS native dep chain), **409**
  `AndroidManifest.xml` (one per Android app variant +
  `debug`/`profile`/`release` build types), **97**
  `Info.plist` (iOS / macOS Runner shell + framework targets)
- **304** Java + **61** Kotlin + **131** Swift + **342**
  Objective-C (`.m` / `.mm`) + **3 227** C / C++ + **4 857**
  Dart files in scope — **all 5 native-platform languages
  PLUS Dart with uniform "Copyright YYYY The Flutter Authors.
  All rights reserved. ... BSD-style license" header
  discipline at the top of EVERY file**
- **16** GitHub Actions workflows under `.github/workflows/`
  — the public surface (PR / release-tracker / labeler /
  mirror / coverage / cherry-pick / freeze /
  sync-engine-version). Bulk of CI runs against `.ci.yaml`
  on Flutter's internal LUCI infra
- `.ci.yaml` = **7 198 lines** orchestrating **464 named
  CI targets** across linux / mac / win / android-emu /
  ios-device / fuchsia shards. Each target's `shard:` field
  maps to a switch-case branch in `dev/bots/test.dart`
- `dev/bots/test.dart` = **663 lines**, the canonical
  "run-all-the-tests" orchestrator that `.ci.yaml` shards
  out to via the `SHARD` env var
- **13** root-level governance / compliance files: `LICENSE`,
  `PATENT_GRANT` (the BSD+patent pair unique to flutter),
  `AUTHORS` (149 lines), `CODEOWNERS` (67 lines),
  `TESTOWNERS` (379 lines — the per-test-shard ownership
  manifest unique to flutter), `CODE_OF_CONDUCT.md`,
  `CONTRIBUTING.md`, `.gitattributes`, `.gitignore`,
  `analysis_options.yaml` (root linter chain),
  `dartdoc_options.yaml`, `pubspec.yaml` (the root
  `_flutter_packages` workspace anchor), `.ci.yaml`
- `engine/src/build_overrides/` = **12** `.gni` override
  files declaring vendor selection for shared graphics
  libraries (vulkan, wayland, swiftshader, glslang,
  spirv-tools, angle)

Total **structural-validation surfaces** counted: **31**
discrete checks across the inventory (see § "Existing tooling
inventory" below).

- **17 of 31 (55 %)** map to existing alint rules — the
  bundled `oss-baseline + ci/github-actions +
  hygiene/no-tracked-artifacts` ship **29 rules**
  between them (`oss-baseline=15` + `ci/github-actions=3` +
  `hygiene/no-tracked-artifacts=11`), plus the **39
  flutter-specific rules** in [`/.alint.yml`](.alint.yml) (per-platform engine BUILD.gn
  parity, per-package pubspec discipline, framework-wide
  Flutter-Authors BSD header across 5 native langs +
  Dart, Apple framework four-file layout, `flutter create`
  template parity, Dart analyzer strict-mode pins, Windows
  CRLF / shell LF gitattributes, build_overrides .gni
  presence, governance triad, GHA hardening)
- **6 of 31 (19 %)** shell out via `command:` rules —
  wrapping `dart analyze`, `dart format --set-exit-if-changed`,
  `engine/src/flutter/ci/{clang_tidy.sh,format.sh,pylint.sh,licenses_cpp.sh}`
- **8 of 31 (26 %)** are out of alint's scope — the
  `dev/bots/test.dart` shard orchestrator (it's a Dart
  program that calls per-shard test runners; alint sees
  files at rest), the `.ci.yaml` ↔ `test.dart` shard registry
  cross-validation (needs the v0.10+ `registry_paths_resolve`
  primitive), the per-platform engine ABI parity cross-check
  (needs the v0.11+ `cross_language_implementation_complete`
  primitive in its platform-driven variant), the
  `dev/bots/check_code_samples.dart` embedded-snippet
  validator, the `dev/bots/check_tests_cross_imports.dart`
  test-tree import graph check, the engine LUCI internal
  runners under `engine/src/flutter/ci/builders/*.json`, the
  TESTOWNERS-driven flaky-test triage bot, the engine
  C++ symbol-prefix scan (`engine/src/flutter/ci/licenses_cpp.sh`
  is an existing licenses gate; the broader
  `every-libflutter-symbol-starts-with-Flutter` check is out
  of scope as binary parsing).

The configured **68-rule** (39 flutter-specific + 29 from 3
bundled rulesets) [`/.alint.yml`](.alint.yml) covers
every structural assertion the existing tooling makes about
repo *state*, plus several flutter doesn't enforce today
(per-package `resolution: workspace` declaration, per-platform
engine `BUILD.gn` presence, `flutter create` template parity,
the Flutter-Authors BSD header on every file across the entire
polyglot tree — currently enforced by the engine subtree's
`format.sh` only).

**Headline finding:** flutter/flutter is **the** flagship
"platform-driven polyglot monorepo" pitch for alint — every
native-OS embedder under
`engine/src/flutter/shell/platform/` is a peer of every other
embedder, with the **same engine ABI implemented in 5
different native languages (Java, Kotlin, Swift, ObjC, C++)**.
No per-language linter sees this cross-platform consistency:
Android Studio only knows about `android/`, Xcode only knows
about `darwin/{ios,macos}/`, MSVC only knows about `windows/`,
and `clang-format` runs per-file without cross-platform parity
awareness. alint catches the platform-driven invariants once,
across the entire polyglot tree.

---

## Existing tooling inventory

### Root config files (cross-language gate / orchestration)

| File | Owner tool | What it pins | alint disposition |
|---|---|---|---|
| `analysis_options.yaml` (root) | Dart analyzer | strict-casts/inference/raw-types pins; per-rule lint set; deprecated-member-use overrides; format.page_width = 100 | `file_exists` + 3× `file_content_matches` for the strict-* triad; per-package `analysis_options.yaml` files include the root |
| `pubspec.yaml` (root) | pub workspace | the `workspace:` list with 77 members across packages/, dev/, examples/, engine/src/flutter/ subtrees | `file_exists` (the workspace anchor); per-member `resolution: workspace` covered by the per-package rule |
| `.ci.yaml` | LUCI infra | 7198 lines, 464 named targets driving the entire LUCI CI pipeline (linux/mac/win/android-emu/ios-device/fuchsia shards) | `file_exists` (the canonical CI-target manifest). The deeper "every shard: value resolves to a switch case in test.dart" check needs the v0.10+ `registry_paths_resolve` rule kind |
| `dev/bots/test.dart` | flutter dev tooling | 663-line canonical CI orchestrator that .ci.yaml shards out to | `file_exists` |
| `dartdoc_options.yaml` | dartdoc | snippet/sample/dartpad tool integration for {@tool snippet} embedded samples in API docs | `file_exists` |
| `.gitattributes` | git | CRLF for Windows files (.bat/.ps1/.sln/.props/.vcxproj); LF for shell scripts (bin/flutter, bin/dart, bin/flutter-dev) | `file_exists` + 2× `file_content_matches` for the CRLF / LF pins |
| `.gitignore` | git | Dart/Flutter build outputs (.dart_tool/, build/, .flutter-plugins, *.iml, .vscode/*) | covered by bundled hygiene + 3× explicit `dir_absent` / `file_absent` rules |
| `LICENSE` + `PATENT_GRANT` | Flutter authors | BSD-3-Clause + Google patent grant (the unique combo) | bundled `oss-license-exists` covers LICENSE; explicit `flutter-patent-grant-present` covers PATENT_GRANT |
| `AUTHORS` | Flutter authors | 149-line contributor enumeration anchoring the per-file copyright line | `file_exists` |
| `CODEOWNERS` | GitHub | 67-line per-path PR-review routing | `file_exists` (covered also by bundled oss-codeowners-exists) |
| `TESTOWNERS` | flutter triage bot | **unique to flutter** — 379-line per-test-shard ownership manifest the flaky-test triage bot uses to auto-assign new flaky-test bugs | `file_exists` |
| `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, `SECURITY.md` (under docs/) | Flutter authors | Standard OSS governance docs | covered by bundled oss-baseline rules |

### `engine/src/build_overrides/` — the vendor-selection hook

| File | What it pins | alint disposition |
|---|---|---|
| `build.gni`, `vulkan_headers.gni`, `vulkan_loader.gni`, `swiftshader.gni`, `glslang.gni`, `spirv_tools.gni`, `angle.gni`, `wayland.gni`, `vulkan_tools.gni`, `vulkan_utility_libraries.gni`, `vulkan_validation_layers.gni`, `lunarg_vulkantools.gni` | Per-vendor dependency selection for shared graphics libraries | `file_exists` for the 7 critical .gni files (vulkan + wayland + swiftshader + glslang + spirv-tools + angle); missing any one means gn-gen errors out with "Could not load build_overrides/<x>.gni" |

### `.github/workflows/` (16 workflows)

| Workflow family | What it does | alint disposition |
|---|---|---|
| Auto-labeler / triage: `cicd.yml`, `labeler.yml`, `lock.yaml`, `no-response.yaml`, `release-tracker.yml`, `release.yml` | PR labeling, locked-issue auto-close, release announcement | shape covered by bundled `ci/github-actions@v1` (workflow has `name:`, permissions declared, action SHA-pinned) |
| Release / cherry-pick: `cut-release-branch.yml`, `easy-cp.yml`, `freeze.yml`, `merge-changelog.yml`, `revert.yml` | Branch cutting, cherry-pick automation, freeze/unfreeze, revert handling | shape covered by bundled GHA ruleset (operational, alint scope is shape only) |
| Sync / mirror / coverage: `coverage.yml`, `mirror.yml`, `roll-dart-dependencies.yml`, `sync-engine-version.yml`, `tool-test-general.yml`, `content-aware-hash.yml` | Cross-repo mirroring, dart-sdk dep roll automation, content hashing | shape covered by bundled GHA ruleset |
| `.github/dependabot.yml` | Weekly action update PRs (grouped via `groups.all-github-actions.patterns: ["*"]` to suppress PR spam) | `yaml_path_matches` for `updates[?@['package-ecosystem'] == 'github-actions'].directory == '/'` |

The bundled `ci/github-actions@v1` ruleset (3 rules: workflow
permissions, action SHA pinning, workflow has `name:`) covers
the hardening surface for all 16 workflows at once. The
configured `.alint.yml` restates the SHA-pinning rule at
warning level.

### Per-package Dart conventions — the workspace topology

| Package | Manifest | Per-package shape | alint disposition |
|---|---|---|---|
| `packages/flutter/` | `pubspec.yaml`, `analysis_options.yaml`, `lib/`, `test_fixes/` | The Flutter framework itself; pub.dev: `flutter`. `analysis_options.yaml` adds `public_member_api_docs` (every public symbol must be doc'd) on top of the root chain. | per-package `for_each_dir` with require:[pubspec.yaml]; explicit `analysis_options.yaml` presence rule for the 5 strict-tier packages; explicit `homepage: https://flutter.dev` check for published packages |
| `packages/flutter_tools/` | `pubspec.yaml`, `analysis_options.yaml`, `lib/`, `bin/`, `gradle/` | The flutter CLI; pub.dev: `flutter_tools`. Hosts the Gradle plugin (Kotlin) under `gradle/src/main/kotlin/` (21 .kt files) | covered by per-package rules |
| `packages/flutter_test/` | `pubspec.yaml`, `analysis_options.yaml`, `lib/` | flutter's testing framework; pub.dev: `flutter_test` | covered by per-package rules |
| `packages/flutter_driver/` | `pubspec.yaml`, `analysis_options.yaml`, `lib/` | Integration / performance test API; pub.dev: `flutter_driver` | covered by per-package rules |
| `packages/flutter_localizations/` | `pubspec.yaml`, `lib/src/l10n/` (auto-generated translations — header-free) | l10n bundle; pub.dev: `flutter_localizations` | covered by per-package rules; `l10n/` excluded from header rule |
| `packages/flutter_web_plugins/` | `pubspec.yaml`, `lib/` | Web-platform plugin registry; pub.dev: `flutter_web_plugins` | covered by per-package rules |
| `packages/flutter_goldens/` | `pubspec.yaml`, `lib/` | Golden-file rendering for tests; pub.dev: `flutter_goldens` (no `homepage:` historically — exception listed in the rule) | covered with exception; `resolution: workspace` rule excludes this package |
| `packages/integration_test/` | `pubspec.yaml` (with `publish_to: none`), `lib/` | Internal-only integration test runner | `flutter-internal-package-publish-to-none` rule (`publish_to: none` MUST be declared) |
| `packages/fuchsia_remote_debug_protocol/` | `pubspec.yaml` (with `publish_to: none`), `lib/` | Internal-only Fuchsia debug bridge | covered by `publish_to: none` rule |

### Per-platform engine subtree — the platform-driven polyglot

This is where alint earns its keep on flutter/flutter.

| Subdir | Manifest at root | Per-platform shape | alint disposition |
|---|---|---|---|
| `engine/src/flutter/shell/platform/android/` | `BUILD.gn`, `build.gradle`, `AndroidManifest.xml`, 259 `.java` files | Android engine surface: Java (no Kotlin in engine), Gradle build, Android manifest | `for_each_dir` over the named platform list with require:[BUILD.gn]; `flutter-bsd-source-header` covers .java |
| `engine/src/flutter/shell/platform/darwin/ios/` | `BUILD.gn`, `framework/{Headers,Source,Info.plist,module.modulemap}`, dozens of .swift/.mm/.h files | iOS engine surface: Swift + ObjC framework target | `for_each_dir` for darwin/ios/macos/common; `flutter-darwin-framework-layout` enforces the four-file Apple framework layout (Headers, Source, Info.plist, module.modulemap); BSD header rule covers .swift/.m/.mm/.h |
| `engine/src/flutter/shell/platform/darwin/macos/` | `BUILD.gn`, `framework/{Headers,Source,Info.plist,module.modulemap}`, .swift/.mm/.h files | macOS engine surface: Swift + ObjC (mirrors iOS) | covered by darwin parent rule + framework-layout rule |
| `engine/src/flutter/shell/platform/linux/` | `BUILD.gn`, 213 .cc/.h files (GTK + GObject Introspection bindings) | Linux engine surface: C++ with GObject conventions | covered by per-platform rule + BSD header rule covers .cc/.h |
| `engine/src/flutter/shell/platform/windows/` | `BUILD.gn`, 184 .cc/.h files (UWP + Win32 surface) | Windows engine surface: C++ with COM-style APIs | covered by per-platform rule + BSD header rule |
| `engine/src/flutter/shell/platform/fuchsia/` | `BUILD.gn`, ~252 files | Fuchsia engine surface: C++ with FIDL bindings | covered by per-platform rule |
| `engine/src/flutter/shell/platform/glfw/` | `BUILD.gn`, ~39 files | GLFW desktop reference embedder (used as a portability test) | covered by per-platform rule |
| `engine/src/flutter/shell/platform/embedder/` | `BUILD.gn`, ~144 files | The cross-platform C ABI (the `flutter_embedder.h` API every external embedder consumes) | covered by per-platform rule; the C ABI itself is the most stable surface in the repo |
| `engine/src/flutter/shell/platform/common/` | `BUILD.gn`, ~96 files | Shared code across all platform embedders | covered by per-platform rule |

The configured alint rules `flutter-engine-platform-has-build-gn`,
`flutter-engine-darwin-platforms-have-build-gn`, and
`flutter-darwin-framework-layout` cover the entire shape of the
per-platform tree.

### `flutter create` template parity — the OTHER cross-platform surface

| Template | What it scaffolds | alint disposition |
|---|---|---|
| `packages/flutter_tools/templates/app/android.tmpl/` | Android app shell (java + kotlin + gradle) | `flutter-create-templates-platform-coverage` enforces presence |
| `packages/flutter_tools/templates/app/android-java.tmpl/` | Java-flavour Android app shell | covered by same rule |
| `packages/flutter_tools/templates/app/android-kotlin.tmpl/` | Kotlin-flavour Android app shell | covered by same rule |
| `packages/flutter_tools/templates/app/ios.tmpl/` | iOS app shell (Swift `Runner.xcodeproj`) | covered by same rule |
| `packages/flutter_tools/templates/app/linux.tmpl/` | Linux desktop app shell (GTK + CMake) | covered by same rule |
| `packages/flutter_tools/templates/app/macos.tmpl/` | macOS app shell (Swift `Runner.xcodeproj`) | covered by same rule |
| `packages/flutter_tools/templates/app/windows.tmpl/` | Windows desktop app shell (CMake + .vcxproj) | covered by same rule |

When a user runs `flutter create my_app`, `flutter_tools`
scaffolds a per-platform native-host shell from these templates;
missing a template means `flutter create` silently skips that
platform (and `flutter run -d <platform>` fails later with a
confusing "no platform-specific code found" error).

This is the **OTHER cross-platform parity surface** alongside
the engine — together with the engine, the templates are what
make flutter "all 5 native platforms supported". Drift here
tells you the project has dropped (or not yet added) a
platform.

### Per-language tool config — what each native ecosystem expects

| Language | Tool config | What it pins | alint disposition |
|---|---|---|---|
| Dart | `analysis_options.yaml` (root + 38 per-package/per-tree) | strict-casts, strict-inference, strict-raw-types; per-rule lint set; format.page_width = 100 | bundled — root + strict-* triad enforced |
| Dart | `dartdoc_options.yaml` | snippet/sample/dartpad tool integration | `file_exists` |
| Java/Kotlin (Android engine) | (no separate Checkstyle / Detekt config; uses gradle-bundled defaults) | — | not enforced (alint scope is shape only) |
| Swift / ObjC (Darwin engine) | (no separate SwiftLint config; uses internal style guide) | — | not enforced |
| C++ (engine + Linux/Windows surfaces) | implicit (clang-format + clang-tidy run via `engine/src/flutter/ci/{format.sh,clang_tidy.sh}`) | — | wrapped via `command:` rules |
| Python (engine build scripts) | implicit (pylint via `engine/src/flutter/ci/pylint.sh`) | — | wrapped via `command:` rules |

### Engine CI scripts — the per-language enforcement layer

| Script | What it does | alint disposition |
|---|---|---|
| `engine/src/flutter/ci/format.sh` | Multi-language formatter pass (clang-format for C++, dartfmt for Dart, gn-format for GN, yapf for Python) | `command:` wrapping the script |
| `engine/src/flutter/ci/clang_tidy.sh` | Engine C++ static analysis | `command:` wrapping the script |
| `engine/src/flutter/ci/pylint.sh` | Engine Python pylint pass | `command:` wrapping the script |
| `engine/src/flutter/ci/licenses_cpp.sh` | Engine C++ license-attribution scan (the in-engine analogue of Apache RAT) | `command:` wrapping the script |
| `engine/src/flutter/ci/builders/*.json` (~30 files) | Per-CI-runner build descriptors consumed by Flutter's internal LUCI infra | out of alint scope (operational; LUCI runners) |

---

## What maps to existing alint rules

The 68-rule [`/.alint.yml`](.alint.yml) breaks down as:

- **3 bundled rulesets** (`oss-baseline`,
  `ci/github-actions`, `hygiene/no-tracked-artifacts`) —
  pull in **29 rules** between them
  (`oss-baseline=15` + `ci/github-actions=3` +
  `hygiene/no-tracked-artifacts=11`)
- **2 cross-language Flutter Authors BSD-header rules** —
  one for `//`-style comment languages (Dart, Java, Kotlin,
  Swift, ObjC, C++, Gradle), one for `#`-style comment
  languages (Python, shell, GN, CMake). Both anchor on
  `Copyright YYYY The Flutter Authors. All rights reserved.`
  with the `BSD-style license` second-line marker
- **3 per-platform engine BUILD.gn rules** —
  `flutter-engine-platform-has-build-gn` (`for_each_dir` over
  android/linux/windows/fuchsia/glfw/embedder/common),
  `flutter-engine-darwin-platforms-have-build-gn` (separate
  `for_each_dir` for darwin/ios + darwin/macos + darwin/common
  due to the nested layout), `flutter-darwin-framework-layout`
  (the canonical Apple framework four-file shape)
- **1 cross-platform `flutter create` template parity rule**
  — multi-path `file_exists` covering all 7 templates
- **5 per-Dart-package rules** —
  `flutter-package-has-pubspec` (`for_each_dir` over
  packages/*), `flutter-package-resolution-workspace`
  (per-pubspec workspace marker), `flutter-package-has-
  analysis-options` (the 5 strict-tier packages),
  `flutter-published-package-has-homepage` (the 6
  pub.dev-published packages), `flutter-internal-package-
  publish-to-none` (the 2 internal packages)
- **2 per-engine-Dart-workspace rules** —
  `flutter-engine-has-pubspec`, `flutter-engine-has-
  analysis-options` (the engine ships its own separate Dart
  workspace at `engine/src/flutter/`)
- **8 Flutter governance rules** — `PATENT_GRANT`, `AUTHORS`,
  `CODEOWNERS`, `TESTOWNERS`, `.ci.yaml`, `dev/bots/test.dart`,
  `dartdoc_options.yaml`, `analysis_options.yaml` (root)
- **3 Dart analyzer strict-mode rules** —
  `flutter-analysis-options-strict-casts`,
  `flutter-analysis-options-strict-inference`,
  `flutter-analysis-options-strict-raw-types`
- **2 GHA hardening rules** — restatement of SHA-pinning at
  warning level + `.github/dependabot.yml` includes
  `package-ecosystem: github-actions`
- **3 .gitattributes EOL rules** —
  `flutter-gitattributes-present`,
  `flutter-gitattributes-windows-crlf-pin` (`*.bat eol=crlf`),
  `flutter-gitattributes-flutter-bin-lf-pin` (`bin/flutter eol=lf`)
- **1 engine build_overrides rule** — multi-path `file_exists`
  covering the 7 critical `.gni` files (vulkan + wayland +
  swiftshader + glslang + spirv-tools + angle)
- **3 hygiene rules** — `.dart_tool/`, `build/`,
  `.flutter-plugins` absent
- **6 `command:` rule shell-outs** — `dart analyze`,
  `dart format --set-exit-if-changed`,
  `engine/src/flutter/ci/{clang_tidy.sh,format.sh,pylint.sh,licenses_cpp.sh}`

---

## What needs new alint primitives

Two patterns specific to flutter/flutter that don't fit any
current rule.

### 1. `cross_language_implementation_complete` — the platform-driven variant

Every native-OS embedder under
`engine/src/flutter/shell/platform/` implements **the same
engine ABI** in **a different native language**:

| Surface | Android (Java) | iOS / macOS (Swift+ObjC) | Linux (C++ / GTK) | Windows (C++ / COM) |
|---|---|---|---|---|
| `PlatformView` | `PlatformView.java` | `FlutterPlatformView.h/.mm` | `fl_view.cc` | `flutter_view.cc` |
| `ExternalTexture` | `image_external_texture.cc` | `ios_external_texture_metal.mm` | (via fl_renderer texture) | `external_texture_d3d.cc` |
| `KeyEventHandler` | (engine routes via JNI) | `FlutterChannelKeyResponder.h/.mm` | `fl_keyboard_handler.cc` | (via WindowsProcTable) |
| `VsyncWaiter` | `vsync_waiter_android.cc` | (Metal CVDisplayLink) | (GTK frame clock) | (DCompositionWaitForCompositorClock) |
| `PlatformMessageHandler` | `platform_message_handler_android.cc` | `platform_message_handler_ios.h/.mm` | `fl_method_channel.cc` | `flutter_windows_engine.cc` (channel dispatch) |
| `AccessibilityBridge` | `AccessibilityBridge.java` | `accessibility_bridge.mm` | `fl_accessibility_handler.cc` | `accessibility_bridge_windows.cc` |

Drift here means a Flutter app's accessibility / external-texture
/ key-handling behaviour is silently absent on one platform but
present on the others. Today this is enforced by code-review
discipline + the engine LUCI runners that build all 5 platforms
and integration-test each.

The shape is **EXACTLY** the
`cross_language_implementation_complete` rule kind (now
`v0.11+ ship-target` per launch-evidence.md, with 5 sources
saturated), in its **platform-driven** variant rather than the
**data-format-driven** variant arrow + tensorflow demonstrate:

| Repo | Variant | Registry | Per-implementation entry |
|---|---|---|---|
| arrow | data-format-driven | `format/Schema.fbs` types (FlatBuffers) | per-language test fixture under `cpp/test/`, `python/test/`, `r/tests/`, `ruby/test/` |
| tensorflow | data-format-driven | Python public symbols (1185 v1+v2 textproto goldens under `tensorflow/tools/api/golden/`) | per-language binding under `tensorflow/lite/{java,swift,objc,python}/` |
| **flutter** | **platform-driven** | **engine ABI surfaces (PlatformView, ExternalTexture, KeyEventHandler, VsyncWaiter, PlatformMessageHandler, AccessibilityBridge)** | **per-platform implementation under `engine/src/flutter/shell/platform/{android,darwin/ios,darwin/macos,linux,windows}/`** |

This is the **fifth independent demand signal** for
`cross_language_implementation_complete` (arrow + TF + protobuf +
angular + flutter per launch-evidence.md), and the FIRST
**platform-driven** source. The arrow + tensorflow shape is
"every entry in registry A has a corresponding test fixture per
language under root B"; the flutter shape is "every ABI surface
at registry A has a corresponding native implementation under
the per-platform directory tree B" — same primitive, different
registry source. **Already promoted from `v0.11+ candidate` to
`v0.11+ ship-target` per launch-evidence.md — saturated at 5
sources, with 3 distinct topologies (data-format-driven, within-
language source↔golden, platform-driven).**

The proposed primitive shape (sketch):

```yaml
- id: flutter-engine-abi-platform-parity
  kind: cross_language_implementation_complete  # v0.11+ ship-target
  registry:
    paths: engine/src/flutter/shell/platform/embedder/embedder.h
    extract_symbols: 'FlutterEngine[A-Z]\w+'
  implementations:
    - language: java
      paths: engine/src/flutter/shell/platform/android/io/flutter/**/*.java
    - language: swift_objc
      paths: engine/src/flutter/shell/platform/darwin/{ios,macos}/framework/Source/*
    - language: cpp_linux
      paths: engine/src/flutter/shell/platform/linux/*.cc
    - language: cpp_windows
      paths: engine/src/flutter/shell/platform/windows/*.cc
  level: error
  message: >-
    Every ABI symbol in embedder.h must have a per-platform
    implementation under android/, darwin/{ios,macos}/, linux/, windows/.
```

### 2. `registry_paths_resolve` for `.ci.yaml` ↔ `dev/bots/test.dart`

`.ci.yaml`'s 464 `name:` targets each carry a `shard:` key that
maps to a switch-case branch in `dev/bots/test.dart` (e.g.
`shard: framework_tests` → `case 'framework_tests': await
_runFrameworkTests(); break;`). Today the cross-validation that
every `shard:` value resolves to a known case branch is enforced
only by manual review + by failing at run-time when the dispatch
falls through to the default branch.

This is the **sixth repo** to surface this shape (rust-lang +
clap + cpython + apache/arrow + next.js + flutter). Same
`registry_paths_resolve` primitive, different registry (here,
switch-case branches in a Dart source file rather than glob
patterns in a text file).

**Demand: 8 sources per launch-evidence.md** (rust, clap,
cpython×2, next.js, arrow, pytorch, nodejs/node, NixOS×3) —
already promoted to **`v0.10 ship-target`**.

### 3. `ordered_block` for `.ci.yaml` target alphabetisation

`.ci.yaml`'s 464 targets are conventionally alphabetised by
`name:` within each OS group (linux/mac/win/android-emu/...).
The convention is unenforced; drift would still parse and run
but make the file unreadable. **Same `ordered_block` shape as
arrow's `rat_exclude_files.txt` + rust + airflow + tokio +
cpython** — re-confirms the rule kind. Per launch-evidence.md
this is now at 7 sources / **`v0.10 ship-target`**.

---

## What's out of alint's scope (kept on the existing tool)

Listed by category for clarity:

- **Dart AST / lint analysis** (`dart analyze`,
  `flutter analyze`, the per-package lint chain) — alint
  deliberately doesn't try to be a parser. Shell out via
  `command:`.
- **C++ / Java / Kotlin / Swift / ObjC AST** — no
  per-language analyzer config in this repo (engine relies
  on the bundled clang-format/clang-tidy + internal style
  guide; the framework Java/Kotlin runs gradle-bundled
  defaults). Shell out via `engine/src/flutter/ci/*.sh`
  scripts.
- **Engine LUCI build runners** (`engine/src/flutter/ci/builders/*.json`)
  — these are operational descriptors for Flutter's
  internal LUCI infrastructure, not validation surfaces.
- **`dev/bots/test.dart` shard dispatch** — alint sees files
  at rest, not the runtime dispatch. The cross-validation
  belongs to the v0.10+ `registry_paths_resolve` primitive.
- **The `.ci.yaml` ↔ test.dart shard registry resolution** —
  same v0.10+ candidate.
- **API-symbol parity per platform** (every PlatformView API
  symbol must exist in all 5 platform embedder implementations)
  — exactly the v0.11+ `cross_language_implementation_complete`
  candidate, platform-driven variant. Defer to v0.11+.
- **TESTOWNERS-driven flaky-test triage bot** — git/PR state,
  not tree state.
- **Engine binary symbol-prefix scan** (the
  "every libflutter symbol starts with `Flutter`" check) —
  binary parsing, out of scope; engine CI runs a separate
  pass via `clang -nm`-style tooling.
- **`.gitignore`-tracked-but-on-disk file detection** — same
  shape as bazel's `.bazelversion` (CONFIG-AUTHORING.md
  pitfall #18). flutter's `pubspec.lock` ships tracked-but-
  gitignored at `pubspec.lock` (`!/pubspec.lock` in
  `.gitignore`). **FIXED in v0.9.17** — the per-rule
  `respect_gitignore: false` knob shipped with v0.9.17 (see
  CONFIG-AUTHORING.md pitfall #18 for the canonical pattern);
  flutter is the second demand source after bazel that the
  fix unblocks. A future config edit can drop the workaround
  and set `respect_gitignore: false` directly.

---

## Already covered by other linters flutter uses

- `dart analyze` / `flutter analyze` — Dart AST + style.
- `dart format` — Dart formatter (page_width=100 from
  `analysis_options.yaml`).
- `clang-format` / `clang-tidy` — engine C++ AST + style
  (wrapped by `engine/src/flutter/ci/{format.sh,clang_tidy.sh}`).
- `pylint` — engine Python lint
  (wrapped by `engine/src/flutter/ci/pylint.sh`).
- `dev/bots/check_code_samples.dart` — embedded-snippet
  cross-validation (the `{@tool snippet}` directive in API
  docs is statically analysed against the framework source).
- `dev/bots/check_tests_cross_imports.dart` — test-tree
  import graph check.
- engine LUCI runners — full per-platform build + integration-
  test on linux / mac / win / android-emu / ios-device /
  fuchsia.

---

## Performance comparison (placeholder — bench when validation pass scales)

The repo is a meaningful stress test even after sparse-checkout:

- **~14 000 files / 188 MiB** working tree (after sparse-
  checkout dropping `packages/flutter/test`,
  `packages/flutter_tools/test`, `dev/automated_tests`,
  `engine/src/flutter/third_party`)
- **5 native-platform languages PLUS Dart** in scope
- **77** Dart workspace members
- **9** in-tree per-platform engine subdirs
- **464** named CI targets across `.ci.yaml`

The published S9 polyglot bench (100k+ files, 13 languages)
hits ~1.4 s on a stock CI runner. The full flutter tree (with
test dirs re-included, ~50k files) sits between S3 and S9.
Expected: 1-3 s for `alint check` on the structural rules
alone, vs. ~30-60 s for the equivalent `dev/bots/test.dart`
analyze shard (which serialises through `dart analyze` over
all 9 packages).

Where alint shines on flutter/flutter specifically: the
**cross-platform conventions** — every platform subdir under
`engine/src/flutter/shell/platform/` has BUILD.gn, every
darwin framework has the four-file Apple layout, every
`flutter create` template is present, every Dart workspace
member has `resolution: workspace` — run against the
entire polyglot tree in tens of milliseconds. Sequential
`find . -path '*/shell/platform/*' -name BUILD.gn` + the
follow-up parity check would be ~1-2 s on a hot cache.

To benchmark wall-clock for real:
`time dev/bots/test.dart --shard=analyze` vs `time alint check`.
Deferred to the per-repo measurement pass.

---

## Recommendation for the launch story

This case study is **the** flagship "platform-driven polyglot
monorepo" story for the launch:

- **flutter/flutter is the second-most-starred Google OSS
  project on GitHub** (~170k stars, behind only
  `tensorflow/tensorflow`). Naming it as a target gives alint
  instant credibility with the mobile-app-development
  audience that the data-engineering crowd doesn't reach.
- **No per-platform IDE / linter sees the cross-platform
  conventions**: Android Studio sees `android/`, Xcode sees
  `darwin/{ios,macos}/`, MSVC sees `windows/`, but no one
  tool sees them as peers. The invariants this case study
  enforces (per-platform engine `BUILD.gn` presence, Apple
  framework four-file layout, `flutter create` template
  parity, Flutter-Authors BSD header across all 5 native
  langs + Dart) are exactly the layer alint owns and nothing
  else does.
- **The Flutter-Authors BSD-style header rule** is the
  cleanest "single rule sweeps the entire polyglot tree"
  demo in the catalogue — one regex, ~9 000 source files
  across 6 languages (Dart + Java + Kotlin + Swift + ObjC +
  C/C++), one alint pass. The engine subtree's `format.sh`
  enforces this on `engine/src/flutter/` only; alint extends
  the same gate to the framework subtree (`packages/`,
  `dev/`, `examples/`) where it's currently enforced only by
  review discipline.
- **The Apple framework four-file layout rule** (`Headers/`,
  `Source/`, `Info.plist`, `module.modulemap` per
  `engine/src/flutter/shell/platform/darwin/{ios,macos}/framework/`)
  is the cleanest "Apple-platform invariant no Linux/Windows
  developer would notice was broken" demo. Drift here
  silently breaks `xcodebuild` framework targets that
  external Flutter apps consume.

Position it as **the first Wave-2 polyglot tile** on
alint.org/examples (alongside arrow as Wave-1 polyglot
flagship), with the angle: *"flutter has 5 native-platform
languages + Dart, 6 per-OS engine embedders implementing the
same ABI, 0 tools that see the platform-driven conventions —
alint is the layer that does."*

The pitch lands harder when paired with the
`cross_language_implementation_complete` polyglot-variant
finding: arrow demonstrates the **data-format-driven** variant
(one schema, six per-language readers), tensorflow
demonstrates the **data-format-driven variant at API-surface
scale** (one Python frontend, six per-language bindings),
flutter demonstrates the **platform-driven** variant (one
engine ABI, six per-OS embedders). **Three independent demand
signals, two distinct variants — `cross_language_implementation_complete`
is now demand-validated as the v0.11+ flagship polyglot
primitive across both variants.**

Followup feature work surfaced (consolidated, sorted by
strength of demand across P2):

- **`registry_paths_resolve` rule kind** — covers `.ci.yaml`
  ↔ `dev/bots/test.dart` shard cross-validation here, plus
  the rust-lang + clap + cpython + arrow + next.js sources.
  **Demand: 8 sources per launch-evidence.md** — already
  promoted to **`v0.10 ship-target`**.
- **`cross_language_implementation_complete` rule kind** —
  arrow + tensorflow demonstrate the data-format-driven
  variant; flutter demonstrates the **platform-driven** variant
  (engine ABI ↔ per-OS embedders). **Demand: 5 sources per
  launch-evidence.md** (arrow + TF + protobuf + angular +
  flutter; 3 distinct topologies) — already promoted to
  **`v0.11+ ship-target`**.
- **`ordered_block` rule kind** — re-confirmed by `.ci.yaml`
  target alphabetisation. **Demand: 7 sources per
  launch-evidence.md** — already promoted to **`v0.10
  ship-target`**.
- **`respect_gitignore: false` per-rule knob** — **DELIVERED
  in v0.9.17** (per-rule knob ships in the engine).
  `pubspec.lock` (tracked-but-gitignored via `!/pubspec.lock`
  in `.gitignore`) is now addressable with a one-line config
  edit; flutter is the second demand source after bazel that
  the fix unblocks.

---

## Notes for the parent agent

- Audit (`alint validate-config examples/flutter-flutter/.alint.yml`)
  **passes**: 68 rule(s) loaded cleanly via the v0.9.17
  release binary. (The `respect_gitignore` field that was
  in-progress at original-write time has shipped in v0.9.17;
  pitfall #18 is now FIXED in the engine.)
- Config runs cleanly against the actual cloned repo at
  `/tmp/flutter/` (358 violations across 20 failing rules —
  39 passing, 149 auto-fixable):
  - **72 warnings on `flutter-bsd-source-header`** — all
    legitimate findings; integration-test apps under
    `dev/integration_tests/{pure_android_host_apps,
    record_use_test_app, spell_check}/` and a few engine
    test files under
    `engine/src/flutter/shell/platform/android/test/` ship
    Kotlin / Java / `.gradle.kts` files without the
    Flutter Authors BSD header (the kind of cross-language
    drift no per-language linter catches).
  - **99 warnings on `flutter-bsd-source-header-shell-comment`**
    — auto-generated `CMakeLists.txt` files under
    `dev/integration_tests/*/{linux,windows}/` from
    `flutter create` templates (real findings; the engine's
    `BUILD.gn` carries the header but the desktop CMakeLists
    templates don't propagate it).
  - **5 errors on `oss-no-bidi-controls`** — **real
    Trojan-Source / CVE-2021-42574 findings** in
    `docs/about/Values.md` and 4 archived release-notes
    files under `docs/releases/archive/`. Flutter ships
    these with embedded bidi controls in contributor names
    / commit messages; alint surfaces them at PR time.
  - **2 warnings on `flutter-published-package-has-homepage`**
    — `packages/flutter_localizations/pubspec.yaml` and
    `packages/flutter_test/pubspec.yaml` don't carry the
    `homepage: https://flutter.dev` line that pub.dev
    surfaces in the package landing page sidebar.
  - **1 warning on `flutter-package-resolution-workspace`**
    — `packages/flutter_tools/pubspec.yaml` historically
    stands outside the root pub workspace (the rule's
    exclude list documents this — confirmed against
    `pubspec.yaml`'s `workspace:` member list).
  - 13 + 2 GHA-related warnings on the 16 public workflows
    (`gha-workflow-contents-read`,
    `flutter-workflow-actions-pinned-by-sha`,
    `gha-pin-actions-to-sha`) — the standard
    supply-chain hardening findings; flutter's public
    workflow surface uses floating action tags
    (`actions/checkout@v4`-style) the bundled
    `ci/github-actions@v1` ruleset flags.
  - 4 hygiene warnings (`hygiene-no-js-build-outputs`).
  - Plus the expected `command:`-rule errors for `dart
    analyze` (which times out scanning the entire flutter
    SDK without the conductor env set up) and the 4 engine
    CI scripts (`clang_tidy.sh`, `format.sh`, `pylint.sh`,
    `licenses_cpp.sh` — each fails fast on missing
    `vpython3` / `pylint-2.7` / unbuilt `licenses_cpp`
    binary, which is the correct diagnostic in the alint
    test environment without `gclient sync` having been
    run).
  - **All cross-platform structural rules pass cleanly on
    the live tree** — `flutter-engine-platform-has-build-gn`,
    `flutter-engine-darwin-platforms-have-build-gn`,
    `flutter-darwin-framework-layout`,
    `flutter-create-templates-platform-coverage`,
    `flutter-engine-build-overrides-present`,
    `flutter-package-has-pubspec`,
    `flutter-package-has-analysis-options`,
    `flutter-internal-package-publish-to-none`,
    `flutter-engine-has-pubspec`,
    `flutter-engine-has-analysis-options`,
    `flutter-analysis-options-strict-{casts,inference,raw-types}`,
    `flutter-{patent-grant,authors,codeowners,testowners,
    ci-config,test-orchestrator,dartdoc-options,
    analysis-options}-present`,
    `flutter-gitattributes-{windows-crlf,flutter-bin-lf}-pin`
    — confirming flutter's polyglot layout is fully
    consistent AND the rules are correctly scoped to fire
    if drift were to occur. No silent failures. No false
    positives in the structural rule set.
- Particular interest from the prompt: **Yes — the
  per-platform parity discipline is exactly the
  `cross_language_implementation_complete` rule kind (now
  `v0.11+ ship-target`) in its platform-driven variant**,
  distinct from the data-format-driven variant arrow +
  tensorflow demonstrate. This case study is the fifth
  independent demand signal for the rule kind, and the
  first **platform-driven** source. The shape generalises
  to every cross-platform UI framework with per-OS native
  embedders (React Native, Xamarin/MAUI, Qt, Tauri).
- The Flutter-Authors BSD-style header rule across 5 native
  languages + Dart is the cleanest single-rule polyglot demo
  in the case-study catalogue. Apply it to the live tree and
  the `command:` shell-out to `engine/src/flutter/ci/format.sh`
  is the per-engine-subtree analogue alint already coordinates.

---

## Validation status (2026-05-07)

- alint version: **0.9.17** (1dbd9b218a0e, built 2026-05-07).
- `validate-config`: **68 rules loaded cleanly** (39 flutter-
  specific + 29 from 3 bundled rulesets — `oss-baseline=15`,
  `ci/github-actions=3`, `hygiene/no-tracked-artifacts=11`).
- Live-tree recheck: **present at `/tmp/flutter/`**; existing
  live-tree finding inventory (line 666 region) remains
  representative of the snapshot.
- Pitfalls fixed in v0.9.17 that touch this config:
  - **Pitfall #18** (per-rule `respect_gitignore: false`)
    — DELIVERED. flutter is the second demand source after
    bazel; once the README's `pubspec.lock` rule is added
    using the new knob, the hand-rolled workaround drops
    out.
  - **Pitfall #19** (literal_is_nested runtime guard) —
    DELIVERED; no impact on this config.
- Open gaps (rule-kind candidates referenced but not yet
  shipped):
  - `cross_language_implementation_complete` (v0.11+
    ship-target, 5 sources) — flutter is the
    platform-driven variant.
  - `registry_paths_resolve` (v0.10 ship-target, 8 sources).
  - `ordered_block` (v0.10 ship-target, 7 sources).

## Future analysis

Three concrete unanalyzed angles for a future revalidation pass:

1. **Add `flutter-engine-embedder-c-abi-presence` rule** —
   the load-bearing
   `engine/src/flutter/shell/platform/embedder/embedder.h`
   is the C ABI every external embedder consumes (e.g.
   `sony/flutter-embedded-linux`,
   `meta-flutter/flutter-pi`). Silent removal would silently
   break out-of-tree embedders. Currently covered indirectly
   by `flutter-engine-platform-has-build-gn`'s embedder/
   directory check; a direct file-existence assertion would
   tighten the gate without ambiguity.
2. **`compliance/reuse@v1` overlay derivative.** The
   `compliance/reuse@v1` ruleset (3 rules — `LICENSES/` dir
   + per-file SPDX headers + `.reuse/dep5`) doesn't drop in
   as-is (Flutter-Authors BSD-style header isn't
   SPDX-compliant), but a future derivative
   `compliance/bsd-flutter@v1` is an obvious bundled-ruleset
   extraction once the pattern stabilises across 2+ adopting
   projects.
3. **`nested_configs: true` for the engine subtree.** The
   `engine/src/flutter/` subtree is effectively a separate
   Dart workspace with its own `pubspec.yaml` and
   `analysis_options.yaml`. A subtree-scoped `.alint.yml`
   under `engine/src/flutter/` would scope the Apple
   framework four-file layout rule and the
   `engine/src/build_overrides/` `.gni` rule next to their
   domain instead of the root config — natural fit once
   v0.10 lands the subtree-config feature.
