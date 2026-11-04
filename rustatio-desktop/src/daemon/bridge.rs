//! Loopback HTTP bridge between the embedded Python daemon and the app.
//!
//! The daemon talks to Rustatio through the same REST API as in Docker. The desktop app has
//! no HTTP server, so this small one answers the handful of routes the daemon uses and runs
//! the matching desktop commands. It only listens on 127.0.0.1, on a random port, and every
//! request needs the per-launch Bearer token (the other apps of the machine cannot use it).

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use rustatio_core::FakerConfig;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Listener, Manager};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, oneshot};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

use super::support::{bearer_token, constant_time_eq, parse_ids};
use crate::commands;
use crate::state::AppState;

#[derive(Clone)]
struct BridgeState {
    app: AppHandle,
    token: Arc<str>,
    logs: broadcast::Sender<String>,
}

/// A running bridge.
pub struct BridgeHandle {
    pub port: u16,
    shutdown: Option<oneshot::Sender<()>>,
}

impl BridgeHandle {
    pub fn shutdown(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

/// Starts the bridge on a free loopback port.
pub async fn start(app: AppHandle, token: &str) -> std::io::Result<BridgeHandle> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();

    // The app already emits every log line to the UI as a "log-event": the daemon's log
    // watcher gets the same events through the /api/logs stream, like on the server.
    let (logs, _) = broadcast::channel(512);
    let forward = logs.clone();
    app.listen_any("log-event", move |event| {
        let _ = forward.send(event.payload().to_owned());
    });

    let state = BridgeState { app, token: Arc::from(token), logs };
    let (shutdown, stop) = oneshot::channel::<()>();

    tauri::async_runtime::spawn(async move {
        let served = axum::serve(listener, router(state))
            .with_graceful_shutdown(async move {
                let _ = stop.await;
            })
            .await;

        if let Err(e) = served {
            log::error!("Daemon bridge stopped: {e}");
        }
    });

    log::info!("Daemon bridge listening on 127.0.0.1:{port}");
    Ok(BridgeHandle { port, shutdown: Some(shutdown) })
}

fn router(state: BridgeState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/instances", get(list_instances))
        .route("/api/instances/{id}", axum::routing::delete(delete_instance))
        .route("/api/instances/{id}/config", patch(update_config))
        .route("/api/faker/{id}/start", post(start_faker))
        .route("/api/faker/{id}/stop", post(stop_faker))
        .route("/api/grid/pause", post(grid_pause))
        .route("/api/grid/resume", post(grid_resume))
        .route("/api/grid/tag", post(grid_tag))
        .route("/api/watch/files", get(list_watch_files).delete(delete_watch_file))
        .route("/api/logs", get(logs))
        .layer(middleware::from_fn_with_state(state.clone(), require_token))
        .with_state(state)
}

async fn require_token(State(state): State<BridgeState>, request: Request, next: Next) -> Response {
    let authorized = {
        let header = request.headers().get(header::AUTHORIZATION).and_then(|v| v.to_str().ok());
        bearer_token(header)
            .is_some_and(|token| constant_time_eq(state.token.as_bytes(), token.as_bytes()))
    };

    if authorized {
        next.run(request).await
    } else {
        fail(StatusCode::UNAUTHORIZED, "Unauthorized")
    }
}

// The daemon expects the same envelope as the server API: {"success": true, "data": ...}
fn ok<T: Serialize>(data: T) -> Response {
    Json(json!({ "success": true, "data": data })).into_response()
}

fn fail(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(json!({ "success": false, "error": message.into() }))).into_response()
}

fn respond<T: Serialize>(result: Result<T, String>) -> Response {
    match result {
        Ok(data) => ok(data),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// Grid commands report failures per instance: any failure is an error for the daemon,
/// which always acts on one instance at a time.
fn grid_result(result: Result<commands::GridActionResponse, String>) -> Response {
    let response = match result {
        Ok(response) => response,
        Err(e) => return fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    };

    let value = serde_json::to_value(&response).unwrap_or(Value::Null);
    let first_error = value["failed"]
        .as_array()
        .and_then(|failed| failed.first())
        .map(|failure| failure["error"].as_str().unwrap_or("Action failed").to_owned());

    first_error.map_or_else(|| ok(value), |error| fail(StatusCode::INTERNAL_SERVER_ERROR, error))
}

fn parse_id(raw: &str) -> Result<u32, String> {
    raw.trim().parse().map_err(|_| format!("Invalid instance id '{raw}'"))
}

/// The daemon sends ids as strings ("3"), accept plain numbers too.
fn ids_of(values: &[Value]) -> Vec<u32> {
    let ids: Vec<String> = values
        .iter()
        .filter_map(|value| {
            value.as_str().map(str::to_owned).or_else(|| value.as_u64().map(|n| n.to_string()))
        })
        .collect();
    parse_ids(&ids)
}

#[allow(clippy::unused_async)]
async fn health() -> &'static str {
    "OK"
}

async fn list_instances(State(bridge): State<BridgeState>) -> Response {
    respond(commands::list_instances(bridge.app.state::<AppState>()).await)
}

#[derive(Deserialize)]
struct DeleteQuery {
    force: Option<bool>,
}

async fn delete_instance(
    State(bridge): State<BridgeState>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Response {
    let id = match parse_id(&id) {
        Ok(id) => id,
        Err(message) => return fail(StatusCode::BAD_REQUEST, message),
    };

    let result = commands::delete_instance(
        id,
        query.force,
        bridge.app.state::<AppState>(),
        bridge.app.clone(),
    )
    .await;
    respond(result.map(|()| json!({})))
}

async fn update_config(
    State(bridge): State<BridgeState>,
    Path(id): Path<String>,
    Json(config): Json<FakerConfig>,
) -> Response {
    let id = match parse_id(&id) {
        Ok(id) => id,
        Err(message) => return fail(StatusCode::BAD_REQUEST, message),
    };

    let result = commands::update_instance_config(id, config, bridge.app.state::<AppState>()).await;
    respond(result.map(|()| json!({})))
}

/// The daemon only starts instances that already exist: the torrent it sends is ignored, the
/// one the app holds is used. The config it sends is applied first, like the server does.
async fn start_faker(
    State(bridge): State<BridgeState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let id = match parse_id(&id) {
        Ok(id) => id,
        Err(message) => return fail(StatusCode::BAD_REQUEST, message),
    };
    let state = bridge.app.state::<AppState>();

    if let Some(config) = body.get("config").cloned() {
        let config: FakerConfig = match serde_json::from_value(config) {
            Ok(config) => config,
            Err(e) => return fail(StatusCode::BAD_REQUEST, format!("Invalid config: {e}")),
        };

        if let Err(e) = state.apply_instance_config(id, config).await {
            return fail(StatusCode::INTERNAL_SERVER_ERROR, e);
        }
    }

    grid_result(commands::grid_start(vec![id], state, bridge.app.clone()).await)
}

async fn stop_faker(State(bridge): State<BridgeState>, Path(id): Path<String>) -> Response {
    let id = match parse_id(&id) {
        Ok(id) => id,
        Err(message) => return fail(StatusCode::BAD_REQUEST, message),
    };

    let result = commands::stop_faker(id, bridge.app.state::<AppState>(), bridge.app.clone()).await;
    respond(result.map(|()| json!({})))
}

#[derive(Deserialize)]
struct IdsBody {
    #[serde(default)]
    ids: Vec<Value>,
}

async fn grid_pause(State(bridge): State<BridgeState>, Json(body): Json<IdsBody>) -> Response {
    let ids = ids_of(&body.ids);
    grid_result(commands::grid_pause(ids, bridge.app.state::<AppState>(), bridge.app.clone()).await)
}

async fn grid_resume(State(bridge): State<BridgeState>, Json(body): Json<IdsBody>) -> Response {
    let ids = ids_of(&body.ids);
    grid_result(
        commands::grid_resume(ids, bridge.app.state::<AppState>(), bridge.app.clone()).await,
    )
}

#[derive(Deserialize)]
struct TagBody {
    #[serde(default)]
    ids: Vec<Value>,
    #[serde(default)]
    add_tags: Vec<String>,
    #[serde(default)]
    remove_tags: Vec<String>,
}

async fn grid_tag(State(bridge): State<BridgeState>, Json(body): Json<TagBody>) -> Response {
    let ids = ids_of(&body.ids);
    let result =
        commands::grid_tag(ids, body.add_tags, body.remove_tags, bridge.app.state::<AppState>())
            .await;
    respond(result.map(|updated| json!({ "updated": updated })))
}

async fn list_watch_files(State(bridge): State<BridgeState>) -> Response {
    respond(commands::list_watch_files(bridge.app.state::<AppState>()).await)
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn delete_watch_file(
    State(bridge): State<BridgeState>,
    Query(query): Query<PathQuery>,
) -> Response {
    let result = commands::delete_watch_file(query.path, bridge.app.state::<AppState>()).await;
    respond(result.map(|()| json!({})))
}

/// Server-sent events: one `data: {"timestamp", "level", "message"}` per app log line.
#[allow(clippy::unused_async)]
async fn logs(
    State(bridge): State<BridgeState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(bridge.logs.subscribe())
        .filter_map(Result::ok)
        .map(|payload| Ok(Event::default().data(payload)));

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_accept_strings_and_numbers() {
        let values = [json!("3"), json!(7), json!(" 12 "), json!("x"), json!(null), json!(-1)];
        assert_eq!(ids_of(&values), vec![3, 7, 12]);
        assert_eq!(ids_of(&[]), Vec::<u32>::new());
    }

    #[test]
    fn instance_ids_must_be_numbers() {
        assert_eq!(parse_id(" 5 ").ok(), Some(5));
        assert!(parse_id("abc").is_err());
        assert!(parse_id("").is_err());
    }
}
