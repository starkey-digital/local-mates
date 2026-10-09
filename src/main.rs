mod host;
mod join;
mod link;
mod packet;
mod tun;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use iroh::{Endpoint, EndpointId, endpoint::presets};

/// Dead-simple virtual LAN for playing LAN games with friends.
#[derive(Parser)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start a session and get a code to share
    Host,
    /// Join a friend's session with their code
    Join { code: EndpointId },
}

fn main() -> Result<()> {
    // Must run first: Velopack may handle install/update hooks here and exit or restart.
    velopack::VelopackApp::build().run();
    run()
}

#[tokio::main]
async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "local_mates=info".into()),
        )
        .init();
    let cli = Cli::parse();
    update::spawn_check();

    // Fresh identity every run: sessions are throwaway, nothing to remember.
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![link::ALPN.to_vec()])
        .bind()
        .await?;
    endpoint.online().await;

    let session = async {
        match cli.command {
            Command::Host => host::run(endpoint.clone()).await,
            Command::Join { code } => join::run(endpoint.clone(), code).await,
        }
    };

    let res = tokio::select! {
        res = session => res,
        _ = tokio::signal::ctrl_c() => Ok(()),
    };
    endpoint.close().await;
    res
}
