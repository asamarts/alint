---
title: 'oss-baseline@v1'
description: Bundled alint ruleset at alint://bundled/oss-baseline@v1.
---

Adopt with:

```yaml
extends:
  - alint://bundled/oss-baseline@v1
```

## Rules

### `oss-readme-exists`

- **kind**: `file_exists`
- **level**: `warning`
- **policy**: <https://opensource.guide/starting-a-project/#writing-a-readme>

> An open-source repo should have a README at the root.

### `oss-license-exists`

- **kind**: `file_exists`
- **level**: `warning`
- **policy**: <https://opensource.guide/legal/#which-open-source-license-is-appropriate-for-my-project>

> An open-source repo should declare a license at the root.

### `oss-license-non-empty`

- **kind**: `file_min_size`
- **level**: `info`

> LICENSE file is suspiciously short; paste the full license text rather than a stub.

### `oss-readme-non-stub`

- **kind**: `file_min_lines`
- **level**: `info`

> README is very short; add a brief description and at least a usage / install section.

### `oss-security-policy-exists`

- **kind**: `file_exists`
- **level**: `info`
- **policy**: <https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository>

> Consider adding a SECURITY.md so vulnerability reporters know where to disclose.

### `oss-code-of-conduct-exists`

- **kind**: `file_exists`
- **level**: `info`
- **policy**: <https://www.contributor-covenant.org/>

> Consider adding a CODE_OF_CONDUCT.md.

### `oss-gitignore-exists`

- **kind**: `file_exists`
- **level**: `info`

> Most OSS repos should have a .gitignore to keep build artefacts and secrets out of git.

### `oss-no-merge-conflict-markers`

- **kind**: `no_merge_conflict_markers`
- **level**: `error`

> Merge-conflict markers must not be committed.

### `oss-no-bidi-controls`

- **kind**: `no_bidi_controls`
- **level**: `error`
- **policy**: <https://trojansource.codes/>

> Unicode bidi override characters are a code-review hazard; reject.

### `oss-final-newline`

- **kind**: `final_newline`
- **level**: `info`

### `oss-no-trailing-whitespace`

- **kind**: `no_trailing_whitespace`
- **level**: `info`

