# v0.9.3 — dispatch flip + 8-rule migration (captured 2026-04-30)

Frozen `target/criterion`-format snapshot of every bench
v0.9.3's per-file dispatch flip + line-scanning + bounded-
read conversions touch. Captured on the v0.9.3-dispatch-flip
branch immediately before merge.

## Headline: 80-85% speedups on the migrated single-file rules

The byte-slice scanning + bounded-read conversions
deferred from v0.9.2 land here, plus the engine
restructure that lets multiple rules sharing one file
collapse N reads into 1. Result: every migrated rule's
single-file bench drops by ~80% at 1k files, ~80% at 100
files.

## v0.9.3 deltas

### vs v0.7.0 baseline (the gate)

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.8/baseline-v0.7.0/criterion \
  --after  docs/benchmarks/v0.9/v0.9.3-dispatch-flip/criterion \
  --threshold 10
```

| bench | before | after | delta |
|---|---:|---:|---:|
| `glob_compile/10` | 974.06 µs | 919.09 µs | -5.64% |
| `regex_content/1024` |    78 ns |    74 ns | -5.26% |
| `glob_match/10000` |   1.06 ms |   1.01 ms | -5.03% |
| `regex_content/65536` |    77 ns |    74 ns | -3.98% |
| `glob_match/100000` |  10.94 ms |  10.63 ms | -2.80% |
| `rule_engine/100000` |  15.62 ms |  15.20 ms | **-2.66%** |
| `rule_engine/1000` | 155.86 µs | 152.26 µs | -2.31% |
| `glob_match/1000` | 102.56 µs | 100.63 µs | -1.88% |
| `regex_content/1048576` |    78 ns |    77 ns | -1.45% |
| `rule_engine/10000` |   1.65 ms |   1.66 ms | +0.43% |
| `glob_compile/1000` | 114.98 ms | 114.74 ms | -0.21% |
| `glob_compile/100` |  10.34 ms |  10.33 ms | -0.11% |

All within ±5.64%. Gate passes. The `rule_engine/*`
improvements compound the v0.9.2 Arc<Path> savings (paths /
rule_ids amortised) with v0.9.3's read coalescing.

### vs pre-v0.9.3 baseline (the per-phase delta)

```sh
xtask bench-compare \
  --before docs/benchmarks/v0.9/baseline-v0.9.2/criterion \
  --after  docs/benchmarks/v0.9/v0.9.3-dispatch-flip/criterion \
  --threshold 10
```

The headline: every migrated single-file rule shows
**80%+** improvement at both 100- and 1000-file tree sizes.

| bench | before | after | delta |
|---|---:|---:|---:|
| `single_file_file_starts_with/1000` |   5.37 ms | 780.63 µs | **-85.46%** ✅ |
| `single_file_no_trailing_whitespace/1000` |   5.31 ms | 777.59 µs | **-85.37%** ✅ |
| `single_file_final_newline/1000` |   5.18 ms | 779.26 µs | **-84.95%** ✅ |
| `single_file_no_trailing_whitespace/100` | 528.84 µs | 100.93 µs | **-80.91%** ✅ |
| `single_file_file_starts_with/100` | 517.24 µs | 101.01 µs | **-80.47%** ✅ |
| `single_file_final_newline/100` | 514.16 µs | 101.33 µs | **-80.29%** ✅ |

Where the wins come from, per rule:

- **`file_starts_with`**: rule-major path now reads only
  `prefix.len()` bytes (not the whole file) via the new
  `read_prefix_n` helper. For most realistic prefixes
  (shebangs, magic numbers, SPDX headers) that's < 200
  bytes vs whole-file reads. Files of any size collapse to
  near-constant scan time.
- **`final_newline`**: rule-major and `evaluate_file` both
  short-circuit on the last byte; declares
  `max_bytes_needed: Some(1)`. The dispatch-flip path
  receives the full file from the engine but `last()` is
  O(1) regardless of size.
- **`no_trailing_whitespace`**: byte-slice scanning via
  `bytes.split(|&b| b == b'\n')` skips the redundant UTF-8
  validation pass. The `last()` byte check on each line
  short-circuits to first-offender.

Bench scenarios pack multiple files but a single rule per
bench, so the read-coalescing-across-rules path doesn't
materialise here — the wins are pure rule-level
optimisation. A dogfood scenario on alint's own
`.alint.yml` (which uses 4 line-oriented rules sharing
`**/*.md` scope) sees both the per-rule speedup AND the
read coalescing win on top.

### Cross-cutting noise

- `glob_*` / `regex_content` / `walker` deltas (±5%) are
  same-day measurement variance; v0.9.3 doesn't touch
  these paths.
- `output_formats_*` deltas (±7-12%) are nanosecond/µs-scale
  noise; v0.9.3 doesn't touch the formatter layer.
- `cross_file_*` deltas (±5%) are noise; cross-file rules
  deliberately stay on the rule-major path (they opt out
  of `as_per_file`).
- `rule_engine/*` improvements (-2.66% at 100k) are real;
  the engine restructure shaves overhead at scale.

Nothing past the ±10% threshold (in either direction);
gate passes.

## Methodology

Captured on the same self-hosted Linux runner (Linux 6.1,
x86_64, 8-core) the main CI lane runs on. Ran via:

```sh
cargo bench -p alint-bench --features fs-benches \
  --bench walker --bench rule_engine \
  --bench glob_compile --bench glob_match --bench regex_content \
  --bench single_file_rules --bench cross_file_rules --bench output_formats
cp -r target/criterion docs/benchmarks/v0.9/v0.9.3-dispatch-flip/criterion
```

Stale benches not exercised by v0.9.3 (`blame_cache_warm_lookup`,
`structured_query_*`) trimmed from the snapshot to keep the
diff surface focused on what actually got measured.

## What's next

v0.9.4 migrates the remaining ~22 per-file content rules
(`file_content_matches`, `file_content_forbidden`,
`file_header`, `file_footer`, `file_max/min_lines/size`,
`file_hash`, `file_is_ascii`, `file_is_text`, `file_shebang`,
`json_path_*` / `yaml_path_*` / `toml_path_*`,
`json_schema_passes`, `no_bom`, `no_bidi_controls`,
`no_zero_width_chars`, `no_merge_conflict_markers`,
`markdown_paths_resolve`, `commented_out_code`) to
`PerFileRule` so they pick up the same read-coalescing
behaviour when multiple rules share a scope. Each is a
mechanical body migration; the engine + trait shipped
in v0.9.3.
