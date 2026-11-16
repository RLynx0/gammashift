use std::{path::PathBuf, sync::Arc};

use tokio::{
    net::UnixListener,
    signal::unix::{Signal, SignalKind, signal},
    sync::{Notify, mpsc},
};

use self::coordinator::start_coordinator;
use self::listener::start_listener;
use crate::gamma_relay_interface::GammaRelayInterfaceProxy;

mod coordinator;
mod listener;
mod shift;

#[tokio::main]
pub async fn start_engine(sock: impl Into<PathBuf>) -> tokio::io::Result<()> {
    let path = sock.into();
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(&dir).await?;
    }
    let connection = zbus::Connection::session()
        .await
        .map_err(std::io::Error::other)?;
    let proxy = GammaRelayInterfaceProxy::builder(&connection)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .map_err(std::io::Error::other)?;
    let listener = UnixListener::bind(&path)?;
    let shutdown = Arc::new(Notify::new());
    let (tx, rx) = mpsc::channel(128);

    let sigint = signal(SignalKind::interrupt())?;
    let sigterm = signal(SignalKind::terminate())?;
    let unix_signals = UnixSignals { sigterm, sigint };

    tracing::info!("Ready for messages");
    tokio::spawn(start_listener(listener, Arc::clone(&shutdown), tx));
    start_coordinator(rx, unix_signals, shutdown, Arc::from(path), proxy).await;

    Ok(())
}

struct UnixSignals {
    sigterm: Signal,
    sigint: Signal,
}
