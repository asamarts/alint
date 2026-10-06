# alint bench-scale — 10k files

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

## Rows

| Tool | Scenario | Mode | Mean (ms) | Stddev | Min | Max | Samples |
|---|---|---|---:|---:|---:|---:|---:|
| alint | S1 | full | 50.7 | 2.2 | 48.7 | 56.5 | 10 |
| ls-lint | S1 | full | 63.7 | 1.3 | 61.4 | 66.3 | 10 |
| shell | S1 | full | 76.7 | 3.4 | 72.6 | 82.8 | 10 |
| alint | S2 | full | 121.5 | 2.8 | 117.0 | 126.2 | 10 |
| shell | S2 | full | 75.6 | 1.6 | 74.1 | 78.6 | 10 |
| repolinter | S2 | full | 1403.2 | 38.2 | 1344.2 | 1455.8 | 10 |

Tree shape: monorepo (`packages=200, files_per_package=48, total=10000`).
