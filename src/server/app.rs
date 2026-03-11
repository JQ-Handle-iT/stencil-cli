use axum::{
    extract::{Request, State},
    http::StatusCode,
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
        // Single catch-all for everything else
        .fallback(catch_all_handler)
        .with_state(state)
}
