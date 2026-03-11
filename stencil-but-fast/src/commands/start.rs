use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

use crate::cache::MemoryCache;
use crate::config::theme_config::ThemeConfigManager;
use crate::config::StencilConfig;
use crate::proxy::BigCommerceClient;
use crate::server::state::{AppState, LiveReloadMessage};
use crate::watcher::file_watcher;

pub struct StartOptions {
    pub open: bool,
    pub variation: Option<String>,
    pub channel_id: Option<u64>,
    pub channel_url: Option<String>,
    pub no_cache: bool,
    pub port: Option<u16>,
    pub work_dir: Option<PathBuf>,
}

pub async fn run(opts: StartOptions) -> Result<()> {
    let cwd = opts.work_dir.unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let theme_path = cwd.clone();

    // Check config.json exists
    if !theme_path.join("config.json").exists() {
        bail!(
            "{}{}{}",
            "You must have a ".red(),
            " config.json ".cyan(),
            "file in your top level theme directory.".red()
        );
    }

    // Load stencil config
    let stencil_config = StencilConfig::load(&cwd)?
        .ok_or_else(|| anyhow::anyhow!(
            "No stencil configuration found. Run {} first.",
            "stencil init".bold()
        ))?;

    // Load theme config
    let mut theme_config = ThemeConfigManager::load(&theme_path)?;

    // Set variation if specified
    if let Some(ref variation) = opts.variation {
        theme_config.set_variation_by_name(variation)?;
    }

    let port = opts.port.unwrap_or(stencil_config.general.port);
    let api_host = &stencil_config.general.api_host;
    let access_token = &stencil_config.secrets.access_token;
    let normal_store_url = &stencil_config.general.normal_store_url;

    println!("{}", "Starting stencil development server...".bold().cyan());

    // Create BC client
    let bc_client = BigCommerceClient::new()?;

    // Get store hash
    let store_hash = bc_client
        .get_store_hash(normal_store_url)
        .await
        .context("Failed to get store hash. Check your store URL.")?;

    tracing::info!("Store hash: {}", store_hash);

    // Resolve channel URL
    let channel_url = if let Some(ref url) = opts.channel_url {
        url.clone()
    } else {
        let channels = bc_client
            .get_store_channels(&store_hash, access_token, api_host)
            .await
            .unwrap_or_default();

        if let Some(ch_id) = opts.channel_id {
            channels
                .iter()
                .find(|c| c.channel_id as u64 == ch_id)
                .map(|c| c.url.clone())
                .unwrap_or_else(|| normal_store_url.clone())
        } else {
            channels
                .first()
                .map(|c| c.url.clone())
                .unwrap_or_else(|| normal_store_url.clone())
        }
    };

    // Check CLI version / get store info
    let store_info = bc_client
        .check_cli_version(&channel_url)
        .await
        .unwrap_or(crate::proxy::client::StoreInfo {
            ssl_url: channel_url.clone(),
            base_url: normal_store_url.clone(),
        });

    let store_url = store_info.ssl_url.clone();
    let resolved_normal_url = store_info.base_url.clone();

    // Get store settings locale
    let store_settings_locale = bc_client
        .get_store_settings_locale(&store_hash, access_token, api_host)
        .await
        .unwrap_or(crate::proxy::client::StoreSettingsLocale {
            default_shopper_language: "en".into(),
            shopper_language_selection_method: "default_shopper_language".into(),
        });

    // Build shared state
    let (live_reload_tx, _) = broadcast::channel::<LiveReloadMessage>(16);
    let theme_config_arc = Arc::new(RwLock::new(theme_config));

    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let state = AppState {
        http_client,
        theme_config: theme_config_arc.clone(),
        cache: Arc::new(RwLock::new(MemoryCache::new())),
        css_cache: Arc::new(RwLock::new(std::collections::HashMap::new())),
        theme_path: theme_path.clone(),
        store_url: store_url.clone(),
        normal_store_url: resolved_normal_url.clone(),
        access_token: access_token.clone(),
        port,
        custom_layouts: stencil_config.general.custom_layouts.clone(),
        use_cache: !opts.no_cache,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        store_settings_locale,
        live_reload_tx: live_reload_tx.clone(),
    };

    // Build router
    let app = crate::server::app::build_router(state);

    // Start file watcher
    let _watcher = file_watcher::start(&theme_path, live_reload_tx, theme_config_arc)?;

    // Print startup info
    println!();
    println!("{}", "-----------------Startup Information-------------".dimmed());
    println!();
    println!("Store URL: {}", resolved_normal_url.cyan());
    println!("SSL Store URL: {}", store_url.cyan());
    println!("Local server: {}", format!("http://localhost:{}", port).cyan());
    println!();
    println!("{}", "-------------------------------------------------".dimmed());
    println!();

    // Open browser if requested
    if opts.open {
        let url = format!("http://localhost:{}", port);
        let _ = open::that(&url);
    }

    // Start server
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!(
        "{} {}",
        "Stencil is ready.".bold().green(),
        format!("Listening on http://localhost:{}", port).dimmed()
    );

    axum::serve(listener, app).await?;

    Ok(())
}
