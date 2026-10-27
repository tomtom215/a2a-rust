// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Resource attributes the process can state about itself: `host.name`,
//! `host.arch`, `os.type`, `process.pid` and `process.executable.name`.
//!
//! The SDK's own resource carried the service name, an instance id and
//! `telemetry.sdk.*`, and nothing about where the process runs. Measured
//! 2026-10-26: a `hello-agent` export's resource held exactly those five keys.
//! Backends group and filter by host (`host.name`) and tell a replica's
//! restarts apart by pid; the Java, Python and Go SDKs detect them by default.
//!
//! Nothing here is read from outside the process except a hostname file, and
//! `OTEL_RESOURCE_ATTRIBUTES` wins over every attribute (the builder adds only
//! the keys the variable does not name). The hostname comes from `HOSTNAME` or
//! `COMPUTERNAME`, else the kernel's own file on Linux; this crate forbids
//! `unsafe`, which `gethostname(2)` needs, and a macOS process that does not
//! export `HOSTNAME` gets no `host.name` rather than a wrong one.
//! [`TelemetryBuilder::without_resource_detection`](super::TelemetryBuilder::without_resource_detection)
//! turns all of it off, for an export to a party that should not learn them.

use opentelemetry::KeyValue;

use super::config::Env;

/// `os.type`'s value for `std::env::consts::OS`, where the semantic
/// conventions name one.
fn os_type(os: &str) -> Option<&'static str> {
    Some(match os {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        "freebsd" => "freebsd",
        "netbsd" => "netbsd",
        "openbsd" => "openbsd",
        "dragonfly" => "dragonflybsd",
        "solaris" | "illumos" => "solaris",
        "aix" => "aix",
        _ => return None,
    })
}

/// `host.arch`'s value for `std::env::consts::ARCH`, where the semantic
/// conventions name one.
fn host_arch(arch: &str) -> Option<&'static str> {
    Some(match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "x86",
        "arm" => "arm32",
        "powerpc" => "ppc32",
        "powerpc64" => "ppc64",
        "s390x" => "s390x",
        _ => return None,
    })
}

/// The host's name: `HOSTNAME` or `COMPUTERNAME`, else the first of
/// `files` that holds one.
fn host_name(env: &Env, read: &dyn Fn(&str) -> Option<String>, files: &[&str]) -> Option<String> {
    ["HOSTNAME", "COMPUTERNAME"]
        .iter()
        .find_map(|var| env(var))
        .or_else(|| files.iter().find_map(|path| read(path)))
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
}

/// What the process can say about where it runs.
pub(super) fn attributes(env: &Env) -> Vec<KeyValue> {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    detect(
        env,
        &read,
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id(),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
    )
}

fn detect(
    env: &Env,
    read: &dyn Fn(&str) -> Option<String>,
    os: &str,
    arch: &str,
    pid: u32,
    executable: Option<String>,
) -> Vec<KeyValue> {
    let mut found = Vec::new();
    if let Some(name) = host_name(env, read, &["/proc/sys/kernel/hostname", "/etc/hostname"]) {
        found.push(KeyValue::new("host.name", name));
    }
    if let Some(arch) = host_arch(arch) {
        found.push(KeyValue::new("host.arch", arch));
    }
    if let Some(os) = os_type(os) {
        found.push(KeyValue::new("os.type", os));
    }
    found.push(KeyValue::new("process.pid", i64::from(pid)));
    if let Some(name) = executable {
        found.push(KeyValue::new("process.executable.name", name));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> Box<Env> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        Box::new(move |k| map.get(k).cloned().filter(|v| !v.trim().is_empty()))
    }

    fn keys(found: &[KeyValue]) -> Vec<&str> {
        found.iter().map(|kv| kv.key.as_str()).collect()
    }

    fn value(found: &[KeyValue], key: &str) -> Option<String> {
        found
            .iter()
            .find(|kv| kv.key.as_str() == key)
            .map(|kv| kv.value.to_string())
    }

    #[test]
    fn the_convention_names_are_used_for_known_systems_and_nothing_for_others() {
        for (os, expected) in [
            ("linux", "linux"),
            ("macos", "darwin"),
            ("windows", "windows"),
            ("freebsd", "freebsd"),
            ("netbsd", "netbsd"),
            ("openbsd", "openbsd"),
            ("dragonfly", "dragonflybsd"),
            ("solaris", "solaris"),
            ("illumos", "solaris"),
            ("aix", "aix"),
        ] {
            assert_eq!(os_type(os), Some(expected), "{os}");
        }
        for (arch, expected) in [
            ("x86_64", "amd64"),
            ("aarch64", "arm64"),
            ("x86", "x86"),
            ("arm", "arm32"),
            ("powerpc", "ppc32"),
            ("powerpc64", "ppc64"),
            ("s390x", "s390x"),
        ] {
            assert_eq!(host_arch(arch), Some(expected), "{arch}");
        }
        assert_eq!(os_type("haiku"), None);
        assert_eq!(host_arch("riscv64"), None);
    }

    #[test]
    fn the_host_name_comes_from_the_environment_then_the_files_and_is_trimmed() {
        let files = |path: &str| (path == "/etc/hostname").then(|| "from-file\n".to_owned());
        assert_eq!(
            host_name(&*env(&[("HOSTNAME", "pod-7")]), &files, &["/etc/hostname"]).as_deref(),
            Some("pod-7")
        );
        assert_eq!(
            host_name(
                &*env(&[("COMPUTERNAME", "WIN-1")]),
                &files,
                &["/etc/hostname"]
            )
            .as_deref(),
            Some("WIN-1")
        );
        assert_eq!(
            host_name(&*env(&[]), &files, &["/missing", "/etc/hostname"]).as_deref(),
            Some("from-file")
        );
        assert_eq!(
            host_name(&*env(&[]), &|_| Some("  \n".to_owned()), &["/x"]),
            None
        );
        assert_eq!(host_name(&*env(&[]), &|_| None, &["/x"]), None);
    }

    #[test]
    fn every_known_attribute_is_reported_and_an_unknown_one_is_not_invented() {
        let found = detect(
            &env(&[("HOSTNAME", "h")]),
            &|_| None,
            "linux",
            "x86_64",
            4242,
            Some("agent".to_owned()),
        );
        assert_eq!(
            keys(&found),
            [
                "host.name",
                "host.arch",
                "os.type",
                "process.pid",
                "process.executable.name"
            ]
        );
        assert_eq!(value(&found, "process.pid").as_deref(), Some("4242"));
        let found = detect(&env(&[]), &|_| None, "haiku", "riscv64", 1, None);
        assert_eq!(keys(&found), ["process.pid"]);
    }

    #[test]
    fn this_process_reports_its_own_pid_and_executable() {
        let found = attributes(&*env(&[]));
        assert_eq!(
            value(&found, "process.pid"),
            Some(std::process::id().to_string())
        );
        assert!(value(&found, "process.executable.name").is_some_and(|n| !n.is_empty()));
    }
}
