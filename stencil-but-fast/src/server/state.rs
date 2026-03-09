use crate::cache::MemoryCache;
use crate::config::theme_config::ThemeConfigManager;
use crate::proxy::client::StoreSettingsLocale;
use crate::config::CustomLayouts;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub http_client: reqwest::Client,
    pub theme_config: Arc<RwLock<ThemeConfigManager>>,
    pub cache: Arc<RwLock<MemoryCache>>,
    pub theme_path: PathBuf,
    pub store_url: String,
    pub normal_store_url: String,
    pub access_token: String,
    pub port: u16,
    pub custom_layouts: CustomLayouts,
    pub use_cache: bool,
    pub cli_version: String,
    pub store_settings_locale: StoreSettingsLocale,
    pub live_reload_tx: broadcast::Sender<LiveReloadMessage>,
}

#[derive(Debug, Clone)]
pub enum LiveReloadMessage {
    FullReload,
    CssReload,
}
