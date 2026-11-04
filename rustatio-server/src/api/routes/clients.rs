//! `BitTorrent` client information endpoints.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
    routing::{delete, get, put},
    Json, Router,
};
use rustatio_core::{ClientDefinition, ClientType};
use serde::Serialize;
use utoipa::ToSchema;

use crate::api::{
    common::{ApiError, ApiSuccess, EmptyData},
    ServerState,
};

#[derive(Serialize, ToSchema)]
pub struct ClientInfoResponse {
    pub id: String,
    pub name: String,
    pub default_version: String,
    pub versions: Vec<String>,
    pub default_port: u16,
}

impl From<rustatio_core::ClientInfo> for ClientInfoResponse {
    fn from(info: rustatio_core::ClientInfo) -> Self {
        Self {
            id: info.id,
            name: info.name,
            default_version: info.default_version,
            versions: info.versions,
            default_port: info.default_port,
        }
    }
}

#[utoipa::path(
    get,
    path = "/clients",
    tag = "clients",
    summary = "Get available client types",
    description = "Returns a list of BitTorrent client type IDs that can be emulated.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "List of client type IDs", body = ApiSuccess<Vec<String>>),
        (status = 401, description = "Unauthorized", body = crate::api::common::ApiError)
    )
)]
pub async fn get_client_types() -> Response {
    ApiSuccess::response(ClientType::all_ids())
}

#[utoipa::path(
    get,
    path = "/clients/info",
    tag = "clients",
    summary = "Get detailed client information",
    description = "Returns detailed information about all available BitTorrent clients including versions and default ports.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "List of client information", body = ApiSuccess<Vec<ClientInfoResponse>>),
        (status = 401, description = "Unauthorized", body = crate::api::common::ApiError)
    )
)]
pub async fn get_client_infos() -> Response {
    let infos: Vec<ClientInfoResponse> =
        ClientType::all_infos().into_iter().map(std::convert::Into::into).collect();
    ApiSuccess::response(infos)
}

#[utoipa::path(
    get,
    path = "/clients/definitions",
    tag = "clients",
    summary = "List client definitions",
    description = "Returns the full definition of every emulated client (peer ID code, user agent, versions...). These are stored in the state file next to the default configuration.",
    security(("bearer_auth" = [])),
    responses(
        (status = 200, description = "Client definitions", body = ApiSuccess<Vec<Object>>),
        (status = 401, description = "Unauthorized", body = ApiError)
    )
)]
pub async fn get_client_definitions() -> Response {
    ApiSuccess::response(rustatio_core::client_definitions())
}

#[utoipa::path(
    put,
    path = "/clients/definitions/{id}",
    tag = "clients",
    summary = "Create or update a client",
    description = "Adds a new emulated client, or replaces the one with this id. Running instances keep their current peer ID until their configuration is updated.",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "Client ID (lowercase letters, digits, '-' or '_')")),
    request_body(content = Object, description = "Client definition"),
    responses(
        (status = 200, description = "Client saved", body = ApiSuccess<EmptyData>),
        (status = 400, description = "Invalid client definition", body = ApiError),
        (status = 401, description = "Unauthorized", body = ApiError),
        (status = 500, description = "Failed to save state", body = ApiError)
    )
)]
pub async fn upsert_client_definition(
    State(state): State<ServerState>,
    Path(id): Path<String>,
    Json(mut definition): Json<ClientDefinition>,
) -> Response {
    definition.id = id;

    if let Err(e) = rustatio_core::upsert_client_definition(definition) {
        return ApiError::response(StatusCode::BAD_REQUEST, e);
    }

    match state.app.save_state().await {
        Ok(()) => ApiSuccess::response(EmptyData {}),
        Err(e) => ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[utoipa::path(
    delete,
    path = "/clients/definitions/{id}",
    tag = "clients",
    summary = "Delete a client",
    description = "Removes an emulated client. Instances still using it fall back to the default client. The last remaining client cannot be deleted.",
    security(("bearer_auth" = [])),
    params(("id" = String, Path, description = "Client ID")),
    responses(
        (status = 200, description = "Client deleted", body = ApiSuccess<EmptyData>),
        (status = 400, description = "The last client cannot be deleted", body = ApiError),
        (status = 401, description = "Unauthorized", body = ApiError),
        (status = 404, description = "Client not found", body = ApiError),
        (status = 500, description = "Failed to save state", body = ApiError)
    )
)]
pub async fn delete_client_definition(
    State(state): State<ServerState>,
    Path(id): Path<String>,
) -> Response {
    if rustatio_core::client_definition(&id).is_none() {
        return ApiError::response(StatusCode::NOT_FOUND, format!("client '{id}' not found"));
    }

    if let Err(e) = rustatio_core::remove_client_definition(&id) {
        return ApiError::response(StatusCode::BAD_REQUEST, e);
    }

    match state.app.save_state().await {
        Ok(()) => ApiSuccess::response(EmptyData {}),
        Err(e) => ApiError::response(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

pub fn router() -> Router<ServerState> {
    Router::new()
        .route("/clients", get(get_client_types))
        .route("/clients/info", get(get_client_infos))
        .route("/clients/definitions", get(get_client_definitions))
        .route("/clients/definitions/{id}", put(upsert_client_definition))
        .route("/clients/definitions/{id}", delete(delete_client_definition))
}
