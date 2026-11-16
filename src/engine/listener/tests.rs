use std::time::Duration;

use tokio::{io::AsyncReadExt, sync::mpsc, time::timeout};

use super::*;
use crate::message::{GammaRelayProperty, ShiftMessage, TargetValue};

#[tokio::test]
async fn shift_round_trip_preserves_id_and_returns_coordinator_response() {
    timeout(Duration::from_secs(5), async {
        let (mut client, server) = UnixStream::pair().unwrap();
        let address = server.peer_addr().unwrap();
        let (tx, mut rx) = mpsc::channel(1);
        handle_accept(Ok((server, address)), tx).await;
        let start = chrono::Local::now();
        let message = Message::Shift(ShiftMessage {
            property: GammaRelayProperty::Temperature,
            id: "example".into(),
            target: TargetValue::Relative(500.0),
            step_duration: Duration::from_millis(10),
            start,
            end: start,
        });
        client
            .write_all(&serde_json::to_vec(&message).unwrap())
            .await
            .unwrap();
        client.shutdown().await.unwrap();
        let Some(Instruction::Shift { message, reply }) = rx.recv().await else {
            panic!("expected a shift instruction");
        };
        assert_eq!(message.id, "example");
        assert!(matches!(message.target, TargetValue::Relative(500.0)));
        reply
            .send("Shifting Temperature: 4000 -> 4500".into())
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        assert_eq!(response, "Shifting Temperature: 4000 -> 4500");
    })
    .await
    .expect("listener round trip timed out");
}
