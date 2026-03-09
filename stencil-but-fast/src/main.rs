use anyhow::Result;
use clap::{Parser, Subcommand};

mod commands;
mod config;
mod server;
mod renderer;
mod proxy;
mod cache;
mod watcher;
mod utils;

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
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

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
        } => {
            commands::start::run(commands::start::StartOptions {
                open,
                variation,
                channel_id,
                channel_url,
                no_cache,
                port,
                work_dir: work_dir.map(std::path::PathBuf::from),
            })
            .await
        }
    }
}
