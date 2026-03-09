use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{Input, Password};
use std::env;

use crate::config::{CustomLayouts, StencilConfig, StencilGeneralConfig, StencilSecretsConfig};

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_API_HOST: &str = "https://api.bigcommerce.com";

pub fn run(
    url: Option<String>,
    token: Option<String>,
    port: Option<u16>,
    api_host: Option<String>,
) -> Result<()> {
    let cwd = env::current_dir()?;

    println!(
        "{}",
        "Initializing stencil configuration...".bold().cyan()
    );

    // Load existing config if present
    let existing = StencilConfig::load(&cwd)?;

    // Prompt for values not provided via CLI flags
    let store_url = match url {
        Some(u) => u,
        None => {
            let default = existing
                .as_ref()
                .map(|c| c.general.normal_store_url.clone())
                .unwrap_or_default();

            let mut prompt = Input::<String>::new()
                .with_prompt("What is the URL of your store's home page?");

            if !default.is_empty() {
                prompt = prompt.default(default);
            }

            prompt
                .validate_with(|input: &String| -> Result<(), &str> {
                    if input.starts_with("http://") || input.starts_with("https://") {
                        Ok(())
                    } else {
                        Err("URL must start with http:// or https://")
                    }
                })
                .interact_text()?
        }
    };

    let access_token = match token {
        Some(t) => t,
        None => {
            let default = existing
                .as_ref()
                .map(|c| c.secrets.access_token.clone())
                .unwrap_or_default();

            let mut prompt = Input::<String>::new()
                .with_prompt("What is your OAuth access token?");

            if !default.is_empty() {
                prompt = prompt.default(default);
            }

            prompt.interact_text()?
        }
    };

    let port_val = match port {
        Some(p) => {
            validate_port(p)?;
            p
        }
        None => {
            let default = existing
                .as_ref()
                .map(|c| c.general.port)
                .unwrap_or(DEFAULT_PORT);

            let input: String = Input::new()
                .with_prompt("What port would you like to run the dev server on?")
                .default(default.to_string())
                .validate_with(|input: &String| -> Result<(), String> {
                    match input.parse::<u16>() {
                        Ok(p) if (1025..=65535).contains(&p) => Ok(()),
                        Ok(_) => Err("Port must be between 1025 and 65535".into()),
                        Err(_) => Err("Invalid port number".into()),
                    }
                })
                .interact_text()?;

            input.parse::<u16>()?
        }
    };

    let api_host_val = api_host.unwrap_or_else(|| {
        existing
            .as_ref()
            .map(|c| c.general.api_host.clone())
            .unwrap_or_else(|| DEFAULT_API_HOST.to_string())
    });

    let custom_layouts = existing
        .as_ref()
        .map(|c| c.general.custom_layouts.clone())
        .unwrap_or_default();

    // Normalize URL - strip trailing slash
    let normalized_url = store_url.trim_end_matches('/').to_string();

    let config = StencilConfig {
        general: StencilGeneralConfig {
            normal_store_url: normalized_url,
            port: port_val,
            api_host: api_host_val,
            custom_layouts,
        },
        secrets: StencilSecretsConfig {
            access_token,
            github_token: existing.and_then(|c| c.secrets.github_token),
        },
    };

    config.save(&cwd)?;

    println!();
    println!(
        "{} Configuration saved!",
        "Done!".bold().green()
    );
    println!(
        "  {} {}",
        "General:".dimmed(),
        StencilConfig::general_config_path(&cwd).display()
    );
    println!(
        "  {} {}",
        "Secrets:".dimmed(),
        StencilConfig::secrets_config_path(&cwd).display()
    );
    println!();
    println!("You may now run {} to start developing.", "stencil start".bold());

    Ok(())
}

fn validate_port(port: u16) -> Result<()> {
    if !(1025..=65535).contains(&port) {
        bail!("Port must be between 1025 and 65535, got {}", port);
    }
    Ok(())
}
