# alint bench-scale — 1k files

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
| alint | S1 | full | 11.5 | 0.8 | 10.3 | 12.8 | 10 |
| ls-lint | S1 | full | 33.0 | 1.6 | 31.0 | 36.6 | 10 |
| shell | S1 | full | 22.3 | 0.5 | 21.6 | 22.9 | 10 |
| alint | S2 | full | 19.6 | 1.0 | 18.2 | 21.6 | 10 |
| shell | S2 | full | 27.4 | 1.0 | 26.1 | 28.7 | 10 |
| repolinter | S2 | full | 383.3 | 9.2 | 366.2 | 396.3 | 10 |

Tree shape: monorepo (`packages=50, files_per_package=18, total=1000`).
