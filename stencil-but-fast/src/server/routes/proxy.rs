use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

use crate::server::state::AppState;

/// Proxy handler for /internalapi/* and /api/storefront/*
pub async fn internal_api(
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, StatusCode> {
    proxy_to_store(state, req).await
}

pub async fn storefront_api(
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, StatusCode> {
    proxy_to_store(state, req).await
}

/// Proxy handler for /graphql
pub async fn graphql(
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, StatusCode> {
    let store_url = &state.store_url;
    let method = req.method().clone();
    let uri = req.uri().clone();
    let original_headers = req.headers().clone();
    let body_bytes = axum::body::to_bytes(req.into_body(), 20 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let target_url = format!("{}/graphql", store_url.trim_end_matches('/'));

    let mut proxy_req = state
        .http_client
        .request(method, &target_url);

    // Forward headers
    for (name, value) in original_headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if matches!(name_str.as_str(), "host" | "connection" | "transfer-encoding") {
            continue;
        }
        proxy_req = proxy_req.header(name.clone(), value.clone());
    }

    // Add required headers for GraphQL
    let host = store_url
        .replace("https://", "")
        .replace("http://", "");
    proxy_req = proxy_req
        .header("origin", store_url.as_str())
        .header("host", host.as_str())
        .header("stencil-cli", state.cli_version.as_str())
        .header("x-auth-token", state.access_token.as_str());

    if !body_bytes.is_empty() {
        proxy_req = proxy_req.body(body_bytes);
    }

    send_proxy_response(proxy_req).await
}

/// Called from the catch-all handler for /internalapi/* and /api/storefront/*
pub async fn proxy_handler(state: AppState, req: Request) -> Response {
    match proxy_to_store(state, req).await {
        Ok(resp) => resp,
        Err(status) => status.into_response(),
    }
}

async fn proxy_to_store(state: AppState, req: Request) -> Result<Response, StatusCode> {
    let store_url = &state.store_url;
    let method = req.method().clone();
    let path_and_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let target_url = format!(
        "{}{}",
        store_url.trim_end_matches('/'),
        path_and_query
    );

    let original_headers = req.headers().clone();
    let body_bytes = axum::body::to_bytes(req.into_body(), 20 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut proxy_req = state
        .http_client
        .request(method, &target_url);

    // Forward headers
    for (name, value) in original_headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if matches!(name_str.as_str(), "host" | "connection" | "transfer-encoding") {
            continue;
        }
        proxy_req = proxy_req.header(name.clone(), value.clone());
    }

    proxy_req = proxy_req
        .header("stencil-cli", state.cli_version.as_str())
        .header("x-auth-token", state.access_token.as_str());

    if !body_bytes.is_empty() {
        proxy_req = proxy_req.body(body_bytes);
    }

    send_proxy_response(proxy_req).await
}

async fn send_proxy_response(proxy_req: reqwest::RequestBuilder) -> Result<Response, StatusCode> {
    let resp = proxy_req
        .send()
        .await
        .map_err(|e| {
            tracing::error!("Proxy request failed: {}", e);
            StatusCode::BAD_GATEWAY
        })?;

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let resp_headers = resp.headers().clone();
    let body = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut response = Response::builder().status(status);

    for (name, value) in resp_headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if matches!(name_str.as_str(), "transfer-encoding" | "content-length") {
            continue;
        }
        response = response.header(name, value);
    }

    response
        .body(Body::from(body))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
