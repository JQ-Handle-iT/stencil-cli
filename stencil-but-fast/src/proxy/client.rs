use anyhow::{bail, Context, Result};
use reqwest::{Client, Method, Response};
use std::collections::HashMap;
use std::path::Path;

/// HTTP client wrapper for BigCommerce API calls
pub struct BigCommerceClient {
    client: Client,
}

impl BigCommerceClient {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("Failed to create HTTP client")?;
        Ok(Self { client })
    }

    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Make a request to BigCommerce store, returning the raw response
    pub async fn request(
        &self,
        method: Method,
        url: &str,
        headers: HashMap<String, String>,
        body: Option<bytes::Bytes>,
        access_token: &str,
    ) -> Result<Response> {
        let mut req = self.client.request(method.clone(), url);

        for (key, val) in &headers {
            req = req.header(key.as_str(), val.as_str());
        }

        req = req.header("x-auth-token", access_token);
        req = req.header("accept-encoding", "identity");

        if let Some(body) = body {
            req = req.body(body);
        }

        let resp = req.send().await.with_context(|| {
            format!("Failed to send {} request to {}", method, url)
        })?;

        Ok(resp)
    }

    /// Fetch the store hash from the store URL
    pub async fn get_store_hash(&self, store_url: &str) -> Result<String> {
        let url = format!("{}/admin/oauth/info", store_url.trim_end_matches('/'));
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to get store info")?;

        let data: serde_json::Value = resp.json().await.context("Failed to parse store info")?;
        let store_hash = data["store_hash"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("store_hash not found in response"))?
            .to_string();

        Ok(store_hash)
    }

    /// Check CLI version against the store
    pub async fn check_cli_version(&self, channel_url: &str) -> Result<StoreInfo> {
        let url = format!(
            "{}/stencil-version-check?v={}",
            channel_url.trim_end_matches('/'),
            env!("CARGO_PKG_VERSION")
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed CLI version check")?;

        if !resp.status().is_success() {
            // Non-critical; proceed with defaults from URL
            return Ok(StoreInfo {
                ssl_url: channel_url.to_string(),
                base_url: channel_url.to_string(),
            });
        }

        let data: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));
        Ok(StoreInfo {
            ssl_url: data["sslUrl"]
                .as_str()
                .unwrap_or(channel_url)
                .to_string(),
            base_url: data["baseUrl"]
                .as_str()
                .unwrap_or(channel_url)
                .to_string(),
        })
    }

    /// Fetch store channels from the API
    pub async fn get_store_channels(
        &self,
        store_hash: &str,
        access_token: &str,
        api_host: &str,
    ) -> Result<Vec<StoreChannel>> {
        let url = format!(
            "{}/stores/{}/v3/channels?type:in=storefront&platform:in=bigcommerce&status:in=active,prelaunch&available=true",
            api_host.trim_end_matches('/'),
            store_hash
        );

        let resp = self
            .client
            .get(&url)
            .header("x-auth-token", access_token)
            .send()
            .await
            .context("Failed to get store channels")?;

        let data: serde_json::Value = resp.json().await.context("Failed to parse channels")?;
        let channels = data["data"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|ch| {
                Some(StoreChannel {
                    channel_id: ch["id"].as_u64()? as u32,
                    url: ch["url"].as_str()?.to_string(),
                    name: ch["name"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();

        Ok(channels)
    }

    // ── Theme management API ──────────────────────────────────────────────────

    /// Upload a theme bundle zip via multipart POST.
    /// Returns the `job_id` to poll for completion.
    pub async fn upload_theme(
        &self,
        store_hash: &str,
        access_token: &str,
        api_host: &str,
        zip_path: &Path,
    ) -> Result<String> {
        let url = format!(
            "{}/stores/{}/v3/themes",
            api_host.trim_end_matches('/'),
            store_hash
        );

        let file_bytes = tokio::fs::read(zip_path)
            .await
            .with_context(|| format!("Cannot read bundle at {}", zip_path.display()))?;

        let filename = zip_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("theme.zip")
            .to_string();

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(filename)
            .mime_str("application/zip")?;
        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = self
            .client
            .post(&url)
            .header("x-auth-token", access_token)
            .header("x-auth-client", "stencil-cli")
            .multipart(form)
            .send()
            .await
            .context("Failed to send theme upload request")?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::json!({}));

        match status.as_u16() {
            201 => {
                let job_id = body["job_id"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("No job_id in upload response"))?
                    .to_string();
                Ok(job_id)
            }
            409 => {
                let msg = body["title"].as_str().unwrap_or("conflict");
                if msg.contains("upload limit") || msg.contains("reached") {
                    bail!("Theme upload limit reached. Delete a theme on BigCommerce and try again.");
                } else {
                    bail!("Upload conflict: {}. Another upload may be in progress.", msg);
                }
            }
            413 => bail!("Bundle is too large (>50 MB). Remove assets and try again."),
            s => bail!("Unexpected status {} from BigCommerce upload API: {:?}", s, body),
        }
    }

    /// Poll the status of a theme processing job.
    /// Returns `(status, percent_complete, Option<theme_id>)`.
    pub async fn get_theme_job(
        &self,
        store_hash: &str,
        access_token: &str,
        api_host: &str,
        job_id: &str,
    ) -> Result<(String, u64, Option<String>)> {
        let url = format!(
            "{}/stores/{}/v3/themes/jobs/{}",
            api_host.trim_end_matches('/'),
            store_hash,
            job_id
        );

        let resp = self
            .client
            .get(&url)
            .header("x-auth-token", access_token)
            .send()
            .await
            .context("Failed to poll theme job")?;

        let body: serde_json::Value = resp.json().await.context("Failed to parse job response")?;
        let data = &body["data"];

        let status = data["status"].as_str().unwrap_or("PENDING").to_string();
        let percent = data["percent_complete"].as_u64().unwrap_or(0);
        let theme_id = data["result"]["theme_id"].as_str().map(|s| s.to_string());

        Ok((status, percent, theme_id))
    }

    /// Fetch all theme variations for a given theme ID.
    /// Returns `Vec<(variation_uuid, variation_name)>`.
    pub async fn get_theme_variations(
        &self,
        store_hash: &str,
        access_token: &str,
        api_host: &str,
        theme_id: &str,
    ) -> Result<Vec<(String, String)>> {
        let url = format!(
            "{}/stores/{}/v3/themes/{}",
            api_host.trim_end_matches('/'),
            store_hash,
            theme_id
        );

        let resp = self
            .client
            .get(&url)
            .header("x-auth-token", access_token)
            .send()
            .await
            .context("Failed to get theme variations")?;

        let body: serde_json::Value = resp.json().await.context("Failed to parse theme response")?;

        let variations = body["data"]["variations"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|v| {
                let id = v["uuid"].as_str().or_else(|| v["id"].as_str())?;
                let name = v["name"].as_str().unwrap_or("Default");
                Some((id.to_string(), name.to_string()))
            })
            .collect();

        Ok(variations)
    }

    /// Activate a theme variation on a channel.
    pub async fn activate_theme(
        &self,
        store_hash: &str,
        access_token: &str,
        api_host: &str,
        variation_id: &str,
        channel_id: Option<u64>,
    ) -> Result<()> {
        let url = format!(
            "{}/stores/{}/v3/themes/actions/activate",
            api_host.trim_end_matches('/'),
            store_hash
        );

        let mut body = serde_json::json!({
            "variation_id": variation_id,
            "which": "original",
        });
        if let Some(ch) = channel_id {
            body["channel_id"] = serde_json::json!(ch);
        }

        let resp = self
            .client
            .post(&url)
            .header("x-auth-token", access_token)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to activate theme")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            bail!("Activation failed ({}): {}", status, text);
        }

        Ok(())
    }

    /// Fetch store settings locale
    pub async fn get_store_settings_locale(
        &self,
        store_hash: &str,
        access_token: &str,
        api_host: &str,
    ) -> Result<StoreSettingsLocale> {
        let url = format!(
            "{}/stores/{}/v3/settings/store/locale",
            api_host.trim_end_matches('/'),
            store_hash
        );

        let resp = self
            .client
            .get(&url)
            .header("x-auth-token", access_token)
            .send()
            .await
            .context("Failed to get store settings locale")?;

        let data: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse store settings locale")?;

        Ok(StoreSettingsLocale {
            default_shopper_language: data["data"]["default_shopper_language"]
                .as_str()
                .unwrap_or("en")
                .to_string(),
            shopper_language_selection_method: data["data"]["shopper_language_selection_method"]
                .as_str()
                .unwrap_or("default_shopper_language")
                .to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct StoreInfo {
    pub ssl_url: String,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct StoreChannel {
    pub channel_id: u32,
    pub url: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct StoreSettingsLocale {
    pub default_shopper_language: String,
    pub shopper_language_selection_method: String,
}
