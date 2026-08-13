---
title: 'pair_hash'
description: 'The algorithm digest (sha256 default / sha512) of every file matching source must appear in the single target file, either as an embedded.'
sidebar:
  order: 2
categories: ['cross-file', 'security-unicode-sanity']
---

The `algorithm` digest (`sha256` default / `sha512`) of every file matching `source` must appear in the single `target` file — either as an embedded hex substring (`format: contains`, default) or a `<hex>  <path>` manifest line (`format: sums-line`, where the path token must be the source's path; a leading `*` binary marker and a `./` prefix are tolerated). The sums-line parser accepts **either order** — coreutils / go-`.sum` `<hex> <path>` *and* the Go FIPS snapshot's path-first `<path> <hex>` — by identifying the digest token by its shape (the algorithm fixes its hex length). One violation per source whose digest is absent or mismatched; a missing `target` is one violation anchored on `target`. Raw bytes are hashed (a CRLF/newline change *is* a digest change — it is an integrity pin). Detection-only: alint never regenerates the manifest (same posture as `file_hash`). The sibling of `file_hash` (one file vs a *literal* hash in the config) and `generated_file_fresh` (a *generator's* stdout); `pair_hash` is the cross-file "B carries A's current digest" relation. golang/go FIPS `fips140.sum` is the canonical, highest-stakes use.

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `algorithm` | one of `sha256` \| `sha512` |  | `sha256` | Digest algorithm (default: sha256). |
| `format` | one of `contains` \| `sums-line` |  | `contains` | How the digest must appear in `target`: `contains` = hex substring anywhere (default); `sums-line` = a `<hex> [*]<path>` line whose path token is the source's path. |
| `source` | string | yes |  | Literal path or glob selecting the file(s) whose content is hashed (one check per match). |
| `target` | string | yes |  | The single file that must carry the digest (a `.sum` / `SHA256SUMS` / a file with an embedded hash). |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.

## Example

### A source file the checksum manifest omits

The rule fires on this repository:

```text
SHA256SUMS
hello.txt
```

`SHA256SUMS`:

```text
0000000000000000000000000000000000000000000000000000000000000000  other.txt
```

`hello.txt`:

```text
hello
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: sums-pins-sources
    kind: pair_hash
    source: "hello.txt"
    target: "SHA256SUMS"
    algorithm: sha256
    format: sums-line
    level: error
```

`alint check` reports:

```ansi
[2m--- hello.txt ------------------------------------------------------------------[0m
  [1m[31mx  error  [0m  [2msums-pins-sources[0m
              hello.txt is not listed in manifest SHA256SUMS

[2mSummary (1 violation):[0m
  [1m[31mx 1 error[0m
  0 passing [2m*[0m 1 failing
```

### Every source digest matches its checksum manifest line

This repository is compliant:

```text
SHA256SUMS
hello.txt
```

`SHA256SUMS`:

```text
5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  hello.txt
```

`hello.txt`:

```text
hello
```

With this `.alint.yml`:

```yaml
version: 1
rules:
  - id: sums-pins-sources
    kind: pair_hash
    source: "hello.txt"
    target: "SHA256SUMS"
    algorithm: sha256
    format: sums-line
    level: error
```

`alint check` reports:

```ansi
[1m[32mv All 1 rule(s) passed.[0m
```

