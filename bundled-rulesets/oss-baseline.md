---
title: 'oss-baseline@v1'
description: 'oss-baseline@v1 bundled alint ruleset: A minimal OSS-hygiene baseline, the documents and conventions most open-source repositories are expected to follow.'
---

A minimal OSS-hygiene baseline — the documents and conventions
most open-source repositories are expected to follow. Adopt it
with:

```yaml
extends:
  - alint://bundled/oss-baseline@v1
```

Defaults are deliberately non-blocking (`info` for community-doc
recommendations, `warning` for missing README/LICENSE, `error` for
unambiguous bugs like committed merge markers or bidi controls).
Upgrade severity in your own config when you're ready to enforce.

## What it checks

15 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [oss-readme-exists](#oss-readme-exists)<br>`warning` | An open-source repo should have a README at the root. |
| [oss-license-exists](#oss-license-exists)<br>`warning` | An open-source repo should declare a license at the root. |
| [oss-license-non-empty](#oss-license-non-empty)<br>`info` | LICENSE file is suspiciously short; paste the full license text rather than a stub. |
| [oss-readme-non-stub](#oss-readme-non-stub)<br>`info` | README is very short; add a brief description and at least a usage / install section. |
| [oss-security-policy-exists](#oss-security-policy-exists)<br>`info` | Consider adding a SECURITY.md so vulnerability reporters know where to disclose. |
| [oss-security-policy-non-empty](#oss-security-policy-non-empty)<br>`info` | SECURITY.md is suspiciously short; describe how to report vulnerabilities and the supported-versions policy. |
| [oss-dependency-update-tool](#oss-dependency-update-tool)<br>`info` | Consider configuring Dependabot or Renovate to keep dependencies and actions up to date. |
| [oss-codeowners-exists](#oss-codeowners-exists)<br>`info` | Consider adding a CODEOWNERS file so PR reviews are auto-routed. |
| [oss-codeowners-non-empty](#oss-codeowners-non-empty)<br>`info` | CODEOWNERS exists but appears empty; add at least one ownership rule (e.g. `* @org/team`). |
| [oss-code-of-conduct-exists](#oss-code-of-conduct-exists)<br>`info` | Consider adding a CODE\_OF\_CONDUCT.md. |
| [oss-gitignore-exists](#oss-gitignore-exists)<br>`info` | Most OSS repos should have a .gitignore to keep build artefacts and secrets out of git. |
| [oss-no-merge-conflict-markers](#oss-no-merge-conflict-markers)<br>`error` | Merge-conflict markers must not be committed. |
| [oss-no-bidi-controls](#oss-no-bidi-controls)<br>`error` | Unicode bidi override characters are a code-review hazard; reject. |
| [oss-final-newline](#oss-final-newline)<br>`info` | File must end with a single \n. |
| [oss-no-trailing-whitespace](#oss-no-trailing-whitespace)<br>`info` | No line may end with space or tab. |

## Rules

### `oss-readme-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **policy**: <https://opensource.guide/starting-a-project/#writing-a-readme>

> An open-source repo should have a README at the root.

```yaml
- id: oss-readme-exists
  kind: file_exists
  paths: ["README.md", "README", "README.rst", "readme.md", "readme"]
  root_only: true
  level: warning
  message: "An open-source repo should have a README at the root."
  policy_url: "https://opensource.guide/starting-a-project/#writing-a-readme"
```

### `oss-license-exists`

Changed in v0.9.18: case-variation broadening — `LICENSE.TXT` (uppercase extension) is dotnet/runtime's canonical filename and was rejected before. `LICENSE.rst` covers Python projects that ship the license in reST. `COPYING.md` / `COPYING.txt` cover less common variants surfaced across the case-study corpus. Filesystem globs are case-sensitive on most platforms; the brace expansion enumerates the case-variants explicitly.

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **policy**: <https://opensource.guide/legal/#which-open-source-license-is-appropriate-for-my-project>

> An open-source repo should declare a license at the root.

```yaml
- id: oss-license-exists
  kind: file_exists
  paths:
    - "LICENSE"
    - "LICENSE.{md,txt,TXT,rst}"
    - "license"
    - "license.{md,txt,rst}"
    - "LICENSE-APACHE"
    - "LICENSE-MIT"
    - "LICENSE-BSD"
    - "LICENSE-MPL"
    - "COPYING"
    - "COPYING.{md,txt}"
  root_only: true
  level: warning
  message: "An open-source repo should declare a license at the root."
  policy_url: "https://opensource.guide/legal/#which-open-source-license-is-appropriate-for-my-project"
```

### `oss-license-non-empty`

A zero-byte LICENSE passes `oss-license-exists` while providing zero legal guidance. 200 bytes is a safely permissive floor — the shortest common OSS license (MIT) runs about 1 KiB once you include the copyright notice. Path list mirrors `oss-license-exists` so both rules check the same set.

- **kind**: [`file_min_size`](/docs/rules/content/file_min_size/)
- **level**: `info`

> LICENSE file is suspiciously short; paste the full license text rather than a stub.

```yaml
- id: oss-license-non-empty
  kind: file_min_size
  paths:
    - "LICENSE"
    - "LICENSE.{md,txt,TXT,rst}"
    - "license"
    - "license.{md,txt,rst}"
    - "LICENSE-APACHE"
    - "LICENSE-MIT"
    - "LICENSE-BSD"
    - "LICENSE-MPL"
    - "COPYING"
    - "COPYING.{md,txt}"
  min_bytes: 200
  level: info
  message: "LICENSE file is suspiciously short; paste the full license text rather than a stub."
```

### `oss-readme-non-stub`

Catches the classic "# Project\\n\\nTODO\\n" README. Three lines is intentionally gentle — passes for most real READMEs without nagging early-stage repos.

- **kind**: [`file_min_lines`](/docs/rules/content/file_min_lines/)
- **level**: `info`

> README is very short; add a brief description and at least a usage / install section.

```yaml
- id: oss-readme-non-stub
  kind: file_min_lines
  paths: ["README.md", "README", "README.rst", "readme.md", "readme"]
  min_lines: 3
  level: info
  message: "README is very short; add a brief description and at least a usage / install section."
```

### `oss-security-policy-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository>

> Consider adding a SECURITY.md so vulnerability reporters know where to disclose.

```yaml
- id: oss-security-policy-exists
  kind: file_exists
  paths: ["SECURITY.md", ".github/SECURITY.md", "docs/SECURITY.md"]
  level: info
  message: "Consider adding a SECURITY.md so vulnerability reporters know where to disclose."
  policy_url: "https://docs.github.com/en/code-security/getting-started/adding-a-security-policy-to-your-repository"
```

### `oss-security-policy-non-empty`

Mirrors OpenSSF Scorecard's Security-Policy check: the file has to actually contain reporting guidance, not be a stub. 200 bytes is the same floor as the LICENSE rule's.

- **kind**: [`file_min_size`](/docs/rules/content/file_min_size/)
- **level**: `info`
- **policy**: <https://github.com/ossf/scorecard/blob/main/docs/checks.md#security-policy>

> SECURITY.md is suspiciously short; describe how to report vulnerabilities and the supported-versions policy.

```yaml
- id: oss-security-policy-non-empty
  kind: file_min_size
  paths: ["SECURITY.md", ".github/SECURITY.md", "docs/SECURITY.md"]
  min_bytes: 200
  level: info
  message: "SECURITY.md is suspiciously short; describe how to report vulnerabilities and the supported-versions policy."
  policy_url: "https://github.com/ossf/scorecard/blob/main/docs/checks.md#security-policy"
```

### `oss-dependency-update-tool`

OpenSSF Scorecard's Dependency-Update-Tool check. Either Dependabot or Renovate satisfies it; both are config-only (no extra Rust deps, no engine work). The wide path list covers every blessed location each tool reads.

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://github.com/ossf/scorecard/blob/main/docs/checks.md#dependency-update-tool>

> Consider configuring Dependabot or Renovate to keep dependencies and actions up to date.

```yaml
- id: oss-dependency-update-tool
  kind: file_exists
  paths:
    - ".github/dependabot.yml"
    - ".github/dependabot.yaml"
    - "renovate.json"
    - "renovate.json5"
    - ".renovaterc"
    - ".renovaterc.json"
    - ".github/renovate.json"
    - ".github/renovate.json5"
  level: info
  message: "Consider configuring Dependabot or Renovate to keep dependencies and actions up to date."
  policy_url: "https://github.com/ossf/scorecard/blob/main/docs/checks.md#dependency-update-tool"
```

### `oss-codeowners-exists`

OpenSSF Scorecard's Code-Review signal. Branch-protection itself is GitHub-API state alint can't see, but a CODEOWNERS file is the on-disk piece — it auto-routes review requests and signals review expectations to contributors.

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://github.com/ossf/scorecard/blob/main/docs/checks.md#code-review>

> Consider adding a CODEOWNERS file so PR reviews are auto-routed.

```yaml
- id: oss-codeowners-exists
  kind: file_exists
  paths: ["CODEOWNERS", ".github/CODEOWNERS", "docs/CODEOWNERS"]
  level: info
  message: "Consider adding a CODEOWNERS file so PR reviews are auto-routed."
  policy_url: "https://github.com/ossf/scorecard/blob/main/docs/checks.md#code-review"
```

### `oss-codeowners-non-empty`

Catches the empty / placeholder CODEOWNERS that satisfies Scorecard's existence check while doing nothing useful. 10 bytes is the smallest meaningful pattern + owner pair.

- **kind**: [`file_min_size`](/docs/rules/content/file_min_size/)
- **level**: `info`

> CODEOWNERS exists but appears empty; add at least one ownership rule (e.g. `* @org/team`).

```yaml
- id: oss-codeowners-non-empty
  kind: file_min_size
  paths: ["CODEOWNERS", ".github/CODEOWNERS", "docs/CODEOWNERS"]
  min_bytes: 10
  level: info
  message: "CODEOWNERS exists but appears empty; add at least one ownership rule (e.g. `* @org/team`)."
```

### `oss-code-of-conduct-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://www.contributor-covenant.org/>

> Consider adding a CODE\_OF\_CONDUCT.md.

```yaml
- id: oss-code-of-conduct-exists
  kind: file_exists
  paths: ["CODE_OF_CONDUCT.md", ".github/CODE_OF_CONDUCT.md", "docs/CODE_OF_CONDUCT.md"]
  level: info
  message: "Consider adding a CODE_OF_CONDUCT.md."
  policy_url: "https://www.contributor-covenant.org/"
```

### `oss-gitignore-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`

> Most OSS repos should have a .gitignore to keep build artefacts and secrets out of git.

```yaml
- id: oss-gitignore-exists
  kind: file_exists
  paths: .gitignore
  root_only: true
  level: info
  message: "Most OSS repos should have a .gitignore to keep build artefacts and secrets out of git."
```

### `oss-no-merge-conflict-markers`

- **kind**: [`no_merge_conflict_markers`](/docs/rules/security-unicode-sanity/no_merge_conflict_markers/)
- **level**: `error`

> Merge-conflict markers must not be committed.

```yaml
- id: oss-no-merge-conflict-markers
  kind: no_merge_conflict_markers
  paths:
    include: ["**/*.md", "**/*.txt", "**/*.toml", "**/*.yml", "**/*.yaml", "**/*.json"]
  level: error
  message: "Merge-conflict markers must not be committed."
```

### `oss-no-bidi-controls`

Trojan Source (CVE-2021-42574): bidirectional override characters can make source code read differently from how it executes.

- **kind**: [`no_bidi_controls`](/docs/rules/security-unicode-sanity/no_bidi_controls/)
- **level**: `error`
- **policy**: <https://trojansource.codes/>

> Unicode bidi override characters are a code-review hazard; reject.

```yaml
- id: oss-no-bidi-controls
  kind: no_bidi_controls
  paths:
    include: ["**/*.md", "**/*.txt", "**/*.toml", "**/*.yml", "**/*.yaml", "**/*.json"]
  level: error
  message: "Unicode bidi override characters are a code-review hazard; reject."
  policy_url: "https://trojansource.codes/"
```

### `oss-final-newline`

- **kind**: [`final_newline`](/docs/rules/text-hygiene/final_newline/)
- **level**: `info`
- **fix**: `file_append_final_newline` (applied by `alint fix`)

```yaml
- id: oss-final-newline
  kind: final_newline
  paths: ["**/*.md", "**/*.txt", "**/*.toml", "**/*.yml", "**/*.yaml"]
  level: info
  fix:
    file_append_final_newline: {}
```

### `oss-no-trailing-whitespace`

- **kind**: [`no_trailing_whitespace`](/docs/rules/text-hygiene/no_trailing_whitespace/)
- **level**: `info`
- **fix**: `file_trim_trailing_whitespace` (applied by `alint fix`)

```yaml
- id: oss-no-trailing-whitespace
  kind: no_trailing_whitespace
  paths: ["**/*.md", "**/*.txt", "**/*.toml", "**/*.yml", "**/*.yaml"]
  level: info
  fix:
    file_trim_trailing_whitespace: {}
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/oss-baseline@v1
rules:
  - id: oss-readme-exists
    level: off
  - id: oss-license-exists
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/oss-baseline@v1
    except: [oss-readme-exists]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/oss-baseline.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/oss-baseline.yml) in the alint repo.
