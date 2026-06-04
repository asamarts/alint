---
title: 'generated_file_fresh'
description: 'A committed file must equal the stdout of a declared command generator, a non-mutating freshness check. alint generated_file_fresh rule, cross-file family.'
sidebar:
  order: 6
---

A committed `file` must equal the stdout of a declared `command` generator — a non-mutating freshness check. **alint does not run codegen as a build step**; this only *verifies* that the committed artefact matches what the user-declared, maintainer-trusted generator produces (stdout captured; the tree is never written — same trust tier as the `command` rule). Single-shot, opt-in. Spawn-failure / non-zero exit / a missing committed file are each a clear, distinct violation. `normalize` (`none` / `trim` / `final-newline`) absorbs trailing-newline churn.

```yaml
- id: bindings-fresh
  kind: generated_file_fresh
  file: crates/ffi/include/core.h
  command: ["cbindgen", "--config", "cbindgen.toml", "crates/core"]
  workdir: "."
  normalize: final-newline
  level: error
```

