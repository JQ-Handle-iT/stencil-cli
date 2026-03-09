use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use regex::Regex;
use tokio::fs;

use crate::server::state::AppState;
use crate::utils::UUID_REGEXP;

pub async fn stencil_handler_from_request(state: AppState, req: axum::extract::Request) -> Response {
    let path = req.uri().path().to_string();
    // Strip /stencil/ prefix
    let rest = path.strip_prefix("/stencil/").unwrap_or(&path);

    let segments: Vec<&str> = rest.split('/').collect();
    let mut asset_path_start = 0;
    
    // Look for recognizable asset directories
    for (i, segment) in segments.iter().enumerate() {
        if *segment == "css" || *segment == "dist" || *segment == "img" || *segment == "fonts" || *segment == "js" || *segment == "lib" {
            asset_path_start = i;
            break;
        }
    }
    
    // Fallback if not found: strip the first segment (version_id)
    if asset_path_start == 0 && segments.len() > 1 {
        asset_path_start = 1;
    }

    let asset_path = segments[asset_path_start..].join("/");

    if let Some(css_file) = asset_path.strip_prefix("css/") {
        return css_handler(&state, css_file).await;
    }

    asset_handler(&state, &asset_path).await
}

pub async fn css_handler_public(state: AppState, file_name: &str) -> Response {
    css_handler(&state, file_name).await
}

pub async fn asset_handler_public(state: AppState, file_name: &str) -> Response {
    let possible_paths = vec![
        state.theme_path.join("assets").join(file_name),
        state.theme_path.join("assets/dist").join(file_name),
        state.theme_path.join("assets/img").join(file_name),
        state.theme_path.join(file_name),
    ];

    for path in possible_paths {
        if let Ok(data) = fs::read(&path).await {
            let content_type = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            return (StatusCode::OK, [("content-type", content_type)], data).into_response();
        }
    }

    StatusCode::NOT_FOUND.into_response()
}

/// Strip the config UUID suffix from a CSS filename.
/// e.g. "theme-00000000-0000-0000-0000-000000000001" -> "theme"
fn get_original_file_name(file_name: &str) -> String {
    let pattern = format!(r"(.+)-{}", UUID_REGEXP);
    let re = Regex::new(&pattern).unwrap();
    match re.captures(file_name) {
        Some(caps) => caps.get(1).unwrap().as_str().to_string(),
        None => file_name.to_string(),
    }
}

async fn css_handler(state: &AppState, file_name: &str) -> Response {
    let file_name_without_ext = file_name.strip_suffix(".css").unwrap_or(file_name);
    let file_name_without_ext = get_original_file_name(file_name_without_ext);
    tracing::debug!("CSS request: raw={}, resolved={}", file_name, file_name_without_ext);

    // Try pre-compiled CSS
    let css_path = state
        .theme_path
        .join("assets/css")
        .join(format!("{}.css", file_name_without_ext));

    if let Ok(css) = fs::read_to_string(&css_path).await {
        return (StatusCode::OK, [("content-type", "text/css")], css).into_response();
    }

    // Try SCSS compilation with grass
    let scss_path = state
        .theme_path
        .join("assets/scss")
        .join(format!("{}.scss", file_name_without_ext));

    if let Ok(scss_content) = fs::read_to_string(&scss_path).await {
        let theme_config = state.theme_config.read().await;
        let settings = theme_config.get_settings();
        let preamble = build_scss_preamble(&settings);
        let full_scss = format!("{}\n{}", preamble, scss_content);

        match grass::from_string(
            full_scss,
            &grass::Options::default()
                .load_path(&state.theme_path.join("assets/scss"))
                .load_path(&state.theme_path.join("node_modules")),
        ) {
            Ok(css) => return (StatusCode::OK, [("content-type", "text/css")], css).into_response(),
            Err(e) => {
                tracing::error!("SCSS compilation error for {}: {}", file_name_without_ext, e);
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    }

    StatusCode::NOT_FOUND.into_response()
}

async fn asset_handler(state: &AppState, file_path_str: &str) -> Response {
    let file_path = state.theme_path.join("assets").join(file_path_str);

    let data = match fs::read(&file_path).await {
        Ok(d) => d,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let content_type = mime_guess::from_path(&file_path)
        .first_or_octet_stream()
        .to_string();

    (StatusCode::OK, [("content-type", content_type)], data).into_response()
}

fn build_scss_preamble(settings: &serde_json::Value) -> String {
    let mut preamble = String::new();
    if let Some(obj) = settings.as_object() {
        for (key, value) in obj {
            let scss_value = match value {
                serde_json::Value::String(s) => {
                    if s.trim().is_empty() {
                        "\"\"".to_string()
                    } else if s.starts_with('#') || s.ends_with("px") || s.ends_with("rem") || s.ends_with("em") || s.ends_with('%') {
                        s.clone()
                    } else {
                        // Enclose in quotes if it's a font family or text content that isn't a known unit
                        s.clone()
                    }
                },
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => continue,
            };
            preamble.push_str(&format!("${}: {};\n", key, scss_value));
        }
    }
    preamble
}
