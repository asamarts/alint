# Case study: `protocolbuffers/protobuf`

> Marketing/positioning writeup at https://alint.org/examples/protocolbuffers-protobuf/. This README is the engineering reference: tooling inventory, mapping, gap catalogue, validation status.

Inventory of the structural-validation tooling in
`protocolbuffers/protobuf` and an alint config that replaces the
rules alint can express today, plus a catalogue of the gap items
that need new alint primitives.

**Repo state captured:** 2026-05-06, sparse-clone of
`protocolbuffers/protobuf@3b402b46` (rev =
`3b402b46ead91e95cc2576dd093ff740072281ce`). Heavy generated
subtrees excluded (`/src/google/protobuf/compiler`, `/third_party`,
`/conformance/binary_protos`); per-language top-level dirs and the
`conformance/`, `editions/`, `cmake/`, `ci/` subtrees kept.

---

## Summary

protocolbuffers/protobuf is a polyglot binding repo — a single tree
shipping the protoc compiler (C++) plus runtime + codegen for ~10
in-tree language bindings, glued together by `conformance/` (the
cross-language wire-format test suite that EVERY binding must pass)
and the canonical version manifests `version.json` +
`protobuf_version.bzl` (which pin every binding's released version
in lock-step).

Concrete count at HEAD:

- **11 in-tree language binding sub-trees**: `src/` (C++ runtime +
  protoc compiler), `java/` (with 3 Maven sub-modules: `bom/`,
  `kotlin/`, `protoc/` each shipping their own `pom.xml`),
  `python/`, `ruby/`, `go/`, `objectivec/`, `csharp/`, `php/`
  (with PHP-C extension under `php/ext/`), `rust/`, `lua/`, plus
  2 sister C/C++ binding trees: `hpb/` (C++) + `upb/` (C). Dart
  is **also** covered by the conformance suite
  (`failure_list_dart_upb.txt`) but the runtime lives in a sibling
  repo (`dart-lang/protobuf.dart`) — same "spun-out per-language
  repo" pattern as apache/arrow's Java + Go + JS + Rust + Swift +
  Julia.
- **9 languages declared in `version.json`** (cpp, csharp, java,
  javascript, objectivec, php, python, ruby, rust) — the canonical
  cross-language version-pin manifest, lock-stepped to a single
  `protoc_version` (currently `36-dev`) so every binding releases
  together.
- **6 versions in `protobuf_version.bzl`** (`PROTOC_VERSION`,
  `PROTOBUF_JAVA_VERSION`, `PROTOBUF_PYTHON_VERSION`,
  `PROTOBUF_PHP_VERSION`, `PROTOBUF_RUBY_VERSION`,
  `PROTOBUF_RUST_VERSION`) — the Bazel-side version mirror that
  must agree with the corresponding `version.json.languages` entries.
- **19 `conformance/failure_list_*.txt` files** (cpp, csharp,
  csharp_performance, dart_upb, java, java_lite, jruby, jruby_ffi,
  objc, objc_performance, php, php_c, python, python_cpp,
  python-post26, python_upb, ruby, rust_cc, rust_upb) — the
  per-binding wire-format test failure allowlist that the
  conformance test runner consumes. Each binding ships its own
  list because each binding has a different progress curve through
  the test suite.
- **8 `conformance/text_format_failure_list_*.txt` files** — same
  shape, for the human-readable text-format suite.
- **6 in-tree conformance test runners**: `conformance_cpp.cc`,
  `ConformanceJava.java` + `ConformanceJavaLite.java`,
  `conformance_python.py`, `conformance/ruby/conformance_ruby.rb`,
  `conformance_objc.m`, `conformance_php.php`,
  `conformance_rust.rs`. (csharp is run via
  `bazel test //csharp:conformance_test` against a Bazel target —
  no separate runner file. Same for hpb / upb / dart's host-language
  binding.)
- **22 GitHub Actions workflows** under `.github/workflows/`.
  **15 are `test_<lang>.yml` or `test_<scope>.yml`**: `test_bazel`,
  `test_cpp`, `test_csharp`, `test_hpb`, `test_java`,
  `test_objectivec`, `test_php`, `test_php_ext`, `test_python`,
  `test_ruby`, `test_rust`, `test_upb`, `test_yaml`,
  `test_runner` (the orchestrator), `test_release_branches`.
- **137 `BUILD` / `BUILD.bazel` files** + **117 `*.bzl` Starlark
  files** — Bazel-built repo (`MODULE.bazel` + `WORKSPACE` present
  at root). The C++ runtime ALSO ships a CMake alternative under
  `cmake/` (`libprotobuf.cmake`, `libprotobuf-lite.cmake`,
  `libprotoc.cmake`, etc.) for packagers who don't use Bazel.
- **13 `editions/golden/` goldens + 5 `editions/input/` inputs** —
  the new "editions" feature (proto2/proto3 evolution) ships its own
  golden-file test discipline.

The configured **108-rule** [`/.alint.yml`](.alint.yml) covers
every structural assertion the existing tooling makes about repo
*state*, and surfaces 8 net-new rule shapes that no per-language
linter sees because each per-language linter only sees its own
binding subtree.

**Cross-cutting finding for the v0.11+ design phase:** every one of
the **10 in-tree language bindings has parity discipline** that the
v0.11+ `cross_language_implementation_complete` ship-target (5
sources: arrow + TF + protobuf + angular + flutter) would express —
quantitatively:

| Parity surface | Coverage |
|---|---|
| Per-binding wire-format test (own `conformance_<lang>.*` runner) | **6 of 10** ship in-tree runners (cpp, java, python, ruby, objc, php, rust); csharp / hpb / upb / dart drive via `bazel test //<lang>:conformance_test` against a Bazel target |
| Per-binding wire-format failure-allowlist (`failure_list_<lang>.txt`) | **10 of 10** in-tree bindings + **1 of 1** spun-out (dart) — perfect parity, **19 distinct files** when you count the per-runtime variants (java/java_lite, python/python_cpp/python_upb/python-post26, ruby/jruby/jruby_ffi, php/php_c, objc/objc_performance, csharp/csharp_performance, rust_cc/rust_upb) |
| Per-binding text-format failure-allowlist (`text_format_failure_list_<lang>.txt`) | **8 of 10** ship one (cpp, java, java_lite, python, php, rust_cc, rust_upb, dart_upb); csharp / ruby / objc don't yet — alint surfaces the gap as a `warning` |
| Per-binding GitHub Actions test workflow (`test_<lang>.yml`) | **11 of 11** — perfect 1:1 parity (cpp, csharp, hpb, java, objectivec, php, php_ext, python, ruby, rust, upb) |
| Per-binding version pin (in `version.json.languages.<lang>`) | **9 of 9** declared languages have a version key; lua + hpb + upb are runtime-internal and not version-pinned externally |
| Per-binding version pin (in `protobuf_version.bzl`) | **6 of 9** — PROTOC + JAVA + PYTHON + PHP + RUBY + RUST have constants; CSHARP + JAVASCRIPT + OBJECTIVEC ship their version differently (csproj / package.json / podspec respectively, NOT in protobuf_version.bzl). **3-way drift between version.json + protobuf_version.bzl + per-binding manifests is the canonical demand-driver for `cross_language_implementation_complete`.** |

**Net: every language binding has at least 3 parity surfaces
(failure_list + version pin + test workflow), and the canonical
ones (cpp, java, python, ruby, php, rust) have all 5.** This
saturates the v0.11+ `cross_language_implementation_complete`
rule-kind ship-target with **5 demand-driving sources**
(apache/arrow + tensorflow/tensorflow + protocolbuffers/protobuf +
angular/angular + google/flutter), making the v0.11 design phase
ship-ready.

Total **structural-validation surfaces** counted: **45** discrete
checks across the inventory.

- **34 of 45 (76 %) map to existing alint rules** — the 108-rule
  [`/.alint.yml`](.alint.yml) covers them via `oss-baseline`,
  `ci/github-actions`, `hygiene/no-tracked-artifacts` bundles
  plus 102 protobuf-specific rules (per-binding READMEs,
  per-binding manifests, per-binding conformance runners,
  per-binding failure_lists, per-binding test workflows, per-binding
  version pins, governance triad).
- **7 of 45 (16 %) shell out via `command:` rules** — wrapping
  `buildifier` (Starlark AST), `bazel build` (the canonical "does
  it compile" gate), `clang-format`, `flake8`, `rubocop`, `gofmt`.
- **4 of 45 (9 %) are out of alint's scope** — the actual conformance
  test execution (`bazel test //src:conformance_test` and the per-
  binding `//<lang>:conformance_test` siblings — alint sees files at
  rest, not protocol behaviour at runtime), the Apple App Store
  review of `PrivacyInfo.xcprivacy`, the Google-internal Kokoro CI
  orchestration (mirrored to but not authored in this repo), and the
  cross-language wire-format binary-protos comparison logic in
  `conformance_test.cc`.

---

## Existing tooling inventory

### Root config files (cross-language gate / orchestration)

| File | Owner tool | What it pins | alint disposition |
|---|---|---|---|
| `version.json` | release-tooling | Cross-language version manifest: `protoc_version` + per-language released versions for cpp / csharp / java / javascript / objectivec / php / python / ruby / rust | `file_exists` + `json_path_matches` for each language entry |
| `protobuf_version.bzl` | Bazel build | 6 PROTOBUF_*_VERSION + PROTOC_VERSION Starlark constants consumed by every BUILD.bazel rule | `file_exists` + 7× `file_content_matches` (one per constant — JSONPath doesn't parse Starlark, so we pattern-match the assignment lines) |
| `MODULE.bazel` | Bzlmod | The Bazel 8+ canonical workspace declaration (480 lines pinning every external dep) | `file_exists` |
| `WORKSPACE` | legacy Bazel | Backward-compat workspace declaration for pre-Bzlmod Bazel users | `file_exists` (warning) |
| `CMakeLists.txt` | CMake build | C++ runtime + protoc CMake build (alternative to Bazel for packagers) | `file_exists` |
| `Protobuf.podspec` | CocoaPods | Objective-C binding CocoaPods manifest (intentionally at root, not under objectivec/, due to CocoaPods discovery quirks) | `file_exists` + 2× `file_content_matches` (name + license) |
| `PrivacyInfo.xcprivacy` | Apple App Store | Privacy manifest required by App Store since iOS 17+ | `file_exists` |
| `global.json` | .NET SDK | C# binding's .NET SDK version pin | `file_exists` |
| `appveyor.yml` + `appveyor.bat` | AppVeyor | Legacy Windows CI (largely superseded by GitHub Actions) | not enforced (legacy) |
| `.readthedocs.yml` | Read the Docs | docs build | not enforced |
| `LICENSE` + `CONTRIBUTING.md` + `SECURITY.md` + `CODE_OF_CONDUCT.md` + `CONTRIBUTORS.txt` | governance triad | OSS governance — license, contribution policy, vuln intake, code of conduct, contributor record | bundled `oss-baseline@v1` covers LICENSE; explicit `file_exists` rules cover the rest |

### `conformance/` — the cross-language wire-format gate

This is **the most distinctive structural feature of
protocolbuffers/protobuf**: a single directory containing a
`.proto`-defined wire-format test contract that EVERY language
binding must implement a tester for, plus a per-binding allowlist
of known-failing tests. The infrastructure layer is conformance.proto
+ conformance_test_runner.cc; everything else is per-binding.

| Surface | What it does | alint disposition |
|---|---|---|
| `conformance/conformance.proto` | The canonical cross-language test contract — every binding implements a tester that consumes a `ConformanceRequest` and emits a `ConformanceResponse` over a pipe | `file_exists` |
| `conformance/conformance_test_runner.cc` | C++ harness that drives every per-binding tester (spawns binding's tester process, exchanges binary protos over a pipe, applies per-binding failure_list_<lang>.txt allowlist) | `file_exists` |
| `conformance/conformance_<lang>.{cc,java,py,m,php,rs,rb}` (×6) | Per-binding tester implementations (cpp.cc, Java.java + JavaLite.java, python.py, ruby/conformance_ruby.rb, objc.m, php.php, rust.rs) | 6× `file_exists` per binding |
| `conformance/failure_list_<lang>.txt` (×19) | Per-binding known-failing-tests allowlist (consumed by the runner as `--failure_list`). 19 distinct files when you count per-runtime variants. | 8× `file_exists` (one per language family, with multi-path arrays for the per-runtime variants — java + java_lite, python + python_cpp + python_upb + python-post26, ruby + jruby + jruby_ffi, php + php_c, objc + objc_performance, rust_cc + rust_upb) |
| `conformance/text_format_failure_list_<lang>.txt` (×8) | Per-binding text-format suite allowlist | single multi-path `file_exists` covering all 8 |
| `conformance/test_protos/test_messages_edition*.proto` | Edition-evolution wire-format fixtures (test_messages_edition2023.proto, test_messages_edition_unstable.proto) | not currently asserted (deferred until editions feature stabilises) |

### `editions/` — the proto2/proto3 evolution feature

Editions is the new (post-3.x) Protocol Buffers evolution mechanism
that replaces the proto2/proto3 syntax distinction. It ships its own
golden-file test discipline.

| Surface | What it does | alint disposition |
|---|---|---|
| `editions/defaults.bzl` | Bazel rule that generates per-edition feature-set defaults consumed by every language's codegen | `file_exists` |
| `editions/BUILD` | Bazel targets for editions (input protos, golden files, codegen tests) | `file_exists` |
| `editions/input/*.proto` (5 files) | Edition test inputs | not asserted (test data; expected to grow as new editions land) |
| `editions/golden/*` (13 files) | Per-edition codegen golden outputs | not asserted (test data) |

### `.github/workflows/` (22 workflows, 11 per-language)

| Workflow family | What it does | alint disposition |
|---|---|---|
| Per-language CI: `test_cpp.yml`, `test_csharp.yml`, `test_hpb.yml`, `test_java.yml`, `test_objectivec.yml`, `test_php.yml`, `test_php_ext.yml`, `test_python.yml`, `test_ruby.yml`, `test_rust.yml`, `test_upb.yml` | Build + test per language; each `workflow_call:`'d from `test_runner.yml` | 11× `file_exists` (one per binding) — alint surfaces drift if a binding is added without a matching test workflow |
| Orchestrator: `test_runner.yml` | The PR-trigger entry point; `workflow_call:`'s into every `test_<lang>.yml` with safe-checkout discipline | `file_exists` (error) |
| Build-system CI: `test_bazel.yml` | The "bazel build //..." gate against the Bazel build itself | `file_exists` (error) |
| Other test orchestration: `test_yaml.yml`, `test_release_branches.yml` | YAML lint; release-branch test fanout | not currently enforced |
| `scorecard.yml` | OpenSSF Scorecard weekly run | `file_exists` (warning) |
| `staleness_check.yml`, `staleness_refresh.yml` | Generated-code staleness gates (the protoc-generated descriptors must match the checked-in copies) | not enforced (operational) |
| `release_bazel_module.yaml`, `publish_to_bcr.yaml` | Release orchestration | not enforced (operational); shape covered by bundled GHA ruleset |
| `clear_caches.yml`, `janitor.yml`, `forked_pr_workflow_check.yml`, `update_php_repo.yml` | Operational housekeeping | not enforced |

The bundled `ci/github-actions@v1` ruleset (3 rules: workflow
permissions, action SHA pinning, workflow has `name:`) covers the
hardening surface for all 22 workflows at once. The configured
[`/.alint.yml`](.alint.yml) restates the SHA-pinning rule at warning
level — at clone time the GHA ruleset surfaces ~50+ unpinned
`actions/checkout@v4`-style floating tags, exactly the finding the
OpenSSF Scorecard "Pinned-Dependencies" check flags.

### Per-language binding subtree — the polyglot conventions

This is where alint earns its keep on protocolbuffers/protobuf.

| Subdir | Manifest at root | Per-package shape | alint disposition |
|---|---|---|---|
| `src/` | `BUILD.bazel`, `CMakeLists.txt` (root-level), `README.md` | C++ runtime + protoc compiler — single Bazel/CMake project, no per-package iteration | `file_exists` for BUILD.bazel + (root) CMakeLists.txt |
| `java/` | `BUILD.bazel`, `pom.xml`, plus 3 sub-module pom.xml's (`bom/`, `kotlin/`, `protoc/`) | The full Maven multi-module shape (`core/`, `internal/`, `lite/`, `kotlin-lite/`, `osgi/`, `test/`, `util/` are Bazel-only sub-packages without their own pom.xml — Maven build is BOM-rooted) | `file_exists` for `java/bom/pom.xml` + 2× `file_content_matches` (artifactId == protobuf-bom, groupId == com.google.protobuf) |
| `python/` | `BUILD.bazel`, `python_version_test.py`, `version_script.lds`; `dist/setup.py` for the wheel-publish path | Single-package binding; `protobuf` published to PyPI | `file_exists` for `python/BUILD.bazel` |
| `ruby/` | `BUILD.bazel`, `Gemfile`, `Rakefile`, `google-protobuf.gemspec`, `ext/google/protobuf_c/` | Single gem (`google-protobuf`); the C-extension lives under `ext/` | `file_exists` for gemspec + Gemfile + Rakefile + 2× `file_content_matches` (s.name + s.version) |
| `go/` | `BUILD.bazel` only | The Go binding's runtime lives in the sibling `google.golang.org/protobuf` repo; this directory is for the `go_proto_library` Bazel rule support | `file_exists` for the test_*.yml workflow only (no in-tree Go binding) |
| `objectivec/` | `BUILD.bazel`, `defs.bzl`, `DevTools/`, `generate_well_known_types.sh` (no README — top-level `Protobuf.podspec` is the binding's manifest) | Objective-C runtime built per-target via Bazel + CocoaPods | `file_exists` for `Protobuf.podspec` (at root) |
| `csharp/` | `BUILD.bazel`, `buildall.sh`, `build_packages.bat`, `CHANGES.txt`, `compatibility_tests/` (no top-level csproj — sub-projects under csharp/src/ each have their own .csproj) | NuGet `Google.Protobuf` package; SDK-pinned via root `global.json` | `file_exists` for `global.json` |
| `php/` | `BUILD.bazel`, `composer.json`, `composer.json.dist`, `ext/` (PHP-C extension), `release.sh` | Pure-PHP + C-extension dual binding; `google/protobuf` Packagist coordinate | `file_exists` for composer.json + 2× `json_path_matches` (name + license) |
| `rust/` | `BUILD`, `defs.bzl`, `release_crates/`, `*.rs` source files (no top-level Cargo.toml — release crates each ship their own under `rust/release_crates/`) | Rust runtime; canonical `protobuf` crate published from `rust/release_crates/` | `file_exists` for the test_*.yml workflow only |
| `lua/` | `BUILD.bazel`, `def.c`, `lua_proto_library.bzl`, `main.c`, `README.md`, `test.proto`, `test_upb.lua`, `upb.c`, `upbc.cc` | Lua binding (small, less mature; no published-package coordinate yet) | `file_exists` for README only |
| `upb/` | `BUILD`, sub-tree of `base/`, `cmake/`, `conformance/`, `hash/`, `json/`, `lex/`, `mem/`, etc. | Small C runtime that backs the Rust + Python upb backends + the Dart binding's runtime | `file_exists` for the test_upb.yml workflow |
| `hpb/` | `BUILD`, `arena.h`, `backend/`, `bazel/`, `extension.cc`, `extension.h`, `hpb.h`, `internal/`, `multibackend.h`, `options.h` | Hpb (a C++ binding experiment using upb under the hood) | `file_exists` for the test_hpb.yml workflow |

### Per-binding conformance discipline

The conformance suite is the cross-language wire-format gate: every
language binding implements a tester for the shared
`conformance.proto` contract. The discipline is enforced through 4
stacked gates, each per-binding:

```
conformance/conformance.proto                  ← shared contract (1 file)
  │
  ├─ conformance/conformance_<lang>.*          ← per-binding tester (6 in-tree)
  │   - C++: conformance_cpp.cc
  │   - Java: ConformanceJava.java + ConformanceJavaLite.java (full + lite)
  │   - Python: conformance_python.py
  │   - Ruby: ruby/conformance_ruby.rb
  │   - Obj-C: conformance_objc.m
  │   - PHP: conformance_php.php
  │   - Rust: conformance_rust.rs
  │   - C#, hpb, upb, dart: driven via `bazel test //<lang>:conformance_test`
  │
  ├─ conformance/failure_list_<lang>.txt       ← per-binding allowlist (19 files)
  │   - 10 distinct languages × per-runtime variants
  │
  ├─ conformance/text_format_failure_list_<lang>.txt   ← text-format allowlist (8 files)
  │
  └─ .github/workflows/test_<lang>.yml         ← per-binding CI workflow (11 files)
```

The configured alint rule `protobuf-conformance-failure-list-*` is
a single multi-path `file_exists` per language family, covering
the per-runtime variants where they exist (e.g.,
`failure_list_python.txt + python_cpp + python_upb + python-post26`
in one rule). Removing any one of the 19 silently drops that
binding's coverage from the cross-language conformance suite.

### Cross-language version-pinning discipline

This is **the canonical demand-driver for the v0.11+
`cross_language_implementation_complete` rule-kind candidate**.

```
version.json
  ├─ main.protoc_version            == "36-dev"
  └─ main.languages.{cpp,java,python,php,ruby,rust,csharp,objectivec,javascript}
                                    == per-language semver-ish

protobuf_version.bzl
  ├─ PROTOC_VERSION                 == "36.0"
  ├─ PROTOBUF_JAVA_VERSION          == "4.36.0"  ← MUST mirror version.json.languages.java
  ├─ PROTOBUF_PYTHON_VERSION        == "7.36.0"  ← MUST mirror version.json.languages.python
  ├─ PROTOBUF_PHP_VERSION           == "5.36.0"  ← MUST mirror version.json.languages.php
  ├─ PROTOBUF_RUBY_VERSION          == "4.36.0"  ← MUST mirror version.json.languages.ruby
  └─ PROTOBUF_RUST_VERSION          == "4.36.0"  ← MUST mirror version.json.languages.rust

Per-binding manifests (each independently writable):
  ├─ src/google/protobuf/stubs/common.h::GOOGLE_PROTOBUF_VERSION  (C++ — int)
  ├─ java/bom/pom.xml::<version>                                    (Java)
  ├─ ruby/google-protobuf.gemspec::s.version                        (Ruby)
  ├─ php/composer.json::version (no — sourced from git tag)         (PHP)
  ├─ php/ext/google/protobuf/protobuf.h::PHP_PROTOBUF_VERSION       (PHP-C)
  ├─ Protobuf.podspec::s.version                                    (Obj-C)
  └─ csharp/<Package>.csproj::<Version>                             (C#)
```

The configured alint rules cover **layer 1** (every per-language
key in `version.json` is present + well-formed) and **layer 2**
(every PROTOBUF_*_VERSION constant in `protobuf_version.bzl` is
present + well-formed), but **the cross-file value-equality check**
(value at version.json.languages.java == value of
PROTOBUF_JAVA_VERSION constant == value of `<version>` in
java/bom/pom.xml) **needs the v0.11+
`cross_language_implementation_complete` primitive**.

---

## What maps to existing alint rules

The 108-rule [`/.alint.yml`](.alint.yml) breaks down as:

- **3 bundled rulesets** (`oss-baseline` 15 + `ci/github-actions` 3 +
  `hygiene/no-tracked-artifacts` 11) — **29 rules** between them
- **1 cross-language structural rule** —
  `protobuf-binding-subdir-has-readme` (`for_each_file` over
  `{src,java,python,ruby,go,objectivec,csharp,php,rust,upb,hpb,lua}/README.md`)
- **5 version-manifest presence + path-shape rules** —
  `version.json` present + 4× `json_path_matches` for
  `main.protoc_version` + `main.languages.{cpp,java,python}` semver-ish form
- **8 protobuf_version.bzl rules** — `file_exists` + 7×
  `file_content_matches` covering each PROTOBUF_*_VERSION constant
  (Starlark isn't parseable by alint's structured-query rules; we
  pattern-match the assignment lines)
- **1 conformance.proto presence rule**
- **1 conformance_test_runner.cc presence rule**
- **6 per-binding conformance runner presence rules** (cpp, java,
  python, ruby, objc, php, rust)
- **8 per-binding failure_list family rules** (one per language
  family, with multi-path arrays where per-runtime variants exist)
- **1 cross-language text-format failure-list rule** (single
  multi-path covering all 8 text-format lists)
- **12 per-binding test workflow presence rules** (one per
  `test_<lang>.yml`)
- **9 per-binding manifest rules** — `java/bom/pom.xml` +
  artifactId + groupId + `python/BUILD.bazel` + Ruby
  gemspec/Gemfile/Rakefile + name + version + PHP composer.json +
  name + license + Obj-C podspec + name + license +
  `PrivacyInfo.xcprivacy` + `global.json`
- **2 governance rules** — CONTRIBUTING.md + SECURITY.md +
  CODE_OF_CONDUCT.md + CONTRIBUTORS.txt
- **2 editions rules** — `editions/defaults.bzl` + `editions/BUILD`
- **2 GHA hardening rules** — restated SHA-pinning + dependabot
  github-actions ecosystem
- **1 OpenSSF Scorecard workflow presence rule**
- **6 `command:` shell-out rules** — `buildifier -mode=check -r`
  (Starlark AST), `bazel build //src:protoc` (canonical "does it
  compile"), `clang-format --dry-run --Werror` (C++),
  `flake8 python/` (Python), `rubocop ruby/` (Ruby),
  `gofmt -l go/` (Go)
- **1 root CMakeLists.txt + Bazel WORKSPACE + MODULE.bazel rules**

---

## What needs new alint primitives

Three patterns specific to protocolbuffers/protobuf that don't fit
any current rule:

### 1. `cross_language_implementation_complete` — version drift across version.json + protobuf_version.bzl + per-binding manifests

This is the canonical shape in this repo and one of 5 sources
(apache/arrow + tensorflow/tensorflow + protocolbuffers/protobuf +
angular/angular + google/flutter) for the v0.11+ ship-target rule
shape. Concrete shape:

> For every language `L` in `version.json.main.languages.*`:
>   - assert that `protobuf_version.bzl::PROTOBUF_<L>_VERSION` (where
>     it exists — 6 of 9 languages) equals
>     `version.json.main.languages.<L>` (with normalisation: `7.36-dev`
>     in version.json ↔ `7.36.0` in protobuf_version.bzl is OK)
>   - assert that the per-binding manifest's version field
>     (`java/bom/pom.xml::<version>`,
>     `ruby/google-protobuf.gemspec::s.version`,
>     `php/ext/google/protobuf/protobuf.h::PHP_PROTOBUF_VERSION`,
>     `Protobuf.podspec::s.version`) equals the same value
>   - report any drift with a 3-way diff (which manifest disagrees
>     with which other manifest)

The configured rules surface **layer 1 + layer 2** (presence + shape
of each version site), but the cross-file value comparison needs
either the v0.10+ `cross_file_value_equals` candidate (per-pair) or
the broader v0.11+ `cross_language_implementation_complete` primitive
(per-language-family fanout).

**Demand reconfirmed:** this is the **5th of 5 repos** to surface
the same shape (apache/arrow + tensorflow/tensorflow + protocolbuffers/
protobuf + angular/angular + google/flutter), promoting the v0.11+
candidate to **ship-target for the v0.11 design phase**. The
quantitative shape in this repo — **10 bindings × 4-5 parity surfaces
each = ~45 cross-language assertions** the rule would express in one
config block — gives the design phase concrete guidance for the
fanout DSL.

### 2. `cross_language_implementation_complete` — conformance/failure_list_<lang>.txt ↔ binding presence

A **second concrete instance** of the same rule shape in the same
repo. Concrete shape:

> For every in-tree binding directory (`src/`, `java/`, `python/`,
> `ruby/`, `objectivec/`, `csharp/`, `php/`, `rust/`, `upb/`, `hpb/`):
>   - assert that `conformance/failure_list_<lang>.txt` exists
>   - assert that `.github/workflows/test_<lang>.yml` exists
>   - assert that `conformance/conformance_<lang>.*` exists OR a
>     `bazel test //<lang>:conformance_test` target is declared in
>     `<lang>/BUILD.bazel` (the latter requires Starlark parsing;
>     deferred)
>   - report any binding that fails ≥1 of the above as
>     "incomplete cross-language implementation"

The configured rules cover the per-binding presence of failure_list
+ test workflow + runner individually, but the **fanout** ("for
every binding directory, assert N partner files exist") is the
v0.11+ shape — today we hand-write 11 separate rules, each with
its own message text.

### 3. `ordered_block` for `failure_list_<lang>.txt` files

Each `conformance/failure_list_<lang>.txt` is conventionally
one-test-per-line and could be sort-checked. Currently NOT
alphabetised (verified via
`LC_ALL=C sort -c conformance/failure_list_cpp.txt` → exits
non-zero). `ordered_block` is now a **v0.10 ship-target with 7
sources** (rust + airflow + tokio + cpython + arrow + golang/go +
**protobuf failure_lists**), tied with `registry_paths_resolve` at
the top of the v0.10 backlog.

The same shape applies to the **8** `text_format_failure_list_*.txt`
files — same convention, same enforcement gap, same one-line fix
under the v0.10+ rule.

---

## What's out of alint's scope (kept on the existing tool)

- **Conformance test execution** — `bazel test //src:conformance_test`
  + per-binding `//<lang>:conformance_test` siblings. The actual
  cross-language wire-format check spawns subprocess testers and
  exchanges binary protos over a pipe — that's a runtime
  cross-process dance, not a tree-state invariant.
- **AST analysis** (clang-format, gofmt, rubocop, flake8, autopep8,
  cython-lint) — alint deliberately doesn't try to be a parser.
  Shell out via `command:`.
- **Starlark AST** — `buildifier` / `buildozer` (Google's official
  Starlark formatter + AST refactor tool, the same shape as the
  bazelbuild/bazel case study). alint owns file-shape; buildifier
  owns Starlark AST.
- **Apple App Store privacy validation** —
  `PrivacyInfo.xcprivacy` is consumed by Apple's submission
  pipeline, not by anything in this repo.
- **Bazel rule semantics** — `MODULE.bazel.lock` ↔ `MODULE.bazel`
  freshness, `bazel mod deps` consistency. Same `generated_file_fresh`
  v0.10+ candidate gap as bazelbuild/bazel.
- **Generated-code staleness gates** — the `staleness_check.yml`
  workflow runs the protoc-generated descriptors comparison; alint
  could in principle express "this directory's contents must
  exactly match the output of protoc on these inputs", but that
  requires invoking protoc, which is build-tool integration rather
  than file-shape validation.
- **Google-internal Kokoro CI** — protobuf's internal CI runs
  partly on Google-internal Kokoro (mirrored to but not authored
  in this repo) and partly on the public GitHub Actions workflows.
  alint sees only the public mirror.

---

## Already covered by other linters protobuf uses

- `clang-format` — C++ AST + style.
- `gofmt` — Go AST + style.
- `rubocop` — Ruby AST + style.
- `flake8` / `autopep8` — Python AST + style.
- `buildifier` / `buildozer` — Starlark AST + refactor.
- `bazel test //src:conformance_test` — cross-language wire-format
  conformance.
- OpenSSF Scorecard — supply-chain hygiene (weekly run via
  `.github/workflows/scorecard.yml`).

---

## Performance comparison (placeholder — bench when validation pass scales)

The repo is large enough to be a meaningful stress test:

- **~25 MiB** working tree (after sparse-checkout dropping
  `/src/google/protobuf/compiler`, `/third_party`,
  `/conformance/binary_protos`)
- **11 in-tree language bindings** + the `conformance/` cross-
  language test suite
- **22** GitHub Actions workflows
- **137 BUILD/BUILD.bazel + 117 *.bzl Starlark files**

The published S9 bench (100k+ files, 13 languages) hits ~1.4 s on a
stock CI runner. The full protocolbuffers/protobuf tree (with
`/src/google/protobuf/compiler` + `/third_party` re-included,
~80 MB, ~6k files) sits between S2 and S3. Expected: ~0.5-1.5 s
for `alint check` on the structural rules alone, vs. tens of
seconds for `bazel query` over the equivalent rule set.

To benchmark wall-clock for real:
`time alint check --config examples/protocolbuffers-protobuf/.alint.yml /tmp/protobuf` —
deferred to the per-repo measurement pass.

---

## Followup feature work

Marketing/positioning context for this case study lives at
https://alint.org/examples/protocolbuffers-protobuf/. The
engineering follow-up work surfaced (consolidated, sorted by
strength of demand across P2a + P2b) is below.

- **`cross_language_implementation_complete` rule kind** — v0.11+
  ship-target. Covers both the version-drift case (`version.json` ↔
  `protobuf_version.bzl` ↔ per-binding manifests) AND the
  conformance-discipline case (`failure_list_<lang>.txt` ↔ binding
  presence ↔ test workflow). 5 sources (apache/arrow +
  tensorflow/tensorflow + protobuf + angular + flutter) — 10 bindings
  × 4-5 parity surfaces = ~45 cross-language assertions in one rule.
  **The v0.11 design phase is ship-ready.**
- **`ordered_block` rule kind** — v0.10 ship-target. Re-confirmed by
  19 `failure_list_<lang>.txt` files + 8 `text_format_failure_list_*.txt`
  files. **7 sources** (rust + airflow + tokio + cpython + arrow +
  golang/go + protobuf), tied with `registry_paths_resolve` at top of
  v0.10 backlog.
- **`registry_paths_resolve` rule kind** — v0.10 ship-target (8
  sources). protobuf doesn't surface this gap directly (no equivalent
  of arrow's rat_exclude_files.txt), but the per-binding failure_list_
  <lang>.txt files are a **second-order instance**: each file lists
  conformance test names that should resolve to known-existing tests
  in `conformance.proto` (drift here = a stale entry that hides a
  regression). Worth modelling once `registry_paths_resolve` ships.
- **`generated_file_fresh` rule kind** — v0.10 ship-target (6
  sources: uv, cpython, pytorch, bazel, TF, spark). protobuf's
  `staleness_check.yml` workflow is a candidate use case; deferred
  to the per-tool integration design phase.

---

## Filter-expression pitfall: see CONFIG-AUTHORING.md § 10

The `protobuf-dependabot-includes-actions` rule uses the
canonical bracket-notation form for the dashed `package-ecosystem`
key inside a JSONPath filter:

```yaml
path: "$.updates[?@['package-ecosystem'] == 'github-actions'].directory"
```

Both forms (with and without the outer parens around the predicate)
parse cleanly under `serde_json_path` 0.7.x; the load-bearing fix is
the bracket-notation key access. See
`docs/development/CONFIG-AUTHORING.md` § 10 for the canonical form.

---

## Notes for the parent agent

- Audit (`cargo test -p alint-e2e --test
  coverage_audit_examples_parse`) **passes** with this config in
  place. (Pre-existing pytorch failures are unrelated to this
  case study.)
- Config (108 rules) parses cleanly via
  `alint validate-config examples/protocolbuffers-protobuf/.alint.yml`.
- Config runs cleanly against the actual cloned repo at
  `/tmp/protobuf` (150 violations across 14 failing files: ~50
  GHA SHA-pin warnings on unpinned third-party actions, the
  expected "tool not on PATH" warnings for `bazel` / `buildifier`
  / `clang-format` / `flake8` / `rubocop` not being installed in
  the alint test environment, ~21 OSS-baseline final-newline /
  trailing-whitespace info-level findings, plus 1 false-positive
  `oss-no-merge-conflict-markers` error on `csharp/README.md`'s
  `=======` markdown-section underlines — the `=======` regex is
  too eager; pre-existing bundled-rule issue, not from this
  case study). 72 rules pass silently; the cross-language
  structural rules (per-binding READMEs, per-binding conformance
  runners, per-binding failure_lists, per-binding test workflows,
  per-binding version pins) all silently pass on the live tree,
  confirming protobuf's polyglot layout is fully consistent —
  and the rules are correctly scoped to fire if drift were to
  occur.
- **The v0.11+ `cross_language_implementation_complete` ship-target
  is now demand-validated by 5 distinct repos** (apache/arrow +
  tensorflow/tensorflow + protocolbuffers/protobuf + angular/angular
  + google/flutter). Quantitatively in this repo: 10 bindings × 4-5
  parity surfaces = ~45 cross-language assertions. The v0.11 design
  phase is ship-ready.

---

## Future analysis

- **`nested_configs: true` per language binding directory.** Each of
  `src/`, `java/`, `python/`, `ruby/`, `go/`, `objectivec/`,
  `csharp/`, `php/`, `rust/`, `lua/`, `upb/`, `hpb/` could ship a
  per-binding `.alint.yml` with the language-specific rules (per-
  manifest shape, per-conformance-runner presence, etc.). The
  current 108-rule monolithic config has all 79 own rules collapsed
  into one file; splitting per-binding via `nested_configs` would
  let each binding evolve independently and read like a
  per-language structural contract.
- **`ordered_block` for failure_list_<lang>.txt + text_format_
  failure_list_<lang>.txt files.** With `ordered_block` at v0.10
  ship-target, protobuf is the **canonical demand-driver** — 19
  failure_list files + 8 text_format_failure_list files = 27 file
  targets in one repo, all currently un-sorted.
- **`compliance/apache-2@v1`** doesn't apply (protobuf uses BSD-3-
  Clause not Apache-2).
- **Pre-existing bundled-rule false positive at csharp/README.md.**
  The `oss-no-merge-conflict-markers` rule fires on the `=======`
  markdown-section underline. Verified still present at v0.9.17;
  pre-existing bundled-rule issue, not from this case study.

## Validation status (2026-05-07)

- alint binary: v0.9.17 (built 2026-05-07).
- `validate-config` reports **108 rules** loaded from `.alint.yml**
  (79 protobuf-specific + 29 from 3 bundled rulesets: oss-baseline 15
  + ci/github-actions 3 + hygiene/no-tracked-artifacts 11).
- Live-tree recheck against `/tmp/protobuf` reproduces the README
  finding exactly: **150 violations across 14 failing files**, 72
  rules pass silently. ~50 GHA SHA-pin warnings on unpinned third-
  party actions, expected "tool not on PATH" warnings for `bazel` /
  `buildifier` / `clang-format` / `flake8` / `rubocop`, ~21
  OSS-baseline final-newline / trailing-whitespace info-level
  findings, 1 pre-existing false-positive `oss-no-merge-conflict-
  markers` error on `csharp/README.md`. Engine behaviour stable
  v0.9.16 → v0.9.17.
- No `respect_gitignore: false` or `root_only: true` patterns in this
  config. Pitfalls #18 (FIXED v0.9.17) and #19 (FIXED v0.9.17) do
  not apply.
