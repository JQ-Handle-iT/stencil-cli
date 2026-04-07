use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use tower_http::services::ServeDir;

use super::routes;
use super::state::AppState;

/// Single catch-all that dispatches based on path prefix
async fn catch_all_handler(
    State(state): State<AppState>,
    req: Request,
) -> Response {
    let path = req.uri().path().to_string();

    if path.starts_with("/stencil/") {
        return routes::theme_assets::stencil_handler_from_request(state, req).await;
    }
    if path.starts_with("/internalapi/") {
        return routes::proxy::proxy_handler(state, req).await;
    }
    if path.starts_with("/api/storefront/") {
        return routes::proxy::proxy_handler(state, req).await;
    }

    if path.ends_with(".css") {
        let file_name = path.trim_start_matches('/');
        return routes::theme_assets::css_handler_public(state, file_name).await;
    }
    if path.ends_with(".js") || path.ends_with(".svg") || path.ends_with(".gif") || path.ends_with(".jpg") || path.ends_with(".png") || path.ends_with(".woff") || path.ends_with(".woff2") || path.ends_with(".ttf") || path.ends_with(".eot") || path.ends_with(".ico") {
        let file_name = path.trim_start_matches('/');
        return routes::theme_assets::asset_handler_public(state, file_name).await;
    }

    // Default: renderer
    routes::renderer::handler_from_request(state, req).await
}

/// Tower middleware that records every HTTP request into `AppState::stats`
/// when the TUI is active. Zero-cost (no-op) when `stats` is None.
async fn request_stats_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let start = std::time::Instant::now();
    let method = req.method().to_string();
    let path = req.uri().path().to_string();

    let response = next.run(req).await;

    if let Some(ref stats) = state.stats {
        if let Ok(mut s) = stats.lock() {
            s.record_request(&method, &path, response.status().as_u16(), start.elapsed());
        }
    }

    response
}

pub fn build_router(state: AppState) -> Router {
    let assets_dir = state.theme_path.join("assets");

    Router::new()
        .route("/__live_reload", get(routes::static_assets::live_reload_ws))
        .route("/favicon.ico", get(routes::favicon::handler))
        .route(
            "/graphql",
            get(routes::proxy::graphql).post(routes::proxy::graphql),
        )
        .nest_service("/assets", ServeDir::new(assets_dir))
        .fallback(catch_all_handler)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            request_stats_middleware,
        ))
        .with_state(state)
}
