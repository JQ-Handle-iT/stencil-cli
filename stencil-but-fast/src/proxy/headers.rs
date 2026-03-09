use axum::http::HeaderMap;
use std::collections::HashMap;

/// Build stencil-options header JSON
pub fn build_stencil_options(get_template_file: bool, get_data_only: bool) -> String {
    serde_json::json!({
        "get_template_file": get_template_file,
        "get_data_only": get_data_only,
    })
    .to_string()
}

/// Convert axum HeaderMap to HashMap<String, String> for proxying,
/// while adding stencil-specific headers
pub fn build_request_headers(
    original_headers: &HeaderMap,
    stencil_options: &str,
    stencil_config: Option<&str>,
    extra_headers: &[(&str, &str)],
) -> HashMap<String, String> {
    let mut headers = HashMap::new();

    // Copy original headers
    for (name, value) in original_headers.iter() {
        let name_str = name.as_str().to_lowercase();
        // Skip hop-by-hop headers
        if matches!(
            name_str.as_str(),
            "host" | "connection" | "transfer-encoding" | "upgrade"
        ) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            headers.insert(name_str, v.to_string());
        }
    }

    // Merge stencil-options
    if let Some(existing) = headers.get("stencil-options") {
        if let (Ok(mut existing_val), Ok(new_val)) = (
            serde_json::from_str::<serde_json::Value>(existing),
            serde_json::from_str::<serde_json::Value>(stencil_options),
        ) {
            if let (Some(obj), Some(new_obj)) = (existing_val.as_object_mut(), new_val.as_object()) {
                for (k, v) in new_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
            headers.insert("stencil-options".into(), existing_val.to_string());
        }
    } else {
        headers.insert("stencil-options".into(), stencil_options.to_string());
    }

    headers.insert("accept-encoding".into(), "identity".to_string());

    // Set stencil-config if not already set
    if let Some(config) = stencil_config {
        let existing = headers.get("stencil-config").map(|s| s.as_str());
        if existing.is_none() || existing == Some("{}") {
            headers.insert("stencil-config".into(), config.to_string());
        }
    }

    // Apply extra headers (can override)
    for (k, v) in extra_headers {
        headers.insert(k.to_lowercase(), v.to_string());
    }

    headers
}
