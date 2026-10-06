# alint bench-scale — 100k files

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
| alint | S1 | full | 453.7 | 17.5 | 438.3 | 501.3 | 10 |
| ls-lint | S1 | full | 331.5 | 6.1 | 322.4 | 342.2 | 10 |
| shell | S1 | full | 481.1 | 6.0 | 476.1 | 494.4 | 10 |
| alint | S2 | full | 1213.5 | 13.8 | 1187.1 | 1231.0 | 10 |
| shell | S2 | full | 486.4 | 13.2 | 475.8 | 518.0 | 10 |
| repolinter | S2 | full | 13627.2 | 963.0 | 12643.0 | 15841.1 | 10 |

Tree shape: monorepo (`packages=1000, files_per_package=98, total=100000`).
