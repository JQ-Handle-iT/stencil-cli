use anyhow::{Context, Result};
use handlebars::Handlebars;
use std::collections::HashMap;

use super::frontmatter;

/// Paper-equivalent engine: Handlebars registry with custom helpers
pub struct PaperEngine<'a> {
    registry: Handlebars<'a>,
    translations: HashMap<String, serde_json::Value>,
    regions: HashMap<String, String>,
}

impl<'a> PaperEngine<'a> {
    pub fn new() -> Self {
        let mut registry = Handlebars::new();
        registry.set_strict_mode(false);
        // Don't escape HTML by default (Stencil templates handle this)
        registry.register_escape_fn(handlebars::no_escape);

        Self {
            registry,
            translations: HashMap::new(),
            regions: HashMap::new(),
        }
    }

    pub fn set_translations(&mut self, translations: HashMap<String, serde_json::Value>) {
        self.translations = translations;
    }

    pub fn set_regions(&mut self, regions: HashMap<String, String>) {
        self.regions = regions;
    }

    /// Load templates as partials in the Handlebars registry
    pub fn load_templates(&mut self, templates: &HashMap<String, String>) -> Result<()> {
        for (name, content) in templates {
            // Strip frontmatter before registering
            let clean = frontmatter::strip_frontmatter(content);
            if let Err(e) = self.registry.register_template_string(name, &clean) {
                tracing::warn!("Failed to register template '{}': {}", name, e);
            }
        }
        Ok(())
    }

    /// Register custom Handlebars helpers for BigCommerce compatibility
    pub fn register_helpers(&mut self) {
        // {{lang}} helper - language string lookup
        let translations = self.translations.clone();
        self.registry.register_helper(
            "lang",
            Box::new(
                move |h: &handlebars::Helper,
                      _: &Handlebars,
                      _: &handlebars::Context,
                      _: &mut handlebars::RenderContext,
                      out: &mut dyn handlebars::Output|
                      -> handlebars::HelperResult {
                    let key = h
                        .param(0)
                        .and_then(|v| v.value().as_str())
                        .unwrap_or("");

                    // Look up key in translations (try "en" locale as default)
                    let result = translations
                        .get("en")
                        .and_then(|locale| {
                            // Navigate dotted key path
                            let mut current = locale;
                            for part in key.split('.') {
                                current = current.get(part)?;
                            }
                            current.as_str().map(String::from)
                        })
                        .unwrap_or_else(|| key.to_string());

                    out.write(&result)?;
                    Ok(())
                },
            ),
        );

        // {{region}} helper - content region
        let regions = self.regions.clone();
        self.registry.register_helper(
            "region",
            Box::new(
                move |h: &handlebars::Helper,
                      _: &Handlebars,
                      _: &handlebars::Context,
                      _: &mut handlebars::RenderContext,
                      out: &mut dyn handlebars::Output|
                      -> handlebars::HelperResult {
                    let name = h
                        .param(0)
                        .and_then(|v| v.value().as_str())
                        .unwrap_or("");
                    if let Some(html) = regions.get(name) {
                        out.write(html)?;
                    }
                    Ok(())
                },
            ),
        );

        // {{stylesheet}} helper - outputs <link> tag
        self.registry.register_helper(
            "stylesheet",
            Box::new(
                |h: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 out: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    let path = h
                        .param(0)
                        .and_then(|v| v.value().as_str())
                        .unwrap_or("");
                    out.write(&format!(
                        r#"<link data-stencil-stylesheet href="/{}"/>"#,
                        path.trim_start_matches('/')
                    ))?;
                    Ok(())
                },
            ),
        );

        // {{getFonts}} helper - outputs font imports
        self.registry.register_helper(
            "getFonts",
            Box::new(
                |_: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 out: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    // MVP: output empty, fonts loaded via CSS
                    out.write("")?;
                    Ok(())
                },
            ),
        );

        // {{inject}} helper - stores a value for later use by {{jsContext}}
        self.registry.register_helper(
            "inject",
            Box::new(
                |_: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 _: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    // MVP: no-op, jsContext will output empty
                    Ok(())
                },
            ),
        );

        // {{jsContext}} helper - outputs injected values as JSON script tag
        self.registry.register_helper(
            "jsContext",
            Box::new(
                |_: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 out: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    out.write(r#"<script>window.jsContext = JSON.parse("{}");</script>"#)?;
                    Ok(())
                },
            ),
        );

        // {{cdn}} helper - CDN URL resolution (local dev = relative path)
        self.registry.register_helper(
            "cdn",
            Box::new(
                |h: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 out: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    let path = h
                        .param(0)
                        .and_then(|v| v.value().as_str())
                        .unwrap_or("");
                    // In dev, CDN is just the local path
                    out.write(&format!("/{}", path.trim_start_matches('/')))?;
                    Ok(())
                },
            ),
        );

        // {{getImage}} helper
        self.registry.register_helper(
            "getImage",
            Box::new(
                |h: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 out: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    let url = h
                        .param(0)
                        .and_then(|v| v.value().as_str())
                        .unwrap_or("");
                    let size = h
                        .param(1)
                        .and_then(|v| v.value().as_str())
                        .unwrap_or("original");
                    // Simple pass-through for local dev
                    if url.contains("{:size}") {
                        out.write(&url.replace("{:size}", size))?;
                    } else {
                        out.write(url)?;
                    }
                    Ok(())
                },
            ),
        );

        // Register no-op helpers for commonly used ones that don't need implementation
        for name in &[
            "getFontLoaderConfig",
            "getContentImage",
            "getContentImageSrcset",
            "getImageSrcset",
            "getImageManagerImage",
            "getImageManagerImageSrcset",
        ] {
            self.registry.register_helper(
                name,
                Box::new(
                    |_: &handlebars::Helper,
                     _: &Handlebars,
                     _: &handlebars::Context,
                     _: &mut handlebars::RenderContext,
                     out: &mut dyn handlebars::Output|
                     -> handlebars::HelperResult {
                        out.write("")?;
                        Ok(())
                    },
                ),
            );
        }
    }

    /// Render a template by name with the given context
    pub fn render(
        &self,
        template_name: &str,
        context: &serde_json::Value,
    ) -> Result<String> {
        self.registry
            .render(template_name, context)
            .with_context(|| format!("Failed to render template '{}'", template_name))
    }
}
