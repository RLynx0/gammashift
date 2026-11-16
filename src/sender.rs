use std::path::Path;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::message::{EngineResponse, Message};

#[tokio::main]
pub async fn send_message(
    socket: impl AsRef<Path>,
    message: &Message,
) -> tokio::io::Result<String> {
    let socket = socket.as_ref();
    let stream = UnixStream::connect(socket).await.map_err(|error| {
        if matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
        ) {
            std::io::Error::new(
                error.kind(),
                format!(
                    "gammashift engine is unavailable at {}; start it with `gammashift start`",
                    socket.display()
                ),
            )
        } else {
            error
        }
    })?;
    let (reader, mut writer) = stream.into_split();
    let data = serde_json::to_vec(&message)?;
    writer.write_all(&data).await?;
    writer.write_all(b"\n").await?;
    drop(writer);

    let mut reader = BufReader::new(reader);
    let mut response = String::new();
    if reader.read_line(&mut response).await? == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "gammashift engine closed the connection without replying",
        ));
    }

    match serde_json::from_str::<EngineResponse>(&response)? {
        EngineResponse::Ok(message) => Ok(message),
        EngineResponse::Error(message) => Err(std::io::Error::other(message)),
    }
}
