//! Integration tests for `alint-core`, built as one test binary.
//!
//! Each module is one file in this directory. `autotests = false` in
//! `Cargo.toml` turns off Cargo's one-binary-per-file discovery, so a new
//! test file runs only once it is declared here; xtask's `test_layout` test
//! fails when one is missing. `cargo test -p alint-core --test it -- <module>::`
//! runs one file's tests; the filter matches substrings, so it also runs any
//! module whose name ends in `<module>` (nextest can anchor it:
//! `-E 'test(/^<module>::/)'`).

mod git_denial;
mod kani_proofs_present;
mod yaml_parser_contract;
