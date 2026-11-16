use std::sync::Arc;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream, unix::SocketAddr},
    sync::{Notify, mpsc::Sender, oneshot},
};

use crate::{
    engine::coordinator::Instruction,
    message::{EngineResponse, Message},
};

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
            Err(error) => {
                tracing::error!("Error decoding message: {error}");
                write_response(
                    &mut writer,
                    EngineResponse::Error(format!("Invalid request: {error}")),
                )
                .await;
            }
            Ok(Message::Stop) => handle_stop_message(&tx, &mut writer).await,
            Ok(Message::Shift(shift_message)) => {
                handle_shift_message(&tx, &mut writer, shift_message).await
            }
            Ok(Message::Get(get_message)) => {
                handle_get_message(&tx, &mut writer, get_message).await
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
    write_response(writer, response).await;
}

async fn handle_get_message(
    tx: &Sender<Instruction>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    message: crate::message::GetMessage,
) {
    tracing::info!("Received Get message");
    let (reply, response) = oneshot::channel();
    let instruction = Instruction::Get { message, reply };
    let response = coordinator_reponse_or_err(tx, instruction, response).await;
    write_response(writer, response).await;
}

async fn handle_stop_message(
    tx: &Sender<Instruction>,
    writer: &mut tokio::net::unix::OwnedWriteHalf,
) {
    tracing::info!("Received Stop message");
    let (reply, response) = oneshot::channel();
    let instruction = Instruction::Stop { reply };
    let response = coordinator_reponse_or_err(tx, instruction, response).await;
    write_response(writer, response).await;
}

async fn write_response(writer: &mut tokio::net::unix::OwnedWriteHalf, response: EngineResponse) {
    let mut response = match serde_json::to_vec(&response) {
        Ok(response) => response,
        Err(error) => {
            tracing::error!("Error encoding response: {error}");
            return;
        }
    };
    response.push(b'\n');
    if let Err(error) = writer.write_all(&response).await {
        tracing::error!("Error writing to stream: {error}");
    }
}

async fn coordinator_reponse_or_err(
    tx: &Sender<Instruction>,
    instruction: Instruction,
    response: oneshot::Receiver<Result<String, String>>,
) -> EngineResponse {
    match tx.send(instruction).await {
        Err(_) => EngineResponse::Error(String::from("Coordinator unavailable")),
        Ok(_) => match response.await {
            Err(_) => EngineResponse::Error(String::from("Coordinator stopped before replying")),
            Ok(Ok(response)) => EngineResponse::Ok(response),
            Ok(Err(error)) => EngineResponse::Error(error),
        },
    }
}
