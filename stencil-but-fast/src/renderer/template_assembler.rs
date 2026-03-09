use anyhow::{Context, Result};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

lazy_static::lazy_static! {
    static ref PARTIAL_REGEX: Regex = Regex::new(r"\{\{#?>\s*([_\-a-zA-Z0-9@'\x22/]+)[^{]*?\}\}").unwrap();
    static ref DYNAMIC_COMPONENT_REGEX: Regex = Regex::new(r#"\{\{\s*?dynamicComponent\s*(?:'|")([_\-a-zA-Z0-9/]+)(?:'|").*?\}\}"#).unwrap();
}

const PARTIAL_BLOCK: &str = "@partial-block";
const PACKAGE_MARKER: &str = "external/";

/// Recursively assemble templates starting from the given template name.
/// Returns a map of template_name -> template_content for all referenced partials.
pub async fn assemble(
    templates_folder: &Path,
    template_name: &str,
) -> Result<HashMap<String, String>> {
    let mut templates = HashMap::new();
    let mut visited = HashSet::new();
    resolve_partials(templates_folder, template_name, &mut templates, &mut visited).await?;
    Ok(templates)
}

/// Read a single template file synchronously
pub fn get_template_content_sync(templates_folder: &Path, template_file: &str) -> Result<String> {
    let path = templates_folder.join(format!("{}.html", template_file));
    std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read template: {}", path.display()))
}

fn get_custom_path(templates_folder: &Path, template_name: &str) -> PathBuf {
    if template_name.starts_with(PACKAGE_MARKER) {
        let custom_dir = templates_folder
            .parent()
            .unwrap_or(templates_folder)
            .join("node_modules");
        let custom_name = &template_name[PACKAGE_MARKER.len()..];
        custom_dir.join(format!("{}.html", custom_name))
    } else {
        templates_folder.join(format!("{}.html", template_name))
    }
}

fn trim_partial(partial: &str) -> String {
    let trimmed = partial.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

#[async_recursion::async_recursion]
async fn resolve_partials(
    templates_folder: &Path,
    template_name: &str,
    templates: &mut HashMap<String, String>,
    visited: &mut HashSet<String>,
) -> Result<()> {
    let clean_name = trim_partial(template_name);

    if visited.contains(&clean_name) || clean_name == PARTIAL_BLOCK {
        return Ok(());
    }
    visited.insert(clean_name.clone());

    let template_path = get_custom_path(templates_folder, &clean_name);
    let content = match tokio::fs::read_to_string(&template_path).await {
        Ok(c) => c,
        Err(_) => {
            tracing::warn!("Template not found: {}", template_path.display());
            return Ok(());
        }
    };

    templates.insert(clean_name.clone(), content.clone());

    // Find partial references
    let mut partials_to_resolve = Vec::new();

    for cap in PARTIAL_REGEX.captures_iter(&content) {
        let partial = trim_partial(&cap[1]);
        if partial != PARTIAL_BLOCK && !visited.contains(&partial) {
            partials_to_resolve.push(partial);
        }
    }

    // Find dynamic component directories
    for cap in DYNAMIC_COMPONENT_REGEX.captures_iter(&content) {
        let component_dir = &cap[1];
        let dir_path = templates_folder.join(component_dir);
        if let Ok(mut entries) = tokio::fs::read_dir(&dir_path).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".html") {
                        let partial_name = format!(
                            "{}/{}",
                            component_dir,
                            name.strip_suffix(".html").unwrap()
                        );
                        if !visited.contains(&partial_name) {
                            partials_to_resolve.push(partial_name);
                        }
                    }
                }
            }
        }
    }

    // Handle external template path prefix
    if clean_name.starts_with(PACKAGE_MARKER) {
        let base_route = clean_name.split('/').take(3).collect::<Vec<_>>().join("/");
        partials_to_resolve = partials_to_resolve
            .into_iter()
            .map(|p| {
                if p.starts_with(PACKAGE_MARKER) {
                    p
                } else {
                    format!("{}/{}", base_route, p)
                }
            })
            .collect();
    }

    for partial in partials_to_resolve {
        resolve_partials(templates_folder, &partial, templates, visited).await?;
    }

    Ok(())
}
