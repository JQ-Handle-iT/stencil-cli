use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const PROFILES_FILE: &str = "profiles.stencil.json";

fn default_api_host() -> String {
    "api.bigcommerce.com".to_string()
}

fn default_port() -> u16 {
    3000
}

/// A named set of API credentials (token + host).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credential {
    pub name: String,
    pub access_token: String,
    #[serde(default = "default_api_host")]
    pub api_host: String,
}

/// A named BigCommerce store URL + local dev port.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreTarget {
    pub name: String,
    pub url: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

/// Persisted profile data: a list of credentials and a list of store targets,
/// each with an active index. Saved to `profiles.stencil.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProfileStore {
    #[serde(default)]
    pub credentials: Vec<Credential>,
    #[serde(default)]
    pub stores: Vec<StoreTarget>,
    #[serde(default)]
    pub active_credential: usize,
    #[serde(default)]
    pub active_store: usize,
}

pub type SharedProfileStore = Arc<Mutex<ProfileStore>>;

impl ProfileStore {
    pub fn load_or_default(dir: &Path) -> Self {
        let path = dir.join(PROFILES_FILE);
        if let Ok(data) = std::fs::read_to_string(&path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Default::default()
        }
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = dir.join(PROFILES_FILE);
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn active_cred(&self) -> Option<&Credential> {
        self.credentials.get(self.active_credential)
    }

    pub fn active_store_target(&self) -> Option<&StoreTarget> {
        self.stores.get(self.active_store)
    }

    // ── Credential navigation ─────────────────────────────────────────────────

    pub fn prev_credential(&mut self) {
        if self.credentials.is_empty() {
            return;
        }
        if self.active_credential == 0 {
            self.active_credential = self.credentials.len() - 1;
        } else {
            self.active_credential -= 1;
        }
    }

    pub fn next_credential(&mut self) {
        if self.credentials.is_empty() {
            return;
        }
        self.active_credential = (self.active_credential + 1) % self.credentials.len();
    }

    // ── Store navigation ──────────────────────────────────────────────────────

    pub fn prev_store(&mut self) {
        if self.stores.is_empty() {
            return;
        }
        if self.active_store == 0 {
            self.active_store = self.stores.len() - 1;
        } else {
            self.active_store -= 1;
        }
    }

    pub fn next_store(&mut self) {
        if self.stores.is_empty() {
            return;
        }
        self.active_store = (self.active_store + 1) % self.stores.len();
    }

    // ── Mutation ──────────────────────────────────────────────────────────────

    pub fn add_credential(&mut self, cred: Credential) {
        self.credentials.push(cred);
        self.active_credential = self.credentials.len() - 1;
    }

    pub fn add_store(&mut self, store: StoreTarget) {
        self.stores.push(store);
        self.active_store = self.stores.len() - 1;
    }

    pub fn remove_active_credential(&mut self) {
        if self.credentials.is_empty() {
            return;
        }
        self.credentials.remove(self.active_credential);
        if self.active_credential > 0 {
            self.active_credential -= 1;
        }
    }

    pub fn remove_active_store(&mut self) {
        if self.stores.is_empty() {
            return;
        }
        self.stores.remove(self.active_store);
        if self.active_store > 0 {
            self.active_store -= 1;
        }
    }
}
