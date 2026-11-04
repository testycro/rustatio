//! Reverse proxy to the embedded Python admin daemon (`rustatio_daemon.py`).
//!
//! The daemon runs its own small Flask server (`ADMIN_PORT`, default `PORT + 1`).
//! Instead of exposing that second port (and a second login), the UI reaches it
//! through `/api/daemon-admin/*`. Because this router lives behind the regular
//! `auth_middleware`, the existing `AUTH_TOKEN` protects it, and the proxy
//! forwards the very same token to the daemon (which reads the same variable).

use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    response::Response,
    routing::any,
    Router,
};

use crate::api::{common::ApiError, middleware::get_auth_token, ServerState};

/// Path prefix of this proxy inside the `/api` router (the `/api` part is
/// already stripped by `nest`).
const PROXY_PREFIX: &str = "/daemon-admin";
const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
const PROXY_TIMEOUT: Duration = Duration::from_secs(30);

/// Base URL of the daemon: `DAEMON_URL` if set, otherwise `127.0.0.1:ADMIN_PORT`
/// (`ADMIN_PORT` defaults to `PORT + 1`, same rule as the daemon itself).
fn daemon_base_url() -> &'static str {
    static BASE: OnceLock<String> = OnceLock::new();
    BASE.get_or_init(|| {
        if let Some(url) = std::env::var("DAEMON_URL")
            .ok()
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty())
        {
            return url;
        }

        let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
        let admin_port: u16 = std::env::var("ADMIN_PORT")
            .ok()
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or_else(|| port.saturating_add(1));

        format!("http://127.0.0.1:{admin_port}")
    })
    .as_str()
}

fn daemon_enabled() -> bool {
    std::env::var("DAEMON_ENABLED")
        .map_or(true, |v| !matches!(v.trim().to_ascii_lowercase().as_str(), "false" | "0" | "no"))
}

fn http_client() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(PROXY_TIMEOUT)
                // The daemon is local: never go through HTTP(S)_PROXY, and pass
                // redirects (e.g. login) back to the caller untouched.
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok()
        })
        .as_ref()
}

/// Maps `/daemon-admin/<rest>?<query>` to `<base>/<rest>?<query>`.
fn target_url(base: &str, path: &str, query: Option<&str>) -> String {
    let rest = path.strip_prefix(PROXY_PREFIX).unwrap_or(path);
    let rest = if rest.is_empty() { "/" } else { rest };

    match query {
        Some(q) if !q.is_empty() => format!("{base}{rest}?{q}"),
        _ => format!("{base}{rest}"),
    }
}

async fn proxy_to_daemon(request: Request) -> Response {
    if !daemon_enabled() {
        return ApiError::response(
            StatusCode::SERVICE_UNAVAILABLE,
            "Admin daemon is disabled (DAEMON_ENABLED=false).",
        );
    }

    let Some(client) = http_client() else {
        return ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, "Daemon proxy unavailable.");
    };

    let (parts, body) = request.into_parts();
    let url = target_url(daemon_base_url(), parts.uri.path(), parts.uri.query());

    let Ok(body_bytes) = to_bytes(body, MAX_BODY_BYTES).await else {
        return ApiError::response(StatusCode::PAYLOAD_TOO_LARGE, "Request body too large.");
    };

    let mut upstream = client.request(parts.method.clone(), url.as_str());

    if let Some(content_type) = parts.headers.get(header::CONTENT_TYPE) {
        upstream = upstream.header(header::CONTENT_TYPE, content_type.clone());
    }
    // The caller is already authenticated by the API middleware; the daemon
    // checks the same AUTH_TOKEN, so forward it as a bearer token.
    if let Some(token) = get_auth_token() {
        upstream = upstream.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if !body_bytes.is_empty() {
        upstream = upstream.body(body_bytes);
    }

    let response = match upstream.send().await {
        Ok(response) => response,
        Err(e) => {
            let (status, message) = if e.is_timeout() {
                (StatusCode::GATEWAY_TIMEOUT, "Admin daemon did not answer in time.")
            } else {
                (
                    StatusCode::BAD_GATEWAY,
                    "Admin daemon is unreachable (not started, or still starting).",
                )
            };
            tracing::debug!("Daemon proxy error for {}: {}", parts.uri.path(), e);
            return ApiError::response(status, message);
        }
    };

    let status = response.status();
    let content_type: Option<HeaderValue> = response.headers().get(header::CONTENT_TYPE).cloned();

    let Ok(bytes) = response.bytes().await else {
        return ApiError::response(
            StatusCode::BAD_GATEWAY,
            "Failed to read the admin daemon response.",
        );
    };

    let mut builder = Response::builder().status(status).header(header::CACHE_CONTROL, "no-store");
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }

    builder.body(Body::from(bytes)).unwrap_or_else(|_| {
        ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to build proxy response.")
    })
}

pub fn router() -> Router<ServerState> {
    Router::new()
        .route("/daemon-admin", any(proxy_to_daemon))
        .route("/daemon-admin/{*path}", any(proxy_to_daemon))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "http://127.0.0.1:8081";

    #[test]
    fn root_maps_to_daemon_index() {
        assert_eq!(target_url(BASE, "/daemon-admin", None), "http://127.0.0.1:8081/");
        assert_eq!(target_url(BASE, "/daemon-admin/", None), "http://127.0.0.1:8081/");
    }

    #[test]
    fn api_paths_are_forwarded_verbatim() {
        assert_eq!(
            target_url(BASE, "/daemon-admin/api/status", None),
            "http://127.0.0.1:8081/api/status"
        );
        assert_eq!(
            target_url(BASE, "/daemon-admin/api/watcher/start", None),
            "http://127.0.0.1:8081/api/watcher/start"
        );
    }

    #[test]
    fn query_string_is_kept() {
        assert_eq!(
            target_url(BASE, "/daemon-admin/api/logs", Some("lines=200")),
            "http://127.0.0.1:8081/api/logs?lines=200"
        );
        assert_eq!(
            target_url(BASE, "/daemon-admin", Some("embed=1")),
            "http://127.0.0.1:8081/?embed=1"
        );
    }
}
