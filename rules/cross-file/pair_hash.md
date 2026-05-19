---
title: 'pair_hash'
description: 'The algorithm digest (sha256 default / sha512) of every file matching source must appear in the single in target, either as an embedded hex.'
sidebar:
  order: 2
---

The `algorithm` digest (`sha256` default / `sha512`) of every file matching `source` must appear in the single `in` target — either as an embedded hex substring (`format: contains`, default) or a coreutils / go-`.sum`-style `<hex>  <path>` manifest line (`format: sums-line`, where the path token must be the source's path; a leading `*` binary marker is tolerated). One violation per source whose digest is absent or mismatched; a missing `in` is one violation anchored on `in`. Raw bytes are hashed (a CRLF/newline change *is* a digest change — it is an integrity pin). Detection-only: alint never regenerates the manifest (same posture as `file_hash`). The sibling of `file_hash` (one file vs a *literal* hash in the config) and `generated_file_fresh` (a *generator's* stdout); `pair_hash` is the cross-file "B carries A's current digest" relation. golang/go FIPS `fips140.sum` is the canonical, highest-stakes use.

```yaml
- id: fips-sum-pins-module
  kind: pair_hash
  source: "src/crypto/internal/fips140/v1.0.0/**/*.go"
  in: "src/crypto/internal/fips140/fips140.sum"
  algorithm: sha256
  format: sums-line
  level: error
```

