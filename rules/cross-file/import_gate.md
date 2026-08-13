---
title: 'import_gate'
description: 'Forbid imports whose extracted target matches a forbid regex, within the paths scope, an architectural import firewall.'
sidebar:
  order: 9
categories: ['cross-file', 'security-unicode-sanity']
---

Forbid imports whose **extracted target** matches a `forbid` regex, within the `paths` scope — an architectural import firewall (staging-layer isolation, core/providers separation, private-API gates). Matches the import target, not the raw line (so a comment or string mentioning the path doesn't fire — the low-false-positive specialisation of `file_content_forbidden`). `language` (`go`/`python`/`rust`/`js`/`scala`/`java`/`dart`/`nix`) supplies a built-in import-line pattern; `import_pattern` overrides it (capture group 1 = target; required for `generic`). The `js` preset (whose pattern is unanchored, to catch dynamic `import("m")` / `require("m")`) additionally blanks `//` and `/* … */` comments before matching, so a JSDoc `@typedef {import("../x")}` type annotation isn't mistaken for a real import. `allow` globs exempt sanctioned files. One violation per offending import.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `allow` | list of string |  | `[]` | File globs inside the scope that are exempt from the gate. |
| `forbid` | string | yes |  | Regex tested against the extracted import target. |
| `import_pattern` | string |  | `null` | Explicit import-line regex (capture group 1 = target). Overrides the `language` preset. |
| `language` | one of `go` \| `python` \| `rust` \| `js` \| `scala` \| `java` \| `dart` \| `nix` \| `generic` |  |  | Built-in import-line pattern preset (capture group 1 = the imported target). Omit to require an explicit `import_pattern`. |

Plus the common `paths`, `level`, `id`, and `when` fields. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A staging file that imports a forbidden module

The rule fires on this repository:

```text
staging/src/k8s.io/api/types.go
```

`staging/src/k8s.io/api/types.go`:

```go
package api

import (
	"fmt"
	"k8s.io/kubernetes/pkg/apis/core"
)
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: staging-no-main-module
    kind: import_gate
    paths: "staging/src/k8s.io/**/*.go"
    language: go
    forbid: "^k8s\\.io/kubernetes/"
    level: error
```

`alint check` reports:

```ansi
[2m--- staging/src/k8s.io/api/types.go --------------------------------------------[0m
  [1m[31mx  error  [0m  [2mstaging-no-main-module[0m
              [2m5:1[0m  forbidden import "k8s.io/kubernetes/pkg/apis/core" at this scope
              (matches /^k8s\.io/kubernetes//)

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Staging files import only allowed modules

This repository is compliant:

```text
staging/src/k8s.io/api/types.go
```

`staging/src/k8s.io/api/types.go`:

```go
package api

import (
	"fmt"
	"k8s.io/apimachinery/pkg/runtime"
)
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: staging-no-main-module
    kind: import_gate
    paths: "staging/src/k8s.io/**/*.go"
    language: go
    forbid: "^k8s\\.io/kubernetes/"
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

