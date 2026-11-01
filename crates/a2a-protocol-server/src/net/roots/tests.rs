// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The root store reads `SSL_CERT_FILE` and `SSL_CERT_DIR` as curl and Go do.
//! The environment itself is read once per process (a test cannot rewrite it
//! under `forbid(unsafe_code)`), so these drive `read_env_roots` with paths;
//! `tests/outbound_proxy_and_roots.rs` drives the real variables in a process
//! of its own.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;

/// A fresh scratch directory under the system temp directory.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("a2a-roots-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A self-signed certificate for `name`, as PEM.
fn pem_for(name: &str) -> String {
    let key = rcgen::KeyPair::generate().unwrap();
    rcgen::CertificateParams::new(vec![name.to_owned()])
        .unwrap()
        .self_signed(&key)
        .unwrap()
        .pem()
}

fn os(p: &Path) -> OsString {
    p.as_os_str().to_owned()
}

#[test]
fn nothing_named_adds_nothing() {
    assert_eq!(read_env_roots(None, None).len(), 0);
}

#[test]
fn a_file_adds_every_certificate_in_it() {
    let dir = scratch("file");
    let bundle = dir.join("bundle.pem");
    std::fs::write(
        &bundle,
        format!("{}{}", pem_for("a.test"), pem_for("b.test")),
    )
    .unwrap();
    assert_eq!(read_env_roots(Some(&os(&bundle)), None).len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_file_without_a_certificate_adds_nothing() {
    let dir = scratch("nocert");
    let junk = dir.join("junk.pem");
    std::fs::write(&junk, "not a certificate\n").unwrap();
    assert_eq!(read_env_roots(Some(&os(&junk)), None).len(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_unreadable_path_is_skipped_and_the_rest_is_kept() {
    let dir = scratch("missing");
    let good = dir.join("good.pem");
    std::fs::write(&good, pem_for("good.test")).unwrap();
    let missing = dir.join("absent.pem");
    // `std::env::join_paths` writes the platform's separator (`:` or `;`).
    let both = std::env::join_paths([missing, good]).unwrap();
    assert_eq!(read_env_roots(Some(&both), None).len(), 1);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_directory_is_read_in_name_order_and_non_pem_files_are_skipped() {
    use rustls_pki_types::CertificateDer;
    use rustls_pki_types::pem::PemObject;

    let dir = scratch("dir");
    let (a, b) = (pem_for("a.test"), pem_for("b.test"));
    // Written in the opposite order to the names, so a listing in creation
    // order would put b first.
    std::fs::write(dir.join("b.pem"), &b).unwrap();
    std::fs::write(dir.join("a.pem"), &a).unwrap();
    std::fs::write(dir.join("README"), "hashed links live here too\n").unwrap();
    std::fs::create_dir(dir.join("subdir")).unwrap();
    let found = read_env_roots(None, Some(&os(&dir)));
    assert_eq!(
        found.len(),
        3 - 1,
        "two certificates; README and subdir skipped"
    );
    // README sorts before a.pem but holds no certificate; then a, then b.
    assert_eq!(
        found[0],
        CertificateDer::from_pem_slice(a.as_bytes()).unwrap()
    );
    assert_eq!(
        found[1],
        CertificateDer::from_pem_slice(b.as_bytes()).unwrap()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_empty_entry_in_the_list_is_ignored() {
    let dir = scratch("emptyentry");
    let good = dir.join("good.pem");
    std::fs::write(&good, pem_for("good.test")).unwrap();
    // `good.pem::` and `::good.pem` are legal lists, as in `PATH`.
    let list = std::env::join_paths([&good, &PathBuf::new(), &good]).unwrap();
    assert_eq!(read_env_roots(Some(&list), None).len(), 2);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_unreadable_directory_is_skipped() {
    let dir = scratch("nodir");
    let gone = dir.join("gone");
    assert_eq!(read_env_roots(None, Some(&os(&gone))).len(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_certificate_from_the_environment_is_trusted_beside_mozillas() {
    let dir = scratch("store");
    let bundle = dir.join("ca.pem");
    std::fs::write(&bundle, pem_for("private.test")).unwrap();
    let extra = read_env_roots(Some(&os(&bundle)), None);
    assert_eq!(extra.len(), 1);
    let mozilla = root_cert_store().len();
    let mut store = root_cert_store();
    for cert in extra {
        store.add(cert).unwrap();
    }
    assert_eq!(store.len(), mozilla + 1);
    assert!(mozilla > 100, "Mozilla's roots are there: {mozilla}");
    std::fs::remove_dir_all(&dir).unwrap();
}
