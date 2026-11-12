use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        UnixListener, UnixStream,
        unix::{OwnedWriteHalf, SocketAddr},
    },
    signal::unix::{SignalKind, signal},
    sync::Notify,
};

use crate::message::Message;

mod shift;

#[tokio::main]
pub async fn start_engine(sock: impl Into<PathBuf>) -> tokio::io::Result<()> {
    let path = sock.into();
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(&dir).await?;
    }
    let listener = UnixListener::bind(&path)?;
    let shutdown = Arc::new(Notify::new());

    let mut sigint = signal(SignalKind::interrupt())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    tracing::info!("Ready for messages");

    loop {
        tokio::select! {
            a = listener.accept() => handle_accept(a, &shutdown, path.clone()).await,
            _ = shutdown.notified() => break,

            _ = sigint.recv() => {
                tracing::info!("Received SIGINT");
                handle_stop(&shutdown, None, path.clone()).await;
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM");
                handle_stop(&shutdown, None, path.clone()).await;
                break;
            }
        }
    }

    Ok(())
}

async fn handle_accept(
    accepted: tokio::io::Result<(UnixStream, SocketAddr)>,
    shutdown: &Arc<Notify>,
    sock_path: PathBuf,
) {
    let stream = match accepted {
        Ok((stream, _)) => stream,
        Err(e) => {
            tracing::error!("Error accepting from unix listener: {e}");
            return;
        }
    };

    let shutdown = Arc::clone(shutdown);
    tokio::spawn(async move {
        let (reader, writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let mut message = String::new();
        if let Err(e) = reader.read_line(&mut message).await {
            return tracing::error!("Error reading stream: {e}");
        }
        match serde_json::from_str::<Message>(&message) {
            Err(e) => tracing::error!("Error decoding message: {e}"),
            Ok(Message::Shift(shift_message)) => {
                tracing::info!("Received Shift message");
                shift::handle_shift(&shift_message, writer).await
            }
            Ok(Message::Stop) => {
                tracing::info!("Received Stop message");
                handle_stop(&shutdown, Some(writer), sock_path).await
            }
        }
    });
}

async fn handle_stop(
    shutdown: &Arc<Notify>,
    response_writer: Option<OwnedWriteHalf>,
    sock_path: impl AsRef<Path>,
) {
    let response = "Shutting down";
    if let Some(mut writer) = response_writer
        && let Err(e) = writer.write_all(response.as_bytes()).await
    {
        tracing::error!("Error writing to stream: {e}");
    };
    tracing::info!("{response}");
    if let Err(e) = tokio::fs::remove_file(sock_path).await {
        tracing::error!("Error removing socket: {e}");
    }
    shutdown.notify_waiters();
}
