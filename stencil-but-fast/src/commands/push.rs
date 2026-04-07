use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::env;
use std::io::{self, Write as IoWrite};
use std::path::PathBuf;
use std::time::Duration;

use crate::bundler::{create_bundle, BundleOptions};
use crate::config::theme_config::ThemeConfigManager;
use crate::config::StencilConfig;
use crate::proxy::BigCommerceClient;

pub struct PushOpts {
    /// Use an existing bundle zip instead of building one
    pub file: Option<PathBuf>,
    /// Activate the first variation after a successful upload
    pub activate: bool,
    /// Target channel ID for activation (default: first channel)
    pub channel_id: Option<u64>,
    /// Include `.js.map` files in the bundle
    pub source_maps: bool,
    /// Theme directory (default: current dir)
    pub work_dir: Option<PathBuf>,
}

pub async fn run(opts: PushOpts) -> Result<()> {
    let cwd = opts
        .work_dir
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let stencil_config = StencilConfig::load(&cwd)?.ok_or_else(|| {
        anyhow::anyhow!(
            "No stencil config found. Run {} first.",
            "stencil init".bold()
        )
    })?;

    let api_host = stencil_config.general.api_host.clone();
    let access_token = stencil_config.secrets.access_token.clone();
    let normal_store_url = stencil_config.general.normal_store_url.clone();

    let bc_client = BigCommerceClient::new()?;

    print!("{}", "Connecting to BigCommerce... ".dimmed());
    io::stdout().flush().ok();
    let store_hash = bc_client
        .get_store_hash(&normal_store_url)
        .await
        .context("Failed to get store hash — check your store URL and token")?;
    println!("{}", "✓".green());

    // ── Prepare bundle ────────────────────────────────────────────────────────
    let bundle_path = if let Some(path) = opts.file {
        println!("Using bundle: {}", path.display().to_string().cyan());
        path
    } else {
        let theme_config =
            ThemeConfigManager::load(&cwd).context("Failed to load config.json")?;
        let name = theme_config.config.name.clone();
        let version = theme_config.config.version.clone();
        let zip_name = if version.is_empty() {
            format!("{}.zip", name)
        } else {
            format!("{}-{}.zip", name, version)
        };
        let tmp_path = std::env::temp_dir().join(zip_name);

        println!("{}", "Bundling theme...".bold().cyan());
        let bundle_start = std::time::Instant::now();

        let bundle_opts = BundleOptions {
            theme_path: cwd.clone(),
            output_path: tmp_path.clone(),
            source_maps: opts.source_maps,
        };
        let result = tokio::task::spawn_blocking(move || create_bundle(&bundle_opts))
            .await?
            .context("Bundle creation failed")?;

        println!(
            "  {} files bundled in {:.2?}  ({:.2} MB)",
            result.file_count,
            bundle_start.elapsed(),
            result.size_bytes as f64 / 1_048_576.0
        );

        tmp_path
    };

    // ── Upload ────────────────────────────────────────────────────────────────
    println!("{}", "Uploading to BigCommerce...".bold().cyan());
    let upload_start = std::time::Instant::now();

    let job_id = bc_client
        .upload_theme(&store_hash, &access_token, &api_host, &bundle_path)
        .await
        .context("Theme upload failed")?;

    println!("  Upload complete in {:.2?}", upload_start.elapsed());

    // ── Poll job ──────────────────────────────────────────────────────────────
    println!("{}", "Processing theme on BigCommerce...".cyan());
    let theme_id =
        poll_job(&bc_client, &store_hash, &access_token, &api_host, &job_id).await?;

    println!(
        "{}  theme_id: {}",
        "Theme uploaded successfully!".bold().green(),
        theme_id.cyan()
    );

    // ── Activate ──────────────────────────────────────────────────────────────
    if opts.activate && !theme_id.is_empty() {
        println!("{}", "Activating theme...".cyan());

        let variations = bc_client
            .get_theme_variations(&store_hash, &access_token, &api_host, &theme_id)
            .await
            .unwrap_or_default();

        if let Some((variation_id, variation_name)) = variations.into_iter().next() {
            bc_client
                .activate_theme(
                    &store_hash,
                    &access_token,
                    &api_host,
                    &variation_id,
                    opts.channel_id,
                )
                .await
                .context("Theme activation failed")?;
            println!(
                "{}  variation: {}",
                "Theme activated!".bold().green(),
                variation_name.cyan()
            );
        } else {
            println!("{}", "No variations found to activate.".yellow());
        }
    }

    Ok(())
}

async fn poll_job(
    client: &BigCommerceClient,
    store_hash: &str,
    access_token: &str,
    api_host: &str,
    job_id: &str,
) -> Result<String> {
    for attempt in 0..180 {
        tokio::time::sleep(Duration::from_secs(2)).await;

        let (status, percent, theme_id) = client
            .get_theme_job(store_hash, access_token, api_host, job_id)
            .await
            .context("Failed to poll job status")?;

        match status.as_str() {
            "COMPLETED" => {
                println!(); // newline after progress dots
                return Ok(theme_id.unwrap_or_default());
            }
            "FAILED" => {
                println!();
                bail!("Theme processing failed on BigCommerce");
            }
            _ => {
                if attempt % 3 == 0 {
                    print!("\r  Processing... {:>3}%  ", percent);
                    io::stdout().flush().ok();
                }
            }
        }
    }

    bail!("Timed out after 6 minutes waiting for theme processing");
}
