use regex::Regex;

lazy_static::lazy_static! {
    /// Matches YAML front-matter block: ---\n...\n---\n
    pub static ref FRONTMATTER_REGEX: Regex = Regex::new(r"(?s)^---\r?\n[\S\s]*?\r?\n---\r?\n").unwrap();

    /// Matches theme_settings interpolation: {{ theme_settings.key }}
    static ref THEME_SETTINGS_REGEX: Regex = Regex::new(r"\{\{\s*theme_settings\.(.+?)\s*\}\}").unwrap();
}

/// Extract the front-matter content (the YAML block between --- markers)
pub fn get_frontmatter_content(template: &str) -> Option<String> {
    FRONTMATTER_REGEX.find(template).map(|m| m.as_str().to_string())
}

/// Interpolate {{ theme_settings.X }} in front-matter with actual values
pub fn interpolate_theme_settings(
    frontmatter: &str,
    settings: &serde_json::Value,
) -> String {
    THEME_SETTINGS_REGEX
        .replace_all(frontmatter, |caps: &regex::Captures| {
            let key = &caps[1];
            settings
                .get(key)
                .and_then(|v| match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => Some(n.to_string()),
                    serde_json::Value::Bool(b) => Some(b.to_string()),
                    _ => Some(v.to_string()),
                })
                .unwrap_or_default()
        })
        .to_string()
}

/// Strip front-matter from template, returning just the template body
pub fn strip_frontmatter(template: &str) -> String {
    FRONTMATTER_REGEX.replace(template, "").to_string()
}

/// Parse YAML front-matter into a serde_json::Value (the resource config)
pub fn parse_frontmatter(template: &str) -> Option<serde_json::Value> {
    let fm = get_frontmatter_content(template)?;
    // Strip the --- markers
    let yaml_content = fm
        .trim()
        .strip_prefix("---")?
        .rsplit_once("---")?
        .0
        .trim();

    // Parse YAML as JSON value
    serde_json::from_str(&serde_yaml::to_string(
        &serde_yaml::from_str::<serde_yaml::Value>(yaml_content).ok()?,
    ).ok()?)
    .ok()
}
