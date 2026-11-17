//! Integration tests for `alint-e2e`, built as one test binary.
//!
//! Each module is one file in this directory. `autotests = false` in
//! `Cargo.toml` turns off Cargo's one-binary-per-file discovery, so a new
//! test file runs only once it is declared here; xtask's `test_layout` test
//! fails when one is missing. `cargo test -p alint-e2e --test it -- <module>::`
//! runs one file's tests; the filter matches substrings, so it also runs any
//! module whose name ends in `<module>` (nextest can anchor it:
//! `-E 'test(/^<module>::/)'`).

// Shared by several modules: loaders and the shared rule registry.
mod support;

mod command_plugin;
mod coverage_audit;
mod coverage_audit_baseline_safety;
mod coverage_audit_bench_listing;
mod coverage_audit_benchmarks_trajectory;
mod coverage_audit_bundled_rulesets;
mod coverage_audit_cross_file_dispatch;
mod coverage_audit_doc_examples;
mod coverage_audit_doc_links;
mod coverage_audit_examples_parse;
mod coverage_audit_fix_coverage;
mod coverage_audit_fix_spawn_gate;
mod coverage_audit_git_modes;
mod coverage_audit_git_tracked_only;
mod coverage_audit_output_no_em_dash;
mod coverage_audit_pass_fail;
mod coverage_audit_planned_rulesets;
mod coverage_audit_readme_claims;
mod coverage_audit_rules_md_drift;
mod coverage_audit_schema_drift;
mod coverage_audit_scope_owns_filter;
mod coverage_audit_site_docs_frontmatter;
mod coverage_audit_smoke_fixtures;
mod coverage_audit_spawn_gate;
mod coverage_audit_when_wiring;
mod dogfood_bundled_no_drift;
mod fingerprint_golden;
mod fixable_accuracy;
mod interp_integration;
mod invariants;
mod output_weird_paths;
mod scenarios;
mod unix_metadata;
mod version_two;
