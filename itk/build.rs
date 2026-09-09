// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

fn main() {
    // SAFETY: a build script runs on the main thread of a fresh process; no
    // other thread exists to read the environment concurrently, and the one
    // reader that matters — prost-build's `protoc` lookup — runs on this
    // thread after the write.
    unsafe {
        std::env::set_var(
            "PROTOC",
            protoc_bin_vendored::protoc_bin_path().expect("vendored protoc"),
        )
    };
    prost_build::compile_protos(&["protos/instruction.proto"], &["protos"])
        .expect("compile instruction.proto");
    println!("cargo:rerun-if-changed=protos/instruction.proto");
}
