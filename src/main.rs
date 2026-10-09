mod client;
mod daemon;
mod host;
mod ipc;
mod join;
mod link;
mod packet;
#[cfg(windows)]
mod service;
mod session;
mod tun;
mod update;

use anyhow::Result;
use clap::{Parser, Subcommand};
use iroh::EndpointId;

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
    /// End the current session
    Leave,
    /// Run the background daemon that owns the network adapter (needs admin/root)
    Daemon {
        /// Started by the Windows Service Control Manager
        #[arg(long, hide = true)]
        service: bool,
    },
    /// Install or remove the background service (Windows, needs admin)
    #[cfg(windows)]
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[cfg(windows)]
#[derive(Subcommand)]
enum ServiceAction {
    /// Install or update the service from this copy of local mates
    Install,
    Uninstall,
}

fn main() -> Result<()> {
    // Must run first: Velopack may handle install/update hooks here and exit or restart.
    velopack::VelopackApp::build().run();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "local_mates=info".into()),
        )
        .init();

    match Cli::parse().command {
        // The SCM dispatcher blocks and builds its own runtime.
        #[cfg(windows)]
        Command::Daemon { service: true } => service::run(),
        #[cfg(windows)]
        Command::Service { action } => match action {
            ServiceAction::Install => service::install(),
            ServiceAction::Uninstall => service::uninstall(),
        },
        command => run(command),
    }
}

#[tokio::main]
async fn run(command: Command) -> Result<()> {
    match command {
        Command::Host => {
            update::spawn_check();
            client::session(ipc::Request::Host).await
        }
        Command::Join { code } => {
            update::spawn_check();
            client::session(ipc::Request::Join {
                code: code.to_string(),
            })
            .await
        }
        Command::Leave => client::leave().await,
        Command::Daemon { .. } => daemon::run(ipc::listen()?).await,
        #[cfg(windows)]
        Command::Service { .. } => unreachable!("handled in main"),
    }
}
