use std::{collections::HashMap, path::Path, sync::Arc};

use tokio::{
    sync::{Notify, mpsc::Receiver, oneshot},
    task::JoinHandle,
};

use super::{
    UnixSignals,
    shift::{ShiftId, ShiftRecord, plan_shift, run_ramp},
};
use crate::{
    gamma_relay_interface::{GammaRelayInterfaceProxy, get_property},
    message::{GammaRelayProperty, ShiftMessage, TargetValue},
};

pub(super) enum Instruction {
    Shift {
        message: ShiftMessage,
        reply: oneshot::Sender<String>,
    },
    Stop {
        reply: oneshot::Sender<String>,
    },
}

struct InFlight {
    target: f64,
    handle: JoinHandle<()>,
}

struct Coordinator {
    proxy: GammaRelayInterfaceProxy<'static>,
    in_flight: HashMap<GammaRelayProperty, InFlight>,
    history: HashMap<ShiftId, ShiftRecord>,
}

impl Coordinator {
    async fn shift(&mut self, message: ShiftMessage) -> String {
        let property = message.property;
        let key = ShiftId {
            property,
            id: Arc::from(message.id.as_str()),
        };
        let last = self.history.get(&key).copied();
        if matches!(message.target, TargetValue::Undo) && last.is_none() {
            return format!(
                "Error: nothing to undo for {property} (id {:?})",
                message.id
            );
        }

        let mut in_flight_target = None;
        if let Some(in_flight) = self.in_flight.remove(&property) {
            if !in_flight.handle.is_finished() {
                in_flight_target = Some(in_flight.target);
                in_flight.handle.abort();
            }
            let _ = in_flight.handle.await;
        }
        let actual = match get_property(&self.proxy, &property).await {
            Ok(value) => value,
            Err(e) => return format!("Error fetching current {property}: {e}"),
        };
        let record = match plan_shift(
            &message.target,
            property,
            actual,
            in_flight_target,
            last.as_ref(),
        ) {
            Ok(record) => record,
            Err(e) => return format!("Error: {e} for {property} (id {:?})", message.id),
        };
        let duration = message.end - message.start;
        let response = format!(
            "Shifting {property}: {actual} -> {} ({:.3}s)",
            record.target,
            duration.as_seconds_f64()
        );
        self.history.insert(key, record);
        let handle = tokio::spawn(run_ramp(self.proxy.clone(), message, actual, record.target));
        self.in_flight.insert(
            property,
            InFlight {
                target: record.target,
                handle,
            },
        );
        tracing::info!("{response}");
        response
    }

    async fn stop(&mut self) {
        for (_, in_flight) in self.in_flight.drain() {
            in_flight.handle.abort();
            let _ = in_flight.handle.await;
        }
    }
}

pub(super) async fn start_coordinator(
    mut receiver: Receiver<Instruction>,
    mut unix_signals: UnixSignals,
    shutdown: Arc<Notify>,
    socket_path: Arc<Path>,
    proxy: GammaRelayInterfaceProxy<'static>,
) {
    let mut coordinator = Coordinator {
        proxy,
        in_flight: HashMap::new(),
        history: HashMap::new(),
    };
    loop {
        tokio::select! {
            inst = receiver.recv() => {
                match inst {
                    Some(Instruction::Shift { message, reply }) => {
                        let response = coordinator.shift(message).await;
                        if response.starts_with("Error") {
                            tracing::error!("{response}");
                        }
                        _ = reply.send(response);
                    }
                    Some(Instruction::Stop { reply }) => {
                        let response = String::from("Shutting down");
                        _ = reply.send(response);
                        break;
                    }
                    None => break,
                }
            }
            _ = unix_signals.sigint.recv() => {
                tracing::info!("Received SIGINT");
                break;
            }
            _ = unix_signals.sigterm.recv() => {
                tracing::info!("Received SIGTERM");
                break;
            }
        }
    }
    coordinator.stop().await;
    handle_stop(&shutdown, &socket_path).await;
}

async fn handle_stop(shutdown: &Arc<Notify>, sock_path: impl AsRef<Path>) {
    tracing::info!("Shutting down");
    if let Err(e) = tokio::fs::remove_file(sock_path).await {
        tracing::error!("Error removing socket: {e}");
    }
    shutdown.notify_waiters();
}

#[cfg(test)]
mod tests {
    use std::{sync::Mutex, time::Duration};

    use chrono::Local;
    use tokio::{sync::mpsc, time::timeout};

    use super::*;
    use GammaRelayProperty::{Brightness, Temperature};
    use TargetValue::{Absolute, Linear, Relative, Undo};

    struct TestRelay {
        values: Arc<Mutex<(u16, f64)>>,
        writes: mpsc::UnboundedSender<(GammaRelayProperty, f64)>,
    }

    #[zbus::interface(name = "rs.wl.gammarelay")]
    impl TestRelay {
        #[zbus(property)]
        fn temperature(&self) -> u16 {
            self.values.lock().unwrap().0
        }

        #[zbus(property)]
        fn set_temperature(&mut self, value: u16) {
            self.values.lock().unwrap().0 = value;
            let _ = self.writes.send((Temperature, f64::from(value)));
        }

        #[zbus(property)]
        fn brightness(&self) -> f64 {
            self.values.lock().unwrap().1
        }

        #[zbus(property)]
        fn set_brightness(&mut self, value: f64) {
            self.values.lock().unwrap().1 = value;
            let _ = self.writes.send((Brightness, value));
        }
    }

    fn message(property: GammaRelayProperty, target: TargetValue, id: &str) -> ShiftMessage {
        let start = Local::now();
        ShiftMessage {
            property,
            id: id.into(),
            target,
            step_duration: Duration::from_secs(3600),
            start,
            end: start + Duration::from_secs(3600),
        }
    }

    #[tokio::test]
    #[ignore = "requires a session bus: dbus-run-session -- cargo test -- --include-ignored"]
    async fn coordinator_preempts_by_property_and_keeps_history_by_property_and_id() {
        timeout(Duration::from_secs(10), async {
            let values = Arc::new(Mutex::new((4000, 1.0)));
            let (writes, mut written) = mpsc::unbounded_channel();
            let server = zbus::connection::Builder::session()
                .serve_at(
                    "/",
                    TestRelay {
                        values: values.clone(),
                        writes,
                    },
                )
                .build()
                .await
                .unwrap();
            let connection = zbus::Connection::session().await.unwrap();
            let proxy = GammaRelayInterfaceProxy::builder(&connection)
                .destination(server.unique_name().unwrap().to_owned())
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
                .unwrap();
            let mut coordinator = Coordinator {
                proxy,
                in_flight: HashMap::new(),
                history: HashMap::new(),
            };

            coordinator
                .shift(message(Brightness, Absolute(0.5), "example"))
                .await;
            assert_eq!(written.recv().await.unwrap().0, Brightness);
            let brightness = coordinator.in_flight[&Brightness].handle.abort_handle();

            let mut previous: Option<tokio::task::AbortHandle> = None;
            for (target, actual, expected) in [
                (Relative(500.0), 4000, 4500.0),
                (Relative(500.0), 4100, 5000.0),
                (Undo, 4200, 4500.0),
                (Undo, 4300, 5000.0),
                (Undo, 4400, 4500.0),
            ] {
                values.lock().unwrap().0 = actual;
                let response = coordinator
                    .shift(message(Temperature, target, "example"))
                    .await;
                assert!(
                    response.starts_with(&format!("Shifting Temperature: {actual} -> {expected} "))
                );
                if let Some(previous) = previous.take() {
                    assert!(
                        previous.is_finished(),
                        "the previous ramp must have stopped"
                    );
                }
                previous = Some(coordinator.in_flight[&Temperature].handle.abort_handle());
                let (property, value) = written.recv().await.unwrap();
                assert_eq!(property, Temperature);
                assert!(
                    (value - f64::from(actual)).abs() < 2.0,
                    "ramp must start near actual, not reference"
                );
                assert!(
                    !brightness.is_finished(),
                    "another property must keep ramping"
                );
            }

            let temperature = previous.unwrap();
            let response = coordinator
                .shift(message(Temperature, Undo, "missing"))
                .await;
            assert!(response.starts_with("Error: nothing to undo"));
            assert!(
                !temperature.is_finished(),
                "invalid undo must not cancel a ramp"
            );

            coordinator
                .shift(message(Temperature, Absolute(6000.0), "other"))
                .await;
            written.recv().await.unwrap();
            assert!(
                temperature.is_finished(),
                "even a different id cancels the property"
            );
            let response = coordinator
                .shift(message(Temperature, Undo, "example"))
                .await;
            assert!(
                response.contains("-> 5000 "),
                "undo must use example's history, not other's"
            );
            written.recv().await.unwrap();

            let temperature = coordinator.in_flight[&Temperature].handle.abort_handle();
            coordinator
                .shift(message(Brightness, Undo, "example"))
                .await;
            written.recv().await.unwrap();
            assert_eq!(coordinator.in_flight[&Brightness].target, 1.0);
            assert!(!temperature.is_finished(), "history is property-specific");

            let mut instant = message(Temperature, Absolute(4500.0), "instant");
            instant.end = instant.start;
            coordinator.shift(instant).await;
            assert_eq!(written.recv().await.unwrap(), (Temperature, 4500.0));
            while !coordinator.in_flight[&Temperature].handle.is_finished() {
                tokio::task::yield_now().await;
            }
            values.lock().unwrap().0 = 4600;
            coordinator
                .shift(message(Temperature, Linear(1.5), "example"))
                .await;
            assert_eq!(
                coordinator.in_flight[&Temperature].target, 6900.0,
                "a completed target is no longer the reference"
            );
            written.recv().await.unwrap();
            let brightness = coordinator.in_flight[&Brightness].handle.abort_handle();
            let temperature = coordinator.in_flight[&Temperature].handle.abort_handle();
            coordinator.stop().await;
            assert!(brightness.is_finished() && temperature.is_finished());
        })
        .await
        .expect("coordinator test timed out");
    }
}
