---
title: 'pair_hash'
description: 'The algorithm digest (sha256 default / sha512) of every file matching source must appear in the single target file, either as an embedded.'
sidebar:
  order: 2
categories: ['cross-file']
---

The `algorithm` digest (`sha256` default / `sha512`) of every file matching `source` must appear in the single `target` file — either as an embedded hex substring (`format: contains`, default) or a `<hex>  <path>` manifest line (`format: sums-line`, where the path token must be the source's path; a leading `*` binary marker and a `./` prefix are tolerated). The sums-line parser accepts **either order** — coreutils / go-`.sum` `<hex> <path>` *and* the Go FIPS snapshot's path-first `<path> <hex>` — by identifying the digest token by its shape (the algorithm fixes its hex length). One violation per source whose digest is absent or mismatched; a missing `target` is one violation anchored on `target`. Raw bytes are hashed (a CRLF/newline change *is* a digest change — it is an integrity pin). Detection-only: alint never regenerates the manifest (same posture as `file_hash`). The sibling of `file_hash` (one file vs a *literal* hash in the config) and `generated_file_fresh` (a *generator's* stdout); `pair_hash` is the cross-file "B carries A's current digest" relation. golang/go FIPS `fips140.sum` is the canonical, highest-stakes use.

```yaml
- id: fips-sum-pins-module
  kind: pair_hash
  source: "src/crypto/internal/fips140/v1.0.0/**/*.go"
  target: "src/crypto/internal/fips140/fips140.sum"
  algorithm: sha256
  format: sums-line
  level: error
```

## Options

| Option | Type | Required | Default | Description |
|---|---|---|---|---|
| `algorithm` | one of `sha256` \| `sha512` |  | `sha256` | Digest algorithm (default: sha256). |
| `format` | one of `contains` \| `sums-line` |  | `contains` | How the digest must appear in `target`: `contains` = hex substring anywhere (default); `sums-line` = a `<hex> [*]<path>` line whose path token is the source's path. |
| `source` | string | yes |  | Literal path or glob selecting the file(s) whose content is hashed (one check per match). |
| `target` | string | yes |  | The single file that must carry the digest (a `.sum` / `SHA256SUMS` / a file with an embedded hash). |

Plus the common `level`, `id`, and `when` fields. This rule analyses the whole repository, so it takes no `paths`. This table is generated from the JSON Schema; option types and defaults are authoritative.
