use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
};
use tokio::fs;

use crate::server::state::AppState;

pub async fn handler(State(state): State<AppState>) -> impl IntoResponse {
    let favicon_path = state.theme_path.join("assets/favicon.ico");
    match fs::read(&favicon_path).await {
        Ok(data) => (
            StatusCode::OK,
            [("content-type", "image/x-icon")],
            data,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
