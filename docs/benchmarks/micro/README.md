# Micro-benchmarks

[criterion](https://docs.rs/criterion)-driven micro-benchmarks for
alint's pure-CPU primitives. 12 bench files under
`crates/alint-bench/benches/`, each isolating one shape of the engine /
rule layer / output pipeline so a regression in any one of them is
visible without re-running the full e2e wall-time matrix.

## How to run

```sh
cargo bench -p alint-bench --features fs-benches            # all 12
cargo bench -p alint-bench --features fs-benches --bench rule_engine
```

See [`../RUNNING.md`](../RUNNING.md) for full options and how to publish.

## What each file measures

### `walker.rs`
The `ignore`-crate parallel walker over synthetic trees of 100 / 1k /
10k files. Captures walker overhead independent of any rule logic; the
v0.9.1 parallel-walker switch's headline came from this bench.

### `glob_compile.rs`
`globset::Glob::new` cost across 10 / 100 / 1000 patterns. Catches any
regression in the pattern → matcher pipeline (rule build time grows
linearly in this).

### `glob_match.rs`
`GlobSet::is_match` cost over 1000 / 10000 / 100000 path tests. Hot
path for every rule's scope filter; the v0.9.5 path-index work
deliberately doesn't change this — it short-circuits *around* it.

### `regex_content.rs`
Regex content-scan throughput at 1024 / 65536 / 1048576-byte buffer
sizes. The v0.7.0 floor's `regex_content` cells gate every later
release — content-rule perf rests on this.

### `rule_engine.rs`
Full `Engine::run` over an in-memory `FileIndex` of 1000 / 10000 /
100000 entries with a fixed 7-rule mix. Captures engine overhead
(glob matching, rayon fanout, result aggregation) without filesystem
I/O. The v0.9.5 path-index fix shows up at the 100k size as a
mostly-flat delta — the engine is no longer the bottleneck.

### `single_file_rules.rs`
Per-file rule throughput (filesystem-touching) for 8 representative
rules: `file_content_matches`, `file_content_forbidden`, `file_header`,
`file_starts_with`, `file_hash`, `file_is_text`, `no_trailing_whitespace`,
`final_newline`. Each at 100 / 1000 file sizes. Headline source for
the v0.9.3 dispatch flip's 80-85 % single-rule wins.

### `cross_file_rules.rs`
Cross-file rule throughput for `pair`, `unique_by`,
`every_matching_has`, `for_each_dir + file_exists`. Captures the fan-out
shapes the v0.9.5 path-index work made cheap.

### `output_formats.rs`
Formatter throughput across 100 / 1000 / 10000 violations for all 8
output formats (json / agent / markdown / human / gitlab / junit /
sarif / github). Catches regressions in the formatter pipeline —
notably any change to `Cow<'static, str>` / `Arc<str>` shapes from
v0.9.2.

### `fix_throughput.rs`
End-to-end throughput of the four content-editing fix ops
(`final_newline`, `no_trailing_whitespace`, `line_endings`, `no_bom`)
in `--apply` mode. Different shape from rule evaluation — measures
read + transform + atomic-rename per file.

### `dsl_extends.rs`
`alint-dsl` resolution cost for configs that `extends:` 1 / 5 / 10
bundled rulesets. The S3 publication scenario extends 4; this bench
isolates the resolution overhead from the rule-evaluation cost.

### `structured_query.rs`
`json_path_*`, `yaml_path_matches`, `toml_path_*`, `json_schema_passes`
throughput. The 6-kind `structured_path` family + JSON-Schema. The
v0.9.5 literal-path fast path on `structured_path` is visible at the
larger sizes here.

### `blame_cache.rs`
`git_blame_age` rule's `BlameCache` build + lookup throughput. Captures
the cost of populating + reading the blame cache; only meaningful
inside a real git repo. Gated behind the `fs-benches` feature.

## Where results live

```
results/
└── linux-x86_64/
    ├── v0.7.0/criterion/      ← bench-compare floor (every later release gates against this)
    ├── v0.8.4/criterion/      ← v0.8 publication-grade snapshot
    ├── v0.9.4/criterion/      ← latest v0.9.x intermediate
    └── v0.9.5/criterion/      ← latest published
```

Per-version subdirs follow criterion's native format
(`<bench-group>/<id>/new/{estimates.json,sample.json,…}`). The bench-
compare tool consumes this format directly.

Development-cycle snapshots (one per phase of the v0.9 cut) live under
[`../archive/v0.9-development-{baselines,phases}/`](../archive/) —
useful for cross-phase diffs but NOT the published numbers.

## When this catches what

Two-axis catch matrix:

| Symptom in micro-bench | Likely v0.9.x cause |
|---|---|
| `walker/*` regression at 10k+ | walker code change (rare; v0.9.1 was the only intentional move here) |
| `glob_match/*` regression | scope.matches inner loop (allocator change, glob regex change) |
| `rule_engine/100000` regression | Engine::run hot path (filter_map, par_iter shape, aggregation) |
| `single_file_*` regression at 1000 | per-file dispatch loop (read coalescing, max_bytes_needed handling) |
| `cross_file_*` regression | for_each_dir / pair / unique_by inner loops; check the path-index fast paths |
| `output_formats_*` regression | formatter struct shape (Arc/Cow leakage, allocator behavior) |
| `structured_query/*` regression | structured_path fast path or jsonpath/regex cost |

## Adding a new micro-bench

1. New `crates/alint-bench/benches/<name>.rs` following the existing
   shape (criterion `criterion_group!` macro, sized parameters).
2. Add to the list above with a one-paragraph "what it measures."
3. First publication-grade run sets the bench's own baseline (since
   it didn't exist at v0.7.0). Per the methodology doc, post-v0.7.0
   benches compare against their own first-run snapshot rather than
   the v0.7.0 floor — append the new bench's name to the list at the
   top of [`../METHODOLOGY.md`](../METHODOLOGY.md) so the gate
   convention is documented.
