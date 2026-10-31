//! Generated proof that the committed alint plugin contract compiles with
//! witforge and round-trips through protobuf and proto3-JSON.
//!
//! `glue` and `plain` are witforge's output, kept byte for byte (hence
//! `rustfmt::skip`) so that `ci/scripts/witforge-consumer.sh` can check them
//! with `--check`; regenerate them with that script's `--write`, never by
//! hand. `proto` is generated at build time from the checked schema. Lints are
//! allowed on the generated modules only; the rest is held to the workspace's.

/// Protobuf and proto3-JSON glue.
#[rustfmt::skip]
#[allow(warnings, clippy::all, clippy::pedantic)]
pub mod glue;
/// Plain Rust contract types and traits.
#[rustfmt::skip]
#[allow(warnings, clippy::all, clippy::pedantic)]
pub mod plain;
/// Prost types generated from the emitted protobuf schema.
pub mod proto;
