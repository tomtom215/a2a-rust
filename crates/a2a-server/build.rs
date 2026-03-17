// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F.

fn main() {
    #[cfg(feature = "grpc")]
    {
        tonic_build::configure()
            .build_server(true)
            .build_client(false)
            .compile_protos(
                &["../../proto/a2a.proto"],
                &["../../proto"],
            )
            .expect("Failed to compile A2A proto");
    }
}
