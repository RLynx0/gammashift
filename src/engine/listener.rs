use std::sync::Arc;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream, unix::SocketAddr},
    sync::{Notify, mpsc::Sender, oneshot},
};

use crate::{engine::coordinator::Instruction, message::Message};

#[cfg(test)]
mod tests;

pub(super) async fn start_listener(
    listener: UnixListener,
    shutdown: Arc<Notify>,
    tx: Sender<Instruction>,
) {
    loop {
        tokio::select! {
            _ = shutdown.notified() => break,
            a = listener.accept() => handle_accept(a, tx.clone()).await,
        }
    }
}

async fn handle_accept(
    accepted: tokio::io::Result<(UnixStream, SocketAddr)>,
    tx: Sender<Instruction>,
) {
    let stream = match accepted {
        Ok((stream, _)) => stream,
        Err(e) => {
            tracing::error!("Error accepting from unix listener: {e}");
            return;
        }
    };

    tokio::spawn(async move {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        let mut message = String::new();
        if let Err(e) = reader.read_line(&mut message).await {
            return tracing::error!("Error reading stream: {e}");
        }
        match serde_json::from_str::<Message>(&message) {
            Err(e) => tracing::error!("Error decoding message: {e}"),
            Ok(Message::Stop) => handle_stop_message(&tx, &mut writer).await,
            Ok(Message::Shift(shift_message)) => {
                handle_shift_message(&tx, &mut writer, shift_message).await
            }
        }
    });
}

async fn handle_shift_message(
    tx: &Sender<Instruction>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: crate::message::ShiftMessage,
) {
    tracing::info!("Received Shift message");
    let (reply, response) = oneshot::channel();
    let instruction = Instruction::Shift { message, reply };
    let response = coordinator_reponse_or_err(tx, instruction, response).await;
    if let Err(e) = writer.write_all(response.as_bytes()).await {
        tracing::error!("Error writing to stream: {e}");
    }
}

async fn handle_stop_message(
    tx: &Sender<Instruction>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) {
    tracing::info!("Received Shift message");
    let (reply, response) = oneshot::channel();
    let instruction = Instruction::Stop { reply };
    let response = coordinator_reponse_or_err(tx, instruction, response).await;
    if let Err(e) = writer.write_all(response.as_bytes()).await {
        tracing::error!("Error writing to stream: {e}");
    }
}

async fn coordinator_reponse_or_err(
    tx: &Sender<Instruction>,
    instruction: Instruction,
    response: oneshot::Receiver<String>,
) -> String {
    match tx.send(instruction).await {
        Err(_) => String::from("Error: coordinator unavailable"),
        Ok(_) => match response.await {
            Err(_) => String::from("Error: coordinator stopped before rplying"),
            Ok(r) => r,
        },
    }
}
