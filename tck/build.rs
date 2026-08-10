// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Generates the TCK's own gRPC client from the canonical A2A proto.
//!
//! Deliberately **without** `extern_path`. The other crates in this workspace
//! point their generated stubs back at `a2a-protocol-types::proto` so there is
//! one set of message types and one conversion layer — correct for an
//! implementation, wrong for a conformance kit. If the TCK reused the
//! implementation's types, a mistake in the shared `.proto`-to-Rust mapping
//! would appear on both sides of every assertion and cancel out. The TCK
//! therefore compiles the schema itself, from its own vendored copy, into its
//! own structs; `scripts/check_proto_copies.sh` keeps that copy honest.

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let proto_dir = std::path::PathBuf::from(&manifest)
        .join("proto")
        .join("a2a_v1");
    let proto_file = proto_dir.join("a2a.proto");
    println!("cargo:rerun-if-changed={}", proto_dir.display());

    // Use the vendored protoc unless the caller supplied one, so a clean
    // machine needs no protobuf-compiler install. Same rationale as the other
    // crates' build scripts.
    if std::env::var_os("PROTOC").is_none() {
        if let Ok(path) = protoc_bin_vendored::protoc_bin_path() {
            std::env::set_var("PROTOC", path);
        }
    }

    let mut includes = vec![proto_dir];
    if let Ok(wkt) = protoc_bin_vendored::include_path() {
        includes.push(wkt);
    }

    tonic_prost_build::configure()
        .build_server(false)
        .build_client(true)
        .compile_protos(&[proto_file], &includes)
        .expect("compile canonical A2A proto (lf.a2a.v1) for the TCK gRPC binding");
}
