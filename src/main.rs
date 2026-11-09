use std::path::PathBuf;

use clap::Parser;
use tokio::net::UnixListener;

use crate::cli::Cli;

mod cli;

const SOCK_FILE: &str = "/run/user/1000/gammashift/gammashift.sock";

fn main() {
    let cli = Cli::parse();
    tracing_subscriber::fmt().init();

    match cli.command {
        cli::GammaShiftCommand::Start { .. } => {
            if let Err(e) = start_engine() {
                tracing::error!("Error starting gammashift engine: {e}")
            }
        }
        cli::GammaShiftCommand::Msg {
            property,
            target_value,
            time,
        } => todo!(),
    }
}

#[tokio::main]
async fn start_engine() -> tokio::io::Result<()> {
    let path = PathBuf::from(SOCK_FILE);
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(&dir).await?;
    }
    let listener = UnixListener::bind(SOCK_FILE)?;
    loop {
        match listener.accept().await {
            Err(e) => tracing::error!("Error accepting from unix listener: {e}"),
            Ok((stream, addr)) => tracing::info!("stream: {stream:?}, addr: {addr:?}"),
        }
    }
}
