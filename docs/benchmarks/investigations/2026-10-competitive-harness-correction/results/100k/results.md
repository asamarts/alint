# alint bench-scale — 100k files

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

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 425.4 | 6.2 | 417.3 | 436.9 | 10 |
| ls-lint | S1 | full | 371.0 | 26.8 | 332.7 | 417.8 | 10 |
| shell | S1 | full | 463.0 | 4.2 | 455.9 | 469.8 | 10 |
| alint | S2 | full | 1198.1 | 21.5 | 1174.3 | 1244.9 | 10 |
| shell | S2 | full | 480.5 | 4.4 | 472.6 | 487.4 | 10 |
| repolinter | S2 | full | 13374.7 | 1136.9 | 12743.7 | 16141.1 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
