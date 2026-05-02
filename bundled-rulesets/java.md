---
title: 'java@v1'
description: Bundled alint ruleset at alint://bundled/java@v1.
---

Hygiene checks for Java projects (Maven + Gradle). Adopt it
with:

```yaml
extends:
  - alint://bundled/java@v1
```

Gated with `when: facts.has_java` (true if any Java build
manifest exists anywhere in the tree) plus a per-rule
`scope_filter: { has_ancestor: [pom.xml, build.gradle,
build.gradle.kts] }` on per-file content rules so they only
apply to files inside a Java module — useful in polyglot
monorepos where Java modules sit alongside Rust / Node /
Python subdirectories.

Build outputs (`target/` for Maven, `build/` for Gradle) are
checked with `git_tracked_only: true` so a developer's
locally-built artefacts don't trigger the rule — only
committed contents do, which is the actual hygiene we care
about.

## Rules

### `java-manifest-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **when**: `facts.has_java`
- **policy**: <https://maven.apache.org/guides/introduction/introduction-to-the-pom.html>

> Java project: `pom.xml` (Maven) or `build.gradle` / `build.gradle.kts` (Gradle) at the root is required.

### `java-build-wrapper-committed`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **when**: `facts.has_java`
- **policy**: <https://docs.gradle.org/current/userguide/gradle_wrapper.html>

> A committed build wrapper (`mvnw` for Maven, `gradlew` for Gradle) lets contributors and CI build the project without pre-installing the right Maven / Gradle version.

### `java-no-tracked-target`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`
- **when**: `facts.has_java`

> Maven's `target/` is a build directory and shouldn't be committed. Add `target/` to your `.gitignore`.

### `java-no-tracked-build`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`
- **when**: `facts.has_java`

> Gradle's `build/` is a build directory and shouldn't be committed. Add `build/` to your `.gitignore`.

### `java-no-class-files`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `error`
- **when**: `facts.has_java`

> Compiled `.class` files don't belong in version control. Build them from the `.java` sources instead.

### `java-sources-pascal-case`

- **kind**: [`filename_case`](/docs/rules/naming/filename_case/)
- **level**: `warning`
- **when**: `facts.has_java`

> Java filenames should match the public class name (PascalCase).

### `java-sources-final-newline`

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `info`
- **when**: `facts.has_java`

### `java-sources-no-trailing-whitespace`

- **kind**: [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/)
- **level**: `info`
- **when**: `facts.has_java`

### `java-sources-no-bidi`

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **when**: `facts.has_java`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars in Java sources are rejected.

### `java-sources-no-zero-width`

- **kind**: [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/)
- **level**: `error`
- **when**: `facts.has_java`

> Zero-width characters in Java sources are rejected (review hazard).

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/java.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/java.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/java@v1
#
# Hygiene checks for Java projects (Maven + Gradle). Adopt it
# with:
#
#     extends:
#       - alint://bundled/java@v1
#
# Gated with `when: facts.has_java` (true if any Java build
# manifest exists anywhere in the tree) plus a per-rule
# `scope_filter: { has_ancestor: [pom.xml, build.gradle,
# build.gradle.kts] }` on per-file content rules so they only
# apply to files inside a Java module — useful in polyglot
# monorepos where Java modules sit alongside Rust / Node /
# Python subdirectories.
#
# Build outputs (`target/` for Maven, `build/` for Gradle) are
# checked with `git_tracked_only: true` so a developer's
# locally-built artefacts don't trigger the rule — only
# committed contents do, which is the actual hygiene we care
# about.

version: 1

facts:
  - id: has_java
    any_file_exists:
      - pom.xml
      - "**/pom.xml"
      - build.gradle
      - "**/build.gradle"
      - build.gradle.kts
      - "**/build.gradle.kts"
      - settings.gradle
      - "**/settings.gradle"
      - settings.gradle.kts
      - "**/settings.gradle.kts"

rules:
  # --- Manifest -----------------------------------------------------
  - id: java-manifest-exists
    when: facts.has_java
    # Maven (`pom.xml`) or Gradle (Groovy or Kotlin DSL). Either
    # is fine; mixed setups are unusual but valid.
    kind: file_exists
    paths:
      - pom.xml
      - build.gradle
      - build.gradle.kts
    root_only: true
    level: error
    message: >-
      Java project: `pom.xml` (Maven) or `build.gradle` /
      `build.gradle.kts` (Gradle) at the root is required.
    policy_url: "https://maven.apache.org/guides/introduction/introduction-to-the-pom.html"

  # --- Wrapper scripts for reproducible builds ----------------------
  - id: java-build-wrapper-committed
    when: facts.has_java
    # `mvnw` (Maven Wrapper) or `gradlew` (Gradle Wrapper) make
    # CI builds and contributor onboarding deterministic. Either
    # script suffices.
    kind: file_exists
    paths:
      - mvnw
      - gradlew
    root_only: true
    level: info
    message: >-
      A committed build wrapper (`mvnw` for Maven, `gradlew` for
      Gradle) lets contributors and CI build the project without
      pre-installing the right Maven / Gradle version.
    policy_url: "https://docs.gradle.org/current/userguide/gradle_wrapper.html"

  # --- Build outputs must not be committed --------------------------
  # We use `git_tracked_only: true` here so the rule only fires
  # if `target/` is *committed* — a developer's locally-built
  # `target/` (gitignored, no tracked content) is silently OK.
  # Same shape for Gradle's `build/`.
  - id: java-no-tracked-target
    when: facts.has_java
    kind: dir_absent
    paths: "**/target"
    git_tracked_only: true
    level: error
    message: >-
      Maven's `target/` is a build directory and shouldn't be
      committed. Add `target/` to your `.gitignore`.

  - id: java-no-tracked-build
    when: facts.has_java
    kind: dir_absent
    paths: "**/build"
    git_tracked_only: true
    level: error
    message: >-
      Gradle's `build/` is a build directory and shouldn't be
      committed. Add `build/` to your `.gitignore`.

  - id: java-no-class-files
    when: facts.has_java
    kind: file_absent
    paths: "**/*.class"
    git_tracked_only: true
    level: error
    message: >-
      Compiled `.class` files don't belong in version control.
      Build them from the `.java` sources instead.

  # --- Source-file conventions --------------------------------------
  - id: java-sources-pascal-case
    when: facts.has_java
    # Java's class-file convention: every public top-level type
    # lives in a file named after it, in PascalCase. Some
    # repositories ship `package-info.java` / `module-info.java`
    # (lowercase, snake-shaped) — those are the only standard
    # exceptions, so we exclude them.
    kind: filename_case
    paths:
      include: ["**/*.java"]
      exclude:
        - "**/package-info.java"
        - "**/module-info.java"
    case: pascal
    level: warning
    message: "Java filenames should match the public class name (PascalCase)."

  - id: java-sources-final-newline
    when: facts.has_java
    kind: final_newline
    paths: "**/*.java"
    scope_filter:
      has_ancestor: [pom.xml, build.gradle, build.gradle.kts]
    level: info
    fix:
      file_append_final_newline: {}

  - id: java-sources-no-trailing-whitespace
    when: facts.has_java
    kind: no_trailing_whitespace
    paths: "**/*.java"
    scope_filter:
      has_ancestor: [pom.xml, build.gradle, build.gradle.kts]
    level: info
    fix:
      file_trim_trailing_whitespace: {}

  # --- Trojan Source defense on Java sources ------------------------
  - id: java-sources-no-bidi
    when: facts.has_java
    kind: no_bidi_controls
    paths: "**/*.java"
    scope_filter:
      has_ancestor: [pom.xml, build.gradle, build.gradle.kts]
    level: error
    message: "Trojan Source (CVE-2021-42574): bidi override chars in Java sources are rejected."
    policy_url: "https://trojansource.codes/"

  - id: java-sources-no-zero-width
    when: facts.has_java
    kind: no_zero_width_chars
    paths: "**/*.java"
    scope_filter:
      has_ancestor: [pom.xml, build.gradle, build.gradle.kts]
    level: error
    message: "Zero-width characters in Java sources are rejected (review hazard)."
```
