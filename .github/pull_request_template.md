<!--
Thanks for contributing!

Title: use Conventional Commits style — `feat(rules): add no_lockfile_drift`,
       `fix(engine): scope_filter must consult ctx.index`, etc.

Body: keep this template; delete the bracketed [example] lines and fill in.
-->

## Summary

[1-3 sentences: what changed and why. Link to the issue if applicable.]

## Test plan

<!-- A bulleted checklist of how you verified the change. CI runs all of this
     too, but listing it here helps reviewers know what to focus on. -->

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` clean
- [ ] `cargo fmt --check` clean
- [ ] `cargo doc --no-deps --workspace` with `RUSTDOCFLAGS=-D warnings` clean
- [ ] [If touching rules] new pass + fail e2e scenarios under `crates/alint-e2e/scenarios/check/<family>/`
- [ ] [If touching rules] coverage_audit_* tests still pass
- [ ] [If touching engine perf hot paths] ran `xtask bench-scale --scenarios S1,S6,S7 --sizes 100k --warmup 3 --runs 10` and pasted the delta vs main below

## Linked issues

<!-- "Closes #123" / "Refs #456" — keeps the issue tracker tidy. -->

## Notes for reviewer

<!-- Anything non-obvious about the approach. If you considered alternatives,
     mention them. If there's a follow-up PR planned, say so. -->
