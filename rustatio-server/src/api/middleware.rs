//! Authentication middleware for API token validation.

use axum::{
    extract::Request,
    http::{header::AUTHORIZATION, HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::OnceLock;

static AUTH_TOKEN: OnceLock<Option<String>> = OnceLock::new();

pub fn get_auth_token() -> Option<&'static str> {
    AUTH_TOKEN.get_or_init(|| std::env::var("AUTH_TOKEN").ok().filter(|s| !s.is_empty())).as_deref()
}

pub fn is_auth_enabled() -> bool {
    get_auth_token().is_some()
}

#[derive(Serialize)]
struct AuthError {
    success: bool,
    error: String,
    auth_required: bool,
}

impl AuthError {
    fn unauthorized() -> Response {
        (
            StatusCode::UNAUTHORIZED,
            Json(Self {
                success: false,
                error: "Authentication required. Provide Authorization: Bearer <token> header."
                    .into(),
                auth_required: true,
            }),
        )
            .into_response()
    }

    fn cross_site() -> Response {
        (
            StatusCode::FORBIDDEN,
            Json(Self {
                success: false,
                error: "Cross-site request refused (set CORS_ALLOWED_ORIGINS to allow it).".into(),
                auth_required: false,
            }),
        )
            .into_response()
    }

    fn forbidden() -> Response {
        (
            StatusCode::FORBIDDEN,
            Json(Self {
                success: false,
                error: "Invalid authentication token.".into(),
                auth_required: true,
            }),
        )
            .into_response()
    }
}

/// Validates Authorization header or query token against `AUTH_TOKEN`.
/// If `AUTH_TOKEN` is not set, all requests are allowed.
pub async fn auth_middleware(request: Request, next: Next) -> Response {
    if is_blocked_cross_site(request.headers()) {
        return AuthError::cross_site();
    }

    let Some(expected_token) = get_auth_token() else {
        return next.run(request).await;
    };

    let auth_header = request.headers().get(AUTHORIZATION).and_then(|value| value.to_str().ok());

    if let Some(header) = auth_header {
        if let Some(provided_token) = header.strip_prefix("Bearer ") {
            if constant_time_eq(provided_token.as_bytes(), expected_token.as_bytes()) {
                return next.run(request).await;
            }
            return AuthError::forbidden();
        }
    }

    if allows_query_token(request.method(), request.uri().path()) {
        if let Some(query) = request.uri().query() {
            for param in query.split('&') {
                if let Some(token_value) = param.strip_prefix("token=") {
                    // URL decode the token
                    let decoded_token = urlencoding::decode(token_value).unwrap_or_default();
                    if constant_time_eq(decoded_token.as_bytes(), expected_token.as_bytes()) {
                        return next.run(request).await;
                    }
                    return AuthError::forbidden();
                }
            }
        }
    }

    AuthError::unauthorized()
}

/// Blocks `Sec-Fetch-Site: cross-site` requests unless `CORS_ALLOWED_ORIGINS` is set.
fn is_blocked_cross_site(headers: &HeaderMap) -> bool {
    static CORS_OPEN: OnceLock<bool> = OnceLock::new();
    let cors_open = *CORS_OPEN.get_or_init(|| {
        std::env::var("CORS_ALLOWED_ORIGINS").is_ok_and(|origins| !origins.trim().is_empty())
    });
    !cors_open && is_cross_site(headers)
}

fn is_cross_site(headers: &HeaderMap) -> bool {
    headers
        .get("sec-fetch-site")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.eq_ignore_ascii_case("cross-site"))
}

/// Query-string tokens are only accepted for the SSE endpoints, because
/// `EventSource` cannot set an `Authorization` header. All other routes must
/// use the header so the secret is never placed in a URL (and therefore never
/// written to access logs, reverse proxies, or browser history).
fn allows_query_token(method: &Method, path: &str) -> bool {
    *method == Method::GET
        && matches!(
            path,
            "/logs" | "/events" | "/stream" | "/api/logs" | "/api/events" | "/api/stream"
        )
}

fn constant_time_eq(expected: &[u8], actual: &[u8]) -> bool {
    if expected.len() != actual.len() {
        return false;
    }

    let mut result = 0u8;
    for (x, y) in expected.iter().zip(actual.iter()) {
        result |= x ^ y;
    }
    result == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_cross_site_detection() {
        let mut headers = HeaderMap::new();
        assert!(!is_cross_site(&headers));
        headers.insert("sec-fetch-site", "same-origin".parse().unwrap_or_else(|_| unreachable!()));
        assert!(!is_cross_site(&headers));
        headers.insert("sec-fetch-site", "cross-site".parse().unwrap_or_else(|_| unreachable!()));
        assert!(is_cross_site(&headers));
    }

    #[test]
    fn test_query_token_allowed_only_for_sse_get() {
        assert!(allows_query_token(&Method::GET, "/logs"));
        assert!(allows_query_token(&Method::GET, "/events"));
        assert!(allows_query_token(&Method::GET, "/stream"));
        assert!(allows_query_token(&Method::GET, "/api/logs"));
        assert!(allows_query_token(&Method::GET, "/api/events"));
        assert!(allows_query_token(&Method::GET, "/api/stream"));

        assert!(!allows_query_token(&Method::POST, "/logs"));
        assert!(!allows_query_token(&Method::POST, "/stream"));
        assert!(!allows_query_token(&Method::GET, "/instances"));
        assert!(!allows_query_token(&Method::GET, "/api/instances"));
        assert!(!allows_query_token(&Method::GET, "/auth/verify"));
    }
}
