use anyhow::Result;
use clap::{Parser, Subcommand};

mod bundler;
mod commands;
mod config;
mod server;
mod renderer;
mod proxy;
mod cache;
mod watcher;
mod utils;
mod stats;
mod tui;

#[derive(Parser)]
#[command(name = "stencil", version, about = "BigCommerce Stencil CLI (Rust) - Fast local theme development")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize stencil configuration for a theme directory
    Init {
        /// Store URL (e.g. https://mystore.mybigcommerce.com)
        #[arg(short = 'u', long)]
        url: Option<String>,

        /// OAuth access token
        #[arg(short = 't', long)]
        token: Option<String>,

        /// Dev server port (1025-65535, default 3000)
        #[arg(short = 'p', long)]
        port: Option<u16>,

        /// BigCommerce API host override
        #[arg(long)]
        api_host: Option<String>,
    },

    /// Start the local development server
    Start {
        /// Automatically open default browser
        #[arg(short = 'o', long)]
        open: bool,

        /// Theme variation to use
        #[arg(short = 'v', long)]
        variation: Option<String>,

        /// Channel ID for the storefront
        #[arg(short = 'c', long)]
        channel_id: Option<u64>,

        /// Custom domain URL to bypass DNS/proxy protection
        #[arg(long)]
        channel_url: Option<String>,

        /// Disable API resource caching
        #[arg(short = 'n', long)]
        no_cache: bool,

        /// Override dev server port
        #[arg(short = 'p', long)]
        port: Option<u16>,

        /// Working directory (theme directory) - defaults to current directory
        #[arg(long)]
        work_dir: Option<String>,

        /// Launch the interactive TUI dashboard (request stats, live-reload log, keybindings)
        #[arg(long)]
        gui: bool,
    },

    /// Bundle the theme into a zip file for upload
    Bundle {
        /// Output directory for the zip (default: current directory)
        #[arg(short = 'd', long)]
        dest: Option<String>,

        /// Override the output filename
        #[arg(short = 'n', long)]
        name: Option<String>,

        /// Include .js.map source map files
        #[arg(short = 'S', long)]
        source_maps: bool,

        /// Theme directory (default: current directory)
        #[arg(long)]
        work_dir: Option<String>,
    },

    /// Bundle and upload the theme to BigCommerce
    Push {
        /// Use an existing bundle zip (skip bundling step)
        #[arg(short = 'f', long)]
        file: Option<String>,

        /// Activate the first variation after upload
        #[arg(short = 'a', long)]
        activate: bool,

        /// Target channel ID for activation
        #[arg(short = 'c', long)]
        channel_id: Option<u64>,

        /// Include .js.map source map files in the bundle
        #[arg(short = 'S', long)]
        source_maps: bool,

        /// Theme directory (default: current directory)
        #[arg(long)]
        work_dir: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Don't init the tracing subscriber in GUI mode — it would write raw log
    // lines to stdout and corrupt the ratatui terminal display.
    let is_gui = matches!(&cli.command, Commands::Start { gui: true, .. });
    if !is_gui {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .init();
    }

    match cli.command {
        Commands::Init {
            url,
            token,
            port,
            api_host,
        } => commands::init::run(url, token, port, api_host),

        Commands::Start {
            open,
            variation,
            channel_id,
            channel_url,
            no_cache,
            port,
            work_dir,
            gui,
        } => {
            commands::start::run(commands::start::StartOptions {
                open,
                variation,
                channel_id,
                channel_url,
                no_cache,
                port,
                work_dir: work_dir.map(std::path::PathBuf::from),
                gui,
            })
            .await
        }

        Commands::Bundle {
            dest,
            name,
            source_maps,
            work_dir,
        } => {
            commands::bundle::run(commands::bundle::BundleOpts {
                dest: dest.map(std::path::PathBuf::from),
                name,
                source_maps,
                work_dir: work_dir.map(std::path::PathBuf::from),
            })
            .await
        }

        Commands::Push {
            file,
            activate,
            channel_id,
            source_maps,
            work_dir,
        } => {
            commands::push::run(commands::push::PushOpts {
                file: file.map(std::path::PathBuf::from),
                activate,
                channel_id,
                source_maps,
                work_dir: work_dir.map(std::path::PathBuf::from),
            })
            .await
        }
    }
}
