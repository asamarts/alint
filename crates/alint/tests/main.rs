//! Integration tests for `alint`, built as one test binary.
//!
//! Each module is one file in this directory. `autotests = false` in
//! `Cargo.toml` turns off Cargo's one-binary-per-file discovery, so a new
//! test file runs only once it is declared here; xtask's `test_layout` test
//! fails when one is missing. `cargo test -p alint --test it -- <module>::`
//! runs one file's tests; the filter matches substrings, so it also runs any
//! module whose name ends in `<module>` (nextest can anchor it:
//! `-E 'test(/^<module>::/)'`).

// Shared by several modules: LSP JSON-RPC harness for the `lsp_*` modules.
#[cfg(unix)]
mod lsp_common;

mod analysis_failures;
mod analysis_time;
mod baseline_cli;
mod baseline_keys;
mod broken_pipe;
mod changed_mode_tree_context;
mod cli;
mod cli_consistency;
mod cli_flag_inventory;
mod cli_format_contract;
mod config_compatibility;
mod config_discovery;
mod config_error_redaction;
mod evidence_redaction;
mod expect_matches;
mod explain_trace;
mod fix_baseline;
mod fix_changed;
mod fix_convergence;
mod fix_diff;
mod fix_flags;
mod fix_robustness;
mod fix_skip_classification;
mod fix_spawn_gate;
mod lsp_audit;
mod lsp_protocol;
mod lsp_smoke;
mod only_filter;
mod path_confinement_cli;
mod pattern_hints_cli;
mod policy_test_cli;
mod pty_color;
mod sarif_fixes;
mod show_notes;
mod spawn_gate;
mod styling_uniform;
mod terminal_sanitize_cli;
mod tracing_color;
mod walk_boundary;
mod weird_path_formats_cli;
mod write_confinement_cli;
