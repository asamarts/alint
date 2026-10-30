#![allow(warnings)]
#![allow(clippy::all)]

//! Generated proof that the committed alint plugin contract compiles with the
//! witforge 0.1 candidate. Do not edit the generated modules; regenerate them
//! with `ci/scripts/witforge-consumer.sh` once the candidate is published.

/// Protobuf and proto3-JSON glue.
pub mod glue;
/// Plain Rust contract types and traits.
pub mod plain;
/// Prost types generated from the emitted protobuf schema.
pub mod proto;
