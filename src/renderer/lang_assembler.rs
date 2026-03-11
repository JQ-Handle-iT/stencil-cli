use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

/// Load all lang/*.json files into a map of locale -> parsed JSON
pub async fn assemble(theme_path: &Path) -> Result<HashMap<String, serde_json::Value>> {
    let lang_dir = theme_path.join("lang");
    let mut translations = HashMap::new();

    if !lang_dir.exists() {
        return Ok(translations);
    }

    let mut entries = tokio::fs::read_dir(&lang_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let locale = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("en")
                .to_string();

            match tokio::fs::read_to_string(&path).await {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(parsed) => {
                        translations.insert(locale, parsed);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse lang file {}: {}", path.display(), e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read lang file {}: {}", path.display(), e);
                }
            }
        }
    }

    Ok(translations)
}
