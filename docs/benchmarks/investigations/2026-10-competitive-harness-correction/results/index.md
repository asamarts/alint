# alint bench-scale results

**Platform:** `linux/x86_64`<br>
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)<br>
**RAM:** 62 GB<br>
**FS:** `overlay`<br>
**rustc:** `rustc 1.88.0 (6b00bc388 2025-06-23)`<br>
**alint:** `0.17.0` (997e925c)<br>
**hyperfine:** `1.20.0`<br>
**Tools:** alint=`0.17.0`, ls-lint=`ls-lint v2.2.3`, repolinter=`0.11.2; node v20.20.2`, shell=`find (GNU findutils) 4.9.0; grep (GNU grep) 3.8; ripgrep 15.1.0`<br>
**Seed:** `0xa11e47`<br>
**Warmup/runs:** 3 / 10<br>
**Generated:** `unix:1791323640`<br>

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

Per-size detail under `<size>/results.md`. JSON: `results.json`.

## Scenarios

- **S1** — Layout & path — filename class / existence / absence / metadata / scope_filter shape (walk-bound)
- **S2** — Per-file content — 13 content kinds + forbidden patterns + ordered_block / import_gate / xml over **/*.rs

## Summary (mean ± stddev, ms)

| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---|---:|---:|---:|---:|---:|
| alint | 1k | S1 | full | 11.5 | 0.8 | 10.3 | 12.8 | 10 |
| ls-lint | 1k | S1 | full | 33.0 | 1.6 | 31.0 | 36.6 | 10 |
| shell | 1k | S1 | full | 22.3 | 0.5 | 21.6 | 22.9 | 10 |
| alint | 1k | S2 | full | 19.6 | 1.0 | 18.2 | 21.6 | 10 |
| shell | 1k | S2 | full | 27.4 | 1.0 | 26.1 | 28.7 | 10 |
| repolinter | 1k | S2 | full | 383.3 | 9.2 | 366.2 | 396.3 | 10 |
| alint | 10k | S1 | full | 50.7 | 2.2 | 48.7 | 56.5 | 10 |
| ls-lint | 10k | S1 | full | 63.7 | 1.3 | 61.4 | 66.3 | 10 |
| shell | 10k | S1 | full | 76.7 | 3.4 | 72.6 | 82.8 | 10 |
| alint | 10k | S2 | full | 121.5 | 2.8 | 117.0 | 126.2 | 10 |
| shell | 10k | S2 | full | 75.6 | 1.6 | 74.1 | 78.6 | 10 |
| repolinter | 10k | S2 | full | 1403.2 | 38.2 | 1344.2 | 1455.8 | 10 |
| alint | 100k | S1 | full | 453.7 | 17.5 | 438.3 | 501.3 | 10 |
| ls-lint | 100k | S1 | full | 331.5 | 6.1 | 322.4 | 342.2 | 10 |
| shell | 100k | S1 | full | 481.1 | 6.0 | 476.1 | 494.4 | 10 |
| alint | 100k | S2 | full | 1213.5 | 13.8 | 1187.1 | 1231.0 | 10 |
| shell | 100k | S2 | full | 486.4 | 13.2 | 475.8 | 518.0 | 10 |
| repolinter | 100k | S2 | full | 13627.2 | 963.0 | 12643.0 | 15841.1 | 10 |
