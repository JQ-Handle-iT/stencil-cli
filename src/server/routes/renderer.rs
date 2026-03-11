use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::time::Duration;

use crate::proxy::headers::{build_request_headers, build_stencil_options};
use crate::renderer::{
    frontmatter, lang_assembler, paper::PaperEngine, response::TemplateFile, template_assembler,
};
use crate::server::state::AppState;
use crate::utils::{int2uuid, normalize_redirect_url, strip_domain_from_cookies};

const CACHE_TTL: Duration = Duration::from_secs(15);

/// Main renderer handler - the core of stencil-cli
pub async fn handler(
    State(state): State<AppState>,
    req: Request,
) -> Result<Response, StatusCode> {
    match handle_request(state, req).await {
        Ok(resp) => Ok(resp),
        Err(e) => {
            tracing::error!("Renderer error: {}", e);
            Ok(Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("Internal Server Error: {}", e)))
                .unwrap())
        }
    }
}

/// Called from the catch-all fallback handler
pub async fn handler_from_request(state: AppState, req: Request) -> Response {
    match handle_request(state, req).await {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("Renderer error: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from(format!("Internal Server Error: {}", e)))
                .unwrap()
        }
    }
}

async fn handle_request(state: AppState, req: Request) -> anyhow::Result<Response> {
    let method = req.method().clone();
    let uri = req.uri().clone();
    let path = uri.path().to_string();
    let query_string = uri.query().unwrap_or("").to_string();
    let original_headers = req.headers().clone();
    let accept_language = original_headers
        .get("accept-language")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("en")
        .to_lowercase();

    // Read request body
    let body_bytes = axum::body::to_bytes(req.into_body(), 20 * 1024 * 1024)
        .await
        .unwrap_or_default();

    // Build target URL on the BigCommerce store
    let store_url_parsed = url::Url::parse(&state.store_url)?;
    let mut full_url = url::Url::parse(&state.store_url)?;
    full_url.set_path(&path);
    full_url.set_query(if query_string.is_empty() {
        None
    } else {
        Some(&query_string)
    });

    // Build stencil-options header for first request
    let stencil_opts = build_stencil_options(true, true);
    let host = store_url_parsed.host_str().unwrap_or("localhost");
    let headers1 = build_request_headers(
        &original_headers,
        &stencil_opts,
        None,
        &[("host", host)],
    );

    // Compute cache signature
    let mut sig_headers = headers1.clone();
    sig_headers.remove("cookie");
    let request_signature = format!(
        "{}{}",
        sha1_hex(&full_url.to_string()),
        sha1_hex(&serde_json::to_string(&sig_headers).unwrap_or_default())
    );

    // Check cache
    if method == axum::http::Method::GET && state.use_cache {
        let cache = state.cache.read().await;
        if let Some(cached) = cache.get(&request_signature) {
            // Parse cached response
            return build_from_cached(&state, cached, &original_headers, &path, &accept_language)
                .await;
        }
    }

    // Clear cache on non-GET or cart requests
    if method != axum::http::Method::GET || path == "/cart.php" {
        state.cache.write().await.clear();
    }

    // Make first request to BigCommerce
    let mut bc_req = state
        .http_client
        .request(method.clone(), full_url.as_str());

    for (k, v) in &headers1 {
        bc_req = bc_req.header(k.as_str(), v.as_str());
    }
    bc_req = bc_req.header("x-auth-token", &state.access_token);

    if !body_bytes.is_empty() {
        bc_req = bc_req.body(body_bytes.clone());
    }

    let response1 = bc_req.send().await?;
    let status1 = response1.status();
    let resp1_headers = response1.headers().clone();

    // Process set-cookie headers
    let set_cookies = extract_set_cookies(&resp1_headers);
    let stripped_cookies = strip_domain_from_cookies(&set_cookies);

    // Handle redirects
    if (301..=303).contains(&status1.as_u16()) {
        let location = resp1_headers
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("/")
            .to_string();

        let normalized = normalize_redirect_url(&location, &state.normal_store_url, &state.store_url);

        let mut resp = Response::builder()
            .status(StatusCode::from_u16(status1.as_u16()).unwrap_or(StatusCode::FOUND))
            .header("location", &normalized);

        for cookie in &stripped_cookies {
            resp = resp.header("set-cookie", cookie);
        }

        return Ok(resp.body(Body::empty())?);
    }

    // Check content type
    let content_type = resp1_headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();

    let is_json = content_type.contains("application/json");
    let is_binary = content_type.starts_with("image/")
        || content_type.starts_with("video/")
        || content_type.starts_with("audio/")
        || content_type.starts_with("application/octet-stream")
        || content_type.starts_with("application/pdf");

    let resp1_body = response1.bytes().await?;

    // Binary content - pass through as raw
    if is_binary || (!is_json && !content_type.contains("text/html")) {
        let mut resp = Response::builder()
            .status(StatusCode::from_u16(status1.as_u16()).unwrap_or(StatusCode::OK));

        for (name, value) in resp1_headers.iter() {
            let n = name.as_str().to_lowercase();
            if n == "transfer-encoding" || n == "content-length" || n == "x-frame-options" {
                continue;
            }
            resp = resp.header(name, value);
        }
        for cookie in &stripped_cookies {
            resp = resp.header("set-cookie", cookie);
        }

        return Ok(resp.body(Body::from(resp1_body))?);
    }

    // Try to parse as JSON
    let bc_app_data: serde_json::Value = if is_json {
        serde_json::from_slice(&resp1_body).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };

    // Cache the response
    if state.use_cache {
        state.cache.write().await.put(
            request_signature,
            bc_app_data.clone(),
            CACHE_TTL,
        );
    }

    // If no pencil_response, it's a raw response
    if !bc_app_data.is_object() || !bc_app_data.get("pencil_response").is_some() {
        let mut resp = Response::builder()
            .status(StatusCode::from_u16(status1.as_u16()).unwrap_or(StatusCode::OK));
        for (name, value) in resp1_headers.iter() {
            let n = name.as_str().to_lowercase();
            if n == "transfer-encoding" || n == "content-length" || n == "x-frame-options" {
                continue;
            }
            resp = resp.header(name, value);
        }
        for cookie in &stripped_cookies {
            resp = resp.header("set-cookie", cookie);
        }
        return Ok(resp.body(Body::from(resp1_body))?);
    }

    // It's a pencil response - need template rendering
    let theme_config = state.theme_config.read().await;
    let configuration = theme_config.get_config();
    drop(theme_config);

    // If remote, render immediately without second request
    if bc_app_data.get("remote").and_then(|v| v.as_bool()).unwrap_or(false) {
        return render_pencil_response(
            &state,
            &bc_app_data,
            &configuration,
            &path,
            &accept_language,
            &original_headers,
            &stripped_cookies,
            status1.as_u16(),
            HashMap::new(),
        )
        .await;
    }

    // Make second request for data
    let template_path = get_template_path(&path, &bc_app_data, &state.custom_layouts);
    let resource_config = get_resource_config(
        &state.theme_path,
        &template_path,
        &bc_app_data,
        &original_headers,
        &configuration.settings,
    );

    let stencil_opts2 = build_stencil_options(false, true);
    let stencil_config_json =
        serde_json::to_string(&resource_config).unwrap_or_else(|_| "{}".into());
    let headers2 = build_request_headers(
        &original_headers,
        &stencil_opts2,
        Some(&stencil_config_json),
        &[("host", host)],
    );

    let mut bc_req2 = state
        .http_client
        .request(method, full_url.as_str());

    for (k, v) in &headers2 {
        bc_req2 = bc_req2.header(k.as_str(), v.as_str());
    }
    bc_req2 = bc_req2.header("x-auth-token", &state.access_token);

    let response2 = bc_req2.send().await?;
    let status2 = response2.status();
    let resp2_headers = response2.headers().clone();

    // Handle redirect on second request
    if (301..=303).contains(&status2.as_u16()) {
        let location = resp2_headers
            .get("location")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("/");
        let normalized = normalize_redirect_url(location, &state.normal_store_url, &state.store_url);
        return Ok(Response::builder()
            .status(StatusCode::from_u16(status2.as_u16()).unwrap_or(StatusCode::FOUND))
            .header("location", &normalized)
            .body(Body::empty())?);
    }

    let data2: serde_json::Value = response2.json().await.unwrap_or(serde_json::Value::Null);

    if data2.get("status").and_then(|v| v.as_u64()) == Some(500) {
        return Ok(Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from("BigCommerce server returned 500"))?);
    }

    // Render the template
    render_pencil_response(
        &state,
        &data2,
        &configuration,
        &path,
        &accept_language,
        &original_headers,
        &stripped_cookies,
        status2.as_u16(),
        HashMap::new(),
    )
    .await
}

async fn render_pencil_response(
    state: &AppState,
    data: &serde_json::Value,
    configuration: &crate::config::theme_config::ThemeConfigSnapshot,
    request_path: &str,
    accept_language: &str,
    original_headers: &HeaderMap,
    set_cookies: &[String],
    status_code: u16,
    rendered_regions: HashMap<String, String>,
) -> anyhow::Result<Response> {
    let theme_config = state.theme_config.read().await;
    let variation_index = theme_config.variation_index;
    drop(theme_config);

    // Build context
    let mut context = data
        .get("context")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    // Override theme_settings and settings for local dev
    if let Some(obj) = context.as_object_mut() {
        obj.insert("theme_settings".into(), configuration.settings.clone());
        obj.insert(
            "template_engine".into(),
            serde_json::Value::String(configuration.template_engine.clone()),
        );
        obj.insert("in_development".into(), serde_json::Value::Bool(true));
        obj.insert("in_production".into(), serde_json::Value::Bool(false));

        // Override CDN settings for local serving
        if let Some(settings) = obj.get_mut("settings") {
            if let Some(s) = settings.as_object_mut() {
                s.insert("cdn_url".into(), serde_json::Value::String("".into()));
                s.insert(
                    "theme_version_id".into(),
                    serde_json::Value::String(int2uuid(1)),
                );
                s.insert(
                    "theme_config_id".into(),
                    serde_json::Value::String(int2uuid((variation_index + 1) as u64)),
                );
                s.insert("theme_session_id".into(), serde_json::Value::Null);
                s.insert(
                    "maintenance".into(),
                    serde_json::json!({
                        "secure_path": format!("http://localhost:{}", state.port)
                    }),
                );
            }
        }
    }

    // Check for ?debug=context
    if original_headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .contains("debug=context")
    {
        let json = serde_json::to_string_pretty(&context)?;
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .body(Body::from(json))?);
    }

    // Determine template path
    let template_file_value = data
        .get("template_file")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let template_path = get_template_path(request_path, data, &state.custom_layouts);

    let template_file = match &template_path {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => arr
            .first()
            .and_then(|v| v.as_str())
            .unwrap_or("pages/home")
            .to_string(),
        _ => "pages/home".to_string(),
    };

    // Assemble templates
    let templates_path = state.theme_path.join("templates");
    let templates = template_assembler::assemble(&templates_path, &template_file).await?;

    // Load translations
    let translations = lang_assembler::assemble(&state.theme_path).await?;

    // Set up Paper engine
    let mut paper = PaperEngine::new();
    paper.set_translations(translations);
    paper.set_regions(rendered_regions);
    paper.register_helpers();
    paper.load_templates(&templates)?;

    // Render
    let output = match paper.render(&template_file, &context) {
        Ok(html) => html,
        Err(e) => {
            tracing::error!("Template render error: {}", e);
            format!(
                "<html><body><h1>Template Error</h1><pre>{}</pre></body></html>",
                e
            )
        }
    };

    // Apply decorators
    let mut output = output;

    // Strip base_url and secure_base_url
    if let Some(settings) = context.get("settings") {
        if let Some(base_url) = settings.get("base_url").and_then(|v| v.as_str()) {
            if !base_url.is_empty() {
                output = output.replace(base_url, "");
            }
        }
        if let Some(secure_base_url) = settings.get("secure_base_url").and_then(|v| v.as_str()) {
            if !secure_base_url.is_empty() {
                output = output.replace(secure_base_url, "");
            }
        }
    }

    // Inject live-reload script before </body>
    let live_reload_script = format!(
        r#"<script>
(function(){{
  var ws=new WebSocket('ws://'+location.hostname+':'+location.port+'/__live_reload');
  ws.onmessage=function(e){{
    var m=JSON.parse(e.data);
    if(m.type==='full')window.location.reload();
    if(m.type==='css')document.querySelectorAll('link[rel=stylesheet]').forEach(function(l){{
      l.href=l.href.split('?')[0]+'?_r='+Date.now();
    }});
  }};
  ws.onclose=function(){{setTimeout(function(){{location.reload();}},2000);}};
}})();
</script>"#
    );
    if let Some(pos) = output.to_lowercase().rfind("</body>") {
        output.insert_str(pos, &live_reload_script);
    }

    // Build response
    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK);
    let mut resp = Response::builder()
        .status(status)
        .header("content-type", "text/html; charset=utf-8");

    for cookie in set_cookies {
        resp = resp.header("set-cookie", cookie);
    }

    Ok(resp.body(Body::from(output))?)
}

fn get_template_path(
    request_path: &str,
    data: &serde_json::Value,
    custom_layouts: &crate::config::CustomLayouts,
) -> serde_json::Value {
    let page_type = data.get("page_type").and_then(|v| v.as_str()).unwrap_or("");
    let valid_types = ["brand", "category", "page", "product"];

    if valid_types.contains(&page_type) {
        let layouts = match page_type {
            "brand" => &custom_layouts.brand,
            "category" => &custom_layouts.category,
            "page" => &custom_layouts.page,
            "product" => &custom_layouts.product,
            _ => return data.get("template_file").cloned().unwrap_or(serde_json::Value::Null),
        };

        for (template_name, paths) in layouts {
            let urls: Vec<String> = match paths {
                serde_json::Value::String(s) => vec![s.clone()],
                serde_json::Value::Array(arr) => arr
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect(),
                _ => continue,
            };

            let normalized_request = request_path.trim_end_matches('/');
            for url in &urls {
                let normalized_url = url.trim_end_matches('/');
                if normalized_url == normalized_request {
                    let custom_path = format!(
                        "pages/custom/{}/{}",
                        page_type,
                        template_name.trim_end_matches(".html")
                    );
                    return serde_json::Value::String(custom_path);
                }
            }
        }
    }

    data.get("template_file")
        .cloned()
        .unwrap_or(serde_json::Value::Null)
}

fn get_resource_config(
    theme_path: &std::path::Path,
    template_path: &serde_json::Value,
    data: &serde_json::Value,
    headers: &HeaderMap,
    settings: &serde_json::Value,
) -> serde_json::Value {
    let templates_path = theme_path.join("templates");

    // If it's an array (render_with), use stencil-config header
    if template_path.is_array() {
        if let Some(config) = headers
            .get("stencil-config")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| serde_json::from_str(s).ok())
        {
            return config;
        }
        return serde_json::json!({});
    }

    // Single template - parse frontmatter
    if let Some(path_str) = template_path.as_str() {
        if let Ok(raw) = template_assembler::get_template_content_sync(&templates_path, path_str) {
            if let Some(fm_content) = frontmatter::get_frontmatter_content(&raw) {
                let interpolated = frontmatter::interpolate_theme_settings(&fm_content, settings);
                // Remove unresolved theme_settings references
                let cleaned = regex::Regex::new(r"\{\{\s*?theme_settings\..+?\s*?\}\}")
                    .unwrap()
                    .replace_all(&interpolated, "")
                    .to_string();

                if let Some(parsed) = frontmatter::parse_frontmatter(&cleaned) {
                    return parsed;
                }
            }
        }
    }

    serde_json::json!({})
}

async fn build_from_cached(
    state: &AppState,
    cached: &serde_json::Value,
    original_headers: &HeaderMap,
    path: &str,
    accept_language: &str,
) -> anyhow::Result<Response> {
    // Re-process the cached BC app data
    if !cached.is_object() || cached.get("pencil_response").is_none() {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Body::from(cached.to_string()))?);
    }

    let theme_config = state.theme_config.read().await;
    let configuration = theme_config.get_config();
    drop(theme_config);

    render_pencil_response(
        state,
        cached,
        &configuration,
        path,
        accept_language,
        original_headers,
        &[],
        200,
        HashMap::new(),
    )
    .await
}

fn sha1_hex(input: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn extract_set_cookies(headers: &reqwest::header::HeaderMap) -> Vec<String> {
    headers
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok().map(String::from))
        .collect()
}
