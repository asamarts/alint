# alint bench-scale — 10k files

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
| alint | S1 | full | 48.2 | 1.0 | 46.9 | 49.6 | 10 |
| ls-lint | S1 | full | 67.7 | 2.2 | 65.4 | 73.2 | 10 |
| shell | S1 | full | 74.7 | 0.9 | 73.3 | 76.6 | 10 |
| alint | S2 | full | 119.7 | 2.5 | 115.7 | 122.9 | 10 |
| shell | S2 | full | 72.6 | 0.9 | 71.4 | 74.3 | 10 |
| repolinter | S2 | full | 1394.3 | 18.2 | 1358.5 | 1415.4 | 10 |

Tree shape: monorepo (`packages=200, files_per_package=48, total=10000`).
