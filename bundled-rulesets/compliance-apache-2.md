---
title: 'compliance/apache-2@v1'
description: 'Hygiene checks for repositories distributed under the Apache License, Version 2.0. alint bundled ruleset compliance/apache-2@v1.'
---

Hygiene checks for repositories distributed under the
Apache License, Version 2.0. Verifies the three artefacts
the license itself requires of redistributors:

1. A LICENSE file with the Apache-2.0 text.
2. A NOTICE file at the repository root.
3. The Apache header on each source file.

Adopt with:

```yaml
extends:
  - alint://bundled/compliance/apache-2@v1
```

No fact gate — extending the ruleset is the user's signal
that the project is Apache-2.0 licensed. If your project is
dual-licensed (e.g. Apache-2.0 OR MIT), extend this ruleset
AND set `level: off` on the rules you don't want firing
strictly.

Still over-firing on generated / vendored / branded-header files?
The source-header rule already excludes vendored (vendor/,
third_party/) and generated-by-naming (*.pb.go, zz_generated.*.go,
*_pb2.py, ...) trees, and accepts the SPDX form
(SPDX-License-Identifier: Apache-2.0). For residual project-specific
cases, override apache-2-source-has-license-header in your own config:
narrow its `paths:` to your source dirs, or set `level: off`.

## Rules

### `apache-2-license-text-present`

- **kind**: [`file_content_matches`](/docs/rules/content/file_content_matches/)
- **level**: `error`
- **policy**: <https://www.apache.org/licenses/LICENSE-2.0>

> Apache-2.0 compliance: LICENSE must contain the Apache License, Version 2.0 text. Pull the canonical copy from https://www.apache.org/licenses/LICENSE-2.0.txt.

### `apache-2-notice-file-exists`

- **kind**: [`file_exists`](/docs/rules/existence/file_exists/)
- **level**: `warning`
- **policy**: <https://www.apache.org/licenses/LICENSE-2.0#redistribution>

> Apache-2.0 §4(d): distributions that include a NOTICE from upstream must propagate it. Even if your direct dependencies don't ship one, having a project-level NOTICE for your own attributions is the canonical Apache pattern.

### `apache-2-source-has-license-header`

- **kind**: [`file_header`](/docs/rules/content/file_header/)
- **level**: `warning`
- **policy**: <https://www.apache.org/licenses/LICENSE-2.0#apply>

> Apache-2.0: source files should carry the canonical Apache header. Use either the short form ("Licensed under the Apache License, Version 2.0...") or the long ASF-preamble form ("Licensed to the Apache Software Foundation (ASF) under one or more contributor license agreements..."). The full boilerplate is at https://www.apache.org/licenses/LICENSE-2.0#apply.

## Source

The full ruleset definition is committed at [`crates/alint-dsl/rulesets/v1/compliance/apache-2.yml`](https://github.com/asamarts/alint/blob/main/crates/alint-dsl/rulesets/v1/compliance/apache-2.yml) in the alint repo (the snapshot below is generated verbatim from that file).

```yaml
# alint://bundled/compliance/apache-2@v1
#
# Hygiene checks for repositories distributed under the
# Apache License, Version 2.0. Verifies the three artefacts
# the license itself requires of redistributors:
#
# 1. A LICENSE file with the Apache-2.0 text.
# 2. A NOTICE file at the repository root.
# 3. The Apache header on each source file.
#
# Adopt with:
#
#     extends:
#       - alint://bundled/compliance/apache-2@v1
#
# No fact gate — extending the ruleset is the user's signal
# that the project is Apache-2.0 licensed. If your project is
# dual-licensed (e.g. Apache-2.0 OR MIT), extend this ruleset
# AND set `level: off` on the rules you don't want firing
# strictly.
#
# Still over-firing on generated / vendored / branded-header files?
# The source-header rule already excludes vendored (vendor/,
# third_party/) and generated-by-naming (*.pb.go, zz_generated.*.go,
# *_pb2.py, ...) trees, and accepts the SPDX form
# (SPDX-License-Identifier: Apache-2.0). For residual project-specific
# cases, override apache-2-source-has-license-header in your own config:
# narrow its `paths:` to your source dirs, or set `level: off`.

version: 1

rules:
  # The LICENSE file at the repo root must contain the
  # Apache 2.0 text. We check by looking for the canonical
  # title line; full bit-for-bit comparison would be too
  # rigid (the SPDX template, the apache.org template, and
  # GitHub's auto-init differ in trailing whitespace and
  # CRLF/LF).
  - id: apache-2-license-text-present
    kind: file_content_matches
    paths: ["LICENSE", "LICENSE.md", "LICENSE.txt", "COPYING"]
    pattern: 'Apache License,?\s*Version 2'
    level: error
    message: >-
      Apache-2.0 compliance: LICENSE must contain the
      Apache License, Version 2.0 text. Pull the canonical
      copy from
      https://www.apache.org/licenses/LICENSE-2.0.txt.
    policy_url: "https://www.apache.org/licenses/LICENSE-2.0"

  # Apache-2.0 §4(d) requires a readable NOTICE file in any
  # distribution that included one upstream. Most projects
  # ship one even if their direct dependencies don't require
  # it — it's the canonical place for required attributions.
  - id: apache-2-notice-file-exists
    kind: file_exists
    paths: ["NOTICE", "NOTICE.md", "NOTICE.txt"]
    root_only: true
    level: warning
    message: >-
      Apache-2.0 §4(d): distributions that include a NOTICE
      from upstream must propagate it. Even if your direct
      dependencies don't ship one, having a project-level
      NOTICE for your own attributions is the canonical
      Apache pattern.
    policy_url: "https://www.apache.org/licenses/LICENSE-2.0#redistribution"

  # Every source file should carry the Apache 2.0 header in
  # its first ~25 lines. Pattern matches BOTH canonical forms
  # users paste from https://www.apache.org/licenses/LICENSE-2.0#apply:
  #
  #   1. Short SPDX-template form, opening with "Licensed under
  #      the Apache License, Version 2.0".
  #   2. Long ASF-preamble form, opening with "Licensed to the
  #      Apache Software Foundation (ASF) under one or more
  #      contributor license agreements...". This is the form
  #      every Apache TLP (arrow, spark, airflow, etc.) uses.
  #
  # v0.9.18: pattern broadened from the short-form-only
  # `Licensed under the Apache License,?\s*Version 2` (which
  # produced 8,228 false positives against airflow's tree, the
  # densest Apache TLP) to the alternation form below. This
  # supersedes the per-repo overrides arrow + spark previously
  # carried.
  - id: apache-2-source-has-license-header
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
    # v0.12: accept the ASF short form, the long ASF-preamble form, OR
    # the modern SPDX identifier (`SPDX-License-Identifier: Apache-2.0`)
    # that CNCF / branded-header projects (helm, istio, kubernetes) use
    # instead of the ASF appendix text. Broadening the accept-pattern is
    # a pure false-positive reduction, so it rides @v1.
    pattern: '(Licensed (to the Apache Software Foundation|under the Apache License,?\s*Version 2)|SPDX-License-Identifier:\s*Apache-2\.0)'
    level: warning
    message: >-
      Apache-2.0: source files should carry the canonical
      Apache header. Use either the short form ("Licensed under
      the Apache License, Version 2.0...") or the long
      ASF-preamble form ("Licensed to the Apache Software
      Foundation (ASF) under one or more contributor license
      agreements..."). The full boilerplate is at
      https://www.apache.org/licenses/LICENSE-2.0#apply.
    policy_url: "https://www.apache.org/licenses/LICENSE-2.0#apply"
```
