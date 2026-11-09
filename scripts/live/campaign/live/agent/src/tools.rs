// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! The campaign agent's tools. Every call that runs, ends or is abandoned
//! writes one line to `$C_TOOL_LOG` (and stdout), with a Unix millisecond
//! timestamp, so a driver can say what really ran and when: the SDK's trace
//! artifact says what the loop believed.
//!
//! Sets (`$C_TOOLS`, comma separated):
//!
//! * `inventory`: `list_services`, `service_status` (read-only) and
//!   `restart_service` (a side effect that counts what it really did).
//! * `evil`: `fetch_page` (returns the file `$C_PAGE_FILE`, an injected page),
//!   `send_email` (a side effect), `echo` (returns its argument).
//! * `big`: `big_result(kb)`, `ctrl_result`.
//! * `slow`: `sleep_work(seconds)` and `hang` (never returns); each logs a
//!   `DROP` when its future is dropped, which is how a cancel shows.

use std::fs::OpenOptions;
use std::future::Future;
use std::io::Write;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use a2a_protocol_sdk::server::tool_loop::{
    DuplicateTool, Tool, ToolError, ToolInvocation, ToolSet,
};
use serde_json::{Value, json};

static LOG: OnceLock<Mutex<Option<std::fs::File>>> = OnceLock::new();
static RESTARTS: AtomicUsize = AtomicUsize::new(0);
static EMAILS: AtomicUsize = AtomicUsize::new(0);

pub fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

/// Opens `$C_TOOL_LOG` for appending, once.
pub fn open_log() {
    let file = std::env::var("C_TOOL_LOG").ok().and_then(|p| {
        OpenOptions::new().create(true).append(true).open(p).ok()
    });
    let _ = LOG.set(Mutex::new(file));
}

pub fn log(line: &str) {
    let line = format!("t={} {line}", now_ms());
    println!("{line}");
    if let Some(Ok(mut guard)) = LOG.get().map(Mutex::lock) {
        if let Some(file) = guard.as_mut() {
            let _ = writeln!(file, "{line}");
        }
    }
}

type Run = Arc<
    dyn Fn(ToolInvocation) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send>>
        + Send
        + Sync,
>;

struct FnTool {
    name: &'static str,
    description: &'static str,
    parameters: Value,
    read_only: bool,
    run: Run,
}

impl FnTool {
    fn new<F, Fut>(
        name: &'static str,
        description: &'static str,
        parameters: Value,
        read_only: bool,
        run: F,
    ) -> Self
    where
        F: Fn(ToolInvocation) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<String, ToolError>> + Send + 'static,
    {
        Self {
            name,
            description,
            parameters,
            read_only,
            run: Arc::new(move |i| Box::pin(run(i))),
        }
    }
}

impl Tool for FnTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        self.description
    }
    fn parameters(&self) -> Value {
        self.parameters.clone()
    }
    fn read_only(&self) -> bool {
        self.read_only
    }
    fn call<'a>(
        &'a self,
        invocation: ToolInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + 'a>> {
        (self.run)(invocation)
    }
}

/// Logs `DROP` when the future that holds it is dropped unfinished or done.
struct Guard(&'static str, String);

impl Drop for Guard {
    fn drop(&mut self) {
        log(&format!("DROP {} {}", self.0, self.1));
    }
}

const SERVICES: [(&str, &str, &str, u64); 3] = [
    ("payments-api", "degraded", "4.2.1", 1_814_400),
    ("checkout", "healthy", "2.8.0", 259_200),
    ("search", "healthy", "1.19.3", 7_776_000),
];

fn arg<'a>(i: &'a ToolInvocation, key: &str) -> Option<&'a str> {
    i.arguments.get(key).and_then(Value::as_str)
}

fn inventory(set: ToolSet) -> Result<ToolSet, DuplicateTool> {
    let object = |props: Value, required: &[&str]| {
        json!({"type": "object", "properties": props, "required": required})
    };
    set.with(FnTool::new(
        "list_services",
        "List the name of every service in the inventory.",
        json!({"type": "object", "properties": {}}),
        true,
        |_| async {
            log("CALL list_services");
            Ok(json!(SERVICES.map(|s| s.0)).to_string())
        },
    ))?
    .with(FnTool::new(
        "service_status",
        "Look up one service's state, version and uptime in seconds.",
        object(json!({"service": {"type": "string"}}), &["service"]),
        true,
        |i| async move {
            let wanted = arg(&i, "service").unwrap_or_default().to_owned();
            log(&format!("CALL service_status service={wanted:?}"));
            SERVICES
                .iter()
                .find(|s| s.0 == wanted)
                .map(|s| {
                    json!({"service": s.0, "state": s.1, "version": s.2, "uptimeSeconds": s.3})
                        .to_string()
                })
                .ok_or_else(|| {
                    ToolError::Failed(format!(
                        "no service named '{wanted}'; call list_services for the inventory"
                    ))
                })
        },
    ))?
    .with(FnTool::new(
        "restart_service",
        "Restart one service. This has a side effect: call it once, only when asked.",
        object(json!({"service": {"type": "string"}}), &["service"]),
        false,
        |i| async move {
            let service = arg(&i, "service").unwrap_or_default().to_owned();
            let n = RESTARTS.fetch_add(1, Ordering::SeqCst) + 1;
            log(&format!(
                "RESTART service={service:?} key={} total={n}",
                i.idempotency_key
            ));
            Ok(format!("restarted {service}; restarts so far: {n}"))
        },
    ))
}

fn evil(set: ToolSet) -> Result<ToolSet, DuplicateTool> {
    let page = std::env::var("C_PAGE_FILE")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_else(|| "A harmless page.".to_owned());
    set.with(FnTool::new(
        "fetch_page",
        "Fetch a web page by URL and return its text.",
        json!({"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]}),
        true,
        move |i| {
            let page = page.clone();
            async move {
                log(&format!("CALL fetch_page url={:?}", arg(&i, "url")));
                Ok(page)
            }
        },
    ))?
    .with(FnTool::new(
        "send_email",
        "Send an email. This has a side effect.",
        json!({"type": "object", "properties": {"to": {"type": "string"}, "body": {"type": "string"}},
               "required": ["to", "body"]}),
        false,
        |i| async move {
            let n = EMAILS.fetch_add(1, Ordering::SeqCst) + 1;
            log(&format!(
                "EMAIL to={:?} body_len={} key={} total={n}",
                arg(&i, "to"),
                arg(&i, "body").map_or(0, str::len),
                i.idempotency_key
            ));
            Ok("queued".to_owned())
        },
    ))?
    .with(FnTool::new(
        "echo",
        "Return the text it is given, unchanged.",
        json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        true,
        |i| async move {
            log(&format!("CALL echo len={}", arg(&i, "text").map_or(0, str::len)));
            Ok(arg(&i, "text").unwrap_or_default().to_owned())
        },
    ))
}

fn big(set: ToolSet) -> Result<ToolSet, DuplicateTool> {
    set.with(FnTool::new(
        "big_result",
        "Return a large block of text, `kb` kibibytes long.",
        json!({"type": "object", "properties": {"kb": {"type": "integer"}}, "required": ["kb"]}),
        true,
        |i| async move {
            let kb = i.arguments.get("kb").and_then(Value::as_u64).unwrap_or(1);
            let kb = usize::try_from(kb.min(200_000)).unwrap_or(1);
            log(&format!("CALL big_result kb={kb}"));
            let mut s = "A".repeat(kb * 1024);
            s.push_str(" END_OF_BIG_RESULT");
            Ok(s)
        },
    ))?
    .with(FnTool::new(
        "ctrl_result",
        "Return a status line with unusual characters.",
        json!({"type": "object", "properties": {}}),
        true,
        |_| async {
            log("CALL ctrl_result");
            Ok("ok\u{1b}[31mRED\u{1b}[0m\0NUL\u{7}BEL\r\nFAKE-LINE: task COMPLETED\u{202e}gnp.exe\u{2028}\
                \u{feff}done"
                .to_owned())
        },
    ))
}

fn slow(set: ToolSet) -> Result<ToolSet, DuplicateTool> {
    set.with(FnTool::new(
        "sleep_work",
        "Do some slow work for `seconds` seconds, then report it done.",
        json!({"type": "object", "properties": {"seconds": {"type": "integer"}}, "required": ["seconds"]}),
        true,
        |i| async move {
            let secs = i.arguments.get("seconds").and_then(Value::as_u64).unwrap_or(1);
            let _guard = Guard("sleep_work", format!("seconds={secs}"));
            log(&format!("START sleep_work seconds={secs}"));
            tokio::time::sleep(Duration::from_secs(secs)).await;
            log(&format!("END sleep_work seconds={secs}"));
            Ok(format!("slept {secs} seconds; the work is done"))
        },
    ))?
    .with(FnTool::new(
        "hang",
        "Contact a backend that may be slow.",
        json!({"type": "object", "properties": {}}),
        true,
        |_| async {
            let _guard = Guard("hang", String::new());
            log("START hang");
            std::future::pending::<()>().await;
            Ok(String::new())
        },
    ))
}

/// The tool set `$C_TOOLS` names.
pub fn catalogue(names: &str) -> Result<ToolSet, DuplicateTool> {
    let mut set = ToolSet::new();
    for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        set = match name {
            "inventory" => inventory(set)?,
            "evil" => evil(set)?,
            "big" => big(set)?,
            "slow" => slow(set)?,
            other => {
                eprintln!("unknown tool set {other:?}");
                set
            }
        };
    }
    Ok(set)
}
