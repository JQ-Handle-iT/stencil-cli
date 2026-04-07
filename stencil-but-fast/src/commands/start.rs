use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, RwLock};

use crate::cache::MemoryCache;
use crate::config::profiles::ProfileStore;
use crate::config::theme_config::ThemeConfigManager;
use crate::config::StencilConfig;
use crate::proxy::BigCommerceClient;
use crate::server::state::{AppState, LiveReloadMessage};
use crate::stats::ServerStats;
use crate::tui::app::{TuiInfo, run_tui};
use crate::watcher::file_watcher;

pub struct StartOptions {
    pub open: bool,
    pub variation: Option<String>,
    pub channel_id: Option<u64>,
    pub channel_url: Option<String>,
    pub no_cache: bool,
    pub port: Option<u16>,
    pub work_dir: Option<PathBuf>,
    pub gui: bool,
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

    let theme_name = theme_config.config.name.clone();
    let theme_version = theme_config.config.version.clone();

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

    let cache = Arc::new(RwLock::new(MemoryCache::new()));

    // Stats are only allocated when the TUI is requested
    let shared_stats = if opts.gui {
        Some(Arc::new(Mutex::new(ServerStats::new())))
    } else {
        None
    };

    let state = AppState {
        http_client,
        theme_config: theme_config_arc.clone(),
        cache: cache.clone(),
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
        stats: shared_stats.clone(),
    };

    // Build router
    let app = crate::server::app::build_router(state);

    // Start file watcher (pass stats so reloads are recorded)
    let _watcher = file_watcher::start(&theme_path, live_reload_tx.clone(), theme_config_arc, shared_stats.clone())?;

    // Bind the listener before potentially handing off to TUI
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    if opts.gui {
        // In GUI mode: suppress the normal stdout banner (TUI takes over)
        let variation_name = {
            // We no longer have a reference to theme_config (moved into state), so
            // use opts.variation or fall back to a sensible default label.
            opts.variation.clone().unwrap_or_else(|| "default".to_string())
        };

        let tui_info = TuiInfo {
            store_url: resolved_normal_url.clone(),
            local_url: format!("http://localhost:{}", port),
            theme_path: theme_path.display().to_string(),
            variation: variation_name,
            caching: !opts.no_cache,
            theme_name,
            theme_version,
        };

        if opts.open {
            let _ = open::that(format!("http://localhost:{}", port));
        }

        let stats_for_tui = shared_stats.unwrap(); // always Some when gui=true
        let live_reload_tx_tui = live_reload_tx.clone();
        let cache_for_tui = cache.clone();
        let profiles = Arc::new(Mutex::new(ProfileStore::load_or_default(&theme_path)));

        // Run TUI in a blocking thread; server runs in the async executor concurrently
        let tui_handle = tokio::task::spawn_blocking(move || {
            run_tui(tui_info, stats_for_tui, cache_for_tui, live_reload_tx_tui, profiles)
        });

        let server_handle = tokio::spawn(async move {
            axum::serve(listener, app).await
        });

        tokio::select! {
            _ = tui_handle => {
                // User pressed q — exit cleanly
                std::process::exit(0);
            }
            result = server_handle => {
                result??;
            }
        }
    } else {
        // Normal (non-TUI) mode: print the startup banner and serve
        println!();
        println!("{}", "-----------------Startup Information-------------".dimmed());
        println!();
        println!("Store URL: {}", resolved_normal_url.cyan());
        println!("SSL Store URL: {}", store_url.cyan());
        println!("Local server: {}", format!("http://localhost:{}", port).cyan());
        println!();
        println!("{}", "-------------------------------------------------".dimmed());
        println!();

        if opts.open {
            let _ = open::that(format!("http://localhost:{}", port));
        }

        println!(
            "{} {}",
            "Stencil is ready.".bold().green(),
            format!("Listening on http://localhost:{}", port).dimmed()
        );

        axum::serve(listener, app).await?;
    }

    Ok(())
}
