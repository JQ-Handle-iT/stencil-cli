use anyhow::Result;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;
use tokio::sync::broadcast;

use crate::config::theme_config::ThemeConfigManager;
use crate::server::state::LiveReloadMessage;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Start watching theme files for changes
pub fn start(
    theme_path: &Path,
    tx: broadcast::Sender<LiveReloadMessage>,
    theme_config: Arc<RwLock<ThemeConfigManager>>,
) -> Result<RecommendedWatcher> {
    let (notify_tx, notify_rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        if let Ok(event) = res {
            let _ = notify_tx.send(event);
        }
    })?;

    // Watch directories
    let scss_path = theme_path.join("assets").join("scss");
    let templates_path = theme_path.join("templates");
    let lang_path = theme_path.join("lang");
    let config_path = theme_path.join("config.json");

    if scss_path.exists() {
        watcher.watch(&scss_path, RecursiveMode::Recursive)?;
    }
    if templates_path.exists() {
        watcher.watch(&templates_path, RecursiveMode::Recursive)?;
    }
    if lang_path.exists() {
        watcher.watch(&lang_path, RecursiveMode::Recursive)?;
    }
    if config_path.exists() {
        watcher.watch(&config_path, RecursiveMode::NonRecursive)?;
    }

    let scss_prefix = scss_path.clone();
    let templates_prefix = templates_path.clone();
    let lang_prefix = lang_path.clone();
    let config_file = config_path.clone();

    // Spawn a thread to process notify events with debouncing
    tokio::task::spawn_blocking(move || {
        let mut last_reload = std::time::Instant::now();
        let debounce = Duration::from_millis(300);

        for event in notify_rx {
            if last_reload.elapsed() < debounce {
                continue;
            }

            let action = classify_event(&event, &scss_prefix, &templates_prefix, &lang_prefix, &config_file);

            match action {
                ReloadAction::CssOnly => {
                    tracing::info!("SCSS changed, reloading CSS");
                    let _ = tx.send(LiveReloadMessage::CssReload);
                    last_reload = std::time::Instant::now();
                }
                ReloadAction::FullReload => {
                    tracing::info!("File changed, full reload");
                    let _ = tx.send(LiveReloadMessage::FullReload);
                    last_reload = std::time::Instant::now();
                }
                ReloadAction::ConfigReload => {
                    tracing::info!("config.json changed, resetting variations");
                    // Reset theme config
                    let tc = theme_config.clone();
                    tokio::runtime::Handle::current().block_on(async {
                        let mut config = tc.write().await;
                        config.reset_variation_settings();
                    });
                    let _ = tx.send(LiveReloadMessage::FullReload);
                    last_reload = std::time::Instant::now();
                }
                ReloadAction::None => {}
            }
        }
    });

    Ok(watcher)
}

enum ReloadAction {
    CssOnly,
    FullReload,
    ConfigReload,
    None,
}

fn classify_event(
    event: &Event,
    scss_prefix: &Path,
    templates_prefix: &Path,
    lang_prefix: &Path,
    config_file: &Path,
) -> ReloadAction {
    // Only respond to modify/create events
    match event.kind {
        EventKind::Modify(_) | EventKind::Create(_) => {}
        _ => return ReloadAction::None,
    }

    for path in &event.paths {
        if path.starts_with(scss_prefix) {
            return ReloadAction::CssOnly;
        }
        if path == config_file {
            return ReloadAction::ConfigReload;
        }
        if path.starts_with(templates_prefix) || path.starts_with(lang_prefix) {
            return ReloadAction::FullReload;
        }
    }

    ReloadAction::None
}
