// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

//! Health Monitor executor — checks agent health via A2A client calls.

use std::future::Future;
use std::pin::Pin;

use a2a_protocol_types::error::A2aResult;
use a2a_protocol_types::message::{Part, PartContent};
use a2a_protocol_types::params::ListTasksParams;
use a2a_protocol_types::task::TaskState;

use a2a_protocol_client::{ClientBuilder, resolve_agent_card};
use a2a_protocol_server::executor::AgentExecutor;
use a2a_protocol_server::executor_helpers::boxed_future;
use a2a_protocol_server::request_context::RequestContext;
use a2a_protocol_server::streaming::EventQueueWriter;

use crate::helpers::EventEmitter;

/// The most URLs one request may ask the monitor to check.
const MAX_CHECKS: usize = 16;

/// Checks agent health by querying their agent cards and task lists. Reports
/// a summary artifact. Designed to test push notification delivery.
///
/// It checks the agents on its **roster** and no others. The URLs to check
/// arrive in the message, so any caller could otherwise aim the monitor at an
/// address of their choosing, from the monitor's position in the network, and
/// read the answer ("HEALTHY", "DEGRADED", "UNREACHABLE") per address: a port
/// scanner that fetches an internal service's card and lists its tasks (audit
/// SEC-15). A URL off the roster is reported as refused and not contacted.
pub struct HealthMonitorExecutor {
    roster: Vec<String>,
}

impl HealthMonitorExecutor {
    /// A monitor that checks the agents at `roster`, and no others.
    #[must_use]
    pub fn new(roster: impl IntoIterator<Item = String>) -> Self {
        Self {
            roster: roster.into_iter().collect(),
        }
    }

    /// Splits `asked` into the URLs to check and the ones to refuse: those off
    /// the roster, and those past [`MAX_CHECKS`].
    fn plan<'a>(&self, asked: &'a [String]) -> (Vec<&'a str>, Vec<(&'a str, &'static str)>) {
        let mut check = Vec::new();
        let mut refuse = Vec::new();
        for url in asked {
            if check.len() >= MAX_CHECKS {
                refuse.push((url.as_str(), "REFUSED (too many URLs in one request)"));
            } else if self.roster.iter().any(|known| known == url) {
                check.push(url.as_str());
            } else {
                refuse.push((url.as_str(), "REFUSED (not on this team's roster)"));
            }
        }
        (check, refuse)
    }
}

impl AgentExecutor for HealthMonitorExecutor {
    fn execute<'a>(
        &'a self,
        ctx: &'a RequestContext,
        queue: &'a dyn EventQueueWriter,
    ) -> Pin<Box<dyn Future<Output = A2aResult<()>> + Send + 'a>> {
        boxed_future(async move {
            let emit = EventEmitter::new(ctx, queue);

            emit.status(TaskState::Working).await?;

            // The message should contain agent URLs as JSON data part.
            let agent_urls: Vec<String> = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Data(data) => {
                        serde_json::from_value::<Vec<String>>(data.clone()).ok()
                    }
                    _ => None,
                })
                .unwrap_or_default();

            let mut results = Vec::new();

            let (check, refused) = self.plan(&agent_urls);
            for url in check {
                let status = check_agent_health(url).await;
                results.push(format!("{url}: {status}"));
            }
            for (url, why) in refused {
                results.push(format!("{url}: {why}"));
            }

            // Also check via text part (for simple "ping" messages).
            let text_input = ctx
                .message
                .parts
                .iter()
                .find_map(|p| match &p.content {
                    PartContent::Text(text) => Some(text.clone()),
                    _ => None,
                })
                .unwrap_or_default();

            if !text_input.is_empty() {
                results.push(format!("Received check request: {text_input}"));
            }

            let report = if results.is_empty() {
                "No agents to check — health monitor standing by.".to_owned()
            } else {
                format!("Health Report:\n{}", results.join("\n"))
            };

            emit.artifact("health-report", vec![Part::text(&report)], None, Some(true))
                .await?;

            emit.status(TaskState::Completed).await?;

            Ok(())
        })
    }
}

/// Checks an individual agent's health by resolving its card and listing tasks.
async fn check_agent_health(url: &str) -> &'static str {
    let list_params = ListTasksParams::new().with_page_size(1);

    match resolve_agent_card(url).await {
        Ok(card) => {
            let binding = card
                .supported_interfaces
                .first()
                .map(|i| i.protocol_binding.as_str())
                .unwrap_or("JSONRPC");
            match ClientBuilder::new(url)
                .with_protocol_binding(binding)
                .build()
            {
                Ok(client) => match client.list_tasks(list_params).await {
                    Ok(_) => "HEALTHY",
                    Err(_) => "DEGRADED",
                },
                Err(_) => "UNREACHABLE",
            }
        }
        Err(_) => {
            // Fall back to default JSON-RPC if agent card is unavailable.
            match ClientBuilder::new(url).build() {
                Ok(client) => match client.list_tasks(list_params).await {
                    Ok(_) => "HEALTHY",
                    Err(_) => "DEGRADED",
                },
                Err(_) => "UNREACHABLE",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor() -> HealthMonitorExecutor {
        HealthMonitorExecutor::new([
            "http://127.0.0.1:1".to_owned(),
            "http://127.0.0.1:2".to_owned(),
        ])
    }

    /// Only the roster is checked; the rest is refused by name.
    #[test]
    fn urls_off_the_roster_are_refused_and_the_roster_is_checked() {
        let asked = vec![
            "http://127.0.0.1:1".to_owned(),
            "http://169.254.169.254/latest/meta-data/".to_owned(),
            "http://127.0.0.1:2".to_owned(),
        ];
        let (check, refuse) = monitor().plan(&asked);
        assert_eq!(check, ["http://127.0.0.1:1", "http://127.0.0.1:2"]);
        assert_eq!(
            refuse,
            [(
                "http://169.254.169.254/latest/meta-data/",
                "REFUSED (not on this team's roster)"
            )]
        );
    }

    /// A roster URL written differently (another port, a path) is a different
    /// URL and is refused: no prefix or host-only matching to get around.
    #[test]
    fn a_near_miss_of_a_roster_url_is_refused() {
        let asked = vec![
            "http://127.0.0.1:1/".to_owned(),
            "http://127.0.0.1:11".to_owned(),
            "http://127.0.0.1:1@evil.example".to_owned(),
        ];
        let (check, refuse) = monitor().plan(&asked);
        assert_eq!(check, Vec::<&str>::new());
        assert_eq!(refuse.len(), 3);
    }

    /// One request cannot ask for more than [`MAX_CHECKS`] checks, even if
    /// every URL is on the roster.
    #[test]
    fn a_request_is_capped() {
        let roster: Vec<String> = (0..40)
            .map(|i| format!("http://127.0.0.1:{}", 2000 + i))
            .collect();
        let monitor = HealthMonitorExecutor::new(roster.clone());
        let (check, refuse) = monitor.plan(&roster);
        assert_eq!(check.len(), MAX_CHECKS);
        assert_eq!(refuse.len(), 40 - MAX_CHECKS);
        assert!(refuse.iter().all(|(_, why)| why.contains("too many")));
    }
}
