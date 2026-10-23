// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Reading one HTTP/1.1 request off a raw socket, for mock webhooks.
//!
//! The mocks used to wait for `readable()` and then `try_read` once. tokio
//! 1.54 starts an accepted socket as readable (tokio #8435), so `readable()`
//! returned before any byte had arrived, the read took nothing, and the mock
//! answered and closed while the sender was still writing: 19 of the 26
//! `push_sender_tests` failed with `client error (Canceled)` (2026-10-20). A
//! readiness event never said a whole request was there; this reads one.

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

/// Reads the request head and the body its `Content-Length` declares, and
/// returns them as text. Stops early, with what it has, at end of stream.
pub async fn read_request(stream: &mut TcpStream) -> String {
    let mut buf = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
            let body_len = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if buf.len() >= end + 4 + body_len {
                break;
            }
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}
