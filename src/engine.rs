use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use chrono::Local;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        UnixListener, UnixStream,
        unix::{OwnedWriteHalf, SocketAddr},
    },
    signal::ctrl_c,
    sync::Notify,
    time::sleep,
};

use crate::gamma_relay_interface::{GammaRelayInterfaceProxy, get_property, set_property};
use crate::message::{GammaRelayProperty, Message, ShiftMessage, TargetValue};
use crate::{DEFAULT_BRIGHTNESS, DEFAULT_GAMMA, DEFAULT_TEMPERATURE};

#[tokio::main]
pub async fn start_engine(sock: impl Into<PathBuf>) -> tokio::io::Result<()> {
    let path = sock.into();
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(&dir).await?;
    }
    let listener = UnixListener::bind(&path)?;
    let shutdown = Arc::new(Notify::new());

    tracing::info!("[GAMMASHIFT] Ready for messages");

    loop {
        tokio::select! {
            a = listener.accept() => handle_accept(a, &shutdown, path.clone()).await,
            _ = shutdown.notified() => break,
            _ = ctrl_c() => {
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
            Ok(Message::Shift(shift_message)) => handle_shift(&shift_message, writer).await,
            Ok(Message::Stop) => handle_stop(&shutdown, Some(writer), sock_path).await,
        }
    });
}

async fn handle_stop(
    shutdown: &Arc<Notify>,
    response_writer: Option<OwnedWriteHalf>,
    sock_path: impl AsRef<Path>,
) {
    let response = "[GAMMASHIFT] Shutting down";
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

async fn handle_shift(
    ShiftMessage {
        property,
        target,
        step_duration,
        start: start_time,
        end: end_time,
    }: &ShiftMessage,
    mut writer: OwnedWriteHalf,
) {
    let step_duration = *step_duration;
    let start_time = *start_time;
    let end_time = *end_time;

    let dbus_connection = match zbus::Connection::session().await {
        Err(e) => return tracing::error!("Error establishing d-bus connection: {e}"),
        Ok(connection) => connection,
    };
    let gammarelay_proxy = match GammaRelayInterfaceProxy::new(&dbus_connection).await {
        Err(e) => return tracing::error!("Error setting up d-bus proxy: {e}"),
        Ok(proxy) => proxy,
    };

    let current = match get_property(&gammarelay_proxy, property).await {
        Err(e) => return tracing::error!("Error fetching current value: {e}"),
        Ok(val) => val,
    };

    let target = match target {
        TargetValue::Absolute(v) => *v,
        TargetValue::Relative(r) => current + r,
        TargetValue::Undo => todo!("Implement Undo"),
        TargetValue::Default => match &property {
            GammaRelayProperty::Temperature => DEFAULT_TEMPERATURE,
            GammaRelayProperty::Brightness => DEFAULT_BRIGHTNESS,
            GammaRelayProperty::Gamma => DEFAULT_GAMMA,
        },
    };
    let duration = Duration::from_secs_f64((end_time - start_time).as_seconds_f64());
    let response = format!("[STARTED] {property}: {current} -> {target} ({duration:?})");
    if let Err(e) = writer.write_all(response.as_bytes()).await {
        tracing::error!("Error writing to stream: {e}");
    }
    drop(writer);
    tracing::info!("{response}");

    loop {
        let now = Local::now();
        if now > end_time {
            match set_property(&gammarelay_proxy, property, target).await {
                Err(e) => return tracing::error!("Failed to set property: {e}"),
                Ok(_) => return tracing::info!("[DONE] {property}: {target}"),
            }
        }
        let elapsed = now - start_time;
        let factor = elapsed.as_seconds_f64() / duration.as_secs_f64();
        let step_target = current + factor * (target - current);
        match set_property(&gammarelay_proxy, property, step_target).await {
            Err(e) => return tracing::error!("Failed to set property: {e}"),
            Ok(_) => tracing::debug!("[STEP] {property}:{step_target}"),
        }
        sleep(step_duration).await;
    }
}
