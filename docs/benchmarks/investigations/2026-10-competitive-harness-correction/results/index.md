# alint bench-scale results

**Platform:** `linux/x86_64`<br>
**CPU:** `AMD Ryzen 9 3900X 12-Core Processor` (24 cores)<br>
**RAM:** 62 GB<br>
**FS:** `overlay`<br>
**rustc:** `rustc 1.88.0 (6b00bc388 2025-06-23)`<br>
**alint:** `0.17.0` (fb4407a6)<br>
**hyperfine:** `1.20.0`<br>
**Tools:** alint=`0.17.0`, ls-lint=`ls-lint v2.2.3`, repolinter=`0.11.2; node v20.20.2`, shell=`find (GNU findutils) 4.9.0; grep (GNU grep) 3.8; ripgrep 15.1.0`<br>
**Seed:** `0xa11e47`<br>
**Warmup/runs:** 3 / 10<br>
**Generated:** `unix:1791312069`<br>

Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md`. Compare numbers like-for-like (same fingerprint), never absolutely.

Per-size detail under `<size>/results.md`. JSON: `results.json`.

## Scenarios

- **S1** — Layout & path — filename class / existence / absence / metadata / scope_filter shape (walk-bound)
- **S2** — Per-file content — 13 content kinds + forbidden patterns + ordered_block / import_gate / xml over **/*.rs

## Summary (mean ± stddev, ms)

| Tool | Size | Scenario | Mode | Mean | Stddev | Min | Max | Samples |
|---|---|---|---|---:|---:|---:|---:|---:|
| alint | 1k | S1 | full | 10.8 | 0.6 | 10.3 | 11.8 | 10 |
| ls-lint | 1k | S1 | full | 35.0 | 1.4 | 33.2 | 37.2 | 10 |
| shell | 1k | S1 | full | 21.8 | 0.4 | 21.2 | 22.3 | 10 |
| alint | 1k | S2 | full | 18.0 | 0.8 | 16.8 | 18.9 | 10 |
| shell | 1k | S2 | full | 26.8 | 0.8 | 25.8 | 28.4 | 10 |
| repolinter | 1k | S2 | full | 394.2 | 13.2 | 372.6 | 419.9 | 10 |
| alint | 10k | S1 | full | 48.2 | 1.0 | 46.9 | 49.6 | 10 |
| ls-lint | 10k | S1 | full | 67.7 | 2.2 | 65.4 | 73.2 | 10 |
| shell | 10k | S1 | full | 74.7 | 0.9 | 73.3 | 76.6 | 10 |
| alint | 10k | S2 | full | 119.7 | 2.5 | 115.7 | 122.9 | 10 |
| shell | 10k | S2 | full | 72.6 | 0.9 | 71.4 | 74.3 | 10 |
| repolinter | 10k | S2 | full | 1394.3 | 18.2 | 1358.5 | 1415.4 | 10 |
| alint | 100k | S1 | full | 425.4 | 6.2 | 417.3 | 436.9 | 10 |
| ls-lint | 100k | S1 | full | 371.0 | 26.8 | 332.7 | 417.8 | 10 |
| shell | 100k | S1 | full | 463.0 | 4.2 | 455.9 | 469.8 | 10 |
| alint | 100k | S2 | full | 1198.1 | 21.5 | 1174.3 | 1244.9 | 10 |
| shell | 100k | S2 | full | 480.5 | 4.4 | 472.6 | 487.4 | 10 |
| repolinter | 100k | S2 | full | 13374.7 | 1136.9 | 12743.7 | 16141.1 | 10 |
