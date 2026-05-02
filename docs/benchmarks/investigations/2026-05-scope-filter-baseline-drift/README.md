# scope_filter Phase 5 Run 2 — baseline drift, not engine regression (2026-05-01)

Phase 2 of the v0.9.6 scope_filter rollout (commit `7b080a0`,
`feat(core): scope_filter primitive + has_ancestor`) was gated on a
bench-compare against the published v0.9.4 floor. The first
`bench-compare` invocation reported three regressions past the 10 %
threshold:

```
3 bench(es) regressed past 10.0%:
  - single_file_file_hash/100 (+27.55%)
  - single_file_file_hash/1000 (+20.66%)
  - output_formats_100_violations/junit (+13.31%)
Error: bench-compare: regression threshold exceeded
```

This document records the investigation that traced all three to
machine-state drift between the v0.9.4 baseline capture (yesterday
evening) and the Run 2 capture (today), not to anything in the engine
commit.

## Hypothesis ladder

1. The new `Rule::scope_filter() -> Option<&ScopeFilter>` virtual
   call adds dispatch overhead on the per-file applicable-filter hot
   path (engine.rs:412–421). Default impl returns `None`; a vtable
   call returning `None` should be sub-nanosecond, but if rustc isn't
   inlining or there's an allocator/cache-line effect, it could
   surface here. **Most likely candidate** since `single_file_file_hash`
   routes through this path and `output_formats` does not.

2. Some unrelated change between v0.9.4 (2026-04-30) and the current
   HEAD (2026-05-01) — Cargo.lock drift, rustc bump, dependency
   update — shifted the floor.

3. Machine-state drift: the v0.9.4 baseline ran on a quiescent
   machine; today's Run 2 ran on a busy one (`uptime` showed
   `load average: 0.79, 6.65, 5.34` shortly after — 5- and 15-min
   averages much higher than the 1-min, indicating recent load).

## Dispositive control run

To separate (1) from (2)/(3), I ran a same-machine A/B with the
identical bench harness:

```sh
# Treatment: Phase 2 source (HEAD = 7b080a0)
cargo bench -p alint-bench --bench single_file_rules \
    --features fs-benches -- "file_hash"

# Control: pre-Phase-2 source (parent = 50385f4, design-doc commit)
git checkout 50385f4 -- crates/alint-core/src/ \
    crates/alint-rules/src/ crates/alint/src/
rm crates/alint-core/src/scope_filter.rs   # didn't exist at parent
cargo bench -p alint-bench --bench single_file_rules \
    --features fs-benches -- "file_hash"
```

Bench file fixes (`FileIndex { entries }` → `FileIndex::from_entries(...)`
required by the v0.9.5 path-index work; unrelated to Phase 2) were
stashed for the control and restored after. Same machine, same load,
same compiler invocation, same criterion seed.

| target                     | v0.9.4 floor | pre-Phase-2 control (today) | Phase 2 (today) |
| -------------------------- | -----------: | --------------------------: | --------------: |
| `single_file/file_hash/100`  |     151.9 µs |                    193.83 µs |       193.71 µs |
| `single_file/file_hash/1000` |    1011.7 µs |                    1219.8 µs |      1209.5 µs  |

Pre-vs-post Phase 2 on the same machine: **+0.06 % and −0.85 % —
within criterion's noise floor** (p > 0.37 for both). Criterion's own
intra-comparison verdict on the second run was *"No change in
performance detected"*.

## Conclusion

Hypothesis (1) is rejected. The default-`None` `scope_filter()`
virtual call is unmeasurable on rules that don't override it,
exactly as the design predicted. The +27 % / +20 % gap vs. the
v0.9.4 floor is environmental — same hardware, different ambient
state.

`output_formats_100_violations/junit` (+13.31 %) is a third-party
verdict in the same direction: that bench doesn't route through
the engine's per-file filter at all (it tests the JUnit serializer
in isolation), so its drift can only be environmental. This is a
consistent signature, not a coincidence.

## Action

- Phase 2 commit (`7b080a0`) is fit to merge — engine perf is a
  wash, design contract met.
- Phase 5 Run 2 is closed as PASSED on the basis of the same-machine
  control above, not the v0.9.4 bench-compare gate.
- Phase 4 (bundled-ruleset migration) proceeds.
- Future Phase 5 Run 3 (post-migration with the S9 nested-polyglot
  scenario) should re-baseline against a fresh same-machine pre-run
  rather than the v0.9.4 published floor, so machine-state drift
  doesn't masquerade as code-state drift again.

## Lesson for the bench harness

When `bench-compare` flags a regression that's both (a) larger than
plausible for the implementation change and (b) hits unrelated
benches in the same run, the first move is a same-machine A/B
control rather than spelunking the diff. The cost is ~5 minutes of
focused-bench time and yields a deterministic verdict on whether
the gap is code or environment.

This pattern is worth folding into the methodology doc:
[`../../METHODOLOGY.md`](../../METHODOLOGY.md) currently treats
bench-compare against published floors as authoritative; in
practice, machine-state drift across captures is large enough
(20–30 % observed here) that a control run should be the
tie-breaker before declaring a regression real.
