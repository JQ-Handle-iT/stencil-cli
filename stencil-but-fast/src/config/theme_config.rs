use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThemeConfig {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default = "default_css_compiler")]
    pub css_compiler: String,
    #[serde(default = "default_true")]
    pub autoprefixer_cascade: bool,
    #[serde(default)]
    pub autoprefixer_browsers: Vec<String>,
    #[serde(default)]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub images: serde_json::Value,
    #[serde(default)]
    pub variations: Vec<ThemeVariation>,
    #[serde(default)]
    pub resources: serde_json::Value,
    #[serde(default)]
    pub template_engine: Option<String>,
    #[serde(default)]
    pub meta: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ThemeVariation {
    pub name: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub images: serde_json::Value,
    #[serde(default)]
    pub meta: serde_json::Value,
}

fn default_css_compiler() -> String {
    "scss".to_string()
}

fn default_true() -> bool {
    true
}

/// Manages theme config.json with variation support
#[derive(Debug)]
pub struct ThemeConfigManager {
    pub config: ThemeConfig,
    pub config_path: PathBuf,
    pub variation_index: usize,
}

impl ThemeConfigManager {
    pub fn load(theme_path: &Path) -> Result<Self> {
        let config_path = theme_path.join("config.json");
        let raw = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;
        let config: ThemeConfig = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", config_path.display()))?;

        Ok(Self {
            config,
            config_path,
            variation_index: 0,
        })
    }

    pub fn reload(&mut self) -> Result<()> {
        let raw = fs::read_to_string(&self.config_path)?;
        self.config = serde_json::from_str(&raw)?;
        Ok(())
    }

    /// Set variation by name, returns error if not found
    pub fn set_variation_by_name(&mut self, name: &str) -> Result<()> {
        let idx = self
            .config
            .variations
            .iter()
            .position(|v| v.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                let available: Vec<&str> = self.config.variations.iter().map(|v| v.name.as_str()).collect();
                anyhow::anyhow!(
                    "Variation '{}' not found. Available: {}",
                    name,
                    available.join(", ")
                )
            })?;
        self.variation_index = idx;
        Ok(())
    }

    pub fn variation_exists(&self, index: usize) -> bool {
        index < self.config.variations.len()
    }

    pub fn set_variation(&mut self, index: usize) {
        self.variation_index = index;
    }

    /// Get merged settings: global settings + variation settings override
    pub fn get_settings(&self) -> serde_json::Value {
        let mut settings = self.config.settings.clone();
        if let Some(variation) = self.config.variations.get(self.variation_index) {
            if let (Some(base), Some(var)) = (settings.as_object_mut(), variation.settings.as_object()) {
                for (k, v) in var {
                    base.insert(k.clone(), v.clone());
                }
            }
        }
        settings
    }

    /// Get the full configuration suitable for rendering
    pub fn get_config(&self) -> ThemeConfigSnapshot {
        ThemeConfigSnapshot {
            settings: self.get_settings(),
            template_engine: self
                .config
                .template_engine
                .clone()
                .unwrap_or_else(|| "handlebars-v4".to_string()),
            resources: self.config.resources.clone(),
        }
    }

    pub fn reset_variation_settings(&mut self) {
        // Reload config from disk to pick up changes
        let _ = self.reload();
    }
}

#[derive(Debug, Clone)]
pub struct ThemeConfigSnapshot {
    pub settings: serde_json::Value,
    pub template_engine: String,
    pub resources: serde_json::Value,
}
