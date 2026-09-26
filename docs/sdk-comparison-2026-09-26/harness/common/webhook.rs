// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

// Shared, SDK-independent push-notification receiver. Included verbatim by
// BOTH drivers. Accepts HTTP/1.1 POSTs, records the body, replies 200.
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Default)]
pub struct Received(pub Arc<Mutex<Vec<String>>>, pub Arc<Mutex<Vec<String>>>);

impl Received {
    pub fn bodies(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }
    /// Lower-cased request heads (request line + headers), one per POST.
    #[allow(dead_code)]
    pub fn heads(&self) -> Vec<String> {
        self.1.lock().unwrap().clone()
    }
}

pub async fn start() -> (String, Received) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", l.local_addr().unwrap());
    let rec = Received::default();
    let r2 = rec.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = l.accept().await else { continue };
            let r3 = r2.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
                loop {
                    let n = match s.read(&mut tmp).await { Ok(0) | Err(_) => return, Ok(n) => n };
                    buf.extend_from_slice(&tmp[..n]);
                    let Some(h) = buf.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
                    let head = String::from_utf8_lossy(&buf[..h]).to_ascii_lowercase();
                    let len = head.lines()
                        .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    if buf.len() >= h + 4 + len {
                        let body = String::from_utf8_lossy(&buf[h + 4..h + 4 + len]).to_string();
                        r3.1.lock().unwrap().push(head.clone());
                        r3.0.lock().unwrap().push(body);
                        let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
                        return;
                    }
                }
            });
        }
    });
    (url, rec)
}
