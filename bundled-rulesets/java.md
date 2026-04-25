---
title: 'java@v1'
description: Bundled alint ruleset at alint://bundled/java@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/java@v1
```

## Rules

### `java-manifest-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **when**: `facts.is_java`
- **policy**: <https://maven.apache.org/guides/introduction/introduction-to-the-pom.html>

> Java project: `pom.xml` (Maven) or `build.gradle` / `build.gradle.kts` (Gradle) at the root is required.

### `java-build-wrapper-committed`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **when**: `facts.is_java`
- **policy**: <https://docs.gradle.org/current/userguide/gradle_wrapper.html>

> A committed build wrapper (`mvnw` for Maven, `gradlew` for Gradle) lets contributors and CI build the project without pre-installing the right Maven / Gradle version.

### `java-no-tracked-target`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`
- **when**: `facts.is_java`

> Maven's `target/` is a build directory and shouldn't be committed. Add `target/` to your `.gitignore`.

### `java-no-tracked-build`

- **kind**: [`dir_absent`](/docs/rules/existence/dir_absent/)
- **level**: `error`
- **when**: `facts.is_java`

> Gradle's `build/` is a build directory and shouldn't be committed. Add `build/` to your `.gitignore`.

### `java-no-class-files`

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `error`
- **when**: `facts.is_java`

> Compiled `.class` files don't belong in version control. Build them from the `.java` sources instead.

### `java-sources-pascal-case`

- **kind**: [`filename_case`](/docs/rules/naming/filename_case/)
- **level**: `warning`
- **when**: `facts.is_java`

> Java filenames should match the public class name (PascalCase).

### `java-sources-final-newline`

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `info`
- **when**: `facts.is_java`

### `java-sources-no-trailing-whitespace`

- **kind**: [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/)
- **level**: `info`
- **when**: `facts.is_java`

### `java-sources-no-bidi`

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **when**: `facts.is_java`
- **policy**: <https://trojansource.codes/>

> Trojan Source (CVE-2021-42574): bidi override chars in Java sources are rejected.

### `java-sources-no-zero-width`

- **kind**: [`no_zero_width_chars`](/docs/rules/security-unicode-sanity/no_zero_width_chars/)
- **level**: `error`
- **when**: `facts.is_java`

> Zero-width characters in Java sources are rejected (review hazard).

