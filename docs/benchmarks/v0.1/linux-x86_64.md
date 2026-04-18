# alint bench-release results

**Mode:** full  
**Seed:** `0xa11e47`  
**OS:** `linux/x86_64`  
**rustc:** `rustc 1.93.1 (01f6ddf75 2026-02-11)`  
**alint git SHA:** `ccc18cd`  
**Generated:** unix:1776551355  

Results measured with `hyperfine` on this machine. Cross-machine variance is expected; see `docs/benchmarks/METHODOLOGY.md` for the reproduction recipe. Do not compare absolute numbers across rows in different files — compare like-for-like.

### 1000 files

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `alint check (synthetic, 1000 files)` | 20.7 ± 7.0 | 18.3 | 65.6 | 1.00 |


### 10000 files

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `alint check (synthetic, 10000 files)` | 112.6 ± 6.7 | 106.8 | 136.9 | 1.00 |


### 100000 files

| Command | Mean [ms] | Min [ms] | Max [ms] | Relative |
|:---|---:|---:|---:|---:|
| `alint check (synthetic, 100000 files)` | 868.3 ± 10.6 | 845.5 | 877.4 | 1.00 |

