use std::time::Duration;

use tokio::{sync::mpsc, time::timeout};

use super::*;
use crate::message::{EngineResponse, GammaRelayProperty, ShiftMessage, TargetValue};

fn shift_message() -> Message {
    let start = chrono::Local::now();
    Message::Shift(ShiftMessage {
        property: GammaRelayProperty::Temperature,
        id: "example".into(),
        target: TargetValue::Relative(500.0),
        step_duration: Duration::from_millis(10),
        start,
        end: start,
    })
}

async fn with_listener(
    message: Message,
    test: impl AsyncFnOnce(&mut UnixStream, &mut mpsc::Receiver<Instruction>),
) {
    timeout(Duration::from_secs(5), async {
        let (mut client, server) = UnixStream::pair().unwrap();
        let address = server.peer_addr().unwrap();
        let (tx, mut rx) = mpsc::channel(1);
        handle_accept(Ok((server, address)), tx).await;
        client
            .write_all(&serde_json::to_vec(&message).unwrap())
            .await
            .unwrap();
        client.shutdown().await.unwrap();
        test(&mut client, &mut rx).await;
    })
    .await
    .expect("listener test timed out");
}

async fn read_response(client: &mut UnixStream) -> String {
    let mut reader = tokio::io::BufReader::new(client);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    match serde_json::from_str::<EngineResponse>(&response).unwrap() {
        EngineResponse::Ok(message) => message,
        EngineResponse::Error(message) => panic!("expected success, got {message}"),
    }
}

#[tokio::test]
async fn shift_forwards_the_message_to_the_coordinator() {
    let expected = serde_json::to_value(shift_message()).unwrap();
    let message = serde_json::from_value(expected.clone()).unwrap();
    with_listener(message, async |client, instructions| {
        let Some(Instruction::Shift { message, reply }) = instructions.recv().await else {
            panic!("expected a shift instruction");
        };

        assert_eq!(
            serde_json::to_value(Message::Shift(message)).unwrap(),
            expected
        );
        reply.send(Ok(String::new())).unwrap();
        read_response(client).await;
    })
    .await;
}

#[tokio::test]
async fn shift_returns_the_coordinator_response() {
    with_listener(shift_message(), async |client, instructions| {
        let Some(Instruction::Shift { reply, .. }) = instructions.recv().await else {
            panic!("expected a shift instruction");
        };

        reply
            .send(Ok("Shifting Temperature: 4000 -> 4500".into()))
            .unwrap();

        assert_eq!(
            read_response(client).await,
            "Shifting Temperature: 4000 -> 4500"
        );
    })
    .await;
}

#[tokio::test]
async fn shift_returns_the_coordinator_error() {
    with_listener(shift_message(), async |client, instructions| {
        let Some(Instruction::Shift { reply, .. }) = instructions.recv().await else {
            panic!("expected a shift instruction");
        };

        reply
            .send(Err("Nothing to undo for Temperature".into()))
            .unwrap();

        assert_eq!(
            read_error_response(client).await,
            "Nothing to undo for Temperature"
        );
    })
    .await;
}

#[tokio::test]
async fn stop_forwards_the_instruction_and_returns_the_coordinator_response() {
    with_listener(Message::Stop, async |client, instructions| {
        let Some(Instruction::Stop { reply }) = instructions.recv().await else {
            panic!("expected a stop instruction");
        };

        reply.send(Ok("Shutting down".into())).unwrap();

        assert_eq!(read_response(client).await, "Shutting down");
    })
    .await;
}

#[tokio::test]
async fn unavailable_coordinator_returns_an_error() {
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    let (reply, response) = oneshot::channel();

    let result = timeout(
        Duration::from_secs(5),
        coordinator_reponse_or_err(&tx, Instruction::Stop { reply }, response),
    )
    .await
    .unwrap();

    assert_eq!(
        result,
        EngineResponse::Error("Coordinator unavailable".into())
    );
}

#[tokio::test]
async fn dropped_coordinator_reply_returns_an_error() {
    with_listener(Message::Stop, async |client, instructions| {
        let Some(Instruction::Stop { reply }) = instructions.recv().await else {
            panic!("expected a stop instruction");
        };

        drop(reply);

        assert_eq!(
            read_error_response(client).await,
            "Coordinator stopped before replying"
        );
    })
    .await;
}

async fn read_error_response(client: &mut UnixStream) -> String {
    let mut reader = tokio::io::BufReader::new(client);
    let mut response = String::new();
    reader.read_line(&mut response).await.unwrap();
    match serde_json::from_str::<EngineResponse>(&response).unwrap() {
        EngineResponse::Error(message) => message,
        EngineResponse::Ok(message) => panic!("expected an error, got {message}"),
    }
}
