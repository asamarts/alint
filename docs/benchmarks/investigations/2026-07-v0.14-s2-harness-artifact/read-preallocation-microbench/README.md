# Read-preallocation microbench

Isolates the v0.14 content-read regression (parent investigation) down to a single
`std` behaviour and validates the fix, independent of alint's rule engine and of the
bench host. It is what turned "the deterministic gate says flat, so it's a harness
artifact" into a proven, mechanistic root cause.

## The mechanism in one paragraph

`std::fs::read` reads a `File` directly, and `Read for File` is *specialized* to
`fstat` the open fd, preallocate a right-sized `Vec`, and read the whole file in one
`read()` (plus one zero-length read to see EOF). v0.14's OOM cap (`c845f7d3`) wrapped
every content read in `File::open(p).take(cap+1).read_to_end(Vec::new())` to bound the
read (TOCTOU/OOM safety). But a **`Take<File>` does not carry `File`'s `read_to_end`
specialization** — it falls back to the generic grow-and-reread loop, issuing several
`read()` syscalls per file into a repeatedly-doubled buffer. Extra syscalls are real
kernel round-trips (context switch + copy) but only a handful of *guest* instructions
each, so they barely move Callgrind `Ir`/`EstimatedCycles` (why `det_check` looked
flat) while costing real wall-clock on read-heavy work (why S2 rose ~13%).

## Reproduce

```sh
# 1. compile the four-mode reader (see readrepro.rs for the paths)
rustc -O readrepro.rs -o readrepro

# 2. a tree of many small files (~680–800 B each, typical source-file size)
mkdir -p tree
python3 - <<'PY'
for i in range(8000):
    open(f"tree/f{i:05d}.txt","w").write(("line %d of file %d\n" % (0,i)) * 40)
PY

# 3. measure each mode, interleaved, report the MIN of 6 runs (robust, like the
#    gate's min_ms). Same box, back-to-back — so host/contamination cancels and the
#    only variable is the read path.
python3 - <<'PY'
import subprocess, time
res={}
for m in (["v13","v14","v14fix","v14fixfree"]*6):
    t0=time.perf_counter()
    subprocess.run(["./readrepro","tree",m], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    res.setdefault(m,[]).append((time.perf_counter()-t0)*1000)
v13=min(res["v13"])
for m in ("v13","v14","v14fix","v14fixfree"):
    xs=sorted(res[m]); print(f"{m:>11}: min={xs[0]:7.1f}ms  vs v13 {100*(xs[0]-v13)/v13:+5.1f}%")
PY
```

## Result (2026-07-22, i7-6700HQ dev box, 8000 files × 5 reads/run)

| read path | min | vs v13 |
|---|--:|--:|
| **v13** `std::fs::read` (File-specialized) | 189.6 ms | — |
| **v14** current: `take(cap+1).read_to_end(Vec::new())` | 276.9 ms | **+46.0 %** |
| v14fix: preallocate via a separate `metadata()` path stat | 227.0 ms | +19.7 % |
| **v14fixfree** the shipped fix: preallocate from the walk-time size | 172.9 ms | **−8.8 %** |

- **v14 +46 %** is the pure per-read cost. In alint's S2 (which also does existence
  checks + rule matching) it dilutes to the ~+13 % the corpus shows; in read-light
  scenarios (S1 filename, S7 cross-file) to ~0.
- **v14fixfree −8.8 %** — the shipped fix beats even v13, because alint already has
  the file size from the walk, so it skips the `fstat` that `std::fs::read` itself
  does. The `take(cap+1)` OOM/TOCTOU bound is retained.
- **v14fix +19.7 %** is *not* what alint ships — it re-`stat`s the path to get the
  size (two path lookups). It is included only to show that where the size comes from
  matters: a free (walk-time) size beats a paid (extra-stat) one.

## Notes

- Numbers are a dev box, not the canonical bench host — but the comparison
  is same-box, back-to-back and interleaved, so it measures the *relative* cost of the
  read path with host load and contamination cancelled. The absolute on-host recovery
  is the v0.14.1 runner bench (parent `../../../HISTORY.md`).
- `strace -c` would show the same story as a deterministic syscall count (v14 issues
  more `read()` per file than v13; v14fixfree matches v13 minus one `fstat`). `strace`
  was not installed on the box used; the wall-clock delta plus the `std` source is the
  evidence here.
