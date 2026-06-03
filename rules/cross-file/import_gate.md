---
title: 'import_gate'
description: 'Forbid imports whose **extracted target** matches a forbid regex, within the paths scope, an architectural import firewall (staging-layer.'
sidebar:
  order: 8
---

Forbid imports whose **extracted target** matches a `forbid` regex, within the `paths` scope — an architectural import firewall (staging-layer isolation, core/providers separation, private-API gates). Matches the import target, not the raw line (so a comment or string mentioning the path doesn't fire — the low-false-positive specialisation of `file_content_forbidden`). `language` (`go`/`python`/`rust`/`js`) supplies a built-in import-line pattern; `import_pattern` overrides it (capture group 1 = target; required for `generic`). `allow` globs exempt sanctioned files. One violation per offending import.

```yaml
- id: staging-no-main-module
  kind: import_gate
  paths: "staging/src/k8s.io/**/*.go"
  language: go
  forbid: "^k8s\\.io/kubernetes/"
  allow: ["staging/src/k8s.io/legacy/**"]
  level: error
```

