# Baseline multi-finding fixtures

Each subdirectory is a self-contained `alint.yml` + `tree/` that makes one rule
kind emit **two or more findings on the same `(rule_id, path)`** in a single
run. Without a per-violation `baseline_key`, those findings collapse to one
fingerprint and the baseline would silently mask all but the first — so these
fixtures are the regression guard for the keys set in slice 4. They are consumed
by `tests/coverage_audit_baseline_safety.rs`, which asserts the findings carry
*distinct* fingerprints (no masking collision).

Remove a rule's `baseline_key` and the matching fixture here turns the audit red.
