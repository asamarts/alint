# Smoke-test fixtures (v0.9.15 Phase 7)

Each subdirectory is one regression-detection fixture for a rule kind
or canonical-correct rule pattern. The audit lives at
`crates/alint-e2e/tests/coverage_audit_smoke_fixtures.rs`.

## Why this exists

`crates/alint-e2e/tests/coverage_audit_examples_parse.rs` (Phase 2)
catches *schema* errors — every `examples/*/.alint.yml` must load +
build. But several pitfalls catalogued in
`docs/development/CONFIG-AUTHORING.md` produce runtime-semantic bugs
that the parse audit can't see:

- **#13** — regex `^`/`$` anchoring (without `(?m)`, the regex
  silently never matches multi-line input).
- **#14** — YAML strings don't expand `\n` to a literal newline
  inside regex patterns (the regex compiles to a literal `\n`
  two-char match that never appears in real files).
- **#16** — `*_path_matches` against a bool field emits a runtime
  "value at path is not a string" violation on every match.
- **#17** — `*_path_equals` against a `[*]` JSONPath fires "wrong"
  on every non-matching array element.

Smoke fixtures are the regression backstop: each fixture exercises a
canonical-correct pattern against a small file tree and asserts an
exact violation count. A future refactor that re-introduces any of
those pitfalls would change the count and fail the audit.

## Fixture layout

Each `<scenario>/` directory contains:

```
<scenario>/
├── alint.yml         ← the config under test (canonical-correct form)
├── tree/             ← input file tree to lint
│   └── …
└── expected.toml     ← expected violation counts
```

`expected.toml` shape:

```toml
# Total violations across all rules in the config.
total = 2

# Per-rule-id violation counts. Sum of values must equal `total`.
[per_rule]
some-rule-id = 1
other-rule-id = 1
```

## Adding a new fixture

1. Create `<scenario>/` with the three files.
2. The `alint.yml` should use the canonical-correct form (per
   `docs/development/CONFIG-AUTHORING.md`) — fixtures pin the
   canonical answer, not the pitfall.
3. The `tree/` should contain at minimum one file that should produce
   a violation AND one that should not, so the audit can detect both
   false-positive and false-negative regressions.
4. Run `cargo test -p alint-e2e --test coverage_audit_smoke_fixtures`
   locally to confirm the counts.
5. Reference the relevant pitfall number in `expected.toml`'s comment
   header so future readers know what regression the fixture guards.
