# alint-testkit

Versioned policy-test documents and a hermetic runner for alint rulesets.

```rust,no_run
use std::path::Path;
use alint_testkit::{PolicyTest, Runner, RunnerOptions};

let source = Path::new("tests/no-debug.yml");
let yaml = std::fs::read_to_string(source)?;
let test = PolicyTest::from_yaml(&yaml)?;
let run = Runner::new(RunnerOptions::default()).run(&test, source)?;
assert!(run.passed(), "{:#?}", run.mismatches);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`Runner::run` validates the document, confines package paths and repository fixtures, disables remote config resolution, isolates Git and process environments, reloads configuration for every step, and removes its temporary repository when the result is dropped.

Rules or fixers that spawn commands, including a rule nested in another rule's `require:`, custom facts, and rules that access a host keyring, require both a document declaration under `capabilities` and the corresponding `RunnerOptions` grant. The default grants neither capability. A test's `given.env` reaches config interpolation, `when:` conditions and granted commands only, and the host's other variables reach none of them; the runner's own Git commands run with a fixed environment and never read a fixture as a repository.

Batch callers can invoke `Runner::preflight` for every selected document before execution.
`Runner::run_deferred_snapshot_update` and `PolicyTestRun::commit_snapshot_update` support
all-inline-assertions-first snapshot updates without executing a test twice.

- [Policy-test v1 schema](https://github.com/asamarts/alint/blob/main/schemas/v1/policy-test.json)
- [Policy-test contract](https://github.com/asamarts/alint/blob/main/docs/design/v0.20/policy-tests.md)
- [Policy-test report v1 schema](https://github.com/asamarts/alint/blob/main/schemas/v1/policy-test-report.json)
- [Tree fixture syntax](TREE_SPEC.md)

The optional `strategies` feature exposes repository-maintained proptest generators. It is disabled by default.

Licensed under Apache-2.0 OR MIT.
