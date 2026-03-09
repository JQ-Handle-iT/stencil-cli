use axum::http::{HeaderMap, StatusCode};
use bytes::Bytes;
use std::collections::HashMap;

/// Represents the possible response types from BigCommerce
pub enum StencilResponse {
    /// Non-template content (images, binary, raw HTML)
    Raw {
        body: Bytes,
        headers: HeaderMap,
        status: StatusCode,
    },
    /// HTTP redirect (301-303)
    Redirect {
        location: String,
        headers: HeaderMap,
        status: StatusCode,
    },
    /// Template rendering response
    Pencil {
        template_file: TemplateFile,
        templates: Option<serde_json::Value>,
        remote: bool,
        remote_data: Option<serde_json::Value>,
        context: serde_json::Value,
        rendered_regions: HashMap<String, String>,
        translations: Option<serde_json::Value>,
        method: String,
        accept_language: String,
        headers: HeaderMap,
        status: StatusCode,
    },
}

/// Template file can be a single path or multiple (for render_with ajax)
#[derive(Debug, Clone)]
pub enum TemplateFile {
    Single(String),
    Multiple(Vec<String>),
}

impl TemplateFile {
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        if let Some(s) = value.as_str() {
            Some(TemplateFile::Single(s.to_string()))
        } else if let Some(arr) = value.as_array() {
            let paths: Vec<String> = arr.iter().filter_map(|v| v.as_str().map(String::from)).collect();
            if paths.is_empty() {
                None
            } else if paths.len() == 1 {
                Some(TemplateFile::Single(paths.into_iter().next().unwrap()))
            } else {
                Some(TemplateFile::Multiple(paths))
            }
        } else {
            None
        }
    }

    pub fn primary_path(&self) -> &str {
        match self {
            TemplateFile::Single(s) => s.as_str(),
            TemplateFile::Multiple(v) => v.first().map(|s| s.as_str()).unwrap_or(""),
        }
    }
}
