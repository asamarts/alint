# Isolated vs full-matrix, 1M cells (raw)

Same box (the bench host), same tag (v0.13.0), same flags (--warmup 3 --runs 10;
1M auto-reduces to 3). The only difference is whether the cell was measured
inside the full 112-cell matrix or in a short isolated invocation.

| cell | isolated mean | matrix mean | inflation |
|---|--:|--:|--:|
| S1/full | 2416 ms | 2496 ms | 1.03x |
| S1/changed | 4279 ms | 4346 ms | 1.02x |
| S2/full | 3667 ms | 8541 ms | 2.33x |
| S2/changed | 4317 ms | 4256 ms | 0.99x |
| S9/full | 11912 ms | 11936 ms | 1.00x |
| S9/changed | 4869 ms | 4777 ms | 0.98x |

## The failing cell, full row

```json
{"tool":"alint","size_files":1000000,"size_label":"1m","scenario":"S2","mode":"full","mean_ms":8541.06922186,"stddev_ms":4382.094969114454,"median_ms":8504.93656986,"min_ms":4177.15230486,"max_ms":12941.118790859999,"samples":3,"command":"alint (1m/S2/full)"}
```
