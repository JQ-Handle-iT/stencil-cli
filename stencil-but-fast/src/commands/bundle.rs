use anyhow::{Context, Result};
use colored::Colorize;
use std::env;
use std::path::PathBuf;

use crate::bundler::{create_bundle, sanitize_filename, BundleOptions};
use crate::config::theme_config::ThemeConfigManager;

pub struct BundleOpts {
    /// Directory to write the ZIP into (default: current dir)
    pub dest: Option<PathBuf>,
    /// Override the output filename
    pub name: Option<String>,
    /// Include `.js.map` files
    pub source_maps: bool,
    /// Theme directory (default: current dir)
    pub work_dir: Option<PathBuf>,
}

pub async fn run(opts: BundleOpts) -> Result<()> {
    let cwd = opts
        .work_dir
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let theme_config = ThemeConfigManager::load(&cwd)
        .context("Failed to load config.json — is this a Stencil theme directory?")?;

    let zip_name = opts.name.unwrap_or_else(|| {
        let name = sanitize_filename(&theme_config.config.name);
        let version = sanitize_filename(&theme_config.config.version);
        if version.is_empty() {
            format!("{}.zip", name)
        } else {
            format!("{}-{}.zip", name, version)
        }
    });

    let dest_dir = opts.dest.unwrap_or_else(|| cwd.clone());
    let output_path = dest_dir.join(&zip_name);

    println!("{}", "Bundling theme...".bold().cyan());
    let start = std::time::Instant::now();

    let bundle_opts = BundleOptions {
        theme_path: cwd,
        output_path: output_path.clone(),
        source_maps: opts.source_maps,
    };

    let result = tokio::task::spawn_blocking(move || create_bundle(&bundle_opts))
        .await?
        .context("Bundle creation failed")?;

    let elapsed = start.elapsed();
    let size_mb = result.size_bytes as f64 / 1_048_576.0;

    println!();
    println!("{}", "Bundle created successfully!".bold().green());
    println!("  Path:  {}", output_path.display().to_string().cyan());
    println!("  Files: {}", result.file_count.to_string().yellow());
    println!("  Size:  {:.2} MB", size_mb);
    println!("  Time:  {:.2?}", elapsed);
    println!();

    Ok(())
}
