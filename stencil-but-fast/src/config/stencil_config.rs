use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const GENERAL_CONFIG_FILE: &str = "config.stencil.json";
const SECRETS_CONFIG_FILE: &str = "secrets.stencil.json";
const LEGACY_CONFIG_FILE: &str = ".stencil";
const DEFAULT_API_HOST: &str = "https://api.bigcommerce.com";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StencilGeneralConfig {
    pub normal_store_url: String,
    pub port: u16,
    #[serde(default = "default_api_host")]
    pub api_host: String,
    #[serde(default)]
    pub custom_layouts: CustomLayouts,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StencilSecretsConfig {
    pub access_token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_token: Option<String>,
}

#[derive(Debug, Clone)]
pub struct StencilConfig {
    pub general: StencilGeneralConfig,
    pub secrets: StencilSecretsConfig,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct CustomLayouts {
    #[serde(default)]
    pub brand: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub category: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub page: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub product: HashMap<String, serde_json::Value>,
}

/// Legacy single-file config format (.stencil)
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyConfig {
    #[serde(default)]
    normal_store_url: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    api_host: Option<String>,
    #[serde(default)]
    custom_layouts: Option<CustomLayouts>,
    #[serde(default)]
    github_token: Option<String>,
}

fn default_api_host() -> String {
    DEFAULT_API_HOST.to_string()
}

impl StencilConfig {
    /// Load config from config.stencil.json + secrets.stencil.json,
    /// or migrate from legacy .stencil file if found.
    pub fn load(dir: &Path) -> Result<Option<Self>> {
        let general_path = dir.join(GENERAL_CONFIG_FILE);
        let secrets_path = dir.join(SECRETS_CONFIG_FILE);
        let legacy_path = dir.join(LEGACY_CONFIG_FILE);

        // Try new format first
        if general_path.exists() && secrets_path.exists() {
            let general: StencilGeneralConfig = serde_json::from_str(
                &fs::read_to_string(&general_path)
                    .with_context(|| format!("Failed to read {}", general_path.display()))?,
            )
            .with_context(|| format!("Failed to parse {}", general_path.display()))?;

            let secrets: StencilSecretsConfig = serde_json::from_str(
                &fs::read_to_string(&secrets_path)
                    .with_context(|| format!("Failed to read {}", secrets_path.display()))?,
            )
            .with_context(|| format!("Failed to parse {}", secrets_path.display()))?;

            return Ok(Some(StencilConfig { general, secrets }));
        }

        // Try legacy format and migrate
        if legacy_path.exists() {
            let legacy: LegacyConfig = serde_json::from_str(
                &fs::read_to_string(&legacy_path)
                    .with_context(|| format!("Failed to read {}", legacy_path.display()))?,
            )
            .with_context(|| format!("Failed to parse {}", legacy_path.display()))?;

            let config = StencilConfig {
                general: StencilGeneralConfig {
                    normal_store_url: legacy.normal_store_url.unwrap_or_default(),
                    port: legacy.port.unwrap_or(3000),
                    api_host: legacy.api_host.unwrap_or_else(default_api_host),
                    custom_layouts: legacy.custom_layouts.unwrap_or_default(),
                },
                secrets: StencilSecretsConfig {
                    access_token: legacy.access_token.unwrap_or_default(),
                    github_token: legacy.github_token,
                },
            };

            // Save in new format and remove legacy
            config.save(dir)?;
            fs::remove_file(&legacy_path).ok();
            eprintln!(
                "Migrated {} to {} + {}",
                LEGACY_CONFIG_FILE, GENERAL_CONFIG_FILE, SECRETS_CONFIG_FILE
            );

            return Ok(Some(config));
        }

        Ok(None)
    }

    /// Save config to config.stencil.json + secrets.stencil.json
    pub fn save(&self, dir: &Path) -> Result<()> {
        let general_path = dir.join(GENERAL_CONFIG_FILE);
        let secrets_path = dir.join(SECRETS_CONFIG_FILE);

        let general_json = serde_json::to_string_pretty(&self.general)
            .context("Failed to serialize general config")?;
        fs::write(&general_path, general_json.as_bytes())
            .with_context(|| format!("Failed to write {}", general_path.display()))?;

        let secrets_json = serde_json::to_string_pretty(&self.secrets)
            .context("Failed to serialize secrets config")?;
        fs::write(&secrets_path, secrets_json.as_bytes())
            .with_context(|| format!("Failed to write {}", secrets_path.display()))?;

        Ok(())
    }

    pub fn general_config_path(dir: &Path) -> PathBuf {
        dir.join(GENERAL_CONFIG_FILE)
    }

    pub fn secrets_config_path(dir: &Path) -> PathBuf {
        dir.join(SECRETS_CONFIG_FILE)
    }
}
