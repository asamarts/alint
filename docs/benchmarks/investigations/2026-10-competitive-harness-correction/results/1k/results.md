# alint bench-scale — 1k files

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
| alint | S1 | full | 10.8 | 0.6 | 10.3 | 11.8 | 10 |
| ls-lint | S1 | full | 35.0 | 1.4 | 33.2 | 37.2 | 10 |
| shell | S1 | full | 21.8 | 0.4 | 21.2 | 22.3 | 10 |
| alint | S2 | full | 18.0 | 0.8 | 16.8 | 18.9 | 10 |
| shell | S2 | full | 26.8 | 0.8 | 25.8 | 28.4 | 10 |
| repolinter | S2 | full | 394.2 | 13.2 | 372.6 | 419.9 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
