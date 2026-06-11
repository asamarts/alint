# Design doc: pragmatic formal methods (Phase 5 / WS4)

Status: Implemented (proptest properties + a verified Kani proof + `debug_assert`
contracts, Phase 5).
Decisions: ADR-0001 (spec-driven development). WS4 of `spec-driven-development.md`.
Demand evidence: the v0.12 path-confinement security work added
`pathsafe::normalize_confined` (a `Some(_)` result must be safe to
`root.join(..)`). Example-based tests cover the cases someone thought of; a
property and a bounded proof cover the ones they didn't.

## 1. Problem

Most of alint is a deterministic batch pipeline, not a concurrent protocol, so
the ROI of heavy formal methods is low and concentrated. But two things are worth
more than example-based tests:

1. **Behaviour specs that can't drift** — properties that hold for *all* inputs
   (idempotent normalisation, a confined path never escaping the root), checked
   in CI on every run.
2. **A real proof on the security-critical core** — the confinement policy is a
   trust boundary; "we tested some paths" is weaker than "no bounded input
   escapes."

Without these, normalisation stability is assumed, and the confinement guarantee
rests on a handful of hand-picked test strings.

## 2. What we adopt (and what we don't)

Following WS4's tiering:

| Tier | Tool | Decision |
|---|---|---|
| Behaviour spec | **`proptest`** | **Adopted** — already a dep; properties added for `normalize_confined` and the `cross_file` normalisers. |
| Bounded proof | **Kani** | **Adopted** — one verified proof of the confinement policy; a separate, scheduled CI job (not a PR gate). |
| Runtime contract | **`debug_assert!`** | **Adopted** — the confinement invariant asserted at the function's exit (living documentation + a debug-build tripwire). |
| Runtime contract | `contracts` crate | **Deferred** — `debug_assert!` covers the few invariants today without a new dependency; migrate toward native compiler contracts (MCP-759) when they stabilise. |
| UB detection | **Miri** | **Deferred** — the workspace is `#![forbid(unsafe_code)]`, so Miri's primary value (UB in `unsafe`) is near-zero here; revisit if `unsafe` is ever introduced. |
| Model checking | **Stateright** | **Deferred** — alint's cross-file dispatch is sequential and order-independence is already covered by determinism tests; a Stateright model earns its keep only if the LSP grows an incremental cross-file cache. |
| Research-grade | Verus / Creusot / Prusti / TLA+ / Loom | **Skip** — ~5:1 proof-to-code for systems/crypto; not justified for a batch linter. "Watch," not "adopt." |

## 3. What shipped

**Properties (`cargo test`, every CI run):**

- `pathsafe`: `confinement_invariant` (a `Some(_)` result is always non-empty and
  purely `Normal` — safe to `root.join`), `normalize_confined_is_idempotent`, and
  `agrees_with_proven_model` (the real `PathBuf`-building code matches the bounded
  model the Kani proof verifies).
- `cross_file`: `normalize_transforms_are_idempotent` (all of `trim` / `lower` /
  `semver-major` / `semver-minor`), `apply_normalize_single_equals_transform`, and
  `semver_minor_yields_a_clean_band` (output is `MAJOR` or `MAJOR.MINOR`, digits
  only).

**Proof (`cargo kani`, scheduled CI):** `pathsafe::kani_proofs::confine_steps_is_sound`
proves, for every bounded component sequence, that the confinement policy
(`model::confine_steps`) is sound — an absolute component always escapes, a
surviving result has positive depth bounded by its `Normal` count (no phantom
components), and the `..` arithmetic never underflows or panics. The model is the
distilled policy; the `agrees_with_proven_model` property ties it to the real
function so the proof's guarantee transfers.

**Contract (`debug_assert!`):** `normalize_confined` asserts `is_confined(&out)`
at its exit — the invariant as runtime-checked documentation.

## 4. How to run

- **Properties:** `cargo test -p alint-rules` (they run as ordinary tests).
- **Proof:** install Kani once (`cargo install --locked kani-verifier && cargo kani
  setup`), then `cargo kani -p alint-rules --harness confine_steps_is_sound`.
  `cfg(kani)` is declared in `[workspace.lints.rust]` so the `#[cfg(kani)]`
  harnesses don't warn in normal builds. CI runs the proofs on a weekly schedule
  + manual dispatch via `.github/workflows/kani.yml` (the official
  `model-checking/kani-github-action`), kept off the PR path so model-checking
  time never blocks a merge.

## 5. Implementation notes

`proptest` added to `alint-rules` `[dev-dependencies]` (already a workspace dep).
The Kani model (`Step`, `confine_steps`, `step_of`) lives under
`#[cfg(any(test, kani))]` so it compiles only for tests and proofs — no dead code
in release. The proof harness is `#[cfg(kani)]`. No production code path changed
beyond the additive `debug_assert!`. Constitution invariants: none affected.

## 6. Tests

The properties *are* the tests (§3). The proof is verified by `cargo kani`
locally and on the scheduled CI job. The `agrees_with_proven_model` property is
the conformance link between the proof and the implementation.

## 7. Open questions

- **Promote `confine_steps` to the real implementation?** Deferred —
  `normalize_confined` must build the actual `PathBuf`, which the depth-only model
  can't; keeping them separate (model proven, impl property-checked against it) is
  the lighter design.
- **More proofs?** The dispatch-ordering helpers and count arithmetic are
  candidates if a future bug suggests one; this phase deliberately ships one
  high-value proof rather than a broad, shallow set.
- **Stateright for the LSP cache** becomes worthwhile if/when the LSP serves
  incremental cross-file results from a cache (today it re-runs).
