---
title: 'apache/governance@v1'
description: 'apache/governance@v1 bundled alint ruleset: Apache Top-Level Project (TLP) governance discipline.'
---

Apache Top-Level Project (TLP) governance discipline. The
governance / release-artefact baseline an Apache TLP is
expected to ship, that arrow + spark + airflow each
re-implement by hand. Adopt with:

```yaml
extends:
  - alint://bundled/apache/governance@v1
```

This is the GOVERNANCE superset, distinct from
`compliance/apache-2@v1` (license redistribution). It is safe
to adopt BOTH — rule ids here are namespaced `apache-gov-*`
so they never collide with `apache-2-*`; the LICENSE / header
overlap is intentional and each id is independently
`level: off`-able.

No fact gate — extending the ruleset is the user's signal
that the project is an Apache TLP. Levels are deliberately
tiered (legally load-bearing artefacts `error`; release
discipline `warning`; nice-to-have `info`); upgrade severity
in your own config when you are ready to enforce.

Scope: graduated TLPs (the three demand sources). Incubating
podlings additionally need a DISCLAIMER — layer that on
yourself (see the design doc, open question 1).

Still over-firing on generated / vendored / branded-header files?
apache-gov-source-license-header already excludes vendored (vendor/,
third_party/) and generated-by-naming (*.pb.go, zz_generated.*.go,
*_pb2.py, ...) trees, and accepts the SPDX form
(SPDX-License-Identifier: Apache-2.0); apache-gov-no-binaries-in-source
likewise skips third_party/. For residual project-specific cases,
override the rule in your own config: narrow its `paths:` to your
source dirs, or set `level: off`.

## What it checks

8 rules. Each links to its section below, which explains the check and shows its definition.

| Rule | Reports |
| --- | --- |
| [apache-gov-license-exists](#apache-gov-license-exists)<br>`error` | Apache governance: a TLP must ship a LICENSE file with the Apache-2.0 text at the repository root. |
| [apache-gov-notice-exists](#apache-gov-notice-exists)<br>`error` | Apache governance: a TLP must ship a NOTICE file at the repository root (Apache-2.0 §4(d)). |
| [apache-gov-notice-asf-attribution](#apache-gov-notice-asf-attribution)<br>`warning` | Apache governance: NOTICE should carry the ASF attribution in either the bare "Copyright `<year>` The Apache Software Foundation" or the long "This product includes software developed at / The Apache Software Foundation (https://www.apache.org/)." form. |
| [apache-gov-keys-exists](#apache-gov-keys-exists)<br>`warning` | Apache governance: a TLP source release is signed with the release managers' OpenPGP keys; ship a KEYS file at the repository root. |
| [apache-gov-source-license-header](#apache-gov-source-license-header)<br>`warning` | Apache RAT: source files must carry the canonical ASF header (short form or the long ASF-preamble form). |
| [apache-gov-no-binaries-in-source](#apache-gov-no-binaries-in-source)<br>`warning` | Apache release policy: an ASF source release must not contain compiled binaries (jars/classes/.so/...). |
| [apache-gov-readme-exists](#apache-gov-readme-exists)<br>`warning` | Apache governance: a TLP should ship a project README at the root. |
| [apache-gov-changelog-exists](#apache-gov-changelog-exists)<br>`info` | Apache release discipline: ship a CHANGES / CHANGELOG / RELEASE\_NOTES file so each release's user-visible changes are recorded. |

## Rules

### `apache-gov-license-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **policy**: <https://www.apache.org/legal/release-policy.html#license-file>

> Apache governance: a TLP must ship a LICENSE file with the Apache-2.0 text at the repository root.

```yaml
- id: apache-gov-license-exists
  kind: file_exists
  paths: ["LICENSE", "LICENSE.txt"]
  root_only: true
  level: error
  message: >-
    Apache governance: a TLP must ship a LICENSE file with the
    Apache-2.0 text at the repository root.
  policy_url: "https://www.apache.org/legal/release-policy.html#license-file"
```

### `apache-gov-notice-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `error`
- **policy**: <https://www.apache.org/legal/release-policy.html#notice-file>

> Apache governance: a TLP must ship a NOTICE file at the repository root (Apache-2.0 §4(d)).

```yaml
- id: apache-gov-notice-exists
  kind: file_exists
  paths: ["NOTICE", "NOTICE.txt"]
  root_only: true
  level: error
  message: >-
    Apache governance: a TLP must ship a NOTICE file at the
    repository root (Apache-2.0 §4(d)).
  policy_url: "https://www.apache.org/legal/release-policy.html#notice-file"
```

### `apache-gov-notice-asf-attribution`

The NOTICE must carry the ASF attribution, not merely exist — the check `compliance/apache-2@v1` does NOT do. Match only the invariant substring "The Apache Software Foundation": this covers BOTH the long template ("This product includes software developed at / The Apache Software Foundation (https://www.apache.org/).") AND the very common bare form ("Copyright `<year>` The Apache Software Foundation"). The `(https://www.apache.org/)` parenthetical is LICENSE-appendix boilerplate, NOT a NOTICE invariant — requiring it false-positived on legitimate TLP NOTICEs (P1 #44 / D1). `warning`, not `error`: a wording mismatch on a baseline-adoption ruleset must not hard-block.

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `warning`
- **policy**: <https://www.apache.org/legal/src-headers.html#notice>

> Apache governance: NOTICE should carry the ASF attribution in either the bare "Copyright `<year>` The Apache Software Foundation" or the long "This product includes software developed at / The Apache Software Foundation (https://www.apache.org/)." form.

```yaml
- id: apache-gov-notice-asf-attribution
  kind: file_content_matches
  paths: ["NOTICE", "NOTICE.txt"]
  pattern: 'The Apache Software Foundation'
  level: warning
  message: >-
    Apache governance: NOTICE should carry the ASF attribution
    in either the bare "Copyright <year> The Apache Software
    Foundation" or the long "This product includes software
    developed at / The Apache Software Foundation
    (https://www.apache.org/)." form.
  policy_url: "https://www.apache.org/legal/src-headers.html#notice"
```

### `apache-gov-keys-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **policy**: <https://www.apache.org/dev/release-signing.html#keys-policy>

> Apache governance: a TLP source release is signed with the release managers' OpenPGP keys; ship a KEYS file at the repository root.

```yaml
- id: apache-gov-keys-exists
  kind: file_exists
  paths: ["KEYS", "KEYS.txt"]
  root_only: true
  level: warning
  message: >-
    Apache governance: a TLP source release is signed with the
    release managers' OpenPGP keys; ship a KEYS file at the
    repository root.
  policy_url: "https://www.apache.org/dev/release-signing.html#keys-policy"
```

### `apache-gov-source-license-header`

Source files carry the ASF license header. Pattern + exclude set are inherited VERBATIM from the v0.9.18-broadened `compliance/apache-2@v1` `apache-2-source-has-license-header` rule (the "v0.9.18 A2 prerequisite"): the short-form-only pattern produced 8,228 false positives against airflow, the densest Apache TLP. Reusing A2's resolved pattern is deliberate — governance must not reintroduce that regression. v0.12: accept the ASF short form, the long ASF-preamble form, OR the modern SPDX identifier (`SPDX-License-Identifier: Apache-2.0`) that CNCF / branded-header projects (helm, istio, kubernetes) use. Pure false-positive reduction; rides @v1. Lockstep with A2.

- **kind**: [`file_header`](/docs/rules/content/file_header/)
- **level**: `warning`
- **policy**: <https://www.apache.org/legal/src-headers.html>

> Apache RAT: source files must carry the canonical ASF header (short form or the long ASF-preamble form). Full boilerplate: https://www.apache.org/licenses/LICENSE-2.0#apply.

```yaml
- id: apache-gov-source-license-header
  kind: file_header
  paths:
    include:
      ["**/*.{rs,py,js,jsx,ts,tsx,go,java,kt,c,cc,cpp,h,hpp,hh,sh,rb,swift,scala}"]
    exclude:
      # Vendored / third-party trees (CNCF + Google convention).
      - "**/vendor/**"
      - "**/node_modules/**"
      - "**/third_party/**"
      - "**/3rdparty/**"
      # Build output.
      - "**/target/**"
      - "**/build/**"
      - "**/dist/**"
      - "**/.cargo/**"
      - "**/generated/**"
      - "**/__generated__/**"
      # Generated source by naming convention. Codegen carries its own
      # header (or none); requiring the ASF header here false-positives
      # at scale across protobuf / kubernetes / istio / tensorflow.
      # Kept verbatim-lockstep with compliance/apache-2@v1.
      - "**/*.pb.go"
      - "**/*_grpc.pb.go"
      - "**/*.gen.go"
      - "**/*_generated.go"
      - "**/zz_generated.*.go"
      - "**/*_pb2.py"
      - "**/*_pb2_grpc.py"
      - "**/*.pb.cc"
      - "**/*.pb.h"
      - "**/*.pb.swift"
      - "**/*_pb.rb"
      - "**/*.generated.*"
  lines: 25
  pattern: '(Licensed (to the Apache Software Foundation|under the Apache License,?\s*Version 2)|SPDX-License-Identifier:\s*Apache-2\.0)'
  level: warning
  message: >-
    Apache RAT: source files must carry the canonical ASF
    header (short form or the long ASF-preamble form). Full
    boilerplate: https://www.apache.org/licenses/LICENSE-2.0#apply.
  policy_url: "https://www.apache.org/legal/src-headers.html"
```

### `apache-gov-no-binaries-in-source`

An Apache \*source release\* must be buildable from source — compiled binaries must not be tracked in the source tree. Conventional binary-fixture directories are excluded (arrow's format test data, spark's test jars legitimately ship); a project with binaries elsewhere narrows or disables this in its own config.

- **kind**: [`file_absent`](/docs/rules/existence/file_absent/)
- **level**: `warning`
- **policy**: <https://www.apache.org/legal/release-policy.html#what>

> Apache release policy: an ASF source release must not contain compiled binaries (jars/classes/.so/...). Build artefacts belong in the build output, not the source tree.

```yaml
- id: apache-gov-no-binaries-in-source
  kind: file_absent
  paths:
    include: ["**/*.{jar,war,ear,class,so,dll,dylib,a,o,pyc,pyo}"]
    exclude:
      - "**/vendor/**"
      - "**/node_modules/**"
      - "**/third_party/**"
      - "**/3rdparty/**"
      - "**/target/**"
      - "**/build/**"
      - "**/dist/**"
      - "**/test/**"
      - "**/tests/**"
      - "**/testdata/**"
      - "**/fixtures/**"
      - "**/__pycache__/**"
  level: warning
  message: >-
    Apache release policy: an ASF source release must not
    contain compiled binaries (jars/classes/.so/...). Build
    artefacts belong in the build output, not the source tree.
  policy_url: "https://www.apache.org/legal/release-policy.html#what"
```

### `apache-gov-readme-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`

> Apache governance: a TLP should ship a project README at the root.

```yaml
- id: apache-gov-readme-exists
  kind: file_exists
  paths: ["README.md", "README", "README.rst", "README.txt"]
  root_only: true
  level: warning
  message: "Apache governance: a TLP should ship a project README at the root."
```

### `apache-gov-changelog-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `info`
- **policy**: <https://www.apache.org/legal/release-policy.html#release-announcements>

> Apache release discipline: ship a CHANGES / CHANGELOG / RELEASE\_NOTES file so each release's user-visible changes are recorded.

```yaml
- id: apache-gov-changelog-exists
  kind: file_exists
  paths:
    - "CHANGES"
    - "CHANGES.md"
    - "CHANGES.txt"
    - "CHANGES.rst"
    - "CHANGELOG"
    - "CHANGELOG.md"
    - "CHANGELOG.rst"
    - "RELEASE_NOTES"
    - "RELEASE_NOTES.md"
    - "RELEASE_NOTES.txt"
  root_only: true
  level: info
  message: >-
    Apache release discipline: ship a CHANGES / CHANGELOG /
    RELEASE_NOTES file so each release's user-visible changes
    are recorded.
  policy_url: "https://www.apache.org/legal/release-policy.html#release-announcements"
```

## Customize

Every rule here can be overridden by id from your own `.alint.yml`: change its `level`, or set `level: off` to drop it. An id that doesn't exist is an error at config load, so a typo can't silently pass.

```yaml
extends:
  - alint://bundled/apache/governance@v1
rules:
  - id: apache-gov-license-exists
    level: off
  - id: apache-gov-notice-asf-attribution
    level: error
```

To take only part of the ruleset, filter it where you extend it with `only:` or `except:`:

```yaml
extends:
  - url: alint://bundled/apache/governance@v1
    except: [apache-gov-license-exists]
```

[Bundled rulesets](/docs/concepts/composition/bundled-rulesets/) covers versioning and how rulesets combine.

## Source

The full ruleset definition, comments included, is committed at [`crates/alint-dsl/rulesets/v1/apache/governance.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/apache/governance.yml) in the alint repo.
