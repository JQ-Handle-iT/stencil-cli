use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use regex::Regex;
use std::io;
use std::path::{Path, PathBuf};
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

    // Check CSS cache first — avoids expensive SCSS recompilation on every request
    {
        let cache = state.css_cache.read().await;
        if let Some(cached_css) = cache.get(file_name) {
            return (StatusCode::OK, [("content-type", "text/css")], cached_css.clone()).into_response();
        }
    }

    // Try pre-compiled CSS from disk
    let css_path = state
        .theme_path
        .join("assets/css")
        .join(format!("{}.css", file_name_without_ext));

    if let Ok(css) = fs::read_to_string(&css_path).await {
        // Cache the pre-compiled CSS too
        let mut cache = state.css_cache.write().await;
        cache.insert(file_name.to_string(), css.clone());
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

        // Generate SCSS function definitions that implement the stencil custom functions.
        let preamble = build_stencil_scss_preamble(&settings);
        let full_scss = format!("{}\n{}", preamble, scss_content);

        // Run SCSS compilation on a blocking thread to avoid blocking the async runtime
        let theme_path = state.theme_path.clone();
        let css_cache = state.css_cache.clone();
        let cache_key = file_name.to_string();
        let file_label = file_name_without_ext.to_string();

        match tokio::task::spawn_blocking(move || {
            grass::from_string(
                full_scss,
                &grass::Options::default()
                    .load_path(&theme_path.join("assets/scss"))
                    .load_path(&theme_path.join("node_modules")),
            )
        })
        .await
        {
            Ok(Ok(css)) => {
                // Cache the compiled CSS
                let mut cache = css_cache.write().await;
                cache.insert(cache_key, css.clone());
                return (StatusCode::OK, [("content-type", "text/css")], css).into_response();
            }
            Ok(Err(e)) => {
                tracing::error!("SCSS compilation error for {}: {}", file_label, e);
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            Err(e) => {
                tracing::error!("SCSS compilation task panicked for {}: {}", file_label, e);
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

/// Build an SCSS preamble that defines the stencil custom functions and a settings map.
///
/// This approach generates actual SCSS functions (stencilColor, stencilString, stencilNumber,
/// stencilImage, stencilFontFamily, stencilFontWeight) that look up values from a map at
/// Sass evaluation time. This correctly handles both literal and variable arguments, unlike
/// text replacement which can only handle literal string arguments.
fn build_stencil_scss_preamble(settings: &serde_json::Value) -> String {
    let mut preamble = String::new();

    // Build the settings map with properly typed SCSS values
    preamble.push_str("$__stencil-settings: (\n");
    if let Some(obj) = settings.as_object() {
        let entries: Vec<String> = obj
            .iter()
            .filter_map(|(key, value)| {
                let scss_val = json_to_scss_value(value)?;
                Some(format!("  \"{}\": {}", escape_scss_string(key), scss_val))
            })
            .collect();
        preamble.push_str(&entries.join(",\n"));
    }
    preamble.push_str("\n);\n\n");

    // Unit map for stencilNumber
    preamble.push_str(
        "$__stencil-units: (\n\
         \x20 \"px\": 1px, \"em\": 1em, \"rem\": 1rem, \"%\": 1%,\n\
         \x20 \"cm\": 1cm, \"mm\": 1mm, \"ch\": 1ch, \"pc\": 1pc,\n\
         \x20 \"in\": 1in, \"pt\": 1pt, \"ex\": 1ex, \"vw\": 1vw,\n\
         \x20 \"vh\": 1vh, \"vmin\": 1vmin, \"vmax\": 1vmax\n\
         );\n\n",
    );

    // Helper: __stencil-str-replace (needed for stencilImage before theme's str-replace is loaded)
    preamble.push_str(
        "@function __stencil-str-replace($string, $search, $replace: \"\") {\n\
         \x20 $index: str-index($string, $search);\n\
         \x20 @if $index {\n\
         \x20   @return str-slice($string, 1, $index - 1) + $replace + __stencil-str-replace(str-slice($string, $index + str-length($search)), $search, $replace);\n\
         \x20 }\n\
         \x20 @return $string;\n\
         }\n\n",
    );

    // stencilColor($name) → Sass Color or null
    preamble.push_str(
        "@function stencilColor($name) {\n\
         \x20 $val: map-get($__stencil-settings, $name);\n\
         \x20 @if $val == null { @return null; }\n\
         \x20 @if type-of($val) == \"color\" { @return $val; }\n\
         \x20 @return null;\n\
         }\n\n",
    );

    // stencilNumber($name, $unit: "px") → Sass Number with unit
    // Matches node-sass behavior: parseFloat(value) || 0, then apply unit
    preamble.push_str(
        "@function stencilNumber($name, $unit: \"px\") {\n\
         \x20 $val: map-get($__stencil-settings, $name);\n\
         \x20 $multiplier: map-get($__stencil-units, $unit);\n\
         \x20 @if $multiplier == null { $multiplier: 1px; }\n\
         \x20 @if $val == null { @return 0 * $multiplier; }\n\
         \x20 @if type-of($val) == \"number\" { @return $val * $multiplier; }\n\
         \x20 @return 0 * $multiplier;\n\
         }\n\n",
    );

    // stencilString($name) → Sass String or null
    preamble.push_str(
        "@function stencilString($name) {\n\
         \x20 $val: map-get($__stencil-settings, $name);\n\
         \x20 @if $val == null { @return null; }\n\
         \x20 @return $val;\n\
         }\n\n",
    );

    // stencilImage($image, $size) → URL string with size replaced, or null
    preamble.push_str(
        "@function stencilImage($image, $size) {\n\
         \x20 $img: map-get($__stencil-settings, $image);\n\
         \x20 $sz: map-get($__stencil-settings, $size);\n\
         \x20 @if $img == null or $sz == null { @return null; }\n\
         \x20 @if str-index($img, \"{:size}\") == null { @return null; }\n\
         \x20 @return __stencil-str-replace($img, \"{:size}\", $sz);\n\
         }\n\n",
    );

    // stencilFontFamily($name) → quoted font family name or null
    // Parses "Google_Open+Sans_400" → "Open Sans", or "Arial_400" → "Arial"
    preamble.push_str(
        "@function stencilFontFamily($name) {\n\
         \x20 $val: map-get($__stencil-settings, $name);\n\
         \x20 @if $val == null or $val == \"\" { @return null; }\n\
         \x20 $parsed: $val;\n\
         \x20 @if str-index($val, \"Google_\") == 1 {\n\
         \x20   $parsed: str-slice($val, 8);\n\
         \x20 }\n\
         \x20 $sep: str-index($parsed, \"_\");\n\
         \x20 $family: if($sep, str-slice($parsed, 1, $sep - 1), $parsed);\n\
         \x20 $family: __stencil-str-replace($family, \"+\", \" \");\n\
         \x20 @return quote($family);\n\
         }\n\n",
    );

    // stencilFontWeight($name) → font weight number or null
    // Parses "Google_Open+Sans_400" → 400, or "Arial_400" → 400
    preamble.push_str(
        "@function stencilFontWeight($name) {\n\
         \x20 $val: map-get($__stencil-settings, $name);\n\
         \x20 @if $val == null or $val == \"\" { @return null; }\n\
         \x20 $parsed: $val;\n\
         \x20 @if str-index($val, \"Google_\") == 1 {\n\
         \x20   $parsed: str-slice($val, 8);\n\
         \x20 }\n\
         \x20 $sep: str-index($parsed, \"_\");\n\
         \x20 @if $sep == null { @return null; }\n\
         \x20 $weight-str: str-slice($parsed, $sep + 1);\n\
         \x20 $comma: str-index($weight-str, \",\");\n\
         \x20 @if $comma { $weight-str: str-slice($weight-str, 1, $comma - 1); }\n\
         \x20 @return __stencil-to-number($weight-str);\n\
         }\n\n",
    );

    // Internal numeric parser for font weights (simple integer only)
    preamble.push_str(
        "@function __stencil-to-number($str) {\n\
         \x20 $digits: (\"0\": 0, \"1\": 1, \"2\": 2, \"3\": 3, \"4\": 4, \"5\": 5, \"6\": 6, \"7\": 7, \"8\": 8, \"9\": 9);\n\
         \x20 $result: 0;\n\
         \x20 @for $i from 1 through str-length($str) {\n\
         \x20   $char: str-slice($str, $i, $i);\n\
         \x20   $digit: map-get($digits, $char);\n\
         \x20   @if $digit == null { @return $result; }\n\
         \x20   $result: $result * 10 + $digit;\n\
         \x20 }\n\
         \x20 @return $result;\n\
         }\n\n",
    );

    preamble
}

/// Convert a JSON value to an SCSS literal for use in a map.
/// Returns None for unsupported types (objects, arrays, null).
fn json_to_scss_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::String(s) => {
            if s.is_empty() {
                return Some("\"\"".to_string());
            }
            // Try to represent color values as SCSS colors so stencilColor returns a Color type
            if let Some(color) = try_as_scss_color(s) {
                Some(color)
            } else {
                Some(format!("\"{}\"", escape_scss_string(s)))
            }
        }
        _ => None,
    }
}

/// Try to interpret a string as a CSS/SCSS color literal.
/// Returns the color in SCSS syntax if it looks like a hex color, None otherwise.
fn try_as_scss_color(s: &str) -> Option<String> {
    let hex = if let Some(stripped) = s.strip_prefix('#') {
        stripped
    } else if s.len() == 3 || s.len() == 6 || s.len() == 8 {
        // Could be a bare hex without #
        s
    } else {
        return None;
    };

    // Validate hex chars
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    match hex.len() {
        3 | 4 | 6 | 8 => Some(format!("#{}", hex)),
        _ => None,
    }
}

/// Escape a string for use inside SCSS double-quoted strings.
fn escape_scss_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
