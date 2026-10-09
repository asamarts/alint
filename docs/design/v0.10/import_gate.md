# `import_gate` — forbid imports matching a pattern within a path scope

Status: **Implemented** — lands with the rule in v0.10 (this
commit; rule kind #5 of the case-study coverage push). Was a
design draft (2026-05-18). v0.10 demand #5 (5 sources,
ROADMAP-canonical). Open questions resolved on implementation:
line-based presets only, language-AST mode deferred (Q1);
`require:` inverse deferred (Q2); per-target allow via the
`forbid` regex's own negative-lookahead, structured list
deferred (Q3); common preset forms shipped + `import_pattern`
escape hatch (Q4); sibling of `file_content_forbidden`, docs
cross-link (Q5). Per-file
rule (the `PerFileRule` fast path).

Demand evidence:
[`docs/development/launch-evidence.md`](../../development/launch-evidence.md)
("forbid imports of pattern X in path scope Y", 4 sources: k8s,
airflow, golang/go, pytorch) and the per-repo tracker in
[`examples/README.md`](../../../examples/README.md#primitive-demand-tracker)
(`import_gate` row: airflow, golang-go, helm, kubernetes,
pytorch). Canonical scope:
[`../ROADMAP.md`](../ROADMAP.md#v010-case-study-coverage-push)
(#5; "k8s prometheus-imports + airflow + go + helm + pytorch").

## Problem

Large repos enforce *architectural layering* with import
firewalls: "files under `staging/src/k8s.io/**` must not import
the main `k8s.io/kubernetes/...` module"; "airflow core must not
`import airflow.providers.*`"; "nothing outside `torch/_C` may
import `torch._C`"; "the metrics layer must not pull
`github.com/prometheus/...` outside the allowed package". These
are dependency-edge rules, and they rot silently — a new import
slips in, the layer leaks, and it's only caught when a refactor
or a cycle bites.

`file_content_forbidden` can forbid a regex in files, but
matching the *raw line* over-fires (a comment or a string
literal mentioning the path) and has no notion of "these few
files are sanctioned exceptions". There is no precise
"imported-target X is forbidden in scope Y, except in these
files".

This is a *precise* rule (ROADMAP v0.10 cross-cutting decision):
a deterministic regex over the **extracted import target**, not
the raw line, with explicit allow exceptions. True
language-AST import parsing is the deliberate non-goal for v0.10
(Open question 1).

## Surface area

New per-file rule kind `import_gate` in `alint-rules`.
`version: 1` unchanged.

```yaml
- id: staging-no-main-module
  kind: import_gate
  paths: "staging/src/k8s.io/**/*.go"          # scope Y
  language: go                                 # go|python|rust|js|generic
  forbid: "^k8s\\.io/kubernetes/"              # regex tested on the EXTRACTED import target (X)
  allow: ["staging/src/k8s.io/legacy/**"]      # files in scope exempt from the gate
  level: error
  message: "{{ctx.path}} imports a forbidden module at this layer"
```

- `forbid` is a regex tested against the **import target**, not
  the line — `from a.b import c` / `import a.b` →`a.b`;
  Go `\t"k8s.io/kubernetes/pkg/x"` → `k8s.io/kubernetes/pkg/x`;
  Rust `use crate::secrets::Key;` → `crate::secrets::Key`; JS
  `import x from "pkg/y"` → `pkg/y`.
- `language` selects a built-in line-based `import_pattern` whose
  **capture group 1 is the imported target**. `generic` (or
  omitting `language`) requires an explicit `import_pattern:`
  regex (group 1 = target), which also overrides a preset for
  edge cases.
- `allow` is a list of file globs inside the scope that are
  exempt (the sanctioned exceptions every firewall has).

## Semantics

Per matching file (`PerFileRule` dispatch):

1. If the file matches any `allow` glob ⇒ silent (exempt).
2. Non-UTF-8 ⇒ skip.
3. For each line, apply `import_pattern`. On a match, take
   capture group 1 as the import target.
4. If `forbid` matches that target ⇒ one violation **per
   offending import**, anchored at that line, message naming the
   target (an import firewall wants every leak flagged, not just
   the first).
5. Lines that aren't imports (no `import_pattern` match) are
   ignored.

The language presets are documented line-based regexes (not a
grammar): Go covers `import "x"`, `import alias "x"`, and
grouped-block member lines (`\t"x"`, `\t_ "x"`, `\talias "x"`),
end-anchored to avoid matching mid-expression string literals;
Python covers `import a.b` and `from a.b import c` (→ `a.b`);
Rust covers `use a::b::c;`; JS covers `import … from "m"` and
`require("m")`. Users override with `import_pattern` when a
preset is too loose or too tight.

## False-positive surface

- **Match the target, not the line.** Comments / string literals
  mentioning the path don't fire — only an actual
  `import_pattern` match's group 1 is tested. The core
  differentiator vs `file_content_forbidden`.
- **`allow` exceptions.** Real firewalls have sanctioned
  exceptions; without an exemption list the rule gets disabled
  wholesale. `allow` globs scope the exemption to specific
  files.
- **Go grouped imports.** The preset matches grouped-block
  member lines end-anchored (`^\s*(?:_\s+|[\w.]+\s+)?"…"\s*$`),
  so a quoted string mid-statement won't match; a precision-
  critical gate can supply an explicit `import_pattern`.
- **Aliased / underscore imports.** Go `_ "x"` blank imports and
  `alias "x"` are covered (the target is still `x`).
- **Dynamic imports.** `importlib`, `require(variable)`,
  `mod := reflect…` — not literal, not matched; documented (a
  layering rule that needs these needs an AST tool, Open
  question 1).
- **No language AST.** v0.10 is line-based by design; multi-line
  / conditional imports beyond the preset are an explicit
  non-goal (Open question 1), mitigated by `import_pattern`
  override + tight `paths` scope.

## Implementation notes

- Module: `crates/alint-rules/src/import_gate.rs`. Per-file:
  `impl Rule { rule_common_impl!(); path_scope()->Some(&scope);
  evaluate()->eval_per_file(self,ctx); as_per_file()->Some }` +
  `impl PerFileRule { path_scope; evaluate_file }`, modelled on
  `file_content_forbidden` / `ordered_block`.
- `forbid` and the resolved `import_pattern` compile to
  `regex::Regex` at `build` time (config error on a bad regex,
  like `file_content_forbidden`). `allow` compiles to a `Scope`
  (`Scope::from_patterns`); the exemption check reuses
  `Scope::matches(path, ctx.index)`.
- `language` → a `const &str` default pattern; explicit
  `import_pattern` overrides; `generic` with neither is a
  config error.
- No `FileIndex` iteration beyond the per-file walk; no shared
  `crate::extract`; O(L) per file.

## Tests

- `forbid` hit (per-import violation, correct line) and miss
  (silent); a comment/string mentioning the forbidden path does
  **not** fire (the target-not-line guarantee).
- `allow` glob exempts a file in scope.
- Each `language` preset: Go single `import "x"` + grouped
  `\t"x"` + `_ "x"`; Python `import a.b` and `from a.b import c`;
  Rust `use a::b;`; JS `import`/`require`.
- Explicit `import_pattern` overrides a preset; `generic` with
  no `import_pattern` ⇒ build error; bad `forbid` regex ⇒ build
  error.
- Multiple forbidden imports in one file ⇒ multiple violations.
- Lockstep with the codebase invariants (same checklist #1-#4
  followed): `coverage_audit_pass_fail` (per-file pass/fail
  scenarios), schema `$def` + dispatch `$ref` in both mirrored
  `config.json`, `all_kinds.yaml` entry, regenerated
  default-options snapshot, rule-count **74 → 75** across README
  ×2 / `docs/site/about` / `coverage_audit_readme_claims`,
  `docs/rules.md` section, CHANGELOG `[Unreleased]` Added (the
  fifth v0.10 item).
- **Bench-compare threshold:** O(L) line scan over scoped files,
  same class as `file_content_forbidden` — full-run S-class wall
  must not regress vs the pre-phase baseline (`xtask
  bench-gate`, per `RELEASING.md`).

## Open questions

Resolve inline when implementation lands.

1. **Language-AST mode.** A true import parser (tree-sitter /
   per-language) would catch multi-line and conditional imports
   the preset misses. Deliberate non-goal for v0.10 (heuristic
   vs precise + a heavy dep); revisit if the line-based preset
   proves insufficient for a demand source.
2. **`require:` (must-import) inverse.** "Every file in scope
   *must* import X" (e.g. a license header import, a required
   shim). Sibling shape; deferred unless a second source needs
   it (1 today).
3. **Per-target `allow` (not just per-file).** "Forbid
   `internal/*` except `internal/sanctioned`." Today the
   `forbid` regex itself can carve that out
   (`internal/(?!sanctioned)`); a structured allow-list of
   target patterns is a v0.11 ergonomics call.
4. **Preset coverage.** Go grouped-import end-anchoring vs
   weird-but-valid gofmt edge cases; Python parenthesised
   `from x import (\n a,\n b)`. v0.10 ships the common forms +
   the `import_pattern` escape hatch; widen presets only on a
   real miss.
5. **Relationship to `file_content_forbidden`.** `import_gate`
   is the import-aware specialisation (target extraction +
   `allow`); docs cross-link, not the same kind.
