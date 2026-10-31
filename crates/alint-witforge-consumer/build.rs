//! Generates the prost types and their proto3-JSON impls from the schema
//! witforge emits (`proto/v0.proto`), as witforge's design expects of a
//! consumer: `protox` parses it, so no `protoc` binary is needed. The schema
//! itself is committed and checked against witforge by
//! `ci/scripts/witforge-consumer.sh`; nothing generated from it is committed,
//! so it cannot go stale.

use std::path::{Path, PathBuf};

use prost::Message;

fn main() {
    let proto_dir = Path::new("proto");
    let schema = proto_dir.join("v0.proto");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", schema.display());
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));

    let descriptors = protox::compile([&schema], [proto_dir]).expect("proto/v0.proto compiles");
    let encoded = descriptors.encode_to_vec();
    prost_build::Config::new()
        .out_dir(&out)
        .compile_fds(descriptors)
        .expect("prost-build generates the protobuf types");
    pbjson_build::Builder::new()
        .out_dir(&out)
        .register_descriptors(&encoded)
        .expect("the descriptor set registers")
        .build(&[".alint.plugin.v0"])
        .expect("pbjson-build generates the proto3-JSON impls");
}
