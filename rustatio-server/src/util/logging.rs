use crate::services::LogEvent;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_log::NormalizeEvent;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// HTTP client/server internals. They reach us through the `log` bridge (target "log"), so
/// without this check they would flow into the stream read by the admin daemon, and every
/// call made by the daemon view (proxy to the daemon) would generate new lines.
const NOISY_TARGET_PREFIXES: [&str; 5] = ["hyper", "reqwest", "h2", "rustls", "tower_http"];

fn is_noisy_target(target: &str) -> bool {
    NOISY_TARGET_PREFIXES.iter().any(|prefix| target.starts_with(prefix))
}

pub struct BroadcastLayer {
    sender: Arc<broadcast::Sender<LogEvent>>,
}

impl BroadcastLayer {
    pub fn new(sender: broadcast::Sender<LogEvent>) -> Self {
        Self { sender: Arc::new(sender) }
    }
}

struct MessageVisitor {
    message: String,
}

impl MessageVisitor {
    const fn new() -> Self {
        Self { message: String::new() }
    }
}

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
            // Remove surrounding quotes if present
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}

impl<S: Subscriber> Layer<S> for BroadcastLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let target = event.metadata().target();

        // Forward logs from:
        // - "log" target (log crate events bridged via tracing-log, which includes rustatio_core)
        // - "rustatio_core" target (direct tracing events from rustatio_core)
        // Exclude other targets like tower_http, hyper, etc.
        if target != "log" && !target.starts_with("rustatio_core") {
            return;
        }

        // For bridged `log` events, the real origin (module path) is in the normalized metadata.
        let normalized = event.normalized_metadata();
        let origin = normalized.as_ref().unwrap_or_else(|| event.metadata()).target();
        if is_noisy_target(origin) {
            return;
        }

        // Extract the message
        let mut visitor = MessageVisitor::new();
        event.record(&mut visitor);

        if visitor.message.is_empty() {
            return;
        }

        // Convert level to string
        let level = match *event.metadata().level() {
            Level::ERROR => "error",
            Level::WARN => "warn",
            Level::INFO => "info",
            Level::DEBUG => "debug",
            Level::TRACE => "trace",
        };

        // Send to broadcast channel (ignore errors - no subscribers is fine)
        let _ = self.sender.send(LogEvent::new(level, visitor.message));
    }
}

#[cfg(test)]
mod tests {
    use super::is_noisy_target;

    #[test]
    fn http_internals_are_filtered() {
        for target in [
            "hyper",
            "hyper_util::client::legacy::connect::http",
            "hyper_util::client::legacy::client",
            "reqwest::connect",
            "h2::codec::framed_read",
            "rustls::client::hs",
            "tower_http::trace::on_response",
        ] {
            assert!(is_noisy_target(target), "{target} should be filtered");
        }
    }

    #[test]
    fn rustatio_logs_are_kept() {
        for target in ["rustatio_core::faker", "rustatio_core::tracker", "log", "rustatio_server"] {
            assert!(!is_noisy_target(target), "{target} should be kept");
        }
    }
}
