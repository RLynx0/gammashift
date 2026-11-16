use std::{
    io,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::{
    net::{UnixListener, UnixStream},
    signal::unix::{Signal, SignalKind, signal},
    sync::{Notify, mpsc},
};

use self::coordinator::start_coordinator;
use self::listener::start_listener;
use crate::gamma_relay_interface::GammaRelayInterfaceProxy;

mod coordinator;
mod listener;
mod shift;

#[cfg(test)]
mod tests;

#[tokio::main]
pub async fn start_engine(sock: impl Into<PathBuf>) -> tokio::io::Result<()> {
    let path = sock.into();
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    prepare_socket(&path).await?;
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

async fn prepare_socket(path: &Path) -> io::Result<()> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("refusing to replace non-socket file at {}", path.display()),
        ));
    }

    match UnixStream::connect(path).await {
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("gammashift is already listening at {}", path.display()),
        )),
        Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
            match tokio::fs::symlink_metadata(path).await {
                Ok(current)
                    if current.file_type().is_socket()
                        && current.dev() == metadata.dev()
                        && current.ino() == metadata.ino() =>
                {
                    tokio::fs::remove_file(path).await
                }
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!(
                        "socket at {} changed while checking for a stale listener",
                        path.display()
                    ),
                )),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    }
}

struct UnixSignals {
    sigterm: Signal,
    sigint: Signal,
}
