#![allow(warnings)]
#![allow(clippy::all)]

//! Generated proof that the committed alint plugin contract compiles with
//! witforge and round-trips through protobuf and proto3-JSON.
//!
//! `glue` and `plain` are witforge's output, kept byte for byte (hence
//! `rustfmt::skip`) so that `ci/scripts/witforge-consumer.sh` can check them
//! with `--check`; regenerate them with that script's `--write`, never by
//! hand. `proto` is generated at build time from the checked schema.

/// Protobuf and proto3-JSON glue.
#[rustfmt::skip]
pub mod glue;
/// Plain Rust contract types and traits.
#[rustfmt::skip]
pub mod plain;
/// Prost types generated from the emitted protobuf schema.
pub mod proto;
